# Review

A PR that requests you by name, in a repo your review config lists,
becomes one **Review docket** for its whole life. Agents review it in Rex,
a checker checks their findings, and you decide what gets posted.

## Set up

With the hub running (`wh up`; [README.md](../README.md) has the steps):

1. Install LuaJIT (`brew install luajit`). The config is a Lua file.
2. Sign in to GitHub with `gh auth login`. Reviews post from your account.
3. Copy [examples/review.lua](../examples/review.lua) to
   `~/.config/wheelhouse/review.lua`, and list your repos with the path to
   your clone of each.
4. Turn Review on. It starts paused, so nothing runs until you resume it:

   ```sh
   wh review on       # checks the config; starts paused
   wh review          # the config it read, and the PRs it sees
   wh review resume   # rounds start
   ```

Used rex-reviewer before? See [Moving over from
rex-reviewer](#moving-over-from-rex-reviewer) at the end instead.

## Commands

A PR is `OWNER/NAME N` or `OWNER/NAME#N`, such as `acme/api 341`.

| | |
| --- | --- |
| `wh review` | Every PR it follows, and what each needs (`--json` for all of it). |
| `wh review on`, `off` | Turn the protocol on (paused, the first time) or off. |
| `wh review pause`, `resume` | Stop or restart new rounds. |
| `wh review check` | Check GitHub now. |
| `wh review post-mode ask\|auto\|config` | Who posts a checked review: you, the checker, or what the config says. |
| `wh review pickup acme/api 341` | Review a PR that didn't request you. |
| `wh review preview\|post\|dismiss acme/api 341` | The round's checked review: see it, post it, or drop it. |

## How a round goes

1. **Prep.** Fetch the PR's head into `refs/wheelhouse/pr-N/round-R` (over
   HTTPS with your `gh` login, so the hub needs no SSH agent), and the base
   branch too. Put each reviewer's worktree at that head. Write the round's
   context to `.tmp/context/round-R/`: the PR, its diff, reviews, comments,
   threads and CI.
2. **Reviewers.** One to four, each blind to the others, in its own
   worktree and its own pane of Rex's `wheelhouse-reviews` session (a tab
   per PR). Effort comes from your policy. `gh` is signed out in their
   panes. Each writes `review.json` and `report.md`, then `READY_TO_POST`.
3. **Checker.** A fresh agent in the primary's worktree merges the drafts,
   checks every finding, sets `found_by`, and has a subagent rate the merge
   risk. It previews with `wh review preview OWNER/NAME N`.
4. **Post.** With `post = "ask"`, the round waits for you: **Post**,
   **Preview** or **Dismiss** in the app (Reviews, the Inbox, or the
   docket), or `wh review post|dismiss`. With `auto`, the checker posts.
   Either way the post passes the refusals (below).
5. **Later rounds.** A re-request starts the next round: the same sessions
   resume with what changed (`delta.diff`). A push mid-round sends the
   reviewers the new commits in the same round, or, if the PR's own diff
   didn't change (the base was merged in), only re-runs the checker.
6. **End.** Merge or close: tokens counted one last time, panes closed,
   worktrees and refs removed.

Everything that happens goes on the docket's record. Each PR's working
state is in the `review_prs` table.

## What the PR is for

