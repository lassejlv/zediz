# Repository Guidelines

## Project Structure & Module Organization
`zediz` is a Rust workspace with a Bun-powered web app. Core backend crates live in `crates/`: `controlplane` contains the HTTP API, scheduler, DB access, and SQL migrations in `crates/controlplane/migrations`; `agent` runs on managed nodes; `hetzner` wraps the Hetzner API; `proto` and `common` hold shared types and utilities. The frontend lives in `web/`, with route files under `web/src/routes`, shared API helpers in `web/src/lib`, reusable UI in `web/src/components`, and Tailwind styles in `web/src/styles`.

## Build, Test, and Development Commands
Use `just` from the repo root for the main workflow:

- `just dev` runs the control plane and Vite dev server together.
- `just dev-cp` starts only `cargo run -p zediz-controlplane`.
- `just dev-web` starts only the frontend with `bun run dev`.
- `just dev-agent` runs `cargo run -p zediz-agent` and expects agent env vars in `.env`.
- `just check` is the full verification pass: `cargo fmt`, `cargo clippy`, `cargo test`, plus frontend typecheck and production build.
- `just fmt` formats all Rust code.

For local services, start Postgres with `docker compose up -d`.

## Coding Style & Naming Conventions
Follow Rust formatting via `cargo fmt`; keep modules small and organized by domain (`auth`, `domains`, `projects`, etc.). Use `snake_case` for Rust modules/functions and `PascalCase` for React components. Frontend code uses TypeScript, 2-space indentation, single quotes, and route filenames that mirror TanStack Router conventions such as `w.$workspaceSlug.projects.$projectSlug.tsx`.

## Testing Guidelines
Backend tests currently live inline as Rust unit tests (`#[cfg(test)]`) near the code they cover. Run all backend tests with `cargo test --workspace`. The web app exposes `bun test`, but the main gate today is `bun run typecheck && bun run build`; include both in your verification for frontend changes. Add tests for new parsing, auth, token, and validation logic when practical.

## Commit & Pull Request Guidelines
Recent commits use short, imperative subjects such as `Publish zediz-controlplane to GHCR` and `Fix zediz-caddy restart loop + unreachable admin API`. Keep commits focused and descriptive. PRs should explain the user-visible or operational impact, call out config or migration changes, and include screenshots for web UI changes. If a change touches provisioning, routing, or deployments, mention rollback or risk notes explicitly.

## Security & Configuration Tips
Do not commit secrets or live Hetzner tokens. Keep local configuration in `.env`, verify database migrations before merging, and document any new required environment variables in `README.md` or the PR description.
