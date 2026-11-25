use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use super::AppError;
use super::interrupt::Interrupt;
use super::sort_candidates;
use crate::cli::{Format, RemovalArgs, Selection, SortKey};
use crate::config::{Config, DeleteConfig};
use crate::delete::{Batch, Confirmation, Executor, Mode, Removal, Risk, Status, confirm};
use crate::detect::Safety;
use crate::output;
use crate::scan::{Candidate, Issue};
use crate::source::Source;

pub struct Request<'a> {
    pub command: &'static str,
    pub policy: Option<&'a str>,
    pub roots: &'a [PathBuf],
    pub mode: Mode,
    pub yes: bool,
    pub confirm: bool,
    pub format: Format,
    pub source: Source,
    pub engines: &'a [String],
    pub delete: &'a DeleteConfig,
}

pub fn mode(args: &RemovalArgs, config: &Config) -> Mode {
    if args.dry_run {
        Mode::DryRun
    } else {
        config.delete.mode.into()
    }
}

pub fn check_selection(selections: &[Selection], known: &[&str]) -> Result<(), AppError> {
    for selection in selections {
        if let Selection::Rule(name) = selection
            && !known.contains(&name.as_str())
        {
            return Err(AppError::UnknownSelection {
                name: name.clone(),
                known: known.join(", "),
            });
        }
    }
    Ok(())
}

pub fn select(mut candidates: Vec<Candidate>, selections: &[Selection]) -> Vec<Candidate> {
    let selections: &[Selection] = if selections.is_empty() {
        &[Selection::Safe]
    } else {
        selections
    };
    candidates.retain(|candidate| {
        selections.iter().any(|selection| match selection {
            Selection::Safe => candidate.rule.safety == Safety::Safe,
            Selection::All => true,
            Selection::Rule(name) => &candidate.rule.name == name,
        })
    });
    sort_candidates(&mut candidates, SortKey::Size);
    candidates
}

fn ask(
    candidates: &[Candidate],
    confirmation: Confirmation,
    dirty: usize,
) -> Result<bool, AppError> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(AppError::NeedsConfirmation);
    }
    let count = candidates.len();
    let prompt = match confirmation {
        Confirmation::Skip => return Ok(true),
        Confirmation::YesNo if dirty == 0 => format!("Remove {count} items? [y/N] "),
        Confirmation::YesNo => format!(
            "Uncommitted changes or unknown state in {dirty} projects. Remove {count} items? [y/N] "
        ),
        Confirmation::TypeCount => format!(
            "{} of these are marked review. Type {count} to remove all {count} items: ",
            confirm::review_items(candidates)
        ),
    };
    let mut err = io::stderr().lock();
    let _ = write!(err, "{prompt}");
    let _ = err.flush();
    drop(err);
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    let answer = answer.trim();
    Ok(match confirmation {
        Confirmation::TypeCount => answer == count.to_string(),
        Confirmation::Skip | Confirmation::YesNo => matches!(answer, "y" | "Y" | "yes"),
    })
}

fn write_line(line: &str) -> Result<(), AppError> {
    let mut out = io::stdout().lock();
    writeln!(out, "{line}")?;
    Ok(())
}

fn print_report(
    request: &Request,
    mode: Mode,
    removals: &[Removal],
    issues: &[Issue],
) -> Result<(), AppError> {
    let (command, roots) = (request.command, request.roots);
    match request.format {
        Format::Text => {}
        Format::Json => println!(
            "{}",
            output::removal_report(command, mode, roots, removals, issues)?
        ),
        Format::JsonLines => write_line(&output::removal_summary_line(
            command, mode, roots, removals, issues,
        )?)?,
    }
    Ok(())
}

fn print_untouched(
    request: &Request,
    mode: Mode,
    candidates: Vec<Candidate>,
    issues: &[Issue],
) -> Result<(), AppError> {
    let removals: Vec<Removal> = candidates
        .into_iter()
        .map(|candidate| Removal {
            candidate,
            status: Status::WouldRemove,
            error: None,
        })
        .collect();
    if request.format == Format::JsonLines {
        for removal in &removals {
            write_line(&output::removal_line(request.command, removal)?)?;
        }
    }
    print_report(request, mode, &removals, issues)
}

/// @name run
/// @description Previews the selected candidates and removes them through the shared executor,
/// recording what was removed in the history file. Asks first unless `yes` is set. With `confirm`
/// off it still asks for `review` items and for projects with uncommitted changes or whose state
/// cannot be read. A dry run prints the preview and touches nothing. With `--json-lines` each
/// item is printed as one line once it is done, followed by one summary line.
///
/// @example
/// removal::run(&request, removal::select(candidates, &args.select), &issues)?;
pub fn run(
    request: &Request,
    candidates: Vec<Candidate>,
    issues: &[Issue],
) -> Result<(), AppError> {
    let mode = request.mode;
    let text = request.format == Format::Text;
    if text {
        if candidates.is_empty() {
            println!("Nothing to remove");
            return Ok(());
        }
        output::print_removal_plan(&candidates, mode);
    }
    if mode == Mode::DryRun || candidates.is_empty() {
        if text {
            println!("Dry run, nothing was removed");
        }
        return print_untouched(request, mode, candidates, issues);
    }
    let review = confirm::review_items(&candidates);
    let dirty = if request.yes || request.confirm || review > 0 {
        0
    } else {
        confirm::dirty_projects(&candidates)
    };
    let confirmation = if request.yes {
        Confirmation::Skip
    } else {
        Risk {
            review,
            dirty,
            unknown: false,
        }
        .confirmation(request.confirm)
    };
    if confirmation != Confirmation::Skip && !ask(&candidates, confirmation, dirty)? {
        if text {
            println!("Cancelled, nothing was removed");
        }
        return print_untouched(request, mode, candidates, issues);
    }

    let executor = Executor::new(request.roots, request.delete)
        .with_engines(request.engines)
        .with_policy(request.policy.map(str::to_owned));
    let interrupt = Interrupt::catch();
    let batch = Batch {
        source: request.source,
        roots: request.roots.to_vec(),
        candidates,
    };
    let unwritten: Mutex<Option<AppError>> = Mutex::new(None);
    let report = executor.run(vec![batch], mode, |_, removal| {
        match request.format {
            Format::Text => output::print_removal(removal),
            Format::Json => {}
            Format::JsonLines => {
                let written = output::removal_line(request.command, removal)
                    .map_err(AppError::from)
                    .and_then(|line| write_line(&line));
                if let Err(error) = written {
                    unwritten
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .get_or_insert(error);
                }
            }
        }
        if interrupt.requested() {
            executor.stop();
        }
    });
    if let Err(error) = executor.finish() {
        eprintln!("kuizer: {error}");
    }
    if let Some(error) = unwritten
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner)
    {
        return Err(error);
    }
    let failed = report.failed();
    let removals: Vec<Removal> = report
        .removals
        .into_iter()
        .map(|(_, removal)| removal)
        .collect();
    if text {
        output::print_removal_summary(&removals, mode);
    } else {
        print_report(request, mode, &removals, issues)?;
    }
    if report.left > 0 {
        return Err(AppError::Interrupted(report.left));
    }
    match failed {
        0 => Ok(()),
        failed => Err(AppError::RemovalFailed(failed)),
    }
}
