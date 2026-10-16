use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::detect::Category;
use crate::units::{ByteSize, Idle};

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::BrightBlack.on_default())
    .valid(AnsiColor::Cyan.on_default())
    .invalid(AnsiColor::Yellow.on_default().effects(Effects::BOLD))
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD));

const HEADER: &str = "\x1b[1;32m";
const LITERAL: &str = "\x1b[1;36m";
const DIM: &str = "\x1b[90m";
const RESET: &str = "\x1b[0m";

const ABOUT_SCAN: &str = "List generated artifacts and caches in projects";
const ABOUT_CLEAN: &str = "Remove generated artifacts and caches from projects";
const ABOUT_CACHE: &str = "List and remove global tool caches";
const ABOUT_DUPLICATES: &str = "Find files with identical contents";
const ABOUT_HISTORY: &str = "Show what kuizer deleted and the space it freed";
const ABOUT_RESTORE: &str = "Put trashed items from a past run back";
const ABOUT_DASHBOARD: &str = "Open a web overview of artifacts, caches, duplicates and history";
const ABOUT_RULES: &str = "List the active detection rules";
const ABOUT_CONFIG: &str = "Show, create and validate the config file";

const COMMAND_GROUPS: [(&str, &[(&str, &str)]); 3] = [
    (
        "Clean up",
        &[
            ("scan", ABOUT_SCAN),
            ("clean", ABOUT_CLEAN),
            ("cache", ABOUT_CACHE),
            ("restore", ABOUT_RESTORE),
        ],
    ),
    (
        "Report",
        &[
            ("dashboard", ABOUT_DASHBOARD),
            ("duplicates", ABOUT_DUPLICATES),
            ("history", ABOUT_HISTORY),
        ],
    ),
    (
        "Setup",
        &[
            ("rules", ABOUT_RULES),
            ("config", ABOUT_CONFIG),
            ("help", "Show help for a command"),
        ],
    ),
];

fn examples(rows: &[(&str, &str, &str)]) -> String {
    let width = rows
        .iter()
        .map(|(command, args, _)| plain_width(command, args))
        .max()
        .unwrap_or(0);
    let mut out = format!("{HEADER}Examples:{RESET}\n");
    for (command, args, description) in rows {
        let padding = " ".repeat(width - plain_width(command, args) + 2);
        let args = if args.is_empty() {
            String::new()
        } else {
            format!(" {args}")
        };
        out.push_str(&format!(
            "  {LITERAL}{command}{RESET}{args}{padding}{DIM}{description}{RESET}\n"
        ));
    }
    out
}

fn plain_width(command: &str, args: &str) -> usize {
    let args = if args.is_empty() {
        0
    } else {
        args.chars().count() + 1
    };
    command.chars().count() + args
}

fn top_template() -> String {
    let width = COMMAND_GROUPS
        .iter()
        .flat_map(|(_, commands)| commands.iter())
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    let mut out = String::from("{name} {version}\n{about-with-newline}\n{usage-heading} {usage}\n");
    for (heading, commands) in COMMAND_GROUPS {
        out.push_str(&format!("\n{HEADER}{heading}:{RESET}\n"));
        for (name, about) in commands {
            out.push_str(&format!("  {LITERAL}{name:width$}{RESET}  {about}\n"));
        }
    }
    out.push_str(&format!(
        "\n{HEADER}Options:{RESET}\n{{options}}{{after-help}}"
    ));
    out
}

fn top_examples() -> String {
    examples(&[
        ("kuizer", "", "Browse the current directory"),
        ("kuizer", "~/apps ~/work", "Browse two folders at once"),
        (
            "kuizer",
            "--confirm",
            "Show a dialog before every trash move",
        ),
        (
            "kuizer scan",
            "~/apps --min-size 50MB",
            "List large artifacts under ~/apps",
        ),
        (
            "kuizer clean",
            "~/apps --dry-run",
            "Show what clean would remove",
        ),
        ("kuizer cache", "", "List global tool caches"),
        ("kuizer duplicates", "~/Downloads", "Find duplicate files"),
        ("kuizer history", "", "Show what past runs deleted"),
        (
            "kuizer restore",
            "--last",
            "Put the last run back from the trash",
        ),
        (
            "kuizer dashboard",
            "~/apps",
            "Open a web overview of everything",
        ),
        ("kuizer", "<command> --help", "Show help for a command"),
    ])
}

