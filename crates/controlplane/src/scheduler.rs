use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value as JsonValue};
use sqlx::PgPool;
use tokio::sync::Notify;

use crate::agent::commands::{self, BuildPayload, CommandKind, RegistryAuth};
use crate::credentials;
use crate::provisioner::hetzner as hetzner_provisioner;
use crate::services::{PortMap, Resources};
use crate::ssh_keys;
use crate::state::AppState;

/// Small envelope reserved per in-flight build so concurrent builds don't
/// starve the node out of capacity for runtime containers.
const BUILD_CPU_MILLIS: u32 = 1000;
const BUILD_MEMORY_MB: u32 = 1024;
const BUILD_DISK_MB: u32 = 2048;

/// Handle used to nudge the scheduler to wake early when new work arrives.
#[derive(Clone, Default)]
pub struct SchedulerHandle {
    notify: Arc<Notify>,
}

impl SchedulerHandle {
    pub fn wake(&self) {
        self.notify.notify_one();
    }

    async fn wait(&self, tick: Duration) {
        tokio::select! {
            _ = tokio::time::sleep(tick) => {}
            _ = self.notify.notified() => {}
        }
    }
}

pub fn nudge(state: &AppState) {
    state.scheduler().wake();
}

pub fn spawn(state: AppState) -> SchedulerHandle {
    let handle = state.scheduler().clone();
    let handle_for_task = handle.clone();
    let state_for_task = state.clone();
    tokio::spawn(async move {
        let tick = Duration::from_secs(2);
        let mut autoscale_counter: u32 = 0;
        let mut tls_probe_counter: u32 = 0;
        let mut reap_counter: u32 = 0;
        let mut metrics_trim_counter: u32 = 0;
        loop {
            if let Err(e) = tick_once(&state_for_task).await {
                tracing::error!(error = ?e, "scheduler tick failed");
            }
            tls_probe_counter = tls_probe_counter.wrapping_add(1);
            if tls_probe_counter.is_multiple_of(5) {
                if let Err(e) = crate::domains::refresh_tls_statuses(state_for_task.pool()).await {
                    tracing::warn!(error = ?e, "tls status refresh failed");
                }
            }
            // Run autoscale-down every ~30 ticks (~60 seconds).
            autoscale_counter = autoscale_counter.wrapping_add(1);
            if autoscale_counter.is_multiple_of(30) {
                if let Err(e) = autoscale_down(&state_for_task).await {
                    tracing::warn!(error = ?e, "autoscale-down failed");
                }
            }
            // Reap deployments stuck in transient states every ~30 ticks
            // (~60 seconds). Catches silent agent stalls so the UI doesn't
            // sit on "pulling image" forever.
            reap_counter = reap_counter.wrapping_add(1);
            if reap_counter.is_multiple_of(30) {
                if let Err(e) = reap_stale_deployments(&state_for_task).await {
                    tracing::warn!(error = ?e, "reap_stale_deployments failed");
                }
            }
            // Prune old deployment_metrics samples every ~150 ticks
            // (~5 minutes). The Metrics tab only loads the last hour so
            // anything older is dead weight.
            metrics_trim_counter = metrics_trim_counter.wrapping_add(1);
            if metrics_trim_counter.is_multiple_of(150) {
                if let Err(e) = trim_metrics_history(&state_for_task).await {
                    tracing::warn!(error = ?e, "trim_metrics_history failed");
                }
            }
            handle_for_task.wait(tick).await;
        }
    });
    handle
}

/// How much deployment_metrics history to keep. Matches what the Metrics
/// tab can ask for and keeps table size bounded.
const METRICS_HISTORY_MINUTES: i64 = 60;

async fn trim_metrics_history(state: &AppState) -> Result<()> {
    sqlx::query("DELETE FROM deployment_metrics WHERE ts < now() - ($1 || ' minutes')::interval")
        .bind(METRICS_HISTORY_MINUTES)
        .execute(state.pool())
        .await?;
    Ok(())
}

/// How long a deployment may sit in `pulling` or `starting` before the
/// scheduler gives up and marks it `errored`. Bigger than the slowest
/// realistic image pull, small enough that users see the failure before
/// they forget they clicked deploy.
const STALE_DEPLOYMENT_MINUTES: i64 = 15;

