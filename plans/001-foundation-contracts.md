# Plan 001: Lay the shared contracts for the general cleanup sources

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s' 5d04119..HEAD -- src/detect/mod.rs src/config/mod.rs src/cli.rs src/commands/mod.rs src/lib.rs src/output/mod.rs src/protect.rs src/tui/view.rs crates/kuizer-gui/src/theme.rs crates/kuizer-gui/src/glossary.rs docs/config.example.toml`
> must print nothing. If it prints commits, compare the "Current state" excerpts with the live code
> and treat a mismatch as a STOP condition.

## Status

- **Priority**: P1 (blocks every other plan)
- **Effort**: L
- **Risk**: MED (touches shared files every later plan builds on)
- **Depends on**: none
- **Wave**: 0, runs alone
- **Model**: Opus 5.5, high effort
- **Category**: direction
- **Planned at**: commit `5d04119` (origin/master), 2026-10-21

## Purpose

kuizer is being extended from a developer disk cleaner into a general PC cleaner: app and browser
caches, the trash, system package caches and large personal files, next to the existing developer
sources. Eight plans in wave 1 run in parallel, each in its own worktree. They can only run in
parallel if none of them has to edit the same shared file. This plan makes every edit to the
shared files once, up front: new categories, the location catalog schema, empty modules, config
sections, CLI commands and output modules. After it lands, each wave-1 plan only edits files it
owns.

Nothing in this plan changes behavior for existing commands. New commands exist but answer with an
error until their owning plan fills them in. That is acceptable because all plans merge into the
integration branch `feat/general-cleanup` and only reach `master` after wave 2.

## Current state

Repository facts:

- Rust 2024, MSRV 1.88, Cargo workspace: `kuizer` (root, core plus CLI and TUI) and
  `crates/kuizer-gui` (GPUI 0.2.2 desktop app). The core must build without GPUI.
- `src/detect/mod.rs:9-29` defines the item category:

  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
  #[serde(rename_all = "kebab-case")]
  pub enum Category {
      Dependencies,
      Build,
      Cache,
      Temp,
  }

  impl Category {
      pub const ALL: [Category; 4] = [Self::Dependencies, Self::Build, Self::Cache, Self::Temp];
      pub fn as_str(self) -> &'static str { /* "dependencies", "build", "cache", "temp" */ }
  }
  ```

  `Category::ALL` is used as "the categories a project rule can have": the default of
  `detect.categories` (`src/config/mod.rs:121`), `--category` parsing (`src/cli.rs:870-875`), TUI
  category cycling (`src/tui/app.rs:275-281`) and GUI start-screen toggles
  (`crates/kuizer-gui/src/start.rs`). It must keep exactly those four values.
- Exhaustive `match`es on `Category` that break when variants are added:
  - `src/tui/view.rs:39-46` `category_color` and `src/tui/view.rs:74-86` (description per
    category and safety).
  - `crates/kuizer-gui/src/theme.rs:198-216` `tile` and `tile_edge` (greys, never hues).
  - `crates/kuizer-gui/src/glossary.rs:98-105` `Term::category`, plus `Term` variants and their
    title and text further down (`Self::Temp => "temp"` at about line 118, text at about 182).
- `src/config/mod.rs:19-31`:

  ```rust
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
  #[serde(default, deny_unknown_fields)]
  pub struct Config {
      pub scan: ScanConfig,
      pub detect: DetectConfig,
      pub rules: Vec<RuleSpec>,
      pub duplicates: DuplicateConfig,
      pub delete: DeleteConfig,
      pub git: GitConfig,
      pub appearance: AppearanceConfig,
      pub toolchains: ToolchainConfig,
      pub docker: DockerConfig,
  }
  ```

  `Config::validate` (about line 329) calls `self.detector()?` and `Excludes::new(...)?`.
  `ConfigError` (about line 248) is a `thiserror` enum. `Config::parse` runs `normalize` then
  `validate`. The test `parses_full_example` parses `docs/config.example.toml`.
- `src/cli.rs`: `COMMAND_GROUPS` (lines 36-66) drives the grouped help; the test
  `help_groups_list_every_command` (about line 976) asserts every clap subcommand appears in it.
  `Command` enum at about line 461. `CacheArgs` at about line 698 has `clean`, `removal`
  (`RemovalArgs`, flattened) and `command: Option<NestedCommand>`. Example blocks are built with
  `examples(&[(command, args, description)])`, see `cache_examples` at about line 223.
- `src/commands/mod.rs`: one `mod` per command, `nested_help` maps `Command::X(args)` with
  `NestedCommand::Help` to its name, `run` dispatches. `AppError` is the error enum.
- `src/lib.rs` lists the public modules. `src/output/mod.rs` declares `mod git; mod html; mod json;
  mod style; mod text;` and re-exports functions.
- Conventions (match them):
  - Standalone functions are `fn` items; closures for callbacks.
  - No comments except a one-line workaround reason, a one-line regex explanation or a `TODO`.
    Exported core functions get a doc block with exactly `@name`, `@description` and `@example`,
    as in `src/caches.rs` (`/// @name find` ...).
  - Config structs use `#[serde(default, deny_unknown_fields)]`, enums `rename_all = "kebab-case"`.
  - Errors are `thiserror` enums. No `unwrap()` outside tests.
  - Tests live in a `#[cfg(test)] mod tests` in the same file, and only touch `tempfile` dirs.

