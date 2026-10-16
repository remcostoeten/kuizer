#!/bin/sh
set -eu

version="$1"
target="$2"
out="${3:-dist}"

root="$(cd "$(dirname "$0")/.." && pwd)"
name="kuizer-$version-$target"
stage="$out/$name"

rm -rf "$stage"
install -Dm755 "$root/target/release/kuizer" "$stage/bin/kuizer"
install -Dm755 "$root/target/release/kuizer-gui" "$stage/bin/kuizer-gui"
install -Dm644 "$root/packaging/kuizer-gui.desktop" "$stage/share/applications/kuizer-gui.desktop"
install -Dm644 "$root/packaging/kuizer-gui.svg" "$stage/share/icons/hicolor/scalable/apps/kuizer-gui.svg"
install -Dm755 "$root/packaging/install.sh" "$stage/install.sh"
install -Dm644 "$root/README.md" "$stage/README.md"
install -Dm644 "$root/LICENSE" "$stage/LICENSE"

tar -C "$out" -czf "$out/$name.tar.gz" "$name"
(cd "$out" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "$out/$name.tar.gz"
