# Driftbase — Self-hosted PaaS on Hetzner (Coolify/Railway-like)

A Rust control plane + Rust node agent that turns a Hetzner API token into an elastic, auto-scaling container platform. Users define services (Docker image or Git repo); the scheduler places them on existing nodes, or provisions new ones on demand, and tears them down when idle.

---

## 1. Architecture overview

```
┌───────────────────────────────────────────────────────────────┐
│ Browser (React / Vite 8 / TanStack Router / Tailwind v4)      │
└──────────────────────────▲────────────────────────────────────┘
                           │ HTTPS (JSON REST + SSE for logs/events)
┌──────────────────────────┴────────────────────────────────────┐
│ Control Plane (Rust, axum)                                    │
│  • Auth (sessions, invites, workspaces/RBAC)                  │
│  • Projects / Services / Deployments API                      │
│  • Scheduler (bin-pack + autoscale)                           │
│  • Hetzner provisioner  • Registry auth  • Build dispatcher   │
│  • Event bus (Postgres LISTEN/NOTIFY)                         │
└─────▲─────────────────▲─────────────────▲────────────────▲────┘
      │ SQL             │ mTLS WS/gRPC    │ HTTPS          │
   Postgres           Node agents     Hetzner API      Registry
   (sqlx)            (Rust, per-VM)   (server/vol/ssh)  (distribution)
                          │
                          ├─ Docker engine (bollard)
                          ├─ Volume mounts (Hetzner volumes)
                          └─ Caddy (L7 proxy per node, auto-TLS)
```

Core loop: `Service` → `Deployment` (pending) → scheduler picks `Node` (or provisions one) → agent pulls image + runs container → proxy gets routes → health checks → status back to control plane.

---

## 2. Repo layout (Cargo workspace + frontend)

```
driftbase/
├── Cargo.toml                       # workspace
├── crates/
│   ├── controlplane/                # axum HTTP API, scheduler, provisioner
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── http/                # routes, extractors, middleware
│   │   │   ├── auth/                # sessions, invites, rbac
│   │   │   ├── domain/              # Service, Deployment, Node, Workspace
│   │   │   ├── scheduler/           # placement, autoscale
│   │   │   ├── provisioner/         # hetzner API client glue
│   │   │   ├── builder/             # git→image build jobs
│   │   │   ├── registry/            # registry auth, tag layout
│   │   │   ├── events/              # pg LISTEN/NOTIFY bus
│   │   │   └── agent_rpc/           # server side of agent protocol
│   │   └── migrations/              # sqlx migrations
│   ├── agent/                       # per-node binary
│   │   └── src/
│   │       ├── main.rs
│   │       ├── docker.rs            # bollard wrapper
│   │       ├── proxy.rs             # caddy admin API
│   │       ├── metrics.rs           # /proc + cgroup sampling
│   │       └── rpc.rs               # control-plane client (reconnecting WS)
│   ├── proto/                       # shared wire types (serde + bincode/JSON)
│   ├── hetzner/                     # typed Hetzner Cloud API client
│   └── common/                      # errors, tracing setup, ids, crypto
├── web/                             # Vite 8 + React + TanStack
│   ├── index.html
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── src/
│   │   ├── main.tsx
│   │   ├── routes/                  # file-based (TanStack Router)
│   │   ├── components/
│   │   ├── lib/                     # api client, query hooks, auth
│   │   └── styles/                  # tailwind.css
│   └── package.json
└── PLAN_DRIFTBASE.md
```

Backend-only install uses Cargo; frontend uses Bun per global rules. `bun install`, `bun run dev`, etc.

---

## 3. Data model (Postgres)

Naming: `snake_case`, `id` = ULID/UUIDv7 as `TEXT`, `created_at`/`updated_at` timestamptz.

