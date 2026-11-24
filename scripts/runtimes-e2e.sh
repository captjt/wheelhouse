#!/usr/bin/env bash
# Rex and Herdr together, end to end, on a scratch hub with stand-ins for
# both (scripts/fake-rex, scripts/fake-hc, scripts/fake-herdr-app). Agents
# in each get dockets, find themselves, message each other both ways by
# name, by a runtime-qualified address and by pane id, name their panes,
# send hooks, have their prompts approved and queued prompts typed in, are
# shown and read; new agents open in the runtime Settings picks; and when
# one runtime loses a pane or restarts, the other carries on. Nothing
# touches your Rex, your Herdr, GitHub, Linear, or ~/.config.
#
#   scripts/runtimes-e2e.sh           # exits non-zero on the first failure
#   KEEP=1 scripts/runtimes-e2e.sh    # keep the scratch directory to look at
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
t=$(mktemp -d)
port=${PORT:-7634}
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

# --- Two worlds ---------------------------------------------------------------
# Rex: wheelhouse/planner and api/builder. Herdr: wheelhouse/planner too (the
# same name, in the other runtime) and web/reviewer.
mkdir -p "$t/home" "$t/code/api" "$t/code/web"
at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
idle() { jq -nc --arg at "$1" '[{state: "idle", app: "claude-code", updated_at: $at}]'; }
jq -n --arg home "$t/home" --arg api "$t/code/api" --argjson r "$(idle "$at")" '{home: $home, pasted: [], keys: [], focused: [], sessions: [
  {session_id: "session:wh", label: "wheelhouse", windows: [{window_id: "window:1", label: "planner",
    blocks: [{block_id: "block:planner-1", label: "planner", cwd: $api, process: "claude", records: $r}]}]},
  {session_id: "session:api", label: "api", windows: [{window_id: "window:2", label: "builder",
    blocks: [{block_id: "block:builder-2", label: "builder", cwd: $api, process: "claude", records: $r}]}]}]}' >"$t/rex.json"
ms=$(python3 -c 'import time; print(int(time.time() * 1000) - 10000)')
term() { # address space pane cwd
  jq -nc --arg a "$1" --arg s "$2" --arg p "$3" --arg c "$4" --argjson ms "$ms" '{address: $a, state: "live", cwd: $c, name: null,
    labels: {"herdr.space": $s, "herdr.tab": ($s + "." + $p), "wh.session": $s, "wh.pane": $p},
    status: {state: "idle", app: "claude-code"}, status_ms: $ms, foreground: {name: "claude", cwd: $c}, screen: ("the " + $p + "'"'"'s screen")}'
}
jq -n --argjson a "$(term t1 wheelhouse planner "$t/code/web")" --argjson b "$(term t2 web reviewer "$t/code/web")" \
  '{terminals: [$a, $b], sent: [], keys: [], focused: [], restarts: 0, n: 2}' >"$t/hc.json"

sock=$(mktemp -u /tmp/wh-rt.XXXX).sock
export HOME="$t/home" USER=you WH_STATE="$t/state" WH_SOCKET="$sock" WH_HTTP="127.0.0.1:$port" \
  WH_TAILNET=off WH_NOTIFY=off WH_LINEAR_OFF=1 WH_GH=/usr/bin/false WH_TAILSCALE=/usr/bin/false \
  WH_REX="$here/fake-rex" WH_WORLD="$t/rex.json" \
  WH_HC="$here/fake-hc" WH_HC_WORLD="$t/hc.json" WH_HERDR_APP="$here/fake-herdr-app" \
  WH_REVIEW_CONFIG="$t/none.lua" WH_BUILD_CONFIG="$t/none.lua" WH_BUILD_DATA="$t/build" WH_REVIEW_DATA="$t/review"
unset CLAUDE_CONFIG_DIR CODEX_HOME REX_SESSION REX_BLOCK REX_CLIENT HC_TERMINAL_ID HC_SCOPE HC_TERMINAL_SOCKET WH_SETTINGS WH_RUNTIME
"$wh" serve >"$t/hub.log" 2>&1 &
hub=$!

api() { curl -fsS -H 'x-wheelhouse: 1' -H 'content-type: application/json' "$@"; }
code() { curl -s -o /dev/null -w '%{http_code}' -H 'x-wheelhouse: 1' -H 'content-type: application/json' "$@"; }
for _ in $(seq 60); do api "$base/api/state" >/dev/null 2>&1 && break; sleep 0.25; done
api "$base/api/state" >/dev/null || fail "the hub didn't start"

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
set_herdr() { # address jq-update-of-the-terminal
  until mkdir "$t/hc.json.lock" 2>/dev/null; do sleep 0.05; done
  jq --arg a "$1" --argjson ms "$(python3 -c 'import time; print(int(time.time() * 1000))')" "(.terminals[] | select(.address == \$a)) |= ($2)" "$t/hc.json" >"$t/hc.tmp" && mv "$t/hc.tmp" "$t/hc.json"
  rmdir "$t/hc.json.lock"
}

# --- Both runtimes ------------------------------------------------------------

step "the hub follows Rex and Herdr at once"
until_ "both runtimes aren't followed" has '[.runtimes[] | select(.connected)] | length == 2'
until_ "not every agent has a docket" has '[.agents[] | select(.docket)] | length == 4'
state | jq -e '[.agents[] | {block, runtime}] | sort_by(.block) == [
  {block: "block:builder-2", runtime: "rex"}, {block: "block:planner-1", runtime: "rex"},
  {block: "herdr:t1", runtime: "herdr"}, {block: "herdr:t2", runtime: "herdr"}]' >/dev/null || fail "the agents' runtimes: $(state | jq -c '[.agents[] | {block, runtime}]')"
