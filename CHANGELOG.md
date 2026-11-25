# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The release workflow puts the section that matches the tagged version on the GitHub release.

For each release, record the uncompressed sizes of `kuizer` and `kuizer-gui` for `x86_64-unknown-linux-gnu`. Measure the packaged executables in decimal MB (1 MB = 1,000,000 bytes).

## [Unreleased]

- GUI: a Keys page in the settings lists every action by its `[keys]` name with its current keys, where it works and its defaults, and filters on name or key with `/`. Enter records the next key combination as the action's key, `a` adds one, `x` or Delete removes the picked key (Left and Right pick it) and `r` goes back to the defaults; Esc while recording changes nothing. An invalid key or one another action in the same scope already uses is refused next to the row and the old keys stay. Changes are written to `[keys]`, so the TUI uses them too, and work in the GUI at once.
- TUI: views on `5` to `7` for toolchains, Docker and Podman objects and git repositories (`view.toolchains`, `view.docker`, `view.repos`). Toolchains and Docker objects are selected and deleted like caches and always removed for good. The git view shows what `git gc` would free and the merged branches per repository; `c` and `C` run `git gc`, `b` and `B` delete merged branches after a confirmation that opens on Cancel (`git.gc`, `git.gc_all`, `git.branches`, `git.branches_all`). A selection with review items now asks for its item count to be typed in the TUI, and the help overlay lists the git and confirmation keys.
- TUI: views on `1` to `4` for artifacts, global caches, duplicates and scan issues, with the keys in the shared `[keys]` table (`view.artifacts`, `view.caches`, `view.duplicates`, `view.issues`). Caches can be selected and deleted like artifacts, each view with its own selection; duplicates and issues are reports. Caches and duplicates are only looked for once their view opens.
- GUI: the confirmation before merged git branches are deleted is no longer a centered modal. It rises in over the bottom of the list like the selection summary, opens with the focus on Cancel so an Enter right after asking deletes nothing, takes Tab, arrows and `hjkl` to move between Cancel and Delete, closes with Esc, and sinks away once answered.
- `appearance.zoom` holds the GUI interface size (50 to 200 percent). The zoom shortcuts and a new Interface size row in the settings write it to the config file, and the size saved by earlier versions moves there on first launch. The GUI settings gained Scanning, Duplicates, Git and Tools pages for scan roots, excludes, size thresholds, thread counts, `duplicates.*`, `git.*`, `toolchains.*` and `docker.*`; a typed value the config would refuse is reported next to the field and the old value stays.
- CLI: `scan`, `clean`, `cache`, `toolchain` and `docker` take repeatable `--rule`, `--project` and `--exclude` filters, and `cache`, `toolchain` and `docker` also `--category` and `--min-size`, before or after `clean`. Flags only narrow the config. `--json-lines` prints one candidate per line as the scan finds it, then a `"type": "summary"` line, for `scan` and the `cache`, `toolchain` and `docker` listings. The hidden `kuizer generate completions SHELL` and `kuizer generate man [--out DIR]` print shell completions and man pages for packagers.
- CLI: `--json-lines` also works on `clean`, `cache clean`, `toolchain clean` and `docker clean`. Each item is printed as a `"type": "removal"` line with its status as soon as it is removed or has failed, then a `"type": "summary"` line with the rest of the `--json` removal report; `--dry-run` prints every selected item as `would-remove`. Confirmation behaves as with `--json`. `docs/report.schema.json` describes the new `removalLine`.
- The CLI, the TUI and the GUI delete through one executor in the core, which revalidates each item, removes it, reports per item and records the history, one run per mode. When to ask before deleting (`review` items, projects with uncommitted changes, `delete.confirm`) is decided in the core for the CLI and the TUI. `kuizer clean`, `kuizer cache` and the GUI now remove files in parallel like the TUI, never two nested paths at once; toolchains and Docker objects still go one at a time.
- Packaging: the kuizer repository is a Homebrew tap (`brew tap remcostoeten/kuizer https://github.com/remcostoeten/kuizer`) with a formula for `kuizer` in `Formula/` and a cask for `kuizer-gui` in `Casks/`, which every release rewrites to point at its archives. `packaging/aur` holds PKGBUILDs for `kuizer` (built from source) and `kuizer-bin` (release binaries), both with the desktop app, its desktop file and icons. The formula and both PKGBUILDs install bash, zsh and fish completions and the man pages.
- Caches and toolchains are sized on one shared pool of `scan.size_threads` threads instead of one thread per item, and `scan.exclude` now also hides caches and toolchains in `kuizer cache`, `kuizer toolchain` and the dashboard, as it already did in the GUI. The GUI shows item ages in the same buckets as the TUI (`today`, `5d`, `3w`, `4mo`, `2y`), and which cleanups can go to the trash is decided in one place for both.
- `kuizer history`, the dashboard, `kuizer restore` and the GUI read what is restored and still in the trash through one core snapshot. A cleanup that was put back completely now shows `restored` in the `kuizer history` trash column instead of `emptied`, and "still in the trash" leaves out restored items. The GUI restores from a single trash listing and updates its history in memory instead of reading the history file again, and works out trash states and totals on the history thread instead of while drawing.
- GUI: after a cleanup the history tab adds the cleanup the core just recorded instead of reading the history file again, and keeps its trash states and totals current without listing the trash. It still reads the file again when the history could not be written.
- GUI: every list tells a scan that found nothing apart from filters that hide every row, instead of showing a blank table, and a failed scan offers Rescan and New scan with their keys. Both states fade in, instantly with `appearance.motion = false`.
- GUI: keys stay responsive on large scans. Stopping a scan keeps the results on screen as they are instead of filtering and rebuilding them, a saved scan, a duplicate search and saved duplicates leave out excluded paths on their worker thread, the chosen folder is resolved off the UI thread, the cursor in every list is found from its last row instead of a search through the list, and the command palette ranks and checks its commands once per query.
- GUI: motion durations live in one place and every state change animates in 150 ms or less; zooming the space map got faster (180 to 140 ms). Sidebar icons no longer move on hover, and bar slivers keep the same minimum size in every view.
- Permanent deletion empties read-only folders the user owns, such as the Go module cache, instead of failing on them. Folders owned by another user are still refused.
- `delete.mode = "trash"` is back as an opt-in, also as a switch in the GUI settings. Deletion stays permanent by default. In trash mode project artifacts and caches go to the trash; toolchains, Docker objects and git branches are still removed for good.
- GUI: deleting no longer asks. While anything is selected, a floating bar above the status bar sums up the selection (count, size, `review` items, and whether it is deleted for good or moved to the trash) with Clear and Delete. Enter, Delete, `d`, the Delete button and the row menu delete at once; a held key never does.
- GUI: the Duplicates, History and Issues lists are virtualized like the others and are no longer capped at 200 groups or 500 issues.
- GUI: deleted items no longer come back from the saved scan on the next launch.
- GUI: without a working GPU driver (no Vulkan), the app prints why it cannot start and points to the `kuizer` terminal UI instead of panicking.
- GUI: Enter, `d` and Delete act only on a selection. With nothing selected they remove nothing; a single row is deleted through its menu.
- GUI: settings are written one at a time, latest value first, and a failed write never undoes a newer change. A bad `scan.exclude` pattern no longer turns off the valid ones for caches and toolchains.
- TUI: holding Enter no longer confirms a deletion.
- `--idle` refuses ages too large to compute, and a project with an unreadable folder counts as unknown and never matches.
- Deletion refuses items the scan could not identify and parent folders that became symlinks. Branch deletion only succeeds while the branch still points at the checked commit. Volta removal keeps every other protection.
- History keeps non-UTF-8 paths, retries a failed write in the same run, and Ctrl-C during a removal finishes the current item and saves the history. A second Ctrl-C exits at once.
- A config that cannot be read falls back to cautious deletion with a warning.
- GUI: items can be deleted while a scan is still running. Rows the running scan found are available at once; rows shown from an earlier scan unlock when the scan replaces their list. Deletion runs in the background, its rows dim until they are gone, and the lists stay usable.
- GUI: Shift+click selects every row from the cursor row to the clicked one. A click or Ctrl+click toggles a single row.
- `appearance.accent` picks the accent color of the desktop app: `purple` (default), `blue`, `teal` or `pink`, in both themes. It is also in the settings under General.
- GUI: a count before a motion repeats it (`10j`, `3` then Page Down, `5G` for row 5), and `v` starts visual mode, where every motion selects the rows it passes until Esc.
- GUI: neutral grey surfaces, an iris accent, 2px and 4px corners, and uppercase monospace labels for column headings and card titles. Text uses Schibsted Grotesk, now bundled with the app, and labels IBM Plex Mono when it is installed. The keyboard cursor row has a stronger tint.
- GUI: deleting animates. Marked and deleting rows fade into each other, and removed rows fade out instead of vanishing. Instant with `appearance.motion = false`.
- GUI and TUI: the status bar always shows the current mode (Normal, Visual, Filter, Confirm and so on), with a typed count next to it in the GUI.
- GUI: a control's tooltip also opens when the keyboard focus ring rests on it. Tab moves between the buttons of the branch-deletion dialog.
- GUI: the Overview has a "Biggest safe wins" card. `w` selects the largest safe project artifacts and opens their list, without deleting anything.
- `appearance.density` is `comfortable` (default) or `compact`, also in the settings. Rows and vertical padding are tighter than before in both.
- GUI: Space moves the cursor on to the next unselected row, so it can be pressed again and again. Selected rows have a softer tint.
- GUI: Select all takes `safe` items first, a further press adds `review` items, and the next clears. A list with only `review` items asks first, and the status bar shows when `review` items are selected.
- GUI: vim motions work in the space map, the shortcuts overlay, the branch dialog and the focus ring.
- GUI: saved scans and their duplicate results are pruned as pairs. Orphaned results and stale temp files are removed.
- GUI: recent folders keep non-UTF-8 paths, and `xdg-open` can no longer hang a reveal.
- TUI: Esc backs out one level and never quits. Quitting during a deletion asks first.
- TUI: `P` selects a project's `safe` items first, a second press adds `review` items, and a third clears.
- CLI: `cache clean`, `toolchain clean` and `docker clean` replace `--clean`, which stays as a hidden alias for one release.
- CLI: `delete.confirm = false` is honored, except for `review` items and projects with uncommitted changes or an unknown git state.
- CLI: `--category` and `--min-size` narrow the config instead of replacing it. `history` and `restore` honor `--config`.
- `--idle` counts activity in nested projects toward their monorepo root.
- Removal errors and other external text can no longer inject terminal escape sequences.
- A permission error on one folder no longer turns off `openat2` for the rest of the deletion.
- A failed thread spawn no longer aborts the app, and failed repository inspections are reported.
- `scan.size_threads` above 64 is capped with a warning, and `docs/config.example.toml` lists every key.
- Release binary sizes: `kuizer` 5.5 MB, `kuizer-gui` 19.8 MB.

