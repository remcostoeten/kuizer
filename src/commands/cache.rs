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
/// @description Measures every known global cache and returns the ones that hold entries,
/// largest first, each paired with a removal candidate.
///
/// @example
/// let entries = measure(caches::find(&locations, &Protected::for_user()));
pub fn measure(found: Vec<KnownCache>) -> Vec<(KnownCache, Candidate)> {
    measure_with(found, &Cancel::new(), |_| {})
}

/// @name measure_with
/// @description Like `measure`, but stops sizing once `cancel` is set and calls `on_measured`
/// as each cache is done, so a frontend can show which cache was measured and how many are left.
///
/// @example
/// let entries = measure_with(found, &cancel, |candidate| println!("{}", candidate.path.display()));
pub fn measure_with(
    found: Vec<KnownCache>,
    cancel: &Cancel,
    on_measured: impl Fn(&Candidate) + Sync,
) -> Vec<(KnownCache, Candidate)> {
    measure_threaded(
        found,
        cancel,
        scan::size_threads(&Config::default()),
        on_measured,
    )
}

/// @name measure_threaded
/// @description Like `measure_with`, with the number of sizing threads given, for example
/// `scan::size_threads(&config)` to follow `scan.size_threads`. The caches are sized one after
/// another and each one is spread over all the threads, so one huge cache no longer decides how
/// long the rest waits.
///
/// @example
/// let entries = measure_threaded(found, &Cancel::new(), scan::size_threads(&config), |_| {});
pub fn measure_threaded(
    found: Vec<KnownCache>,
    cancel: &Cancel,
    threads: usize,
    on_measured: impl Fn(&Candidate) + Sync,
) -> Vec<(KnownCache, Candidate)> {
    let mut entries: Vec<(KnownCache, Candidate)> = found
        .into_iter()
        .enumerate()
        .map(|(index, cache)| {
            let measured = scan::measure_parallel(&cache.path, cancel, threads);
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
                project_last_active: None,
            };
            on_measured(&candidate);
            (cache, candidate)
        })
        .collect();
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
    let entries = measure_threaded(
        caches::find(&locations, &Protected::for_user()),
        &Cancel::new(),
        scan::size_threads(config),
        |_| {},
    );
    let cleaning = args.clean || args.removal.dry_run;
    if !cleaning {
        if json {
            println!("{}", output::cache_report(&entries)?);
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
        remove: removal::files,
    };
    removal::run(&request, selected, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::Safety;

    #[test]
    fn measure_with_reports_each_cache_and_drops_empty_ones() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let full = tmp.path().join("full");
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&full).expect("full");
        std::fs::create_dir_all(&empty).expect("empty");
        std::fs::write(full.join("blob"), vec![0u8; 8192]).expect("blob");
        let cache = |path: &std::path::Path| KnownCache {
            name: "test-cache",
            tool: "test",
            path: path.to_path_buf(),
            safety: Safety::Safe,
            note: "",
        };
        let seen = std::sync::Mutex::new(Vec::new());
        let entries = measure_with(vec![cache(&full), cache(&empty)], &Cancel::new(), |c| {
            seen.lock().expect("seen").push(c.path.clone());
        });
        let mut seen = seen.into_inner().expect("seen");
        seen.sort();
        assert_eq!(seen, [empty, full.clone()]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.path, full);
    }
}
