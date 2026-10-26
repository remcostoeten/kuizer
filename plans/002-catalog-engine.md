# Plan 002: Resolve the location catalog and move the tool caches into it

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/caches.rs src/catalog src/commands/cache.rs src/commands/dashboard.rs src/output/text.rs src/output/json.rs crates/kuizer-gui/src/model.rs crates/kuizer-gui/src/snapshot.rs crates/kuizer-gui/src/app.rs crates/kuizer-gui/src/app/keys.rs`
> Every commit listed must carry `Plan: plans/001-foundation-contracts.md`. Any other commit is
> drift: compare the excerpts below with the live code and STOP on a mismatch.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED (replaces the source of `kuizer cache` and the GUI's Global caches section)
- **Depends on**: plans/001-foundation-contracts.md
- **Wave**: 1, parallel with 003 to 009
- **Model**: Opus 5.5, high effort
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

`src/caches.rs` holds a hardcoded Rust array of about 30 developer tool caches. To clean app,
browser and system caches too, kuizer needs that list as data that can grow to hundreds of
entries and that users can extend. Plan 001 defined the data format (`src/catalog`). This plan
writes the engine that turns catalog entries into real paths on this machine, moves the existing
tool caches into `src/catalog/data/developer.toml` unchanged, and switches `kuizer cache`, the
dashboard and the GUI to it. After this plan, adding a cache is adding a TOML entry.

## Current state

- `src/catalog/mod.rs` (from plan 001): `Entry { name, app, group, category, kind, base, path,
  safety, platforms, processes, note }`, enums `Group`, `Base`, `Platform`, `parse(text, origin)`,
  `check_unique`, `embedded()`, and an empty `CatalogConfig` with `validate(&self) -> Result<(),
  ConfigError>`. Data files `src/catalog/data/{developer,linux,macos,browsers}.toml` hold one
  comment line each. Plans 004, 005 and 011 fill `linux.toml`, `macos.toml`, `browsers.toml` in
  parallel with you: do not touch those three files.
- `src/caches.rs` today:
  - `enum Base { Home, Cache, Data, Cargo, GoModules, Gradle, Bun }`, `struct Spec { name, tool,
    base, relative, safety, note }` and `const SPECS: &[Spec]` (about 30 entries, lines 26-266).
  - `pub struct Locations { home, cache, data, cargo, go_modules, gradle, bun }` with
    `Locations::under(home)` (test layout under one folder) and `Locations::for_user()` (uses
    `dirs::cache_dir`, `dirs::data_dir`, and `CARGO_HOME`, `GOMODCACHE`/`GOPATH`,
    `GRADLE_USER_HOME`, `BUN_INSTALL`).
  - `pub struct KnownCache { name: &'static str, tool: &'static str, path, safety, note: &'static str }`.
  - `pub fn names()`, `pub fn known(name, path)`, `pub fn find(locations, protected) -> Vec<KnownCache>`:
    resolves every spec, keeps real directories (`symlink_metadata(..).is_dir()`) that
    `protected.covers` does not cover.
- Callers of the caches API:
  - `src/commands/cache.rs`: `measure_with` builds a `Candidate` per cache with
    `kind: Kind::Dir`, `category: Category::Cache`, `ecosystem: cache.tool`;
    `run` calls `removal::check_selection(&args.removal.select, &caches::names().collect())`, then
    `measure(caches::find(&locations, &Protected::for_user()))`.
  - `src/commands/dashboard.rs:58-59`: same `find` call.
  - `src/output/text.rs:425` `print_caches` (columns SIZE, SAFETY, NAME, TOOL, PATH) and
    `src/output/json.rs:278` `cache_report` (`CacheJson { name, tool, path, safety, note, ... }`).
  - `crates/kuizer-gui/src/model.rs:404-406`: `caches::find(&locations, &Protected::for_user())`
    filtered by `config.excludes()`.
  - `crates/kuizer-gui/src/snapshot.rs`: `Stored.caches: Vec<(String, PathBuf, StoredCandidate)>`,
    restored with `caches::known(&name, path)`; `SCHEMA` constant guards the file format; a test
    near line 540 builds a cache with `caches::known("npm", ...)`.
  - `crates/kuizer-gui/src/app/keys.rs:~100` `shows_cache` uses `cache.name`; `crates/kuizer-gui/src/app.rs:~4960`
    `cache_label` uses `cache.name` and `cache.tool`.
- `src/protect.rs`: `Protected::refuses(path)` (plan 001, same as `covers` for now; plan 003
  refines it in parallel). Use `refuses`, never `covers`, in the catalog.
- `src/cli.rs`: `CacheArgs.group: Vec<catalog::Group>` exists (plan 001) but nothing reads it.
- Conventions: function items, no comments except the allowed one-liners, `@name/@description/@example`
  doc blocks on exported functions (see `src/caches.rs` `find`), `thiserror` errors, tests in
  `#[cfg(test)] mod tests` with `tempfile`.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Catalog tests | `cargo test -p kuizer catalog:: -- --test-threads=2` | all pass |
| Cache tests | `cargo test -p kuizer -- caches:: commands::cache --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| GUI tests | `cargo test -p kuizer-gui -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |

One cargo command at a time.

## Scope

**In scope**:

- `src/catalog/mod.rs`, `src/catalog/resolve.rs` (create), `src/catalog/data/developer.toml`
- `src/caches.rs`
- `src/commands/cache.rs`, `src/commands/dashboard.rs`
- `src/output/text.rs` (only `print_caches`), `src/output/json.rs` (only `cache_report`/`CacheJson`)
- `crates/kuizer-gui/src/model.rs` (only the `caches::find` call), `crates/kuizer-gui/src/snapshot.rs`,
  `crates/kuizer-gui/src/app.rs` (only `cache_label`), `crates/kuizer-gui/src/app/keys.rs` (only `shows_cache`)
- `docs/config.example.toml` (only inside the `[catalog]` section)

**Out of scope**:

- `src/catalog/data/linux.toml`, `macos.toml`, `browsers.toml`: other plans fill them now.
- `src/protect.rs` and `src/delete/`: plan 003.
- Checking whether an app is running (`processes`): plan 010. Carry the field through, do not act on it.
- Any GUI layout or new section: plans 014 and 017.

## Git workflow

- Branch `general/002-catalog-engine` from `feat/general-cleanup` (after 001 merged).
- One commit `feat(core): resolve the location catalog and move tool caches into it`, body line
  `Plan: plans/002-catalog-engine.md`. No Claude references. Do not push.

## Steps

### Step 1: Move `SPECS` into `developer.toml`

Translate every `Spec` in `src/caches.rs` into one `[[entry]]` in
`src/catalog/data/developer.toml`, in the same order, keeping the header line:

| Spec field | Entry field |
|---|---|
| `name` | `name` (unchanged, history and `--select` use these names) |
| `tool` | `app` |
| `base` | `base`: Home→`home`, Cache→`cache`, Data→`data`, Cargo→`cargo`, GoModules→`go-modules`, Gradle→`gradle`, Bun→`bun` |
| `relative` | `path` (`""` stays `""`) |
| `safety` | `safety` (always write it, even `review`) |
| `note` | `note` (omit when empty) |
| (new) | `group = "developer"` |

Leave `category`, `kind`, `platforms` and `processes` at their defaults.

**Verify**: `cargo test -p kuizer catalog::tests::embedded_catalog_is_valid -- --test-threads=2`
passes, and `grep -c '^\[\[entry\]\]' src/catalog/data/developer.toml` equals the number of
`Spec {` blocks in `git show 5d04119:src/caches.rs` (`git show 5d04119:src/caches.rs | grep -c 'Spec {'`, minus 1 for the struct definition).

### Step 2: Locations for every base

Move `Locations` and `env_dir` from `src/caches.rs` into `src/catalog/resolve.rs` (re-export from
`catalog`, and keep `pub use crate::catalog::Locations;` in `src/caches.rs` so existing imports
compile). Add fields `config: PathBuf`, `state: Option<PathBuf>`, `library: Option<PathBuf>`:

- `under(home)`: `config = home/.config`, `state = Some(home/.local/state)`,
  `library = Some(home/Library)`; the rest as today.
- `for_user()`: `config = dirs::config_dir()`, `state = dirs::state_dir()`, and `library =
  Some(home/Library)` only when `cfg!(target_os = "macos")`, else `None`.

Add `fn base(&self, base: Base) -> Option<&Path>` covering all ten `Base` values.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 3: Resolve entries

In `src/catalog/resolve.rs` add:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub entry: Entry,
    pub path: PathBuf,
}