- **users**: id, email (unique), password_hash (argon2id), display_name, created_at
- **sessions**: id, user_id, token_hash, expires_at, user_agent, ip
- **workspaces**: id, slug (unique), name, owner_user_id, created_at
- **workspace_members**: workspace_id, user_id, role (`owner|admin|member|viewer`), joined_at — PK (workspace_id, user_id)
- **invites**: id, workspace_id, email, role, token_hash, expires_at, accepted_by, accepted_at
- **credentials**: id, workspace_id, kind (`hetzner_api_token|github_pat|registry`), name, encrypted_blob (AES-GCM, key from env), last_used_at
- **ssh_keys**: id, workspace_id, name, public_key, private_key_encrypted (nullable — can also reference existing Hetzner SSH key by id), hetzner_key_id
- **projects**: id, workspace_id, slug, name, created_at — unique (workspace_id, slug)
- **services**: id, project_id, slug, source (`image|git`), image_ref (nullable), git_repo (nullable), git_branch, git_commit, dockerfile_path, env_vars (jsonb), ports (jsonb), resources (jsonb: cpu, memory_mb, disk_mb), replicas, restart_policy, created_at — unique (project_id, slug)
- **service_versions**: id, service_id, number, image_digest, build_log_id, created_at
- **deployments**: id, service_id, version_id, node_id (nullable until placed), status (`pending|building|placing|pulling|starting|running|failing|stopped|errored`), container_id, started_at, stopped_at, reason
- **nodes**: id, workspace_id, name, hetzner_server_id, server_type, location, public_ipv4, private_ipv4, status (`provisioning|ready|draining|terminated`), total_cpu, total_memory_mb, total_disk_mb, labels (jsonb), last_seen_at, agent_version
- **node_allocations**: node_id, deployment_id, cpu, memory_mb, disk_mb — PK (node_id, deployment_id)
- **volumes**: id, workspace_id, name, hetzner_volume_id, size_gb, attached_node_id, mount_path, created_at
- **events**: id, workspace_id, kind, subject_id, payload (jsonb), created_at — append-only audit
- **builds**: id, service_id, source_ref, status, log_ref, started_at, finished_at

Indexes on FKs, `(workspace_id, created_at desc)`, `sessions.token_hash`, `invites.token_hash`.

---

## 4. Authentication & multi-tenancy

- **Session auth** via httpOnly `__Host-driftbase_session` cookie, sliding 30-day expiry; store `token_hash` (SHA-256) not raw token.
- **Password**: argon2id with id+salt per user. Email/password only at v1; OAuth later.
- **Workspaces**: every request scoped via `X-Workspace-Slug` header or `/w/:slug/...` URL segment. Middleware resolves `(user, workspace)` → `WorkspaceMember` and attaches role.
- **RBAC roles**:
  - `owner`: everything, billing, delete workspace
  - `admin`: manage members, credentials, nodes
  - `member`: create projects, services, deploy
  - `viewer`: read-only
- **Invites**: owner/admin generates a signed token link (`/invite/:token`). Unauth users sign up via invite; auth users with matching email auto-accept.
- **CSRF**: `SameSite=Lax` + double-submit token for state-changing requests from browser origin.
- **Credential storage**: AES-256-GCM with a master key from `DRIFTBASE_MASTER_KEY` env; encrypt-at-rest in DB; decrypt only in memory during use.

---

## 5. Hetzner integration

Own typed client in `crates/hetzner` (reqwest + serde). Scope to the endpoints we actually need:

- `GET /servers`, `POST /servers` (create), `DELETE /servers/:id`
- `GET /server_types`, `GET /locations`, `GET /images`
- `GET /ssh_keys`, `POST /ssh_keys`
- `POST /networks`, attach
- `POST /volumes`, `POST /volumes/:id/actions/attach`, detach, delete
- Action polling: `GET /actions/:id` until `status=success`

