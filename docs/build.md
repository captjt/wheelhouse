# Build

Build turns a docket into a pull request. The docket's **intent** is the
spec: what must be true when it's done, and what's out of scope. A Linear or
GitHub issue seeds it, or you write it yourself on any docket. Two agents
work it in Rex: a **builder** plans and builds, and a **checker** checks the
work against the plan. You approve the plan, answer the decisions, and
approve the PR before it opens. The PR opens from your GitHub account.

Build is Wheelhouse's own. It keeps the contracts that rex-develop, an
earlier issue-to-PR tool on Rex by the same author, proved in its tests:
the decision format, the plan format, the check output, the opening
refusals, and the agents' wall. You don't need rex-develop. Wheelhouse puts
everything on the docket's record.

## Set up

With the hub running (`wh up`; [README.md](../README.md) has the steps):

1. Have a review config, or a build config, that lists your repos. Both
   are Lua, read with LuaJIT (`brew install luajit`). Build uses the
   repos in `~/.config/wheelhouse/review.lua`
   ([examples/review.lua](../examples/review.lua)). To change something for
   Build, or to build in a repo you don't review, copy
   [examples/build.lua](../examples/build.lua) to
   `~/.config/wheelhouse/build.lua` and edit it.
2. Sign in to GitHub with `gh auth login`. The hub opens PRs with your
   `gh` login.
3. For Linear issues, run `wh setup-linear` once to store a Linear API key.

Build is on from the start (`wh build pause` stops new Builds). `wh build`
says what's wrong with the config, if anything, and `wh build --json`
shows what it read.

## Commands

| | |
| --- | --- |
| `wh build start D-12` | Build a docket from its intent. |
| `wh build start acme/api#45` | Build a GitHub issue (its URL works too). |
| `wh build start ENG-142` | Build a Linear issue (needs `wh setup-linear`). |
| `--repo acme/api` | Which repo, when Build has more than one. |
| `wh build` | Every Build, its phase, and what needs you. |
| `wh build discard D-12` | Stop a Build. Its branch stays. |
| `wh build pause`, `resume` | Stop or restart new Builds. |

In the app, **Build it** on a docket does the same as `wh build start`.

## How it goes

