# Wheelhouse

<img src="docs/images/social-preview.png" alt="Wheelhouse: every piece of agent work as a docket. Talk to your agents, review PRs, build issues, and check what happened, from your Mac or your phone. Beside it, a docket: its intent with acceptance boxes, the files it kept, and its signed activity.">

Every piece of agent work as a docket, on a record you can check.

Wheelhouse follows [Rex](https://www.superlogical.com/rex) and [Herdr](https://herdr.dev), side
by side. Every Claude Code, Codex or Antigravity agent running in a Rex pane
or a Herdr terminal gets a **docket**: what you asked,
what the agent answered, when it waited on you, the files it made, and how
it ended. Each entry is signed and chained, so `wh verify` shows that
nothing in a docket's record was changed, removed or reordered.

On top of the record, Wheelhouse runs the work you'd otherwise babysit:

- **Inbox.** What needs you, from every agent: a permission prompt to
  approve, a question with your answers as buttons, a failure, a finished
  turn, a review ready to post.
- **Chat.** Talk to any agent from your phone. What you send while it
  works waits and goes in when its turn ends; its reply comes back as a
  push.
- **Review.** A PR that asks you for a review gets blind reviewers, a
  checker that merges their drafts and rates the merge risk, and a post
  you approve. The round's reports stay with the docket.
- **Build.** A docket's intent becomes a PR: a builder plans and builds, a
  checker checks, blind pre-reviewers can look before it opens, and you
  approve the plan and the PR before anything reaches GitHub. Once it's
  open, review comments, failing checks and a base that moved become
  rounds of fixes, with one reply per thread you approve before it posts.
  Linear issues with a label (or in your cycle) can start one.
- **Recap.** What happened today, this week, or in 30 days, as a standup
  note, and the tokens all of it took, by provider and model.
- **Teams.** A wheelhouse of wheelhouses: share the dockets you choose
  with your teammates, see who's on each, people and agents alike, and let
  your agents work with theirs across Macs, on one signed record. Your
  panes, credentials and approvals stay on your Mac.

<table>
  <tr>
    <td><img src="docs/images/mac-docket-light.png" alt="Wheelhouse.app: the sidebar, the Active list grouped by what needs you, and a docket beside it with its intent's acceptance boxes, the two files it kept, its signed activity, and its properties in the inspector"></td>
    <td><img src="docs/images/mac-inbox-dark.png" alt="Wheelhouse.app's Inbox in dark mode: an agent asking to run the checkout tests with Approve and Deny, a question with two answers as buttons, a failed migration, and a finished turn; beside it the first item's docket"></td>
  </tr>
</table>

<p align="center">
  <img src="docs/images/phone-chat-light.png" width="250" alt="Chat on the phone: your prompt, the agent's plan with a table, the files it kept, and the agent working, with a box to queue the next message">
  <img src="docs/images/phone-inbox-dark.png" width="250" alt="The Inbox on the phone, in dark mode, with a tab bar">
  <img src="docs/images/phone-artifact-light.png" width="250" alt="A kept file open on the phone, pinned by its entry's hash">
</p>

> Rex is in private beta. Wheelhouse needs a Rex build, or
> [herdr-core](https://herdr.dev/core/) 0.1.20 or later, whose agents report program status (OSC 7501): Claude
> Code, Codex and Antigravity do on their own. With both installed,
> Wheelhouse follows both.

## Rex and Herdr, together

Wheelhouse isn't tied to one terminal. It follows
[Rex](https://www.superlogical.com/rex) and [Herdr](https://herdr.dev)
([herdr-core](https://herdr.dev/core/), with the Herdr app and its
[phone app](https://herdr.dev/mobile/)) at the same time, through one interface, so
it doesn't matter which one an agent runs in:

- **One list of agents.** Every agent in a Rex pane or a Herdr terminal
  gets a docket, in the same Inbox, Active list, Agents view and sessions.
- **Several agents on one docket.** Any number of agents, in Rex and
  Herdr at once, can work on one docket: `wh join D-12`, or **Add an
  agent…** on its page. Each has its own place on the record, the apps
  say which one did what and which one is asking, and `wh send --to
  @D-12` reaches all of them.
- **Agents talk across runtimes.** A Claude Code in Rex can `wh send` to a
  Codex in Herdr and get its reply. A message goes in through the
  receiving pane's own runtime, between its turns. When both runtimes have
  a session of the same name, addresses say which: `herdr:wheelhouse/D-12`.
- **Talk to it while it works.** In Chat, what you send while an agent
  works is queued, and everything queued goes in together when its turn
  ends; **Send now** (⌘⇧↩) steers Claude Code or Codex mid-turn instead.
- **Answer either the same way.** Approve a prompt, queue a prompt, read
  the screen, or **Open in Rex** / **Open in Herdr**, from the Mac, the
  menu bar or your phone. The prompt is read again from its own runtime
  before a key goes in.
- **New agents open where you say.** `wh start`, Build and Review open
  their panes in Rex or Herdr: Settings → Runtimes.
- **More to come.** Each runtime is an adapter behind the same verbs
  (identify, observe, deliver, lay out), so another multiplexer is one
  more adapter: [docs/runtimes.md](docs/runtimes.md).

<table>
  <tr>
    <td><img src="docs/images/agents-light.png" alt="The Agents view: agents in Rex's acme-api and storefront sessions and in Herdr's acme-web space, side by side, each with its state and docket"></td>
    <td><img src="docs/images/docket-herdr-dark.png" alt="A docket whose agent runs in Herdr, in dark mode: its Herdr pane in the properties, and Open in Herdr"></td>
  </tr>
  <tr>
    <td><img src="docs/images/runtimes-light.png" alt="Settings, Runtimes: new agents open in Rex or Herdr, and both are followed, each with its agents"></td>
    <td><img src="docs/images/mac-settings-agents-dark.png" alt="Wheelhouse.app's Settings in dark mode: Runtimes, where new agents open and each runtime's state, above Build's and Review's agents"></td>
  </tr>
  <tr>
    <td><img src="docs/images/mac-agents-light.png" alt="Wheelhouse.app's Agents: Rex's acme-api and storefront and Herdr's acme-web, each tagged with its runtime, and a Herdr agent's docket beside them"></td>
    <td><img src="docs/images/inbox-dark.png" alt="The Inbox in dark mode: a Herdr agent asking to run the a11y tests, with Approve, Deny and Open in Herdr, above a Rex agent's prompt"></td>
  </tr>
</table>

## Wheelhouse.app

On the Mac, Wheelhouse is a native app (SwiftUI), with the web app's look
and words in the system's own controls:

- **The window.** A sidebar, each list beside its docket (as Mail does),
  and the docket's properties in an inspector. Chat with an agent, read
  the record, tick the intent's boxes, and ⌘K for everything. Double-click
  a docket for a window of its own.
- **⌥⌘K from any app.** Wheelhouse's palette floats over whatever you're
  in: approve a prompt, pick an agent's answer, post a review, find a
  docket, or start an agent, without switching apps.
- **The menu bar.** The wheel turns orange with a count when something needs
  you, and its panel answers it in place: Approve, Deny, an agent's
  options, Post.
- **Notifications you answer from the banner.** Approve a prompt, reply to
  an agent, pick one of its options, post a review. Each is withdrawn once
  it's answered, here or on your phone. They sound like the Mac, or like a
  [peon-ping](https://www.peonping.com/) pack you pick in Settings: an orc
  peon's "Work complete!" when an agent finishes.
- **The Mac's own ways.** Kept files open in Quick Look once their bytes
  match the record's hash; drop a file on a docket to keep it there; swipe
  a row to Done; the Dock badge counts what needs you, and its menu lists
  it; ⌘1–⌘8 go anywhere.

<p align="center">
  <img src="docs/images/mac-menubar-light.png" width="340" alt="The menu bar panel: a prompt to approve, a question with its two answers as buttons, a failed migration, and a finished turn">
  <img src="docs/images/mac-quick-dark.png" width="520" alt="⌥⌘K over any app: what needs you first (approve or deny a prompt, an agent's two answers), then Create and your dockets">
</p>

<details>
<summary>More of Wheelhouse.app</summary>

<table>
  <tr>
    <td><img src="docs/images/mac-chat-light.png" alt="Chat with an agent: your prompt, its plan with a table, the files it kept, and a box to queue the next message while it works"></td>
    <td><img src="docs/images/mac-board-dark.png" alt="All dockets as a board in dark mode, grouped by status; drag a card to change its status"></td>
  </tr>
  <tr>
    <td><img src="docs/images/mac-record-light.png" alt="A docket's record: verified intact, who signed, and every entry, filtered by how it's known"></td>
    <td><img src="docs/images/mac-reviews-dark.png" alt="Reviews in dark mode: the protocol's state, PRs by what they need, and a Review docket's round history"></td>
  </tr>
  <tr>
    <td><img src="docs/images/mac-window-light.png" alt="A docket in a window of its own"></td>
    <td><img src="docs/images/mac-shortcuts-dark.png" alt="Every keyboard shortcut, from ⌘/"></td>
  </tr>
  <tr>
    <td><img src="docs/images/mac-verify-light.png" alt="A docket's Verify section: done rests on test passing at its commit, witnessed; lint failed but is optional; each check's output a click away"></td>
    <td><img src="docs/images/mac-settings-guides-dark.png" alt="Settings, Guides and Briefs in dark mode: built-in guides and each protocol's briefs beside the Markdown editor, with Compare with Built-in"></td>
  </tr>
  <tr>
    <td><img src="docs/images/mac-settings-orders-light.png" alt="Settings, Standing Orders: what Wheelhouse does on its own, as sentences, with Review's and Build's switches and each gate as Ask or Auto"></td>
    <td><img src="docs/images/settings-dark.png" alt="The web app's Settings in dark mode: the standing orders as sentences, then each gate"></td>
  </tr>
</table>

</details>

The web app is the same Wheelhouse at http://127.0.0.1:7500, and on your
phone over Tailscale.

## Teams

A **workspace** joins each engineer's own Wheelhouse into one: a wheelhouse
of wheelhouses. A small **workspace hub** (the same `wh`, on a Linux box or
an always-on Mac on your tailnet) orders the dockets the team shares, and
every engineer's hub keeps a copy it checks entry by entry.

- **Share what you choose.** `wh team share D-12`, or `wh new --team`.
  Nothing else leaves your Mac, and a shared docket carries approvals
  redacted: the team sees that you approved, not what.
- **Owners and members.** Any number of each on a shared docket. Owners
  change its intent and status; everyone can note, keep files, and put
  agents on it.
- **Agents across Macs.** Every agent's brief lists its teammates' agents,
  and `wh send --to @ACME-1` reaches them all. A teammate's agent's first
  message waits in your Inbox until you let it in. An agent's question
  reaches its person and the docket's owners, and the first answer stands.
- **Checked, not trusted.** Every entry is signed by whoever wrote it. A
  workspace hub that changes, drops or reorders an entry, or adds a key
  nobody issued, is caught by every hub that follows it.

<table>
  <tr>
    <td><img src="docs/images/team-light.png" alt="The Acme workspace in the web app: connected, an entry held back because it looks like a GitHub token, four shared dockets with their agents and people, and three members"></td>
    <td><img src="docs/images/team-docket-dark.png" alt="A shared docket in dark mode: a teammate's agent's message held with Deliver, Drop and Let sam's agents through, its question with two answers, the intent, and its owners, members and teammates' agents in the properties"></td>
  </tr>
  <tr>
    <td><img src="docs/images/mac-team-light.png" alt="Wheelhouse.app's Team place: shared dockets with their people, the members, and ACME-1 open beside them"></td>
    <td><img src="docs/images/team-inbox-dark.png" alt="The Inbox in dark mode: a teammate's agent's question and its held message, beside the shared docket"></td>
  </tr>
</table>

[docs/teams-guide.md](docs/teams-guide.md) is how it works day to day,
[docs/teams-start.md](docs/teams-start.md) gets a workspace running, and
[docs/teams.md](docs/teams.md) is the design and the protocol.

## Requirements

- macOS and [Rex](https://www.superlogical.com/rex), with the `rex` CLI, or
  [Herdr](https://herdr.dev) ([herdr-core](https://herdr.dev/core/) 0.1.20
  or later, with the `hc` CLI), or both: see
  [docs/runtimes.md](docs/runtimes.md)
- [Rust](https://rustup.rs) 1.88 or later, [Node](https://nodejs.org) 22.18
  or later, and [just](https://github.com/casey/just) (`brew install just`)
- Xcode 26 (Swift 6.2), for Wheelhouse.app
- [LuaJIT](https://luajit.org) (`brew install luajit`), to read
  `review.lua` and `build.lua`
- [GitHub CLI](https://cli.github.com), signed in (`gh auth login`), for
  Review and Build
- Claude Code, and Codex or Antigravity (`agy`) if you want them as
  reviewers or builders
- Optional: [Tailscale](https://tailscale.com) with HTTPS on, for the app
  on your phone and Web Push; `terminal-notifier`, for clickable Mac
  notifications; a Linear API key, for linking issues

## Install

```sh
git clone https://github.com/captjt/wheelhouse ~/src/wheelhouse
cd ~/src/wheelhouse
just setup
```

`just setup` builds the hub (`wh`, in `~/.cargo/bin`, with the web app
built in), runs it at login as a LaunchAgent (`wh up`; logs with
`wh logs`), installs Claude Code's and Antigravity's hooks (names from your
prompts, turns with their last message, a new docket on `/clear`) and the `wheelhouse`
skill for agents, and puts Wheelhouse.app in `~/Applications`. Run it again
after pulling to update (`just app` rebuilds only the Mac app).

Then:

1. Open Wheelhouse.app, and allow its notifications. Every agent pane in
   Rex and every agent terminal in Herdr is already a docket. (Or open http://127.0.0.1:7500.)
2. Tell your agents about Wheelhouse: paste the block in
   [docs/agent-instructions.md](docs/agent-instructions.md) into
   `~/.claude/CLAUDE.md` (and `~/.codex/AGENTS.md`,
   `~/.gemini/config/GEMINI.md`).
3. Tab completion: add `eval "$(wh completions zsh)"` to `~/.zshrc`
   (bash: `eval "$(wh completions bash)"` in `~/.bashrc`). Tab fills in
   commands and flags, and from the hub: the dockets in flight with their
   titles (closed ones too, once you type a number), your messages and
   their options, agents' addresses for `wh send --to`, and the PRs Review
   follows. `wh doctor` says whether your shell has it.
4. Optional, the phone: `wh web --publish` serves the app on your tailnet
   at `https://<your-mac>.ts.net:7500`, to your Tailscale user only. Open it
   in Safari and add it to your Home Screen for the app and its
   notifications.
5. Optional, Linear: `wh setup-linear` stores a read-only API key in the
   Keychain.

## Review and Build

Both are off until you set them up.

- **Review:** copy [examples/review.lua](examples/review.lua) to
  `~/.config/wheelhouse/review.lua` and list your repos and their
  checkouts. `wh review on` checks it and turns Review on, paused;
  `wh review resume` starts it. Each PR that requests you becomes a Review
  docket. See [docs/review.md](docs/review.md).
- **Build:** copy [examples/build.lua](examples/build.lua) to
  `~/.config/wheelhouse/build.lua`. Press **Build it** on a docket with an
  intent, or `wh build start D-12`. Pre-review (`prereview`), Linear intake
  (`sources.linear`) and how rounds update the base (`update`) are set
  there too. See [docs/build.md](docs/build.md).

Only the hub writes to GitHub, with your `gh` login, and only what you
approved: agents' panes have no GitHub token, and their pushes fail.

- **Verify:** list a repo's `checks` (in build.lua or Settings) and the
  hub runs them itself, at each commit the checker passes, and records
  them as witnessed. A failing one goes back to the builder and holds the
  PR. A docket with acceptance items closes on evidence, or on your word
  with a reason. `wh hold` pauses a docket's protocol. See
  [docs/verify.md](docs/verify.md).
- **Settings,** in both apps: the standing orders as sentences, gates,
  agents, each repo's notes and checks, and every guide and brief the
  agents are given, editable, with the built-in one a click away. See
  [docs/settings.md](docs/settings.md).

## Commands

| | |
| --- | --- |
| `wh`, `wh ls [--all]` | What needs you, and what's in flight |
| `wh show D-4` | One docket's record, each entry with how it's known |
| `wh new "title"`, `wh new --linear ENG-142` | Open a docket by hand, or from a Linear issue |
| `wh note D-4 "text"`, `wh set D-4 --title … --status done` | Add a note; rename, or set the status (`--anyway --reason …` closes one with acceptance items on your word) |
| `wh check D-4 [--wait]`, `wh hold D-4 ["why"] [--off]`, `wh attention` | Run its witnessed checks; hold or resume it; what needs a look |
| `wh intent D-4 [TEXT\|-]` | Print a docket's intent, or set it |
| `wh link D-4 ENG-142` | Link a Linear issue |
| `wh attach FILE\|URL [--title T] [--docket D-4]` | Keep a file (pinned by its hash) or a link with a docket |
| `wh start [--codex\|--agy] [-C DIR] [--docket D-4] "prompt"` | Start an agent on a new docket, or on one you have (beside any agents already on it), in Rex (or Herdr, with `runtime` set) |
| `wh join D-4 [--role R]`, `wh leave` | From an agent's pane: put it on a docket beside the agents there, or take it off its docket |
| `wh send --to ADDR "text"`, `wh reply N "text"`, `wh whoami`, `wh name NAME` | From an agent's pane, in either runtime: message another agent (`api/builder`, `reviewer`, `herdr:wheelhouse/D-12`, `herdr:t2h`), every agent on a docket (`@D-12`), or you (`--to me`); answer; where you are; name your pane |
| `wh team`, `wh team create\|invite\|join\|members\|share\|people\|trust\|role\|remove\|leave\|send\|drop\|sync\|follow` | Your workspace, and everything a team does with it ([docs/teams-guide.md](docs/teams-guide.md#commands)) |
| `wh new --team "title"`, `wh serve --workspace`, `wh up --workspace` | Open a docket shared from the start; run a workspace hub |
| `wh open [D-4]` | Open a docket (or the Inbox) in Wheelhouse.app, or the web app without it |
| `wh verify [D-4]`, `wh export D-4 > D-4.jsonl`, `wh verify --file D-4.jsonl` | Check a record, every kept file included; one that checks without the hub |
| `wh review`, `on`, `off`, `pause`, `resume`, `check` | The Review protocol, and its switches |
| `wh review post-mode ask\|auto\|config` | Who posts a checked review: you, or the checker |
| `wh review pickup\|preview\|post\|dismiss OWNER/NAME N` | Review a PR that didn't ask; a round's checked review |
| `wh build`, `wh build start D-12\|owner/name#45\|ENG-142`, `discard`, `pause`, `resume` | The Build protocol |
| `wh recap [today\|week\|30d]` | What happened, as Markdown |
| `wh up`, `down`, `logs`, `status` | The hub at login; its log; what it sees, Rex and Herdr each |
| `wh web [--publish]` | The app's address on your tailnet |
| `wh hooks [install\|remove]`, `wh skill [install\|remove]` | Claude Code's and Antigravity's hooks; the agents' skill |
| `wh setup-linear` | Store a Linear API key in the Keychain |
| `wh completions zsh\|bash` | Tab completion, for `eval "$(wh completions zsh)"` in `~/.zshrc` |
| `wh import rex-reviewer` | Move over from rex-reviewer, once |

Agents use `wh` from their panes too: `brief`, `intent`, `note`, `attach`,
`send`, `reply`, `inbox`, `read`, `wait`, `name`, `whoami` and `share`. The
skill (`wh skill`) teaches them how.

## The app

The same keys in Wheelhouse.app and the web app; on the Mac, the menus
have them too (⌘N new docket, ⌘K search, ⌘1–⌘8 go, ⌥⌘1–4 status), ⌥⌘K
works from any app, and ⌘/ lists them all.

| Keys | |
| --- | --- |
| `⌘K`, `/` | Search dockets and commands |
| `C` | New docket |
| `G` then `I`, `A`, `D`, `X`, `N`, `R`, `B`, `P` | Inbox, Active, All, Done, Agents, Reviews, Builds, Recap |
| `J`, `K`, `↩`, `Esc` | Move, open, back |
| `S`, `R`, `M`, `O` | Status, rename, note, the agent's pane |
| `A`, `D`, `1`–`6`, `E` | In the Inbox: approve or deny a prompt, pick a message's option, mark done |
| `⌘,`, `G` then `S` | Settings |

On a phone it's laid out natively: a tab bar, a docket that slides onto
its list (swipe right to go back), rows that swipe to Done or Status, and
sheets from the bottom. The theme follows your system, or pin Light or
Dark at the bottom of the sidebar.

**The record, readable.** Click **Verified** on any docket to read its
record: who signed each entry, how each is known, and what the check means
(and doesn't). [docs/record.md](docs/record.md) is the format.

## Configuration

Everything has a default. These are for running a second hub, tests, and
odd setups.

| Variable | Default | |
| --- | --- | --- |
| `WH_STATE` | `~/.local/state/wheelhouse` | The record, keys, kept files and socket |
| `WH_HTTP` | `127.0.0.1:7500` | The app (`off` for none) |
| `WH_TAILNET` | `127.0.0.1:7501` | The listener `wh web --publish` serves (`off` for none) |
| `WH_SOCKET` | `$WH_STATE/wh.sock` | The CLI's and hooks' socket |
| `WH_REVIEW_CONFIG`, `WH_BUILD_CONFIG` | `~/.config/wheelhouse/review.lua`, `build.lua` | The protocols' configs |
| `WH_SETTINGS` | `~/.config/wheelhouse/settings.json` | What Settings changed, laid over them |
| `WH_NOTIFY` | on | `off`: no Mac notifications or Web Push |
| `WH_REX`, `WH_GH`, `WH_LUAJIT`, `WH_TAILSCALE` | found on PATH | The tools the hub runs |
| `WH_LINEAR_KEY` | the Keychain | A Linear API key |
| `WH_WEB_DIR` | built in | Serve `web/dist` from disk, to iterate on the app |

## How it works

- **The hub** (`wh serve`) follows Rex's and Herdr's events and mirrors
  their sessions, panes and each agent's program status
  ([docs/runtimes.md](docs/runtimes.md)). An agent pane gets a docket;
  Claude Code's and Antigravity's hooks add your prompts and their turns.
- **The record** is SQLite: one signed, chained entry per thing that
  happened, and each docket's state folded from its entries. Files are kept
  once, by hash. Keys are Ed25519, one per signer, on this Mac.
- **The apps**: Wheelhouse.app (SwiftUI, `mac/`) on the Mac, and the web
  app (React, built into the binary and served by the hub) in a browser
  and, through Tailscale, on your phone. Both follow the hub's API and its
  server-sent events. While Wheelhouse.app listens, it posts the Mac's
  notifications, with their actions; the hub posts none of its own.
- **Review and Build** run agents in panes of their own, in Rex or Herdr, with a wall
  around them, and keep their working state per PR or docket.

[DESIGN.md](DESIGN.md) has the why. [docs/roadmap.md](docs/roadmap.md) has
what's next.

## Security

Agents run as you, on your Mac. Wheelhouse keeps their mistakes off GitHub
and out of the app, but it isn't a sandbox against a hostile agent.
[docs/security.md](docs/security.md) says what it protects and what it
doesn't.

## Develop

```sh
just check          # what a pull request needs to pass (there's no CI: run it)
just e2e            # a Build end to end in Rex and in Herdr, both runtimes together, and several agents on one docket, with stand-ins for both, gh, GitHub and Linear
just app            # build Wheelhouse.app into ~/Applications
just dev-install    # Wheelhouse Dev: this checkout beside your everyday one, never meeting it
just dev            # the web app with hot reload on :5180, against a running hub
just screenshots    # the README's images, from a made-up world
```

[CONTRIBUTING.md](CONTRIBUTING.md) covers Wheelhouse Dev and a second hub to try changes on,
where things are, and the house style.

## License

MIT. See [LICENSE](LICENSE).