## [0.1.3] - 2026-11-07

- GUI: settings in the sidebar footer and behind Ctrl+, with a light theme, a motion switch and sidebar options; one toolbar per section; Shift+1 to Shift+9 switch sections; Escape stops a scan and keeps the results found so far; tooltips anchor to their trigger.
- Documentation at kuizer.app/docs.
- Deletion is always permanent. Trash mode, the `delete.mode` key, `--permanent`, the TUI `t` toggle and the GUI undo bar are gone; `delete.confirm` now defaults to `true`. `kuizer restore` still brings back runs that older versions moved to the trash.
- A folder your user cannot remove, such as a `node_modules` installed with `sudo`, is refused before anything is touched, with the owner and how to remove it instead of a raw permission error.
- The packaged `kuizer` binary is 5.15 MB and `kuizer-gui` is 17.59 MB.

## [0.1.2] - 2026-10-24

- Toolchain versions, Docker and Podman objects, and git repositories with their `git gc` gain and merged branches, in the CLI and the GUI.
- GUI: command palette and full keyboard control, right-click menu, undo after a trash run, space map with zoom, dashboard charts, custom window decorations on macOS and Linux, zoom with Ctrl and the scroll wheel, tooltips, scan progress, shift-select, a new palette and icon, and reopening with the previous scan.
- Faster scan with streaming results in the GUI; Docker queries run side by side.
- Ctrl+Q quits the GUI and the TUI.
- Releases publish to crates.io.
- The packaged `kuizer` binary is 5.15 MB and `kuizer-gui` is 17.31 MB.

## [0.1.1] - 2026-10-17

- `--idle` for `scan` and `clean`.
- Detection rules for twelve more ecosystems.
- GUI start screen with folder choice and scan options.
- Releases built with cargo-dist: shell installers, an npm package and prebuilt binaries for Linux and macOS.
- CI builds and tests on macOS.
- The packaged `kuizer` binary is 4.79 MB and `kuizer-gui` is 16.33 MB.

## [0.1.0] - 2026-10-16

First release: `scan`, `clean`, `cache`, `duplicates`, `history` and `restore`, the HTML report and dashboard, a terminal UI and a desktop app, with deletion to the trash and the safety checks.

- The packaged `kuizer` binary is 4.71 MB and `kuizer-gui` is 16.26 MB.
