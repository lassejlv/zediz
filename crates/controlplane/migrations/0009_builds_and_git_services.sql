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
