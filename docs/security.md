# Security

Wheelhouse runs on your Mac, as you, next to agents that also run as you.
This page says what it protects, what it doesn't, and what to do about the
gap.

## What it protects

**The app answers only you.**

- On this Mac, it listens on 127.0.0.1 only. A request must name one of
  Wheelhouse's own hosts, which stops DNS rebinding.
- On your tailnet, `wh web --publish` serves it through `tailscale serve`.
  Every request must carry your Tailscale login; anyone else on the tailnet
  gets a 403.
- Writes must carry a header a browser can't send cross-site without a
  preflight, and the hub refuses preflights, so another website can't act
  through your browser.
- No other site can frame the app (`frame-ancestors 'none'`), so it can't
  trick you into clicking Approve or Post.
- The app's content security policy runs only its own scripts.

**What agents and PRs write can't run in the app.** Agent turns, messages,
PR descriptions, review drafts and Linear text are Markdown. Wheelhouse
renders them to HTML, sanitizes it (DOMPurify), and inserts the result as
DOM nodes, never as an HTML string parsed again. Files an agent keeps (an
HTML page, an SVG) open in a sandboxed frame with no access to the app's
origin. When opened directly, they're served with a sandbox CSP and
`nosniff`.

**Text typed into a pane is only text.** Messages, queued prompts and
message headers lose every control character (escape sequences included)
except newlines and tabs before they're pasted, so one agent can't type
keys into another. Pane names are letters, digits, `.`, `_` and `-`.
Messages go in only between an agent's turns, only into an agent, and
only while the hub can see Rex.

**Only the hub writes to GitHub, and only what you approved.**

- The panes Review and Build start have no `GH_TOKEN`, no Linear key and
  no SSH agent. `gh` is signed out, and every `git push` is rewritten to a
  URL that fails. Build's Claude Code also can't run `git push`, `gh` or
  `wh send`, and has no MCP servers.
- The hub posts a review only if its draft is byte for byte the one you
  saw, and opens a PR only if its title and description are the ones you
  were asked about. It pushes only the exact commit the checker checked and
  you approved.
- The hub's own git commands, which run with your credentials in repos
  agents can write to, never run hooks, an fsmonitor or other programs from
  the repo's config. They never skip TLS checks.
- The hub never writes, reads or deletes through a symbolic link in an
  agent's `.tmp` folder.

**The record can't be changed quietly.** Each entry is signed, by you,
Wheelhouse or an agent, each with its own key. Entries are chained by hash,
and kept files are pinned by their hash. `wh verify` (or **Verified** on a
docket) shows that nothing was changed, removed, added in between or
reordered. [record.md](record.md) has the format.

**Your state is yours.** `~/.local/state/wheelhouse` is readable only by
you (0700). The keys and the database are 0600, and the socket is too.

## What it doesn't protect

**Agents run as you, so a determined agent can do what you can.** The wall
around Review's and Build's agents stops mistakes. It doesn't stop an agent
that's trying, for example one following instructions planted in a PR it
reviews. Running as you, an agent can:

- read your files, including `~/.config/gh` and Keychain items the
  `security` tool can read without asking;
- undo the environment the wall sets up, and push with your credentials;
- call the hub's API on 127.0.0.1 or its socket, the way the app and `wh`
  do. A caller that doesn't say which pane it's in counts as you, so it
  can approve prompts, post reviews or start agents. It can change
  Settings too (docs/settings.md): a gate to `auto`, a repo's checks, a
  guide or brief. Settings refuses what reaches outside (agents' programs,
  PATH, the wall), and keeps what each save replaces in
  `~/.config/wheelhouse/.history`, but it can't tell such a call from
  yours.

Claude Code asks before running commands unless you've allowed them, and
that's the real line of defense today. Wheelhouse makes the honest mistakes
harmless; it can't make a hostile agent harmless.

## What to do about it

- **Review PRs from people you trust.** Review picks up the PRs that
  request you, in the repos you list. For repos open to outside
  contributors, keep `post = "ask"`, and read what the checker drafted
  before you press Post.
- **Keep agents' permissions tight.** Don't run review or build agents
  with `--dangerously-skip-permissions`, or with blanket allow rules for
  `curl`, `security` or `git push`.
- **For untrusted code, isolate the agents:** Claude Code's sandbox with
  network limits, or a separate macOS user with its own Rex. Real isolation
  between agents and the hub is on the [roadmap](roadmap.md).

## Reporting a problem

Open a private security advisory on the repository, or message the
maintainer directly. Please don't open a public issue for a vulnerability.
