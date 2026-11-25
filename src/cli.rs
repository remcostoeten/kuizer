use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand};

use crate::detect::Category;
pub use crate::scan::SortKey;
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
const WEBSITE: &str = "https://kuizer.app";
const RESET: &str = "\x1b[0m";

const ABOUT_SCAN: &str = "List generated artifacts and caches in projects";
const ABOUT_CLEAN: &str = "Remove generated artifacts and caches from projects";
const ABOUT_CACHE: &str = "List and remove global tool caches";
const ABOUT_TOOLCHAIN: &str = "List and remove installed toolchain versions";
const ABOUT_DOCKER: &str = "List and remove unused Docker and Podman objects";
const ABOUT_CACHE_CLEAN: &str = "Remove the selected caches after a preview";
const ABOUT_TOOLCHAIN_CLEAN: &str =
    "Remove the selected versions after a preview, always permanently";
const ABOUT_DOCKER_CLEAN: &str = "Remove the selected objects after a preview, always permanently";
const ABOUT_GIT: &str = "Pack loose git objects and delete merged branches";
const ABOUT_DUPLICATES: &str = "Find files with identical contents";
const ABOUT_HISTORY: &str = "Show what kuizer deleted and the space it freed";
const ABOUT_RESTORE: &str = "Put trashed items from a past run back";
const ABOUT_DASHBOARD: &str = "Open a web overview of artifacts, caches, duplicates and history";
const ABOUT_RULES: &str = "List the active detection rules";
const ABOUT_POLICY: &str = "List and inspect configured cleanup policies";
const ABOUT_CONFIG: &str = "Show, create and validate the config file";

const COMMAND_GROUPS: [(&str, &[(&str, &str)]); 3] = [
    (
        "Clean up",
        &[
            ("scan", ABOUT_SCAN),
            ("clean", ABOUT_CLEAN),
            ("cache", ABOUT_CACHE),
            ("toolchain", ABOUT_TOOLCHAIN),
            ("docker", ABOUT_DOCKER),
            ("git", ABOUT_GIT),
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
            ("policy", ABOUT_POLICY),
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
    let mut out = examples(&[
        ("kuizer", "", "Browse the current directory"),
        ("kuizer", "~/apps ~/work", "Browse two folders at once"),
        (
            "kuizer",
            "--confirm",
            "Ask before every delete, even with delete.confirm off",
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
    ]);
    out.push_str(&format!("\n{DIM}Docs: {WEBSITE}/docs{RESET}\n"));
    out
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
            "~/apps --idle 3mo",
            "Only projects untouched for three months",
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
        "Selects safe items unless --select says otherwise, and deletes them permanently after a confirmation, or moves them to the trash with delete.mode = \"trash\".\n\n{}",
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
                "~/apps --idle 60d",
                "Only projects idle for 60 days"
            ),
            (
                "kuizer clean",
                "~/apps --select all",
                "Remove everything, after a confirmation"
            ),
            (
                "kuizer clean",
                "~/apps --category build --yes",
                "Remove build output without asking"
            ),
            (
                "kuizer clean",
                "--policy stale-deps --dry-run",
                "Preview a configured policy"
            ),
        ])
    )
}

fn cache_examples() -> String {
    examples(&[
        ("kuizer cache", "", "List global tool caches"),
        (
            "kuizer cache clean",
            "--dry-run",
            "Show which caches would be removed",
        ),
        (
            "kuizer cache clean",
            "",
            "Remove safe caches, after a confirmation",
        ),
        (
            "kuizer cache clean",
            "--select npm --select pip",
            "Remove the npm and pip caches",
        ),
    ])
}

