# wheelhouse-rune

The ⌥⌘K quick panel in [Rune](https://github.com/anomalyco/rune), a trial
([docs/rune.md](../docs/rune.md): the findings and the decision). Its own Cargo
workspace: Rune's pinned rev and patched crates stay here, out of the hub's
`Cargo.lock`, and its own toolchain (`rust-toolchain.toml`, Rune's).

It does what the SwiftUI panel does floating: what needs you (approve or deny a
prompt, pick an agent's option, post a review), Create (a docket, an agent on a
new docket, a docket from a Linear issue), and docket search with the same
scoring. Return on a docket opens it in the web app. It follows the hub's
events, and reads `WH_HUB`: the made-up world by default. It refuses the daily
hub on :7500.

```sh
PORT=7458 scripts/screenshots.sh --serve        # the made-up world's hub, in another terminal
cd rune && cargo run --release -- --show         # the panel; ⌃⌥⌘K from any app shows it again
cargo test --release                             # the model: inbox order, scoring
```

Its identity never meets the installed app's: binary `wheelhouse-rune`,
bundle id `dev.wheelhouse.rune`, no URL scheme, and ⌃⌥⌘K rather than ⌥⌘K
(or Wheelhouse Dev's ⇧⌥⌘K). Never `cargo install` it.

## Trying it on your own dockets

Beside your everyday Wheelhouse, against Wheelhouse Dev's hub (`just
dev-install`: a copy of your dockets with nothing live, on :7600):

```sh
rune/scripts/bundle.sh                      # target/Wheelhouse Rune.app, following :7600
open "rune/target/Wheelhouse Rune.app"      # the panel shows; ⌃⌥⌘K brings it back
```

On a flavored hub it wears the hub's badge (DEV, a blue wheel). Answering
an ask or a prompt there answers Wheelhouse Dev's copy, never the agent.
`WH_HUB=… scripts/bundle.sh` points the app elsewhere, but never at :7500.
Quit it from Activity Monitor or with `pkill -x wheelhouse-rune`.

## Checking it headlessly

Rune's agent tooling, in release builds through the `tooling` feature (on by
default; `--no-default-features` leaves it out). Each run renders the panel
window with the hub's state at that moment, on a virtual clock; calls to the
hub are made inline there, so frames don't depend on the network.

```sh
B=target/release/wheelhouse-rune
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
WH_RUNE_THEME=dark $B --show -AppleShowScrollBars Always          # light|dark, else the system's
$B --show --demo --native --trace trace.jsonl --frames 600         # present times under motion
swift scripts/ax-tree.swift $(pgrep -n wheelhouse-rune)            # what VoiceOver reads
```

The panel takes the keyboard when it shows (a launcher panel does), and goes
when you click elsewhere, as the SwiftUI one does.

## Moving Rune

`scripts/bump-rune.sh [REV]` moves every pin to REV (default: `dev`'s head)
after checking Rune's own `[patch.crates-io]` at that rev still names the crates
patched here, then builds and tests.
