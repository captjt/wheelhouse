#!/usr/bin/env bash
# Render the Teams screenshots (docs/teams-guide.md and the README) from a
# made-up workspace, so nothing of yours shows up in them: Acme, with a
# workspace hub and three members' hubs on this Mac (Alex's, whose apps the
# shots show, Sam's and Dana's), each with its own state, keys, port and
# stand-in for Rex, reaching the workspace through scripts/fake-tailscale-serve.
#
# - Alex shares a docket (ACME-1), makes Sam an owner and Dana a member;
#   Sam opens one straight on the workspace (ACME-2), Dana another (ACME-3),
#   and Alex one nobody has started (ACME-4);
# - agents from all three Macs on them, with prompts and turns played
#   through the same hooks Claude Code calls;
# - Sam's agent messages Alex's (held in Alex's Inbox until Alex lets it
#   in) and asks the owners a question; one of Alex's notes is held back,
#   because it looks like it holds a token.
#
# Your own Rex, hub, Tailscale and config are left alone. Needs jq, Node 22
# or later, Google Chrome and, for Wheelhouse.app's shots, Xcode 26.
#
#   scripts/team-screenshots.sh           # render the screenshots
#   scripts/team-screenshots.sh --serve   # keep the world running, with
#                                         # Alex's app at http://127.0.0.1:7451
set -euo pipefail
serve=""
[ "${1:-}" = --serve ] && serve=1

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
images="$root/docs/images"
t=$(mktemp -d)
wsport=${PORT:-7450}
pids=()
cleanup() {
  for p in "${pids[@]:-}"; do
    [ -n "$p" ] && kill "$p" 2>/dev/null || true
  done
  wait 2>/dev/null || true
  rm -rf "$t"
}
trap cleanup EXIT
fail() {
  printf '\nFAIL: %s\n' "$*" >&2
  for l in ws alex sam dana; do
    printf '\n%s log:\n' "$l" >&2
    tail -15 "$t/$l.log" >&2 || true
  done
  exit 1
}
until_() { # what command...
  local what=$1
  shift
  for _ in $(seq 60); do
    "$@" >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  fail "$what"
}

(cd "$root/web" && npm run build --silent >/dev/null)
cargo build --release --quiet --manifest-path "$root/Cargo.toml"
wh="$root/target/release/wh"
mkdir -p "$images"

# --- The world -----------------------------------------------------------------
# Each member's Mac: agents in Rex panes, in a checkout of acme/api in their
# made-up HOME (so the apps show ~/code/acme-api, never this folder).
people=(alex sam dana)
ago() { date -u -v-"$1"M +%Y-%m-%dT%H:%M:%SZ; }
pane() { # who n app state minutes-ago
  jq -n --arg b "block:$1-$2" --arg l "$1-$2" --arg c "$t/$1/home/code/acme-api" --arg app "$3" --arg st "$4" --arg at "$(ago "$5")" \
    '{block_id: $b, label: $l, cwd: $c, process: (if $app == "codex" then "codex" else "claude" end), records: [{state: $st, app: $app, updated_at: $at}]}'
}
world() { # who panes-json
  jq -n --arg home "$t/$1/home" --argjson panes "$2" '{home: $home, pasted: [], keys: [], focused: [], sessions: [
    {session_id: "session:api", label: "acme-api", windows: [{window_id: "window:1", label: "work", blocks: $panes}]}]}'
}
for who in "${people[@]}"; do mkdir -p "$t/$who/home/code/acme-api"; done
mkdir -p "$t/ws"
world alex "[$(pane alex 1 claude-code working 1), $(pane alex 2 codex idle 8)]" >"$t/alex/rex.json"
world sam "[$(pane sam 1 codex working 2), $(pane sam 2 claude-code working 3)]" >"$t/sam/rex.json"
world dana "[$(pane dana 1 claude-code idle 12)]" >"$t/dana/rex.json"

