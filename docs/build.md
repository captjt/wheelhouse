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
| Start | `wh build start` with a docket, a GitHub issue, or a Linear issue; or a GitHub issue assigned to you with the label in `sources.github.label` | |
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

`~/.config/wheelhouse/build.lua` returns a table, read with LuaJIT like
`review.lua`. [examples/build.lua](../examples/build.lua) is a commented
copy to start from. Without the file, Build uses `review.lua`'s repos,
agents, PATH and caches, with the defaults below. `WH_BUILD_CONFIG` points
the hub at another file.

| Key | What | Default |
| --- | --- | --- |
| `repos["owner/name"]` | `checkout`, `notes`, `base`, `plan` (`"full"`, `"summary"`, `false`). A repo in `review.lua` is already listed; `checkout` is required only for one that isn't. | `review.lua`'s repos |
| `sources.github` | `{ label = "agent" }`: open issues assigned to you, in Build's repos, with the label start a Build | off |
| `builder`, `checker` | `"claude"`, `"codex"`, or `{ agent, extra_args, effort }` | `"claude"`, `"claude"` |
| `gates` | `plan`, `preview`, `open`: `"ask"` or `"auto"` | `"ask"` |
| `fix_loops` | fix rounds after the checker finds something | 2 |
| `max_decisions` | decisions a plan may leave open at once | 3 |
| `max_tasks` | Builds working at once (one waiting on a gate doesn't count) | 2 |
| `poll_seconds` | how often to check GitHub for labeled issues and open PRs | 180 |
| `pr` | `{ draft = true, reviewers = { "dana" } }` | a draft, no reviewers |
| `branch_prefix` | for dockets and GitHub issues (a Linear issue uses Linear's branch name) | `"wh/"` |
| `guides` | `plan`, `build`, `check` (added to the briefs), `pr_style`, `plan_format` (replace the bundled ones): instructions or a file | |
| `effort` | `{ build = …, check = … }`: a level, or `function(task)` with `key`, `title`, `repo`, `source` | the agent's own |
| `agents`, `path`, `caches` | as in `review.lua` | `review.lua`'s |
| `wall` | `{ mcp = true }` turns MCP servers back on for builders and checkers | off |

Planned, not read yet:

- `sources.linear`: `{ teams = { ENG = "acme/api" }, label = "agent",
  cycle = true, skip_label = "no-agent" }`. Linear issues with the label,
  and issues that enter your active cycle, would start or offer a Build.
  Today a Linear issue starts a Build only with `wh build start ENG-142`.
- `gates.start = { label = …, cycle = … }`: whether those Linear starts ask
  you first.

## Next

After the PR opens, Build follows it until it merges or closes. Rounds that
answer review comments (one reply per thread), fix CI, and rebase are next,
with pre-review through the Review protocol before opening, and Linear
intake. [roadmap.md](roadmap.md) has the order.
