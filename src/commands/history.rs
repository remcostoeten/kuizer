use std::collections::HashSet;

use serde::Serialize;

use super::AppError;
use crate::cli::{HistoryAction, HistoryArgs};
use crate::delete::Mode;
use crate::history::{self, Action, Entry, History, Run, Total};
use crate::units::ByteSize;

#[derive(Serialize)]
struct Report<'a> {
    #[serde(flatten)]
    history: &'a History,
    in_trash: Option<Total>,
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

struct Trash {
    kept: HashSet<(usize, usize)>,
    known: bool,
}

impl Trash {
    fn read(history: &History) -> Self {
        let trashed = history::trashed_paths();
        Self {
            kept: trashed
                .as_deref()
                .map(|paths| history::still_in_trash(history, paths))
                .unwrap_or_default(),
            known: trashed.is_some(),
        }
    }

    fn state(
        &self,
        history: &History,
        restored: &HashSet<&str>,
        run: usize,
        entry: usize,
    ) -> &'static str {
        let item = &history.runs[run].deleted[entry];
        if history.runs[run].action == Action::Restore {
            return "restored here";
        }
        if restored.contains(history.item_id(run, entry).as_str()) {
            return "restored";
        }
        match (item.mode, self.known) {
            (Mode::Permanent, _) => "deleted",
            (Mode::DryRun, _) => "dry run",
            (Mode::Trash, false) => "trashed",
            (Mode::Trash, true) if self.kept.contains(&(run, entry)) => "in trash",
            (Mode::Trash, true) => "left trash",
        }
    }
}

pub fn run(args: HistoryArgs, json: bool) -> Result<(), AppError> {
    let path = history::default_path()?;
    let history = history::load(&path)?;
    let trash = Trash::read(&history);
    if let Some(HistoryAction::Show { run }) = args.command {
        return show(&history, &trash, &run, json);
    }
    let in_trash = trash.known.then(|| kept_total(&history, &trash.kept));
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
        Some(trash) if trash.items > 0 => println!(
            "{} of that is still in the trash across {} items, `kuizer restore ID` puts it back",
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

fn trash_column(history: &History, trash: &Trash, run_index: usize) -> String {
    let run = &history.runs[run_index];
    if run.action == Action::Restore {
        return "restore".to_owned();
    }
    let trashed = run
        .deleted
        .iter()
        .filter(|entry| entry.mode == Mode::Trash)
        .count();
    if trashed == 0 {
        return "-".to_owned();
    }
    if !trash.known {
        return "unknown".to_owned();
    }
    let kept = (0..run.deleted.len())
        .filter(|&entry| trash.kept.contains(&(run_index, entry)))
        .count();
    if kept == 0 {
        "emptied".to_owned()
    } else {
        format!("{kept} kept")
    }
}

fn print_runs(history: &History, trash: &Trash) {
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
            run.command
        );
    }
}

fn show(history: &History, trash: &Trash, id: &str, json: bool) -> Result<(), AppError> {
    let run_index = history
        .runs
        .iter()
        .position(|run| run.session() == id)
        .ok_or_else(|| AppError::UnknownRun(id.to_owned()))?;
    let run = &history.runs[run_index];
    let restored = history.restored_ids();
    let items: Vec<ItemJson> = run
        .deleted
        .iter()
        .enumerate()
        .map(|(entry_index, entry)| ItemJson {
            id: history.item_id(run_index, entry_index),
            state: trash.state(history, &restored, run_index, entry_index),
            entry,
        })
        .collect();
    if json {
        let report = ShowReport {
            id: run.session(),
            run,
            items,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
        return Ok(());
    }
    println!(
        "{}  {}  {} items  {}",
        run.session(),
        run.started_at,
        run.total.items,
        run.total.size
    );
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
            item.entry.path.display()
        );
    }
    Ok(())
}