fn toolchain_help() -> String {
    format!(
        "Lists rustup toolchains, nvm, fnm and volta Node versions, pyenv and uv Python versions, SDKMAN candidates and Go SDKs in ~/sdk. A version that is a tool's default or that a project under PATH pins (rust-toolchain, .nvmrc, .python-version and similar) is marked review. Removal is always permanent: rustup and uv versions go through their own uninstall command, the rest is deleted. The tool downloads a version again when it is needed.\n\n{}",
        examples(&[
            (
                "kuizer toolchain",
                "",
                "List versions, pins searched in the current directory"
            ),
            ("kuizer toolchain", "~/apps", "Search ~/apps for pin files"),
            (
                "kuizer toolchain clean",
                "~/apps --dry-run",
                "Show which unused versions would be removed",
            ),
            (
                "kuizer toolchain clean",
                "~/apps --select nvm",
                "Remove every nvm Node version, after a confirmation",
            ),
        ])
    )
}

fn docker_help() -> String {
    format!(
        "Lists dangling images, stopped containers, the build cache and volumes no container uses, for every engine in docker.engines. Only the engines' own CLIs run, so DOCKER_HOST and contexts apply. Removal is always permanent and goes through `image rm`, `container rm`, `builder prune` and `volume rm` without force, so the engine refuses anything still in use. Volumes and containers that stopped recently are marked review.\n\n{}",
        examples(&[
            (
                "kuizer docker",
                "",
                "List what Docker and Podman could free"
            ),
            (
                "kuizer docker clean",
                "--dry-run",
                "Show which objects would be removed",
            ),
            (
                "kuizer docker clean",
                "--select image --select build-cache",
                "Remove dangling images and the build cache",
            ),
        ])
    )
}

fn git_help() -> String {
    format!(
        "Neither action goes through the trash. Restore a deleted branch with the\n`git branch NAME COMMIT` line kuizer prints for it.\n\n{}",
        examples(&[
            ("kuizer git", "~/apps", "List repositories worth cleaning"),
            (
                "kuizer git",
                "--gc --dry-run",
                "Show where git gc would run and its expected gain",
            ),
            ("kuizer git", "--gc", "Run git gc, after a confirmation"),
            (
                "kuizer git",
                "--branches",
                "Delete local branches already merged, after a confirmation",
            ),
        ])
    )
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
        (
            "kuizer history",
            "prune --older-than 90d",
            "Drop runs older than 90 days and compact the file",
        ),
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

fn policy_examples() -> String {
    examples(&[
        ("kuizer policy", "list", "List configured cleanup policies"),
        (
            "kuizer policy",
            "show stale-deps",
            "Show the effective filters and selection of a policy",
        ),
        (
            "kuizer clean",
            "--policy stale-deps --dry-run",
            "Preview what a policy would remove",
        ),
        ("kuizer --json policy", "list", "Print policies as JSON"),
    ])
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
        (
            "kuizer config",
            "check --fix",
            "Rewrite deprecated keys and fix invalid settings",
        ),
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
        help = "Ask before deleting, even with delete.confirm off [config: delete.confirm]"
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
        long,
        global = true,
        conflicts_with = "json",
        help = "Print one JSON object per item as it is found or removed, then a summary line",
        help_heading = "Global options"
    )]
    pub json_lines: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
    JsonLines,
}

