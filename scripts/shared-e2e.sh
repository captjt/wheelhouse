#!/usr/bin/env bash
# Several agents on one docket, end to end, on a scratch hub with stand-ins
# for Rex and Herdr (scripts/fake-rex, scripts/fake-hc, scripts/fake-herdr-app):
# two agents in Rex and one in Herdr join a docket you opened, find each
# other in wh brief, send hooks onto it, message the whole docket, and have
# one of their prompts approved (only that pane gets the key). A fourth is
# started on it from Wheelhouse. The hub restarts and finds them all again.
# One leaves; the docket stays held while any agent is on it, and is
# released when the last one goes. Nothing touches your Rex, your Herdr,
# GitHub, Linear, or ~/.config.
#
#   scripts/shared-e2e.sh           # exits non-zero on the first failure
#   KEEP=1 scripts/shared-e2e.sh    # keep the scratch directory to look at
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
t=$(mktemp -d)
port=${PORT:-7644}
base="http://127.0.0.1:$port"
hub=""
cleanup() {
  if [ -n "$hub" ]; then
    kill "$hub" 2>/dev/null || true
    wait "$hub" 2>/dev/null || true
  fi
  if [ -n "${KEEP:-}" ]; then echo "kept $t"; else rm -rf "$t"; fi
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$*"; }
ok() { printf '   ok: %s\n' "$*"; }
fail() {
  printf '\nFAIL: %s\n' "$*" >&2
  printf '\nthe hub log:\n' >&2
  tail -25 "$t/hub.log" >&2
  exit 1
}

cargo build --release --quiet --manifest-path "$root/Cargo.toml"
wh="$root/target/release/wh"

# --- The world ------------------------------------------------------------------
# Rex: api/planner and api/builder. Herdr: web/reviewer.
mkdir -p "$t/home" "$t/code/api" "$t/code/web"
at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
idle() { jq -nc --arg at "$1" '[{state: "idle", app: "claude-code", updated_at: $at}]'; }
jq -n --arg home "$t/home" --arg api "$t/code/api" --argjson r "$(idle "$at")" '{home: $home, pasted: [], keys: [], focused: [], sessions: [
  {session_id: "session:api", label: "api", windows: [{window_id: "window:1", label: "work",
    blocks: [{block_id: "block:planner-1", label: "planner", cwd: $api, process: "claude", records: $r},
             {block_id: "block:builder-2", label: "builder", cwd: $api, process: "claude", records: $r}]}]}]}' >"$t/rex.json"
ms=$(python3 -c 'import time; print(int(time.time() * 1000) - 10000)')
jq -n --arg c "$t/code/web" --argjson ms "$ms" '{terminals: [{address: "t1", state: "live", cwd: $c, name: null,
    labels: {"herdr.space": "web", "herdr.tab": "web.reviewer", "wh.session": "web", "wh.pane": "reviewer"},
    status: {state: "idle", app: "claude-code"}, status_ms: $ms, foreground: {name: "claude", cwd: $c}, screen: "the reviewer'"'"'s screen"}],
  sent: [], keys: [], focused: [], restarts: 0, n: 1}' >"$t/hc.json"

sock=$(mktemp -u /tmp/wh-sh.XXXX).sock
export HOME="$t/home" USER=you WH_STATE="$t/state" WH_SOCKET="$sock" WH_HTTP="127.0.0.1:$port" \
  WH_TAILNET=off WH_NOTIFY=off WH_LINEAR_OFF=1 WH_GH=/usr/bin/false WH_TAILSCALE=/usr/bin/false \
  WH_REX="$here/fake-rex" WH_WORLD="$t/rex.json" \
  WH_HC="$here/fake-hc" WH_HC_WORLD="$t/hc.json" WH_HERDR_APP="$here/fake-herdr-app" \
  WH_REVIEW_CONFIG="$t/none.lua" WH_BUILD_CONFIG="$t/none.lua" WH_BUILD_DATA="$t/build" WH_REVIEW_DATA="$t/review"
unset CLAUDE_CONFIG_DIR CODEX_HOME REX_SESSION REX_BLOCK REX_CLIENT HC_TERMINAL_ID HC_SCOPE HC_TERMINAL_SOCKET WH_SETTINGS WH_RUNTIME

