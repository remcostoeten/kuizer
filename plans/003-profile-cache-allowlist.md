# Plan 003: Let kuizer remove cache folders inside browser and Electron profiles, and nothing else there

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/protect.rs src/delete/guard.rs`
> Every commit listed must carry `Plan: plans/001-foundation-contracts.md`. Anything else is drift:
> compare the excerpts below with the live code and STOP on a mismatch.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: HIGH (changes what the delete guard refuses; a mistake here can delete cookies or saved logins)
- **Depends on**: plans/001-foundation-contracts.md
- **Wave**: 1, parallel with 002 and 004 to 009
- **Model**: Opus 5.5, high effort
- **Category**: security
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

Browser and Electron app caches are among the largest folders a general cleaner can free (Chrome,
Firefox, Discord, Slack, VS Code). kuizer protects whole browser and Electron profiles today,
which is right: they hold cookies, sessions and saved logins. This plan adds one narrow
exception: a directory whose name is a known disk cache name (`Cache`, `Code Cache`, `GPUCache`,
`cache2` and a few more), sitting inside a protected profile, may be listed and removed. Everything
else inside a profile stays refused, and credential and tool folders get no exception at all.
Plan 011 adds the catalog entries that point at these folders.

## Current state

- `src/protect.rs`:
  - `MARKER_SETS` (`["Local State"]`, `["profiles.ini"]`, `["Cookies", "Preferences"]`,
    `["cookies.sqlite", "prefs.js"]`): a directory holding all names of one set is profile data.
  - `BROWSERS` (browser folder names, joined under `.config`, `.cache` and `.local/share`),
    `HOME_DIRS` (browser home folders, `.ssh`, `.gnupg`, `.password-store`, `.pki`,
    `.local/share/keyrings`, `.local/share/kwalletd`, Flatpak and Snap browser folders, macOS
    browser folders and `Library/Keychains`), `TOOL_DIRS` (editor extensions, global package installs).
  - `Protected { dirs: HashSet<PathBuf>, homes: Vec<PathBuf> }`; `Protected::new(home)` fills
    `dirs` from all three lists for the home and its canonical path.
  - `is_known(path)`: exact match in `dirs` (the walker uses it to never enter these folders).
  - `covers(path)`: any ancestor of `path` (including itself) is in `dirs` or holds a marker set on disk:

    ```rust
    pub fn covers(&self, path: &Path) -> bool {
        path.ancestors()
            .any(|dir| self.dirs.contains(dir) || marked_on_disk(dir))
    }
    ```

  - `refuses(path)` (added by plan 001): currently returns `self.covers(path)`.
- `src/delete/guard.rs` `Guard::check`:

  ```rust
  pub fn check(&self, path: &Path, identity: Option<(u64, u64)>) -> Result<(), Refusal> {
      self.check_path(path)?;
      if self.protected.covers(path) {
          return Err(Refusal::AppData);
      }
      // symlink, identity, mount point checks follow
  ```

- Plan 002 (running in parallel) makes the catalog call `protected.refuses(path)` when resolving
  entries. You do not touch the catalog.
- Real layouts this plan must handle:
  - Chrome on Linux: `~/.config/google-chrome/Local State`, profile `~/.config/google-chrome/Default/`
    with `Preferences`, cookies in `Default/Network/Cookies` (newer Chromium) or `Default/Cookies`,
    caches `Default/Code Cache`, `Default/GPUCache`, and the HTTP cache under
    `~/.cache/google-chrome/Default/Cache`.
  - Electron apps (Discord, Slack, VS Code): user data such as `~/.config/discord/` holds
    `Local State`, `Cookies`, `Preferences`, and caches `Cache`, `Code Cache`, `GPUCache`. VS Code
    adds `CachedData` and `CachedExtensionVSIXs`.
  - Firefox on Linux: profile in `~/.mozilla/firefox/<id>/`, HTTP cache in
    `~/.cache/mozilla/firefox/<id>/cache2` and `startupCache`.
- Conventions: function items, no comments except allowed one-liners, `@name/@description/@example`
  doc blocks on exported functions, tests in `#[cfg(test)] mod tests` using `tempfile` only.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Protect and guard tests | `cargo test -p kuizer -- protect:: delete:: --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |

## Scope

**In scope**: `src/protect.rs`, `src/delete/guard.rs`, `tests/delete.rs` (only to add tests at the end).

**Out of scope**:

- `is_known` and `holds_app_data`: the walker and the duplicate finder must still never enter
  profiles. Do not change them.
- `src/catalog/`, `src/caches.rs`: plan 002. Catalog data: plan 011.
- Service worker storage (`Service Worker/CacheStorage`), `IndexedDB`, `Local Storage`,
  `Session Storage`: they hold offline data of web apps, not disk cache. Never allow them.

## Git workflow

- Branch `general/003-profile-cache-allowlist` from `feat/general-cleanup`.
- One commit `feat(core): allow cache folders inside browser and app profiles`, body line
  `Plan: plans/003-profile-cache-allowlist.md`. No Claude references. Do not push.

## Steps

### Step 1: Separate credentials from browser folders

In `src/protect.rs` move `.ssh`, `.gnupg`, `.password-store`, `.pki`, `.local/share/keyrings`,
`.local/share/kwalletd` and `Library/Keychains` out of `HOME_DIRS` into a new
`const CREDENTIAL_DIRS: &[&str]`. `Protected::new` still adds all of `HOME_DIRS`,
`CREDENTIAL_DIRS` and `TOOL_DIRS` to `dirs`, so `covers` and `is_known` behave exactly as
before. Also store the joined credential and tool paths in a new field
`never: Vec<PathBuf>` (for the home and its canonical path).

**Verify**: `cargo test -p kuizer protect:: -- --test-threads=2` passes unchanged.

### Step 2: Recognize profile cache folders

Add:

```rust
const PROFILE_CACHES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GraphiteDawnCache",
    "GrShaderCache",
    "ShaderCache",
    "cache2",
    "startupCache",
    "CachedData",
    "CachedExtensionVSIXs",
];
```

and a private `fn is_profile_cache(&self, path: &Path) -> bool` that is true only when all hold:

1. `path.file_name()` is in `PROFILE_CACHES` (exact, case-sensitive).
2. No entry of `self.never` is `path` or an ancestor of `path`.
3. `path` is not itself in `self.dirs`.
4. `fs::symlink_metadata(path)` is a real directory, not a symlink, and the same holds for its parent.
5. `path` does not itself look like a profile: `marked_on_disk(path)` is false, and none of
   `Local State`, `Cookies`, `Login Data`, `cookies.sqlite`, `logins.json`, `key4.db` exists
   directly inside it (`symlink_metadata(path.join(name)).is_ok()`).

Then change `refuses`:

```rust
/// @name refuses
/// @description Checks whether kuizer must leave `path` alone because it lies inside protected
/// application data. A known disk cache folder directly inside a browser or Electron profile,
/// such as `GPUCache` or Firefox's `cache2`, is the one exception; nothing inside credential
/// or tool folders is ever allowed.
///
/// @example
/// if protected.refuses(&found) {
///     continue;
/// }
pub fn refuses(&self, path: &Path) -> bool {
    self.covers(path) && !self.is_profile_cache(path)
}
```

Note that a path inside a cache folder (`Default/Cache/Cache_Data`) is still refused: only the
cache folder itself is allowed, and removing it removes its contents.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 3: Use it in the delete guard

In `src/delete/guard.rs` `check`, replace `self.protected.covers(path)` with
`self.protected.refuses(path)`. The symlink, identity and mount point checks after it stay as they
are. Do not change `check_path`, `PROTECTED` or `Refusal`.

**Verify**: `cargo test -p kuizer delete:: -- --test-threads=2` passes.

### Step 4: Tests

Write the tests in "Test plan", then run all commands in "Commands you will need".

## Test plan

In `src/protect.rs` tests, build a fake home with `tempfile` and `Protected::new(Some(home))`:

1. `allows_only_cache_folders_inside_chromium_profiles`. Create
   `.config/google-chrome/Local State`, `.config/google-chrome/Default/Preferences`,
   `.config/google-chrome/Default/Network/Cookies`, `.config/google-chrome/Default/GPUCache/data_0`,
   `.config/google-chrome/Default/Code Cache/js/index`,
   `.cache/google-chrome/Default/Cache/Cache_Data/f_000001`.
   `refuses` is false for `Default/GPUCache`, `Default/Code Cache` and
   `.cache/google-chrome/Default/Cache`. It is true for `.config/google-chrome`, `Default`,
   `Default/Network`, `Default/Network/Cookies`, `Default/GPUCache/data_0` and
   `.cache/google-chrome/Default/Cache/Cache_Data`.
2. `allows_electron_and_firefox_caches`: `.config/discord/{Local State,Cookies,Preferences}` plus
   `.config/discord/Cache/x` → `Cache` allowed; `.mozilla/firefox/ab.default/{prefs.js,cookies.sqlite}`
   is refused; `.cache/mozilla/firefox/ab.default/cache2/entries/x` → `cache2` allowed.
3. `never_allows_credentials_tools_or_profile_lookalikes`: `.ssh/Cache`,
   `.local/share/keyrings/Cache`, `.vscode/extensions/x/Cache` are refused; a
   `.config/discord/Cache` that contains `Local State` is refused; a `GPUCache` that is a symlink
   to a real folder is refused.

In `src/delete/guard.rs` tests:

4. `guard_allows_profile_cache_and_refuses_profile`: with the tree of test 1,
   `Guard::new(&[profile_dir]).with_cwd(None).with_protected(Protected::new(Some(home)))`:
   `check(&gpu_cache, None)` is `Ok`, `check(&network_dir, None)` is `Err(Refusal::AppData)`.

Existing tests must keep passing unchanged; if one of them needs editing, that is a STOP condition.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -n "protected.covers" src/delete/guard.rs` prints nothing
- [ ] `grep -n "fn is_known\|fn holds_app_data" src/protect.rs` shows both unchanged (`git diff feat/general-cleanup -- src/protect.rs` has no hunk inside them)
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] No commit mentions Claude

## STOP conditions

- An existing protect, guard or `tests/delete.rs` test fails after step 3.
- Allowing a cache folder seems to require changing `covers`, `is_known` or the walker.
- You find a real browser or Electron layout where a name in `PROFILE_CACHES` holds cookies,
  logins or keys. Report it instead of adding special cases.

## Maintenance notes

- `PROFILE_CACHES` is the only allowlist. Adding a name there widens what kuizer may delete inside
  every profile on the machine; each addition needs a test like test 1 and a reviewer who checks
  the browser's documentation.
- The walker still never enters profiles, so these folders only ever come from catalog entries
  (plan 011), never from a folder scan.
- Reviewers: read `is_profile_cache` against the five conditions above, and check that the
  guard's later symlink and identity checks still run for allowed paths.
