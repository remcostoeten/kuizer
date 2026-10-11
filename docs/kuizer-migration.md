# Porting kuizer v1 into the Rust kuizer

Status: plan. Nothing in this document is implemented yet.

kuizer v1 is a TypeScript dead-code finder (Bun, ts-morph). It lives on the `v1-typescript` branch of `github.com/remcostoeten/kuizer` at `44fc149`. `master` now holds the Rust disk cleanup tool that was called sweep until October 2026 and took over the kuizer name and repository. This plan rebuilds v1 inside it as a `kuizer code` command: kuizer already removes generated folders that a tool can rebuild, and v1 removes source that nothing imports. Both answer "what on this disk can go", with different evidence.

The port is a rewrite, not a translation. ts-morph gives kuizer v1 a full TypeScript program and the type checker; Rust has no equivalent. The replacement is the oxc toolchain, which oxlint, the linter in the TypeScript repos, is built on: a parser, a semantic pass with scopes and references, and a module resolver that reads `tsconfig.json`. That covers everything v1 actually uses from the checker, which is symbol references and module resolution, not types.

## 1. Audit: kuizer v1

### What it does

| Area | Files | Behaviour |
| --- | --- | --- |
| Engine | `src/engine/index.ts` (417 lines) | Loads the ts-morph project from `tsconfig.json`, adds every `*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}` under the root plus test files, builds an import graph from `import`, `export ... from` and literal `import()` calls, finds entry points, and runs a BFS for production reachability and a second one for test-only reachability. Non-literal `import()` calls are recorded as analysis boundaries. |
| Cache | `src/engine/cache.ts` | Per-file `size-mtime` key in `node_modules/.cache/kuizer/analyze-cache.json`, storing adjacency, boundaries and consumer keys. |
| Detectors | `src/detectors/` | `unusedFiles`: files not reachable from an entry. `unusedImports`: imported bindings with no local reference. `unusedExports`: exported declarations with zero production consumers, split by kind into `unusedFunctions`, `unusedTypes`, `unusedClasses`, `unusedComponents` (PascalCase function or class in `.tsx`/`.jsx`), `unusedConstants`, `unusedEnums`, `unusedDefaults`, `unusedOthers`. Each export finding is flagged `testOnly` or `internalOnly` where that applies. |
| Framework rules | `unused-exports.ts`, `config/presets.ts` | Presets `nextjs-app`, `nextjs-pages`, `vite`, `none` add entry globs and allowed exports. Next.js route files keep `default`, `metadata`, `generateMetadata`, route handlers (`GET`, `POST`...), segment config and OG image exports. Root `*.config.*` default exports count as consumed. |
| Fix planning | `src/fixers/planners/` | Unused file: delete it (`destructive`). Unused export used locally: strip the `export` keyword (`safe`). Unused export that is the file's only statement: delete the file (`destructive`). Otherwise delete the declaration (`cautious`). Refuses default exports, re-export declarations, grouped `export const a, b`, and anything when a dynamic-import boundary exists. |
| Fix apply | `src/fixers/apply.ts` | Line-range edits, applied bottom-up per file. `remove-export-keyword` regex-strips a leading `export` on every line in the range. `delete-declaration` drops the lines and collapses blank runs. |
| Snapshots | `src/backup/` | Copies every touched file to `.kuizer/snapshots/<timestamp>-fix-<uuid>/files/` with a `manifest.json`. `restore` and `revert` both copy the files back. |
| Config | `src/config/` | cosmiconfig lookup: `--config`, then `kuizer.config.{json,ts,js}`, `.kuizerrc*`, the `kuizer` key in `package.json`, then `~/.kuizerrc`. Keys: `tsconfig`, `entrypoints`, `framework`, `countTestsAsConsumers`, `allowedExports`, `ignore.{files,symbols,paths}`, `detectors.<id>.{enabled,severity}`, `outputDir`, `dryRunByDefault`. |
| CLI | `src/cli/index.ts` (984 lines), `help.ts` (567) | `detect` (`--only`, `--skip`, `--filter` by detector or kind, `--exclude-pattern` regex on symbol names, `--json`, `--reporter`, `--interactive`), `fix` (`-w`, `--dry-run`, `--no-backup`, `--interactive`), `restore`, `revert`, `init`. Exit code 1 when any finding has severity `error`. |
| TUI | `src/cli/interactive.ts` (1195 lines) | Hand-rolled ANSI two-pane browser with detector and kind toggles, selection and a fix hand-off. |
| JSON | `src/reporters/json.ts` | `{ meta: { version, durationMs, total, analysisBoundaryCount }, findings, analysisBoundaries }`. |
| Tests | `__tests__/` | Bun tests per module plus `e2e.test.ts` over `__tests__/fixtures/sample`. |

