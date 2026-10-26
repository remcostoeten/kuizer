# Plan 006: List and empty the user's trash with `kuizer trash`

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/trashcan.rs src/commands/trash.rs src/output/trash.rs src/delete src/commands/removal.rs`
> Every commit listed must carry `Plan: plans/001-foundation-contracts.md`. Anything else is drift:
> compare the excerpts below with the live code and STOP on a mismatch.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED (permanent deletion, but only inside the trash)
- **Depends on**: plans/001-foundation-contracts.md
- **Wave**: 1, parallel with 002 to 005 and 007 to 009
- **Model**: Opus 5.5, high effort
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

Emptying the trash is the most basic thing a general PC cleaner does, and kuizer's own default
removal mode fills the trash. This plan adds a `trash` source: `kuizer trash` lists the user's
trash with its size, `kuizer trash --clean` empties it permanently after the usual preview and
confirmation, and the removal is recorded in the history. It covers the home trash on Linux
(freedesktop layout) and `~/.Trash` on macOS. Trash folders on other mounts are out of scope for
now because the delete guard refuses paths under `/run` and `/media`.

## Current state

- Plan 001 created: an empty `src/trashcan.rs` (declared `pub mod trashcan` in `src/lib.rs`),
  `src/commands/trash.rs` with a stub `run` returning `Err(AppError::NotBuilt("kuizer trash"))`,
  an empty `src/output/trash.rs` (declared `pub mod trash` in `src/output/mod.rs`), the CLI
  command `Command::Trash(TrashArgs)` with `TrashArgs { clean: bool, removal: RemovalArgs,
  command: Option<NestedCommand> }`, and `Category::Trash`.
- Exemplar source to copy the shape from: Docker (`src/docker.rs`, `src/commands/docker.rs`).
  `src/commands/docker.rs` lists, prints text or JSON, and for `--clean` builds a
  `removal::Request { command, roots, mode, yes, json, remove }` and calls
  `removal::run(&request, removal::select(candidates, &args.removal.select), &issues)`.
  Docker forces `Mode::Permanent` unless `--dry-run`; do the same.
- `src/commands/removal.rs`: `pub type Remove = fn(&Guard, &Candidate, Mode) -> Result<(), DeleteError>;`.
  `removal::run` builds `Guard::new(request.roots)`, asks for confirmation (typing the count when
  any item is `review`), calls `remove` per candidate and records each success in the history.
- `src/delete/mod.rs`: `delete(guard, &Target { path, identity }, mode)` runs `guard.check` then
  removes (`Mode::Permanent` uses `remove_dir_all` or `remove_file`). `DeleteError::Tool(String)`
  exists for free-form failures.
- `src/delete/guard.rs` `Guard::check` refuses: non-absolute paths, paths with fewer than 3
  components, system prefixes (including `/run`), anchors (a root, the home folder, `/` or any of
  their parents), paths outside every root, the current directory's ancestors, protected app
  data, symlinks, changed identity, and mount points. Guard roots are anchors themselves, so a
  root can never be removed, only things inside it.
- `src/scan/size.rs` `scan::measure(path, &cancel)` returns `Measured { usage, newest, identity }`.
- `src/platform/mod.rs` `platform::identity(&metadata) -> Option<(u64, u64)>` (device, inode).
- The freedesktop trash: `$XDG_DATA_HOME/Trash` (default `~/.local/share/Trash`) with `files/`
  (the trashed items), `info/` (one `<name>.trashinfo` per item) and an optional
  `directorysizes` cache file. `dirs::data_dir()` honors `XDG_DATA_HOME`.
- macOS: `~/.Trash`, readable only when the terminal or app has Full Disk Access; otherwise
  `read_dir` fails with permission denied.
- Integration test pattern: `tests/clean.rs` spawns `env!("CARGO_BIN_EXE_kuizer")` with `HOME`,
  `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` pointing into a `TempDir` and `NO_COLOR=1`.
- Conventions: function items, no comments except allowed one-liners, `@name/@description/@example`
  doc blocks on exported functions (see `src/docker.rs` `list`), `thiserror` errors.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Unit tests | `cargo test -p kuizer trashcan:: -- --test-threads=2` | all pass |
| Integration tests | `cargo test -p kuizer --test trash -- --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |

