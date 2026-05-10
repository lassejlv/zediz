
-- === crates/controlplane/migrations/0001_init.sql ===
-- Extensions
CREATE EXTENSION IF NOT EXISTS citext;

-- Users
CREATE TABLE users (
    id              TEXT PRIMARY KEY,
    email           CITEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Sessions
CREATE TABLE sessions (
    id              TEXT PRIMARY KEY,
    user_id         TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      BYTEA NOT NULL UNIQUE,
    user_agent      TEXT,
    ip              TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL,
    revoked_at      TIMESTAMPTZ
);
CREATE INDEX sessions_user_id_idx ON sessions(user_id);
CREATE INDEX sessions_expires_at_idx ON sessions(expires_at);

-- Workspaces
CREATE TABLE workspaces (
    id              TEXT PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    owner_user_id   TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX workspaces_owner_idx ON workspaces(owner_user_id);

-- Workspace membership with roles
CREATE TABLE workspace_members (
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id         TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX workspace_members_user_idx ON workspace_members(user_id);

-- Invites
CREATE TABLE invites (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    email           CITEXT NOT NULL,
    role            TEXT NOT NULL CHECK (role IN ('admin', 'member', 'viewer')),
    token_hash      BYTEA NOT NULL UNIQUE,
    invited_by      TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL,
    accepted_by     TEXT REFERENCES users(id) ON DELETE SET NULL,
    accepted_at     TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ
);
CREATE INDEX invites_workspace_idx ON invites(workspace_id);
CREATE UNIQUE INDEX invites_pending_unique
    ON invites(workspace_id, email)
    WHERE accepted_at IS NULL AND revoked_at IS NULL;

-- === crates/controlplane/migrations/0002_credentials_and_ssh_keys.sql ===
-- Credentials (encrypted-at-rest secrets scoped to a workspace)
CREATE TABLE credentials (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK (kind IN ('hetzner_api_token', 'github_pat', 'registry')),
    name            TEXT NOT NULL,
    encrypted       BYTEA NOT NULL,
    metadata        JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_by      TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at    TIMESTAMPTZ,
    UNIQUE (workspace_id, kind, name)
);
CREATE INDEX credentials_workspace_idx ON credentials(workspace_id);

-- SSH keys (public key + OpenSSH SHA256 fingerprint, optional encrypted private key, optional Hetzner key id)
CREATE TABLE ssh_keys (
    id                    TEXT PRIMARY KEY,
    workspace_id          TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name                  TEXT NOT NULL,
    public_key            TEXT NOT NULL,
    fingerprint           TEXT NOT NULL,
    private_key_encrypted BYTEA,
    hetzner_key_id        BIGINT,
    created_by            TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name)
);
CREATE INDEX ssh_keys_workspace_idx ON ssh_keys(workspace_id);
CREATE INDEX ssh_keys_fingerprint_idx ON ssh_keys(fingerprint);

-- === crates/controlplane/migrations/0003_projects_services_deployments.sql ===
-- Projects
CREATE TABLE projects (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL,
    name            TEXT NOT NULL,
    created_by      TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, slug)
);
CREATE INDEX projects_workspace_idx ON projects(workspace_id);

-- Services (image source only in phase 3; git arrives in later phases)
CREATE TABLE services (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL,
    name            TEXT NOT NULL,
    source          TEXT NOT NULL CHECK (source IN ('image')),
    image_ref       TEXT,
    env_vars        JSONB NOT NULL DEFAULT '{}'::jsonb,
    ports           JSONB NOT NULL DEFAULT '[]'::jsonb,
    resources       JSONB NOT NULL DEFAULT '{"cpu_millis":500,"memory_mb":256,"disk_mb":1024}'::jsonb,
    replicas        INTEGER NOT NULL DEFAULT 1 CHECK (replicas >= 1),
    restart_policy  TEXT NOT NULL DEFAULT 'on-failure' CHECK (restart_policy IN ('no', 'on-failure', 'always')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, slug)
);
CREATE INDEX services_project_idx ON services(project_id);

-- Nodes (can be local-docker during dev, or hetzner-provisioned later)
CREATE TABLE nodes (
    id                TEXT PRIMARY KEY,
    workspace_id      TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    provider          TEXT NOT NULL CHECK (provider IN ('local_docker', 'hetzner')),
    provider_node_id  TEXT,
    status            TEXT NOT NULL CHECK (status IN ('provisioning', 'ready', 'draining', 'terminated')),
    total_cpu_millis  INTEGER NOT NULL,
    total_memory_mb   INTEGER NOT NULL,
    total_disk_mb     INTEGER NOT NULL,
    labels            JSONB NOT NULL DEFAULT '{}'::jsonb,
    agent_version     TEXT,
    last_seen_at      TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name)
);
CREATE INDEX nodes_workspace_idx ON nodes(workspace_id);
CREATE INDEX nodes_status_idx ON nodes(status);

