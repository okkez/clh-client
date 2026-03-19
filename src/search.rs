use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use skim::prelude::*;

use crate::client;
use crate::config::Config;
use crate::models::History;

/// A skim item wrapping a History record.
struct HistoryItem {
    history: History,
    /// The text shown in the skim list (command string)
    display_text: String,
    /// Preview text (shown in preview window)
    preview_text: String,
}

impl HistoryItem {
    fn new(history: History) -> Self {
        let preview_text = format!(
            "command : {}\nhostname: {}\npwd     : {}\nupdated : {}",
            history.command,
            history.hostname,
            history.working_directory.as_deref().unwrap_or("-"),
            history.updated_at.format("%Y-%m-%d %H:%M:%S UTC"),
        );
        let display_text = history.command.clone();
        Self {
            history,
            display_text,
            preview_text,
        }
    }
}

impl SkimItem for HistoryItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.display_text)
    }

    fn preview(&self, _ctx: PreviewContext) -> ItemPreview {
        ItemPreview::Text(self.preview_text.clone())
    }

    fn output(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.history.command)
    }
}

/// Fetch all history records, deduplicate if configured, launch skim, and
/// print the selected command to stdout.
pub fn run_search(cfg: &Config) -> Result<()> {
    run_search_inner(cfg, None)
}

/// Same as `run_search` but pre-filtered to the given working directory.
pub fn run_search_with_pwd(cfg: &Config, pwd: &str) -> Result<()> {
    run_search_inner(cfg, Some(pwd))
}

fn run_search_inner(cfg: &Config, pwd: Option<&str>) -> Result<()> {
    let records = client::fetch_all(cfg, pwd)?;
    let items = dedup_and_sort(records, cfg.search.dedup);

    if items.is_empty() {
        anyhow::bail!("No history records found.");
    }

    let options = SkimOptionsBuilder::default()
        .height("40%")
        .reverse(true)
        // empty string enables preview window backed by SkimItem::preview()
        .preview(String::new())
        .multi(false)
        .build()
        .map_err(|e| anyhow!("{e}"))?;

    let skim_items: Vec<Arc<dyn SkimItem>> = items
        .into_iter()
        .map(|h| Arc::new(HistoryItem::new(h)) as Arc<dyn SkimItem>)
        .collect();

    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
    tx.send(skim_items)?;
    drop(tx);

    let output = Skim::run_with(options, Some(rx)).map_err(|e| anyhow!("{e}"))?;

    if output.is_abort {
        return Ok(());
    }

    for item in &output.selected_items {
        print!("{}", item.item.output());
    }

    Ok(())
}

/// Dedup by command string, keeping the record with the latest `updated_at`.
fn dedup_and_sort(records: Vec<History>, dedup: bool) -> Vec<History> {
    if !dedup {
        return records;
    }

    let mut map: HashMap<String, History> = HashMap::new();
    for record in records {
        let entry = map.entry(record.command.clone());
        entry
            .and_modify(|existing| {
                if record.updated_at > existing.updated_at {
                    *existing = record.clone();
                }
            })
            .or_insert(record);
    }

    let mut deduped: Vec<History> = map.into_values().collect();
    // Most recently used first
    deduped.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn make_history(id: i32, command: &str, updated_secs: i64) -> History {
        History {
            id,
            hostname: "test-host".to_string(),
            working_directory: Some("/home/user".to_string()),
            command: command.to_string(),
            created_at: Utc.timestamp_opt(0, 0).unwrap(),
            updated_at: Utc.timestamp_opt(updated_secs, 0).unwrap(),
        }
    }

    #[test]
    fn dedup_keeps_most_recently_used() {
        let records = vec![
            make_history(1, "git status", 1000),
            make_history(2, "git status", 2000), // newer — should win
            make_history(3, "ls -la", 500),
        ];

        let result = dedup_and_sort(records, true);

        assert_eq!(result.len(), 2);
        let git_status = result.iter().find(|h| h.command == "git status").unwrap();
        assert_eq!(git_status.id, 2, "newer record should be kept");
    }

    #[test]
    fn dedup_sorts_by_updated_at_desc() {
        let records = vec![
            make_history(1, "echo a", 100),
            make_history(2, "echo b", 300),
            make_history(3, "echo c", 200),
        ];

        let result = dedup_and_sort(records, true);

        assert_eq!(result[0].command, "echo b"); // updated_at=300
        assert_eq!(result[1].command, "echo c"); // updated_at=200
        assert_eq!(result[2].command, "echo a"); // updated_at=100
    }

    #[test]
    fn dedup_false_returns_all_records_unchanged() {
        let records = vec![
            make_history(1, "git status", 1000),
            make_history(2, "git status", 2000),
        ];

        let result = dedup_and_sort(records, false);

        assert_eq!(result.len(), 2);
    }

    #[test]
    fn dedup_empty_input_returns_empty() {
        let result = dedup_and_sort(vec![], true);
        assert!(result.is_empty());
    }

    #[test]
    fn dedup_single_record_unchanged() {
        let records = vec![make_history(1, "cargo build", 999)];
        let result = dedup_and_sort(records, true);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].command, "cargo build");
    }
}
