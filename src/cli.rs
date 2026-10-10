use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::detect::Category;
use crate::units::ByteSize;

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::BrightBlack.on_default())
    .valid(AnsiColor::Cyan.on_default())
    .invalid(AnsiColor::Yellow.on_default().effects(Effects::BOLD))
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD));

const TEMPLATE: &str = "\
{before-help}{name} {version}
{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}";

const EXAMPLES: &str = "\
\x1b[1;32mExamples:\x1b[0m
  \x1b[1;36mkuizer\x1b[0m                               \x1b[90mBrowse the current directory\x1b[0m
  \x1b[1;36mkuizer scan\x1b[0m ~/apps --min-size 50MB     \x1b[90mList large artifacts under ~/apps\x1b[0m
  \x1b[1;36mkuizer scan\x1b[0m --category cache          \x1b[90mOnly show caches\x1b[0m
  \x1b[1;36mkuizer duplicates\x1b[0m ~/Downloads         \x1b[90mFind duplicate files\x1b[0m
  \x1b[1;36mkuizer insight\x1b[0m                       \x1b[90mShow what past runs deleted\x1b[0m
  \x1b[1;36mkuizer\x1b[0m <command> --help                \x1b[90mShow help for a command\x1b[0m
";

#[derive(Debug, Parser)]
#[command(
    name = "kuizer",
    version,
    about = "Find and clean generated artifacts, caches and duplicates",
    styles = STYLES,
    help_template = TEMPLATE,
    after_help = EXAMPLES,
    subcommand_help_heading = "Commands",
    next_line_help = false
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Use this config file",
        help_heading = "Global options"
    )]
    pub config: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Print machine-readable JSON",
        help_heading = "Global options"
    )]
    pub json: bool,
    #[arg(
        short,
        long,
        global = true,
        help = "Hide progress and summary output",
        help_heading = "Global options"
    )]
    pub quiet: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Open the interactive cleanup interface")]
    Ui(PathsArgs),
    #[command(about = "Find generated artifacts and caches")]
    Scan(ScanArgs),
    #[command(about = "Find duplicate files", visible_alias = "deduplicate")]
    Duplicates(DuplicatesArgs),
    #[command(about = "Clean generated artifacts")]
    Clean(CleanArgs),
    #[command(about = "Show and manage configuration")]
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    #[command(about = "Find and clean global tool caches")]
    Cache(CacheArgs),
    #[command(about = "List active detection rules")]
    Rules(CommandArgs),
    #[command(about = "Show everything kuizer has deleted and the space it freed")]
    Insight(CommandArgs),
}

#[derive(Debug, Args, Default)]
#[command(subcommand_precedence_over_arg = true)]
pub struct PathsArgs {
    #[arg(
        value_name = "PATH",
        help = "Directories to search [default: current directory]"
    )]
    pub paths: Vec<PathBuf>,
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
}

#[derive(Debug, Args, Default)]
#[command(subcommand_precedence_over_arg = true)]
pub struct ScanArgs {
    #[arg(
        value_name = "PATH",
        help = "Directories to scan [default: current directory]"
    )]
    pub paths: Vec<PathBuf>,
    #[arg(
        long = "category",
        value_name = "NAME",
        value_parser = parse_category,
        help = "Only include this category, repeatable"
    )]
    pub categories: Vec<Category>,
    #[arg(
        long,
        value_name = "SIZE",
        help = "Skip entries smaller than this, e.g. 50MB"
    )]
    pub min_size: Option<ByteSize>,
    #[arg(long, value_enum, default_value_t = SortKey::Size, hide_possible_values = true,
        help = "Sort by size, path or category")]
    pub sort: SortKey,
    #[arg(long, help = "Only include entries that are safe to delete")]
    pub safe_only: bool,
    #[arg(long, value_name = "N", help = "Show at most N entries")]
    pub limit: Option<usize>,
    #[arg(long, help = "Print results once the scan finishes")]
    pub no_live: bool,
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
}

#[derive(Debug, Args)]
#[command(subcommand_precedence_over_arg = true)]
pub struct CleanArgs {
    #[arg(
        value_name = "PATH",
        help = "Directories to clean [default: current directory]"
    )]
    pub paths: Vec<PathBuf>,
    #[arg(
        long = "category",
        value_name = "NAME",
        value_parser = parse_category,
        help = "Only include this category, repeatable"
    )]
    pub categories: Vec<Category>,
    #[arg(
        long,
        value_name = "SIZE",
        help = "Skip entries smaller than this, e.g. 50MB"
    )]
    pub min_size: Option<ByteSize>,
    #[command(flatten)]
    pub removal: RemovalArgs,
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
}

#[derive(Debug, Args, Default)]
pub struct RemovalArgs {
    #[arg(
        long,
        value_name = "WHAT",
        value_parser = parse_selection,
        help = "What to remove: safe, all or a rule name, repeatable [default: safe]"
    )]
    pub select: Vec<Selection>,
    #[arg(long, help = "Show what would be removed without removing it")]
    pub dry_run: bool,
    #[arg(
        long,
        conflicts_with = "dry_run",
        help = "Delete permanently instead of moving to the trash"
    )]
    pub permanent: bool,
    #[arg(short, long, help = "Remove without asking for confirmation")]
    pub yes: bool,
}

