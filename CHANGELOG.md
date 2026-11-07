# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The release workflow puts the section that matches the tagged version on the GitHub release.

## [Unreleased]

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
