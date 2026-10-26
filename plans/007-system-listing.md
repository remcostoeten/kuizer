# Plan 007: List system package caches, old snap revisions and the journal with `kuizer system`

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/system src/commands/system.rs src/output/system.rs src/docker.rs src/lib.rs`
> Every commit listed must carry `Plan: plans/001-foundation-contracts.md`. Anything else is drift:
> compare the excerpts below with the live code and STOP on a mismatch.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW (read-only: runs package tools in listing modes and measures folders)
- **Depends on**: plans/001-foundation-contracts.md
- **Wave**: 1, parallel with 002 to 006, 008 and 009
- **Model**: Sonnet 5.5, high effort
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

On a typical Linux machine the package manager's download cache, old snap revisions and the
systemd journal often take several gigabytes; on macOS, Homebrew's old versions and downloads do.
A general PC cleaner should show them. This plan adds a read-only `system` source: `kuizer system`
lists what each installed package tool could free. Removing it needs root for most tools and is
a separate, gated plan (012). Nothing here deletes anything.

## Current state

- Plan 001 created: `src/system/mod.rs` with an empty `SystemConfig` (serde default,
  `deny_unknown_fields`, `validate` returning `Ok(())`), `Config.system`, `src/commands/system.rs`
  with a stub returning `AppError::NotBuilt("kuizer system")`, an empty `src/output/system.rs`
  (declared `pub mod system` in `src/output/mod.rs`), `Command::System(SystemArgs)` with
  `SystemArgs { command: Option<NestedCommand> }`, and `ConfigError::Invalid { key, message }`.
- Exemplar: the Docker source.
  - `src/docker.rs` runs external CLIs with private helpers: `spawn(program, args)` (retries
    `ETXTBSY`, stdin null, stdout and stderr piped), `drain(pipe)` (reads a pipe on a thread),
    `run(program, args, timeout) -> Result<String, Failure>` (kills the child after `timeout`) and
    `enum Failure { Missing, TimedOut, Failed(String) }`. See `src/docker.rs:105-180`.
  - `Item` has a pseudo path `docker:image/<id>` and builds a `Candidate` with `Kind::External`
    (`src/docker.rs:54-100`).
  - `list(engines, timeout, recent_hours) -> Listing { items, issues }`: a missing tool yields an
    issue string, not an error.
  - Tests fake the tool with a shell script written into a temp dir (`script`, `fake_docker` in
    `src/docker.rs` tests).
- `src/units.rs`: `ByteSize` parses `"1.5GiB"`, `"200MB"` (`FromStr`) and displays sizes.
- `src/scan/size.rs`: `scan::measure(path, &Cancel::new())` sizes a folder without following symlinks.
- Conventions: function items, no comments except allowed one-liners, `@name/@description/@example`
  doc blocks on exported functions, `thiserror` errors, platform code behind `cfg`, no Windows code.
  Do not rely on tools only one distribution ships: detect each tool and skip what is missing.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Unit tests | `cargo test -p kuizer -- system:: docker:: --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Try it | `cargo run -p kuizer -- system` | a table or "Nothing to clean up", exit 0 |

## Scope

**In scope**: `src/system/` (create files inside), `src/commands/system.rs`, `src/output/system.rs`,
`src/external.rs` (create), `src/docker.rs` (only to use `src/external.rs`), `src/lib.rs` (one line),
`docs/config.example.toml` (only inside `[system]`).

**Out of scope**:

- Any removal, `sudo`, `pkexec`, or command that changes the system: plan 012.
- `src/cli.rs`: `SystemArgs` already exists; if you need a flag, STOP.
- Coredumps (no tool removes them selectively), `/var/log` files, GUI and TUI.

## Git workflow

- Branch `general/007-system-listing` from `feat/general-cleanup`.
- One commit `feat: list system package caches, snap revisions and the journal`, body line
  `Plan: plans/007-system-listing.md`. No Claude references. Do not push.

## Steps

### Step 1: Share the external command runner

