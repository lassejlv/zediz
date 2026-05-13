use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(InitialSchema),
            Box::new(AddManagedRuntimeSchema),
            Box::new(AddGitHubAppBuildsSchema),
            Box::new(RepairGitHubAppBuildsSchema),
        ]
    }
}

#[derive(DeriveMigrationName)]
struct InitialSchema;

#[async_trait::async_trait]
impl MigrationTrait for InitialSchema {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let schema = strip_line_comments(include_str!("migration/initial_schema.sql"));
        for statement in split_sql_statements(&schema) {
            db.execute_unprepared(statement).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for statement in split_sql_statements(DOWN_SQL) {
            db.execute_unprepared(statement).await?;
        }
        Ok(())
    }
}

#[derive(DeriveMigrationName)]
struct AddManagedRuntimeSchema;

#[async_trait::async_trait]
impl MigrationTrait for AddManagedRuntimeSchema {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for statement in split_sql_statements(ADD_MANAGED_RUNTIME_SQL) {
            db.execute_unprepared(statement).await?;
        }
        Ok(())
    }
}

#[derive(DeriveMigrationName)]
struct AddGitHubAppBuildsSchema;

#[async_trait::async_trait]
impl MigrationTrait for AddGitHubAppBuildsSchema {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for statement in split_sql_statements(ADD_GITHUB_APP_BUILDS_SQL) {
            db.execute_unprepared(statement).await?;
        }
        Ok(())
    }
}

#[derive(DeriveMigrationName)]
struct RepairGitHubAppBuildsSchema;

#[async_trait::async_trait]
impl MigrationTrait for RepairGitHubAppBuildsSchema {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for statement in split_sql_statements(ADD_GITHUB_APP_BUILDS_SQL) {
            db.execute_unprepared(statement).await?;
        }
        Ok(())
    }
}

fn split_sql_statements(sql: &str) -> impl Iterator<Item = &str> {
    sql.split(';')
        .map(str::trim)
        .filter(|stmt| !stmt.is_empty())
}

fn strip_line_comments(sql: &str) -> String {
    let mut stripped = String::with_capacity(sql.len());
    for line in sql.lines() {
        let line = line.split_once("--").map_or(line, |(before, _)| before);
        stripped.push_str(line);
        stripped.push('\n');
    }
    stripped
}

const DOWN_SQL: &str = r#"
DROP TABLE IF EXISTS project_network_node_subnets CASCADE;
DROP TABLE IF EXISTS project_networks CASCADE;
DROP TABLE IF EXISTS deployment_metrics CASCADE;
DROP TABLE IF EXISTS volumes CASCADE;
DROP TABLE IF EXISTS builds CASCADE;
DROP TABLE IF EXISTS service_domains CASCADE;
DROP TABLE IF EXISTS deployment_logs CASCADE;
DROP TABLE IF EXISTS agent_commands CASCADE;
DROP TABLE IF EXISTS node_allocations CASCADE;
DROP TABLE IF EXISTS deployments CASCADE;
DROP TABLE IF EXISTS nodes CASCADE;
DROP TABLE IF EXISTS services CASCADE;
DROP TABLE IF EXISTS projects CASCADE;
DROP TABLE IF EXISTS ssh_keys CASCADE;
DROP TABLE IF EXISTS credentials CASCADE;
DROP TABLE IF EXISTS invites CASCADE;
DROP TABLE IF EXISTS workspace_members CASCADE;
DROP TABLE IF EXISTS workspaces CASCADE;
DROP TABLE IF EXISTS sessions CASCADE;
DROP TABLE IF EXISTS users CASCADE;
DROP EXTENSION IF EXISTS citext;
"#;

const ADD_MANAGED_RUNTIME_SQL: &str = r#"
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS private_network_capable BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS wireguard_public_key TEXT;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS wireguard_mesh_ip TEXT;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS wireguard_listen_port INTEGER NOT NULL DEFAULT 51820;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS private_network_synced_at TIMESTAMPTZ;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS private_network_sync_error TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS nodes_wireguard_mesh_ip_unique
    ON nodes (wireguard_mesh_ip)
    WHERE wireguard_mesh_ip IS NOT NULL;

CREATE TABLE IF NOT EXISTS project_networks (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL UNIQUE REFERENCES projects(id) ON DELETE CASCADE,
    cidr        TEXT NOT NULL UNIQUE,
    domain      TEXT NOT NULL DEFAULT 'driftbase.internal',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS project_network_node_subnets (
    project_network_id TEXT NOT NULL REFERENCES project_networks(id) ON DELETE CASCADE,
    node_id            TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    cidr               TEXT NOT NULL UNIQUE,
    gateway_ip         TEXT NOT NULL,
    dns_ip             TEXT NOT NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_network_id, node_id)
);

CREATE INDEX IF NOT EXISTS project_network_node_subnets_node_idx
    ON project_network_node_subnets (node_id);

ALTER TABLE deployments ADD COLUMN IF NOT EXISTS private_ipv4 TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS deployments_private_ipv4_unique
    ON deployments (private_ipv4)
    WHERE private_ipv4 IS NOT NULL;

