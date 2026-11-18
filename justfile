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
