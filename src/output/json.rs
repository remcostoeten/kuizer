use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::caches::KnownCache;
use crate::delete::{Mode, Removal, Status};
use crate::detect::{Category, Kind, Safety};
use crate::dupes::{Duplicates, Group};
use crate::scan::{Candidate, Issue, Summary};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct CandidateJson<'a> {
    id: u64,
    path: &'a Path,
    kind: Kind,
    rule: &'a str,
    category: Category,
    safety: Safety,
    ecosystem: &'a str,
    project: Option<&'a Path>,
    allocated_bytes: u64,
    apparent_bytes: u64,
    entries: u64,
    partial: bool,
    newest_modified: Option<u64>,
}

#[derive(Serialize)]
struct ScanReport<'a> {
    schema_version: u32,
    command: &'static str,
    roots: &'a [PathBuf],
    candidates: Vec<CandidateJson<'a>>,
    issues: &'a [Issue],
    summary: &'a Summary,
}

pub fn scan_report(
    roots: &[PathBuf],
    candidates: &[Candidate],
    issues: &[Issue],
    summary: &Summary,
) -> String {
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
            })
            .collect(),
        issues,
        summary,
    };
    serde_json::to_string_pretty(&report).unwrap_or_default()
}

#[derive(Serialize)]
struct RemovalJson<'a> {
    path: &'a Path,
    kind: Kind,
    rule: &'a str,
    category: Category,
    safety: Safety,
    ecosystem: &'a str,
    allocated_bytes: u64,
    status: Status,
    error: Option<&'a str>,
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
    roots: &'a [PathBuf],
    items: Vec<RemovalJson<'a>>,
    issues: &'a [Issue],
    summary: RemovalSummary,
}

pub fn removal_report(
    command: &str,
    mode: Mode,
    roots: &[PathBuf],
    removals: &[Removal],
    issues: &[Issue],
) -> String {
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
                    status: removal.status,
                    error: removal.error.as_deref(),
                }
            })
            .collect(),
        issues,
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
    serde_json::to_string_pretty(&report).unwrap_or_default()
}

fn seconds(time: Option<SystemTime>) -> Option<u64> {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
}

#[derive(Serialize)]
struct MemberJson<'a> {
    path: &'a Path,
    links: &'a [PathBuf],
    modified: Option<u64>,
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
    roots: &'a [PathBuf],
    groups: Vec<GroupJson<'a>>,
    issues: &'a [Issue],
    summary: DuplicatesSummary,
}

pub fn duplicates_report(roots: &[PathBuf], found: &Duplicates, groups: &[Group]) -> String {
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
                    })
                    .collect(),
            })
            .collect(),
        issues: &found.issues,
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
    serde_json::to_string_pretty(&report).unwrap_or_default()
}

#[derive(Serialize)]
struct CacheJson<'a> {
    name: &'a str,
    tool: &'a str,
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

pub fn cache_report(entries: &[(KnownCache, Candidate)]) -> String {
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
    serde_json::to_string_pretty(&report).unwrap_or_default()
}
