# Kuizer Status

This file is the current implementation status for Kuizer.

It is meant to answer four questions quickly:

1. What is already built and working?
2. What has been verified?
3. What is partially done or currently in progress?
4. What should be built next?

## Current Baseline

Latest commit:
- `f5cc745` — `Add dead-code detect MVP pipeline`

Working tree after that commit:
- There are follow-up detector/framework improvements beyond `f5cc745`.
- There are also unrelated untracked/local files in the repo that are not part of the MVP status.

## What Is Fully Done

### CLI detect pipeline

Implemented:
- `kuizer detect`
- `--config`
- `--tsconfig`
- `--reporter`
- `--only`
- `--skip`
- `--json`
- exit code handling

Code:
- [src/cli/index.ts](/home/remco/dev/kuizer/src/cli/index.ts)

What it does:
- loads config
- bootstraps the engine
- runs selected detectors
- runs the chosen reporter
- exits with `1` when any `error` severity finding exists, otherwise `0`
- exits with `2` on execution/setup failure

### Config loading

Implemented:
- config discovery
- explicit config path loading
- relative path rebasing from the config file location
- framework preset merge

Code:
- [src/config/index.ts](/home/remco/dev/kuizer/src/config/index.ts)
- [src/config/defaults.ts](/home/remco/dev/kuizer/src/config/defaults.ts)
- [src/config/presets.ts](/home/remco/dev/kuizer/src/config/presets.ts)

Important behavior:
- resolved config values are treated as absolute filesystem paths after loading

### Engine bootstrap

Implemented:
- `ts-morph` project bootstrap from `tsconfig`
- source file collection from the `tsconfig` program, not a repo-wide glob crawl
- entrypoint resolution from explicit paths or glob patterns
- fallback root inference when no configured entries resolve
- BFS reachability traversal over static imports
- default exclusion of `*.test.*` and `*.spec.*` files from the analysis set

Code:
- [src/engine/index.ts](/home/remco/dev/kuizer/src/engine/index.ts)

Important behavior:
- repo-root `unusedFiles` runs no longer flag the entire Kuizer source tree just because no entrypoint was configured

### Reporters

Implemented:
- terminal reporter
- JSON reporter
- version metadata for bundled and source execution

Code:
- [src/reporters/terminal.ts](/home/remco/dev/kuizer/src/reporters/terminal.ts)
- [src/reporters/json.ts](/home/remco/dev/kuizer/src/reporters/json.ts)
- [src/reporters/registry.ts](/home/remco/dev/kuizer/src/reporters/registry.ts)
- [src/meta.ts](/home/remco/dev/kuizer/src/meta.ts)

### Detectors

Implemented and working:
- `unusedFiles`
- `unusedExports`

Code:
- [src/detectors/unused-files.ts](/home/remco/dev/kuizer/src/detectors/unused-files.ts)
- [src/detectors/unused-exports.ts](/home/remco/dev/kuizer/src/detectors/unused-exports.ts)

Current `unusedExports` behavior:
- inspects exports only in reachable files
- counts only references from reachable consumers
- handles named re-export chains
- handles `export *` chains
- handles default export aliases through barrels
- ignores re-export syntax as a real consumer by itself
- suppresses framework-consumed default exports for Next entry files
- respects `allowedExports`
- respects `ignore.symbols`
- returns sane line/column locations
- emits minimal machine-readable `context`

### Test fixture

Implemented:
- deterministic sample project fixture checked into the repo

Code:
- [tests/fixtures/sample/kuizer.config.json](/home/remco/dev/kuizer/tests/fixtures/sample/kuizer.config.json)
- [tests/fixtures/sample/tsconfig.json](/home/remco/dev/kuizer/tests/fixtures/sample/tsconfig.json)
- [tests/fixtures/sample/src/index.ts](/home/remco/dev/kuizer/tests/fixtures/sample/src/index.ts)

Purpose:
- stable end-to-end verification without depending on generation scripts

## What Has Been Verified

These checks passed on the current code while building the MVP:

- `bun test`
- `npm run typecheck`
- `npm run build`

Verified CLI behavior:
- `node dist/index.js detect --reporter json --only unusedFiles`
  - returns `0` findings for the Kuizer repo root instead of flooding on the entire source tree
- `node dist/index.js detect --config tests/fixtures/sample/kuizer.config.json --reporter json --only unusedFiles,unusedExports`
  - returns the expected fixture findings

Expected fixture findings:
- `unusedFiles`
  - `src/components/Button.ts`
  - `src/components/Modal.ts`
  - `src/utils/format.ts`
  - `src/utils/math.ts`
- `unusedExports`
  - `shout`
  - `FileLogger`
  - `Timestamp`
  - `PaginatedResponse`

## What Is In Progress

These changes exist locally after `f5cc745` and are working, but are not committed yet:

- re-export-aware reachability in the engine
- framework-aware default export suppression for Next entry files
- richer finding metadata via `context`

Files:
- [src/engine/index.ts](/home/remco/dev/kuizer/src/engine/index.ts)
- [src/detectors/unused-exports.ts](/home/remco/dev/kuizer/src/detectors/unused-exports.ts)
- [src/types.ts](/home/remco/dev/kuizer/src/types.ts)

## Known Limitations

These are not bugs in the current MVP so much as unimplemented scope:

- no namespace import property-access tracing
- no broader framework-aware export exceptions beyond the current Next entry/default and allowlist handling
- no dynamic import boundary handling
- no fixer, rewrite, backup/restore, watch mode, SARIF, HTML reports, CI command, or plugin system
- no monorepo-specific handling
- no transitive stabilization pass for export deadness

## Next Recommended Work

### 1. Extend `unusedExports` beyond current graph cases

Priority: high

Why:
- this detector is the most valuable next source of false positives or false negatives

Next tasks:
- handle namespace imports and property-access tracing
- handle more framework-specific opaque consumption cases
- add more mixed barrel/default/type re-export cases

### 2. Expand framework-aware export handling

Priority: high

Why:
- Next.js framework-consumed exports are called out as core spec behavior

Next tasks:
- keep Next.js preset coverage aligned with the spec
- expand beyond current default-export and allowlist behavior where needed

### 3. Improve detector and finding model further

Priority: medium

Why:
- the system is still detector-minimal and finding metadata is thin

Next tasks:
- add richer context on findings
- make source locations consistent across detectors
- clean up detector implementation typing and internal utilities

### 4. Implement next MVP detectors or outputs

Priority: medium

Most likely next:
- better `unusedExports`
- then possibly `unusedTypes` or fixer groundwork

## Notes For Future Work

- Keep tests in `__tests__/` rather than next to implementation files.
- Keep fixture-based e2e coverage for graph behavior.
- Prefer engine behavior that follows the `tsconfig` program instead of broad filesystem scans.
- Treat “reachable graph” as the source of truth when evaluating whether a consumer should count.
