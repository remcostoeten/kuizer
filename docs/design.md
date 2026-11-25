# kuizer design

Status: CLI, Ratatui TUI and GPUI desktop app (`kuizer-gui`) are shipped on Linux and macOS: project artifacts, global caches, toolchains, Docker/Podman, git repositories, duplicates, `--idle`, HTML reports and dashboard, guarded deletion (permanent by default, opt-in trash), deletion history with ids, and `restore`. Still open from this spec: `big`, policies, `--idle` on non-project sources, `cache`/`toolchain`/`docker` `clean` subcommands, a JSON schema file, exclusion from the UI, and the toolchain, Docker and git views in the TUI.

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
| `toolchain` | rustup toolchains, nvm, fnm and volta Node versions, pyenv and uv Python versions, SDKMAN candidates, Go SDKs in `~/sdk` | yes, always permanently |
| `docker` | Docker and Podman dangling images, build cache, stopped containers, orphaned volumes | yes, through the engine's CLI, always permanently |
| `git` | git repositories with loose objects to pack and local branches already merged | yes, through the `git` CLI, merged branches permanently |
| `duplicates` | groups of files with identical contents | no, report only |
| `big` | large files and directories that match no artifact rule | no, report only |

Non-goals for 1.0: a daemon, cloud storage, Windows-specific caches before the platform layer exists. Scheduled cleanup is out of scope; run `kuizer clean --policy NAME --yes` from cron or a timer you manage.

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
| `delete` (phase 4) | revalidation, trash or unlink, the shared executor (per-item results, history, nested paths never at once), when to ask before deleting | detection, prompts and rendering |
| `dupes` (phase 5) | size grouping, partial and full BLAKE3 hashing, byte compare | artifact rules |
| `repos` | git repositories noted during the walk, `count-objects` estimates, merged branches, busy checks, `git gc` and branch deletion with revalidation | artifact rules, the trash |

Scan pipeline (`src/scan/mod.rs`, `src/scan/pool.rs`, `src/scan/size.rs`):

```text
                        work-stealing Queue<Task> (Walk + Size)
                                         │
coordinator ──seeds──► N worker threads (local LIFO stacks + shared spill)
                                         │
                           Event ──► bounded(1024) ──► consumer
                                     atomics: dirs, files, candidates, bytes, issues
```

- A coordinator thread (`kuizer-scan`) loads the mount table and protection rules, seeds a work-stealing `Queue<Task>` (`src/scan/pool.rs`) with the scan roots, and spawns `N` worker threads inside `thread::scope`. Default `N` is twice the available cores, capped at 16 (or 2 when any root sits on a spinning disk); `scan.size_threads` overrides it, capped at 64.
- Every worker runs both discovery (`Task::Walk`) and artifact sizing (`Task::Size`). Each keeps a local LIFO stack and spills the bottom half to a shared list whenever another worker is idle or its stack grows past 64 while the shared list is empty; idle workers steal half of the shared list, so one deep or wide subtree never pins a single thread.
- Discovery reads directories using `d_type` from directory entries so plain files cost no `stat`. Each directory listing is kept as a sorted name list, so marker checks (`package.json` beside `node_modules`) and content checks (`CACHEDIR.TAG` inside `target`) need no extra syscalls.
- A matched directory is pruned from discovery: the walker never descends into `node_modules`. Single-file matches are delivered immediately; matched directories are queued as `Task::Size` tasks whose `Total` aggregates allocated blocks, apparent bytes, entries, newest mtime and a sharded hard-link set across threads.
- Workers send `Event::Candidate` and `Event::Issue` into a single `bounded(1024)` channel so a slow consumer slows the scan instead of growing memory; once the workers finish, the coordinator sends `Event::Finished`.
- Progress is read from atomics (flushed by workers in batches of 256 directories) rather than pushed as events, so a UI can poll it every frame at no cost to the scan; a recent directory path is sampled every 32 entries.
- Cancellation is a shared `Cancel` (`Arc<AtomicBool>`) checked per task and directory and propagated to `Queue::stop()`. Dropping a `Scan` cancels it, drains the event channel and joins the coordinator thread.
- Memory is bounded by the work stacks plus emitted results; the filesystem tree is never held in memory.

