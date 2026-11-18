# Wheelhouse

Every piece of agent work as a docket, on a record you can check.

Wheelhouse follows Rex. Every Claude Code or Codex agent running in a Rex
pane gets a **docket**. Its record says what you asked, what the agent said
at the end of each turn, when it waited on you, and when it ended. Every
entry is signed and chained, so `wh verify` can show that nothing in the
record was changed, removed, or reordered.

This is the first slice of [DESIGN.md](DESIGN.md): the record
([docs/record.md](docs/record.md)), the Agent protocol, and the app,
including answering prompts and messaging agents from any device. The
Review and Build protocols (what rex-reviewer and rex-develop do today) and
the menu bar app come next.

## Run it

```sh
just build                # the web app, then the binary (the app is built in)
./target/release/wh serve # follow Rex, keep the record, serve the app
```

The app is on http://127.0.0.1:7500. State is in
`~/.local/state/wheelhouse`: the SQLite record, the keys, and the socket the
CLI uses.

For names taken from your prompts, agent turns with their last message, and
a new docket on `/clear`, install Claude Code's hooks. Without them, agents
are still tracked, by pane.

```sh
cargo install --path .    # wh on your PATH, so the hooks have a path that stays put
wh hooks install          # SessionStart, UserPromptSubmit, Stop, beside your other hooks
wh hooks remove
```

## The app

Inbox (agents waiting on you, failed, or finished a turn), Active, All
dockets, Done, Agents, and a view for each Rex session. Lists group by
attention, status, or Rex session, as a list or a board. A docket shows its
signed activity and its properties, with the record's check, head, and
export.

| Keys | |
| --- | --- |
| `⌘K`, `/` | Search dockets and commands |
| `C` | New docket |
| `G` then `I`, `A`, `D`, `X`, `N` | Inbox, Active, All, Done, Agents |
| `J`, `K`, `↩`, `Esc` | Move, open, back (on a docket, `J`/`K` step through the list you came from) |
| `S` | Change status (`1`–`4` in the menu) |
| `O` | On the Mac: show the agent's pane in Rex. Anywhere else: view the pane |
| `R`, `M` | Rename, write a note |
| `E` | In the Inbox: mark done |

**Answering prompts.** When Claude Code waits on a permission, the
docket, its Inbox item, and the pane all offer **Approve** and **Deny**.
Before sending the key, the hub reads the prompt from Rex again and sends
it only if it's the same prompt you saw (same message, same time, the only
one waiting). Each answer goes on the record. There's no single-key
shortcut for these, on purpose; ⌘K has them.

**The pane, from anywhere.** On the Mac, **Open in Rex** brings the
agent's pane to the front, and the eye button peeks at it in the app. On a
phone or another computer, **View pane** shows the agent's terminal, live,
with a box to message it. A message goes in only between turns, and only
into an agent: never mid-turn, never over a prompt it's waiting on, never
into a shell. It goes on the docket's record as yours.

The theme follows your system's light or dark mode. The switch at the
bottom of the sidebar (or "Theme" in ⌘K) pins Light or Dark. Colors come
from Cloudflare's Kumo design system; the tokens are at the top of
`web/src/styles.css`.

## The CLI

| | |
| --- | --- |
| `wh`, `wh ls [--all]` | What needs you, and what's in flight. |
| `wh show D-4` | One docket's record, each entry with how it's known. |
| `wh new "title"` | Open a docket by hand. |
| `wh note D-4 "text"` | Add a note. From an agent's pane, it's signed as the agent. |
| `wh set D-4 --title … --status done` | Rename, or set the status. |
| `wh attach FILE\|URL [--title T] [--docket D-4]` | Keep a file (pinned in the record by its hash) or a link with a docket. From an agent's pane, its own. |
| `wh verify [D-4]` | Check ids, order, chain and signatures, and every kept file against its hash. |
| `wh export D-4 > D-4.jsonl`, `wh verify --file D-4.jsonl` | A record that checks without the hub. |
| `wh build start D-12\|owner/name#45\|GRD-142 [--repo owner/name]` | Build it: a builder plans and builds, a checker checks, you approve the plan and the PR (docs/build.md). |
| `wh build`, `wh build discard D-12`, `pause`, `resume` | Builds at a glance; stop one (its branch stays). |
| `wh recap [today\|week\|30d]` | What happened, as a Markdown note for a standup. |
| `wh review` | The Review protocol: every PR it follows, and what each needs. |
| `wh review on`, `off`, `pause`, `resume`, `check` | Turn it on (importing rex-reviewer's ledger), stop new rounds, check GitHub now. |
| `wh review post-mode ask\|auto\|config` | Who posts a checked review: you, the checker, or the config's choice. |
| `wh review pickup OWNER/NAME N` | Review a PR that didn't ask you. |
| `wh review preview\|post\|dismiss OWNER/NAME N` | The round's checked review: see it, post it, or drop it. |

Reviews are in [docs/review.md](docs/review.md).

## Develop

```sh
just check   # Rust tests, clippy, and the web app's types
just dev     # the web app with hot reload on :5180, against a running hub
```

Docs, fixtures and screenshots use a made-up workspace (team `ENG`, repo
`acme/api`), so the repo can be opened later without rewriting history.
