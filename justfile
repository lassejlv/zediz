set shell := ["bash", "-cu"]

# Default: list recipes
default:
    @just --list

# Run control plane + web dev server in parallel.
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'kill 0 2>/dev/null || true' EXIT INT TERM
    cargo run -p driftbase-controlplane &
    (cd web && bun run dev) &
    wait

# Run only the control plane.
dev-cp:
    cargo run -p driftbase-controlplane

# Run only the web dev server.
dev-web:
    cd web && bun run dev

# Run only the agent (expects DRIFTBASE_CONTROL_PLANE_URL + DRIFTBASE_BOOTSTRAP_TOKEN in .env).
dev-agent:
    cargo run -p driftbase-agent

# Full verification: fmt, clippy, tests, frontend typecheck + build.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace --no-fail-fast
    cd web && bun run typecheck && bun run build

fmt:
    cargo fmt --all
