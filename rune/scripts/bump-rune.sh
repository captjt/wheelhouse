#!/bin/bash
# Move rune/ to another Rune rev (default: dev's head): every pin in
# Cargo.toml together, after checking Rune's own [patch.crates-io] at that rev
# still names the crates we patch. Then build and test, timing each.
#
#   rune/scripts/bump-rune.sh [REV]
set -euo pipefail
cd "$(dirname "$0")/.."
repo=https://github.com/anomalyco/rune
rev=${1:-$(git ls-remote "$repo" refs/heads/dev | cut -f1)}
old=$(grep -m1 -o 'rev = "[0-9a-f]\{40\}"' Cargo.toml | cut -d'"' -f2)
[ "$rev" = "$old" ] && { echo "already at $rev"; exit 0; }
echo "Rune $old → $rev"
# Rune's patch table at the new rev, against ours.
theirs=$(curl -fsSL "https://raw.githubusercontent.com/anomalyco/rune/$rev/Cargo.toml" \
  | awk '/^\[patch.crates-io\]/{p=1; next} /^\[/{p=0} p && /^[a-z_-]+ *=/{print $1}' | sort)
ours=$(awk '/^\[patch.crates-io\]/{p=1; next} /^\[/{p=0} p && /^[a-z_-]+ *=/{print $1}' Cargo.toml | sort)
if [ "$theirs" != "$ours" ]; then
  echo "Rune's [patch.crates-io] changed; patch these, then run again:" >&2
  diff <(echo "$ours") <(echo "$theirs") >&2 || true
  exit 1
fi
sed -i '' "s/$old/$rev/g" Cargo.toml
echo "commits: $(curl -fsSL "https://api.github.com/repos/anomalyco/rune/compare/$old...$rev" | jq '.total_commits')"
start=$(date +%s)
cargo update -q
cargo build --release 2>&1 | tail -3
echo "build: $(( $(date +%s) - start )) s"
cargo test --release -q 2>&1 | tail -2
