# Build

Build turns a docket into a pull request. The docket's **intent** is the
spec: what must be true when it's done, and what's out of scope. A Linear or
GitHub issue seeds it, or you write it yourself on any docket. Two agents
work it in Rex: a **builder** plans and builds, and a **checker** checks the
work against the plan. You approve the plan, answer the decisions, and
approve the PR before it opens. The PR opens from your GitHub account.

Build is Wheelhouse's own. It keeps the contracts rex-develop proved in its
tests (the decision format, the plan format, the check output, the opening
refusals, the agents' wall) and puts everything on the docket's record.

## How it goes

| Phase | What happens | You |
| --- | --- | --- |
| Start | `wh build start D-12` (a docket's intent), `wh build start ENG-142` or `owner/name#45` (an issue), or an issue assigned to you with the `agent` label | |
| Prep | A worktree on the docket's branch from `origin/<base>`, the intent and issue in `.tmp/context/` | |
| Plan | The builder reads, then writes a plan, or says it isn't ready | **Proceed / Drop** when it isn't ready |
| Decisions | Each open question is a message with its options as buttons | Pick one, or answer in your own words |
| Plan gate | The plan: Intent, Acceptance, Decisions, Approach, Out of scope, Notes | **Approve / Revise / Discard** |
| Build | The builder implements, tests, commits, drafts the PR | |
| Check | The checker checks the branch against the plan; up to two fix rounds | |
| Preview | When there's something to see, `wh share` puts it on your tailnet | **Looks right / Needs work** |
| Open gate | The PR as it will open: title, body, commits, the check | **Open PR / Needs changes / Discard** |
| Open | Wheelhouse pushes the checked head and opens a draft PR with the plan | |
| End | Merged or closed: the worktree goes (the branch too, on merge) | |

Approving the plan **is** approving the intent: its Intent, Acceptance and
Out of scope become the docket's intent, a new version on the record signed
as you. The PR's plan block is rendered from the intent in force and the
decisions as they stand, with who decided each.

Every ask is a Wheelhouse message to you with buttons: in the Inbox, on the
docket, as a notification and on your phone. Answers can be a button, a
number (`2`), a button and a note (`2, but cap it at 3`), or your own words
(Revise, Needs work and Needs changes take words). Each answer is on the
record as a decision or an approval. An answer to an ask that's moved on
changes nothing.

## The wall

Builders and checkers can't reach outward. In their panes:

- `gh` is signed out (`GH_TOKEN`, `GITHUB_TOKEN` unset, an empty
  `GH_CONFIG_DIR`); no Linear key; no SSH agent, `GIT_SSH_COMMAND=false`.
- Every push URL is rewritten to one that can't work, credential helpers
  are cleared, and `.tmp/` is ignored by git.
- Claude Code is denied `git push`, `gh`, and `wh send|share|reply`, and runs
  with no MCP servers (`--strict-mcp-config`), so a tracker or GitHub
  connector can't route around the wall.

Only the hub writes outward, with your sign-ins, and only after your
approval. Opening refuses when:

- the branch is the base;
- HEAD isn't the head the checker checked and you approved;
- any commit carries `.tmp/` files;
- `.tmp/pr.json` has no title or body, or the body speaks in the first
  person;
- origin's branch has commits this one doesn't (never a force-push).

An open PR on the branch is reused, never duplicated.

## Agents

Each agent reports by writing one line to its status file, which
Wheelhouse reads when the agent stops:

- builder, `.tmp/status.txt`: `PLAN_READY`, `NOT_READY: <why>`,
  `DECISION D<n>`, `BUILT`, `BLOCKED: <what you need>`
- checker, `.tmp/checker-status.txt`: `CHECKED`, `BLOCKED: <why>`

An agent that stops for ten minutes without writing one is flagged as
stalled. Its pane closing is flagged too. Agents get your shell's PATH. The
briefs are in `briefs/build/`; a file of the same name in
`~/.config/wheelhouse/briefs/build/` replaces one.

## Config

`~/.config/wheelhouse/build.lua` returns a table. Repos not listed in it come
from `review.lua` (the same checkouts and notes).

| Key | What | Default |
| --- | --- | --- |
| `repos["owner/name"]` | `checkout`, `notes`, `base`, `plan` (`"full"`, `"summary"`, `false`) | from `review.lua` |
| `sources.github` | `{ label = "agent" }`: issues assigned to you with the label start | off |
| `sources.linear` | `{ teams = { GRD = "owner/name" }, label = "agent", cycle = true, skip_label = "no-agent" }` | off |
| `builder`, `checker` | `"claude"`, `"codex"`, or `{ agent, extra_args, effort }` | `"claude"`, `"claude"` |
| `gates` | `start = { label, cycle }`, `plan`, `preview`, `open`: `"ask"` or `"auto"` | label auto, cycle ask, the rest ask |
| `fix_loops`, `max_decisions`, `max_tasks` | fix rounds, decisions open at once, Builds at once | 2, 3, 2 |
| `pr` | `{ draft = true, reviewers = {} }` | |
| `branch_prefix` | for dockets and GitHub issues (Linear names its own) | `"wh/"` |
| `guides` | `plan`, `build`, `check`, `pr_style`, `plan_format` | |
| `effort` | `{ build = …, check = … }`: a level, or `function(task)` | the agent's own |
| `agents`, `path`, `caches` | as in `review.lua` | `review.lua`'s |
| `wall` | `{ mcp = false }`: turn MCP servers back on for builders | |

## Later

Address rounds (review comments, CI, rebase) after the PR opens, built on
the `pr-reply` contract; pre-review with the Review protocol before opening;
Linear cycle offers once a Linear key is set up (`wh setup-linear`).
