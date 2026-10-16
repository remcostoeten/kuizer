# Phase 6: more rules, `--json-lines` and the JSON reference

Status: plan. Global caches (`kuizer cache`, `src/caches.rs`), glob markers and the ecosystem rules for Python, .NET, CocoaPods, Elixir, Zig, Haskell, Dart, Swift, Maven, Gradle, CMake, Terraform and Unity are done. Temp and editor files, the remaining rules in the table, `--json-lines` and a documented schema are open.

## 1. Rules

Built-in rules live in `src/detect/builtin.rs`. Ecosystem rules come first and the generic JavaScript fallbacks (`js-dist`, `js-build`, `js-out`, `js-tool-cache`) and `cachedir-tag` last, so a folder with stronger evidence is named by its own rule. The `temp` category has only `unity-temp` so far.

### Detector change: glob markers

Rule markers are plain names (`is_plain_name` in `src/detect/detector.rs`). .NET and Haskell projects are identified by `*.csproj`, `*.fsproj` and `*.cabal`, whose names vary. Allow a marker to be a glob on a sibling name: compile markers with `globset` like patterns, keep the fast path for plain names through `Listing::has`, and fall back to scanning the sorted sibling list only for glob markers. Markers still may not contain `/` or `..`, so the path traversal check in `design.md` section 8 holds.

### New rules

| Name | Kind | Pattern | Evidence | Category | Safety |
| --- | --- | --- | --- | --- | --- |
| `ds-store` | file | `.DS_Store` | none | temp | safe |
| `thumbs-db` | file | `Thumbs.db` | none | temp | safe |
| `vim-swap` | file | `.*.sw[a-p]` | none | temp | review (the editor may be open) |
| `backup-tilde` | file | `*~` | none | temp | review |
| `merge-leftovers` | file | `*.orig`, `*.rej` | none | temp | review |
| `npm-debug-log` | file | `npm-debug.log*`, `yarn-error.log`, `pnpm-debug.log` | `package.json` | temp | safe |
| `pytest-cache` | dir | `.pytest_cache` | contains `CACHEDIR.TAG` | cache | safe |
| `mypy-cache` | dir | `.mypy_cache` | contains `CACHEDIR.TAG` | cache | safe |
| `ruff-cache` | dir | `.ruff_cache` | contains `CACHEDIR.TAG` | cache | safe |
| `tox-env` | dir | `.tox` | `tox.ini`, `pyproject.toml`, `setup.cfg` | dependencies | safe |
| `nox-env` | dir | `.nox` | `noxfile.py` | dependencies | safe |
| `python-venv` | dir | `.venv`, `venv` | contains `pyvenv.cfg` | dependencies | review (may hold hand-installed packages) |
| `dart-tool` | dir | `.dart_tool` | `pubspec.yaml` | cache | safe |
| `flutter-build` | dir | `build` | `pubspec.yaml` | build | review (nothing inside proves Flutter made it) |
| `dotnet-bin` | dir | `bin` | `*.csproj`, `*.fsproj`, `*.vbproj`, contains `Debug` or `Release` | build | safe |
| `dotnet-obj` | dir | `obj` | `*.csproj`, `*.fsproj`, `*.vbproj`, contains `project.assets.json` | build | safe |
| `zig-cache` | dir | `.zig-cache`, `zig-cache` | `build.zig` | cache | safe |
| `zig-out` | dir | `zig-out` | `build.zig` | build | safe |
| `elixir-build` | dir | `_build` | `mix.exs` | build | safe |
| `elixir-deps` | dir | `deps` | `mix.exs` | dependencies | safe |
| `haskell-stack` | dir | `.stack-work` | `stack.yaml` | build | safe |
| `haskell-cabal` | dir | `dist-newstyle` | `cabal.project`, `*.cabal` | build | safe |
| `elm-stuff` | dir | `elm-stuff` | `elm.json` | dependencies | safe |
| `go-vendor` | dir | `vendor` | `go.mod`, contains `modules.txt` | dependencies | review (vendoring is often committed) |
| `php-vendor` | dir | `vendor` | `composer.json`, contains `autoload.php` | dependencies | safe |
| `cocoapods` | dir | `Pods` | `Podfile`, contains `Manifest.lock` | dependencies | review |
| `terraform` | dir | `.terraform` | any `*.tf`, contains `providers` or `modules` | cache | review (holds provider state) |
| `swift-build` | dir | `.build` | `Package.swift` | build | safe |
| `maven-target` | dir | `target` | `pom.xml`, contains `maven-status`, `maven-archiver` or `classes` | build | safe |
| `gradle-build` | dir | `build` | `build.gradle`, `build.gradle.kts`, contains `intermediates`, `outputs`, `tmp` or `classes` | build | safe |
| `cmake-build` | dir | `build`, `build-*`, `cmake-build-*` | contains `CMakeCache.txt` | build | review (the cache holds configure options) |
| `unity-library` | dir | `Library` | `ProjectSettings`, contains `ArtifactDB`, `SourceAssetDB` or `PackageCache` | cache | review (reimport is slow) |
| `unity-temp` | dir | `Temp` | `ProjectSettings` | temp | review (in use while the editor is open) |
| `astro-docusaurus` | dir | `.astro`, `.docusaurus` | `package.json` | cache | safe |
| `storybook-static` | dir | `storybook-static` | `package.json` | build | safe |