### Defects to fix in the port, not copy

1. **`unusedFiles` reports almost nothing by default.** With no configured entry points, `inferEntries` makes every file that nothing imports an entry, so it is reachable by definition. Only files inside import cycles that nothing outside imports are ever reported. The port reads real entry points from `package.json` (`main`, `module`, `bin`, `exports`, `scripts` that name a file) and framework conventions, and reports unreachable files only when at least one real entry exists. Otherwise it says so in the report.
2. **The cache gives stale results.** A consumer key is `<declaring file>:<byte offset>:<symbol>`, and it is cached under the consuming file. Editing the declaring file shifts the offset, but the consumer's `size-mtime` stays the same, so its cached keys point at the old offset and the export shows as unused. Adjacency is cached the same way, though it depends on which other files exist. The port starts with no cache (section 4.6).
3. **`ignore.symbols` is documented as a regex (`"^_"` in `CONFIG.md`) but matched as an exact string.** The port takes globs and says so.
4. **`ignore.files` is compared against absolute paths in `unusedFiles` and against root-relative paths in the engine.** A relative entry only works in one of the two places. The port resolves every ignore path once, at config load.
5. **`node_modules`, `.next` and `dist` are hard-coded skips.** In kuizer they come from the existing artifact rules, so `target`, `.turbo`, `build` with a marker and the rest are skipped too.
6. **Every split detector recomputes everything.** `detectById` runs `collectUnusedExportFindings` and the full consumer count once per detector, which is nine times with defaults. The port computes it once and filters.
7. **`hasInternalConsumer` matches identifier text across the whole file.** A shadowing local with the same name counts as a use. oxc's semantic pass resolves references to the declaring symbol, so this case is correct by construction.
8. **`remove-export-keyword` edits by line and regex.** Any line in the statement's range that starts with `export`, such as a line in a template literal, gets rewritten too. The port edits byte spans from the AST.
9. **Restore never checks whether files changed after the snapshot.** It overwrites later work without asking. The port stores a BLAKE3 hash per file and refuses or asks when the current content differs.
10. **The `vite` preset lists `index.html` as an entry.** It is never in the file set, so it never matches, and `<script type="module" src>` tags are not followed. The port parses `index.html` script tags as entries.
11. **`.ts` and `.js` config files are loaded by executing them through cosmiconfig.** That is not available in Rust and is not wanted: a config file should not run code. The port reads JSON and TOML only and prints a one-line hint for a found `kuizer.config.ts`.
12. **`restore` and `revert` do the same thing.** One command remains.

## 2. Audit: kuizer, as the host

State at the time of writing: `master` at `cc74c13`, Rust 2024, 8.6k lines of Rust including tests, one crate, no uncommitted work. History, insight, `clean`, `duplicates`, `cache` and profile protection landed in `c944b2c`, and the rename from sweep in `2286883`. The rename kept two fallbacks: `.sweepignore` markers are still honoured, and `~/.sweep-history.json` is read until `~/.kuizer-history.json` exists.

What carries over directly:

| kuizer piece | Use in `kuizer code` |
| --- | --- |
| `delete::Guard`, `delete::delete`, `trash` | Deleting an unused file goes through the same guard as an artifact: inside a scanned root, not a symlink, same device and inode as at analysis time, and moved to the trash by default. `Target.identity` is recorded during analysis. |
| `history` | Every applied fix is appended to `~/.kuizer-history.json`. `Entry` needs a `kind` field (`artifact` or `code`) and optional `symbol` and `snapshot` fields. `History` uses `#[serde(default)]`, so old files still load. `kuizer insight` lists code runs separately, since their byte counts are not comparable. |
| `git::uncommitted_changes` | Before a write, files with uncommitted changes are listed in the confirmation, as `kuizer clean` does now for projects. Unlike `clean`, a code fix edits tracked files, so `--fix` refuses when a touched file is dirty unless `--allow-dirty` is passed. |
| `blake3` | Content hashes for revalidation before an edit and for snapshot integrity. |
| `config` | A new `[code]` section with `deny_unknown_fields` like the others. |
| `config::Excludes`, `scan.skip_dirs`, `.kuizerignore` (and the legacy `.sweepignore`), `protect` | Source collection skips the same paths, browser profiles included. |
| `detect::Detector` | Directories that match an artifact rule are pruned from source collection. |
| `output::json` with `schema_version` | A new `code` report document. Adding a command adds a document; it does not bump `SCHEMA_VERSION`. |
| `cli::RemovalArgs` (`--select`, `--dry-run`, `--permanent`, `--yes`) | Reused for `kuizer code --fix`, matching `kuizer cache --clean`. |
| `tui` (ratatui `App` and `view`) | The v1 browser becomes a view in kuizer's TUI in phase 6. The design doc already reserves keys `1` to `4` for views. |
| CI (`fmt`, `clippy -D warnings`, `test`) | Unchanged; the new crate is part of the workspace. |

What does not fit as is:

- **Safety levels.** kuizer has `safe` and `review`, and bulk actions select `safe` only. A code edit is never regenerable, so the mapping is strict: stripping `export` from a symbol that is still used locally is `safe`; deleting a declaration or a file is `review`. Nothing a code fix does is selected by `a` in the TUI or by `--select safe` unless it is the export-keyword strip.
- **One crate.** oxc adds roughly a dozen crates and noticeable compile time. `design.md` section 3 says the crate stays single until compile times or reuse justify a split. This is that point: the analysis goes into a workspace member, `crates/kuizer-code`, which knows nothing about deletion, the TUI or the history file. The `kuizer` crate wires it to those.
- **Platform.** kuizer's mount handling is Linux-only so far. The code analysis has no platform code and works wherever oxc does.
- **The scan pipeline.** kuizer's walker emits `Candidate`s for directories and never reads files. Source analysis reads every source file, so it uses its own collector (section 4.1) instead of `scan::start`.

## 3. Command and configuration

```text
kuizer code [PATHS...]                report findings for each project under PATHS
    --only ID|KIND                    repeatable: unused-files, unused-exports, unused-imports, or a kind
    --skip ID                         repeatable
    --exclude-symbol GLOB             repeatable
    --tsconfig PATH                   override the tsconfig for a single project
    --fix                             plan fixes, show them, apply after confirmation
        --select safe|all|ID          default safe, repeatable
        --dry-run                     print the plan and a unified diff, touch nothing
        --permanent                   delete unused files instead of trashing them
        --allow-dirty                 allow edits to files with uncommitted changes
        --no-snapshot                 skip the snapshot of edited files
        -y, --yes
kuizer code restore [ID]              restore the latest or the given snapshot
```

`PATHS` may hold several projects. A project is a directory with a `tsconfig.json` or a `package.json`; monorepos are handled by treating each workspace package as a project and resolving cross-package imports through `oxc_resolver`, which follows workspace symlinks in `node_modules`.

Detector ids switch from camelCase to kuizer's kebab-case: `unused-files`, `unused-imports`, `unused-exports`, and kinds `function`, `type`, `class`, `component`, `const`, `enum`, `default`, `other`. v1's split detectors become a `kind` field and a filter, not separate detectors.

