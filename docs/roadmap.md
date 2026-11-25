# Roadmap

Wheelhouse is one hub and one app for agent work in Rex and [Herdr](https://herdr.dev): every agent on a
docket, a signed record of what happened, and two protocols, Review and
Build. It replaces three earlier tools on Rex by the same author:
rex-inbox (follow agents and answer them), rex-reviewer (multi-agent PR
review) and rex-develop (an issue to a PR). A new user doesn't need any of
them. [DESIGN.md](../DESIGN.md) has the full design and its milestones.

`[x]` is built and tested. `[ ]` is planned.

## Built

### The hub and the record

- [x] Every Claude Code, Codex, Antigravity or Pi agent in a Rex pane or a
      Herdr terminal gets a docket. Claude Code's and Antigravity's hooks,
      and Pi's extension (`wh hooks install`), add names from your prompts,
      each turn's summary, and a new docket on `/clear` (Pi's `/new`).
- [x] A signed, hash-chained record per docket: `wh verify`, `wh export`,
      and `wh verify --file` for an export ([record.md](record.md))
- [x] Rex and Herdr as runtimes behind one interface, followed side by
      side: agents in each message, answer and start each other, and new
      agents open in the one you pick ([runtimes.md](runtimes.md))
- [x] Several agents on one docket, in either runtime: `wh join`,
      `wh leave`, `wh start --docket` beside the others, `wh send --to
      @D-12`; each agent on the record by its pane, and the docket released
      when the last one leaves ([record.md](record.md))
- [x] The hub at login, as a LaunchAgent: `wh up`, `down`, `logs`, `status`
- [x] The app on your tailnet, answering only you (`wh web --publish`)

### Inbox, messages, previews and push

- [x] The app: Inbox, Active, All, Done, Agents, a view per session (Rex's and Herdr's),
      lists and a board, docket pages, ⌘K, and light and dark themes
- [x] Guarded approve and deny for Claude Code's permission prompts: a key
      goes in only if the prompt is still the one you saw
- [x] The pane from any device, and messages to an agent between turns
- [x] Messages: `wh send --to me|PANE`, `reply`, `inbox`, `read`, `wait`,
      `name`, `whoami`; options as buttons; each one on the record
- [x] Start an agent on a new docket, from the app or with `wh start`
- [x] Previews: `wh share PORT` puts an app an agent serves on your
      tailnet, answering only you, with a question and buttons
- [x] Notifications on the Mac, and Web Push on the phone
- [x] The agents' skill (`wh skill install`) and the lines for your global
      agent instructions ([agent-instructions.md](agent-instructions.md))

### Dockets as the source of truth

- [x] An intent on every docket: a versioned Markdown spec at the top of
      the page, edited in place. Agents read it with `wh brief` and update
      it with `wh intent`.
- [x] Linear: `wh setup-linear` stores a read-only key. `wh new --linear
      ENG-142` or `wh link D-12 ENG-142` ties an issue to a docket. The
      issue seeds the intent, and its changes and comments come in as
      entries.
- [x] Artifacts: the hub keeps what the work made (each review round's
      reports, checked review and merge risk; each Build's plan and
      checks). Agents keep anything else with `wh attach FILE|URL`. The
      record pins each file by its hash.
- [x] Decisions in force under the intent, and in `wh brief`: what each
      agent asked, the answer that stands, and who gave it
- [x] The readable record: open it in the app, see who signed each entry,
      and check every kept file against its hash
- [x] The composer on a docket's Activity: a note, `@builder` (or `@all`)
      to instruct the agents on it, queued while they work; `/ask` to ask
      them, with options after `|`; `/decide Subject: answer` for a
      decision of yours, which stands under Decisions and is told to the
      agents, or `/decide #12 …` to answer a question; and `/task` to add
      an agent with that prompt (`POST /api/dockets/{id}/compose`,
      `src/compose.rs`). `/` and `@` complete as you type, in both apps.
- [x] Changes: every docket with code shows what it changed, `+N −M` on
      its page and in full on its Changes view (`D`), drawn with Pierre's
      diffs (highlighted, split or unified, a file list, folded context).
      Compare from the base, or any point on the record (a round's head, a
      checked commit), to HEAD, a later point, or what isn't committed. It
      reads a Build's or Review's worktree, or the agent's folder; once the
      worktree goes, the repo's checkout, then the patches kept with checks
      (`GET /api/dockets/{id}/diff`, `src/diffs.rs`)

