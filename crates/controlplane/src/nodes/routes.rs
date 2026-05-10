use axum::extract::{Path, Query, State};
use axum::routing::{delete as delete_route, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use driftbase_common::Id;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::auth::AuthUser;
use crate::credentials;
use crate::error::{ApiError, ApiResult};
use crate::provisioner::hetzner as hetzner_provisioner;
use crate::state::AppState;
use crate::workspaces::membership::{self, Role};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces/:slug/nodes", get(list).post(provision))
        .route("/workspaces/:slug/nodes/:id/drain", post(drain))
        .route(
            "/workspaces/:slug/nodes/:id/agent-update/check",
            post(check_agent_update),
        )
        .route(
            "/workspaces/:slug/nodes/:id/agent-update",
            post(update_agent),
        )
        .route("/workspaces/:slug/nodes/:id", delete_route(delete))
}

#[derive(Serialize)]
pub struct NodeSummary {
    pub id: Id,
    pub name: String,
    pub provider: String,
    pub status: String,
    pub total_cpu_millis: i32,
    pub total_memory_mb: i32,
    pub total_disk_mb: i32,
    pub used_cpu_millis: i32,
    pub used_memory_mb: i32,
    pub used_disk_mb: i32,
    pub labels: JsonValue,
    pub public_ipv4: Option<String>,
    pub agent_version: Option<String>,
    pub agent_image_ref: Option<String>,
    pub agent_image_digest: Option<String>,
    pub agent_self_update_capable: bool,
    pub agent_update_status: String,
    pub agent_update_checked_at: Option<DateTime<Utc>>,
    pub agent_update_target_image_ref: Option<String>,
    pub agent_update_target_digest: Option<String>,
    pub agent_update_command_id: Option<Id>,
    pub agent_update_error: Option<String>,
    pub agent_update_started_at: Option<DateTime<Utc>>,
    pub agent_update_finished_at: Option<DateTime<Utc>>,
    pub private_network_capable: bool,
    pub wireguard_mesh_ip: Option<String>,
    pub private_network_synced_at: Option<DateTime<Utc>>,
    pub private_network_sync_error: Option<String>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub workloads: Vec<NodeWorkloadSummary>,
}

#[derive(Debug, Serialize)]
pub struct NodeWorkloadSummary {
    pub kind: String,
    pub status: String,
    pub project_slug: String,
    pub service_slug: String,
    pub deployment_id: Id,
    pub build_id: Option<Id>,
    pub cpu_millis: i32,
    pub memory_mb: i32,
    pub disk_mb: i32,
}

#[derive(sea_orm::FromQueryResult)]
struct NodeRow {
    id: String,
    name: String,
    provider: String,
    status: String,
    total_cpu_millis: i32,
    total_memory_mb: i32,
    total_disk_mb: i32,
    used_cpu_millis: Option<i64>,
    used_memory_mb: Option<i64>,
    used_disk_mb: Option<i64>,
    labels: JsonValue,
    public_ipv4: Option<String>,
    agent_version: Option<String>,
    agent_image_ref: Option<String>,
    agent_image_digest: Option<String>,
    agent_self_update_capable: bool,
    agent_update_status: String,
    agent_update_checked_at: Option<DateTime<Utc>>,
    agent_update_target_image_ref: Option<String>,
    agent_update_target_digest: Option<String>,
    agent_update_command_id: Option<String>,
    agent_update_error: Option<String>,
    agent_update_started_at: Option<DateTime<Utc>>,
    agent_update_finished_at: Option<DateTime<Utc>>,
    private_network_capable: bool,
    wireguard_mesh_ip: Option<String>,
    private_network_synced_at: Option<DateTime<Utc>>,
    private_network_sync_error: Option<String>,
    last_seen_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(sea_orm::FromQueryResult)]
struct NodeWorkloadRow {
    node_id: String,
    kind: String,
    status: String,
    project_slug: String,
    service_slug: String,
    deployment_id: String,
    build_id: Option<String>,
    cpu_millis: i32,
    memory_mb: i32,
    disk_mb: i32,
}

