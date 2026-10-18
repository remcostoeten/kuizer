use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use kuizer::caches;
use kuizer::detect::{Category, Kind, RuleInfo, Safety};
use kuizer::dupes::{Duplicates, Group, Member};
use kuizer::history::History;
use kuizer::repos::Repository;
use kuizer::scan::{Candidate, Issue, IssueKind, Progress, Summary, Usage};
use serde::{Deserialize, Serialize};

use crate::model::{Disk, HistoryView, Report};
use crate::start::ScanOptions;

const SCHEMA: u32 = 1;
const LIMIT: usize = 8;
const EXTENSION: &str = "json";

/// @name Snapshots
/// @description The last finished scan for each set of folders and options, one JSON file per
/// pair in the GUI state folder, so the window can show it at once while a fresh scan runs.
/// Keeps the newest eight and ignores files written by another schema or kuizer version.
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

    pub fn load(&self, roots: &[PathBuf], options: &ScanOptions) -> Option<Report> {
        let bytes = std::fs::read(self.file(roots, options)?).ok()?;
        let stored: Stored = serde_json::from_slice(&bytes).ok()?;
        (stored.schema == SCHEMA
            && stored.version == env!("CARGO_PKG_VERSION")
            && stored.roots == roots
            && stored.options == *options)
            .then(|| stored.restore())
    }

    pub fn save(&self, report: &Report) {
        let (Some(dir), Some(file)) = (&self.dir, self.file(&report.roots, &report.options)) else {
            return;
        };
        if let Err(error) = write(dir, &file, &Stored::new(report)) {
            eprintln!("kuizer-gui: cannot save the scan for next time: {error}");
            return;
        }
        prune(dir, &file);
    }

    fn file(&self, roots: &[PathBuf], options: &ScanOptions) -> Option<PathBuf> {
        let key = serde_json::to_vec(&(roots, options)).ok()?;
        let dir = self.dir.as_ref()?;
        Some(dir.join(format!("{:016x}.{EXTENSION}", fnv(&key))))
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn write(dir: &Path, file: &Path, stored: &Stored) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let bytes = serde_json::to_vec(stored)?;
    let temp = file.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

fn prune(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut others: Vec<(SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path != keep && path.extension().is_some_and(|ext| ext == EXTENSION))
        .map(|path| {
            let modified = std::fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (modified, path)
        })
        .collect();
    others.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, path) in others.into_iter().skip(LIMIT - 1) {
        let _ = std::fs::remove_file(path);
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
    candidates: Vec<StoredCandidate>,
    issues: Vec<StoredIssue>,
    summary: StoredSummary,
    caches: Vec<(String, PathBuf, StoredCandidate)>,
    #[serde(default)]
    repos: Vec<Repository>,
    duplicates: Option<StoredDuplicates>,
    history: Result<StoredHistory, String>,
    disks: Vec<(Option<u64>, Option<u64>)>,
}

impl Stored {
    fn new(report: &Report) -> Self {
        Self {
            schema: SCHEMA,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            saved: report.scanned,
            roots: report.roots.clone(),
            options: report.options.clone(),
            candidates: report.candidates.iter().map(StoredCandidate::new).collect(),
            issues: report.issues.iter().map(StoredIssue::new).collect(),
            summary: StoredSummary::new(&report.summary),
            caches: report
                .caches
                .iter()
                .map(|(cache, candidate)| {
                    (
                        cache.name.to_owned(),
                        cache.path.clone(),
                        StoredCandidate::new(candidate),
                    )
                })
                .collect(),
            repos: report.repos.clone(),
            duplicates: report.duplicates.as_ref().map(StoredDuplicates::new),
            history: report
                .history
                .as_ref()
                .map(|view| StoredHistory {
                    history: view.history.clone(),
                    kept: view.kept.iter().copied().collect(),
                    trash_known: view.trash_known,
                })
                .map_err(Clone::clone),
            disks: report
                .disks
                .iter()
                .map(|disk| (disk.available, disk.total))
                .collect(),
        }
    }

    fn restore(self) -> Report {
        Report {
            roots: self.roots,
            options: self.options,
            candidates: self
                .candidates
                .into_iter()
                .map(StoredCandidate::restore)
                .collect(),
            issues: self.issues.into_iter().map(StoredIssue::restore).collect(),
            summary: self.summary.restore(),
            caches: self
                .caches
                .into_iter()
                .filter_map(|(name, path, candidate)| {
                    Some((caches::known(&name, path)?, candidate.restore()))
                })
                .collect(),
            repos: self.repos,
            duplicates: self.duplicates.map(StoredDuplicates::restore),
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
    rule: StoredRule,
    project: Option<PathBuf>,
    usage: (u64, u64, u64, bool),
    newest: Option<SystemTime>,
    identity: Option<(u64, u64)>,
    project_last_active: Option<SystemTime>,
}

impl StoredCandidate {
    fn new(candidate: &Candidate) -> Self {
        let usage = candidate.usage;
        Self {
            id: candidate.id,
            path: candidate.path.clone(),
            kind: candidate.kind,
            rule: StoredRule {
                name: candidate.rule.name.clone(),
                category: candidate.rule.category,
                safety: candidate.rule.safety,
                ecosystem: candidate.rule.ecosystem.clone(),
            },
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

    fn restore(self) -> Candidate {
        let (allocated, apparent, entries, partial) = self.usage;
        Candidate {
            id: self.id,
            path: self.path,
            kind: self.kind,
            rule: Arc::new(RuleInfo {
                name: self.rule.name,
                category: self.rule.category,
                safety: self.rule.safety,
                ecosystem: self.rule.ecosystem,
            }),
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
        }
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
            duplicates: Some(Duplicates {
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
            }),
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

    #[test]
    fn a_saved_scan_comes_back_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let snapshots = Snapshots::new(Some(dir.path().join("scans")));
        let saved = report("/p");
        snapshots.save(&saved);
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
        assert_eq!(*first.rule, *saved.candidates[0].rule);
        assert_eq!(loaded.issues[0].kind, IssueKind::PermissionDenied);
        assert_eq!(loaded.summary, saved.summary);
        assert_eq!(loaded.caches[0].0, saved.caches[0].0);
        assert_eq!(loaded.repos, saved.repos);
        let duplicates = loaded.duplicates.expect("duplicates");
        assert_eq!(
            duplicates.groups[0].members[1].links,
            [PathBuf::from("/p/z")]
        );
        assert_eq!(duplicates.reclaimable(), 4096);
        let history = loaded.history.expect("history");
        assert!(history.kept.contains(&(0, 1)));
        assert_eq!(loaded.disks[0].total, Some(1000));
        assert_eq!(
            std::fs::read_dir(dir.path().join("scans")).unwrap().count(),
            1
        );
    }

    #[test]
    fn other_roots_or_options_do_not_match() {
        let dir = tempfile::tempdir().unwrap();
        let snapshots = Snapshots::new(Some(dir.path().to_path_buf()));
        let saved = report("/p");
        snapshots.save(&saved);
        assert!(
            snapshots
                .load(&[PathBuf::from("/q")], &saved.options)
                .is_none()
        );
        let mut options = saved.options.clone();
        options.duplicates = false;
        assert!(snapshots.load(&saved.roots, &options).is_none());
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
        let file = snapshots.file(&saved.roots, &saved.options).unwrap();
        for (field, value) in [
            ("schema", serde_json::json!(SCHEMA + 1)),
            ("version", serde_json::json!("0.0.0")),
        ] {
            snapshots.save(&saved);
            let mut stored: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
            stored[field] = value;
            std::fs::write(&file, serde_json::to_vec(&stored).unwrap()).unwrap();
            assert!(snapshots.load(&saved.roots, &saved.options).is_none());
        }
        std::fs::write(&file, b"{ not json").unwrap();
        assert!(snapshots.load(&saved.roots, &saved.options).is_none());
        snapshots.save(&saved);
        assert!(snapshots.load(&saved.roots, &saved.options).is_some());
    }

    #[test]
    fn only_the_newest_scans_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let snapshots = Snapshots::new(Some(dir.path().to_path_buf()));
        for index in 0..LIMIT + 4 {
            snapshots.save(&report(&format!("/p{index}")));
        }
        let files = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(files, LIMIT);
        let last = report(&format!("/p{}", LIMIT + 3));
        assert!(snapshots.load(&last.roots, &last.options).is_some());
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