-- Deployments (one row per service instance lifetime)
CREATE TABLE deployments (
    id              TEXT PRIMARY KEY,
    service_id      TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    node_id         TEXT REFERENCES nodes(id) ON DELETE SET NULL,
    status          TEXT NOT NULL CHECK (status IN (
        'pending', 'placing', 'pulling', 'starting', 'running', 'failing', 'stopped', 'errored'
    )),
    image_ref       TEXT NOT NULL,
    env_vars        JSONB NOT NULL DEFAULT '{}'::jsonb,
    ports           JSONB NOT NULL DEFAULT '[]'::jsonb,
    resources       JSONB NOT NULL,
    container_id    TEXT,
    reason          TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    stopped_at      TIMESTAMPTZ,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX deployments_service_idx ON deployments(service_id);
CREATE INDEX deployments_node_idx ON deployments(node_id);
CREATE INDEX deployments_status_idx ON deployments(status);

-- Node allocations — current resource reservations per active deployment
CREATE TABLE node_allocations (
    node_id         TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    deployment_id   TEXT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    cpu_millis      INTEGER NOT NULL,
    memory_mb       INTEGER NOT NULL,
    disk_mb         INTEGER NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (node_id, deployment_id)
);

-- === crates/controlplane/migrations/0004_agent_protocol_and_caps.sql ===
-- Node auth + lifecycle
ALTER TABLE nodes ADD COLUMN bootstrap_token_hash TEXT;
ALTER TABLE nodes ADD COLUMN node_token_hash TEXT;
ALTER TABLE nodes ADD COLUMN hetzner_server_id BIGINT;
ALTER TABLE nodes ADD COLUMN hetzner_location TEXT;
ALTER TABLE nodes ADD COLUMN hetzner_server_type TEXT;
ALTER TABLE nodes ADD COLUMN public_ipv4 TEXT;
ALTER TABLE nodes ADD COLUMN persistent BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE nodes ADD COLUMN idle_since_at TIMESTAMPTZ;
ALTER TABLE nodes ADD COLUMN registered_at TIMESTAMPTZ;

CREATE UNIQUE INDEX nodes_hetzner_server_id_idx
    ON nodes(hetzner_server_id)
    WHERE hetzner_server_id IS NOT NULL;

-- Workspace defaults and caps
ALTER TABLE workspaces ADD COLUMN hetzner_location TEXT NOT NULL DEFAULT 'nbg1';
ALTER TABLE workspaces ADD COLUMN default_server_type TEXT;
ALTER TABLE workspaces ADD COLUMN max_nodes INTEGER NOT NULL DEFAULT 3;
ALTER TABLE workspaces ADD COLUMN max_monthly_euro INTEGER NOT NULL DEFAULT 50;
ALTER TABLE workspaces ADD COLUMN autoscale_idle_ttl_seconds INTEGER NOT NULL DEFAULT 600;

-- Command queue: control plane enqueues, agent polls via heartbeat, acks update.
CREATE TABLE agent_commands (
    id              TEXT PRIMARY KEY,
    node_id         TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    deployment_id   TEXT REFERENCES deployments(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK (kind IN (
        'pull_and_run', 'stop', 'restart', 'remove', 'drain', 'prune'
    )),
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN (
        'pending', 'dispatched', 'acked', 'errored'
    )),
    result          TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    dispatched_at   TIMESTAMPTZ,
    acked_at        TIMESTAMPTZ
);
CREATE INDEX agent_commands_node_status_idx ON agent_commands(node_id, status);
CREATE INDEX agent_commands_deployment_idx ON agent_commands(deployment_id);

-- Deployment log buffer (last N lines; kept bounded by agent-pushing in chunks).
CREATE TABLE deployment_logs (
    id              BIGSERIAL PRIMARY KEY,
    deployment_id   TEXT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    stream          TEXT NOT NULL CHECK (stream IN ('stdout', 'stderr')),
    ts              TIMESTAMPTZ NOT NULL,
    line            TEXT NOT NULL,
    received_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX deployment_logs_deployment_id_idx ON deployment_logs(deployment_id, id DESC);

-- === crates/controlplane/migrations/0005_drop_local_docker_nodes.sql ===
-- Local-docker nodes are no longer auto-bootstrapped. Sweep out any leftover
-- rows from earlier runs so workspaces only surface real (Hetzner) nodes.
DELETE FROM nodes WHERE provider = 'local_docker';

-- === crates/controlplane/migrations/0006_scheduler_pause.sql ===
-- Lets admins pause auto-provisioning for a workspace. Deleting a node
-- automatically sets this to now() + 2 min so the scheduler doesn't immediately
-- replace a just-deleted node while the user is investigating.
ALTER TABLE workspaces
    ADD COLUMN scheduler_paused_until TIMESTAMPTZ;

-- === crates/controlplane/migrations/0007_service_domains.sql ===
-- Custom (BYO) domains attached to services.
-- Each domain maps a public hostname to a (service, container_port). Caddy on
-- the node hosting the deployment issues a Let's Encrypt cert for it.
CREATE TABLE service_domains (
    id              TEXT PRIMARY KEY,
    service_id      TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    hostname        TEXT NOT NULL,
    container_port  INTEGER NOT NULL,
    tls_status      TEXT NOT NULL DEFAULT 'pending' CHECK (tls_status IN (
        'pending', 'active', 'failed'
    )),
    last_error      TEXT,
    last_cert_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (hostname)
);
CREATE INDEX service_domains_service_idx ON service_domains(service_id);

-- === crates/controlplane/migrations/0008_agent_update_routes.sql ===
-- Allow the new `update_routes` command kind so the scheduler can push Caddy
-- config refreshes to nodes when service_domains or placements change.
ALTER TABLE agent_commands DROP CONSTRAINT agent_commands_kind_check;
ALTER TABLE agent_commands ADD CONSTRAINT agent_commands_kind_check
    CHECK (kind IN (
        'pull_and_run', 'stop', 'restart', 'remove',
        'drain', 'prune', 'update_routes'
    ));

-- === crates/controlplane/migrations/0009_builds_and_git_services.sql ===
-- Phase 5: git service source + builds + builder pool.

-- Widen services.source to include git.
ALTER TABLE services DROP CONSTRAINT services_source_check;
ALTER TABLE services ADD CONSTRAINT services_source_check
    CHECK (source IN ('image', 'git'));

-- Git fields on services (all nullable; required only when source='git').
-- image_ref stays nullable for git services until the first successful build
-- writes the pushed tag back.
ALTER TABLE services ADD COLUMN git_repo               TEXT;
ALTER TABLE services ADD COLUMN git_branch             TEXT;
ALTER TABLE services ADD COLUMN git_commit             TEXT;
ALTER TABLE services ADD COLUMN dockerfile_path        TEXT;
ALTER TABLE services ADD COLUMN build_context          TEXT;
ALTER TABLE services ADD COLUMN registry_repo          TEXT;
ALTER TABLE services ADD COLUMN github_credential_id   TEXT REFERENCES credentials(id) ON DELETE SET NULL;
ALTER TABLE services ADD COLUMN registry_credential_id TEXT REFERENCES credentials(id) ON DELETE SET NULL;

-- One row per build attempt. deployment_id is the deployment that triggered
-- the build; the pushed image_tag ends up on that deployment so the normal
-- pull_and_run path can run it.
CREATE TABLE builds (
    id            TEXT PRIMARY KEY,
    service_id    TEXT NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    deployment_id TEXT REFERENCES deployments(id) ON DELETE SET NULL,
    node_id       TEXT REFERENCES nodes(id) ON DELETE SET NULL,
    status        TEXT NOT NULL CHECK (status IN (
        'queued','cloning','building','pushing','succeeded','failed','cancelled'
    )),
    git_commit    TEXT,
    image_digest  TEXT,
    image_tag     TEXT,
    reason        TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at    TIMESTAMPTZ,
    finished_at   TIMESTAMPTZ,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX builds_service_idx    ON builds(service_id);
CREATE INDEX builds_deployment_idx ON builds(deployment_id);
CREATE INDEX builds_status_idx     ON builds(status);

-- Allow the new 'build' command kind.
ALTER TABLE agent_commands DROP CONSTRAINT agent_commands_kind_check;
ALTER TABLE agent_commands ADD CONSTRAINT agent_commands_kind_check
    CHECK (kind IN (
        'pull_and_run','stop','restart','remove',
        'drain','prune','update_routes','build'
    ));

-- 'building' is a new transient status for git-source deployments that are
-- waiting on their build to finish. Only the CP sets it; agents still report
-- the old runtime statuses.
ALTER TABLE deployments DROP CONSTRAINT deployments_status_check;
ALTER TABLE deployments ADD CONSTRAINT deployments_status_check
    CHECK (status IN (
        'pending','building','placing','pulling','starting','running','failing','stopped','errored'
    ));

-- === crates/controlplane/migrations/0010_root_dir_and_railpack.sql ===
-- Phase 5 follow-up: generalise `build_context` to `root_dir` (the subdir
-- within the repo where the service lives, useful for monorepos) and let
-- users pick Railpack as an alternative to a hand-written Dockerfile.

ALTER TABLE services RENAME COLUMN build_context TO root_dir;

ALTER TABLE services ADD COLUMN builder TEXT NOT NULL DEFAULT 'dockerfile'
    CHECK (builder IN ('dockerfile', 'railpack'));

-- === crates/controlplane/migrations/0011_deployment_runtime_metrics.sql ===
-- Latest container metrics sample the agent reported for a deployment.
-- Overwritten on every heartbeat while the container is running. We don't
-- keep history server-side; the UI maintains its own rolling buffer over
-- the tab's lifetime.
ALTER TABLE deployments
    ADD COLUMN IF NOT EXISTS runtime_metrics JSONB;

-- === crates/controlplane/migrations/0012_deployment_metrics_history.sql ===
-- Per-sample history of container stats the agent reports on each
-- heartbeat. Complements deployments.runtime_metrics (which stays as the
-- quick "latest" JSONB for list views) by giving the Metrics tab a real
-- time series it can load on mount and slice by time range.
--
-- Retained for one hour by a scheduler task; CASCADE on deployment delete
-- keeps cleanup automatic when a deployment row goes away.
CREATE TABLE IF NOT EXISTS deployment_metrics (
    deployment_id TEXT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    ts TIMESTAMPTZ NOT NULL,
    cpu_percent REAL NOT NULL,
    memory_bytes BIGINT NOT NULL,
    memory_limit_bytes BIGINT,
    rx_bytes BIGINT NOT NULL,
    tx_bytes BIGINT NOT NULL,
    PRIMARY KEY (deployment_id, ts)
);

CREATE INDEX IF NOT EXISTS idx_deployment_metrics_ts
    ON deployment_metrics (deployment_id, ts DESC);

-- === crates/controlplane/migrations/0013_user_approval.sql ===
-- Waitlist / platform-admin approval for new signups.
--
-- `status` gates login: only 'approved' users can sign in. New signups
-- default to 'pending'; the first user ever to sign up is auto-promoted
-- to 'approved' + platform admin so the instance owner can bootstrap.
--
-- `is_platform_admin` grants access to the /admin endpoints (user
-- approval, future platform-wide settings). Distinct from workspace
-- ownership — platform admin is a property of the self-hosted
-- installation, not of any workspace.

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'pending';

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS is_platform_admin BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE users DROP CONSTRAINT IF EXISTS users_status_check;
ALTER TABLE users
    ADD CONSTRAINT users_status_check
    CHECK (status IN ('pending', 'approved', 'rejected'));

-- Existing installs: approve everyone already here so no one gets
-- locked out, and flag the earliest-registered user as platform admin
-- so they can manage approvals going forward.
UPDATE users SET status = 'approved' WHERE status = 'pending';
UPDATE users
SET is_platform_admin = true
WHERE id = (SELECT id FROM users ORDER BY created_at ASC LIMIT 1);

CREATE INDEX IF NOT EXISTS idx_users_status ON users (status);

-- === crates/controlplane/migrations/0014_volumes.sql ===
-- Hetzner block volumes attachable to at most one service in a workspace.
-- PLAN_DRIFTBASE.md §11. One-volume-per-service invariant enforced by the
-- partial unique index on attached_service_id.
--
-- Lifecycle:
--   creating  → row exists, Hetzner API call in flight
--   available → Hetzner volume exists, not bound to a service
--   attached  → bound to a service (attached_service_id + mount_path set)
--   detaching → mid-detach from a node (transient; set by the scheduler)
--   errored   → Hetzner API returned an error we couldn't recover from

CREATE TABLE IF NOT EXISTS volumes (
    id                  TEXT PRIMARY KEY,
    workspace_id        TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name                TEXT NOT NULL,
    size_gb             INTEGER NOT NULL CHECK (size_gb BETWEEN 10 AND 10240),
    hetzner_volume_id   BIGINT,
    hetzner_location    TEXT NOT NULL,
    attached_node_id    TEXT REFERENCES nodes(id) ON DELETE SET NULL,
    attached_service_id TEXT REFERENCES services(id) ON DELETE SET NULL,
    mount_path          TEXT,
    status              TEXT NOT NULL DEFAULT 'creating'
        CHECK (status IN ('creating','available','attached','detaching','errored')),
    reason              TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name)
);

-- Partial unique: a service can only have one volume attached at a time.
-- Nullable columns can't participate in a regular UNIQUE if multiple
-- unattached volumes should coexist.
CREATE UNIQUE INDEX IF NOT EXISTS volumes_service_unique
    ON volumes (attached_service_id)
    WHERE attached_service_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS volumes_workspace_idx
    ON volumes (workspace_id);

-- === crates/controlplane/migrations/0015_project_private_networks.sql ===
-- Project-scoped private networking. Additive only: existing nodes and
-- deployments keep working until updated agents report mesh capability.

ALTER TABLE nodes ADD COLUMN private_network_capable BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE nodes ADD COLUMN wireguard_public_key TEXT;
ALTER TABLE nodes ADD COLUMN wireguard_mesh_ip TEXT;
ALTER TABLE nodes ADD COLUMN wireguard_listen_port INTEGER NOT NULL DEFAULT 51820;
ALTER TABLE nodes ADD COLUMN private_network_synced_at TIMESTAMPTZ;
ALTER TABLE nodes ADD COLUMN private_network_sync_error TEXT;

CREATE UNIQUE INDEX nodes_wireguard_mesh_ip_unique
    ON nodes (wireguard_mesh_ip)
    WHERE wireguard_mesh_ip IS NOT NULL;

CREATE TABLE project_networks (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL UNIQUE REFERENCES projects(id) ON DELETE CASCADE,
    cidr        TEXT NOT NULL UNIQUE,
    domain      TEXT NOT NULL DEFAULT 'driftbase.internal',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE project_network_node_subnets (
    project_network_id TEXT NOT NULL REFERENCES project_networks(id) ON DELETE CASCADE,
    node_id            TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    cidr               TEXT NOT NULL UNIQUE,
    gateway_ip         TEXT NOT NULL,
    dns_ip             TEXT NOT NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_network_id, node_id)
);

CREATE INDEX project_network_node_subnets_node_idx
    ON project_network_node_subnets (node_id);

ALTER TABLE deployments ADD COLUMN private_ipv4 TEXT;

CREATE UNIQUE INDEX deployments_private_ipv4_unique
    ON deployments (private_ipv4)
    WHERE private_ipv4 IS NOT NULL;

-- Backfill one /16 per existing project. Runtime allocation code uses the
-- same deterministic address pool and will fail clearly if the pool is full.
WITH numbered AS (
    SELECT id, ROW_NUMBER() OVER (ORDER BY created_at ASC, id ASC) - 1 AS idx
    FROM projects
)
INSERT INTO project_networks (id, project_id, cidr)
SELECT
    id,
    id,
    '10.' || (64 + idx)::text || '.0.0/16'
FROM numbered
WHERE idx < 191
ON CONFLICT (project_id) DO NOTHING;

ALTER TABLE agent_commands DROP CONSTRAINT agent_commands_kind_check;
ALTER TABLE agent_commands ADD CONSTRAINT agent_commands_kind_check
    CHECK (kind IN (
        'pull_and_run','stop','restart','remove',
        'drain','prune','update_routes','build','sync_private_network'
    ));

-- === crates/controlplane/migrations/0016_node_agent_updates.sql ===
ALTER TABLE nodes ADD COLUMN agent_image_ref TEXT;
ALTER TABLE nodes ADD COLUMN agent_image_digest TEXT;
ALTER TABLE nodes ADD COLUMN agent_self_update_capable BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE nodes ADD COLUMN agent_update_status TEXT NOT NULL DEFAULT 'unknown';
ALTER TABLE nodes ADD COLUMN agent_update_checked_at TIMESTAMPTZ;
ALTER TABLE nodes ADD COLUMN agent_update_target_image_ref TEXT;
ALTER TABLE nodes ADD COLUMN agent_update_target_digest TEXT;
ALTER TABLE nodes ADD COLUMN agent_update_command_id TEXT REFERENCES agent_commands(id) ON DELETE SET NULL;
ALTER TABLE nodes ADD COLUMN agent_update_error TEXT;
ALTER TABLE nodes ADD COLUMN agent_update_started_at TIMESTAMPTZ;
ALTER TABLE nodes ADD COLUMN agent_update_finished_at TIMESTAMPTZ;

ALTER TABLE agent_commands DROP CONSTRAINT agent_commands_kind_check;
ALTER TABLE agent_commands ADD CONSTRAINT agent_commands_kind_check
    CHECK (kind IN (
        'pull_and_run','stop','restart','remove',
        'drain','prune','update_routes','build','sync_private_network',
        'update_agent'
    ));
