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
/// bare repository (common dir is not named `.git`), or a non-git directory.
/// Setups driven by `GIT_DIR` / `core.worktree` are not detected.
pub fn counterpart_dir(pwd: &Path) -> Option<PathBuf> {
    // `$PWD` may go through symlinks; work with the physical path like git does.
    let pwd = pwd.canonicalize().ok()?;
    let (toplevel, dot_git) = pwd
        .ancestors()
        .map(|dir| (dir, dir.join(".git")))
        .find(|(_, dot_git)| dot_git.exists())?;
    if !dot_git.is_file() {
        return None;
    }

    let content = fs::read_to_string(&dot_git).ok()?;
    let git_dir = toplevel.join(content.strip_prefix("gitdir:")?.trim());
    let common_dir = fs::read_to_string(git_dir.join("commondir")).ok()?;
    let common_dir = git_dir.join(common_dir.trim()).canonicalize().ok()?;
    if common_dir.file_name()? != ".git" {
        return None;
    }

    let main_root = common_dir.parent()?;
    let relative = pwd.strip_prefix(toplevel).ok()?;
    Some(main_root.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
            .args(args)
            .output()
            .expect("failed to run git");
        assert!(
            status.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    /// Create `<tmp>/main` (with one commit) and a linked worktree at `<tmp>/wt`.
    /// Paths are canonicalized so comparisons work where the temp dir is a symlink.
    fn setup_main_and_worktree() -> (TempDir, PathBuf, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let main = base.join("main");
        fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let wt = base.join("wt");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                wt.to_str().unwrap(),
            ],
        );
        (tmp, main, wt)
    }

    #[test]
    fn worktree_root_maps_to_main_root() {
        let (_tmp, main, wt) = setup_main_and_worktree();
        assert_eq!(counterpart_dir(&wt), Some(main));
    }

    #[test]
    fn worktree_subdir_maps_to_same_subdir_in_main() {
        let (_tmp, main, wt) = setup_main_and_worktree();
        let sub = wt.join("src").join("foo");
        fs::create_dir_all(&sub).unwrap();
        assert_eq!(counterpart_dir(&sub), Some(main.join("src").join("foo")));
    }

    #[test]
    fn main_working_tree_has_no_counterpart() {
        let (_tmp, main, _wt) = setup_main_and_worktree();
        assert_eq!(counterpart_dir(&main), None);
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
        assert_eq!(counterpart_dir(&main.join("vendor").join("lib")), None);
    }

    #[test]
    fn non_git_directory_has_no_counterpart() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(counterpart_dir(tmp.path()), None);
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
        assert_eq!(counterpart_dir(&wt), None);
    }
}
