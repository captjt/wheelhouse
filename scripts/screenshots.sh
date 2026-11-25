#!/bin/bash
# Render the README's screenshots and the GitHub social preview card from a
# made-up world, so nothing of yours shows up in them:
#
# - Rex sessions "acme-api" and "storefront" with agents: one at work on
#   webhook retries, one asking to run the checkout tests, one failed, some
#   finished; and a Herdr space "acme-web" beside them, with a Codex at work
#   on the landing page and a Claude Code asking to run the a11y tests;
# - their prompts and replies, played through the same hooks Claude Code
#   calls; an intent with acceptance; files kept with a docket; a message
#   to you with options;
# - Review history for acme/storefront, imported from a made-up
#   rex-reviewer ledger.
#
# The hub follows that world through scripts/fake-rex and scripts/fake-hc, with its own HOME,
# state, socket and port; GitHub and Linear are stand-ins and nothing
# notifies. Your own Rex, hub and config are left alone. Needs jq, Node 22
# or later, and Google Chrome. Run it again after changing a layout; then
# upload docs/images/social-preview.png in the repo's Settings, under
# Social preview.
#
#   scripts/screenshots.sh           # render the screenshots
#   scripts/screenshots.sh --serve   # keep the made-up world's hub running
#                                    # at http://127.0.0.1:7438 (Ctrl-C stops it)
set -euo pipefail
serve=""
[ "${1:-}" = --serve ] && serve=1

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
images="$root/docs/images"
# Its real path (macOS puts temp folders under a /var that links to /private/var): paths under HOME read as ~.
demo=$(cd "$(mktemp -d)" && pwd -P)
port=${PORT:-7438}
base="http://127.0.0.1:$port"
hub=""
cleanup() {
  [ -n "$hub" ] && kill "$hub" 2>/dev/null || true
  rm -rf "$demo"
}
trap cleanup EXIT

(cd "$root/web" && npm run build --silent >/dev/null)
cargo build --release --quiet --manifest-path "$root/Cargo.toml"
wh="$root/target/release/wh"
mkdir -p "$images" "$demo/home/code/acme-api" "$demo/home/code/storefront" "$demo/home/code/acme-web" "$demo/state"
# Herdr has nothing open until the Rex agents have their dockets, so their
# numbers stay the same (the Mac app's shots pick dockets by number).
echo '{"terminals": [], "sent": [], "keys": [], "focused": [], "restarts": 0, "n": 0}' >"$demo/hc.json"
# acme/api is a real checkout, with checks the hub can run (Verify).
(
  cd "$demo/home/code/acme-api"
  git init --quiet -b main && git config user.email you@example.com && git config user.name You
  printf '#!/bin/sh\necho "webhooks: 18 passed"\necho "checkout: 41 passed"\necho "Test Files  12 passed (12)"\n' >run-tests
  printf '#!/bin/sh\necho "src/deliver.ts:14:7  warning  Prefer a named constant for 250  no-magic-numbers" >&2\nexit 1\n' >lint
  chmod +x run-tests lint
  printf 'export function deliver() {}\n' >deliver.ts
  git add -A && git commit --quiet -m "chore: start"
)

# --- The world --------------------------------------------------------------

cat >"$demo/world.jq" <<'EOF'
# Times are in minutes before now.
def ago($m): (now - $m * 60) | floor | todate;
def pane($label; $cwd; $process; $records):
  {block_id: "block:\($label)", label: $label, cwd: $cwd, process: $process, records: $records};
