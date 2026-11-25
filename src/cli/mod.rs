mod help;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use self::help::*;
use crate::detect::Category;
pub use crate::scan::SortKey;
use crate::units::{ByteSize, Idle, WatchInterval};

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
    #[command(about = ABOUT_EXPLAIN, after_help = explain_examples())]
    Explain(ExplainArgs),
    #[command(about = ABOUT_WATCH, after_help = watch_examples())]
    Watch(WatchArgs),
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

#[derive(Debug, Args)]
pub struct ExplainArgs {
    #[arg(value_name = "PATH", help = "Path to inspect")]
    pub path: PathBuf,
}

#[derive(Debug, Args, Default)]
#[command(subcommand_precedence_over_arg = true)]
pub struct WatchArgs {
    #[arg(
        value_name = "PATH",
        help = "Directories to watch [default: scan.roots or current directory]"
    )]
    pub paths: Vec<PathBuf>,
    #[arg(
        long,
        value_name = "DURATION",
        help = "Time between rescans, e.g. 30s or 5m [default: 30s]"
    )]
    pub interval: Option<WatchInterval>,
    #[arg(
        long,
        value_name = "N",
        help = "Stop after N scans instead of running until Ctrl-C"
    )]
    pub count: Option<usize>,
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
    #[command(flatten)]
    pub scope: ScopeArgs,
    #[command(subcommand)]
    pub command: Option<NestedCommand>,
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
    pub command: Option<DuplicatesAction>,
}

impl DuplicatesArgs {
    pub fn wants_help(&self) -> bool {
        match &self.command {
            Some(DuplicatesAction::Help) => true,
            Some(DuplicatesAction::Link(link)) => link.command == Some(NestedCommand::Help),
            None => false,
        }
    }

    pub fn link_unattended(&self) -> bool {
        matches!(
            &self.command,
            Some(DuplicatesAction::Link(link)) if link.yes && !link.dry_run
        )
    }
}

#[derive(Debug, Subcommand, Clone, PartialEq, Eq)]
#[command(disable_help_subcommand = true)]
pub enum DuplicatesAction {
    #[command(about = ABOUT_DUPLICATES_LINK)]
    Link(DuplicatesLinkArgs),
    #[command(hide = true)]
    Help,
}

#[derive(Debug, Args, Default, Clone, PartialEq, Eq)]
#[command(subcommand_precedence_over_arg = true)]
pub struct DuplicatesLinkArgs {
    #[arg(
        value_name = "PATH",
        help = "Directories to search [default: current directory]"
    )]
    pub paths: Vec<PathBuf>,
    #[arg(
        long,
        help = "Use hard links instead of reflinks (same filesystem only; edits change every copy)"
    )]
    pub hardlink: bool,
    #[arg(long, help = "Show what would be linked without changing any files")]
    pub dry_run: bool,
    #[arg(short, long, help = "Link without asking for confirmation")]
    pub yes: bool,
    #[arg(
        long,
        value_name = "SIZE",
        help = "Skip files smaller than this [default: duplicates.min_size]"
    )]
    pub min_size: Option<ByteSize>,
    #[arg(long, value_name = "N", help = "Process at most N groups")]
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
        assert_eq!(args.command, Some(DuplicatesAction::Help));

        let cli = Cli::try_parse_from([
            "kuizer",
            "duplicates",
            "link",
            "/tmp/a",
            "--hardlink",
            "--dry-run",
            "-y",
        ])
        .expect("parses");
        let Some(Command::Duplicates(args)) = cli.command else {
            panic!("expected duplicates");
        };
        let Some(DuplicatesAction::Link(link)) = args.command else {
            panic!("expected duplicates link");
        };
        assert_eq!(link.paths, [PathBuf::from("/tmp/a")]);
        assert!(link.hardlink);
        assert!(link.dry_run);
        assert!(link.yes);
    }
}
