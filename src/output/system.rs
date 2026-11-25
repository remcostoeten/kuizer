use std::io::{self, BufWriter, Write};

use serde::Serialize;

use super::SCHEMA_VERSION;
use super::style::{bold, dim};
use crate::detect::Safety;
use crate::scan::Candidate;
use crate::system::{Item, Manager};
use crate::units::ByteSize;

pub fn print_system(entries: &[(Item, Candidate)], issues: &[String], quiet: bool) {
    for issue in issues {
        eprintln!("kuizer: {issue}");
    }
    if entries.is_empty() {
        println!("Nothing to clean up");
        return;
    }
    let mut out = BufWriter::new(io::stdout().lock());
    let _ = writeln!(
        out,
        "{}",
        dim(&format!(
            "{:>10}  {:<6}  {:<4}  NAME",
            "SIZE", "SAFETY", "ROOT"
        ))
    );
    for (item, _) in entries {
        let size = if item.size_known {
            bold(&format!("{:>10}", ByteSize(item.bytes).to_string()))
        } else {
            dim(&format!("{:>10}", "unknown"))
        };
        let root = if item.needs_root { "yes" } else { "no" };
        let _ = writeln!(
            out,
            "{size}  {:<6}  {root:<4}  {}",
            item.safety.as_str(),
            item.label,
        );
        if !item.note.is_empty() {
            let _ = writeln!(out, "{}", dim(&format!("{:>12}{}", "", item.note)));
        }
    }
    drop(out);
    if quiet {
        return;
    }
    let known_bytes: u64 = entries
        .iter()
        .filter(|(item, _)| item.size_known)
        .map(|(item, _)| item.bytes)
        .sum();
    let safe_bytes: u64 = entries
        .iter()
        .filter(|(item, _)| item.size_known && item.safety == Safety::Safe)
        .map(|(item, _)| item.bytes)
        .sum();
    let unknown = entries.iter().filter(|(item, _)| !item.size_known).count();
    let suffix = if unknown > 0 {
        format!(" (+{unknown} unknown size)")
    } else {
        String::new()
    };
    println!(
        "\n{} across {} system items: {} safe, {} to review{suffix}",
        bold(&ByteSize(known_bytes).to_string()),
        entries.len(),
        ByteSize(safe_bytes),
        ByteSize(known_bytes - safe_bytes),
    );
    println!(
        "{}",
        dim("Read-only report; kuizer does not escalate privileges to clean system caches.")
    );
}

#[derive(Serialize)]
struct ItemJson<'a> {
    manager: Manager,
    id: &'a str,
    label: &'a str,
    bytes: u64,
    size_known: bool,
    safety: Safety,
    needs_root: bool,
    note: &'a str,
}

#[derive(Serialize)]
struct SummaryJson {
    items: usize,
    bytes: u64,
    safe_bytes: u64,
}

#[derive(Serialize)]
struct SystemReport<'a> {
    schema_version: u32,
    command: &'static str,
    items: Vec<ItemJson<'a>>,
    issues: &'a [String],
    summary: SummaryJson,
}

pub fn system_report(
    entries: &[(Item, Candidate)],
    issues: &[String],
) -> Result<String, serde_json::Error> {
    let items = entries
        .iter()
        .map(|(item, _)| ItemJson {
            manager: item.manager,
            id: &item.id,
            label: &item.label,
            bytes: item.bytes,
            size_known: item.size_known,
            safety: item.safety,
            needs_root: item.needs_root,
            note: &item.note,
        })
        .collect();
    let bytes: u64 = entries
        .iter()
        .filter(|(item, _)| item.size_known)
        .map(|(item, _)| item.bytes)
        .sum();
    let safe_bytes: u64 = entries
        .iter()
        .filter(|(item, _)| item.size_known && item.safety == Safety::Safe)
        .map(|(item, _)| item.bytes)
        .sum();
    serde_json::to_string_pretty(&SystemReport {
        schema_version: SCHEMA_VERSION,
        command: "system",
        items,
        issues,
        summary: SummaryJson {
            items: entries.len(),
            bytes,
            safe_bytes,
        },
    })
}
