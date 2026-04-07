# Kuizer Roadmap

## Overview

Kuizer is a CLI tool that analyzes TypeScript/JavaScript codebases to find unused exports and dead code, with automatic fix capabilities.

## Status

Most Phase 1-4 items from the original audit plan are complete. Below is the current roadmap.

## In Progress

- **refactor/unused-exports**: Split the large unused-exports detector into separate modules for export collection, consumer graph, classification, and detector adapters.

## Backlog

- Add `--plain` flag to interactive mode for accessibility
- Wire `detectTsConfig()` into `loadConfig()` for documented tsconfig discovery
- Add runtime validation for raw config values before merging defaults

## Completed (Recent)

- Split CLI modules (parser/dispatch/commands) for safer future changes
- Added reduced-motion/plain mode and terminal resize handling to interactive TUI
- Cleaned up 1322 lint warnings to 0
- Fixed CLI command dispatch and flag validation

## Architecture

```
src/
├── cli/           # CLI entry points
│   ├── index.ts   # Entry point
│   ├── dispatch.ts
│   ├── commands.ts
│   ├── parser.ts
│   ├── interactive.ts
│   └── help.ts
├── detectors/     # Code analysis detectors
├── fixers/        # Automatic fix planning and application
├── reporters/     # Output formatters (terminal, json)
├── config/        # Configuration loading
├── engine/        # Analysis bootstrap
├── backup/       # Snapshot/restore
└── output/       # Terminal output utilities
```