## Commands you will need

Set these once in your shell. The machine has 8 GB of RAM and other plans may build at the same
time; a shared target dir makes cargo wait for its lock instead of running two builds at once.

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected on success |
|---|---|---|
| Build core | `cargo build -p kuizer` | exit 0 |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| GUI tests | `cargo test -p kuizer-gui -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, no warnings |

Never start a second cargo command while one is running.

## Scope

**In scope** (the only files you may modify or create):

- `src/detect/mod.rs`
- `src/config/mod.rs`
- `src/catalog/mod.rs` (create)
- `src/catalog/data/developer.toml`, `linux.toml`, `macos.toml`, `browsers.toml` (create)
- `src/trashcan.rs`, `src/big.rs`, `src/system/mod.rs`, `src/keymap/mod.rs` (create)
- `src/commands/trash.rs`, `src/commands/system.rs`, `src/commands/big.rs` (create)
- `src/output/trash.rs`, `src/output/system.rs`, `src/output/big.rs` (create)
- `src/lib.rs`, `src/output/mod.rs`, `src/cli.rs`, `src/commands/mod.rs`
- `src/protect.rs` (only the `refuses` seam in step 3)
- `src/tui/view.rs`
- `crates/kuizer-gui/src/theme.rs`, `crates/kuizer-gui/src/glossary.rs`
- `docs/config.example.toml`

**Out of scope**:

- `src/caches.rs`, `src/commands/cache.rs`: plan 002 moves the cache list into the catalog. Only
  add the `--group` flag to `CacheArgs` in `src/cli.rs`; do not implement filtering.
- Any behavior of existing commands, JSON shapes, or GUI sections.
- `docs/design.md`, `README.md`, `site/`: plan 018.
- New dependencies. Everything here uses crates already in `Cargo.toml` (`serde`, `toml`,
  `thiserror`, `dirs`).

## Git workflow

- Branch `general/001-foundation` from `feat/general-cleanup`.
- One commit, conventional style as in `git log` (`feat(core): ...`). Body line:
  `Plan: plans/001-foundation-contracts.md`.
- No `Co-Authored-By`, no Claude or claude.ai references anywhere in the commit.
- Do not push.

## Steps

### Step 1: Add three categories

In `src/detect/mod.rs` add `Logs`, `Trash` and `Files` to `Category` (after `Temp`), with
`as_str` values `"logs"`, `"trash"`, `"files"`. Keep `Category::ALL` as the four rule categories
and add:

```rust
impl Category {
    /// @name Category::is_rule_category
    /// @description Whether a project detection rule may use this category. Logs, trash and
    /// files only come from the location catalog and the trash and big-file sources.
    ///
    /// @example
    /// assert!(!Category::Trash.is_rule_category());
    pub fn is_rule_category(self) -> bool {
        Self::ALL.contains(&self)
    }
}
```

Fix every exhaustive match listed in "Current state":

- `src/tui/view.rs` `category_color`: `Logs => Color::Indexed(144)`, `Trash => Color::Indexed(103)`,
  `Files => Color::Indexed(109)`. The description match: `(Category::Logs, Safety::Safe) =>
  "Log files. Deleting is safe, programs start new ones."`, `(Category::Trash, Safety::Safe) =>
  "Items already in the trash. Emptying it cannot be undone."`; `Files` is always `Review`, so the
  existing `(_, Safety::Review)` arm covers it, but the match must still compile: add
  `(Category::Files, Safety::Safe) => "Large file. Look before deleting, it cannot be regenerated."`.
- `crates/kuizer-gui/src/theme.rs` `tile` and `tile_edge`: continue the neutral grey steps
  (`tile`: Logs `0x0020_2020`, Trash `0x001e_1e1e`, Files `0x001c_1c1c`; `tile_edge`: Logs
  `0x0030_3030`, Trash `0x002a_2a2a`, Files `0x0026_2626`). No hues.
- `crates/kuizer-gui/src/glossary.rs`: add `Term::Logs`, `Term::TrashContents`,
  `Term::PersonalFiles` (`Term::Trash` already exists and means "Move to trash", do not reuse it),
  map them in `Term::category`, and give each a title and text in the same style as `Term::Temp`:
  - Logs: "Logs", "Files programs write to record what they did, such as crash reports and session logs. Programs start new ones."
  - TrashContents: "Trash", "Items you already moved to the trash. Emptying the trash removes them for good."
  - PersonalFiles: "Personal files", "Large or old files of your own, such as installers and disk images in Downloads. Nothing can bring them back, so they are never selected in bulk."

In `src/config/mod.rs` reject non-rule categories: add

```rust
#[error("`{category}` cannot be used in {key}, use dependencies, build, cache or temp")]
NotARuleCategory { key: &'static str, category: &'static str },
```

to `ConfigError`, filled with `category.as_str()`. In `Config::validate`, before the existing
checks, check `detect.categories` (key `"detect.categories"`) and every `[[rules]]` entry's
`category` (key `"rules"`) with `is_rule_category`.

**Verify**: `cargo build -p kuizer && cargo build -p kuizer-gui` exit 0.

### Step 2: Create the catalog schema

Create `src/catalog/mod.rs`. This is the contract four other plans write data against, so match
these names and serde shapes exactly:

```rust
use serde::{Deserialize, Serialize};

