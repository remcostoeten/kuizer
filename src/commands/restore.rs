use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use super::AppError;
use crate::cli::RestoreArgs;
use crate::history::restore::{self, RestoreError, Status};
use crate::history::{self, History};
use crate::output::{self, SCHEMA_VERSION};
use crate::units::ByteSize;

#[derive(Serialize)]
struct Outcome<'a> {
    id: &'a str,
    path: &'a Path,
    bytes: u64,
    status: Status,
    reason: Option<&'a str>,
}

#[derive(Serialize)]
struct Summary {
    restored: usize,
    restored_bytes: u64,
    skipped: usize,
    failed: usize,
}

#[derive(Serialize)]
struct Report<'a> {
    schema_version: u32,
    command: &'static str,
    dry_run: bool,
    items: &'a [Outcome<'a>],
    summary: Summary,
}

fn resolve(
    history: &History,
    args: &RestoreArgs,
    restored: &HashSet<String>,
) -> Result<Vec<(usize, usize)>, AppError> {
    if args.last {
        return restore::latest_run(history, restored).ok_or(AppError::NothingToRestore);
    }
    let mut targets = Vec::new();
    for id in &args.ids {
        let found = history
            .find(id)
            .ok_or_else(|| AppError::UnknownRun(id.clone()))?;
        for target in found {
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
    }
    Ok(targets)
}

pub fn run(args: RestoreArgs, json: bool) -> Result<(), AppError> {
    let history = history::load(&history::default_path()?)?;
    let restored: HashSet<String> = history
        .restored_ids()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let targets = resolve(&history, &args, &restored)?;
    let result =
        restore::restore(&history, &targets, args.dry_run).map_err(|error| match error {
            RestoreError::Unsupported => AppError::RestoreUnsupported,
            RestoreError::Trash(error) => AppError::Trash(error),
        })?;
    result.record()?;
    let outcomes: Vec<Outcome> = result
        .outcomes
        .iter()
        .map(|outcome| Outcome {
            id: &outcome.id,
            path: &outcome.path,
            bytes: outcome.bytes,
            status: outcome.status,
            reason: outcome.reason.as_deref(),
        })
        .collect();
    report(&outcomes, args.dry_run, json);
    let failed = result.count(Status::Failed);
    if failed > 0 {
        return Err(AppError::RestoreFailed(failed));
    }
    Ok(())
}

fn report(outcomes: &[Outcome], dry_run: bool, json: bool) {
    let count = |status: Status| {
        outcomes
            .iter()
            .filter(|outcome| outcome.status == status)
            .count()
    };
    let done = if dry_run {
        Status::WouldRestore
    } else {
        Status::Restored
    };
    let summary = Summary {
        restored: count(done),
        restored_bytes: outcomes
            .iter()
            .filter(|outcome| outcome.status == done)
            .map(|outcome| outcome.bytes)
            .sum(),
        skipped: count(Status::Skipped),
        failed: count(Status::Failed),
    };
    if json {
        let report = Report {
            schema_version: SCHEMA_VERSION,
            command: "restore",
            dry_run,
            items: outcomes,
            summary,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
        return;
    }
    for outcome in outcomes {
        let label = match outcome.status {
            Status::Restored => "restored",
            Status::WouldRestore => "would restore",
            Status::Skipped => "skipped",
            Status::Failed => "failed",
        };
        let reason = outcome
            .reason
            .map(|reason| format!("  {reason}"))
            .unwrap_or_default();
        println!(
            "{label:<13}  {:<8}  {:>10}  {}{reason}",
            outcome.id,
            ByteSize(outcome.bytes).to_string(),
            output::display_path(outcome.path)
        );
    }
    let verb = if dry_run {
        "would be restored"
    } else {
        "restored"
    };
    println!(
        "\n{} items {verb} ({}), {} skipped{}",
        summary.restored,
        ByteSize(summary.restored_bytes),
        summary.skipped,
        if summary.failed > 0 {
            format!(", {} failed", summary.failed)
        } else {
            String::new()
        }
    );
}