**Provisioning flow**:
1. Pick cheapest `server_type` that fits requested resources (`cx22`, `cpx11`, etc.) + region from workspace default.
2. Create server with:
   - Selected SSH key (from workspace `ssh_keys`, uploaded to Hetzner if not present).
   - `user_data` cloud-init that:
     - Installs Docker (`curl -fsSL get.docker.com | sh`)
     - Pulls `driftbase-agent` from our public release URL
     - Drops a systemd unit with `CONTROL_PLANE_URL`, `NODE_BOOTSTRAP_TOKEN` (one-shot, signed JWT), `WORKSPACE_ID` baked in
     - Starts `driftbase-agent.service`
3. Mark node `provisioning` in DB; wait for agent to call `POST /agent/register` with the bootstrap token.
4. On registration, upgrade to `ready` and start accepting deployments.
5. All nodes placed inside a per-workspace private Hetzner network; public IP only needed for ingress nodes (later: dedicated ingress node pool).

Rate limits: Hetzner is ~3600 req/hr per token — wrap client with governor + exponential backoff on 429.

---

## 6. Node agent protocol

- Connection: agent dials control plane over **TLS WebSocket**, carrying `Authorization: Bearer <node_token>` (issued at register; rotated weekly).
- Framed newline-delimited JSON messages (simple, debuggable); bincode later if perf matters.
- **Agent → CP** messages: `Heartbeat {cpu,mem,disk,load}`, `DeploymentStatus`, `LogChunk`, `BuildStatus`, `Ack`.
- **CP → Agent** commands: `PullAndRun {image, digest, env, ports, mounts, resources, deployment_id}`, `Stop {deployment_id}`, `Restart`, `Prune`, `AttachVolume`, `ReloadProxy`, `UpgradeAgent`.
- Commands are **idempotent** and carry `command_id`; agent replies with `Ack{command_id, result}`. CP retries until ack.
- Agent keeps local SQLite (`bun:sqlite`-equivalent `rusqlite`) of desired state; reconciles every 10s against Docker.
- Logs: Docker `logs --follow` piped through a ring buffer; streamed to CP on demand via SSE tunnel (control plane proxies to browser).

---

## 7. Scheduler & autoscaler

Placement is online bin-packing by resource vector `(cpu_millis, memory_mb, disk_mb)`.

**Place(deployment)**:
1. Filter nodes by `workspace_id`, `status=ready`, matching labels/region constraints.
2. Compute free = `total - Σ allocations`.
3. First-fit-decreasing by memory; tiebreak on highest utilization (pack tight → easier to drain).
4. If no node fits → enqueue `ProvisionRequest {required_resources}` and mark deployment `placing`.
5. Provisioner picks smallest server type that fits + 20% headroom, creates node, waits for `ready`, then retries placement.

**Autoscale-down** (cron every 60s):
- For each node with `allocations == 0` for > `idle_ttl` (default 10 min) AND `status=ready`: mark `draining`, wait grace window, call Hetzner delete, mark `terminated`.
- Never scale down a node flagged `persistent=true` (manual pin) or hosting a volume that can't migrate.

**Drain**: mark node `draining`, scheduler stops placing new work there, existing deployments are migrated to another node (pull new image on target, stop old once target passes health), then node terminated.

**Safety**:
- Per-workspace caps: `max_nodes`, `max_monthly_euro_estimate`.
- Dry-run endpoint `POST /scheduler/plan` returns what would happen without executing.
- All provisioner actions append to `events` table.

---

## 8. Builds (Git → image)

- Dedicated **builder pool**: 1+ Hetzner nodes labeled `role=builder` running the same agent plus BuildKit (`docker buildx`).
- On `deploy` for a git service:
  1. Create `build` row, status `queued`.
  2. Scheduler picks a builder node; sends `Build` command with repo URL + commit + Dockerfile path (or auto-detect via Nixpacks-style heuristics later).
  3. Agent runs `git clone --depth 1`, `docker buildx build --push -t registry.driftbase.internal/<workspace>/<service>:<sha>`.
  4. Streams logs back; on success stores image digest → new `service_version`.
  5. Triggers standard deploy flow with that version.