## 3. Directory structure

```text
kuizer/
├─ Cargo.toml
├─ src/
│  ├─ main.rs            exit codes, top-level error print
│  ├─ lib.rs
│  ├─ cli.rs             clap definitions
│  ├─ units.rs           ByteSize parse and display, shared item ages
│  ├─ source.rs          removal sources and which of them can go to the trash
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
| `clap` 4 (derive) | CLI parsing, help | Adopted. Standard, well maintained. `clap_complete` and `clap_mangen` print completions and man pages from the hidden `kuizer generate` command, so no build script is needed. |
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

File: `~/.config/kuizer/config.toml` (`kuizer config path`). Every key is optional. Normal runs use the lenient loader (`src/config/lenient.rs`): unknown or invalid keys are warned and ignored, each bad setting falls back to its default (and list entries are dropped one by one), and the app keeps running. When a rejected setting could widen what gets removed (`scan.*`, `detect.*`, `delete.*`, `git.*`, `toolchains.*`, `docker.*`, or an unknown section), `delete.confirm` is forced on and unattended `--yes` removal is refused until fixed; an unreadable or unparseable file, or an invalid `delete.mode`, also falls back to `delete.mode = "trash"`. `kuizer config show` prints the effective config, `kuizer config check` validates strictly and fails on any warning (including unknown keys), and `kuizer config init` writes the defaults. A full example is in [`config.example.toml`](config.example.toml).

| Key | Default | Meaning |
| --- | --- | --- |
| `scan.roots` | current directory | Roots when no path is given on the command line |
| `scan.exclude` | `[]` | Globs. Without `/` they match a file or directory name at any depth, with `/` the absolute path. `~` expands. |
| `scan.exclude_extensions` | `[]` | File extensions never reported |
| `scan.skip_dirs` | `.git .hg .svn .jj` | Directory names never entered |
| `scan.ignore_file` | `.kuizerignore` | A directory containing this file, or the ignore file from before the rename (`.sweepignore`), is skipped with its subtree |
| `scan.min_size` | `0` | Candidates smaller than this are dropped; bytes or `10MB`, `1.5GiB` |
| `scan.max_depth` | none | Depth limit for discovery |
| `scan.one_file_system` | `true` | Never cross into another mount |
| `scan.excluded_fs_types` | proc, sysfs, devtmpfs, cgroup, nfs, cifs, fuse.sshfs and others | Mounts of these types are never entered |
| `scan.excluded_mounts` | `/proc /sys /dev /run` | Paths never entered |
| `scan.size_threads` | `0` (twice the cores, max 16; 2 on a spinning disk) | Threads that walk and size a scan (explicit values capped at 64); idle-activity checks use cores, max 8 |
| `detect.categories` | all | `dependencies`, `build`, `cache`, `temp` |
| `detect.builtin_rules` | `true` | Turn off to use only custom rules |
| `detect.disabled_rules` | `[]` | Built-in or custom rule names to skip; unknown names warn and are ignored on load, rejected by `config check` |
| `[[rules]]` | none | Custom rules, checked before built-ins |
| `duplicates.enabled` | `false` | Run the duplicate search automatically after each GUI scan once results are on screen; `kuizer duplicates` is not affected |
| `duplicates.min_size` | `4KiB` | Skip files smaller than this when searching for duplicates |
| `duplicates.threads` | `0` | Hashing threads (`0` = automatic) |
| `duplicates.verify` | `false` | Compare matched files byte by byte after hashing |
| `duplicates.include_hidden` | `false` | Look inside hidden home folders and tool-managed cache and toolchain folders |
| `delete.mode` | `permanent` | `permanent` deletes for good, `trash` moves project artifacts and caches to the trash. Toolchains, Docker objects and git branches are always removed for good |
| `delete.confirm` | `true` | Ask before deleting in the CLI and the TUI. `review` items and projects with uncommitted changes always ask |
| `git.min_gain` | `1MiB` | Minimum estimated `git gc` savings before a repository is listed |
| `git.aggressive` | `false` | Pass `--aggressive` to `git gc` |
| `git.keep_branches` | `["main", "master", "develop", "trunk"]` | Branch names never offered for merged-branch deletion |
| `git.threads` | `0` | Threads inspecting git repositories (`0` = automatic) |
| `appearance.motion` | `true` | Animations in the GUI: state transitions, tooltips, loaders and skeletons. GPUI 0.2.2 cannot read the system reduced-motion setting, so this key is the only switch; off makes state changes instant |
| `appearance.sidebar` | `expanded` | `expanded` shows the GUI sidebar with labels, `collapsed` opens it as a strip of icons |
| `appearance.theme` | `dark` | `dark` or `light` color theme in the GUI |
| `appearance.accent` | `purple` | `purple`, `blue`, `teal` or `pink`: the color of the selection, the focus outline and the bars in the GUI |
| `appearance.density` | `comfortable` | `comfortable` or `compact`: `compact` uses shorter list rows and less vertical padding |
| `appearance.zoom` | `100` | GUI interface size in percent, 50 to 200, rounded to steps of ten. The zoom shortcuts change it and save it here |
| `route_shortcuts.enabled` | `true` | Whether `Shift+1` to `Shift+9` switch GUI sidebar sections |
| `route_shortcuts.hints` | `true` | Whether key badges appear in the GUI sidebar and section tooltips |
| `toolchains.tools` | `["rustup", "nvm", "fnm", "volta", "pyenv", "uv", "sdkman", "go"]` | Toolchain managers to inspect |
| `toolchains.pins` | `true` | Search scanned roots for version pin files (`rust-toolchain.toml`, `.nvmrc`, `.python-version`, etc.) and mark pinned versions `review` |
| `docker.engines` | `["docker", "podman"]` | Container engine CLIs to inspect |
| `docker.timeout_secs` | `10` | Per-command timeout in seconds for engine CLI calls (minimum 1) |
| `docker.recent_hours` | `24` | Containers stopped within this window are marked `review` |

A rule:

```toml
[[rules]]
name = "jekyll-site"
kind = "dir"              # dir or file
pattern = "_site"         # name or glob on the name
markers = ["_config.yml"] # any of these beside it; globs such as "*.csproj" allowed
contains = ["index.html"] # any of these inside it
category = "build"
safety = "review"         # custom rules default to review
ecosystem = "jekyll"
```

`kuizer rules` lists the active rules.

A policy is a named set of filters and a selection, run with `kuizer clean --policy NAME`:

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
```

