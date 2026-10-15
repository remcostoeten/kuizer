mod cache;
mod clean;
mod config;
mod duplicates;
mod insight;
mod removal;
mod rules;
mod scan;
mod ui;

use std::path::PathBuf;

use crate::cli::{Cli, Command, NestedCommand, ScanArgs};
use crate::config::{Config, ConfigError};
use crate::history::HistoryError;
use clap::CommandFactory;

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
    #[error("config file already exists at {0}")]
    ConfigExists(PathBuf),
    #[error("no config directory is known for this platform")]
    NoConfigDir,
}

fn interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::io::stdin().is_terminal()
}

fn nested_help(command: &Command) -> Option<&'static str> {
    match command {
        Command::Ui(args) if args.paths.command == Some(NestedCommand::Help) => Some("ui"),
        Command::Scan(args) if args.command == Some(NestedCommand::Help) => Some("scan"),
        Command::Duplicates(args) if args.command == Some(NestedCommand::Help) => {
            Some("duplicates")
        }
        Command::Clean(args) if args.command == Some(NestedCommand::Help) => Some("clean"),
        Command::Cache(args) if args.command == Some(NestedCommand::Help) => Some("cache"),
        Command::Rules(args) if args.command == Some(NestedCommand::Help) => Some("rules"),
        Command::Insight(args) if args.command == Some(NestedCommand::Help) => Some("insight"),
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
        None if !cli.json && interactive() => ui::run(&loaded.config, Vec::new(), false),
        None => scan::run(&loaded.config, ScanArgs::default(), cli.json, cli.quiet),
        Some(Command::Ui(args)) => ui::run(&loaded.config, args.paths.paths, args.confirm),
        Some(Command::Scan(args)) => scan::run(&loaded.config, args, cli.json, cli.quiet),
        Some(Command::Config { action }) => config::run(&loaded, action, cli.json),
        Some(Command::Rules(_)) => rules::run(&loaded.config, cli.json),
        Some(Command::Insight(_)) => insight::run(cli.json),
        Some(Command::Duplicates(args)) => {
            duplicates::run(&loaded.config, args, cli.json, cli.quiet)
        }
        Some(Command::Clean(args)) => clean::run(&loaded.config, args, cli.json, cli.quiet),
        Some(Command::Cache(args)) => cache::run(&loaded.config, args, cli.json, cli.quiet),
    }
}