### Review ([review.md](review.md))

- [x] PRs that request you, in the repos `review.lua` lists, become Review
      dockets, one per PR for its whole life
- [x] Rounds: one to four blind reviewers, a checker that merges and
      checks their findings and rates the merge risk, and later rounds on
      what changed
- [x] Posting from your account, behind the post's refusals, in `ask` or
      `auto` mode
- [x] Effort and guides as Lua policies; the Reviews view and a review
      panel on each Review docket
- [x] Each review links the Linear issues its PR addresses, and writes the
      docket's intent from them
- [x] `wh import rex-reviewer` moves an existing rex-reviewer setup over,
      once

### Build ([build.md](build.md))

- [x] A docket's intent is the spec. Start with `wh build start D-12`, a
      GitHub issue (`acme/api#45`), a Linear issue (`ENG-142`), or Build it
      in the app.
- [x] Readiness, a plan, decisions as messages with buttons, the plan
      gate, build, check with fix rounds, preview, the open gate, and a
      draft PR with the plan block
- [x] The wall: builders and checkers can't push, post, or reach a tracker
- [x] GitHub issues assigned to you with a label start on their own
      (`sources.github.label`)
- [x] The Builds view and a Build panel on each docket
- [x] After the PR opens, address rounds: review threads, review bodies and
      comments that need an answer, failing checks (with the failing part
      of their logs), and a base that moved become a round of fixes, with
      one reply per thread as the `pr-reply` contract writes them, behind a
      **Push and reply** gate. The base merges in (no force-push), or with
      `update = "rebase"` is rebased and pushed with a lease.
- [x] Linear intake (`sources.linear`): an issue assigned to you with the
      label starts a Build on Linear's branch; one in your active cycle is
      offered (**Build it / Not this one**); a skip label leaves it alone
- [x] Pre-review (`prereview`): reviewers review the branch blind before
      the PR opens, posting nothing; blocking findings go back to the
      builder, and what's left is at the open gate
- [x] `scripts/build-e2e.sh`: a Build end to end on a scratch hub, with
      stand-in agents, gh, origin and Linear; `scripts/runtimes-e2e.sh`
      and `scripts/shared-e2e.sh` for Rex and Herdr together, and several
      agents on one docket

### Verify ([verify.md](verify.md))

- [x] Witnessed checks: each repo's `checks`, run by the hub in the
      worktree behind the wall, recorded with the commit, the patch they
      cover and their output; `wh check D-12`
- [x] Checks with teeth: a failing required check goes back to the
      builder, and the PR doesn't open, nor a round push, over one unless
      you approve it (the approval records what failed)
- [x] The checker's verdict as its own entry, marked independent when its
      session and pane aren't the builder's
- [x] The rule for done: acceptance items need evidence (a passing check,
      a merged PR, a completed issue, a posted review), or your word with a
      reason
- [x] Hold (`wh hold`), and the Attention list (`wh attention`, the Inbox)
- [x] A Verify section on each docket, in both apps

### Settings ([settings.md](settings.md))

- [x] Standing orders as sentences, in both apps
- [x] Gates, agents, each repo's notes, checks, plan and review guide,
      changed in the apps and kept in `settings.json` over the Lua,
      checked before they're written
- [x] Guides and briefs: edit, compare with the built-in, reset, and
      write your own; every replaced version kept
- [x] Standing decisions: a decision made standing joins its repo's notes,
      so every later brief there, reviewers' too, carries it
- [x] Reviews come home: each person's review and comment on a Build's PR
      lands on its docket as a checkable entry
- [x] `wh doctor`: what's set up, what isn't, and the fix for each

### Recap and the phone

- [x] Recap: today, this week or 30 days. Reviews and their verdicts, PRs
      merged, waiting or closed, the agents' work, and your decisions; Copy
      as Markdown, and `wh recap`.
- [x] Recap's tokens: across all the work (agents, Builds, reviews), by
      provider and model, per day, and the dockets that took the most, from
      the agents' own Claude Code, Codex, Antigravity and Pi transcripts
- [ ] Keep tokens after the tools prune their transcripts (Claude Code's
      go after 30 days): snapshot each day's usage into the hub's store
