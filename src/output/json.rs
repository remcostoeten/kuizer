use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Serialize, Serializer};

use crate::caches::KnownCache;
use crate::delete::{Mode, Removal, Status};
use crate::detect::{Category, Kind, Safety};
use crate::docker::{Item, Object};
use crate::dupes::{Duplicates, Group};
use crate::scan::{Candidate, Issue, IssueKind, Summary};
use crate::toolchains::Toolchain;

pub const SCHEMA_VERSION: u32 = 1;

pub(super) fn lossy_path<S: Serializer>(path: &&Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&path.to_string_lossy())
}

pub(super) fn lossy_opt_path<S: Serializer>(
    path: &Option<&Path>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match path {
        Some(path) => serializer.serialize_str(&path.to_string_lossy()),
        None => serializer.serialize_none(),
    }
}

pub(super) fn lossy_paths<S: Serializer>(
    paths: &&[PathBuf],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(paths.iter().map(|path| path.to_string_lossy()))
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn lossy(path: &Path) -> bool {
    path.to_str().is_none()
}

#[derive(Serialize)]
struct IssueJson<'a> {
    #[serde(serialize_with = "lossy_path")]
    path: &'a Path,
    kind: IssueKind,
    message: &'a str,
}

fn issues_json(issues: &[Issue]) -> Vec<IssueJson<'_>> {
    issues
        .iter()
        .map(|issue| IssueJson {
            path: &issue.path,
            kind: issue.kind,
            message: &issue.message,
        })
        .collect()
}

#[derive(Serialize)]
struct CandidateJson<'a> {
    id: u64,
    #[serde(serialize_with = "lossy_path")]
    path: &'a Path,
    kind: Kind,
    rule: &'a str,
    category: Category,
    safety: Safety,
    ecosystem: &'a str,
    #[serde(serialize_with = "lossy_opt_path")]
    project: Option<&'a Path>,
    allocated_bytes: u64,
    apparent_bytes: u64,
    entries: u64,
    partial: bool,
    newest_modified: Option<u64>,
    project_last_active: Option<u64>,
    #[serde(skip_serializing_if = "is_false")]
    path_lossy: bool,
}

#[derive(Serialize)]
struct ScanReport<'a> {
    schema_version: u32,
    command: &'static str,
    #[serde(serialize_with = "lossy_paths")]
    roots: &'a [PathBuf],
    candidates: Vec<CandidateJson<'a>>,
    issues: Vec<IssueJson<'a>>,
    summary: &'a Summary,
}

pub fn scan_report(
    roots: &[PathBuf],
    candidates: &[Candidate],
    issues: &[Issue],
    summary: &Summary,
) -> Result<String, serde_json::Error> {
    let report = ScanReport {
        schema_version: SCHEMA_VERSION,
        command: "scan",
        roots,
        candidates: candidates
            .iter()
            .map(|candidate| CandidateJson {
                id: candidate.id,
                path: &candidate.path,
                kind: candidate.kind,
                rule: &candidate.rule.name,
                category: candidate.rule.category,
                safety: candidate.rule.safety,
                ecosystem: &candidate.rule.ecosystem,
                project: candidate.project.as_deref(),
                allocated_bytes: candidate.usage.allocated,
                apparent_bytes: candidate.usage.apparent,
                entries: candidate.usage.entries,
                partial: candidate.usage.partial,
                newest_modified: seconds(candidate.newest),
                project_last_active: seconds(candidate.project_last_active),
                path_lossy: lossy(&candidate.path)
                    || candidate.project.as_deref().is_some_and(lossy),
            })
            .collect(),
        issues: issues_json(issues),
        summary,
    };
    serde_json::to_string_pretty(&report)
}

#[derive(Serialize)]
struct RemovalJson<'a> {
    #[serde(serialize_with = "lossy_path")]
    path: &'a Path,
    kind: Kind,
    rule: &'a str,
    category: Category,
    safety: Safety,
    ecosystem: &'a str,
    allocated_bytes: u64,
    project_last_active: Option<u64>,
    status: Status,
    error: Option<&'a str>,
    #[serde(skip_serializing_if = "is_false")]
    path_lossy: bool,
}