impl Cli {
    /// @name Cli::format
    /// @description Returns the output format the global `--json` and `--json-lines` flags ask
    /// for. Clap rejects both at once.
    ///
    /// @example
    /// let json = cli.format() != Format::Text;
    pub fn format(&self) -> Format {
        if self.json_lines {
            Format::JsonLines
        } else if self.json {
            Format::Json
        } else {
            Format::Text
        }
    }
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
    #[command(about = ABOUT_TOOLCHAIN, visible_alias = "toolchains", after_help = toolchain_help())]
    Toolchain(ToolchainArgs),
    #[command(about = ABOUT_DOCKER, visible_alias = "podman", after_help = docker_help())]
    Docker(DockerArgs),
    #[command(about = ABOUT_GIT, visible_alias = "repos", after_help = git_help())]
    Git(GitArgs),
    #[command(about = ABOUT_POLICY, visible_alias = "policies", after_help = policy_examples())]
    Policy(PolicyArgs),
    #[command(about = ABOUT_RULES, after_help = rules_examples())]
    Rules(CommandArgs),
    #[command(about = ABOUT_HISTORY, visible_alias = "insight", after_help = history_examples())]
    History(HistoryArgs),
    #[command(about = ABOUT_RESTORE, after_help = restore_help())]
    Restore(RestoreArgs),
    #[command(about = ABOUT_DASHBOARD, visible_alias = "web", after_help = dashboard_help())]
    Dashboard(DashboardArgs),
    #[command(
        about = "Print shell completions or man pages for packagers",
        hide = true
    )]
    Generate {
        #[command(subcommand)]
        target: GenerateTarget,
    },
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq)]
pub enum GenerateTarget {
    #[command(about = "Print the completion script for SHELL")]
    Completions {
        #[arg(value_enum, value_name = "SHELL")]
        shell: clap_complete::Shell,
    },
    #[command(about = "Print the kuizer(1) man page, or write one page per command to DIR")]
    Man {
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
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
    #[command(flatten)]
    pub scope: ScopeArgs,
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
        long,
        value_name = "NAME",
        help = "Run a named policy from the config; other flags narrow it and never widen it",
        help_heading = "Filters"
    )]
    pub policy: Option<String>,
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
    pub scope: ScopeArgs,
    #[command(flatten)]
    pub removal: RemovalArgs,
    #[arg(
        long,
        help = "Delete permanently instead of using the trash; refused with --policy unless permanent = true",
        help_heading = "Removal"
    )]
    pub permanent: bool,
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
}

#[derive(Debug, Args, Default, Clone, PartialEq, Eq)]
pub struct ScopeArgs {
    #[arg(
        long = "rule",
        value_name = "NAME",
        global = true,
        help = "Only include items of this rule, repeatable: a name --select takes",
        help_heading = "Filters"
    )]
    pub rules: Vec<String>,
    #[arg(
        long = "project",
        value_name = "GLOB",
        global = true,
        help = "Only include items in a project matching GLOB, repeatable",
        help_heading = "Filters"
    )]
    pub projects: Vec<String>,
    #[arg(
        long = "exclude",
        value_name = "GLOB",
        global = true,
        help = "Skip paths matching GLOB, repeatable, on top of scan.exclude",
        help_heading = "Filters"
    )]
    pub excludes: Vec<String>,
}

#[derive(Debug, Args, Default, Clone, PartialEq, Eq)]
pub struct SourceFilterArgs {
    #[arg(
        long = "category",
        value_name = "NAME",
        value_parser = parse_category,
        global = true,
        help = "Only include this category, repeatable: dependencies, build, cache, temp",
        help_heading = "Filters"
    )]
    pub categories: Vec<Category>,
    #[arg(
        long,
        value_name = "SIZE",
        global = true,
        help = "Skip entries smaller than this, e.g. 50MB",
        help_heading = "Filters"
    )]
    pub min_size: Option<ByteSize>,
    #[command(flatten)]
    pub scope: ScopeArgs,
}

#[derive(Debug, Args, Default, Clone, PartialEq, Eq)]
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
    #[arg(short, long, help = "Remove without asking for confirmation")]
    pub yes: bool,
}

#[derive(Debug, Args, Default, Clone, PartialEq, Eq)]
pub struct HiddenRemovalArgs {
    #[arg(
        long,
        hide = true,
        value_name = "WHAT",
        value_parser = parse_selection,
        help = "What to remove: safe, all or a rule name, repeatable [default: safe]"
    )]
    pub select: Vec<Selection>,
    #[arg(
        long,
        hide = true,
        help = "Show what would be removed without removing it"
    )]
    pub dry_run: bool,
    #[arg(
        short,
        long,
        hide = true,
        help = "Remove without asking for confirmation"
    )]
    pub yes: bool,
}

impl From<HiddenRemovalArgs> for RemovalArgs {
    fn from(args: HiddenRemovalArgs) -> Self {
        Self {
            select: args.select,
            dry_run: args.dry_run,
            yes: args.yes,
        }
    }
}

