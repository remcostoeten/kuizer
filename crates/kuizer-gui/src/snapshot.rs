use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use kuizer::caches;
use kuizer::detect::{Category, Kind, RuleInfo, Safety};
use kuizer::docker;
use kuizer::dupes::{self, Duplicates, Group, Member};
use kuizer::history::History;
use kuizer::repos::Repository;
use kuizer::scan::{Candidate, Issue, IssueKind, Progress, Summary, Usage};
use kuizer::toolchains::Toolchain;
use serde::{Deserialize, Serialize};

use crate::model::{Disk, HistoryView, Loaded, Report};
use crate::start::ScanOptions;

const SCHEMA: u32 = 4;
const LIMIT: usize = 12;
const BUDGET: u64 = 48 << 20;
const EXTENSION: &str = "json";
const SIDECAR: &str = "dupes.json";

/// @name Snapshots
/// @description The last finished scan for each set of folders, one JSON file per set in the
/// GUI state folder, so the window can show it at once while a fresh scan runs. Duplicate files
/// live in a sidecar file next to it, written only when a duplicate search ran and read only
/// when the Duplicates section needs it. Keeps the newest files within a count and a size
/// budget and ignores files written by another schema or kuizer version.
///
/// @example
/// let snapshots = Snapshots::for_user();
/// let shown = snapshots.load(&roots, &options);
#[derive(Debug, Clone)]
pub struct Snapshots {
    dir: Option<PathBuf>,
}

impl Snapshots {
    pub fn for_user() -> Self {
        Self::new(
            dirs::state_dir()
                .or_else(dirs::data_local_dir)
                .map(|dir| dir.join("kuizer").join("gui-scans")),
        )
    }

    pub fn new(dir: Option<PathBuf>) -> Self {
        Self { dir }
    }

    /// @name Snapshots::load
    /// @description The saved scan of these folders, fitted to `options`: sections the options
    /// turn off are left empty and artifacts of categories that are off are dropped, so a scan
    /// with other options still opens with everything that applies.
    ///
    /// @example
    /// let shown = snapshots.load(&roots, &options);
    pub fn load(&self, roots: &[PathBuf], options: &ScanOptions) -> Option<Report> {
        let bytes = std::fs::read(self.file(roots, EXTENSION)?).ok()?;
        let stored: Stored = serde_json::from_slice(&bytes).ok()?;
        if stored.schema != SCHEMA
            || stored.version != env!("CARGO_PKG_VERSION")
            || stored.roots != roots
        {
            return None;
        }
        let mut report = stored.restore();
        fit(&mut report, options);
        Some(report)
    }

    fn write(&self, stored: &Stored) {
        let (Some(dir), Some(file)) = (&self.dir, self.file(&stored.roots, EXTENSION)) else {
            return;
        };
        if let Err(error) = write(dir, &file, stored) {
            eprintln!("kuizer-gui: cannot save the scan for next time: {error}");
            return;
        }
        prune(dir, &file);
    }

    /// @name Snapshots::load_duplicates
    /// @description The duplicate files found by the last search under these folders, when it
    /// ran with the same duplicate options.
    ///
    /// @example
    /// let found = snapshots.load_duplicates(&roots, Options::from_config(&config));
    pub fn load_duplicates(
        &self,
        roots: &[PathBuf],
        options: dupes::Options,
    ) -> Option<Duplicates> {
        let bytes = std::fs::read(self.file(roots, SIDECAR)?).ok()?;
        let stored: StoredSidecar = serde_json::from_slice(&bytes).ok()?;
        (stored.schema == SCHEMA
            && stored.version == env!("CARGO_PKG_VERSION")
            && stored.roots == roots
            && stored.min_size == options.min_size
            && stored.verify == options.verify)
            .then(|| stored.duplicates.restore())
    }

    /// @name Snapshots::stage_duplicates
    /// @description Builds the saved form of a duplicate search, so the result can go to the
    /// window right away and the file is written after it with `Staged::save`.
    ///
    /// @example
    /// let staged = snapshots.stage_duplicates(&roots, options, &found);
    /// show(found);
    /// staged.save();
    pub fn stage_duplicates(
        &self,
        roots: &[PathBuf],
        options: dupes::Options,
        duplicates: &Duplicates,
    ) -> Staged {
        Staged {
            snapshots: self.clone(),
            stored: StoredSidecar {
                schema: SCHEMA,
                version: env!("CARGO_PKG_VERSION").to_owned(),
                roots: roots.to_vec(),
                min_size: options.min_size,
                verify: options.verify,
                duplicates: StoredDuplicates::new(duplicates),
            },
        }
    }

