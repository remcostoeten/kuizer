# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The release workflow puts the section that matches the tagged version on the GitHub release.

## [Unreleased]

- `delete.mode = "trash"` is back as an opt-in, also as a switch in the GUI settings. Deletion stays permanent by default. In trash mode project artifacts and caches go to the trash; toolchains, Docker objects and git branches are still removed for good.
- GUI: a selection with `review` items asks you to type the item count before it is removed, as the CLI does.
- GUI: items can be deleted while a scan is still running. Rows the running scan found are available at once; rows shown from an earlier scan unlock when the scan replaces their list. Deletion runs in the background, its rows dim until they are gone, and the lists stay usable.
- GUI: Shift+click selects every row from the cursor row to the clicked one. A click or Ctrl+click toggles a single row.
- `appearance.accent` picks the accent color of the desktop app: `purple` (default), `blue`, `teal` or `pink`, in both themes. It is also in the settings under General.
- GUI: a count before a motion repeats it (`10j`, `3` then Page Down, `5G` for row 5), and `v` starts visual mode, where every motion selects the rows it passes until Esc.
- GUI: Tab, Left and Right move between Cancel and Delete in the delete confirmation.
- GUI: neutral grey surfaces, an iris accent, 2px and 4px corners, and uppercase monospace labels for column headings and card titles. Text uses Schibsted Grotesk, now bundled with the app, and labels IBM Plex Mono when it is installed. The keyboard cursor row has a stronger tint.
- GUI: holding Enter no longer accepts a delete confirmation; only a fresh key press does.
- GUI: deleting animates. Marked, confirming and deleting rows fade into each other, and removed rows fade out instead of vanishing. Instant with `appearance.motion = false`.
- GUI and TUI: the status bar always shows the current mode (Normal, Visual, Filter, Confirm and so on), with a typed count next to it in the GUI.
- GUI: a control's tooltip also opens when the keyboard focus ring rests on it. Tab moves between the buttons of the branch-deletion dialog.
- GUI: the Overview has a "Biggest safe wins" card. `w` selects the largest safe project artifacts and opens their list, without deleting anything.
- `appearance.density` is `comfortable` (default) or `compact`, also in the settings. Rows and vertical padding are tighter than before in both.

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
