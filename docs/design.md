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

Every item comes from a source, and every source has the same verbs, filters, selection rules, history and JSON shape:

| Source | Items | Removable |
| --- | --- | --- |
| projects (default) | dependency folders, build output, project caches, temp files | yes |
| `cache` | global tool caches: npm, pnpm, cargo, pip, uv and others | yes |
| `toolchain` | rustup toolchains, nvm, fnm and volta Node versions, pyenv and uv Python versions | yes |
| `docker` | dangling images, build cache, stopped containers | yes, through the `docker` CLI |
| `duplicates` | groups of files with identical contents | no, report only |
| `big` | large files and directories that match no artifact rule | no, report only |

Non-goals for 1.0: a daemon, cloud storage, Windows-specific caches before the platform layer exists. Scheduled cleanup comes after 1.0, as a systemd user timer that runs a policy (section 6.6), not as a daemon.

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
| `delete.confirm` | `false` | Ask before moving to the trash in the TUI. Permanent deletion always asks |

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

A policy is a named set of filters and a selection, run with `kuizer clean --policy NAME` or by a schedule (section 6.6):

```toml
[policy.stale-deps]
source = "projects"       # projects, cache, toolchain or docker
paths = ["~/apps"]        # projects only; defaults to scan.roots
category = ["dependencies", "build"]
rule = []                 # rule names, empty means all
idle = "60d"
min_size = "50MB"
exclude = []              # globs, added to scan.exclude
select = "safe"           # safe, all or a list of rule names
permanent = false
```

Every key except `source` is optional. Unknown keys and unknown rule names are an error, as everywhere else in the config.

## 6. CLI

```text
kuizer [--config PATH] [--json | --json-lines] [-q] [COMMAND]

kuizer [PATHS...]                         TUI on a terminal
kuizer ui [PATHS...]                      same, explicitly
kuizer scan  [PATHS...] [FILTERS]         list project items
    --sort size|path|category|idle
    --limit N
    --no-live
kuizer clean [PATHS...] [FILTERS] [REMOVAL]
    --policy NAME                        filters and selection from the config

kuizer cache            [FILTERS]                  list global tool caches
kuizer cache clean      [FILTERS] [REMOVAL]
kuizer toolchain        [FILTERS]                  list installed toolchain versions
kuizer toolchain clean  [FILTERS] [REMOVAL]
kuizer docker           [FILTERS]                  list Docker images, build cache, containers
kuizer docker clean     [FILTERS] [REMOVAL]

kuizer duplicates [PATHS...] --min-size SIZE --verify --limit N    report only
kuizer big        [PATHS...] --min-size SIZE --limit N             report only

kuizer history                            sessions with their ids; `insight` is an alias
kuizer history show SESSION               every item of one session
kuizer restore ID... | --last             put trashed items back

kuizer policy list | show NAME
kuizer schedule install NAME --every DURATION | remove NAME | list

kuizer rules                              active detection rules
kuizer config show | path | init | check

FILTERS
    --category NAME                      repeatable: dependencies, build, cache, temp
    --rule NAME                          repeatable
    --min-size SIZE                      10MB, 1.5GiB
    --idle DURATION                      30d, 2w, 6mo; `--older-than` is an alias
    --project GLOB                       repeatable, matched on the project path
    --exclude GLOB                       repeatable, added to scan.exclude

REMOVAL
    --select safe|all|RULE               repeatable, default safe
    --dry-run                            print the selection, touch nothing
    --permanent                          skip the trash
    -y, --yes                            skip the confirmation
```

### 6.1 Filters and selection

Filters decide which items are listed; `--select` decides which listed items are removed. `scan` and `clean` take the same filters, so `kuizer scan X` lists exactly what `kuizer clean X` considers. `scan --safe-only` stays as a hidden alias for `--select safe` applied as a filter. Within one flag, repeated values are a union; across flags, an intersection.

### 6.2 Sources

A source other than projects is a subcommand whose bare form lists and whose `clean` sub-verb removes. `cache --clean` stays as a hidden alias of `cache clean` for one minor release. A listing never gains a flag that turns it into a deletion.

`duplicates` and `big` have no `clean`: removing the wrong copy of a file cannot be regenerated. Duplicate handling, when it comes, is `duplicates link`, which replaces copies with hard links or reflinks and deletes no content.

`docker clean` removes through `docker image rm`, `docker builder prune` and `docker container rm`, never by deleting files under `/var/lib/docker`. Without a `docker` binary the source lists nothing and reports one issue.

### 6.3 Idle time

`--idle 60d` keeps items whose project has had no activity for 60 days. Project activity is the later of the newest modification time outside the project's artifacts and the last commit in its git repository. An artifact's own modification time is not used, because `npm install` and `cargo build` refresh it in a project that is otherwise abandoned. Other sources use their own signal: a toolchain's last use from its shims or tool metadata, a cache's newest entry, an image's last use by a container. An item whose activity cannot be determined never matches `--idle`.