Every key except `source` is optional. Policies are not yet implemented; when added, unknown policy keys and unknown rule names will be warned on load and rejected by `config check`.

## 6. CLI

```text
kuizer [--config PATH] [--json | --json-lines] [-q] [COMMAND]

kuizer [PATHS...] [--confirm]             TUI on a terminal, scan output otherwise
kuizer scan  [PATHS...] [FILTERS]         list project items
    --sort size|path|category|idle
    --limit N
    --no-live
    --safe-only
    --report[=FILE] [--no-open] [--force] write an interactive HTML report and open it
kuizer clean [PATHS...] [FILTERS] [REMOVAL]
    --policy NAME                         filters and selection from the config (planned)

kuizer cache            [FILTERS]                  list global tool caches
kuizer cache clean      [FILTERS] [REMOVAL]        (`--clean` is a hidden alias per D8)
kuizer toolchain        [PATHS...] [FILTERS]       list installed toolchain versions; `toolchains` is an alias
kuizer toolchain clean  [PATHS...] [FILTERS] [REMOVAL] (`--clean` is a hidden alias per D8)
kuizer docker           [FILTERS]                  list Docker/Podman images, build cache, containers, volumes; `podman` is an alias
kuizer docker clean     [FILTERS] [REMOVAL]        (`--clean` is a hidden alias per D8)
kuizer git              [PATHS...]                 list git repos with loose objects or merged branches; `repos` is an alias
    --gc                                           run `git gc` in listed repositories
    --branches                                     delete listed merged local branches
    --dry-run                                      print what would run, touch nothing
    -y, --yes                                      skip the confirmation

kuizer duplicates [PATHS...] --min-size SIZE --verify --limit N    report only; `deduplicate` is an alias
kuizer big        [PATHS...] --min-size SIZE --limit N             report only (planned)

kuizer dashboard [PATHS...]               offline HTML overview (or `--json`); `web` is an alias
    --no-duplicates                       skip the duplicate search
    --min-size SIZE                       minimum duplicate size
    --out=FILE [--no-open] [--force]      write to FILE instead of the cache directory

kuizer history                            sessions with their ids; `insight` is an alias
kuizer history show SESSION               every item of one session
kuizer restore ID... | --last [--dry-run] put trashed items back

kuizer policy list | show NAME            (planned)

kuizer rules                              active detection rules
kuizer config show | path | init | check

FILTERS
    --category NAME                      repeatable: dependencies, build, cache, temp
    --rule NAME                          repeatable, a name --select takes
    --min-size SIZE                      10MB, 1.5GiB
    --idle DURATION                      30d, 2w, 6mo, 1y; `--older-than` is an alias
    --project GLOB                       repeatable, matched on the project path or a folder above it
    --exclude GLOB                       repeatable, added to scan.exclude

REMOVAL
    --select safe|all|RULE               repeatable, default safe
    --dry-run                            print the selection, touch nothing
    -y, --yes                            skip the confirmation
```

