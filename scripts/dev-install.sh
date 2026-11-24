#!/bin/bash
# Wheelhouse Dev: this checkout's hub and Mac app, installed beside the daily
# ones and never meeting them. Run it again to update to this checkout.
#
#   scripts/dev-install.sh            # build, install, start; the first time, copy your dockets in
#   scripts/dev-install.sh --no-app   # the hub and wh-dev only
#   scripts/dev-install.sh --remove   # take it away (its dockets stay, in ~/.local/state/wheelhouse-dev)
#   wh-dev sync                       # copy the daily dockets in again
#
# What it is, beside the daily one:
#
#   wh-dev                   ~/.local/bin/wh-dev runs ~/.local/share/wheelhouse-dev/bin/wh with:
#   state, socket            ~/.local/state/wheelhouse-dev            (daily: ~/.local/state/wheelhouse)
#   config                   ~/.config/wheelhouse-dev                 (daily: ~/.config/wheelhouse)
#   address                  127.0.0.1:7600, no tailnet               (daily: :7500, and :7501 for the tailnet)
#   LaunchAgent, log         dev.wheelhouse.hub.dev, ~/Library/Logs/dev.wheelhouse.hub.dev.log
#   app                      ~/Applications/Wheelhouse Dev.app: dev.wheelhouse.app.dev, wheelhouse-dev://, ⇧⌥⌘K
#
# Nothing live: Rex, gh, Linear, Tailscale and notifications are off, and
# Review and Build have stand-in configs, so it never reads or types into a
# pane, posts to GitHub, or reaches your phone. Its dockets are a copy of the
# daily ones (SQLite's online backup: the daily hub keeps running). The
# agents' hooks keep reporting to the daily hub; wh-dev won't install its own.
set -euo pipefail
umask 077

root=$(cd "$(dirname "$0")/.." && pwd)
share="$HOME/.local/share/wheelhouse-dev"
state="$HOME/.local/state/wheelhouse-dev"
config="$HOME/.config/wheelhouse-dev"
wrapper="$HOME/.local/bin/wh-dev"
label=dev.wheelhouse.hub.dev
port=${WH_DEV_PORT:-7600}
app="$HOME/Applications/Wheelhouse Dev.app"
daily_state="$HOME/.local/state/wheelhouse"
daily_config="$HOME/.config/wheelhouse"
domain="gui/$(id -u)"

say() { printf '%s\n' "$*"; }

# A copy of the daily dockets: SQLite's online backup (the daily hub keeps
# running and is only read), its signing keys so the records still verify,
# and no push subscriptions, so the copy can never reach your phone.
sync() { # [--stopped]
  command -v sqlite3 >/dev/null || { say "dev-install.sh: needs sqlite3 to copy the dockets" >&2; exit 1; }
  [ -f "$daily_state/wheelhouse.db" ] || { say "no daily dockets in $daily_state: Wheelhouse Dev starts empty"; return 0; }
  local was_up=""
  if [ "${1:-}" != --stopped ] && launchctl print "$domain/$label" >/dev/null 2>&1; then
    was_up=1
    "$wrapper" down >/dev/null
  fi
  mkdir -p "$state" && chmod 700 "$state"
  rm -f "$state/wheelhouse.db.new"
  sqlite3 "$daily_state/wheelhouse.db" ".backup '$state/wheelhouse.db.new'"
  sqlite3 "$state/wheelhouse.db.new" "DELETE FROM push_subs;" 2>/dev/null || true
  sqlite3 "$state/wheelhouse.db.new" "PRAGMA wal_checkpoint(TRUNCATE);" >/dev/null
  rm -f "$state/wheelhouse.db.new-wal" "$state/wheelhouse.db.new-shm" "$state/wheelhouse.db-wal" "$state/wheelhouse.db-shm"
  mv -f "$state/wheelhouse.db.new" "$state/wheelhouse.db"
  if [ -d "$daily_state/keys" ]; then
    rm -rf "$state/keys" && cp -R "$daily_state/keys" "$state/keys" && chmod -R go-rwx "$state/keys"
  fi
  say "copied the daily dockets into Wheelhouse Dev ($(du -h "$state/wheelhouse.db" | cut -f1))"
  if [ -n "$was_up" ]; then
    "$wrapper" up
    pause
  fi
}

# Review and Build paused on the dev hub, whatever the copied settings say:
# with no real repo, gh or Rex they can't act, and paused they don't try.
pause() {
  local api="http://127.0.0.1:$port/api"
  for what in review build; do
    curl -fsS -H 'x-wheelhouse: dev-install' -H 'content-type: application/json' -X POST "$api/$what/settings" -d '{"paused": true}' >/dev/null ||
      say "couldn't pause $what on the dev hub" >&2
  done
}

case "${1:-}" in
  --sync)
    sync "${2:-}"
    exit 0
    ;;
  --remove)
    "$wrapper" down 2>/dev/null || launchctl bootout "$domain/$label" 2>/dev/null || true
    rm -f "$HOME/Library/LaunchAgents/$label.plist"
    pkill -x "Wheelhouse Dev" 2>/dev/null || true
    rm -rf "$app" "$share"
    rm -f "$wrapper"
    say "Wheelhouse Dev removed. Its dockets and config stay in $state and $config: delete them to start over."
    exit 0
    ;;
  --no-app | "") ;;
  *) say "dev-install.sh: unknown $1 (--no-app, --remove)" >&2; exit 2 ;;