#[derive(Serialize)]
struct RemovalSummary {
    selected: usize,
    selected_bytes: u64,
    removed: usize,
    removed_bytes: u64,
    failed: usize,
}

#[derive(Serialize)]
struct RemovalReport<'a> {
    schema_version: u32,
    command: &'a str,
    mode: Mode,
    #[serde(serialize_with = "lossy_paths")]
    roots: &'a [PathBuf],
    items: Vec<RemovalJson<'a>>,
    issues: Vec<IssueJson<'a>>,
    summary: RemovalSummary,
}

pub fn removal_report(
    command: &str,
    mode: Mode,
    roots: &[PathBuf],
    removals: &[Removal],
    issues: &[Issue],
) -> Result<String, serde_json::Error> {
    let removed: Vec<&Removal> = removals
        .iter()
        .filter(|removal| removal.status == Status::Removed)
        .collect();
    let report = RemovalReport {
        schema_version: SCHEMA_VERSION,
        command,
        mode,
        roots,
        items: removals
            .iter()
            .map(|removal| {
                let candidate = &removal.candidate;
                RemovalJson {
                    path: &candidate.path,
                    kind: candidate.kind,
                    rule: &candidate.rule.name,
                    category: candidate.rule.category,
                    safety: candidate.rule.safety,
                    ecosystem: &candidate.rule.ecosystem,
                    allocated_bytes: candidate.usage.allocated,
                    project_last_active: seconds(candidate.project_last_active),
                    status: removal.status,
                    error: removal.error.as_deref(),
                    path_lossy: lossy(&candidate.path),
                }
            })
            .collect(),
        issues: issues_json(issues),
        summary: RemovalSummary {
            selected: removals.len(),
            selected_bytes: removals
                .iter()
                .map(|removal| removal.candidate.usage.allocated)
                .sum(),
            removed: removed.len(),
            removed_bytes: removed
                .iter()
                .map(|removal| removal.candidate.usage.allocated)
                .sum(),
            failed: removals
                .iter()
                .filter(|removal| removal.status == Status::Failed)
                .count(),
        },
    };
    serde_json::to_string_pretty(&report)
}

fn seconds(time: Option<SystemTime>) -> Option<u64> {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
}

#[derive(Serialize)]
struct MemberJson<'a> {
    #[serde(serialize_with = "lossy_path")]
    path: &'a Path,
    #[serde(serialize_with = "lossy_paths")]
    links: &'a [PathBuf],
    modified: Option<u64>,
    #[serde(skip_serializing_if = "is_false")]
    path_lossy: bool,
}

#[derive(Serialize)]
struct GroupJson<'a> {
    size: u64,
    allocated_bytes: u64,
    hash: &'a str,
    verified: bool,
    reclaimable_bytes: u64,
    members: Vec<MemberJson<'a>>,
}

#[derive(Serialize)]
struct DuplicatesSummary {
    files: u64,
    groups: usize,
    duplicates: usize,
    reclaimable_bytes: u64,
    duration: u128,
}

#[derive(Serialize)]
struct DuplicatesReport<'a> {
    schema_version: u32,
    command: &'static str,
    #[serde(serialize_with = "lossy_paths")]
    roots: &'a [PathBuf],
    groups: Vec<GroupJson<'a>>,
    issues: Vec<IssueJson<'a>>,
    summary: DuplicatesSummary,
}

