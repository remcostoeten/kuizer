use std::io::IsTerminal;

use super::AppError;
use super::filter::{self, Filters};
use super::removal::{self, Request};
use super::scan::{keep_idle, resolve_roots};
use crate::cli::{CleanArgs, Format};
use crate::config::Config;
use crate::output;
use crate::scan::{self, Cancel};
use crate::source::Source;

pub fn run(config: &Config, args: CleanArgs, format: Format, quiet: bool) -> Result<(), AppError> {
    let rules = filter::project_rules(config);
    let names: Vec<&str> = rules.iter().map(String::as_str).collect();
    let filters = Filters::new(config, &args.categories, args.min_size, &args.scope, &names)?;
    let config = filters.config();
    let specs = config.rule_specs()?;
    let names: Vec<&str> = specs.iter().map(|spec| spec.name.as_str()).collect();
    removal::check_selection(&args.removal.select, &names)?;
    let roots = scan::normalize_roots(&resolve_roots(config, args.paths)?);
    let handle = scan::start(config, &roots, Cancel::new())?;
    let show_progress = format == Format::Text && !quiet && std::io::stderr().is_terminal();
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
        mode: removal::mode(&args.removal, config),
        yes: args.removal.yes,
        confirm: config.delete.confirm,
        format,
        source: Source::Artifact,
        engines: &config.docker.engines,
        delete: &config.delete,
    };
    let mut candidates = collected.candidates;
    candidates.retain(|candidate| filters.matches(candidate));
    keep_idle(filters.evidence(), &mut candidates, args.idle, false)?;
    let selected = removal::select(candidates, &args.removal.select);
    removal::run(&request, selected, &collected.issues)
}
