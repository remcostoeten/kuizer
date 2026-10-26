# Plan 011: Add browser and Electron app cache entries to the catalog

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/catalog/data/browsers.toml src/protect.rs`
> Every commit listed must carry `Plan:` with 001, 002 or 003. Anything else is drift: STOP.

## Status

- **Priority**: P1 (browser caches are the largest general win)
- **Effort**: M
- **Risk**: MED (entries point into protected profiles; the allowlist from plan 003 is the safety net)
- **Depends on**: plans/002-catalog-engine.md, plans/003-profile-cache-allowlist.md
- **Wave**: 2, parallel with 010 and 012 to 016
- **Model**: Sonnet 5.5, high effort (research per entry)
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

Browsers and Electron apps keep hundreds of megabytes to gigabytes of disk cache each. kuizer
protects their profiles because they hold cookies and saved logins; plan 003 added one narrow
exception for known cache folder names inside a profile. This plan adds the catalog entries that
point at those folders for the common browsers and Electron apps on Linux and macOS, plus one
integration test that proves cache folders are found and profile data is not.

## Current state

- Entry format (`src/catalog/mod.rs`):

  ```toml
  [[entry]]
  name = "chrome-gpu-cache"
  app = "Google Chrome"
  group = "browsers"
  base = "config"
  path = "google-chrome/*/GPUCache"
  safety = "safe"
  platforms = ["linux"]
  processes = ["chrome"]
  note = "Compiled graphics data; Chrome makes it again."
  ```

  A path component may be exactly `*` (one directory level, real directories only). Bases on
  Linux: `cache` `~/.cache`, `config` `~/.config`, `data` `~/.local/share`. On macOS: `cache`
  `~/Library/Caches`, `config` and `data` `~/Library/Application Support`, `library` `~/Library`.
- `src/catalog/data/browsers.toml` holds one header line. Keep it.
- `src/protect.rs` (plan 003): `Protected::refuses(path)` allows a path inside a protected
  profile only when its last component is one of `Cache`, `Code Cache`, `GPUCache`, `DawnCache`,
  `DawnGraphiteCache`, `DawnWebGPUCache`, `GraphiteDawnCache`, `GrShaderCache`, `ShaderCache`,
  `cache2`, `startupCache`, `CachedData`, `CachedExtensionVSIXs`, and nothing marks it as a
  profile. An entry whose last component is anything else is dropped at runtime if it lies in a
  protected folder. Do not change `src/protect.rs`.
- `src/caches.rs` (plan 002): `find(&Locations, &Protected, &CatalogConfig) -> Vec<KnownCache>`;
  `Locations::under(home)` builds the Linux layout under a test folder.
- Names must be unique across all data files; `catalog::tests::embedded_catalog_is_valid` checks.
- Electron user data folders on Linux (verify each): Discord `~/.config/discord`, Slack
  `~/.config/Slack`, VS Code `~/.config/Code`, VSCodium `~/.config/VSCodium`, Signal
  `~/.config/Signal`, Obsidian `~/.config/obsidian`, Element `~/.config/Element`, Teams for Linux
  `~/.config/teams-for-linux`. On macOS the same names under `~/Library/Application Support`.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Validate data | `cargo test -p kuizer catalog:: -- --test-threads=2` | all pass |
| Integration test | `cargo test -p kuizer --test profile_caches -- --test-threads=2` | all pass |
| Core tests | `cargo test -p kuizer -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |
| Try it | `cargo run -p kuizer -- cache --group browsers` | lists cache folders only |

## Suggested executor toolkit

- Web search to confirm each folder layout (Chromium's user data dir documentation, Mozilla's
  profile and cache documentation, each Electron app's data folder). Report one URL per app.

## Scope

**In scope**: `src/catalog/data/browsers.toml`, `tests/profile_caches.rs` (create).

**Out of scope**: `src/protect.rs`, the catalog engine, other data files. Service worker storage,
IndexedDB, Local Storage, Session Storage, extensions, history, cookies: never add them.

## Git workflow

- Branch `general/011-catalog-data-browsers` from `feat/general-cleanup` (with wave 1 merged).
- One commit `feat(catalog): browser and Electron app caches`, body line
  `Plan: plans/011-catalog-data-browsers.md`. No Claude references. Do not push.

## Steps

### Step 1: Browsers on Linux

For each Chromium-family browser, with its user data folder `U` under `config` and its disk
cache folder `C` under `cache`: Chrome (`U = google-chrome`, `C = google-chrome`), Chromium
(`chromium`), Brave (`BraveSoftware/Brave-Browser`), Microsoft Edge (`microsoft-edge`), Vivaldi
(`vivaldi`). Add entries, all `group = "browsers"`, `safety = "safe"`, `platforms = ["linux"]`,
with the browser's process name (`chrome`, `chromium`, `brave`, `msedge`, `vivaldi-bin`; verify):

| name suffix | base | path |
|---|---|---|
| `-cache` | cache | `C/*/Cache` |
| `-code-cache` | config | `U/*/Code Cache` |
| `-gpu-cache` | config | `U/*/GPUCache` |
| `-shader-cache` | config | `U/ShaderCache` |
| `-gr-shader-cache` | config | `U/GrShaderCache` |
| `-graphite-cache` | config | `U/GraphiteDawnCache` |

Name prefix per browser: `chrome`, `chromium`, `brave`, `edge`, `vivaldi`.

Firefox family: `firefox-cache` (cache, `mozilla/firefox/*/cache2`, process `firefox`) and
`librewolf-cache` (cache, `librewolf/*/cache2` if confirmed, process `librewolf`).

**Verify**: `cargo test -p kuizer catalog:: -- --test-threads=2` passes.

### Step 2: Electron apps on Linux

For each app in "Current state" with user data folder `A` under `config`: entries `-cache`
(`A/Cache`), `-code-cache` (`A/Code Cache`), `-gpu-cache` (`A/GPUCache`), all `group = "apps"`,
`safety = "safe"`, `platforms = ["linux"]`, `processes` the app's executable (`Discord`, `slack`,
`code`, `codium`, `signal-desktop`, `obsidian`, `element-desktop`, `teams-for-linux`; verify).
VS Code and VSCodium also get `-cached-data` (`A/CachedData`) and `-cached-vsix`
(`A/CachedExtensionVSIXs`). Prefixes: `discord`, `slack`, `vscode`, `vscodium`, `signal`,
`obsidian`, `element`, `teams`.

