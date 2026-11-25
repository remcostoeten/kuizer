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

pub const SCHEMA_VERSION: u32 = 2;

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

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Source {
    Projects,
    Cache,
    Toolchain,
    Docker,
}

impl Source {
    fn command(self) -> &'static str {
        match self {
            Self::Projects => "scan",
            Self::Cache => "cache",
            Self::Toolchain => "toolchain",
            Self::Docker => "docker",
        }
    }

    fn removal(command: &str) -> Self {
        match command {
            "cache" => Self::Cache,
            "toolchain" => Self::Toolchain,
            "docker" => Self::Docker,
            _ => Self::Projects,
        }
    }
}

#[derive(Serialize)]
struct CandidateJson<'a> {
    id: u64,
    source: Source,
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

impl<'a> CandidateJson<'a> {
    fn new(source: Source, candidate: &'a Candidate) -> Self {
        Self {
            id: candidate.id,
            source,
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
            path_lossy: lossy(&candidate.path) || candidate.project.as_deref().is_some_and(lossy),
        }
    }
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
            .map(|candidate| CandidateJson::new(Source::Projects, candidate))
            .collect(),
        issues: issues_json(issues),
        summary,
    };
    serde_json::to_string_pretty(&report)
}

#[derive(Serialize)]
struct Line<Row: Serialize> {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(flatten)]
    row: Row,
}

fn candidate_line<Row: Serialize>(row: Row) -> Result<String, serde_json::Error> {
    serde_json::to_string(&Line {
        kind: "candidate",
        row,
    })
}

fn summary_line<Row: Serialize>(row: Row) -> Result<String, serde_json::Error> {
    serde_json::to_string(&Line {
        kind: "summary",
        row,
    })
}

/// @name scan_line
/// @description Serializes one project candidate as a single `--json-lines` line with
/// `"type": "candidate"` and the fields of a `scan` report row.
///
/// @example
/// println!("{}", output::scan_line(&candidate)?);
pub fn scan_line(candidate: &Candidate) -> Result<String, serde_json::Error> {
    candidate_line(CandidateJson::new(Source::Projects, candidate))
}

#[derive(Serialize)]
struct ScanSummary<'a> {
    schema_version: u32,
    command: &'static str,
    #[serde(serialize_with = "lossy_paths")]
    roots: &'a [PathBuf],
    issues: Vec<IssueJson<'a>>,
    summary: &'a Summary,
}

/// @name scan_summary_line
/// @description Serializes the closing `--json-lines` line of a scan: `"type": "summary"` with
/// every `scan` report field except `candidates`.
///
/// @example
/// println!("{}", output::scan_summary_line(&roots, &issues, &summary)?);
pub fn scan_summary_line(
    roots: &[PathBuf],
    issues: &[Issue],
    summary: &Summary,
) -> Result<String, serde_json::Error> {
    summary_line(ScanSummary {
        schema_version: SCHEMA_VERSION,
        command: "scan",
        roots,
        issues: issues_json(issues),
        summary,
    })
}

#[derive(Serialize)]
struct RemovalJson<'a> {
    #[serde(flatten)]
    candidate: CandidateJson<'a>,
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
    #[serde(serialize_with = "lossy_paths")]
    roots: &'a [PathBuf],
    #[serde(skip_serializing_if = "Option::is_none")]
    items: Option<Vec<RemovalJson<'a>>>,
    issues: Vec<IssueJson<'a>>,
    summary: RemovalSummary,
}

impl<'a> RemovalJson<'a> {
    fn new(command: &str, removal: &'a Removal) -> Self {
        Self {
            candidate: CandidateJson::new(Source::removal(command), &removal.candidate),
            status: removal.status,
            error: removal.error.as_deref(),
        }
    }
}

impl RemovalSummary {
    fn new(removals: &[Removal]) -> Self {
        let mut summary = Self {
            selected: removals.len(),
            selected_bytes: 0,
            removed: 0,
            removed_bytes: 0,
            failed: 0,
        };
        for removal in removals {
            let bytes = removal.candidate.usage.allocated;
            summary.selected_bytes += bytes;
            match removal.status {
                Status::Removed => {
                    summary.removed += 1;
                    summary.removed_bytes += bytes;
                }
                Status::Failed => summary.failed += 1,
                Status::WouldRemove => {}
            }
        }
        summary
    }
}

