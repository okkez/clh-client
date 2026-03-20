use anyhow::Result;

use crate::client;
use crate::config;

/// POST a new history record to the server.
/// Commands matching any pattern in `config.add.ignore_patterns` are silently skipped.
pub fn run_add(hostname: &str, pwd: &str, command: &str) -> Result<()> {
    let cfg = config::Config::load()?;
    for pattern in &cfg.add.ignore_patterns {
        match regex::Regex::new(pattern) {
            Ok(re) => {
                if re.is_match(command) {
                    return Ok(());
                }
            }
            Err(e) => {
                eprintln!("clh: invalid ignore_pattern {pattern:?}: {e}");
            }
        }
    }
    client::post_history(&cfg, hostname, pwd, command)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(patterns: Vec<&str>) -> config::Config {
        config::Config {
            server: config::ServerConfig {
                url: "http://localhost".to_string(),
                basic_auth_user: None,
                basic_auth_password: None,
            },
            search: config::SearchConfig::default(),
            add: config::AddConfig {
                ignore_patterns: patterns.into_iter().map(str::to_string).collect(),
            },
        }
    }

    fn should_ignore(cfg: &config::Config, command: &str) -> bool {
        for pattern in &cfg.add.ignore_patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                if re.is_match(command) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn ignore_matching_command() {
        let cfg = make_config(vec!["^ls", "^cd "]);
        assert!(should_ignore(&cfg, "ls -la"));
        assert!(should_ignore(&cfg, "cd /tmp"));
    }

    #[test]
    fn allow_non_matching_command() {
        let cfg = make_config(vec!["^ls", "^cd "]);
        assert!(!should_ignore(&cfg, "git status"));
        assert!(!should_ignore(&cfg, "cargo build"));
    }

    #[test]
    fn empty_patterns_allows_all() {
        let cfg = make_config(vec![]);
        assert!(!should_ignore(&cfg, "ls -la"));
    }

    #[test]
    fn invalid_pattern_is_skipped() {
        let cfg = make_config(vec!["[invalid"]);
        // Should not panic; invalid pattern is ignored
        assert!(!should_ignore(&cfg, "ls"));
    }
}
