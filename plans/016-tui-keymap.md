# Plan 016: Drive the TUI's key bindings from the shared keymap

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/tui src/keymap`
> Every commit listed must carry `Plan:` with 001 or 009. Anything else is drift (for example the
> shift-selection work that sat uncommitted on `master` in October 2026): compare the excerpts
> below with the live code and STOP on a mismatch.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED (every TUI key goes through this)
- **Depends on**: plans/009-shared-keymap.md
- **Wave**: 2, parallel with 010 to 015
- **Model**: Sonnet 5.5, high effort
- **Category**: tech-debt
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

`AGENTS.md` requires that both frontends read one keymap from the core, so a rebind in
`[keys]` applies to the GUI and the TUI. Plan 009 built it (`src/keymap`); plan 015 moves the
GUI. This plan moves the TUI: `handle_key` looks keys up in the keymap instead of matching
hardcoded characters, and the help overlay shows the configured keys.

## Current state

- Core (`src/keymap`, plan 009): `Keymap::resolve(&KeysConfig)`, `Keymap::defaults()`,
  `Keymap::lookup(frontend, scopes, chord) -> Option<&'static str>`, `Keymap::keys(action)`,
  `Chord::parse`, `Keystroke { ctrl, alt, shift, cmd, function, key }` (uppercase letters are
  stored as lowercase plus `shift`; named keys lowercase: `space`, `enter`, `escape`, `tab`,
  `backspace`, `delete`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`).
  TUI actions: `help.open` (`?`), `app.quit` (`q`, `escape`), `nav.down`, `nav.up`, `nav.top`,
  `nav.bottom`, `nav.page_down` (`pagedown`, `ctrl-d`), `nav.page_up` (`pageup`, `ctrl-u`),
  `select.toggle`, `select.safe` (`a`), `select.none` (`secondary-shift-a`, `u`), `select.project`
  (`P`), `delete.selected` (`enter`, `delete`), `delete.cursor` (`d`), `filter.focus` (`/`),
  `sort.cycle` (`s`), `group.toggle` (`p`), `filter.category` (`c`), `filter.safe_only` (`f`),
  `filter.reset` (`x`) in scope `List` (`help.open`, `app.quit` in `Global`), and
  `confirm.accept` (`y`, `enter`), `confirm.toggle_mode` (`t`) in scope `Confirm`.
- `src/tui/mod.rs` `handle_key(app, key: KeyEvent, jobs)` (around lines 93-145): `ctrl-c` quits
  first; `app.filtering` routes to `handle_filter`; then a `match (&app.overlay, key.code)` with
  the hardcoded keys listed in plan 009; in `Overlay::Confirm` any other key cancels; in
  `Overlay::Help` any key closes it. `PAGE` is the page step.
- `src/tui/view.rs` around line 579 lists key hints for the help overlay as literals such as
  `(&["c"], "cycle category: dependencies, build, cache, temp")`.
- `tui::run(config, roots, confirm)` has the `Config`.
- Two deliberate behavior changes come with the shared defaults: `ctrl-d`/`ctrl-u` page, and the
  `delete` key deletes the selection (`delete.selected`) instead of the selection-or-cursor
  (`d` keeps that).
- Conventions: function items, no comments except allowed one-liners, tests in `src/tui/app.rs`
  style (`App::new`, pushing candidates, asserting state).

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| TUI tests | `cargo test -p kuizer tui:: -- --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Run | `cargo run -p kuizer -- .` | every key in the help overlay works |

## Scope

**In scope**: `src/tui/mod.rs`, `src/tui/view.rs`, `src/tui/keys.rs` (create), `src/tui/app.rs`
(only if `App` needs to hold the keymap).

**Out of scope**: `src/keymap` (if an action is missing, STOP), the GUI, `ctrl-c` (stays a
hardcoded quit), text input in filter mode (`handle_filter`: keys type text there).

## Git workflow

- Branch `general/016-tui-keymap` from `feat/general-cleanup` (with wave 1 merged).
- One commit `feat(tui): take key bindings from the shared keymap`, body line
  `Plan: plans/016-tui-keymap.md`. No Claude references. Do not push.

## Steps

### Step 1: Convert crossterm keys

In `src/tui/keys.rs`, `pub fn keystroke(event: &KeyEvent) -> Option<Keystroke>`:
`KeyCode::Char(c)` becomes key `c` lowercased with `shift` set when `c` is uppercase or the
event has `SHIFT`; `' '` becomes `space`; named `KeyCode`s map to the names in "Current state";
`F(n)` to `f<n>`; `CONTROL` sets `ctrl`, `ALT` sets `alt`. Anything else is `None`.
Because `secondary` resolves to `ctrl` outside macOS, `secondary-shift-a` matches ctrl-shift-a.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 2: Look up actions

Resolve the keymap once in `tui::run` (`Keymap::resolve(&config.keys)`, mapping an error to
`std::io::Error::other`) and pass it to `handle_key`. Replace the hardcoded `match` with:

- `Overlay::Confirm`: look up in `[Scope::Confirm]`; `confirm.accept` and `confirm.toggle_mode`
  do what `y`/enter and `t` did; anything else cancels the overlay, as now.
- `Overlay::Help`: any key closes it, as now.
- `Overlay::None`: look up in `[Scope::Global, Scope::List]` and dispatch by action name to the
  same `App` methods as today (`delete.cursor` → `app.delete()`, `delete.selected` →
  `app.delete_selected()`, `nav.page_down` → `app.move_by(PAGE)`, and so on).

Multi-stroke chords: keep the last keystroke when it is the first stroke of a configured
multi-stroke chord, as plan 015 does for the GUI; defaults have none.

**Verify**: `cargo test -p kuizer tui:: -- --test-threads=2` passes.

### Step 3: Help overlay from the keymap

In `src/tui/view.rs`, build the key hint rows from `keymap.keys(action)` (chord `text`) instead of
literals; keep the descriptions. Pass the keymap to the view the same way `App` state reaches it.

**Verify**: `cargo run -p kuizer -- .`, press `?`: the overlay lists the same keys as before plus
`ctrl-d`/`ctrl-u`.

### Step 4: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need".

## Test plan

In `src/tui/keys.rs` or `src/tui/mod.rs` tests:

1. `converts_crossterm_events`: `Char('G')` with and without `SHIFT` → `shift-g`; `Char(' ')` →
   `space`; `PageDown` → `pagedown`; `Char('d')` with `CONTROL` → `ctrl-d`.
2. `rebound_keys_move_the_cursor`: an `App` with three items, a keymap with `"nav.down" = ["n"]`;
   `n` moves the cursor, `j` does not. Follow the existing tests in `src/tui/app.rs` for building
   `App` and items; call `handle_key` with an unbounded `jobs` channel.
3. `confirm_scope_only_in_the_dialog`: in `Overlay::Confirm`, `t` toggles the mode; with
   `Overlay::None`, `t` does nothing.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -n "KeyCode::Char('j')\|KeyCode::Char('s')\|KeyCode::Char('a')" src/tui/mod.rs` prints nothing
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- A TUI key has no action in `src/keymap` `ACTIONS`.
- The drift check shows TUI commits from outside these plans (for example shift-selection with
  `J`/`K`): those keys need actions in the core first; report them.

## Maintenance notes

- New TUI features add their action to `src/keymap` `ACTIONS` first, then dispatch here.
- Reviewers: run the TUI and press every key in the help overlay; check `esc` still quits from
  the list and only cancels inside the confirm dialog.
