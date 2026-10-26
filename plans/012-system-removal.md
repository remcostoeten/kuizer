# Plan 012: Clean system package caches and the journal through each tool, with explicit escalation

> **GATED**: execute only if the orchestrator recorded decision D3 = "system cleanup is in scope"
> in `plans/README.md`. Otherwise this plan is REJECTED and nothing here is built.
>
> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/system src/commands/system.rs src/external.rs src/cli.rs`
> Every commit listed must carry `Plan:` with 001 or 007. Anything else is drift: compare the
> excerpts below with the live code and STOP on a mismatch.

## Status

- **Priority**: P3
- **Effort**: L
- **Risk**: HIGH (runs package tools as root)
- **Depends on**: plans/007-system-listing.md
- **Wave**: 2, parallel with 010, 011 and 013 to 016
- **Model**: Opus 5.5, high effort
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

Plan 007 lists what system package tools could free. This plan lets `kuizer system --clean`
free it, by running each tool's own cleanup command, never by deleting files under `/var`
itself. Most of these commands need root. kuizer itself never runs as root: it asks for
administrator rights per command, with `sudo` in a terminal or `pkexec` in a graphical session,
and only for a fixed argument list resolved from system folders. When neither is possible, it
prints the exact command for the user to run.

## Current state

- `src/system/mod.rs` and `src/system/list.rs` (plan 007): `Manager { Pacman, Apt, Dnf, Zypper,
  Flatpak, Snap, Homebrew, Journal }`, `Item { manager, id, label, bytes, size_known, safety,
  needs_root, note }`, pseudo path `system:<manager>/<id>` (`Item::path`), `entries(items)`,
  `SystemConfig { managers, timeout_secs }`, `for_user(&SystemConfig) -> Listing`. Snap items have
  id `<name>@<rev>`.
- `src/external.rs` (plan 007): `run(program, args, timeout) -> Result<String, Failure>` with
  stdin set to null and stdout/stderr captured.
- `src/commands/system.rs` (plan 007): lists and prints; no removal.
- `src/cli.rs`: `SystemArgs { command: Option<NestedCommand> }`, `system_help()` examples.
  Exemplar for a removable source: `DockerArgs { clean: bool, removal: RemovalArgs, command }`
  and `src/commands/docker.rs` (forces `Mode::Permanent` unless `--dry-run`, roots `&[]`,
  `remove: docker::remove`).
- `src/docker.rs` `remove(_guard, candidate, mode)`: parses the pseudo path, refuses ids starting
  with `-`, returns `Ok(())` for `Mode::DryRun`, runs the engine with a 300 s timeout and maps
  failures to `DeleteError::Tool`. Copy this shape.
- `src/commands/removal.rs`: `removal::run` previews, asks for confirmation (typing the count when
  any item is `review`), calls `remove` per item and records the history.
- Cleanup commands (verify each against its man page before coding):
  - Pacman: `paccache -rk<N>` (pacman-contrib) when available, else `pacman -Sc --noconfirm`.
  - Apt: `apt-get clean`. Dnf: `dnf clean packages`. Zypper: `zypper --non-interactive clean --all`.
  - Flatpak: `flatpak uninstall --unused --noninteractive` (flatpak asks polkit itself; never escalate).
  - Snap: `snap remove <name> --revision=<rev>`.
  - Homebrew: `brew cleanup` (must never run as root).
  - Journal: `journalctl --vacuum-size=<size>`.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Unit tests | `cargo test -p kuizer system:: -- --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Dry run | `cargo run -p kuizer -- system --clean --dry-run` | preview, nothing runs |

Never run `kuizer system --clean` without `--dry-run` during this plan. Tests use fake programs only.

## Scope

**In scope**: `src/system/` (new `remove.rs`, edits to `mod.rs`), `src/commands/system.rs`,
`src/external.rs` (add an interactive runner), `src/cli.rs` (only `SystemArgs` and
`system_help`), `docs/config.example.toml` (only inside `[system]`).

**Out of scope**: any deletion of files under `/var`, `/usr` or `/etc` by kuizer itself; the delete
guard; GUI (plan 018 wires it if this plan lands).

## Git workflow

- Branch `general/012-system-removal` from `feat/general-cleanup` (with wave 1 merged).
- One commit `feat: clean system package caches through their own tools`, body line
  `Plan: plans/012-system-removal.md`. No Claude references. Do not push.

## Steps

### Step 1: CLI and config

