use super::AppError;
use super::removal::{self, Request};
use super::scan::resolve_roots;
use crate::cli::ToolchainArgs;
use crate::config::Config;
use crate::delete::Mode;
use crate::output;
use crate::scan::{Cancel, Candidate};
use crate::toolchains::{self, Locations, Tool, pins};

pub fn run(config: &Config, args: ToolchainArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    let names: Vec<&str> = Tool::ALL.iter().map(|tool| tool.name()).collect();
    removal::check_selection(&args.removal.select, &names)?;
    let locations = Locations::for_user().ok_or(AppError::NoHome)?;
    let mut found = toolchains::find(&locations, &config.toolchains.tools);
    if config.toolchains.pins && !found.is_empty() {
        let roots = resolve_roots(config, args.paths)?;
        if !quiet && !json {
            output::print_status("looking for pinned versions");
        }
        let pins = pins::find(config, &roots, &Cancel::new())?;
        if !quiet && !json {
            output::clear_progress();
        }
        toolchains::apply_pins(&mut found, &pins);
    }
    let entries = toolchains::measure(found);
    let cleaning = args.clean || args.removal.dry_run;
    if !cleaning {
        if json {
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
    let selected = removal::select(candidates, &args.removal.select);
    let roots = toolchains::roots(&selected);
    let request = Request {
        command: "toolchain",
        roots: &roots,
        mode: if args.removal.dry_run {
            Mode::DryRun
        } else {
            Mode::Permanent
        },
        yes: args.removal.yes,
        json,
        remove: toolchains::remove,
    };
    removal::run(&request, selected, &[])
}
