use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
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

fn history_id(item: &Arc<dyn SkimItem>) -> Option<i32> {
    (**item)
        .as_any()
        .downcast_ref::<HistoryItem>()
        .map(|h| h.history.id)
}

fn history_command(item: &Arc<dyn SkimItem>) -> Option<String> {
    (**item)
        .as_any()
        .downcast_ref::<HistoryItem>()
        .map(|h| h.history.command.clone())
}

/// Build skim options. Ctrl+D is always bound to accept.
fn build_skim_options() -> Result<SkimOptions> {
    SkimOptionsBuilder::default()
        .height("40%")
        .reverse(true)
        // empty string enables preview window backed by SkimItem::preview()
        .preview(String::new())
        .multi(false)
        .bind(vec!["ctrl-d:accept".to_string()])
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

/// Delete all server records that share `command`, then remove them from `records`.
/// Used in pwd-scoped mode (^S) where all duplicates in the current directory should go.
fn delete_by_command(cfg: &Config, command: &str, records: &mut Vec<History>) -> Result<()> {
    let ids: Vec<i32> = records
        .iter()
        .filter(|r| r.command == command)
        .map(|r| r.id)
        .collect();
    for id in ids {
        client::delete_history(cfg, id)?;
    }
    records.retain(|r| r.command != command);
    Ok(())
}

/// Delete a single record by ID, then remove it from `records`.
/// Used in global mode (^T) to avoid accidentally deleting history from other directories.
fn delete_by_id(cfg: &Config, id: i32, records: &mut Vec<History>) -> Result<()> {
    client::delete_history(cfg, id)?;
    records.retain(|r| r.id != id);
    Ok(())
}

/// Spawn a background thread that fetches history pages and streams `HistoryItem`s to `tx`.
/// Fetched records are also accumulated in the returned `Arc<Mutex<Vec<History>>>` so the
/// caller can take a snapshot at any time without waiting for the thread to finish.
/// The `JoinHandle` is intentionally discarded — the thread is detached and will finish on
/// its own; dropping the handle does NOT kill the thread in Rust.
fn spawn_stream_thread(
    cfg: Config,
    pwd: Option<String>,
    tx: SkimItemSender,
) -> Arc<Mutex<Vec<History>>> {
    let collected: Arc<Mutex<Vec<History>>> = Arc::new(Mutex::new(Vec::new()));
    let collected_bg = Arc::clone(&collected);

    thread::spawn(move || {
        let mut seen: HashSet<String> = HashSet::new();

        let _ = client::for_each_page(&cfg, pwd.as_deref(), |page| {
            {
                let mut guard = collected_bg.lock().expect("collected mutex poisoned");
                guard.extend(page.iter().cloned());
            }
            let batch: Vec<Arc<dyn SkimItem>> = page
                .into_iter()
                .filter(|r| seen.insert(r.command.clone()))
                .map(|h| Arc::new(HistoryItem::new(h)) as Arc<dyn SkimItem>)
                .collect();
            if !batch.is_empty() {
                let _ = tx.send(batch);
            }
            Ok(())
        });
        // tx dropped here → skim receives EOF
    });

    collected
}

/// Fetch all history records, deduplicate, launch skim, and print the selected command.
pub fn run_search(cfg: &Config) -> Result<()> {
    run_search_inner(cfg, None)
}

/// Same as `run_search` but pre-filtered to the given working directory.
/// Ctrl+D deletes all records sharing the same command within the directory.
pub fn run_search_with_pwd(cfg: &Config, pwd: &str) -> Result<()> {
    run_search_inner(cfg, Some(pwd))
}

fn is_ctrl_d(key: KeyEvent) -> bool {
    key == KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
}

fn run_search_inner(cfg: &Config, pwd: Option<&str>) -> Result<()> {
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
    let collected = spawn_stream_thread(cfg.clone(), pwd.map(str::to_string), tx);

    let output = Skim::run_with(build_skim_options()?, Some(rx))
        .map_err(|e| anyhow!("{e}"))?;

    // Fast path: abort or Enter — return immediately without waiting for the stream thread.
    // The thread is detached and will finish on its own.
    if output.is_abort {
        return Ok(());
    }

    if !is_ctrl_d(output.final_key) {
        // Enter: print selected command immediately
        if output.selected_items.is_empty() {
            anyhow::bail!("No history records found.");
        }
        for item in &output.selected_items {
            print!("{}", item.item.output());
        }
        return Ok(());
    }

    // Ctrl+D: take a snapshot of whatever has been fetched so far (speed over completeness).
    if let Some(item) = output.selected_items.first() {
        let mut snapshot = collected.lock().expect("collected mutex poisoned").clone();
        if pwd.is_some() {
            // ^S mode: delete all records with the same command (pwd-scoped set)
            if let Some(command) = history_command(&item.item) {
                delete_by_command(cfg, &command, &mut snapshot)?;
            }
        } else {
            // ^T mode: delete only the single displayed record
            if let Some(id) = history_id(&item.item) {
                delete_by_id(cfg, id, &mut snapshot)?;
            }
        }
    }

    // Post-delete loop: start a fresh streaming session so the user sees up-to-date results.
    post_delete_loop(cfg, pwd)
}

/// Run the post-delete skim loop. Opens a fresh streaming session each time.
fn post_delete_loop(cfg: &Config, pwd: Option<&str>) -> Result<()> {
    loop {
        let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
        let collected = spawn_stream_thread(cfg.clone(), pwd.map(str::to_string), tx);

        let output = Skim::run_with(build_skim_options()?, Some(rx))
            .map_err(|e| anyhow!("{e}"))?;

        if output.is_abort {
            return Ok(());
        }

        if !is_ctrl_d(output.final_key) {
            if output.selected_items.is_empty() {
                anyhow::bail!("No history records found.");
            }
            for item in &output.selected_items {
                print!("{}", item.item.output());
            }
            return Ok(());
        }

        if let Some(item) = output.selected_items.first() {
            let mut snapshot = collected.lock().expect("collected mutex poisoned").clone();
            if pwd.is_some() {
                if let Some(command) = history_command(&item.item) {
                    delete_by_command(cfg, &command, &mut snapshot)?;
                }
            } else if let Some(id) = history_id(&item.item) {
                delete_by_id(cfg, id, &mut snapshot)?;
            }
        }
        // loop: open another fresh session
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
