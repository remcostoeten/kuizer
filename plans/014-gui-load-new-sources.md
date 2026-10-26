# Plan 014: Load the trash, system and large-file sources into the GUI's scan report

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- crates/kuizer-gui/src/model.rs crates/kuizer-gui/src/snapshot.rs crates/kuizer-gui/src/activity.rs crates/kuizer-gui/src/start.rs`
> Every commit listed must carry `Plan:` with 001 or 002. Anything else is drift, in particular
> a merge of the branch `wip/gui-streaming-parts`: STOP and report.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED (the scan worker is the GUI's busiest code path)
- **Depends on**: plans/002-catalog-engine.md, plans/006-trash-source.md, plans/007-system-listing.md, plans/008-big-files-listing.md
- **Wave**: 2, parallel with 010 to 013, 015 and 016
- **Model**: Opus 5.5, high effort
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

The core now has three new sources: the trash (`src/trashcan.rs`), system package caches
(`src/system`) and large files and old downloads (`src/big.rs`). The GUI's scan worker
(`model::load`) builds one `Report` with every source. This plan adds the three new sources to
that report and to the cached snapshot, with progress shown in the existing activity panel. It
adds no sections or rows to the window; plan 017 renders them. Splitting it this way lets this
plan run while plan 015 changes the GUI's key handling.

## Current state

- `crates/kuizer-gui/src/model.rs`:
  - `pub struct Report { roots, options, candidates, issues, summary, caches, toolchains, docker,
    docker_issues, repos, duplicates, history, disks, scanned }` (around line 140).
  - `pub fn load(config, roots, options: &ScanOptions, cancel, activity: &Mutex<Activity>) ->
    Result<Report, String>` (around line 298): starts the Docker listing on its own thread, runs the
    folder walk with `repos::start_scan(&config, roots, cancel.clone(), duplicate_options,
    pin_search)` and `scan::collect_with`, receives `walked.files` (an `Option<dupes::Files>`),
    groups duplicates with `dupes::group(files, options, ..)`, then measures caches, toolchains,
    repos and history one after another, each wrapped in `activity.begin(Stage::X, ..)` and
    `activity.finish(Step::X, elapsed, summary)` and followed by
    `if cancel.is_cancelled() { return Err(CANCELLED.to_owned()); }`.
  - `pub enum Stage { Starting, Scanning(Progress), Duplicates(..), Caches, Toolchains, Docker, Repos, History }`.
- `crates/kuizer-gui/src/activity.rs`: `pub enum Step { Folders, Duplicates, Caches, Toolchains,
  Repos, Docker, History }` with `Step::ALL` and labels; the panel lists steps in that order.
- `crates/kuizer-gui/src/start.rs`: `ScanOptions { categories, min_size, max_depth, caches,
  toolchains, docker, repos, duplicates, one_file_system }`, serde with `#[serde(default =
  "enabled")]` on newer flags, `ScanOptions::from_config`.
- `crates/kuizer-gui/src/snapshot.rs`: `Stored` mirrors `Report`; `SCHEMA` guards the format;
  `Stored::from(&Report)` and `Stored::restore(self) -> Report`.
- Core APIs to call:
  - Trash: `trashcan::home_trash() -> Option<(PathBuf, Layout)>`, `trashcan::inspect(path,
    layout) -> Result<Option<Folder>, Issue>`, `trashcan::entries(folders, &cancel) -> Vec<(Folder, Candidate)>`.
  - System: `system::for_user(&config.system) -> Listing { items, issues }`, `system::entries(items)`.
  - Big: `dupes::Files::at_least(&self, min_size) -> Vec<LargeFile>`, `big::old_downloads(dir,
    age, min_size, now, &Protected)`, `big::downloads_dir()`, `big::candidates(large, old)`,
    `big::find(config, roots, min_size, on_progress)`; config in `config.big` (`min_size`,
    `downloads`, `downloads_age`, `downloads_min_size`).
- A branch `wip/gui-streaming-parts` rewrites `model::load` to stream parts. If it has merged
  before you start, this plan's step 3 no longer matches: STOP.
- Conventions: function items, no comments except allowed one-liners; GPUI 0.2.2 (read its source
  in `~/.cargo/registry/src/*/gpui-0.2.2` instead of guessing); nothing on the UI thread blocks
  on I/O.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| GUI tests | `cargo test -p kuizer-gui -- --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Run | `cargo run -p kuizer-gui -- ~/Downloads` | window opens, scan finishes |

## Scope

**In scope**: `crates/kuizer-gui/src/model.rs`, `snapshot.rs`, `activity.rs`, `start.rs`.

**Out of scope**: `app.rs`, `app/*.rs`, `tab.rs`, `commands.rs`, `ui.rs`, `icon.rs` (plans 015
and 017 edit them in parallel). Removal of the new sources (plan 018). Core code: if the core
lacks something, STOP.

## Git workflow

- Branch `general/014-gui-load-new-sources` from `feat/general-cleanup` (with wave 1 merged).
- One commit `feat(gui): load trash, system and large files into the scan report`, body line
  `Plan: plans/014-gui-load-new-sources.md`. No Claude references. Do not push.

## Steps

### Step 1: Options

Add to `ScanOptions`, each `#[serde(default = "enabled")]`: `trash: bool`, `system: bool`,
`big: bool`. `from_config` sets all three to `true`. Existing snapshots keep loading because of
the serde defaults.

**Verify**: `cargo test -p kuizer-gui start:: -- --test-threads=2` passes.

### Step 2: Report fields and snapshot

Add to `Report`: `trash: Vec<(trashcan::Folder, Candidate)>`, `trash_issues: Vec<Issue>`,
`system: Vec<(system::Item, Candidate)>`, `system_issues: Vec<String>`, `big: Vec<Candidate>`.
Add totals in the style of `cache_total`: `trash_total`, `system_total`, `big_total` (each with
the same `safety: Option<Safety>` filter).

In `snapshot.rs` store and restore all five (store `Candidate`s as `StoredCandidate`, like the
other sources), and increment `SCHEMA`.

**Verify**: `cargo build -p kuizer-gui` exit 0.

### Step 3: Load them

In `model::load`:

1. Start the system listing next to the Docker listing, on its own named thread
   (`kuizer-gui-system`), when `options.system`.
2. When `options.big`, make sure the walk collects files even if duplicates are off: pass
   `Some(Options { min_size: config.big.min_size.0, ..Options::from_config(&config) })` as the
   duplicate options in that case, and only run `dupes::group` when `options.duplicates`. Before
   `dupes::group` consumes the files, take `files.at_least(config.big.min_size.0)`. When both are
   on, the duplicate min size (default 4 KiB) is smaller, so `at_least` still finds every large file.
3. After toolchains, when `options.trash`: inspect the home trash and measure it with
   `trashcan::entries`, as a `Step::Trash` with `Stage::Trash`.
4. When `options.big` and `config.big.downloads`: run `big::old_downloads` on the downloads
   folder and build `big::candidates(large, old)`, as `Step::Big` with `Stage::Big`.
5. Join the system thread before history, as `Step::System` with `Stage::System`; a panicked or
   failed thread gives an empty listing and one issue.
6. Keep the `cancel` checks between steps.

Add the new `Stage` variants and `Step` variants (`Trash`, `System`, `Big`, placed after
`Toolchains` in `Step::ALL`) with labels "Trash", "System", "Large files" and finish summaries in
the existing style (`"{n} items, {size}"`).

**Verify**: `cargo build -p kuizer-gui` exit 0; `cargo run -p kuizer-gui -- ~/Downloads`
finishes a scan and the activity panel shows the three new steps.

### Step 4: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need".

## Test plan

- `snapshot.rs`: extend the existing round-trip test (the one that builds a `Report` and checks
  `restore`) with one trash folder, one system item and one big candidate, and assert they come
  back. Model after the existing docker or toolchain part of that test.
- No test for `load` itself unless an existing one covers similar parts; if one exists, extend it
  with `options.trash = false, system = false, big = false` to keep it hermetic.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- `wip/gui-streaming-parts` has been merged, or `model::load` no longer has the sequential shape
  described above.
- A core function listed above is missing or has a different signature.
- The GUI needs a change in `app.rs` to compile (for example an exhaustive match on `Stage` or
  `Step`). Add the minimal arm only if it is a pure `match` arm with an existing neutral
  rendering; otherwise STOP.

## Maintenance notes

- Plan 017 renders the new report fields as sections and adds start-screen toggles for the three
  options; plan 018 makes them removable.
- `big` reuses the folder walk, so it covers the scanned folders only, not the whole home folder
  as `kuizer big` does. Plan 017 should say so in the section's empty state.
- Reviewers: check that no new work runs on the UI thread and that cancel stops the new steps.