def root($state; $app; $m): {state: $state, app: $app, updated_at: ago($m)};
{
  sessions: [
    {session_id: "session:acme-api", label: "acme-api", windows: [
      {window_id: "window:webhooks", label: "webhooks", blocks: [
        pane("webhooks"; $api; "claude"; [root("working"; "claude-code"; 2) + {msg: "Running the webhook tests"}]) + {screen: $webhooks},
        pane("checkout"; $api; "claude"; [root("blocked"; "claude-code"; 1) + {kind: "permission", msg: "Bash: pnpm test --filter checkout"}]) + {screen: $checkout}
      ]},
      {window_id: "window:data", label: "data", blocks: [
        pane("search"; $api; "codex"; [root("done"; "codex"; 6)]),
        pane("migrations"; $api; "codex"; [root("error"; "codex"; 9) + {msg: "Migration 0042 fails on the replica: column \"region\" already exists"}])
      ]}
    ]},
    {session_id: "session:storefront", label: "storefront", windows: [
      {window_id: "window:pricing", label: "pricing", blocks: [
        pane("pricing"; $store; "claude"; [root("idle"; "claude-code"; 14)]),
        pane("docs"; $store; "claude"; [root("done"; "claude-code"; 22)])
      ]}
    ]}
  ],
  home: $home
}
EOF
jq -n --rawfile webhooks "$here/screens/webhooks.txt" --rawfile checkout "$here/screens/checkout.txt" \
  --arg home "$demo/home" --arg api "$demo/home/code/acme-api" --arg store "$demo/home/code/storefront" \
  -f "$demo/world.jq" >"$demo/world.json"

# GitHub, as a stand-in: you're "you", and no PR asks for you right now.
cat >"$demo/gh" <<'EOF'
#!/bin/bash
case "$*" in
  "api user"*) echo you ;;
  "api graphql"*) echo '{"data":{"viewer":{"login":"you"},"search":{"nodes":[]}}}' ;;
  *) echo "not in the demo: gh $*" >&2; exit 1 ;;
esac
EOF
chmod +x "$demo/gh"

# Review: a config for a repo the demo never reviews, and rex-reviewer's
# ledger for acme/storefront: a PR merged, one waiting on its author.
cat >"$demo/review.lua" <<EOF
return { repos = { ["acme/api"] = { checkout = "$demo/home/code/acme-api" } }, reviewers = { "claude", { name = "codex", agent = "codex" } }, checker = "claude", post = "ask" }
EOF
# Build: acme/api's checks, which the hub runs itself.
cat >"$demo/build.lua" <<EOF
return { repos = { ["acme/api"] = { checkout = "$demo/home/code/acme-api",
  checks = { test = "./run-tests", lint = { run = "./lint", required = false } } } } }
EOF
mkdir -p "$demo/reviewer/repos/acme/storefront"
echo '{"schema": 1}' >"$demo/reviewer/state.json"
ago() { date -u -v-"$1"M +%Y-%m-%dT%H:%M:%SZ; }
round() { # n verdict risk blocking non_blocking minutes-ago
  jq -n --argjson n "$1" --arg v "$2" --arg r "$3" --argjson b "$4" --argjson nb "$5" --arg at "$(ago "$6")" --arg req "$(ago $(($6 + 40)))" \
    '{round: $n, verdict: $v, merge_risk: $r, blocking: $b, non_blocking: $nb, at: $at, requested_at: $req, started_at: $req, by: "you", trigger: "requested",
      reviewers: {claude: {drafted: 4, kept: 3, only: 1, effort: "high", tokens: 2100000}, codex: {drafted: 3, kept: 2, only: 1, effort: "high", tokens: 1700000}},
      tokens: {total: 3800000, input: 900000, output: 120000, cache_read: 2700000, cache_write: 80000}, url: "https://github.com/acme/storefront/pull/0"}'
}
jq -n --argjson h "[$(round 1 REQUEST_CHANGES high 2 1 700), $(round 2 APPROVE low 0 1 380)]" --arg at "$(ago 380)" \
  '{repo: "acme/storefront", pr: 205, round: 2, state: "cleaned", outcome: "merged", title: "Cache product images at the edge", author: "dana",
    url: "https://github.com/acme/storefront/pull/205", verdict: "APPROVE", merge_risk: "low", posted_at: $at, ended_at: $at,
    implementation: "Serves product images through the CDN with a 1-day cache and purges on upload.", history: $h}' \
  >"$demo/reviewer/repos/acme/storefront/pr-205.json"