pub fn duplicates_report(
    roots: &[PathBuf],
    found: &Duplicates,
    groups: &[Group],
) -> Result<String, serde_json::Error> {
    let report = DuplicatesReport {
        schema_version: SCHEMA_VERSION,
        command: "duplicates",
        roots,
        groups: groups
            .iter()
            .map(|group| GroupJson {
                size: group.size,
                allocated_bytes: group.allocated,
                hash: &group.hash,
                verified: group.verified,
                reclaimable_bytes: group.reclaimable(),
                members: group
                    .members
                    .iter()
                    .map(|member| MemberJson {
                        path: &member.path,
                        links: &member.links,
                        modified: seconds(member.modified),
                        path_lossy: lossy(&member.path),
                    })
                    .collect(),
            })
            .collect(),
        issues: issues_json(&found.issues),
        summary: DuplicatesSummary {
            files: found.files,
            groups: found.groups.len(),
            duplicates: found
                .groups
                .iter()
                .map(|group| group.members.len() - 1)
                .sum(),
            reclaimable_bytes: found.reclaimable(),
            duration: found.duration.as_millis(),
        },
    };
    serde_json::to_string_pretty(&report)
}

#[derive(Serialize)]
struct CacheJson<'a> {
    name: &'a str,
    tool: &'a str,
    #[serde(serialize_with = "lossy_path")]
    path: &'a Path,
    safety: Safety,
    note: Option<&'a str>,
    allocated_bytes: u64,
    apparent_bytes: u64,
    entries: u64,
    partial: bool,
    newest_modified: Option<u64>,
}

#[derive(Serialize)]
struct CacheSummary {
    items: usize,
    bytes: u64,
    safe_bytes: u64,
}

#[derive(Serialize)]
struct CacheReport<'a> {
    schema_version: u32,
    command: &'static str,
    caches: Vec<CacheJson<'a>>,
    summary: CacheSummary,
}

pub fn cache_report(entries: &[(KnownCache, Candidate)]) -> Result<String, serde_json::Error> {
    let report = CacheReport {
        schema_version: SCHEMA_VERSION,
        command: "cache",
        caches: entries
            .iter()
            .map(|(cache, candidate)| CacheJson {
                name: cache.name,
                tool: cache.tool,
                path: &cache.path,
                safety: cache.safety,
                note: (!cache.note.is_empty()).then_some(cache.note),
                allocated_bytes: candidate.usage.allocated,
                apparent_bytes: candidate.usage.apparent,
                entries: candidate.usage.entries,
                partial: candidate.usage.partial,
                newest_modified: seconds(candidate.newest),
            })
            .collect(),
        summary: CacheSummary {
            items: entries.len(),
            bytes: entries
                .iter()
                .map(|(_, candidate)| candidate.usage.allocated)
                .sum(),
            safe_bytes: entries
                .iter()
                .filter(|(cache, _)| cache.safety == Safety::Safe)
                .map(|(_, candidate)| candidate.usage.allocated)
                .sum(),
        },
    };
    serde_json::to_string_pretty(&report)
}

#[derive(Serialize)]
struct ToolchainJson<'a> {
    tool: &'static str,
    runtime: &'a str,
    version: &'a str,
    #[serde(serialize_with = "lossy_path")]
    path: &'a Path,
    safety: Safety,
    default: bool,
    #[serde(serialize_with = "lossy_paths")]
    pinned_by: &'a [PathBuf],
    allocated_bytes: u64,
    apparent_bytes: u64,
    entries: u64,
    partial: bool,
}

#[derive(Serialize)]
struct ToolchainReport<'a> {
    schema_version: u32,
    command: &'static str,
    toolchains: Vec<ToolchainJson<'a>>,
    summary: CacheSummary,
}

pub fn toolchain_report(entries: &[(Toolchain, Candidate)]) -> Result<String, serde_json::Error> {
    let report = ToolchainReport {
        schema_version: SCHEMA_VERSION,
        command: "toolchain",
        toolchains: entries
            .iter()
            .map(|(toolchain, candidate)| ToolchainJson {
                tool: toolchain.tool.name(),
                runtime: &toolchain.runtime,
                version: &toolchain.version,
                path: &toolchain.path,
                safety: toolchain.safety(),
                default: toolchain.default,
                pinned_by: &toolchain.pins,
                allocated_bytes: candidate.usage.allocated,
                apparent_bytes: candidate.usage.apparent,
                entries: candidate.usage.entries,
                partial: candidate.usage.partial,
            })
            .collect(),
        summary: CacheSummary {
            items: entries.len(),
            bytes: entries
                .iter()
                .map(|(_, candidate)| candidate.usage.allocated)
                .sum(),
            safe_bytes: entries
                .iter()
                .filter(|(toolchain, _)| toolchain.safety() == Safety::Safe)
                .map(|(_, candidate)| candidate.usage.allocated)
                .sum(),
        },
    };
    serde_json::to_string_pretty(&report)
}

