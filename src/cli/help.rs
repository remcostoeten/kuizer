use clap::builder::styling::{AnsiColor, Effects, Styles};

pub(super) const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::BrightBlack.on_default())
    .valid(AnsiColor::Cyan.on_default())
    .invalid(AnsiColor::Yellow.on_default().effects(Effects::BOLD))
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD));

pub(super) const HEADER: &str = "\x1b[1;32m";
pub(super) const LITERAL: &str = "\x1b[1;36m";
pub(super) const DIM: &str = "\x1b[90m";
pub(super) const WEBSITE: &str = "https://kuizer.app";
pub(super) const RESET: &str = "\x1b[0m";

pub(super) const ABOUT_SCAN: &str = "List generated artifacts and caches in projects";
pub(super) const ABOUT_CLEAN: &str = "Remove generated artifacts and caches from projects";
pub(super) const ABOUT_CACHE: &str = "List and remove global tool caches";
pub(super) const ABOUT_TOOLCHAIN: &str = "List and remove installed toolchain versions";
pub(super) const ABOUT_DOCKER: &str = "List and remove unused Docker and Podman objects";
pub(super) const ABOUT_CACHE_CLEAN: &str = "Remove the selected caches after a preview";
pub(super) const ABOUT_TOOLCHAIN_CLEAN: &str =
    "Remove the selected versions after a preview, always permanently";
pub(super) const ABOUT_DOCKER_CLEAN: &str =
    "Remove the selected objects after a preview, always permanently";
pub(super) const ABOUT_GIT: &str = "Pack loose git objects and delete merged branches";
pub(super) const ABOUT_DUPLICATES: &str = "Find files with identical contents";
pub(super) const ABOUT_HISTORY: &str = "Show what kuizer deleted and the space it freed";
pub(super) const ABOUT_RESTORE: &str = "Put trashed items from a past run back";
pub(super) const ABOUT_DASHBOARD: &str =
    "Open a web overview of artifacts, caches, duplicates and history";
pub(super) const ABOUT_RULES: &str = "List the active detection rules";
pub(super) const ABOUT_EXPLAIN: &str = "Explain why a path is or is not a candidate";
pub(super) const ABOUT_POLICY: &str = "List and inspect configured cleanup policies";
pub(super) const ABOUT_CONFIG: &str = "Show, create and validate the config file";

pub(super) const COMMAND_GROUPS: [(&str, &[(&str, &str)]); 3] = [
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
            ("explain", ABOUT_EXPLAIN),
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

pub(super) fn top_template() -> String {
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

pub(super) fn top_examples() -> String {
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

pub(super) fn scan_examples() -> String {
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

pub(super) fn clean_help() -> String {
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

pub(super) fn cache_examples() -> String {
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

pub(super) fn toolchain_help() -> String {
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

pub(super) fn docker_help() -> String {
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

pub(super) fn git_help() -> String {
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

pub(super) fn duplicates_help() -> String {
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

pub(super) fn history_examples() -> String {
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

pub(super) fn restore_help() -> String {
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

pub(super) fn dashboard_help() -> String {
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

pub(super) fn policy_examples() -> String {
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

pub(super) fn rules_examples() -> String {
    examples(&[
        ("kuizer rules", "", "List built-in and custom rules"),
        ("kuizer --json rules", "", "Print the rules as JSON"),
    ])
}

pub(super) fn explain_examples() -> String {
    examples(&[
        (
            "kuizer explain",
            "./node_modules",
            "Show which rule, markers, safety and guards apply to a path",
        ),
        (
            "kuizer --json explain",
            "./dist",
            "Print the explanation as JSON",
        ),
    ])
}

pub(super) fn config_examples() -> String {
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
