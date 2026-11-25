mod cache;
mod clean;
mod config;
mod dashboard;
mod docker;
mod duplicates;
mod filter;
mod generate;
mod git;
mod history;
mod interrupt;
mod policy;
mod removal;
mod restore;
mod rules;
mod scan;
mod toolchain;
mod ui;

use std::path::PathBuf;
use std::process::ExitCode;

use crate::cli::{Cli, Command, Format, HistoryAction, NestedCommand, PolicyAction, ScanArgs};
use crate::config::{Config, ConfigError, Loaded, rejected_scope};
use crate::history::HistoryError;
use crate::scan::ScopeError;
use clap::CommandFactory;

pub use crate::scan::sort_candidates;
pub use cache::measure as measure_caches;
pub use cache::measure_threaded as measure_caches_threaded;
pub use cache::measure_with as measure_caches_with;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error("cannot write the report: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the config has problems, see the warnings above")]
    ConfigProblems,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Usage(String),
    #[error("unknown selection `{name}`, expected safe, all or one of: {known}")]
    UnknownSelection { name: String, known: String },
    #[error("unknown rule `{name}`, expected one of: {known}")]
    UnknownRule { name: String, known: String },
    #[error(transparent)]
    Scope(#[from] ScopeError),
    #[error("--json-lines works with scan, clean, cache, toolchain and docker; use --json")]
    JsonLinesUnsupported,
    #[error("refusing to remove without confirmation: no terminal to ask on, pass --yes")]
    NeedsConfirmation,
    #[error(
        "refusing to remove with --yes: the config rejected {0}, which may widen what gets removed; fix the config or run without --yes to confirm"
    )]
    ScopeRejected(String),
    #[error("{0} items could not be removed")]
    RemovalFailed(usize),
    #[error("stopped by Ctrl-C, {0} items were left in place")]
    Interrupted(usize),
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

impl AppError {
    /// @name AppError::exit_code
    /// @description Maps usage errors to exit code `2` and all other runtime errors to `1`.
    ///
    /// @example
    /// std::process::ExitCode::from(error.exit_code());
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) | Self::UnknownSelection { .. } => ExitCode::from(2),
            _ => ExitCode::FAILURE,
        }
    }
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
        Command::Cache(args) if args.wants_help() => Some("cache"),
        Command::Toolchain(args) if args.wants_help() => Some("toolchain"),
        Command::Docker(args) if args.wants_help() => Some("docker"),
        Command::Git(args) if args.command == Some(NestedCommand::Help) => Some("git"),
        Command::Policy(args) if args.command == Some(PolicyAction::Help) => Some("policy"),
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

fn removes_unattended(command: Option<&Command>) -> bool {
    match command {
        Some(Command::Clean(args)) => args.removal.yes && !args.removal.dry_run,
        Some(Command::Cache(args)) => args.plan().unattended(),
        Some(Command::Toolchain(args)) => args.plan().unattended(),
        Some(Command::Docker(args)) => args.plan().unattended(),
        Some(Command::Git(args)) => args.yes && !args.dry_run && (args.gc || args.branches),
        _ => false,
    }
}

fn streams_lines(command: Option<&Command>) -> bool {
    matches!(
        command,
        None | Some(
            Command::Scan(_)
                | Command::Clean(_)
                | Command::Cache(_)
                | Command::Toolchain(_)
                | Command::Docker(_),
        )
    )
}

fn config_notice(loaded: &Loaded) -> Option<String> {
    let first = loaded.warnings.first()?;
    let more = loaded.warnings.len() - 1;
    Some(if more == 0 {
        format!("config: {first}")
    } else {
        format!("config: {first} (and {more} more)")
    })
}

pub fn run(cli: Cli) -> Result<(), AppError> {
    if let Some(name) = cli.command.as_ref().and_then(nested_help) {
        return print_subcommand_help(name);
    }
    if let Some(Command::Generate { target }) = &cli.command {
        return generate::run(target);
    }
    let format = cli.format();
    if format == Format::JsonLines && !streams_lines(cli.command.as_ref()) {
        return Err(AppError::JsonLinesUnsupported);
    }
    let json = format != Format::Text;
    let loaded = Config::load(cli.config.as_deref())?;
    for warning in &loaded.warnings {
        eprintln!("kuizer: config: {warning}");
    }
    let rejected = rejected_scope(&loaded.warnings);
    if !rejected.is_empty() && removes_unattended(cli.command.as_ref()) {
        return Err(AppError::ScopeRejected(rejected.join(", ")));
    }
    match cli.command {
        None if !json && interactive() => {
            let notice = config_notice(&loaded);
            let config_path = cli
                .config
                .or(loaded.source)
                .or_else(crate::config::default_path);
            ui::run(&loaded.config, cli.paths, cli.confirm, notice, config_path)
        }
        None => {
            let args = ScanArgs {
                paths: cli.paths,
                ..ScanArgs::default()
            };
            scan::run(&loaded.config, args, format, cli.quiet)
        }
        Some(Command::Scan(args)) => scan::run(&loaded.config, args, format, cli.quiet),
        Some(Command::Config { action }) => config::run(&loaded, action, cli.json),
        Some(Command::Policy(args)) => policy::run(&loaded.config, args, cli.json),
        Some(Command::Rules(_)) => rules::run(&loaded.config, cli.json),
        Some(Command::History(args)) => history::run(args, cli.json),
        Some(Command::Restore(args)) => restore::run(args, cli.json),
        Some(Command::Duplicates(args)) => {
            duplicates::run(&loaded.config, args, cli.json, cli.quiet)
        }
        Some(Command::Clean(args)) => clean::run(&loaded.config, args, format, cli.quiet),
        Some(Command::Cache(args)) => cache::run(&loaded.config, args, format, cli.quiet),
        Some(Command::Docker(args)) => docker::run(&loaded.config, args, format, cli.quiet),
        Some(Command::Toolchain(args)) => toolchain::run(&loaded.config, args, format, cli.quiet),
        Some(Command::Git(args)) => git::run(&loaded.config, args, cli.json, cli.quiet),
        Some(Command::Dashboard(args)) => dashboard::run(&loaded.config, args, cli.json, cli.quiet),
        Some(Command::Generate { target }) => generate::run(&target),
    }
}
