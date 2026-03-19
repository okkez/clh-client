use anyhow::Result;

use crate::client;
use crate::config;

/// POST a new history record to the server.
pub fn run_add(hostname: &str, pwd: &str, command: &str) -> Result<()> {
    let cfg = config::Config::load()?;
    client::post_history(&cfg, hostname, pwd, command)?;
    Ok(())
}