use crate::detect::{Category, Kind, Safety};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    Developer,
    Apps,
    Browsers,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Base {
    Home,
    Cache,
    Config,
    Data,
    State,
    Library,
    Cargo,
    GoModules,
    Gradle,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Platform {
    Linux,
    Macos,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub app: String,
    pub group: Group,
    #[serde(default = "default_category")]
    pub category: Category,
    #[serde(default = "default_kind")]
    pub kind: Kind,
    pub base: Base,
    pub path: String,
    #[serde(default = "default_safety")]
    pub safety: Safety,
    #[serde(default)]
    pub platforms: Vec<Platform>,
    #[serde(default)]
    pub processes: Vec<String>,
    #[serde(default)]
    pub note: String,
}
```

Defaults: `category = Cache`, `kind = Dir`, `safety = Review`. Base meanings (document them in
the `@description` of `parse`): `home` the home folder; `cache` `dirs::cache_dir()` (`~/.cache`,
`~/Library/Caches`); `config` `dirs::config_dir()`; `data` `dirs::data_dir()`; `state`
`dirs::state_dir()` (Linux only); `library` `~/Library` (macOS only); `cargo`, `go-modules`,
`gradle`, `bun` as in `src/caches.rs` `Locations::for_user`. Resolving bases is plan 002's job;
this plan only defines the enum.

Add `#[derive(Debug, thiserror::Error)] pub enum CatalogError` with variants for: a TOML parse
error (`file: &'static str` or `String`, boxed `toml::de::Error`), a duplicate name, an invalid
name, an invalid path, and a non-catalog category. Then:

- `pub fn parse(text: &str, origin: &str) -> Result<Vec<Entry>, CatalogError>`: parses a file of
  the form below and validates every entry.

  ```toml
  [[entry]]
  name = "thumbnails"
  app = "Desktop"
  group = "system"
  base = "cache"
  path = "thumbnails"
  safety = "safe"
  platforms = ["linux"]
  note = "Previews of your files; file managers make them again."
  ```

  Validation rules:
  - `name`: non-empty, only `a-z`, `0-9` and `-`, not starting or ending with `-`.
  - `app`: non-empty.
  - `path`: empty string (the base itself) or `/`-separated components; no empty component, no
    `.` or `..`, no leading `/`, no `\`. A component may be exactly `*` (any one directory
    level); any other `*`, `?`, `[`, `]`, `{`, `}` is invalid.
  - `category`: `Cache`, `Logs` or `Temp` only.
  - `kind`: `Dir` or `File` (not `External`).
- `pub fn check_unique<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Result<(), CatalogError>`.
- `pub fn embedded() -> &'static [Entry]`: parses the four embedded files once (`std::sync::OnceLock`)
  with `include_str!("data/developer.toml")` and the others, concatenated in the order developer,
  linux, macos, browsers. A file that fails to parse contributes no entries (the test below makes
  that impossible to ship).
- `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)] #[serde(default,
  deny_unknown_fields)] pub struct CatalogConfig {}` with `pub fn validate(&self) -> Result<(),
  crate::config::ConfigError> { Ok(()) }`. Plan 002 adds its fields.

Create the four data files, each containing only this header line and nothing else (owning plans
fill them):

- `developer.toml`: `# Developer tool caches. Filled by plan 002.`

  Use the same one-line form for `linux.toml` (plan 004), `macos.toml` (plan 005) and
  `browsers.toml` (plan 011). Note: these comment lines are data-file headers, not code comments.

Tests in `src/catalog/mod.rs`:

- `embedded_catalog_is_valid`: parse each embedded file with `parse` and assert `Ok`, then
  `check_unique` across all of them. This is the test every data plan runs.
- `rejects_bad_entries`: one table-driven test covering a path with `..`, a path with `a*b`, a
  name with uppercase, a `kind = "external"`, `category = "build"`, and a duplicate name.
- `accepts_wildcard_and_empty_path`: `path = "google-chrome/*/Cache"` and `path = ""` parse.

**Verify**: `cargo test -p kuizer catalog:: -- --test-threads=2` → 3 tests pass.

### Step 3: Create empty source modules and config sections

Create, each with only what compiles and is needed by the config:

- `src/trashcan.rs`: empty file.
- `src/big.rs`: `BigConfig` struct, same shape as `CatalogConfig` (empty, serde default,
  `deny_unknown_fields`, `validate` returning `Ok(())`).
- `src/system/mod.rs`: `SystemConfig`, same shape.
- `src/keymap/mod.rs`: `KeysConfig`, same shape.

Add to `src/lib.rs`: `pub mod big; pub mod catalog; pub mod keymap; pub mod system; pub mod
trashcan;` in alphabetical order with the existing lines.

Add to `Config` (after `docker`): `pub catalog: crate::catalog::CatalogConfig`, `pub system:
crate::system::SystemConfig`, `pub big: crate::big::BigConfig`, `pub keys:
crate::keymap::KeysConfig`. In `Config::validate`, after the existing checks, call
`self.catalog.validate()?; self.system.validate()?; self.big.validate()?; self.keys.validate()?;`.

Add one generic variant to `ConfigError`, so later plans never have to edit
`src/config/mod.rs` to report a bad key in their own section:

```rust
#[error("invalid {key}: {message}")]
Invalid { key: String, message: String },
```

Add a seam to `src/protect.rs` that plan 002 calls and plan 003 refines. Put it right after
`covers` in `impl Protected`:

```rust
/// @name refuses
/// @description Checks whether kuizer must leave `path` alone because it lies inside protected
/// application data. Today this is the same as `covers`.
///
/// @example
/// if protected.refuses(&found) {
///     continue;
/// }
pub fn refuses(&self, path: &Path) -> bool {
    self.covers(path)
}
```

Append to `docs/config.example.toml`, after `[docker]`, separated by blank lines:

```toml
# Locations outside your projects that kuizer knows: app, browser and system caches and logs.
[catalog]

# System package caches and logs, cleaned through each tool's own command.
[system]

# Large files anywhere under the scanned folders, and old files in Downloads.
[big]

# Key bindings shared by the GUI and the TUI. Each action name maps to a list of keys.
[keys]
```

**Verify**: `cargo test -p kuizer config:: -- --test-threads=2` → all pass, including
`parses_full_example`.

### Step 4: Register the CLI commands

In `src/cli.rs`:

- Add `const ABOUT_TRASH: &str = "Show and empty the trash";`, `const ABOUT_SYSTEM: &str = "List
  system package caches, journals and unused runtimes";`, `const ABOUT_BIG: &str = "List large
  files and old downloads";`.
- Add rows to `COMMAND_GROUPS`: `("trash", ABOUT_TRASH)` in "Clean up" after `"cache"`;
  `("system", ABOUT_SYSTEM)` in "Clean up" after `"docker"`; `("big", ABOUT_BIG)` in "Report"
  after `"duplicates"`.
- Add to `Command`:

  ```rust
  #[command(about = ABOUT_TRASH, after_help = trash_help())]
  Trash(TrashArgs),
  #[command(about = ABOUT_SYSTEM, after_help = system_help())]
  System(SystemArgs),
  #[command(about = ABOUT_BIG, after_help = big_help())]
  Big(BigArgs),
  ```

- Args structs, modeled on `DockerArgs` (`#[derive(Debug, Args, Default)]`,
  `#[command(subcommand_precedence_over_arg = true)]`):
  - `TrashArgs { clean: bool, removal: RemovalArgs, command: Option<NestedCommand> }`, `clean`
    help: "Empty the selected trash folders after a preview, always permanently".
  - `SystemArgs { command: Option<NestedCommand> }`. No removal flags: plan 012 adds them only
    if system cleanup is approved.
  - `BigArgs { paths: Vec<PathBuf>, min_size: Option<ByteSize>, limit: Option<usize>, command:
    Option<NestedCommand> }`. `paths` value name `PATH`, help "Directories to search [default:
    big.roots, or the home folder]". `--min-size` help "Smallest file to list [config:
    big.min_size]". `--limit` help "List at most this many files [config: big.limit]". No removal
    flags: plan 013 adds them only if deleting personal files is approved.
