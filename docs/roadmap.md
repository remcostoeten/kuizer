# kuizer roadmap

Written on 2026-11-22 against `feat/gui-polish` at `f571856` plus the uncommitted GUI work on that branch. This is the working list of everything still to build, fix, refactor and decide: product, safety, code health, features and distribution. It replaces nothing; it points into the documents that already hold the details.

| Document | Holds |
| --- | --- |
| [`design.md`](design.md) | Product spec, module boundaries, threat model, phase acceptance criteria |
| [`shared-core-followups.md`](shared-core-followups.md) | Items left over from the 2026-10-24 performance and architecture audit, with audit ids (P1-01 and so on) |
| [`../plans/README.md`](../plans/README.md) | Plans 001 to 019: kuizer as a general PC cleaner (catalog, trash, system, big files, shared keymap) |
| [`phases/`](phases/README.md) | Older phase plans 2 to 11 for the CLI |

Every item below has a priority and a size.

- **P0**: can delete the wrong thing or lose history. Fix before the next release.
- **P1**: blocks other work, or a rule in `AGENTS.md` is broken today.
- **P2**: makes the product clearly better.
- **P3**: worth doing when nearby.
- **S** is under a day, **M** a few days, **L** a week or more.

## 1. Where kuizer stands

kuizer 0.1.3 ships a CLI, a Ratatui TUI and a GPUI desktop app over one Rust core, on Linux and macOS (x86_64 and aarch64), through cargo-dist shell installers, npm and crates.io.

| Area | State |
| --- | --- |
| Sources | Projects (45 built-in rules), global caches, toolchains, Docker and Podman, git repos (`gc` gain, merged branches), duplicates |
| Deletion | Permanent by default, opt-in trash mode, revalidation per item, `openat2`-based tree delete on Linux, history with ids, restore from trash |
| CLI | `scan`, `clean`, `cache`, `toolchain`, `docker`, `git`, `duplicates`, `rules`, `history`, `restore`, `dashboard`, `config`, HTML report |
| TUI | Project artifacts only: list, sort, filter, selection, range select, grouping, confirm overlay |
| GUI | Every source, overview with charts and space map, palette, settings view, themes, accents, density, motion switch, saved scans, visual mode, counts before motions, delete motion |
| Config | Lenient loader with warnings, `scan`, `detect`, `rules`, `duplicates`, `delete`, `git`, `appearance`, `route_shortcuts`, `toolchains`, `docker` |
| Missing from the spec | Shared keymap, `--json-lines`, `big`, policies, `--rule`/`--project`/`--exclude` filters, filters on non-project sources, a JSON schema file |

Overall health: the walker, detector and Linux tree delete are careful and well guarded. The risk sits at the edges (confirm flows, `--idle` evidence, config fallback, branch deletion), the CLI and TUI have drifted behind `design.md`, and the GUI crate is slowly turning into a second core (`model.rs`, `snapshot.rs`, `theme::age`, cache roots, toolchain repinning). Five core files and three GUI files are over 1,000 lines.

## 2. Product and business

### 2.1 Positioning

kuizer today is a developer disk cleaner: it understands projects, so it can say why a folder is safe to remove. That is the thing no general cleaner does, and it should stay the lead message even if the general sources from plans 001 to 019 land.

Audience, in order:

1. Developers on Linux and macOS with many projects and a full disk (JS, Rust, Python, JVM, mobile).
2. Developers who also want their machine cleaned in general (browser and app caches, trash, package caches, downloads).
3. Teams that want a scripted cleanup on CI runners or shared build machines (`clean --policy NAME --yes`, JSON output).

### 2.2 Decisions to make

These change what kuizer is, or what it may delete. Nothing in sections 3 and later that depends on one of them should start before it is recorded.

| Id | Question | Recommendation |
| --- | --- | --- |
| D1 | Reposition as a general cleaner, or stay a developer cleaner that also does general cleanup? (plans/README) | Developer first. Revisit after the general sources have been used for a release or two. |
| D2 | May kuizer delete personal files that cannot be regenerated (large files, old downloads)? Gates plan 013. | Yes, behind the guards in plan 013: always `review`, never bulk-selected, typed confirmation, recheck, trash by default for that source. |
| D3 | Is system cleanup with root rights (package caches, journal, snap revisions) in scope? Gates plan 012. | Not yet. List the space and print the command to run. |
| D4 | What does Enter or `d` do in the GUI when nothing is selected? Today it permanently deletes the cursor row, and the cursor falls back to the first row of a list nobody has moved in (section 3.1). | Require a selection. Enter on an unselected row opens or toggles; deleting one row goes through the row menu or selects it first. |
| D5 | Does `delete.confirm = false` apply to the CLI? `AGENTS.md` says yes, `src/commands/removal.rs:156` ignores it. | Honor it, except for `review` items and projects with uncommitted changes. |
| D6 | Cross-device trash (`delete.cross_device = refuse | copy`). From the follow-ups. | Add the key with `refuse` as default. |
| D7 | Bulk select and "select project" include `review` items in the GUI (`SelectAll`, header checkbox) and the TUI (`P`). The spec says bulk actions select `safe` only. | Make both select `safe` only; a second press widens to everything, shown in the mode indicator. |
| D8 | Should source verbs become subcommands (`cache clean`) as `design.md` 6.2 says, or should the spec follow the `--clean` flag the CLI ships? | Follow the spec: `cache clean`, keep `--clean` as a hidden alias for one minor release. A listing must never gain a flag that deletes. |
| D9 | License and funding. kuizer is MIT and free. | Stay MIT and free. If funding comes up, GitHub Sponsors or a paid signed macOS build, never a feature gate on safety or cleanup. |

### 2.3 Distribution

Installers today: shell script, npm, crates.io, prebuilt archives. What is missing for the desktop app to feel like a desktop app:

| Item | Priority | Size | Notes |
| --- | --- | --- | --- |
| macOS `.app` bundle with icon, `Info.plist` and a `.dmg` | P1 | M | Today the GUI is a bare binary on macOS. |
| macOS code signing and notarization | P1 | M | Gatekeeper blocks unsigned downloads. Needs an Apple developer account. |
| Homebrew tap (`brew install kuizer`, cask for the GUI) | P2 | S | The kuizer repository is the tap (`Formula/`, `Casks/`). After each release `.github/workflows/publish-homebrew.yml` renders both from `packaging/homebrew` and commits them with the workflow token; no second repository or personal token. |
| AUR packages (`kuizer`, `kuizer-bin`) | P2 | S | Development happens on Arch; cheap to keep up to date. |
| Linux desktop integration: install `packaging/kuizer-gui.desktop` and icons from the installers | P2 | S | The files exist but nothing installs them. |
| Flatpak on Flathub | P3 | M | Sandbox needs `--filesystem=home` and host access for git and docker, which reviewers question. Decide after AUR and Homebrew. |
| `.deb` and `.rpm` | P3 | S | `cargo-deb` and `cargo-generate-rpm`, attached to releases. |
| In-app update notice | P3 | S | A check against the GitHub release API, off by default (`ui.update_check`), never auto-installing. |

### 2.4 Release cadence and measures

- Release from `master` when a milestone in section 13 is done, not on a calendar. Every release gets its `CHANGELOG.md` section before the tag.
- kuizer sends nothing over the network, and that stays true. Measures are what the public channels already report: crates.io and npm downloads, GitHub release downloads, stars, issues opened against `review` and false-positive detections.
- A "rule request" issue template makes the false-positive and missing-rule reports structured: ecosystem, folder name, markers, what regenerates it.

### 2.5 Site and docs

`site/` is out of scope for code work unless the task is the site, but the product needs:

- Docs that match the release: the safety page and desktop guide changed on this branch; check every page against the shipped behavior at each release.
- A page per source (caches, toolchains, Docker, git) with what kuizer removes and how.
- A "what kuizer will never delete" page built from `protect.rs` and the threat model. It is the strongest trust argument the product has.

## 3. Safety (P0)

Each item was found in a code read and confirmed against the source. Every one needs a test, since each can come back.

### 3.1 GUI

| Item | Where | Fix |
| --- | --- | --- |
| With nothing selected, Enter, `d` and Delete permanently delete the cursor row. `cursor_target` falls back to the first row, so opening Toolchains or Docker and pressing Enter removes the first item for good, `review` items included. | `crates/kuizer-gui/src/app/clean.rs:57-63`, `app/keys.rs:947-956`, `commands.rs:256` | Decision D4. At minimum, never fall back to the first row for a destructive action, and never delete a `review` item that is not selected. |
| The row menu's Delete tooltip says "after a confirmation", but it deletes at once. | `app/menu.rs:24` | Fix the copy, or add the inline confirm from `AGENTS.md`. |
| A bad `scan.exclude` pattern is dropped with `.ok()`, so caches and toolchains ignore every exclusion, and excluded items become deletable without a warning. | `model.rs:777,810` | Surface the error through the config banner and keep excluded items out. |
| Settings writes are unordered background tasks; a failed write rolls back to a value captured earlier and can undo a later successful change, or leave the file behind the screen. A wrong `delete.mode` on disk is a safety problem. | `app/settings.rs:294-336` | One write queue, latest value wins; roll back only while the failed value is still current. |

### 3.2 TUI

| Item | Where | Fix |
| --- | --- | --- |
| Holding Enter deletes. The first press opens the confirm overlay, an auto-repeat confirms it, often in the same frame because the loop drains every queued event. | `src/tui/mod.rs:105,130,260-271` | Ignore confirm keys for about 300 ms after the overlay opens, and stop draining once an overlay opens. The GUI already guards against held keys. |
| `Esc` in normal mode quits, and quitting with deletions queued marks them cancelled without asking. | `src/tui/mod.rs:116` | `Esc` backs out one level; quitting while deletions run asks first. |

### 3.3 Core

| Item | Where | Fix |
| --- | --- | --- |
| The `--idle` activity walk skips folders it cannot open or read, so a project whose recent work sits in an unreadable folder looks idle and `clean --idle` selects it. The spec says unknown activity never matches. | `src/scan/activity.rs:251,274` | Any open or read failure makes the project's activity unknown. |
| A huge `--idle` overflows `checked_sub`, the cutoff becomes `None` and the filter is skipped, so `clean --idle` selects everything. | `src/commands/scan.rs:57` | Reject the value at parse time, and treat a `None` cutoff as matching nothing. |
| When the absorbed config table still fails to deserialize, `unwrap_or_default()` returns full defaults with no warning, losing the cautious delete config the syntax-error path uses. | `src/config/lenient.rs:281` | Fall back to `DeleteConfig::cautious()` and push a warning. |
| Branch deletion checks the commit with `rev-parse`, then runs `git branch -D`. A commit in between deletes unmerged work. | `src/repos.rs:510-517` | `git update-ref -d refs/heads/<name> <commit>`, which only deletes if the ref still points there. |
| A candidate whose `lstat` failed during the scan has `identity: None`, and the delete then skips the inode check. The parent is opened without `O_NOFOLLOW`. | `src/scan/size.rs:166-176`, `src/delete/tree.rs:294,302` | Refuse identity-less candidates, or re-stat and require review. |
| `Guard::with_protected` is `pub`; the Volta path uses it to drop all protection, including the on-disk browser profile check. | `src/delete/guard.rs:135`, `src/toolchains/mod.rs:576` | `pub(crate)`, and exempt only `.volta/tools`. |
| With `panic = "abort"` and no SIGINT handling, Ctrl-C or a panic during `clean` skips the `Recorder` flush and loses up to one history batch, so trashed items cannot be restored. | `Cargo.toml`, `src/history/mod.rs:1034` | Catch Ctrl-C in removing commands, set the stop flag, flush, exit. |
| After a failed history write the queued session is dropped, so one run is split in two, and a flush can lose the pending batch. | `src/history/mod.rs:861-883` | Keep the queued entries and retry with the same session. |
| History cannot record non-UTF-8 paths: the item is deleted, the history write fails, restore cannot bring it back. (Follow-ups, "Decide first" 1.) | `src/history/mod.rs`, `src/scan/types.rs` | Byte-preserving path encoding in the history format. |
| Removal error strings are printed raw and can contain nested paths from inside `node_modules`, so a hostile name can inject terminal escapes. | `src/output/text.rs:323-327` | Escape control characters, as `display_path` does. |
| `open_below` disables `openat2` process-wide on the first EPERM, including a per-path one. | `src/delete/tree.rs` | Latch only when the `openat` fallback succeeds. (Follow-ups.) |

