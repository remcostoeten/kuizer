use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::style::{bold, dim};
use crate::caches::KnownCache;
use crate::delete::{Mode, Removal, Status};
use crate::detect::{Category, Safety};
use crate::dupes::{Duplicates, Group};
use crate::platform;
use crate::scan::{Candidate, Issue, Progress, Summary};
use crate::units::ByteSize;

const MAX_ISSUES: usize = 10;
const LARGEST: usize = 10;

pub fn display_path(path: &Path) -> String {
    let display: String = match platform::home_dir()
        .and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf))
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => PathBuf::from("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    };
    display
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

pub fn print_progress(progress: Progress) {
    let mut err = io::stderr().lock();
    let _ = write!(
        err,
        "\r\x1b[2K{}",
        dim(&format!(
            "scanning  {} dirs  {} files  {} found  {}",
            progress.dirs,
            progress.files,
            progress.candidates,
            ByteSize(progress.bytes)
        ))
    );
    let _ = err.flush();
}

pub fn clear_progress() {
    let mut err = io::stderr().lock();
    let _ = write!(err, "\r\x1b[2K");
    let _ = err.flush();
}

fn header() -> String {
    format!(
        "{:>10}  {:<6}  {:<12}  {:<20}  PATH",
        "SIZE", "SAFETY", "CATEGORY", "RULE"
    )
}

fn row(candidate: &Candidate) -> String {
    let partial = if candidate.usage.partial {
        dim(" (partial)")
    } else {
        String::new()
    };
    let safety = match candidate.rule.safety {
        Safety::Safe => candidate.rule.safety.as_str().to_owned(),
        Safety::Review => dim(candidate.rule.safety.as_str()),
    };
    format!(
        "{}  {safety:<pad$}  {:<12}  {:<20}  {}{partial}",
        bold(&format!(
            "{:>10}",
            ByteSize(candidate.usage.allocated).to_string()
        )),
        candidate.rule.category.as_str(),
        candidate.rule.name,
        display_path(&candidate.path),
        pad = 6 + safety.len() - candidate.rule.safety.as_str().len(),
    )
}

pub fn print_scan(candidates: &[Candidate], issues: &[Issue], summary: &Summary, quiet: bool) {
    let mut out = io::stdout().lock();
    if !candidates.is_empty() {
        let _ = writeln!(out, "{}", dim(&header()));
    }
    for candidate in candidates {
        let _ = writeln!(out, "{}", row(candidate));
    }
    drop(out);
    if !quiet {
        print_distribution(candidates, LARGEST);
        print_totals(candidates, summary);
        print_issues(issues);
    }
}

#[derive(Default)]
struct Bucket {
    count: usize,
    bytes: u64,
}

fn bar(bucket: &Bucket, total: u64) -> String {
    let share = if total == 0 {
        0.0
    } else {
        bucket.bytes as f64 / total as f64
    };
    let filled = (share * 20.0).round() as usize;
    format!("{}{}", "█".repeat(filled), dim(&"░".repeat(20 - filled)))
}

fn bucket_line(label: &str, bucket: &Bucket, total: u64) -> String {
    format!(
        "  {label:<22} {:>5}  {:>10}  {}",
        bucket.count,
        ByteSize(bucket.bytes).to_string(),
        bar(bucket, total),
    )
}

fn project_line(project: &Path, bucket: &Bucket, total: u64) -> String {
    format!(
        "  {:>10}  {}  {:>5}  {}",
        ByteSize(bucket.bytes).to_string(),
        bar(bucket, total),
        bucket.count,
        display_path(project),
    )
}

fn print_distribution(candidates: &[Candidate], largest: usize) {
    let total: u64 = candidates.iter().map(|c| c.usage.allocated).sum();
    let mut categories: BTreeMap<Category, Bucket> = BTreeMap::new();
    let mut rules: BTreeMap<&str, Bucket> = BTreeMap::new();
    let mut projects: BTreeMap<&Path, Bucket> = BTreeMap::new();
    for candidate in candidates {
        let category = categories.entry(candidate.rule.category).or_default();
        category.count += 1;
        category.bytes += candidate.usage.allocated;
        let rule = rules.entry(candidate.rule.name.as_str()).or_default();
        rule.count += 1;
        rule.bytes += candidate.usage.allocated;
        let project = projects
            .entry(candidate.project.as_deref().unwrap_or(&candidate.path))
            .or_default();
        project.count += 1;
        project.bytes += candidate.usage.allocated;
    }

    let mut out = io::stdout().lock();
    if candidates.is_empty() {
        return;
    }

    let _ = writeln!(out, "\n{}", bold("By category"));
    for (category, bucket) in &categories {
        let _ = writeln!(out, "{}", bucket_line(category.as_str(), bucket, total));
    }

    let mut by_rule: Vec<(&str, Bucket)> = rules.into_iter().collect();
    by_rule.sort_by_key(|entry| std::cmp::Reverse(entry.1.bytes));
    let _ = writeln!(out, "\n{}", bold("By rule"));
    for (rule, bucket) in &by_rule {
        let _ = writeln!(out, "{}", bucket_line(rule, bucket, total));
    }

    let mut by_project: Vec<(&Path, Bucket)> = projects.into_iter().collect();
    by_project.sort_by_key(|entry| std::cmp::Reverse(entry.1.bytes));
    let _ = writeln!(out, "\n{}", bold("By project"));
    for (project, bucket) in by_project.iter().take(largest) {
        let _ = writeln!(out, "{}", project_line(project, bucket, total));
    }
    if by_project.len() > largest {
        let _ = writeln!(
            out,
            "{}",
            dim(&format!(
                "  and {} more projects",
                by_project.len() - largest
            ))
        );
    }

    let mut biggest: Vec<&Candidate> = candidates.iter().collect();
    biggest.sort_by_key(|c| std::cmp::Reverse(c.usage.allocated));
    let _ = writeln!(out, "\n{}", bold("Largest"));
    for candidate in biggest.into_iter().take(largest) {
        let _ = writeln!(
            out,
            "  {:>10}  {}",
            ByteSize(candidate.usage.allocated).to_string(),
            display_path(&candidate.path)
        );
    }
}

pub fn print_overview(
    candidates: &[Candidate],
    issues: &[Issue],
    summary: &Summary,
    largest: usize,
) {
    print_distribution(candidates, largest);
    print_totals(candidates, summary);
    print_issues(issues);
}

pub fn largest_count(limit: Option<usize>) -> usize {
    limit.unwrap_or(LARGEST)
}

fn print_totals(candidates: &[Candidate], summary: &Summary) {
    let total: u64 = candidates.iter().map(|c| c.usage.allocated).sum();
    let safe: u64 = candidates
        .iter()
        .filter(|c| c.rule.safety == Safety::Safe)
        .map(|c| c.usage.allocated)
        .sum();
    println!(
        "\n{} in {} items: {} safe, {} to review",
        bold(&ByteSize(total).to_string()),
        candidates.len(),
        ByteSize(safe),
        ByteSize(total - safe),
    );
    println!(
        "{}",
        dim(&format!(
            "Scanned {} dirs and {} files in {:.2}s{}",
            summary.progress.dirs,
            summary.progress.files,
            summary.duration.as_secs_f64(),
            if summary.cancelled { ", cancelled" } else { "" },
        ))
    );
}

fn print_issues(issues: &[Issue]) {
    if issues.is_empty() {
        return;
    }
    let mut err = io::stderr().lock();
    let _ = writeln!(err, "{} paths could not be read:", issues.len());
    for issue in issues.iter().take(MAX_ISSUES) {
        let _ = writeln!(err, "  {}: {}", display_path(&issue.path), issue.message);
    }
    if issues.len() > MAX_ISSUES {
        let _ = writeln!(
            err,
            "  and {} more, use --json for the full list",
            issues.len() - MAX_ISSUES
        );
    }
}

pub fn print_candidates(candidates: &[Candidate]) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{}", dim(&header()));
    for candidate in candidates {
        let _ = writeln!(out, "{}", row(candidate));
    }
}

