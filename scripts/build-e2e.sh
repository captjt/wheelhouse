#!/usr/bin/env bash
# Build, end to end, on a scratch hub with stand-ins: a made-up Rex
# (scripts/fake-rex), a made-up gh (scripts/fake-gh), and a local origin.
# This script plays the builder and the checker (writing their files, as
# the briefs ask) and you (answering each ask through the API). Nothing
# touches your Rex, GitHub, Linear, or ~/.config.
#
#   scripts/build-e2e.sh           # run it; exits non-zero on the first failure
#   KEEP=1 scripts/build-e2e.sh    # keep the scratch directory to look at
#
# It goes: plan → approve → build → check → open the PR; then a round on
# review feedback and a failing check (a reply that speaks in the first
# person is refused first), pushed with its replies; then a round on a base
# that moved, merged in and pushed; then the merge, which cleans up. Then
# Linear intake (a stand-in Linear): a labeled issue starts a Build, an
# issue in your cycle is offered one, and a skip label is left alone.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
t=$(mktemp -d)
port=${PORT:-7614}
base="http://127.0.0.1:$port"
hub=""
linear=""
cleanup() {
  for p in "$hub" "${linear:-}"; do
    [ -n "$p" ] || continue
    kill "$p" 2>/dev/null || true
    wait "$p" 2>/dev/null || true
  done
  if [ -n "${KEEP:-}" ]; then echo "kept $t"; else rm -rf "$t"; fi
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$*"; }
ok() { printf '   ok: %s\n' "$*"; }
fail() {
  printf '\nFAIL: %s\n' "$*" >&2
  printf '\nthe Build: %s\n' "$(task 2>/dev/null)" >&2
  printf '\nthe hub log:\n' >&2
  tail -25 "$t/hub.log" >&2
  exit 1
}

cargo build --release --quiet --manifest-path "$root/Cargo.toml"
wh="$root/target/release/wh"

# --- The origin, the checkout, the world -------------------------------------

origin="$t/origin/acme/api.git"
git init --bare --quiet -b main "$origin"
git clone --quiet "$origin" "$t/checkout" 2>/dev/null
(
  cd "$t/checkout"
  git config user.email you@example.com
  git config user.name You
  printf 'export function deliver() {}\n' >deliver.ts
  git add . && git commit --quiet -m "chore: start"
  git push --quiet origin HEAD:main
)
mkdir -p "$t/home" "$t/gh" "$t/state"
echo you >"$t/gh/login"
echo "{\"sessions\": [], \"home\": \"$t/home\"}" >"$t/world.json"
cat >"$t/build.lua" <<EOF
return {
  repos = { ["acme/api"] = { checkout = "$t/checkout" } },
  sources = { linear = { teams = { ENG = "acme/api" }, label = "agent", cycle = true, skip_label = "no-agent" } },
}
EOF
# Linear: a stand-in server, with no issues until the last part.
lport=$((port + 1))
echo '[]' >"$t/linear.json"
python3 "$here/fake-linear.py" "$lport" "$t/linear.json" &
linear=$!

export HOME="$t/home" USER=you WH_STATE="$t/state" WH_SOCKET="$t/wh.sock" WH_HTTP="127.0.0.1:$port" \
  WH_TAILNET=off WH_NOTIFY=off WH_LINEAR_KEY=test WH_LINEAR_ENDPOINT="http://127.0.0.1:$lport/graphql" \
  WH_REX="$here/fake-rex" WH_WORLD="$t/world.json" \
  WH_GH="$here/fake-gh" WH_FAKE_GH="$t/gh" WH_GIT_BASE="$t/origin" WH_BUILD_CONFIG="$t/build.lua" \
  WH_REVIEW_CONFIG="$t/none.lua" WH_BUILD_DATA="$t/build" WH_BUILD_POLL_SECONDS=2
unset CLAUDE_CONFIG_DIR CODEX_HOME REX_SESSION REX_BLOCK WH_LINEAR_OFF LINEAR_API_KEY
"$wh" serve >"$t/hub.log" 2>&1 &
hub=$!

api() { curl -fsS -H 'x-wheelhouse: 1' -H 'content-type: application/json' "$@"; }
for _ in $(seq 60); do api "$base/api/state" >/dev/null 2>&1 && break; sleep 0.25; done
api "$base/api/state" >/dev/null || fail "the hub didn't start"

task() { api "$base/api/build" | jq -c '.tasks[] | select(.docket == "D-1")'; }
until_() { # what jq-filter-on-the-Build
  for _ in $(seq 120); do
    task | jq -e "$2" >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  fail "$1"
}
ask_id() { # option: the open ask that offers it
  api "$base/api/state" | jq -r --arg o "$1" '[.messages[] | select(.docket == "D-1" and (.options | index($o)))][0].id // empty'
}
answer() { # option
  local id=""
  for _ in $(seq 60); do
    id=$(ask_id "$1")
    [ -n "$id" ] && break
    sleep 0.5
  done
  [ -n "$id" ] || fail "no ask offers \"$1\""
  api -X POST "$base/api/messages/$id/reply" -d "$(jq -nc --arg c "$1" '{body: "", choice: $c}')" >/dev/null
}
wt="$t/build/worktrees/D-1"
say() { printf '%s\n' "$1" >"$wt/.tmp/$2"; } # text file: an agent's status line
commit() { git -C "$wt" add -A -- . ':!.tmp' && git -C "$wt" commit --quiet -m "$1" && git -C "$wt" rev-parse --short HEAD; }

# --- Plan, build, check, open ------------------------------------------------

step "a docket with an intent, and its Build"
api -X POST "$base/api/dockets" -d '{"title": "Retry failed webhook deliveries"}' >/dev/null
api -X POST "$base/api/dockets/D-1/intent" -d '{"body": "Failed deliveries retry with backoff.\n\n## Acceptance\n- [ ] A 5xx is retried up to 3 times"}' >/dev/null
api -X POST "$base/api/build/start" -d '{"from": "D-1", "repo": "acme/api"}' >/dev/null
until_ "the builder starts planning" '.state == "active" and .phase == "planning"'
ok "the builder is planning in its own pane"

step "the plan, and your approval"
cat >"$wt/.tmp/plan.md" <<'EOF'
## Intent
Failed deliveries retry with backoff.

## Acceptance
- [ ] A 5xx is retried up to 3 times

## Approach
Wrap the POST in a retry loop.

## Out of scope
- A dead-letter queue
EOF
say PLAN_READY status.txt
until_ "the plan reaches its gate" '.phase == "plan_gate"'
answer Approve
until_ "the builder builds" '.phase == "building"'
ok "approved; building"

step "the build, the check, and opening the PR"
printf 'export function deliver() { /* retries 5xx up to 3 times */ }\n' >"$wt/deliver.ts"
commit "fix(webhooks): retry failed deliveries" >/dev/null
echo '{"title": "fix(webhooks): retry failed deliveries", "body": "A 5xx or a timeout is retried up to 3 times, with backoff."}' >"$wt/.tmp/pr.json"
echo "Built the retry loop." >"$wt/.tmp/report.md"
say BUILT status.txt
until_ "the checker checks" '.phase == "checking"'
mkdir -p "$wt/.tmp/check"
echo '{"verdict": "pass", "summary": "The retry loop matches the plan.", "findings": []}' >"$wt/.tmp/check/check-1.json"
say CHECKED checker-status.txt
until_ "the PR reaches its gate" '.phase == "open_gate"'
answer "Open PR"
until_ "the PR opens" '.state == "open"'
head1=$(git -C "$wt" rev-parse HEAD)
[ "$(git --git-dir="$origin" rev-parse "$(task | jq -r .branch)")" = "$head1" ] || fail "origin doesn't have the checked head"
jq -e 'select(.kind == "pr") | .draft == true and (.body | contains("on behalf of @you"))' "$t/gh/writes.jsonl" >/dev/null || fail "no draft PR with its footer"
ok "pushed $(git -C "$wt" rev-parse --short HEAD) and opened #45 as a draft"

# --- Round 1: review feedback and a failing check -----------------------------

step "review feedback and a failing check come in"
branch=$(task | jq -r .branch)
pr_json() { # state threads reviews comments contexts merge-state base-oid
  jq -n --arg state "$1" --argjson threads "$2" --argjson reviews "$3" --argjson comments "$4" --argjson ctx "$5" \
    --arg merge "$6" --arg boid "$7" --arg head "$(git --git-dir="$origin" rev-parse "$branch")" '{data: {
      viewer: {login: "you"},
      repository: {pullRequest: {
        number: 45, title: "fix(webhooks): retry failed deliveries", state: $state, url: "https://github.com/acme/api/pull/45",
        headRefOid: $head, baseRefName: "main", baseRefOid: $boid, mergeable: "MERGEABLE", mergeStateStatus: $merge,
        author: {login: "you"}, reviewRequests: {nodes: []},
        reviews: {nodes: $reviews}, comments: {nodes: $comments}, reviewThreads: {nodes: $threads},
        commits: {nodes: [{commit: {oid: $head, statusCheckRollup: {state: "FAILURE", contexts: {nodes: $ctx}}}}]}
      }}}}' >"$t/gh/pr-45.json"
}
thread='[{"isResolved": false, "isOutdated": false, "path": "deliver.ts", "line": 1, "comments": {"nodes": [
  {"databaseId": 100, "url": "https://github.com/acme/api/pull/45#discussion_r100", "body": "What stops a retry storm when the endpoint is down?", "createdAt": "2026-11-17T10:00:00Z", "author": {"login": "dana", "__typename": "User"}}]}}]'
