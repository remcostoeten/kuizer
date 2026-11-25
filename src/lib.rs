pub mod caches;
#[cfg(feature = "cli")]
pub(crate) mod cli;
#[cfg(feature = "cli")]
pub(crate) mod commands;
pub mod config;
pub mod delete;
pub mod detect;
pub mod docker;
pub mod dupes;
pub mod git;
pub mod history;
pub mod keymap;
#[cfg(feature = "cli")]
pub(crate) mod output;
pub mod platform;
mod process;
pub mod protect;
pub mod repos;
pub mod scan;
pub mod snapshot;
pub mod source;
pub mod stats;
pub mod toolchains;
#[cfg(feature = "cli")]
pub(crate) mod tui;
pub mod units;
pub mod update;

/// @name run_cli
/// @description Parses command-line arguments and runs the requested CLI or TUI command.
///
/// @example
/// fn main() -> std::process::ExitCode {
///     kuizer::run_cli()
/// }
#[cfg(feature = "cli")]
pub fn run_cli() -> std::process::ExitCode {
    use clap::Parser;
    match commands::run(cli::Cli::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kuizer: {error}");
            error.exit_code()
        }
    }
}