## 4. Shared core work (P1)

`AGENTS.md` puts logic in the core and keeps frontends thin. These are the core APIs that have to exist before the frontends can stop duplicating each other. Items 4.1 to 4.8 are specified in [`shared-core-followups.md`](shared-core-followups.md); this table only orders them and adds what the code read found since.

| # | Work | Size | Unblocks |
| --- | --- | --- | --- |
| 4.1 | **Shared keymap** (follow-ups 2, plans 009, 015, 016). `[keys]` table, stable action names, defaults in the core, a GPUI `Keystroke` adapter, pre-parsed patterns. Include the row menu accelerators (`app.rs:278-284`, `app/menu.rs:5-77`), which differ from the global keys today. | L | Every new binding. `AGENTS.md` forbids adding bindings before it exists. |
| 4.2 | **Delete executor and confirmation policy** (follow-ups 1). One `delete::run` with lanes, workers, the recorder, a stop flag and per-item results. Fold in the GUI's per-source batching (`app/clean.rs:81-128`) and the uncommitted-changes warning the GUI never shows. | L | Per-item row animation, trash mode in the GUI, parallel trash moves. |
| 4.3 | **Sort, filter and selection in core** (follow-ups 3). `SortKey` out of `kuizer::cli`, one `Query` type, shift-range `Span`. | M | TUI parity, `--rule`/`--project`/`--exclude`, build shape (4.8). |
| 4.4 | **Bounded measuring pool** (follow-ups 4) and excludes passed to `caches::find_excluding` and `toolchains::find` from every frontend. | M | Removes the GUI-only filters in `measure_caches`. |
| 4.5 | **Protection lists in `protect.rs`** with `delete.protected` in config (follow-ups 5). | M | User-defined protected paths. |
| 4.6 | **Walk and dupes** (follow-ups 6): `Flow::Skip`, stat during the walk. | M | Faster duplicate search. |
| 4.7 | **Trash and history types** (follow-ups 7): `TrashSnapshot`, `Restored`, `HistoryStats` computed on the history thread. | M | Stops the GUI reloading history after every clean. |
| 4.8 | **Build shape** (follow-ups 8): `clap` and `ratatui` optional behind a `cli` feature, drop the Windows `trash` COM option. | S | Smaller GUI binary and build. |
| 4.9 | **Saved scans in the core.** Move `crates/kuizer-gui/src/snapshot.rs` (about 820 lines of non-test code) behind a small `src/` API so the TUI can open on the last scan too. | M | Instant TUI start. |
| 4.10 | **Move GUI rules into the core.** `model.rs:908-916` `cache_roots` copies `src/commands/cache.rs:87-95`; `theme.rs:530-543` `age` disagrees with `src/units.rs:134`, so the GUI and TUI show different ages; `model.rs:836-853` `repin` recomputes toolchain safety; `model.rs:891-893` hardcodes which sources are always permanent. | S each | One answer per question across frontends. |
| 4.11 | **One process runner** for git, docker, podman and toolchain uninstallers, with `core.hooksPath=/dev/null`, `LC_ALL=C`, `GIT_TERMINAL_PROMPT=0` and a timeout (follow-ups, `src/docker.rs:116-181`). | M | Consistent errors, no hanging git. |
| 4.12 | **One atomic write** (`platform::write_atomic`) replacing four: `history/mod.rs:716`, `config/edit.rs:154` (no fsync, loses permissions), GUI `persist.rs:56` and `snapshot.rs:283`. | S | |
| 4.13 | **Streaming scan pipeline in the GUI** (follow-ups, GUI section): `collect_streaming`, `Update::Cached` off the event consumer, sort once at the end. | M | Smoother first seconds of a big scan. |

## 5. Refactoring and code health

### 5.1 Split oversized files (P2)

| File | Lines | Split into |
| --- | --- | --- |
| `crates/kuizer-gui/src/app/keys.rs` | 1,959 | Move about 800 lines of tests to `app/keys/tests.rs`; split `blocked_with` (554-674) and `run` (710-841) per command group. |
| `crates/kuizer-gui/src/app.rs` | 1,558 | Split the 64-field `Kuizer` struct into `ScanSession`, `StartForm`, `ListView`, `Overlays`, `Chrome` (follow-ups P4-11). |
| `src/history/mod.rs` | 1,499 | `ids`, `store` (lock, stamp, store), `recorder`, `trash_match`. |
| `crates/kuizer-gui/src/model.rs` | 1,328 | Shrinks by itself as 4.2, 4.3 and 4.10 move logic to the core. |
| `src/repos.rs` | 1,272 | `refs` and `pack` parsing (747-1025), runner (4.11), actions. |
| `src/tui/app.rs` | 1,202 | State, selection, filtering (moves to 4.3), deletion. |
| `crates/kuizer-gui/src/snapshot.rs` | 1,164 | Moves to the core (4.9). |
| `src/dupes/mod.rs` | 1,058 | Walk, grouping, hashing, verify. |
| `src/docker.rs` | 1,050 | Runner out (4.11), then engine and objects. |
| `src/cli.rs` | 1,004 | About 350 lines of help text to `cli/help.rs`. |

### 5.2 Duplication (P2)

- `app.rs:804-845` `select_safe` has four near-identical branches; one generic version over the listed slots.
- `app/lists.rs:283-330, 415-464, 555-617`: caches, toolchains and Docker repeat the page skeleton (toolbar, tally, empty state, table). One `section_page(tab, header, empty_text)`.
- Per-source row renderers that genuinely match (follow-ups P4-11).
- `app/settings.rs:20-22`: label arrays kept in parallel with `Accent::ALL` and the other enums; use label methods on the core enums.
- `Metadata` helpers into `Stat::from_metadata` (follow-ups).

