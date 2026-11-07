# Contributing

kuizer is a Rust workspace: the core in `src/` (scan, detect, delete, config, history), the CLI and terminal UI in the `kuizer` crate, and the GPUI desktop app in `crates/kuizer-gui`. Read [docs/design.md](../docs/design.md) for the module boundaries before changing the core, and [AGENTS.md](../AGENTS.md) for the rules the project follows.

## Build and test

Rust 1.88 or newer. CI runs these:

```sh
cargo test --workspace --all-targets -j 2 -- --test-threads=2
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check
```

`./dev` is a keyboard menu for the same commands. On Linux the desktop app needs the X11, Wayland, Vulkan and font development packages; the README lists them per distribution.

## What a change needs

- Logic lives in the core. The GUI and the TUI render state and dispatch actions; neither walks the filesystem, matches rules or deletes files itself.
- The CLI and the TUI keep working. A GUI change that breaks either is not done.
- Deletion stays safe: scanning never deletes, every removal goes through `src/delete` and its checks, and tests touch only temp directories.
- A setting a user could have an opinion about gets a config key in the same change, documented in `docs/config.example.toml`.
- Every action has a keybinding.
- Tests cover behavior that would hurt if it regressed, not dependencies or trivial code.

## Pull requests

Open an issue first for anything beyond a fix, so the design is settled before the code exists. Keep a pull request to one change, say which frontends you ran it in, and squash it to one commit before it merges.