esac

# Never the daily one's, whatever the environment says.
for p in "$share" "$state" "$config"; do
  case "$p" in "$daily_state" | "$daily_config" | "$HOME/.local/share/wheelhouse") say "refusing: $p is the daily install's" >&2; exit 1 ;; esac
done
[ "$port" = 7500 ] || [ "$port" = 7501 ] && { say "refusing: :$port is the daily hub's" >&2; exit 1; }

# --- The hub -----------------------------------------------------------------

command -v npm >/dev/null || PATH="$(ls -d "$HOME"/.nvm/versions/node/*/bin 2>/dev/null | tail -1):$PATH"
command -v npm >/dev/null || { say "dev-install.sh: needs Node (npm) for the web app" >&2; exit 1; }
say "building the hub and its web app from $(git -C "$root" rev-parse --abbrev-ref HEAD) $(git -C "$root" rev-parse --short HEAD)"
(cd "$root/web" && { [ -d node_modules ] || npm ci --no-audit --no-fund --silent; } && npm run build --silent >/dev/null)
cargo build --release --locked --quiet --manifest-path "$root/Cargo.toml"
mkdir -p "$share/bin" "$(dirname "$wrapper")"
# Beside the old one, then over it: the running dev hub keeps its file.
cp "$root/target/release/wh" "$share/bin/wh.new" && mv -f "$share/bin/wh.new" "$share/bin/wh"
cp "$root/scripts/dev-install.sh" "$share/dev-install.sh"

cat >"$wrapper" <<EOF
#!/bin/sh
# wh for Wheelhouse Dev (scripts/dev-install.sh): its own state, config,
# socket, port and LaunchAgent, beside the daily hub and never on it. Nothing
# live: Rex, gh, Linear, Tailscale and notifications are off.
export WH_LABEL=$label
export WH_FLAVOR=dev
export WH_STATE="$state"
export WH_SOCKET="$state/wh.sock"
export WH_HTTP=127.0.0.1:$port
export WH_CONFIG="$config"
export WH_SETTINGS="$config/settings.json"
export WH_REVIEW_CONFIG="$config/review.lua"
export WH_BUILD_CONFIG="$config/build.lua"
export WH_REVIEW_DATA="$share/review"
export WH_BUILD_DATA="$share/build"
export WH_REVIEW_CACHE="$share/cache"
export WH_REVIEW_SESSION=wheelhouse-dev-reviews
export WH_BUILD_SESSION=wheelhouse-dev-builds
export WH_TAILNET=off
export WH_NOTIFY=off
export WH_LINEAR_OFF=1
export WH_REX=/usr/bin/false
export WH_HC=/usr/bin/false
export WH_HERDR_APP=/usr/bin/false
export WH_GH=/usr/bin/false
export WH_TAILSCALE=/usr/bin/false
case "\$1 \${2:-}" in
  "hooks install"* | "hooks uninstall"* | "skill install"* | "skill uninstall"*)
    echo "wh-dev: the agents' hooks and skill belong to the daily wh; Wheelhouse Dev leaves them alone" >&2
    exit 1 ;;
  "sync "*) exec "$share/dev-install.sh" --sync ;;
esac
exec "$share/bin/wh" "\$@"
EOF
chmod +x "$wrapper"

# Its config: the daily guides, and stand-ins for Review and Build that name
# no real repo, so their loops never touch a real PR, pane or worktree.
mkdir -p "$config"
[ -d "$config/guides" ] || { [ -d "$daily_config/guides" ] && cp -R "$daily_config/guides" "$config/guides"; } || true
[ -f "$config/review.lua" ] || cat >"$config/review.lua" <<EOF
-- Wheelhouse Dev's stand-in: a repo it never reviews (scripts/dev-install.sh).
return { repos = { ["o/api"] = { checkout = "$config/none" } }, reviewers = { "claude" }, checker = "claude", post = "ask" }
EOF
[ -f "$config/build.lua" ] || cat >"$config/build.lua" <<EOF
-- Wheelhouse Dev's stand-in: a repo it never builds (scripts/dev-install.sh).
return { repos = { ["o/api"] = { checkout = "$config/none" } } }
EOF

# The first time, a copy of the daily dockets.
[ -f "$state/wheelhouse.db" ] || "$share/dev-install.sh" --sync --stopped
"$wrapper" up
pause

# --- The app -----------------------------------------------------------------

if [ "${1:-}" != --no-app ]; then
  WH_DEV_HUB="http://127.0.0.1:$port" WH_DEV_COMMAND="$wrapper" "$root/mac/build.sh" --dev --install
fi
say "Wheelhouse Dev is up at http://127.0.0.1:$port (wh-dev; $( [ "${1:-}" = --no-app ] && echo "no app" || echo "$app, ⇧⌥⌘K"))"