**Verify**: `cargo test -p kuizer catalog:: -- --test-threads=2` passes.

### Step 3: macOS

The same browsers and apps with `platforms = ["macos"]` and a `-macos` name suffix:

- Chrome: cache `Google/Chrome/*/Cache`; data `Google/Chrome/*/Code Cache`, `Google/Chrome/*/GPUCache`,
  `Google/Chrome/ShaderCache`, `Google/Chrome/GrShaderCache`. Brave, Edge, Vivaldi, Chromium with
  their macOS folder names (verify each).
- Firefox: cache `Firefox/Profiles/*/cache2`.
- Safari: `safari-cache-macos`, base `cache`, path `com.apple.Safari`, `safety = "review"`, note
  "Needs Full Disk Access to read; Safari makes it again." Safari's folder is not protected by
  kuizer, so no allowlist name is involved.
- Electron apps: data `<A>/Cache`, `<A>/Code Cache`, `<A>/GPUCache` (and the VS Code extras) with
  the macOS folder names (`discord`, `Slack`, `Code`, `Signal`, `obsidian`, `Element`; verify).

**Verify**: `cargo test -p kuizer catalog:: -- --test-threads=2` passes.

### Step 4: Integration test

Create `tests/profile_caches.rs` (`#![cfg(target_os = "linux")]`). Build a fake home in a
`TempDir` with:

- `.config/google-chrome/Local State`, `.config/google-chrome/Default/Preferences`,
  `.config/google-chrome/Default/Network/Cookies`, `.config/google-chrome/Default/GPUCache/data_0`,
  `.config/google-chrome/Default/Code Cache/js/a`, `.config/google-chrome/Default/IndexedDB/x`,
  `.cache/google-chrome/Default/Cache/Cache_Data/f_1`;
- `.config/discord/Local State`, `.config/discord/Cookies`, `.config/discord/Preferences`,
  `.config/discord/Cache/Cache_Data/f_1`, `.config/discord/Local Storage/leveldb/x`.

Call `kuizer::caches::find(&Locations::under(home), &Protected::new(Some(home)),
&CatalogConfig { unknown_caches: false, ..Default::default() })` and assert the returned paths are
exactly the Chrome `GPUCache`, `Code Cache`, `Cache` and the Discord `Cache` (as a sorted list),
and that no returned path contains `Cookies`, `IndexedDB`, `Local Storage`, `Preferences` or
`Local State`.

**Verify**: `cargo test -p kuizer --test profile_caches -- --test-threads=2` passes.

## Test plan

Covered by step 4 and `embedded_catalog_is_valid`.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -n "IndexedDB\|Local Storage\|Session Storage\|Service Worker\|Cookies\|Login Data\|Extensions" src/catalog/data/browsers.toml` prints nothing
- [ ] Every entry whose path lies inside a profile ends in a name from the allowlist in "Current state" (check by reading)
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] Your report lists a source URL per browser and app

## STOP conditions

- A browser or app keeps its disk cache under a folder name not in the allowlist. Report it; do
  not edit `src/protect.rs` and do not add an entry that would be dropped silently.
- The integration test finds a path you did not expect, or misses one, and the reason is in the
  engine or the allowlist rather than in your entries.

## Maintenance notes

- Browsers move cache folders between versions; when one moves, update the entry, never widen
  the allowlist without a plan like 003.
- Flatpak and Snap browsers (`~/.var/app/...`, `~/snap/...`) are protected and not covered here;
  they need entries with paths inside those folders and a test, as a follow-up.
- Reviewers: run `kuizer cache --group browsers --group apps --json` on a machine with Chrome and
  Discord installed, and check every listed path by hand.
