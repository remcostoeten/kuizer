# Plan 009: Build the shared keymap in the core

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/keymap src/tui/mod.rs crates/kuizer-gui/src/commands.rs`
> Every commit listed must carry `Plan: plans/001-foundation-contracts.md`. Anything else is drift:
> compare the excerpts below with the live code and STOP on a mismatch.

## Status

- **Priority**: P1 (the GUI and TUI plans 015, 016 and 017 need it)
- **Effort**: L
- **Risk**: MED (wrong defaults change how both apps respond to keys once 015 and 016 land)
- **Depends on**: plans/001-foundation-contracts.md
- **Wave**: 1, parallel with 002 to 008
- **Model**: Opus 5.5, high effort
- **Category**: tech-debt
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

The general cleanup work adds new sections and actions to the GUI and the TUI. The project rules
(`AGENTS.md`) say: every action has a stable name (`select.toggle`, `delete.selected`,
`nav.down`), a default binding and an override in config; both frontends read the same keymap
from the core, so a rebind applies to both; this shared keymap must be built in `src/` before
more bindings are added. Today the GUI keeps its keys in a table in
`crates/kuizer-gui/src/commands.rs` and the TUI hardcodes them in `src/tui/mod.rs`. This plan
builds the core keymap and its `[keys]` config. It does not change either frontend; plans 015
(GUI) and 016 (TUI) switch them over.

## Current state

- Plan 001 created `src/keymap/mod.rs` with an empty `KeysConfig` (serde default,
  `deny_unknown_fields`, `validate(&self) -> Result<(), ConfigError>` returning `Ok(())`),
  `Config.keys`, `ConfigError::Invalid { key: String, message: String }`, and an empty `[keys]`
  section in `docs/config.example.toml`.
- GUI keys (`crates/kuizer-gui/src/commands.rs`, `fn table()` around lines 150-265). Key strings
  use GPUI's keystroke syntax: modifiers joined with `-` before the key (`ctrl`, `alt`, `shift`,
  `cmd`, `fn`, and `secondary` meaning Cmd on macOS and Ctrl elsewhere), named keys such as
  `space`, `enter`, `escape`, `tab`, `backspace`, `delete`, `up`, `down`, `left`, `right`,
  `home`, `end`, `pageup`, `pagedown`, `menu`, `f1`..`f12`, and single characters (`?`, `/`,
  `G`). `secondary--` means secondary plus the `-` key. Several keystrokes separated by a space
  form a chord. Current GUI bindings:
  - General: palette `secondary-k` (extra `secondary-shift-p`), shortcuts `?`, rescan
    `secondary-r`, new scan `secondary-n`, undo `secondary-z`.
  - Sections: go to section `1`..`9` (`Tab::keys` in `crates/kuizer-gui/src/tab.rs`), space map
    `m`, next section `ctrl-tab`, previous `ctrl-shift-tab`.
  - Lists: down `j`/`down`, up `k`/`up`, first `g`/`home`, last `G`/`end`, `pagedown`,
    `pageup`, toggle `space`, select all `secondary-a`, select none `secondary-shift-a`, trash
    `enter`/`delete`, reveal `o`, copy path `y`, item menu `menu`/`shift-f10`, filter `/`.
  - Map: open `enter`, out `backspace`/`escape`, top `home`, toggle `space`, move with arrows.
  - Git: gc `c`, gc all `C`, merged branches `b`, all `B`.
  - View: zoom in `secondary-=` (extra `secondary-+`, `secondary-shift-=`), zoom out
    `secondary--`, reset `secondary-0`, sidebar `secondary-b`.
- TUI keys (`src/tui/mod.rs` `handle_key`, around lines 93-145): quit `q`/`esc` (and `ctrl-c`,
  which stays hardcoded), down `j`/down, up `k`/up, `pagedown`, `pageup`, first `g`/home, last
  `G`/end, toggle space, select safe `a`, clear `u`, delete selection or cursor item `d`/delete,
  delete selection `enter`, sort `s`, grouping `p`, select project `P`, filter `/`, cycle category
  `c`, safe only `f`, reset filters `x`, help `?`; in the confirm overlay `y`/enter confirm and
  `t` toggles trash or permanent.
- The core must not depend on GPUI or crossterm types; plain Rust only.
- Conventions: function items, no comments except allowed one-liners, `@name/@description/@example`
  doc blocks on exported functions, `thiserror` errors, tests in `#[cfg(test)] mod tests`.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Keymap tests | `cargo test -p kuizer keymap:: -- --test-threads=2` | all pass |
