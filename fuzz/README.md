# Fuzzing kuizer

This package lives outside the main Cargo workspace so fuzzing adds no dependencies to `kuizer` or `kuizer-gui`.

## Targets

- `config_lenient`: exercises `Config::parse_lenient` (including the cautious delete fallback on broken TOML syntax) and `config::fix_text` idempotency on arbitrary UTF-8 input.
- `hostile_names`: exercises `platform::display_path` (with arbitrary Unix byte paths) and `platform::sanitize_text`, asserting that neither panics nor leaks any Unicode/ASCII control characters.

## Running

Requires nightly Rust and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz):

```sh
cargo +nightly fuzz run config_lenient -- -max_total_time=60
cargo +nightly fuzz run hostile_names -- -max_total_time=60
```
