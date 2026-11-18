#!/bin/bash
# Wrap the release binary as rune/target/Wheelhouse Rune.app: bundle id
# dev.wheelhouse.rune, the Mac app's icon (Icon.swift's), and no URL scheme,
# so it never answers wheelhouse:// links. It isn't installed anywhere: open
# it from rune/target, or run the binary directly.
#
# Opened, it follows Wheelhouse Dev's hub (:7600, `just dev-install`): your
# dockets, with nothing live. WH_HUB here picks another, such as the made-up
# world's (WH_HUB=http://127.0.0.1:7458). It opens on the main window, with
# its wheel in the menu bar and its quick panel on ⌃⌥⌘K.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -q
app="target/Wheelhouse Rune.app"
rm -rf "$app" && mkdir -p "$app/Contents/MacOS"
cp target/release/wheelhouse-rune "$app/Contents/MacOS/"
mkdir -p "$app/Contents/Resources" target/AppIcon.iconset
(cd ../mac && swift Icon.swift "$OLDPWD/target/AppIcon.iconset" >/dev/null) && iconutil -c icns target/AppIcon.iconset -o "$app/Contents/Resources/AppIcon.icns" || true
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
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSEnvironment</key>
  <dict>
    <key>WH_HUB</key><string>HUB</string>
  </dict>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
</dict>
</plist>
PLIST
/usr/libexec/PlistBuddy -c "Set :LSEnvironment:WH_HUB ${WH_HUB:-http://127.0.0.1:7600}" "$app/Contents/Info.plist"
codesign --force --sign - "$app" >/dev/null 2>&1 || true
echo "$app"