- `SystemArgs` gains `clean: bool` (help: "Run the selected tools' cleanup after a preview,
  always for good") and `#[command(flatten)] removal: RemovalArgs`, like `DockerArgs`. Add the
  examples `("kuizer system", "--clean --dry-run", "Show which cleanups would run")` and
  `("kuizer system", "--clean", "Run them, asking for administrator rights where needed")`.
- `SystemConfig` gains `escalate: Escalate` (`auto` default, `sudo`, `pkexec`, `none`),
  `journal_keep: ByteSize` (default `500MB`) and `pacman_keep: u32` (default `2`, must be at least
  1). Document under `[system]`.

**Verify**: `cargo test -p kuizer -- cli:: config:: --test-threads=2` passes.

### Step 2: Commands as data

In `src/system/remove.rs`:

```rust
pub struct Plan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub escalate: bool,
}

pub fn plan(item: &Item, config: &SystemConfig, tools: &Tools) -> Result<Plan, DeleteError>
```

- Builds the argument list from the table in "Current state". Snap ids are split on the last
  `@`; the name must match `[a-z0-9-]+` and the revision `[0-9]+`, otherwise refuse.
- `program` is an absolute path. For commands that are escalated, resolve the program only from
  the fixed list `/usr/bin`, `/usr/sbin`, `/bin`, `/sbin`, never from `$PATH`. Non-escalated
  programs (`brew`, `flatpak`) resolve from `tools.path`.
- `escalate` is `item.needs_root` and the current effective user id is not 0
  (`rustix::process::geteuid()`, already a dependency on unix).

**Verify**: `cargo build -p kuizer` exit 0.

### Step 3: Escalation

- `pub fn command_line(plan: &Plan, escalate: Escalate, env: &Session) -> Result<Vec<String>, DeleteError>`
  where `Session { terminal: bool, graphical: bool, sudo: Option<PathBuf>, pkexec: Option<PathBuf> }`
  (`graphical` when `WAYLAND_DISPLAY` or `DISPLAY` is set; `sudo` and `pkexec` resolved from the
  fixed system folders).
  - Not escalated: `[program, args...]`.
  - `auto`: `sudo` when `terminal` and `sudo` exists, else `pkexec` when `graphical` and it
    exists, else refuse.
  - `sudo` / `pkexec`: that one, or refuse when missing.
  - `none`: refuse.
  - A refusal is `DeleteError::Tool(format!("needs administrator rights; run: sudo {}", plain))`
    with the plain command line, so the user can copy it.
  - sudo form: `[sudo, "--", program, args...]`. pkexec form: `[pkexec, program, args...]`.
- In `src/external.rs` add `pub(crate) fn run_attached(argv: &[String], timeout: Duration) ->
  Result<(), Failure>` that inherits stdin, stdout and stderr (so `sudo` can ask for a password
  in the terminal) and waits up to `timeout`.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 4: Remove

`pub fn remove(_guard: &Guard, candidate: &Candidate, mode: Mode) -> Result<(), DeleteError>` in
`src/system/remove.rs`: parse the pseudo path back into manager and id, rebuild the `Item` by
listing again (`list::for_user`) and refuse when the item is gone; `Mode::DryRun` returns `Ok`;
otherwise build the plan and command line and run it: `run_attached` when it uses `sudo`, else
`external::run` with a 600 s timeout. Map failures to `DeleteError::Tool` with the tool's last
stderr line.

`src/commands/system.rs`: with `--clean` or `--dry-run`, check the selection against the item
names (`manager.name()` and ids), select (default `safe`), and call `removal::run` with
`command: "system"`, `roots: &[]`, mode `DryRun` or `Permanent`, `remove: system::remove`. Before
running, print one line per selected item with the command line it will run (also in dry run).

**Verify**: `cargo run -p kuizer -- system --clean --dry-run` prints the command lines and
"Dry run, nothing was removed".

### Step 5: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need".

## Test plan

In `src/system/remove.rs` tests:

1. `builds_fixed_argument_lists`: one item per manager → the expected argv; a snap id
   `bad;rm@1` and `name@x` are refused.
2. `escalation_table`: `command_line` for every combination of `Escalate` × {terminal, graphical,
   neither} × {sudo present, missing} on a root-needing plan, asserting the chosen wrapper or the
   refusal text containing `sudo <command>`.
3. `escalated_programs_never_come_from_path`: a fake `pacman` in a temp `tools.path` is ignored for
   an escalated plan (the program is `/usr/bin/pacman` or the plan is refused when it does not exist).
4. `runs_a_non_root_tool`: a fake `brew` script in a temp dir that writes its arguments to a file;
   `remove` on a `homebrew-cleanup` candidate (with `Tools` pointing at the temp dir) writes
   `cleanup`.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -rn "sh -c\|bash -c\|Command::new(\"sh\")" src/system src/external.rs` prints nothing
- [ ] `grep -rn "remove_dir_all\|remove_file" src/system` prints nothing
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- A tool's cleanup command needs interactive answers that cannot be given with a flag.
- Escalation would need to run kuizer itself as root, or to pass anything not built from the
  fixed table.
- `rustix` lacks `geteuid` with the enabled features (`fs` only): report; do not add features
  without saying so.

## Maintenance notes

- The listed size is an estimate (pacman keeps the newest versions; flatpak's size is unknown);
  the history records the estimate.
- Plan 018 adds a GUI action for these items: in the GUI, `pkexec` is the only escalation path.
- Reviewers: read every argv in step 2 against the tools' man pages; check that nothing reaches a
  shell and that escalated programs are absolute system paths.
