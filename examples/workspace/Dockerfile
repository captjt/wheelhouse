# syntax=docker/dockerfile:1@sha256:572cebbc2b15e6f84c077742952674889cb63cd5e9b80abdffcbe587b6f19183
#
# A workspace hub (wh serve --workspace) in a container, for a Linux box or
# VM on your tailnet (docs/teams.md). From the repo's root:
#
#   docker build -f examples/workspace/Dockerfile -t wheelhouse-workspace .
#   docker run -d --name wheelhouse-workspace --restart unless-stopped \
#     --read-only --tmpfs /tmp:rw,noexec,nosuid,nodev,size=16m \
#     --cap-drop ALL --security-opt no-new-privileges:true \
#     -p 127.0.0.1:7700:7700 -v wheelhouse-workspace:/var/lib/wheelhouse-workspace \
#     wheelhouse-workspace
#   tailscale serve --bg 7700
#   docker logs wheelhouse-workspace     # the setup code, for wh team create
#
# It listens inside the container on 0.0.0.0:7700, published only on the
# host's loopback, where tailscale serve reaches it. Every request still
# needs a member's Tailscale login and key. (If the host's Tailscale runs
# in userspace mode, publish on 127.0.0.2 and serve that instead:
# docs/teams-gcp.md says why.)
#
# What it is, and why:
# - Every base image is pinned by digest, so a build uses exactly these.
#   To move to newer ones: docker buildx imagetools inspect IMAGE:TAG.
# - Only what the binary is built from goes into the build (Cargo files,
#   src, and the briefs, guides and skill it embeds), never the whole
#   checkout.
# - wh is built static (musl), so it runs on distroless static: about
#   2 MB of CA certificates, tzdata and a passwd entry. No libc, no shell,
#   no package manager: nothing to exploit or patch but wh itself. The
#   build fails if the binary links anything dynamically. It runs as
#   uid 65532 (nonroot), never root. It writes only its state and
#   SQLite's temporary files (a small tmpfs at /tmp), so it runs with
#   --read-only and every capability dropped.
#   RUNTIME_IMAGE=cgr.dev/chainguard/static@sha256:… (same uid) or
#   scratch (no CA certificates, no /tmp) work too.
# - wh handles SIGTERM itself, so docker stop is prompt with no --init.
#
# Before this image, the hub ran as uid 999. An existing state volume
# needs its owner changed once:
#
#   docker run --rm -v wheelhouse-workspace:/v alpine chown -R 65532:65532 /v

ARG NODE_IMAGE=node:24-bookworm-slim@sha256:873f586f3794f52281bd039bf02c048a0be08fde619338f9941bad1eb6b34530
ARG RUST_IMAGE=rust:1.93-alpine3.22@sha256:3c06253e433c1b2ac2c279a98226d385d25c5f324138ab2861a5414bfa6855f9
ARG RUNTIME_IMAGE=gcr.io/distroless/static-debian13:nonroot@sha256:8ce6dd87e9823c5b48dcfb2c8604714ccb351ce59607327bcc4957448da279b5

# The web app. Packages exactly as locked, and none of their install
# scripts run.
FROM ${NODE_IMAGE} AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund --ignore-scripts
COPY web/ ./
RUN npm run build

# The binary, with the web app built into it: static, against musl.
FROM ${RUST_IMAGE} AS build
# C compilers and headers for the bundled SQLite and aws-lc.
RUN apk add --no-cache build-base cmake perl
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src/ src/
COPY briefs/ briefs/
COPY guides/ guides/
COPY skills/ skills/
COPY --from=web /src/web/dist web/dist
ENV CARGO_PROFILE_RELEASE_STRIP=symbols
# COPY keeps each file's mtime, and cargo trusts mtimes against the cached
# target folder: touch the sources so a checkout older than the cache's
# last build is still built, never served from it.
RUN find src briefs guides skills web/dist build.rs Cargo.toml Cargo.lock -exec touch {} +
RUN --mount=type=cache,id=wh-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
  --mount=type=cache,id=wh-target-musl,target=/src/target,sharing=locked \
  cargo build --release --locked \
  && install -D -m 0755 target/release/wh /out/wh \
  && install -d -m 0700 /out/state \
  && readelf -lW /out/wh > /tmp/wh.headers && readelf -dW /out/wh > /tmp/wh.dynamic \
  && if grep -q INTERP /tmp/wh.headers || grep -q NEEDED /tmp/wh.dynamic; then \
  echo "wh links dynamically: it must be static to run on distroless static" >&2; cat /tmp/wh.dynamic >&2; exit 1; \
  fi

FROM ${RUNTIME_IMAGE}
ARG REVISION=unknown
LABEL org.opencontainers.image.title="wheelhouse-workspace" \
  org.opencontainers.image.description="A Wheelhouse workspace hub: wh serve --workspace" \
  org.opencontainers.image.source="https://github.com/captjt/wheelhouse" \
  org.opencontainers.image.licenses="MIT" \
  org.opencontainers.image.revision="${REVISION}"
COPY --from=build --chown=0:0 --chmod=0755 /out/wh /usr/local/bin/wh
COPY --from=build --chown=65532:65532 --chmod=0700 /out/state /var/lib/wheelhouse-workspace
USER 65532:65532
ENV WH_STATE=/var/lib/wheelhouse-workspace WH_WORKSPACE_HTTP=0.0.0.0:7700
VOLUME /var/lib/wheelhouse-workspace
EXPOSE 7700
# Nothing in the image could run a probe (no shell, no curl), and the only
# open route wants Serve's login header. A crash ends the process, and
# Docker's --restart or systemd's Restart= brings it back.
HEALTHCHECK NONE
ENTRYPOINT ["/usr/local/bin/wh"]
CMD ["serve", "--workspace"]