pub fn print_removal_plan(candidates: &[Candidate], mode: Mode) {
    print_candidates(candidates);
    let total: u64 = candidates.iter().map(|c| c.usage.allocated).sum();
    let review = candidates
        .iter()
        .filter(|c| c.rule.safety == Safety::Review)
        .count();
    let review_note = match review {
        0 => String::new(),
        count => format!(", {count} marked review"),
    };
    println!(
        "\n{} in {} items{review_note}, {}",
        bold(&ByteSize(total).to_string()),
        candidates.len(),
        mode.label()
    );
}

pub fn print_removal(removal: &Removal) {
    let size = ByteSize(removal.candidate.usage.allocated).to_string();
    let path = display_path(&removal.candidate.path);
    match (removal.status, &removal.error) {
        (Status::Failed, error) => eprintln!(
            "{:>10}  failed   {path}: {}",
            size,
            error.as_deref().unwrap_or("unknown error")
        ),
        (Status::Removed, _) => println!("{:>10}  removed  {path}", size),
        (Status::WouldRemove, _) => println!("{:>10}  kept     {path}", size),
    }
}

pub fn print_removal_summary(removals: &[Removal], mode: Mode) {
    let removed: u64 = removals
        .iter()
        .filter(|removal| removal.status == Status::Removed)
        .map(|removal| removal.candidate.usage.allocated)
        .sum();
    let count = removals
        .iter()
        .filter(|removal| removal.status == Status::Removed)
        .count();
    let failed = removals.len() - count;
    let verb = match mode {
        Mode::Trash => "Moved to the trash",
        Mode::Permanent => "Freed",
        Mode::DryRun => "Would remove",
    };
    let failures = match failed {
        0 => String::new(),
        failed => format!(", {failed} failed"),
    };
    println!(
        "\n{verb}: {} across {count} items{failures}",
        bold(&ByteSize(removed).to_string())
    );
    if mode == Mode::Trash && count > 0 {
        println!(
            "{}",
            dim(
                "The space is freed when the trash is emptied. `kuizer history` shows what is still there and `kuizer restore` brings it back."
            )
        );
    }
}