review='[{"databaseId": 7, "url": "https://github.com/acme/api/pull/45#pullrequestreview-7", "state": "CHANGES_REQUESTED", "body": "The cap needs a test.", "submittedAt": "2026-11-17T10:01:00Z", "author": {"login": "dana", "__typename": "User"}}]'
failing='[{"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "https://github.com/acme/api/actions/runs/5/job/6"}]'
echo "FAIL deliver.test.ts > gives up after 3 tries: expected 3, got 4" >"$t/gh/job-6.log"
main1=$(git --git-dir="$origin" rev-parse main)
pr_json OPEN "$thread" "$review" '[]' "$failing" CLEAN "$main1"
until_ "the feedback is an ask" '.asks | index("address")'
id=$(ask_id "Address it")
api "$base/api/messages/$id" | jq -e '.subject | contains("a review thread, a review, a failing check")' >/dev/null || fail "the ask doesn't say what's open"
ok "asked: $(api "$base/api/messages/$id" | jq -r .subject)"

step "address it: the builder reopens on the round"
answer "Address it"
until_ "the round starts" '.state == "active" and .phase == "addressing" and .round.n == 1'
r1="$wt/.tmp/address/round-1"
jq -e '.threads | length == 1' "$r1/open.json" >/dev/null || fail "the round's open.json has no thread"
grep -q "expected 3, got 4" "$r1/ci-1.log" || fail "the failing job's log isn't with the round"
grep -q "Merge it in" "$wt/.tmp/tasks/address.md" && fail "the brief asks for a merge, but main hasn't moved"
ok "open.json, the failing log, and the brief are in place"