| Phase | What happens | You |
| --- | --- | --- |
| Start | `wh build start` with a docket, a GitHub issue, or a Linear issue; or an issue assigned to you with the label (`sources.github`, `sources.linear`); or one in your Linear cycle, offered | **Build it / Not this one**, for an offer |
| Prep | A worktree on the docket's branch from `origin/<base>`, the intent and issue in `.tmp/context/` | |
| Plan | The builder reads, then writes a plan, or says it isn't ready | **Proceed / Drop** when it isn't ready |
| Decisions | Each open question is a message with its options as buttons | Pick one, or answer in your own words |
| Plan gate | The plan: Intent, Acceptance, Decisions, Approach, Out of scope, Notes | **Approve / Revise / Discard** |
| Build | The builder implements, tests, commits, drafts the PR | |
| Check | The checker checks the branch against the plan; up to two fix rounds | |
| Verify | The hub runs the repo's `checks` itself at that commit ([verify.md](verify.md)); a required one that fails goes back to the builder | |
| Pre-review | With `prereview` set: reviewers review the branch blind, as they would its PR, posting nothing; blocking findings go back to the builder | |
| Preview | When there's something to see, `wh share` puts it on your tailnet | **Looks right / Needs work** |
| Open gate | The PR as it will open: title, body, commits, the check, and how the witnessed checks went (a PR doesn't open over a failing one unless you say so) | **Open PR / Needs changes / Discard** |
| Open | Wheelhouse pushes the checked head and opens a draft PR with the plan | |
| Follow | Each poll reads the PR: review threads, review bodies and comments that need an answer, failing checks, and whether the base moved on | **Address it / Not now** |
| Round | The builder reopens on what's new: fixes, merges the base in, drafts a reply to each; the checker checks the fixes and the replies | |
| Push gate | The round's commits, and every reply as it will post | **Push and reply / Needs changes / Drop the round** |
| Push | Wheelhouse pushes the round, posts the replies from your account, and asks the reviewers again | |
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

`~/.config/wheelhouse/build.lua` returns a table, read with LuaJIT like
`review.lua`. [examples/build.lua](../examples/build.lua) is a commented
copy to start from. Without the file, Build uses `review.lua`'s repos,
agents, PATH and caches, with the defaults below. `WH_BUILD_CONFIG` points
the hub at another file. Settings, in the apps, changes what isn't code
(gates, agents, notes, checks) in `settings.json`, laid over this file
([settings.md](settings.md)).

| Key | What | Default |
| --- | --- | --- |
| `repos["owner/name"]` | `checkout`, `notes`, `base`, `plan` (`"full"`, `"summary"`, `false`), and `checks`: witnessed checks the hub runs itself ([verify.md](verify.md)). A repo in `review.lua` is already listed; `checkout` is required only for one that isn't. | `review.lua`'s repos |
| `sources.github` | `{ label = "agent" }`: open issues assigned to you, in Build's repos, with the label start a Build | off |
| `sources.linear` | `{ teams, label, cycle, skip_label }`: Linear issues assigned to you start a Build or are offered one (below) | off |
| `builder`, `checker` | `"claude"`, `"codex"`, `"agy"`, or `{ agent, extra_args, effort }` | `"claude"`, `"claude"` |
| `gates` | `plan`, `preview`, `open`, `address`, `push`: `"ask"` or `"auto"`; `start = { label, cycle }` for Linear intake | `"ask"`; `start`: label `"auto"`, cycle `"ask"` |
| `update` | how a round brings the branch up to date with its base: `"merge"`, or `"rebase"` (pushed with a lease) | `"merge"` |
| `prereview` | `{ reviewers = { "codex" }, loops = 1 }`: reviewers who review the branch blind before the PR opens; `loops` is how many times they may send it back | off |
| `fix_loops` | fix rounds after the checker finds something (each address round has its own) | 2 |
| `max_decisions` | decisions a plan may leave open at once | 3 |
| `max_tasks` | Builds working at once (one waiting on a gate doesn't count) | 2 |
| `poll_seconds` | how often to check GitHub for labeled issues and open PRs | 180 |
| `pr` | `{ draft = true, reviewers = { "dana" } }` | a draft, no reviewers |
| `branch_prefix` | for dockets and GitHub issues (a Linear issue uses Linear's branch name) | `"wh/"` |
| `guides` | `plan`, `build`, `check` (added to the briefs), `pr_style`, `plan_format` (replace the bundled ones): instructions or a file | |
| `effort` | `{ build = …, check = … }`: a level, or `function(task)` with `key`, `title`, `repo`, `source` | the agent's own |
| `agents`, `path`, `caches` | as in `review.lua` | `review.lua`'s |
| `wall` | `{ mcp = true }` turns MCP servers back on for builders and checkers | off |

### Linear intake

`sources.linear` takes in Linear issues assigned to you, in the teams it
maps to repos:

```lua
sources = {
  linear = {
    teams = { ENG = "acme/api" },  -- each team, and the repo it builds into
    label = "agent",               -- an issue with it starts a Build
    cycle = true,                  -- an issue in your active cycle is offered one
    skip_label = "no-agent",       -- an issue with it is left alone
  },
},
gates = { start = { label = "auto", cycle = "ask" } },
```

With the label, an issue starts a Build on Linear's branch name, with its
description as the intent and "Closes ENG-142" on the PR
(`gates.start.label = "ask"` offers it instead). In your active cycle, it's
offered: a docket made from the issue, with **Build it / Not this one** on
it (`gates.start.cycle = "auto"` starts it). Each issue is handled once,
remembered across restarts; one you pass on isn't offered again. It needs
a Linear key (`wh setup-linear`), and polls with GitHub's intake
(`poll_seconds`).

A docket linked to a Linear issue builds as that issue, whether intake
started it or you did with **Build it**.

## After it opens

Build follows the PR until it merges or closes. Each poll reads it the way
the `pr-reply` contract does, so what counts as open is what a person
answering by hand would answer:

- review threads not resolved, whose last comment isn't yours;
- review bodies, and conversation comments from people (not bots), that no
  later comment of yours links to;
- checks that failed on the PR's head, once every check has finished;
- the base, when the PR is behind it or conflicts with it.

Something new is an ask, with each thing in a line. **Address it** starts a
round; **Not now** sets these aside, and anything newer asks again. Words
with Address it go to the builder too. With `gates.address = "auto"`, a
round starts without asking.

A round reopens the builder in its own session, so it remembers the work.
Its brief (`briefs/build/address.md`) is the contract: fix what's valid
with a test, leave what isn't and say why, find each failing check's cause
in its log (Wheelhouse fetches the failing part of each GitHub Actions
job's log), bring the branch up to date with the base, commit, and write
`.tmp/address/round-N/replies.json`: one reply per thread, one answer per
review body or comment, and the reviewers to ask again. Wheelhouse
refuses a draft that skips anything open, speaks in the first person,
mentions `.tmp/`, or names a commit that isn't on the branch, and says why.

The checker checks the round's commits against what reviewers and CI said,
and that each reply is true to the change it names. Then the push gate
shows the commits and every reply exactly as it will post, footer and all.
**Push and reply** pushes (fast-forward), posts the thread replies, one
comment answering the reviews and comments (each under a link to what it
answers), and asks the reviewers for another review, from your account.
What posts is what you saw: if the replies change after you're asked, or
the branch moves, nothing goes out. **Drop the round** throws its commits
away. Threads are never resolved: the reviewers do that.

`update = "rebase"` rebases instead of merging the base in, and pushes the
rebased branch with a lease: only over the head the round started from,
never over anyone's newer commits.

Each person's review, comment and review-thread comment on the PR also
comes home to the docket, once, as a checkable entry (it's on GitHub for
anyone to read again), so the docket reads as the whole story and the next
round's agents see it in their brief.

## Pre-review

With `prereview = { reviewers = { "codex" } }`, each reviewer reviews the
branch blind once the checker passes it, before the open gate: the intent,
the plan, the PR's draft, and the whole diff, held to what counts as
blocking in a review (`briefs/build/prereview.md`). Each writes its own
findings, with evidence, and reads no one else's. Nothing is posted.

Blocking findings send the branch back to the builder, then through the
checker, and the reviewers look again, in their own sessions, up to `loops`
times (1). What's left, blocking or not, is listed at the open gate, so
you decide with it in front of you. Each look's findings are kept on the
docket.

Pre-reviewers run behind the same wall as the builder and the checker. Each
costs a review's worth of tokens per look, so it's off until you name them.

 [roadmap.md](roadmap.md) has the order.

## Trying it

`scripts/build-e2e.sh` (`just e2e`) runs a Build end to end on a scratch
hub, with a made-up Rex and gh and a local origin: the plan, the check,
opening the PR, a round on review feedback and a failing check (a reply in
the first person is refused first), a round on a base that moved, and the
merge; then Linear intake against a stand-in Linear. It plays the agents
and you, and checks what would reach GitHub.
