# Wheelhouse

Product design, draft, 2026-11-17. The decisions table marks what's
settled. The repo is `captjt/wheelhouse` (private for now). The product
is Wheelhouse. This is the design: what's built so far is in
[docs/roadmap.md](docs/roadmap.md), and the commands that exist are in
README.md and `wh --help`.

Wheelhouse is where you run all your agent work on your Mac. Each piece of
work is a **docket**. Agents run in Rex. Wheelhouse starts them, briefs
them, and watches them. It asks you only what you must decide, and keeps a
record you can check.

**New here?** rex-inbox, rex-reviewer and rex-develop, named throughout
this document, are earlier, separate tools on Rex by the same author:
rex-inbox followed agents and answered them, rex-reviewer ran multi-agent
PR reviews, and rex-develop took an issue to a PR. Wheelhouse replaces all
three, so you don't need any of them.

It brings together what rex-inbox, rex-reviewer and rex-develop each do.
Those three become built-in features on one record, and one app shows
them. The three tools stay as their own open-source projects for anyone
who runs one alone. Wheelhouse shares no code with them: it ports what
they proved and goes further.

This is the local, one-person product. Teams come after it works well for
one person. The record format, first written for rex-inbox, is in
`docs/record.md`.

## Who it's for

One developer running many agents (Claude Code and Codex) in Rex, across
their own Macs. Their work is in GitHub repos, with issues in Linear or
GitHub. They review other people's PRs and write their own, and they also
start agents by hand all day. They want to steer all of it from one place,
on the Mac or on their phone, without watching panes.

## Principles

1. **One record.** Every piece of work is a docket, and everything that
   happens to it is an entry. Every screen is a view of the record, and
   no feature keeps state of its own. That's what one product makes
   possible and three tools can't do.
2. **Agents do, you decide.** Agents ask only what only you can decide. A
   question comes with options and a recommendation, and the agent keeps
   working on the recommendation while it waits.
3. **Nothing goes outward without you.** Agents never push, post, or open
   a PR. Wheelhouse does those things for them after you approve, or under
   a standing order you wrote. Nothing ever merges.
4. **Every claim says how it's known.** An entry is marked *said*,
   *witnessed*, or *checkable*. A docket can't be closed on what was said.
5. **Rex is the runtime.** Panes live in Rex. Wheelhouse never draws a
   terminal: **Open** takes you to the pane.
6. **Answer from anywhere.** The window, the menu bar, a notification,
   the phone, the keyboard, or a terminal.
7. **Quiet by default.** It notifies only for what needs you, and only
   once.

## The model

| Noun | What it is |
| --- | --- |
| **Docket** | One piece of work, from the first ask to the last merge, numbered `D-41`. It carries the issue or PR key it came from (`ENG-142`, `api#341`) and has properties: title, status, priority, repo, labels, protocol. |
| **Entry** | One signed, append-only record in a docket's log. A correction is a new entry that names the one it replaces. The format is in `docs/record.md`. |
| **Protocol** | The playbook a docket follows: its steps, its roles, and its gates. v1 ships three: **Build**, **Review**, and **Agent**. |
| **Standing order** | A rule you wrote: when something happens, open a docket with a protocol. For example: "when a PR in acme/api requests me, Review it". |
| **Task** | A unit of work an agent claims: builder, checker, a reviewer. A docket with no tasks has one task with no name. |
| **Agent** | A Claude Code or Codex session in a Rex pane, holding a task. Every agent is on a docket. |
| **Ask** | Something that waits on you: a permission prompt, a decision, a gate (plan, open PR, post review, push replies), a preview, an issue to start or skip, a failure, a message. Your inbox is your open asks. |
| **Artifact** | A versioned file on a docket: a plan, a review draft, a preview, a screenshot, a diff. |
| **Verdict** | A checker's judgment of a patch: pass, pass with notes, or fail. |
| **Link** | An issue, PR, commit, or review the docket is tied to. Links are read again from their source, which makes them *checkable*. |

## Three protocols on one record

Each standalone tool becomes a protocol. Each concept it had becomes an
entry kind, so all three read the same way, and every view works for all
three.

### Build (what rex-develop does)

A standing order opens the docket. Either an issue assigned to you in
Todo has the `agent` label, or it's in your active cycle (you're asked to
**Start** or **Skip** first). You can also run `wh build start ENG-142`.