D_REX=$(docket_of block:builder-2) D_HERDR=$(docket_of herdr:t2)
st=$("$wh" status)
grep -q "Rex      following · 2 agents · new agents open here" <<<"$st" || fail "wh status: $st"
grep -q "Herdr    following · 2 agents" <<<"$st" || fail "wh status: $st"
ok "4 agents, 2 in each, each on a docket ($D_REX in Rex, $D_HERDR in Herdr); wh status lists both"

step "each agent knows where it is"
[ "$(in_rex block:builder-2 whoami 2>&1 | head -1)" != "" ] || fail "wh whoami in Rex says nothing"
grep -q "api/builder" <<<"$(in_rex block:builder-2 whoami)" || fail "Rex's builder: $(in_rex block:builder-2 whoami)"
grep -q "web/reviewer" <<<"$(in_herdr t2 whoami)" || fail "Herdr's reviewer: $(in_herdr t2 whoami)"
grep -q "herdr:wheelhouse/planner" <<<"$(in_herdr t1 whoami)" || fail "Herdr's planner: $(in_herdr t1 whoami)"
grep -q "rex:wheelhouse/planner" <<<"$(in_rex block:planner-1 whoami)" || fail "Rex's planner: $(in_rex block:planner-1 whoami)"
grep -q "$D_HERDR" <<<"$(in_herdr t2 brief)" || fail "wh brief in Herdr isn't $D_HERDR's"
ok "api/builder, web/reviewer, and rex:/herdr:wheelhouse/planner where both have a wheelhouse; wh brief finds its docket"

# --- Messages across runtimes ----------------------------------------------------

step "Rex to Herdr, by name"
in_rex block:builder-2 send --to reviewer "Can you look at PR #12?" >/dev/null || fail "Rex's builder couldn't send to reviewer"
until_ "the message didn't reach Herdr's reviewer" herdr_got t2 "Can you look at PR #12?"
herdr_got t2 "[wheelhouse] Message" >/dev/null || fail "no header"
herdr_got t2 "api/builder" >/dev/null || fail "the message doesn't say who it's from"
ok "typed into herdr:t2 with hc send, from api/builder"

step "Herdr to Rex, as a reply"
# Its number, as the reviewer read it: "[wheelhouse] Message #N from …".
n=$(jq -r '[.sent[] | select(.terminal == "t2" and (.text | contains("PR #12")))][0].text | capture("Message #(?<n>[0-9]+)").n' "$t/hc.json")
[ -n "$n" ] && [ "$n" != null ] || fail "the message has no number"
in_herdr t2 reply "$n" "On it: two nits." >/dev/null || fail "Herdr's reviewer couldn't reply"
until_ "the reply didn't reach Rex's builder" rex_got block:builder-2 "On it: two nits."
jq -e '[.keys[] | select(.block == "block:builder-2" and .keys == ["enter"])] | length >= 1' "$t/rex.json" >/dev/null || fail "no Enter after the paste"
ok "pasted into block:builder-2, then Enter"

step "the same name in both runtimes"
out=$(in_herdr t2 send --to wheelhouse/planner "which?" 2>&1) && fail "an ambiguous address was taken: $out"
echo "$out" | grep -q "rex:wheelhouse/planner" && echo "$out" | grep -q "herdr:wheelhouse/planner" || fail "the refusal doesn't name both: $out"
in_herdr t2 send --to herdr:wheelhouse/planner "To Herdr's planner" >/dev/null || fail "herdr:wheelhouse/planner wasn't taken"
in_rex block:builder-2 send --to rex:wheelhouse/planner "To Rex's planner" >/dev/null || fail "rex:wheelhouse/planner wasn't taken"
in_rex block:builder-2 send --to herdr:t2 "By its id" >/dev/null || fail "a Herdr pane id wasn't taken"
until_ "Herdr's planner didn't get its message" herdr_got t1 "To Herdr's planner"
until_ "Rex's planner didn't get its message" rex_got block:planner-1 "To Rex's planner"
until_ "the reviewer didn't get the one by id" herdr_got t2 "By its id"
rex_got block:planner-1 "To Herdr's planner" >/dev/null && fail "Herdr's message went to Rex's planner"
# From inside a wheelhouse, a bare name is that session's.
in_herdr t1 send --to builder "From the Herdr planner" >/dev/null || fail "a bare name across runtimes"
until_ "the bare name didn't reach Rex's builder" rex_got block:builder-2 "From the Herdr planner"
ok "refused, naming rex:wheelhouse/planner and herdr:wheelhouse/planner; each qualified one, an id, and a bare name reach the right pane"

