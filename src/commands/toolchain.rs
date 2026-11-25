use std::path::Path;

use super::AppError;
use super::filter::Filters;
use super::removal::{self, Request};
use super::scan::resolve_roots;
use crate::cli::{Format, ToolchainAction, ToolchainArgs};
use crate::config::Config;
use crate::output;
use crate::scan::{self, Cancel, Candidate};
use crate::source::Source;
use crate::toolchains::{self, Locations, Tool, pins};

fn clean_may_mean_a_folder(args: &ToolchainArgs) -> bool {
    matches!(args.command, Some(ToolchainAction::Clean(_))) && Path::new("clean").exists()
}

pub fn run(
    config: &Config,
    args: ToolchainArgs,
    format: Format,
    quiet: bool,
) -> Result<(), AppError> {
    let json = format != Format::Text;
    let plan = args.plan();
    let names: Vec<&str> = Tool::ALL.iter().map(|tool| tool.name()).collect();
    removal::check_selection(&plan.removal.select, &names)?;
    let filters = Filters::source(config, &args.filters, &names)?;
    let config = filters.config();
    let locations = Locations::for_user().ok_or(AppError::NoHome)?;
    let mut found =
        toolchains::find_excluding(&locations, &config.toolchains.tools, &config.excludes()?);
    if config.toolchains.pins && !found.is_empty() {
        let roots = resolve_roots(config, args.pin_paths())?;
        if !quiet && !json {
            output::print_status("looking for pinned versions");
        }
        let pins = pins::find(filters.evidence(), &roots, &Cancel::new())?;
        if !quiet && !json {
            output::clear_progress();
        }
        toolchains::apply_pins(&mut found, &pins);
    }
    let mut entries =
        toolchains::measure_with_threads(found, &Cancel::new(), scan::size_threads(config), |_| {});
    filters.keep(&mut entries);
    if !plan.cleaning {
        if format == Format::JsonLines {
            println!("{}", output::toolchain_lines(&entries)?);
        } else if json {
            println!("{}", output::toolchain_report(&entries)?);
        } else {
            output::print_toolchains(&entries, quiet);
        }
        return Ok(());
    }
    let candidates: Vec<Candidate> = entries
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect();
    let selected = removal::select(candidates, &plan.removal.select);
    let roots = toolchains::roots(&selected);
    let request = Request {
        command: "toolchain",
        roots: &roots,
        mode: Source::Toolchain.mode(removal::mode(&plan.removal, config)),
        yes: plan.removal.yes,
        confirm: config.delete.confirm || clean_may_mean_a_folder(&args),
        json,
        source: Source::Toolchain,
        engines: &config.docker.engines,
        delete: &config.delete,
    };
    removal::run(&request, selected, &[])
}