### 5.3 API surface (P2)

- `src/lib.rs` exports `cli`, `commands`, `tui` and `output`; the GUI imports CLI types through it. Make frontend-only items `pub(crate)` once 4.3 moves `SortKey`.
- `delete::Lanes`, `delete::workers` and `Target`'s fields are `pub`; narrow them when 4.2 lands.
- `docker::remove` keeps an unused `_guard` parameter and has no engine allowlist (follow-ups).

### 5.4 Theme and motion tokens (P2)

- Durations live in `flow.rs:10`, `tooltip.rs:19-21`, `icon.rs:14`, `chart.rs:13-14` and `app/map.rs:12`. Move them into `motion.rs` so the 150 ms rule is checked in one place. `ZOOM` in `app/map.rs:12` is 180 ms, over the limit.
- `icon.rs:14,257`: sidebar icons wiggle on hover. `AGENTS.md` allows motion for state changes only; remove it or tie it to a state change.
- Magic numbers in views: `app/map.rs:459` (`0.35 + 0.65 * progress`), `app/map.rs:552` (`opacity(0.8)`, should be `theme::DIM`), `app/lists.rs:279` (summary bar height guessed; add `size::SUMMARY`), minimum bar sliver widths that differ per view (`chart.rs:64`, `ui.rs:203`, `app/overview.rs:321`, `app/history.rs:40`).
- `app/duplicates.rs:285-286`: hand-written caps headers instead of `head()` with a `Term`.

### 5.5 Error handling (P2)

- `.expect` on thread spawn aborts the process: `src/tui/jobs.rs:73,115,154`, `src/repos.rs:218`, `src/scan/mod.rs:386`, GUI `persist.rs:42` (on the UI thread). Return `io::Result` or degrade.
- `src/repos.rs:299-306,313`: failed repository inspections vanish; collect them as issues, and use `PoisonError::into_inner` like the rest of the code.
- `crates/kuizer-gui/src/snapshot.rs:24,301`: `prune` counts sidecar `.dupes.json` files toward its limits and can delete a scan's sidecar or main file while keeping its partner; crashed `.tmp` files are never removed.
- `crates/kuizer-gui/src/start.rs:160-166`: recent folders silently drop non-UTF-8 paths.
- `crates/kuizer-gui/src/reveal.rs:121-126`: `.status()` waits on `xdg-open`, which can hang a background thread; `spawn()` without waiting.

## 6. Performance

Claim a speedup only with a measurement (`cargo run --release --example synth`).

### 6.1 GUI input path (P1)

`AGENTS.md`: a keypress shows a response in the same frame, and navigation costs the same with 1,000 or 1,000,000 rows.

| Item | Where |
| --- | --- |
| `s` (stop scan) runs `drop_paths` over the whole report, then rebuilds stats, rows and the map inside the key handler. | `app/launch.rs:437-456` |
| `show_duplicates` runs `drop_paths` over every list and re-sorts groups on the UI thread. | `app/duplicates.rs:115-128` |
| `Update::Cached` runs `drop_paths` over the whole saved report on the foreground thread. | `app/session.rs:85-86` |
| Outside Artifacts, `target_exists` and `listed_row` are linear `PathBuf` scans, run several times per `j`/`k`. Keep a row hint per tab, as `artifact_row` does. | `app/keys.rs:855-866, 894-905` |
| The palette rebuilds and re-ranks every entry and recomputes `blocked_with` on every key, arrows and Esc included. | `app/keys.rs:439-441` |
| `start_scan` canonicalizes roots on the UI thread. | `app/launch.rs:414` |
| The space map rebuilds even when hidden; `Node::leaves` allocates. | follow-ups, `treemap.rs:80-95` |
| Duplicates, History and Issues are not virtualized. | follow-ups |

### 6.2 Core (P2)

- `src/dupes/mod.rs:521`: `full_hash` allocates up to 1 MiB per file, `same_bytes` four 64 KiB buffers per compare. Reuse per-thread scratch buffers.
- `src/history/mod.rs:716`: every flush re-serializes and fsyncs the whole history, which grows without limit. Cap, compact, or switch to append-only lines.
- `src/scan/activity.rs:113`: a monorepo root's activity ignores nested projects, so `--idle` undercounts edits there (also a correctness bug).
- `trash::delete` re-reads `/proc/mounts` under a global lock on every call, which serializes parallel trash moves (follow-ups 1).
- JSON and HTML reports are not written through a `BufWriter` (follow-ups; measure peak RSS first).
- TUI input on its own thread or zero-timeout polling (follow-ups P3-12).

### 6.3 Benchmarks in CI (P2, M)

CI now benchmarks `kuizer --json scan` with `hyperfine` on a fixed 200-project, 100-dependency synthetic tree. It compares the median against `benchmarks/scan-baseline.json` and fails above a 15% regression. Add time to first candidate and peak RSS after this wall-time measurement is stable.

## 7. GUI

### 7.1 Keyboard and commands (P1)

`AGENTS.md`: every action works without a mouse, vim motions work everywhere, the palette lists every action.

- Restore, Always exclude and Copy command exist only in the row menu (`app/menu.rs:5-77`). Add them as `Command`s so the palette shows them.
- The space map takes `h` and `l` but not `j` and `k` (`commands.rs:283-293`).
- The shortcuts overlay scrolls with the mouse only; at the 860x520 minimum window its lower groups cannot be reached by keyboard (`app/keys.rs:228-236`, `app/overlay.rs:196`).
- All of these wait on the shared keymap (4.1) for any new binding.

### 7.2 States and feedback (P2)

- The invalid config banner shows only the first warning and a count (`main.rs:91-97`); let it expand to list every bad key with its fallback.
- Per-item delete results streamed in, so rows leave one by one (needs 4.2).

### 7.3 Settings (P1)