pub fn resolve(entries: &[Entry], locations: &Locations, platform: Platform, protected: &Protected) -> Vec<Resolved>
```

Rules, in order:

1. Skip entries whose `platforms` is non-empty and does not contain `platform`.
2. Skip entries whose base resolves to `None`.
3. Walk `path` component by component from the base. A literal component is joined. A `*`
   component lists the current directory with `fs::read_dir`, keeps children whose
   `symlink_metadata` is a real directory (never a symlink), sorted by name, and continues from
   each. Unreadable directories yield nothing (no panic, no issue).
4. The final path must exist with `symlink_metadata`, must not be a symlink, and must match
   `kind` (`Dir` → `is_dir`, `File` → `is_file`).
5. Skip it when `protected.refuses(&path)`.
6. Deduplicate: when two results have the same path, keep the first. When one result's path is
   inside another's, keep the inner one and drop the outer one, so nothing is counted or removed
   twice.

Add `pub fn current_platform() -> Platform` (`Macos` when `cfg!(target_os = "macos")`, else `Linux`).

**Verify**: `cargo build -p kuizer` exit 0.

### Step 4: Catalog config

Give `CatalogConfig` its fields (all `#[serde(default)]`):

```rust
pub struct CatalogConfig {
    pub disabled: Vec<String>,
    pub locations: Vec<Entry>,
    pub unknown_caches: bool,
}
```

