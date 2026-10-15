# Phase 8: shared filters, idle time and `cache clean`

Status: plan. `scan` and `clean` take `--category` and `--min-size`; `scan` also has `--safe-only`. `cache` takes `--clean` and the removal flags. Behaviour is specified in `design.md` sections 6.1 to 6.3.

## 1. One filter type

### CLI

`src/cli.rs` gets a `FilterArgs` struct, flattened with `#[command(flatten)]` into `ScanArgs`, `CleanArgs`, `CacheArgs`, `DashboardArgs` and, in phase 10, the new sources. `RemovalArgs` already exists and stays as it is.

```rust
pub struct FilterArgs {
    pub categories: Vec<Category>,   // --category, repeatable
    pub rules: Vec<String>,          // --rule, repeatable
    pub min_size: Option<ByteSize>,  // --min-size
    pub idle: Option<Idle>,          // --idle, alias --older-than
    pub projects: Vec<String>,       // --project GLOB, repeatable
    pub excludes: Vec<String>,       // --exclude GLOB, repeatable
}
```

`--safe-only` on `scan` stays as a hidden flag. `Idle` is a new type in `src/units.rs` beside `ByteSize`, parsing `30d`, `2w`, `6mo` (30 days) and `1y` (365 days) into seconds, with the same error style as `SizeError`.

### Filters apply after detection, not by removing rules

`--category` used to narrow `config.detect.categories`, which removed rules from the detector, and a removed rule no longer pruned the walk: `kuizer scan --category build` descended into `node_modules` and reported the `dist` folders of installed packages. Fixed in `Detector::reporting`: the detector keeps every enabled rule for pruning and reports only the wanted categories (`tests/scan.rs` `category_filter_keeps_pruning_other_artifacts`). `--rule` must follow the same rule: it filters candidates, it never removes rules from the detector.

A new `src/filter.rs` owns this:

```rust
pub struct Filter { categories, rules, min_size, idle, projects: GlobSet, now }
impl Filter {
    pub fn from_args(args: &FilterArgs, config: &Config) -> Result<Self, AppError>;
    pub fn matches(&self, candidate: &Candidate) -> bool;
}
```

- `detect.categories` and `detect.disabled_rules` in the config keep their current meaning (rules that are off for good). Only the CLI flags move to `Filter`.
- `--min-size` merges with `scan.min_size`: the larger wins. The sizing threads in `src/scan/mod.rs` keep dropping below `scan.min_size` so memory stays bounded.
- `--exclude` appends to `scan.exclude` before the walk, since an excluded path should not be entered at all.
- `--rule` names are checked against `config.rule_specs()`; unknown names are a usage error, as `--select` already does in `removal::check_selection`.
- Repeated values of one flag are a union; different flags intersect.

`scan`, `clean`, `dashboard` and the TUI call `Filter::matches` on every candidate. That single call site is what makes `scan X` and `clean --dry-run X` list the same items.

## 2. Idle time

### What activity means

From `design.md` section 6.3: project activity is the later of the newest modification time outside the project's artifacts and the last commit in its git repository. The artifact's own time is ignored.

`Candidate.project` is the artifact's parent directory (`parent_project` in `src/scan/mod.rs`). In a monorepo that is `packages/web`, not the repository. The git commit time comes from the repository, so an active monorepo keeps every package active, while the file time is per package. That is the intended result: a package nobody edits in a repository that is still worked on is not idle.

### How it is computed

The walker uses `d_type` and never stats plain files, which is what keeps the scan fast. Idle time needs file times, so it is computed only when asked for (`--idle`, `--sort idle`, the idle column in the TUI, or the dashboard):

1. A new `src/scan/activity.rs` with `fn project_activity(project: &Path, artifacts: &[PathBuf], cancel: &Cancel) -> Option<SystemTime>`. It walks the project with `lstat` on every entry, skips the artifact paths, `scan.skip_dirs` and nested projects, and returns the newest `mtime`. It stops at the first entry newer than the idle cutoff, because one recent file is enough to rule the project out.
2. `src/git.rs` gains `last_commit(dir) -> Option<SystemTime>`, running `git -C dir log -1 --format=%ct` with the same environment as `uncommitted_changes`. Results are cached per repository root (`git rev-parse --show-toplevel`).
3. Activity is computed once per project, after the scan, on a pool of `size_threads` workers, and stored in a new `Candidate.project_last_active: Option<SystemTime>`.
4. Unknown activity (no readable files, no git) is `None`, and `None` never matches `--idle`.

Phase 7's benchmark gives the cost of step 1 on a real home directory. If it is too slow for the TUI, the TUI shows the idle column as it fills in, the same way the uncommitted-changes column already arrives from `spawn_git` in `src/tui/mod.rs`.

### Output

- `SortKey::Idle` in `src/cli.rs`; `--sort idle` puts the longest idle first.
- An `IDLE` column in `output::print_scan` and the TUI list (`src/tui/view.rs`), formatted like `tui::app::age`.
- JSON: `project_last_active` (Unix seconds or `null`) on every candidate, as in `design.md` section 6.
- Dashboard: an "idle" column in the artifacts table and a "stale" insight that uses real project activity instead of the artifact's newest file, which is what it uses now.

## 3. `cache clean`

`CacheArgs` gets an optional subcommand `clean` that takes `FilterArgs` and `RemovalArgs`. Bare `kuizer cache` lists. `--clean` stays as a hidden alias until the next minor release and prints a one-line notice on stderr pointing to `kuizer cache clean`. Update the examples in `cache_examples`, `tests/clean.rs` `cache_lists_and_cleans_known_caches_only`, the README and the dashboard insight command (`kuizer cache --clean` in `insights()` in `report.html`).

## Tests

- Property test (`proptest` dev dependency) in `tests/filters.rs`: generate small project trees and random `FilterArgs`, run `kuizer --json scan` and `kuizer --json clean --dry-run --select all` with the same filters, and assert the same set of paths.
- `scan --rule js-dist` on a tree with `node_modules/pkg/dist` reports no `dist` inside `node_modules`.
- Idle tests in `tests/idle.rs`, setting times with `filetime` (dev dependency):
  - artifact touched recently in a project whose files are old: idle.
  - a commit with no file changes newer than the cutoff: active.
  - a project outside git with old files: idle.
  - a project with no readable files and no git: never matches.

## Accepted when

The tests above pass, `kuizer scan --idle 60d ~/apps` and `kuizer clean --idle 60d --dry-run ~/apps` print the same items, and `kuizer cache clean` replaces `kuizer cache --clean` in all docs.
