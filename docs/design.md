# kuizer design

Status: Phase 1 done. Phase 2 to 6 are in progress: live scan output, the interactive list with selection, guarded deletion with trash and permanent modes, a deletion history, `kuizer clean`, `kuizer duplicates` and `kuizer cache` exist; category selection, exclusion from the UI, the duplicates and caches views in the TUI, and `--json-lines` are still open. Later phases are specified here and land one pull request at a time.

## 1. Product specification

kuizer finds files and directories on a developer machine that can be removed and regenerated: dependency folders, build output, tool caches, temporary files and duplicate files. It shows them in a full-screen terminal UI or prints them for scripts, and deletes only what the user selects, after a preview.

Principles:

- Scanning never deletes. Deletion is a separate, explicit step with its own validation.
- A candidate needs evidence, not just a name. `dist` is only a candidate next to a `package.json`; `target` only next to a `Cargo.toml` and with Cargo's own `CACHEDIR.TAG` or `.rustc_info.json` inside.
- Every candidate carries a safety level. `safe` means a tool regenerates it from project files. `review` means it is probably generated but a person should look first. Bulk actions select `safe` only unless the user widens them.
- The UI stays responsive while the filesystem is slow. Results stream in as they are found and sized.
- Defaults are conservative: no symlink following, no crossing filesystems, virtual and network filesystems skipped.

Non-goals for 1.0: a daemon, scheduled cleanup, cloud storage, Windows-specific caches before the platform layer exists.

## 2. Architecture and module boundaries

```text
              ┌──────────┐
  cli ───────►│ commands │──────────────► output (text, json)
              └────┬─────┘                tui (phase 2)
                   │ Config
         ┌─────────┼───────────┬──────────────┐
         ▼         ▼           ▼              ▼
      config     scan ──────► detect       delete (phase 4)
                   │  walk      rules         guard, trash
                   │  size                    
                   ▼                       dupes (phase 5)
                platform                     size groups, blake3
                  mounts, inode, blocks
```

| Module | Owns | Knows nothing about |
| --- | --- | --- |
| `config` | TOML schema, defaults, `~` expansion, validation, exclusion globs | the walker, output |
| `detect` | rule specs, built-in rules, `Detector` (name, marker and content matching) | threads, I/O beyond `CACHEDIR.TAG` verification |
| `scan` | discovery walk, sizing workers, cancellation, events, progress counters | how results are shown |
| `platform` | mount table, device ids, allocated blocks, hard link identity | rules |
| `output` | text table and the versioned JSON report | scanning |
| `commands` | one function per CLI command, wiring the above | rendering details |
| `tui` (phase 2) | Ratatui app state, key handling, rendering | filesystem calls; it reads `scan::Event` and calls `delete` |
| `delete` (phase 4) | revalidation, trash or unlink, per-item results | detection |
| `dupes` (phase 5) | size grouping, partial and full BLAKE3 hashing, byte compare | artifact rules |

Scan pipeline:

```text
walk thread ──Found──► bounded(256) ──► N sizing threads ──Event──► bounded(1024) ──► consumer
     │                                         │
     └──────── Issue events ───────────────────┘          atomics: dirs, files, candidates, bytes
```

- One discovery thread walks with `read_dir`, using `d_type` from directory entries so plain files cost no `stat`. Each directory listing is kept as a sorted name list, shared with its children through an `Arc`, so marker checks (`package.json` beside `node_modules`) and content checks (`CACHEDIR.TAG` inside `target`) need no extra syscalls.
- A matched directory is pruned: the walker never descends into `node_modules`. Most of the bytes on a developer disk sit inside artifacts, so the walk itself stays small.
- Sizing threads (default: available cores, capped at 8) measure each artifact with `lstat`, count allocated blocks, and count a hard-linked inode once.
- Channels are bounded, so a slow consumer slows the scan instead of growing memory.
- Progress is read from atomics, never pushed as events, so a UI can poll it every frame at no cost to the scan.
- Cancellation is one `AtomicBool` checked per directory in both the walker and the sizers. Dropping a `Scan` cancels it, drains it and joins the threads.
- Memory is bounded by the walk stack plus results; the tree is never held in memory.

## 3. Directory structure

