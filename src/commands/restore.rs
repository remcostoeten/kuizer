use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use super::AppError;
use crate::cli::RestoreArgs;
use crate::delete::Mode;
use crate::history::{self, Action, Entry, History, Run, TrashRecord};
use crate::output::{self, SCHEMA_VERSION};
use crate::units::ByteSize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Restored,
    WouldRestore,
    Skipped,
    Failed,
}

#[derive(Serialize)]
struct Outcome<'a> {
    id: String,
    path: &'a Path,
    bytes: u64,
    status: Status,
    reason: Option<String>,
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

#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
mod backend {
    use super::AppError;

    pub fn list() -> Result<Vec<trash::TrashItem>, AppError> {
        trash::os_limited::list().map_err(|error| AppError::Trash(error.to_string()))
    }

    pub fn restore(item: trash::TrashItem) -> Result<(), String> {
        trash::os_limited::restore_all([item]).map_err(|error| error.to_string())
    }
}

#[cfg(not(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
)))]
mod backend {
    use super::AppError;

    pub fn list() -> Result<Vec<trash::TrashItem>, AppError> {
        Err(AppError::RestoreUnsupported)
    }

    pub fn restore(_item: trash::TrashItem) -> Result<(), String> {
        Err(AppError::RestoreUnsupported.to_string())
    }
}

fn latest_run(history: &History, restored: &HashSet<String>) -> Option<Vec<(usize, usize)>> {
    history
        .runs
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, run)| run.action == Action::Delete)
        .map(|(run_index, run)| {
            (0..run.deleted.len())
                .filter(|&entry| run.deleted[entry].mode == Mode::Trash)
                .filter(|&entry| !restored.contains(&history.item_id(run_index, entry)))
                .map(|entry| (run_index, entry))
                .collect::<Vec<_>>()
        })
        .find(|targets| !targets.is_empty())
}

fn resolve(
    history: &History,
    args: &RestoreArgs,
    restored: &HashSet<String>,
) -> Result<Vec<(usize, usize)>, AppError> {
    if args.last {
        return latest_run(history, restored).ok_or(AppError::NothingToRestore);
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

fn precheck(
    history: &History,
    restored: &HashSet<String>,
    target: (usize, usize),
) -> Option<String> {
    let (run, entry) = target;
    let item = &history.runs[run].deleted[entry];
    if history.runs[run].action == Action::Restore {
        return Some("is a restore, not a deletion".to_owned());
    }
    if restored.contains(&history.item_id(run, entry)) {
        return Some("was already restored".to_owned());
    }
    match item.mode {
        Mode::Permanent => Some("was deleted permanently".to_owned()),
        Mode::DryRun => Some("was a dry run, nothing was removed".to_owned()),
        Mode::Trash => None,
    }
}

fn blocked(path: &Path) -> Option<String> {
    if path.symlink_metadata().is_ok() {
        return Some(format!(
            "{} exists, not overwriting",
            output::display_path(path)
        ));
    }
    path.parent()
        .filter(|parent| !parent.is_dir())
        .map(|parent| format!("{} no longer exists", output::display_path(parent)))
}

pub fn run(args: RestoreArgs, json: bool) -> Result<(), AppError> {
    let history = history::load(&history::default_path()?)?;
    let restored: HashSet<String> = history
        .restored_ids()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let targets = resolve(&history, &args, &restored)?;
    let items = backend::list()?;
    let records: Vec<TrashRecord> = items
        .iter()
        .map(|item| TrashRecord {
            path: item.original_path(),
            deleted_at: (item.time_deleted >= 0).then_some(item.time_deleted),
        })
        .collect();

    let (eligible, refused): (Vec<_>, Vec<_>) = targets
        .iter()
        .map(|&target| (target, precheck(&history, &restored, target)))
        .partition(|(_, reason)| reason.is_none());
    let eligible: Vec<(usize, usize)> = eligible.into_iter().map(|(target, _)| target).collect();
    let matched = history::match_trash(&history, &eligible, &records);

    let entry = |(run, entry): (usize, usize)| -> &Entry { &history.runs[run].deleted[entry] };
    let outcome = |target: (usize, usize), status: Status, reason: Option<String>| Outcome {
        id: history.item_id(target.0, target.1),
        path: &entry(target).path,
        bytes: entry(target).bytes,
        status,
        reason,
    };

    let mut outcomes: Vec<Outcome> = refused
        .into_iter()
        .map(|(target, reason)| outcome(target, Status::Skipped, reason))
        .collect();
    let mut put_back: Vec<Entry> = Vec::new();
    for (&target, found) in eligible.iter().zip(matched) {
        let Some(index) = found else {
            outcomes.push(outcome(
                target,
                Status::Skipped,
                Some("is no longer in the trash".to_owned()),
            ));
            continue;
        };
        if let Some(reason) = blocked(&entry(target).path) {
            outcomes.push(outcome(target, Status::Skipped, Some(reason)));
            continue;
        }
        if args.dry_run {
            outcomes.push(outcome(target, Status::WouldRestore, None));
            continue;
        }
        match backend::restore(items[index].clone()) {
            Ok(()) => {
                put_back.push(Entry::restored(
                    entry(target),
                    history.item_id(target.0, target.1),
                ));
                outcomes.push(outcome(target, Status::Restored, None));
            }
            Err(error) => outcomes.push(outcome(target, Status::Failed, Some(error))),
        }
    }
    outcomes.sort_by_key(|outcome| order(&targets, &history, &outcome.id));

    let failed = outcomes
        .iter()
        .filter(|outcome| outcome.status == Status::Failed)
        .count();
    if !put_back.is_empty() {
        history::record(Run::restore(history::now(), put_back))?;
    }
    report(&outcomes, args.dry_run, json);
    if failed > 0 {
        return Err(AppError::RestoreFailed(failed));
    }
    Ok(())
}

fn order(targets: &[(usize, usize)], history: &History, id: &str) -> usize {
    targets
        .iter()
        .position(|&(run, entry)| history.item_id(run, entry) == id)
        .unwrap_or(usize::MAX)
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
            .as_deref()
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
