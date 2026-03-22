use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;
use std::thread;

use anyhow::{Result, anyhow};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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

fn history_command(item: &Arc<dyn SkimItem>) -> Option<String> {
    (**item)
        .as_any()
        .downcast_ref::<HistoryItem>()
        .map(|h| h.history.command.clone())
}

/// Build skim options. Ctrl+D is only bound (as accept) when `allow_delete` is true.
fn build_skim_options(allow_delete: bool) -> Result<SkimOptions> {
    let bindings: Vec<String> = if allow_delete {
        vec!["ctrl-d:accept".to_string()]
    } else {
        vec![]
    };
    SkimOptionsBuilder::default()
        .height("40%")
        .reverse(true)
        // empty string enables preview window backed by SkimItem::preview()
        .preview(String::new())
        .multi(false)
        .bind(bindings)
        .build()
        .map_err(|e| anyhow!("{e}"))
}

/// Deduplicate records by command, keeping the first occurrence (server returns newest first).
/// Returns items ready to send to skim.
fn make_deduped_items(records: &[History]) -> Vec<Arc<dyn SkimItem>> {
    let mut seen: HashSet<String> = HashSet::new();
    records
        .iter()
        .filter(|r| seen.insert(r.command.clone()))
        .map(|h| Arc::new(HistoryItem::new(h.clone())) as Arc<dyn SkimItem>)
        .collect()
}

/// Delete all server records that share `command`, then remove them from `all_records`.
fn delete_by_command(cfg: &Config, command: &str, all_records: &mut Vec<History>) -> Result<()> {
    let ids: Vec<i32> = all_records
        .iter()
        .filter(|r| r.command == command)
        .map(|r| r.id)
        .collect();
    for id in ids {
        client::delete_history(cfg, id)?;
    }
    all_records.retain(|r| r.command != command);
    Ok(())
}

/// Fetch all history records, deduplicate, launch skim, and print the selected command.
pub fn run_search(cfg: &Config) -> Result<()> {
    run_search_inner(cfg, None, false)
}

/// Same as `run_search` but pre-filtered to the given working directory.
/// Ctrl+D deletion is enabled in this mode.
pub fn run_search_with_pwd(cfg: &Config, pwd: &str) -> Result<()> {
    run_search_inner(cfg, Some(pwd), true)
}

fn run_search_inner(cfg: &Config, pwd: Option<&str>, allow_delete: bool) -> Result<()> {
    // --- First display: stream records from server so skim opens immediately ---
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();

    let cfg_clone = cfg.clone();
    let pwd_owned = pwd.map(str::to_string);

    // Background thread: fetch pages and stream items to skim.
    // Also collects all records for potential delete-loop re-use.
    let stream_handle: thread::JoinHandle<Result<Vec<History>>> = thread::spawn(move || {
        let mut seen: HashSet<String> = HashSet::new();
        let mut collected: Vec<History> = Vec::new();

        client::for_each_page(&cfg_clone, pwd_owned.as_deref(), |page| {
            collected.extend(page.iter().cloned());
            let batch: Vec<Arc<dyn SkimItem>> = page
                .into_iter()
                .filter(|r| seen.insert(r.command.clone()))
                .map(|h| Arc::new(HistoryItem::new(h)) as Arc<dyn SkimItem>)
                .collect();
            if !batch.is_empty() {
                // Ignore send errors: skim may have closed (user aborted early)
                let _ = tx.send(batch);
            }
            Ok(())
        })?;
        Ok(collected)
    });

    let output = Skim::run_with(build_skim_options(allow_delete)?, Some(rx))
        .map_err(|e| anyhow!("{e}"))?;

    // Wait for background thread to finish (collects all records for delete loop)
    let mut all_records = stream_handle
        .join()
        .map_err(|_| anyhow!("stream thread panicked"))??;

    if all_records.is_empty() {
        anyhow::bail!("No history records found.");
    }

    if output.is_abort {
        return Ok(());
    }

    if allow_delete
        && output.final_key == KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
    {
        if let Some(command) = output.selected_items.first().and_then(|i| history_command(&i.item))
        {
            delete_by_command(cfg, &command, &mut all_records)?;
        }
        // fall through to post-delete loop
    } else {
        for item in &output.selected_items {
            print!("{}", item.item.output());
        }
        return Ok(());
    }

    // --- Post-delete loop: use in-memory records (no re-fetch needed) ---
    loop {
        let items = make_deduped_items(&all_records);
        if items.is_empty() {
            anyhow::bail!("No history records found.");
        }

        let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
        tx.send(items).map_err(|e| anyhow!("{e}"))?;
        drop(tx);

        let output = Skim::run_with(build_skim_options(allow_delete)?, Some(rx))
            .map_err(|e| anyhow!("{e}"))?;

        if output.is_abort {
            return Ok(());
        }

        if allow_delete
            && output.final_key == KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
        {
            if let Some(command) =
                output.selected_items.first().and_then(|i| history_command(&i.item))
            {
                delete_by_command(cfg, &command, &mut all_records)?;
            }
            continue;
        }

        for item in &output.selected_items {
            print!("{}", item.item.output());
        }
        return Ok(());
    }
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

    /// Convenience: extract command strings from make_deduped_items output.
    fn commands(records: &[History]) -> Vec<String> {
        make_deduped_items(records)
            .iter()
            .filter_map(|item| history_command(item))
            .collect()
    }

    #[test]
    fn dedup_keeps_first_occurrence() {
        // Server returns newest first; first occurrence of a command = most recent
        let records = vec![
            make_history(2, "git status", 2000), // newer — sent first by server
            make_history(1, "git status", 1000), // older duplicate
            make_history(3, "ls -la", 500),
        ];

        let result = commands(&records);

        assert_eq!(result.len(), 2);
        let items = make_deduped_items(&records);
        let git_item = items
            .iter()
            .find(|i| history_command(i).as_deref() == Some("git status"))
            .unwrap();
        // Should keep id=2 (the first record in the slice = most recently used)
        let hist = (**git_item)
            .as_any()
            .downcast_ref::<HistoryItem>()
            .unwrap();
        assert_eq!(hist.history.id, 2);
    }

    #[test]
    fn dedup_preserves_server_order() {
        // Server returns newest-first; dedup should preserve that order
        let records = vec![
            make_history(3, "echo c", 300),
            make_history(2, "echo b", 200),
            make_history(1, "echo a", 100),
        ];

        let result = commands(&records);

        assert_eq!(result, vec!["echo c", "echo b", "echo a"]);
    }

    #[test]
    fn dedup_empty_input_returns_empty() {
        let result = make_deduped_items(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn dedup_single_record_unchanged() {
        let records = vec![make_history(1, "cargo build", 999)];
        let result = commands(&records);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "cargo build");
    }
}
