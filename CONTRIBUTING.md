# Contributing to Wheelhouse

Thanks for looking. Wheelhouse is macOS only for now, and needs
[Rex](https://www.superlogical.com/rex), which is in private beta. Read the
[README](README.md) for what it does and [DESIGN.md](DESIGN.md) for why
it's built the way it is. This page covers how to work on it.

Open an issue before a large change, so the shape is agreed first. Small
fixes can go straight to a pull request.

## Setting up

You need what the README's [Requirements](README.md#requirements) list:
Rust 1.88 or later, Node 22.18 or later, Xcode 26, `just`, LuaJIT, and Rex. Each
recipe in the `justfile` is a line or two, so you can run its commands by
hand instead.

```sh
git clone https://github.com/captjt/wheelhouse
cd wheelhouse
just                 # every task
just check           # what a pull request needs to pass: run it before you push
just e2e             # a Build end to end, with stand-ins
```

To run your build as your everyday Wheelhouse, `just setup` installs it,
runs the hub at login, and installs the agents' hooks and skill.

## Where things are

| Path | What's there |
| --- | --- |
| `src/main.rs` | The CLI (`wh`), and `wh serve` starting the hub's loops |
| `src/hub.rs`, `src/store.rs` | The hub's state, and SQLite: entries, dockets, messages, blobs. A new column goes in `migrate` |
| `src/record/` | The record: entries, DSSE envelopes, the chain, keys, verify, and the fold to a docket's state. [docs/record.md](docs/record.md) is the contract |
| `src/mirror.rs`, `src/follow.rs`, `src/rex.rs` | Rex: following its events, and calling its CLI |
| `src/agents.rs`, `src/hooks.rs` | Agent dockets: adopting panes, prompts and turns, Claude Code's hooks |
| `src/messages.rs`, `src/approve.rs`, `src/preview.rs`, `src/start.rs` | Messages and delivery, guarded approvals, `wh share`, `wh start` |
| `src/server.rs` | The HTTP API, its guard, and the app's files |
| `src/artifacts.rs`, `src/recap.rs`, `src/linear.rs`, `src/push.rs`, `src/notify.rs` | Kept files, Recap, Linear, Web Push, Mac notifications |
| `src/review/` | The Review protocol: rules, rounds, posting, links to Linear. [docs/review.md](docs/review.md) |
| `src/build/` | The Build protocol: steps, plans, decisions, the wall. [docs/build.md](docs/build.md) |
| `briefs/`, `guides/`, `skills/` | What agents read: round briefs, review and plan guides, the `wheelhouse` skill |
| `web/` | The web app: React and TypeScript, built with Vite into the binary. `web/src/mobile.tsx` is the phone's gestures; `web/src/markdown.ts` renders and sanitizes Markdown |
| `mac/` | Wheelhouse.app: SwiftUI, against the same API. `Store.swift` follows the hub; `Models.swift` reads its JSON; `Markdown.swift` renders as the web app does; `Notifier.swift` posts notifications with their actions. `./build.sh` makes the bundle |
| `scripts/` | The made-up world for screenshots and demos (below) |
| `examples/` | `review.lua` and `build.lua` to start from |

## Trying a change

**Wheelhouse Dev.** `just dev-install` installs your checkout beside your
everyday Wheelhouse, as a second one that never meets it: `wh-dev`, its hub
on :7600 with its own state (`~/.local/state/wheelhouse-dev`), config,
socket and LaunchAgent (`dev.wheelhouse.hub.dev`), and Wheelhouse Dev.app
(`dev.wheelhouse.app.dev`, `wheelhouse-dev://`, ⇧⌥⌘K). It looks like a
nightly: a blue brand where the daily one is orange, a DEV icon, and DEV
badges with the branch and commit it was built from, in the menu bar, the
window, the quick panel and the web app it serves (the hub's `flavor`, from
`WH_FLAVOR`). It shows a copy of your dockets with nothing live: Rex, gh,
Linear, Tailscale and notifications are off, Review and Build are paused on
stand-in configs, and the agents' hooks stay with your everyday hub. Run it
again after a change to update it; `just dev-sync` copies your dockets in
again, and `just dev-remove` takes it away. See `scripts/dev-install.sh`.

**A hub of your own.** Your everyday hub keeps running. A second one, in
the foreground, needs its own state, socket and port. It still follows
your real Rex, so leave `WH_REX` pointing at a stand-in if your change
could type into panes:

```sh
WH_STATE=/tmp/wh-try WH_HTTP=127.0.0.1:7610 WH_TAILNET=off WH_NOTIFY=off \
  WH_REX=$PWD/scripts/fake-rex WH_WORLD=/path/to/world.json \
  cargo run -- serve
```

`wh serve` refuses to start beside a hub that's already running on the
same socket or port.

**The made-up world.** `just screenshots` (`scripts/screenshots.sh`) builds
the binary, makes a world of agents in Rex (through `scripts/fake-rex`),
plays their prompts and replies through the same hooks Claude Code calls,
adds review history and kept files, and renders the README's images and
the social preview card. Nothing of yours shows up in them.
`scripts/screenshots.sh --serve` keeps that world's hub running at
http://127.0.0.1:7438 to work on the app against.

**Wheelhouse.app.** `cd mac && swift run` runs it against your hub;
`WH_APP_HUB=http://127.0.0.1:7438 swift run` against the made-up world
(`scripts/screenshots.sh --serve`). `swift test` runs its tests. Run
outside a bundle, it posts no notifications; `just app` builds the real
bundle into `~/Applications`. `Wheelhouse --snapshot DIR` renders its
windows into DIR (see `Snapshot.swift`).

**The web app with hot reload.** `just dev` serves the app on :5180 against a
running hub. `WH_WEB_DIR=web/dist` makes a hub serve the built app from disk,
so a rebuilt app shows on reload.

**Tests.** `cargo test` runs the hub's tests: the record, the protocols'
rules, the post's refusals, links to Linear, the wall. They use an
in-memory store and stand-ins, never your Rex, GitHub or Linear. `npm test`
in `web/` runs the renderer's checks. Tests that touch the outside world
use made-up data (`acme/api`, team `ENG`, `@dana`), never real repos or
people.

## Style

- **Rust:** `just fmt` (rustfmt, `rustfmt.toml`). `just check` refuses
  code that doesn't match, and clippy's warnings.
- **Swift:** the system's controls first (lists, sheets, the inspector,
  menus), Kumo's colors from `Theme.swift`, and the web app's words. Read
  the hub's JSON through `Models.swift`, never with a decoder that fails
  on a field it doesn't expect.
- **Words:** docs, comments, UI text and messages to agents are plain
  American English, in short sentences, without "I", "we" or em dashes.
  Name things for what they do. A doc comment says what something is for,
  not how it works line by line.
- **Commits:** a short summary of what changed for someone using
  Wheelhouse, then why, in plain sentences.