/// Find deployments that have been in a transient state for too long and
/// mark them `errored`. Happens when the agent's status POST silently
/// fails, the agent crashes mid-pull, or Docker hangs. Releases the
/// allocation, asks the node to tear down any orphaned container, and
/// refreshes Caddy routes so a stuck deployment can't keep claiming a
/// domain.
async fn reap_stale_deployments(state: &AppState) -> Result<()> {
    let rows: Vec<(String, Option<String>, String)> = sqlx::query_as(
        "UPDATE deployments \
         SET status = 'errored', \
             reason = 'stuck in ' || status || ' for over ' || $1::text || ' minutes', \
             stopped_at = now(), \
             updated_at = now() \
         WHERE status IN ('pulling', 'starting') \
           AND updated_at < now() - ($1 || ' minutes')::interval \
         RETURNING id, node_id, service_id",
    )
    .bind(STALE_DEPLOYMENT_MINUTES)
    .fetch_all(state.pool())
    .await?;

    for (deployment_id, node_id, service_id) in rows {
        tracing::warn!(
            deployment = %deployment_id,
            service = %service_id,
            node = ?node_id,
            "reaped deployment stuck in transient state",
        );

        let _ = sqlx::query("DELETE FROM node_allocations WHERE deployment_id = $1")
            .bind(&deployment_id)
            .execute(state.pool())
            .await;

        if let Some(node_id) = node_id {
            let _ = commands::enqueue(
                state.pool(),
                &node_id,
                Some(&deployment_id),
                CommandKind::Remove,
                json!({}),
            )
            .await;
            if let Err(e) = push_routes_for_node(state.pool(), &node_id).await {
                tracing::warn!(error = ?e, node = %node_id, "push_routes_for_node after reap");
            }
        }
    }
    Ok(())
}

async fn tick_once(state: &AppState) -> Result<()> {
    // Kick off queued builds first — their success flips the deployment back
    // to 'pending' and lets the same tick pick it up on the next pass.
    let queued = fetch_queued_builds(state.pool()).await?;
    for b in queued {
        let build_id = b.build_id.clone();
        if let Err(e) = dispatch_build(state, b).await {
            tracing::error!(build = %build_id, error = ?e, "build dispatch failed");
            fail_build(state.pool(), &build_id, &e.to_string()).await?;
        }
    }

    let pending = fetch_pending(state.pool()).await?;
    for p in pending {
        if let Err(e) = place_and_run(state, &p).await {
            tracing::error!(deployment = %p.deployment_id, error = ?e, "placement failed");
            let _ = sqlx::query(
                "UPDATE deployments SET status = 'errored', reason = $1, updated_at = now() \
                 WHERE id = $2",
            )
            .bind(e.to_string())
            .bind(&p.deployment_id)
            .execute(state.pool())
            .await;
        }
    }
    refresh_idle_since(state.pool()).await?;
    Ok(())
}

struct QueuedBuild {
    build_id: String,
    deployment_id: String,
    service_id: String,
    workspace_id: String,
    git_repo: String,
    git_branch: String,
    builder: String,
    dockerfile_path: Option<String>,
    root_dir: String,
    registry_repo: Option<String>,
    github_credential_id: Option<String>,
    registry_credential_id: Option<String>,
}

async fn fetch_queued_builds(pool: &PgPool) -> Result<Vec<QueuedBuild>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        build_id: String,
        deployment_id: Option<String>,
        service_id: String,
        workspace_id: String,
        git_repo: Option<String>,
        git_branch: Option<String>,
        builder: String,
        dockerfile_path: Option<String>,
        root_dir: Option<String>,
        registry_repo: Option<String>,
        github_credential_id: Option<String>,
        registry_credential_id: Option<String>,
    }

    let rows: Vec<Row> = sqlx::query_as(
        "SELECT b.id AS build_id, b.deployment_id, s.id AS service_id, w.id AS workspace_id, \
                s.git_repo, s.git_branch, s.builder, s.dockerfile_path, s.root_dir, \
                s.registry_repo, s.github_credential_id, s.registry_credential_id \
         FROM builds b \
         JOIN services s ON s.id = b.service_id \
         JOIN projects p ON p.id = s.project_id \
         JOIN workspaces w ON w.id = p.workspace_id \
         WHERE b.status = 'queued' \
         ORDER BY b.created_at ASC \
         LIMIT 10",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(deployment_id) = r.deployment_id else {
            continue;
        };
        let Some(git_repo) = r.git_repo else {
            continue;
        };
        out.push(QueuedBuild {
            build_id: r.build_id,
            deployment_id,
            service_id: r.service_id,
            workspace_id: r.workspace_id,
            git_repo,
            git_branch: r.git_branch.unwrap_or_else(|| "main".into()),
            builder: r.builder,
            dockerfile_path: r.dockerfile_path,
            root_dir: r.root_dir.unwrap_or_else(|| ".".into()),
            registry_repo: r.registry_repo,
            github_credential_id: r.github_credential_id,
            registry_credential_id: r.registry_credential_id,
        });
    }
    Ok(out)
}

