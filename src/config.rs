use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub search: SearchConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ServerConfig {
    pub url: String,
    pub basic_auth_user: Option<String>,
    pub basic_auth_password: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SearchConfig {
    /// If set, filter results to this hostname only
    pub hostname: Option<String>,
    /// Number of records to fetch per page
    pub page_size: usize,
    /// Deduplicate by command string (keep most recently used)
    pub dedup: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            hostname: None,
            page_size: 1000,
            dedup: true,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            anyhow::bail!(
                "Config file not found: {}\nRun `clh config init` to create one.",
                path.display()
            );
        }
        let content =
            std::fs::read_to_string(&path).with_context(|| format!("Reading {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("Parsing {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Print the current config path and contents, or an init template if missing
    pub fn show() -> Result<()> {
        let path = config_path()?;
        println!("Config path: {}", path.display());
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            println!("{}", content);
        } else {
            println!("(not found — run `clh config init` to create)");
        }
        Ok(())
    }

    pub fn init(url: &str, user: Option<&str>, password: Option<&str>) -> Result<()> {
        let path = config_path()?;
        if path.exists() {
            anyhow::bail!(
                "Config already exists at {}. Remove it first if you want to reinitialize.",
                path.display()
            );
        }
        let cfg = Config {
            server: ServerConfig {
                url: url.to_string(),
                basic_auth_user: user.map(str::to_string),
                basic_auth_password: password.map(str::to_string),
            },
            search: SearchConfig::default(),
        };
        cfg.save()?;
        println!("Created config at {}", path.display());
        Ok(())
    }
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("Cannot determine config directory")?;
    Ok(base.join("clh").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_config_toml() -> &'static str {
        r#"
[server]
url = "https://clh.example.com"
basic_auth_user = "user"
basic_auth_password = "pass"

[search]
hostname = "my-machine"
page_size = 500
dedup = false
"#
    }

    fn minimal_config_toml() -> &'static str {
        r#"
[server]
url = "http://localhost:8088"
"#
    }

    #[test]
    fn parse_full_config() {
        let cfg: Config = toml::from_str(full_config_toml()).unwrap();
        assert_eq!(cfg.server.url, "https://clh.example.com");
        assert_eq!(cfg.server.basic_auth_user.as_deref(), Some("user"));
        assert_eq!(cfg.server.basic_auth_password.as_deref(), Some("pass"));
        assert_eq!(cfg.search.hostname.as_deref(), Some("my-machine"));
        assert_eq!(cfg.search.page_size, 500);
        assert!(!cfg.search.dedup);
    }

    #[test]
    fn parse_minimal_config_uses_search_defaults() {
        let cfg: Config = toml::from_str(minimal_config_toml()).unwrap();
        assert_eq!(cfg.server.url, "http://localhost:8088");
        assert!(cfg.server.basic_auth_user.is_none());
        assert!(cfg.server.basic_auth_password.is_none());
        assert!(cfg.search.hostname.is_none());
        assert_eq!(cfg.search.page_size, 1000);
        assert!(cfg.search.dedup);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let cfg = Config {
            server: ServerConfig {
                url: "https://clh.test".to_string(),
                basic_auth_user: Some("u".to_string()),
                basic_auth_password: Some("p".to_string()),
            },
            search: SearchConfig {
                hostname: None,
                page_size: 250,
                dedup: true,
            },
        };

        let content = toml::to_string_pretty(&cfg).unwrap();
        std::fs::write(&path, &content).unwrap();

        let loaded: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.server.url, "https://clh.test");
        assert_eq!(loaded.search.page_size, 250);
    }
}
