use std::process::ExitCode;

use clap::Parser;

use kuizer::cli::Cli;
use kuizer::commands;

fn main() -> ExitCode {
    match commands::run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kuizer: {error}");
            ExitCode::FAILURE
        }
    }
}