    fn file(&self, roots: &[PathBuf], extension: &str) -> Option<PathBuf> {
        let key = serde_json::to_vec(roots).ok()?;
        let dir = self.dir.as_ref()?;
        Some(dir.join(format!("{:016x}.{extension}", fnv(&key))))
    }
}

/// A duplicate search ready to be written next to the scan of the same folders.
pub struct Staged {
    snapshots: Snapshots,
    stored: StoredSidecar,
}

impl Staged {
    pub fn save(self) {
        let (Some(dir), Some(file)) = (
            &self.snapshots.dir,
            self.snapshots.file(&self.stored.roots, SIDECAR),
        ) else {
            return;
        };
        if let Err(error) = write(dir, &file, &self.stored) {
            eprintln!("kuizer-gui: cannot save the duplicates for next time: {error}");
            return;
        }
        prune(dir, &file);
    }
}

/// @name Recorder
/// @description Builds the saved form of a scan from its parts as they arrive, on the thread
/// that delivers them, so the window gets each part without waiting for it to be saved and the
/// file is written once the whole scan is on screen.
///
/// @example
/// let recorder = Recorder::new(&roots, &options);
/// recorder.record(&loaded);
/// recorder.save(&snapshots, SystemTime::now());
pub struct Recorder {
    stored: Mutex<Stored>,
}

impl Recorder {
    pub fn new(roots: &[PathBuf], options: &ScanOptions) -> Self {
        Self {
            stored: Mutex::new(Stored::empty(roots, options)),
        }
    }

    pub fn record(&self, loaded: &Loaded) {
        if let Ok(mut stored) = self.stored.lock() {
            stored.put(loaded);
        }
    }

    pub fn save(self, snapshots: &Snapshots, scanned: SystemTime) {
        let mut stored = self
            .stored
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        stored.saved = scanned;
        snapshots.write(&stored);
    }
}

