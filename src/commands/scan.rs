use std::io::IsTerminal;
use std::path::PathBuf;

use super::AppError;
use crate::cli::{ScanArgs, SortKey};
use crate::config::Config;
use crate::detect::Safety;
use crate::output;
use crate::scan::{self, Cancel, Candidate};
use crate::tui;

pub fn sort_candidates(candidates: &mut [Candidate], key: SortKey) {
    match key {
        SortKey::Size => candidates.sort_by(|a, b| {
            b.usage
                .allocated
                .cmp(&a.usage.allocated)
                .then_with(|| a.path.cmp(&b.path))
        }),
        SortKey::Path => candidates.sort_by(|a, b| a.path.cmp(&b.path)),
        SortKey::Category => candidates.sort_by(|a, b| {
            a.rule
                .category
                .cmp(&b.rule.category)
                .then_with(|| b.usage.allocated.cmp(&a.usage.allocated))
        }),
    }
}

pub fn resolve_roots(config: &Config, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, AppError> {
    if !paths.is_empty() {
        return Ok(paths);
    }
    if !config.scan.roots.is_empty() {
        return Ok(config.scan.roots.clone());
    }
    Ok(vec![std::env::current_dir()?])
}

pub fn run(config: &Config, args: ScanArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    let mut config = config.clone();
    if !args.categories.is_empty() {
        config.detect.categories = args.categories;
    }
    if let Some(min_size) = args.min_size {
        config.scan.min_size = min_size;
    }
    let roots = scan::normalize_roots(&resolve_roots(&config, args.paths)?);
    let handle = scan::start(&config, &roots, Cancel::new())?;
    let terminal = std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal()
        && std::io::stdin().is_terminal();
    let live = !json && !args.no_live && terminal;
    let show_progress = !json && !quiet && std::io::stderr().is_terminal();
    let safe_only = args.safe_only;
    let collected = if live {
        tui::watch(&handle, safe_only)?
    } else {
        let collected = scan::collect(&handle, |progress| {
            if show_progress {
                output::print_progress(progress);
            }
        });
        if show_progress {
            output::clear_progress();
        }
        collected
    };

    let mut candidates = collected.candidates;
    if safe_only {
        candidates.retain(|candidate| candidate.rule.safety == Safety::Safe);
    }

    if live {
        output::print_overview(
            &candidates,
            &collected.issues,
            &collected.summary,
            output::largest_count(args.limit),
        );
        return Ok(());
    }

    sort_candidates(&mut candidates, args.sort);
    if let Some(limit) = args.limit {
        candidates.truncate(limit);
    }
    if json {
        println!(
            "{}",
            output::scan_report(&roots, &candidates, &collected.issues, &collected.summary)
        );
    } else {
        output::print_scan(&candidates, &collected.issues, &collected.summary, quiet);
    }
    Ok(())
}