Durations accept `d`, `w`, `mo` (30 days) and `y` (365 days).

### 6.4 History and restore

Each session in the history file gets an id `s<N>`, and each item in it `s<N>.<M>`, numbered in order and never reused. Existing history files get ids assigned in file order on first load. `kuizer history` lists sessions newest first with id, time, command, item count, freed bytes and how much is still in the trash. `kuizer insight` stays as an alias.

`kuizer restore s42` restores every item of a session that is still in the trash, `restore s42.3` one item, `restore --last` the latest session. A restore refuses an item when something now exists at its original path, or when the trash no longer holds it, and reports each refusal. It never overwrites. Every restore is appended to the history as its own session.

### 6.5 Policies

`kuizer policy list` prints the configured policies, `policy show NAME` the effective filters and selection, and `clean --policy NAME` runs one. Flags given next to `--policy` narrow it further but cannot widen its selection or make it permanent.

### 6.6 Schedules

`kuizer schedule install NAME --every 1w` writes a systemd user service and timer, `kuizer-NAME.service` and `.timer`, that run `kuizer clean --policy NAME --yes -q`, and enables the timer. `schedule remove` disables and deletes both, and `schedule list` shows each timer with its last and next run. Linux only; on other platforms the command exits with a usage error that says so.

A scheduled run removes `review` items only when its policy says `select = "all"` or names their rules, and deletes permanently only when the policy says `permanent = true`. Neither can be set by a flag at install time. Runs are recorded in the history like any other session.

Exit codes: `0` success, `1` error, `2` usage error. Unreadable paths are reported as issues and do not change the exit code; scripts read `issues` in the JSON.

JSON (`--json`) is one document with `schema_version: 1`. Items from every source share the `candidates` shape; `source`, `project_last_active` and, in history output, `id` are added fields:

```json
{
  "schema_version": 1,
  "command": "scan",
  "roots": ["/home/me/projects"],
  "candidates": [{
    "id": 1, "path": "/home/me/projects/web/node_modules", "kind": "dir",
    "source": "projects", "rule": "node-modules", "category": "dependencies", "safety": "safe",
    "ecosystem": "node", "project": "/home/me/projects/web",
    "allocated_bytes": 412000256, "apparent_bytes": 380112345,
    "entries": 41231, "partial": false, "newest_modified": 1760000000,
    "project_last_active": 1752000000
  }],
  "issues": [{ "path": "/home/me/projects/locked", "kind": "permission-denied", "message": "..." }],
  "summary": { "progress": { "dirs": 0, "files": 0, "candidates": 0, "bytes": 0, "issues": 0 }, "duration": 812, "cancelled": false }
}
```

Fields are only added within a schema version; renames or removals bump it. `--json-lines` prints one candidate per line as it is found, then one summary line with `"type": "summary"`.

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
| `s` | cycle sort: size, path, category, idle |
| `1` `2` `3` `4` | views: artifacts, caches, duplicates, issues |
| `enter` / `l` | expand detail or duplicate group |
| `x` | exclude path (writes to config after confirmation) |
| `d` | delete selection, or the item under the cursor |
| `enter` | delete selection when something is selected |
| `D` | toggle permanent deletion for the next dialog |
| `p` | pause or resume the scan |
| `ctrl-c` / `q` | cancel the scan, then quit |
| `?` | help |

By default `d` and `enter` move the selection to the trash straight away. With `delete.confirm = true` in the config or `kuizer ui --confirm`, a dialog first lists every selected path with size and safety, shows the mode (trash or permanent), and needs `y` or `enter`. Permanent deletion always shows the dialog because it cannot be undone. Nothing is preselected for deletion.

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
| 8 | Shared `FILTERS` and `REMOVAL` on every source, `cache clean` with the old flag as an alias, `--idle`, idle column and sort, `--project`, `--rule`, `--exclude` | `scan` and `clean --dry-run` list the same items for every filter combination (property test). Idle time tests: artifact touched in an idle project, commit without file changes, project outside git, unknown activity never matches. |
| 9 | History ids, `history`, `history show`, `restore` | Ids stable across reloads of an old history file. Restore tests in a temp trash: whole session, one item, item already emptied from the trash, path reoccupied, partial session. |
| 10 | `toolchain`, `docker` and `big` sources | Each source has a positive and a negative test. A toolchain pinned by a `rust-toolchain.toml`, `.nvmrc` or `.python-version` under the scan roots is `review`, never `safe`. `docker` without a daemon reports one issue and exits `0`. |
| 11 | Policies and schedules | `config check` rejects unknown policy keys. A flag next to `--policy` cannot widen it (test). Generated unit files match a snapshot; `schedule install` and `remove` leave no files behind on failure. |
