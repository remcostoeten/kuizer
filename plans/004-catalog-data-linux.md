# Plan 004: Add Linux app, desktop and system cache entries to the catalog

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: `git log --format='%h %s%n%b' 5d04119..HEAD -- src/catalog/data/linux.toml src/catalog/mod.rs`
> Every commit listed must carry `Plan: plans/001-foundation-contracts.md`. Anything else is drift: STOP.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED (a wrong `safe` mark lets bulk selection remove data that is not a cache)
- **Depends on**: plans/001-foundation-contracts.md
- **Wave**: 1, parallel with 002, 003 and 005 to 009
- **Model**: Sonnet 5.5, high effort (needs research per entry)
- **Category**: direction
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

kuizer is growing from a developer cleaner into a general PC cleaner. Its location catalog
(`src/catalog`, defined by plan 001) lists folders outside projects that hold regenerable data.
This plan fills `src/catalog/data/linux.toml` with Linux desktop, system and app caches and logs
that live in the user's home folder. It only writes data; another plan (002) turns entries into
paths, and the existing delete guard still checks every path before removal.

## Current state

- Entry format (from `src/catalog/mod.rs`, plan 001):

  ```toml
  [[entry]]
  name = "thumbnails"            # unique, a-z 0-9 and -, used by `kuizer cache --select`
  app = "Desktop"                # who owns it, shown to the user
  group = "system"               # developer | apps | browsers | system
  category = "cache"             # cache (default) | logs | temp
  kind = "dir"                   # dir (default) | file
  base = "cache"                 # home | cache | config | data | state | library | cargo | go-modules | gradle | bun
  path = "thumbnails"            # relative to base, "/"-separated; a component may be exactly "*"
  safety = "safe"                # safe | review (default review)
  platforms = ["linux"]
  processes = []                 # executable names (as in /proc/<pid>/comm) of the owning app
  note = "..."                   # one plain sentence; required when safety = "review"
  ```

  On Linux, `cache` is `$XDG_CACHE_HOME` or `~/.cache`, `config` is `~/.config`, `data` is
  `~/.local/share`, `state` is `~/.local/state`.
- `src/catalog/data/linux.toml` holds one header comment line. Keep it.
- `src/catalog/data/developer.toml` (filled by plan 002 in parallel) already covers package
  manager and build tool caches: npm, pnpm, yarn, bun, deno, node-gyp, typescript, electron,
  playwright, puppeteer, cargo, sccache, pip, uv, poetry, pre-commit, go, gradle, maven, composer,
  jetbrains, huggingface, bazel. Do not duplicate them.
- `src/catalog/data/browsers.toml` (plan 011) covers browsers and Electron apps (Discord, Slack,
  VS Code, Signal, Teams, Obsidian and similar). Do not add those here.
- `src/protect.rs` refuses browser profiles, `.ssh`, `.gnupg`, keyrings and editor extension
  folders; entries pointing there are dropped at runtime, so do not add them.
- The test `catalog::tests::embedded_catalog_is_valid` parses every data file and checks that
  names are unique across files.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Validate data | `cargo test -p kuizer catalog:: -- --test-threads=2` | all pass |
| Format | `cargo fmt --all --check` | exit 0 |

## Suggested executor toolkit

- Use web search to confirm each path against the project's own documentation, source or man
  page. Record the URL you used for each entry in your final report (not in the file).

## Scope

**In scope**: `src/catalog/data/linux.toml` only.

**Out of scope**: every other file, including the other data files and `src/catalog/mod.rs`.

## Git workflow

- Branch `general/004-catalog-data-linux` from `feat/general-cleanup`.
- One commit `feat(catalog): Linux desktop, system and app caches`, body line
  `Plan: plans/004-catalog-data-linux.md`. No Claude references. Do not push.

## Steps

### Step 1: Add the seed entries

Add these entries, all with `platforms = ["linux"]`. Verify each path and safety level against
upstream documentation before writing it. If a path turns out wrong, fix it; if you cannot confirm
it, leave the entry out and list it in your report.