| Config tests | `cargo test -p kuizer config:: -- --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |

## Scope

**In scope**: `src/keymap/` (files inside), `docs/config.example.toml` (only inside `[keys]`).

**Out of scope**: `src/tui/`, `crates/kuizer-gui/` (plans 015 and 016), any new dependency.

## Git workflow

- Branch `general/009-shared-keymap` from `feat/general-cleanup`.
- One commit `feat(core): shared keymap with configurable bindings`, body line
  `Plan: plans/009-shared-keymap.md`. No Claude references. Do not push.

## Steps

### Step 1: Keystrokes and chords

In `src/keymap/keys.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keystroke {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    pub function: bool,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub text: String,
    pub strokes: Vec<Keystroke>,
}
```

- `Chord::parse(text: &str) -> Result<Chord, KeyError>`: split on single spaces into keystrokes.
  Each keystroke: if it ends with `--`, the key is `-`; otherwise the key is the part after the
  last `-`, and every part before it must be a modifier (`ctrl`, `alt`, `shift`, `cmd`, `fn`,
  `secondary`). `secondary` sets `cmd` on macOS and `ctrl` elsewhere (`cfg!(target_os = "macos")`).
  The key is a named key from the list in "Current state" or `f1`..`f24`, or exactly one
  printable character. A single ASCII uppercase letter `X` becomes key `x` with `shift` set.
  Named keys are lowercase. `text` keeps the original string, so frontends get exactly what the
  user wrote.
- `KeyError` (`thiserror`): empty, unknown modifier, unknown key, duplicate modifier.
- `Chord` equality for conflicts uses `strokes`, not `text` (`G` equals `shift-g`; `secondary-k`
  equals `ctrl-k` on Linux).

**Verify**: `cargo build -p kuizer` exit 0.

### Step 2: The action table

In `src/keymap/actions.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope { Global, List, Map, Git, Confirm }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frontend { Gui, Tui }

#[derive(Debug, Clone, Copy)]
pub struct Action {
    pub name: &'static str,
    pub keys: &'static [&'static str],
    pub scope: Scope,
    pub gui: bool,
    pub tui: bool,
}

pub const ACTIONS: &[Action] = &[ /* table below */ ];
```

Fill `ACTIONS` with exactly this table. It keeps every current key of both frontends; where the
two frontends had different keys for the same action, the action gets both.

| name | keys | scope | gui | tui |
|---|---|---|---|---|
| `palette.open` | `secondary-k`, `secondary-shift-p` | Global | yes | no |
| `help.open` | `?` | Global | yes | yes |
| `scan.rescan` | `secondary-r` | Global | yes | no |
| `scan.new` | `secondary-n` | Global | yes | no |
| `history.undo` | `secondary-z` | Global | yes | no |
| `app.quit` | `q`, `escape` | Global | no | yes |
| `section.next` | `ctrl-tab` | Global | yes | no |
| `section.previous` | `ctrl-shift-tab` | Global | yes | no |
| `section.overview` | `1` | Global | yes | no |
| `section.artifacts` | `2` | Global | yes | no |
| `section.caches` | `3` | Global | yes | no |
| `section.toolchains` | `4` | Global | yes | no |
| `section.docker` | `5` | Global | yes | no |
| `section.repos` | `6` | Global | yes | no |
| `section.duplicates` | `7` | Global | yes | no |
| `section.history` | `8` | Global | yes | no |
| `section.issues` | `9` | Global | yes | no |
| `view.space_map` | `m` | Global | yes | no |
| `view.zoom_in` | `secondary-=`, `secondary-+`, `secondary-shift-=` | Global | yes | no |
| `view.zoom_out` | `secondary--` | Global | yes | no |
| `view.zoom_reset` | `secondary-0` | Global | yes | no |
| `view.sidebar` | `secondary-b` | Global | yes | no |
| `nav.down` | `j`, `down` | List | yes | yes |
| `nav.up` | `k`, `up` | List | yes | yes |
| `nav.top` | `g`, `home` | List | yes | yes |
| `nav.bottom` | `G`, `end` | List | yes | yes |
| `nav.page_down` | `pagedown`, `ctrl-d` | List | yes | yes |
| `nav.page_up` | `pageup`, `ctrl-u` | List | yes | yes |
| `select.toggle` | `space` | List | yes | yes |
| `select.all` | `secondary-a` | List | yes | no |
| `select.safe` | `a` | List | no | yes |
| `select.none` | `secondary-shift-a`, `u` | List | yes | yes |
| `select.project` | `P` | List | no | yes |
| `delete.selected` | `enter`, `delete` | List | yes | yes |
| `delete.cursor` | `d` | List | no | yes |
| `item.reveal` | `o` | List | yes | no |
| `item.copy_path` | `y` | List | yes | no |
| `item.menu` | `menu`, `shift-f10` | List | yes | no |
| `filter.focus` | `/` | List | yes | yes |
| `sort.cycle` | `s` | List | no | yes |
| `group.toggle` | `p` | List | no | yes |
| `filter.category` | `c` | List | no | yes |
| `filter.safe_only` | `f` | List | no | yes |
| `filter.reset` | `x` | List | no | yes |
| `map.open` | `enter` | Map | yes | no |
| `map.out` | `backspace`, `escape` | Map | yes | no |
| `map.top` | `home` | Map | yes | no |
| `map.toggle` | `space` | Map | yes | no |
| `map.left` | `left` | Map | yes | no |
| `map.right` | `right` | Map | yes | no |
| `map.up` | `up` | Map | yes | no |
| `map.down` | `down` | Map | yes | no |
| `repo.gc` | `c` | Git | yes | no |
| `repo.gc_all` | `C` | Git | yes | no |
| `repo.branches` | `b` | Git | yes | no |
| `repo.branches_all` | `B` | Git | yes | no |
| `confirm.accept` | `y`, `enter` | Confirm | no | yes |
| `confirm.toggle_mode` | `t` | Confirm | no | yes |

Two deliberate changes, both noted for plans 015 and 016: `ctrl-d` and `ctrl-u` are new page
motions (`AGENTS.md` asks for them), and the TUI's `delete` key now deletes the selection
(`delete.selected`) while `d` keeps deleting the selection or the cursor item.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 3: Resolve the keymap

In `src/keymap/mod.rs`:

- Replace `KeysConfig` with a map, keeping `Default`:

  ```rust
  #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
  #[serde(transparent)]
  pub struct KeysConfig(pub BTreeMap<String, Vec<String>>);
  ```

- `pub struct Keymap { bindings: Vec<(Action, Vec<Chord>)> }` in `ACTIONS` order.
- `pub fn resolve(config: &KeysConfig) -> Result<Keymap, KeymapError>`: start from the defaults,
  replace an action's keys with the configured list (an empty list unbinds it), then check:
  - every configured name exists in `ACTIONS` (`KeymapError::UnknownAction`);
  - every key parses (`KeymapError::Key { action, key, source }`);
  - for each frontend, among the actions that frontend uses, no chord appears twice in one scope,
    and no chord of a `Global` action appears in any other scope (`KeymapError::Conflict {
    frontend, key, first, second }`). A chord that is a prefix of another chord in the same scope
    (`g` and `g g`) is also a conflict.
- `impl Keymap`: `pub fn defaults() -> Keymap` (resolve of an empty config, cannot fail; test it),
  `pub fn keys(&self, action: &str) -> &[Chord]`, `pub fn actions(&self, frontend: Frontend) ->
  impl Iterator<Item = (&Action, &[Chord])>`, and `pub fn lookup(&self, frontend: Frontend,
  scopes: &[Scope], chord: &Chord) -> Option<&'static str>` returning the first action in `ACTIONS`
  order whose scope is in `scopes` and that has an equal chord.