pub fn print_status(text: &str) {
    let mut err = io::stderr().lock();
    let _ = write!(err, "\r\x1b[2K{}", dim(text));
    let _ = err.flush();
}

pub fn print_duplicates(found: &Duplicates, groups: &[Group], quiet: bool) {
    let mut out = io::stdout().lock();
    for group in groups {
        let _ = writeln!(
            out,
            "{}  {} copies of {}, {} reclaimable",
            bold(&format!(
                "{:>10}",
                ByteSize(group.reclaimable()).to_string()
            )),
            group.members.len(),
            ByteSize(group.size),
            ByteSize(group.reclaimable()),
        );
        for member in &group.members {
            let links = match member.links.len() {
                0 => String::new(),
                count => dim(&format!(" (+{count} hard links)")),
            };
            let _ = writeln!(out, "            {}{links}", display_path(&member.path));
        }
    }
    drop(out);
    if quiet {
        return;
    }
    let copies: usize = found
        .groups
        .iter()
        .map(|group| group.members.len() - 1)
        .sum();
    if found.groups.is_empty() {
        println!("No duplicate files found among {} files", found.files);
    } else {
        println!(
            "\n{} reclaimable across {} groups ({copies} extra copies)",
            bold(&ByteSize(found.reclaimable()).to_string()),
            found.groups.len(),
        );
        if groups.len() < found.groups.len() {
            println!(
                "{}",
                dim(&format!(
                    "Showing the {} largest groups, use --limit or --json for more",
                    groups.len()
                ))
            );
        }
        println!(
            "{}",
            dim("kuizer only reports duplicates. Choose which copy to keep yourself.")
        );
    }
    println!(
        "{}",
        dim(&format!(
            "Checked {} files in {:.2}s",
            found.files,
            found.duration.as_secs_f64()
        ))
    );
    print_issues(&found.issues);
}

pub fn print_caches(entries: &[(KnownCache, Candidate)], quiet: bool) {
    if entries.is_empty() {
        println!("No known tool caches found");
        return;
    }
    let mut out = io::stdout().lock();
    let _ = writeln!(
        out,
        "{}",
        dim(&format!(
            "{:>10}  {:<6}  {:<20}  {:<11}  PATH",
            "SIZE", "SAFETY", "NAME", "TOOL"
        ))
    );
    for (cache, candidate) in entries {
        let safety = cache.safety.as_str();
        let _ = writeln!(
            out,
            "{}  {safety:<6}  {:<20}  {:<11}  {}",
            bold(&format!(
                "{:>10}",
                ByteSize(candidate.usage.allocated).to_string()
            )),
            cache.name,
            cache.tool,
            display_path(&cache.path),
        );
        if !cache.note.is_empty() {
            let _ = writeln!(out, "{}", dim(&format!("{:>12}{}", "", cache.note)));
        }
    }
    drop(out);
    if quiet {
        return;
    }
    let total: u64 = entries.iter().map(|(_, c)| c.usage.allocated).sum();
    let safe: u64 = entries
        .iter()
        .filter(|(cache, _)| cache.safety == Safety::Safe)
        .map(|(_, c)| c.usage.allocated)
        .sum();
    println!(
        "\n{} in {} caches: {} safe, {} to review",
        bold(&ByteSize(total).to_string()),
        entries.len(),
        ByteSize(safe),
        ByteSize(total - safe),
    );
    println!(
        "{}",
        dim(
            "Browser profiles, cookies and sessions are never listed. Clean with `kuizer cache --clean`."
        )
    );
}
