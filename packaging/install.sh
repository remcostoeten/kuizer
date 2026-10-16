#!/bin/sh
set -eu

prefix="${PREFIX:-$HOME/.local}"
here="$(cd "$(dirname "$0")" && pwd)"

install -Dm755 "$here/bin/kuizer" "$prefix/bin/kuizer"
install -Dm755 "$here/bin/kuizer-gui" "$prefix/bin/kuizer-gui"
install -Dm644 "$here/share/applications/kuizer-gui.desktop" "$prefix/share/applications/kuizer-gui.desktop"
install -Dm644 "$here/share/icons/hicolor/scalable/apps/kuizer-gui.svg" "$prefix/share/icons/hicolor/scalable/apps/kuizer-gui.svg"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$prefix/share/applications" >/dev/null 2>&1 || true
fi

echo "installed kuizer and kuizer-gui to $prefix/bin"
case ":$PATH:" in
    *":$prefix/bin:"*) ;;
    *) echo "add $prefix/bin to your PATH to run them from a terminal" ;;
esac
