#!/usr/bin/env bash
# Teams, end to end, on one Mac: a workspace hub and two members' hubs
# (Jordan's and Sam's), each with its own state, socket, port and keys, and
# stand-ins for Rex. Jordan makes the workspace and invites Sam, who joins.
# Jordan shares a docket; both follow it, write on it at once, and land in
# one order. Sam's agent joins it from Sam's Mac; Jordan's hub sees it and
# leaves it alone. Owners change the intent and members can't. Sam's hub
# goes offline and catches up; an entry that looks like a secret is held.
# Then the workspace hub misbehaves: an entry changed after it was ordered,
# and a key slipped into the member log, are both caught. Last, Sam is
# removed and can't write any more. Nothing touches your Rex, Tailscale,
# GitHub, Linear, or ~/.config. docs/teams.md is the design.
#
# Each member's hub reaches the workspace hub through a stand-in for
# `tailscale serve` (scripts/fake-tailscale-serve) that sets their login,
# so the workspace hub checks logins as it does on a tailnet.
#
#   scripts/team-e2e.sh           # exits non-zero on the first failure
#   KEEP=1 scripts/team-e2e.sh    # keep the scratch directory to look at
#   HOLD=1 scripts/team-e2e.sh   # pause with everything running after the
#                                 # collaboration steps, to look at the apps
#   WS_IMAGE=wheelhouse-workspace:dev scripts/team-e2e.sh
#                                 # the workspace hub in its Linux container
#                                 # (examples/workspace/Dockerfile)
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
t=$(mktemp -d)
wsport=${PORT:-7710}
pids=()
ws_container=""
cleanup() {
  [ -n "$ws_container" ] && docker rm -f "$ws_container" >/dev/null 2>&1 || true
  for p in "${pids[@]:-}"; do
    [ -n "$p" ] && kill "$p" 2>/dev/null || true
  done
  wait 2>/dev/null || true
  if [ -n "${KEEP:-}" ]; then echo "kept $t"; else rm -rf "$t"; fi
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$*"; }
ok() { printf '   ok: %s\n' "$*"; }
fail() {
  printf '\nFAIL: %s\n' "$*" >&2
  for l in ws jordan sam; do
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

cargo build --release --quiet --manifest-path "$root/Cargo.toml"
wh="$root/target/release/wh"
command -v sqlite3 >/dev/null || fail "sqlite3 is needed to play a misbehaving workspace hub"

# --- The world -----------------------------------------------------------------
# Each member's Mac: one Claude Code agent in a Rex pane, in its own checkout.
at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
for who in jordan sam; do
  mkdir -p "$t/$who/home" "$t/$who/code/api"
  jq -n --arg home "$t/$who/home" --arg api "$t/$who/code/api" --arg at "$at" --arg b "block:$who-1" '{home: $home, pasted: [], keys: [], focused: [], sessions: [
    {session_id: "session:api", label: "api", windows: [{window_id: "window:1", label: "work",
      blocks: [{block_id: $b, label: "agent", cwd: $api, process: "claude", records: [{state: "idle", app: "claude-code", updated_at: $at}]}]}]}]}' >"$t/$who/rex.json"
done
unset CLAUDE_CONFIG_DIR CODEX_HOME REX_SESSION REX_BLOCK REX_CLIENT HC_TERMINAL_ID HC_SCOPE HC_TERMINAL_SOCKET WH_SETTINGS WH_RUNTIME
base_env=(WH_TAILNET=off WH_NOTIFY=off WH_LINEAR_OFF=1 WH_GH=/usr/bin/false WH_TAILSCALE=/usr/bin/false WH_HC=/usr/bin/false
  WH_REX="$here/fake-rex" WH_REVIEW_CONFIG="$t/none.lua" WH_BUILD_CONFIG="$t/none.lua")