jq -n --argjson h "[$(round 1 REQUEST_CHANGES medium 1 2 95)]" --arg at "$(ago 95)" \
  '{repo: "acme/storefront", pr: 212, round: 1, state: "posted", title: "Retry failed webhook deliveries with backoff", author: "sam",
    url: "https://github.com/acme/storefront/pull/212", verdict: "REQUEST_CHANGES", merge_risk: "medium", posted_at: $at,
    implementation: "Retries 5xx and timeouts up to 3 times with exponential backoff; 4xx fail at once.", history: $h}' \
  >"$demo/reviewer/repos/acme/storefront/pr-212.json"
jq -n --arg at "$(ago 12)" \
  '{repo: "acme/storefront", pr: 214, round: 0, state: "queued", title: "Discount rules for bundles", author: "dana",
    url: "https://github.com/acme/storefront/pull/214", requested_at: $at, trigger: "requested", history: []}' \
  >"$demo/reviewer/repos/acme/storefront/pr-214.json"

# --- The hub ----------------------------------------------------------------

export HOME="$demo/home" WH_STATE="$demo/state" WH_SOCKET="$demo/wh.sock" WH_HTTP="127.0.0.1:$port" \
  WH_TAILNET=off WH_NOTIFY=off WH_LINEAR_KEY=demo WH_LINEAR_ENDPOINT=http://127.0.0.1:9 \
  WH_REX="$here/fake-rex" WH_WORLD="$demo/world.json" \
  WH_HC="$here/fake-hc" WH_HC_WORLD="$demo/hc.json" WH_HERDR_APP=/usr/bin/true \
  WH_GH="$demo/gh" WH_REVIEW_CONFIG="$demo/review.lua" WH_BUILD_CONFIG="$demo/build.lua" \
  REX_REVIEWER_STATE_DIR="$demo/reviewer" WH_LISTENER="$here/fake-listener" USER=you
# Claude Code's hooks, in the made-up HOME, so the app shows them on. Never
# your own config, even when this runs inside Claude Code.
unset CLAUDE_CONFIG_DIR CODEX_HOME HC_TERMINAL_ID HC_SCOPE HC_TERMINAL_SOCKET
mkdir -p "$demo/home/.claude"
"$wh" hooks install >/dev/null
"$wh" serve >"$demo/hub.log" 2>&1 &
hub=$!
api() { curl -fsS -H 'x-wheelhouse: 1' -H 'content-type: application/json' "$@"; }
for _ in $(seq 60); do
  n=$(api "$base/api/state" 2>/dev/null | jq '[.dockets[] | select(.agent)] | length' 2>/dev/null || echo 0)
  [ "$n" -ge 6 ] && break
  sleep 0.5
