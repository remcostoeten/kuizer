# Plan 017: Show app caches, trash, system and large files as GUI sections

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- crates/kuizer-gui/src src/keymap`
> Every commit listed must carry `Plan:` with 001, 002, 009, 010, 014 or 015. Anything else is
> drift: compare the excerpts below with the live code and STOP on a mismatch.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED (largest GUI change of the set; the GUI is the main product)
- **Depends on**: plans/014-gui-load-new-sources.md, plans/015-gui-keymap.md
- **Wave**: 3, before 018
- **Model**: Opus 5.5, high effort
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

After plans 014 and 015, the GUI's scan report holds the new sources and every key comes from
the shared keymap, but nothing new is visible. This plan adds the sections a general PC cleaner
needs: app and browser caches (separate from developer caches), the trash, system package caches
and large files, regroups the sidebar, adds start-screen toggles, and gives each new section a
keymap action. Rows are read-only here; plan 018 makes them selectable and removable. This keeps
the change reviewable and lets the user see the result before deletion is wired.

## Decision input

Read decision D1 in `plans/README.md` before step 2:

- **D1 = general first**: sidebar order Overview, App caches, Trash, System, Large files | Project
  artifacts, Developer caches, Toolchains, Docker, Git repositories | Duplicates | History, Issues.
- **D1 = developer first** (or not recorded): Overview, Project artifacts, Developer caches,
  Toolchains, Docker, Git repositories | App caches, Trash, System, Large files | Duplicates |
  History, Issues.

## Current state

- `crates/kuizer-gui/src/tab.rs`: `pub enum Tab { Overview, Artifacts, Caches, Toolchains, Docker,
  Repos, Duplicates, History, Issues }`, `Tab::ALL` (sidebar order), `next`/`previous`, `selects()`
  (sections whose rows can be selected), `term()`, `icon()`, `heading()` (group headings
  "Reclaim", "Tidy up", "Activity" on the first tab of a group), `label()`, `status(stage, options)`
  (placeholder text while a scan runs), and `action()` (plan 015, `section.<name>`).
- `crates/kuizer-gui/src/app.rs`:
  - `render_sidebar` (around line 1861) iterates `Tab::ALL` and renders `tab.heading()`.
  - The section switch around line 2145 (`Tab::Caches => self.render_caches(cx)` and so on), row
    counts around line 1812 (`Tab::Caches => Some(report.caches.len())`) and byte totals around
    line 1832 (`Tab::Caches => report.cache_total(None)`).
  - `render_caches` (around line 3035) is the exemplar list: `table_header()` with `head(col::M,
    "Size", Term::Size)`, `table_row((key, id), checked)`, `menu_row`, a `debug_selector` per row,
    filtered with `shows_cache`. `cache_label` (around line 4960).
  - `render_start` (around line 2164) with `toggle("opt-caches", options.caches, |options| ..)`
    around line 2450.
- `crates/kuizer-gui/src/app/keys.rs`: `shows_cache` (filter match), cursor targets per tab
  (around lines 660-700: `Target::Item(Source::Cache, ..)`), and the `SELECT_LISTS` hint text.
- `crates/kuizer-gui/src/model.rs` (after 014): `Report.caches: Vec<(KnownCache, Candidate)>`
  where `KnownCache.group` is `Developer`, `Apps`, `Browsers` or `System`; `Report.trash`,
  `trash_issues`, `system`, `system_issues`, `big`; totals `cache_total`, `trash_total`,
  `system_total`, `big_total`. `ScanOptions.trash`, `.system`, `.big`.
- `crates/kuizer-gui/src/icon.rs`: `enum Icon` of layered 24-unit line icons with hover motions;
  `crates/kuizer-gui/src/glossary.rs`: `Term` with titles and texts (plan 001 added `Logs`,
  `TrashContents`, `PersonalFiles`).
- `src/keymap/actions.rs` `ACTIONS` has `section.*` actions for the nine current tabs with keys
  `1`..`9`.
- Project rules that apply (`AGENTS.md`): colors, spacing and radii only from
  `crates/kuizer-gui/src/theme.rs`; reuse existing components before adding new ones; motion only
  for state changes, under 150 ms, and off when `appearance.motion` is false; every action has a
  keybinding; focus always visible.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| GUI tests | `cargo test -p kuizer-gui -- --test-threads=2` | all pass |
| Keymap tests | `cargo test -p kuizer keymap:: -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Run | `cargo run -p kuizer-gui -- ~/Downloads` | the new sections show data |

## Suggested executor toolkit

- Skills `gpui`, `gpui-kit` and `gpui-kit-design-guides` if available; `animated-icons` for the
  icon hover motion style.

## Scope

**In scope**: `crates/kuizer-gui/src/tab.rs`, `app.rs`, `app/keys.rs`, `app/map.rs` (only if the
space map needs the new totals), `icon.rs`, `glossary.rs`, `src/keymap/actions.rs` (new
`section.*` rows only).

**Out of scope**: selection and removal of the new rows (plan 018), `model.rs` (plan 014 is
done; if a field is missing, STOP), the TUI, `site/`.

## Git workflow