fn merge_removal(mut primary: RemovalArgs, fallback: &HiddenRemovalArgs) -> RemovalArgs {
    primary.select.extend(fallback.select.iter().cloned());
    primary.dry_run |= fallback.dry_run;
    primary.yes |= fallback.yes;
    primary
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalPlan {
    pub cleaning: bool,
    pub removal: RemovalArgs,
}

impl RemovalPlan {
    /// @name RemovalPlan::unattended
    /// @description Returns whether this plan removes items with `--yes` and no dry run, which a
    /// config with rejected scope settings must refuse.
    ///
    /// @example
    /// if args.plan().unattended() && !rejected.is_empty() {
    ///     return Err(AppError::ScopeRejected(rejected.join(", ")));
    /// }
    pub fn unattended(&self) -> bool {
        self.cleaning && self.removal.yes && !self.removal.dry_run
    }
}

fn removal_plan(
    clean: bool,
    listing: &HiddenRemovalArgs,
    nested: Option<&RemovalArgs>,
) -> RemovalPlan {
    let removal = match nested {
        Some(nested) => merge_removal(nested.clone(), listing),
        None => listing.clone().into(),
    };
    RemovalPlan {
        cleaning: clean || nested.is_some() || removal.dry_run,
        removal,
    }
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
    #[arg(long, hide = true, help = "Same as the clean subcommand")]
    pub clean: bool,
    #[command(flatten)]
    pub filters: SourceFilterArgs,
    #[command(flatten)]
    pub removal: HiddenRemovalArgs,
    #[command(subcommand)]
    pub command: Option<CacheAction>,
}

impl CacheArgs {
    /// @name CacheArgs::plan
    /// @description Combines the `clean` subcommand, the hidden `--clean` alias and flags given
    /// before the subcommand. Only `clean`, `--clean` or `--dry-run` leave the plain listing.
    ///
    /// @example
    /// let plan = args.plan();
    pub fn plan(&self) -> RemovalPlan {
        let nested = match &self.command {
            Some(CacheAction::Clean(removal)) => Some(removal),
            _ => None,
        };
        removal_plan(self.clean, &self.removal, nested)
    }

    pub fn wants_help(&self) -> bool {
        self.command == Some(CacheAction::Help)
    }
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq)]
#[command(disable_help_subcommand = true)]
pub enum CacheAction {
    #[command(about = ABOUT_CACHE_CLEAN)]
    Clean(RemovalArgs),
    #[command(hide = true)]
    Help,
}

#[derive(Debug, Args, Default)]
#[command(subcommand_precedence_over_arg = true)]
pub struct ToolchainArgs {
    #[arg(
        value_name = "PATH",
        help = "Directories to search for pin files [default: scan.roots or the current directory]"
    )]
    pub paths: Vec<PathBuf>,
    #[arg(long, hide = true, help = "Same as the clean subcommand")]
    pub clean: bool,
    #[command(flatten)]
    pub filters: SourceFilterArgs,
    #[command(flatten)]
    pub removal: HiddenRemovalArgs,
    #[command(subcommand)]
    pub command: Option<ToolchainAction>,
}

impl ToolchainArgs {
    /// @name ToolchainArgs::plan
    /// @description Combines the `clean` subcommand, the hidden `--clean` alias and flags given
    /// before the subcommand. Only `clean`, `--clean` or `--dry-run` leave the plain listing.
    ///
    /// @example
    /// let plan = args.plan();
    pub fn plan(&self) -> RemovalPlan {
        let nested = match &self.command {
            Some(ToolchainAction::Clean(clean)) => Some(&clean.removal),
            _ => None,
        };
        removal_plan(self.clean, &self.removal, nested)
    }

    /// @name ToolchainArgs::pin_paths
    /// @description Returns the folders to search for pin files, given before or after `clean`.
    /// Dropping one would make a version pinned there look unused.
    ///
    /// @example
    /// let roots = resolve_roots(config, args.pin_paths())?;
    pub fn pin_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.paths.clone();
        if let Some(ToolchainAction::Clean(clean)) = &self.command {
            paths.extend(clean.paths.iter().cloned());
        }
        paths
    }

    pub fn wants_help(&self) -> bool {
        match &self.command {
            Some(ToolchainAction::Help) => true,
            Some(ToolchainAction::Clean(clean)) => clean.command == Some(NestedCommand::Help),
            None => false,
        }
    }
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq)]
#[command(disable_help_subcommand = true)]
pub enum ToolchainAction {
    #[command(about = ABOUT_TOOLCHAIN_CLEAN)]
    Clean(ToolchainCleanArgs),
    #[command(hide = true)]
    Help,
}