with `Default` setting `unknown_caches: true`. `validate` returns
`ConfigError::Invalid { key, message }` (from plan 001) when:

- a `[[catalog.locations]]` entry fails the same checks `parse` applies (factor those checks into
  `fn check_entry(entry: &Entry) -> Result<(), CatalogError>` and call it from both places);
- a user entry's name collides with an embedded name;
- a name in `disabled` is neither embedded nor a user location.

Add `pub fn entries(config: &CatalogConfig) -> Vec<Entry>`: user locations first, then
`embedded()`, minus `disabled`.

Document the three keys in `docs/config.example.toml` under `[catalog]`:

```toml
[catalog]
disabled = ["jetbrains"]
# List the folders directly under your cache folder that no entry claims, marked review.
unknown_caches = true

[[catalog.locations]]
name = "my-renderer-cache"
app = "Renderer"
group = "apps"
base = "cache"
path = "renderer"
safety = "safe"
```

Make sure `jetbrains` is a real embedded name (it is in `SPECS` today) so `parses_full_example` passes.

**Verify**: `cargo test -p kuizer config:: -- --test-threads=2` passes.

### Step 5: Rebuild `caches.rs` on the catalog

Replace `Spec`, `Base` and `SPECS` in `src/caches.rs`. New public shape:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownCache {
    pub name: String,
    pub tool: String,
    pub group: Group,
    pub category: Category,
    pub kind: Kind,
    pub path: PathBuf,
    pub safety: Safety,
    pub note: String,
    pub processes: Vec<String>,
}

pub fn find(locations: &Locations, protected: &Protected, catalog: &CatalogConfig) -> Vec<KnownCache>
pub fn names(found: &[KnownCache], catalog: &CatalogConfig) -> Vec<String>
```

`tool` keeps its name (it is the JSON field and the GUI label) and holds the entry's `app`.
`find` resolves `catalog::entries(catalog)` for `current_platform()`, then, when
`catalog.unknown_caches` is true, adds one item per direct child of `locations.cache` that is a
real directory, is not a symlink, is not refused by `protected.refuses`, and is not equal to,
inside, or a parent of any resolved path. Such an item has `name = format!("other:{child}")`,
`tool = child`, `group = Apps`, `category = Cache`, `kind = Dir`, `safety = Review`, empty
`processes`, and `note = "No catalog entry. Programs keep data they can rebuild in this folder,
but check which program owns it first."`. `names` returns every catalog entry name plus every
found name, so `--select other:spotify` is accepted after a listing.

Delete `known`. Keep the `@name/@description/@example` doc blocks on `find` and `names`.

**Verify**: `cargo build -p kuizer` shows errors only in the callers fixed in step 6.

### Step 6: Callers