| name | app | group | base | path | kind | category | safety | processes | note (summary) |
|---|---|---|---|---|---|---|---|---|---|
| `thumbnails` | Desktop | system | cache | `thumbnails` | dir | cache | safe | | File previews; file managers make them again (freedesktop thumbnail spec). |
| `fontconfig-cache` | Fontconfig | system | cache | `fontconfig` | dir | cache | safe | | Font lists rebuilt on next use. |
| `mesa-shader-cache` | Mesa | system | cache | `mesa_shader_cache` | dir | cache | safe | | Compiled GPU shaders; games may stutter once while it refills. |
| `mesa-shader-cache-db` | Mesa | system | cache | `mesa_shader_cache_db` | dir | cache | safe | | Same as above, newer single-database layout. |
| `nvidia-gl-cache` | NVIDIA driver | system | cache | `nvidia/GLCache` | dir | cache | safe | | Compiled shaders of the NVIDIA driver. |
| `nvidia-compute-cache` | NVIDIA driver | system | home | `.nv/ComputeCache` | dir | cache | safe | | Compiled CUDA kernels. |
| `gstreamer-registry` | GStreamer | system | cache | `gstreamer-1.0` | dir | cache | safe | | Plugin registry, rebuilt at next start. |
| `tracker-index` | GNOME search | system | cache | `tracker3` | dir | cache | review | `localsearch-3`, `tracker-miner-f` | Search index; rebuilt by indexing all files again, which takes a while. |
| `baloo-index` | KDE search | system | data | `baloo` | dir | cache | review | `baloo_file` | Search index; rebuilt by indexing all files again. Turn indexing off first if you do not use it. |
| `flatpak-app-caches` | Flatpak apps | apps | home | `.var/app/*/cache` | dir | cache | review | | Cache folder of each Flatpak app; check which app first. |
| `spotify-cache` | Spotify | apps | cache | `spotify` | dir | cache | review | `spotify` | Streaming cache and downloaded songs; downloads must be fetched again. |
| `steam-shader-cache` | Steam | apps | data | `Steam/steamapps/shadercache` | dir | cache | review | `steam` | Compiled shaders per game; games stutter while it refills. |
| `steam-html-cache` | Steam | apps | data | `Steam/config/htmlcache` | dir | cache | safe | `steam` | Store and community page cache. |
| `wine-cache` | Wine | apps | cache | `wine` | dir | cache | safe | | Installers Wine downloaded (Gecko, Mono); fetched again when needed. |
| `winetricks-cache` | Winetricks | apps | cache | `winetricks` | dir | cache | safe | | Downloaded installers. |
| `lutris-cache` | Lutris | apps | cache | `lutris` | dir | cache | review | `lutris` | Installer downloads and runtime files. |
| `yay-cache` | yay | system | cache | `yay` | dir | cache | safe | | AUR build folders and built packages. |
| `paru-cache` | paru | system | cache | `paru` | dir | cache | safe | | AUR build folders and built packages. |
| `xsession-errors` | X session | system | home | `.xsession-errors` | file | logs | safe | | Log of the current or last X session. |
| `xsession-errors-old` | X session | system | home | `.xsession-errors.old` | file | logs | safe | | Log of the session before that. |

Write each `note` as one plain sentence, no em dashes, no marketing words.

**Verify**: `cargo test -p kuizer catalog:: -- --test-threads=2` passes.

### Step 2: Add more entries, carefully

Add further Linux entries you can confirm, for widely used desktop software whose folder is a
cache by its own documentation (for example GIMP's `babl`/`gegl` caches, Thunderbird's cache under
`~/.cache/thunderbird`, Zoom, Telegram Desktop's `tdata/user_data/cache` if confirmed). Limits:

- At most 25 additional entries.
- `safe` only when the owner documents that the folder is a cache it rebuilds and losing it costs
  nothing but time or bandwidth. Everything else is `review` with a note saying what is lost.
- Never add: anything under `.config` that is configuration, mail or chat history, game saves,
  password stores, `~/.local/share/Trash` (the trash has its own source), `recently-used.xbel`,
  or anything owned by a browser or Electron app.

**Verify**: `cargo test -p kuizer catalog:: -- --test-threads=2` passes.

## Test plan

No new tests: `embedded_catalog_is_valid` validates the file and name uniqueness. If plan 002 has
already merged into your base, also run `cargo run -p kuizer -- cache --group system` and
`cargo run -p kuizer -- cache --group apps` and paste the output in your report.

## Done criteria

- [ ] `cargo test -p kuizer catalog:: -- --test-threads=2` passes
- [ ] Every entry with `safety = "review"` has a non-empty `note`: `grep -c 'safety = "review"' src/catalog/data/linux.toml` is at most the number of notes in review entries (check by reading)
- [ ] `grep -n "Trash\|\.ssh\|keyrings\|google-chrome\|mozilla\|discord" src/catalog/data/linux.toml` prints nothing
- [ ] `git diff --name-only feat/general-cleanup` prints only `src/catalog/data/linux.toml`
- [ ] Your report lists a source URL per entry and every entry you left out

## STOP conditions

- A seed entry's documented location differs between distributions in a way one path cannot cover.
  Report it; do not add several guessed paths.
- Validation needs a schema change (for example a glob inside a file name). Do not change `src/catalog/mod.rs`.

## Maintenance notes

- `processes` feeds plan 010's running-app check: an entry whose owner runs is shown as `review`.
  Use the exact executable name from `/proc/<pid>/comm` (at most 15 characters).
- Reviewers: check every `safe` entry against its source URL in the executor's report.