port_of() { if [ "$1" = jordan ]; then echo $((wsport + 1)); else echo $((wsport + 2)); fi; }
sock() { echo "$t/$1/wh.sock"; }
hub_env() { # who
  echo "${base_env[@]}" HOME="$t/$1/home" USER="$1" WH_STATE="$t/$1/state" WH_SOCKET="$(sock "$1")" WH_HTTP="127.0.0.1:$(port_of "$1")" \
    WH_WORLD="$t/$1/rex.json" WH_BUILD_DATA="$t/$1/build" WH_REVIEW_DATA="$t/$1/review"
}
as() { # who wh-args...  (as that member, in a shell on their Mac)
  local who=$1
  shift
  env WH_SOCKET="$(sock "$who")" HOME="$t/$who/home" "$wh" "$@" </dev/null
}
in_pane() { # who wh-args...  (as that member's agent, in its Rex pane)
  local who=$1
  shift
  env WH_SOCKET="$(sock "$who")" HOME="$t/$who/home" REX_BLOCK="block:$who-1" "$wh" "$@" </dev/null
}
api() { # who path
  curl -fsS --max-time 20 -H 'x-wheelhouse: 1' "http://127.0.0.1:$(port_of "$1")$2"
}
# Where each member's hub reaches the workspace: its own stand-in for
# tailscale serve, which says who's calling.
ws_url() { if [ "$1" = jordan ]; then echo "http://127.0.0.1:$((wsport + 10))"; else echo "http://127.0.0.1:$((wsport + 11))"; fi; }
for who in jordan sam; do
  port=$(ws_url "$who" | sed 's/.*://')
  "$here/fake-tailscale-serve" "$port" "http://127.0.0.1:$wsport" "$who@acme.dev" 2>>"$t/serve.log" &
  pids+=("$!")
done
start_ws() {
  if [ -n "${WS_IMAGE:-}" ]; then
    if [ -z "$ws_container" ]; then
      mkdir -p "$t/ws/state"
      chmod 777 "$t/ws/state"
      ws_container=$(docker run -d -p "127.0.0.1:$wsport:7700" -v "$t/ws/state:/var/lib/wheelhouse-workspace" "$WS_IMAGE")
    else
      docker start "$ws_container" >/dev/null
    fi
  else
    env "${base_env[@]}" HOME="$t/ws" WH_STATE="$t/ws/state" WH_WORKSPACE_HTTP="127.0.0.1:$wsport" "$wh" serve --workspace >>"$t/ws.log" 2>&1 &
    wspid=$!
    pids+=("$wspid")
  fi
  until_ "the workspace hub didn't start" curl -fsS "$(ws_url jordan)/team/v1/hello"
}
stop_ws() {
  if [ -n "$ws_container" ]; then
    docker stop "$ws_container" >/dev/null
  else
    kill "$wspid" 2>/dev/null || true
    wait "$wspid" 2>/dev/null || true
  fi
}
start_hub() { # who
  # shellcheck disable=SC2046
  env $(hub_env "$1") "$wh" serve >>"$t/$1.log" 2>&1 &
  pids+=("$!")
  until_ "$1's hub didn't start" api "$1" /api/state
}
sync() {
  local out
  out=$(as "$1" team sync 2>&1) || fail "$1's sync: $out"
}
refused() { # pattern command...  (fails, and says pattern)
  local pat=$1 out
  shift
  if out=$("$@" 2>&1); then return 1; fi
  echo "$out" | grep -iE "$pat" >/dev/null
}
head_of() { as "$1" show "$2" | sed -n 's/.*head \([0-9a-f]*\).*/\1/p' | head -1; }
png() { # file width height  (a real PNG header: enough for the hub to take it)
  python3 -I -c 'import struct,sys,zlib
w,h=int(sys.argv[2]),int(sys.argv[3])
c=lambda t,d: struct.pack(">I",len(d))+t+d+struct.pack(">I",zlib.crc32(t+d)&0xffffffff)
open(sys.argv[1],"wb").write(b"\x89PNG\r\n\x1a\n"+c(b"IHDR",struct.pack(">IIBBBBB",w,h,8,2,0,0,0))+c(b"IDAT",zlib.compress(b"\0"*(w*3+1)*h))+c(b"IEND",b""))' "$@"
}
stage() { # who file  (stages an image; prints its digest)
  curl -fsS --max-time 20 -H 'x-wheelhouse: 1' --data-binary @"$2" "http://127.0.0.1:$(port_of "$1")/api/images?name=$(basename "$2")" | jq -r .digest
}
compose() { # who docket json
  curl -fsS --max-time 20 -H 'x-wheelhouse: 1' -H 'content-type: application/json' -d "$3" "http://127.0.0.1:$(port_of "$1")/api/dockets/$2/compose"
}
blob_code() { # who digest
  curl -s -o /dev/null -w '%{http_code}' -H 'x-wheelhouse: 1' "http://127.0.0.1:$(port_of "$1")/api/blobs/$2?name=x.png"
}