done
[ "$n" -ge 6 ] || { echo "the demo hub didn't pick up its agents:" >&2; cat "$demo/hub.log" >&2; exit 1; }
docket() { api "$base/api/state" | jq -r --arg p "$1" '.dockets[] | select(.agent.pane == $p) | .id'; }
hook() { # pane (a Rex block's label, or herdr:tN) event payload
  local b=$1
  case $b in herdr:*) ;; *) b="block:$b" ;; esac
  api -X POST "$base/api/hook" -d "$(jq -n --arg b "$b" --arg e "$2" --argjson p "$3" '{event: $e, agent: "claude-code", block: $b, payload: $p}')" >/dev/null
}
prompt() { hook "$1" UserPromptSubmit "$(jq -n --arg t "$2" --arg s "s-${1#herdr:}" '{session_id: $s, prompt: $t}')"; }
reply() { hook "$1" Stop "$(jq -n --arg t "$2" '{last_assistant_message: $t}')"; sleep 4; }

for b in webhooks checkout pricing docs; do
  hook "$b" SessionStart "$(jq -n --arg s "s-$b" '{session_id: $s, source: "startup"}')"
done
prompt webhooks "Retry failed webhook deliveries with backoff. A 5xx or a timeout is retried up to 3 times; a 4xx isn't. Tests for both."
cat >"$demo/reply-webhooks.md" <<'EOF'
Here's the plan before I change anything:

1. **`deliver.ts`**: wrap the POST in a retry loop. 5xx and timeouts retry with backoff (1s, 2s, 4s); a 4xx fails at once, since resending can't fix it.
2. **Jitter**: up to 250 ms on each wait, so one outage doesn't make every retry land at the same moment.
3. **Tests**: one per case, with a fake clock so they run in milliseconds.

| Response | Retried | Logged |
| --- | :---: | --- |
| 5xx, timeout | up to 3 times | each attempt |
| 4xx | no | once, as failed |

Going ahead with that.
EOF
reply webhooks "$(cat "$demo/reply-webhooks.md")"
prompt webhooks "Also log each retry with its attempt number, so support can see them."
prompt checkout "The checkout tests fail after the cart refactor. Fix them."
prompt pricing "Bundle discounts: apply the larger of the bundle price and the item discounts, never both."
reply pricing "Done. **\`applyBundle\`** now takes the larger discount, never both, and the cart test covers a bundle that also has an item on sale. All 214 pricing tests pass. Not committed: say if you want a PR."
prompt docs "Update the API docs for webhook retries."
reply docs "Updated **docs/webhooks.md**: retries, which responses are final, and the new \`X-Attempt\` header. The OpenAPI file is regenerated."

W=$(docket webhooks)
C=$(docket checkout)
S=$(docket search)
M=$(docket migrations)
"$wh" set "$W" --title "Retry failed webhook deliveries" >/dev/null
"$wh" set "$C" --title "Fix the checkout tests after the cart refactor" >/dev/null
"$wh" set "$S" --title "Reindex products after the catalog import" >/dev/null
"$wh" set "$M" --title "Add a region column to orders" >/dev/null
"$wh" set "$(docket docs)" --status done >/dev/null
"$wh" intent "$W" - >/dev/null <<'EOF'
A flaky endpoint shouldn't lose events: failed deliveries retry with backoff, and the ones that can't succeed stop at once.

## Acceptance

- [x] A 5xx or a timeout is retried up to 3 times, with backoff and jitter
- [x] A 4xx isn't retried, and is logged once as failed
- [ ] Each retry is logged with its attempt number
- [ ] `pnpm test --filter webhooks` passes

> [!NOTE]
> Out of scope: a dead-letter queue. That's its own docket.
EOF
"$wh" intent "$C" - >/dev/null <<'EOF'
The checkout tests pass again after the cart refactor, without changing what checkout does.

## Acceptance

- [ ] `pnpm test --filter checkout` passes
- [ ] No change to checkout's behavior: only imports and fixtures
EOF
cat >"$demo/retry-design.md" <<'EOF'
# Webhook retries

Deliveries that fail with a 5xx or time out are retried three times, waiting 1s, 2s and 4s, each with up to 250 ms of jitter. A 4xx is final: the endpoint said no, and resending won't change its mind.

| Attempt | Wait | Total |
| --- | --- | --- |
| 1 | 1s | 1s |
| 2 | 2s | 3s |
| 3 | 4s | 7s |

After the third failure the delivery is marked failed, and the endpoint's failure count goes up. Five in an hour pauses the endpoint.
EOF
# A decision in force on the hero docket: the agent asked, you answered.
q=$(api -X POST "$base/api/messages" -d "$(jq -n '{to: "me", block: "block:webhooks", subject: "How many retries?",
  body: "Three keeps a down endpoint to four calls a minute; five rides out longer blips. Which cap?", options: ["3", "5"]}')" | jq -r .id)
api -X POST "$base/api/messages/$q/reply" -d '{"body": "To match the API docs.", "choice": "3"}' >/dev/null
"$wh" attach "$demo/retry-design.md" --docket "$W" --title "Retry design" >/dev/null
"$wh" attach https://claude.ai/artifact/demo-retry-timeline --docket "$W" --title "Retry timeline (Claude artifact)" >/dev/null
api -X POST "$base/api/messages" -d "$(jq -n '{to: "me", block: "block:search", subject: "Reindex now or tonight?",
  body: "The catalog import finished: 18,204 products. Reindexing takes about 20 minutes and slows search while it runs. I would run it tonight at 2am, when traffic is lowest; meanwhile new products only show up by exact name.",
  options: ["Tonight at 2am", "Now"]}')" >/dev/null
api -X POST "$base/api/import/rex-reviewer" -d '{"force": true}' >/dev/null
api -X POST "$base/api/review/settings" -d '{"enabled": true, "paused": true}' >/dev/null || true
api -X POST "$base/api/build/settings" -d '{"paused": true}' >/dev/null || true
# Verify: the hero docket's checks, run by the hub; a docket on hold.
"$wh" check "$W" --wait >/dev/null || true
"$wh" hold "$M" "Waiting on the DBA's review of the migration" >/dev/null

# Changes: the webhook docket's branch, one commit and an edit not yet
# committed, and a comment on the line the agent should change.
(
  cd "$demo/home/code/acme-api"
  git update-ref refs/remotes/origin/main main && git symbolic-ref refs/remotes/origin/HEAD refs/remotes/origin/main
  git checkout --quiet -b eng-142-retry-failed-webhooks
  cat >deliver.ts <<'TS'
export function deliver() {}

/** Retry a failed delivery: 5xx and timeouts only, with backoff. */
export async function retry(send: () => Promise<Response>) {
  const tries = 5;
  for (let i = 0; i < tries; i++) {
    const r = await send();
    if (r.ok || (r.status >= 400 && r.status < 500)) return r;
    await sleep(250 * 2 ** i);
  }
}
TS
  git commit --quiet -am "feat: retry failed deliveries"
  printf '\nconst sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));\n' >>deliver.ts
)
base_sha=$(git -C "$demo/home/code/acme-api" rev-parse main)
head_sha=$(git -C "$demo/home/code/acme-api" rev-parse HEAD)
api -X POST "$base/api/dockets/$W/comments" -d "$(jq -n --arg from "$base_sha" --arg head "$head_sha" \
  '{path: "deliver.ts", side: "additions", line: 5, from: $from, to: null, head: $head, excerpt: "  const tries = 5;", tell: false,
    body: "Cap it at **3**, as decided: five keeps a down endpoint busy for too long."}')" >/dev/null