Exit codes follow the other commands: `0` success, `1` error, `2` usage. v1's "1 when a finding has severity `error`" moves behind `--fail-on-findings`, so CI keeps it without changing the meaning of exit 1 for other commands.

Global defaults in `~/.config/kuizer/config.toml`:

```toml
[code]
count_tests_as_consumers = false
allowed_exports = []
exclude_symbols = []          # globs
snapshot = true
```

Per-project settings come from the first of `kuizer-code.toml`, the `kuizer` key in `package.json` (the same key v1 used, so its contents are read as they are), or a v1 `kuizer.config.json`, `.kuizerrc` or `.kuizerrc.json` (read-only, with a notice to migrate). Keys: `tsconfig`, `entrypoints` (globs), `framework` (`auto`, `nextjs-app`, `nextjs-pages`, `vite`, `none`; `auto` detects from `package.json` dependencies and the presence of `app/` or `pages/`), `ignore.files`, `ignore.paths`, `ignore.symbols`, `detectors.<id>.{enabled,severity}`. `outputDir` and `dryRunByDefault` are dropped: snapshots move out of the project (section 4.8) and `kuizer code` without `--fix` never writes.

## 4. Design of `crates/kuizer-code`

```text
kuizer-code/src/
├─ lib.rs          analyze(project, options) -> Analysis; plan(analysis, selection) -> Plan
├─ project.rs      project discovery, per-project config, tsconfig, framework detection
├─ files.rs        source collection
├─ parse.rs        one file -> ModuleInfo (owned, no AST lifetimes)
├─ resolve.rs      oxc_resolver wrapper with a shared cache
├─ graph.rs        adjacency, entries, prod and test reachability
├─ exports.rs      export tables and re-export resolution
├─ framework.rs    presets and framework-consumed exports
├─ detect/         files.rs, exports.rs, imports.rs
├─ fix/            plan.rs, edit.rs (span edits), diff.rs
└─ snapshot.rs     create and restore, with hashes
```

### 4.1 Source collection

Walk the project with `read_dir`, applying kuizer's `Excludes`, `skip_dirs`, `.kuizerignore` (and the legacy `.sweepignore`), the protected paths and pruning on artifact rule matches. Respect `.gitignore` through the `ignore` crate (0.4.33 at the time of writing, from ripgrep like `globset`). Keep files whose extension is `ts tsx mts cts js jsx mjs cjs`, and `.d.ts` files separately (they declare types for others and are never reported as unused files). `tsconfig.json` `include`, `exclude` and `files` narrow the set when present, matching v1's use of the ts-morph project.

### 4.2 Parsing

One `oxc_allocator::Allocator` per file. `oxc_parser` with the source type from the extension, then `oxc_semantic` for scopes and references. Extract an owned `ModuleInfo` and drop the arena, so no AST outlives its file and files parse in parallel on a scoped thread pool (`std::thread::scope` over a work queue, or `rayon` 1.12, which `design.md` already earmarks for phase 5).

```rust
pub struct ModuleInfo {
    pub path: PathBuf,
    pub hash: blake3::Hash,
    pub imports: Vec<Import>,          // specifier, span, default / named / namespace bindings, type-only
    pub reexports: Vec<Reexport>,      // export { a as b } from, export * from, export * as ns from
    pub exports: Vec<LocalExport>,     // name, kind, statement span, name span, local reference count
    pub dynamic_imports: Vec<DynamicImport>, // literal specifier or boundary span
    pub namespace_uses: Vec<(BindingName, Member)>, // ns.member accesses, including `import('x').then(m => m.a)`
    pub unused_import_bindings: Vec<Binding>,
    pub statement_count: usize,
}
```

`oxc_semantic` gives every export its local references, which replaces both `findReferences` and the text-matching `hasInternalConsumer`. It also flags import bindings with zero references, which is the whole `unusedImports` detector. Line and column come from `oxc_span` offsets through a line index computed once per file.