- `src/commands/cache.rs`: `run` calls `caches::find(&locations, &Protected::for_user(),
  &config.catalog)`, filters by `args.group` when it is non-empty, then checks the selection with
  `caches::names(&found, &config.catalog)`. `measure_with` uses `kind: cache.kind`,
  `category: cache.category`, `ecosystem: cache.tool.clone()`, `name: cache.name.clone()`.
- `src/commands/dashboard.rs`: pass `&config.catalog`.
- `src/output/text.rs` `print_caches`: add a GROUP column (`group` as kebab-case text) between
  NAME and TOOL; widen NAME to 28. Change the empty message to "No known caches found".
- `src/output/json.rs` `CacheJson`: add `group` and `category` fields (additive, schema version
  unchanged); adapt the borrowed fields to `String`.
- `crates/kuizer-gui/src/model.rs`: pass `&config.catalog` to `caches::find`.
- `crates/kuizer-gui/src/snapshot.rs`: store `Vec<(KnownCache, StoredCandidate)>`, restore it
  directly, increment `SCHEMA` by one, and rewrite the test that used `caches::known` to build a
  `KnownCache` literal.
- `crates/kuizer-gui/src/app.rs` `cache_label` and `app/keys.rs` `shows_cache`: adjust for
  `String` fields (`&cache.name`, `cache.tool.as_str()`).

**Verify**: `cargo build -p kuizer && cargo build -p kuizer-gui` exit 0; `cargo run -p kuizer --
cache --json | head -c 400` shows `"group":"developer"` on developer caches.

### Step 7: Tests and full checks

Write the tests in "Test plan", then run every command in "Commands you will need".

## Test plan

In `src/catalog/resolve.rs` (fake home with `tempfile`, `Locations::under(home)`,
`Protected::new(Some(home))`, entries built with `parse` from inline TOML):

1. `expands_wildcards_to_real_directories_only`: `path = "browser/*/Cache"` with two profile dirs
   that hold `Cache`, one profile without it, and one symlinked profile; exactly two results, in
   name order.
2. `skips_other_platforms_missing_bases_and_wrong_kinds`: a `platforms = ["macos"]` entry when
   resolving for `Linux`, a `kind = "file"` entry pointing at a directory, a missing path.
3. `keeps_the_inner_of_nested_results`: entries for `logs` and `logs/crash`; only `logs/crash` is returned.
4. `skips_refused_paths`: an entry under `.ssh` (protected) is not returned.

In `src/caches.rs`:

5. Keep `finds_existing_caches_only` (adapted to `find(.., &CatalogConfig { unknown_caches: false, ..Default::default() })`).
6. `lists_unknown_cache_folders_for_review`: `.cache/pip` (a developer entry), `.cache/someapp`
   (unknown), `.cache/google-chrome` (protected): only `other:someapp` is added, with `Review`.
7. `user_locations_and_disabled_entries_apply`: a user entry is found, a disabled embedded entry is not.

In `src/config/mod.rs` tests, extend `parses_full_example` only if it fails; add
`rejects_unknown_disabled_catalog_entry` modeled on the existing `rejects_unknown_fields`.

Keep `commands::cache` `measure_with_reports_each_cache_and_drops_empty_ones` passing (build the
`KnownCache` literal with the new fields).

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -n "const SPECS" src/caches.rs` prints nothing
- [ ] `grep -rn "caches::known" src crates` prints nothing
- [ ] `cargo run -p kuizer -- cache --group developer --json` exits 0 and every item has `"group":"developer"`
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- `SPECS` has a field or base not listed in the step 1 table.
- A GUI file needs more than the listed signature fixes to compile.
- `cargo run -p kuizer -- cache` on this machine lists a path under a browser profile, `.ssh`,
  `.gnupg` or a keyring folder.
- Making `parses_full_example` pass needs a change outside `[catalog]` in `docs/config.example.toml`.

## Maintenance notes

- Plan 010 adds the running-app check inside `find`; keep `processes` on `KnownCache`.
- Plan 003 changes what `Protected::refuses` allows (profile cache folders). The catalog must keep
  calling `refuses`, so browser cache entries from plan 011 appear once 003 lands.
- Reviewers: confirm every old cache name still exists (`kuizer cache --json` before and after on
  the same machine lists the same developer names) and that unknown-cache items are never `safe`.
