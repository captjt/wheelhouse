#!/bin/bash
# Pose the made-up world's ask again ("Reindex now or tonight?", on the
# search agent's docket), so scripts/answer.json can answer it once more.
# Only ever a trial hub: never the daily one on :7500.
set -euo pipefail
hub=${WH_HUB:-http://127.0.0.1:7458}
case "$hub" in *:7500 | *:7500/*) echo "ask.sh: $hub is the daily hub" >&2; exit 2 ;; esac
api() { curl -fsS -H 'x-wheelhouse: rune' -H 'content-type: application/json' "$@"; }
# Answer any copy still open first, so there's exactly one.
for id in $(api "$hub/api/state" | jq -r '.messages[] | select(.subject == "Reindex now or tonight?") | .id'); do
  api -X POST "$hub/api/messages/$id/done" >/dev/null
done
api -X POST "$hub/api/messages" -d "$(jq -n '{to: "me", block: "block:search", subject: "Reindex now or tonight?",
  body: "The catalog import finished: 18,204 products. Reindexing takes about 20 minutes and slows search while it runs. I would run it tonight at 2am, when traffic is lowest; meanwhile new products only show up by exact name.",
  options: ["Tonight at 2am", "Now"]}')" | jq -r '"posed #\(.id)"'
