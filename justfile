# Wheelhouse. `just` lists the recipes.

default:
    @just --list

# Build the web app, then the binary (the app is built into it).
build: web
    cargo build --release

# Build the web app into web/dist.
web:
    cd ./web && npm install --no-audit --no-fund && npm run build

# Every check: Rust tests, clippy, and the web app's types.
check:
    cargo test
    cargo clippy --all-targets -- -D warnings
    cd ./web && npm run typecheck

# Run the hub from this checkout.
serve: build
    ./target/release/wh serve

# The web app with hot reload, against a running hub (http://localhost:5180).
dev:
    cd ./web && npm run dev

# Put `wh` on your PATH (~/.cargo/bin).
install: web
    cargo install --path .