async fn dispatch_build(state: &AppState, b: QueuedBuild) -> Result<()> {
    let registry_cred_id = b
        .registry_credential_id
        .clone()
        .ok_or_else(|| anyhow!("git service missing registry credential — set one in Settings"))?;
    let registry_cred = credentials::fetch_decrypted(
        state.pool(),
        state.master_key(),
        &b.workspace_id,
        &registry_cred_id,
    )
    .await?
    .ok_or_else(|| anyhow!("registry credential {registry_cred_id} not found"))?;
    if registry_cred.kind != "registry" {
        return Err(anyhow!(
            "credential {registry_cred_id} is not a registry credential"
        ));
    }
    let registry_meta = RegistryMeta::from_metadata(&registry_cred.metadata)?;

    // Derive a repo name if the user didn't set one.
    let registry_repo = b.registry_repo.clone().unwrap_or_else(|| {
        format!(
            "{host}/{ws}/{svc}",
            host = registry_meta.url.to_ascii_lowercase(),
            ws = b.workspace_id.to_ascii_lowercase(),
            svc = b.service_id.to_ascii_lowercase()
        )
    });
    let image_tag = format!("{registry_repo}:build-{id}", id = b.build_id);

    let github_pat = match &b.github_credential_id {
        Some(id) => {
            let cred =
                credentials::fetch_decrypted(state.pool(), state.master_key(), &b.workspace_id, id)
                    .await?
                    .ok_or_else(|| anyhow!("github credential {id} not found"))?;
            if cred.kind != "github_pat" {
                return Err(anyhow!("credential {id} is not a github_pat"));
            }
            Some(cred.secret)
        }
        None => None,
    };

    // Pick a builder node; any ready node works if no explicit builder pool.
    // If nothing's ready, trigger autoscale (same mechanism runtime deployments
    // use) and leave the build queued so the next tick picks it up once the
    // node registers. Builds are NOT marked failed on "no node" — that's a
    // transient condition, not a hard error.
    let node = match pick_builder_node(state.pool(), &b.workspace_id).await? {
        Some(n) => n,
        None => {
            let build_resources = Resources {
                cpu_millis: BUILD_CPU_MILLIS,
                memory_mb: BUILD_MEMORY_MB,
                disk_mb: BUILD_DISK_MB,
            };
            let reason = match try_provision_for(state, &b.workspace_id, &build_resources).await? {
                ProvisionOutcome::Provisioning => {
                    "waiting for provisioned node to register".to_string()
                }
                ProvisionOutcome::Skipped(msg) => msg,
            };
            sqlx::query(
                "UPDATE builds SET reason = $1, updated_at = now() \
                 WHERE id = $2 AND status = 'queued'",
            )
            .bind(&reason)
            .bind(&b.build_id)
            .execute(state.pool())
            .await?;
            return Ok(());
        }
    };

    // Reserve + claim the build.
    let mut tx = state.pool().begin().await?;
    sqlx::query(
        "INSERT INTO node_allocations (node_id, deployment_id, cpu_millis, memory_mb, disk_mb) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (node_id, deployment_id) DO NOTHING",
    )
    .bind(&node.id)
    .bind(&b.deployment_id)
    .bind(BUILD_CPU_MILLIS as i32)
    .bind(BUILD_MEMORY_MB as i32)
    .bind(BUILD_DISK_MB as i32)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE builds SET node_id = $1, status = 'cloning', started_at = now(), \
                            image_tag = $2, updated_at = now() \
         WHERE id = $3 AND status = 'queued'",
    )
    .bind(&node.id)
    .bind(&image_tag)
    .bind(&b.build_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let payload = commands::build_payload(&BuildPayload {
        build_id: &b.build_id,
        deployment_id: &b.deployment_id,
        service_id: &b.service_id,
        git_repo: &b.git_repo,
        git_branch: &b.git_branch,
        builder: &b.builder,
        dockerfile_path: b.dockerfile_path.as_deref(),
        root_dir: &b.root_dir,
        image_tag: &image_tag,
        github_pat: github_pat.as_deref(),
        registry: Some(RegistryAuth {
            url: &registry_meta.url,
            username: &registry_meta.username,
            password: &registry_cred.secret,
        }),
    });

    commands::enqueue(
        state.pool(),
        &node.id,
        Some(&b.deployment_id),
        CommandKind::Build,
        payload,
    )
    .await?;
    Ok(())
}