pub fn removal_report(
    command: &str,
    mode: Mode,
    roots: &[PathBuf],
    removals: &[Removal],
    issues: &[Issue],
) -> Result<String, serde_json::Error> {
    let report = RemovalReport {
        schema_version: SCHEMA_VERSION,
        command,
        mode,
        roots,
        items: Some(
            removals
                .iter()
                .map(|removal| RemovalJson::new(command, removal))
                .collect(),
        ),
        issues: issues_json(issues),
        summary: RemovalSummary::new(removals),
    };
    serde_json::to_string_pretty(&report)
}

/// @name removal_line
/// @description Serializes one finished removal as a single `--json-lines` line with
/// `"type": "removal"` and the fields of a removal report item, including its status.
///
/// @example
/// println!("{}", output::removal_line("clean", &removal)?);
pub fn removal_line(command: &str, removal: &Removal) -> Result<String, serde_json::Error> {
    serde_json::to_string(&Line {
        kind: "removal",
        row: RemovalJson::new(command, removal),
    })
}

/// @name removal_summary_line
/// @description Serializes the closing `--json-lines` line of a removal: `"type": "summary"`
/// with every removal report field except `items`.
///
/// @example
/// println!("{}", output::removal_summary_line("clean", mode, &roots, &removals, &issues)?);
pub fn removal_summary_line(
    command: &str,
    mode: Mode,
    roots: &[PathBuf],
    removals: &[Removal],
    issues: &[Issue],
) -> Result<String, serde_json::Error> {
    summary_line(RemovalReport {
        schema_version: SCHEMA_VERSION,
        command,
        mode,
        roots,
        items: None,
        issues: issues_json(issues),
        summary: RemovalSummary::new(removals),
    })
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
struct CandidateDetails<'a, Details: Serialize> {
    #[serde(flatten)]
    candidate: CandidateJson<'a>,
    #[serde(flatten)]
    details: Details,
}

#[derive(Serialize)]
struct ListingSummary {
    items: usize,
    bytes: u64,
    safe_bytes: u64,
}

#[derive(Serialize)]
struct ListingReport<'a, Details: Serialize> {
    schema_version: u32,
    command: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidates: Option<Vec<CandidateDetails<'a, Details>>>,
    issues: Vec<IssueJson<'a>>,
    summary: ListingSummary,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Document,
    Lines,
}

fn listing_report<'a, Item, Details: Serialize>(
    source: Source,
    entries: &'a [(Item, Candidate)],
    issues: Vec<IssueJson<'a>>,
    details: impl Fn(&'a Item) -> Details,
    shape: Shape,
) -> Result<String, serde_json::Error> {
    let mut summary = ListingSummary {
        items: entries.len(),
        bytes: 0,
        safe_bytes: 0,
    };
    let candidates: Vec<CandidateDetails<'a, Details>> = entries
        .iter()
        .map(|(item, candidate)| {
            summary.bytes += candidate.usage.allocated;
            if candidate.rule.safety == Safety::Safe {
                summary.safe_bytes += candidate.usage.allocated;
            }
            CandidateDetails {
                candidate: CandidateJson::new(source, candidate),
                details: details(item),
            }
        })
        .collect();
    let mut report = ListingReport {
        schema_version: SCHEMA_VERSION,
        command: source.command(),
        candidates: None,
        issues,
        summary,
    };
    if shape == Shape::Document {
        report.candidates = Some(candidates);
        return serde_json::to_string_pretty(&report);
    }
    let mut lines = candidates
        .into_iter()
        .map(candidate_line)
        .collect::<Result<Vec<_>, _>>()?;
    lines.push(summary_line(report)?);
    Ok(lines.join("\n"))
}

#[derive(Serialize)]
struct CacheJson {
    name: &'static str,
    tool: &'static str,
    note: Option<&'static str>,
}

/// @name cache_report
/// @description Serializes global cache candidates and their allocated byte totals.
///
/// @example
/// let report = cache_report(&entries)?;
pub fn cache_report(entries: &[(KnownCache, Candidate)]) -> Result<String, serde_json::Error> {
    listing_report(
        Source::Cache,
        entries,
        Vec::new(),
        cache_json,
        Shape::Document,
    )
}

/// @name cache_lines
/// @description Serializes global cache candidates as `--json-lines`: one candidate per line,
/// then a summary line.
///
/// @example
/// println!("{}", output::cache_lines(&entries)?);
pub fn cache_lines(entries: &[(KnownCache, Candidate)]) -> Result<String, serde_json::Error> {
    listing_report(Source::Cache, entries, Vec::new(), cache_json, Shape::Lines)
}

