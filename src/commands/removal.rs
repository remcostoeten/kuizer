use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;

use super::AppError;
use crate::cli::{RemovalArgs, Selection};
use crate::config::Config;
use crate::delete::{self, Guard, Mode, Removal, Status, Target};
use crate::detect::Safety;
use crate::history::{self, Entry};
use crate::output;
use crate::scan::{Candidate, Issue};

pub struct Request<'a> {
    pub command: &'static str,
    pub roots: &'a [PathBuf],
    pub mode: Mode,
    pub yes: bool,
    pub json: bool,
}

pub fn mode(args: &RemovalArgs, config: &Config) -> Mode {
    if args.dry_run {
        Mode::DryRun
    } else if args.permanent {
        Mode::Permanent
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
    candidates.sort_by(|a, b| {
        b.usage
            .allocated
            .cmp(&a.usage.allocated)
            .then_with(|| a.path.cmp(&b.path))
    });
    candidates
}

fn confirm(candidates: &[Candidate]) -> Result<bool, AppError> {
    if !io::stdin().is_terminal() {
        return Err(AppError::NeedsConfirmation);
    }
    let count = candidates.len();
    let review = candidates
        .iter()
        .filter(|candidate| candidate.rule.safety == Safety::Review)
        .count();
    let prompt = match review {
        0 => format!("Remove {count} items? [y/N] "),
        review => format!(
            "{review} of these are marked review. Type {count} to remove all {count} items: "
        ),
    };
    let mut err = io::stderr().lock();
    let _ = write!(err, "{prompt}");
    let _ = err.flush();
    drop(err);
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    let answer = answer.trim();
    Ok(match review {
        0 => matches!(answer, "y" | "Y" | "yes"),
        _ => answer == count.to_string(),
    })
}

fn target(candidate: &Candidate) -> Target {
    Target {
        path: candidate.path.clone(),
        identity: candidate.identity,
    }
}

/// @name run
/// @description Previews the selected candidates, asks for confirmation unless `yes` is set,
/// then removes each one through the delete guard and records what was removed in the history
/// file. A dry run prints the preview and touches nothing.
///
/// @example
/// removal::run(&request, removal::select(candidates, &args.select), &issues)?;
pub fn run(
    request: &Request,
    candidates: Vec<Candidate>,
    issues: &[Issue],
) -> Result<(), AppError> {
    let mode = request.mode;
    if !request.json {
        if candidates.is_empty() {
            println!("Nothing to remove");
            return Ok(());
        }
        output::print_removal_plan(&candidates, mode);
    }
    let pending = |candidates: Vec<Candidate>| -> Vec<Removal> {
        candidates
            .into_iter()
            .map(|candidate| Removal {
                candidate,
                status: Status::WouldRemove,
                error: None,
            })
            .collect()
    };
    if mode == Mode::DryRun || candidates.is_empty() {
        if request.json {
            println!(
                "{}",
                output::removal_report(
                    request.command,
                    mode,
                    request.roots,
                    &pending(candidates),
                    issues
                )
            );
        } else {
            println!("Dry run, nothing was removed");
        }
        return Ok(());
    }
    if !request.yes && !confirm(&candidates)? {
        if request.json {
            println!(
                "{}",
                output::removal_report(
                    request.command,
                    mode,
                    request.roots,
                    &pending(candidates),
                    issues
                )
            );
        } else {
            println!("Cancelled, nothing was removed");
        }
        return Ok(());
    }

    let started_at = history::now();
    let guard = Guard::new(request.roots);
    let mut removals = Vec::with_capacity(candidates.len());
    let mut deleted = Vec::new();
    for candidate in candidates {
        let removal = match delete::delete(&guard, &target(&candidate), mode) {
            Ok(()) => {
                deleted.push(Entry::new(&candidate, mode));
                Removal {
                    candidate,
                    status: Status::Removed,
                    error: None,
                }
            }
            Err(error) => Removal {
                candidate,
                status: Status::Failed,
                error: Some(error.to_string()),
            },
        };
        if !request.json {
            output::print_removal(&removal);
        }
        removals.push(removal);
    }
    if let Err(error) = history::save(started_at, request.roots, deleted) {
        eprintln!("kuizer: {error}");
    }
    if request.json {
        println!(
            "{}",
            output::removal_report(request.command, mode, request.roots, &removals, issues)
        );
    } else {
        output::print_removal_summary(&removals, mode);
    }
    match removals
        .iter()
        .filter(|removal| removal.status == Status::Failed)
        .count()
    {
        0 => Ok(()),
        failed => Err(AppError::RemovalFailed(failed)),
    }
}