| Step | What happens | Entries |
| --- | --- | --- |
| Intake | A worktree on the issue's branch, with the issue and its comments as context | `link` (issue), `order` |
| Readiness | Can it be built as written? If not, you get **Proceed** or **Drop** | `question`, `decision` |
| Plan | Intent, acceptance items, what's out of scope, and the decisions it can't make alone. Gate: **Approve**, **Revise**, or **Discard** | `intent`, `artifact` (plan), `question` per decision, `approval` |
| Build | The builder implements, follows your answers, runs the checks, and commits | `task`, `claim`, `turn`, `check` |
| Check | A fresh checker holds the diff to the plan. Its findings go back to the builder, up to `fix_loops` times | `verdict` |
| Preview | When the change is something you can see: **Looks right** or **Needs work** | `artifact` (preview), `approval` |
| Open | Gate: **Open PR**. The draft PR goes out with the plan block and the footer | `approval`, `link` (PR) |
| Address | Each review becomes a round: fixes, one reply per thread, then the gate **Push & reply** | `observed`, `turn`, `verdict`, `approval` |
| End | When the PR merges or closes, the worktree goes | `observed` |

### Review (what rex-reviewer does)

A standing order opens the docket when a PR requests you by name. You can
also run `wh review pickup acme/api 341`.

| Step | What happens | Entries |
| --- | --- | --- |
| Prep | A worktree at the PR's head, with its description, diff, reviews, and CI results as context | `link` (PR), `order` |
| Review | One to four reviewers, each blind to the others, each in its own worktree. Effort is set by the PR. | `task`, `claim`, `artifact` (draft) per reviewer |
| Check | A checker merges the drafts, checks every finding, and assesses the merge risk | `verdict`, `artifact` (checked review) |
| Post | Gate: **Post**, **Preview**, or **Dismiss** (or `auto`) | `approval`, `observed` (posted) |
| Rounds | A re-request starts the next round, which reviews only what changed | as above |
| End | Merge or close cleans up the tab, worktrees, and refs | `observed` |

### Agent (what rex-inbox tracks)

Every agent session that no protocol started gets its own docket. These
include an agent you start from Wheelhouse, and `claude` typed into any
Rex pane.

- **A session is a docket.** `/clear` starts a new session, so it starts
  a new docket. `--resume` and compaction keep the same session, so they
  keep the same docket, even when resumed in another pane.
- **Its name is your first prompt**, cut to one line. You can edit it in
  place.
- **Its entries:** each prompt you give (`note`), each turn's summary
  (`turn`), each permission you answer (`approval`), its messages, and
  what it shares.
- **Join instead (planned).** `wh join D-41` in a pane, or **Working
  on…** in the app, attaches the session to an existing docket. Its own
  docket then folds into that one.
- **Promote.** **Make this a Build** turns a spike into planned work.
  The plan starts from the session's record.
- **It ends with the session** and moves to Done on its own. Short
  sessions (one prompt, a few minutes) collapse into one **Quick** line.

### Where the protocols meet

These things come only from having the protocols on one record:

- **Pre-review.** Build's check step can run a Review on its own branch
  before the PR opens, with your own reviewers, and nothing is posted.
- **Reviews check the plan.** A Review of a PR that carries a plan block
  checks every acceptance item and decision against the code. That holds
  whether you or a teammate wrote it.
- **Reviews come home.** People's reviews on a Build PR land on the same
  docket that planned it, and the next round starts from there.
- **Standing decisions.** A decision you make on one docket for a repo
  goes into every later brief for that repo, reviewers' briefs included.
- **One budget.** `max_agents` caps every protocol together, so a busy
  review day can't starve your builds, and builds can't starve reviews.
- **One safety model, one Recap, one inbox.**

## Experience

### Surfaces

