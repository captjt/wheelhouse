# The Review protocol

Wheelhouse does what rex-reviewer did, natively. A PR that requests you by
name, in a repo your review config lists, becomes one **Review docket** for
its whole life. Each round:

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
   Either way the post passes `post-review`'s refusals (below).
5. **Later rounds.** A re-request starts the next round: the same sessions
   resume with what changed (`delta.diff`). A push mid-round sends the
   reviewers the new commits in the same round, or, if the PR's own diff
   didn't change (the base was merged in), only re-runs the checker.
6. **End.** Merge or close: tokens counted one last time, panes closed,
   worktrees and refs removed.

Everything that happens goes on the docket's record. The PR's working
state (rex-reviewer's ledger format, field for field) is in the
`review_prs` table.

## Config

Your rex-reviewer config, read as it is: `~/.config/wheelhouse/review.lua`
if you make one, else `~/.config/rex-reviewer/config.lua`. LuaJIT reads it,
so effort functions and guides work as they did; Wheelhouse calls them per
PR with the same view (`lines`, `files`, `touches`, `round`, `delta_lines`,
`last_merge_risk`, ...). Briefs are rex-reviewer's, copied as text into
`briefs/review/`; a file of the same name in
`~/.config/wheelhouse/briefs/review/` replaces one.

`wh review post-mode ask|auto|config` overrides the config's `post`.

## The post's refusals

Ported from `bin/post-review`. Nothing posts when:

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

```sh
rex-reviewer down        # stop its watcher (pausing isn't enough: it still cleans up)
wh review on             # import its ledger; starts paused
wh review post-mode ask  # optional: you post the first rounds yourself
wh review resume
```

`wh review on` refuses while rex-reviewer's watcher runs. The import keeps
each PR's rounds, heads, sessions and worktrees, so a re-request resumes
where rex-reviewer left off.

## What's different from rex-reviewer

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