- Add to `CacheArgs`: `#[arg(long, value_name = "GROUP", help = "Only this group, repeatable:
  developer, apps, browsers, system", help_heading = "Filters")] pub group: Vec<Group>` with
  `Group` from `crate::catalog` (derive `clap::ValueEnum` on `Group` in `src/catalog/mod.rs` with
  `#[value(rename_all = "kebab-case")]`, or parse with a `value_parser` function; pick whichever
  compiles cleanly with clap 4 derive).
- Help functions, each with `examples(&[...])` like `cache_examples`:
  - `trash_help`: `("kuizer trash", "", "List trash folders and their size")`,
    `("kuizer trash", "--clean --dry-run", "Show what emptying would remove")`,
    `("kuizer trash", "--clean", "Empty the trash, after a confirmation")`.
  - `system_help`: `("kuizer system", "", "List package caches, journals and unused runtimes")`,
    `("kuizer system", "--json", "The same as JSON")`.
  - `big_help`: `("kuizer big", "", "List files of 500 MB and more under your home folder")`,
    `("kuizer big", "~/Videos --min-size 2GB", "Only files of 2 GB and more in ~/Videos")`.

In `src/commands/mod.rs`: add `mod big; mod system; mod trash;` (alphabetical), `nested_help`
arms for the three, and dispatch arms
`Some(Command::Trash(args)) => trash::run(&loaded.config, args, cli.json, cli.quiet)` and the same
for `system` and `big`. Add `AppError::NotBuilt(&'static str)` with message `"{0} is not built yet"`.