#[derive(Debug, Args, Clone, PartialEq, Eq)]
#[command(subcommand_precedence_over_arg = true)]
pub struct ToolchainCleanArgs {
    #[arg(
        value_name = "PATH",
        help = "Directories to search for pin files [default: scan.roots or the current directory]"
    )]
    pub paths: Vec<PathBuf>,
    #[command(flatten)]
    pub removal: RemovalArgs,
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
}

#[derive(Debug, Args, Default)]
#[command(subcommand_precedence_over_arg = true)]
pub struct DockerArgs {
    #[arg(long, hide = true, help = "Same as the clean subcommand")]
    pub clean: bool,
    #[command(flatten)]
    pub filters: SourceFilterArgs,
    #[command(flatten)]
    pub removal: HiddenRemovalArgs,
    #[command(subcommand)]
    pub command: Option<DockerAction>,
}

impl DockerArgs {
    /// @name DockerArgs::plan
    /// @description Combines the `clean` subcommand, the hidden `--clean` alias and flags given
    /// before the subcommand. Only `clean`, `--clean` or `--dry-run` leave the plain listing.
    ///
    /// @example
    /// let plan = args.plan();
    pub fn plan(&self) -> RemovalPlan {
        let nested = match &self.command {
            Some(DockerAction::Clean(removal)) => Some(removal),
            _ => None,
        };
        removal_plan(self.clean, &self.removal, nested)
    }

    pub fn wants_help(&self) -> bool {
        self.command == Some(DockerAction::Help)
    }
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq)]
#[command(disable_help_subcommand = true)]
pub enum DockerAction {
    #[command(about = ABOUT_DOCKER_CLEAN)]
    Clean(RemovalArgs),
    #[command(hide = true)]
    Help,
}

#[derive(Debug, Args, Default)]
#[command(subcommand_precedence_over_arg = true)]
pub struct GitArgs {
    #[arg(
        value_name = "PATH",
        help = "Directories to search for repositories [default: current directory]"
    )]
    pub paths: Vec<PathBuf>,
    #[arg(
        long,
        help = "Run git gc in each listed repository after a preview [config: git.aggressive]",
        help_heading = "Cleanup"
    )]
    pub gc: bool,
    #[arg(
        long,
        help = "Delete the listed merged local branches after a preview",
        help_heading = "Cleanup"
    )]
    pub branches: bool,
    #[arg(
        long,
        help = "Show what would run without changing anything",
        help_heading = "Cleanup"
    )]
    pub dry_run: bool,
    #[arg(
        short,
        long,
        help = "Run without asking for confirmation",
        help_heading = "Cleanup"
    )]
    pub yes: bool,
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
    #[command(about = "Trim older runs and compact the history file")]
    Prune(HistoryPruneArgs),
    #[command(hide = true)]
    Help,
}

#[derive(Debug, Args, Clone, PartialEq, Eq, Default)]
pub struct HistoryPruneArgs {
    #[arg(long, value_name = "N", help = "Keep at most the N newest runs")]
    pub keep: Option<usize>,
    #[arg(
        long,
        value_name = "AGE",
        help = "Remove runs older than AGE, e.g. 90d, 6mo or 1y"
    )]
    pub older_than: Option<Idle>,
    #[arg(
        long,
        value_name = "SIZE",
        help = "Trim oldest runs until the history file is at most SIZE [default: 10 MiB]"
    )]
    pub max_size: Option<ByteSize>,
    #[arg(long, help = "Show what would be pruned without changing the file")]
    pub dry_run: bool,
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

