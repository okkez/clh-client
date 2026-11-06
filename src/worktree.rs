use std::fs;
use std::path::{Path, PathBuf};

/// If `pwd` is inside a linked git worktree, return the directory at the same
/// relative position inside the main working tree.
///
/// Reads git's on-disk layout directly instead of running `git`:
///
/// - a linked worktree has a `.git` *file* containing `gitdir: <main>/.git/worktrees/<name>`
/// - that per-worktree git dir has a `commondir` file pointing at `<main>/.git`
///
/// Returns `None` when `pwd` is not inside a linked worktree: plain repository
/// (`.git` is a directory), submodule (its git dir has no `commondir`), worktree of a
/// bare repository (`core.bare = true`, or common dir not named `.git`), or a non-git
/// directory.
/// Setups driven by `GIT_DIR` / `core.worktree` are not detected.
///
/// History is recorded with the shell's logical `$PWD`, so when `pwd` goes through a
/// symlink the result goes through the same symlink (see [`to_logical`]).
pub fn main_worktree_counterpart(logical_pwd: &Path) -> Option<PathBuf> {
    // Work with the physical path like git does; map back to logical at the end.
    let pwd = logical_pwd.canonicalize().ok()?;
    let (toplevel, dot_git) = pwd
        .ancestors()
        .map(|dir| (dir, dir.join(".git")))
        .find(|(_, dot_git)| dot_git.exists())?;
    if !dot_git.is_file() {
        return None;
    }

    let dot_git_content = fs::read_to_string(&dot_git).ok()?;
    let git_dir = toplevel.join(dot_git_content.strip_prefix("gitdir:")?.trim());
    let commondir_content = fs::read_to_string(git_dir.join("commondir")).ok()?;
    let common_dir = git_dir.join(commondir_content.trim()).canonicalize().ok()?;
    if common_dir.file_name()? != ".git" || is_bare(&common_dir) {
        return None;
    }

    let main_root = common_dir.parent()?;
    let relative = pwd.strip_prefix(toplevel).ok()?;
    Some(to_logical(&main_root.join(relative), logical_pwd, &pwd))
}

/// Rewrite the physical `path` so it goes through the same symlinked prefix as
/// `logical_pwd`.
///
/// The trailing components shared by `logical_pwd` and its physical form `physical_pwd`
/// are dropped; what remains is the symlinked prefix (logical) and its target
/// (physical). If `path` lives under that target, the prefix is swapped back.
/// Otherwise (or when `logical_pwd` is relative) `path` is returned unchanged.
fn to_logical(path: &Path, logical_pwd: &Path, physical_pwd: &Path) -> PathBuf {
    if !logical_pwd.is_absolute() {
        return path.to_path_buf();
    }
    let mut logical_prefix = logical_pwd;
    let mut physical_prefix = physical_pwd;
    while let (Some(l), Some(p)) = (logical_prefix.file_name(), physical_prefix.file_name()) {
        if l != p {
            break;
        }
        match (logical_prefix.parent(), physical_prefix.parent()) {
            (Some(lp), Some(pp)) => {
                logical_prefix = lp;
                physical_prefix = pp;
            }
            _ => break,
        }
    }
    path.strip_prefix(physical_prefix)
        .map_or_else(|_| path.to_path_buf(), |rest| logical_prefix.join(rest))
}

/// Whether `<git_dir>/config` sets `core.bare` to true.
///
/// A bare repository can still be named `.git` (`git clone --bare URL proj/.git`),
/// so the directory name alone doesn't tell whether a main working tree exists.
fn is_bare(git_dir: &Path) -> bool {
    let Ok(config) = fs::read_to_string(git_dir.join("config")) else {
        return false;
    };
    let mut in_core = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_core = line.eq_ignore_ascii_case("[core]");
        } else if in_core
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("bare")
        {
            return matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "true" | "yes" | "on" | "1"
            );
        }
    }
    false
}

#[cfg(test)]
pub mod test_support {
    use std::ffi::OsStr;
    use std::fmt::Debug;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;