mkdir -p "$t/ws"
start_ws
start_hub jordan
start_hub sam
# Without a login, the workspace hub answers no one.
code=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$wsport/team/v1/hello")
[ "$code" = 403 ] || fail "the workspace hub answered without a Tailscale login: $code"

step "Jordan makes the workspace with the workspace hub's setup code"
until_ "no setup code" test -s "$t/ws/state/setup-code"
setup=$(cat "$t/ws/state/setup-code")
[ -n "$setup" ] || fail "no setup code"
as jordan team create Acme --prefix ACME --url "$(ws_url jordan)" --setup "$setup" --handle jordan --login jordan@acme.dev | grep >/dev/null "Made Acme" || fail "wh team create"
refused "already" as jordan team create Acme --prefix ACME --url "$(ws_url jordan)" --setup "$setup" --handle jordan || fail "a second create wasn't refused"
[ ! -e "$t/ws/state/setup-code" ] || fail "the setup code is still there"
ok "ACME made; Jordan is its first admin; the setup code is spent"

step "Jordan invites Sam, who joins with the code"
code=$(as jordan team invite sam | grep '^whi1\.')
[ -n "$code" ] || fail "no invite code"
# The code names the workspace as Jordan reaches it; Sam's Mac reaches it
# through Sam's own tailscale serve stand-in.
recode() { python3 -c 'import sys,base64,json; c=sys.argv[1][5:]; d=json.loads(base64.urlsafe_b64decode(c+"="*(-len(c)%4))); d["u"]=sys.argv[2]; print("whi1."+base64.urlsafe_b64encode(json.dumps(d).encode()).decode().rstrip("="))' "$1" "$2"; }
code=$(recode "$code" "$(ws_url sam)")
# Joining as someone else's login: refused.
refused "login" as sam team join "$code" --login priya@acme.dev || fail "a join with another login wasn't refused"
# A tampered code (another key's secret) can't join.
bad=$(python3 -c 'import sys,base64,json; c=sys.argv[1][5:]; d=json.loads(base64.urlsafe_b64decode(c+"="*(-len(c)%4))); d["s"]="11"*32; print("whi1."+base64.urlsafe_b64encode(json.dumps(d).encode()).decode().rstrip("="))' "$code")
refused "proof" as sam team join "$bad" --login sam@acme.dev || fail "a code with the wrong secret wasn't refused"
as sam team join "$code" --login sam@acme.dev | grep >/dev/null "You're in Acme as sam" || fail "wh team join"
refused "already" as sam team join "$code" --login sam@acme.dev || fail "a second join wasn't refused"
sync jordan
as jordan team members | grep >/dev/null "sam .*member" || fail "Jordan doesn't see Sam"
as sam team members | grep >/dev/null "jordan .*admin" || fail "Sam doesn't see Jordan"
as sam verify ACME-0 >/dev/null || fail "Sam's copy of the member log doesn't check"
ok "Sam is in; both hubs have the same member log, and it checks"

step "Jordan shares a docket; its history stays on Jordan's Mac"
D=$(as jordan new "Rate limits on the public API")
png "$t/page.png" 40 30
mock=$(stage jordan "$t/page.png")
as jordan intent "$D" "Every public route answers 429 past its bucket.