#[derive(Debug, Args, Default)]
#[command(subcommand_precedence_over_arg = true)]
pub struct DuplicatesArgs {
    #[arg(
        value_name = "PATH",
        help = "Directories to search [default: current directory]"
    )]
    pub paths: Vec<PathBuf>,
    #[arg(
        long,
        value_name = "SIZE",
        help = "Skip files smaller than this [default: duplicates.min_size]"
    )]
    pub min_size: Option<ByteSize>,
    #[arg(long, help = "Compare file contents byte by byte after a hash match")]
    pub verify: bool,
    #[arg(long, value_name = "N", help = "Show at most N groups")]
    pub limit: Option<usize>,
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
}

#[derive(Debug, Args, Default)]
#[command(subcommand_precedence_over_arg = true)]
pub struct CacheArgs {
    #[arg(long, help = "Remove the selected caches after a preview")]
    pub clean: bool,
    #[command(flatten)]
    pub removal: RemovalArgs,
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Safe,
    All,
    Rule(String),
}

#[derive(Debug, Args, Default)]
pub struct CommandArgs {
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
}

#[derive(Debug, Subcommand, Clone, Copy, PartialEq, Eq)]
#[command(disable_help_subcommand = true)]
pub enum NestedCommand {
    Help,
}

#[derive(Debug, Subcommand, Clone, Copy)]
pub enum ConfigAction {
    #[command(about = "Print the effective configuration")]
    Show,
    #[command(about = "Print the config file location")]
    Path,
    #[command(about = "Write a default config file")]
    Init,
    #[command(about = "Validate the config file")]
    Check,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum SortKey {
    #[default]
    Size,
    Path,
    Category,
}

fn parse_selection(value: &str) -> Result<Selection, String> {
    match value {
        "" => Err("expected safe, all or a rule name".to_owned()),
        "safe" => Ok(Selection::Safe),
        "all" => Ok(Selection::All),
        name => Ok(Selection::Rule(name.to_owned())),
    }
}

fn parse_category(value: &str) -> Result<Category, String> {
    Category::ALL
        .into_iter()
        .find(|category| category.as_str() == value)
        .ok_or_else(|| {
            let names: Vec<&str> = Category::ALL.iter().map(|c| c.as_str()).collect();
            format!("expected one of: {}", names.join(", "))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_scan_flags() {
        let cli = Cli::try_parse_from([
            "kuizer",
            "--json",
            "scan",
            "/tmp/a",
            "--category",
            "cache",
            "--min-size",
            "10MB",
            "--sort",
            "path",
        ])
        .expect("parses");
        let Some(Command::Scan(args)) = cli.command else {
            panic!("expected scan");
        };
        assert!(cli.json);
        assert_eq!(args.categories, [Category::Cache]);
        assert_eq!(args.min_size, Some(ByteSize(10_000_000)));
        assert_eq!(args.sort, SortKey::Path);
    }

    #[test]
    fn parses_clean_selection() {
        let cli = Cli::try_parse_from([
            "kuizer",
            "clean",
            "/tmp/a",
            "--select",
            "all",
            "--select",
            "node-modules",
            "--dry-run",
            "-y",
        ])
        .expect("parses");
        let Some(Command::Clean(args)) = cli.command else {
            panic!("expected clean");
        };
        assert_eq!(
            args.removal.select,
            [Selection::All, Selection::Rule("node-modules".to_owned())]
        );
        assert!(args.removal.dry_run && args.removal.yes);
        assert!(Cli::try_parse_from(["kuizer", "clean", "--dry-run", "--permanent"]).is_err());
    }

    #[test]
    fn rejects_unknown_category() {
        assert!(Cli::try_parse_from(["kuizer", "scan", "--category", "music"]).is_err());
    }

    #[test]
    fn parses_nested_help_and_deduplicate_alias() {
        let cli = Cli::try_parse_from(["kuizer", "scan", "help"]).expect("parses");
        let Some(Command::Scan(args)) = cli.command else {
            panic!("expected scan");
        };
        assert_eq!(args.command, Some(NestedCommand::Help));

        let cli = Cli::try_parse_from(["kuizer", "deduplicate", "help"]).expect("parses");
        let Some(Command::Duplicates(args)) = cli.command else {
            panic!("expected duplicates");
        };
        assert_eq!(args.command, Some(NestedCommand::Help));
    }
}