- `KeysConfig::validate` calls `resolve` and maps an error to `ConfigError::Invalid { key:
  "keys".into(), message: error.to_string() }`.

Document the section in `docs/config.example.toml`:

```toml
[keys]
"nav.down" = ["j", "down"]
"delete.selected" = ["enter"]
```

**Verify**: `cargo test -p kuizer config:: -- --test-threads=2` passes (`parses_full_example`).

### Step 4: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need".

## Test plan

In `src/keymap/` tests:

1. `defaults_resolve_without_conflicts`: `Keymap::defaults()` and `resolve(&KeysConfig::default())`
   succeed and every action of `ACTIONS` has at least one chord.
2. `parses_keystroke_syntax`: `secondary--` is the `-` key with ctrl (Linux) or cmd (macOS);
   `G` equals `shift-g`; `g g` has two strokes; `ctrl-shift-tab` has three modifiers set
   correctly; `hyper-x`, `ctrl-`, `ctrl-ctrl-x` and `` are errors.
3. `overrides_replace_and_unbind`: `"nav.down" = ["n"]` gives only `n`; `"item.reveal" = []`
   gives no chord; `"nav.dwon" = ["x"]` is `UnknownAction`.
4. `detects_conflicts_per_frontend`: binding `select.safe` (TUI only) to `c` conflicts with
   `filter.category` for the TUI; binding `repo.gc` (GUI, Git scope) to `j` does not conflict,
   because List and Git are different scopes; binding `nav.down` to `1` conflicts with the
   Global `section.overview` for the GUI; `nav.top` as `g` together with `nav.bottom` as `g g`
   is a prefix conflict.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `cargo tree -p kuizer -e normal | grep -c "gpui\|crossterm"` is the same as before this plan (`ratatui` already pulls `crossterm`; no new edge from `keymap`)
- [ ] `grep -rn "gpui" src/keymap` prints nothing
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- The defaults table above conflicts under your own conflict rule. Report the pair; do not
  silently drop a key.
- Matching GPUI's syntax needs parsing rules not listed here (read
  `~/.cargo/registry/src/*/gpui-0.2.2/src/platform/keystroke.rs` `Keystroke::parse` to compare,
  and report any difference instead of guessing).

## Maintenance notes

- Plan 015 makes the GUI build its table from `Keymap` (labels and command mapping stay in the
  GUI), plan 016 does the same for the TUI, and plan 017 adds `section.*` actions for the new
  sections. New actions are added to `ACTIONS` first, never to a frontend.
- Action names are a public, stable interface: users write them in config. Rename only with an
  alias.
- Reviewers: compare the table with the two frontends' current keys; every current key must
  appear.
