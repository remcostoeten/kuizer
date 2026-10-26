# Plan 005: Add macOS app, system and Xcode cache entries to the catalog

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/catalog/data/macos.toml src/catalog/mod.rs`
> Every commit listed must carry `Plan: plans/001-foundation-contracts.md`. Anything else is drift: STOP.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED (a wrong `safe` mark lets bulk selection remove data that is not a cache)
- **Depends on**: plans/001-foundation-contracts.md
- **Wave**: 1, parallel with 002 to 004 and 006 to 009
- **Model**: Sonnet 5.5, high effort (needs research per entry)
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

kuizer is growing from a developer cleaner into a general PC cleaner on macOS and Linux. Its
location catalog (`src/catalog`, defined by plan 001) lists folders outside projects that hold
regenerable data. This plan fills `src/catalog/data/macos.toml` with macOS caches, logs and Xcode
data in the user's home folder. It only writes data; plan 002 resolves entries into paths, and the
delete guard checks every path before removal.

## Current state

- Entry format (from `src/catalog/mod.rs`, plan 001):

  ```toml
  [[entry]]
  name = "xcode-derived-data"    # unique, a-z 0-9 and -, used by `kuizer cache --select`
  app = "Xcode"                  # owner shown to the user
  group = "developer"            # developer | apps | browsers | system
  category = "cache"             # cache (default) | logs | temp
  kind = "dir"                   # dir (default) | file
  base = "library"               # home | cache | config | data | state | library | ...
  path = "Developer/Xcode/DerivedData"   # relative, "/"-separated; a component may be exactly "*"
  safety = "safe"                # safe | review (default review)
  platforms = ["macos"]
  processes = ["Xcode"]          # executable names of the owning app
  note = "..."                   # one plain sentence; required when safety = "review"
  ```

  On macOS, `library` is `~/Library`, `cache` is `~/Library/Caches`, `config` and `data` are
  `~/Library/Application Support`. `state` does not exist on macOS.
- `src/catalog/data/macos.toml` holds one header comment line. Keep it.
- `developer.toml` (plan 002) already covers npm, pnpm, yarn, bun, deno, cargo, pip, uv, poetry,
  go, gradle, maven, composer, jetbrains, huggingface, bazel, playwright, puppeteer, electron and
  similar tool caches (`base = "cache"` resolves to `~/Library/Caches` on macOS). Do not duplicate them.
- `browsers.toml` (plan 011) covers Safari, Chrome, Firefox and Electron apps (Slack, Discord,
  VS Code, Teams). Do not add those here.
- `src/protect.rs` refuses `Library/Keychains` and browser folders; entries there are dropped.
- Plan 002 drops the outer of two nested results, so avoid entries that contain each other.
- `catalog::tests::embedded_catalog_is_valid` validates every data file and name uniqueness.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Validate data | `cargo test -p kuizer catalog:: -- --test-threads=2` | all pass |

You are on Linux; the entries cannot be checked against a real Mac here. Research them.

## Suggested executor toolkit

- Use web search to confirm each path against Apple's documentation, the vendor's support pages
  or well-known references. Record the URL per entry in your final report (not in the file).

## Scope

**In scope**: `src/catalog/data/macos.toml` only.

**Out of scope**: every other file.

## Git workflow

- Branch `general/005-catalog-data-macos` from `feat/general-cleanup`.
- One commit `feat(catalog): macOS caches, logs and Xcode data`, body line
  `Plan: plans/005-catalog-data-macos.md`. No Claude references. Do not push.

## Steps

### Step 1: Add the seed entries

All with `platforms = ["macos"]` except where the table says otherwise. Verify each before writing.

| name | app | group | base | path | category | safety | processes | note (summary) |
|---|---|---|---|---|---|---|---|---|
| `xcode-derived-data` | Xcode | developer | library | `Developer/Xcode/DerivedData` | cache | safe | `Xcode` | Build products and indexes; Xcode rebuilds them. |
| `xcode-device-support` | Xcode | developer | library | `Developer/Xcode/iOS DeviceSupport` | cache | review | `Xcode` | Debug symbols per iOS version; copied again when that device connects. |
| `xcode-archives` | Xcode | developer | library | `Developer/Xcode/Archives` | cache | review | `Xcode` | App archives with the debug symbols needed to read crash reports of shipped builds. |
| `xcode-cache` | Xcode | developer | cache | `com.apple.dt.Xcode` | cache | safe | `Xcode` | Xcode's own cache. |
| `simulator-caches` | iOS Simulator | developer | library | `Developer/CoreSimulator/Caches` | cache | safe | `Simulator` | Simulator caches. |
| `cocoapods-cache` | CocoaPods | developer | cache | `CocoaPods` | cache | safe | | Downloaded pods. |
| `diagnostic-reports` | macOS | system | library | `Logs/DiagnosticReports` | logs | safe | | Crash and hang reports already sent or not needed. |
| `simulator-logs` | iOS Simulator | developer | library | `Logs/CoreSimulator` | logs | safe | | Simulator logs. |
| `sandboxed-app-caches` | Sandboxed apps | apps | library | `Containers/*/Data/Library/Caches` | cache | review | | Cache folder of each sandboxed app; check which app first. |
| `spotify-cache-macos` | Spotify | apps | cache | `com.spotify.client` | cache | review | `Spotify` | Streaming cache and downloaded songs; downloads must be fetched again. |
| `steam-shader-cache-macos` | Steam | apps | data | `Steam/steamapps/shadercache` | cache | review | `steam_osx` | Compiled shaders per game. |

Notes:

- `sandboxed-app-caches` may need Full Disk Access to read; that is fine, unreadable folders are
  skipped at runtime.
- Names must be unique across all data files, which is why Spotify and Steam get a `-macos` suffix
  (the Linux plan uses `spotify-cache` and `steam-shader-cache`).

**Verify**: `cargo test -p kuizer catalog:: -- --test-threads=2` passes.

### Step 2: Add more entries, carefully

Add further macOS entries you can confirm (for example Adobe media caches, Zoom, Microsoft Office
caches under `~/Library/Containers`, Android Studio and Unity editor caches if not in
`developer.toml`). Limits:

- At most 25 additional entries.
- `safe` only when the vendor documents the folder as a cache it rebuilds. Otherwise `review` with
  a note saying what is lost.
- Never add Homebrew's cache (`~/Library/Caches/Homebrew`): plan 007 lists it through
  `brew cleanup --dry-run`, and listing it here too would count it twice.
- Never add: `~/Library/Application Support/MobileSync/Backup` (iPhone backups), Mail data, Photos
  libraries, Messages, iCloud Drive (`Mobile Documents`), Keychains, anything owned by a browser
  or Electron app, `~/.Trash`, or any `com.apple.*` folder whose purpose you cannot confirm.

**Verify**: `cargo test -p kuizer catalog:: -- --test-threads=2` passes.

## Test plan

No new tests: `embedded_catalog_is_valid` validates the file and name uniqueness.

## Done criteria

- [ ] `cargo test -p kuizer catalog:: -- --test-threads=2` passes
- [ ] `grep -n "MobileSync\|Keychains\|Mobile Documents\|Safari\|Chrome\|Firefox\|Slack\|Homebrew" src/catalog/data/macos.toml` prints nothing
- [ ] `git diff --name-only feat/general-cleanup` prints only `src/catalog/data/macos.toml`
- [ ] Your report lists a source URL per entry and every entry you left out

## STOP conditions

- A seed path is documented differently for current macOS versions (14 and later). Report it.
- Validation needs a schema change. Do not change `src/catalog/mod.rs`.

## Maintenance notes

- `processes` feeds plan 010's running-app check (on macOS it matches the executable name).
- Reviewers: check every `safe` entry against its source URL, and test on a Mac before release:
  `kuizer cache --group apps --group system` on a real machine.