```text
kuizer/
├─ Cargo.toml
├─ src/
│  ├─ main.rs            exit codes, top-level error print
│  ├─ lib.rs
│  ├─ cli.rs             clap definitions
│  ├─ units.rs           ByteSize parse and display
│  ├─ commands/          scan, config, rules (clean, duplicates, cache later)
│  ├─ config/            schema, defaults, exclusions
│  ├─ detect/            rule types, built-ins, detector
│  ├─ scan/              walk, size, cancel, events
│  ├─ platform/          mount table (Linux), metadata helpers
│  ├─ output/            text, json
│  ├─ tui/               phase 2
│  ├─ delete/            phase 4
│  └─ dupes/             phase 5
├─ tests/                integration tests on temporary trees
├─ examples/synth.rs     synthetic tree generator for benchmarks
├─ benches/              phase 7
└─ docs/                 design, config example, JSON schema
```

It stays one crate until compile times or reuse justify a workspace split (`kuizer-core` and `kuizer`).

## 4. Dependencies

| Crate | Use | Decision |
| --- | --- | --- |
| `clap` 4 (derive) | CLI parsing, help, completions later | Adopted. Standard, well maintained. |
| `serde`, `toml`, `serde_json` | config and JSON output | Adopted. |
| `globset` | exclusion globs and rule patterns | Adopted. From ripgrep, compiles many globs into one matcher. |
| `crossbeam-channel` | bounded MPMC channels, `recv_timeout` | Adopted. `std::sync::mpsc` lacks multi-consumer receivers for the sizing pool. |
| `thiserror` | typed errors | Adopted. |
| `dirs` | config and home directories per platform | Adopted. |
| `tempfile` (dev) | isolated test trees | Adopted. |
| `walkdir` | traversal | Rejected for discovery: it hides the sibling listing that marker checks need and costs a `stat` per entry for pruning decisions. Own `read_dir` loop is about 150 lines. |
| `jwalk` | parallel traversal | Deferred to phase 7. Pruning is the main win already; parallel discovery is measured before adopting it. |
| `rayon` | CPU parallelism | Deferred to phase 5 for hashing, if BLAKE3's own `rayon` feature does not cover it. Sizing is I/O bound and uses a fixed thread pool. |
| `ratatui`, `crossterm` | TUI | Phase 2. Both active, crossterm works on all three target platforms. |
| `blake3` | duplicate hashing | Phase 5. |
| `trash` | recoverable deletion | Phase 4. Implements the freedesktop trash spec on Linux, Finder on macOS and the Recycle Bin on Windows. Its behaviour on other mounts is tested before relying on it. |

## 5. Configuration

File: `~/.config/kuizer/config.toml` (`kuizer config path`). Every key is optional; unknown keys are an error so typos never silently disable a safety setting. `kuizer config show` prints the effective config, `kuizer config check` validates, `kuizer config init` writes the defaults. A full example is in [`config.example.toml`](config.example.toml).

| Key | Default | Meaning |
| --- | --- | --- |
| `scan.roots` | current directory | Roots when no path is given on the command line |
| `scan.exclude` | `[]` | Globs. Without `/` they match a file or directory name at any depth, with `/` the absolute path. `~` expands. |
| `scan.exclude_extensions` | `[]` | File extensions never reported |
| `scan.skip_dirs` | `.git .hg .svn .jj` | Directory names never entered |
| `scan.ignore_file` | `.kuizerignore` | A directory containing this file, or the ignore file from before the rename, is skipped with its subtree |
| `scan.min_size` | `0` | Candidates smaller than this are dropped; bytes or `10MB`, `1.5GiB` |
| `scan.max_depth` | none | Depth limit for discovery |
| `scan.one_file_system` | `true` | Never cross into another mount |
| `scan.excluded_fs_types` | proc, sysfs, devtmpfs, cgroup, nfs, cifs, fuse.sshfs and others | Mounts of these types are never entered |
| `scan.excluded_mounts` | `/proc /sys /dev /run` | Paths never entered |
| `scan.size_threads` | `0` (cores, max 8) | Sizing workers |
| `detect.categories` | all | `dependencies`, `build`, `cache`, `temp` |
| `detect.builtin_rules` | `true` | Turn off to use only custom rules |
| `detect.disabled_rules` | `[]` | Built-in or custom rule names to skip; unknown names are an error |
| `[[rules]]` | none | Custom rules, checked before built-ins |
| `duplicates.min_size` | `4KiB` | Phase 5 |
| `duplicates.threads` | `0` | Phase 5 |
| `duplicates.verify` | `false` | Byte compare after hash match, phase 5 |
| `delete.mode` | `trash` | `trash` or `permanent`, phase 4 |

A rule:

```toml
[[rules]]
name = "android-build"
kind = "dir"              # dir or file
pattern = "build"         # name or glob on the name
markers = ["build.gradle", "build.gradle.kts"]   # any of these beside it
contains = ["intermediates", "outputs"]          # any of these inside it
category = "build"
safety = "review"         # custom rules default to review
ecosystem = "android"
```

`kuizer rules` lists the active rules.

## 6. CLI

```text
kuizer [--config PATH] [--json] [-q] [COMMAND]

kuizer                          TUI on a terminal (phase 2); scan output until then
kuizer scan [PATHS...]          list candidates
    --category NAME            repeatable
    --min-size SIZE
    --sort size|path|category
    --safe-only
    --limit N
kuizer duplicates [PATHS...]    --min-size SIZE, --verify, --limit N; reports only, never deletes
kuizer clean [PATHS...]         --dry-run, --permanent, --select safe|all|RULE, --yes, --category, --min-size
kuizer config [show|path|init|check]
kuizer rules                    active detection rules
kuizer cache                    known tool caches outside projects (npm, pnpm store, cargo, bun)
    --clean                    remove the selection, with the same flags as clean
```

Exit codes: `0` success, `1` error, `2` usage error. Unreadable paths are reported as issues and do not change the exit code; scripts read `issues` in the JSON.

JSON (`--json`) is one document with `schema_version: 1`:

```json
{
  "schema_version": 1,
  "command": "scan",
  "roots": ["/home/me/projects"],
  "candidates": [{
    "id": 1, "path": "/home/me/projects/web/node_modules", "kind": "dir",
    "rule": "node-modules", "category": "dependencies", "safety": "safe",
    "ecosystem": "node", "project": "/home/me/projects/web",
    "allocated_bytes": 412000256, "apparent_bytes": 380112345,
    "entries": 41231, "partial": false, "newest_modified": 1760000000
  }],
  "issues": [{ "path": "/home/me/projects/locked", "kind": "permission-denied", "message": "..." }],
  "summary": { "progress": { "dirs": 0, "files": 0, "candidates": 0, "bytes": 0, "issues": 0 }, "duration": 812, "cancelled": false }
}
```

Fields are only added within a schema version; renames or removals bump it. `--json-lines` streaming output arrives in phase 6.

## 7. TUI interaction model

Layout: header with roots, scan state and totals; main list; detail pane (path, rule, evidence, project, newest file, duplicate members); footer with key hints and the selected total.

Rendering runs at most every 50 ms or on input. The scan runs on its own threads; the UI thread drains up to a fixed number of events per frame, so a burst of results never delays a keypress.

| Key | Action |
| --- | --- |
| `j` `k` / arrows | move |
| `g` `G` | top, bottom |
| `space` | toggle selection |
| `a` | select all `safe` in the current view |
| `A` | select everything in the current view, after a confirmation |
| `c` | select or clear the current category |
| `u` | clear selection |
| `/` | filter by path or rule; `esc` clears |
| `s` | cycle sort: size, path, category, age |
| `1` `2` `3` `4` | views: artifacts, caches, duplicates, issues |
| `enter` / `l` | expand detail or duplicate group |
| `x` | exclude path (writes to config after confirmation) |
| `d` | delete selection: opens the preview dialog |
| `D` | toggle permanent deletion for the next dialog |
| `p` | pause or resume the scan |
| `ctrl-c` / `q` | cancel the scan, then quit |
| `?` | help |

The delete dialog lists every selected path with size and safety, shows the mode (trash or permanent), and needs `y` (`review` items require typing the count). Nothing is preselected for deletion.

## 8. Threat model

Assets: user source code, personal files, application data. Attacker: anything that can change the filesystem while kuizer runs (another process, a malicious repository, a shared directory).

