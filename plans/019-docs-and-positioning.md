# Plan 019: Update the spec, config example and README for the general cleanup sources

> **Executor instructions**: Follow this plan step by step. Run every verification command and
> confirm the expected result before moving on. If anything in "STOP conditions" occurs, stop and
> report; do not improvise. Do not edit `plans/README.md`; the orchestrator keeps the status table.
>
> **Drift check (run first)**: none. This plan runs last and describes what the other plans built.
> Read `plans/README.md` for which plans are DONE and which were REJECTED, and describe only DONE work.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW (documentation and one dead error variant)
- **Depends on**: every other plan that is DONE
- **Wave**: 3, after 018
- **Model**: Sonnet 5.5, high effort
- **Category**: docs
- **Planned at**: commit `5d04119`, 2026-10-21

## Purpose

`docs/design.md`, `README.md` and the crate descriptions still describe kuizer as a developer
tool that only removes what can be regenerated. After the general cleanup plans, kuizer also
cleans app and browser caches, the trash and, depending on decisions D2 and D3, system package
caches and personal files. This plan brings the documents in line with the code, removes the
temporary `AppError::NotBuilt`, and makes sure `docs/config.example.toml` lists every new key.

## Decision input

From `plans/README.md`:

- D1 (positioning): "general first" changes the README intro and crate descriptions to a general
  cleaner; "developer first" keeps the developer framing and adds the general sources as a section.
- D2 (personal files) and D3 (system cleanup): describe removal only for the sources whose plan
  (013, 012) is DONE.

## Current state

- `docs/design.md` section 1 "Product specification" says kuizer "finds files and directories on a
  developer machine that can be removed and regenerated", lists sources in a table (projects,
  cache, toolchain, docker, git, duplicates, big), and section 6.2 says `duplicates` and `big`
  have no `clean` because "removing the wrong copy of a file cannot be regenerated". Section 2
  has the module table; section 5 the config table; section 6 the CLI synopsis; section 8 the
  threat model (row "Browser and app profiles" says profiles are never entered and are refused).
- `README.md` intro: "A terminal tool that finds build output, development caches and duplicate
  files on a developer machine, and removes the ones you select." followed by a bullet list per
  command.
- `Cargo.toml` `description = "Keyboard-driven disk cleanup for developers: generated artifacts,
  caches and duplicates"`, `keywords = ["cleanup", "disk", "node-modules", "cache", "cli"]`.
- `src/commands/mod.rs` has `AppError::NotBuilt(&'static str)` (plan 001); plans 006 to 008 stopped
  using it.
- `docs/config.example.toml` has `[catalog]`, `[system]`, `[big]` and `[keys]` sections filled by
  plans 002, 007, 008, 009 and 012.
- Writing rules for all prose here: plain declarative sentences with concrete facts, no em dashes,
  no marketing words, no benefit lists, no "why this matters" sections, no exclamation marks.
  README rules: the intro says what the project is and does in plain words; no meta sections.

## Commands you will need

```sh
export CARGO_TARGET_DIR="$HOME/.cache/kuizer-plans-target"
export CARGO_BUILD_JOBS=2
```

| Purpose | Command | Expected |
|---|---|---|
| Config example parses | `cargo test -p kuizer config::tests::parses_full_example -- --test-threads=2` | pass |
| Workspace tests | `cargo test --workspace --all-targets -- --test-threads=2` | all pass |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Em dash check | `grep -nP '\x{2014}' docs/design.md README.md docs/config.example.toml` | no new matches (compare with `git diff`) |

## Suggested executor toolkit

- Skill `generic-program-rules` (README and writing rules) and `agent-writing` if available.

## Scope

**In scope**: `docs/design.md`, `README.md`, `docs/config.example.toml`, `Cargo.toml` and
`crates/kuizer-gui/Cargo.toml` (`description` and `keywords` only), `src/commands/mod.rs` (only
removing `NotBuilt`), `docs/phases/README.md` (one line pointing to `plans/README.md` if the
user keeps `plans/` in the repo; otherwise leave it).

**Out of scope**: `site/` (the marketing site has its own conventions and plans), `AGENTS.md`
(the user's instructions file; list the lines that are now outdated in your report instead).

## Git workflow

- Branch `general/019-docs-and-positioning` from `feat/general-cleanup` (with everything else merged).
- One commit `docs: describe the general cleanup sources`, body line
  `Plan: plans/019-docs-and-positioning.md`. No Claude references. Do not push.

## Steps

### Step 1: Remove `NotBuilt`

Delete `AppError::NotBuilt` if `grep -rn "NotBuilt" src` shows no other use.

**Verify**: `cargo build -p kuizer` exit 0.

### Step 2: `docs/design.md`

- Section 1: rewrite the first paragraph per D1; replace the principle "finds what can be removed
  and regenerated" with the rule that now holds (regenerable items can be `safe`; personal files,
  if D2, are always `review`, never selected in bulk, and need a typed confirmation). Add rows to
  the sources table for `trash`, `system`, and the catalog-backed `cache` (groups developer, apps,
  browsers, system), with their removal behavior.
- Section 2: add `catalog`, `trashcan`, `system`, `big`, `keymap`, `external` to the module table.
- Section 5: add every new config key with default and meaning (`catalog.*`, `system.*`,
  `big.*`, `[keys]`).
- Section 6: add `kuizer trash`, `kuizer system`, `kuizer big` and `kuizer cache --group` to the
  synopsis; update 6.2 for `big` per D2.
- Section 8: update the "Browser and app profiles" row: profiles are never entered by the walker;
  only folders named in the allowlist (`src/protect.rs` `PROFILE_CACHES`) inside a profile may be
  removed, and only through catalog entries. Add a row for privilege escalation if D3.

**Verify**: `grep -c "trashcan\|PROFILE_CACHES\|kuizer system" docs/design.md` is at least 3.

### Step 3: README and crate metadata

Per D1, update the intro sentence and add one bullet each for `kuizer cache` groups, `kuizer
trash`, `kuizer big` and (if D3) `kuizer system`, in the existing bullet style. Update
`description` and `keywords` in both `Cargo.toml` files only if D1 is "general first".

**Verify**: run every command in "Commands you will need".

## Test plan

No new tests; `parses_full_example` and the workspace tests must pass.

## Done criteria

- [ ] All commands in "Commands you will need" pass
- [ ] `grep -rn "NotBuilt" src` prints nothing
- [ ] Every key under `[catalog]`, `[system]`, `[big]` in `docs/config.example.toml` appears in `docs/design.md` section 5
- [ ] `git diff --name-only feat/general-cleanup` lists only in-scope files
- [ ] Your report lists the `AGENTS.md` lines that no longer match

## STOP conditions

- `plans/README.md` does not record D1, D2 and D3: ask the orchestrator.
- The code does something the documents would have to describe differently from its plan:
  describe the code, and report the difference.

## Maintenance notes

- The site (`site/`) still describes the developer tool; a separate site task should follow D1.
