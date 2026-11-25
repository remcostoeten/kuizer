#!/usr/bin/env bash
set -euo pipefail

if [ $# -ne 3 ]; then
  printf 'usage: %s <version> <checksums-dir> <repo-root>\n' "$0" >&2
  exit 64
fi

version=$1
checksums=$2
root=$3
templates=$(dirname "$0")

if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.+-]+)?$ ]]; then
  printf 'not a version: %s\n' "$version" >&2
  exit 1
fi

checksum() {
  local file="$checksums/$1.sha256"
  local value
  value=$(cut -d ' ' -f 1 "$file")
  if [[ ! $value =~ ^[0-9a-f]{64}$ ]]; then
    printf 'no sha256 in %s\n' "$file" >&2
    return 1
  fi
  printf '%s' "$value"
}

render() {
  local template=$1
  local output=$2
  local text asset value assets
  text=$(<"$template")
  text=${text//@VERSION@/$version}
  # Placeholders look like @SHA256(kuizer-x86_64-apple-darwin.tar.xz)@.
  mapfile -t assets < <(grep -o '@SHA256([^)]*)@' "$template" | sed -e 's/^@SHA256(//' -e 's/)@$//' | sort -u)
  for asset in "${assets[@]}"; do
    value=$(checksum "$asset")
    text=${text//"@SHA256($asset)@"/$value}
  done
  if [[ $text =~ @[A-Z0-9_]+(\([^\)]*\))?@ ]]; then
    printf 'unfilled placeholder %s in %s\n' "${BASH_REMATCH[0]}" "$template" >&2
    return 1
  fi
  mkdir -p "$(dirname "$output")"
  printf '%s\n' "$text" > "$output"
}

render "$templates/kuizer.rb.in" "$root/Formula/kuizer.rb"
render "$templates/kuizer-gui.rb.in" "$root/Casks/kuizer-gui.rb"