Crate versions at the time of writing: `oxc_parser`, `oxc_ast`, `oxc_semantic`, `oxc_span`, `oxc_allocator` 0.153.0, `oxc_resolver` 11.24.3. oxc releases often and breaks its AST API between minors. Pin exact versions the way `Cargo.toml` already does, and upgrade them together.

### 4.3 Resolution

`oxc_resolver` configured from the project's `tsconfig.json` (`paths`, `baseUrl`, project references), with TypeScript's extension order, `.js` to `.ts` mapping, `index` files, `package.json` `exports` and conditions `import`, `require`, `types`, `default`. Resolutions that land outside the project or in `node_modules` are external and ignored, unless they land in another workspace package that is part of the same run. This replaces v1's two separate resolvers: ts-morph for static imports and a hand-written ten-candidate list for `import()`.

### 4.4 Graph and entries

Entries, in order: configured `entrypoints`; framework preset globs; `package.json` `main`, `module`, `types`, `bin`, every `exports` target, and files named in `scripts`; `index.html` module scripts for Vite; config files at the root (`*.config.*`). Test files (`*.test.*`, `*.spec.*`, `__tests__/`) are their own entry set. Production reachability and test-only reachability are two BFS passes over the adjacency, as in v1. A non-literal `import()` in a reachable file is recorded as a boundary, and findings in that project carry `boundary: true`, as v1 does.

### 4.5 Export resolution and consumer counting

This is the part ts-morph did for free through `getExportedDeclarations` and `getAliasedSymbol`, and the part most likely to be wrong in a first port.

- Build an export table per module: local exports, named re-exports with aliases, `export *` (which never re-exports `default`) and `export * as ns`.
- Resolve every (module, name) pair to its origin (declaring module, local name), following re-export chains with a visited set for cycles. An ambiguous name reachable through two `export *` is not exported, which matches TypeScript.
- A consumer is a reachable module that imports the name directly, imports it through a barrel, accesses it as `ns.name` on a namespace import, or accesses it on the module object of a literal `import()`. A namespace import whose binding escapes (passed as an argument, spread, or used with a computed key) marks every export of the target as used, since the members can no longer be listed. v1 silently missed that case.
- Count prod and test consumers per origin, once. An export with zero prod consumers is a finding; `test_only` when it has test consumers, `internal_only` when it has local references.

### 4.6 Caching

None in the first version. oxc parses and analyses a large TypeScript repository in a fraction of the time ts-morph takes to load it, and v1's cache was wrong (section 1, defect 2). Measure with the benchmark in phase 7 before adding one. If one is added, key the whole module graph on the hashes of every file plus the tsconfig and package manifests, and store it under `dirs::cache_dir()/kuizer/code/`, not in `node_modules`.

### 4.7 Fix planning and edits

The planner keeps v1's decision table (section 1) and its refusals. Edits are byte spans taken from `ModuleInfo`:

| Edit | Span | Safety |
| --- | --- | --- |
| `remove-export` | from the `export` keyword to the start of the declaration, plus `default ` where present | `safe` |
| `remove-declaration` | the statement plus its leading comments and one trailing newline | `review` |
| `remove-file` | the whole file, through `delete::delete` | `review` |
| `remove-import` (new) | the specifier, or the whole import statement when it was the last one, keeping side-effect imports | `review` |

Edits for one file are sorted by start offset, checked for overlap (overlap is a planning error, never applied), and applied from the end. Before each file is written, its current BLAKE3 hash must equal the hash from analysis; otherwise the file is skipped with `changed since analysis`. Writes go to a temporary file in the same directory followed by a rename, so a crash never leaves a half-written source file. Each file is then re-parsed; a parse error after an edit restores the original content and marks the item failed. `--dry-run` prints a unified diff with the `similar` crate (3.2.0).

### 4.8 Snapshots and restore

Snapshots live in `dirs::state_dir()/kuizer/snapshots/<id>/` (on Linux `~/.local/state`), out of the project tree so they never show up in `git status`. The manifest records the project root, each file's original path and BLAKE3 hash before and after the edit. Files removed through the trash need no copy: restoring them means taking them out of the trash, which `trash` supports on Linux. The snapshot id goes into the history entry. `kuizer code restore` refuses to overwrite a file whose current hash differs from the recorded post-edit hash, unless `--force` is given.

