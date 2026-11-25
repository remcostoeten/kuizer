# Shared core follow-ups

Left over from the 2026-10-24 performance and architecture audit (released in 0.1.2). Each audit partition could only edit its own files, so everything that needs a new core API used by more than one frontend was skipped. Finding ids (P1-01 and so on) refer to the audit reports.

## Decide first

These change deletion behavior, so they need a product call before anyone implements them.

1. **History cannot record non-UTF-8 paths.** The item is deleted but the history write fails, so `restore` cannot bring it back, and a flush drops the whole pending batch (up to 64 entries). Needs a byte-preserving path encoding in the history format. Files: `src/history/mod.rs`, `src/scan/types.rs`.
2. **Cross-device trash.** Add `delete.cross_device = refuse | copy` (default `refuse`) to the config, `docs/config.example.toml` and the GUI settings view, then enforce it in `src/delete` (P2-02).
3. **GUI bulk select includes review items.** `Command::SelectAll` and the header checkbox select review items, which contradicts `docs/design.md` section 1 ("bulk actions select safe only"). Files: `crates/kuizer-gui/src/app/keys.rs`, `app.rs`.

## Shared core APIs to build

Build each in `src/` with tests, then switch all three frontends over.

### 1. Delete executor and confirmation policy (P2-05, P3-08, P3-09, P4-06, P5-03, P5-04)
- One `delete::run` executor with lanes, worker count, the history `Recorder`, a stop flag and a per-item callback. Replaces `commands/removal.rs:180-207`, `tui/jobs.rs:29-74` and the GUI `run_batches` in `model.rs:930-979`, which deletes one item at a time.
- One confirmation policy (`delete.confirm`, permanent mode, review items, uncommitted changes). The GUI confirm dialog never warns about uncommitted changes. Merge the CLI rule (`removal.rs:80-106`), the TUI rule (`tui/app.rs:597-603`) and `Confirmation::for_items` (`model.rs:847-879`).
- GUI follow-ups once it exists: pass `config.delete.mode` through `model::clean` and `accept()`, send the outcome before `HistoryView::load()`, stream per-item results so rows animate out one by one.
- Batch trash moves: `trash::delete` re-reads `/proc/mounts` under a global lock on every call, which serializes parallel workers.
- `git::StatusCache` with in-flight dedupe, used by the confirm policy.

### 2. Shared keymap (P3-04, P3-13, P4-12, P5-09)
- `[keys]` table in config, stable action names, defaults in the core, documented in `docs/config.example.toml` and `docs/design.md` section 7.
- GUI `commands.rs` `build_table` generates its specs from the core keymap through a GPUI `Keystroke` adapter. Pre-parse patterns when the table is built instead of on every keystroke (`commands::matches`, `lookup`).
- TUI input path reads the same keymap. Menu accelerators need a field on `Spec`.
- `appearance.theme` and `appearance.accent` keys, with named palette roles in core shared by `src/tui/palette.rs` and `crates/kuizer-gui/src/theme.rs`.

### 3. Sort, filter and selection in core (P3-07, P3-10, P4-13)
- Move `SortKey`, `sort_candidates`, a `Filter`/`Query` type with narrowing, and the shift-range `Span` into a core module (new file under `src/scan`). `model.rs:10` currently imports `kuizer::cli::SortKey`.
- TUI `App::matches` (`tui/app.rs:187-200`) and the GUI per-source matchers in `lists.rs` and `keys.rs` then share one matcher. The GUI `refresh_rows` and `extend` duplicate TUI logic.

