# Plan 013: Let `kuizer big --clean` move selected personal files to the trash

> **GATED**: execute only if the orchestrator recorded decision D2 = "kuizer may delete personal
> files that cannot be regenerated" in `plans/README.md`. Otherwise this plan is REJECTED.
>
> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/big.rs src/commands/big.rs src/cli.rs tests/big.rs`
> Every commit listed must carry `Plan:` with 001 or 008. Anything else is drift: STOP.

## Status

- **Priority**: P3
- **Effort**: M
- **Risk**: HIGH (removes files nothing can regenerate)
- **Depends on**: plans/008-big-files-listing.md
- **Wave**: 2, parallel with 010 to 012 and 014 to 016
- **Model**: Opus 5.5, high effort
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

kuizer's design says it only removes what can be regenerated, so `big` is report-only. A general
PC cleaner is expected to let the user delete a forgotten 4 GB disk image. If that change is
approved, this plan adds removal with the strongest guards kuizer has: every big item is
`review`, so it is never selected in bulk and the confirmation requires typing the item count;
the file must be unchanged since it was listed; and the default mode stays the trash.

## Current state

- `src/big.rs` (plan 008): `LargeFile { path, size, allocated, identity, modified }`,
  `find(config, roots, min_size, on_progress)`, `old_downloads(dir, age, min_size, now,
  protected)`, `downloads_dir()`, `candidates(large, old) -> Vec<Candidate>` with rules
  `large-file` and `old-download`, `Category::Files`, `Safety::Review`.
- `src/commands/big.rs` (plan 008): resolves roots (args, `big.roots`, home), lists, prints.
- `src/cli.rs`: `BigArgs { paths, min_size, limit, command }`, `big_help()`.
- Removal pipeline (`src/commands/removal.rs`): `removal::select(candidates, selections)` with the
  default `[Safe]` (selects nothing here, since every item is `Review`); `removal::mode(&args,
  config)` respects `--dry-run`, `--permanent` and `delete.mode`; `removal::run(&request, selected,
  issues)` previews, confirms (typing the count for `review` items), removes with
  `request.remove`, records history; `removal::files(guard, candidate, mode)` removes a path
  through `delete::delete`, which checks the guard (roots, symlink, identity, mount point) first.
- `tests/big.rs` (plan 008) and `tests/clean.rs` show the `kuizer(home, args)` helper with
  `HOME` and `XDG_*` pointing into a temp dir; the trash then lands in the temp
  `XDG_DATA_HOME/Trash`.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Unit tests | `cargo test -p kuizer big:: -- --test-threads=2` | all pass |
| Integration tests | `cargo test -p kuizer --test big -- --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |

Never run `kuizer big --clean` without `HOME` and `XDG_DATA_HOME` pointing into a temp folder.

## Scope

**In scope**: `src/big.rs`, `src/commands/big.rs`, `src/cli.rs` (only `BigArgs` and `big_help`),
`tests/big.rs`.

**Out of scope**: `src/commands/removal.rs`, the delete guard, the duplicates source, GUI.

## Git workflow

- Branch `general/013-big-files-removal` from `feat/general-cleanup` (with wave 1 merged).
- One commit `feat: move selected large files and old downloads to the trash`, body line
  `Plan: plans/013-big-files-removal.md`. No Claude references. Do not push.

## Steps

### Step 1: CLI

`BigArgs` gains `clean: bool` (help: "Remove the selected files after a preview; nothing is
selected unless you pass --select") and `#[command(flatten)] removal: RemovalArgs`. Add examples:
`("kuizer big", "--clean --select old-download", "Move old downloads to the trash, after typing the count")`
and `("kuizer big", "--clean --select all --dry-run", "Show what would be removed")`.

**Verify**: `cargo test -p kuizer cli:: -- --test-threads=2` passes.

### Step 2: Remove with a size and time check

In `src/big.rs` add `pub fn remove(guard: &Guard, candidate: &Candidate, mode: Mode) ->
Result<(), DeleteError>`: for a file, re-read `symlink_metadata` and refuse with
`DeleteError::Tool("the file changed since it was listed".into())` when its `len()` differs from
`candidate.usage.apparent` or its modification time differs from `candidate.newest`; then call
`crate::commands::removal::files(guard, candidate, mode)` (or `delete::delete` with the same
`Target`, if `removal::files` is not reachable from here). Directories (old downloads that are
folders) skip the size check and keep the identity check the guard does.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 3: Command

In `src/commands/big.rs`, when `args.clean || args.removal.dry_run`:

1. `removal::check_selection(&args.removal.select, &["large-file", "old-download"])?`.
2. When `args.removal.select` is empty, print "Nothing selected. Large files are never selected
   as safe; pass --select large-file, --select old-download or --select all." and return `Ok(())`.
3. Otherwise select, and call `removal::run` with `command: "big"`, `roots` = the resolved big
   roots plus the downloads folder when old downloads were listed, `mode: removal::mode(&args.removal,
   config)`, `remove: big::remove`.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 4: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need".

## Test plan

In `tests/big.rs`:

1. `clean_without_select_removes_nothing`: temp home with one sparse 600 MB file; `kuizer big
   --clean --yes <home>` exits 0, prints "Nothing selected", the file exists.
2. `clean_moves_selected_files_to_the_trash`: two sparse 600 MB files; `kuizer big --clean
   --select all --yes <home>` exits 0; both are gone from their folder and present under the temp
   `XDG_DATA_HOME/Trash/files`; `kuizer history --json` lists one run with two items.
3. `a_file_that_grew_is_refused`: unit test in `src/big.rs`: build the candidate, append a byte
   to the file, call `remove` → `Err`, file still there.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -n "Safety::Safe" src/big.rs` prints nothing
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- `removal::run` would remove items without the typed-count confirmation for `review` items
  (check the confirm path in `src/commands/removal.rs`).
- A test writes outside its temp folder.

## Maintenance notes

- `docs/design.md` must state the new rule (plan 019): personal files may be removed, never in
  bulk, always with a typed confirmation.
- Plan 018 adds the same removal to the GUI if this plan lands.
- Reviewers: run test 2 and check the trash location is inside the temp dir.
