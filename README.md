# Kuizer

Kuizer is a Bun-based CLI for finding and removing dead code in TypeScript
projects. It analyzes your import graph, reports unreachable files and
unused exports/imports, and can apply supported fixes with snapshot-based
rollback.

## Current scope

Kuizer currently implements dead-code analysis and cleanup. It does not
implement the broader style-refactor workflow described in older project
notes.

Implemented today:

- Detect unreachable source files.
- Detect unused imports and unused exports in reachable files.
- Split unused export findings by functions, types, classes, components,
  constants, enums, defaults, and other exports.
- Apply supported fixes for unused files and simple unused export
  declarations.
- Create snapshots before writes and restore them later.
- Browse findings in an interactive terminal UI.

Known fixer limits:

- Default exports are not auto-fixed.
- Barrel export declarations are not auto-fixed.
- Grouped variable exports such as `export const a = 1, b = 2` are not
  auto-fixed.

## Commands

Kuizer currently exposes these commands:

- `kuizer detect`
- `kuizer fix`
- `kuizer restore`
- `kuizer init`
- `kuizer revert`

You can also omit the command for many detect-style invocations, for
example `kuizer --json`.

## Quick start

Install dependencies:

```bash
bun install --frozen-lockfile
```

Run detection:

```bash
kuizer detect
kuizer detect --json
kuizer detect --interactive
kuizer detect --only unusedImports
kuizer detect --only exports
```

Preview fixes:

```bash
kuizer fix --dry
```

Apply fixes:

```bash
kuizer fix -w
```

Restore a snapshot:

```bash
kuizer restore --snapshot ./.kuizer/snapshots/<id>/manifest.json
kuizer revert --snapshot 2026-04-05T04-45-49-701Z-fix-f1f310e4
```

## Configuration lookup

Kuizer resolves configuration in this order:

1. `--config <path>`
2. Project config discovered from the current working directory upward
3. `~/.kuizerrc`
4. Built-in defaults

For TypeScript project resolution, Kuizer uses this order:

1. `--tsconfig <path>`
2. `tsconfig` from config files
3. The nearest `tsconfig.json` from the current working directory upward
4. The default `./tsconfig.json` relative to the selected config base

## Development

Run the core checks:

```bash
bun run typecheck
bun run lint
bun run test
bun run build
```