struct RegistryMeta {
    url: String,
    username: String,
}

impl RegistryMeta {
    fn from_metadata(m: &JsonValue) -> Result<Self> {
        let url = m
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("registry credential metadata missing 'url'"))?;
        let username = m
            .get("username")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("registry credential metadata missing 'username'"))?;
        Ok(Self {
            url: url.to_string(),
            username: username.to_string(),
        })
    }
}

async fn pick_builder_node(pool: &PgPool, workspace_id: &str) -> Result<Option<NodeCapacity>> {
    // If the workspace has any node tagged `role=builder`, prefer those
    // exclusively. Otherwise any ready node will do.
    let rows: Vec<NodeCapacity> = sqlx::query_as(
        "SELECT n.id, n.provider, n.total_cpu_millis, n.total_memory_mb, n.total_disk_mb, \
                COALESCE(SUM(a.cpu_millis), 0)::bigint AS used_cpu_millis, \
                COALESCE(SUM(a.memory_mb), 0)::bigint AS used_memory_mb, \
                COALESCE(SUM(a.disk_mb), 0)::bigint AS used_disk_mb \
         FROM nodes n \
         LEFT JOIN node_allocations a ON a.node_id = n.id \
         WHERE n.workspace_id = $1 AND n.status = 'ready' \
         AND ( \
             (n.labels->>'role') = 'builder' \
             OR NOT EXISTS ( \
                 SELECT 1 FROM nodes n2 \
                 WHERE n2.workspace_id = $1 \
                   AND n2.status = 'ready' \
                   AND (n2.labels->>'role') = 'builder' \
             ) \
         ) \
         GROUP BY n.id \
         ORDER BY n.created_at ASC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;

    let mut fits: Vec<(NodeCapacity, i64)> = rows
        .into_iter()
        .filter_map(|row| {
            let free_cpu = row.total_cpu_millis as i64 - row.used_cpu_millis.unwrap_or(0);
            let free_mem = row.total_memory_mb as i64 - row.used_memory_mb.unwrap_or(0);
            let free_disk = row.total_disk_mb as i64 - row.used_disk_mb.unwrap_or(0);
            if free_cpu >= BUILD_CPU_MILLIS as i64
                && free_mem >= BUILD_MEMORY_MB as i64
                && free_disk >= BUILD_DISK_MB as i64
            {
                Some((row, free_mem))
            } else {
                None
            }
        })
        .collect();

    fits.sort_by_key(|(_, free_mem)| *free_mem);
    Ok(fits.into_iter().next().map(|(n, _)| n))
}

async fn fail_build(pool: &PgPool, build_id: &str, reason: &str) -> Result<()> {
    sqlx::query(
        "UPDATE builds SET status = 'failed', reason = $1, finished_at = now(), \
                            updated_at = now() \
         WHERE id = $2 AND status NOT IN ('succeeded','failed','cancelled')",
    )
    .bind(reason)
    .bind(build_id)
    .execute(pool)
    .await?;
    // Mirror the failure onto the deployment the build was for.
    sqlx::query(
        "UPDATE deployments SET status = 'errored', reason = $1, \
                                 stopped_at = now(), updated_at = now() \
         WHERE id = (SELECT deployment_id FROM builds WHERE id = $2) \
           AND status = 'building'",
    )
    .bind(reason)
    .bind(build_id)
    .execute(pool)
    .await?;
    sqlx::query(
        "DELETE FROM node_allocations \
         WHERE deployment_id = (SELECT deployment_id FROM builds WHERE id = $1)",
    )
    .bind(build_id)
    .execute(pool)
    .await?;
    Ok(())
}

struct PendingDeployment {
    deployment_id: String,
    service_id: String,
    workspace_id: String,
    image: String,
    env_vars: BTreeMap<String, String>,
    ports: Vec<PortMap>,
    resources: Resources,
    /// Service's registry_credential_id (if any). Threaded into the
    /// PullAndRun payload so the agent's pull hits the CP auth proxy with
    /// the right basic-auth creds.
    registry_credential_id: Option<String>,
}

async fn fetch_pending(pool: &PgPool) -> Result<Vec<PendingDeployment>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        deployment_id: String,
        service_id: String,
        workspace_id: String,
        image: String,
        env: JsonValue,
        ports: JsonValue,
        resources: JsonValue,
        registry_credential_id: Option<String>,
    }

    let rows: Vec<Row> = sqlx::query_as(
        "SELECT d.id AS deployment_id, d.service_id, w.id AS workspace_id, d.image_ref AS image, \
                d.env_vars AS env, d.ports, d.resources, s.registry_credential_id \
         FROM deployments d \
         JOIN services s ON s.id = d.service_id \
         JOIN projects p ON p.id = s.project_id \
         JOIN workspaces w ON w.id = p.workspace_id \
         WHERE d.status IN ('pending', 'placing') \
         ORDER BY d.created_at ASC \
         LIMIT 20",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let env_vars: BTreeMap<String, String> =
            serde_json::from_value(r.env).map_err(|e| anyhow!("bad env_vars json: {e}"))?;
        let ports: Vec<PortMap> =
            serde_json::from_value(r.ports).map_err(|e| anyhow!("bad ports json: {e}"))?;
        let resources: Resources =
            serde_json::from_value(r.resources).map_err(|e| anyhow!("bad resources json: {e}"))?;
        out.push(PendingDeployment {
            deployment_id: r.deployment_id,
            service_id: r.service_id,
            workspace_id: r.workspace_id,
            image: r.image,
            env_vars,
            ports,
            resources,
            registry_credential_id: r.registry_credential_id,
        });
    }
    Ok(out)
}

