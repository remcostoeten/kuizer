# Phase 9: history ids and restore

Status: plan. `src/history/mod.rs` stores runs in `~/.kuizer-history.json` with entries per deleted item, and `kuizer insight` lists them. Runs and entries have no ids, and nothing can be put back. Behaviour is specified in `design.md` section 6.4.

## 1. Ids

### Format

Sessions are `s<N>`, items `s<N>.<M>`. `N` counts from 1 in file order and is never reused. `M` is the 1-based position of the entry in its run; entries are never edited after a run is written, so it is stable.

### Storage

```rust
pub struct History {
    pub next_session: u64,      // new, #[serde(default)]
    pub runs: Vec<Run>,
    pub cumulative: Cumulative,
}

pub struct Run {
    pub id: u64,                // new, #[serde(default)]
    pub action: Action,         // new, #[serde(default)]: delete or restore
    ...
}
```

`load` assigns ids to runs that have `id == 0`, in file order, and sets `next_session` past the highest id. `append` takes `next_session`, increments it and writes both. Storing the ids, instead of deriving them from positions, keeps them stable if runs are ever pruned or merged.

An old history file loads with ids `s1..sN` and keeps them after the next write. The legacy `.sweep-history.json` path through `readable_path_in` keeps working.

## 2. `kuizer history`

`Command::Insight` becomes `Command::History` with `insight` as a visible alias, so `kuizer insight` keeps working and `nested_help` in `src/commands/mod.rs` maps both.

| Command | Output |
| --- | --- |
| `kuizer history` | One line per session, newest first: id, start time, command, items, freed, still in trash. Then the totals line `insight` prints today. |
| `kuizer history show s42` | Every item: `s42.1`, size, state (in trash, left trash, deleted, restored), rule, path. |
| `--json` | The current insight JSON with `id` added to runs and entries (`"s42"`, `"s42.1"`). Additive, no schema bump. |

`src/commands/insight.rs` becomes `src/commands/history.rs`. The `state` function there gains `restored`, found by matching restore runs (below).

## 3. `kuizer restore`

```text
kuizer restore s42          every item of session 42 still in the trash
kuizer restore s42.3 s42.7  single items
kuizer restore --last       the newest delete session
kuizer restore ... --dry-run
```

### Matching an entry to a trash item

The `trash` crate lists trash items with `trash::os_limited::list()` (Linux and Windows) and restores them with `trash::os_limited::restore_all(items)`. A `TrashItem` has `original_parent`, `name` and `time_deleted`. The same path can be in the trash several times, so an entry matches the item with the same original path whose `time_deleted` is closest to the entry's `deleted_at`, within 60 seconds. Each trash item matches at most one entry, the same rule `still_in_trash` uses.

### Refusals

Each refused item is reported with its reason and the rest continue:

| Reason | Message |
| --- | --- |
| Deleted permanently | `s42.3 was deleted permanently` |
| Not in the trash anymore | `s42.3 is no longer in the trash` |
| Original path exists | `s42.3: ~/apps/web/node_modules exists, not overwriting` |
| Parent directory gone | `s42.3: ~/apps/web no longer exists` |

`restore_all` returns `Error::RestoreCollision` for an existing path; check with `symlink_metadata` first anyway so the refusal is reported per item and nothing is half-restored. kuizer never overwrites.

Exit code is `0` when everything requested was restored or refused for a reason above, and `1` when the trash itself failed.

### Recording

A restore appends its own run with `action: restore` and one entry per restored item, carrying the original entry id in a new optional `restores: Option<String>` field. `Cumulative` subtracts restored bytes from `bytes`, so "freed" stays honest. `insight`, `history` and the dashboard read the restore runs to mark entries as restored.

### macOS

`trash::os_limited` is not available on macOS. `kuizer restore` exits with a usage error there: "restore needs a trash kuizer can read; on macOS use Put Back in Finder". `history` shows trash state as unknown, as `insight` does today.

## 4. Dashboard

- History tab: show `s<N>` in the runs table and a copyable `kuizer restore s<N>` button on runs with items still in the trash.
- Mark restored entries in the run detail and exclude them from "freed" totals and charts.

## Tests

`tests/restore.rs`, each test with its own `HOME` and `XDG_DATA_HOME` in a temp dir so the freedesktop trash lives inside the test directory:

- whole session restored, history gains a restore run, `history show` marks items restored.
- one item of a session.
- item already emptied from the trash: refused, others restored.
- original path reoccupied: refused, nothing overwritten (compare file contents).
- partial session where some items were permanent: those refused with the permanent reason.
- the same path trashed twice: `restore` of the older session restores the older copy (check by a marker file inside).
- old history file without ids: ids assigned in order, stable across two loads and one append.

## Accepted when

The tests above pass on Linux and Windows CI, and `kuizer insight` still works as an alias.
