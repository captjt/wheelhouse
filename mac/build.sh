#!/usr/bin/env bash
# Build Wheelhouse.app: the release binary, its bundle, and its icon, signed
# for this Mac. `./build.sh --install` puts it in ~/Applications.
set -euo pipefail
cd "$(dirname "$0")"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' ../Cargo.toml | head -1)
build=$(git rev-list --count HEAD 2>/dev/null || echo 1)

swift build -c release --product Wheelhouse
bin="$(swift build -c release --show-bin-path)/Wheelhouse"

app=build/Wheelhouse.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/Wheelhouse"
sed -e "s/@VERSION@/${version:-0.1.0}/" -e "s/@BUILD@/$build/" Info.plist > "$app/Contents/Info.plist"

# The icon, drawn by Icon.swift at every size macOS asks for.
iconset=build/AppIcon.iconset
rm -rf "$iconset"
swift Icon.swift "$iconset" >/dev/null
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"

# Signed for this Mac only (ad hoc): notifications and login items need a
# signed bundle with a stable identifier.
codesign --force --sign - --timestamp=none "$app" >/dev/null
echo "built $app"

if [[ "${1:-}" == "--install" ]]; then
  dest="$HOME/Applications/Wheelhouse.app"
  mkdir -p "$HOME/Applications"
  pkill -x Wheelhouse 2>/dev/null && sleep 0.5 || true
  rm -rf "$dest"
  cp -R "$app" "$dest"
  echo "installed $dest"
fi
