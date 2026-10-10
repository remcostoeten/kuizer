use std::collections::HashSet;

use serde::Serialize;

use super::AppError;
use crate::delete::Mode;
use crate::history::{self, History, Total};

#[derive(Serialize)]
struct Report<'a> {
    #[serde(flatten)]
    history: &'a History,
    in_trash: Option<Total>,
}

fn state(mode: Mode, kept: bool) -> &'static str {
    match (mode, kept) {
        (Mode::Permanent, _) => "deleted",
        (Mode::Trash, true) => "in trash",
        (Mode::Trash, false) => "left trash",
        (Mode::DryRun, _) => "dry run",
    }
}

pub fn run(json: bool) -> Result<(), AppError> {
    let path = history::default_path()?;
    let history = history::load(&path)?;
    let trashed = history::trashed_paths();
    let kept = trashed
        .as_deref()
        .map(|paths| history::still_in_trash(&history, paths))
        .unwrap_or_default();
    let in_trash = trashed.as_ref().map(|_| kept_total(&history, &kept));
    if json {
        let report = Report {
            history: &history,
            in_trash,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
        return Ok(());
    }
    if history.runs.is_empty() {
        println!("nothing deleted yet, history is kept in {}", path.display());
        return Ok(());
    }
    print_runs(&history, &kept, trashed.is_some());
    let total = &history.cumulative;
    println!();
    println!(
        "{} removed across {} items in {} runs since {}",
        total.size,
        total.items,
        total.runs,
        total.first_run.as_deref().unwrap_or("-")
    );
    match in_trash {
        Some(trash) if trash.items > 0 => println!(
            "{} of that is still in the trash across {} items, emptying the trash frees it",
            trash.size, trash.items
        ),
        Some(_) => println!("none of it is still in the trash"),
        None => println!("the trash cannot be read on this platform"),
    }
    println!("{}", path.display());
    Ok(())
}

fn kept_total(history: &History, kept: &HashSet<(usize, usize)>) -> Total {
    Total::sum(
        kept.iter()
            .filter_map(|&(run, entry)| history.runs.get(run)?.deleted.get(entry)),
    )
}

fn print_runs(history: &History, kept: &HashSet<(usize, usize)>, trash_known: bool) {
    println!(
        "{:<25}  {:>5}  {:>10}  COMMAND",
        "STARTED", "ITEMS", "REMOVED"
    );
    for (run_index, run) in history.runs.iter().enumerate() {
        println!(
            "{:<25}  {:>5}  {:>10}  {}",
            run.started_at, run.total.items, run.total.size, run.command
        );
        for (entry_index, entry) in run.deleted.iter().enumerate() {
            let label = match (entry.mode, trash_known) {
                (Mode::Trash, false) => "trashed",
                (mode, _) => state(mode, kept.contains(&(run_index, entry_index))),
            };
            println!(
                "  {:>10}  {:<10}  {:<14}  {}",
                entry.size,
                label,
                entry.rule,
                entry.path.display()
            );
        }
    }
}