fn scan_examples() -> String {
    examples(&[
        ("kuizer scan", "", "Scan the current directory"),
        (
            "kuizer scan",
            "~/apps --min-size 50MB",
            "Only artifacts of 50 MB or more",
        ),
        (
            "kuizer scan",
            "--category cache --safe-only",
            "Only caches that are safe to delete",
        ),
        (
            "kuizer scan",
            "~/apps --sort path --limit 20",
            "First 20 results by path",
        ),
        (
            "kuizer scan",
            "~/apps --report",
            "Open an interactive report",
        ),
        ("kuizer --json scan", "~/apps", "Print JSON for scripts"),
    ])
}

fn clean_help() -> String {
    format!(
        "Selects safe items unless --select says otherwise, and moves them to the trash after a confirmation.\n\n{}",
        examples(&[
            (
                "kuizer clean",
                "~/apps --dry-run",
                "Show what would be removed"
            ),
            (
                "kuizer clean",
                "~/apps",
                "Remove safe items, after a confirmation"
            ),
            (
                "kuizer clean",
                "~/apps --select node-modules",
                "Remove only node_modules folders"
            ),
            (
                "kuizer clean",
                "~/apps --select all --permanent",
                "Remove everything, skipping the trash"
            ),
            (
                "kuizer clean",
                "~/apps --category build --yes",
                "Remove build output without asking"
            ),
        ])
    )
}

fn cache_examples() -> String {
    examples(&[
        ("kuizer cache", "", "List global tool caches"),
        (
            "kuizer cache",
            "--clean --dry-run",
            "Show which caches would be removed",
        ),
        (
            "kuizer cache",
            "--clean",
            "Remove safe caches, after a confirmation",
        ),
        (
            "kuizer cache",
            "--clean --select npm --select pip",
            "Remove the npm and pip caches",
        ),
    ])
}

fn duplicates_help() -> String {
    format!(
        "Reports only; kuizer never deletes duplicates.\n\n{}",
        examples(&[
            (
                "kuizer duplicates",
                "~/Downloads",
                "Find duplicates in Downloads"
            ),
            (
                "kuizer duplicates",
                "~/Pictures --min-size 1MB",
                "Skip files under 1 MB"
            ),
            (
                "kuizer duplicates",
                "~/Pictures --verify",
                "Compare matches byte by byte"
            ),
        ])
    )
}

fn history_examples() -> String {
    examples(&[
        ("kuizer history", "", "List past runs with their ids"),
        ("kuizer history", "show s4", "Every item of run s4"),
        ("kuizer --json history", "", "Print the history as JSON"),
    ])
}

fn restore_help() -> String {
    format!(
        "Moves items back from the trash to where they were. Never overwrites: an item whose path exists again, or that left the trash, is skipped and reported.\n\n{}",
        examples(&[
            ("kuizer restore", "--last", "Restore the latest run"),
            ("kuizer restore", "s4", "Restore every item of run s4"),
            ("kuizer restore", "s4.2 s4.5", "Restore two items"),
            (
                "kuizer restore",
                "s4 --dry-run",
                "Show what would be restored"
            ),
        ])
    )
}

fn dashboard_help() -> String {
    format!(
        "Scans the paths, measures global caches, searches for duplicates and reads the deletion history, then writes one HTML page and opens it. The page loads nothing from the network.\n\n{}",
        examples(&[
            ("kuizer dashboard", "", "Overview of the current directory"),
            (
                "kuizer dashboard",
                "~/apps ~/work",
                "Overview of two folders"
            ),
            (
                "kuizer dashboard",
                "~/apps --no-duplicates",
                "Skip the duplicate search"
            ),
            (
                "kuizer dashboard",
                "--out=./kuizer.html --no-open",
                "Write to a file without opening it"
            ),
            (
                "kuizer --json dashboard",
                "~/apps",
                "Print the combined data as JSON"
            ),
        ])
    )
}

fn rules_examples() -> String {
    examples(&[
        ("kuizer rules", "", "List built-in and custom rules"),
        ("kuizer --json rules", "", "Print the rules as JSON"),
    ])
}

fn config_examples() -> String {
    examples(&[
        (
            "kuizer config",
            "init",
            "Write a config file with the defaults",
        ),
        ("kuizer config", "check", "Validate the config file"),
        ("kuizer config", "show", "Print the effective configuration"),
        (
            "kuizer --config",
            "./kuizer.toml scan",
            "Use another config file",
        ),
    ])
}

