use std::io::{self, BufRead, IsTerminal, Write};

use super::AppError;
use super::scan::resolve_roots;
use crate::cli::GitArgs;
use crate::config::Config;
use crate::output::{self, GitOutcome};
use crate::repos::{self, RepoError, Repository};
use crate::scan::Cancel;
use crate::units::ByteSize;

fn confirm(prompt: &str) -> Result<bool, AppError> {
    if !io::stdin().is_terminal() {
        return Err(AppError::NeedsConfirmation);
    }
    let mut err = io::stderr().lock();
    let _ = write!(err, "{prompt} Neither can be undone from the trash. [y/N] ");
    let _ = err.flush();
    drop(err);
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

fn failure(error: &RepoError) -> bool {
    !matches!(error, RepoError::Busy(_))
}

fn plan(found: &[Repository], args: &GitArgs, min_gain: u64) -> String {
    let ready = found.iter().filter(|repository| repository.busy.is_none());
    let mut parts = Vec::new();
    if args.gc {
        let collectable: Vec<&Repository> = ready
            .clone()
            .filter(|repository| repository.collectable(min_gain))
            .collect();
        let gain: u64 = collectable.iter().map(|r| r.objects.gain()).sum();
        parts.push(format!(
            "run git gc in {} repositories, freeing up to {}",
            collectable.len(),
            ByteSize(gain)
        ));
    }
    if args.branches {
        let branches: usize = ready.map(|r| r.merged.len()).sum();
        parts.push(format!("delete {branches} merged local branches"));
    }
    let mut text = parts.join(" and ");
    if let Some(first) = text.get(..1) {
        text.replace_range(..1, &first.to_uppercase());
    }
    format!("{text}?")
}

pub fn run(config: &Config, args: GitArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    let roots = resolve_roots(config, args.paths.clone())?;
    let found = repos::inspect_all(repos::find(config, &roots)?, &config.git, &Cancel::new());
    let acting = (args.gc || args.branches) && !args.dry_run;
    if !acting {
        if json {
            println!("{}", output::git_report(&found, None)?);
        } else {
            output::print_repositories(&found, quiet);
            if args.dry_run && (args.gc || args.branches) && !found.is_empty() {
                println!(
                    "\nDry run: {}",
                    plan(&found, &args, config.git.min_gain.0).trim_end_matches('?')
                );
            }
        }
        return Ok(());
    }
    if found.is_empty() {
        if json {
            println!("{}", output::git_report(&found, Some(&[]))?);
        } else {
            println!("Nothing to clean");
        }
        return Ok(());
    }
    if !json {
        output::print_repositories(&found, true);
        println!();
    }
    if !args.yes && !confirm(&plan(&found, &args, config.git.min_gain.0))? {
        println!("Nothing changed");
        return Ok(());
    }
    let outcomes: Vec<GitOutcome> = found
        .iter()
        .map(|repository| {
            let pruned = if args.branches && !repository.merged.is_empty() {
                repos::delete_branches(repository, &repository.merged, &config.git)
            } else {
                Vec::new()
            };
            let collected = (args.gc && repository.collectable(config.git.min_gain.0))
                .then(|| repos::collect_garbage(repository, &config.git));
            let outcome = GitOutcome {
                repository,
                collected,
                pruned,
            };
            if !json {
                output::print_git_outcome(&outcome);
            }
            outcome
        })
        .collect();
    let failed = outcomes
        .iter()
        .map(|outcome| {
            usize::from(matches!(&outcome.collected, Some(Err(error)) if failure(error)))
                + outcome
                    .pruned
                    .iter()
                    .filter(|p| p.result.as_ref().is_err_and(failure))
                    .count()
        })
        .sum();
    if json {
        println!("{}", output::git_report(&found, Some(&outcomes))?);
    } else if !quiet {
        let freed: u64 = outcomes
            .iter()
            .filter_map(|outcome| outcome.collected.as_ref()?.as_ref().ok())
            .map(|collected| collected.freed())
            .sum();
        let deleted: usize = outcomes
            .iter()
            .map(|outcome| outcome.pruned.iter().filter(|p| p.result.is_ok()).count())
            .sum();
        println!("\nFreed {}, deleted {deleted} branches", ByteSize(freed));
    }
    match failed {
        0 => Ok(()),
        failed => Err(AppError::GitFailed(failed)),
    }
}