unset CLAUDE_CONFIG_DIR CODEX_HOME REX_SESSION REX_BLOCK REX_CLIENT HC_TERMINAL_ID HC_SCOPE HC_TERMINAL_SOCKET WH_SETTINGS WH_RUNTIME
base_env=(WH_TAILNET=off WH_NOTIFY=off WH_GH=/usr/bin/false WH_TAILSCALE=/usr/bin/false WH_HC=/usr/bin/false
  WH_REX="$here/fake-rex" WH_REVIEW_CONFIG="$t/none.lua" WH_BUILD_CONFIG="$t/none.lua" WH_LINEAR_KEY=demo WH_LINEAR_ENDPOINT=http://127.0.0.1:9)
n_of() { case $1 in alex) echo 1 ;; sam) echo 2 ;; dana) echo 3 ;; esac; }
port_of() { echo $((wsport + $(n_of "$1"))); }
serve_port() { echo $((wsport + 10 + $(n_of "$1"))); }
ws_url() { echo "http://127.0.0.1:$(serve_port "$1")"; }
sock() { echo "$t/$1/wh.sock"; }
as() { # who wh-args...  (as that member, in a shell on their Mac)
  local who=$1
  shift
  env WH_SOCKET="$(sock "$who")" HOME="$t/$who/home" "$wh" "$@" </dev/null
}
as_in() { # who wh-args...  (as as, with this script's stdin: an intent from a heredoc)
  local who=$1
  shift
  env WH_SOCKET="$(sock "$who")" HOME="$t/$who/home" "$wh" "$@"
}
in_pane() { # who n wh-args...  (as that member's agent, in its Rex pane)
  local who=$1 n=$2
  shift 2
  env WH_SOCKET="$(sock "$who")" HOME="$t/$who/home" REX_BLOCK="block:$who-$n" "$wh" "$@" </dev/null
}
get() { curl -fsS --max-time 20 -H 'x-wheelhouse: 1' "http://127.0.0.1:$(port_of "$1")$2"; }
post() { curl -fsS --max-time 20 -H 'x-wheelhouse: 1' -H 'content-type: application/json' -X POST -d "$3" "http://127.0.0.1:$(port_of "$1")$2"; }
sync_all() {
  for _ in 1 2; do
    for who in "${people[@]}"; do as "$who" team sync >/dev/null 2>&1 || true; done
  done
}
hook() { # who n event payload  (Claude Code's hooks, from that pane)
  post "$1" /api/hook "$(jq -n --arg b "block:$1-$2" --arg e "$3" --argjson p "$4" '{event: $e, agent: "claude-code", block: $b, payload: $p}')" >/dev/null
}
prompt() { hook "$1" "$2" UserPromptSubmit "$(jq -n --arg t "$3" --arg s "s-$1-$2" '{session_id: $s, prompt: $t}')"; }
reply() { hook "$1" "$2" Stop "$(jq -n --arg t "$3" '{last_assistant_message: $t}')"; sleep 1; }

# Claude Code's hooks, in Alex's made-up HOME, so the app shows them on.
HOME="$t/alex/home" "$wh" hooks install >/dev/null
for who in "${people[@]}"; do
  "$here/fake-tailscale-serve" "$(serve_port "$who")" "http://127.0.0.1:$wsport" "$who@acme.dev" 2>>"$t/serve.log" &
  pids+=("$!")
done
env "${base_env[@]}" HOME="$t/ws" WH_STATE="$t/ws/state" WH_WORKSPACE_HTTP="127.0.0.1:$wsport" "$wh" serve --workspace >>"$t/ws.log" 2>&1 &
pids+=("$!")
until_ "the workspace hub didn't start" curl -fsS "$(ws_url alex)/team/v1/hello"
for who in "${people[@]}"; do
  env "${base_env[@]}" HOME="$t/$who/home" USER="$who" WH_STATE="$t/$who/state" WH_SOCKET="$(sock "$who")" WH_HTTP="127.0.0.1:$(port_of "$who")" \
    WH_WORLD="$t/$who/rex.json" WH_BUILD_DATA="$t/$who/build" WH_REVIEW_DATA="$t/$who/review" "$wh" serve >>"$t/$who.log" 2>&1 &
  pids+=("$!")
  until_ "$who's hub didn't start" get "$who" /api/state
