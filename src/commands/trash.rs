use super::AppError;
use super::removal::{self, Request};
use crate::cli::{Format, TrashArgs};
use crate::config::Config;
use crate::output;
use crate::scan::{Cancel, Candidate, Issue};
use crate::source::Source;
use crate::trashcan;

pub fn run(config: &Config, args: TrashArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    let cleaning = args.clean || args.removal.dry_run;
    removal::check_selection(&args.removal.select, &["trash"])?;

    let mut folders = Vec::new();
    let mut issues: Vec<Issue> = Vec::new();
    if let Some((path, layout)) = trashcan::home_trash() {
        match trashcan::inspect(&path, layout) {
            Ok(Some(folder)) => folders.push(folder),
            Ok(None) => {}
            Err(issue) => issues.push(issue),
        }
    }
    let entries = trashcan::entries(folders, &Cancel::new());

    if !cleaning {
        if json {
            println!("{}", output::trash::trash_report(&entries, &issues)?);
        } else {
            output::trash::print_trash(&entries, &issues, quiet);
        }
        return Ok(());
    }

    if !json {
        for issue in &issues {
            eprintln!("kuizer: {}", issue.message);
        }
    }
    let roots: Vec<_> = entries
        .iter()
        .flat_map(|(folder, _)| trashcan::roots(folder))
        .collect();
    let candidates: Vec<Candidate> = entries
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect();
    let selected = removal::select(candidates, &args.removal.select);
    let format = if json { Format::Json } else { Format::Text };
    let request = Request {
        command: "trash",
        policy: None,
        roots: &roots,
        mode: Source::Trash.mode(removal::mode(&args.removal, config)),
        yes: args.removal.yes,
        confirm: config.delete.confirm,
        format,
        source: Source::Trash,
        engines: &config.docker.engines,
        delete: &config.delete,
    };
    removal::run(&request, selected, &issues)
}