Move `spawn`, `drain`, `run` and `Failure` from `src/docker.rs` into `src/external.rs` as
`pub(crate)` items, unchanged, and add `pub(crate) mod external;` to `src/lib.rs`. `src/docker.rs`
imports them. No behavior change.

**Verify**: `cargo test -p kuizer docker:: -- --test-threads=2` passes unchanged.

### Step 2: The item model

In `src/system/mod.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Manager { Pacman, Apt, Dnf, Zypper, Flatpak, Snap, Homebrew, Journal }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub manager: Manager,
    pub id: String,
    pub label: String,
    pub bytes: u64,
    pub size_known: bool,
    pub safety: Safety,
    pub needs_root: bool,
    pub note: String,
}
```

- `Manager::name()` returns the kebab-case name; `Manager::ALL`; `Manager::from_name`.
- `Item::path()` returns `PathBuf::from(format!("system:{}/{}", manager.name(), id))`.
- `Item::candidate(id: u64) -> Candidate`: `Kind::External`, `RuleInfo { name: manager.name(),
  category: Category::Logs` for `Journal` and `Category::Cache` otherwise, `safety, ecosystem:
  "system" }`, usage `allocated = apparent = bytes`, `entries = 1`, `partial = !size_known`.
- `pub fn entries(items: Vec<Item>) -> Vec<(Item, Candidate)>`, ids from 1, like `docker::entries`.

Give `SystemConfig` these fields (serde default):

```rust
pub struct SystemConfig {
    pub managers: Vec<Manager>,   // default: Manager::ALL
    pub timeout_secs: u64,        // default: 10
}
```

`validate` rejects `timeout_secs = 0` with `ConfigError::Invalid { key: "system.timeout_secs", .. }`.
Document both keys under `[system]` in `docs/config.example.toml`.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 3: One lister per manager

Create `src/system/list.rs` with `pub fn list(config: &SystemConfig, tools: &Tools) -> Listing`
where `Listing { items: Vec<Item>, issues: Vec<String> }` and `Tools` maps each manager to the
program it runs and the folders it reads, so tests can point them at fakes:

```rust
pub struct Tools {
    pub path: Vec<PathBuf>,          // directories searched for programs, default: $PATH split
    pub pacman_conf: PathBuf,        // default /etc/pacman.conf
    pub snaps_dir: PathBuf,          // default /var/lib/snapd/snaps
}
```

The `//` remarks in these sketches are for you; do not copy them into the code. A manager is
skipped silently when its program is not found in `tools.path`. Each lister:

| Manager | Program | How to list (read-only) | Item | Safety, root |
|---|---|---|---|---|
| Pacman | `pacman` | Read `CacheDir` lines from `pacman_conf` (default `/var/cache/pacman/pkg/`), `scan::measure` each | `pacman-cache` | Safe, root. Note: "Downloaded packages. Cleaning keeps the two newest versions of each, so you can still downgrade." |
| Apt | `apt-get` | measure `/var/cache/apt/archives` | `apt-cache` | Safe, root |
| Dnf | `dnf` | measure `/var/cache/libdnf5` and `/var/cache/dnf` (whichever exist, summed) | `dnf-cache` | Safe, root |
| Zypper | `zypper` | measure `/var/cache/zypp/packages` | `zypper-cache` | Safe, root |
| Flatpak | `flatpak` | no size available without changing anything | `flatpak-unused`, `size_known = false` | Safe, not root (flatpak asks for permission itself). Note: "Runtimes no installed app uses. The size is known after removal." |
| Snap | `snap` | `snap list --all`, rows whose Notes column contains `disabled`; size from `<snaps_dir>/<name>_<rev>.snap` | one item per row, id `<name>@<rev>` | Review, root. Note: "An older revision kept for rollback; you can no longer revert to it after removal." |
| Homebrew | `brew` | `brew cleanup --dry-run`, parse "This operation would free approximately <size> of disk space" | `homebrew-cleanup` | Safe, not root. Note: "Old versions and downloads Homebrew no longer needs." |
| Journal | `journalctl` | `journalctl --disk-usage`, parse "take up <size> in the file system" | `journal` | Review, root. Note: "System logs. Cleaning keeps the newest part, so recent problems can still be looked up." |