impl TryFrom<(NodeRow, Vec<NodeWorkloadSummary>)> for NodeSummary {
    type Error = ApiError;
    fn try_from((r, workloads): (NodeRow, Vec<NodeWorkloadSummary>)) -> Result<Self, ApiError> {
        Ok(Self {
            id: r
                .id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            name: r.name,
            provider: r.provider,
            status: r.status,
            total_cpu_millis: r.total_cpu_millis,
            total_memory_mb: r.total_memory_mb,
            total_disk_mb: r.total_disk_mb,
            used_cpu_millis: r.used_cpu_millis.unwrap_or(0) as i32,
            used_memory_mb: r.used_memory_mb.unwrap_or(0) as i32,
            used_disk_mb: r.used_disk_mb.unwrap_or(0) as i32,
            labels: r.labels,
            public_ipv4: r.public_ipv4,
            agent_version: r.agent_version,
            agent_image_ref: r.agent_image_ref,
            agent_image_digest: r.agent_image_digest,
            agent_self_update_capable: r.agent_self_update_capable,
            agent_update_status: r.agent_update_status,
            agent_update_checked_at: r.agent_update_checked_at,
            agent_update_target_image_ref: r.agent_update_target_image_ref,
            agent_update_target_digest: r.agent_update_target_digest,
            agent_update_command_id: r
                .agent_update_command_id
                .map(|s| s.parse())
                .transpose()
                .map_err(|e: ulid::DecodeError| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            agent_update_error: r.agent_update_error,
            agent_update_started_at: r.agent_update_started_at,
            agent_update_finished_at: r.agent_update_finished_at,
            private_network_capable: r.private_network_capable,
            wireguard_mesh_ip: r.wireguard_mesh_ip,
            private_network_synced_at: r.private_network_synced_at,
            private_network_sync_error: r.private_network_sync_error,
            last_seen_at: r.last_seen_at,
            created_at: r.created_at,
            workloads,
        })
    }
}

impl TryFrom<NodeWorkloadRow> for NodeWorkloadSummary {
    type Error = ApiError;

