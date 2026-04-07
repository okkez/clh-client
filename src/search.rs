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

/// Wrap `command` so that each line fits within `max_width` columns.
///
/// The first line is prefixed with `"command : "` (10 chars); continuation lines
/// are indented with 10 spaces so everything lines up.  When `max_width` is 0
/// (e.g. in unit tests that skip terminal sizing), the command is returned
/// unwrapped, still with the usual `"command : "` prefix.
fn wrap_command(command: &str, max_width: usize) -> String {
    const PREFIX: &str = "command : ";
    const INDENT: &str = "          "; // 10 spaces
    const PREFIX_LEN: usize = PREFIX.len(); // 10

    if max_width == 0 || max_width <= PREFIX_LEN {
        return format!("{PREFIX}{command}");
    }

    let available = max_width - PREFIX_LEN;
    let mut result = String::with_capacity(command.len() + 32);
    result.push_str(PREFIX);

    let mut remaining = command;
    let mut first = true;
    while !remaining.is_empty() {
        if !first {
            result.push('\n');
            result.push_str(INDENT);
        }
        first = false;

        if remaining.len() <= available {
            result.push_str(remaining);
            break;
        }

        // Find the largest byte offset ≤ available that ends on a valid char boundary,
        // so we never slice in the middle of a multi-byte codepoint.
        let safe_available = remaining
            .char_indices()
            .map(|(i, c)| i + c.len_utf8())
            .take_while(|&end| end <= available)
            .last()
            .unwrap_or(0);

        // If the character right after safe_available is a space, take exactly that
        // many bytes — a clean word boundary.
        let next_is_space = remaining
            .as_bytes()
            .get(safe_available)
            .is_some_and(|&b| b == b' ');
        let break_pos = if next_is_space {
            safe_available
        } else {
            // Try to break at the last space within the allowed width.
            remaining[..safe_available]
                .rfind(' ')
                .unwrap_or(safe_available)
        };

        result.push_str(&remaining[..break_pos]);
        // Strip exactly one separator space; preserve any intentional extra spaces.
        let next_remaining = &remaining[break_pos..];
        remaining = if next_remaining.as_bytes().first() == Some(&b' ') {
            &next_remaining[1..]
        } else {
            next_remaining
        };
    }

    result
}

/// A skim item wrapping a History record.
struct HistoryItem {
    history: History,
    /// The text shown in the skim list (command string)
    display_text: String,
}

impl HistoryItem {
    fn new(history: History) -> Self {
        let display_text = history.command.clone();
        Self {
            history,
            display_text,
        }
    }
}

impl SkimItem for HistoryItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.display_text)
    }

    fn preview(&self, ctx: PreviewContext) -> ItemPreview {
        let command_line = wrap_command(&self.history.command, ctx.width);
        let text = format!(
            "{}\nhostname: {}\npwd     : {}\nupdated : {}",
            command_line,
            self.history.hostname,
            self.history.working_directory.as_deref().unwrap_or("-"),
            self.history.updated_at.format("%Y-%m-%d %H:%M:%S UTC"),
        );
        ItemPreview::Text(text)
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
fn build_skim_options(height: &str) -> Result<SkimOptions> {
    SkimOptionsBuilder::default()
        .height(height.to_string())
        .reverse(true)
        // empty string enables preview window backed by SkimItem::preview()
        .preview(String::new())
        .multi(false)
        .bind(vec!["ctrl-d:accept".to_string()])
        .build()
        .map_err(|e| anyhow!("{e}"))
}

/// Deduplicate records by command, keeping the first occurrence (server returns newest first).
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

    let output = Skim::run_with(build_skim_options(&cfg.search.height)?, Some(rx))
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

        let output = Skim::run_with(build_skim_options(&cfg.search.height)?, Some(rx))
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

    /// Deduplicate `records` by command (first occurrence wins) and return skim items.
    fn make_deduped_items(records: &[History]) -> Vec<Arc<dyn SkimItem>> {
        let mut seen: HashSet<String> = HashSet::new();
        records
            .iter()
            .filter(|r| seen.insert(r.command.clone()))
            .map(|h| Arc::new(HistoryItem::new(h.clone())) as Arc<dyn SkimItem>)
            .collect()
    }

    /// Convenience: extract command strings from `make_deduped_items` output.
    fn commands(records: &[History]) -> Vec<String> {
        make_deduped_items(records)
            .iter()
            .filter_map(history_command)
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
        let hist = (**git_item).as_any().downcast_ref::<HistoryItem>().unwrap();
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

    // --- wrap_command tests ---

    #[test]
    fn wrap_command_short_fits_on_one_line() {
        let result = wrap_command("ls -la", 40);
        assert_eq!(result, "command : ls -la");
    }

    #[test]
    fn wrap_command_zero_width_returns_as_is() {
        let result = wrap_command("ls -la", 0);
        assert_eq!(result, "command : ls -la");
    }

    #[test]
    fn wrap_command_wraps_at_space() {
        // max_width=20 → available=10; "git commit -m 'fix'" is longer
        // "git commit" fits in 10, next word "-m" starts new line
        let result = wrap_command("git commit -m 'fix'", 20);
        assert_eq!(result, "command : git commit\n          -m 'fix'");
    }

    #[test]
    fn wrap_command_no_space_breaks_at_width() {
        // A single long token with no spaces must be cut hard at available width (10)
        // 25 'a's → 10 + 10 + 5 across three lines
        let long = "a".repeat(25);
        let result = wrap_command(&long, 20);
        let expected = format!(
            "command : {}\n          {}\n          {}",
            "a".repeat(10),
            "a".repeat(10),
            "a".repeat(5)
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn wrap_command_multiple_wraps() {
        // max_width=20 → available=10; "aa bb cc dd ee ff" wraps to 2 lines
        let cmd = "aa bb cc dd ee ff";
        let result = wrap_command(cmd, 20);
        let expected = "command : aa bb cc\n          dd ee ff";
        assert_eq!(result, expected);

        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        let joined = lines
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                if idx == 0 {
                    line.strip_prefix("command : ").unwrap()
                } else {
                    line.strip_prefix("          ").unwrap()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(joined, cmd);
    }

    #[test]
    fn wrap_command_non_ascii_wraps_safely() {
        // Each 'é' is 2 bytes (UTF-8). available = 14 - 10 = 4 bytes → fits 2 chars.
        // "ééééé" (5 × 2 = 10 bytes) should wrap without panicking.
        let cmd = "ééééé";
        let result = std::panic::catch_unwind(|| wrap_command(cmd, 14));
        assert!(result.is_ok(), "wrap_command panicked on non-ASCII input");
        // 2 chars (4 bytes) fit on first line, 2 on second, 1 on third
        assert_eq!(result.unwrap(), "command : éé\n          éé\n          é");
    }
}