done
until_ "the agents' dockets didn't open" sh -c "[ \$(curl -fsS -H 'x-wheelhouse: 1' http://127.0.0.1:$(port_of sam)/api/state | jq '[.dockets[] | select(.agent)] | length') -ge 2 ]"
for p in alex-1 sam-2 dana-1; do hook "${p%-*}" "${p#*-}" SessionStart "$(jq -n --arg s "s-$p" '{session_id: $s, source: "startup"}')"; done

# --- The workspace ---------------------------------------------------------------
until_ "no setup code" test -s "$t/ws/state/setup-code"
as alex team create Acme --prefix ACME --url "$(ws_url alex)" --setup "$(cat "$t/ws/state/setup-code")" --handle alex --login alex@acme.dev >/dev/null || fail "wh team create"
# An invite names the workspace as Alex reaches it; each Mac here reaches it
# through its own stand-in for tailscale serve.
recode() { python3 -c 'import sys,base64,json; c=sys.argv[1][5:]; d=json.loads(base64.urlsafe_b64decode(c+"="*(-len(c)%4))); d["u"]=sys.argv[2]; print("whi1."+base64.urlsafe_b64encode(json.dumps(d).encode()).decode().rstrip("="))' "$1" "$2"; }
join() { # who name
  local code
  code=$(as alex team invite "$1" | grep '^whi1\.') || fail "no invite for $1"
  as "$1" team join "$(recode "$code" "$(ws_url "$1")")" --login "$1@acme.dev" --name "$2" >/dev/null || fail "$1's join"
}
join sam "Sam Rivera"
join dana "Dana Okafor"
sync_all

# ACME-1: Alex's, shared with its history kept on Alex's Mac.
D=$(as alex new "Rate limits on the public API")
as_in alex intent "$D" - >/dev/null <<'EOF'
Every public route answers 429 past its bucket, so one noisy client can't take the API down for everyone.

## Acceptance

- [x] A token bucket per API key, in middleware
- [ ] 429 with `Retry-After` and the `X-RateLimit-*` headers
- [ ] The limits shown on the settings page
- [ ] Support's docs say what the limits are
EOF
as alex note "$D" "Token bucket in middleware first; Redis only if one node isn't enough." >/dev/null
S=$(as alex team share "$D" | sed -n 's/.* shared as \(ACME-[0-9]*\).*/\1/p')
[ "$S" = ACME-1 ] || fail "the share"
as alex set "$S" --status in_progress >/dev/null
sync_all
as alex team people "$S" sam --owner >/dev/null
as alex team people "$S" dana >/dev/null
sync_all

# ACME-2: Sam's, opened straight on the workspace.
W=$(as sam new --team "Retry failed webhook deliveries")
as_in sam intent "$W" - >/dev/null <<'EOF'
A flaky endpoint shouldn't lose events: a 5xx or a timeout is retried with backoff, and a 4xx stops at once.

## Acceptance

- [ ] Up to 3 retries, with backoff and jitter
- [ ] Each retry logged with its attempt number
EOF
as sam set "$W" --status in_progress >/dev/null
as sam team people "$W" alex >/dev/null
# ACME-3: Dana's, done. ACME-4: Alex's, not started.
R=$(as dana new --team "Reindex search after the catalog import")
sync_all
as alex new --team "Per-key limits for partners" >/dev/null
sync_all

