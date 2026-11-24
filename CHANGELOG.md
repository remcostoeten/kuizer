# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The release workflow puts the section that matches the tagged version on the GitHub release.

## [Unreleased]

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

## [0.1.2] - 2026-10-24

- Toolchain versions, Docker and Podman objects, and git repositories with their `git gc` gain and merged branches, in the CLI and the GUI.
- GUI: command palette and full keyboard control, right-click menu, undo after a trash run, space map with zoom, dashboard charts, custom window decorations on macOS and Linux, zoom with Ctrl and the scroll wheel, tooltips, scan progress, shift-select, a new palette and icon, and reopening with the previous scan.
- Faster scan with streaming results in the GUI; Docker queries run side by side.
- Ctrl+Q quits the GUI and the TUI.
- Releases publish to crates.io.

## [0.1.1] - 2026-10-17

- `--idle` for `scan` and `clean`.
- Detection rules for twelve more ecosystems.
- GUI start screen with folder choice and scan options.
- Releases built with cargo-dist: shell installers, an npm package and prebuilt binaries for Linux and macOS.
- CI builds and tests on macOS.

## [0.1.0] - 2026-10-16

First release: `scan`, `clean`, `cache`, `duplicates`, `history` and `restore`, the HTML report and dashboard, a terminal UI and a desktop app, with deletion to the trash and the safety checks.
