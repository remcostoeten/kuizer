use serde::Serialize;

use super::AppError;
use crate::cli::{HistoryAction, HistoryArgs, HistoryPruneArgs};
use crate::history::{
    self, Action, Entry, History, PruneOptions, Run, Total, TrashSnapshot, TrashState,
};
use crate::output;
use crate::units::ByteSize;

#[derive(Serialize)]
struct Report<'a> {
    #[serde(flatten)]
    history: &'a History,
    in_trash: Option<Total>,
    freed_bytes: u64,
}

#[derive(Serialize)]
struct ItemJson<'a> {
    id: String,
    state: &'static str,
    #[serde(flatten)]
    entry: &'a Entry,
}

#[derive(Serialize)]
struct ShowReport<'a> {
    id: String,
    #[serde(flatten)]
    run: &'a Run,
    items: Vec<ItemJson<'a>>,
}

pub fn run(args: HistoryArgs, json: bool) -> Result<(), AppError> {
    if let Some(HistoryAction::Prune(prune_args)) = args.command {
        return prune(prune_args, json);
    }
    let path = history::default_path()?;
    let history = history::load(&path)?;
    if history.runs.is_empty() && !json && args.command.is_none() {
        println!("nothing deleted yet, history is kept in {}", path.display());
        return Ok(());
    }
    let trash = TrashSnapshot::read(&history);
    if let Some(HistoryAction::Show { run }) = args.command {
        return show(&history, &trash, &run, json);
    }
    let in_trash = trash.known.then(|| trash.in_trash(&history));
    let freed = history::freed_bytes(&history, &trash);
    if json {
        let report = Report {
            history: &history,
            in_trash,
            freed_bytes: freed,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    print_runs(&history, &trash);
    let total = &history.cumulative;
    println!();
    println!(
        "{} removed across {} items in {} runs since {}",
        total.size,
        total.items,
        total.runs,
        total.first_run.as_deref().unwrap_or("-")
    );
    if total.restored_items > 0 {
        println!(
            "{} across {} items was restored and is not counted",
            ByteSize(total.restored_bytes),
            total.restored_items
        );
    }
    match in_trash {
        Some(trash) if trash.items > 0 => {
            println!(
                "{} of that is freed, deleted permanently or emptied from the trash",
                ByteSize(freed)
            );
            println!(
                "{} is still in the trash across {} items and frees once the trash is emptied, `kuizer restore ID` puts it back",
                trash.size, trash.items
            );
        }
        Some(_) => println!("all of it is freed, none of it is still in the trash"),
        None => println!(
            "{} of that was deleted permanently and is freed; the trash cannot be read on this platform",
            ByteSize(freed)
        ),
    }
    println!("{}", path.display());
    Ok(())
}

fn trash_column(history: &History, trash: &TrashSnapshot, run_index: usize) -> String {
    if history.runs[run_index].action == Action::Restore {
        return "restore".to_owned();
    }
    match trash.state(history, run_index) {
        TrashState::Restorable { items, .. } => format!("{items} kept"),
        TrashState::Restored => "restored".to_owned(),
        TrashState::Emptied => "emptied".to_owned(),
        TrashState::Unknown => "unknown".to_owned(),
        TrashState::NotTrashed => "-".to_owned(),
    }
}

fn command_column(run: &Run) -> String {
    match &run.policy {
        Some(policy) => format!("[policy: {policy}] {}", run.command),
        None => run.command.clone(),
    }
}

fn print_runs(history: &History, trash: &TrashSnapshot) {
    println!(
        "{:<6}  {:<25}  {:>5}  {:>10}  {:<9}  COMMAND",
        "ID", "STARTED", "ITEMS", "SIZE", "TRASH"
    );
    for (index, run) in history.runs.iter().enumerate().rev() {
        println!(
            "{:<6}  {:<25}  {:>5}  {:>10}  {:<9}  {}",
            run.session(),
            run.started_at,
            run.total.items,
            run.total.size,
            trash_column(history, trash, index),
            command_column(run)
        );
    }
}

fn show(history: &History, trash: &TrashSnapshot, id: &str, json: bool) -> Result<(), AppError> {
    let run_index = history
        .runs
        .iter()
        .position(|run| run.session() == id)
        .ok_or_else(|| AppError::UnknownRun(id.to_owned()))?;
    let run = &history.runs[run_index];
    let items: Vec<ItemJson> = run
        .deleted
        .iter()
        .enumerate()
        .map(|(entry_index, entry)| ItemJson {
            id: history.item_id(run_index, entry_index),
            state: history::item_state(history, trash, run_index, entry_index).as_str(),
            entry,
        })
        .collect();
    if json {
        let report = ShowReport {
            id: run.session(),
            run,
            items,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!(
        "{}  {}  {} items  {}",
        run.session(),
        run.started_at,
        run.total.items,
        run.total.size
    );
    if let Some(policy) = &run.policy {
        println!("policy: {policy}");
    }
    println!("{}", run.command);
    println!();
    for item in &items {
        let source = item
            .entry
            .restores
            .as_deref()
            .map(|id| format!("  from {id}"))
            .unwrap_or_default();
        println!(
            "  {:<8}  {:>10}  {:<13}  {:<14}  {}{source}",
            item.id,
            item.entry.size,
            item.state,
            item.entry.rule,
            output::display_path(&item.entry.path)
        );
    }
    Ok(())
}

fn prune(args: HistoryPruneArgs, json: bool) -> Result<(), AppError> {
    let options = PruneOptions {
        keep: args.keep,
        older_than: args.older_than.map(|age| age.0),
        max_bytes: args.max_size.map(|size| size.0),
        dry_run: args.dry_run,
    };
    let report = history::prune_default(&options)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if report.dry_run {
        println!(
            "would prune {} runs, keeping {} ({} -> {})",
            report.pruned_runs,
            report.kept_runs,
            ByteSize(report.bytes_before),
            ByteSize(report.bytes_after),
        );
    } else if report.pruned_runs == 0 && report.bytes_after == report.bytes_before {
        println!(
            "nothing to prune ({} runs, {})",
            report.kept_runs,
            ByteSize(report.bytes_after),
        );
    } else {
        println!(
            "pruned {} runs, kept {} ({} -> {})",
            report.pruned_runs,
            report.kept_runs,
            ByteSize(report.bytes_before),
            ByteSize(report.bytes_after),
        );
    }
    println!("{}", report.path.display());
    Ok(())
}
