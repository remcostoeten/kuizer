use std::path::PathBuf;

use super::AppError;
use super::removal::{self, Request};
use crate::caches::{self, Locations};
use crate::cli::CacheArgs;
use crate::config::Config;
use crate::output;
use crate::protect::Protected;
use crate::scan::{self, Cancel, Candidate};

pub use crate::caches::{measure, measure_threaded, measure_with};

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
    let plan = args.plan();
    removal::check_selection(&plan.removal.select, &caches::names().collect::<Vec<_>>())?;
    let locations = Locations::for_user().ok_or(AppError::NoHome)?;
    let entries = measure_threaded(
        caches::find(&locations, &Protected::for_user()),
        &Cancel::new(),
        scan::size_threads(config),
        |_| {},
    );
    if !plan.cleaning {
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
    let selected = removal::select(candidates, &plan.removal.select);
    let roots = parents(&selected);
    let request = Request {
        command: "cache",
        roots: &roots,
        mode: removal::mode(&plan.removal, config),
        yes: plan.removal.yes,
        confirm: config.delete.confirm,
        json,
        remove: &removal::files,
        delete: &config.delete,
    };
    removal::run(&request, selected, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caches::KnownCache;
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