#[derive(Serialize)]
struct DockerJson<'a> {
    engine: &'a str,
    object: Object,
    id: &'a str,
    name: &'a str,
    #[serde(serialize_with = "lossy_path")]
    path: &'a Path,
    safety: Safety,
    note: Option<&'a str>,
    allocated_bytes: u64,
}

#[derive(Serialize)]
struct DockerReport<'a> {
    schema_version: u32,
    command: &'static str,
    objects: Vec<DockerJson<'a>>,
    issues: &'a [String],
    summary: CacheSummary,
}

pub fn docker_report(
    entries: &[(Item, Candidate)],
    issues: &[String],
) -> Result<String, serde_json::Error> {
    let report = DockerReport {
        schema_version: SCHEMA_VERSION,
        command: "docker",
        objects: entries
            .iter()
            .map(|(item, candidate)| DockerJson {
                engine: &item.engine,
                object: item.object,
                id: &item.id,
                name: &item.label,
                path: &candidate.path,
                safety: item.safety,
                note: (!item.note.is_empty()).then_some(item.note.as_str()),
                allocated_bytes: candidate.usage.allocated,
            })
            .collect(),
        issues,
        summary: CacheSummary {
            items: entries.len(),
            bytes: entries
                .iter()
                .map(|(_, candidate)| candidate.usage.allocated)
                .sum(),
            safe_bytes: entries
                .iter()
                .filter(|(item, _)| item.safety == Safety::Safe)
                .map(|(_, candidate)| candidate.usage.allocated)
                .sum(),
        },
    };
    serde_json::to_string_pretty(&report)
}

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::detect::RuleInfo;
    use crate::scan::{Progress, Usage};

    fn candidate(path: PathBuf) -> Candidate {
        Candidate {
            id: 1,
            path,
            kind: Kind::Dir,
            rule: Arc::new(RuleInfo {
                name: "node-modules".to_owned(),
                category: Category::Dependencies,
                safety: Safety::Safe,
                ecosystem: "node".to_owned(),
            }),
            project: None,
            usage: Usage::default(),
            newest: None,
            identity: None,
            project_last_active: None,
        }
    }

    #[test]
    fn paths_that_are_not_utf8_still_produce_a_report() {
        let odd = PathBuf::from(OsStr::from_bytes(b"/p/proj\xff/node_modules"));
        let fine = PathBuf::from("/p/ok/node_modules");
        let issue = Issue {
            path: odd.clone(),
            kind: IssueKind::Io,
            message: "unreadable".to_owned(),
        };
        let summary = Summary {
            progress: Progress::default(),
            duration: Duration::ZERO,
            cancelled: false,
        };
        let candidates = [candidate(odd.clone()), candidate(fine)];
        let text = scan_report(
            std::slice::from_ref(&odd),
            &candidates,
            std::slice::from_ref(&issue),
            &summary,
        )
        .expect("serializes");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(value["candidates"][0]["path_lossy"], true);
        assert!(
            value["candidates"][0]["path"]
                .as_str()
                .unwrap()
                .contains('\u{fffd}')
        );
        assert!(value["candidates"][1].get("path_lossy").is_none());
        assert!(
            value["issues"][0]["path"]
                .as_str()
                .unwrap()
                .contains('\u{fffd}')
        );
        assert!(value["roots"][0].as_str().unwrap().contains('\u{fffd}'));

        let removal = Removal {
            candidate: candidates[0].clone(),
            status: Status::Removed,
            error: None,
        };
        let text =
            removal_report("clean", Mode::Trash, &[odd], &[removal], &[issue]).expect("serializes");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(value["items"][0]["path_lossy"], true);
    }
}
