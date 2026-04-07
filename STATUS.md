# Kuizer status

This file summarizes the current implementation in the working tree.

## Implemented

Kuizer currently ships:

- A dead-code analysis engine built on `ts-morph`
- Detectors for `unusedFiles`, `unusedImports`, and split unused export
  categories
- Fix planning and application for supported dead-code cases
- Snapshot creation and restore
- Terminal and JSON reporting
- An interactive terminal UI for browsing findings and applying supported
  fixes
- CLI commands for `detect`, `fix`, `restore`, `init`, and `revert`
- Global and project config discovery, including `--tsconfig` overrides

## Known limitations

Current gaps include:

- No broader style-refactor engine
- No auto-fix for default exports
- No auto-fix for barrel export declarations
- No auto-fix for grouped variable export statements
- No auto-fix for unused imports
- Interactive mode has no code preview, editor jump, or text search

## Health checks

Use these commands to verify the repository:

```bash
bun run typecheck
bun run test
bun run build
```
