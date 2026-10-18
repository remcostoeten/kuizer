mod cache;
mod clean;
mod config;
mod dashboard;
mod docker;
mod duplicates;
mod git;
mod history;
mod removal;
mod restore;
mod rules;
mod scan;
mod toolchain;
mod ui;

use std::path::PathBuf;

use crate::cli::{Cli, Command, HistoryAction, NestedCommand, ScanArgs};
use crate::config::{Config, ConfigError};
use crate::history::HistoryError;
use clap::CommandFactory;

pub use cache::{measure as measure_caches, measure_until as measure_caches_until};
pub use scan::sort_candidates;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("unknown selection `{name}`, expected safe, all or one of: {known}")]
    UnknownSelection { name: String, known: String },
    #[error("refusing to remove without confirmation: stdin is not a terminal, pass --yes")]
    NeedsConfirmation,
    #[error("{0} items could not be removed")]
    RemovalFailed(usize),
    #[error("no home directory is known for this platform")]
    NoHome,
    #[error("{0} already exists, pass --force to overwrite it")]
    PageExists(PathBuf),
    #[error("config file already exists at {0}")]
    ConfigExists(PathBuf),
    #[error("no config directory is known for this platform")]
    NoConfigDir,
    #[error("no run or item `{0}` in the history, `kuizer history` lists the ids")]
    UnknownRun(String),
    #[error("restore needs a trash kuizer can read; on macOS use Put Back in Finder")]
    RestoreUnsupported,
    #[error("the history has no run with items to restore")]
    NothingToRestore,
    #[error("cannot read the trash: {0}")]
    Trash(String),
    #[error("{0} items could not be restored")]
    RestoreFailed(usize),
    #[error("{0} git actions failed")]
    GitFailed(usize),
}

fn interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::io::stdin().is_terminal()
}

fn nested_help(command: &Command) -> Option<&'static str> {
    match command {
        Command::Scan(args) if args.command == Some(NestedCommand::Help) => Some("scan"),
        Command::Duplicates(args) if args.command == Some(NestedCommand::Help) => {
            Some("duplicates")
        }
        Command::Clean(args) if args.command == Some(NestedCommand::Help) => Some("clean"),
        Command::Cache(args) if args.command == Some(NestedCommand::Help) => Some("cache"),
        Command::Toolchain(args) if args.command == Some(NestedCommand::Help) => Some("toolchain"),
        Command::Docker(args) if args.command == Some(NestedCommand::Help) => Some("docker"),
        Command::Git(args) if args.command == Some(NestedCommand::Help) => Some("git"),
        Command::Rules(args) if args.command == Some(NestedCommand::Help) => Some("rules"),
        Command::History(args) if args.command == Some(HistoryAction::Help) => Some("history"),
        Command::Restore(args) if args.command == Some(NestedCommand::Help) => Some("restore"),
        Command::Dashboard(args) if args.command == Some(NestedCommand::Help) => Some("dashboard"),
        _ => None,
    }
}

fn print_subcommand_help(name: &str) -> Result<(), AppError> {
    let mut command = Cli::command();
    command
        .find_subcommand_mut(name)
        .expect("nested help target must be a CLI subcommand")
        .print_help()?;
    println!();
    Ok(())
}

pub fn run(cli: Cli) -> Result<(), AppError> {
    if let Some(name) = cli.command.as_ref().and_then(nested_help) {
        return print_subcommand_help(name);
    }
    let loaded = Config::load(cli.config.as_deref())?;
    match cli.command {
        None if !cli.json && interactive() => ui::run(&loaded.config, cli.paths, cli.confirm),
        None => {
            let args = ScanArgs {
                paths: cli.paths,
                ..ScanArgs::default()
            };
            scan::run(&loaded.config, args, cli.json, cli.quiet)
        }
        Some(Command::Scan(args)) => scan::run(&loaded.config, args, cli.json, cli.quiet),
        Some(Command::Config { action }) => config::run(&loaded, action, cli.json),
        Some(Command::Rules(_)) => rules::run(&loaded.config, cli.json),
        Some(Command::History(args)) => history::run(args, cli.json),
        Some(Command::Restore(args)) => restore::run(args, cli.json),
        Some(Command::Duplicates(args)) => {
            duplicates::run(&loaded.config, args, cli.json, cli.quiet)
        }
        Some(Command::Clean(args)) => clean::run(&loaded.config, args, cli.json, cli.quiet),
        Some(Command::Cache(args)) => cache::run(&loaded.config, args, cli.json, cli.quiet),
        Some(Command::Docker(args)) => docker::run(&loaded.config, args, cli.json, cli.quiet),
        Some(Command::Toolchain(args)) => toolchain::run(&loaded.config, args, cli.json, cli.quiet),
        Some(Command::Git(args)) => git::run(&loaded.config, args, cli.json, cli.quiet),
        Some(Command::Dashboard(args)) => dashboard::run(&loaded.config, args, cli.json, cli.quiet),
    }
}