## 5. Phases

Each phase is one pull request, in the style of `design.md` section 10.

| Phase | Scope | Accepted when |
| --- | --- | --- |
| 0 | Create the workspace (`Cargo.toml` with `members = [".", "crates/kuizer-code"]`). Check out `v1-typescript`, run v1's `kuizer detect --json` from that checkout (not the Rust binary of the same name) on `__tests__/fixtures/sample` and on three real repositories (a library, a Next.js app router app, a Vite app), and commit the outputs as golden files under `crates/kuizer-code/tests/golden/`. Copy the fixture tree to `crates/kuizer-code/tests/fixtures/`. | Workspace builds, CI passes unchanged, golden files committed with the commands that produced them. |
| 1 | `project.rs`, `files.rs`, `parse.rs`, `resolve.rs`, `graph.rs`. A debug command prints entries, reachable and test-reachable sets. | Graph matches v1 on the fixture. Tests for `tsconfig` `paths`, `.js` specifiers resolving to `.ts`, `index` files, `exports` maps, a workspace package import, a cycle, a non-literal `import()` boundary, and a symlink in the tree (not followed). |
| 2 | `exports.rs`, the three detectors, framework rules. | Fixture findings equal the golden files, except where section 1 lists a defect, and each such difference has a named test. Re-export chains, `export *` ambiguity, `export * as ns`, namespace member access, escaping namespace bindings, `import().then(m => m.x)`, shadowed names, type-only imports, Next.js app and pages conventions. |
| 3 | `kuizer code` report: text output in `output/text.rs` style, `code` JSON document, filters, `--fail-on-findings`, `[code]` config and per-project config, v1 config reader. | JSON snapshot test. `config check` rejects unknown `[code]` keys. Real-repository runs differ from the golden files only by documented defects. |
| 4 | Fix planning, span edits, diff, `--fix` with `RemovalArgs`, dirty-file refusal, hash revalidation, atomic writes, re-parse check, snapshots, history entries, `kuizer code restore`. | Every destructive test runs in a temp dir. Dry run leaves a tree snapshot unchanged. Tests: file changed between analysis and fix, overlapping edits, parse failure after an edit rolls back, restore refuses a file edited after the fix, unused file goes to the trash and comes back, history file from the current release still loads. |
| 5 | Remove-import fixes; grouped `export const a, b` (remove one declarator); `export { a, b }` lists (remove one specifier). v1 refused all three. | Positive and negative test per case; output re-parses and the fixture still type-checks with `tsc --noEmit`. |
| 6 | TUI view `4`, code findings grouped by project and file, selection, `d` opens the fix preview with the diff. Replaces v1's 1,200-line interactive mode. | Same acceptance as `design.md` phase 2 and 3: input latency under 50 ms, terminal restored on panic, selection survives filtering. |
| 7 | Benchmark on a fixed large repository against kuizer v1, decide on caching, document the schema. Retire v1: a final npm release prints a deprecation notice that points to the Rust binary, and the `v1-typescript` branch README says the same. | Numbers recorded in `design.md` section 9. Deprecation release published. |

## 6. Risks

- **No type checker.** kuizer inherited TypeScript's resolution of `declare module`, global augmentation and `/// <reference>` directives. These are rare in application code. The port treats a module with `declare global` or `declare module` as always used and reports such cases as boundaries, rather than guessing.
- **JSX and decorators.** oxc parses both. Decorator metadata (Angular, NestJS) can consume classes through reflection that no import shows; framework detection should mark those projects and lower exports in them to `review`.
- **oxc API churn.** Contained to `parse.rs` and `resolve.rs`; nothing else imports oxc types.
- **Scope creep in kuizer.** `kuizer code` adds a second kind of finding to a tool built around directories and bytes. Keeping it in its own crate and its own report document keeps the scan pipeline, `Candidate` and the artifact JSON untouched.