api() { curl -fsS -H 'x-wheelhouse: 1' -H 'content-type: application/json' "$@"; }
start_hub() {
  "$wh" serve >>"$t/hub.log" 2>&1 &
  hub=$!
  for _ in $(seq 60); do api "$base/api/state" >/dev/null 2>&1 && return 0; sleep 0.25; done
  fail "the hub didn't start"
}
stop_hub() {
  kill "$hub" 2>/dev/null || true
  wait "$hub" 2>/dev/null || true
  hub=""
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
state() { api "$base/api/state"; }
has() { state | jq -e "$1"; }
page() { api "$base/api/dockets/$D"; }
on_it() { page | jq -c '[.docket.agents[] | select(.on) | .block] | sort'; }
docket_of() { state | jq -r --arg b "$1" '.agents[] | select(.block == $b) | .docket'; }
in_rex() { env REX_BLOCK="$1" "$wh" "${@:2}"; }
in_herdr() { env HC_TERMINAL_ID="$1" "$wh" "${@:2}"; }
rex_got() { jq -e --arg b "$1" --arg x "$2" '[.pasted[] | select(.block == $b and (.text | contains($x)))] | length >= 1' "$t/rex.json"; }
herdr_got() { jq -e --arg t "$1" --arg x "$2" '[.sent[] | select(.terminal == $t and (.text | contains($x)))] | length >= 1' "$t/hc.json"; }
set_rex() { # block jq-update-of-its-records
  until mkdir "$t/rex.json.lock" 2>/dev/null; do sleep 0.05; done
  jq --arg b "$1" "(.sessions[].windows[].blocks[] | select(.block_id == \$b) | .records) |= $2" "$t/rex.json" >"$t/rex.tmp" && mv "$t/rex.tmp" "$t/rex.json"
  rmdir "$t/rex.json.lock"
}
close_rex() { # block
  until mkdir "$t/rex.json.lock" 2>/dev/null; do sleep 0.05; done
  jq --arg b "$1" '.sessions[].windows[].blocks |= [.[] | select(.block_id != $b)]' "$t/rex.json" >"$t/rex.tmp" && mv "$t/rex.tmp" "$t/rex.json"
  rmdir "$t/rex.json.lock"
}
set_herdr() { # address jq-update-of-the-terminal
  until mkdir "$t/hc.json.lock" 2>/dev/null; do sleep 0.05; done
  jq --arg a "$1" --argjson ms "$(python3 -c 'import time; print(int(time.time() * 1000))')" "(.terminals[] | select(.address == \$a)) |= ($2)" "$t/hc.json" >"$t/hc.tmp" && mv "$t/hc.tmp" "$t/hc.json"
  rmdir "$t/hc.json.lock"
}

start_hub

step "three agents, in Rex and Herdr, each on a docket of its own"
until_ "both runtimes aren't followed" has '[.runtimes[] | select(.connected)] | length == 2'
until_ "not every agent has a docket" has '[.agents[] | select(.docket)] | length == 3'
OWN_PLANNER=$(docket_of block:planner-1)
ok "planner on $OWN_PLANNER, builder on $(docket_of block:builder-2), reviewer on $(docket_of herdr:t1)"

step "they join a docket you opened"
D=$(api -X POST "$base/api/dockets" -d '{"title": "Shared dockets"}' | jq -r .docket)
in_rex block:planner-1 join "$D" | grep -q "on $D" || fail "the planner couldn't join"
in_rex block:builder-2 join "$D" --role builder | grep -q "beside 1 other agent" || fail "the builder didn't join beside the planner"
in_herdr t1 join "$D" --role reviewer | grep -q "beside 2 other agents" || fail "the reviewer didn't join beside two"
[ "$(on_it)" = '["block:builder-2","block:planner-1","herdr:t1"]' ] || fail "on $D: $(on_it)"
page | jq -e '.docket.status == "in_progress" and .docket.released == null' >/dev/null || fail "$D isn't held: $(page | jq -c '.docket | {status, released}')"
page | jq -e '[.docket.agents[] | select(.block == "herdr:t1")][0] | .role == "reviewer" and .runtime == "herdr"' >/dev/null || fail "the reviewer's role and runtime"
api "$base/api/dockets/$OWN_PLANNER" | jq -e '.docket.status == "dropped"' >/dev/null || fail "the planner's own empty docket wasn't dropped"
for b in block:planner-1 block:builder-2 herdr:t1; do [ "$(docket_of $b)" = "$D" ] || fail "$b is on $(docket_of $b)"; done
in_rex block:planner-1 join "$D" | grep -q "already on $D" || fail "joining twice"
ok "$D holds all three, with their roles; the dockets they left are dropped"

step "they find each other"
b=$(in_rex block:planner-1 brief)
grep -q "## Other agents on $D" <<<"$b" || fail "the planner's brief: $b"
grep -q "api/builder\`: Claude Code, builder, in Rex" <<<"$b" || fail "the builder isn't in the planner's brief: $b"
grep -q "web/reviewer\`: Claude Code, reviewer, in Herdr" <<<"$b" || fail "the reviewer isn't in the planner's brief: $b"
grep -q "planner\`" <<<"$b" && fail "the planner is listed to itself"
grep -q "api/planner" <<<"$(in_herdr t1 brief)" || fail "the reviewer's brief doesn't list the planner"
ok "wh brief lists the other two, by address, role and runtime"

step "each one's hooks land on the docket, under that agent"
printf '%s' '{"session_id": "s-rev", "prompt": "Review the fold"}' | in_herdr t1 hook --agent claude-code UserPromptSubmit
printf '%s' '{"session_id": "s-build", "prompt": "Build the fold"}' | in_rex block:builder-2 hook --agent claude-code UserPromptSubmit
until_ "the prompts didn't reach $D" sh -c "curl -fsS -H 'x-wheelhouse: 1' $base/api/dockets/$D | jq -e '.docket.prompts == 2'"
page | jq -e '[.entries[] | select(.kind == "note" and .data.via == "prompt") | .data.block] == ["herdr:t1", "block:builder-2"]' >/dev/null || fail "the prompts don't name their agents: $(page | jq -c '[.entries[] | select(.kind == "note") | .data]')"
ok "two prompts on $D, each naming the pane it came from"

step "a message to the whole docket"
in_rex block:planner-1 send --to "@$D" "I'm taking the record; who has the apps?" | grep -q "@$D" || fail "wh send --to @$D"
until_ "the builder didn't get it" rex_got block:builder-2 "who has the apps?"
until_ "the reviewer didn't get it" herdr_got t1 "who has the apps?"
rex_got block:planner-1 "who has the apps?" >/dev/null && fail "the sender got its own message"
n=$(jq -r '[.sent[] | select(.terminal == "t1" and (.text | contains("who has the apps?")))][0].text | capture("Message #(?<n>[0-9]+)").n' "$t/hc.json")
in_herdr t1 reply "$n" "Me: the apps." >/dev/null || fail "the reviewer's reply"
until_ "the reply didn't reach the planner" rex_got block:planner-1 "Me: the apps."
page | jq -e --arg d "@$D" '[.entries[] | select(.data.to == $d)] | length == 1' >/dev/null || fail "the message isn't one entry on $D"
page | jq -e '[.entries[] | select(.data.replies_to)] | length == 1' >/dev/null || fail "the reply isn't on $D's thread"
ok "the builder and the reviewer got it, not the planner; the reply came back on $D's thread"

step "two agents ask; approving one answers only its pane"
set_rex block:builder-2 "[{state: \"blocked\", kind: \"permission\", app: \"claude-code\", msg: \"Run cargo test?\", updated_at: \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\"}]"
set_herdr t1 '.status = {state: "blocked", kind: "permission", app: "claude-code", msg: "Run npm test?"} | .status_ms = $ms'
until_ "both aren't asking" has "[.asks[] | select(.docket == \"$D\")] | length == 2"
has "[.asks[] | select(.docket == \"$D\") | .block] | sort == [\"block:builder-2\", \"herdr:t1\"]" >/dev/null || fail "the asks don't say which agent"
seen=$(page | jq -c '.docket.agents[] | select(.block == "herdr:t1") | .live | {record, updated_at: .since, msg, block}')
api -X POST "$base/api/dockets/$D/approve" -d "$seen" | jq -e '.answer == "approve" and .cleared' >/dev/null || fail "the reviewer's approval didn't clear"
jq -e '[.keys[] | select(.terminal == "t1" and .keys == ["1"])] | length == 1' "$t/hc.json" >/dev/null || fail "no 1 in Herdr"
jq -e '[.keys[] | select(.block == "block:builder-2" and .keys == ["1"])] | length == 0' "$t/rex.json" >/dev/null || fail "the approve key went to the builder"
page | jq -e '.docket.lead == "block:builder-2" and .docket.live.msg == "Run cargo test?"' >/dev/null || fail "the builder isn't the lead now: $(page | jq -c '.docket | {lead, live}')"
seen=$(page | jq -c '.docket.live | {record, updated_at: .since, msg}')
api -X POST "$base/api/dockets/$D/approve" -d "$seen" | jq -e '.cleared' >/dev/null || fail "the lead's approval, with no pane named"
jq -e '[.keys[] | select(.block == "block:builder-2" and .keys == ["1"])] | length == 1' "$t/rex.json" >/dev/null || fail "no 1 for the builder"
page | jq -e '[.entries[] | select(.kind == "approval") | .data.block] == ["herdr:t1", "block:builder-2"]' >/dev/null || fail "the approvals don't name their panes"
ok "the reviewer's prompt answered in Herdr alone; then the builder's, as the lead"

step "one more, started from Wheelhouse"
r=$(api -X POST "$base/api/agents/start" -d "{\"app\": \"claude-code\", \"cwd\": \"$t/code/api\", \"prompt\": \"Write the docs\", \"docket\": \"$D\"}")
jq -e '.others == 3' <<<"$r" >/dev/null || fail "start didn't say it joins three: $r"
jq -e --arg p "wheelhouse/$D" '.pane == $p' <<<"$r" >/dev/null || fail "its pane: $r"
NEW=$(jq -r .block <<<"$r")
until_ "the new agent isn't on $D" sh -c "curl -fsS -H 'x-wheelhouse: 1' $base/api/dockets/$D | jq -e '[.docket.agents[] | select(.on)] | length == 4'"
ok "a fourth ($NEW, wheelhouse/$D) beside the other three"

step "the hub restarts, and finds them all on the docket"
stop_hub
start_hub
until_ "not every agent is back" has '[.agents[] | select(.docket)] | length == 4'
for b in block:planner-1 block:builder-2 herdr:t1 "$NEW"; do [ "$(docket_of "$b")" = "$D" ] || fail "after the restart $b is on $(docket_of "$b")"; done
ok "all four back on $D"

step "one leaves: the docket stays held"
in_herdr t1 leave | grep -q "left $D" || fail "wh leave"
OWN_REVIEWER=$(docket_of herdr:t1)
[ -n "$OWN_REVIEWER" ] && [ "$OWN_REVIEWER" != "$D" ] || fail "the reviewer's pane has no docket of its own: $OWN_REVIEWER"
page | jq -e '.docket.status == "in_progress" and .docket.released == null' >/dev/null || fail "$D was let go"
page | jq -e '[.docket.agents[] | select(.block == "herdr:t1")][0] | .on == false and .left == "left"' >/dev/null || fail "the reviewer isn't shown as having left"
ok "the reviewer is on $OWN_REVIEWER now; $D is still held by three"

step "panes close; the last one releases the docket"
close_rex block:builder-2
close_rex "$NEW"
until_ "the closed panes weren't let go" sh -c "curl -fsS -H 'x-wheelhouse: 1' $base/api/dockets/$D | jq -e '[.docket.agents[] | select(.on)] | length == 1'"
page | jq -e '.docket.status == "in_progress" and .docket.released == null' >/dev/null || fail "$D was released with the planner still on it"
close_rex block:planner-1
until_ "$D wasn't released" sh -c "curl -fsS -H 'x-wheelhouse: 1' $base/api/dockets/$D | jq -e '.docket.released == \"pane closed\"'"
page | jq -e '.docket.status == "done"' >/dev/null || fail "$D isn't done"
"$wh" verify "$D" >/dev/null || fail "$D's record doesn't check"
ok "held while the planner stayed; released and done when it closed; the record checks"

printf '\nSeveral agents on one docket: every step passed.\n'
