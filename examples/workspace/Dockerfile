# A workspace hub (wh serve --workspace) in a container, for a Linux box or
# VM on your tailnet (docs/teams.md). From the repo's root:
#
#   docker build -f examples/workspace/Dockerfile -t wheelhouse-workspace .
#   docker run -d --name wheelhouse-workspace --restart unless-stopped \
#     -p 127.0.0.1:7700:7700 -v wheelhouse-workspace:/var/lib/wheelhouse-workspace \
#     wheelhouse-workspace
#   tailscale serve --bg 7700
#   docker logs wheelhouse-workspace     # the setup code, for wh team create
#
# It listens inside the container on 0.0.0.0:7700, published only on the
# host's loopback, where tailscale serve reaches it. Every request still
# needs a member's Tailscale login and key.

FROM node:24-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

FROM rust:1.93-bookworm AS build
WORKDIR /src
COPY . .
COPY --from=web /src/web/dist web/dist
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
  && useradd --system --home /var/lib/wheelhouse-workspace --create-home wheelhouse
COPY --from=build /src/target/release/wh /usr/local/bin/wh
USER wheelhouse
ENV WH_STATE=/var/lib/wheelhouse-workspace WH_WORKSPACE_HTTP=0.0.0.0:7700
VOLUME /var/lib/wheelhouse-workspace
EXPOSE 7700
CMD ["wh", "serve", "--workspace"]
