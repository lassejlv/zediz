-- Hetzner block volumes attachable to at most one service in a workspace.
-- PLAN_ZEDIZ.md §11. One-volume-per-service invariant enforced by the
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
