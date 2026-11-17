# Roadmap

The goal: Wheelhouse replaces rex-inbox, rex-reviewer and rex-develop for
everyday development and reviewing. Each step below retires something you
run today. `[x]` is built and checked; `[ ]` is next.

## Replace rex-inbox (milestone 1)

- [x] Every agent tracked as a docket, with Claude Code's hooks
- [x] The app: Inbox, lists, board, docket pages, ⌘K, Cloudflare theme
- [x] The pane from any device; messages to an agent between turns
- [x] Guarded approve and deny for Claude Code's permission prompts
- [x] The hub as a LaunchAgent: `wh up`, `down`, `logs`, `status`
- [x] The app on your tailnet, answering only you (`wh web --publish`)
- [x] Notifications on the Mac: what needs you, failures, messages
      (quiet for prompts and failures while rex-inbox runs)
- [x] Start an agent from the app or `wh start`, on a docket
- [x] Messages: `wh send --to me|PANE`, `reply`, `inbox`, `read`,
      `wait`, `name`, `whoami`, options as buttons, delivered between turns,
      each on the record (questions, decisions, notes on the thread)
- [x] The `wheelhouse` agent skill (`wh skill install`)
- [x] The global agent instructions point at Wheelhouse (Claude Code and Codex)
- [x] Web Push on the phone (Home Screen app, Turn on in the sidebar)
- [x] Previews: `wh share PORT`, answering only you (each request checked)
- [ ] Other Rex hosts
- [ ] Codex's prompt keys
- [ ] `wh import`: history from rex-inbox, rex-reviewer, rex-develop
- [x] A bridge to rex-reviewer until Review lands: one Review docket per
      PR with all its panes, following its round, verdict and merge risk;
      "ready to post" reviews in the Inbox with Post, Preview and Dismiss
- [x] Switch over (2026-11-17): rex-inbox down, its hook and skill links removed

## Dockets as the living source of truth

- [x] Intent on every docket: a versioned Markdown spec at the top of the
      page (what it's for, acceptance, out of scope), edited in place,
      with its history on the record; agents read it with `wh brief`
- [x] Linear: `wh setup-linear`; a docket from a Linear issue
      (`wh new --linear ENG-142`, or New docket › From Linear); link an
      issue to any docket; the issue's description seeds the intent, and
      changes to it, its state and its comments come in as entries
      (built and checked against a stand-in for Linear's API; needs your key)
- [ ] Decisions in force under the intent, with who made each one

## Replace rex-reviewer (milestone 2)

- [x] GitHub source: PRs that request you, rounds, heads (one GraphQL check, as rex-reviewer did)
- [x] The Review protocol: prep, blind reviewers, checker, merge risk, head moves, cleanup;
      your rex-reviewer config read as it is (LuaJIT); its ledger imported
- [x] The post and its refusals (post-review, ported and tested); effort; guides; briefs
- [x] The Reviews view, a review panel on each Review docket, ready reviews in the Inbox
- [x] Recap: today, this week or 30 days; reviews and their verdicts, PRs
      merged, waiting or closed, agents' work, your decisions; Copy as
      Markdown, and `wh recap`
- [x] Switch over (2026-11-17): rex-reviewer down, `wh review on`, ask mode
      for the first live rounds
- [x] Review is Wheelhouse's own: its config and guides in
      `~/.config/wheelhouse` (`wh import rex-reviewer` once), the bridge to
      rex-reviewer gone; the first live round (guardian#742) ran end to end
- [ ] Next for Review: drafts and status on the docket's record rather
      than in `.tmp/`, once a few more live rounds have run

## Replace rex-develop (milestone 3)

- [ ] Linear (above) and GitHub Issues as sources for Build
- [ ] The Build protocol: readiness, plan, decisions, build, check,
      preview, open, address rounds
- [ ] Switch over: rex-develop down
