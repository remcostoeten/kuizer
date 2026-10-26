# Plan 010: Mark a cache as review while the app that owns it is running

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/platform src/caches.rs src/catalog`
> Every commit listed must carry `Plan:` with 001, 002, 004, 005 or 007. Anything else is drift:
> compare the excerpts below with the live code and STOP on a mismatch.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW (only makes items less eligible for bulk selection)
- **Depends on**: plans/002-catalog-engine.md, plans/007-system-listing.md (for `src/external.rs`)
- **Wave**: 2, parallel with 011 to 016
- **Model**: Sonnet 5.5, high effort
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

Deleting the cache of a running app (Chrome, Discord, Spotify, Steam) either has no effect,
because the app writes it back at once, or confuses the app. Catalog entries list the executable
names of their owner in `processes`. This plan detects running programs on Linux and macOS and,
when an entry's owner runs, marks that cache `review` with a note asking to close the app first.
`review` items are never selected in bulk and need a typed confirmation, so nothing else changes.

## Current state

- `src/catalog/mod.rs` `Entry.processes: Vec<String>` (plan 001); data files fill it (plans 004,
  005, 011), for example `processes = ["spotify"]`.
- `src/caches.rs` (after plan 002): `pub fn find(locations: &Locations, protected: &Protected,
  catalog: &CatalogConfig) -> Vec<KnownCache>`; `KnownCache` carries `processes`, `safety` and
  `note` as owned values. Read the function before changing it; plan 002 may have structured it
  with a private helper.
- `src/external.rs` (plan 007): `pub(crate) fn run(program: &str, args: &[&str], timeout:
  Duration) -> Result<String, Failure>` runs a program without a shell.
- `src/platform/mod.rs` holds small platform helpers behind `cfg` (`#[cfg(target_os = "linux")]
  mod linux;`, `#[cfg(unix)]` functions with `#[cfg(not(unix))]` fallbacks). No Windows code.
- On Linux, `/proc/<pid>/comm` holds the executable name truncated to 15 bytes plus a newline,
  readable for every process. On macOS, `/bin/ps -Ac -o comm=` prints one executable name per line.
- Conventions: function items, no comments except allowed one-liners, `@name/@description/@example`
  doc blocks on exported functions, tests with `tempfile`.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Tests | `cargo test -p kuizer -- platform:: caches:: --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| GUI build | `cargo build -p kuizer-gui` | exit 0 |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |

## Scope

**In scope**: `src/platform/mod.rs` (one `mod` line and a re-export), `src/platform/process.rs`
(create), `src/caches.rs`.

**Out of scope**: catalog data files, GUI and TUI code (they show `safety` and `note` already),
the delete guard.

## Git workflow

- Branch `general/010-running-app-check` from `feat/general-cleanup` (with wave 1 merged).
- One commit `feat(core): mark caches of running apps as review`, body line
  `Plan: plans/010-running-app-check.md`. No Claude references. Do not push.

## Steps

### Step 1: List running programs

`src/platform/process.rs`:

- `pub fn running_programs() -> HashSet<String>`: Linux reads every numeric directory in
  `/proc`, then `comm`, trimmed; macOS runs `/bin/ps -Ac -o comm=` through `external::run` with a
  2 second timeout and keeps the last path component of each trimmed line. Any error yields what
  was collected so far (possibly empty); never panic.
- `pub fn is_running(running: &HashSet<String>, name: &str) -> bool`: true when the set contains
  `name`, or contains `name` truncated to 15 bytes on Linux.

Declare `mod process;` in `src/platform/mod.rs` and re-export both functions.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 2: Apply it in `caches::find`

Split `find` so the running set can be injected in tests:

```rust
pub fn find(locations: &Locations, protected: &Protected, catalog: &CatalogConfig) -> Vec<KnownCache> {
    find_with(locations, protected, catalog, &platform::running_programs)
}

fn find_with(..., running: &dyn Fn() -> HashSet<String>) -> Vec<KnownCache>
```

Call `running()` at most once, and only when at least one found cache has a non-empty
`processes`. For every cache whose `processes` contains a running name: set `safety =
Safety::Review` and prepend `"<tool> is running; close it first. "` to `note`.

**Verify**: `cargo build -p kuizer && cargo build -p kuizer-gui` exit 0.

### Step 3: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need".

## Test plan

1. `platform::process` (`#[cfg(target_os = "linux")]`): `running_programs()` contains the name in
   `/proc/self/comm`.
2. `caches` test `running_owner_turns_a_safe_cache_into_review`: a fake home with a user catalog
   location (`safety = "safe"`, `processes = ["fakeapp"]`), `find_with` with a closure returning
   `{"fakeapp"}` → `Review` and the note starts with the tool name; with an empty set → `Safe`.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- `find` no longer has the shape described (plan 002 changed it differently): report what you found.
- Reading `/proc` takes more than 50 ms on this machine (`cargo test` timing or a quick
  `std::time::Instant` measurement in a test you then delete). Report the number.

## Maintenance notes

- The check runs when the list is built, not again at removal; an app started in between is not
  caught. The removal itself is still guarded.
- Reviewers: check that the check can only lower safety, never raise it.
