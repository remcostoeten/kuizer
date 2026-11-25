use std::io::IsTerminal;

use super::AppError;
use crate::cli::SystemArgs;
use crate::config::Config;
use crate::output;
use crate::system;

pub fn run(config: &Config, _args: SystemArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    let show_progress = !quiet && !json && std::io::stderr().is_terminal();
    if show_progress {
        output::print_status("asking the package tools");
    }
    let listing = system::for_user(&config.system);
    if show_progress {
        output::clear_progress();
    }
    let entries = system::entries(listing.items);
    if json {
        println!(
            "{}",
            output::system::system_report(&entries, &listing.issues)?
        );
    } else {
        output::system::print_system(&entries, &listing.issues, quiet);
    }
    Ok(())
}
