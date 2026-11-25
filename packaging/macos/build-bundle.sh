#!/usr/bin/env bash
set -euo pipefail

# Builds Kuizer.app (with iconset + Info.plist) and packages it into a .dmg.
# Signs ad-hoc by default (free, works on Apple Silicon without an Apple Developer account).
# When APPLE_SIGNING_IDENTITY is set, signs with Developer ID + hardened runtime, and when
# APPLE_ID / APPLE_TEAM_ID / APPLE_APP_PASSWORD are also set, notarizes and staples the DMG.
#
# Usage:
#   packaging/macos/build-bundle.sh <binary> <version> <target> <out-dir>

if [ $# -ne 4 ]; then
  printf 'usage: %s <binary> <version> <target> <out-dir>\n' "$0" >&2
  exit 64
fi

binary=$1
version=$2
target=$3
out_dir=$4
repo_root=$(cd "$(dirname "$0")/../.." && pwd)

if [ ! -f "$binary" ]; then
  printf 'binary not found: %s\n' "$binary" >&2
  exit 1
fi

mkdir -p "$out_dir"
work=$(mktemp -d "${TMPDIR:-/tmp}/kuizer-macos-bundle.XXXXXX")
trap 'rm -rf "$work"' EXIT

app="$work/Kuizer.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$binary" "$app/Contents/MacOS/kuizer-gui"
chmod 755 "$app/Contents/MacOS/kuizer-gui"

sed "s/@VERSION@/${version}/g" "$repo_root/packaging/macos/Info.plist.in" \
  > "$app/Contents/Info.plist"

# Build Kuizer.icns from the existing PNG icons using native macOS sips + iconutil.
iconset="$work/Kuizer.iconset"
mkdir -p "$iconset"
src_512="$repo_root/packaging/icons/512x512.png"
sips -z 16 16     "$src_512" --out "$iconset/icon_16x16.png"      >/dev/null
sips -z 32 32     "$src_512" --out "$iconset/icon_16x16@2x.png"   >/dev/null
sips -z 32 32     "$src_512" --out "$iconset/icon_32x32.png"      >/dev/null
sips -z 64 64     "$src_512" --out "$iconset/icon_32x32@2x.png"   >/dev/null
sips -z 128 128   "$src_512" --out "$iconset/icon_128x128.png"    >/dev/null
sips -z 256 256   "$src_512" --out "$iconset/icon_128x128@2x.png" >/dev/null
sips -z 256 256   "$src_512" --out "$iconset/icon_256x256.png"    >/dev/null
sips -z 512 512   "$src_512" --out "$iconset/icon_256x256@2x.png" >/dev/null
cp "$src_512"                      "$iconset/icon_512x512.png"
sips -z 1024 1024 "$src_512" --out "$iconset/icon_512x512@2x.png" >/dev/null
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Kuizer.icns"

if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  codesign --force --deep --options runtime --timestamp \
    --sign "$APPLE_SIGNING_IDENTITY" "$app"
else
  # Ad-hoc signature: free and required on Apple Silicon so macOS executes the bundle.
  codesign --force --deep --sign - "$app"
fi
codesign --verify --deep --strict "$app"

dmg_stage="$work/dmg"
mkdir -p "$dmg_stage"
cp -R "$app" "$dmg_stage/Kuizer.app"
ln -s /Applications "$dmg_stage/Applications"

dmg_name="kuizer-gui-${target}.dmg"
dmg_path="$out_dir/$dmg_name"
rm -f "$dmg_path"
hdiutil create \
  -volname "Kuizer ${version}" \
  -srcfolder "$dmg_stage" \
  -ov \
  -format UDZO \
  "$dmg_path" >/dev/null

if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dmg_path"
  if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
    xcrun notarytool submit "$dmg_path" \
      --apple-id "$APPLE_ID" \
      --team-id "$APPLE_TEAM_ID" \
      --password "$APPLE_APP_PASSWORD" \
      --wait
    xcrun stapler staple "$dmg_path"
  fi
fi

(cd "$out_dir" && shasum -a 256 "$dmg_name" > "${dmg_name}.sha256")
printf 'Built %s\n' "$dmg_path"
