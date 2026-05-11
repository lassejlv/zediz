# syntax=docker/dockerfile:1.7
# Agent is normally installed directly on the host by cloud-init, not run in Docker.
# This image is for local dev / testing with docker-in-docker or a mounted socket.
FROM rust:1.94-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ ./crates/

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p driftbase-agent \
    && cp target/release/driftbase-agent /usr/local/bin/driftbase-agent

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/driftbase-agent /usr/local/bin/driftbase-agent

ENTRYPOINT ["/usr/local/bin/driftbase-agent"]
