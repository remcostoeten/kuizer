#!/usr/bin/env bash
set -euo pipefail

if [ $# -ne 3 ]; then
  printf 'usage: %s <version> <artifacts-dir> <output>\n' "$0" >&2
  exit 64
fi

version=$1
artifacts=$2
output=$3
template="$(dirname "$0")/kuizer-gui.rb.in"

if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.+-]+)?$ ]]; then
  printf 'not a version: %s\n' "$version" >&2
  exit 1
fi

checksum() {
  local file="$artifacts/kuizer-gui-$1-apple-darwin.tar.xz.sha256"
  local value
  value=$(cut -d ' ' -f 1 "$file")
  if [[ ! $value =~ ^[0-9a-f]{64}$ ]]; then
    printf 'no sha256 in %s\n' "$file" >&2
    return 1
  fi
  printf '%s' "$value"
}

arm=$(checksum aarch64)
intel=$(checksum x86_64)

mkdir -p "$(dirname "$output")"
sed -e "s/@VERSION@/$version/" -e "s/@SHA256_ARM@/$arm/" -e "s/@SHA256_INTEL@/$intel/" "$template" > "$output"
