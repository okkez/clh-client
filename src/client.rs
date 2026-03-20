use anyhow::Result;
use reqwest::blocking::Client;

use crate::config::{AddConfig, Config};
use crate::models::History;

/// Fetch all history records from the server, handling pagination automatically.
///
/// If the server returns an `X-Total-Count` header, records are fetched page by
/// page until all are retrieved.  Otherwise a single request with a large limit
/// is made (backwards-compatible with servers that do not support pagination).
///
/// `pwd` optionally filters results to a specific working directory.
pub fn fetch_all(cfg: &Config, pwd: Option<&str>) -> Result<Vec<History>> {
    let client = build_client(cfg)?;
    let base_url = cfg.server.url.trim_end_matches('/');
    let page_size = cfg.search.page_size;

    // First request: page 0
    let (mut records, total) = fetch_page(&client, base_url, cfg, pwd, 0, page_size)?;

    if let Some(total) = total {
        // Server supports pagination — fetch remaining pages
        let mut offset = page_size;
        while offset < total {
            let (page, _) = fetch_page(&client, base_url, cfg, pwd, offset, page_size)?;
            records.extend(page);
            offset += page_size;
        }
    }

    Ok(records)
}

/// Post a new history record to the server.
pub fn post_history(cfg: &Config, hostname: &str, pwd: &str, command: &str) -> Result<()> {
    let client = build_client(cfg)?;
    let base_url = cfg.server.url.trim_end_matches('/');

    let params = [
        ("hostname", hostname),
        ("working_directory", pwd),
        ("command", command),
    ];

    let mut req = client.post(base_url).form(&params);
    req = apply_auth(req, cfg);
    let resp = req.send()?;
    resp.error_for_status()?;
    Ok(())
}

// --- private helpers ---------------------------------------------------------

fn fetch_page(
    client: &Client,
    base_url: &str,
    cfg: &Config,
    pwd: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<(Vec<History>, Option<usize>)> {
    let mut req = client
        .get(base_url)
        .query(&[("limit", limit.to_string()), ("offset", offset.to_string())]);

    if let Some(ref hostname) = cfg.search.hostname {
        req = req.query(&[("hostname", hostname)]);
    }

    if let Some(pwd) = pwd {
        req = req.query(&[("pwd", pwd)]);
    }

    req = apply_auth(req, cfg);

    let resp = req.send()?.error_for_status()?;

    // Parse total count from header if present (server-side pagination support)
    let total: Option<usize> = resp
        .headers()
        .get("X-Total-Count")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let records: Vec<History> = resp.json()?;
    Ok((records, total))
}

fn build_client(cfg: &Config) -> Result<Client> {
    let client = Client::builder()
        .user_agent(concat!("clh-client/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let _ = cfg; // cfg used in apply_auth; client itself doesn't need it
    Ok(client)
}

fn apply_auth(
    req: reqwest::blocking::RequestBuilder,
    cfg: &Config,
) -> reqwest::blocking::RequestBuilder {
    match (&cfg.server.basic_auth_user, &cfg.server.basic_auth_password) {
        (Some(user), Some(pass)) => req.basic_auth(user, Some(pass)),
        (Some(user), None) => req.basic_auth(user, Option::<&str>::None),
        _ => req,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SearchConfig, ServerConfig};
    use mockito::Server;

    fn make_config(url: &str) -> Config {
        Config {
            server: ServerConfig {
                url: url.to_string(),
                basic_auth_user: None,
                basic_auth_password: None,
            },
            search: SearchConfig {
                hostname: None,
                page_size: 2,
                dedup: true,
            },
            add: AddConfig::default(),
        }
    }

    fn history_json(id: i32, command: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "hostname": "host",
            "working_directory": "/tmp",
            "command": command,
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        })
    }

    #[test]
    fn fetch_all_single_page_without_total_count() {
        let mut server = Server::new();
        let body = serde_json::to_string(&serde_json::json!([
            history_json(1, "echo a"),
            history_json(2, "echo b"),
        ]))
        .unwrap();

        let mock = server
            .mock("GET", "/")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("limit".into(), "2".into()),
                mockito::Matcher::UrlEncoded("offset".into(), "0".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&body)
            .create();

        let cfg = make_config(&server.url());
        let records = fetch_all(&cfg, None).unwrap();

        mock.assert();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].command, "echo a");
    }

    #[test]
    fn fetch_all_paginates_when_total_count_header_present() {
        let mut server = Server::new();

        let page1_body = serde_json::to_string(&serde_json::json!([
            history_json(1, "echo a"),
            history_json(2, "echo b"),
        ]))
        .unwrap();
        let page2_body =
            serde_json::to_string(&serde_json::json!([history_json(3, "echo c"),])).unwrap();

        let mock1 = server
            .mock("GET", "/")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("limit".into(), "2".into()),
                mockito::Matcher::UrlEncoded("offset".into(), "0".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("X-Total-Count", "3")
            .with_body(&page1_body)
            .create();

        let mock2 = server
            .mock("GET", "/")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("limit".into(), "2".into()),
                mockito::Matcher::UrlEncoded("offset".into(), "2".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("X-Total-Count", "3")
            .with_body(&page2_body)
            .create();

        let cfg = make_config(&server.url());
        let records = fetch_all(&cfg, None).unwrap();

        mock1.assert();
        mock2.assert();
        assert_eq!(records.len(), 3);
        assert_eq!(records[2].command, "echo c");
    }

    #[test]
    fn fetch_all_returns_error_on_non_200() {
        let mut server = Server::new();
        let mock = server
            .mock("GET", "/")
            .match_query(mockito::Matcher::Any)
            .with_status(401)
            .with_body("Unauthorized")
            .create();

        let cfg = make_config(&server.url());
        let result = fetch_all(&cfg, None);

        mock.assert();
        assert!(result.is_err());
    }
}