### 6.1 Filters and selection

Filters decide which items are listed; `--select` decides which listed items are removed. `scan` and `clean` take the same filters, so `kuizer scan X` lists exactly what `kuizer clean X` considers. `scan --safe-only` stays as a hidden alias for `--select safe` applied as a filter. Within one flag, repeated values are a union; across flags, an intersection.

Flags narrow the config and never widen it. `--project` and `--exclude` globs use the `scan.exclude` syntax: a pattern with a `/` matches a whole path, one without matches a folder name, and `~` is the home directory. An item without a project, such as a global cache, never matches `--project`. Project activity for `--idle` and toolchain pins are still gathered without the `--exclude` globs, so excluding a folder cannot make a project look idle or a pinned version look unused. `cache`, `toolchain` and `docker` take `--category`, `--min-size`, `--rule`, `--project` and `--exclude` before or after `clean`; `--idle` is not available on them yet.

### 6.2 Sources

A source other than projects is a subcommand whose bare form lists and whose `clean` sub-verb removes: `cache clean`, `toolchain clean`, and `docker clean`. Per decision D8, `--clean` stays as a hidden alias for one minor release. A listing never gains a flag that turns it into a deletion.

`duplicates` and `big` have no `clean`: removing the wrong copy of a file cannot be regenerated. Duplicate handling, when it comes, is `duplicates link`, which replaces copies with hard links or reflinks and deletes no content.

`docker clean` removes through `docker image rm`, `docker builder prune`, `docker container rm` and `docker volume rm` (and the equivalent `podman` commands), never by deleting files under `/var/lib/docker`. Without an engine binary the source lists nothing and reports one issue.

`kuizer git` (`repos` alias) lists repositories with reclaimable loose objects (`--gc`) or merged local branches (`--branches`), and runs directly through `git` rather than the trash.

### 6.3 Idle time

`--idle 60d` keeps items whose project has had no activity for 60 days. Project activity is the later of the newest modification time outside the project's artifacts, the activity of its nested projects and the last commit in its git repository. An artifact's own modification time is not used, because `npm install` and `cargo build` refresh it in a project that is otherwise abandoned. Activity only counts what the scan itself would enter: `scan.exclude`, `scan.exclude_extensions`, skipped folders, ignore files, excluded mounts and `scan.one_file_system` apply to it the same way. Other sources use their own signal: a toolchain's last use from its shims or tool metadata, a cache's newest entry, an image's last use by a container. An item whose activity cannot be determined never matches `--idle`.

Durations accept `d`, `w`, `mo` (30 days) and `y` (365 days).

### 6.4 History and restore

Each session in the history file gets an id `s<N>`, and each item in it `s<N>.<M>`, numbered in order and never reused. Existing history files get ids assigned in file order on first load. `kuizer history` lists sessions newest first with id, time, command, item count, freed bytes and how much is still in the trash. `kuizer insight` stays as an alias.