# A call: its transcript in, and the Listener's proposals (a stand-in, so
# the shots stay the same); one applied, so both states show.
C=$(api -X POST "$base/api/calls" -d "$(jq -n '{title: "Webhooks sync", transcript: "Dana: Where are we on the webhook retries?\nSam: Retries are 5xx and timeouts only, and three tries at most. That is settled.\nDana: Good. Can the checkout agent tell us whether the cart test covers a bundle on sale?\nSam: I will ask. Somebody should write the runbook for the outage page before Friday.\nDana: Agreed, make it its own piece of work."}')" | jq -r .docket)
for _ in $(seq 60); do
  [ "$(api "$base/api/dockets/$C" | jq -r '.call.listening.state')" = done ] && break
  sleep 0.5
done
api -X POST "$base/api/calls/$C/proposals/1" -d '{"action": "apply"}' >/dev/null

# Recap's tokens and cost: a week of made-up usage, kept the way the hub's
# snapshots keep it (no transcripts in this world to read), one model unpriced.
python3 - "$demo/state/wheelhouse.db" "$(api "$base/api/state" | jq -r '[.dockets[].id] | join(",")')" <<'PY'
import random, sqlite3, sys, time
db, ids = sqlite3.connect(sys.argv[1]), sys.argv[2].split(",")
random.seed(7)
now = int(time.time()) // 60 * 60
plan = [("anthropic", "Opus 5.5", 9_000_000), ("anthropic", "Opus 5.5", 5_000_000), ("openai", "GPT-6.2 Sol", 4_000_000),
        ("openai", "GPT-6.2 Astra", 1_500_000), ("google", "Gemini 4 Neon", 600_000), ("anthropic", "Haiku 5.5", 200_000), ("ollama", "Llama 4", 300_000)]