step "a reply in the first person is refused"
printf 'export function deliver() { /* retries 5xx up to 3 times, then stops */ }\n' >"$wt/deliver.ts"
sha=$(commit "fix(webhooks): stop after the third retry")
cat >"$r1/replies.json" <<EOF
{"replies": [{"comment_id": 100, "body": "I capped it at three in $sha."}],
 "answers": [{"url": "https://github.com/acme/api/pull/45#pullrequestreview-7", "label": "dana's review", "body": "Covered in $sha."}],
 "rerequest": ["dana"]}
EOF
say BUILT status.txt
for _ in $(seq 40); do grep -q "WORKING addressing" "$wt/.tmp/status.txt" && break; sleep 0.5; done
grep -q "WORKING addressing" "$wt/.tmp/status.txt" || fail "the first-person reply wasn't sent back"
[ "$(task | jq -r .phase)" = addressing ] || fail "the round moved on with a first-person reply"
ok "sent back to the builder"

step "the fixed round is checked, and you see every reply"
cat >"$r1/replies.json" <<EOF
{"replies": [{"comment_id": 100, "body": "Fixed in $sha. \`deliver\` now stops after the third retry, so a down endpoint gets at most four calls. \`gives up after 3 tries\` covers it."}],
 "answers": [{"url": "https://github.com/acme/api/pull/45#pullrequestreview-7", "label": "dana's review", "body": "The cap's test is \`gives up after 3 tries\`, in $sha."}],
 "rerequest": ["dana"]}
