# Revert and restore

Kuizer can roll back applied fixes by restoring a previously created
snapshot.

## Snapshot sources

Snapshots are created automatically when you run:

```bash
kuizer fix -w
```

unless you also pass `--no-backup`.

Each snapshot contains:

- A `manifest.json` file
- The original contents of changed files

## Restore from the CLI

Use `restore` or `revert` with a snapshot ID or manifest path:

```bash
kuizer restore --snapshot ./.kuizer/snapshots/<id>/manifest.json
kuizer revert --snapshot 2026-04-05T04-45-49-701Z-fix-f1f310e4
```

`revert` is currently a convenience wrapper around the same restore logic.

## Revert from interactive mode

During `kuizer detect --interactive`, the UI stores the most recent snapshot
created by an in-session apply action.

Press `R` to restore that last snapshot.

This only works after you have already applied at least one fix in the same
interactive session.

## Current behavior notes

The current implementation accepts:

- A snapshot ID
- A path to `manifest.json`

It does not currently accept a snapshot directory path by itself.
