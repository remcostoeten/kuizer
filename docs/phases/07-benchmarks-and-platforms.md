# Phase 7: benchmarks, fuzzing, macOS and Windows

Status: plan. CI runs fmt, clippy and tests on `ubuntu-latest` only. There is no `benches/` directory; `examples/synth.rs` generates synthetic trees and `design.md` section 9 records one warm-cache baseline.

This phase is three pull requests: benchmarks, fuzzing, platforms.

## 1. Benchmarks

### Local

- `benches/detector.rs` and `benches/hash.rs` with `criterion` (dev dependency): `Detector::classify_dir` and `classify_file` over a fixed set of listings, and BLAKE3 partial and full hashing over in-memory buffers of 4 KiB, 1 MiB and 64 MiB.
- `benches/scan.sh`: builds a seeded tree with `examples/synth.rs`, then runs `hyperfine --warmup 1 --runs 5` on `kuizer --json scan`, `kuizer --json duplicates` and `kuizer --json dashboard --no-open`, with `size_threads` 1, 2, 4 and 8. Prints a table for `design.md` section 9.
- `examples/synth.rs` gains a `--seed` flag and a `--entries` target so the dataset is the same on every machine, and a duplicate-heavy preset for phase 5's hashing.

### CI regression gate

Wall time on shared runners varies by 20 percent or more between runs, which hides real regressions. The gate measures instruction counts instead, which are stable to a fraction of a percent:

- `benches/ci.rs` with `iai-callgrind` (Linux only, needs `valgrind`), running a scan of a 50k entry synthetic tree and a duplicate pass over a 2k file tree, both generated into a temp dir by the bench itself.
- A `bench` job in `ci.yml` runs on pull requests, compares against the base branch with `iai-callgrind`'s baseline support and fails above a 5 percent instruction increase.

Criterion numbers stay local and go into `design.md` by hand.

### Questions this answers

The open measurements from section 9: cold cache numbers, `jwalk` against the single-threaded walker, `stat` against `statx`, and the cost of stat'ing every file, which phase 8's idle time needs.

## 2. Fuzzing

`fuzz/` with `cargo-fuzz` (nightly, outside the main workspace):

| Target | Input | Checks |
| --- | --- | --- |
| `config` | arbitrary bytes as TOML | `Config::parse` never panics; a parsed config round-trips through `to_toml`. |
| `byte_size` | arbitrary string | `ByteSize::from_str` never panics; `Display` output parses back to the same value. |
| `mountinfo` | arbitrary bytes | `platform::linux::parse_mountinfo` never panics. |
| `names` | arbitrary byte strings as file names | `Detector::classify_*` never panics; text output and the control character escaping in `src/output/text.rs` never emit a raw control character. |

A scheduled workflow (`fuzz.yml`, weekly) runs each target for five minutes and uploads crashes as artifacts. It does not run on pull requests.

## 3. macOS and Windows

First goal: compile and pass the test suite on both. Add `macos-latest` and `windows-latest` to the CI matrix and fix what breaks. Known gaps in `src/platform/mod.rs`:

| Function | Linux | macOS | Windows |
| --- | --- | --- | --- |
| `MountTable::load` | `/proc/self/mountinfo` | `getfsstat` through `rustix` or `libc`, giving mount point and fs type | Volume list is not needed: `one_file_system` compares volume serial numbers instead |
| `device` | `st_dev` | `st_dev` | volume serial number from `GetFileInformationByHandle` |
| `identity` | `(dev, ino)` | `(dev, ino)` | `(volume serial, file index)` from the same call |
| `allocated` | `st_blocks * 512` | `st_blocks * 512` | `GetCompressedFileSizeW`, which also handles sparse and compressed files |
| `available_space`, `total_space` | `statvfs` | `statvfs` | `GetDiskFreeSpaceExW` |
| `open_in_browser` | `xdg-open` | `open` | `cmd /C start` (done) |

Windows calls go through `windows-sys` with only the features used. Everything else stays behind the existing `cfg` split.

Other platform work:

- **Delete guard.** `src/delete/mod.rs` revalidates `identity` through `Guard::check`, then calls `fs::remove_dir_all`, which does not follow symlinks or junctions on any platform. On Windows `identity` is `None` today, so the inode swap check is skipped; it starts working once `identity` returns the file index. Also refuse a target whose attributes include `FILE_ATTRIBUTE_REPARSE_POINT`. Add a Windows case to `tests/delete.rs` that swaps a directory for a junction. `design.md` section 8 describes an `openat`/`unlinkat` deletion that was never built; either build it on Linux in this phase or change the threat model to what the code does.
- **Protected paths.** `src/protect.rs` covers Linux locations. Add `~/Library/Application Support/{Google/Chrome,BraveSoftware,Firefox,Code,discord,Slack}`, `~/Library/Keychains` and `~/Library/Containers` on macOS, and `%LOCALAPPDATA%\{Google\Chrome\User Data,BraveSoftware,Microsoft\Edge}`, `%APPDATA%\{Mozilla,Code,discord,Slack}` on Windows. The listing-based detection (`Local State`, `profiles.ini`) already works everywhere.
- **Caches.** `src/caches.rs` `Locations` uses `dirs` for cache and data directories, so most entries resolve. Add `~/Library/Caches/{Homebrew,pip,Yarn,ms-playwright}` and Xcode's `~/Library/Developer/Xcode/DerivedData` (review) on macOS; `%LOCALAPPDATA%\{npm-cache,pnpm-cache,Yarn\Cache,pip\Cache,uv\cache}` on Windows.
- **Trash.** `history::trashed_paths` returns `None` on macOS because the `trash` crate cannot list the macOS trash. The dashboard and `insight` already show "unknown" then.
- **Tests.** `tests/filesystem.rs` uses symlinks and hard links; gate symlink creation on Windows behind a check for developer mode and skip with a message otherwise.

## Accepted when

- `cargo bench` runs locally and `design.md` section 9 has cold and warm numbers from the Arch machine.
- The CI bench job fails a pull request that adds 5 percent instructions to the scan.
- The four fuzz targets exist and the weekly job runs.
- CI is green on Linux, macOS and Windows.