#[derive(sqlx::FromRow)]
struct NodeCapacity {
    id: String,
    provider: String,
    total_cpu_millis: i32,
    total_memory_mb: i32,
    total_disk_mb: i32,
    used_cpu_millis: Option<i64>,
    used_memory_mb: Option<i64>,
    used_disk_mb: Option<i64>,
}

/// First-fit-decreasing by free memory. Ready nodes only.
async fn pick_node(
    pool: &PgPool,
    workspace_id: &str,
    need: &Resources,
) -> Result<Option<NodeCapacity>> {
    let rows: Vec<NodeCapacity> = sqlx::query_as(
        "SELECT n.id, n.provider, n.total_cpu_millis, n.total_memory_mb, n.total_disk_mb, \
                COALESCE(SUM(a.cpu_millis), 0)::bigint AS used_cpu_millis, \
                COALESCE(SUM(a.memory_mb), 0)::bigint AS used_memory_mb, \
                COALESCE(SUM(a.disk_mb), 0)::bigint AS used_disk_mb \
         FROM nodes n \
         LEFT JOIN node_allocations a ON a.node_id = n.id \
         WHERE n.workspace_id = $1 AND n.status = 'ready' \
         GROUP BY n.id \
         ORDER BY n.created_at ASC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;

    let mut fits: Vec<(NodeCapacity, i64)> = rows
        .into_iter()
        .filter_map(|row| {
            let free_cpu = row.total_cpu_millis as i64 - row.used_cpu_millis.unwrap_or(0);
            let free_mem = row.total_memory_mb as i64 - row.used_memory_mb.unwrap_or(0);
            let free_disk = row.total_disk_mb as i64 - row.used_disk_mb.unwrap_or(0);
            if free_cpu >= need.cpu_millis as i64
                && free_mem >= need.memory_mb as i64
                && free_disk >= need.disk_mb as i64
            {
                Some((row, free_mem))
            } else {
                None
            }
        })
        .collect();

    // FFD: pick the node with *smallest* free memory that still fits — pack tight.
    fits.sort_by_key(|(_, free_mem)| *free_mem);
    Ok(fits.into_iter().next().map(|(n, _)| n))
}

async fn pick_preferred_node_for_domains(
    pool: &PgPool,
    service_id: &str,
    deployment_id: &str,
    workspace_id: &str,
    need: &Resources,
) -> Result<Option<NodeCapacity>> {
    let has_domains: Option<(bool,)> =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM service_domains WHERE service_id = $1)")
            .bind(service_id)
            .fetch_optional(pool)
            .await?;
    if !has_domains.map(|(v,)| v).unwrap_or(false) {
        return Ok(None);
    }

    let preferred_node: Option<(String,)> = sqlx::query_as(
        "SELECT d.node_id \
         FROM deployments d \
         WHERE d.service_id = $1 \
           AND d.id <> $2 \
           AND d.node_id IS NOT NULL \
         ORDER BY \
           CASE WHEN d.status = 'running' THEN 0 ELSE 1 END, \
           d.created_at DESC \
         LIMIT 1",
    )
    .bind(service_id)
    .bind(deployment_id)
    .fetch_optional(pool)
    .await?;
    let Some((node_id,)) = preferred_node else {
        return Ok(None);
    };

    let row: Option<NodeCapacity> = sqlx::query_as(
        "SELECT n.id, n.provider, n.total_cpu_millis, n.total_memory_mb, n.total_disk_mb, \
                COALESCE(SUM(a.cpu_millis), 0)::bigint AS used_cpu_millis, \
                COALESCE(SUM(a.memory_mb), 0)::bigint AS used_memory_mb, \
                COALESCE(SUM(a.disk_mb), 0)::bigint AS used_disk_mb \
         FROM nodes n \
         LEFT JOIN node_allocations a ON a.node_id = n.id \
         WHERE n.workspace_id = $1 AND n.status = 'ready' AND n.id = $2 \
         GROUP BY n.id",
    )
    .bind(workspace_id)
    .bind(node_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.filter(|row| {
        let free_cpu = row.total_cpu_millis as i64 - row.used_cpu_millis.unwrap_or(0);
        let free_mem = row.total_memory_mb as i64 - row.used_memory_mb.unwrap_or(0);
        let free_disk = row.total_disk_mb as i64 - row.used_disk_mb.unwrap_or(0);
        free_cpu >= need.cpu_millis as i64
            && free_mem >= need.memory_mb as i64
            && free_disk >= need.disk_mb as i64
    }))
}

