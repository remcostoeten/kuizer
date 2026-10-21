# kuizer

kuizer is a keyboard-driven disk usage explorer and cleaner for macOS and Linux. It finds what can be removed and regenerated (dependency folders, build output, tool caches, temp files, duplicates) and deletes only what the user selects.

## Where the work is

The Rust core in `src/` (scan, detect, delete, config, history) powers two frontends:

- `crates/kuizer-gui`: the GPUI desktop app. This is the current focus.
- `src/tui`: the Ratatui terminal UI, part of the `kuizer` CLI.

The CLI and TUI are the foundation and must keep working. A GUI change that breaks the TUI or the CLI is not done.

Logic belongs in the core. Frontends render state and dispatch actions; they never walk the filesystem, match rules or delete files themselves. If the GUI needs something the core does not expose, add it to the core with tests, then use it from both frontends.

`docs/design.md` holds the product spec and module boundaries. Read it before touching the core.

`site/` is the marketing site (Next.js). It is far from done and has its own conventions. Leave it alone unless the task is about the site.

## Stack

- Rust 2024, MSRV 1.88, one Cargo workspace (`kuizer` at the root, `crates/kuizer-gui`).
- GPUI is pinned at `=0.2.2`. Its docs are thin: read the source in `~/.cargo/registry/src/*/gpui-0.2.2` instead of guessing APIs.
- Platforms are macOS and Linux. Development and testing happen on Arch with KDE Plasma on Wayland, but the app must work across distros, desktops (GNOME, KDE, tiling window managers) and on X11. Do not rely on KDE-specific behavior or on tools only Arch ships. Do not add Windows code paths.

## Configuration

Anything a user could reasonably have a strong preference about is a setting: deletion behavior, keybindings, appearance and scan scope. When adding such a feature, add its config key in the same change. Internal tuning values (paddings, buffer sizes, animation curves) are not settings; keep them as constants.

- Config lives in `~/.config/kuizer/config.toml`, parsed by `src/config`. `docs/config.example.toml` lists every key and must be updated with each new one.
- Defaults are conservative; config can make them aggressive. Examples: `delete.mode` switches between trash and instant permanent deletion, `delete.confirm` turns the confirmation step off, scan roots, exclusions, thresholds and thread counts are all keys.
- Keybindings are configurable. Every action has a stable name (`select.toggle`, `delete.selected`, `nav.down`), a default binding and an override in config. Both frontends read the same keymap from the core, so a rebind applies to the GUI and the TUI. This shared keymap does not exist yet: today the bindings are hardcoded per frontend. Build the keymap in `src/` before adding more bindings, and move existing ones into it when touching them.
- Appearance is configurable: theme, accent, density, motion on or off.
- The GUI exposes settings in a settings view that reads and writes the same config file. No setting exists only in the GUI.
- Invalid config never crashes the app. Report the bad key, fall back to the default, keep running.

## Safety

Deletion is the one thing this app must never get wrong.

- Scanning never deletes. Deletion goes through `src/delete`, which revalidates every item before removing it.
- The defaults are trash mode with a confirmation step that shows item count and total size. A user may switch to instant permanent deletion through config; respect that choice without nagging.
- `src/protect.rs` refuses system paths, the home directory itself and mount points. Config can add protected paths but cannot remove the built-in ones.
- Tests touch only temp directories (`tempfile`). Never run a deleting test against a real path.

## Speed

The app should feel instant. Every frame and millisecond counts.

- A keypress produces a visible response in the same frame. Nothing on an input path blocks on I/O.
- Scanning and sizing run off the UI thread and stream results in. Show progress, never a blocking spinner.
- Long lists are virtualized. Navigation, filtering and selection cost the same with 1,000 or 1,000,000 entries.
- Cold start reaches an interactive window with no loading screen.
- Claim a speedup only with a measurement. `cargo run --release --example synth` generates fixture trees for this.

## Keyboard

Every action works without a mouse.

- Tab and shift-tab move focus, arrow keys and vim motions (`hjkl`, `gg`, `G`, `ctrl-d`, `ctrl-u`) navigate everywhere, including dialogs and settings.
- `/` filters, `space` toggles selection, `enter` confirms or opens, `esc` backs out exactly one level.
- While a text input has focus (filter, search, settings fields), keys type text and motions are off. `esc` leaves the input and returns to normal mode.
- A command palette lists every action with its current binding.
- Focus is always visible, and the current mode (normal, filter, confirm) is always shown.
- A feature without a keybinding is not finished.

## Look and feel

The look is a feature, not polish added at the end.

- GUI colors, spacing and radii come from `crates/kuizer-gui/src/theme.rs`. No hardcoded values in views.
- The TUI follows the same palette and hierarchy, adapted to the terminal.
- Motion shows state changes only: selection, expand and collapse, an item leaving after deletion, a view appearing. Keep it short (under 150ms) and never animate in response to typing or plain navigation.
- Stateful behavior transitions instead of jumping. When an item moves from idle to marked to confirming to deleting to gone, each step animates from the previous one, and nothing pops in or out.
- Confirmations and other follow-up actions happen inline, in place, as motion. A delete confirm expands in the row or the footer where the action was triggered; it is not a modal or popup. Reach for a modal only when there is no sensible place inline, and justify it in the summary.
- Motion can be turned off in config (`ui.motion`). GPUI 0.2.2 cannot read the system reduced-motion setting, so this config key is the only switch; do not build OS detection. With motion off, state changes still happen, just instantly.
- Consistency over novelty. A new view reuses existing components and spacing before it introduces its own.

## Testing

Test what can break in our own code, and nothing else. A test has to earn its place.

- Test behavior that would hurt if it regressed: deletion and its guards, detection rules, config parsing and fallbacks, scan correctness, keymap resolution.
- Do not test our dependencies. GPUI, Ratatui, serde, toml, trash, blake3 and the rest are tested by their maintainers. A test that only proves a crate does what its docs say is redundant.
- Do not write tests for coverage, for trivial getters, for constructors, or that restate the implementation line by line.
- One focused test beats five that check the same path with different literals.
- When fixing a bug, add a test only if the bug could plausibly come back.
- Delete tests that no longer guard anything real.

## Dependencies and build size

The binary should stay as small as possible and builds as fast as possible.

- Add a dependency only when writing it ourselves would be clearly worse. A few dozen lines of our own code beat a new crate.
- Before adding a crate, check what it pulls in (`cargo tree -p <crate>`). Avoid crates with heavy transitive trees or proc-macro chains.
- Disable default features and enable only what is used, as the existing `Cargo.toml` entries do.
- Prefer crates already in the tree over new ones that do the same job.
- Keep the core free of GUI dependencies. The `kuizer` crate must build without GPUI.
- Do not add dev-dependencies for test convenience when the standard library or `tempfile` covers it.
- When a change noticeably grows the release binary or the clean build time, say so in the summary with the before and after.

## Commands

`./dev` is an interactive menu for all of these.

- Run the GUI: `cargo run -p kuizer-gui -- <folder>`
- Run the CLI: `cargo run -p kuizer -- <args>`
- Test: `cargo test --workspace --all-targets -j 2 -- --test-threads=2`
- Check what CI checks: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`

This machine has 8 GB of RAM. Run one build or test at a time, with at most 2 jobs.

Before calling a task done, build and test both packages, and run clippy with no warnings.
