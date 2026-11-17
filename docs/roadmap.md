# Roadmap

The goal: Wheelhouse replaces rex-inbox, rex-reviewer and rex-develop for
everyday development and reviewing. Each step below retires something you
run today. `[x]` is built and checked; `[ ]` is next.

## Replace rex-inbox (milestone 1)

- [x] Every agent tracked as a docket, with Claude Code's hooks
- [x] The app: Inbox, lists, board, docket pages, ⌘K, Cloudflare theme
- [x] The pane from any device; messages to an agent between turns
- [x] Guarded approve and deny for Claude Code's permission prompts
- [ ] The hub as a LaunchAgent: `wh up`, `down`, `logs`, `status`
- [ ] The app on your tailnet, answering only you (`wh web`)
- [ ] Notifications on the Mac: what needs you, failures, messages
- [ ] Start an agent from the app or `wh start`, on a docket
- [ ] Messages: `wh send --to me|PANE`, `reply`, `inbox`, `read`,
      `wait`, `name`, options as buttons, delivered between turns
- [ ] The `wheelhouse` agent skill, and the global agent instructions
- [ ] Web Push on the phone
- [ ] Previews: `wh share PORT`
- [ ] Other Rex hosts
- [ ] Codex's prompt keys
- [ ] `wh import`: history from rex-inbox, rex-reviewer, rex-develop
- [ ] Switch over: rex-inbox down, its hook and skill removed

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

- [ ] GitHub source: PRs that request you, rounds, heads
- [ ] The Review protocol: prep, reviewers, checker, post gate
- [ ] The post action and its refusals; effort; guides; briefs
- [ ] The Review tab and Recap
- [ ] Switch over: rex-reviewer down

## Replace rex-develop (milestone 3)

- [ ] Linear (above) and GitHub Issues as sources for Build
- [ ] The Build protocol: readiness, plan, decisions, build, check,
      preview, open, address rounds
- [ ] Switch over: rex-develop down