step "naming a Herdr pane"
in_herdr t2 name critic >/dev/null || fail "wh name in Herdr"
jq -e '.terminals[] | select(.address == "t2") | .labels["wh.pane"] == "critic"' "$t/hc.json" >/dev/null || fail "the label wasn't set"
until_ "the new name isn't followed" has '.agents[] | select(.block == "herdr:t2") | .pane == "critic"'
in_rex block:builder-2 send --to web/critic "Renamed?" >/dev/null || fail "Rex couldn't reach the new name"
until_ "the renamed pane didn't get it" herdr_got t2 "Renamed?"
ok "web/critic, set as a label, reached from Rex"

step "a hook from a Herdr pane"
printf '%s' '{"session_id": "s-herdr", "prompt": "Review the retry change"}' | in_herdr t2 hook --agent claude-code UserPromptSubmit
until_ "the prompt didn't reach $D_HERDR" sh -c "curl -fsS -H 'x-wheelhouse: 1' $base/api/dockets/$D_HERDR | jq -e '.docket.title == \"Review the retry change\"'"
ok "$D_HERDR is named by the prompt its hook sent"

# --- Approvals, queued prompts, showing, reading --------------------------------

step "a prompt to approve, in each runtime"
set_herdr t2 '.status = {state: "blocked", kind: "permission", app: "claude-code", msg: "Run npm test?"} | .status_ms = $ms'
set_rex block:builder-2 "[{state: \"blocked\", kind: \"permission\", app: \"claude-code\", msg: \"Run cargo test?\", updated_at: \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\"}]"
until_ "Herdr's prompt isn't answerable" has ".dockets[] | select(.id == \"$D_HERDR\") | .live.answerable"
until_ "Rex's prompt isn't answerable" has ".dockets[] | select(.id == \"$D_REX\") | .live.answerable"
for d in "$D_HERDR" "$D_REX"; do
  seen=$(state | jq -c --arg d "$d" '.dockets[] | select(.id == $d) | .live | {record, updated_at: .since, msg}')
  api -X POST "$base/api/dockets/$d/approve" -d "$seen" | jq -e '.answer == "approve" and .cleared' >/dev/null || fail "$d's approval didn't clear"
done
jq -e '[.keys[] | select(.terminal == "t2" and .keys == ["1"])] | length == 1' "$t/hc.json" >/dev/null || fail "no 1 in Herdr"
jq -e '[.keys[] | select(.block == "block:builder-2" and .keys == ["1"])] | length == 1' "$t/rex.json" >/dev/null || fail "no 1 in Rex"
# A prompt that changed since you saw it isn't answered.
set_herdr t2 '.status = {state: "blocked", kind: "permission", app: "claude-code", msg: "rm -rf build?"} | .status_ms = $ms'
until_ "the new prompt isn't seen" has ".dockets[] | select(.id == \"$D_HERDR\") | .live.msg == \"rm -rf build?\""
[ "$(code -X POST "$base/api/dockets/$D_HERDR/approve" -d '{"record": "", "updated_at": "2020-01-01T00:00:00.000Z", "msg": "Run npm test?"}')" = 409 ] || fail "a stale approval went through"
set_herdr t2 '.status = {state: "idle", app: "claude-code"} | .status_ms = $ms'
ok "approved with 1 in both, each read again from its runtime first; a stale one refused"

step "a queued prompt, in each runtime"
set_herdr t2 '.status = {state: "working", app: "claude-code"} | .status_ms = $ms'
until_ "Herdr's agent isn't working" has ".dockets[] | select(.id == \"$D_HERDR\") | .live.state == \"working\""
api -X POST "$base/api/dockets/$D_HERDR/send" -d '{"text": "Then run lint", "queue": true}' | jq -e '.queued' >/dev/null || fail "not queued in Herdr"
herdr_got t2 "Then run lint" >/dev/null && fail "typed while it worked"
set_herdr t2 '.status = {state: "idle", app: "claude-code"} | .status_ms = ($ms - 5000)'
until_ "the queued prompt didn't go into Herdr" herdr_got t2 "Then run lint"
api -X POST "$base/api/dockets/$D_REX/send" -d '{"text": "Push it"}' | jq -e '.ok' >/dev/null || fail "Rex's send"
until_ "the prompt didn't go into Rex" rex_got block:builder-2 "Push it"
ok "queued while Herdr's worked, typed when it went idle; sent straight into Rex's"