#[derive(Debug, Parser)]
#[command(
    name = "kuizer",
    version,
    about = "Find and clean generated artifacts, caches and duplicates",
    styles = STYLES,
    help_template = top_template(),
    after_help = top_examples(),
    next_line_help = false,
    override_usage = "kuizer [OPTIONS] [PATH]...\n       kuizer [OPTIONS] <COMMAND>"
)]
pub struct Cli {
    #[arg(
        value_name = "PATH",
        help = "Directories to browse without a command [default: current directory]"
    )]
    pub paths: Vec<PathBuf>,
    #[arg(
        long,
        help = "Ask before moving items to the trash [config: delete.confirm]"
    )]
    pub confirm: bool,
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
    #[command(about = ABOUT_SCAN, after_help = scan_examples())]
    Scan(ScanArgs),
    #[command(about = ABOUT_DUPLICATES, visible_alias = "deduplicate", after_help = duplicates_help())]
    Duplicates(DuplicatesArgs),
    #[command(about = ABOUT_CLEAN, after_help = clean_help())]
    Clean(CleanArgs),
    #[command(about = ABOUT_CONFIG, after_help = config_examples())]
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    #[command(about = ABOUT_CACHE, after_help = cache_examples())]
    Cache(CacheArgs),
    #[command(about = ABOUT_RULES, after_help = rules_examples())]
    Rules(CommandArgs),
    #[command(about = ABOUT_HISTORY, visible_alias = "insight", after_help = history_examples())]
    History(HistoryArgs),
    #[command(about = ABOUT_RESTORE, after_help = restore_help())]
    Restore(RestoreArgs),
    #[command(about = ABOUT_DASHBOARD, visible_alias = "web", after_help = dashboard_help())]
    Dashboard(DashboardArgs),
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
        help = "Only include this category, repeatable: dependencies, build, cache, temp",
        help_heading = "Filters"
    )]
    pub categories: Vec<Category>,
    #[arg(
        long,
        value_name = "SIZE",
        help = "Skip entries smaller than this, e.g. 50MB",
        help_heading = "Filters"
    )]
    pub min_size: Option<ByteSize>,
    #[arg(
        long,
        visible_alias = "older-than",
        value_name = "AGE",
        help = "Only include projects without changes for AGE, e.g. 60d, 8w, 6mo or 1y",
        help_heading = "Filters"
    )]
    pub idle: Option<Idle>,
    #[arg(
        long,
        help = "Only include entries that are safe to delete",
        help_heading = "Filters"
    )]
    pub safe_only: bool,
    #[arg(long, value_enum, default_value_t = SortKey::Size, hide_possible_values = true,
        help = "Sort by size, path, category or idle", help_heading = "Output")]
    pub sort: SortKey,
    #[arg(
        long,
        value_name = "N",
        help = "Show at most N entries",
        help_heading = "Output"
    )]
    pub limit: Option<usize>,
    #[arg(
        long,
        help = "Print results once the scan finishes",
        help_heading = "Output"
    )]
    pub no_live: bool,
    #[arg(
        long,
        value_name = "FILE",
        num_args = 0..=1,
        require_equals = true,
        help = "Write an interactive HTML report and open it, to FILE if given",
        help_heading = "Output"
    )]
    pub report: Option<Option<PathBuf>>,
    #[arg(
        long,
        requires = "report",
        help = "Write the report without opening a browser",
        help_heading = "Output"
    )]
    pub no_open: bool,
    #[arg(
        long,
        requires = "report",
        help = "Overwrite the report file if it already exists",
        help_heading = "Output"
    )]
    pub force: bool,
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
        help = "Only include this category, repeatable: dependencies, build, cache, temp",
        help_heading = "Filters"
    )]
    pub categories: Vec<Category>,
    #[arg(
        long,
        value_name = "SIZE",
        help = "Skip entries smaller than this, e.g. 50MB",
        help_heading = "Filters"
    )]
    pub min_size: Option<ByteSize>,
    #[arg(
        long,
        visible_alias = "older-than",
        value_name = "AGE",
        help = "Only include projects without changes for AGE, e.g. 60d, 8w, 6mo or 1y",
        help_heading = "Filters"
    )]
    pub idle: Option<Idle>,
    #[command(flatten)]
    pub removal: RemovalArgs,
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
}

