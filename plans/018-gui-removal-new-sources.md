# Plan 018: Select and remove trash, system and large-file rows in the GUI

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- crates/kuizer-gui/src`
> Every commit listed must carry `Plan:` with 001, 002, 010, 014, 015 or 017. Anything else is
> drift: compare the excerpts below with the live code and STOP on a mismatch.

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: HIGH (deletion from the GUI)
- **Depends on**: plans/017-gui-new-sections.md, plans/006-trash-source.md; plans/012-system-removal.md and plans/013-big-files-removal.md only if they landed
- **Wave**: 3, after 017
- **Model**: Opus 5.5, high effort
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

Plan 017 shows the new sections read-only. This plan makes them act like the existing ones:
rows can be selected with the keyboard and mouse, and the inline confirmation removes them
through the same core functions the CLI uses. The trash is always enabled. System rows are
removable only if plan 012 landed, large files only if plan 013 landed; otherwise those sections
stay read-only and say how to clean them from the terminal.

## Current state

- `crates/kuizer-gui/src/model.rs`:
  - `pub enum Source { Artifact, Cache, Toolchain, Docker }` with `permanent()` (true for
    Toolchain and Docker).
  - `Confirmation::for_items` (plain yes for all-safe selections, typed count when any is review).
  - `pub struct Batch { source, roots, candidates }`; `cache_roots(candidates)` uses parent folders.
  - The clean function around line 690 dispatches per batch: `Source::Toolchain =>
    toolchains::remove(&guard, &candidate, mode)`, `Source::Docker => docker::remove(..)`,
    `Source::Artifact | Source::Cache => ` delete through the guard; records history.
- `crates/kuizer-gui/src/app.rs`: selection is a set of `(Source, id)`; per-source repetition
  around lines 620-640 (selected candidates), 785-810 (batches), 825-870 (select all/none),
  905-960 (candidate lookup and labels); `Target::Item(Source, id, path)` maps to a `Tab` around
  line 155. `app/map.rs` labels sources around line 469. `app/keys.rs` builds cursor targets per
  tab around lines 660-700 and has `SELECT_LISTS`.
- Plan 017 added `Tab::AppCaches`, `Tab::Trash`, `Tab::System`, `Tab::LargeFiles` with read-only
  cursor targets; `Tab::selects()` is false for them. App caches are `Source::Cache` rows already
  and become selectable by turning `selects()` on for `AppCaches`.
- Core removal functions:
  - `trashcan::remove(guard, candidate, mode)` with guard roots `trashcan::roots(&folder)`;
    always permanent.
  - `system::remove(guard, candidate, mode)` (plan 012), roots `&[]`; in the GUI, escalation must
    use `pkexec` (no terminal): pass a config with `system.escalate` forced to `pkexec` when it
    is `auto`, or STOP if plan 012's API does not allow that.
  - `big::remove(guard, candidate, mode)` (plan 013), roots = scan roots plus the downloads folder,
    mode from `delete.mode`.
- Project rules (`AGENTS.md`): confirmations are inline in the row or footer where the action
  started, not modals; motion for state changes only, under 150 ms, off with
  `appearance.motion = false`; each step idle, marked, confirming, deleting, gone animates from
  the previous one.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| GUI tests | `cargo test -p kuizer-gui -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |

Do not empty your real trash while testing the GUI by hand: run it with `HOME` and
`XDG_DATA_HOME` pointing at a temp folder that holds a fake trash.

## Scope

**In scope**: `crates/kuizer-gui/src/model.rs` (`Source`, clean dispatch), `app.rs`,
`app/keys.rs`, `app/map.rs`, `tab.rs` (`selects()`), `glossary.rs` (confirmation wording).

**Out of scope**: core removal functions (if one misbehaves, STOP), the TUI.

## Git workflow

- Branch `general/018-gui-removal-new-sources` from `feat/general-cleanup` (with 017 merged).
- One commit `feat(gui): remove trash, system and large-file rows`, body line
  `Plan: plans/018-gui-removal-new-sources.md`. No Claude references. Do not push.

## Steps

### Step 1: Sources

Add `Source::Trash`, `Source::System` (only if plan 012 landed) and `Source::Big` (only if plan
013 landed). `permanent()` is true for `Trash` and `System`. Fix every `match` on `Source` the
compiler reports, following the existing arm for `Docker` (permanent, external) or `Artifact`
(trash mode) as the closest model. Map `Target::Item(Source::Trash, ..)` to `Tab::Trash` and so on.

**Verify**: `cargo build -p kuizer-gui` exit 0.

### Step 2: Removal dispatch

In the clean function, add the arms listed in "Current state" with their guard roots. History is
recorded by the existing code path; check that a trash emptying appears in the History section
after a run.

**Verify**: `cargo build -p kuizer-gui` exit 0.

### Step 3: Selection and confirmation

Turn on `Tab::selects()` for `AppCaches`, `Trash` and, when removable, `System` and `LargeFiles`.
Replace the read-only cursor targets from plan 017 with `Target::Item(source, id, path)`. Update
`SELECT_LISTS`. The inline confirmation states the extra consequence per source:

- Trash: "Emptying the trash cannot be undone. Items kuizer moved there earlier can no longer be restored."
- System: "Runs <tool> and may ask for your password."
- Large files: the typed count is always required (they are all review).

Read-only sections (when 012 or 013 did not land) show a footer line with the CLI alternative,
for example "Clean these with `sudo paccache -rk2` in a terminal".

**Verify**: GUI tests pass; by hand with a fake trash in a temp `XDG_DATA_HOME`: select the
trash row, confirm, the row animates out and the fake trash is empty.

### Step 4: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need".

## Test plan

Following the existing GUI removal tests (search `crates/kuizer-gui` for a test that confirms a
cache removal and checks the row disappears):

1. `empties_a_fake_trash_from_the_gui`: report with one trash folder built from a temp trash; select,
   confirm; the temp `files/` is empty and the row is gone (assert through app state).
2. `large_files_need_the_typed_count` (only with plan 013): selecting one large file requires
   `Confirmation::TypeCount(1)`.
3. `read_only_sections_cannot_select`: when a source has no removal, `space` does not add it to
   the selection.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- A core removal function would need a change (for example escalation that cannot be forced to `pkexec`).
- The inline confirmation pattern cannot express a source's extra warning without a modal.

## Maintenance notes

- `app.rs` repeats per-source code in several places; adding these sources makes that heavier. A
  follow-up could fold sources into a table, but not in this plan.
- Reviewers: test keyboard-only, motion on and off, and check that no real trash or system tool is
  touched by the tests.