Never run `kuizer trash --clean` without `HOME` and `XDG_DATA_HOME` pointing into a temp folder.
It empties your real trash.

## Scope

**In scope**: `src/trashcan.rs`, `src/commands/trash.rs`, `src/output/trash.rs`, `tests/trash.rs` (create).

**Out of scope**:

- `src/delete/`, `src/protect.rs`: plan 003 edits them in parallel. Use the guard as it is.
- `src/cli.rs`: `TrashArgs` is already defined. If you need another flag, STOP.
- Trash folders on other mounts (`.Trash-<uid>`), GUI and TUI.

## Git workflow

- Branch `general/006-trash-source` from `feat/general-cleanup`.
- One commit `feat: list and empty the trash`, body line `Plan: plans/006-trash-source.md`.
  No Claude references. Do not push.

## Steps

### Step 1: Find the trash

In `src/trashcan.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub path: PathBuf,
    pub items: usize,
    pub layout: Layout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    Freedesktop,
    Finder,
}
```

- `pub fn home_trash() -> Option<(PathBuf, Layout)>`: on macOS `dirs::home_dir()?.join(".Trash")`
  with `Finder`; elsewhere `dirs::data_dir()?.join("Trash")` with `Freedesktop`.
- `pub fn inspect(path: &Path, layout: Layout) -> Result<Option<Folder>, Issue>`: the folder must
  exist as a real directory (not a symlink); `items` counts the entries of `files/` (Freedesktop)
  or of the folder itself excluding `.DS_Store` (Finder). Zero items returns `Ok(None)`. A
  permission error returns `Err(Issue::from_io(..))`; for `Finder`, replace the message with
  "cannot read the trash; give this terminal or kuizer Full Disk Access in System Settings".
- `pub fn entries(folders: Vec<Folder>, cancel: &Cancel) -> Vec<(Folder, Candidate)>`: measures
  each folder with `scan::measure` and builds a `Candidate { id: index + 1, path, kind: Kind::Dir,
  rule: RuleInfo { name: "trash", category: Category::Trash, safety: Safety::Safe, ecosystem:
  "trash" }, project: None, usage, newest, identity, project_last_active: None }`.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 2: Empty it through the guard

- `pub fn roots(folder: &Folder) -> Vec<PathBuf>`: `[path/files, path/info]` for Freedesktop,
  `[path]` for Finder. These become the guard roots, so the folders themselves can never be removed.
- `pub fn remove(guard: &Guard, candidate: &Candidate, mode: Mode) -> Result<(), DeleteError>`:
  1. `Mode::DryRun` returns `Ok(())`.
  2. Revalidate the trash folder: `fs::symlink_metadata(&candidate.path)` must be a real directory
     and `platform::identity(&metadata)` must equal `candidate.identity`; otherwise
     `Err(DeleteError::Tool("the trash folder changed since it was listed".into()))`.
  3. Detect the layout again from the path (`files/` present means Freedesktop).
  4. For every child of the roots (sorted, `.DS_Store` skipped for Finder): if
     `symlink_metadata` says it is a symlink, count it as skipped and continue; otherwise call
     `delete::delete(guard, &Target { path: child, identity: None }, Mode::Permanent)` and count
     failures, keeping the first error text.
  5. Leave `directorysizes` and the folder structure in place.
  6. If anything failed or was skipped, return `Err(DeleteError::Tool(format!("{failed} items
     could not be removed, {skipped} links were left in the trash: {first}")))` (drop the parts
     that are zero); otherwise `Ok(())`.

Always permanent, whatever `mode` says, because the trash cannot go to the trash.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 3: Output

