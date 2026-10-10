use std::path::Path;
use std::process::{Command, Stdio};

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
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "core.fsmonitor=false",
            "status",
            "--porcelain",
            "--untracked-files=normal",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then(|| {
        output
            .stdout
            .split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty())
            .count()
    })
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
}