v1 accepts `Dockerfile` only. Nixpacks/Buildpacks are a v2 feature behind a `builder_kind` column.

---

## 9. Registry

v1: self-host `distribution/distribution` (CNCF registry) on a dedicated small node, backed by **Hetzner Object Storage** (S3-compatible) so we don't lose images when nodes die.

- Auth: registry uses token auth; control plane is the token issuer (JWT signed with registry's public key).
- Image naming: `registry.driftbase.<tld>/<workspace_slug>/<service_slug>:<commit_or_tag>`.
- Garbage collection: weekly job deletes untagged blobs older than 14 days.

v2 option: allow BYO registry (ghcr.io, Docker Hub) via stored registry credentials.

---

## 10. Networking & ingress

- Each node gets Caddy running in a container, managed via Caddy Admin API by the agent.
- Service exposes ports; control plane assigns each deployment a stable hostname `<service>-<project>-<workspace>.driftbase.app` (wildcard DNS → ingress node).
- **Ingress** v1: a designated node per workspace holds the wildcard cert (Let's Encrypt via Caddy) and proxies to backend container IPs inside the private Hetzner network.
- v2: multiple ingress nodes behind Hetzner LB.

---

## 11. Volumes (v2, scaffolded in v1)

- UI/API to create a volume (size, name) → Hetzner volume created unattached.
- On deploy, a volume can be bound to a service at a mount path.
- Scheduler constraint: a deployment bound to a volume must be placed on the node in the volume's region; the agent attaches the Hetzner volume before starting the container.
- Single-writer only (Hetzner volumes aren't RWX). UI warns if `replicas>1` + volume.

---

## 12. HTTP API surface (v1)

All under `/api/v1`. JSON. Errors: `{error:{code,message}}` with stable codes.

**Auth**
- `POST /auth/signup` `{email, password, invite_token?}`
- `POST /auth/login` → sets cookie
- `POST /auth/logout`
- `GET /auth/me`

**Workspaces**
- `POST /workspaces`, `GET /workspaces`, `GET /workspaces/:slug`
- `POST /workspaces/:slug/invites`, `GET ../invites`, `DELETE ../invites/:id`
- `POST /invites/:token/accept`
- `GET /workspaces/:slug/members`, `PATCH .../members/:id`, `DELETE .../members/:id`

**Credentials**
- `POST /workspaces/:slug/credentials` (hetzner token, registry creds, etc.)
- `GET .../credentials` (redacted)
- `DELETE .../credentials/:id`
- `POST /workspaces/:slug/ssh-keys`, `GET`, `DELETE`

**Projects / Services**
- `POST /workspaces/:slug/projects`, `GET`, `DELETE`
- `POST /projects/:id/services`, `GET`, `PATCH`, `DELETE`
- `POST /services/:id/deploy` → creates deployment (and build if git)
- `GET /services/:id/deployments`
- `GET /deployments/:id`
- `POST /deployments/:id/stop`, `.../restart`
- `GET /deployments/:id/logs` (SSE)
- `GET /deployments/:id/events` (SSE)

**Nodes**
- `GET /workspaces/:slug/nodes`
- `POST /workspaces/:slug/nodes/:id/drain`
- `DELETE /workspaces/:slug/nodes/:id` (explicit delete, refuses if busy unless `force=true`)

**Internal (agent)**
- `POST /agent/register` (bootstrap)
- `GET  /agent/ws` (WebSocket, node token)

---

## 13. Frontend (Vite 8 + React + TanStack)

Stack: Vite 8, React 19 TS, TanStack Router file-based, TanStack Query, Tailwind v4, Radix primitives (headless), lucide-react icons, zod for schemas.

**Route tree** (`web/src/routes/`):
```
__root.tsx                     # shell, theme, auth gate
index.tsx                      # redirects to /w/:slug or /login
login.tsx
signup.tsx
invite.$token.tsx
w/$workspaceSlug/
  __layout.tsx                 # sidebar + header
  index.tsx                    # dashboard
  projects/
    index.tsx                  # list
    $projectSlug/
      index.tsx                # services list
      services/$serviceSlug/
        index.tsx              # overview + deploy button
        deployments.tsx
        logs.tsx               # SSE live tail
        settings.tsx
        env.tsx
  nodes.tsx
  volumes.tsx
  credentials.tsx
  ssh-keys.tsx
  members.tsx
  settings.tsx
```

**Design**:
- Tailwind v4 with `@import "tailwindcss"` and `@theme` block in `styles/tailwind.css` for color tokens.
- Dark mode via `class="dark"` on `<html>`, toggle persisted in `localStorage`, `prefers-color-scheme` fallback.
- Minimal: subtle borders (`border-white/10` on dark), `bg-neutral-950` / `bg-neutral-50`, mono for identifiers, generous whitespace. Primary accent: a single restrained color (`emerald-500` candidate).
- Components: `Button`, `Input`, `Select`, `Dialog`, `Sheet`, `Badge`, `Tabs`, `Table` (dense), `CodeBlock`, `LogStream`, `StatusDot`.
- Query client: one `QueryClient`, staleTime 15s default; mutations invalidate by key prefix (`['workspace', slug, ...]`).
- API client: typed `fetch` wrapper that auto-injects workspace slug header, handles `401`→redirect, `403`→toast.
- SSE helper hook `useEventStream(url)` for logs/events.

---

## 14. Observability

- `tracing` + `tracing-subscriber` JSON logs; `opentelemetry` OTLP exporter optional (env toggle).
- Request id middleware; propagate to agent commands.
- `/metrics` Prometheus endpoint on both control plane and agent.
- Event log in DB powers per-workspace activity timeline in UI.

---

## 15. Security

- All agent↔CP traffic TLS with CP cert pinned in agent binary.
- Node tokens short-lived (24h) + refresh over the same WS.
- Master key for at-rest encryption required at boot; refuse to start without it.
- SQL via sqlx compile-time-checked queries; no string-concat SQL.
- Rate limit auth endpoints (IP + account) — `tower_governor`.
- Audit log for: login, invite accept, credential create/delete, node provision/delete, service deploy.
- CSP on frontend: `default-src 'self'` + explicit api origin.

---

## 16. Local dev

- `docker compose up postgres` for DB.
- `cargo watch -x 'run -p controlplane'` for API.
- Mock Hetzner behind a trait `CloudProvider` with a `LocalDocker` impl that spins local Docker containers instead of Hetzner servers — lets you develop scheduler without burning euros.
- `bun run dev` in `web/` with Vite proxy `/api` → `http://localhost:8080`.

---

## 17. Implementation phases

### Phase 0 — Scaffold
- [x] Cargo workspace with `controlplane`, `agent`, `proto`, `hetzner`, `common`
- [x] Frontend scaffold (Vite 8, TanStack Router file-based, Query, Tailwind v4, dark mode toggle)
- [x] CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `bun run build`, `bun test`
- [x] Docker images for controlplane & agent; release workflow publishes agent binary to GitHub Releases

### Phase 1 — Auth & workspaces
- [x] Postgres schema + sqlx migrations for users, sessions, workspaces, members, invites
- [x] Signup, login, logout, me
- [x] Workspace CRUD, invite flow
- [x] RBAC middleware
- [x] Frontend: login/signup/invite pages, workspace switcher, member management

### Phase 2 — Credentials & SSH keys
- [x] AES-GCM crypto module with key loading
- [x] Hetzner API token + SSH key CRUD
- [x] Validate token on save (call `GET /servers` with it)
- [x] UI forms with masked reveal

### Phase 3 — Projects, services, manual image deploys
- [x] Projects + services (image-only) CRUD
- [x] `deployments` table, state machine
- [x] `CloudProvider` trait + `LocalDocker` impl
- [x] Agent MVP: register, heartbeat, pull+run, stop, status _(shipped in Phase 4 as an HTTP-polling agent)_
- [x] Scheduler MVP: place on any ready node, no autoscale
- [x] Deploy button end-to-end against LocalDocker
- [x] Log streaming (SSE)

### Phase 4 — Hetzner provisioner + autoscale
- [x] `HetznerCloudProvider` impl _(implemented as scheduler branching on `node.provider` + `agent_commands` queue — simpler than a second CloudProvider trait impl with different semantics)_
- [x] cloud-init template, bootstrap token _(HMAC-signed opaque tokens; cloud-init installs docker + agent + systemd unit)_
- [x] Provision → register → ready flow _(provisioner inserts `provisioning` row, Hetzner create_server with cloud-init, agent registers with bootstrap token, status flips to `ready`)_
- [x] Bin-packing scheduler, autoscale-down cron _(first-fit-decreasing by free memory; 60s autoscale-down loop respects per-workspace `autoscale_idle_ttl_seconds` and `persistent` flag)_
- [x] Node list UI, drain action _(drain + delete buttons on `/nodes`; Hetzner delete terminates the VM)_
- [x] Per-workspace caps _(`max_nodes`, `max_monthly_euro`, default location/server type; editable in Settings)_

**Agent protocol note**: Phase 4 ships an HTTP-polling agent (register → heartbeat every 10s → executes commands → posts status). WebSocket upgrade deferred to a later polish phase.

### Phase 5 — Registry + git builds
- [x] Self-hosted registry deployment playbook _(distribution:2 service added to docker-compose.yml behind Caddy at `REGISTRY_SITE`, htpasswd basic auth; README/.env.example document the generation step)_
- [x] Builder pool labels, build agent commands _(new `build` command kind + scheduler `tick_builds` + `pick_builder_node` that prefers nodes labeled `role=builder` and falls back to any ready node)_
- [x] Git service type, webhook-less manual deploy first _(services.source = 'image' | 'git'; deploy endpoint creates a `builds` row + deployment in 'building' state; on success the scheduler dispatches `pull_and_run` using the pushed image tag)_
- [x] Build logs streaming _(agent streams buildx stdout/stderr as `[build:<tag>] …` lines through the existing `/agent/deployments/:id/logs` → `deployment_logs` → SSE path, so the Logs tab shows build then runtime in one stream)_

### Phase 6 — Ingress + domains
- [ ] Caddy on each node managed via admin API
- [ ] Wildcard DNS + ingress node
- [ ] Per-deployment hostnames, custom domains (later)

### Phase 7 — Volumes
- [ ] Volume CRUD (Hetzner volumes)
- [ ] Scheduler constraints (region pinning)
- [ ] Mount in deploy command

### Phase 8 — Polish
- [ ] Prometheus metrics, dashboards
- [ ] Audit log UI
- [ ] Dry-run scheduler endpoint + "what will happen" preview modal
- [ ] Cost estimator per workspace

---

## 18. Open decisions (flag before building)

1. **Agent transport**: WebSocket+JSON (simple) vs gRPC (structured). Recommend WS+JSON for v1; switch to gRPC only if perf/shape issues appear.
2. **Primary key type**: ULID (sortable, human-ish) vs UUIDv7. Recommend ULID stored as `TEXT`.
3. **Builder placement**: dedicated builder pool vs build-on-first-target-node. Dedicated is cleaner and lets us reuse layer cache.
4. **DNS**: do we own `driftbase.app` and run wildcard for users, or require BYO domain? Wildcard on our domain for v1, BYO in v2.
5. **Billing/cost caps**: hard stop at cap vs. warn only? Recommend hard stop to avoid runaway Hetzner bills.
6. **Single-region v1**? Recommend yes — user picks one Hetzner location per workspace; multi-region is v2.
