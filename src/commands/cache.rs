use super::AppError;
use super::filter::Filters;
use super::removal::{self, Request};
use crate::caches::{self, Locations};
use crate::cli::{CacheArgs, Format};
use crate::config::Config;
use crate::output;
use crate::protect::Protected;
use crate::scan::{self, Cancel, Candidate};
use crate::source::Source;

pub use crate::caches::{measure, measure_threaded, measure_with};

pub fn run(config: &Config, args: CacheArgs, format: Format, quiet: bool) -> Result<(), AppError> {
    let json = format != Format::Text;
    let plan = args.plan();
    let names: Vec<&str> = caches::names().collect();
    removal::check_selection(&plan.removal.select, &names)?;
    let filters = Filters::source(config, &args.filters, &names)?;
    let config = filters.config();
    let locations = Locations::for_user().ok_or(AppError::NoHome)?;
    let mut entries = measure_threaded(
        caches::find_excluding(&locations, &Protected::for_user(), &config.excludes()?),
        &Cancel::new(),
        scan::size_threads(config),
        |_| {},
    );
    filters.keep(&mut entries);
    if !plan.cleaning {
        if format == Format::JsonLines {
            println!("{}", output::cache_lines(&entries)?);
        } else if json {
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
    let roots = caches::guard_roots(&selected);
    let request = Request {
        command: "cache",
        roots: &roots,
        mode: removal::mode(&plan.removal, config),
        yes: plan.removal.yes,
        confirm: config.delete.confirm,
        format,
        source: Source::Cache,
        engines: &config.docker.engines,
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