async fn place_and_run(state: &AppState, p: &PendingDeployment) -> Result<()> {
    let picked = match pick_preferred_node_for_domains(
        state.pool(),
        &p.service_id,
        &p.deployment_id,
        &p.workspace_id,
        &p.resources,
    )
    .await?
    {
        Some(node) => Some(node),
        None => pick_node(state.pool(), &p.workspace_id, &p.resources).await?,
    };
    let node = match picked {
        Some(n) => n,
        None => {
            let outcome = try_provision_for(state, &p.workspace_id, &p.resources).await?;
            let reason = match outcome {
                ProvisionOutcome::Provisioning => {
                    "waiting for provisioned node to register".to_string()
                }
                ProvisionOutcome::Skipped(msg) => msg,
            };
            sqlx::query(
                "UPDATE deployments SET status = 'placing', reason = $1, updated_at = now() \
                 WHERE id = $2",
            )
            .bind(&reason)
            .bind(&p.deployment_id)
            .execute(state.pool())
            .await?;
            return Ok(());
        }
    };

    // Reserve capacity atomically.
    let mut tx = state.pool().begin().await?;
    sqlx::query(
        "INSERT INTO node_allocations (node_id, deployment_id, cpu_millis, memory_mb, disk_mb) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (node_id, deployment_id) DO NOTHING",
    )
    .bind(&node.id)
    .bind(&p.deployment_id)
    .bind(p.resources.cpu_millis as i32)
    .bind(p.resources.memory_mb as i32)
    .bind(p.resources.disk_mb as i32)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE deployments SET node_id = $1, status = 'pulling', reason = NULL, updated_at = now() \
         WHERE id = $2",
    )
    .bind(&node.id)
    .bind(&p.deployment_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // Only Hetzner agents run containers — local placements are intentionally gone.
    match node.provider.as_str() {
        "hetzner" => dispatch_to_agent(state, &node.id, p).await,
        other => Err(anyhow!("unsupported node provider: {other}")),
    }
}