EOF
say BUILT status.txt
until_ "the round is checked" '.phase == "checking"'
echo '{"verdict": "pass", "summary": "The cap holds, and both replies are true.", "findings": []}' >"$wt/.tmp/check/check-2.json"
say CHECKED checker-status.txt
until_ "the round reaches its push gate" '.phase == "push_gate"'
id=$(ask_id "Push and reply")
api "$base/api/messages/$id" | jq -e --arg s "$sha" '.body | contains("Fixed in " + $s) and contains("on behalf of @you") and contains("requested again from dana")' >/dev/null ||
  fail "the push gate doesn't show the replies as they'll post"
ok "the gate shows each reply with its footer"

step "push and reply"
answer "Push and reply"
until_ "the round is pushed, and the PR followed again" '.state == "open" and .round == null'
[ "$(git --git-dir="$origin" rev-parse "$branch")" = "$(git -C "$wt" rev-parse HEAD)" ] || fail "origin doesn't have the round"
jq -se --arg s "$sha" '[.[] | select(.kind == "reply")] | length == 1 and .[0].comment_id == "100" and (.[0].body | startswith("Fixed in " + $s)) and (.[0].body | contains("on behalf of @you"))' "$t/gh/writes.jsonl" >/dev/null ||
  fail "the thread's reply didn't post as approved"
jq -se '[.[] | select(.kind == "comment")] | length == 1 and (.[0].body | contains("> **[dana'"'"'s review](https://github.com/acme/api/pull/45#pullrequestreview-7)**"))' "$t/gh/writes.jsonl" >/dev/null ||
  fail "the answer to the review didn't post under its link"
jq -se '[.[] | select(.kind == "rerequest")] | .[0].reviewers == ["dana"]' "$t/gh/writes.jsonl" >/dev/null || fail "dana wasn't asked again"
ok "pushed, one thread reply, one comment, dana asked again"

# --- Round 2: the base moved --------------------------------------------------

