use std::io::IsTerminal;

use super::AppError;
use super::removal::{self, Request};
use super::scan::{keep_idle, resolve_roots};
use crate::cli::CleanArgs;
use crate::config::Config;
use crate::output;
use crate::scan::{self, Cancel};

pub fn run(config: &Config, args: CleanArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    let mut config = config.clone();
    if !args.categories.is_empty() {
        config.detect.categories = args.categories;
    }
    if let Some(min_size) = args.min_size {
        config.scan.min_size = min_size;
    }
    let specs = config.rule_specs()?;
    let names: Vec<&str> = specs.iter().map(|spec| spec.name.as_str()).collect();
    removal::check_selection(&args.removal.select, &names)?;
    let roots = scan::normalize_roots(&resolve_roots(&config, args.paths)?);
    let handle = scan::start(&config, &roots, Cancel::new())?;
    let show_progress = !json && !quiet && std::io::stderr().is_terminal();
    let collected = scan::collect(&handle, |progress| {
        if show_progress {
            output::print_progress(progress);
        }
    });
    if show_progress {
        output::clear_progress();
    }
    let request = Request {
        command: "clean",
        roots: &roots,
        mode: removal::mode(&args.removal, &config),
        yes: args.removal.yes,
        json,
        remove: removal::files,
    };
    let mut candidates = collected.candidates;
    keep_idle(&config, &mut candidates, args.idle, false)?;
    let selected = removal::select(candidates, &args.removal.select);
    removal::run(&request, selected, &collected.issues)
}
