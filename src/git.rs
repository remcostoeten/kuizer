use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
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

pub struct RootFinder {
    ceilings: Vec<PathBuf>,
    across_filesystems: bool,
    delegate: bool,
    known: Mutex<HashMap<PathBuf, Option<PathBuf>>>,
}

fn env_bool(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => true,
        other => other.parse::<i64>().is_ok_and(|number| number != 0),
    })
}

fn is_git_dir(dir: &Path) -> bool {
    ["HEAD", "objects", "refs"]
        .iter()
        .all(|name| std::fs::symlink_metadata(dir.join(name)).is_ok())
}

fn is_git_file(path: &Path) -> bool {
    let mut header = [0u8; 7];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok_and(|()| header.as_slice() == b"gitdir:")
}

fn holds_repository(dir: &Path) -> bool {
    let marker = dir.join(".git");
    match std::fs::metadata(&marker) {
        Ok(metadata) if metadata.is_dir() => is_git_dir(&marker),
        Ok(metadata) if metadata.is_file() => is_git_file(&marker),
        _ => false,
    }
}

impl RootFinder {
    /// @name RootFinder::from_env
    /// @description Builds a finder that honours `GIT_CEILING_DIRECTORIES` and
    /// `GIT_DISCOVERY_ACROSS_FILESYSTEM`. When `GIT_DIR` or `GIT_WORK_TREE` is set, discovery
    /// is left to `git rev-parse`.
    ///
    /// @example
    /// let finder = git::RootFinder::from_env();
    pub fn from_env() -> Self {
        let ceilings = std::env::var_os("GIT_CEILING_DIRECTORIES")
            .map(|list| {
                std::env::split_paths(&list)
                    .filter(|path| path.is_absolute())
                    .flat_map(|path| {
                        let resolved = std::fs::canonicalize(&path).ok();
                        [Some(path), resolved].into_iter().flatten()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let delegate = ["GIT_DIR", "GIT_WORK_TREE"]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| value != OsStr::new("")));
        Self {
            ceilings,
            across_filesystems: env_bool("GIT_DISCOVERY_ACROSS_FILESYSTEM"),
            delegate,
            known: Mutex::new(HashMap::new()),
        }
    }

    /// @name RootFinder::find
    /// @description Returns the top-level directory of the repository that contains `dir`,
    /// which must be an absolute path without symlinks, or `None` outside a repository.
    ///
    /// @example
    /// let root = finder.find(Path::new("/home/me/apps/kearn/packages/web"));
    pub fn find(&self, dir: &Path) -> Option<PathBuf> {
        if self.delegate {
            return repository_root(dir);
        }
        let mut chain: Vec<&Path> = Vec::new();
        let mut found = None;
        let mut current = dir;
        loop {
            if let Some(known) = self
                .known
                .lock()
                .ok()
                .and_then(|known| known.get(current).cloned())
            {
                found = known;
                break;
            }
            chain.push(current);
            if holds_repository(current) {
                found = Some(current.to_path_buf());
                break;
            }
            let Some(parent) = current.parent() else {
                break;
            };
            if self.ceilings.iter().any(|ceiling| ceiling == parent) {
                break;
            }
            if !self.across_filesystems && !same_device(current, parent) {
                break;
            }
            current = parent;
        }
        if let Ok(mut known) = self.known.lock() {
            for visited in chain {
                known.insert(visited.to_path_buf(), found.clone());
            }
        }
        found
    }
}

fn same_device(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => false,
    }
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

    fn finder_with(ceilings: Vec<PathBuf>) -> RootFinder {
        RootFinder {
            ceilings,
            across_filesystems: false,
            delegate: false,
            known: Mutex::new(HashMap::new()),
        }
    }

    fn fake_repository(dir: &Path) {
        std::fs::create_dir_all(dir.join(".git/objects")).expect("objects");
        std::fs::create_dir_all(dir.join(".git/refs")).expect("refs");
        std::fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").expect("head");
    }

    #[test]
    fn finds_roots_the_way_git_does() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = std::fs::canonicalize(tmp.path()).expect("canonicalize");
        let outer = base.join("repo");
        fake_repository(&outer);
        let nested = outer.join("packages/web");
        std::fs::create_dir_all(nested.join(".git")).expect("empty .git");
        std::fs::create_dir_all(base.join("plain/deep")).expect("plain");
        let linked = base.join("linked/src");
        std::fs::create_dir_all(&linked).expect("linked");
        std::fs::write(
            base.join("linked/.git"),
            "gitdir: ../repo/.git/worktrees/w\n",
        )
        .expect("gitfile");
        let symlinked = base.join("symlinked/src");
        std::fs::create_dir_all(&symlinked).expect("symlinked");
        std::os::unix::fs::symlink(outer.join(".git"), base.join("symlinked/.git"))
            .expect("symlinked .git");

        let finder = finder_with(vec![base.parent().expect("parent").to_path_buf()]);
        assert_eq!(finder.find(&nested), Some(outer.clone()));
        assert_eq!(finder.find(&outer), Some(outer.clone()));
        assert_eq!(finder.find(&linked), Some(base.join("linked")));
        assert_eq!(finder.find(&symlinked), Some(base.join("symlinked")));
        assert_eq!(finder.find(&base.join("plain/deep")), None);
        assert_eq!(finder.find(&base.join("plain/deep")), None);
        assert_eq!(finder.find(&base), None);

        let fenced = finder_with(vec![outer.clone()]);
        assert_eq!(fenced.find(&nested), None);
        assert_eq!(fenced.find(&outer), Some(outer));
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
