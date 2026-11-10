#!/usr/bin/env bash
#
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 Christophe Opoix
#
# Build zrush.app from this checkout: the desktop app as macOS expects one,
# with its icon in the Finder and the Dock, started by a double click
# rather than through Terminal.
#
#   tools/bundle-macos.sh              target/release/zrush.app
#   tools/bundle-macos.sh --install    and copy it to /Applications
#   tools/bundle-macos.sh --dmg        and put it in target/release/zrush.dmg,
#                                      ready to drag to Applications
#   tools/bundle-macos.sh --target x86_64-apple-darwin --dmg
#                                      the same for another architecture,
#                                      under target/<target>/release
#
# Signing: ad hoc by default, which changes with every build, so macOS asks
# again for the folders it was allowed. Sign with a certificate of your own
# to keep them: create a "Code Signing" certificate in Keychain Access
# (Certificate Assistant > Create a Certificate), then
#
#   ZRUSH_SIGN=zrush-dev tools/bundle-macos.sh --install
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
install=false
dmg=false
target=""
while [ $# -gt 0 ]; do
  case "$1" in
    --install) install=true ;;
    --dmg) dmg=true ;;
    --target) target="${2:?--target needs a target}"; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

[ "$(uname -s)" = Darwin ] || { echo "macOS only" >&2; exit 1; }

echo "building (release)…"
if [ -n "$target" ]; then
  cargo build --release --locked --manifest-path "$here/Cargo.toml" -p zrush-gui --target "$target"
  out="$here/target/$target/release"
else
  cargo build --release --locked --manifest-path "$here/Cargo.toml" -p zrush-gui
  out="$here/target/release"
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$here/crates/zrush-gui/Cargo.toml" | head -1)
app="$out/zrush.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$out/zrush-gui" "$app/Contents/MacOS/zrush"

# Every size the Finder and the Dock ask for, from the one 1024 px master.
iconset=$(mktemp -d)/zrush.iconset
mkdir -p "$iconset"
png="$here/crates/zrush-gui/assets/icon.png"
for size in 16 32 128 256 512; do
  sips -z $size $size "$png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z $double $double "$png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/zrush.icns"
rm -rf "$(dirname "$iconset")"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>zrush</string>
  <key>CFBundleDisplayName</key><string>zrush</string>
  <key>CFBundleIdentifier</key><string>io.github.christphe.zrush</string>
  <key>CFBundleExecutable</key><string>zrush</string>
  <key>CFBundleIconFile</key><string>zrush</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

# A bundle is signed as a whole: the binary's own ad hoc signature does
# not cover the Info.plist or the icon.
codesign --force --sign "${ZRUSH_SIGN:--}" --identifier io.github.christphe.zrush "$app"
echo "built $app (signed: ${ZRUSH_SIGN:-ad hoc})"

if $dmg; then
  # The app and a link to Applications, side by side: the window the disk
  # image opens on says what to do with it.
  stage=$(mktemp -d)/zrush
  mkdir -p "$stage"
  cp -R "$app" "$stage/"
  ln -s /Applications "$stage/Applications"
  image="$out/zrush.dmg"
  rm -f "$image"
  # hdiutil fails now and then on a busy runner ("Resource busy"): again.
  for try in 1 2 3; do
    if hdiutil create -quiet -volname zrush -srcfolder "$stage" -ov -format UDZO "$image"; then
      break
    fi
    [ "$try" = 3 ] && exit 1
    sleep 5
  done
  rm -rf "$(dirname "$stage")"
  echo "built $image"
fi

if $install; then
  rm -rf /Applications/zrush.app
  cp -R "$app" /Applications/zrush.app
  echo "installed /Applications/zrush.app"
fi