Create `src/commands/trash.rs`, `src/commands/system.rs`, `src/commands/big.rs`, each:

```rust
use super::AppError;
use crate::cli::TrashArgs;
use crate::config::Config;

pub fn run(_config: &Config, _args: TrashArgs, _json: bool, _quiet: bool) -> Result<(), AppError> {
    Err(AppError::NotBuilt("kuizer trash"))
}
```

(with `SystemArgs`/`"kuizer system"` and `BigArgs`/`"kuizer big"` respectively).

Create empty `src/output/trash.rs`, `src/output/system.rs`, `src/output/big.rs` and declare them
in `src/output/mod.rs` as `pub mod big; pub mod system; pub mod trash;`. Owning plans call them as
`output::trash::...` so no plan has to edit the re-export list.

**Verify**: `cargo test -p kuizer cli:: -- --test-threads=2` → all pass, including
`help_groups_list_every_command`. `cargo run -p kuizer -- trash` prints
`kuizer trash is not built yet` (through the existing error printing) and exits non-zero.

### Step 5: Full checks

**Verify**:

- `cargo fmt --all --check` → exit 0
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `cargo test -p kuizer -- --test-threads=2` → all pass
- `cargo test -p kuizer-gui -- --test-threads=2` → all pass

## Test plan

- `src/catalog/mod.rs`: `embedded_catalog_is_valid`, `rejects_bad_entries`,
  `accepts_wildcard_and_empty_path` (step 2).