### 4. Bounded measuring pool (P1-01 follow-up, P2-07, P3-05): done
- Caches and toolchains are sized through `scan::measure_candidates`, which runs `scan::measure_many` on one pool of `scan::size_threads` threads. `repos.rs` no longer measures per item; `collect_garbage` measures one git folder before and after.
- The CLI cache, toolchain and dashboard commands and the GUI call `caches::find_excluding` and `toolchains::find_excluding`; the GUI-only filters are gone.
- Also moved from the GUI into the core (#83): `caches::guard_roots`, `toolchains::apply_pins_measured`, `units::age` and `units::since` for ages, and `source::Source` for which sources can go to the trash.

### 5. Protection lists in `protect.rs` (P1-07)
- Move the `PROTECTED` prefixes (`guard.rs:7-32`), the anchors (`guard.rs:72-76`) and the mount refusal (`guard.rs:121-144`) behind `Protected`. Add `delete.protected` to config, the example file and the GUI settings view. Invalid entries are reported and skipped.
- `Protected::covers` costs about 24 stats per item. Check the target's lstat first and cache marker results for ancestors.

### 6. Walk and dupes (P1-02, P2-13 follow-up)
- `Flow::Skip` from `Visitor::entered()` so a standalone duplicate walk can prune hidden and tool folders (`~/.cache`, `~/.cargo`, `~/.rustup`).
- `dupes::Files` stats through `entry.stat()` during the walk and keeps only files at or above `min_size`, dropping the post-walk `in_parallel` stat pass. `PinFinder::file` matches `entry.name` before building a path.

### 7. Trash and history types (P2-10, P2-12, P5-07)
- Done: `Restored`, `TrashSnapshot`, `TrashState` and `TrashView` live in `src/history/state.rs`, `restore::restore_listed` restores from an existing listing, and the GUI computes `HistoryStats` in `HistoryView::new` on the history thread. A GUI restore lists the trash once and updates the history in memory.
- Left: after a clean the GUI still calls `HistoryView::load()`. It can push the recorded run into the view once the delete executor hands back the run it recorded.

### 8. Build shape (P2-06)
- Make `clap` and `ratatui` optional behind a default `cli` feature, mark the `kuizer` bin `required-features`, and set `default-features = false` in the GUI manifest. Needs `SortKey` and `sort_candidates` moved first (item 3).
- Drop the `trash` Windows COM option (`coinit_apartmentthreaded`) in `Cargo.toml`, together with the remaining non-Unix leftovers (P1-14).

## GUI work blocked on core pieces

- **Cancellable duplicate search (P4-05, #48): done.** `model::find_duplicates` takes a `&Cancel` and calls `dupes::find_cancellable`; `crates/kuizer-gui/src/app.rs` stores a `Cancel` in `Search` and cancels it on drop, and `crates/kuizer-gui/src/app/duplicates.rs` passes `&Cancel` and skips `staged.save()` when cancelled.
- **Idle-since chart (P1-08).** Run `annotate_in` on a background thread after the walk and refresh `Charts`; only then make `stats::idle` use `project_last_active`. Land both together.
- **ReportStats aggregates (P4-14, P5-10, P5-14).** Add duplicates reclaimable, repos gain and merged count to `ReportStats`, call `ReportStats::grow` from `grow_stats`, then delete `cache_total`, `toolchain_total` and `docker_total` in `model.rs:365-387`.
- **Streaming scan pipeline.**
  - `model::walk` clones every candidate into `Loaded::Found` and `load` emits them again as `Loaded::Artifacts`, which forces a full row and map rebuild on the foreground thread. Use `scan::collect_streaming` (P1-12).
  - `emit` in `scan_parts` joins the snapshot-loading thread through `shown_first()` inside the event consumer, so a large saved scan stalls the walker. Send `Update::Cached` from a separate task.
  - `Report::apply` re-sorts the whole list on every 100 ms batch. Keep arrival order while streaming and sort once at the end. Merging a sorted batch was measured and is slower than std's sort.
- **Space map.** `Kuizer::take` calls `rebuild_map()` even when the map is hidden or on another tab. Rebuild only when `map_active()`, otherwise mark dirty, and drop the old tree off the UI thread. Add a non-allocating fold for `Node::leaves` (`treemap.rs:80-95`). `ZOOM` is 180 ms in `app/map.rs:12`, over the 150 ms motion limit.
- **Foreground work.** `App::start_scan` calls `scan::normalize_roots` (a `canonicalize`) on the foreground thread. Move it into the `background_spawn` next to `check_folder`. `model::short_path` clones the home dir per row; return `Option<&'static Path>` from `platform::home_dir()`.
- **Virtualize the single-page views.** Duplicates, History and Issues rebuild only on real content changes now but are not virtualized. They need `gpui::list` with a `ListState`, or splitting the stat cards from the table. Needs a visual check. A lazy `tooltip::path` lets non-virtualized rows hold only a key.
- **Split the `Kuizer` app struct (P4-11).** Group the 64 fields into `ScanSession`, `StartForm`, `ListView`, `Overlays` and `Chrome`, and merge the per-source row renderers where they genuinely match.
- **Invalid config banner: done.** `crates/kuizer-gui/src/main.rs` wires `loaded.warnings` from the lenient loader into `fallback::notice` and `fallback::remember`, rendered at the top of the window via `fallback::wrap` in `crates/kuizer-gui/src/frame.rs`. (Expanding the banner to list every warning individually is tracked in #102.)

## Smaller items

- `delete/tree.rs` `open_below` disables `openat2` process-wide on the first EPERM, including a per-path EPERM. Latch only when the fallback `openat` succeeds. Protection after the latch equals HEAD, so this is not a regression.
- Second git runner in `src/git.rs` lacks `core.hooksPath=/dev/null`, `LC_ALL=C` and `GIT_TERMINAL_PROMPT=0`. Share one runner with `docker`, the toolchain uninstaller and `repos.rs` (P2-15). Optionally add a timeout; `uncommitted_changes` output can be large, so test it.
- Split `history/mod.rs` and `repos.rs` (both over 1,200 lines); `history/mod.rs`, `tui/app.rs` and `dupes/mod.rs` grew during the audit.
- `docker::remove` keeps its `_guard` parameter and has no engine allowlist; removing them changes a `pub` signature.
- `scan::size_threads` clamps to 64 with no reporting channel for the clamp (P1-10).
- JSON and HTML reports are not streamed through a `BufWriter` (P3-03); measure peak RSS first. The HTML path needs a custom escaping writer.
- TUI main loop input on its own thread or zero-timeout polling (P3-12); needs an interactive check of input latency.
- Consolidate `Metadata` helpers into `Stat::from_metadata` (callers in `src/delete`, `repos.rs`, `dupes`, `tui`).
- Docs (#119), done: `docs/design.md` section 2 updated to the work-stealing pool (`src/scan/pool.rs`), section 5 updated to the lenient loader and current config keys (without the reduced-motion clause), section 6 updated to match `src/cli.rs`, and `AGENTS.md` updated to `appearance.motion`.

## Measured and rejected

- Carrying the fd, names and stat from classification into the first size task (P1-09): saves about 2,000 syscalls on the 217k fixture, under 1%, and risks `RLIMIT_NOFILE`.
- Sort-batch-then-merge in `Report::apply` (P5-05): slower than std's stable sort at every size tested.
- Overlapping dashboard cache sizing with other work (P3-11): 258 ms vs 249 ms median, within noise.
