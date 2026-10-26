# Plan 008: List large files and old downloads with `kuizer big`

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/big.rs src/commands/big.rs src/output/big.rs src/dupes/mod.rs src/units.rs src/scan/walk.rs`
> Every commit listed must carry `Plan: plans/001-foundation-contracts.md`. Anything else is drift:
> compare the excerpts below with the live code and STOP on a mismatch.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW (report only, deletes nothing)
- **Depends on**: plans/001-foundation-contracts.md
- **Wave**: 1, parallel with 002 to 007 and 009
- **Model**: Sonnet 5.5, high effort
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

Most space a non-developer can free sits in a few large files: disk images, videos, archives and
installers left in Downloads. kuizer's design already reserves a report-only `big` source for
"large files that no artifact rule explains" (`docs/design.md` section 1, `docs/phases/10-sources.md`
section 4). This plan builds it and adds old downloads. It only lists. Whether kuizer may delete
personal files is an open product decision handled by plan 013.

## Current state

- Plan 001 created: `src/big.rs` with an empty `BigConfig` (serde default, `deny_unknown_fields`,
  `validate` returning `Ok(())`) on `Config.big`; `src/commands/big.rs` stub returning
  `AppError::NotBuilt("kuizer big")`; empty `src/output/big.rs` (declared `pub mod big` in
  `src/output/mod.rs`); `Command::Big(BigArgs)` with `BigArgs { paths: Vec<PathBuf>, min_size:
  Option<ByteSize>, limit: Option<usize>, command: Option<NestedCommand> }`; `Category::Files`;
  `ConfigError::Invalid { key, message }`.
- The duplicate finder already walks every file outside artifacts, excludes and protected data:
  `src/dupes/mod.rs` `find` builds a `Walker { scan, detector, excludes, mounts, protected,
  visitor }` from `crate::scan::walk` and calls `walker.walk_root(root)` per root. Its visitor
  `Files` (`src/dupes/mod.rs:103-160`) implements `Visitor::file(path, file_type)`, reads
  `symlink_metadata`, skips empty and small files and keeps

  ```rust
  struct Seen { path: PathBuf, size: u64, allocated: u64, identity: Option<(u64, u64)>, modified: Option<SystemTime> }
  ```

  The walker prunes matched artifacts (`node_modules` next to `package.json` and so on), so
  files inside them never reach `Visitor::file`.
- `src/commands/duplicates.rs` `run` shows how a command resolves roots
  (`scan::normalize_roots(&resolve_roots(config, args.paths)?)`), prints progress with
  `output::print_status` only when stderr is a terminal, and clears it.
- `src/units.rs`: `ByteSize` has `FromStr`, `Display`, `Serialize` and `Deserialize` (lines
  about 14-90). `Idle(pub Duration)` parses `"90d"`, `"2w"`, `"6mo"` with `FromStr` and has
  `Display`, but no serde impls.
- `crate::config::expand_home(path)` expands a leading `~`.
- GUI plan 014 will reuse the walk the GUI already does: it needs a way to pull large files out of
  a `dupes::Files` before `dupes::group` consumes it.
- Conventions: function items, no comments except allowed one-liners, `@name/@description/@example`
  doc blocks on exported functions, tests with `tempfile` only.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Unit tests | `cargo test -p kuizer -- big:: units:: dupes:: --test-threads=2` | all pass |
| Integration tests | `cargo test -p kuizer --test big -- --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |

## Scope

**In scope**: `src/big.rs`, `src/commands/big.rs`, `src/output/big.rs`, `src/units.rs` (serde for
`Idle` only), `src/dupes/mod.rs` (one new method on `Files`), `tests/big.rs` (create),
`docs/config.example.toml` (only inside `[big]`).

**Out of scope**:

- Any removal or `--clean` flag: plan 013, and only if deleting personal files is approved.
- `src/cli.rs`: `BigArgs` exists; if you need another flag, STOP.
- `src/scan/walk.rs`: use `Walker` and `Visitor` as they are.
- GUI and TUI.

## Git workflow

- Branch `general/008-big-files-listing` from `feat/general-cleanup`.
- One commit `feat: list large files and old downloads`, body line
  `Plan: plans/008-big-files-listing.md`. No Claude references. Do not push.

## Steps

### Step 1: Config

Add `Serialize` and `Deserialize` for `Idle` in `src/units.rs`, mirroring `ByteSize` (serialize
with `Display`, deserialize from a string through `FromStr`).

Give `BigConfig` these fields (serde default, `Default` as listed):

| Field | Type | Default |
|---|---|---|
| `roots` | `Vec<PathBuf>` | empty, meaning the home folder |
| `min_size` | `ByteSize` | `500MB` |
| `limit` | `usize` | `50` |
| `downloads` | `bool` | `true` |
| `downloads_age` | `Idle` | `90d` |
| `downloads_min_size` | `ByteSize` | `10MB` |

`validate` rejects `min_size` of 0 and `limit` of 0 with `ConfigError::Invalid`. Document every
key under `[big]` in `docs/config.example.toml`.

**Verify**: `cargo test -p kuizer config:: -- --test-threads=2` passes (including `parses_full_example`).

### Step 2: Find large files

In `src/big.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LargeFile {
    pub path: PathBuf,
    pub size: u64,
    pub allocated: u64,
    pub identity: Option<(u64, u64)>,
    pub modified: Option<SystemTime>,
}
```

- A private visitor `Large { min_size, found: Vec<LargeFile>, issues: Vec<Issue>, files: u64 }`
  implementing `crate::scan::walk::Visitor` the same way `dupes::Files` does, keeping files whose
  `metadata.len() >= min_size`.
