# syntax=docker/dockerfile:1.7
FROM rust:1.94-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ ./crates/

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p driftbase-controlplane \
    && cp target/release/driftbase-controlplane /usr/local/bin/driftbase-controlplane

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/driftbase-controlplane /usr/local/bin/driftbase-controlplane

ENV DRIFTBASE_BIND_ADDR=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/driftbase-controlplane"]