- `src/config/mod.rs`: one test `rejects_non_rule_categories` with `detect.categories =
  ["trash"]` and a `[[rules]]` entry with `category = "logs"`, both `Err`. Model it after
  `rejects_unknown_fields`.
- No tests for the empty modules or the stub commands.

## Done criteria

- [ ] `cargo fmt --all --check` exits 0
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` exits 0
- [ ] `cargo test -p kuizer -- --test-threads=2` and `cargo test -p kuizer-gui -- --test-threads=2` pass
- [ ] `grep -c "\[\[entry\]\]" src/catalog/data/*.toml` prints 0 for every file
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] `git log -1 --format=%B | grep -i claude` prints nothing

## STOP conditions

- An excerpt in "Current state" does not match the code.
- Adding `Group` to clap requires a new crate or feature.
- Any existing test changes outcome (other than the new ones).
- A change seems to require editing `src/caches.rs`, a GUI file other than `theme.rs` and
  `glossary.rs`, or anything under `site/`.

## Maintenance notes

- Every later plan relies on the exact names here: `catalog::{Entry, Group, Base, Platform,
  parse, embedded, check_unique, CatalogConfig}`, the three new categories, `Config.{catalog,
  system, big, keys}`, `ConfigError::Invalid`, `Protected::refuses`, `output::{trash, system,
  big}` and the three CLI commands. Renaming any of
  them after wave 1 starts breaks parallel branches.
- `AppError::NotBuilt` is temporary. Plans 006, 007 and 008 stop using it; plan 018 deletes it.
- Reviewers: check that `Category::ALL` still has four entries and `kuizer scan --category trash`
  still fails to parse.