- `pub fn find(config: &Config, roots: &[PathBuf], min_size: u64, on_progress: impl FnMut(u64)) -> Result<Found, ConfigError>`
  where `Found { files: Vec<LargeFile>, issues: Vec<Issue> }`. Build the `Walker` exactly like
  `dupes::find` does. Sort largest first, then by path. Call `on_progress(files_seen)` at most
  every 4096 files from the visitor's `counted` hook.
- In `src/dupes/mod.rs` add to `impl Files`:

  ```rust
  /// @name Files::at_least
  /// @description Returns the collected files of at least `min_size` bytes, largest first, so a
  /// caller that walked once for duplicates also gets the large files of that walk.
  ///
  /// @example
  /// let large = files.at_least(500_000_000);
  pub fn at_least(&self, min_size: u64) -> Vec<LargeFile>
  ```

**Verify**: `cargo build -p kuizer` exit 0.

### Step 3: Find old downloads

- `pub fn old_downloads(dir: &Path, age: Duration, min_size: u64, now: SystemTime, protected: &Protected) -> Vec<LargeFile>`:
  looks at the direct children of `dir` only. Skip symlinks and anything `protected.refuses`.
  For a file use its own size and modification time; for a directory use
  `scan::measure(child, &Cancel::new())` (`usage.allocated`, `newest`). Keep entries whose newest
  modification is older than `now - age` and whose size is at least `min_size`. Entries with an
  unknown modification time are never kept.
- `pub fn downloads_dir() -> Option<PathBuf>` returns `dirs::download_dir()`.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 4: Candidates, output and command

- `pub fn candidates(large: Vec<LargeFile>, old: Vec<LargeFile>) -> Vec<Candidate>`: rule
  `large-file` for the first list and `old-download` for the second, both `Category::Files`,
  `Safety::Review`, ecosystem `"files"`, `kind` from the path (`File` or `Dir`), ids from 1. A
  path in both lists appears once, as `old-download`.
- `src/output/big.rs`: `print_big(candidates, issues, quiet)` with columns SIZE, MODIFIED
  (`YYYY-MM-DD`, use `jiff` like the rest of `src/output`), KIND (`large` or `download`), PATH;
  "No large files found" when empty; summary line unless `quiet`. `big_report(roots, candidates,
  issues) -> String`: JSON with `schema_version`, `command: "big"`, `roots`, `candidates` in the
  shape `src/output/json.rs` uses for scan candidates (reuse its candidate serializer if it is
  `pub(crate)`; otherwise make it `pub(crate)` in a separate one-line change and note it in your
  report), `issues`, `summary`.
- `src/commands/big.rs`: roots are `args.paths`, else `config.big.roots` (each through
  `expand_home`), else the home folder; then `scan::normalize_roots`. `min_size` and `limit` come
  from the args, else the config. Old downloads only when `config.big.downloads` and the
  downloads folder exists. Show progress like `src/commands/duplicates.rs`. Text output shows at
  most `limit` large files; JSON shows all.

**Verify**: `cargo run -p kuizer -- big ~/Downloads --min-size 100MB` exits 0.

### Step 5: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need".

## Test plan

Unit tests in `src/big.rs` (temp dirs only):

1. `finds_large_files_outside_artifacts`: a sparse 600 MB file (`File::create` then `set_len`) in
   `home/Videos/`, another in `home/web/node_modules/pkg/` next to `home/web/package.json`, and a
   1 KB file. With `min_size = 500MB`, only the first is found.
2. `old_downloads_respect_age_and_size`: in a temp downloads dir, a 20 MB file modified 120 days
   ago (`File::set_modified`), a 20 MB file modified today, a 1 KB file modified 120 days ago, and
   a symlink. With `age = 90d`, `min_size = 10MB`, only the first is returned.
3. `a_path_in_both_lists_is_one_old_download`: `candidates` with the same path in both lists
   yields one candidate with rule `old-download`.

In `src/units.rs`: extend the existing `Idle` tests only with a serde round trip if one test can
cover it (`"90d"` deserializes and serializes back to `"90d"`).

Integration test `tests/big.rs` (`#![cfg(unix)]`, helper like `tests/clean.rs` `kuizer(home, args)`):

4. `big_lists_and_never_deletes`: a temp home with one sparse 600 MB file; `kuizer big --json
   <home>` lists it with `"category":"files"` and `"safety":"review"`; the file still exists.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -n "NotBuilt" src/commands/big.rs` prints nothing
- [ ] `grep -rn "delete::\|remove_file\|remove_dir" src/big.rs src/commands/big.rs` prints nothing
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files (plus the one-line `pub(crate)` change if you made it)
- [ ] No commit mentions Claude

## STOP conditions

- `Walker` or `Visitor` cannot be used from `src/big.rs` without changing `src/scan/walk.rs`.
- A sparse test file is reported with `allocated == 0` in a way that breaks sorting or totals.
  Report it; the size column must use `size` (apparent), totals `allocated`.
- You need a flag that `BigArgs` does not have.

## Maintenance notes

- Plan 013 (gated) adds removal; it relies on every big candidate being `Review`, so that
  `--select safe` never picks a personal file.
- Plan 014 calls `Files::at_least` on the files the GUI's walk already collected, so the GUI does
  not walk twice.
- Reviewers: check that nothing here deletes, and that the walk honors `scan.exclude` and the
  protected browser and credential folders (it does if it uses `Walker` like `dupes::find`).