| Surface | What it's for |
| --- | --- |
| **Wheelhouse.app** | One menu bar icon, orange with a count while something needs you. The panel shows your asks. The window holds the full app. Notifications carry the answer (Approve and Deny, Post and Dismiss, a decision's options) and are withdrawn once answered. |
| **The window** | The web UI in a native window: Home, Dockets, Agents, Recap, Settings, and ⌘K. |
| **The phone** | The same UI from the Home Screen over your tailnet, with Web Push, laid out natively: a tab bar, large titles, a docket that slides onto its list (swipe right to go back), rows that swipe (Done, Drop, Status, File, Approve) and long-press for their menu, sheets from the bottom, and pull to refresh. An agent's docket opens to its conversation: what you asked, what it answered, what it's doing now, and a box to send the next thing (queued while it works; its reply comes as a push). Previews open here. |
| **The terminal** | `wh` for you: `wh` (your asks), `wh show D-41`, `wh reply 3 --choice "5xx + timeouts"`, `wh build start ENG-142`. Rex key bindings (planned): `wh_next` focuses the next pane that needs you. |
| **Agents** | `wh` for agents: `brief`, `intent`, `note`, `attach`, `send`, `reply`, `wait`, `share`. Also a skill (docs/agent-instructions.md), and a brief at `SessionStart`. |

### Home

```
Wheelhouse                                            ⌘K     rex ●  mac ●
──────────────────────────────────────────────────────────────────────────
Needs you · 4
  D-41  ENG-142  decision  D1 · Retries for 4xx?   [5xx + timeouts] [Every non-2xx]
  D-44  api#341  review    round 2 · changes · 2 blocking · risk high  [Post] [Preview]
  D-45  ENG-150  prompt    builder · Bash: pnpm add zod           [Approve] [Deny]
  D-47           finished  "Fixed the flaky retry test. Tests pass."  [Open] [Done]

In flight · 5
  D-41  ENG-142  Retry failed webhooks       Build · checking     ●●   acme/api
  D-45  ENG-150  Validate webhook payloads   Build · building     ●    acme/api
  D-44  api#341  Add rate limiting           Review · round 2     ●●●  acme/api
  D-43  api#338  Paginate exports            Review · posted ✓
  D-47           Flaky retry test            Agent                ●    ~/git/api

Done today · 2, and 6 quick
  D-39  api#330  merged · reviewed ✕ → ✓ · 212k tokens
  D-36  ENG-131  merged · PR #329 · 2 decisions · 1.1M tokens
```

- **Needs you** is ordered by the cost of waiting. A stopped agent comes
  first (a prompt, or a blocking decision). Then gates and reviews, then
  provisional decisions and previews, then finished agents.
- **The keyboard:** `j`/`k` move, `↩` opens, `1` to `6` pick an option,
  `a`/`d` approve or deny, `p` posts, `r` replies, `o` opens in Rex, `e`
  marks done, and `s` snoozes.
- **Dots** are agents: green is working, orange is blocked, red has
  failed, and grey is idle.

### A docket

```
D-41  Retry failed webhooks                     Build · checking · acme/api
ENG-142 ↗ · eng-142-retry-failed-webhooks · no PR yet · 412k tokens · 9f2c41ab
Needs you: D1 · Retries for 4xx?   [5xx + timeouts] [Every non-2xx]

 Activity   Plan   Artifacts   Agents   Verify
  #1   10:02  link      ENG-142, assigned to you, labeled agent    checkable
  #2   10:02  order     Build, by your standing order              said
  #4   10:05  artifact  plan v1                                    said
  #5   10:06  decision  you: Revise · "cap retries at 3"           said
  #7   10:11  approval  plan v2 · D2 stood                         said
  #8   10:12  question  D1 · Retries for 4xx? · meanwhile 5xx only said
  #11  10:40  check     tests: cargo test · exit 0 at 4e1d9a0      witnessed
  #12  10:48  verdict   checker (Codex) · 1 finding → fix loop 1   said
  #14  10:55  turn      builder: "Fixed: attempts now logged."     witnessed
```

- **Activity:** the timeline, each entry with its mark. Asks are cards
  with their buttons. The composer is at the bottom: a plain message is a
  note, `@builder` instructs that agent, and `/decide`, `/ask`, and
  `/task` write those entries.
- **Plan**, for Build: the plan as approved, every version, and each
  decision with who chose it or that the recommendation stood.
  **Review**, for Review: each reviewer's draft, the checked review, and
  the merge risk.
- **Artifacts:** every version and the diff between any two. Comments go
  to the agent that made the artifact.
- **Agents:** each task, its holder, and its state, with **Open**,
  **Screen**, and **Message**.
- **Verify:** the checks, the verdicts, and whether the chain is intact.

### Dockets, Agents, Recap, Settings

- **Dockets:** a list, or a board by status, with saved views: Building,
  Reviewing, Agents, Waiting on me, Stalled, and by repo.
- **Agents:** every pane on every host, under its docket.
- **Recap:** today, this week, or the last 30 days. It covers reviews
  posted and their verdicts, PRs your agents opened and merged, decisions
  you made and recommendations that stood, tokens by protocol and agent,
  and time from request to merge. **Copy as Markdown** turns it into a
  standup note.
- **Settings:** repos, trackers, agents per role, hosts, and standing
  orders, written as sentences you can toggle:
  - *When a PR in acme/api requests me, Review it with Codex and Claude
    Code, check with Claude Code, and ask me before posting.*
  - *When an ENG issue assigned to me is labeled `agent`, Build it with
    Claude Code, check with Codex, and ask me at plan and open.*
  - *When an ENG issue enters my active cycle, ask me whether to start
    it.*

### A day with it

1. **Morning, on the phone.** There are three asks. You approve
   ENG-142's plan, answer D1 with a tap, and post the review of api#341.
   The agents carry on.
2. **A PR requests you.** D-44 opens, three agents review it in its own
   Rex tab, and you get a notification for the checked review, with
   **Post** and **Preview**.
3. **You type `claude` in a pane.** D-47 appears, named by your prompt.
   When the agent finishes, its summary is on the docket. You mark it
   done, or make it a Build.
4. **An agent hits a prompt.** You approve it from the notification. The
   guard checks it's still the same prompt first.
5. **Friday.** Recap, then **Copy as Markdown**.

## Architecture

```
 Rex: panes, program status, hosts
   │ rex CLI: events, send, capture, do
   ▼
 wheelhouse daemon (Rust, one binary, a LaunchAgent)
   ├── runtime    mirror, hosts, program status → asks, delivery,
   │              guarded approve, start agents, share previews, hooks
   ├── record     entries, chain, signatures, fold to state, verify, brief
   ├── protocols  Build, Review, Agent; standing orders; the decide loop
   ├── sources    GitHub (gh), Linear (GraphQL), polled
   ├── actions    post a review, push, open a PR, reply: the only outward writes
   ├── store      SQLite: entries (the truth), derived tables, blobs
   └── api        unix socket (CLI, agents) · HTTP + SSE (UI) · Web Push
          │
          ├── React UI (embedded): the window, the phone, a browser
          └── Wheelhouse.app (Swift): menu bar, notifications, the window
```

- **The decide loop.** Each protocol is a pure function:
  `decide(docket, observation, now) → actions`. rex-reviewer and
  rex-develop both use this pattern, with a `replay` command. Here a
  docket's state is the fold of its entries, so replay comes free:
  `wh replay D-41` (planned) runs the rules over the record.
- **The runtime is behind four verbs.** `identify`, `observe`, `deliver`,
  and `brief`, as the concept defines them. Rex is the only runtime in
  v1. Herdr could come later without touching the protocols.
- **Rex layout.** Each host has one `wheelhouse` session. A docket a
  protocol staffs gets a tab, and each task gets a pane in it, named
  `D-41 · builder`. Agents you start yourself stay where you started
  them.
- **Briefs.** One compiler turns the record into a brief for each role:
  the intent, the decisions in force (standing ones included), what's
  open, and where the last holder stopped. The protocol's role prompt and
  your guides (review style, merge-risk rubric, plan format) are added
  to it. It's delivered when the session starts and again after
  compaction.
- **Agents write back** with `wh`, over the socket. When a sandbox blocks
  the socket, `wh` writes to an outbox folder in the worktree, and the
  daemon picks it up. rex-develop's decision files already work this way.
- **Outward actions run through Rex**, as rex-reviewer's do, so they use
  your shell's git and gh sign-ins, not the LaunchAgent's environment.
- **Config.** Lua files in `~/.config/wheelhouse` (`review.lua` and
  `build.lua`; `examples/` has a commented copy of each), like the tools,
  because effort and routing are functions. Settings shows them and checks
  them. Runtime changes (pausing, a gate on one docket) are `set` entries
  on the record.

## The record format

This is settled at milestone 0, in `docs/record.md`. The approach takes
Nostr's shape without using Nostr: signed, self-contained entries, ids by
hash, and a key for each actor. Nostr's wire format, curve, and relays
are left out.

- **An entry is Wheelhouse's own JSON, signed as exact bytes.** The
  payload is the entry object from `docs/record.md`: `v`, `docket`,
  `kind`, `at`, `actor`, `body`, `data`, and `blobs`. Nothing is
  re-serialized before it's hashed or checked, so there are no
  canonical-JSON rules to get wrong.
- **Each entry is wrapped in a [DSSE](https://github.com/secure-systems-lab/dsse)
  envelope**, with `payloadType: application/vnd.wheelhouse.entry+json`.
  DSSE is the signing envelope that in-toto and Sigstore use, so any DSSE
  library can check an entry's signature. A `check` or a `verdict` can be
  exported as an in-toto attestation about a commit, with no conversion.
- **Keys are Ed25519.** There's one for you, one for each agent (issued
  by you with a `key` entry), and one for the hub as witness. Ed25519 is
  what SSH and git commit signing use. So later, an agent's commits can be
  signed with the same key that signs its entries, and verdict
  independence can rest on signatures rather than on git author names.
  `ed25519-compact` and `sha2` are already in rex-inbox's lockfile.
- **The hub orders entries.** It gives each entry a `seq` in its docket
  and `chain = sha256(prev chain ‖ sha256(envelope))`. Agents and people
  sign before ordering, so the chain sits outside what they sign. The
  newest chain value is the docket's head, and its first eight hex
  characters go in the PR footer. Once that footer is on GitHub, nothing
  before it can change without showing. GitHub is the first witness, at
  no cost.
- **A correction is a new entry**, and so is a countersign. Envelopes are
  never re-signed.
- **Blobs** are stored on disk by sha256.

Later additions that need no change to entries:
- **A team (milestone 6):** a Merkle log over every entry in the
  workspace, with checkpoints in C2SP's
  [signed-note](https://github.com/C2SP/C2SP/blob/main/tlog-checkpoint.md)
  format. Teammates' hubs cosign them as witnesses, so the workspace hub
  can't show two people different histories.
- **Verify (milestone 4):** [Agent Trace](https://agent-trace.dev/)
  records for which agent wrote which lines, which verdict independence
  needs.
- **Buzz, if a team uses it:** a bridge that posts dockets to a Buzz relay
  as the hub's own Nostr identity, linking to the verifiable export.

**Why not Nostr.** Most of its value is in relays and clients, and
Wheelhouse uses neither. No Nostr client would render Wheelhouse's two
dozen entry kinds; Buzz's own custom kinds don't render in standard
clients either. Nostr events have no order, and Buzz adds its own
hash-chained audit log on the relay side, so Wheelhouse would need its own
chain anyway. A Nostr id is computed over a specific JSON serialization,
while Wheelhouse signs exact bytes. Nostr uses secp256k1 Schnorr
signatures, which git, SSH, and the Keychain don't use, so Buzz had to
write `git-sign-nostr`. Its agent identity isn't standard Nostr either:
Buzz's owner attestation and agent tokens are its own NIP documents. What
Wheelhouse gives up is signatures that check natively inside Buzz. A
bridge re-signs as the hub, which is fine until a team actually works in
Buzz.

## Safety

Agents now write code and push branches, so the wall has to be harder
than any one tool's:

- **Agents can't reach outward.** `gh` is signed out in their panes.
  There's no tracker key and no SSH agent. Push URLs are rewritten to one
  that can't work, and the credential helpers are cleared. Claude Code's
  deny rules block `git push` and `gh`. (This is rex-develop's wall.)
- **Only actions write outward**, only after an `approval` entry or a
  standing order's `auto` gate. They refuse:
  - a branch that isn't the docket's own;
  - the base or default branch;
  - a force-push outside an approved rebase;
  - a remote branch or PR head that moved since the round started;
  - a body without the footer or the plan block;
  - a review in the first person, or one whose verdict doesn't match its
    findings.
- **Never:** merge, resolve threads, close PRs or issues, or write to a
  tracker (in v1).
- **Guarded approve.** A prompt is answered only if it's still the same
  prompt you saw.
- **Text from issues, PRs and comments is data**, never instructions.
- **The UI answers only you**, over loopback or as your own Tailscale
  user.

## What carries over

Wheelhouse ports what the tools proved instead of starting over:

| From | What | How |
| --- | --- | --- |
| rex-inbox | The mirror, hosts, status into asks, messages and delivery, guarded approve, starting agents, previews, the `Stop` hook, Web Push, Tailscale auth | Rust modules ported and adapted. The Swift notifier and socket client come too. The phone UI is redone in React. |
| rex-reviewer | Round rules, several blind reviewers, the checker's merge, effort functions, briefs, guides (review style, merge risk), the posting script's refusals, token counting, Recap | Rules ported to Rust and tested against its replay fixtures. Briefs and guides copied as text. |
| rex-develop | Intake (label and cycle), the Linear and GitHub Issues adapters, readiness, the plan and decision formats, gates, the plan block on the PR, the safety wall, review, CI and rebase rounds | The same way |
| `docs/dockets.md` | The record format, entry kinds, verification, the team model | Becomes `docs/record.md`, without its rex-inbox file paths |

From then on, fixes don't flow between Wheelhouse and the tools on their
own. A fix worth having in both is ported on purpose.

## Moving over from the tools

This applies only to someone who ran the earlier tools. A new user
starts with Wheelhouse alone.

Each milestone below takes over from one tool, so you switch over in
stages:

- **History comes with you.** `wh import rex-reviewer` brings
  rex-reviewer's config, guides and ledger over, so its PRs resume where
  they were. Importing rex-inbox's database and rex-develop's ledger is
  planned.
- **No double work.** Two reviewers on one PR, or two hubs pasting into
  one pane, is the failure to avoid. `wh review on` and `wh import
  rex-reviewer` refuse while rex-reviewer's watcher runs. A `wh doctor`
  that checks every replaced tool is down and its hooks are removed is
  planned.
- **Until a protocol lands,** Wheelhouse can show the remaining tool's
  status records and answer them through that tool's actions. It did this
  for rex-reviewer until Review landed; that bridge is gone now.

## Milestones

Each milestone is usable on its own and takes over from one tool.

0. **Foundations.** `docs/record.md` reviewed and settled, because the
   entry format is the costly part to change. The daemon, the store, the
   fold, `verify`, and the React shell. Fixtures use a made-up workspace
   (team `ENG`, repo `acme/api`).
   *Status: built. The record format is still a draft until it's
   reviewed.*
1. **Inbox and every agent. Replaces rex-inbox.** The runtime, the Agent
   protocol, asks, messages, guarded approve, starting agents, previews,
   and the hooks. In the UI: Home, Dockets, Agents, and a docket's
   Activity. Also Wheelhouse.app, the phone with push, `wh`, the skill,
   and `wh import`. rex-reviewer and rex-develop keep running behind the
   bridge.
   *Status: built, with messages between agents, previews (`wh share`),
   starting agents (`wh start`), Mac notifications and Web Push, and the
   phone app with Chat. Still to come: the menu bar app, other Rex hosts,
   Codex's permission prompts, and importing rex-inbox's history.*
2. **Review. Replaces rex-reviewer.** The Review protocol, the GitHub
   source, standing orders for review requests, several reviewers and a
   checker, the post action, the Review tab, and Recap for reviews.
   *Status: built (docs/review.md). `wh import rex-reviewer` moves an
   existing setup over.*
3. **Build. Replaces rex-develop.** The Build protocol, the Linear and
   GitHub Issues sources, readiness, plans and decisions, gates,
   previews, the open action, address rounds, the Plan tab, and
   pre-review through Review.
   *Status: built up to the draft PR (docs/build.md): plans, decisions,
   gates, checks, previews and opening, with GitHub issue intake by label
   and Linear issues started by hand. Still to come: address rounds
   (review comments, CI, rebase), pre-review, and Linear intake.*
4. **Verify.** Witnessed checks and the patch each one covers, verdict
   independence, checks with teeth, the rule for done, the Attention
   list, and Hold.
5. **Your own protocols.** Standing orders and protocols you write,
   boundaries, variants in worktrees, and the trace view.
6. **A team.** The workspace hub, member keys, and shared dockets, as the
   record design lays out.

Milestones 4 to 6 haven't started. [docs/roadmap.md](docs/roadmap.md)
tracks what's built and what's next.

Review comes before Build because rex-reviewer is the more settled tool
(0.5, used daily), while rex-develop (0.1) is still finding its shape.
Build also gains pre-review by landing second.

## Decisions

| Question | Decision | Status |
| --- | --- | --- |
| One product or three tools behind a hub? | One product. Inbox, review, and build are built in, on one record. | decided |
| Is rex-hub Wheelhouse? | Yes. The repo is `captjt/wheelhouse` and the product is Wheelhouse. Dockets live here. | decided |
| The standalone tools | They stay separate open-source projects, maintained on their own. Wheelhouse shares no code with them and ports what it needs. | decided |
| Which agents are tracked? | Every one. A session no protocol started is an Agent docket. | decided |
| UI | React and TypeScript, built with Vite and embedded in the binary. One UI covers the window, the phone, and a browser. A native menu bar app wraps it. | decided |
| UX bar | It looks, feels, and works like Linear: a sidebar with Inbox and views, dense lists grouped under headers, a board, a docket page with a properties rail and an activity feed, ⌘K, single-key shortcuts, menus with numbered choices, and a new-docket dialog. The UX is vital; the brand stays Wheelhouse's own. | decided |
| The pane off the Mac | Rex has no link that opens a pane in its iOS app, so off the Mac, "Open in Rex" becomes **View pane**: the agent's terminal, live, in the app, with a box to message the agent between turns. On the Mac it stays "Open in Rex". | decided |
| Theme | Colors from Cloudflare's open-source Kumo design system: neutral oklch surfaces, Cloudflare blue for actions and links, Cloudflare orange (`#f6821f`) for the brand, Kumo's success, warning and danger colors and tints. Every token is a CSS `light-dark()`: the mode follows the system by default, or you pin Light or Dark in the sidebar or ⌘K. | decided |
| Repo | Private for now, and possibly open source later. Fixtures and screenshots use a made-up workspace from the start. | decided |
| Teams | Not in v1. The record is built so teams can share it later. | decided |
| Language | A Rust daemon (rex-inbox's code ports directly), a React UI, and a Swift menu bar app. | proposed |
| Protocol rules | Ported to Rust as pure functions, tested by replaying the tools' fixtures | proposed |
| Config | A Lua file, like the tools', so effort and routing can be functions. Runtime changes are entries. | proposed |
| Record format | Wheelhouse's own JSON entries, signed as exact bytes in DSSE envelopes with Ed25519 keys, and hash-chained per docket by the hub (see "The record format") | proposed |
| Nostr | No. Take its shape (signed self-contained entries, ids by hash, a key per actor), not its wire format, curve, or relays. A bridge to Buzz can come later. | proposed |
| Order | Review before Build | proposed |
| Names | The app is Wheelhouse.app, the CLI is `wh`, and Rex actions start with `wh_` | proposed |
| Rex layout | One `wheelhouse` session per host, with a tab per staffed docket and a pane per task | proposed |
| Runtime | Rex only in v1, behind the four verbs | proposed |

## Not in v1

- Teams, shared dockets, and channels.
- Merging, resolving threads, and writing to Linear or GitHub Issues.
- Herdr, or any runtime other than Rex.
- Platforms other than macOS, and a native iOS app.
- Drawing terminals, or storing transcripts. It keeps your prompts and
  each turn's summary.

## Risks

| Risk | What keeps it in check |
| --- | --- |
| Scope: three tools' worth of features | Port, don't rewrite. Each milestone takes over from one tool and is used daily before the next starts. |
| The record format is hard to change once there's history | Settled in milestone 0, before anything writes to it |
| Two systems acting on the same PR or pane during the switch | Refusals while rex-reviewer's watcher runs, the staged moves, and `wh doctor` (planned) |
| Sandboxed agents can't reach the socket | The outbox folder |
| The LaunchAgent lacks your sign-ins | Outward actions run through Rex, in your shell's environment |
| Wheelhouse and the tools drift apart | Accepted. Fixes are ported on purpose. |

## Open questions

- **Settings as source of truth?** Keep the Lua file, or move what the
  Settings page can edit into the record, and keep Lua only for
  functions?
- **Rex layout by repo?** One `wheelhouse` session per host, or one per
  repo?
- **Quick work.** Where's the line: one prompt and under five minutes?