async fn dispatch_to_agent(state: &AppState, node_id: &str, p: &PendingDeployment) -> Result<()> {
    // If the service references a registry credential, decrypt it and thread
    // the (url, username, password) into the payload so bollard's create_image
    // pulls with auth. Only meaningful for images in private registries
    // (bundled or user-hosted). Failure to decrypt is fatal — better to error
    // the deployment than to ship a broken command.
    let registry_cred = match &p.registry_credential_id {
        Some(id) => Some(
            credentials::fetch_decrypted(state.pool(), state.master_key(), &p.workspace_id, id)
                .await
                .context("fetching registry credential for pull")?
                .ok_or_else(|| anyhow!("registry credential {id} missing"))?,
        ),
        None => None,
    };
    let registry_auth = registry_cred.as_ref().and_then(|c| {
        let url = c.metadata.get("url").and_then(|v| v.as_str())?;
        let username = c.metadata.get("username").and_then(|v| v.as_str())?;
        Some(RegistryAuth {
            url,
            username,
            password: &c.secret,
        })
    });

    let payload = commands::pull_and_run_payload(
        &p.image,
        &json!(p.env_vars),
        &json!(p.ports),
        p.resources.cpu_millis,
        p.resources.memory_mb,
        registry_auth.as_ref(),
    );
    commands::enqueue(
        state.pool(),
        node_id,
        Some(&p.deployment_id),
        CommandKind::PullAndRun,
        payload,
    )
    .await?;
    Ok(())
}

/// Outcome of `try_provision_for`. `Skipped`/`Provisioning` land in the
/// deployment's `reason` column so users can see why they're waiting.
pub enum ProvisionOutcome {
    Provisioning,
    Skipped(String),
}