for n, (provider, model, per_day) in enumerate(plan):
    docket = ids[n % len(ids)]
    for day in range(7):
        for k in range(6):
            minute = now - day * 86400 - k * 3600 - random.randint(0, 50) * 60
            t = int(per_day / 6 * random.uniform(0.5, 1.5))
            write = int(t * 0.06)
            db.execute("INSERT OR REPLACE INTO usage (file, minute, provider, model, docket, input, cache_read, cache_write, output, write_1h) VALUES (?,?,?,?,?,?,?,?,?,?)",
                       (f"demo:{n}", minute, provider, model, docket, int(t * 0.02), int(t * 0.9), write, int(t * 0.02), int(write * 0.95) if provider == "anthropic" else 0))
for k in ("usage.snapshot", "usage.snapshot_ran"):
    db.execute("INSERT OR REPLACE INTO meta (k, v) VALUES (?, ?)", (k, str(now)))
db.commit()
PY

# Herdr: the acme-web space, beside Rex's sessions, last, so its dockets
# come after the others (D-8, D-9).
ms() { echo $(( ($(date +%s) - $1 * 60) * 1000 )); }
herdr_term() { # address pane app status-json minutes-ago
  jq -nc --arg a "$1" --arg p "$2" --arg app "$3" --argjson st "$4" --argjson ms "$(ms "$5")" --arg c "$demo/home/code/acme-web" --rawfile screen "$here/screens/$2.txt" \
    '{address: $a, state: "live", cwd: $c, name: null, labels: {"herdr.space": "acme-web", "herdr.tab": ("acme-web." + $p), "wh.session": "acme-web", "wh.tab": $p, "wh.pane": $p},
      status: ($st + {app: $app}), status_ms: $ms, foreground: {name: (if $app == "codex" then "codex" else "claude" end), cwd: $c}, screen: $screen}'
}
jq -n --argjson a "$(herdr_term t1 landing codex '{"state": "working", "msg": "Rebuilding the hero with the new pricing"}' 1)" \
  --argjson b "$(herdr_term t2 a11y claude-code '{"state": "blocked", "kind": "permission", "msg": "Bash: npx playwright test a11y"}' 0)" \
  '{terminals: [$a, $b], sent: [], keys: [], focused: [], restarts: 0, n: 2}' >"$demo/hc.json.new" && mv "$demo/hc.json.new" "$demo/hc.json"
for _ in $(seq 60); do
  n=$(api "$base/api/state" | jq '[.agents[] | select(.runtime == "herdr" and .docket)] | length')
  [ "$n" -ge 2 ] && break
  sleep 0.5
