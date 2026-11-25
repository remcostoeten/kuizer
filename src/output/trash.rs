use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::SCHEMA_VERSION;
use super::style::{bold, dim};
use super::text::display_path;
use crate::scan::{Candidate, Issue, IssueKind};
use crate::trashcan::Folder;
use crate::units::ByteSize;

pub fn print_trash(entries: &[(Folder, Candidate)], issues: &[Issue], quiet: bool) {
    for issue in issues {
        eprintln!("kuizer: {}", issue.message);
    }
    if entries.is_empty() {
        println!("The trash is empty");
        return;
    }
    let mut out = BufWriter::new(io::stdout().lock());
    let _ = writeln!(
        out,
        "{}",
        dim(&format!("{:>10}  {:>6}  PATH", "SIZE", "ITEMS"))
    );
    for (folder, candidate) in entries {
        let _ = writeln!(
            out,
            "{}  {:>6}  {}",
            bold(&format!(
                "{:>10}",
                ByteSize(candidate.usage.allocated).to_string()
            )),
            folder.items,
            display_path(&folder.path),
        );
    }
    drop(out);
    if quiet {
        return;
    }
    let total_bytes: u64 = entries.iter().map(|(_, c)| c.usage.allocated).sum();
    let total_items: usize = entries.iter().map(|(f, _)| f.items).sum();
    let noun = if total_items == 1 { "item" } else { "items" };
    println!(
        "\n{} in {} {noun} in the trash",
        bold(&ByteSize(total_bytes).to_string()),
        total_items,
    );
    println!("{}", dim("Empty permanently with `kuizer trash --clean`."));
}

#[derive(Serialize)]
struct FolderJson<'a> {
    path: String,
    items: usize,
    allocated_bytes: u64,
    apparent_bytes: u64,
    newest_modified: Option<u64>,
    #[serde(skip_serializing_if = "is_false")]
    path_lossy: bool,
    #[serde(skip)]
    _marker: std::marker::PhantomData<&'a Path>,
}

#[derive(Serialize)]
struct IssueJson<'a> {
    path: String,
    kind: IssueKind,
    message: &'a str,
}

#[derive(Serialize)]
struct SummaryJson {
    items: usize,
    bytes: u64,
}

#[derive(Serialize)]
struct TrashReport<'a> {
    schema_version: u32,
    command: &'static str,
    folders: Vec<FolderJson<'a>>,
    issues: Vec<IssueJson<'a>>,
    summary: SummaryJson,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn unix_secs(time: Option<SystemTime>) -> Option<u64> {
    time.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

pub fn trash_report(
    entries: &[(Folder, Candidate)],
    issues: &[Issue],
) -> Result<String, serde_json::Error> {
    let folders = entries
        .iter()
        .map(|(folder, candidate)| FolderJson {
            path: folder.path.to_string_lossy().into_owned(),
            items: folder.items,
            allocated_bytes: candidate.usage.allocated,
            apparent_bytes: candidate.usage.apparent,
            newest_modified: unix_secs(candidate.newest),
            path_lossy: folder.path.to_str().is_none(),
            _marker: std::marker::PhantomData,
        })
        .collect();
    let report_issues = issues
        .iter()
        .map(|issue| IssueJson {
            path: issue.path.to_string_lossy().into_owned(),
            kind: issue.kind,
            message: &issue.message,
        })
        .collect();
    let summary = SummaryJson {
        items: entries.iter().map(|(folder, _)| folder.items).sum(),
        bytes: entries.iter().map(|(_, c)| c.usage.allocated).sum(),
    };
    serde_json::to_string_pretty(&TrashReport {
        schema_version: SCHEMA_VERSION,
        command: "trash",
        folders,
        issues: report_issues,
        summary,
    })
}
