# wheelhouse-rune

The ⌥⌘K quick panel in [Rune](https://github.com/anomalyco/rune), a trial
([docs/rune.md](../docs/rune.md)). Its own Cargo workspace: Rune's pinned rev
and patched crates stay here, out of the hub's `Cargo.lock`.

```sh
PORT=7458 scripts/screenshots.sh --serve      # the made-up world's hub, in another terminal
cd rune && cargo run                          # the panel, against it (WH_HUB to point elsewhere)
cargo run -- --snap out.png --theme dark      # headless: check your own work
cargo run -- --tree tree.json
```

Never point it at your daily hub (127.0.0.1:7500), and never install it.
