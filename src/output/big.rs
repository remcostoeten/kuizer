use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::time::SystemTime;

use chrono::{DateTime, Local};
use serde::Serialize;

use super::SCHEMA_VERSION;
use super::json::{CandidateJson, IssueJson, Source, issues_json};
use super::style::{bold, dim};
use super::text::display_path;
use crate::scan::{Candidate, Issue};
use crate::units::ByteSize;

fn format_date(time: Option<SystemTime>) -> String {
    match time {
        Some(time) => DateTime::<Local>::from(time).format("%Y-%m-%d").to_string(),
        None => "-".to_owned(),
    }
}

fn kind_label(candidate: &Candidate) -> &'static str {
    if candidate.rule.name == "old-download" {
        "download"
    } else {
        "large"
    }
}

pub fn print_big(candidates: &[Candidate], issues: &[Issue], quiet: bool) {
    for issue in issues {
        eprintln!("kuizer: {}: {}", display_path(&issue.path), issue.message);
    }
    if candidates.is_empty() {
        println!("No large files found");
        return;
    }
    let mut out = BufWriter::new(io::stdout().lock());
    let _ = writeln!(
        out,
        "{}",
        dim(&format!(
            "{:>10}  {:<10}  {:<8}  PATH",
            "SIZE", "MODIFIED", "KIND"
        ))
    );
    for candidate in candidates {
        let _ = writeln!(
            out,
            "{}  {:<10}  {:<8}  {}",
            bold(&format!(
                "{:>10}",
                ByteSize(candidate.usage.apparent).to_string()
            )),
            format_date(candidate.newest),
            kind_label(candidate),
            display_path(&candidate.path),
        );
    }
    drop(out);
    if quiet {
        return;
    }
    let allocated: u64 = candidates.iter().map(|c| c.usage.allocated).sum();
    let apparent: u64 = candidates.iter().map(|c| c.usage.apparent).sum();
    let noun = if candidates.len() == 1 {
        "file"
    } else {
        "files"
    };
    println!(
        "\n{} on disk ({} apparent) in {} {noun}",
        bold(&ByteSize(allocated).to_string()),
        ByteSize(apparent),
        candidates.len(),
    );
}

#[derive(Serialize)]
struct SummaryJson {
    items: usize,
    allocated_bytes: u64,
    apparent_bytes: u64,
}

#[derive(Serialize)]
struct BigReport<'a> {
    schema_version: u32,
    command: &'static str,
    roots: Vec<String>,
    candidates: Vec<CandidateJson<'a>>,
    issues: Vec<IssueJson<'a>>,
    summary: SummaryJson,
}

pub fn big_report(
    roots: &[PathBuf],
    candidates: &[Candidate],
    issues: &[Issue],
) -> Result<String, serde_json::Error> {
    let allocated_bytes: u64 = candidates.iter().map(|c| c.usage.allocated).sum();
    let apparent_bytes: u64 = candidates.iter().map(|c| c.usage.apparent).sum();
    serde_json::to_string_pretty(&BigReport {
        schema_version: SCHEMA_VERSION,
        command: "big",
        roots: roots
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect(),
        candidates: candidates
            .iter()
            .map(|candidate| CandidateJson::new(Source::Big, candidate))
            .collect(),
        issues: issues_json(issues),
        summary: SummaryJson {
            items: candidates.len(),
            allocated_bytes,
            apparent_bytes,
        },
    })
}
