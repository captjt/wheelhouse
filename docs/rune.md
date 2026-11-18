# Rune: a trial on `dev/rune`

[Rune](https://github.com/anomalyco/rune) is a GPU-rendered Rust UI framework
(macOS, Windows, Linux, and an experimental web target) from Anomaly, the
OpenCode team. This branch tries it on one Wheelhouse surface to decide
whether, and where, it belongs in Wheelhouse. Nothing here replaces
Wheelhouse.app (SwiftUI) or the web app (React). The docket's intent holds the
acceptance items; this page holds the context and the rules.

## What Rune is (November 2026)

- Rust views (`fn view(&State, &mut Cx) -> El`), Taffy flexbox, analytic
  springs on every property, one instanced draw call a frame, its own text
  stack tuned to match CoreText pixel for pixel. Native means compiled and
  GPU-drawn, not system controls: Rune draws its own, close to macOS's.
- **Agent tooling built in:** any app runs headless with `--snap out.png`,
  `--tree tree.json` (the element and accessibility tree), `--script` input
  replay, and `--video`. That is what our SwiftUI `--snapshot` mode and the
  Chrome screenshots do by hand.
- Accessibility through AccessKit (VoiceOver, Voice Control). Multiple
  windows, panels and overlays, global shortcuts, `on_open_url`, context
  menus, drag in and out, rich text editing, streaming Markdown (rune-kit),
  Liquid Glass through native views (`system_material`).
- `rune-kit` mirrors `@opencode/ui`: OpenCode, a coding-agent app like
  Wheelhouse, is being ported onto it, so chat, diffs and sidebars get real use.
- **Maturity:** created 2026-10-22, MIT, no releases. Consumers pin a `dev`
  rev, and patch winit, wgpu-hal, parley and AccessKit with Rune's own forks
  at that rev. 15 to 37 commits a day, two main authors, 460 open issues. No
  pull requests: issues only.

## Against what Wheelhouse uses

| Wheelhouse needs | Rune today |
| --- | --- |
| The phone (the web app on iPhone) | Web target experimental (one WebGPU canvas); its docs say real iPhone keyboard, dictation, autocorrect and Safari WebGPU aren't verified. iOS comes after web, Android after that. |
| Notifications with Approve, Deny, a reply | `rune-notify` posts, withdraws and reports clicks; no actions |
| The menu bar item and its panel | The app owns its tray; Rune gives compact windows and handles |
| Quick Look for kept files | None |
| ⌥⌘K from any app, a floating panel | Global shortcuts, panels and overlays: there |
| VoiceOver | AccessKit: there |
| System controls (DESIGN.md decides "the system's own controls") | Rune draws its own: a design change, Jordan's call |

## The trial

The ⌥⌘K quick panel, rebuilt in Rune against the hub's API: what needs you
(approve, deny, an agent's options), Create, and docket search. It's small,
it's text-input heavy (the riskiest area in any GPU UI), and the SwiftUI one
is there to compare with, on the same data.

## Running it beside your daily Wheelhouse

The Wheelhouse you work in (the LaunchAgent hub on 127.0.0.1:7500,
`~/.local/state/wheelhouse`, `~/.cargo/bin/wh`, `~/Applications/Wheelhouse.app`)
is never touched from this branch:

- **Its own worktree** (`~/git/captjt/rex-hub-rune`, branch `dev/rune`), its
  own `target/`. `main` stays checked out in `~/git/captjt/rex-hub` for daily
  fixes; this branch merges `main` in as it moves.
- **Its own Cargo workspace** in `rune/`, so Rune's pinned rev and its patched
  crates never reach the hub's `Cargo.lock`, and `just check` on the hub
  doesn't build Rune.
- **Its own hub: the made-up world.** `PORT=7458 scripts/screenshots.sh
  --serve` runs a hub with its own HOME, a stand-in Rex and gh, at
  http://127.0.0.1:7458. The Rune app reads `WH_HUB` (default that world) and
  must never write to 7500.
- **Its own identity:** binary `wheelhouse-rune`, bundle id
  `dev.wheelhouse.rune`, no `wheelhouse://` scheme, and ⌃⌥⌘K instead of ⌥⌘K
  so it never takes the installed app's shortcut.
- **Never from here:** `just install`, `just app`, `just setup`, `wh up`,
  `wh down`, `cargo install`, `wh hooks install`, `wh skill install`.

## Deciding

Write the findings here when the trial is done: how it looks beside the
SwiftUI panel (light and dark), text input and IME, VoiceOver, build times
(cold and incremental), binary size, frame times, how well an agent checks
its own work headlessly, and what an upgrade to a newer Rune rev cost. Then
one of:

- **Adopt** for a named surface (and say what it replaces),
- **Wait** for a named trigger (a tagged release, the iOS target,
  notification actions),
- **Drop** it, and say why.
