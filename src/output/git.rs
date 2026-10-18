use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

use super::json::SCHEMA_VERSION;
use super::style::{bold, dim};
use super::text::display_path;
use crate::repos::{Branch, Busy, Collected, Pruned, RepoError, Repository};
use crate::units::ByteSize;

pub struct GitOutcome<'a> {
    pub repository: &'a Repository,
    pub collected: Option<Result<Collected, RepoError>>,
    pub pruned: Vec<Pruned>,
}

#[derive(Serialize)]
struct RepositoryJson<'a> {
    path: &'a Path,
    loose_objects: u64,
    loose_bytes: u64,
    packs: u64,
    packed_bytes: u64,
    garbage_bytes: u64,
    estimated_gain: u64,
    default_branch: Option<&'a str>,
    current_branch: Option<&'a str>,
    merged_branches: &'a [Branch],
    busy: Option<Busy>,
}

#[derive(Serialize)]
struct GcJson {
    before_bytes: u64,
    after_bytes: u64,
    freed_bytes: u64,
}

#[derive(Serialize)]
struct BranchJson<'a> {
    name: &'a str,
    commit: &'a str,
    deleted: bool,
    error: Option<String>,
}

#[derive(Serialize)]
struct OutcomeJson<'a> {
    path: &'a Path,
    gc: Option<GcJson>,
    gc_error: Option<String>,
    branches: Vec<BranchJson<'a>>,
}

#[derive(Serialize)]
struct GitSummary {
    repositories: usize,
    estimated_gain: u64,
    merged_branches: usize,
}

#[derive(Serialize)]
struct GitReport<'a> {
    schema_version: u32,
    command: &'static str,
    repositories: Vec<RepositoryJson<'a>>,
    results: Option<Vec<OutcomeJson<'a>>>,
    summary: GitSummary,
}

fn repository_json(repository: &Repository) -> RepositoryJson<'_> {
    RepositoryJson {
        path: &repository.root,
        loose_objects: repository.objects.loose,
        loose_bytes: repository.objects.loose_bytes,
        packs: repository.objects.packs,
        packed_bytes: repository.objects.packed_bytes,
        garbage_bytes: repository.objects.garbage_bytes,
        estimated_gain: repository.objects.gain(),
        default_branch: repository.default_branch.as_deref(),
        current_branch: repository.current_branch.as_deref(),
        merged_branches: &repository.merged,
        busy: repository.busy,
    }
}

fn outcome_json<'a>(outcome: &'a GitOutcome<'a>) -> OutcomeJson<'a> {
    let (gc, gc_error) = match &outcome.collected {
        Some(Ok(collected)) => (
            Some(GcJson {
                before_bytes: collected.before,
                after_bytes: collected.after,
                freed_bytes: collected.freed(),
            }),
            None,
        ),
        Some(Err(error)) => (None, Some(error.to_string())),
        None => (None, None),
    };
    OutcomeJson {
        path: &outcome.repository.root,
        gc,
        gc_error,
        branches: outcome
            .pruned
            .iter()
            .map(|pruned| BranchJson {
                name: &pruned.branch.name,
                commit: &pruned.branch.commit,
                deleted: pruned.result.is_ok(),
                error: pruned.result.as_ref().err().map(ToString::to_string),
            })
            .collect(),
    }
}

fn summary(repositories: &[Repository]) -> GitSummary {
    GitSummary {
        repositories: repositories.len(),
        estimated_gain: repositories.iter().map(|r| r.objects.gain()).sum(),
        merged_branches: repositories.iter().map(|r| r.merged.len()).sum(),
    }
}

/// @name git_report
/// @description Renders the repositories worth cleaning, and the results of `--gc` or
/// `--branches` when they ran, as the versioned JSON report.
///
/// @example
/// println!("{}", output::git_report(&found, None));
pub fn git_report(repositories: &[Repository], outcomes: Option<&[GitOutcome]>) -> String {
    let report = GitReport {
        schema_version: SCHEMA_VERSION,
        command: "git",
        repositories: repositories.iter().map(repository_json).collect(),
        results: outcomes.map(|outcomes| outcomes.iter().map(outcome_json).collect()),
        summary: summary(repositories),
    };
    serde_json::to_string_pretty(&report).unwrap_or_default()
}

pub fn print_repositories(repositories: &[Repository], quiet: bool) {
    if repositories.is_empty() {
        println!("No git repositories worth cleaning");
        return;
    }
    let mut out = io::stdout().lock();
    let _ = writeln!(
        out,
        "{}",
        dim(&format!(
            "{:>10}  {:>7}  {:>8}  PATH",
            "GC GAIN", "LOOSE", "MERGED"
        ))
    );
    for repository in repositories {
        let _ = writeln!(
            out,
            "{}  {:>7}  {:>8}  {}",
            bold(&format!(
                "{:>10}",
                ByteSize(repository.objects.gain()).to_string()
            )),
            repository.objects.loose,
            repository.merged.len(),
            display_path(&repository.root),
        );
        if let Some(busy) = repository.busy {
            let _ = writeln!(
                out,
                "{}",
                dim(&format!("{:>12}skipped: {}", "", busy.describe()))
            );
        }
        if !repository.merged.is_empty() {
            let names: Vec<&str> = repository
                .merged
                .iter()
                .map(|branch| branch.name.as_str())
                .collect();
            let base = repository.default_branch.as_deref().unwrap_or("default");
            let _ = writeln!(
                out,
                "{}",
                dim(&format!(
                    "{:>12}merged into {base}: {}",
                    "",
                    names.join(", ")
                ))
            );
        }
    }
    drop(out);
    if quiet {
        return;
    }
    let summary = summary(repositories);
    println!(
        "\n{} repositories: git gc frees up to {}, {} merged branches",
        summary.repositories,
        bold(&ByteSize(summary.estimated_gain).to_string()),
        summary.merged_branches,
    );
    println!(
        "{}",
        dim("Run with --gc or --branches to clean, add --dry-run to preview")
    );
}

pub fn print_git_outcome(outcome: &GitOutcome) {
    let path = display_path(&outcome.repository.root);
    match &outcome.collected {
        Some(Ok(collected)) => println!(
            "gc        {path}  freed {}",
            bold(&ByteSize(collected.freed()).to_string())
        ),
        Some(Err(error)) => println!("gc        {path}  {error}"),
        None => {}
    }
    for pruned in &outcome.pruned {
        let name = &pruned.branch.name;
        match &pruned.result {
            Ok(()) => println!(
                "deleted   {path}  {name}  {}",
                dim(&format!(
                    "restore: git branch {name} {}",
                    pruned.branch.commit
                ))
            ),
            Err(error) => println!("kept      {path}  {name}  {error}"),
        }
    }
}
