use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Result, anyhow};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use skim::prelude::*;

use crate::client;
use crate::config::Config;
use crate::models::History;
use crate::worktree;

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

/// IDs of records with `command` recorded in exactly `pwd`.
///
/// In a git worktree the list also contains records from the main working tree;
/// those are intentionally left alone.
fn ids_to_delete(records: &[History], command: &str, pwd: &str) -> Vec<i32> {
    records
        .iter()
        .filter(|r| r.command == command && r.working_directory.as_deref() == Some(pwd))
        .map(|r| r.id)
        .collect()
}

/// Directories to search for pwd-scoped mode (^S): `pwd` itself, plus the same
/// position in the main working tree when `pwd` is inside a linked git worktree.
///
/// `pwd` is normalized like the shell's `$PWD` (absolute, no trailing slash, no `.`
/// components) because both the server filter and Ctrl+D compare paths exactly.
fn search_pwds(pwd: &str) -> Vec<String> {
    let pwd = normalize_pwd(pwd);
    // A non-UTF-8 path can't match any stored record, so skip it rather than
    // sending a lossily converted one.
    let counterpart = worktree::counterpart_dir(Path::new(&pwd))
        .as_deref()
        .and_then(Path::to_str)
        .map(str::to_string);
    std::iter::once(pwd).chain(counterpart).collect()
}

/// Make `pwd` absolute and drop trailing slashes and `.` components, without
/// resolving symlinks or `..` (which would diverge from the logical `$PWD`).
/// Falls back to `pwd` as given if the result isn't valid UTF-8.
fn normalize_pwd(pwd: &str) -> String {
    let path = Path::new(pwd);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(_) => return pwd.to_string(),
        }
    };
    let normalized: PathBuf = absolute.components().collect();
    normalized
        .to_str()
        .map_or_else(|| pwd.to_string(), str::to_string)
}

/// Delete all server records in `pwd` that share `command`.
/// Used in pwd-scoped mode (^S) where all duplicates in the current directory should go.
fn delete_by_command(
    cfg: &Config,
    command: &str,
    pwd: &str,
    collected: &Mutex<Vec<History>>,
) -> Result<()> {
    // Pick the IDs under the lock, then release it before the HTTP calls.
    let ids = ids_to_delete(
        &collected.lock().expect("collected mutex poisoned"),
        command,
        pwd,
    );
    if ids.is_empty() {
        eprintln!("clh: no record of this command in {pwd}; nothing deleted");
    }
    for id in ids {
        client::delete_history(cfg, id)?;
    }
    Ok(())
}

/// Delete a single record by ID.
/// Used in global mode (^T) to avoid accidentally deleting history from other directories.
fn delete_by_id(cfg: &Config, id: i32) -> Result<()> {
    client::delete_history(cfg, id)?;
    Ok(())
}