- Done: the Keys page lists every action with its keys and defaults, filters with `/`, records a new key with Enter, adds, removes and resets keys, and refuses invalid or conflicting keys inline through `keymap::rebind`. The change is written to `[keys]` and applies to the GUI at once and to the TUI on its next start.
- Done: `delete.protected` and `delete.cross_device` are on the Deleting page.

### 7.4 New GUI features (P2 and P3)

- **Exclude from the list**: `x` on a row writes to `scan.exclude` through `config::add_exclude`, with an inline confirm (also in the TUI spec).
- **Compare with the last scan**: saved scans already exist; show what grew since the previous scan of the same folder, per project.
- **Project view**: group every artifact, cache link and repo under its project, with one "clean this project" action.
- **Selection presets**: save the current filter and selection as a policy (section 9.3), so a GUI session can become a scripted `clean --policy`.
- **Drag a folder onto the window** to scan it.
- **Notifications** when a long delete finishes while the window is in the background.

## 8. TUI

The TUI only lists project artifacts. The CLI and TUI "are the foundation and must keep working", but they also fall behind every GUI release.

| Item | Priority | Size |
| --- | --- | --- |
| Safety fixes from 3.2 | P0 | S |
| Shared keymap (plan 016) | P1 | M |
| Views `1` to `4` from the spec: artifacts, caches, duplicates, issues. Then toolchains, Docker and git. | P2 | L |
| Key drift from `design.md` section 7: `x` resets filters instead of excluding, `p` toggles grouping instead of pausing, `c` cycles the category filter instead of selecting it; `A`, `D`, `l` and pause are missing. Either the spec or the code changes, per key. | P1 | M |
| History and restore view | P2 | M |
| Command palette and Tab focus cycling | P3 | M |
| Open on the last scan (needs 4.9) | P3 | S |
| Same palette roles as the GUI (`appearance.theme`, `appearance.accent` in the TUI) | P3 | S |

## 9. CLI

### 9.1 Spec drift (P1)

Everything `design.md` promises that the CLI does not do. Either build it or take it out of the spec; leaving both is how drift grows.

- `cache clean`, `toolchain clean`, `docker clean` subcommands (D8).
- `--idle` on `cache`, `toolchain` and `docker`, each with its own activity signal (`design.md` 6.3).
- `--permanent` (removed in 0.1.3; the spec still lists it).
- `big` (plan 008).
- `policy list | show` and `clean --policy` (phase 11).
- `--category` and `--min-size` replace config values instead of intersecting with them (`src/commands/scan.rs:80-86`, `clean.rs:13-18`), so a flag can widen a narrow config.
- `history` and `restore` skip loading config, so `--config` is ignored for them (`src/commands/mod.rs:136`).

### 9.2 Missing basics (P2)

- History in XDG state (`~/.local/state/kuizer/history.json`, `~/Library/Application Support` on macOS) instead of `~/.kuizer-history.json` with a `.lock` beside it in `$HOME`; migrate like the sweep-era file.
- `config check --fix` or `config migrate` to rewrite deprecated keys.
- `history prune` and a size cap.

### 9.3 Policies (P2, M)

Phase 11 is specified in `design.md` 5 and 6.5 and in [`phases/11-policies.md`](phases/11-policies.md). It is the feature that turns kuizer into something teams run on CI machines. Policies also give the GUI a way to save a selection (7.4).

## 10. Configuration

- `docs/config.example.toml` misses `scan.max_depth` and `scan.excluded_fs_types` (P1, S). A test that compares the example file's keys with the config schema keeps it from drifting again.
- `design.md` section 5 misses `duplicates.enabled`, `git.*`, `appearance.sidebar`, `appearance.theme`, `appearance.density`, `route_shortcuts.*`, `toolchains.*` and `docker.*`, and still says unknown keys are errors, which the lenient loader no longer does.
- `AGENTS.md` says `ui.motion`; config and `design.md` use `appearance.motion`. Settle on `appearance.motion` (follow-ups).
- New keys this roadmap adds: `[keys]`, `delete.protected`, `delete.cross_device`, `appearance.zoom`, `ui.update_check`, plus whatever plans 001 to 019 add (`catalog`, `system`, `big`).
- `scan::size_threads` clamps to 64 with no way to tell the user (follow-ups P1-10); report it as a config warning.

## 11. Testing

`AGENTS.md`: test what can break in our code, nothing else.

### 11.1 Tests to add (P0 and P1)

- Every item in section 3 gets a regression test: GUI Enter with an empty selection, TUI held Enter, unreadable folder under `--idle`, huge `--idle`, lenient fallback keeping the cautious delete config, branch moving between check and delete, Volta removal still refusing a browser profile, recorder retry without losing a batch, identity-less candidates refused.
- `select_wins` ("Biggest safe wins") never picks a `review` item.
- A bad exclude pattern keeps excluded caches out.
- Settings write ordering and rollback.
- `snapshot::prune` with sidecars present.
- One table-driven test over `builtin_rules()` with a positive and a negative case per rule (phase 6 asks for it; 45 rules have about 9 detector tests today).
- The example config lists every schema key (section 10).

### 11.2 Tests to remove or merge (P3)

Candidates that test a dependency or restate the implementation: the serde parse tests in `src/config/mod.rs:552-614`, the clap `parses_*` tests in `src/cli.rs:904-969` (keep `cli_is_consistent` and `help_groups_list_every_command`), `icon.rs` motion shape tests, `frame.rs` geometry tests, `theme.rs` `share_is_clamped_and_safe_for_zero`, `glossary.rs` `every_term_has_a_title_and_an_explanation`, `model.rs` `cache_roots_are_the_unique_parents`, the zoom test in `app/tests.rs` that overlaps `zoom.rs`, and most of the positioning tests in `tooltip.rs`. Read each before deleting; keep any that guards a bug that happened.

### 11.3 Test infrastructure (P2)

- The six `isolated(...)` tests in `app/tests.rs` each re-run the test binary as a subprocess; one harness for all deletion cases is cheaper on an 8 GB machine.
- `src/delete/mod.rs:259` depends on 150 ms and 50 ms timeouts and can flake under load; use a barrier.
- Fuzzing for config parsing and hostile names (phase 7), with `cargo fuzz` outside the workspace so it adds no dependency.

