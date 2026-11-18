# Verify

An agent saying "tests pass" is something it *said*. Verify is how a
docket's work comes to rest on more than that: checks the hub runs and
watches, a checker whose judgment is its own, and a rule that keeps a
docket from closing on words alone. It's milestone 4 in
[DESIGN.md](../DESIGN.md). Each docket's **Verify** section shows it, and the
Inbox's **Attention** list gathers what needs a look.

## Witnessed checks

A repo's own checks, listed with it in `build.lua` or in Settings:

```lua
repos = {
  ["acme/api"] = {
    checkout = "~/code/acme-api",
    checks = {
      test = "pnpm test",                                   -- required
      lint = { run = "pnpm lint", required = false },       -- only informs
      e2e = { run = "pnpm e2e", timeout = 1800 },           -- seconds; 1200 by default
    },
  },
},
```

The hub runs each one itself, in the docket's worktree (a Build's) or its
agent's folder (when that's inside one of the repos), with your login
shell (`$SHELL -lc`), behind the same wall as the agents: no GitHub or
Linear tokens, no SSH agent, a signed-out `gh`, and every push sent
nowhere. `WH_CHECK=1` is set, if a script wants to know.

Each check becomes a `check` entry, written by the hub, so it's
**witnessed**. It names:

- the command, and whether it's required;
- `head`, the exact commit it ran at, and `dirty`, when the folder had
  uncommitted changes to tracked files (then it covered more than the
  commit, and doesn't count as passing it);
- `patch`, the diff from the Build's base to that commit, kept by hash: the
  change the check covered;
- its exit (none when it timed out), how long it took, and its output's
  last 256 KB, kept by hash.

Run them by hand with `wh check D-12` (`--wait` prints the results and
exits non-zero when a required one fails), or **Run checks** on the
docket.

### With teeth

In a Build, the checks run after the checker passes the branch, at that
commit, before pre-review and the gates. Then:

- A required check that fails goes back to the builder with the end of its
  output, as a fix round (within `fix_loops`).
- The PR doesn't open, and an address round doesn't push, unless the
  required checks passed at exactly the commit going out. The open and push
  gates say how they went. **Open PR** or **Push and reply** over failing
  checks goes ahead on your word, and the approval entry records what
  failed (`over_checks`).
- An `auto` open gate waits for passing checks; it never opens over a
  failure on its own.

## The checker's verdict

A Build's checker writes its verdict as its own `verdict` entry, signed
for its session, so the record shows the judgment as *said* by the
checker, not witnessed by the hub. The entry says whether it's
**independent**: from a session and pane other than the builder's. Today
every agent of one app shares a signing key, so independence rests on the
session; per-session keys come with isolation (docs/security.md).

## The rule for done

A docket whose intent has acceptance items (task boxes) closes only on
evidence:

- **witnessed:** its required checks all passed at one commit, or its PR
  merged;
- **checkable:** its Linear issue is completed, or its review is on GitHub.

Without any, setting it done is refused, and says why. You can still close
it on your word, with a reason: `wh set D-12 --status done --anyway
--reason "Covered by the wiki page"`, or **Close on your word** in the app.
The record keeps the reason, and the docket shows it was closed on your
word. Dockets without acceptance items close as before.

## Hold

`wh hold D-12 "Waiting on the DBA"` stops Build and Review from taking
any step on the docket, polls included, until `wh hold D-12 --off`. The
reason is on the record and on the docket.

## Attention

What needs a look beyond your asks, worst first (`wh attention`, and the
Inbox):

| Kind | When |
| --- | --- |
| Build, Review | A protocol stopped and needs you: an error, a stall, a refusal |
| Failing | A required check fails on an open docket |
| Held | On hold, with the reason |
| Long | An agent has worked over an hour without finishing a turn |
| Quiet | Nothing for a week, and no agent on it |
| Your word | Closed on your word, in the last week |

## In the API

- `GET /api/dockets/{id}` adds `evidence`: `{needs, evidence, checks,
  repo, configured, running, verdicts, hold, closed_on}`.
- `POST /api/dockets/{id}/check` runs the checks; `POST
  /api/dockets/{id}/hold` `{reason}` or `{off: true}`.
- `POST /api/dockets/{id}/set` `{status: "done"}` answers 409 without
  evidence; add `anyway: true, reason` to close on your word.
- `GET /api/state` adds `attention`.

`just e2e` covers all of it: a failing check sent back and fixed, the open
gate's account, independent verdicts, `wh check`, the evidence a merged
docket rests on, the rule for done, and hold.
