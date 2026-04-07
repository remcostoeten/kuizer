# Audit fix plan

This plan prioritizes the repository audit findings by user risk and
implementation dependencies.

## Phase 1: Safety and correctness

Fix these first because they can corrupt workspaces or miss dead code.

- Harden snapshot and restore path handling in `src/backup/create.ts` and
  `src/backup/restore.ts`.
- Preflight every restore source before copying any file.
- Fix `unusedExports` consumer counting so unused imports do not keep exports
  alive.
- Wire `detectTsConfig()` into `loadConfig()` so documented tsconfig discovery
  is real.
- Add runtime validation for raw config values before merging defaults.
- Stop detect progress in a `finally` block so unexpected errors cannot leave
  the spinner interval active.
- Fix `scripts/test-runner.sh` so a failed `bun test` process always fails the
  wrapper.

## Phase 2: CLI and fixture integrity

Fix these next because they affect public behavior and test confidence.

- Reject conflicting `kuizer fix --write --dry-run` invocations.
- Correct `__tests__/fixtures/sample/kuizer.config.json` or intentionally
  support the wrapped `{ "kuizer": ... }` shape.
- Add CLI tests for command rewriting, aliases, empty flag values, `init`,
  `revert`, reporter selection, and dry-run/write precedence.
- Add an integration test that loads the sample fixture through `loadConfig()`
  or `main(["detect", "--config", ...])`.
- Fix `scripts/scaffold-e2e-tests.sh` so generated fixture output matches the
  committed fixture.

## Phase 3: Performance and structure

Address these after correctness is covered.

- Avoid recomputing the full unused-export analysis once per split detector.
- Decide whether `unusedExports` is a public aggregate detector or only an
  internal compatibility export.
- Rename `UnusedExportDetectorKey` or split it into export and non-export
  detector key unions.
- Delete leftover moved-file stubs such as `src/detectors/fixture.ts`.
- Split oversized modules such as `src/cli/index.ts`,
  `src/cli/interactive.ts`, and `src/detectors/unused-exports.ts` along
  cohesive boundaries.

## Phase 4: Accessibility, output, and tooling

Handle these once the core behavior is stable.

- Add a plain or reduced-motion alternative for interactive mode, or document
  the visual TUI limitation clearly.
- Recompute the interactive viewport on terminal resize.
- Render terminal reporter paths relative to `cwd` and include columns.
- Add `bun run lint`, `bun run fmt:check`, and `bun run build` to CI.
- Pin the CI Bun version instead of using `latest`.
- Ignore generated local artifacts such as `test-results/`, and remove the
  typo config `.oxfmtrc.jsonn`.

## Suggested branch sequence

Use small branches in this order:

1. `fix/restore-safety`
2. `fix/config-resolution-validation`
3. `fix/detector-consumer-counts`
4. `fix/cli-test-runner`
5. `chore/lint-ci-cleanup`
6. `refactor/export-detector-cache`
7. `feat/interactive-accessibility`