- [ ] Cost: tokens at each model's price
- [x] The phone app, from the Home Screen: a tab bar, swipe back, rows that
      swipe, sheets, and pull to refresh
- [x] Chat: an agent's docket as a conversation, with a box to send the
      next thing (queued while it works) and its reply pushed to the phone

### Wheelhouse.app

- [x] A native Mac app (SwiftUI, `mac/`): the sidebar, lists beside their
      dockets, the inspector, Chat, the record, Reviews, Builds, Recap and
      ⌘K, in the web app's look and words
- [x] Dockets in windows of their own; kept files in Quick Look, checked
      against their hash first; a file dropped on a docket is kept with it
- [x] A docket's Changes in a window of its own (⇧⌘D, or its `+N −M`
      chip): the hub's own diff page in a web view, the app's only one
- [x] The menu bar: the wheel, orange with a count, and a panel to answer
      what needs you in place
- [x] Notifications with their answers on the banner: Approve and Deny, a
      reply to an agent, an agent's options, Post; withdrawn once answered
      anywhere. The hub posts none while the app does.
- [x] Notification sounds from peon-ping's packs, picked, heard and
      installed in Settings › Notifications, each file checked against the
      hash the registry pins
- [ ] Signed and notarized builds, so a teammate can install it without
      building it

### Teams ([teams.md](teams.md))

- [x] A workspace hub (`wh serve --workspace`): no runtime, Keychain or
      launchctl, so it runs on Linux too; it orders the member log and
      shared dockets and checks every entry before it does
- [x] The member log: a workspace made with a setup code, invites bound
      to the joiner's key by the invite's secret, keys members issue,
      roles, removals, and always an admin (`wh team create`, `invite`,
      `join`, `members`, `role`, `remove`, `leave`)
- [x] Shared dockets: `wh team share D-12` (its history goes with it) and
      `wh new --team`, numbered `ACME-n`; owners and members, any number
      of each; owners change the intent and status
- [x] Each member's hub follows the workspace, writes through an outbox
      (offline too), redacts approvals and blocked prompts at writing,
      and holds an entry that looks like a secret
- [x] Checked on every hub: a changed entry, a fork, or a key no member
      issued stops it following, says so in Attention and `wh verify`
- [x] Agents across Macs: teammates' agents in `wh brief`, `wh send --to
      @ACME-n` reaching them, held for their person until let in (or
      trusted), and marked a peer's in the pane; an agent's question to
      its own person and the owners, the first answer standing
- [x] `scripts/team-e2e.sh`: a workspace hub and two members' hubs
      through all of it
- [ ] The Team view in both apps, and the docket page's people
- [ ] The workspace hub packaged for Linux (systemd, a container) and
      `wh up --workspace` on a Mac
- [ ] A live test with other engineers on a real repo

## Next

### Build after the PR opens

- [ ] Build run live on more real repos

### Review

- [ ] Codex, Antigravity and Pi reviewers and checkers tested in live rounds.
      The config supports them, but they haven't run a live Wheelhouse
      round yet.
- [ ] Drafts and round status on the docket's record, rather than in the
      worktree's `.tmp/`

### Isolation

- [ ] Agents isolated from the hub and your credentials: Review's and
      Build's agents in Claude Code's sandbox (or a separate macOS user),
      and owner-only API calls that need a secret agents can't read. Today
      the wall stops mistakes, not a hostile agent (docs/security.md).

### Changes

- [ ] Comments on a diff's lines, on the record (Pierre's annotations), and
      sent to the agent as a note
- [ ] Unchanged context expanded in place, from the file at each side
- [ ] Comparing two dockets' work: two agents who tried the same issue

### Elsewhere

- [ ] Codex's and Antigravity's permission prompts answered from the app,
      as Claude Code's are
- [ ] Runtimes on other machines: Rex hosts (`rex -S`) and Herdr machines
      (`hc --on`)
- [ ] More runtimes behind the same interface (tmux, Zellij)
- [ ] Whether every agent session needs a docket: a one-off question
      could join a shared docket, or stay unfiled in the Agents view

## Later

- **Verify, further:** per-session signing keys, so a verdict's
  independence rests on its key
- **Your own protocols:** standing orders and protocols you write
  (milestone 5)
- **Teams, further:** handoff, shared standing decisions, Build and
  Review across the team, a team Recap, and cosigned checkpoints
  ([teams.md](teams.md); D-187's phases 2 and 3).
