# Repository Guidelines

## Important 

- DONT ever mention your self in commit messages
- DO RESEARCH, not slop and unfinsied tasks
- SEARCH, SEARCH
- Listen TO ME when i tell you to do something!!
- Use mcp for context7 if its there and not the cli, only if the mcp is not available

## Architecture Overview
Zediz is a Hetzner-first PaaS with one Rust control plane, one Rust node agent, and a Bun/Vite web client. The control plane serves `/api/v1`, runs SQLx migrations on startup, builds shared `AppState`, and starts the background scheduler. The main resource chain is `workspace -> project -> service -> deployment`; workspaces also own members, invites, credentials, SSH keys, nodes, and service domains. Deploys are control-plane driven: creating a deploy marks a deployment `pending`, the scheduler picks capacity or provisions a Hetzner node, then the agent pulls commands via heartbeat and performs Docker/Caddy work locally.

## Project Structure & Module Organization
Backend crates live in `crates/`:

- `controlplane`: Axum API, auth, scheduler, provisioning, migrations.
- `agent`: node bootstrap, heartbeats, Docker executor, node-local Caddy.
- `hetzner`, `proto`, `common`: provider client, shared wire types, shared utilities.

Frontend code lives in `web/`: TanStack Router routes in `web/src/routes`, API helpers in `web/src/lib`, reusable UI in `web/src/components`, and Tailwind styles in `web/src/styles`. Route files mirror backend resource structure. Do not edit `web/src/routeTree.gen.ts`; it is generated.

## Build, Test, and Development Commands
- `just dev`: runs the control plane and Vite dev server together.
- `just dev-cp`: runs only `cargo run -p zediz-controlplane`.
- `just dev-web`: runs only the frontend on `http://127.0.0.1:5173` with `/api` proxied to `:8080`.
- `just dev-agent`: runs the agent; requires `ZEDIZ_CONTROL_PLANE_URL` plus `ZEDIZ_BOOTSTRAP_TOKEN` or `ZEDIZ_NODE_TOKEN`.
- `just check`: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, then `cd web && bun run typecheck && bun run build`.
- `just fmt`: formats Rust only.

Run `bun install` in `web/` before frontend work. The checked-in `docker-compose.yml` does not provide Postgres, so do not assume `docker compose up -d` gives you a full local stack.

## Coding Style & Naming Conventions
Use Rust `snake_case` for modules/functions and keep backend code grouped by domain modules such as `auth`, `workspaces`, `projects`, `services`, `domains`, and `nodes`. Frontend components use `PascalCase`; route filenames follow TanStack Router patterns like `w.$workspaceSlug.projects.$projectSlug.$serviceSlug.tsx`. TypeScript uses 2-space indentation and single quotes. There is no checked-in frontend lint config, so typecheck/build are the main guardrails.

## Testing Guidelines
Backend tests are mostly inline Rust unit tests near the code they cover. Add or extend tests when changing auth, token handling, slug validation, encryption, scheduler behavior, or parsing logic. Frontend `bun test` exists, but the current contributor gate is `bun run typecheck && bun run build`. If you change an API shape, update both the backend route/module and the corresponding `web/src/lib/*` client code.

## Configuration, Security & Ops
Required control-plane env vars are `ZEDIZ_DATABASE_URL` and `ZEDIZ_MASTER_KEY`; the master key must be base64 for exactly 32 bytes and protects stored credentials and SSH private keys. Treat key rotation as a migration task, not a casual env swap. `ZEDIZ_PUBLIC_URL`, `ZEDIZ_BIND_ADDR`, and `ZEDIZ_COOKIE_SECURE` affect agent registration and cookie behavior. There are two Caddy layers: repo-root Caddy serves the SPA and proxies `/api`, while each managed node runs its own Caddy sidecar for service domains on ports `80/443`. Provisioning is Hetzner-only right now, and services are image-based today; do not document Git-source deploys or true multi-replica reconciliation as if they already exist.

## Commit & Pull Request Guidelines
Follow the existing style: short, imperative, outcome-focused subjects such as `Fix zediz-caddy restart loop` or `Publish zediz-web to GHCR`. Keep PRs scoped, explain user-visible or operational impact, call out env or migration changes, and include screenshots for web UI work. If a change touches scheduling, provisioning, domains, or node lifecycle, include rollback or debugging notes.