done
[ "$n" -ge 2 ] || { echo "the demo hub didn't pick up Herdr's agents:" >&2; tail "$demo/hub.log" >&2; exit 1; }
for b in herdr:t1 herdr:t2; do hook "$b" SessionStart "$(jq -n --arg s "s-${b#herdr:}" '{session_id: $s, source: "startup"}')"; done
prompt herdr:t1 "The landing page's hero still shows last year's prices. Rebuild it from the pricing API, and keep the layout."
prompt herdr:t2 "Run the accessibility checks on the new checkout and fix what they find."
L=$(api "$base/api/state" | jq -r '.agents[] | select(.block == "herdr:t1") | .docket')
"$wh" set "$L" --title "Rebuild the landing hero from the pricing API" >/dev/null
"$wh" set "$(api "$base/api/state" | jq -r '.agents[] | select(.block == "herdr:t2") | .docket')" --title "Fix the checkout's a11y findings" >/dev/null
# An agent in Herdr asks one in Rex: they talk across runtimes.
api -X POST "$base/api/messages" -d "$(jq -n '{to: "acme-api/webhooks", block: "herdr:t1", body: "Does the pricing API retry on a 5xx now? The hero reads it on every deploy."}')" >/dev/null
sleep 2

if [ -n "$serve" ]; then
  echo "The made-up world's hub: $base (Ctrl-C stops it)"
  wait "$hub"
  exit 0
fi

# --- The shots ---------------------------------------------------------------

shot() { node "$here/shot.mjs" "$@" >/dev/null && echo "wrote ${2#"$root"/}"; }
# Scroll the docket to its intent, past the title, for the hero shot.
intent_js='document.querySelector(".docket-main")?.scrollTo(0, 0)'
record_js='document.querySelector(".verify-chip")?.click(); new Promise(r => setTimeout(r, 900))'
runtimes_js='document.getElementById("set-runtimes")?.scrollIntoView(); new Promise(r => setTimeout(r, 300))'
artifact_js='[...document.querySelectorAll(".art-card")].find(c => c.textContent.includes("Retry design"))?.click(); new Promise(r => setTimeout(r, 900))'
for look in light dark; do
  shot "$base/#/d/$W" "$images/docket-$look.png" "$look" desktop "$intent_js"
  shot "$base/#/inbox" "$images/inbox-$look.png" "$look" desktop
  shot "$base/#/reviews" "$images/reviews-$look.png" "$look" desktop
  shot "$base/#/d/$W" "$images/record-$look.png" "$look" desktop "$record_js"
  shot "$base/#/recap" "$images/recap-$look.png" "$look" desktop
  shot "$base/#/d/$W/changes" "$images/changes-$look.png" "$look" desktop
  shot "$base/#/d/$C" "$images/call-$look.png" "$look" desktop 'document.querySelector(".call")?.scrollIntoView({block: "start"})'
  shot "$base/#/settings" "$images/settings-$look.png" "$look" desktop
  shot "$base/#/settings" "$images/runtimes-$look.png" "$look" desktop "$runtimes_js"
  shot "$base/#/agents" "$images/agents-$look.png" "$look" desktop
  shot "$base/#/d/$L" "$images/docket-herdr-$look.png" "$look" desktop "$intent_js"
  shot "$base/#/d/$W" "$images/phone-chat-$look.png" "$look" phone
  shot "$base/#/inbox" "$images/phone-inbox-$look.png" "$look" phone
  shot "$base/#/view/active" "$images/phone-active-$look.png" "$look" phone
  shot "$base/#/d/$W" "$images/phone-artifact-$look.png" "$look" phone \
    "[...document.querySelectorAll('.docket-seg button')].find(b => b.textContent === 'Docket')?.click(); new Promise(r => setTimeout(r, 600)).then(() => { $artifact_js })"
done

# Wheelhouse.app, against the same world: it renders its own windows (kept
# behind yours; the app never takes the keyboard). Needs Xcode 26, and
# screen recording for your terminal, as screencapture does.
if swift --version 2>/dev/null | grep -qE "Swift version (6\.[2-9]|[7-9])"; then
  (cd "$root/mac" && swift build -c release --product Wheelhouse >/dev/null)
  WH_APP_HUB="$base" WH_SNAP_HERDR="$L" "$root/mac/.build/release/Wheelhouse" --snapshot "$images" \
    --only "${MAC_SHOTS:-docket,inbox,chat,reviews,board,record,quick,shortcuts,window,menubar,verify,settings-orders,settings-agents,settings-guides,agents,docket-herdr}" | sed "s|$root/||"
else
  echo "skipped Wheelhouse.app's shots: they need Xcode 26 (Swift 6.2)"
fi

# The social preview card shows docket-dark.png, so render it after.
chrome="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
# A profile of its own: never your Chrome's, which may be open.
# A mock keychain too: no Keychain prompt for "Chrome Safe Storage".
"$chrome" --headless=new --disable-gpu --hide-scrollbars --use-mock-keychain --password-store=basic --no-first-run \
  --window-size=1280,640 --user-data-dir="$demo/chrome" \
  --screenshot="$images/social-preview.png" "file://$images/social-preview.html" >/dev/null 2>&1
echo "wrote docs/images/social-preview.png"