ALTER TABLE projects ADD COLUMN IF NOT EXISTS hetzner_location TEXT NOT NULL DEFAULT 'nbg1';

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

ALTER TABLE nodes ADD COLUMN IF NOT EXISTS agent_image_ref TEXT;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS agent_image_digest TEXT;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS agent_self_update_capable BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS agent_update_status TEXT NOT NULL DEFAULT 'unknown';
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS agent_update_checked_at TIMESTAMPTZ;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS agent_update_target_image_ref TEXT;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS agent_update_target_digest TEXT;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS agent_update_command_id TEXT REFERENCES agent_commands(id) ON DELETE SET NULL;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS agent_update_error TEXT;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS agent_update_started_at TIMESTAMPTZ;
ALTER TABLE nodes ADD COLUMN IF NOT EXISTS agent_update_finished_at TIMESTAMPTZ;

ALTER TABLE agent_commands DROP CONSTRAINT IF EXISTS agent_commands_kind_check;
ALTER TABLE agent_commands ADD CONSTRAINT agent_commands_kind_check
    CHECK (kind IN (
        'pull_and_run','stop','restart','remove',
        'drain','prune','update_routes','build','sync_private_network',
        'update_agent'
    ));
"#;

const ADD_GITHUB_APP_BUILDS_SQL: &str = r#"
ALTER TABLE services ADD COLUMN IF NOT EXISTS github_installation_id BIGINT;
ALTER TABLE services ADD COLUMN IF NOT EXISTS github_repository_id BIGINT;
ALTER TABLE services ADD COLUMN IF NOT EXISTS github_repository_full_name TEXT;
ALTER TABLE services ADD COLUMN IF NOT EXISTS github_auto_deploy BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE services ADD COLUMN IF NOT EXISTS github_statuses_enabled BOOLEAN NOT NULL DEFAULT TRUE;

ALTER TABLE builds ADD COLUMN IF NOT EXISTS trigger_kind TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE builds ADD COLUMN IF NOT EXISTS git_ref TEXT;
ALTER TABLE builds ADD COLUMN IF NOT EXISTS git_sha TEXT;
ALTER TABLE builds ADD COLUMN IF NOT EXISTS github_delivery_id TEXT;

ALTER TABLE builds DROP CONSTRAINT IF EXISTS builds_trigger_kind_check;
ALTER TABLE builds ADD CONSTRAINT builds_trigger_kind_check
    CHECK (trigger_kind IN ('manual','github_push'));

CREATE INDEX IF NOT EXISTS builds_github_delivery_idx
    ON builds(github_delivery_id)
    WHERE github_delivery_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS github_installations (
    id                    TEXT PRIMARY KEY,
    workspace_id          TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    installation_id       BIGINT NOT NULL,
    account_login         TEXT NOT NULL,
    account_id            BIGINT NOT NULL,
    account_type          TEXT NOT NULL,
    repository_selection  TEXT NOT NULL,
    permissions           JSONB NOT NULL DEFAULT '{}'::jsonb,
    events                JSONB NOT NULL DEFAULT '[]'::jsonb,
    html_url              TEXT,
    active                BOOLEAN NOT NULL DEFAULT TRUE,
    suspended_at          TIMESTAMPTZ,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, installation_id)
);

CREATE INDEX IF NOT EXISTS github_installations_workspace_idx
    ON github_installations(workspace_id);
CREATE INDEX IF NOT EXISTS github_installations_installation_idx
    ON github_installations(installation_id);

CREATE TABLE IF NOT EXISTS github_repositories (
    workspace_id          TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    installation_id       BIGINT NOT NULL,
    repository_id         BIGINT NOT NULL,
    full_name             TEXT NOT NULL,
    private               BOOLEAN NOT NULL DEFAULT FALSE,
    default_branch        TEXT NOT NULL DEFAULT 'main',
    clone_url             TEXT NOT NULL,
    html_url              TEXT NOT NULL,
    archived              BOOLEAN NOT NULL DEFAULT FALSE,
    disabled              BOOLEAN NOT NULL DEFAULT FALSE,
    permissions           JSONB NOT NULL DEFAULT '{}'::jsonb,
    pushed_at             TIMESTAMPTZ,
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, installation_id, repository_id)
);

CREATE INDEX IF NOT EXISTS github_repositories_workspace_idx
    ON github_repositories(workspace_id);
CREATE INDEX IF NOT EXISTS github_repositories_installation_repo_idx
    ON github_repositories(installation_id, repository_id);

CREATE TABLE IF NOT EXISTS github_webhook_deliveries (
    delivery_id           TEXT PRIMARY KEY,
    event                 TEXT NOT NULL,
    installation_id       BIGINT,
    status                TEXT NOT NULL CHECK (status IN ('processing','processed','ignored','failed')),
    error                 TEXT,
    received_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at          TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS github_webhook_deliveries_installation_idx
    ON github_webhook_deliveries(installation_id);
"#;