    /// Run `git` isolated from the developer's environment: ignore global/system
    /// config (e.g. `commit.gpgsign`, `core.hooksPath`) and any inherited `GIT_*`
    /// variables (e.g. `GIT_DIR` when tests run from a git hook).
    pub fn git<S: AsRef<OsStr> + Debug>(dir: &Path, args: &[S]) {
        let mut cmd = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(key);
            }
        }
        let output = cmd
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
            .args(args)
            .output()
            .expect("failed to run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Create a repository at `main` (with one commit) and a linked worktree at `wt`.
    pub fn create_main_and_worktree(main: &Path, wt: &Path) {
        fs::create_dir(main).unwrap();
        git(main, &["init", "-q"]);
        git(main, &["commit", "-q", "--allow-empty", "-m", "init"]);
        // `wt` may not be valid UTF-8, so pass it as an `OsStr`.
        let args: [&OsStr; 6] = [
            "worktree".as_ref(),
            "add".as_ref(),
            "-q".as_ref(),
            "-b".as_ref(),
            "feature".as_ref(),
            wt.as_os_str(),
        ];
        git(main, &args);
    }

    /// Create `<tmp>/main` (with one commit) and a linked worktree at `<tmp>/wt`.
    /// Paths are canonicalized so comparisons work where the temp dir is a symlink.
    pub fn setup_main_and_worktree() -> (TempDir, PathBuf, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let main = base.join("main");
        let wt = base.join("wt");
        create_main_and_worktree(&main, &wt);
        (tmp, main, wt)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{git, setup_main_and_worktree};
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn worktree_root_maps_to_main_root() {
        let (_tmp, main, wt) = setup_main_and_worktree();
        assert_eq!(main_worktree_counterpart(&wt), Some(main));
    }

    #[test]
    fn worktree_subdir_maps_to_same_subdir_in_main() {
        let (_tmp, main, wt) = setup_main_and_worktree();
        let sub = wt.join("src").join("foo");
        fs::create_dir_all(&sub).unwrap();
        assert_eq!(
            main_worktree_counterpart(&sub),
            Some(main.join("src").join("foo"))
        );
    }

    #[test]
    fn symlinked_pwd_maps_to_main_through_the_same_symlink() {
        // History is recorded with the shell's logical `$PWD`, so the counterpart
        // must go through the same symlink rather than the physical path.
        let (tmp, main, wt) = setup_main_and_worktree();
        let base = main.parent().unwrap();
        let link = tmp.path().canonicalize().unwrap().join("link");
        std::os::unix::fs::symlink(base, &link).unwrap();
        let sub = wt.join("src");
        fs::create_dir(&sub).unwrap();

        assert_eq!(
            main_worktree_counterpart(&link.join("wt").join("src")),
            Some(link.join("main").join("src"))
        );
    }

    #[test]
    fn main_working_tree_has_no_counterpart() {
        let (_tmp, main, _wt) = setup_main_and_worktree();
        assert_eq!(main_worktree_counterpart(&main), None);
    }

    #[test]
    fn submodule_has_no_counterpart() {
        let (_tmp, main, _wt) = setup_main_and_worktree();
        let lib = main.parent().unwrap().join("lib");
        fs::create_dir(&lib).unwrap();
        git(&lib, &["init", "-q"]);
        git(&lib, &["commit", "-q", "--allow-empty", "-m", "init"]);
        git(
            &main,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                lib.to_str().unwrap(),
                "vendor/lib",
            ],
        );
        assert_eq!(
            main_worktree_counterpart(&main.join("vendor").join("lib")),
            None
        );
    }

    #[test]
    fn non_git_directory_has_no_counterpart() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(main_worktree_counterpart(tmp.path()), None);
    }

    #[test]
    fn worktree_of_bare_repository_has_no_counterpart() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let seed = base.join("seed");
        fs::create_dir(&seed).unwrap();
        git(&seed, &["init", "-q"]);
        git(&seed, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let bare_repo = base.join("bare.git");
        git(
            &base,
            &[
                "clone",
                "-q",
                "--bare",
                seed.to_str().unwrap(),
                bare_repo.to_str().unwrap(),
            ],
        );
        let wt = base.join("wt");
        git(
            &bare_repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                wt.to_str().unwrap(),
            ],
        );
        assert_eq!(main_worktree_counterpart(&wt), None);
    }

    #[test]
    fn worktree_of_bare_repository_named_dot_git_has_no_counterpart() {
        // Layout from `git clone --bare URL proj/.git` with worktrees next to it.
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let seed = base.join("seed");
        fs::create_dir(&seed).unwrap();
        git(&seed, &["init", "-q"]);
        git(&seed, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let proj = base.join("proj");
        let bare_repo = proj.join(".git");
        git(
            &base,
            &[
                "clone",
                "-q",
                "--bare",
                seed.to_str().unwrap(),
                bare_repo.to_str().unwrap(),
            ],
        );
        let wt = proj.join("feature");
        git(
            &bare_repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                wt.to_str().unwrap(),
            ],
        );
        assert_eq!(main_worktree_counterpart(&wt), None);
    }
}