#[derive(Debug, Args, Default)]
#[command(next_help_heading = "Removal")]
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
pub struct DashboardArgs {
    #[arg(
        value_name = "PATH",
        help = "Directories to scan [default: current directory]"
    )]
    pub paths: Vec<PathBuf>,
    #[arg(
        long,
        help = "Skip the duplicate search, which reads every file",
        help_heading = "Sources"
    )]
    pub no_duplicates: bool,
    #[arg(
        long,
        value_name = "SIZE",
        help = "Skip duplicate files smaller than this [default: duplicates.min_size]",
        help_heading = "Sources"
    )]
    pub min_size: Option<ByteSize>,
    #[arg(
        long,
        value_name = "FILE",
        require_equals = true,
        help = "Write the page to FILE [default: the kuizer cache directory]",
        help_heading = "Output"
    )]
    pub out: Option<PathBuf>,
    #[arg(
        long,
        help = "Write the page without opening a browser",
        help_heading = "Output"
    )]
    pub no_open: bool,
    #[arg(
        long,
        requires = "out",
        help = "Overwrite the page file if it already exists",
        help_heading = "Output"
    )]
    pub force: bool,
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
}

#[derive(Debug, Args, Default)]
#[command(subcommand_precedence_over_arg = true)]
pub struct CacheArgs {
    #[arg(
        long,
        help = "Remove the selected caches after a preview",
        help_heading = "Removal"
    )]
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
pub struct HistoryArgs {
    #[command(subcommand)]
    pub command: Option<HistoryAction>,
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq)]
#[command(disable_help_subcommand = true)]
pub enum HistoryAction {
    #[command(about = "List every item of one run")]
    Show {
        #[arg(value_name = "RUN", help = "Run id, e.g. s4")]
        run: String,
    },
    #[command(hide = true)]
    Help,
}

#[derive(Debug, Args, Default)]
#[command(subcommand_precedence_over_arg = true, subcommand_negates_reqs = true)]
pub struct RestoreArgs {
    #[arg(
        value_name = "ID",
        required_unless_present = "last",
        help = "Run ids like s4 or item ids like s4.2"
    )]
    pub ids: Vec<String>,
    #[arg(long, conflicts_with = "ids", help = "Restore the latest run")]
    pub last: bool,
    #[arg(long, help = "Show what would be restored without restoring it")]
    pub dry_run: bool,
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
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
    Idle,
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
    fn bare_paths_open_the_interface() {
        let cli = Cli::try_parse_from(["kuizer", "/tmp/a", "--confirm"]).expect("parses");
        assert!(cli.command.is_none());
        assert_eq!(cli.paths, [PathBuf::from("/tmp/a")]);
        assert!(cli.confirm);

        for argv in [
            &["kuizer", "scan", "/tmp/a"][..],
            &["kuizer", "--json", "scan", "/tmp/a"][..],
            &["kuizer", "-q", "--config", "k.toml", "scan", "/tmp/a"][..],
        ] {
            let cli = Cli::try_parse_from(argv).expect("parses");
            assert!(matches!(cli.command, Some(Command::Scan(_))), "{argv:?}");
            assert!(cli.paths.is_empty(), "{argv:?}");
        }
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
    fn parses_report_flag() {
        let cli = Cli::try_parse_from(["kuizer", "scan", "--report", "/tmp/a"]).expect("parses");
        let Some(Command::Scan(args)) = cli.command else {
            panic!("expected scan");
        };
        assert_eq!(args.report, Some(None));
        assert_eq!(args.paths, [PathBuf::from("/tmp/a")]);

        let cli = Cli::try_parse_from(["kuizer", "scan", "--report=out.html", "--no-open"])
            .expect("parses");
        let Some(Command::Scan(args)) = cli.command else {
            panic!("expected scan");
        };
        assert_eq!(args.report, Some(Some(PathBuf::from("out.html"))));
        assert!(args.no_open);
        assert!(Cli::try_parse_from(["kuizer", "scan", "--no-open"]).is_err());
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
    fn help_groups_list_every_command() {
        let mut grouped: Vec<&str> = COMMAND_GROUPS
            .iter()
            .flat_map(|(_, commands)| commands.iter().map(|(name, _)| *name))
            .filter(|name| *name != "help")
            .collect();
        let command = Cli::command();
        let mut actual: Vec<&str> = command.get_subcommands().map(|c| c.get_name()).collect();
        grouped.sort_unstable();
        actual.sort_unstable();
        assert_eq!(grouped, actual);
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