- Branch `general/017-gui-new-sections` from `feat/general-cleanup` (with wave 2 merged).
- One commit `feat(gui): sections for app caches, trash, system and large files`, body line
  `Plan: plans/017-gui-new-sections.md`. No Claude references. Do not push.

## Steps

### Step 1: Keymap actions for the new sections

In `src/keymap/actions.rs` add, all `Global`, `gui: true`, `tui: false`, with no default keys
(`keys: &[]`): `section.app_caches`, `section.trash`, `section.system`, `section.large_files`.
Rename nothing. Digit keys stay on the first nine sidebar entries in step 2.

**Verify**: `cargo test -p kuizer keymap:: -- --test-threads=2` passes.

### Step 2: Tabs and sidebar

- Add `Tab::AppCaches`, `Tab::Trash`, `Tab::System`, `Tab::LargeFiles`. Keep `Tab::Caches` and
  relabel it "Developer caches".
- Order `Tab::ALL` per decision D1. Headings: "Clean up" (first general tab), "Developer"
  (first developer tab), "Files" (`Duplicates`), "Activity" (`History`).
- Move the digit keys so `1`..`9` follow the new sidebar order: change the default keys of the
  `section.*` actions in `src/keymap/actions.rs` accordingly (the first nine tabs get `1`..`9`,
  the rest none). Run the keymap tests again.
- Labels: "App caches", "Trash", "System", "Large files". Terms: `AppCaches` ("Caches that apps,
  browsers and the desktop keep in your home folder. Safe caches fill up again on their own."),
  `TrashContents` (exists), `SystemCaches` ("Downloads and logs your package tools keep. Cleaning
  them runs the tool itself and may ask for your password."), `PersonalFiles` (exists).
- Icons: add `Icon::AppCaches`, `Icon::Trash`, `Icon::System`, `Icon::LargeFiles`, drawn with the
  same layer and motion vocabulary as the existing icons (look at `Icon::Caches` and
  `Icon::Duplicates` first). Hover motion ends where it started, like the others.
- `status(stage, options)`: one line each, in the style of the existing ones ("Trash is off for
  this scan", "Measuring the trash", and so on), using the `Stage` variants plan 014 added.

**Verify**: `cargo build -p kuizer-gui` exit 0; the sidebar shows the new order and headings.

### Step 3: Lists

- `Tab::Caches` (Developer caches) shows only `group == Developer`; `Tab::AppCaches` shows
  `Apps`, `Browsers` and `System` groups, with a column "Group" after "Tool". Split
  `render_caches` into one function with a group filter rather than copying it.
- `Tab::Trash`: one row per folder: Size, Items, Path. Empty state: "The trash is empty". Issues
  (for example missing Full Disk Access on macOS) appear in the Issues section like other issues.
- `Tab::System`: Size (or "Unknown"), Safety, Needs password (yes/no), Name, Note. System issues
  listed under the table like `docker_issues`.
- `Tab::LargeFiles`: Size, Modified, Kind (Large or Download), Path. Empty state explains that
  the GUI looks in the scanned folders and Downloads, and `kuizer big` searches the whole home folder.
- Filtering (`/`) matches path, name and tool for every new list, like `shows_cache`.
- Row counts and byte totals for the sidebar and overview, like `Tab::Caches`.
- Keyboard: the cursor moves through the new lists with the same motions (`Target::Item` needs a
  `Source`; until plan 018 adds new `Source` variants, use a read-only cursor target such as
  `Target::Row(Tab, index)` if one exists, or add the minimal variant needed and say so in your
  report). `Tab::selects()` stays false for the new tabs in this plan.

**Verify**: `cargo run -p kuizer-gui -- ~/Downloads` shows rows in App caches and Trash on this
machine; `j`/`k` move in each new list.

### Step 4: Start-screen toggles

In `render_start`, add toggles for `options.trash`, `options.system` and `options.big` next to
the existing caches toggle, with labels "Trash", "System caches", "Large files".

**Verify**: toggling one off and scanning shows the section's "off for this scan" status.

### Step 5: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need".

## Test plan

Follow the existing GPUI tests in `crates/kuizer-gui` (they render the app with a fixed
`Report` and assert through `debug_bounds` on `debug_selector` ids; assert presence through
`debug_bounds`, absence through app state, because `debug_bounds` entries are never cleared):

1. `app_caches_and_developer_caches_split_by_group`: a report with one developer and one apps
   cache; each section shows its own row.
2. `new_sections_list_their_rows`: one trash folder, one system item, one big file; each
   section renders one row (`trash-row-0`, `system-row-0`, `large-row-0`).
3. `digits_follow_the_sidebar_order`: pressing `2` opens the second tab of `Tab::ALL`.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -rn "rgb(\|px(" crates/kuizer-gui/src/app.rs | grep -v theme::` shows no new hardcoded values (compare with `git diff`)
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- Decision D1 is not recorded and the orchestrator is not reachable: use "developer first" and say so.
- A report field from plan 014 is missing.
- A new list needs a new visual component that has no equivalent in the existing sections.

## Maintenance notes

- Plan 018 adds `Source::Trash`, `Source::System`, `Source::Big`, selection and removal, and turns
  `Tab::selects()` on for the sections whose removal exists.
- Reviewers: run the GUI with motion on and off (`appearance.motion = false`), and keyboard only.