## 12. Build, dependencies and CI

| Item | Priority | Size |
| --- | --- | --- |
| Build shape (4.8): `clap` and `ratatui` behind a `cli` feature, GUI with `default-features = false`. | P2 | S |
| `trash` pulls in `chrono` while the core uses `jiff`. Drop one; parsing the few dates kuizer needs by hand may be enough. | P3 | S |
| Release binary sizes today: `kuizer` 4.9 MB, `kuizer-gui` 19.2 MB. Record them per release in `CHANGELOG.md` so growth is visible. | P3 | S |
| The first CI job runs `cargo clippy --all-targets` and `cargo test` without `--workspace`; make it match the `AGENTS.md` commands. | P2 | S |
| Benchmarks in CI (6.3). | P2 | M |
| An X11 smoke run of the GUI in CI (Xvfb), since development happens on Wayland. | P3 | M |
| A CI check that fails when `docs/config.example.toml` and the schema disagree (also 11.1). | P2 | S |

## 13. Future features

Grouped by how far out they are. Each needs its decision (section 2.2) and a plan before it starts.

### 13.1 Next (planned in `plans/`)

The general-cleaner set, plans 001 to 019, waiting on D1 to D3. Nothing in it has started.

- A data-driven location catalog for app, browser and system caches (001, 002, 004, 005, 011), with cache folders inside browser and Electron profiles on an allowlist (003).
- `kuizer trash` to list and empty the trash (006).
- `kuizer system` to list package caches, snaps and the journal (007), and maybe remove them with escalation (012, D3).
- `kuizer big` for large files and old downloads (008), and maybe removal to the trash (013, D2).
- Caches of running apps become `review` (010).
- GUI sections and removal for all of it (014, 017, 018), and docs and positioning (019).

### 13.2 Later

- **`duplicates link`**: replace copies with hard links or reflinks, deleting no content (`design.md` 6.2). Reflinks on Btrfs and APFS are the safe version.
- **More rules**: Bazel, Nx and Turborepo caches, Deno, Bun, Nix result links, Android `build` and AVD images, iOS simulators and DerivedData (macOS catalog), Jupyter checkpoints, Hugging Face and Ollama model caches (always `review`), Terraform providers per workspace, `.venv` with a stale interpreter.
- **Per-project rules** in a `.kuizer.toml` at a project root: extra artifacts, never-delete paths, a project-level idle override.
- **Rule packs**: rules shared as TOML files, loaded from `~/.config/kuizer/rules.d/`.
- **`kuizer explain PATH`**: why a path is or is not a candidate, which rule, which markers, which guard would refuse it. Useful for bug reports and trust.
- **Disk pressure hint**: on start, show free space per mount and how much the scan would bring back.
- **Watch mode**: rescan a folder on a timer and show what grew, without deleting. A foreground process, not a daemon.

### 13.3 Not planned

From `design.md` and `plans/README.md`, kept here so nobody re-proposes them without a reason:

- A daemon or scheduled cleanup inside kuizer; cron or a systemd timer runs `clean --policy NAME --yes`.
- Windows. The project rules exclude it.
- Cloud storage, accounts, telemetry or anything that sends data off the machine.
- Trash folders on other mounts (`/run/media/...`): the guard refuses `/run` and widening it is its own safety decision.
- Coredumps, a separate logs source, BleachBit CleanerML files (GPL-3).
- OS reduced-motion detection; GPUI 0.2.2 cannot read it, `appearance.motion` is the only switch.

## 14. Documentation

- `design.md` status line (line 3) still says category selection and other shipped work is open. Update it at each release.
- `design.md` section 2 describes one discovery thread and a bounded(256) channel; the code runs one work-stealing pool (`scan/pool.rs`).
- `design.md` sections 5, 6 and 7 per sections 8, 9 and 10 above.
- `shared-core-followups.md`: the invalid config banner and the cancellable duplicate search are done (`main.rs:91-97`, `app.rs:116-127`, `app/duplicates.rs:37`); mark them.
- `plans/README.md`: record D1 to D3 and update the status column as plans land.
- `README.md`: once the TUI has more views or the general sources land, the feature list changes with them.

## 15. Milestones

A suggested order. Each milestone is one release. Releases bump only the patch version (0.1.x) until a minor bump is decided.

### 0.1.4: safe and honest

Everything P0 in section 3 with its tests, decisions D4, D5 and D7, the config example and spec fixes from section 10, the `shared-core-followups.md` bookkeeping, and the CI workspace fix. No new features.

### 0.1.5: one core

Shared keymap (4.1, plans 009, 015, 016), delete executor and confirmation policy (4.2), sort and filter in core (4.3), GUI logic moved into the core (4.9, 4.10), process runner and atomic write (4.11, 4.12), GUI input path fixes (6.1), the file splits that those changes touch anyway. TUI key drift resolved.

### 0.1.6: the spec, delivered

`--json-lines`, the missing filters, `cache clean` subcommands (D8), one JSON `candidates` shape with a schema file, policies, completions and a man page, history in XDG state, TUI views for caches, duplicates and issues, the missing GUI settings sections, benchmarks in CI. macOS `.app` bundle with signing, Homebrew and AUR.

### 0.1.7 and on: general cleanup

Plans 001 to 019 in their waves, after D1 to D3, then `duplicates link`, per-project rules and `kuizer explain`.

### Later: 1.0

When the minor bump is decided. The design's phase acceptance criteria all met, the JSON schema frozen at version 1 or 2, no P0 or P1 item open, and the docs at kuizer.app matching the shipped behavior.

## 16. Issues

Every item above is tracked on GitHub, grouped by milestone. Labels carry the priority (`P0` to `P3`) and the area.

### 0.1.4