`kuizer restore s42` restores every item of a session that is still in the trash, `restore s42.3` one item, `restore --last` the latest session. A restore refuses an item when something now exists at its original path, or when the trash no longer holds it, and reports each refusal. It never overwrites. Every restore is appended to the history as its own session.

### 6.5 Policies

`kuizer policy list` prints the configured policies, `policy show NAME` the effective filters and selection, and `clean --policy NAME` runs one. Flags given next to `--policy` narrow it further but cannot widen its selection or make it permanent.

Exit codes: `0` success, `1` error, `2` usage error. Unreadable paths are reported as issues and do not change the exit code; scripts read `issues` in the JSON.

Versioned JSON reports use `schema_version: 2`. Scan, cache, toolchain and Docker listings use `candidates`. Their rows include `source` and the same candidate fields. Removal reports use `items` with the candidate fields and a removal outcome. [JSON Schema for versioned reports](report.schema.json)

Scripts migrating from version 1 can read `candidates` in place of `caches`, `toolchains` and `objects`. The Docker row's numeric `id` identifies the candidate in the report. The engine's object ID is in `object_id`, and Docker issues have `path`, `kind` and `message` fields.

```json
{
  "schema_version": 2,
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

Fields are only added within a schema version; renames or removals bump it. `--json-lines` prints one candidate per line as it is found, with `"type": "candidate"` and the fields of a report row, then one summary line with `"type": "summary"` and every other field of the report. It works on `scan` and the `cache`, `toolchain` and `docker` listings. With `--idle`, `--sort idle` or `--limit` a scan prints its lines once it finishes, in sort order. `clean`, `cache clean`, `toolchain clean` and `docker clean` print one line per item with `"type": "removal"` and the fields of a removal report item as soon as that item is removed or has failed, in the order they finish, then one summary line with every other field of the removal report. A dry run, or a removal that is cancelled at the prompt, prints every selected item with status `would-remove`. Confirmation works exactly as with `--json`: the prompt goes to stderr, and without a terminal to ask on the command refuses with exit code 1 and prints nothing unless `--yes` is given. Other commands refuse `--json-lines` and ask for `--json`.

## 7. TUI interaction model

The TUI shows project artifacts, scan progress, the focused item's details, and selection totals. It draws on input or at most every 50 ms while scanning. A worker streams results and the UI processes a bounded number of events per frame.

Seven views sit on `1` to `7`: artifacts, caches, duplicates, issues, toolchains, Docker and git. Caches, toolchains and Docker objects are listed and duplicates searched the first time their view opens, on their own threads: each cache and toolchain appears once it is sized, Docker objects appear once Docker and Podman answer, and duplicates appear when the search finishes, with its progress in the header. Toolchains wait for the pin search under the scan roots when `toolchains.pins` is on, so a pinned version is never shown as safe. Artifacts, caches, toolchains and Docker objects are lists you select from and delete; each keeps its own selection and cursor, and a delete removes only what the open view selected, through the same core executor as the CLI. Caches are revalidated against their parent folders, as `kuizer cache clean` does. Toolchains and Docker objects are always removed for good, whatever `delete.mode` says, and the confirmation says so. Duplicates and issues are reports: selection and deletion keys say so and do nothing, and `/` searches their paths.

The git view lists the repositories the scan walked through that are worth cleaning: what `git gc` would free and the local branches already merged into the default branch. They are read once the scan finishes and the view is open. `c` runs `git gc` in the repository under the cursor and `C` in every repository shown, without asking, since gc only packs what git would pack on its own. `b` and `B` delete merged branches after a confirmation that lists them, opens with the focus on Cancel and takes Tab, Left, Right, `h` and `l` to move it, so an Enter right after asking deletes nothing. Each branch is checked again before it goes, through the same core functions as the GUI and `kuizer git`. Busy repositories and repositories still running git are skipped.

The GUI and TUI read configured shortcuts from the same `[keys]` table. The TUI help overlay shows the effective bindings. These are the TUI defaults.

| Key | Action |
| --- | --- |
| `1` to `7` | Switch view: artifacts, caches, duplicates, issues, toolchains, Docker, git |
| `j`, `k`, up, down | Move one row |
| `g`, `G`, Home, End | Jump to the first or last row |
| `J`, `K`, Shift+Up, Shift+Down | Move and extend selection |
| Page Up, Page Down, Ctrl+U, Ctrl+D | Move a full or half page |
| Space | Toggle selection and advance |
| `a` | Select the safe items in the current view |
| `P` | Select safe items in the current project. Press again to add review items, then again to clear |
| `u` | Clear selection |
| `/` | Filter by path or rule. Enter applies and Esc clears |
| `c` | Cycle the category filter |
| `f` | Toggle the safe-only filter |
| `x` | Clear all filters |
| Esc | Back out one level: a dialog, the search, the filters, then the selection. It never quits |
| `s` | Cycle sort by size, path, or category |
| `p` | Switch between grouping by rule and grouping by project |
| `d`, Delete | Delete the selection, or the item under the cursor |
| Enter | Delete the selection when it is non-empty |
| `c`, `C` | In the git view: `git gc` in this repository, or in every repository shown |
| `b`, `B` | In the git view: delete merged branches here, or in every repository shown, after a confirmation |
| `q`, Ctrl+Q, Ctrl+C | Quit, asking first if a deletion is running |
| `?` | Show shortcut help |

Deleting uses the mode set by `delete.mode`. By default, deletion asks for confirmation and lists the selected items with size and safety. Press `y` or Enter to confirm. A selection with review items asks for its item count instead: only the exact number followed by `y` or Enter deletes, a wrong number clears the field and keeps the dialog open, Backspace erases a digit and Esc cancels. A held key never confirms any dialog. With `delete.confirm = false`, the TUI still asks for review items and projects with uncommitted changes or unknown Git status. `kuizer --confirm` forces a confirmation. The TUI starts with an empty selection.

Colors follow the GUI palette, adapted to 256-color terminals in `src/tui/palette.rs`: accent for the open view and focus, yellow for review and warnings, a red fill for the destructive button when it has the focus.

## 8. Threat model

Assets: user source code, personal files, application data. Attacker: anything that can change the filesystem while kuizer runs (another process, a malicious repository, a shared directory).

| Threat | Mitigation |
| --- | --- |
| Symlink in a scanned tree points at `/` or `$HOME` | Symlinks are never followed in discovery or sizing, and never become candidates. Tested. |
| Directory replaced by a symlink between scan and delete (TOCTOU) | Phase 4: before deleting, `lstat` the path again and require the same device and inode as at scan time and not a symlink. On Linux, delete through `openat` with `O_NOFOLLOW` and `unlinkat` relative to a verified parent descriptor, so a swapped parent path component cannot redirect the delete. |
| Path traversal through rule patterns or config | Rule markers and contains entries must be plain names (no `/`, `..`); a marker may be a glob on a sibling name such as `*.csproj`. Rejected at load. Candidates are always produced by the walker, never from user input. |
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

CI benchmarks `kuizer --json scan` on the same 200-project, 100-dependency fixture with four sizing threads. `benchmarks/scan-baseline.json` stores the `ubuntu-latest` reference (0.13 s median, calibrated from 0.103–0.163 s runner variance) and CI fails when the measured median exceeds it by more than 45 percent.

## 10. Phases and acceptance criteria

Implementation plans for the open phases are in [`phases/`](phases/README.md).

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
| 9 | History ids, `history`, `history show`, `restore` | Done on Linux; Windows needs the phase 7 CI runner. Ids stable across reloads of an old history file. Restore tests in a temp trash: whole session, one item, item already emptied from the trash, path reoccupied, partial session. |
| 10 | `toolchain`, `docker` and `big` sources | Each source has a positive and a negative test. A toolchain pinned by a `rust-toolchain.toml`, `.nvmrc` or `.python-version` under the scan roots is `review`, never `safe`. `docker` without a daemon reports one issue and exits `0`. |
| 11 | Policies | `config check` rejects unknown policy keys. A flag next to `--policy` cannot widen it (test). |
