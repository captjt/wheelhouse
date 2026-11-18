#!/usr/bin/env bash
# Build Wheelhouse.app: the release binary, its bundle, and its icon, signed
# for this Mac. `./build.sh --install` puts it in ~/Applications.
#
# `--dev` builds Wheelhouse Dev.app instead, to run beside the daily one
# (`just dev-install`): its own name, executable, bundle id, wheelhouse-dev://
# scheme, a blue icon and a DEV mark, ⇧⌥⌘K, the dev hub (:7600) as its
# default, and `wh-dev` as its wh. Installing it never touches Wheelhouse.app.
set -euo pipefail
cd "$(dirname "$0")"

dev=""
install=""
for arg in "$@"; do
  case $arg in
    --dev) dev=1 ;;
    --install) install=1 ;;
    *) echo "build.sh: unknown $arg (--dev, --install)" >&2; exit 2 ;;
  esac
done
if [[ -n "$dev" ]]; then
  name="Wheelhouse Dev" id=dev.wheelhouse.app.dev scheme=wheelhouse-dev
else
  name="Wheelhouse" id=dev.wheelhouse.app scheme=wheelhouse
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' ../Cargo.toml | head -1)
build=$(git rev-list --count HEAD 2>/dev/null || echo 1)

swift build -c release --product Wheelhouse
bin="$(swift build -c release --show-bin-path)/Wheelhouse"

app="build/$name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
# The executable is named for the app, so `pkill -x Wheelhouse` never stops Dev.
cp "$bin" "$app/Contents/MacOS/$name"
plist="$app/Contents/Info.plist"
sed -e "s/@VERSION@/${version:-0.1.0}/" -e "s/@BUILD@/$build/" Info.plist > "$plist"
if [[ -n "$dev" ]]; then
  pb() { /usr/libexec/PlistBuddy -c "$1" "$plist"; }
  pb "Set :CFBundleName $name"
  pb "Set :CFBundleDisplayName $name"
  pb "Set :CFBundleExecutable $name"
  pb "Set :CFBundleIdentifier $id"
  pb "Set :CFBundleURLTypes:0:CFBundleURLName $id"
  pb "Set :CFBundleURLTypes:0:CFBundleURLSchemes:0 $scheme"
  # Read by Flavor.swift.
  pb "Add :WHFlavor string dev"
  pb "Add :WHURLScheme string $scheme"
  pb "Add :WHDefaultHub string ${WH_DEV_HUB:-http://127.0.0.1:7600}"
  pb "Add :WHCommand string ${WH_DEV_COMMAND:-~/.local/bin/wh-dev}"
fi

# The icon, drawn by Icon.swift at every size macOS asks for (blue for Dev).
iconset=build/AppIcon.iconset
rm -rf "$iconset"
swift Icon.swift "$iconset" ${dev:+dev} >/dev/null
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"

# Signed for this Mac only (ad hoc): notifications and login items need a
# signed bundle with a stable identifier.
codesign --force --sign - --timestamp=none "$app" >/dev/null
echo "built $app"

if [[ -n "$install" ]]; then
  dest="$HOME/Applications/$name.app"
  mkdir -p "$HOME/Applications"
  pkill -x "$name" 2>/dev/null && sleep 0.5 || true
  rm -rf "$dest"
  cp -R "$app" "$dest"
  echo "installed $dest"
fi
