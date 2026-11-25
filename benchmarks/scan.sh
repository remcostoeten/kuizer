#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
temp_dir=$(mktemp -d)
trap 'rm -rf "$temp_dir"' EXIT

cargo build --locked --release --bin kuizer --example synth -j 2 --manifest-path "$repo_root/Cargo.toml"
"$repo_root/target/release/examples/synth" \
  "$temp_dir/tree" \
  200 \
  100

cat > "$temp_dir/config.toml" <<'EOF'
[scan]
size_threads = 4
EOF

command=(
  "$repo_root/target/release/kuizer"
  --config "$temp_dir/config.toml"
  scan
  --json
  --no-live
  "$temp_dir/tree"
)
printf -v benchmark_command '%q ' "${command[@]}"
hyperfine \
  --warmup 1 \
  --runs 5 \
  --export-json "$temp_dir/scan.json" \
  "$benchmark_command"

python3 "$repo_root/benchmarks/check_scan.py" "$temp_dir/scan.json"