/// Spawn a background thread that fetches history pages and streams `HistoryItem`s to `tx`.
/// Fetched records are also accumulated in the returned `Arc<Mutex<Vec<History>>>` so the
/// caller can take a snapshot at any time without waiting for the thread to finish.
/// The `JoinHandle` is intentionally discarded — the thread is detached and will finish on
/// its own; dropping the handle does NOT kill the thread in Rust.
fn spawn_stream_thread(
    cfg: Config,
    pwds: Vec<String>,
    tx: SkimItemSender,
) -> Arc<Mutex<Vec<History>>> {
    let collected: Arc<Mutex<Vec<History>>> = Arc::new(Mutex::new(Vec::new()));
    let collected_bg = Arc::clone(&collected);

    thread::spawn(move || {
        let mut seen: HashSet<String> = HashSet::new();

        let _ = client::for_each_page(&cfg, &pwds, |page| {
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
    let pwds = pwd.map(search_pwds).unwrap_or_default();
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
    let collected = spawn_stream_thread(cfg.clone(), pwds.clone(), tx);

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

    if let Some(item) = output.selected_items.first() {
        delete_selected(cfg, &item.item, &pwds, &collected)?;
    }

    // Post-delete loop: start a fresh streaming session so the user sees up-to-date results.
    post_delete_loop(cfg, &pwds)
}

/// Handle Ctrl+D on `item`.
///
/// `pwds` is the search scope from [`search_pwds`]: empty for global mode (^T), where
/// only the selected record is deleted; otherwise `pwds[0]` is the current directory
/// (^S), and every record of the selected command recorded there is deleted.
/// Deletion works on a snapshot of whatever has been fetched so far (speed over
/// completeness).
fn delete_selected(
    cfg: &Config,
    item: &Arc<dyn SkimItem>,
    pwds: &[String],
    collected: &Mutex<Vec<History>>,
) -> Result<()> {
    if let Some(pwd) = pwds.first() {
        if let Some(command) = history_command(item) {
            delete_by_command(cfg, &command, pwd, collected)?;
        }
    } else if let Some(id) = history_id(item) {
        delete_by_id(cfg, id)?;
    }
    Ok(())
}

/// Run the post-delete skim loop. Opens a fresh streaming session each time.
fn post_delete_loop(cfg: &Config, pwds: &[String]) -> Result<()> {
    loop {
        let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
        let collected = spawn_stream_thread(cfg.clone(), pwds.to_vec(), tx);

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
            delete_selected(cfg, &item.item, pwds, &collected)?;
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

    // --- pwd-scoped deletion tests ---

    fn make_history_in(id: i32, command: &str, dir: &str) -> History {
        History {
            working_directory: Some(dir.to_string()),
            ..make_history(id, command, 0)
        }
    }

    #[test]
    fn ids_to_delete_only_targets_records_in_current_pwd() {
        let records = vec![
            make_history_in(1, "cargo test", "/repo/wt"),
            make_history_in(2, "cargo test", "/repo/main"),
            make_history_in(3, "cargo test", "/repo/wt"),
            make_history_in(4, "cargo build", "/repo/wt"),
        ];
        assert_eq!(
            ids_to_delete(&records, "cargo test", "/repo/wt"),
            vec![1, 3]
        );
    }

    #[test]
    fn ids_to_delete_is_empty_when_command_exists_only_in_counterpart() {
        let records = vec![make_history_in(2, "rake hocho:apply", "/repo/main")];
        assert_eq!(
            ids_to_delete(&records, "rake hocho:apply", "/repo/wt"),
            Vec::<i32>::new()
        );
    }

    #[test]
    fn search_pwds_outside_worktree_is_just_pwd() {
        let tmp = tempfile::TempDir::new().unwrap();
        let pwd = tmp.path().to_str().unwrap();
        assert_eq!(search_pwds(pwd), vec![pwd.to_string()]);
    }

    #[test]
    fn search_pwds_normalizes_trailing_slash_and_dot_components() {
        // Stored records use `$PWD` (no trailing slash, no `.`), and the query and
        // Ctrl+D both compare exactly, so `--pwd` must be normalized the same way.
        let tmp = tempfile::TempDir::new().unwrap();
        let pwd = tmp.path().to_str().unwrap();
        let sub = tmp.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let sub = sub.to_str().unwrap();

        assert_eq!(search_pwds(&format!("{pwd}/")), vec![pwd.to_string()]);
        assert_eq!(search_pwds(&format!("{pwd}/./sub")), vec![sub.to_string()]);
    }

    #[test]
    fn search_pwds_skips_non_utf8_counterpart() {
        use std::os::unix::ffi::OsStrExt;

        // A lossily converted path would never match a stored record, so don't send it.
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let main = base.join(std::ffi::OsStr::from_bytes(b"main-\xff"));
        let wt = base.join("wt");
        crate::worktree::test_support::create_main_and_worktree(&main, &wt);

        let pwd = wt.to_str().unwrap();
        assert_eq!(search_pwds(pwd), vec![pwd.to_string()]);
    }

    #[test]
    fn search_pwds_in_worktree_lists_pwd_then_main_counterpart() {
        let (_tmp, main, wt) = crate::worktree::test_support::setup_main_and_worktree();
        let pwd = wt.to_str().unwrap();
        assert_eq!(
            search_pwds(pwd),
            vec![pwd.to_string(), main.to_str().unwrap().to_string()]
        );
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