| Issue | Priority | Title |
| --- | --- | --- |
| [#41](https://github.com/remcostoeten/kuizer/issues/41) | P0 | Preserve history entries for non-UTF-8 deleted paths |
| [#58](https://github.com/remcostoeten/kuizer/issues/58) | P0 | GUI: Enter, d or Delete with no selection permanently deletes the cursor row |
| [#60](https://github.com/remcostoeten/kuizer/issues/60) | P0 | GUI: an invalid scan.exclude pattern silently disables exclusions for caches and toolchains |
| [#61](https://github.com/remcostoeten/kuizer/issues/61) | P0 | GUI: settings writes are unordered and a failed write can roll back a later change |
| [#62](https://github.com/remcostoeten/kuizer/issues/62) | P0 | TUI: holding Enter confirms a deletion |
| [#64](https://github.com/remcostoeten/kuizer/issues/64) | P0 | --idle treats unreadable folders as no activity, so clean --idle can select active projects |
| [#65](https://github.com/remcostoeten/kuizer/issues/65) | P0 | A huge --idle value skips the idle filter and selects everything |
| [#66](https://github.com/remcostoeten/kuizer/issues/66) | P0 | Lenient config loader falls back to full defaults instead of the cautious delete config |
| [#67](https://github.com/remcostoeten/kuizer/issues/67) | P0 | Branch deletion can delete unmerged commits made between the check and the delete |
| [#68](https://github.com/remcostoeten/kuizer/issues/68) | P0 | Delete skips the inode check for candidates whose lstat failed during the scan |
| [#69](https://github.com/remcostoeten/kuizer/issues/69) | P0 | Guard::with_protected is public and the Volta path drops all protection |
| [#70](https://github.com/remcostoeten/kuizer/issues/70) | P0 | Ctrl-C or a panic during clean loses the pending history batch |
| [#71](https://github.com/remcostoeten/kuizer/issues/71) | P0 | A failed history write splits a session and can drop queued entries |
| [#59](https://github.com/remcostoeten/kuizer/issues/59) | P1 | GUI: row menu Delete tooltip promises a confirmation that never comes |
| [#63](https://github.com/remcostoeten/kuizer/issues/63) | P1 | TUI: Esc quits and cancels queued deletions without asking |
| [#72](https://github.com/remcostoeten/kuizer/issues/72) | P1 | Removal error messages can inject terminal escape sequences |
| [#74](https://github.com/remcostoeten/kuizer/issues/74) | P1 | Decide: does delete.confirm = false apply to the CLI? |
| [#75](https://github.com/remcostoeten/kuizer/issues/75) | P1 | Bulk select and select-project include review items in the GUI and the TUI |
| [#76](https://github.com/remcostoeten/kuizer/issues/76) | P1 | Decide: source verbs as subcommands (cache clean) or the --clean flag |
| [#114](https://github.com/remcostoeten/kuizer/issues/114) | P1 | CLI flags replace config values instead of narrowing them; history ignores --config |
| [#118](https://github.com/remcostoeten/kuizer/issues/118) | P1 | config.example.toml misses keys; add a test that keeps it complete |
| [#119](https://github.com/remcostoeten/kuizer/issues/119) | P1 | Bring design.md, AGENTS.md and the follow-ups in line with the code |
| [#123](https://github.com/remcostoeten/kuizer/issues/123) | P2 | CI: run clippy and tests for the whole workspace in the first job |

### 0.1.5

| Issue | Priority | Title |
| --- | --- | --- |
| [#43](https://github.com/remcostoeten/kuizer/issues/43) | P1 | Add a shared delete executor for the CLI, TUI, and GUI |
| [#45](https://github.com/remcostoeten/kuizer/issues/45) | P1 | Build the shared configurable keymap |
| [#46](https://github.com/remcostoeten/kuizer/issues/46) | P1 | Add a shared filter and sort module |
| [#78](https://github.com/remcostoeten/kuizer/issues/78) | P1 | Bounded measuring pool and excludes for caches and toolchains |
| [#79](https://github.com/remcostoeten/kuizer/issues/79) | P1 | Move protection lists into protect.rs and add delete.protected |
| [#83](https://github.com/remcostoeten/kuizer/issues/83) | P1 | Move GUI-side rules into the core: cache roots, age, toolchain repin, permanent sources |
| [#93](https://github.com/remcostoeten/kuizer/issues/93) | P1 | GUI: keep work off the input path (stop scan, duplicates, cursor moves, palette) |
| [#98](https://github.com/remcostoeten/kuizer/issues/98) | P1 | GUI: list Restore, Always exclude and Copy command in the palette |
| [#99](https://github.com/remcostoeten/kuizer/issues/99) | P1 | GUI: vim motions in the space map, shortcuts overlay and dialogs |
| [#103](https://github.com/remcostoeten/kuizer/issues/103) | P1 | Add appearance.zoom; interface zoom is a GUI-only setting today |
| [#108](https://github.com/remcostoeten/kuizer/issues/108) | P1 | TUI: resolve key drift from design.md section 7 |
| [#120](https://github.com/remcostoeten/kuizer/issues/120) | P1 | One table-driven test per built-in rule |
| [#44](https://github.com/remcostoeten/kuizer/issues/44) | P2 | Add the delete.cross_device configuration setting |
| [#49](https://github.com/remcostoeten/kuizer/issues/49) | P2 | Feature-gate CLI and TUI code out of the GUI build |
| [#73](https://github.com/remcostoeten/kuizer/issues/73) | P2 | openat2 is disabled process-wide on the first per-path EPERM |
| [#80](https://github.com/remcostoeten/kuizer/issues/80) | P2 | Faster duplicate search: prune during the walk, stat once, reuse buffers |
| [#81](https://github.com/remcostoeten/kuizer/issues/81) | P2 | Shared trash and history types; compute history stats off the UI thread |
| [#82](https://github.com/remcostoeten/kuizer/issues/82) | P2 | Move saved scans from the GUI into the core |
| [#84](https://github.com/remcostoeten/kuizer/issues/84) | P2 | One process runner for git, docker, podman and toolchain uninstallers |
| [#85](https://github.com/remcostoeten/kuizer/issues/85) | P2 | One atomic file write instead of four |
| [#86](https://github.com/remcostoeten/kuizer/issues/86) | P2 | GUI: stream scan results without rebuilding rows on the foreground thread |
| [#87](https://github.com/remcostoeten/kuizer/issues/87) | P2 | Split oversized core files |
| [#88](https://github.com/remcostoeten/kuizer/issues/88) | P2 | Split oversized GUI files and remove duplicated page code |
| [#89](https://github.com/remcostoeten/kuizer/issues/89) | P2 | Narrow the public API of the kuizer crate |
| [#90](https://github.com/remcostoeten/kuizer/issues/90) | P2 | GUI: collect motion and theme values in tokens; keep motion under 150 ms |
| [#91](https://github.com/remcostoeten/kuizer/issues/91) | P2 | Replace panicking thread spawns and silently dropped errors |
| [#92](https://github.com/remcostoeten/kuizer/issues/92) | P2 | GUI: saved-scan prune counts sidecar files and never removes crashed .tmp files |
| [#94](https://github.com/remcostoeten/kuizer/issues/94) | P2 | GUI: virtualize Duplicates, History and Issues; rebuild the space map only when visible |
| [#100](https://github.com/remcostoeten/kuizer/issues/100) | P2 | GUI: distinct empty states and a recoverable failed state (done) |
| [#101](https://github.com/remcostoeten/kuizer/issues/101) | P2 | GUI: branch deletion confirm is a modal that pops in (done: inline footer confirm, focus on Cancel) |
| [#102](https://github.com/remcostoeten/kuizer/issues/102) | P2 | GUI: invalid config banner lists every bad key |

### 0.1.6

| Issue | Priority | Title |
| --- | --- | --- |
| [#126](https://github.com/remcostoeten/kuizer/issues/126) | P1 | macOS: .app bundle, dmg, code signing and notarization |
| [#95](https://github.com/remcostoeten/kuizer/issues/95) | P2 | History storage: move to XDG state, stop rewriting the whole file, add a cap |
| [#96](https://github.com/remcostoeten/kuizer/issues/96) | P2 | --idle ignores activity in nested projects of a monorepo |
| [#97](https://github.com/remcostoeten/kuizer/issues/97) | P2 | Benchmarks in CI with a regression threshold |
| [#104](https://github.com/remcostoeten/kuizer/issues/104) | P2 | GUI settings view: sections for the remaining config keys |
| [#105](https://github.com/remcostoeten/kuizer/issues/105) | P2 | Exclude a row from the list with x in the GUI and the TUI |
| [#109](https://github.com/remcostoeten/kuizer/issues/109) | P2 | TUI: views for caches, duplicates and issues, then toolchains, Docker and git |
| [#110](https://github.com/remcostoeten/kuizer/issues/110) | P2 | TUI: history and restore view |
| [#111](https://github.com/remcostoeten/kuizer/issues/111) | P2 | Add --json-lines |
| [#112](https://github.com/remcostoeten/kuizer/issues/112) | P2 | Add --rule, --project and --exclude filters, and filters on cache, toolchain and docker |
| [#113](https://github.com/remcostoeten/kuizer/issues/113) | P2 | One JSON candidates shape across sources, and a JSON schema file |
| [#115](https://github.com/remcostoeten/kuizer/issues/115) | P2 | Policies: policy list, policy show and clean --policy |
| [#116](https://github.com/remcostoeten/kuizer/issues/116) | P2 | Shell completions and a man page |
| [#127](https://github.com/remcostoeten/kuizer/issues/127) | P2 | Homebrew tap with a formula and a cask |
| [#128](https://github.com/remcostoeten/kuizer/issues/128) | P2 | AUR packages: kuizer and kuizer-bin |
| [#129](https://github.com/remcostoeten/kuizer/issues/129) | P2 | Install the desktop file and icons on Linux |

### 0.1.7

| Issue | Priority | Title |
| --- | --- | --- |
| [#77](https://github.com/remcostoeten/kuizer/issues/77) | P2 | Tracking: general cleaner (plans 001 to 019) and decisions D1 to D3 |

### Later

| Issue | Priority | Title |
| --- | --- | --- |
| [#106](https://github.com/remcostoeten/kuizer/issues/106) | P3 | GUI: show what grew since the last scan of a folder |
| [#107](https://github.com/remcostoeten/kuizer/issues/107) | P3 | GUI: project view with one clean-this-project action |
| [#117](https://github.com/remcostoeten/kuizer/issues/117) | P3 | config check --fix to rewrite deprecated and invalid keys |
| [#121](https://github.com/remcostoeten/kuizer/issues/121) | P3 | Remove tests that test dependencies; fix the timing-based lanes test |
| [#122](https://github.com/remcostoeten/kuizer/issues/122) | P3 | Fuzz config parsing and hostile file names |
| [#124](https://github.com/remcostoeten/kuizer/issues/124) | P3 | Drop either chrono or jiff |
| [#125](https://github.com/remcostoeten/kuizer/issues/125) | P3 | CI: X11 smoke run of the GUI |
| [#130](https://github.com/remcostoeten/kuizer/issues/130) | P3 | Flatpak, .deb and .rpm packages |
| [#131](https://github.com/remcostoeten/kuizer/issues/131) | P3 | Opt-in update notice |
| [#132](https://github.com/remcostoeten/kuizer/issues/132) | P3 | Issue template for rule requests and false positives |
| [#133](https://github.com/remcostoeten/kuizer/issues/133) | P3 | duplicates link: replace copies with hard links or reflinks |
| [#134](https://github.com/remcostoeten/kuizer/issues/134) | P3 | More detection rules |
| [#135](https://github.com/remcostoeten/kuizer/issues/135) | P3 | Per-project rules in .kuizer.toml |
| [#136](https://github.com/remcostoeten/kuizer/issues/136) | P3 | Rule packs loaded from rules.d |
| [#137](https://github.com/remcostoeten/kuizer/issues/137) | P3 | kuizer explain PATH |
| [#138](https://github.com/remcostoeten/kuizer/issues/138) | P3 | Free space per mount and a watch mode |