In `src/output/trash.rs`, following `print_caches` (`src/output/text.rs:425`) and `cache_report`
(`src/output/json.rs:278`):

- `pub fn print_trash(entries: &[(Folder, Candidate)], issues: &[Issue], quiet: bool)`: columns
  SIZE, ITEMS, PATH; "The trash is empty" when there are no entries; issues printed as
  `kuizer: <message>` to stderr; unless `quiet`, a summary line with the total.
- `pub fn trash_report(entries: &[(Folder, Candidate)], issues: &[Issue]) -> String`: JSON with
  `schema_version` (`crate::output::SCHEMA_VERSION`), `command: "trash"`, `folders: [{ path,
  items, allocated_bytes, apparent_bytes, newest_modified }]`, `issues`, `summary: { items, bytes }`.
  Reuse the private helpers of `json.rs` only if they are public; otherwise write the few lines
  needed here.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 4: The command

Replace the stub in `src/commands/trash.rs`:

1. `removal::check_selection(&args.removal.select, &["trash"])?`.
2. Find the home trash; `inspect` it; collect the folder or the issue.
3. Without `--clean` and `--dry-run`: print text or JSON and return.
4. With them: `select` the candidates, build `Request { command: "trash", roots:
   &trashcan::roots(folder), mode: DryRun or Permanent, yes, json, remove: trashcan::remove }`
   and call `removal::run`.

Use the `Cancel::new()` from `crate::scan`. `AppError::NotBuilt` must no longer be used here.

**Verify**: `HOME=$(mktemp -d) XDG_DATA_HOME="$HOME/.local/share" cargo run -p kuizer -- trash`
prints "The trash is empty" and exits 0.

### Step 5: Tests and full checks

Write the tests in "Test plan" and run every command in "Commands you will need".

## Test plan

Unit tests in `src/trashcan.rs` (temp dirs only):

1. `inspects_a_freedesktop_trash`: `Trash/files/{a.txt,dir/b}`, `Trash/info/{a.txt,dir}.trashinfo`
   → `items == 2`; an empty `files/` → `None`; a `Trash` that is a symlink → `None` or an issue,
   never a folder.
2. `remove_empties_files_and_info_and_keeps_links`: build the tree above plus a symlink in
   `files/` pointing to a file outside the trash. Build the guard as `removal::run` would
   (`Guard::new(&roots(&folder)).with_cwd(None)`) and call `remove`. Afterwards `files/` holds only
   the link, `info/` is empty, the link's target still exists, `directorysizes` still exists, and
   the result is an `Err` mentioning 1 link.
3. `remove_refuses_a_swapped_folder`: change `candidate.identity` to `Some((0, 0))` → `Err`, and
   nothing was removed.

Integration test `tests/trash.rs` (`#![cfg(target_os = "linux")]`, same `kuizer(home, args)`
helper as `tests/clean.rs`):

4. `lists_and_empties_the_home_trash`: create the trash under the temp `XDG_DATA_HOME`; `kuizer
   trash --json` lists one folder with 2 items; `kuizer trash --clean --yes` exits 0; `files/` and
   `info/` are empty; a file outside the trash in the temp home is untouched; `kuizer history
   --json` lists one run.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -n "NotBuilt" src/commands/trash.rs` prints nothing
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- The guard refuses children of the home trash for a reason other than symlink (for example
  `Anchor` or `OutsideRoots`). Report the refusal; do not change the guard.
- A test would need to touch a real trash folder.
- `trash::os_limited` functions seem necessary. They act on every trash on the machine; this plan
  avoids them on purpose.

## Maintenance notes

- Items kuizer moved to the trash earlier cannot be restored after emptying; `kuizer history`
  already shows them as emptied. Plan 017 should say so in the GUI confirmation.
- Mount trashes (`<mount>/.Trash-<uid>`) need a guard rule for `/run/media` and `/media`; that is a
  separate decision, not part of this plan.
- Reviewers: check that `remove` never deletes the trash folder itself, never follows a link, and
  is permanent regardless of `delete.mode`.
