# Phase 10: toolchain, Docker and big-file sources

Status: `toolchain` is done (`src/toolchains/`, `kuizer toolchain`, the GUI's Toolchains section) without the source abstraction: it follows the `cache` pattern and adds a `remove` function to `removal::Request`. It also lists SDKMAN candidates and Go SDKs, reads `.sdkmanrc`, `.tool-versions` and `go.mod` pins, and always removes permanently. The rest is a plan. Projects and global caches are sources today, each with its own code path (`src/commands/scan.rs`, `src/commands/cache.rs`). Duplicates is a report. Behaviour is specified in `design.md` sections 1 and 6.2.

This phase is four pull requests: the source abstraction, `toolchain`, `docker`, `big`.

## 1. Source abstraction

Every removable source lists candidates and removes them through the same filters, selection, confirmation and history. Today `cache.rs` builds `Candidate`s by hand and passes them to `removal::run`, which is the pattern to generalise.

```rust
pub enum SourceKind { Projects, Cache, Toolchain, Docker }

pub struct Listing {
    pub candidates: Vec<Candidate>,
    pub issues: Vec<Issue>,
}

pub trait Source {
    fn kind(&self) -> SourceKind;
    fn list(&self, config: &Config, roots: &[PathBuf]) -> Result<Listing, AppError>;
    fn remove(&self, candidate: &Candidate, mode: Mode) -> Result<(), DeleteError>;
}
```

- `src/sources/` holds `projects.rs`, `cache.rs`, `toolchain.rs`, `docker.rs`. Projects wraps `scan::start`; cache moves `measure` out of `src/commands/cache.rs`.
- The default `remove` goes through `delete::delete` with the guard. Docker overrides it.
- `Candidate` gains `source: SourceKind`, serialised as `"source"` in JSON (additive). `removal::run` and the history entry record it.
- Commands become thin: `kuizer toolchain` lists, `kuizer toolchain clean` removes, both with `FilterArgs` from phase 8.
- `Kind` in `src/detect/mod.rs` gains `External` for candidates that are not a path on disk (Docker objects). The guard and the text output's path column handle it.

## 2. `toolchain`

### What is listed

| Tool | Location | One candidate per |
| --- | --- | --- |
| rustup | `$RUSTUP_HOME/toolchains/*`, default `~/.rustup` | toolchain |
| nvm | `$NVM_DIR/versions/node/*`, default `~/.nvm` | Node version |
| fnm | `$FNM_DIR/node-versions/*`, default `~/.local/share/fnm` | Node version |
| volta | `~/.volta/tools/image/node/*` | Node version |
| pyenv | `$PYENV_ROOT/versions/*`, default `~/.pyenv` | Python version |
| uv | `~/.local/share/uv/python/*` | Python version |

Sizes come from `scan::measure`, as for caches.

### Safety

- The default or global version is never `safe`: rustup's `default_toolchain` in `settings.toml`, nvm's `alias/default`, fnm's `aliases/default`, volta's `~/.volta/tools/user/platform.json`, pyenv's `version` file, uv's default. These are `review` with the note "default".
- A version pinned by a file under the scan roots is `review` with the note "pinned by ~/apps/web/.nvmrc". Pin files: `rust-toolchain`, `rust-toolchain.toml`, `.nvmrc`, `.node-version`, `.python-version`, and the `volta` key in `package.json`. The pin search reuses the projects walk with a small detector that records these files instead of artifacts.
- Everything else is `safe`: the tool downloads it again on demand.

### Removal

rustup and uv keep metadata outside the version directory. Removal uses their CLI when it is on `PATH` (`rustup toolchain uninstall NAME`, `uv python uninstall VERSION`) and fails the item with a clear message when it is not. nvm, fnm, volta and pyenv versions are self-contained directories and go through the normal trash or permanent delete.

Idle time: none of these tools record last use, so toolchain activity is unknown and `--idle` never matches, as `design.md` section 6.3 allows.

## 3. `docker`

### What is listed

| Item | Command | Candidate |
| --- | --- | --- |
| Dangling images | `docker image ls --filter dangling=true --format json` | one per image id, size from the listing |
| Stopped containers | `docker container ls -a --filter status=exited --filter status=created --format json` | one per container |
| Build cache | `docker system df --format json` | one candidate for the whole build cache |

The path is `docker:image/<id>`, `docker:container/<id>`, `docker:build-cache`, with `Kind::External`. Every item is `safe` except containers that exited within the last 24 hours, which are `review`.

### Removal

`docker image rm <id>`, `docker container rm <id>`, `docker builder prune --force`. Never touch `/var/lib/docker`. Every Docker removal is permanent, so the confirmation always shows and the history records `mode: permanent`; `--permanent` is implied and the trash mode is refused with a clear message.

### No daemon

Each `docker` call has a 10 second timeout. A missing binary or an unreachable daemon gives an empty listing, one issue ("docker is not running"), and exit code `0`. `DOCKER_HOST` and contexts are honoured because kuizer only runs the CLI.

## 4. `big`

Large files that no artifact rule explains, for example a forgotten ISO in `~/Downloads`. Report only, no `clean`, per `design.md` section 6.2.

- `kuizer big [PATHS...] --min-size SIZE --limit N`, default `--min-size 500MB`, `--limit 50`.
- The file walk in `src/dupes/mod.rs` (`Walk`) already visits every file with its size while skipping artifacts, excludes and protected paths. Move it to `src/scan/files.rs` and use it for both duplicates and big. Big keeps only files at or above `min_size`.
- Output: size, last modified, path. JSON uses the candidate shape with `source: "big"` and no rule.

## 5. Dashboard

One tab per source with data: Toolchains (grouped by tool, default and pinned versions marked), Docker (images, containers, build cache) and Big files (largest first). The overview's "where the space is" bar gets a segment per source.

## Tests

- Toolchain: a fake home with `.rustup/toolchains/{stable,nightly,1.80.0}`, `settings.toml` naming `stable` as default, and a project under the roots with `rust-toolchain.toml` pinning `1.80.0`. Assert `stable` and `1.80.0` are `review`, `nightly` is `safe`. One negative test: no tool directories, empty listing.
- Docker: put a fake `docker` script on `PATH` in the test that prints fixed JSON, and one that exits with "Cannot connect to the Docker daemon". Assert the listing and the single issue with exit code `0`.
- Big: a temp tree with a large sparse file outside any project and one inside `node_modules`; only the first is reported.

## Accepted when

Each source has a positive and a negative test, a pinned toolchain is never `safe`, and `kuizer docker` without a daemon reports one issue and exits `0`.
