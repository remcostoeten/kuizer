use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// @name uncommitted_changes
/// @description Counts the uncommitted changes, untracked files included, in the git repository
/// that contains `dir`. Returns `None` when `dir` is not inside a repository or git is missing.
/// Repository fsmonitor hooks are disabled and no index lock is taken.
///
/// @example
/// if let Some(count) = git::uncommitted_changes(Path::new("/home/me/apps/kearn")) {
///     println!("{count} uncommitted changes");
/// }
pub fn uncommitted_changes(dir: &Path) -> Option<usize> {
    let output = run(dir, &["status", "--porcelain", "--untracked-files=normal"])?;
    Some(
        output
            .stdout
            .split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty())
            .count(),
    )
}

/// @name repository_root
/// @description Returns the top-level directory of the git repository that contains `dir`, or
/// `None` when `dir` is not inside a repository or git is missing.
///
/// @example
/// let root = git::repository_root(Path::new("/home/me/apps/kearn/packages/web"));
pub fn repository_root(dir: &Path) -> Option<PathBuf> {
    let output = run(dir, &["rev-parse", "--show-toplevel"])?;
    let text = String::from_utf8(output.stdout).ok()?;
    let line = text.trim_end_matches(['\n', '\r']);
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// @name last_commit
/// @description Returns the committer time of the latest commit on the checked out branch of
/// the git repository that contains `dir`. Returns `None` outside a repository, in a repository
/// without commits, or when git is missing.
///
/// @example
/// if let Some(time) = git::last_commit(Path::new("/home/me/apps/kearn")) {
///     println!("last commit at {time:?}");
/// }
pub fn last_commit(dir: &Path) -> Option<SystemTime> {
    let output = run(dir, &["log", "-1", "--format=%ct"])?;
    let seconds: u64 = String::from_utf8(output.stdout).ok()?.trim().parse().ok()?;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
}

fn run(dir: &Path, args: &[&str]) -> Option<Output> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn counts_changes_and_ignores_plain_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(uncommitted_changes(dir.path()), None);
        if !git(dir.path(), &["init", "-q"]) {
            return;
        }
        assert_eq!(uncommitted_changes(dir.path()), Some(0));
        std::fs::write(dir.path().join("a.txt"), "a").expect("write");
        std::fs::write(dir.path().join("b.txt"), "b").expect("write");
        assert_eq!(uncommitted_changes(dir.path()), Some(2));
    }

    #[test]
    fn reads_the_repository_root_and_last_commit() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(repository_root(dir.path()), None);
        assert_eq!(last_commit(dir.path()), None);
        if !git(dir.path(), &["init", "-q"]) {
            return;
        }
        let nested = dir.path().join("packages/web");
        std::fs::create_dir_all(&nested).expect("mkdir");
        let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
        assert_eq!(repository_root(&nested), Some(root));
        assert_eq!(last_commit(dir.path()), None);
        std::fs::write(dir.path().join("a.txt"), "a").expect("write");
        let committed = git(dir.path(), &["add", "a.txt"])
            && git(
                dir.path(),
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "commit",
                    "-q",
                    "-m",
                    "a",
                    "--date=@1700000000",
                ],
            );
        if !committed {
            return;
        }
        assert!(last_commit(&nested).is_some_and(|time| {
            time.duration_since(UNIX_EPOCH)
                .is_ok_and(|elapsed| elapsed.as_secs() > 1_700_000_000)
        }));
    }
}
