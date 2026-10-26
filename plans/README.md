# Plans: kuizer as a general PC cleaner

Written on 2026-10-21 against `origin/master` at `5d04119`. Each plan is self-contained and
written for an executor with no other context. The plans extend kuizer from a developer cleaner
to a general cleaner: a data-driven location catalog for app, browser and system caches, the
trash, system package caches, large files and old downloads, and a shared keymap. Developer
sources stay as they are.

The orchestrator (you, or the session that dispatches executors) keeps this file. Executors
never edit it; they report back and the orchestrator updates the status column.

## Before you start

1. **Local `master` is two commits behind `origin/master`** (#34 scan activity, #35 docker calls)
   and carries uncommitted work in the GUI (`app.rs`, `app/keys.rs`, `commands.rs`, `theme.rs`,
   `tooltip.rs`, `ui.rs`) and the TUI (`src/tui/*`, shift-selection with a failing test). Plans
   001, 015, 016 and 017 touch those files. Land or stash that work first; otherwise it conflicts
   with every GUI and TUI plan.
2. **Branch `wip/gui-streaming-parts`** rewrites `model::load`. Plan 014 assumes it has not
   merged. Merge it before wave 2 and re-plan 014, or leave it until this set is done.
3. **Record the three decisions below.** Plans 012, 013, 017 and 019 read them.
4. Create the integration branch: `git fetch origin && git branch feat/general-cleanup origin/master`.

## Decisions

| Id | Question | Recommendation | Recorded answer |
|---|---|---|---|
| D1 | Full repositioning as a general cleaner, or a developer cleaner that also does general cleanup? Decides the sidebar order (017) and the README and crate descriptions (019). | Developer first for now; switch the order once the general sources have been used for a while. | OPEN |
| D2 | May kuizer delete personal files that cannot be regenerated (large files, old downloads)? Gates plan 013 and part of 018. | Yes, with the guards in 013: always `review`, never bulk-selected, typed confirmation, size and time recheck, trash by default. | OPEN |
| D3 | Is system cleanup with administrator rights (package caches, journal, snap revisions) in scope? Gates plan 012 and part of 018. | Not in the first release; listing (007) is enough to show the space, and the CLI prints the command to run. | OPEN |

A gated plan whose decision is "no" gets status REJECTED with the decision as the reason.

## Execution order and status

| Plan | Title | Wave | Model | Effort | Risk | Depends on | Status |
|---|---|---|---|---|---|---|---|
| [001](001-foundation-contracts.md) | Shared contracts: categories, catalog schema, stubs, config sections, CLI commands | 0 | Opus 5.5 high | L | MED | none | TODO |
| [002](002-catalog-engine.md) | Catalog engine; tool caches move into it | 1 | Opus 5.5 high | L | MED | 001 | TODO |
| [003](003-profile-cache-allowlist.md) | Cache folders inside browser and Electron profiles | 1 | Opus 5.5 high | M | HIGH | 001 | TODO |
| [004](004-catalog-data-linux.md) | Catalog data: Linux desktop, system and app caches | 1 | Sonnet 5.5 high | M | MED | 001 | TODO |
| [005](005-catalog-data-macos.md) | Catalog data: macOS caches, logs, Xcode | 1 | Sonnet 5.5 high | M | MED | 001 | TODO |
| [006](006-trash-source.md) | `kuizer trash`: list and empty the trash | 1 | Opus 5.5 high | M | MED | 001 | TODO |
| [007](007-system-listing.md) | `kuizer system`: list package caches, snaps, journal | 1 | Sonnet 5.5 high | M | LOW | 001 | TODO |
| [008](008-big-files-listing.md) | `kuizer big`: list large files and old downloads | 1 | Sonnet 5.5 high | M | LOW | 001 | TODO |
| [009](009-shared-keymap.md) | Shared keymap in the core with `[keys]` config | 1 | Opus 5.5 high | L | MED | 001 | TODO |
| [010](010-running-app-check.md) | Caches of running apps become `review` | 2 | Sonnet 5.5 high | S | LOW | 002, 007 | TODO |
| [011](011-catalog-data-browsers.md) | Catalog data: browsers and Electron apps | 2 | Sonnet 5.5 high | M | MED | 002, 003 | TODO |
| [012](012-system-removal.md) | System cleanup through each tool, with escalation (gated D3) | 2 | Opus 5.5 high | L | HIGH | 007 | TODO |
| [013](013-big-files-removal.md) | `kuizer big --clean` to the trash (gated D2) | 2 | Opus 5.5 high | M | HIGH | 008 | TODO |
| [014](014-gui-load-new-sources.md) | GUI: load trash, system, large files into the report | 2 | Opus 5.5 high | M | MED | 002, 006, 007, 008 | TODO |
| [015](015-gui-keymap.md) | GUI: every key from the shared keymap | 2 | Opus 5.5 high | L | MED | 009 | TODO |
| [016](016-tui-keymap.md) | TUI: every key from the shared keymap | 2 | Sonnet 5.5 high | M | MED | 009 | TODO |
| [017](017-gui-new-sections.md) | GUI: new sections, sidebar groups, start toggles | 3 | Opus 5.5 high | L | MED | 014, 015 | TODO |
| [018](018-gui-removal-new-sources.md) | GUI: select and remove the new rows | 3 | Opus 5.5 high | L | HIGH | 017, 006 (012, 013 if done) | TODO |
| [019](019-docs-and-positioning.md) | Spec, config example, README, crate metadata | 3 | Sonnet 5.5 high | M | LOW | all DONE plans | TODO |

Status values: TODO | IN PROGRESS | DONE | BLOCKED (one-line reason) | REJECTED (one-line reason).

## Waves

```text
wave 0   001
           │
wave 1   002  003  004  005  006  007  008  009        8 in parallel
           │    │              │    │    │    │
wave 2   010 (002,007)   011 (002,003)   012* (007)   013* (008)
         014 (002,006,007,008)   015 (009)   016 (009)  7 in parallel
           │
wave 3   017 (014,015) ──► 018 (017) ──► 019          serial

* gated by D3 (012) and D2 (013)
```

Model choice: Opus 5.5 at high effort for plans that change what can be deleted, touch the
delete guard, run tools as root, or reshape a frontend's core paths (001, 002, 003, 006, 009,
012, 013, 014, 015, 017, 018). Sonnet 5.5 at high effort for data research and well-bounded
work (004, 005, 007, 008, 010, 011, 016, 019).