    fn try_from(r: NodeWorkloadRow) -> Result<Self, ApiError> {
        Ok(Self {
            kind: r.kind,
            status: r.status,
            project_slug: r.project_slug,
            service_slug: r.service_slug,
            deployment_id: r
                .deployment_id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            build_id: r
                .build_id
                .map(|s| s.parse())
                .transpose()
                .map_err(|e: ulid::DecodeError| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            cpu_millis: r.cpu_millis,
            memory_mb: r.memory_mb,
            disk_mb: r.disk_mb,
        })
    }
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> ApiResult<Json<Vec<NodeSummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;

    let rows: Vec<NodeRow> = crate::db::query_as(
        "SELECT n.id, n.name, n.provider, n.status, \
                n.total_cpu_millis, n.total_memory_mb, n.total_disk_mb, \
                COALESCE(SUM(a.cpu_millis), 0)::bigint AS used_cpu_millis, \
                COALESCE(SUM(a.memory_mb), 0)::bigint AS used_memory_mb, \
                COALESCE(SUM(a.disk_mb), 0)::bigint AS used_disk_mb, \
                n.labels, n.public_ipv4, \
                n.agent_version, n.agent_image_ref, n.agent_image_digest, \
                n.agent_self_update_capable, n.agent_update_status, \
                n.agent_update_checked_at, n.agent_update_target_image_ref, \
                n.agent_update_target_digest, n.agent_update_command_id, \
                n.agent_update_error, n.agent_update_started_at, n.agent_update_finished_at, \
                n.private_network_capable, \
                n.wireguard_mesh_ip, n.private_network_synced_at, \
                n.private_network_sync_error, n.last_seen_at, n.created_at \
         FROM nodes n \
         LEFT JOIN node_allocations a ON a.node_id = n.id \
         WHERE n.workspace_id = $1 \
         GROUP BY n.id \
         ORDER BY n.created_at ASC",
    )
    .bind(ctx.workspace_id.to_string())
    .fetch_all(state.pool())
    .await?;

    let workload_rows: Vec<NodeWorkloadRow> = crate::db::query_as(
        "SELECT a.node_id, \
                CASE WHEN b.id IS NULL THEN 'runtime' ELSE 'build' END AS kind, \
                COALESCE(b.status, d.status) AS status, \
                p.slug AS project_slug, \
                s.slug AS service_slug, \
                d.id AS deployment_id, \
                b.id AS build_id, \
                a.cpu_millis, a.memory_mb, a.disk_mb \
         FROM node_allocations a \
         JOIN deployments d ON d.id = a.deployment_id \
         JOIN services s ON s.id = d.service_id \
         JOIN projects p ON p.id = s.project_id \
         LEFT JOIN LATERAL ( \
            SELECT b.id, b.status \
            FROM builds b \
            WHERE b.deployment_id = d.id \
              AND b.node_id = a.node_id \
              AND b.status NOT IN ('succeeded', 'failed', 'cancelled') \
            ORDER BY b.updated_at DESC \
            LIMIT 1 \
         ) b ON true \
         WHERE p.workspace_id = $1 \
         ORDER BY a.node_id ASC, kind DESC, a.memory_mb DESC, a.created_at ASC",
    )
    .bind(ctx.workspace_id.to_string())
    .fetch_all(state.pool())
    .await?;

    let mut workloads_by_node =
        std::collections::BTreeMap::<String, Vec<NodeWorkloadSummary>>::new();
    for row in workload_rows {
        workloads_by_node
            .entry(row.node_id.clone())
            .or_default()
            .push(NodeWorkloadSummary::try_from(row)?);
    }

    rows.into_iter()
        .map(|row| {
            let workloads = workloads_by_node.remove(&row.id).unwrap_or_default();
            NodeSummary::try_from((row, workloads))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

async fn drain(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, node_id)): Path<(String, String)>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    let row: Option<(String,)> =
        crate::db::query_tuple("SELECT id FROM nodes WHERE id = $1 AND workspace_id = $2")
            .bind(&node_id)
            .bind(ctx.workspace_id.to_string())
            .fetch_optional(state.pool())
            .await?;
    row.ok_or(ApiError::NotFound)?;

    crate::db::query("UPDATE nodes SET status = 'draining' WHERE id = $1")
        .bind(&node_id)
        .execute(state.pool())
        .await?;
    Ok(())
}

async fn check_agent_update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, node_id)): Path<(String, String)>,
) -> ApiResult<Json<crate::agent_updates::AgentUpdateResponse>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    crate::agent_updates::check_node_update(
        state.pool(),
        state.config(),
        &ctx.workspace_id.to_string(),
        &node_id,
    )
    .await
    .map(Json)
}

async fn update_agent(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, node_id)): Path<(String, String)>,
) -> ApiResult<Json<crate::agent_updates::AgentUpdateResponse>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    crate::agent_updates::enqueue_node_update(
        state.pool(),
        state.config(),
        &ctx.workspace_id.to_string(),
        &node_id,
    )
    .await
    .map(Json)
}

#[derive(Deserialize, Default)]
struct DeleteQuery {
    #[serde(default)]
    force: Option<bool>,
}

async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, node_id)): Path<(String, String)>,
    Query(q): Query<DeleteQuery>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    #[derive(sea_orm::FromQueryResult)]
    struct Node {
        id: String,
        provider: String,
        status: String,
        hetzner_server_id: Option<i64>,
    }
    let node: Option<Node> = crate::db::query_as(
        "SELECT id, provider, status, hetzner_server_id FROM nodes \
         WHERE id = $1 AND workspace_id = $2",
    )
    .bind(&node_id)
    .bind(ctx.workspace_id.to_string())
    .fetch_optional(state.pool())
    .await?;
    let node = node.ok_or(ApiError::NotFound)?;

    let (busy,): (i64,) =
        crate::db::query_tuple("SELECT COUNT(*)::bigint FROM node_allocations WHERE node_id = $1")
            .bind(&node.id)
            .fetch_one(state.pool())
            .await?;
    if busy > 0 && !q.force.unwrap_or(false) {
        return Err(ApiError::Conflict(format!(
            "node has {busy} active deployments; pass force=true to delete anyway"
        )));
    }

    // Only call Hetzner when there's actually a live VM to kill — skip if the
    // row is already tombstoned as 'terminated' or there's no server id.
    let should_call_hetzner = node.provider == "hetzner"
        && node.status != "terminated"
        && node.hetzner_server_id.is_some();

    if should_call_hetzner {
        let token = credentials::first_hetzner_token(
            state.pool(),
            state.master_key(),
            &ctx.workspace_id.to_string(),
        )
        .await
        .map_err(ApiError::Internal)?;
        if let (Some(token), Some(server_id)) = (token, node.hetzner_server_id) {
            hetzner_provisioner::terminate(state.pool(), &token, &node.id, server_id)
                .await
                .map_err(ApiError::Internal)?;
            pause_scheduler(state.pool(), &ctx.workspace_id.to_string()).await?;
            return Ok(());
        }
    }

    // Already-terminated tombstone, or no credential to hit Hetzner with —
    // just drop the row so the UI stays clean. No pause: no live VM was killed.
    crate::db::query("DELETE FROM nodes WHERE id = $1")
        .bind(&node.id)
        .execute(state.pool())
        .await?;
    Ok(())
}