`.DS_Store` and `Thumbs.db` are small but numerous. Text output groups file rules per directory already through the project column; check that the TUI list stays readable on a tree with thousands of them, and if not, collapse file matches per project into one row before adding the rules.

### Tests

Table-driven, in `src/detect/detector.rs` tests: for each new rule one positive case (pattern plus evidence) and one negative case (pattern without evidence, or evidence without the content check). One integration test in `tests/scan.rs` builds a tree with one project per ecosystem and asserts the rule names found.

## 2. `--json-lines`

A global flag in `src/cli.rs` next to `--json`, `conflicts_with = "json"`.

| Command | Output |
| --- | --- |
| `scan` | One `{"type":"candidate", ...}` line per `Event::Candidate` as it arrives, one `{"type":"issue", ...}` line per issue, then one `{"type":"summary", ...}` line. Disables the live TUI, like `--json`. |
| `clean` | One `{"type":"removal", ...}` line per item as it is removed, then the summary. |
| `cache`, `duplicates`, `rules`, `insight` | The same lines, printed after the work finishes, since these commands do not stream. |
| `dashboard`, `config` | Usage error: "`--json-lines` is not supported by `kuizer dashboard`, use `--json`". |

Each line reuses the existing `*Json` structs in `src/output/json.rs` with `#[serde(tag = "type")]` on a new `Line` enum. Lines are flushed after every write so `jq --stream` and `while read` loops see them immediately. Streaming `scan` goes through `scan::collect_with`, whose callback already sees each event.

## 3. JSON reference

`docs/json.md` documents every document kuizer prints: `scan`, `clean`/removal, `cache`, `duplicates`, `insight`, `rules`, `dashboard`, and the `--json-lines` line types. One table per document: field, type, meaning, since which version. The example in `design.md` section 6 moves there and `design.md` links to it.

Snapshot tests in `tests/json.rs` run each command on a fixed temp tree, replace the temp path with `<root>` and volatile fields (`duration`, timestamps, `newest_modified`) with fixed values, and compare against `tests/snapshots/<command>.json`. `KUIZER_UPDATE_SNAPSHOTS=1 cargo test --test json` rewrites them. No new dependency; a field added without updating the snapshot fails the test, which is the reminder to update `docs/json.md`.

## Accepted when

- Every rule has a positive and a negative test.
- `docs/json.md` exists and every documented command has a snapshot test.
- `kuizer --json-lines scan ~/apps | head -1` prints the first candidate before the scan finishes.