![The rate limit page](/api/blobs/$mock?name=page.png)" >/dev/null
as jordan note "$D" "Start with the token bucket in middleware." >/dev/null
out=$(as jordan team share "$D")
S=$(echo "$out" | sed -n 's/.* shared as \(ACME-[0-9]*\).*/\1/p')
[ "$S" = "ACME-1" ] || fail "the share: $out"
as jordan show "$D" | grep >/dev/null "shared_as" || fail "$D doesn't say it was shared"
# The intent, pinned again with its image, goes from Jordan's outbox.
sync jordan
sync sam
as sam show "$S" | grep >/dev/null "Rate limits on the public API" || fail "Sam doesn't see $S"
api sam "/api/dockets/$S" | jq -e '.docket.intent.body | contains("429")' >/dev/null || fail "Sam doesn't see the intent"
as sam verify "$S" >/dev/null || fail "$S doesn't check on Sam's hub"
api sam "/api/dockets/$S" | jq -e '[.entries[] | select(.kind == "share")][0] | (.data.history == null) and (.data.head | length == 64)' >/dev/null || fail "the share carries the history, or no head"
# The intent's mockup goes with it: pinned on the shared docket, so Sam's hub fetches it.
[ "$(blob_code sam "$mock")" = 200 ] || fail "Sam's hub didn't get the intent's image"
as sam brief "$S" | grep >/dev/null "(file: " || fail "Sam's brief doesn't give the intent's image as a file"
ok "$D went on as $S; Sam's hub has it, intent, its image and all, and it checks"

step "Writes from both, at once, land in one order"
as jordan note "$S" "jordan: middleware first" >/dev/null &
w1=$!
as sam note "$S" "sam: I'll take the settings page" >/dev/null &
w2=$!
wait "$w1" "$w2"
for _ in 1 2 3; do sync jordan; sync sam; done
hj=$(head_of jordan "$S")
hs=$(head_of sam "$S")
[ -n "$hj" ] && [ "$hj" = "$hs" ] || fail "the hubs disagree: $hj vs $hs"
api jordan "/api/dockets/$S" | jq -e '[.entries[] | select(.kind == "note")] | length >= 2' >/dev/null || fail "a note is missing"
ok "both notes are on both hubs, at head $hj"

step "Only owners change the intent; an owner can make Sam one"
refused "only an owner" as sam intent "$S" "Sam's version" || fail "Sam changed the intent as a member"
# With a mockup in it: refused before anything's written, so nothing of it reaches the record.
png "$t/sams.png" 41 30
sams=$(stage sam "$t/sams.png")
refused "only an owner" as sam intent "$S" "Sam's version ![Sam's mockup](/api/blobs/$sams?name=sams.png)" || fail "Sam changed the intent with an image"
sync sam
sync jordan
api jordan "/api/dockets/$S" | jq -e --arg d "$sams" '[.entries[] | select(.kind == "artifact" and .data.digest == $d)] | length == 0' >/dev/null || fail "Sam's refused intent kept its image"
as sam team people "$S" sam >/dev/null
as jordan team people "$S" sam --owner >/dev/null
# Sam's join may land after Jordan made Sam an owner: Sam stays one.
for _ in 1 2; do sync jordan; sync sam; done
api sam "/api/dockets/$S" | jq -e '(.docket.owners | index("sam")) and ((.docket.members // []) | index("sam") | not)' >/dev/null || fail "Sam isn't an owner: $(api sam "/api/dockets/$S" | jq -c '[.docket.owners, .docket.members]')"
as sam intent "$S" "Every public route answers 429 past its bucket, with Retry-After." | grep >/dev/null "sent to the workspace" || fail "Sam, an owner now, couldn't change the intent"
sync sam
sync jordan
api jordan "/api/dockets/$S" | jq -e '(.docket.intent.body | contains("Retry-After")) and (.docket.owners | index("sam"))' >/dev/null || fail "Jordan doesn't see Sam's intent"
ok "a member's intent was refused, its image too; Sam, made an owner, changed it"

step "A private prompt's images stay off the shared record"
as jordan team trust "$S" --private-prompts on >/dev/null
sync jordan
png "$t/admin.png" 42 30
admin=$(stage jordan "$t/admin.png")
compose jordan "$S" "{\"text\": \"/task match the admin console\", \"images\": [{\"digest\": \"$admin\", \"caption\": \"Internal admin console\"}]}" | jq -e '.did == "task" and (.prompt | contains("Internal admin console"))' >/dev/null || fail "the task didn't carry its image"
png "$t/public.png" 43 30
public=$(stage jordan "$t/public.png")
compose jordan "$S" "{\"text\": \"The public page\", \"images\": [{\"digest\": \"$public\"}]}" | jq -e '.did == "note"' >/dev/null || fail "the note didn't go"
sync jordan
sync sam
api sam "/api/dockets/$S" | jq -e --arg a "$admin" --arg p "$public" '([.entries[] | select(.data.digest == $a or (.data.images // [] | map(.digest) | index($a)))] | length == 0) and ([.entries[] | select(.kind == "artifact" and .data.digest == $p)] | length == 1)' >/dev/null || fail "a private prompt's image reached Sam, or a note's didn't"
[ "$(blob_code sam "$admin")" = 404 ] || fail "Sam's hub has the private prompt's image"
[ "$(blob_code sam "$public")" = 200 ] || fail "Sam's hub doesn't have the note's image"
as jordan team trust "$S" --private-prompts off >/dev/null
ok "the task's image stayed on Jordan's Mac; the note's went to Sam"