fn fit(report: &mut Report, options: &ScanOptions) {
    if !options.caches {
        report.caches.clear();
    }
    if !options.toolchains {
        report.toolchains.clear();
    }
    if !options.docker {
        report.docker.clear();
        report.docker_issues.clear();
    }
    if !options.repos {
        report.repos.clear();
    }
    report.candidates.retain(|candidate| {
        options.categories.contains(&candidate.rule.category)
            && candidate.usage.allocated >= options.min_size
    });
    report.options = options.clone();
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn write(dir: &Path, file: &Path, stored: &impl Serialize) -> std::io::Result<()> {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(dir)?;
    let bytes = serde_json::to_vec(stored)?;
    let temp = file.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

fn prune(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut others: Vec<(SystemTime, u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path != keep && path.extension().is_some_and(|ext| ext == EXTENSION))
        .map(|path| {
            let metadata = std::fs::metadata(&path).ok();
            let modified = metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (
                modified,
                metadata.map_or(0, |metadata| metadata.len()),
                path,
            )
        })
        .collect();
    others.sort_by_key(|(modified, _, _)| std::cmp::Reverse(*modified));
    let mut used = std::fs::metadata(keep).map_or(0, |metadata| metadata.len());
    for (index, (_, bytes, path)) in others.into_iter().enumerate() {
        used += bytes;
        if index + 1 >= LIMIT || used > BUDGET {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// @name age_label
/// @description Says how old a shown scan is, in words, for the marker on results that come
/// from an earlier scan.
///
/// @example
/// assert_eq!(age_label(then, then + Duration::from_secs(7200)), "From 2 hours ago");
pub fn age_label(saved: SystemTime, now: SystemTime) -> String {
    let seconds = now
        .duration_since(saved)
        .map_or(0, |elapsed| elapsed.as_secs());
    let (count, noun) = match seconds {
        0..60 => return "From just now".to_owned(),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        _ => (seconds / 86_400, "day"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("From {count} {noun}{plural} ago")
}

#[derive(Serialize, Deserialize)]
struct Stored {
    schema: u32,
    version: String,
    saved: SystemTime,
    roots: Vec<PathBuf>,
    options: ScanOptions,
    rules: Vec<StoredRule>,
    candidates: Vec<StoredCandidate>,
    issues: Vec<StoredIssue>,
    summary: StoredSummary,
    caches: Vec<(String, PathBuf, StoredCandidate)>,
    toolchains: Vec<(Toolchain, StoredCandidate)>,
    docker: Vec<docker::Item>,
    docker_issues: Vec<String>,
    repos: Vec<Repository>,
    history: Result<StoredHistory, String>,
    disks: Vec<(Option<u64>, Option<u64>)>,
}

impl Stored {
    fn empty(roots: &[PathBuf], options: &ScanOptions) -> Self {
        Self {
            schema: SCHEMA,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            saved: SystemTime::now(),
            roots: roots.to_vec(),
            options: options.clone(),
            rules: Vec::new(),
            candidates: Vec::new(),
            issues: Vec::new(),
            summary: StoredSummary::new(&Summary {
                progress: Progress::default(),
                duration: Duration::ZERO,
                cancelled: false,
            }),
            caches: Vec::new(),
            toolchains: Vec::new(),
            docker: Vec::new(),
            docker_issues: Vec::new(),
            repos: Vec::new(),
            history: Err(String::new()),
            disks: Vec::new(),
        }
    }

    fn put(&mut self, loaded: &Loaded) {
        match loaded {
            Loaded::Found(_) => {}
            Loaded::Artifacts {
                candidates,
                issues,
                summary,
                disks,
            } => self.put_artifacts(candidates, issues, summary, disks),
            Loaded::Caches(caches) => self.put_caches(caches),
            Loaded::Toolchains(toolchains) => self.put_toolchains(toolchains),
            Loaded::Docker { entries, issues } => {
                self.docker = entries.iter().map(|(item, _)| item.clone()).collect();
                self.docker_issues.clone_from(issues);
            }
            Loaded::Repos(repos) => self.repos.clone_from(repos),
            Loaded::History(history) => self.put_history(history),
        }
    }

    fn put_artifacts(
        &mut self,
        candidates: &[Candidate],
        issues: &[Issue],
        summary: &Summary,
        disks: &[Disk],
    ) {
        self.candidates = candidates
            .iter()
            .map(|candidate| StoredCandidate::new(candidate, &mut self.rules))
            .collect();
        self.issues = issues.iter().map(StoredIssue::new).collect();
        self.summary = StoredSummary::new(summary);
        self.disks = disks
            .iter()
            .map(|disk| (disk.available, disk.total))
            .collect();
    }

    fn put_caches(&mut self, caches: &[(caches::KnownCache, Candidate)]) {
        self.caches = caches
            .iter()
            .map(|(cache, candidate)| {
                (
                    cache.name.to_owned(),
                    cache.path.clone(),
                    StoredCandidate::new(candidate, &mut self.rules),
                )
            })
            .collect();
    }

    fn put_toolchains(&mut self, toolchains: &[(Toolchain, Candidate)]) {
        self.toolchains = toolchains
            .iter()
            .map(|(toolchain, candidate)| {
                (
                    toolchain.clone(),
                    StoredCandidate::new(candidate, &mut self.rules),
                )
            })
            .collect();
    }

    fn put_history(&mut self, history: &Result<HistoryView, String>) {
        self.history = history
            .as_ref()
            .map(|view| StoredHistory {
                history: view.history.clone(),
                kept: view.kept.iter().copied().collect(),
                trash_known: view.trash_known,
            })
            .map_err(Clone::clone);
    }

    fn restore(self) -> Report {
        let rules: Vec<Arc<RuleInfo>> = self
            .rules
            .into_iter()
            .map(|rule| {
                Arc::new(RuleInfo {
                    name: rule.name,
                    category: rule.category,
                    safety: rule.safety,
                    ecosystem: rule.ecosystem,
                })
            })
            .collect();
        Report {
            roots: self.roots,
            options: self.options,
            candidates: self
                .candidates
                .into_iter()
                .filter_map(|candidate| candidate.restore(&rules))
                .collect(),
            issues: self.issues.into_iter().map(StoredIssue::restore).collect(),
            summary: self.summary.restore(),
            caches: self
                .caches
                .into_iter()
                .filter_map(|(name, path, candidate)| {
                    Some((caches::known(&name, path)?, candidate.restore(&rules)?))
                })
                .collect(),
            toolchains: self
                .toolchains
                .into_iter()
                .filter_map(|(toolchain, candidate)| Some((toolchain, candidate.restore(&rules)?)))
                .collect(),
            docker: docker::entries(self.docker),
            docker_issues: self.docker_issues,
            repos: self.repos,
            duplicates: None,
            history: self.history.map(|stored| HistoryView {
                history: stored.history,
                kept: stored.kept.into_iter().collect(),
                trash_known: stored.trash_known,
            }),
            disks: self
                .disks
                .into_iter()
                .map(|(available, total)| Disk { available, total })
                .collect(),
            scanned: self.saved,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredSidecar {
    schema: u32,
    version: String,
    roots: Vec<PathBuf>,
    min_size: u64,
    verify: bool,
    duplicates: StoredDuplicates,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct StoredRule {
    name: String,
    category: Category,
    safety: Safety,
    ecosystem: String,
}

#[derive(Serialize, Deserialize)]
struct StoredCandidate {
    id: u64,
    path: PathBuf,
    kind: Kind,
    rule: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project: Option<PathBuf>,
    usage: (u64, u64, u64, bool),
    #[serde(default, skip_serializing_if = "Option::is_none")]
    newest: Option<SystemTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity: Option<(u64, u64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_last_active: Option<SystemTime>,
}

impl StoredCandidate {
    fn new(candidate: &Candidate, rules: &mut Vec<StoredRule>) -> Self {
        let usage = candidate.usage;
        let info = &candidate.rule;
        let rule = rules
            .iter()
            .position(|rule| {
                rule.name == info.name
                    && rule.category == info.category
                    && rule.safety == info.safety
                    && rule.ecosystem == info.ecosystem
            })
            .unwrap_or_else(|| {
                rules.push(StoredRule {
                    name: info.name.clone(),
                    category: info.category,
                    safety: info.safety,
                    ecosystem: info.ecosystem.clone(),
                });
                rules.len() - 1
            });
        Self {
            id: candidate.id,
            path: candidate.path.clone(),
            kind: candidate.kind,
            rule,
            project: candidate.project.clone(),
            usage: (
                usage.allocated,
                usage.apparent,
                usage.entries,
                usage.partial,
            ),
            newest: candidate.newest,
            identity: candidate.identity,
            project_last_active: candidate.project_last_active,
        }
    }

    fn restore(self, rules: &[Arc<RuleInfo>]) -> Option<Candidate> {
        let (allocated, apparent, entries, partial) = self.usage;
        Some(Candidate {
            id: self.id,
            path: self.path,
            kind: self.kind,
            rule: Arc::clone(rules.get(self.rule)?),
            project: self.project,
            usage: Usage {
                allocated,
                apparent,
                entries,
                partial,
            },
            newest: self.newest,
            identity: self.identity,
            project_last_active: self.project_last_active,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct StoredIssue {
    path: PathBuf,
    kind: u8,
    message: String,
}

const ISSUE_KINDS: [IssueKind; 6] = [
    IssueKind::PermissionDenied,
    IssueKind::Vanished,
    IssueKind::Io,
    IssueKind::SkippedMount,
    IssueKind::InvalidRoot,
    IssueKind::Protected,
];

impl StoredIssue {
    fn new(issue: &Issue) -> Self {
        let kind = ISSUE_KINDS
            .iter()
            .position(|&kind| kind == issue.kind)
            .and_then(|index| u8::try_from(index).ok())
            .unwrap_or(2);
        Self {
            path: issue.path.clone(),
            kind,
            message: issue.message.clone(),
        }
    }

    fn restore(self) -> Issue {
        Issue {
            path: self.path,
            kind: ISSUE_KINDS
                .get(usize::from(self.kind))
                .copied()
                .unwrap_or(IssueKind::Io),
            message: self.message,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredSummary {
    progress: (u64, u64, u64, u64, u64),
    duration: Duration,
    cancelled: bool,
}

impl StoredSummary {
    fn new(summary: &Summary) -> Self {
        let progress = summary.progress;
        Self {
            progress: (
                progress.dirs,
                progress.files,
                progress.candidates,
                progress.bytes,
                progress.issues,
            ),
            duration: summary.duration,
            cancelled: summary.cancelled,
        }
    }

    fn restore(self) -> Summary {
        let (dirs, files, candidates, bytes, issues) = self.progress;
        Summary {
            progress: Progress {
                dirs,
                files,
                candidates,
                bytes,
                issues,
            },
            duration: self.duration,
            cancelled: self.cancelled,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredMember {
    path: PathBuf,
    links: Vec<PathBuf>,
    modified: Option<SystemTime>,
}

#[derive(Serialize, Deserialize)]
struct StoredGroup {
    size: u64,
    allocated: u64,
    hash: String,
    verified: bool,
    members: Vec<StoredMember>,
}

#[derive(Serialize, Deserialize)]
struct StoredDuplicates {
    groups: Vec<StoredGroup>,
    issues: Vec<StoredIssue>,
    files: u64,
    duration: Duration,
}

impl StoredDuplicates {
    fn new(duplicates: &Duplicates) -> Self {
        Self {
            groups: duplicates
                .groups
                .iter()
                .map(|group| StoredGroup {
                    size: group.size,
                    allocated: group.allocated,
                    hash: group.hash.clone(),
                    verified: group.verified,
                    members: group
                        .members
                        .iter()
                        .map(|member| StoredMember {
                            path: member.path.clone(),
                            links: member.links.clone(),
                            modified: member.modified,
                        })
                        .collect(),
                })
                .collect(),
            issues: duplicates.issues.iter().map(StoredIssue::new).collect(),
            files: duplicates.files,
            duration: duplicates.duration,
        }
    }

    fn restore(self) -> Duplicates {
        Duplicates {
            groups: self
                .groups
                .into_iter()
                .map(|group| Group {
                    size: group.size,
                    allocated: group.allocated,
                    hash: group.hash,
                    verified: group.verified,
                    members: group
                        .members
                        .into_iter()
                        .map(|member| Member {
                            path: member.path,
                            links: member.links,
                            modified: member.modified,
                        })
                        .collect(),
                })
                .collect(),
            issues: self.issues.into_iter().map(StoredIssue::restore).collect(),
            files: self.files,
            duration: self.duration,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredHistory {
    history: History,
    kept: Vec<(usize, usize)>,
    trash_known: bool,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use kuizer::config::Config;

    use super::*;

    fn candidate(id: u64, path: &str) -> Candidate {
        Candidate {
            id,
            path: PathBuf::from(path),
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: "node_modules".to_owned(),
                category: Category::Dependencies,
                safety: Safety::Review,
                ecosystem: "node".to_owned(),
            }),
            project: Some(PathBuf::from("/p/a")),
            usage: Usage {
                allocated: id * 4096,
                apparent: id * 4000,
                entries: id,
                partial: false,
            },
            newest: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
            identity: Some((7, id)),
            project_last_active: None,
        }
    }

    fn duplicates() -> Duplicates {
        Duplicates {
            groups: vec![Group {
                size: 5,
                allocated: 4096,
                hash: "abc".to_owned(),
                verified: true,
                members: vec![
                    Member {
                        path: PathBuf::from("/p/x"),
                        links: Vec::new(),
                        modified: None,
                    },
                    Member {
                        path: PathBuf::from("/p/y"),
                        links: vec![PathBuf::from("/p/z")],
                        modified: None,
                    },
                ],
            }],
            issues: Vec::new(),
            files: 9,
            duration: Duration::from_millis(20),
        }
    }

    fn report(root: &str) -> Report {
        let options = ScanOptions::from_config(&Config::default());
        Report {
            roots: vec![PathBuf::from(root)],
            options,
            candidates: vec![candidate(1, "/p/a/node_modules"), candidate(2, "/p/b/dist")],
            issues: vec![Issue {
                path: PathBuf::from("/p/locked"),
                kind: IssueKind::PermissionDenied,
                message: "denied".to_owned(),
            }],
            summary: Summary {
                progress: Progress {
                    dirs: 10,
                    files: 20,
                    candidates: 2,
                    bytes: 3,
                    issues: 1,
                },
                duration: Duration::from_millis(1500),
                cancelled: false,
            },
            caches: vec![(
                caches::known("npm", PathBuf::from("/h/.npm/_cacache")).unwrap(),
                candidate(1, "/h/.npm/_cacache"),
            )],
            toolchains: vec![(
                Toolchain {
                    tool: kuizer::toolchains::Tool::Nvm,
                    runtime: "node".to_owned(),
                    version: "v18.17.0".to_owned(),
                    path: PathBuf::from("/h/.nvm/versions/node/v18.17.0"),
                    default: false,
                    pins: vec![PathBuf::from("/p/a/.nvmrc")],
                },
                candidate(1, "/h/.nvm/versions/node/v18.17.0"),
            )],
            docker: docker::entries(vec![docker::Item {
                engine: "docker".to_owned(),
                program: "docker".to_owned(),
                object: docker::Object::Image,
                id: "abc".to_owned(),
                label: "<none> abc".to_owned(),
                bytes: 5,
                safety: Safety::Safe,
                note: String::new(),
            }]),
            docker_issues: vec!["podman is not running: down".to_owned()],
            repos: vec![Repository {
                root: PathBuf::from("/p/kearn"),
                git_dir: PathBuf::from("/p/kearn/.git"),
                objects: kuizer::repos::Objects {
                    loose: 900,
                    loose_bytes: 4 << 20,
                    ..Default::default()
                },
                default_branch: Some("main".to_owned()),
                current_branch: Some("main".to_owned()),
                merged: vec![kuizer::repos::Branch {
                    name: "done".to_owned(),
                    commit: "abc".to_owned(),
                }],
                busy: None,
            }],
            duplicates: Some(duplicates()),
            history: Ok(HistoryView {
                history: History::default(),
                kept: HashSet::from([(0, 1)]),
                trash_known: true,
            }),
            disks: vec![Disk {
                available: Some(100),
                total: Some(1000),
            }],
            scanned: SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
        }
    }

    fn save(snapshots: &Snapshots, root: &str) {
        let saved = report(root);
        let recorder = Recorder::new(&saved.roots, &saved.options);
        let scanned = saved.scanned;
        for part in saved.into_parts() {
            recorder.record(&part);
        }
        recorder.save(snapshots, scanned);
    }

    fn duplicate_options() -> dupes::Options {
        dupes::Options::from_config(&Config::default())
    }

    #[test]
    fn a_saved_scan_comes_back_unchanged_and_without_its_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let snapshots = Snapshots::new(Some(dir.path().join("scans")));
        let saved = report("/p");
        let recorder = Recorder::new(&saved.roots, &saved.options);
        for part in report("/p").into_parts() {
            recorder.record(&part);
        }
        recorder.save(&snapshots, saved.scanned);
        let loaded = snapshots
            .load(&saved.roots, &saved.options)
            .expect("the saved scan");
        assert_eq!(loaded.scanned, saved.scanned);
        assert_eq!(loaded.candidates.len(), 2);
        let first = &loaded.candidates[0];
        assert_eq!(first.path, PathBuf::from("/p/a/node_modules"));
        assert_eq!(first.usage, saved.candidates[0].usage);
        assert_eq!(first.identity, Some((7, 1)));
        assert_eq!(first.newest, saved.candidates[0].newest);
        assert_eq!(first.project, saved.candidates[0].project);
        assert_eq!(*first.rule, *saved.candidates[0].rule);
        assert_eq!(loaded.issues[0].kind, IssueKind::PermissionDenied);
        assert_eq!(loaded.summary, saved.summary);
        assert_eq!(loaded.caches[0].0, saved.caches[0].0);
        assert_eq!(loaded.toolchains[0].0, saved.toolchains[0].0);
        assert_eq!(loaded.docker[0].1.path, PathBuf::from("docker:image/abc"));
        assert_eq!(loaded.docker_issues, saved.docker_issues);
        assert_eq!(loaded.toolchains[0].1.path, saved.toolchains[0].1.path);
        assert_eq!(loaded.repos, saved.repos);
        assert!(loaded.duplicates.is_none());
        assert!(loaded.history.expect("history").kept.contains(&(0, 1)));
        assert_eq!(loaded.disks[0].total, Some(1000));
        assert!(
            snapshots
                .load_duplicates(&saved.roots, duplicate_options())
                .is_none()
        );

        snapshots
            .stage_duplicates(&saved.roots, duplicate_options(), &duplicates())
            .save();
        let found = snapshots
            .load_duplicates(&saved.roots, duplicate_options())
            .expect("the saved duplicates");
        assert_eq!(found.groups[0].members[1].links, [PathBuf::from("/p/z")]);
        assert_eq!(found.reclaimable(), 4096);
        let other = dupes::Options {
            min_size: 1,
            ..duplicate_options()
        };
        assert!(snapshots.load_duplicates(&saved.roots, other).is_none());
        assert_eq!(
            std::fs::read_dir(dir.path().join("scans")).unwrap().count(),
            2
        );
    }

    #[test]
    fn other_options_keep_only_what_still_applies() {
        let dir = tempfile::tempdir().unwrap();
        let snapshots = Snapshots::new(Some(dir.path().to_path_buf()));
        let saved = report("/p");
        save(&snapshots, "/p");
        assert!(
            snapshots
                .load(&[PathBuf::from("/q")], &saved.options)
                .is_none()
        );
        let mut options = saved.options.clone();
        options.caches = false;
        options.docker = false;
        options.min_size = 8192;
        let loaded = snapshots
            .load(&saved.roots, &options)
            .expect("the same folders");
        assert_eq!(loaded.options, options);
        assert!(loaded.caches.is_empty());
        assert!(loaded.docker.is_empty());
        assert_eq!(loaded.toolchains.len(), 1);
        assert_eq!(loaded.candidates.len(), 1);
        assert!(
            Snapshots::new(None)
                .load(&saved.roots, &saved.options)
                .is_none()
        );
    }

    #[test]
    fn another_version_or_a_broken_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let snapshots = Snapshots::new(Some(dir.path().to_path_buf()));
        let saved = report("/p");
        let file = snapshots.file(&saved.roots, EXTENSION).unwrap();
        for (field, value) in [
            ("schema", serde_json::json!(SCHEMA - 1)),
            ("version", serde_json::json!("0.0.0")),
        ] {
            save(&snapshots, "/p");
            let mut stored: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
            stored[field] = value;
            std::fs::write(&file, serde_json::to_vec(&stored).unwrap()).unwrap();
            assert!(snapshots.load(&saved.roots, &saved.options).is_none());
        }
        std::fs::write(&file, b"{ not json").unwrap();
        assert!(snapshots.load(&saved.roots, &saved.options).is_none());
        save(&snapshots, "/p");
        assert!(snapshots.load(&saved.roots, &saved.options).is_some());
    }

    #[test]
    fn only_the_newest_scans_within_the_budget_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let snapshots = Snapshots::new(Some(dir.path().to_path_buf()));
        for index in 0..LIMIT + 4 {
            save(&snapshots, &format!("/p{index}"));
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), LIMIT);
        let last = report(&format!("/p{}", LIMIT + 3));
        assert!(snapshots.load(&last.roots, &last.options).is_some());

        let huge = dir.path().join("huge.json");
        std::fs::File::create(&huge)
            .unwrap()
            .set_len(BUDGET)
            .unwrap();
        save(&snapshots, "/fresh");
        assert!(!huge.exists(), "a file past the size budget is removed");
        assert!(
            snapshots
                .load(&[PathBuf::from("/fresh")], &last.options)
                .is_some()
        );
    }

    #[test]
    fn the_age_reads_as_words() {
        let then = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let after = |seconds| then + Duration::from_secs(seconds);
        assert_eq!(age_label(then, after(5)), "From just now");
        assert_eq!(age_label(then, after(60)), "From 1 minute ago");
        assert_eq!(age_label(then, after(7_200)), "From 2 hours ago");
        assert_eq!(age_label(then, after(3 * 86_400)), "From 3 days ago");
        assert_eq!(age_label(after(10), then), "From just now");
    }
}
