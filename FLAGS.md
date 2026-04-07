# Kuizer flags reference

This page documents the current CLI surface as implemented in the repository.

## Commands

Kuizer supports these commands:

- `detect` or `d`
- `fix` or `f`
- `restore` or `r`
- `init`
- `revert` or `rv`

## Shared flags

These flags are used by more than one command.

### `-c, --config <path>`

Use a specific config file instead of auto-discovery.

### `--tsconfig <path>`

Use a specific TypeScript config file.

### `-o, --only <detectors>`

Run only selected detectors. Supported detector IDs are:

- `unusedExports`
- `unusedImports`
- `unusedFiles`
- `unusedFunctions`
- `unusedTypes`
- `unusedClasses`
- `unusedComponents`
- `unusedConstants`
- `unusedEnums`
- `unusedDefaults`
- `unusedOthers`

Supported aliases include:

- `exports`
- `imports`
- `files`
- `components`

### `-s, --skip <detectors>`

Skip selected detectors.

## Detect flags

These flags apply to `kuizer detect`.

### `-r, --reporter <format>`

Choose `terminal` or `json`.

### `--json`

Shortcut for `--reporter json`.

### `-f, --filter <types>`

Filter findings by export kind or detector. Supported values include:

- `functions`
- `types`
- `interfaces`
- `classes`
- `vars`
- `enums`
- `defaults`
- `unusedImports`
- `unusedComponents`
- `exports`
- `all`

### `--exclude-pattern <regex>`

Exclude export names that match a JavaScript regular expression.

### `--interactive`

Launch the interactive terminal UI. The current UI supports:

- File and finding panes
- Type filters
- Selection and batch apply
- Reverting the last in-session snapshot with `R`

It does not currently implement code preview, editor jumping, or text search.

## Fix flags

These flags apply to `kuizer fix`.

### `--dry`, `--dr`, `--dry-run`

Preview the fix plan without writing files.

### `-w, --write`

Apply supported fixes to disk.

### `--no-backup`

Skip snapshot creation before applying fixes.

## Restore and revert flags

These flags apply to `kuizer restore` and `kuizer revert`.

### `--snapshot <path|id>`

Restore from a snapshot ID or a `manifest.json` path.

## Notes

Kuizer accepts some detect and fix flags without an explicit command and
infers the command for convenience. For example:

```bash
kuizer --json
kuizer -o unusedExports
kuizer --dry
```