## Running a wave in parallel

Every plan names the only files it may change, and plan 001 makes all shared edits up front, so
plans in one wave do not touch the same code.

- **One worktree per plan**, branched from `feat/general-cleanup` at the start of the wave:
  `general/NNN-<slug>`. With the Agent tool, use `isolation: "worktree"`. Executors read their
  plan by absolute path (`/home/remcostoeten/apps/kuizer/plans/NNN-*.md`), since `plans/` is not
  committed.
- **Builds are serialized, thinking is not.** The machine has 8 GB of RAM. Every executor sets
  `CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"` and `CARGO_BUILD_JOBS=2`; cargo's lock on
  the shared target dir makes concurrent builds wait for each other, and dependencies (GPUI) are
  compiled once. Run at most 3 or 4 executors at a time; more only add waiting.
- **Merge** the finished branches into `feat/general-cleanup` in plan order with
  `git cherry-pick` (or rebase and fast-forward), so the branch holds exactly one commit per plan
  and no merge commits; the drift checks read the `Plan:` line of each commit. Then run the full
  checks once for the wave:
  `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --all-targets -- --test-threads=2`.
- **Expected trivial conflicts**: `docs/config.example.toml` (each plan fills its own section;
  usually clean), `src/cli.rs` in wave 2 (012 edits `SystemArgs`, 013 edits `BigArgs`). Anything
  else conflicting means a plan left its scope: send it back.
- **Commits**: one per plan, conventional style, body line `Plan: plans/NNN-<slug>.md` (the drift
  checks rely on it). No Claude attribution anywhere. Squash `feat/general-cleanup` before it
  merges into `master`, and grep the outgoing messages for "claude" before pushing.
- **When to merge into `master`**: after any wave whose full checks pass, from wave 1 on. Until
  wave 1 is merged, `kuizer trash`, `system` and `big` answer "not built yet".

To dispatch from Claude Code, either ask for one plan at a time ("execute plans/006 in a
worktree and review the diff"), or ask explicitly for a workflow ("use a workflow to run wave 1
of plans/README.md, at most 4 agents at a time, Opus for 002, 003, 006, 009 and Sonnet for the
rest").

## Dependency notes

- Everything depends on 001: it defines the names the others write against
  (`catalog::Entry`, `Category::{Logs, Trash, Files}`, `Config.{catalog, system, big, keys}`,
  `ConfigError::Invalid`, `Protected::refuses`, `output::{trash, system, big}`, the CLI commands).
- 004, 005 and 011 only write TOML data; their files are validated by a test from 001 and resolved
  by 002. 011 waits for 003 because its entries point into protected profiles.
- 010 needs 002 (`caches::find`) and 007 (`src/external.rs`).
- 014 needs every core source (002, 006, 007, 008); 015 and 016 need 009. 014 and 015 both edit
  the GUI crate, in disjoint files.
- 017 needs 014 (data) and 015 (keymap-driven sections); 018 needs 017; 019 describes whatever is DONE.

## Considered and not planned

- **Trash folders on other mounts** (`/run/media/*/.Trash-1000`, `/media/...`): the delete guard
  refuses `/run`, and widening it is a separate safety decision.
- **Coredumps**: no tool removes them selectively without root and `systemd-tmpfiles`; listing
  what cannot be cleaned is noise.
- **A separate `logs` source**: user logs are catalog entries with `category = "logs"`; the
  journal is a `system` item.
- **Reusing BleachBit's CleanerML files**: GPL-3, kuizer is MIT. Used only as a reference for
  which paths exist, never copied.
- **TUI views for the new sources, a GUI settings view, dashboard tabs**: worth doing, but each is
  its own plan after this set; `AGENTS.md` requires the settings view eventually.
- **Windows**: the project rules exclude it.