step "showing and reading a pane, in each runtime"
api -X POST "$base/api/dockets/$D_HERDR/open" >/dev/null || fail "open in Herdr"
jq -e '.focused == ["t2"]' "$t/hc.json" >/dev/null || fail "the Herdr app wasn't asked to show t2"
api -X POST "$base/api/dockets/$D_REX/open" >/dev/null || fail "open in Rex"
jq -e '.focused == ["block:builder-2"]' "$t/rex.json" >/dev/null || fail "Rex wasn't asked to show the builder"
api "$base/api/dockets/$D_HERDR/screen" | jq -e '.text == "the reviewer'"'"'s screen" and .live.runtime == "herdr"' >/dev/null || fail "Herdr's screen"
ok "agent.focus in the Herdr app, rex focus in Rex; the screen read with hc read"

# --- New agents ---------------------------------------------------------------

step "new agents open in the runtime Settings picks"
api -X POST "$base/api/settings" -d '{"runtime": "herdr"}' | jq -e '.runtime.value == "herdr"' >/dev/null || fail "the runtime didn't change"
[ "$(code -X POST "$base/api/settings" -d '{"runtime": "tmux"}')" = 400 ] || fail "a runtime that isn't one was taken"
in_rex block:builder-2 start -C "$t/code/web" "Write the changelog" >/dev/null || fail "wh start from Rex"
until_ "the new agent isn't a Herdr terminal" jq -e '[.terminals[] | select(.labels["wh.session"] == "wheelhouse" and .labels.wh == "1")] | length == 1' "$t/hc.json"
herdr_new=$(jq -r '[.terminals[] | select(.labels.wh == "1")][0].address' "$t/hc.json")
until_ "it has no docket" has ".agents[] | select(.block == \"herdr:$herdr_new\") | .docket"
api -X POST "$base/api/settings" -d '{"runtime": null}' | jq -e '.runtime.value == "rex"' >/dev/null || fail "the runtime didn't go back to Rex"
in_herdr t2 start -C "$t/code/api" "Bump the version" >/dev/null || fail "wh start from Herdr"
until_ "the next one isn't in Rex's wheelhouse" jq -e '[.sessions[] | select(.label == "wheelhouse") | .windows[].blocks[]] | length == 2' "$t/rex.json"
ok "started from a Rex pane into Herdr (herdr:$herdr_new), then from a Herdr pane into Rex"

# --- Losing a pane, losing a runtime ----------------------------------------------

step "a Herdr pane closes; Rex's agents carry on"
until mkdir "$t/hc.json.lock" 2>/dev/null; do sleep 0.05; done
jq '.terminals |= [.[] | select(.address != "t1")]' "$t/hc.json" >"$t/hc.tmp" && mv "$t/hc.tmp" "$t/hc.json"
rmdir "$t/hc.json.lock"
until_ "Herdr's planner wasn't released" sh -c "curl -fsS -H 'x-wheelhouse: 1' $base/api/state | jq -e '[.agents[] | select(.block == \"herdr:t1\")] | length == 0'"
has '[.agents[] | select(.runtime == "rex")] | length == 3' >/dev/null || fail "Rex's agents changed"
ok "herdr:t1 went; Rex's three stayed"

step "herdr-core restarts; the hub follows it again"
before=$(grep -c "following Herdr" "$t/hub.log")
until mkdir "$t/hc.json.lock" 2>/dev/null; do sleep 0.05; done
jq '.restarts += 1' "$t/hc.json" >"$t/hc.tmp" && mv "$t/hc.tmp" "$t/hc.json"
rmdir "$t/hc.json.lock"
until_ "it didn't say it lost Herdr" grep -q "lost Herdr" "$t/hub.log"
until_ "it didn't follow Herdr again" sh -c "[ \$(grep -c 'following Herdr' '$t/hub.log') -gt $before ]"
until_ "Herdr isn't connected again" has '.runtimes[] | select(.name == "herdr") | .connected'
in_rex block:builder-2 send --to web/critic "Still there?" >/dev/null || fail "Herdr's pane after the restart"
until_ "the message after the restart didn't arrive" herdr_got t2 "Still there?"
has '[.agents[] | select(.runtime == "rex")] | length == 3' >/dev/null || fail "Rex's agents changed while Herdr restarted"
ok "lost, followed again, and reachable; Rex's agents untouched"

printf '\nRex and Herdr together: every step passed.\n'