step "Sam's agent joins from Sam's Mac; Jordan's hub leaves it alone"
in_pane sam join "$S" --role web | grep >/dev/null "on $S" || fail "Sam's agent didn't join"
sync sam
sync jordan
api jordan "/api/dockets/$S" | jq -e '[.docket.agents[] | select(.on and .member == "sam")] | length == 1' >/dev/null || fail "Jordan doesn't see Sam's agent"
api jordan /api/state | jq -e '[.agents[] | select(.block == "block:sam-1")] | length == 0' >/dev/null || fail "Jordan's hub tracks Sam's pane"
api jordan "/api/dockets/$S" | jq -e '[.entries[] | select(.kind == "release")] | length == 0' >/dev/null || fail "something released Sam's agent"
ok "Sam's agent is on $S as Sam's; Jordan's hub doesn't follow or release it"

step "Agents message each other across Macs: held until Jordan lets Sam's in"
in_pane jordan join "$S" --role api >/dev/null || fail "Jordan's agent didn't join"
sync jordan
sync sam
in_pane sam brief | grep "jordan/" >/dev/null || fail "Sam's agent's brief doesn't list Jordan's agent"
in_pane sam send --to "@$S" "Header names: X-RateLimit-Limit and X-RateLimit-Remaining" >/dev/null || fail "Sam's agent couldn't message the docket"
sync sam
sync jordan
msgs() { sqlite3 "$t/$1/state/wheelhouse.db" "$2"; }
[ "$(msgs jordan "SELECT COUNT(*) FROM messages WHERE status = 'waiting' AND to_block = 'block:jordan-1'")" = 1 ] || fail "the message isn't waiting for Jordan: $(msgs jordan "SELECT id, status, from_addr, to_addr FROM messages")"
gate=$(api jordan /api/messages | jq -r '[.[] | select(.from == "wheelhouse" and .status == "open" and (.subject | contains("sam")))][0].id')
[ -n "$gate" ] && [ "$gate" != null ] || fail "no gate in Jordan's Inbox"
sleep 3
jq -e '[.pasted[] | select(.text | contains("X-RateLimit"))] | length == 0' "$t/jordan/rex.json" >/dev/null || fail "the message went into Jordan's pane before Jordan let it in"
as jordan reply "$gate" --choice 3 >/dev/null || fail "Jordan couldn't answer the gate"
until_ "the message didn't reach Jordan's agent" jq -e '[.pasted[] | select(.block == "block:jordan-1" and (.text | contains("X-RateLimit")) and (.text | contains("teammate")))] | length == 1' "$t/jordan/rex.json"
sync jordan
sync sam
in_pane sam send --to "@$S" "Retry-After in seconds, please" >/dev/null || fail "Sam's agent's second message"
sync sam
sync jordan
until_ "a trusted teammate's next message waited" jq -e '[.pasted[] | select(.block == "block:jordan-1" and (.text | contains("Retry-After in seconds")))] | length == 1' "$t/jordan/rex.json"
ok "held as a gate in Jordan's Inbox, delivered (marked a teammate's) once let in, and the next went straight in"

