#!/bin/sh
set -eu

repository=remcostoeten/kuizer
version=${KUIZER_VERSION:-}
temporary=$(mktemp -d "${TMPDIR:-/tmp}/kuizer-gui.XXXXXX")
trap 'rm -rf "$temporary"' 0 HUP INT TERM

download() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$2" -o "$1"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$1" "$2"
  else
    printf '%s\n' 'kuizer GUI installation needs curl or wget.' >&2
    exit 1
  fi
}

if [ -z "$version" ]; then
  download "$temporary/release.json" "https://api.github.com/repos/$repository/releases/latest"
  version=$(sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' "$temporary/release.json" | head -n 1)
fi

if [ -z "$version" ]; then
  printf '%s\n' 'Could not determine the latest kuizer release.' >&2
  exit 1
fi

case "$(uname -s):$(uname -m)" in
  Linux:x86_64) target=x86_64-unknown-linux-gnu ;;
  Linux:aarch64 | Linux:arm64) target=aarch64-unknown-linux-gnu ;;
  *)
    printf '%s\n' 'The kuizer desktop installer supports Linux x86_64 and aarch64.' >&2
    exit 1
    ;;
esac

release="https://github.com/$repository/releases/download/$version"
archive="kuizer-gui-$target.tar.xz"
download "$temporary/$archive" "$release/$archive"
download "$temporary/$archive.sha256" "$release/$archive.sha256"
(cd "$temporary" && sha256sum -c "$archive.sha256")
tar -xJf "$temporary/$archive" -C "$temporary"
binary=$(find "$temporary" -type f -name kuizer-gui -print -quit)

if [ -z "$binary" ]; then
  printf '%s\n' 'The release archive did not contain kuizer-gui.' >&2
  exit 1
fi

bin_dir="$HOME/.local/bin"
app_dir="$HOME/.local/share/applications"
icon_dir="$HOME/.local/share/icons/hicolor"
download "$temporary/kuizer-gui.desktop" "https://raw.githubusercontent.com/$repository/$version/packaging/kuizer-gui.desktop"
download "$temporary/kuizer-gui.svg" "https://raw.githubusercontent.com/$repository/$version/packaging/kuizer-gui.svg"
for size in 32 128 256 512; do
  download "$temporary/$size.png" "https://raw.githubusercontent.com/$repository/$version/packaging/icons/$size"'x'"$size.png"
done

install -Dm755 "$binary" "$bin_dir/kuizer-gui"
exec_path=$(printf '%s' "$bin_dir/kuizer-gui" | sed 's/\\/\\\\/g; s/"/\\"/g; s/%/%%/g')
while IFS= read -r line || [ -n "$line" ]; do
  case "$line" in
    Exec=*) printf 'Exec="%s"\n' "$exec_path" ;;
    *) printf '%s\n' "$line" ;;
  esac
done < "$temporary/kuizer-gui.desktop" > "$temporary/installed.desktop"
install -Dm644 "$temporary/installed.desktop" "$app_dir/kuizer-gui.desktop"
install -Dm644 "$temporary/kuizer-gui.svg" "$icon_dir/scalable/apps/kuizer-gui.svg"

for size in 32 128 256 512; do
  install -Dm644 "$temporary/$size.png" "$icon_dir/$size"'x'"$size/apps/kuizer-gui.png"
done

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$app_dir" >/dev/null 2>&1 || true
fi

printf 'Installed kuizer-gui %s. Open it from your desktop menu or run %s/kuizer-gui.\n' "$version" "$bin_dir"