/// After any manual delete/terminate, pause auto-provisioning for 2 minutes so
/// the scheduler doesn't immediately replace the node while the user is
/// investigating. Admins can override by resuming via Settings (future work).
async fn pause_scheduler(pool: &sea_orm::DatabaseConnection, workspace_id: &str) -> ApiResult<()> {
    crate::db::query(
        "UPDATE workspaces SET scheduler_paused_until = now() + interval '2 minutes' \
         WHERE id = $1",
    )
    .bind(workspace_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Deserialize, Default)]
struct ProvisionRequest {
    #[serde(default)]
    server_type: Option<String>,
    #[serde(default)]
    location: Option<String>,
}

#[derive(Serialize)]
struct ProvisionResponse {
    node_id: Id,
    hetzner_server_id: i64,
}

async fn provision(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
    Json(req): Json<ProvisionRequest>,
) -> ApiResult<Json<ProvisionResponse>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    #[derive(sea_orm::FromQueryResult)]
    struct Ws {
        hetzner_location: String,
        default_server_type: Option<String>,
        max_nodes: i32,
    }
    let ws: Ws = crate::db::query_as(
        "SELECT hetzner_location, default_server_type, max_nodes FROM workspaces WHERE id = $1",
    )
    .bind(ctx.workspace_id.to_string())
    .fetch_one(state.pool())
    .await?;

    let (current,): (i64,) = crate::db::query_tuple(
        "SELECT COUNT(*)::bigint FROM nodes \
         WHERE workspace_id = $1 AND provider = 'hetzner' AND status <> 'terminated'",
    )
    .bind(ctx.workspace_id.to_string())
    .fetch_one(state.pool())
    .await?;
    if current >= ws.max_nodes as i64 {
        return Err(ApiError::Conflict(format!(
            "max_nodes ({}) reached for this workspace",
            ws.max_nodes
        )));
    }

    let server_type = req
        .server_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| ws.default_server_type.clone())
        .unwrap_or_else(|| "cx22".to_string());
    let location = req
        .location
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(ws.hetzner_location.as_str())
        .to_string();

    let token = credentials::first_hetzner_token(
        state.pool(),
        state.master_key(),
        &ctx.workspace_id.to_string(),
    )
    .await
    .map_err(ApiError::Internal)?
    .ok_or_else(|| ApiError::Validation("no Hetzner API token in this workspace".into()))?;

    let ssh_key_ids =
        crate::ssh_keys::ensure_on_hetzner(state.pool(), &ctx.workspace_id.to_string(), &token)
            .await
            .map_err(ApiError::Internal)?;

    let result = hetzner_provisioner::provision(
        state.pool(),
        state.config(),
        state.master_key(),
        &token,
        &ctx.workspace_id.to_string(),
        &location,
        hetzner_provisioner::NodeSize::Explicit(&server_type),
        ssh_key_ids,
    )
    .await
    .map_err(ApiError::Internal)?;

    crate::scheduler::nudge(&state);

    Ok(Json(ProvisionResponse {
        node_id: result.node_id,
        hetzner_server_id: result.hetzner_server_id,
    }))
}
