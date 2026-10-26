# Plan 015: Drive the GUI's key bindings from the shared keymap

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- crates/kuizer-gui/src/commands.rs crates/kuizer-gui/src/tab.rs crates/kuizer-gui/src/app/keys.rs crates/kuizer-gui/src/app/overlay.rs crates/kuizer-gui/src/palette.rs crates/kuizer-gui/src/main.rs src/keymap`
> Every commit listed must carry `Plan:` with 001, 002 or 009. Anything else is drift: compare
> the excerpts below with the live code and STOP on a mismatch.

## Status

- **Priority**: P1 (plan 017 adds sections through the keymap)
- **Effort**: L
- **Risk**: MED (every GUI key goes through this)
- **Depends on**: plans/009-shared-keymap.md
- **Wave**: 2, parallel with 010 to 014 and 016
- **Model**: Opus 5.5, high effort
- **Category**: tech-debt
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

`AGENTS.md` requires one keymap in the core that both frontends read, so a rebind in
`~/.config/kuizer/config.toml` (`[keys]`) applies to the GUI and the TUI. Plan 009 built that
keymap (`src/keymap`). This plan makes the GUI take every key from it: the command table, the
palette and shortcut overlay labels, the GPUI global bindings, and the section keys. After this
plan, a GUI command without a keymap action does not compile into the table, so new GUI features
cannot skip the keymap.

## Current state

- Core (`src/keymap`, plan 009): `ACTIONS: &[Action { name, keys, scope, gui, tui }]`,
  `Scope { Global, List, Map, Git, Confirm }`, `Frontend { Gui, Tui }`, `Chord { text, strokes }`,
  `Keymap::resolve(&KeysConfig) -> Result<Keymap, KeymapError>`, `Keymap::defaults()`,
  `Keymap::keys(&self, action) -> &[Chord]`, `Keymap::actions(frontend)`, `Keymap::lookup(frontend,
  scopes, chord)`. `Config.keys: KeysConfig` is validated at config load.
  Action names for the GUI: `palette.open`, `help.open`, `scan.rescan`, `scan.new`,
  `history.undo`, `section.next`, `section.previous`, `section.overview`, `section.artifacts`,
  `section.caches`, `section.toolchains`, `section.docker`, `section.repos`, `section.duplicates`,
  `section.history`, `section.issues`, `view.space_map`, `view.zoom_in`, `view.zoom_out`,
  `view.zoom_reset`, `view.sidebar`, `nav.down`, `nav.up`, `nav.top`, `nav.bottom`,
  `nav.page_down`, `nav.page_up`, `select.toggle`, `select.all`, `select.none`, `delete.selected`,
  `item.reveal`, `item.copy_path`, `item.menu`, `filter.focus`, `map.open`, `map.out`, `map.top`,
  `map.toggle`, `map.left`, `map.right`, `map.up`, `map.down`, `repo.gc`, `repo.gc_all`,
  `repo.branches`, `repo.branches_all`.
- `crates/kuizer-gui/src/commands.rs`:
  - `pub enum Command { Palette, Shortcuts, GoTo(Tab), NextTab, PreviousTab, Rescan, NewScan,
    Undo, Down, Up, First, Last, PageDown, PageUp, Toggle, SelectAll, SelectNone, Trash, Reveal,
    CopyPath, OpenMenu, FocusFilter, ZoomIn, ZoomOut, ResetZoom, Sidebar, SpaceMap, MapIn, MapOut,
    MapTop, MapMove(Direction), MapToggle, Scan(PathBuf), Gc, GcAll, Branches, BranchesAll }`.
  - `pub struct Spec { command, label, keys: &'static [&'static str], extra: &'static [&'static str],
    group, palette, global }`, built in `pub fn table() -> Vec<Spec>` (around line 149) with
    `.keys(&[...])`, `.extra(&[...])`, `.hidden()`, `.global()`. `keys` are shown and matched,
    `extra` only matched.
  - `pub fn shortcut(command) -> Option<String>` (first key's label), `pub fn bind_keys(cx)` (binds
    the `global` specs as GPUI actions `ZoomIn`, `ZoomOut`, `ResetZoom`, `Undo`, `TogglePalette`),
    `pub fn matches(pattern, keystroke) -> bool` (GPUI `Keystroke::parse` of the pattern compared
    with the typed keystroke, symbols by character), `pub fn label(pattern) -> String`.
  - A test around line 489 asserts every key in the table parses with GPUI.
- `crates/kuizer-gui/src/tab.rs` `Tab::keys()` returns `&["1"]`..`&["9"]` per tab; `table()`
  uses it for `GoTo(tab)`.
- `commands::table()` is called from `app/keys.rs` (key routing, around lines 281 and 1346),
  `app/overlay.rs` (shortcuts overlay, around line 168) and `palette.rs` (ranking and tests).
- `crates/kuizer-gui/src/main.rs` loads the config and calls `commands::bind_keys`.
- Plan 017 runs later and adds new tabs and their `section.*` actions; plan 014 edits
  `model.rs`, `snapshot.rs`, `activity.rs`, `start.rs` in parallel with you.
- Conventions: function items, no comments except allowed one-liners, GPUI 0.2.2 (read
  `~/.cargo/registry/src/*/gpui-0.2.2` rather than guessing), every action keeps working without
  a mouse.

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
| Run | `cargo run -p kuizer-gui -- .` | every key in the shortcuts overlay (`?`) still works |

## Scope

**In scope**: `crates/kuizer-gui/src/commands.rs`, `tab.rs` (only `Tab::keys` and a new
`Tab::action`), `app/keys.rs`, `app/overlay.rs`, `palette.rs`, `main.rs`.

**Out of scope**: `src/keymap` (if an action is missing, STOP), `model.rs`, `snapshot.rs`,
`activity.rs`, `start.rs` (plan 014), sidebar layout and new sections (plan 017), the TUI (plan 016).

## Git workflow

- Branch `general/015-gui-keymap` from `feat/general-cleanup` (with wave 1 merged).
- One commit `feat(gui): take every key binding from the shared keymap`, body line
  `Plan: plans/015-gui-keymap.md`. No Claude references. Do not push.

## Steps

### Step 1: Install the keymap once

In `commands.rs` add a process-wide keymap:

```rust
static KEYMAP: OnceLock<Keymap> = OnceLock::new();

pub fn install_keymap(keymap: Keymap) { let _ = KEYMAP.set(keymap); }

fn keymap() -> &'static Keymap { KEYMAP.get_or_init(Keymap::defaults) }
```

`main.rs` calls `commands::install_keymap(Keymap::resolve(&config.keys)...)` before
`bind_keys`. Config validation already rejected a bad `[keys]` table at load, so on the
impossible error fall back to `Keymap::defaults()`.

**Verify**: `cargo build -p kuizer-gui` exit 0.

### Step 2: Specs name actions instead of keys

- Add `pub fn action(&self) -> Option<&'static str>` on `Command` mapping every variant to its
  action name from "Current state" (`GoTo(tab)` uses `tab.action()`, `MapMove(direction)` maps to
  `map.left`/`map.right`/`map.up`/`map.down`, `Scan(_)` has none).
- Add `Tab::action(self) -> &'static str` (`section.overview` .. `section.issues`) and delete
  `Tab::keys`.
- Change `Spec` to drop `keys` and `extra` and expose `pub fn keys(&self) -> Vec<&'static str>`:
  the `text` of every chord the keymap holds for `self.command.action()` (empty when none).
  The first chord is the shown one; all are matched. Remove the `.keys()` and `.extra()` builder
  calls from `table()`; keep labels, groups, `hidden` and `global` as they are.
- `shortcut`, `bind_keys`, the overlay, the palette and the key router use `spec.keys()`.
- `bind_keys` binds every chord of a `global` spec; a chord with several strokes is passed to
  `KeyBinding::new` as its text (GPUI supports multi-stroke bindings).

**Verify**: `cargo build -p kuizer-gui` exit 0, `cargo test -p kuizer-gui -- --test-threads=2`
passes after updating the table test (step 3).

### Step 3: Multi-stroke chords in the key router

The router in `app/keys.rs` matches one keystroke at a time with `commands::matches`. Add a
pending-prefix buffer to the app state: when a typed keystroke matches the first stroke of a
multi-stroke chord, keep it and wait for the next keystroke; any keystroke that completes no
chord clears the buffer and is then handled normally. Single-stroke behavior must not change.
Defaults have no multi-stroke chords, so this only matters for user config.

**Verify**: GUI tests pass; add the test in "Test plan".

### Step 4: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need", then run the
GUI and press every key listed in the shortcuts overlay once.

## Test plan

In `commands.rs` tests:

1. Replace the "every key parses" test: for every action of `Keymap::defaults().actions(Frontend::Gui)`,
   every chord's every stroke parses with GPUI `Keystroke::parse`.
2. `every_command_has_an_action`: every `Spec` in `table()` returns `Some` from `command.action()`.
3. `rebinding_moves_the_key`: build a keymap with `KeysConfig` `{"nav.down": ["n"]}` and check the
   resolved spec for `Command::Down` lists `n` and not `j`. Do this without the global
   `OnceLock` (add a `fn table_for(keymap: &Keymap)` that `table()` calls, and test that).
4. In `app/keys.rs` tests (follow the existing key tests there): with `nav.bottom` bound to `g g`
   through `table_for`, typing `g` then `g` moves to the last row, typing `g` then `j` moves down
   one row.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -n "\.keys(&\[\|\.extra(&\[" crates/kuizer-gui/src/commands.rs` prints nothing
- [ ] `grep -rn "fn keys" crates/kuizer-gui/src/tab.rs` prints nothing
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- A GUI command has no matching action in `src/keymap` `ACTIONS`.
- A default chord from the keymap does not parse with GPUI.
- Making a test pass needs a change in `model.rs`, `start.rs` or the sidebar rendering.

## Maintenance notes

- The new `u` (select none) and `ctrl-d`/`ctrl-u` (page motions) from the shared defaults now
  work in the GUI; the shortcuts overlay lists them automatically.
- Plan 017 adds `section.*` actions to `src/keymap` for the new sections and `Tab::action` arms.
- Reviewers: run the GUI and try a rebind in a temp config (`kuizer-gui` honors `--config`? if not,
  use `XDG_CONFIG_HOME`), for example `"nav.down" = ["n"]`.
