# Phase plans

[`design.md`](../design.md) says what kuizer does. These documents say how to build the phases that are still open: which files change, which decisions are already made, how each phase is tested and when it counts as done. Each plan is written against the code as of `998d87e` and lands as one pull request unless it says otherwise.

| Phase | Plan | Depends on |
| --- | --- | --- |
| 2 to 5 | [Closing out the TUI, selection, deletion and duplicates](02-05-closeout.md) | none |
| 6 | [More rules, `--json-lines` and the JSON reference](06-rules-and-json-lines.md) | none |
| 7 | [Benchmarks, fuzzing, macOS and Windows](07-benchmarks-and-platforms.md) | none |
| 8 | [Shared filters, idle time and `cache clean`](08-filters-and-idle.md) | 7 for the idle cost numbers |
| 9 | [History ids and restore](09-history-and-restore.md) | none |
| 10 | [Toolchain, Docker and big-file sources](10-sources.md) | 8 |
| 11 | [Policies](11-policies.md) | 8, and 10 for non-project policies |

Order: 2 to 5 closeout, 6 and 9 can run in parallel, then 7, 8, 10, 11. Phase 9 is independent of the filter work and gives the dashboard its first action, so it is the best next phase.

Every phase follows the rules that already hold for the codebase:

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass, as in `.github/workflows/ci.yml`.
- Destructive tests run in a `tempfile::TempDir`, never against the real home directory. Tests that need a home set `HOME` (and `XDG_DATA_HOME` for the trash) on the spawned `kuizer` process, as `tests/clean.rs` does.
- New JSON fields are additive within `schema_version: 1`. A rename or removal bumps `SCHEMA_VERSION` in `src/output/json.rs`.
- New commands get a help example block in `src/cli.rs` and an entry in `COMMAND_GROUPS`.
- The dashboard (`src/output/report.html`) gains a tab or a column when a phase adds data worth seeing. Each plan names what that is.
