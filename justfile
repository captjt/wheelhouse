# Wheelhouse. `just` lists the recipes.

default:
    @just --list

# The web app's packages, exactly as locked.
deps:
    cd ./web && npm ci --no-audit --no-fund

# Build the web app into web/dist.
web: deps
    cd ./web && npm run build

# Build the web app, then the binary (the app is built into it).
build: web
    cargo build --release

# Every check a pull request needs to pass: formatting, the Rust tests and
# clippy, the web app's types and tests, and the Mac app's build and tests.
check: deps
    cargo fmt --check
    cargo test
    cargo clippy --all-targets -- -D warnings
    cd ./web && npm run typecheck && npm test
    just check-app

# The Mac app: its tests, and a release build. Needs Xcode 26 (Swift 6.2).
check-app:
    cd ./mac && swift test && swift build -c release

# A Build end to end on a scratch hub, with stand-ins for Rex, Herdr, gh
# and GitHub: the plan, the PR, two address rounds, and the merge. Once
# with the agents' panes in Rex, once in Herdr. Then Rex and Herdr
# together: agents in each messaging, approved and started across both.
e2e:
    scripts/build-e2e.sh rex
    PORT=7624 scripts/build-e2e.sh herdr
    scripts/runtimes-e2e.sh

# Build Wheelhouse.app and put it in ~/Applications.
app:
    ./mac/build.sh --install

# Format the Rust code (rustfmt.toml).
fmt:
    cargo fmt

# Put `wh` on your PATH (~/.cargo/bin).
install: web
    cargo install --path . --locked

# Install, then run the hub at login (a LaunchAgent), with the agents'
# hooks and skill, and the Mac app. Run it again after pulling to update.
setup: install
    wh up
    wh hooks install
    wh skill install
    just app

# On a copy of your dockets with nothing live: wh-dev, :7600, Wheelhouse
# Dev.app and ⇧⌥⌘K, never meeting the daily ones (scripts/dev-install.sh).
# Install or update Wheelhouse Dev: this checkout beside the daily install.
dev-install:
    scripts/dev-install.sh

# Copy the daily dockets into Wheelhouse Dev again.
dev-sync:
    ~/.local/bin/wh-dev sync

# Take Wheelhouse Dev away (its dockets stay until you delete them).
dev-remove:
    scripts/dev-install.sh --remove

# Run the hub from this checkout, in the foreground.
serve: build
    ./target/release/wh serve

# The web app with hot reload, against a running hub (http://localhost:5180).
dev:
    cd ./web && npm run dev

# The README's screenshots (the web app, on a Mac and a phone, and
# Wheelhouse.app) and the social preview card, from a made-up world.
screenshots:
    scripts/screenshots.sh