# The agents, each on its own Mac.
in_pane alex 1 join "$S" --role api >/dev/null || fail "Alex's agent didn't join $S"
in_pane sam 1 join "$S" --role web >/dev/null || fail "Sam's agent didn't join $S"
in_pane sam 2 join "$W" >/dev/null || fail "Sam's agent didn't join $W"
in_pane dana 1 join "$R" >/dev/null || fail "Dana's agent didn't join $R"
sync_all
prompt alex 1 "Add the token bucket middleware: 100 requests a minute per API key, bursts up to 20. Tests with a fake clock."
reply alex 1 "Done: **\`rateLimit()\`** wraps every public route, one bucket per API key, 100 a minute with bursts of 20. Six tests with a fake clock, all passing. Next: the 429 and its headers."
as sam note "$S" "I'll take the settings page and the 429 copy." >/dev/null
prompt sam 2 "Retry webhook deliveries that fail with a 5xx or time out: 3 times, 1s, 2s and 4s, with jitter."
reply sam 2 "Retries are in **\`deliver.ts\`**: 5xx and timeouts retry 3 times with jitter; a 4xx fails at once. 18 webhook tests pass."
prompt dana 1 "Reindex products after the catalog import, tonight, and say how long it took."
reply dana 1 "Reindexed 18,204 products in 19 minutes. Search answers from the new index; no errors."
as dana set "$R" --status "done" >/dev/null
sync_all
as dana note "$S" "Support's docs need the numbers: ping me when they settle." >/dev/null
sync_all
# Sam's agent asks Alex's; Alex's hub holds it until Alex says.
in_pane sam 1 send --to "@$S" "The settings page reads X-RateLimit-Limit and X-RateLimit-Remaining: are those the names you're sending?" >/dev/null || fail "Sam's agent's message"
in_pane sam 1 send --to me --subject "Empty bucket" --option 429 --option 503 \
  "When a key's bucket is empty, 429 or 503? 429 tells the client it's them; 503 reads as an outage. I'd send 429 with Retry-After." >/dev/null || fail "Sam's agent's question"
sync_all
until_ "Alex's Inbox didn't get the gate" sh -c "curl -fsS -H 'x-wheelhouse: 1' http://127.0.0.1:$(port_of alex)/api/messages | jq -e '[.[] | select(.status == \"open\" and .subject == \"Empty bucket\")] | length == 1'"
# Something Alex wrote that looks like a token: held on Alex's Mac.
as alex note "$S" "Load test key for staging: ghp_4f9KcQ2mZx7LrT0bWv8sNy3hJd6aPe1uGo5i" >/dev/null
get alex /api/team | jq -e '[.outbox[] | select(.status == "held")] | length == 1' >/dev/null || fail "the token wasn't held"
sleep 2

if [ -n "$serve" ]; then
  echo "Acme: Alex's app at http://127.0.0.1:$(port_of alex), Sam's at http://127.0.0.1:$(port_of sam), Dana's at http://127.0.0.1:$(port_of dana) (Ctrl-C stops it)"
  wait "${pids[@]}"
  exit 0
fi

# --- The shots ---------------------------------------------------------------------
base="http://127.0.0.1:$(port_of alex)"
shot() { node "$here/shot.mjs" "$@" >/dev/null && echo "wrote ${2#"$root"/}"; }
# Scroll a docket to its Activity: who wrote what, teammates by name.
activity_js='(() => { const h = [...document.querySelectorAll("*")].find(x => x.children.length === 0 && x.textContent.trim() === "Activity"); h?.scrollIntoView({block: "start"}); return new Promise(r => setTimeout(r, 400)); })()'
for look in light dark; do
  shot "$base/#/team" "$images/team-$look.png" "$look" desktop
  shot "$base/#/d/$S" "$images/team-docket-$look.png" "$look" desktop
  shot "$base/#/d/$S" "$images/team-activity-$look.png" "$look" desktop "$activity_js"
  shot "$base/#/inbox" "$images/team-inbox-$look.png" "$look" desktop
  shot "$base/#/team" "$images/phone-team-$look.png" "$look" phone
  shot "$base/#/d/$W" "$images/phone-team-docket-$look.png" "$look" phone "$activity_js"
done

# Wheelhouse.app against Alex's hub: the Team place with ACME-1 beside it,
# and the Inbox with Sam's agent's message held.
if swift --version 2>/dev/null | grep -qE "Swift version (6\.[2-9]|[7-9])"; then
  (cd "$root/mac" && swift build -c release --product Wheelhouse >/dev/null)
  mkdir -p "$t/mac"
  WH_APP_HUB="$base" "$root/mac/.build/release/Wheelhouse" --snapshot "$t/mac" --only team,inbox >/dev/null
  for look in light dark; do
    cp "$t/mac/mac-team-$look.png" "$images/mac-team-$look.png" && echo "wrote docs/images/mac-team-$look.png"
    cp "$t/mac/mac-inbox-$look.png" "$images/mac-team-inbox-$look.png" && echo "wrote docs/images/mac-team-inbox-$look.png"
  done
else
  echo "skipped Wheelhouse.app's shots: they need Xcode 26 (Swift 6.2)"
fi