fn cache_json(cache: &KnownCache) -> CacheJson {
    CacheJson {
        name: cache.name,
        tool: cache.tool,
        note: (!cache.note.is_empty()).then_some(cache.note),
    }
}

#[derive(Serialize)]
struct ToolchainJson<'a> {
    tool: &'static str,
    runtime: &'a str,
    version: &'a str,
    default: bool,
    #[serde(serialize_with = "lossy_paths")]
    pinned_by: &'a [PathBuf],
}

/// @name toolchain_report
/// @description Serializes installed toolchains as candidates with version and pin metadata.
///
/// @example
/// let report = toolchain_report(&entries)?;
pub fn toolchain_report(entries: &[(Toolchain, Candidate)]) -> Result<String, serde_json::Error> {
    listing_report(
        Source::Toolchain,
        entries,
        Vec::new(),
        toolchain_json,
        Shape::Document,
    )
}

/// @name toolchain_lines
/// @description Serializes installed toolchains as `--json-lines`: one candidate per line, then
/// a summary line.
///
/// @example
/// println!("{}", output::toolchain_lines(&entries)?);
pub fn toolchain_lines(entries: &[(Toolchain, Candidate)]) -> Result<String, serde_json::Error> {
    listing_report(
        Source::Toolchain,
        entries,
        Vec::new(),
        toolchain_json,
        Shape::Lines,
    )
}

fn toolchain_json(toolchain: &Toolchain) -> ToolchainJson<'_> {
    ToolchainJson {
        tool: toolchain.tool.name(),
        runtime: &toolchain.runtime,
        version: &toolchain.version,
        default: toolchain.default,
        pinned_by: &toolchain.pins,
    }
}

#[derive(Serialize)]
struct DockerJson<'a> {
    engine: &'a str,
    object: Object,
    object_id: &'a str,
    name: &'a str,
    note: Option<&'a str>,
}

/// @name docker_report
/// @description Serializes container engine candidates and reports engine failures as issues.
///
/// @example
/// let report = docker_report(&entries, &listing.issues)?;
pub fn docker_report(
    entries: &[(Item, Candidate)],
    issues: &[String],
) -> Result<String, serde_json::Error> {
    listing_report(
        Source::Docker,
        entries,
        docker_issues(issues),
        docker_json,
        Shape::Document,
    )
}

/// @name docker_lines
/// @description Serializes container engine candidates as `--json-lines`: one candidate per
/// line, then a summary line that carries the engine failures as issues.
///
/// @example
/// println!("{}", output::docker_lines(&entries, &listing.issues)?);
pub fn docker_lines(
    entries: &[(Item, Candidate)],
    issues: &[String],
) -> Result<String, serde_json::Error> {
    listing_report(
        Source::Docker,
        entries,
        docker_issues(issues),
        docker_json,
        Shape::Lines,
    )
}

fn docker_issues(issues: &[String]) -> Vec<IssueJson<'_>> {
    issues
        .iter()
        .map(|message| IssueJson {
            path: Path::new("docker"),
            kind: IssueKind::Io,
            message,
        })
        .collect()
}

fn docker_json(item: &Item) -> DockerJson<'_> {
    DockerJson {
        engine: &item.engine,
        object: item.object,
        object_id: &item.id,
        name: &item.label,
        note: (!item.note.is_empty()).then_some(item.note.as_str()),
    }
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

    #[test]
    fn json_lines_keep_the_report_fields_one_object_per_line() {
        let summary = Summary {
            progress: Progress::default(),
            duration: Duration::ZERO,
            cancelled: false,
        };
        let found = candidate(PathBuf::from("/p/ok/node_modules"));
        let document: serde_json::Value = serde_json::from_str(
            &scan_report(&[], std::slice::from_ref(&found), &[], &summary).expect("serializes"),
        )
        .expect("valid json");
        let mut line: serde_json::Value =
            serde_json::from_str(&scan_line(&found).expect("serializes")).expect("valid json");
        assert_eq!(line["type"], "candidate");
        line.as_object_mut().expect("object").remove("type");
        assert_eq!(line, document["candidates"][0]);

        let closing: serde_json::Value =
            serde_json::from_str(&scan_summary_line(&[], &[], &summary).expect("serializes"))
                .expect("valid json");
        let mut expected = document;
        let report = expected.as_object_mut().expect("object");
        report.remove("candidates");
        report.insert("type".to_owned(), "summary".into());
        assert_eq!(closing, expected);
    }
}