Rules:

- Run every program with `external::run(program, args, timeout)`; never through a shell.
- A failure (permission denied, timeout, unparsable output) yields one issue string per manager,
  such as "journalctl: <first line of stderr>", and no item for that manager.
- Skip items with `bytes == 0 && size_known`.
- Sort items by `bytes` descending, then manager, then id.
- Parse sizes with `ByteSize::from_str`; `journalctl` prints binary units like `1.2G`, so map a
  bare `K`, `M`, `G`, `T` suffix to `KiB`, `MiB`, `GiB`, `TiB` before parsing.

`pub fn for_user(config: &SystemConfig) -> Listing` builds the default `Tools` and calls `list`.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 4: Output and command

- `src/output/system.rs`: `print_system(entries, issues, quiet)` with columns SIZE (or `unknown`),
  SAFETY, ROOT (`yes`/`no`), NAME (`label`), and the note dimmed below like `print_caches`;
  `system_report(entries, issues) -> String` JSON with `schema_version`, `command: "system"`,
  `items: [{ manager, id, label, bytes, size_known, safety, needs_root, note }]`, `issues`, `summary`.
- `src/commands/system.rs`: replace the stub; print status "asking the package tools" like
  `src/commands/docker.rs`, call `system::for_user(&config.system)`, print text or JSON, return
  `Ok(())`. `AppError::NotBuilt` is no longer used here.

**Verify**: `cargo run -p kuizer -- system` exits 0 on this machine; paste its output in your report.

### Step 5: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need".

## Test plan

In `src/system/list.rs` tests, with fake programs written into a temp dir (copy the `script`
helper pattern from `src/docker.rs` tests) and `Tools { path: vec![tmp], .. }`:

1. `lists_pacman_cache_from_its_config`: a fake `pacman` (exits 0), a temp `pacman.conf` with
   `CacheDir = <tmp>/pkg/` and a 2 MB file there → one `pacman-cache` item of about 2 MB, `needs_root`.
2. `lists_disabled_snap_revisions`: a fake `snap` printing a `snap list --all` table with one
   active and one `disabled` row, a matching `.snap` file in a temp `snaps_dir` → one item
   `name@rev` with that file's size and `Review`.
3. `parses_journal_and_brew_sizes`: fake `journalctl` printing "Archived and active journals take
   up 1.5G in the file system." and fake `brew` printing "==> This operation would free
   approximately 300MB of disk space." → bytes `1.5 GiB` and `300 MB`.
4. `missing_tools_are_skipped_and_failures_are_issues`: an empty `path` → no items, no issues; a
   fake `journalctl` that exits 1 with "Permission denied" → one issue, no item.

Keep all `docker::` tests passing after step 1.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -rn "sudo\|pkexec\|--vacuum\|paccache\|apt-get clean\|uninstall" src/system` prints nothing
- [ ] `grep -n "NotBuilt" src/commands/system.rs` prints nothing
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- Moving the runner out of `src/docker.rs` changes any docker test outcome.
- A lister would need a command that changes state to learn a size (for example answering a
  prompt of `flatpak uninstall --unused`). Use `size_known = false` instead, or STOP if even that
  is impossible.
- You need a new CLI flag or a new dependency.

## Maintenance notes

- Plan 012 adds removal: it reuses `Item`, `needs_root` and the pseudo path, and adds the commands.
- Linuxbrew users also see `~/.cache/Homebrew` as an unknown cache folder in `kuizer cache`
  (plan 002), which overlaps with `homebrew-cleanup`. Adding a catalog entry that claims that
  folder with a note pointing to `kuizer system` would remove the overlap.
- Reviewers: check that nothing in `src/system` can change the machine, and that a missing tool
  never produces an error exit.
