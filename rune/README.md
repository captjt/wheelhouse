# wheelhouse-rune

Wheelhouse.app rebuilt in [Rune](https://github.com/anomalyco/rune), beside
the SwiftUI one ([docs/rune.md](../docs/rune.md): the trial, the port, and
what's left). Its own Cargo workspace: Rune's pinned rev and patched crates
stay here, out of the hub's `Cargo.lock`, with its own toolchain
(`rust-toolchain.toml`, Rune's).

The whole Mac app: the main window (Inbox, Reviews, Builds, Active, All
dockets, Done, Rex sessions, Agents, Recap), a docket's page and chat, the
record, Verify, the Build and Review panels, ⌘K, every sheet, dockets in
windows of their own, the menu bar's wheel and panel, notifications you can
answer on the banner, the Dock badge, the quick panel, and Settings. It
follows the hub's events, and reads `WH_HUB`: the made-up world by default.
It refuses the daily hub on :7500.

```sh
PORT=7458 scripts/screenshots.sh --serve        # the made-up world's hub, in another terminal
cd rune && cargo run --release                   # the main window; ⌃⌥⌘K from any app, the quick panel
cargo test --release                             # the model, notifications, settings patches, Markdown offsets
```

Its identity never meets the installed app's: binary `wheelhouse-rune`,
bundle id `dev.wheelhouse.rune`, no URL scheme, and ⌃⌥⌘K rather than ⌥⌘K
(or Wheelhouse Dev's ⇧⌥⌘K). Never `cargo install` it.

| | |
| --- | --- |
| `src/app/` | The main window, on rune-kit's shell (`mod.rs`), and each of its parts: `sidebar`, `lists`, `docket`, `activity`, `chat`, `record`, `verify`, `panels`, `recap`, `palette`, `forms`, `menus`, `shortcuts` |
| `src/app/windows.rs` | Docket windows: four made at launch, reused (Rune can't add one while running) |
| `src/app/menubar.rs`, `notify.rs` | The menu bar's wheel and panel, the Dock badge, and notifications, partly in AppKit |
| `src/app/settings/` | Settings, a window of its own with every tab |
| `src/panel.rs`, `view.rs`, `commands.rs` | The quick panel (the trial), whose commands ⌘K shares |
| `src/store.rs`, `hub.rs`, `model.rs` | One store every window reads, following the hub; the hub's API; the hub's JSON |
| `scripts/compare.sh` | Both apps, screen by screen, light and dark, side by side |

## Trying it on your own dockets

Beside your everyday Wheelhouse, against Wheelhouse Dev's hub (`just
dev-install`: a copy of your dockets with nothing live, on :7600):

```sh
rune/scripts/bundle.sh                      # target/Wheelhouse Rune.app, following :7600
open "rune/target/Wheelhouse Rune.app"      # the main window, the wheel in the menu bar; ⌃⌥⌘K the quick panel
```

On a flavored hub it wears the hub's badge (DEV, a blue wheel). Answering
an ask or a prompt there answers Wheelhouse Dev's copy, never the agent.
`WH_HUB=… scripts/bundle.sh` points the app elsewhere, but never at :7500.
Quit it with ⌘Q, the menu bar panel's "…", or `pkill -x wheelhouse-rune`.

## Checking it headlessly

Rune's agent tooling, in release builds through the `tooling` feature (on by
default; `--no-default-features` leaves it out). Each run renders one window
with the hub's state at that moment, on a virtual clock; calls to the hub are
made inline there, so frames don't depend on the network. Every window has
scenes (`--list-windows`, then `--window W --list-scenes`): `main` has one for
each place and sheet, `settings` one for each tab, `menubar`, `docket` and
`panel` theirs. Some lay made-up data over the state (a Build, a review
waiting on you) for that run only; none write to the hub.

```sh
B=target/release/wheelhouse-rune
$B --window main --scene docket --snap docket.png --size 1440x900 --scale 1
$B --window settings --scene settings-repos --theme dark --snap repos.png
$B --window menubar --scene inbox --size 380x324 --snap menubar.png      # --snap keeps the launch size: pass the panel's
SKIP_SWIFT=1 scripts/compare.sh                                           # every pair again, from rune/../.tmp/compare/swift
$B --window panel --snap light.png --theme light --scroll-bars always
$B --window panel --snap dark.png --theme dark --scale 1          # 1x, as screencapture takes the SwiftUI one
$B --window panel --tree tree.json                                # elements and the accessibility tree
$B --window panel --script scripts/edit.json --tree t.json --at mark:composing   # typing, undo, IME
scripts/ask.sh && $B --window panel --script scripts/answer.json --video answer.mp4 --keys bottom-right --pointer
```

`scripts/answer.json` answers the made-up world's ask for real, so
`scripts/ask.sh` poses it again first (on `WH_HUB`, never :7500).
`scripts/approve.json` approves and denies D-1's prompt, and
`scripts/create.json` makes a docket with a note: both write to the world.
`WH_RUNE_LOG=1` prints each state that lands, to watch it follow the hub.

## On screen

```sh
WH_RUNE_THEME=dark $B -AppleShowScrollBars Always                  # light|dark, else the system's
$B --show                                                          # the quick panel too, at launch
$B --show --demo --native --trace trace.jsonl --frames 600         # present times under motion
swift scripts/ax-tree.swift $(pgrep -n wheelhouse-rune)            # what VoiceOver reads
WH_RUNE_NOTIFY=off $B                                              # no notifications this run
```

Notifications need the bundle (macOS notifies only for one): run
`scripts/bundle.sh` and open that. The quick panel takes the keyboard when it
shows (a launcher panel does), and goes when you click elsewhere, as the
SwiftUI one does.

## Moving Rune

`scripts/bump-rune.sh [REV]` moves every pin to REV (default: `dev`'s head)
after checking Rune's own `[patch.crates-io]` at that rev still names the crates
patched here, then builds and tests.