step "Sam's agent asks a question; Jordan, an owner, answers it; first answer stands"
in_pane sam send --to me --subject "Empty bucket" --option 429 --option 503 "429 or 503 when the bucket is empty?" >/dev/null || fail "Sam's agent's question"
sync sam
sync jordan
q=$(api jordan /api/messages | jq -r '[.[] | select(.status == "open" and .subject == "Empty bucket")][0].id')
[ -n "$q" ] && [ "$q" != null ] || fail "Jordan, an owner, didn't get Sam's agent's question"
api jordan /api/messages | jq -e --argjson q "$q" '.[] | select(.id == $q) | .from_member == "sam" and .from_app == "Claude Code"' >/dev/null || fail "the question isn't named as Sam's Claude Code: $(api jordan /api/messages | jq -c --argjson q "$q" '.[] | select(.id == $q) | [.from_member, .from_app]')"
as jordan reply "$q" --choice 429 >/dev/null || fail "Jordan couldn't answer"
sync jordan
sync sam
until_ "the answer didn't reach Sam's agent" jq -e '[.pasted[] | select(.block == "block:sam-1" and (.text | contains("Re: Empty bucket")) and (.text | contains("429")) and (.text | contains("from jordan")))] | length == 1' "$t/sam/rex.json"
[ "$(msgs sam "SELECT status FROM messages WHERE subject = 'Empty bucket' AND to_addr = 'me'")" = answered ] || fail "Sam's own copy of the question is still open"
ok "the question reached Jordan as an owner; Jordan's answer went to Sam's agent, and Sam's copy was withdrawn"

step "Jordan starts another agent on the shared docket; Sam sees it as Jordan's"
as jordan start --docket "$S" -C "$t/jordan/code/api" "Check the middleware's tests" >/dev/null || fail "wh start --docket on a shared docket"
sync jordan
sync sam
until_ "Sam doesn't see Jordan's new agent" sh -c "curl -fsS --max-time 20 -H 'x-wheelhouse: 1' http://127.0.0.1:$(port_of sam)/api/dockets/$S | jq -e '[.docket.agents[] | select(.on and .member == \"jordan\")] | length == 2'"
ok "Jordan's second agent is on $S, and Sam's hub counts two of Jordan's"

if [ -n "${HOLD:-}" ]; then
  printf '\n== Holding, with everything running:\n'
  printf '   Jordan: http://127.0.0.1:%s/#/team   Sam: http://127.0.0.1:%s/#/team\n' "$(port_of jordan)" "$(port_of sam)"
  printf '   touch %s to go on\n' "$t/release"
  until [ -e "$t/release" ]; do sleep 1; done
fi

step "Sam's hub writes offline and catches up"
stop_ws
as sam note "$S" "written on a plane" | grep >/dev/null "sent to the workspace" || fail "an offline note wasn't kept"
api sam /api/team | jq -e '[.outbox[] | select(.status == "queued")] | length >= 1' >/dev/null || fail "the offline note isn't in Sam's outbox"
start_ws
until_ "Sam's outbox didn't empty" sh -c "curl -fsS -H 'x-wheelhouse: 1' http://127.0.0.1:$(port_of sam)/api/team | jq -e '[.outbox[] | select(.status == \"queued\")] | length == 0'"
sync jordan
api jordan "/api/dockets/$S" | jq -e '[.entries[] | select(.body == "written on a plane")] | length == 1' >/dev/null || fail "Jordan doesn't have the offline note"
ok "the note waited in Sam's outbox, went when the workspace came back, and reached Jordan"

step "An entry that looks like a secret is held"
as sam note "$S" "use ghp_abcdefghijklmnopqrstuvwxyz0123456789 for CI" >/dev/null
api sam /api/team | jq -e '[.outbox[] | select(.status == "held")] | length == 1' >/dev/null || fail "the token wasn't held"
api sam /api/state | jq -e '[.attention[] | select(.kind == "team-held")] | length == 1' >/dev/null || fail "the held entry isn't in Attention"
n=$(api sam /api/team | jq -r '.outbox[] | select(.status == "held") | .n')
as sam team drop "$n" >/dev/null
sync sam
sync jordan
api jordan "/api/dockets/$S" | jq -e '[.entries[] | select((.body // "") | contains("ghp_"))] | length == 0' >/dev/null || fail "the token reached the workspace"
ok "the token stayed on Sam's Mac until Sam dropped it"

if [ -n "${WS_IMAGE:-}" ]; then
  printf '\n== (the misbehaving workspace hub steps run on a native workspace hub only)\n'