/// Decide whether to provision a new Hetzner node for `need`. Returns the
/// outcome along with a human-readable reason.
async fn try_provision_for(
    state: &AppState,
    workspace_id: &str,
    need: &Resources,
) -> Result<ProvisionOutcome> {
    #[derive(sqlx::FromRow)]
    struct WsSettings {
        hetzner_location: String,
        max_nodes: i32,
        scheduler_paused_until: Option<DateTime<Utc>>,
    }

    let ws: Option<WsSettings> = sqlx::query_as(
        "SELECT hetzner_location, max_nodes, scheduler_paused_until \
         FROM workspaces WHERE id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(state.pool())
    .await?;
    let Some(ws) = ws else {
        return Ok(ProvisionOutcome::Skipped(
            "workspace settings missing".into(),
        ));
    };

    if let Some(until) = ws.scheduler_paused_until {
        let remaining = until - Utc::now();
        if remaining.num_seconds() > 0 {
            return Ok(ProvisionOutcome::Skipped(format!(
                "scheduler paused for {}s (after a manual node delete); retry by restarting the deployment",
                remaining.num_seconds()
            )));
        }
    }

    // Don't pile up parallel provisions — one at a time.
    let (in_flight,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM nodes \
         WHERE workspace_id = $1 AND provider = 'hetzner' \
               AND status IN ('provisioning', 'draining')",
    )
    .bind(workspace_id)
    .fetch_one(state.pool())
    .await?;
    if in_flight > 0 {
        return Ok(ProvisionOutcome::Skipped(format!(
            "a node is already provisioning ({in_flight} in-flight); waiting for it to register"
        )));
    }

    let (current_nodes,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM nodes \
         WHERE workspace_id = $1 AND provider = 'hetzner' AND status <> 'terminated'",
    )
    .bind(workspace_id)
    .fetch_one(state.pool())
    .await?;
    if current_nodes >= ws.max_nodes as i64 {
        return Ok(ProvisionOutcome::Skipped(format!(
            "max_nodes reached ({current_nodes}/{cap}) — raise the cap in Settings",
            cap = ws.max_nodes
        )));
    }

    let token =
        match credentials::first_hetzner_token(state.pool(), state.master_key(), workspace_id)
            .await
            .context("fetching Hetzner token")?
        {
            Some(t) => t,
            None => {
                return Ok(ProvisionOutcome::Skipped(
                    "no Hetzner API token credential in this workspace".into(),
                ));
            }
        };

    let ssh_key_ids = ssh_keys::ensure_on_hetzner(state.pool(), workspace_id, &token).await?;

    let result = hetzner_provisioner::provision(
        state.pool(),
        state.config(),
        state.master_key(),
        &token,
        workspace_id,
        &ws.hetzner_location,
        hetzner_provisioner::NodeSize::Fit(need),
        ssh_key_ids,
    )
    .await?;
    tracing::info!(
        workspace = %workspace_id,
        node = %result.node_id,
        server = result.hetzner_server_id,
        "provisioned hetzner node"
    );
    Ok(ProvisionOutcome::Provisioning)
}

/// Update `idle_since_at` on nodes: stamp when allocations first drop to zero; clear when non-zero.
async fn refresh_idle_since(pool: &PgPool) -> Result<()> {
    sqlx::query(
        "UPDATE nodes SET idle_since_at = CASE \
            WHEN (SELECT COUNT(*) FROM node_allocations a WHERE a.node_id = nodes.id) = 0 \
                AND idle_since_at IS NULL AND status = 'ready' \
            THEN now() \
            WHEN (SELECT COUNT(*) FROM node_allocations a WHERE a.node_id = nodes.id) > 0 \
            THEN NULL \
            ELSE idle_since_at \
         END",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Terminate Hetzner nodes that have been idle longer than their workspace's
/// `autoscale_idle_ttl_seconds` and aren't flagged `persistent`.
async fn autoscale_down(state: &AppState) -> Result<()> {
    #[derive(sqlx::FromRow)]
    struct Candidate {
        id: String,
        workspace_id: String,
        hetzner_server_id: Option<i64>,
        idle_since_at: Option<DateTime<Utc>>,
        ttl_seconds: i32,
    }

    let rows: Vec<Candidate> = sqlx::query_as(
        "SELECT n.id, n.workspace_id, n.hetzner_server_id, n.idle_since_at, \
                w.autoscale_idle_ttl_seconds AS ttl_seconds \
         FROM nodes n \
         JOIN workspaces w ON w.id = n.workspace_id \
         WHERE n.provider = 'hetzner' \
               AND n.status = 'ready' \
               AND n.persistent = FALSE \
               AND n.idle_since_at IS NOT NULL",
    )
    .fetch_all(state.pool())
    .await?;

    let now = Utc::now();
    for c in rows {
        let Some(idle_since) = c.idle_since_at else {
            continue;
        };
        if (now - idle_since).num_seconds() < c.ttl_seconds as i64 {
            continue;
        }
        let Some(server_id) = c.hetzner_server_id else {
            continue;
        };
        let token =
            credentials::first_hetzner_token(state.pool(), state.master_key(), &c.workspace_id)
                .await?;
        let Some(token) = token else { continue };

        tracing::info!(node = %c.id, "autoscale-down: terminating idle hetzner node");
        if let Err(e) = hetzner_provisioner::terminate(state.pool(), &token, &c.id, server_id).await
        {
            tracing::warn!(error = ?e, node = %c.id, "terminate failed");
        }
    }
    Ok(())
}

/// Enqueue an `update_routes` command for the given node with the current
/// hostname → deployment route set (derived from service_domains + running
/// deployments).
pub async fn push_routes_for_node(pool: &PgPool, node_id: &str) -> Result<()> {
    let routes = crate::domains::routes_for_node(pool, node_id).await?;
    let payload = json!({
        "routes": routes.iter().map(|r| json!({
            "hostname": r.hostname,
            "container_port": r.container_port,
            "deployment_id": r.deployment_id,
            "container_name": r.container_name,
        })).collect::<Vec<_>>(),
    });
    commands::enqueue(pool, node_id, None, CommandKind::UpdateRoutes, payload).await?;
    Ok(())
}

/// Push route updates to every node currently running a deployment of this service.
pub async fn push_routes_for_service(pool: &PgPool, service_id: &str) -> Result<()> {
    let nodes = crate::domains::nodes_for_service(pool, service_id).await?;
    for node_id in nodes {
        push_routes_for_node(pool, &node_id).await?;
    }
    Ok(())
}