| Threat | Mitigation |
| --- | --- |
| Symlink in a scanned tree points at `/` or `$HOME` | Symlinks are never followed in discovery or sizing, and never become candidates. Tested. |
| Directory replaced by a symlink between scan and delete (TOCTOU) | Phase 4: before deleting, `lstat` the path again and require the same device and inode as at scan time and not a symlink. On Linux, delete through `openat` with `O_NOFOLLOW` and `unlinkat` relative to a verified parent descriptor, so a swapped parent path component cannot redirect the delete. |
| Path traversal through rule patterns or config | Rule markers and contains entries must be plain names (no `/`, `..`); rejected at load. Candidates are always produced by the walker, never from user input. |
| Deleting a root or important parent | Phase 4 guard refuses `/`, home, any scan root, any mount point, any ancestor of the current directory, and system prefixes (`/usr`, `/etc`, `/bin`, `/boot`, `/var/lib`), regardless of selection. |
| Mislabelled user data (`dist` with handwritten files) | Context markers, content checks, `review` level for ambiguous names, no bulk selection of `review` by default. |
| Persistent data marked as cache | `CACHEDIR.TAG` only means "do not back up". uv, for example, tags its installed tool environments under `~/.local/share/uv/tools`. The generic tag rule is therefore `review`. Found on a real machine during Phase 1. |
| Browser and app profiles (cookies, sessions, saved logins) | Directories holding Chromium `Local State`, Firefox `profiles.ini` or a profile cookie store, and known browser, keyring and credential paths, are never entered by the scanner or the duplicate finder and are refused by the delete guard. Electron apps such as Discord ship `node_modules` with a `package.json` beside them, which the rules alone would mark `safe`. Found on a real machine. |
| Virtual or network filesystems | Excluded mounts and filesystem types, no crossing by default. |
| Hostile names (newlines, escape sequences) | Text output and TUI escape control characters (phase 2); JSON is always escaped. |
| Huge trees or deep nesting | Iterative walks, bounded channels, cancellation. |
| Permission errors, vanishing files | Reported as issues, never fatal. Tested, including deletion mid-scan. |
| Partial deletion | Each item reports success or failure; trash is preferred so mistakes are recoverable. |

## 9. Benchmarking strategy

Measure first, optimize second, and only with numbers from both warm and cold caches.

- Datasets: `examples/synth.rs` (projects with `node_modules`, `src` and `dist`; parameters for project and dependency counts), a duplicate-heavy set (phase 5), and real home directories reported by count only.
- Metrics: entries per second, total wall time, time to first candidate, peak RSS, CPU time, bytes read per second while hashing, TUI frame time and input latency under load (phase 2).
- Cold cache: `echo 3 > /proc/sys/vm/drop_caches` on a dedicated Linux machine; warm cache: three runs after one discarded run.
- Storage: NVMe, SATA SSD and a spinning disk or USB drive; concurrency matters most on slow storage.
- Tools: `hyperfine` for wall time, `/usr/bin/time -v` for RSS, `perf` and `cargo flamegraph` for profiles, `criterion` for the detector and hashing micro benchmarks.
- Comparisons: sizing threads 1, 2, 4, 8; single-threaded discovery against `jwalk` and a work-stealing walker; `stat` against `statx`.

Phase 1 baseline (container, 4 cores, warm cache, 200 projects with 100 dependencies each, 109k entries, 250 candidates totalling 472 MiB): 0.30 s with 1 sizing thread, 0.11 s with 4, 0.10 s with 8. Cold-cache and slow-storage numbers come from the Arch machine.

## 10. Phases and acceptance criteria

| Phase | Scope | Accepted when |
| --- | --- | --- |
| 1 | Crate, CLI, config, safe traversal, artifact rules, sizing, `scan` text and JSON | Done. 46 tests pass: rule context, pruning, excludes, ignore file, custom rules, symlinks, hard links, missing roots, mounts, cancellation, vanishing files. `clippy -D warnings` clean. Permission tests need a non-root user (CI). |
| 2 | TUI: streaming list, progress, sort, filter, detail pane | First result visible while scanning; input latency under 50 ms during a 1M entry scan; terminal restored on panic and `ctrl-c`. |
| 3 | Selection, category selection, totals, exclusion from the UI, cleanup preview | Selection survives re-sorting and filtering; totals match the JSON; preview lists exactly the selection. |
| 4 | `delete` module, trash and permanent modes, `kuizer clean`, dry run | Guard and revalidation tests: symlink swap, inode swap, root and home refusal, mount points, partial failures. Every destructive test runs in a temp dir. Dry run touches nothing (verified by tree snapshot). |
| 5 | Duplicates: size groups, partial then full BLAKE3, optional byte compare, hard link awareness | Correct groups on crafted sets including same size different content, hard links, zero-length files; reclaimable bytes exclude one copy per group; no automatic selection when the original is ambiguous. |
| 6 | Temp and editor files, global tool caches (`kuizer cache`), more ecosystems, `--json-lines`, documented schema file | Each rule has a positive and a negative test; schema documented and covered by a snapshot test. |
| 7 | Benchmarks, profiling, fuzzing of config and names, macOS and Windows platform layer | Benchmarks in CI on a fixed dataset; regression threshold set; builds and tests pass on macOS and Windows runners. |