else
step "The workspace hub changes an entry after ordering it: caught"
as sam note "$S" "the original words" >/dev/null
sync sam
n=$(sqlite3 "$t/ws/state/workspace.db" "SELECT MAX(seq) FROM entries WHERE docket = '$S'")
sqlite3 "$t/ws/state/workspace.db" "UPDATE entries SET envelope = replace(envelope, '\"sig\":\"', '\"sig\":\"AA') WHERE docket = '$S' AND seq = $n"
sync jordan || true
api jordan /api/team | jq -e --arg s "$S" '.halted[$s] | contains("bytes don'"'"'t match")' >/dev/null || fail "Jordan's hub took a changed entry: $(api jordan /api/team | jq -c .halted)"
api jordan /api/state | jq -e '[.attention[] | select(.kind == "team-halted")] | length == 1' >/dev/null || fail "the halt isn't in Attention"
refused "not followed" as jordan verify "$S" || fail "wh verify doesn't say $S isn't followed"
ok "Jordan's hub refused the changed entry, stopped following $S, and says so"

step "The workspace hub slips a key into the member log: refused"
python3 - "$t/ws/state/workspace.db" <<'PY'
import sqlite3, sys, json, base64, hashlib, os
from datetime import datetime, timezone
db = sqlite3.connect(sys.argv[1])
seq, chain = db.execute("SELECT seq, chain FROM entries WHERE docket = 'ACME-0' ORDER BY seq DESC LIMIT 1").fetchone()
# An entry signed by a key nobody issued, claiming to be Sam's.
from hashlib import sha256
try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives import serialization
    k = Ed25519PrivateKey.generate()
    pub = k.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw).hex()
    sign = k.sign
except ImportError:
    pub = os.urandom(32).hex()
    sign = lambda m: os.urandom(64)
at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")
e = json.dumps({"v": 1, "docket": "ACME-0", "kind": "key", "at": at, "actor": {"type": "person", "id": "sam"}, "data": {"keyid": "ed25519:" + pub, "use": "hub", "name": "hub"}}, separators=(",", ":")).encode()
t = b"application/vnd.wheelhouse.entry+json"
pae = b"DSSEv1 %d %s %d %s" % (len(t), t, len(e), e)
env = json.dumps({"payloadType": t.decode(), "payload": base64.b64encode(e).decode(), "signatures": [{"keyid": "ed25519:" + pub, "sig": base64.b64encode(sign(pae)).decode()}]}, separators=(",", ":"))
i = sha256(env.encode()).digest()
c = sha256(bytes.fromhex(chain) + i).hexdigest()
db.execute("INSERT INTO entries (docket, seq, id, chain, kind, at, keyid, envelope) VALUES ('ACME-0', ?, ?, ?, 'key', ?, ?, ?)", (seq + 1, "sha256:" + i.hex(), c, at, "ed25519:" + pub, env))
db.commit()
PY
# The workspace hub serves from its database: Sam's hub asks for the new entry.
refused "active member|signature" as sam team sync || fail "Sam's hub took a key no member issued"
as sam verify ACME-0 >/dev/null || fail "Sam's own copy of the member log changed"
ok "Sam's hub refused the slipped-in key; its copy of the member log still checks"

# Take the slipped-in entry out again, so the workspace hub's database and
# what it holds in memory agree for the last step.
sqlite3 "$t/ws/state/workspace.db" "DELETE FROM entries WHERE docket = 'ACME-0' AND seq = (SELECT MAX(seq) FROM entries WHERE docket = 'ACME-0')"
fi

step "Jordan removes Sam: Sam's hub can't write any more"
as jordan team remove sam --reason "left the team" | grep >/dev/null "out of the workspace" || fail "wh team remove"
as jordan team members | grep >/dev/null "sam .*(removed)" || fail "Sam isn't shown as removed"
as sam note "$S" "one more thing" >/dev/null 2>&1 || true
# Sam's hub can't get in any more: the note waits (nothing is lost), and
# its hub says why.
refused "isn't an active member" as sam team sync || fail "the workspace let Sam's hub in"
sync jordan
api jordan "/api/dockets/$S" | jq -e '[.entries[] | select(.body == "one more thing")] | length == 0' >/dev/null || fail "Sam's note reached Jordan after Sam was removed"
as jordan verify ACME-0 >/dev/null || fail "the member log doesn't check after the removal"
ok "Sam is out; the workspace refuses Sam's hub, and what Sam wrote before still checks"

printf '\nTeams: every step passed.\n'
