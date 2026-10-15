# Phases 2 to 5: closeout

Status: plan. The features of phases 2 to 5 exist; their acceptance criteria in `design.md` section 10 are not all verified, and the TUI key map has drifted from the table in section 7.

## Key map drift

`src/tui/mod.rs` `handle_key` is the source of truth. Differences from `design.md` section 7:

| Key | Design | Code |
| --- | --- | --- |
| `p` | pause or resume the scan | toggle grouping by project |
| `P` | not listed | select the whole project under the cursor |
| `x` | exclude path, write to config | reset filters |
| `f` | not listed | toggle safe-only filter |
| `c` | select or clear the current category | cycle the category filter |
| `D` | toggle permanent for the next dialog | `t` inside the confirm dialog |
| `A` | select everything, after a confirmation | missing |
| `1` to `4` | views: artifacts, caches, duplicates, issues | missing |
| `enter` / `l` | expand detail | `enter` deletes the selection |

Decision: keep the code's bindings, since they are in use, and change the design table to match. The missing features get free keys:

| Feature | Key | Notes |
| --- | --- | --- |
| Select all in view, after confirmation | `A` | Reuses `Overlay::Confirm` with a "select N items including M review items" message instead of a delete. |
| Exclude path | `e` | Opens a dialog with the path and the glob it will add (`~/apps/old/node_modules`). `y` appends it to `scan.exclude` through a new `config::append_exclude(path, glob)` that edits the TOML file in place with `toml_edit`, so comments survive. The item leaves the list. |
| Pause scan | `ctrl-p` | Sets a pause flag the walker checks next to the cancel flag in `scan/walk.rs`. |
| Views | `1` artifacts, `2` issues | Caches and duplicates stay in their own commands and the dashboard. A caches view in the TUI waits for phase 10's source abstraction, which gives every source the same list. |

`toml_edit` is a new dependency. `toml` alone would rewrite the file and drop the user's comments.

## Acceptance checks still open

| Phase | Criterion | How to verify |
| --- | --- | --- |
| 2 | First result visible while scanning | Unit test on `App`: push one candidate before `Event::Finished`, assert `refresh` lists it. |
| 2 | Input latency under 50 ms during a 1M entry scan | Needs the phase 7 harness. Run `examples/synth.rs` with 1M entries, drive the TUI through a pty (`expectrl` dev dependency) and time key to redraw. Record the number in `design.md` section 9. |
| 2 | Terminal restored on panic and `ctrl-c` | `ratatui::init` installs the panic hook. Add a pty test that sends `ctrl-c` mid-scan and asserts the alternate screen is left (`\x1b[?1049l` in the output). |
| 3 | Selection survives re-sorting and filtering | Extend `sorts_by_size_and_keeps_cursor_on_item` in `src/tui/app.rs` to select two items, cycle sort and filter, and assert both stay selected. |
| 3 | Totals match the JSON | Integration test: `kuizer --json scan` total equals the sum `App` shows after the same scan (run `App` headless on `scan::collect`). |
| 3 | Preview lists exactly the selection | Unit test on `Overlay::Confirm` contents. |
| 4 | Dry run touches nothing, by tree snapshot | Done in `tests/clean.rs` `dry_run_touches_nothing`. |
| 4 | Guard and revalidation | Done in `tests/delete.rs`. Mount point refusal is untested; add one gated on Linux with a bind mount when running as root in CI, skipped otherwise. |
| 5 | Zero-length files, same size different content | Add both cases to `tests/dupes.rs`. Zero-length files are below every `min_size`, so the test sets `min_size = 0` and asserts they are never grouped. |

When every row passes, mark phases 2 to 5 as done in `design.md` section 10 and update the status line at the top.
