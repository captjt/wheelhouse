#!/bin/bash
# Both apps on one hub, screen by screen, light and dark: Wheelhouse.app
# (SwiftUI) through its own --snapshot, the Rune app headlessly. Writes each
# pair side by side (SwiftUI left, Rune right) into OUT, with a list the
# review page reads.
#
#   rune/scripts/compare.sh [OUT]          # default .tmp/compare; WH_HUB picks the hub
#   ONLY="inbox docket" rune/scripts/compare.sh
#   SKIP_SWIFT=1 rune/scripts/compare.sh   # keep the SwiftUI shots already in OUT/swift
#
# The SwiftUI shots need Xcode 26 and screen recording for the terminal (as
# scripts/screenshots.sh's do); its windows stay behind yours and never take
# the keyboard. Never the daily hub on :7500.
set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD
hub=${WH_HUB:-http://127.0.0.1:7458}
case "$hub" in *:7500 | *:7500/*) echo "compare.sh: $hub is the daily hub" >&2; exit 2 ;; esac
out=${1:-.tmp/compare}
mkdir -p "$out/swift" "$out/rune" "$out/pairs"

# swift shot | rune window | rune scene | size | what it is
pairs=(
  "inbox|main|inbox|1440x900|Inbox: what needs you, and the first of it"
  "docket|main|docket|1440x900|A docket: intent, decisions, Verify, kept files, activity, inspector"
  "chat|main|chat|1440x900|A docket's chat with its agent"
  "reviews|main|reviews|1440x900|Reviews, by phase, beside a Review docket"
  "board|main|board|1440x900|All dockets as a board"
  "recap|main|recap|1440x900|Recap"
  "palette|main|palette|1440x900|⌘K over the window"
  "record|main|record|1440x900|A docket's record"
  "envelope|main|record-open|1440x900|A record entry, open to its signed envelope"
  "agent|main|agent|1440x900|New docket: an agent"
  "start-agent|main|start-agent|1440x900|Start an agent on a docket"
  "hold|main|hold|1440x900|Hold a docket"
  "closing|main|close|1440x900|Done refused: close on your word"
  "shortcuts|main|shortcuts|1440x900|Keyboard shortcuts"
  "verify|main|verify|1440x900|Verify: checks, witnessed"
  "verify-held|main|verify-held|1440x900|Verify on a docket on hold"
  "output|main|output|1440x900|A check's output"
  "window|docket|review|1000x840|A docket in a window of its own"
  "menubar|menubar|inbox|380x324|The menu bar item's panel"
  "quick|panel||648x520|The quick panel from any app (⌥⌘K; ⌃⌥⌘K in Rune)"
  "settings-general|settings|general|720x560|Settings: General"
  "settings-orders|settings|orders|720x560|Settings: Standing Orders"
  "settings-rule|settings|rule|720x560|Settings: a rule's editor"
  "settings-agents|settings|agents|720x560|Settings: Agents"
  "settings-repos|settings|repos|720x560|Settings: Repos"
  "settings-check|settings|check|720x560|Settings: a repo's check"
  "settings-posting|settings|posting|720x560|Settings: who posts a checked review"
  "settings-guides|settings|guides|720x560|Settings: Guides"
  "settings-brief|settings|brief|720x560|Settings: the agents' brief"
)
only=${ONLY:-}
keep() { [ -z "$only" ] || [[ " $only " == *" $1 "* ]]; }

names=()
for p in "${pairs[@]}"; do
  IFS='|' read -r s _ _ _ _ <<<"$p"
  keep "$s" && names+=("$s")
done

if [ -z "${SKIP_SWIFT:-}" ]; then
  echo "SwiftUI: $hub"
  (cd mac && swift build -c release --product Wheelhouse >/dev/null)
  WH_APP_HUB="$hub" mac/.build/release/Wheelhouse --snapshot "$root/$out/swift" --only "$(IFS=,; echo "${names[*]}")" -AppleShowScrollBars Always | sed "s|$root/||"
fi

echo "Rune: $hub"
(cd rune && cargo build --release -q)
bin=rune/target/release/wheelhouse-rune
: >"$out/pairs.tsv"
for p in "${pairs[@]}"; do
  IFS='|' read -r s window scene size what <<<"$p"
  keep "$s" || continue
  for theme in light dark; do
    r="$out/rune/$s-$theme.png"
    rm -f "$r"
    args=(--window "$window" --theme "$theme" --size "$size" --scale 1 --scroll-bars always --at 0.8 --snap "$r")
    [ -n "$scene" ] && args+=(--scene "$scene")
    WH_HUB="$hub" $bin "${args[@]}" >/dev/null 2>&1 || true
    sw="$out/swift/mac-$s-$theme.png"
    pair="$out/pairs/$s-$theme.png"
    if [ -f "$sw" ] && [ -f "$r" ]; then
      # Side by side at one height, each padded on a neutral ground.
      ffmpeg -loglevel error -y -i "$sw" -i "$r" -filter_complex \
        "[0]scale=-1:900:force_original_aspect_ratio=decrease,pad=iw+40:940:20:20:color=0x888888@0.15[a];[1]scale=-1:900:force_original_aspect_ratio=decrease,pad=iw+40:940:20:20:color=0x888888@0.15[b];[a][b]hstack" "$pair"
    fi
    printf '%s\t%s\t%s\t%s\t%s\n' "$s" "$theme" "$( [ -f "$sw" ] && echo "$sw" )" "$( [ -f "$r" ] && echo "$r" )" "$what" >>"$out/pairs.tsv"
  done
  echo "  $s"
done
echo "wrote $out/pairs.tsv"
