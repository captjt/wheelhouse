#!/bin/bash
# Wrap the release binary as rune/target/Wheelhouse Rune.app: bundle id
# dev.wheelhouse.rune, a menu-bar-less accessory (LSUIElement), and no URL
# scheme, so it never answers wheelhouse:// links. It isn't installed
# anywhere: open it from rune/target, or run the binary directly.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -q
app="target/Wheelhouse Rune.app"
rm -rf "$app" && mkdir -p "$app/Contents/MacOS"
cp target/release/wheelhouse-rune "$app/Contents/MacOS/"
cat >"$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>dev.wheelhouse.rune</string>
  <key>CFBundleName</key><string>Wheelhouse Rune</string>
  <key>CFBundleExecutable</key><string>wheelhouse-rune</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.1</string>
  <key>LSUIElement</key><true/>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$app" >/dev/null 2>&1 || true
echo "$app"
