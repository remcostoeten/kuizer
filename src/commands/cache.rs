use std::path::PathBuf;
use std::sync::Arc;

use super::AppError;
use super::removal::{self, Request};
use crate::caches::{self, KnownCache, Locations};
use crate::cli::CacheArgs;
use crate::config::Config;
use crate::detect::{Category, Kind, RuleInfo};
use crate::output;
use crate::protect::Protected;
use crate::scan::{self, Cancel, Candidate};

/// @name measure
/// @description Measures every known global cache in parallel and returns the ones that hold
/// entries, largest first, each paired with a removal candidate.
///
/// @example
/// let entries = measure(caches::find(&locations, &Protected::for_user()));
pub fn measure(found: Vec<KnownCache>) -> Vec<(KnownCache, Candidate)> {
    let cancel = Cancel::new();
    let mut entries: Vec<(KnownCache, Candidate)> = std::thread::scope(|scope| {
        let handles: Vec<_> = found
            .into_iter()
            .enumerate()
            .map(|(index, cache)| {
                let cancel = &cancel;
                scope.spawn(move || {
                    let measured = scan::measure(&cache.path, cancel);
                    let candidate = Candidate {
                        id: index as u64 + 1,
                        path: cache.path.clone(),
                        kind: Kind::Dir,
                        rule: Arc::new(RuleInfo {
                            name: cache.name.to_owned(),
                            category: Category::Cache,
                            safety: cache.safety,
                            ecosystem: cache.tool.to_owned(),
                        }),
                        project: None,
                        usage: measured.usage,
                        newest: measured.newest,
                        identity: measured.identity,
                    };
                    (cache, candidate)
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect()
    });
    entries.retain(|(_, candidate)| candidate.usage.entries > 0);
    entries.sort_by_key(|(_, candidate)| std::cmp::Reverse(candidate.usage.allocated));
    entries
}

fn parents(candidates: &[Candidate]) -> Vec<PathBuf> {
    let mut parents: Vec<PathBuf> = candidates
        .iter()
        .filter_map(|candidate| candidate.path.parent().map(PathBuf::from))
        .collect();
    parents.sort();
    parents.dedup();
    parents
}

pub fn run(config: &Config, args: CacheArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    removal::check_selection(&args.removal.select, &caches::names().collect::<Vec<_>>())?;
    let locations = Locations::for_user().ok_or(AppError::NoHome)?;
    let entries = measure(caches::find(&locations, &Protected::for_user()));
    let cleaning = args.clean || args.removal.dry_run;
    if !cleaning {
        if json {
            println!("{}", output::cache_report(&entries));
        } else {
            output::print_caches(&entries, quiet);
        }
        return Ok(());
    }
    let candidates: Vec<Candidate> = entries
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect();
    let selected = removal::select(candidates, &args.removal.select);
    let roots = parents(&selected);
    let request = Request {
        command: "cache",
        roots: &roots,
        mode: removal::mode(&args.removal, config),
        yes: args.removal.yes,
        json,
    };
    removal::run(&request, selected, &[])
}
