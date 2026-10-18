use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime};

use crossbeam_channel::Receiver;
use serde::{Deserialize, Serialize};

use crate::config::{Config, ConfigError, GitConfig};
use crate::dupes::{Files, Options};
use crate::platform::MountTable;
use crate::protect::Protected;
use crate::scan::walk::{Flow, Visitor, Walker};
use crate::scan::{self, Cancel, Issue, Scan};

const KIB: u64 = 1024;
const STALE_GC: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Objects {
    pub loose: u64,
    pub loose_bytes: u64,
    pub packs: u64,
    pub packed_bytes: u64,
    pub prunable: u64,
    pub garbage_bytes: u64,
}

impl Objects {
    /// @name Objects::gain
    /// @description Estimates what `git gc` frees: the loose objects it packs and the garbage
    /// files it removes. Packing keeps a compressed copy, so the real gain is somewhat lower.
    ///
    /// @example
    /// println!("up to {} freed", ByteSize(objects.gain()));
    pub fn gain(&self) -> u64 {
        self.loose_bytes + self.garbage_bytes
    }

    pub fn total(&self) -> u64 {
        self.loose_bytes + self.packed_bytes + self.garbage_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub commit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Busy {
    Rebase,
    Merge,
    CherryPick,
    Revert,
    Bisect,
    Locked,
    Collecting,
}

impl Busy {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Rebase => "a rebase is in progress",
            Self::Merge => "a merge is in progress",
            Self::CherryPick => "a cherry-pick is in progress",
            Self::Revert => "a revert is in progress",
            Self::Bisect => "a bisect is in progress",
            Self::Locked => "another git process holds a lock",
            Self::Collecting => "git gc is already running",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub objects: Objects,
    pub default_branch: Option<String>,
    pub current_branch: Option<String>,
    pub merged: Vec<Branch>,
    pub busy: Option<Busy>,
}

impl Repository {
    /// @name Repository::worth
    /// @description Returns whether the repository has something to clean: a `git gc` gain of at
    /// least `min_gain`, or local branches already merged into the default branch.
    ///
    /// @example
    /// found.retain(|repository| repository.worth(config.git.min_gain.0));
    pub fn worth(&self, min_gain: u64) -> bool {
        self.collectable(min_gain) || !self.merged.is_empty()
    }

    /// @name Repository::collectable
    /// @description Returns whether `git gc` would free at least `min_gain` here, the bar for
    /// running it in a bulk cleanup.
    ///
    /// @example
    /// let ready = found.iter().filter(|repository| repository.collectable(min_gain));
    pub fn collectable(&self, min_gain: u64) -> bool {
        self.objects.gain() >= min_gain.max(1)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RepoError {
    #[error("{} is not a git repository", .0.display())]
    NotRepository(PathBuf),
    #[error("skipped because {}", .0.describe())]
    Busy(Busy),
    #[error("branch `{0}` is no longer merged or has moved")]
    Changed(String),
    #[error("branch `{0}` is kept by the git.keep_branches setting or is checked out")]
    Kept(String),
    #[error("git failed: {0}")]
    Git(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Collected {
    pub before: u64,
    pub after: u64,
}

impl Collected {
    pub fn freed(&self) -> u64 {
        self.before.saturating_sub(self.after)
    }
}

#[derive(Debug)]
pub struct Pruned {
    pub branch: Branch,
    pub result: Result<(), RepoError>,
}

#[derive(Debug, Default)]
pub struct Repositories {
    pub roots: Vec<PathBuf>,
}

impl Visitor for Repositories {
    fn issue(&mut self, _issue: Issue) -> Flow {
        Flow::Continue
    }

    fn repository(&mut self, root: &Path) {
        self.roots.push(root.to_path_buf());
    }
}

pub struct Walked {
    pub files: Option<Files>,
    pub repositories: Vec<PathBuf>,
}

/// @name start_scan
/// @description Starts an artifact scan that also notes every git repository and, with
/// `duplicates`, collects files for duplicate detection, all during one walk. The receiver
/// yields what was collected once the walk ends.
///
/// @example
/// let (scan, walked) = repos::start_scan(&config, &roots, Cancel::new(), None)?;
/// let collected = scan::collect(&scan, |_| {});
/// let found = repos::inspect_all(walked.recv()?.repositories, &config.git, &Cancel::new());
pub fn start_scan(
    config: &Config,
    roots: &[PathBuf],
    cancel: Cancel,
    duplicates: Option<Options>,
) -> Result<(Scan, Receiver<Walked>), ConfigError> {
    let (scan, companion) = scan::start_with(
        config,
        roots,
        cancel,
        (duplicates.map(Files::new), Repositories::default()),
    )?;
    let (sender, receiver) = crossbeam_channel::bounded(1);
    thread::Builder::new()
        .name("kuizer-repos".to_owned())
        .spawn(move || {
            if let Ok((files, repositories)) = companion.recv() {
                let _ = sender.send(Walked {
                    files,
                    repositories: repositories.roots,
                });
            }
        })
        .expect("spawn repository thread");
    Ok((scan, receiver))
}

/// @name find
/// @description Walks `roots` with the scan settings and returns the top-level folder of every
/// git repository found, without sizing any artifact.
///
/// @example
/// let roots = repos::find(&config, &[PathBuf::from("/home/me/apps")])?;
pub fn find(config: &Config, roots: &[PathBuf]) -> Result<Vec<PathBuf>, ConfigError> {
    let detector = config.detector()?;
    let excludes = config.excludes()?;
    let mounts = MountTable::load();
    let protected = Protected::for_user();
    let mut walker = Walker {
        scan: &config.scan,
        detector: &detector,
        excludes: &excludes,
        mounts: &mounts,
        protected: &protected,
        visitor: Repositories::default(),
    };
    for root in scan::normalize_roots(roots) {
        walker.walk_root(&root);
    }
    let mut found = walker.visitor.roots;
    found.sort();
    found.dedup();
    Ok(found)
}

fn threads(config: &GitConfig) -> usize {
    match config.threads {
        0 => thread::available_parallelism().map_or(4, |n| n.get().min(8)),
        n => n,
    }
}

/// @name inspect_all
/// @description Inspects every repository in parallel and keeps the ones worth cleaning, largest
/// gain first. Stops early once `cancel` is set.
///
/// @example
/// let found = repos::inspect_all(repos::find(&config, &roots)?, &config.git, &Cancel::new());
pub fn inspect_all(roots: Vec<PathBuf>, config: &GitConfig, cancel: &Cancel) -> Vec<Repository> {
    let queue = Mutex::new(roots);
    let found = Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..threads(config) {
            scope.spawn(|| {
                loop {
                    if cancel.is_cancelled() {
                        return;
                    }
                    let Some(root) = queue.lock().ok().and_then(|mut queue| queue.pop()) else {
                        return;
                    };
                    if let Ok(repository) = inspect(&root, config)
                        && repository.worth(config.min_gain.0)
                        && let Ok(mut found) = found.lock()
                    {
                        found.push(repository);
                    }
                }
            });
        }
    });
    let mut found = found.into_inner().unwrap_or_default();
    found.sort_by(|a, b| {
        b.objects
            .gain()
            .cmp(&a.objects.gain())
            .then_with(|| b.merged.len().cmp(&a.merged.len()))
            .then_with(|| a.root.cmp(&b.root))
    });
    found
}

/// @name inspect
/// @description Reads one repository: its object counts from `git count-objects`, its default
/// and current branch, the local branches already merged into the default branch that may be
/// removed, and whether another git operation is in progress. Kept branches, the default branch
/// and branches checked out in any worktree are never listed as merged.
///
/// @example
/// let repository = repos::inspect(Path::new("/home/me/apps/kearn"), &config.git)?;
/// println!("{} merged branches", repository.merged.len());
pub fn inspect(root: &Path, config: &GitConfig) -> Result<Repository, RepoError> {
    let git_dir = root.join(".git");
    if !git_dir.is_dir() {
        return Err(RepoError::NotRepository(root.to_path_buf()));
    }
    let objects = count_objects(root)?;
    let current_branch = read(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let default_branch = default_branch(root, config);
    let merged = match &default_branch {
        Some(base) => merged_branches(root, base, current_branch.as_deref(), config),
        None => Vec::new(),
    };
    Ok(Repository {
        busy: busy(&git_dir),
        root: root.to_path_buf(),
        git_dir,
        objects,
        default_branch,
        current_branch,
        merged,
    })
}

/// @name busy
/// @description Returns the git operation in progress in `git_dir`, if any: a rebase, merge,
/// cherry-pick, revert or bisect, a held index or ref lock, or a running `git gc`.
///
/// @example
/// if let Some(reason) = repos::busy(Path::new("/home/me/apps/kearn/.git")) {
///     println!("skipped: {}", reason.describe());
/// }
pub fn busy(git_dir: &Path) -> Option<Busy> {
    let has = |name: &str| git_dir.join(name).exists();
    if has("rebase-merge") || has("rebase-apply") {
        return Some(Busy::Rebase);
    }
    if has("MERGE_HEAD") {
        return Some(Busy::Merge);
    }
    if has("CHERRY_PICK_HEAD") {
        return Some(Busy::CherryPick);
    }
    if has("REVERT_HEAD") {
        return Some(Busy::Revert);
    }
    if has("BISECT_LOG") {
        return Some(Busy::Bisect);
    }
    if ["index.lock", "HEAD.lock", "packed-refs.lock", "config.lock"]
        .iter()
        .any(|name| has(name))
    {
        return Some(Busy::Locked);
    }
    let collecting = fs::metadata(git_dir.join("gc.pid"))
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .is_ok_and(|age| age < STALE_GC)
        });
    collecting.then_some(Busy::Collecting)
}

/// @name collect_garbage
/// @description Runs `git gc` in the repository, with `--aggressive` when `git.aggressive` is
/// set, after checking again that no other git operation is in progress. Measures the git
/// folder before and after so the result reports what was actually freed. Nothing goes to the
/// trash: unreachable objects older than git's prune expiry are gone for good.
///
/// @example
/// let collected = repos::collect_garbage(&repository, &config.git)?;
/// println!("freed {}", ByteSize(collected.freed()));
pub fn collect_garbage(
    repository: &Repository,
    config: &GitConfig,
) -> Result<Collected, RepoError> {
    if !repository.git_dir.is_dir() {
        return Err(RepoError::NotRepository(repository.root.clone()));
    }
    if let Some(reason) = busy(&repository.git_dir) {
        return Err(RepoError::Busy(reason));
    }
    let cancel = Cancel::new();
    let before = scan::measure(&repository.git_dir, &cancel).usage.allocated;
    let mut args = vec!["gc", "--quiet"];
    if config.aggressive {
        args.push("--aggressive");
    }
    let output = git(&repository.root, &args).map_err(RepoError::Git)?;
    if !output.status.success() {
        return Err(RepoError::Git(stderr(&output)));
    }
    let after = scan::measure(&repository.git_dir, &cancel).usage.allocated;
    Ok(Collected { before, after })
}

/// @name delete_branches
/// @description Deletes the named local branches of the repository. Each one is checked again
/// first: it must still point at the commit that was listed, still be merged into the default
/// branch, and not be kept, the default branch or checked out. Remote branches are never
/// touched. Restore one with `git branch NAME COMMIT`.
///
/// @example
/// for pruned in repos::delete_branches(&repository, &repository.merged, &config.git) {
///     println!("{}: {:?}", pruned.branch.name, pruned.result);
/// }
pub fn delete_branches(
    repository: &Repository,
    branches: &[Branch],
    config: &GitConfig,
) -> Vec<Pruned> {
    if let Some(reason) = busy(&repository.git_dir) {
        return branches
            .iter()
            .map(|branch| Pruned {
                branch: branch.clone(),
                result: Err(RepoError::Busy(reason)),
            })
            .collect();
    }
    let root = &repository.root;
    let current = read(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let base = default_branch(root, config);
    let checked_out = worktree_branches(root);
    branches
        .iter()
        .map(|branch| Pruned {
            branch: branch.clone(),
            result: delete_branch(
                root,
                branch,
                base.as_deref(),
                current.as_deref(),
                &checked_out,
                config,
            ),
        })
        .collect()
}

fn delete_branch(
    root: &Path,
    branch: &Branch,
    base: Option<&str>,
    current: Option<&str>,
    checked_out: &[String],
    config: &GitConfig,
) -> Result<(), RepoError> {
    let name = branch.name.as_str();
    if kept(name, base, current, checked_out, config) {
        return Err(RepoError::Kept(branch.name.clone()));
    }
    let reference = format!("refs/heads/{name}");
    let at = read(root, &["rev-parse", "--verify", "--quiet", &reference]);
    let merged = base.is_some_and(|base| {
        bases(root, base)
            .iter()
            .any(|base| succeeds(root, &["merge-base", "--is-ancestor", &branch.commit, base]))
    });
    if at.as_deref() != Some(branch.commit.as_str()) || !merged {
        return Err(RepoError::Changed(branch.name.clone()));
    }
    let output = git(root, &["branch", "-D", name]).map_err(RepoError::Git)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(RepoError::Git(stderr(&output)))
    }
}

fn kept(
    name: &str,
    base: Option<&str>,
    current: Option<&str>,
    checked_out: &[String],
    config: &GitConfig,
) -> bool {
    Some(name) == base
        || Some(name) == current
        || checked_out.iter().any(|branch| branch == name)
        || config.keep_branches.iter().any(|kept| kept == name)
}

fn count_objects(root: &Path) -> Result<Objects, RepoError> {
    let output = git(root, &["count-objects", "-v"]).map_err(RepoError::Git)?;
    if !output.status.success() {
        return Err(RepoError::NotRepository(root.to_path_buf()));
    }
    Ok(parse_count(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_count(text: &str) -> Objects {
    let mut objects = Objects::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let Ok(value) = value.trim().parse::<u64>() else {
            continue;
        };
        match key.trim() {
            "count" => objects.loose = value,
            "size" => objects.loose_bytes = value * KIB,
            "packs" => objects.packs = value,
            "size-pack" => objects.packed_bytes = value * KIB,
            "prune-packable" => objects.prunable = value,
            "size-garbage" => objects.garbage_bytes = value * KIB,
            _ => {}
        }
    }
    objects
}

fn default_branch(root: &Path, config: &GitConfig) -> Option<String> {
    if let Some(remote) = read(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) && let Some(name) = remote.strip_prefix("origin/")
    {
        return Some(name.to_owned());
    }
    ["main", "master"]
        .into_iter()
        .chain(config.keep_branches.iter().map(String::as_str))
        .find(|name| has_ref(root, &format!("refs/heads/{name}")))
        .map(str::to_owned)
}

fn bases(root: &Path, base: &str) -> Vec<String> {
    [
        format!("refs/heads/{base}"),
        format!("refs/remotes/origin/{base}"),
    ]
    .into_iter()
    .filter(|reference| has_ref(root, reference))
    .collect()
}

fn merged_branches(
    root: &Path,
    base: &str,
    current: Option<&str>,
    config: &GitConfig,
) -> Vec<Branch> {
    let checked_out = worktree_branches(root);
    let mut merged: Vec<Branch> = Vec::new();
    for reference in bases(root, base) {
        let Some(listed) = read(
            root,
            &[
                "for-each-ref",
                "--format=%(refname:lstrip=2)%09%(objectname)",
                &format!("--merged={reference}"),
                "refs/heads/",
            ],
        ) else {
            continue;
        };
        for line in listed.lines() {
            let Some((name, commit)) = line.split_once('\t') else {
                continue;
            };
            if kept(name, Some(base), current, &checked_out, config)
                || merged.iter().any(|branch| branch.name == name)
            {
                continue;
            }
            merged.push(Branch {
                name: name.to_owned(),
                commit: commit.to_owned(),
            });
        }
    }
    merged.sort_by(|a, b| a.name.cmp(&b.name));
    merged
}

fn worktree_branches(root: &Path) -> Vec<String> {
    read(root, &["worktree", "list", "--porcelain"])
        .map(|text| {
            text.lines()
                .filter_map(|line| line.strip_prefix("branch refs/heads/"))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn has_ref(root: &Path, reference: &str) -> bool {
    succeeds(root, &["show-ref", "--verify", "--quiet", reference])
}

fn succeeds(root: &Path, args: &[&str]) -> bool {
    git(root, args).is_ok_and(|output| output.status.success())
}

fn read(root: &Path, args: &[&str]) -> Option<String> {
    let output = git(root, args).ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim_end_matches(['\n', '\r']);
    (!text.is_empty()).then(|| text.to_owned())
}

fn stderr(output: &Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    let text = text.trim();
    if text.is_empty() {
        format!("exited with {}", output.status)
    } else {
        text.lines().last().unwrap_or(text).to_owned()
    }
}

fn git(root: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args([
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn commit(dir: &Path, file: &str) -> bool {
        fs::write(dir.join(file), file).is_ok()
            && run(dir, &["add", file])
            && run(dir, &["commit", "-q", "-m", file])
    }

    fn repository(dir: &Path) -> bool {
        run(dir, &["init", "-q"]) && commit(dir, "a.txt")
    }

    #[test]
    fn parses_count_objects_output() {
        let objects = parse_count(
            "count: 12\nsize: 48\nin-pack: 3000\npacks: 2\nsize-pack: 1200\n\
             prune-packable: 3\ngarbage: 1\nsize-garbage: 4\n",
        );
        assert_eq!(
            objects,
            Objects {
                loose: 12,
                loose_bytes: 48 * KIB,
                packs: 2,
                packed_bytes: 1200 * KIB,
                prunable: 3,
                garbage_bytes: 4 * KIB,
            }
        );
        assert_eq!(objects.gain(), 52 * KIB);
        assert_eq!(parse_count("nonsense\nsize: x\n"), Objects::default());
    }

    #[test]
    fn lists_merged_branches_but_never_kept_or_checked_out_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        if !repository(root) {
            return;
        }
        assert!(run(root, &["branch", "done"]));
        assert!(run(root, &["branch", "develop"]));
        assert!(run(root, &["checkout", "-q", "-b", "open"]));
        assert!(commit(root, "b.txt"));
        assert!(run(root, &["checkout", "-q", "-b", "current", "main"]));

        let found = inspect(root, &GitConfig::default()).expect("inspects");
        assert_eq!(found.default_branch.as_deref(), Some("main"));
        assert_eq!(found.current_branch.as_deref(), Some("current"));
        let names: Vec<&str> = found.merged.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["done"]);
        assert!(found.objects.loose > 0);
        assert!(found.worth(1));
        assert_eq!(found.busy, None);
    }

    #[test]
    fn plain_folders_are_not_repositories() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            inspect(dir.path(), &GitConfig::default()),
            Err(RepoError::NotRepository(dir.path().to_path_buf()))
        );
    }

    #[test]
    fn deletes_merged_branches_after_checking_them_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        if !repository(root) {
            return;
        }
        assert!(run(root, &["branch", "done"]));
        assert!(run(root, &["branch", "moved"]));
        let config = GitConfig::default();
        let found = inspect(root, &config).expect("inspects");
        assert_eq!(found.merged.len(), 2);

        assert!(run(root, &["checkout", "-q", "moved"]));
        assert!(commit(root, "c.txt"));
        assert!(run(root, &["checkout", "-q", "main"]));

        let mut asked = found.merged.clone();
        asked.push(Branch {
            name: "main".to_owned(),
            commit: String::new(),
        });
        let pruned = delete_branches(&found, &asked, &config);
        let results: Vec<(&str, &Result<(), RepoError>)> = pruned
            .iter()
            .map(|pruned| (pruned.branch.name.as_str(), &pruned.result))
            .collect();
        assert_eq!(
            results,
            [
                ("done", &Ok(())),
                ("moved", &Err(RepoError::Changed("moved".to_owned()))),
                ("main", &Err(RepoError::Kept("main".to_owned()))),
            ]
        );
        assert!(!has_ref(root, "refs/heads/done"));
        assert!(has_ref(root, "refs/heads/moved"));
    }

    #[test]
    fn busy_repositories_are_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        if !repository(root) {
            return;
        }
        assert!(run(root, &["branch", "done"]));
        let config = GitConfig::default();
        let found = inspect(root, &config).expect("inspects");
        fs::write(root.join(".git/index.lock"), "").expect("lock");
        assert_eq!(busy(&found.git_dir), Some(Busy::Locked));
        assert_eq!(
            collect_garbage(&found, &config),
            Err(RepoError::Busy(Busy::Locked))
        );
        let pruned = delete_branches(&found, &found.merged, &config);
        assert_eq!(pruned[0].result, Err(RepoError::Busy(Busy::Locked)));
        assert!(has_ref(root, "refs/heads/done"));
        fs::remove_file(root.join(".git/index.lock")).expect("unlock");
        fs::create_dir(root.join(".git/rebase-merge")).expect("rebase");
        assert_eq!(busy(&found.git_dir), Some(Busy::Rebase));
    }

    #[test]
    fn collects_garbage_and_reports_the_gain() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        if !repository(root) {
            return;
        }
        for index in 0..20 {
            assert!(commit(root, &format!("{index}.txt")));
        }
        let config = GitConfig::default();
        let found = inspect(root, &config).expect("inspects");
        assert!(found.objects.loose >= 40);
        let collected = collect_garbage(&found, &config).expect("gc");
        assert!(collected.after < collected.before, "{collected:?}");
        let after = inspect(root, &config).expect("inspects");
        assert_eq!(after.objects.loose, 0);
        assert!(after.objects.packs >= 1);
    }

    #[test]
    fn the_walk_finds_nested_repositories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let nested = root.join("apps/kearn");
        fs::create_dir_all(&nested).expect("mkdir");
        fs::create_dir_all(root.join("notes")).expect("mkdir");
        if !run(&nested, &["init", "-q"]) || !run(root, &["init", "-q"]) {
            return;
        }
        let found = find(&Config::default(), &[root.to_path_buf()]).expect("walks");
        let root = fs::canonicalize(root).expect("canonicalize");
        assert_eq!(found, [root.clone(), root.join("apps/kearn")]);
    }
}