#[derive(Debug, Args, Default)]
pub struct PolicyArgs {
    #[command(subcommand)]
    pub command: Option<PolicyAction>,
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq)]
#[command(disable_help_subcommand = true)]
pub enum PolicyAction {
    #[command(about = "List configured policies")]
    List,
    #[command(about = "Show the effective filters and selection of one policy")]
    Show {
        #[arg(value_name = "NAME", help = "Policy name from the config")]
        name: String,
    },
    #[command(hide = true)]
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
    Check {
        #[arg(
            long,
            help = "Rewrite deprecated keys and remove or correct invalid ones"
        )]
        fix: bool,
    },
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
    }

    #[test]
    fn toolchain_clean_reads_the_same_in_every_spelling() {
        let expected = RemovalPlan {
            cleaning: true,
            removal: RemovalArgs {
                select: vec![Selection::Rule("nvm".to_owned())],
                dry_run: true,
                yes: true,
            },
        };
        for argv in [
            ["clean", "/tmp/a", "--select", "nvm", "--dry-run", "-y"],
            ["/tmp/a", "clean", "--select", "nvm", "--dry-run", "-y"],
            ["/tmp/a", "--select", "nvm", "--dry-run", "-y", "clean"],
            ["/tmp/a", "--clean", "--select", "nvm", "--dry-run", "-y"],
        ] {
            let cli = Cli::try_parse_from(["kuizer", "toolchain"].into_iter().chain(argv))
                .expect("parses");
            let Some(Command::Toolchain(args)) = cli.command else {
                panic!("expected toolchain");
            };
            assert_eq!(args.plan(), expected, "{argv:?}");
            assert_eq!(args.pin_paths(), [PathBuf::from("/tmp/a")], "{argv:?}");
        }
    }

    #[test]
    fn a_listing_never_becomes_a_removal() {
        for argv in [
            &["kuizer", "cache", "--select", "npm", "-y"][..],
            &["kuizer", "docker", "--yes"][..],
            &["kuizer", "toolchain", "/tmp/a", "-y"][..],
        ] {
            let plan = match Cli::try_parse_from(argv).expect("parses").command {
                Some(Command::Cache(args)) => args.plan(),
                Some(Command::Docker(args)) => args.plan(),
                Some(Command::Toolchain(args)) => args.plan(),
                _ => panic!("expected a source command"),
            };
            assert!(!plan.cleaning, "{argv:?}");
        }
        assert!(Cli::try_parse_from(["kuizer", "cache", "--select", "clean", "-y"]).is_err());
    }

    #[test]
    fn help_groups_list_every_command() {
        let mut grouped: Vec<&str> = COMMAND_GROUPS
            .iter()
            .flat_map(|(_, commands)| commands.iter().map(|(name, _)| *name))
            .filter(|name| *name != "help")
            .collect();
        let command = Cli::command();
        let mut actual: Vec<&str> = command
            .get_subcommands()
            .filter(|c| !c.is_hide_set())
            .map(|c| c.get_name())
            .collect();
        grouped.sort_unstable();
        actual.sort_unstable();
        assert_eq!(grouped, actual);
    }

    #[test]
    fn source_filters_read_the_same_before_and_after_clean() {
        for argv in [
            &[
                "kuizer",
                "cache",
                "--rule",
                "npm",
                "clean",
                "--exclude",
                "x",
            ][..],
            &[
                "kuizer",
                "cache",
                "clean",
                "--rule",
                "npm",
                "--exclude",
                "x",
            ][..],
            &[
                "kuizer",
                "cache",
                "--exclude",
                "x",
                "--rule",
                "npm",
                "--clean",
            ][..],
        ] {
            let Some(Command::Cache(args)) = Cli::try_parse_from(argv).expect("parses").command
            else {
                panic!("expected cache");
            };
            assert_eq!(args.filters.scope.rules, ["npm"], "{argv:?}");
            assert_eq!(args.filters.scope.excludes, ["x"], "{argv:?}");
            assert!(args.plan().cleaning, "{argv:?}");
        }
        assert!(Cli::try_parse_from(["kuizer", "--json", "--json-lines", "scan"]).is_err());
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