step "main moves on, and the PR falls behind"
(
  cd "$t/checkout"
  git pull --quiet origin main 2>/dev/null || true
  printf '# acme/api\n' >README.md
  git add README.md && git commit --quiet -m "docs: a README"
  git push --quiet origin HEAD:main
)
main2=$(git --git-dir="$origin" rev-parse main)
# GitHub now shows the replies as answered: the thread's last word is yours,
# and the review is linked from your comment.
answered_thread=$(jq -c '.[0].comments.nodes += [{"databaseId": 101, "url": "u101", "body": "Fixed in …", "createdAt": "2026-11-17T11:00:00Z", "author": {"login": "you", "__typename": "User"}}]' <<<"$thread")
mine='[{"databaseId": 95, "url": "u95", "body": "> **[dana'"'"'s review](https://github.com/acme/api/pull/45#pullrequestreview-7)**", "createdAt": "2026-11-17T11:00:01Z", "author": {"login": "you", "__typename": "User"}}]'
passing='[{"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "SUCCESS", "detailsUrl": "x"}]'
pr_json OPEN "[$(jq -c '.[0]' <<<"$answered_thread")]" "$review" "$mine" "$passing" BEHIND "$main2"
until_ "falling behind is an ask" '.asks | index("address")'
answer "Address it"
until_ "round 2 starts" '.phase == "addressing" and .round.n == 2'
grep -q "Merge it in" "$wt/.tmp/tasks/address.md" || fail "the brief doesn't ask for main to be merged in"
git -C "$wt" merge --quiet --no-edit origin/main
echo '{"replies": [], "answers": [], "rerequest": []}' >"$wt/.tmp/address/round-2/replies.json"
say BUILT status.txt
until_ "round 2 is checked" '.phase == "checking"'
echo '{"verdict": "pass", "summary": "main merges in cleanly.", "findings": []}' >"$wt/.tmp/check/check-3.json"
say CHECKED checker-status.txt
until_ "round 2 reaches its gate" '.phase == "push_gate"'
answer "Push and reply"
until_ "round 2 is pushed" '.state == "open" and .round == null and .rounds == 2'
git --git-dir="$origin" merge-base --is-ancestor "$main2" "$branch" || fail "the pushed branch doesn't have main in it"
git --git-dir="$origin" merge-base --is-ancestor "$head1" "$branch" || fail "the push rewrote the branch"
ok "main merged in and pushed, fast-forward"

# --- The merge ----------------------------------------------------------------

step "the PR merges"
pr_json MERGED '[]' '[]' '[]' "$passing" CLEAN "$main2"
until_ "the Build ends" '.state == "merged"'
[ ! -d "$wt" ] || fail "the worktree is still there"
ok "merged; the worktree is gone"

# --- Linear intake -------------------------------------------------------------

step "Linear: a labeled issue starts a Build; a cycle's issue is offered one"
by_key() { api "$base/api/build" | jq -c --arg k "$1" '.tasks[] | select(.key == $k)'; }
cat >"$t/linear.json" <<'ISSUES'
[
  {"key": "ENG-7", "title": "Retry the outbox", "team": "ENG", "labels": ["agent"], "description": "The outbox retries failed sends.\n\n## Acceptance\n- [ ] A failed send is retried", "branch": "you/eng-7-retry-the-outbox"},
  {"key": "ENG-8", "title": "Paginate the audit log", "team": "ENG", "labels": [], "cycle_active": true, "description": "Page the audit log by 50."},
  {"key": "ENG-9", "title": "Leave this one", "team": "ENG", "labels": ["agent", "no-agent"], "cycle_active": true}
]
ISSUES
for _ in $(seq 60); do [ -n "$(by_key ENG-7)" ] && break; sleep 0.5; done
by_key ENG-7 | jq -e '.source == "linear" and .branch == "you/eng-7-retry-the-outbox"' >/dev/null || fail "ENG-7 didn't start a Build on its Linear branch"
ok "ENG-7 (labeled) started a Build on Linear's branch"
oid=""
for _ in $(seq 60); do
  oid=$(api "$base/api/state" | jq -r '[.messages[] | select(.subject | startswith("ENG-8"))][0].id // empty')
  [ -n "$oid" ] && break
  sleep 0.5
done
[ -n "$oid" ] || fail "ENG-8 (in the cycle) wasn't offered"
api -X POST "$base/api/messages/$oid/reply" -d '{"body": "", "choice": "Build it"}' >/dev/null
for _ in $(seq 60); do [ -n "$(by_key ENG-8)" ] && break; sleep 0.5; done
by_key ENG-8 | jq -e '.source == "linear"' >/dev/null || fail "Build it didn't start ENG-8's Build"
ok "ENG-8 (in the cycle) was offered, and Build it started it"
sleep 5
[ -z "$(by_key ENG-9)" ] || fail "ENG-9 has the skip label, but it was built"
[ "$(api "$base/api/state" | jq '[.messages[] | select(.subject | startswith("ENG-"))] | length')" = 0 ] || fail "an issue was offered twice"
ok "ENG-9 (skip label) left alone; nothing offered twice"

printf '\nBuild end to end: every step passed.\n'