When a review kicks off (a PR is first requested, and again at each
round's prep), the hub reads what the PR is for (`src/review/links.rs`):

- **The Linear issues it addresses**, surest first: Linear's own link on
  the PR (its linkback comment, which also carries the issue's title, link
  and description), Linear's closing words (`Closes ENG-12`), its other
  magic words (`Part of ENG-12`, `Refs ENG-12`), the branch name, and the
  title. A bare mention in the description doesn't count. A key found only
  in the branch or title needs a team Linear has shown before, so
  `fix/utf-8-names` isn't an issue. Each one is linked on the docket; the
  first is the one the hub follows in Linear (with a key from `wh
  setup-linear`, which also says how the issue stands, and drops keys
  Linear doesn't know).
- **The docket's intent**: the PR, its author, the issues it addresses,
  the author's own summary (quoted), and what the review holds it to: its
  part of each issue's acceptance criteria. The intent follows the PR while
  it's Wheelhouse's; once you edit it, it's yours.
- **`linear.md`** in the round's context, for the reviewers and the merge
  risk check: each issue, how it's linked, and its description.

PRs the hub already followed before this was built were read once, one
every few seconds, when the hub started.

## Config

`~/.config/wheelhouse/review.lua`, a Lua file that returns a table, read
with LuaJIT so policies can be functions.
[examples/review.lua](../examples/review.lua) is a commented copy to start
from. The hub reads the file again when it changes. `WH_REVIEW_CONFIG`
points the hub at another file.

| Key | What | Default |
| --- | --- | --- |
| `repos["owner/name"]` | `checkout` (required), `notes` for the briefs, `guides`, `effort` | |
| `reviewers` | one to four: `"claude"`, `"codex"`, or `{ name, agent, extra_args, guides, effort }` | `{ "codex" }` |
| `checker` | `"claude"`, `"codex"`, or `{ agent, extra_args, effort }` | `"claude"` |
| `post` | `"ask"` (you post) or `"auto"` (the checker posts) | `"ask"` |
| `max_rounds` | rounds at once (waiting on you doesn't count) | 3 |
| `poll_seconds` | how often to check GitHub (at least 30) | 180 |
| `effort` | `{ review = …, check = … }`: a level, or `function(pr)` returning one | the agent's own |
| `guides` | `review`, `style`, `merge_risk`, `check`: instructions, a file, `"bundled"`, `false`, or a function | style and merge risk bundled |
| `agents` | `claude`/`codex`: `program`, `args`, `extra_args` | |
| `path` | folders put first on the agents' PATH | |
| `caches`, `extra_caches` | folders the agents may write (package caches) | Go, npm, pnpm, uv, Playwright, Cargo |

Effort is a level: `low`, `medium`, `high`, `xhigh` or `max`, or nil for
the agent's own setting. A reviewer's own effort wins, then the repo's,
then the top-level one. An effort function gets the PR: `repo`, `pr`,
`title`, `author`, `round`, `additions`, `deletions`, `lines`, `files`,
`labels`, `changed_files`, `touches(paths)`, and on later rounds
`delta_lines`, `last_verdict`, `last_blocking`, `last_merge_risk`; the
checker's also `drafted` and `drafted_blocking`.

A guide that's a file is a path starting with `/` or `~`, and it must
exist. `guides.merge_risk = false` turns the merge risk off. The briefs are
in `briefs/review/`; a file of the same name in
`~/.config/wheelhouse/briefs/review/` replaces one.

`wh review post-mode ask|auto|config` overrides the config's `post`.

## The post's refusals

The rules are in `src/review/post.rs`. Nothing posts when:

- `event` isn't `APPROVE` or `REQUEST_CHANGES`, or doesn't match the
  blocking findings;
- there's no verdict, or a finding has no bullet or title;
- a merge-risk verdict isn't one of the rubric's words, its gaps aren't a
  list, or (with the merge-risk guide on) there's no merge risk;
- any prose speaks in the first person, outside code;
- the PR isn't open, or its head moved past the round's.

Only you, or the PR's own checker in `auto` mode, can post; only you can
dismiss. Any other agent is refused.

## Moving over from rex-reviewer

rex-reviewer is an earlier, separate review tool on Rex by the same author.
Review began as a port of it. A new user doesn't need it, and can skip this
section.

### The move

```sh
rex-reviewer down        # stop its watcher (pausing isn't enough: it still cleans up)
wh import rex-reviewer   # its config and guides into ~/.config/wheelhouse; its PRs and rounds
wh review on             # starts paused
wh review post-mode ask  # optional: you post the first rounds yourself
wh review resume
```

`wh import rex-reviewer` and `wh review on` both refuse while
rex-reviewer's watcher runs. The import writes `review.lua` from
rex-reviewer's `config.lua` (unless `review.lua` already exists) and copies
its guides. It keeps each PR's rounds, heads, sessions and worktrees, so a
re-request resumes where rex-reviewer left off. After it, nothing of
rex-reviewer's is read.

### What's changed since rex-reviewer

Review keeps rex-reviewer's proven contracts: the briefs, the agents'
`.tmp/status.txt`, the draft format, and the posting refusals. What's
changed:

- A PR reopened after cleanup, that asks you again, is reviewed again.
- A re-request is a new request: the one that started the last round never
  starts another, whatever the clocks say. And no round starts within three
  minutes of the PR's last post.
- A reviewer that goes idle for ten minutes without finishing is flagged
  as stalled, instead of holding the round silently.
- Before a round starts, panes left with its labels are closed.
- A reviewer whose worktree moved (it became the primary) starts fresh,
  since a session resumes only where it began.
- The inline-comment reader takes paths with spaces, and added lines that
  start with `++`.
- Paused or not, the protocol checks GitHub; pausing only stops new rounds.
