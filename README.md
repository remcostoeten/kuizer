# kuizer

A terminal tool that finds generated project artifacts, development caches and duplicate files on a developer machine, and removes the ones you select.

kuizer reports `node_modules`, `.next`, Cargo `target` directories and other build output only when the surrounding project confirms them: a `dist` folder counts next to a `package.json`, a `target` folder next to a `Cargo.toml` with Cargo's cache tag inside. Each result is marked `safe` or `review`. Scanning never deletes anything.

## Status

`kuizer scan` with live text output and JSON, configuration, the detection rules, an interactive list with selection and guarded deletion, `kuizer clean`, `kuizer duplicates` and `kuizer cache`. The plan is in [docs/design.md](docs/design.md).

## Install

```sh
cargo install --path .
```

## Usage

```sh
kuizer                  # interactive list of the current directory
kuizer ui ~/projects    # interactive list of a folder
```

| Key | Action |
| --- | --- |
| `j` `k` or arrows | move |
| `space` | select or unselect, then move down |
| `a` | select every `safe` item in view |
| `P` | select every item in the project under the cursor |
| `u` | clear selection |
| `d` | delete the selection, or the item under the cursor, after a confirmation |
| `t` | in the confirmation: switch between trash and permanent deletion |
| `/` | filter by path or rule |
| `s` | sort by size, path or category |
| `p` | group by rule (default) or by project |
| `?` | all keys |
| `q` | quit |

Deletion moves items to the trash by default (`delete.mode` in the config). Before each item is removed, kuizer checks again that it is the same directory it scanned, not a symlink, not a mount point, inside a scanned root, and not your home directory, a scan root or a system directory.

Every session that deletes something is appended to `~/.kuizer-history.json`: when it ran, the command, each removed path with its rule, mode, size and timestamp, the session total, and a cumulative total at the bottom. `kuizer insight` prints it and marks each trashed item as still in the trash or gone from it, with the total that emptying the trash would free. `kuizer --json insight` prints the file with an added `in_trash` total.

The header shows the free space on the scanned filesystems. With a selection in permanent mode it also shows the free space after deleting it; in trash mode it notes that the selection stays on disk. The footer counts freed and trashed space separately.

Projects inside a git repository with uncommitted changes get a warning with the change count, in the detail line, the project heading and the delete confirmation. It never blocks a delete: removing generated folders does not touch tracked files.

```sh
kuizer scan ~/projects
kuizer scan ~/projects --safe-only --min-size 50MB
kuizer --json scan ~/projects
kuizer rules
kuizer config show
```

`kuizer clean` scans, prints the selection and removes it after a confirmation. It selects `safe` items unless `--select` says otherwise: `all`, or a rule name such as `--select js-dist`, repeatable. A selection with `review` items asks you to type the item count. `--dry-run` prints the selection and touches nothing, `--permanent` skips the trash, and `--yes` skips the question, which scripts and piped input need.

```sh
kuizer clean ~/projects --dry-run
kuizer clean ~/projects --select all --permanent
kuizer --json clean ~/projects --yes
```

`kuizer duplicates` lists files with identical contents, largest reclaimable groups first. Files are compared by size, a BLAKE3 hash of the first 16 KiB, then a full hash; `--verify` adds a byte comparison. Hard links to the same file count once, and empty files and generated artifact folders are skipped. It never deletes: you choose which copy to keep.

```sh
kuizer duplicates ~/Downloads --min-size 1MB
kuizer --json duplicates ~/Pictures --verify
```

`kuizer cache` lists global tool caches outside projects: npm, pnpm, yarn, bun, deno, cargo, pip, uv, Go, Gradle, Maven, Playwright and others, each marked `safe` or `review` with a note where removal has a catch. `--clean` removes the selection with the same flags and confirmation as `kuizer clean`.

```sh
kuizer cache
kuizer cache --clean --select npm --select pip
```

kuizer never enters or deletes browser and app profile data. A directory with a Chromium `Local State`, a Firefox `profiles.ini`, or a profile's cookie store is skipped with everything under it, which covers Brave, Chrome, Firefox, Edge and Electron apps such as Discord, Slack and VS Code. The browser directories under `~/.config`, `~/.cache` and `~/.local/share`, Flatpak and Snap browsers, `~/.ssh`, `~/.gnupg` and the keyrings are skipped by path as well. The delete guard checks the same markers again before every removal.

In a terminal, `kuizer scan` opens a full-screen view with a fixed header (scan progress, column names) and footer (totals, keys, position). Rows follow the scan as they arrive; scroll with `j` `k` or arrows, `ctrl-d` `ctrl-u` for half a page, `ctrl-f` `ctrl-b` or `pgup` `pgdn` for a page, `g` `G` for top and bottom. `G` resumes following. `q` leaves the view, cancels a running scan and prints the overview. `--no-live`, `--json` and piped output stay plain text.

Configuration lives in `~/.config/kuizer/config.toml`. Every key is optional; see [docs/config.example.toml](docs/config.example.toml). A directory with a `.kuizerignore` file is skipped with everything under it.

kuizer was called sweep until October 2026; it still skips directories with a `.sweepignore` file and reads `~/.sweep-history.json` until `~/.kuizer-history.json` exists, without deleting the old file.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo run --release --example synth -- /tmp/kuizer-bench 200 100
```

Tests build their trees in temporary directories. The permission tests skip themselves when run as root.

xxx

Remco Stoeten

<small>MIT</small>
