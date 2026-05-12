use axum::extract::{Path, Query, State};
use axum::routing::{delete as delete_route, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use driftbase_common::Id;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::auth::extractor::AuthUser;
use crate::credentials;
use crate::error::{ApiError, ApiResult};
use crate::provisioner::hetzner as hetzner_provisioner;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/overview", get(overview))
        .route("/admin/nodes", get(list_nodes))
        .route("/admin/nodes/:id/drain", post(drain_node))
        .route(
            "/admin/nodes/:id/agent-update/check",
            post(check_agent_update),
        )
        .route("/admin/nodes/:id/agent-update", post(update_agent))
        .route("/admin/nodes/:id", delete_route(delete_node))
        .route("/admin/users", get(list_users))
        .route("/admin/users/:id/approve", post(approve_user))
        .route("/admin/users/:id/reject", post(reject_user))
}

#[derive(Serialize)]
struct AdminOverview {
    counts: AdminCounts,
    pending_users: i64,
    unhealthy_deployments: Vec<AdminDeployment>,
}

#[derive(Serialize, sea_orm::FromQueryResult)]
struct AdminCounts {
    users: i64,
    workspaces: i64,
    projects: i64,
    services: i64,
    deployments: i64,
    running_deployments: i64,
    nodes: i64,
    ready_nodes: i64,
    errored_deployments: i64,
}

#[derive(Serialize, sea_orm::FromQueryResult)]
struct AdminDeployment {
    id: String,
    workspace_slug: String,
    project_slug: String,
    service_slug: String,
    status: String,
    image_ref: String,
    reason: Option<String>,
    node_id: Option<String>,
    updated_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

async fn overview(State(state): State<AppState>, auth: AuthUser) -> ApiResult<Json<AdminOverview>> {
    super::require_platform_admin(state.pool(), &auth.user_id).await?;

    let counts: AdminCounts = crate::db::query_as(
        "SELECT \
            (SELECT COUNT(*)::bigint FROM users) AS users, \
            (SELECT COUNT(*)::bigint FROM workspaces) AS workspaces, \
            (SELECT COUNT(*)::bigint FROM projects) AS projects, \
            (SELECT COUNT(*)::bigint FROM services) AS services, \
            (SELECT COUNT(*)::bigint FROM deployments) AS deployments, \
            (SELECT COUNT(*)::bigint FROM deployments WHERE status = 'running') AS running_deployments, \
            (SELECT COUNT(*)::bigint FROM nodes WHERE status <> 'terminated') AS nodes, \
            (SELECT COUNT(*)::bigint FROM nodes WHERE status = 'ready') AS ready_nodes, \
            (SELECT COUNT(*)::bigint FROM deployments WHERE status IN ('errored', 'failing')) AS errored_deployments",
    )
    .fetch_one(state.pool())
    .await?;

    let (pending_users,): (i64,) =
        crate::db::query_tuple("SELECT COUNT(*)::bigint FROM users WHERE status = 'pending'")
            .fetch_one(state.pool())
            .await?;

    let unhealthy_deployments: Vec<AdminDeployment> = crate::db::query_as(
        "SELECT d.id, w.slug AS workspace_slug, p.slug AS project_slug, s.slug AS service_slug, \
                d.status, d.image_ref, d.reason, d.node_id, d.updated_at, d.created_at \
         FROM deployments d \
         JOIN services s ON s.id = d.service_id \
         JOIN projects p ON p.id = s.project_id \
         JOIN workspaces w ON w.id = p.workspace_id \
         WHERE d.status IN ('errored', 'failing') \
            OR (d.status IN ('pulling', 'starting', 'placing') AND d.updated_at < now() - interval '5 minutes') \
         ORDER BY d.updated_at DESC \
         LIMIT 20",
    )
    .fetch_all(state.pool())
    .await?;

    Ok(Json(AdminOverview {
        counts,
        pending_users,
        unhealthy_deployments,
    }))
}

#[derive(Serialize)]
struct AdminNode {
    id: String,
    workspace_id: String,
    workspace_slug: String,
    workspace_name: String,
    name: String,
    provider: String,
    status: String,
    hetzner_location: Option<String>,
    hetzner_server_type: Option<String>,
    total_cpu_millis: i32,
    total_memory_mb: i32,
    total_disk_mb: i32,
    used_cpu_millis: i32,
    used_memory_mb: i32,
    used_disk_mb: i32,
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
    agent_update_command_id: Option<Id>,
    agent_update_error: Option<String>,
    agent_update_started_at: Option<DateTime<Utc>>,
    agent_update_finished_at: Option<DateTime<Utc>>,
    private_network_capable: bool,
    wireguard_mesh_ip: Option<String>,
    private_network_synced_at: Option<DateTime<Utc>>,
    private_network_sync_error: Option<String>,
    last_seen_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    workloads: Vec<AdminNodeWorkload>,
}

#[derive(Debug, Serialize)]
struct AdminNodeWorkload {
    kind: String,
    status: String,
    workspace_slug: String,
    project_slug: String,
    service_slug: String,
    deployment_id: Id,
    build_id: Option<Id>,
    cpu_millis: i32,
    memory_mb: i32,
    disk_mb: i32,
}

#[derive(sea_orm::FromQueryResult)]
struct AdminNodeRow {
    id: String,
    workspace_id: String,
    workspace_slug: String,
    workspace_name: String,
    name: String,
    provider: String,
    status: String,
    hetzner_location: Option<String>,
    hetzner_server_type: Option<String>,
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
struct AdminNodeWorkloadRow {
    node_id: String,
    kind: String,
    status: String,
    workspace_slug: String,
    project_slug: String,
    service_slug: String,
    deployment_id: String,
    build_id: Option<String>,
    cpu_millis: i32,
    memory_mb: i32,
    disk_mb: i32,
}

impl TryFrom<AdminNodeWorkloadRow> for AdminNodeWorkload {
    type Error = ApiError;

    fn try_from(r: AdminNodeWorkloadRow) -> Result<Self, ApiError> {
        Ok(Self {
            kind: r.kind,
            status: r.status,
            workspace_slug: r.workspace_slug,
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

impl TryFrom<(AdminNodeRow, Vec<AdminNodeWorkload>)> for AdminNode {
    type Error = ApiError;

    fn try_from((r, workloads): (AdminNodeRow, Vec<AdminNodeWorkload>)) -> Result<Self, ApiError> {
        Ok(Self {
            id: r.id,
            workspace_id: r.workspace_id,
            workspace_slug: r.workspace_slug,
            workspace_name: r.workspace_name,
            name: r.name,
            provider: r.provider,
            status: r.status,
            hetzner_location: r.hetzner_location,
            hetzner_server_type: r.hetzner_server_type,
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

async fn list_nodes(
    State(state): State<AppState>,
    auth: AuthUser,
) -> ApiResult<Json<Vec<AdminNode>>> {
    super::require_platform_admin(state.pool(), &auth.user_id).await?;

    let rows: Vec<AdminNodeRow> = crate::db::query_as(
        "SELECT n.id, n.workspace_id, w.slug AS workspace_slug, w.name AS workspace_name, \
                n.name, n.provider, n.status, n.hetzner_location, n.hetzner_server_type, \
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
                n.private_network_capable, n.wireguard_mesh_ip, n.private_network_synced_at, \
                n.private_network_sync_error, n.last_seen_at, n.created_at \
         FROM nodes n \
         JOIN workspaces w ON w.id = n.workspace_id \
         LEFT JOIN node_allocations a ON a.node_id = n.id \
         WHERE n.status <> 'terminated' \
         GROUP BY n.id, w.id \
         ORDER BY n.created_at DESC",
    )
    .fetch_all(state.pool())
    .await?;

    let workload_rows: Vec<AdminNodeWorkloadRow> = crate::db::query_as(
        "SELECT a.node_id, \
                CASE WHEN b.id IS NULL THEN 'runtime' ELSE 'build' END AS kind, \
                COALESCE(b.status, d.status) AS status, \
                w.slug AS workspace_slug, p.slug AS project_slug, s.slug AS service_slug, \
                d.id AS deployment_id, b.id AS build_id, \
                a.cpu_millis, a.memory_mb, a.disk_mb \
         FROM node_allocations a \
         JOIN deployments d ON d.id = a.deployment_id \
         JOIN services s ON s.id = d.service_id \
         JOIN projects p ON p.id = s.project_id \
         JOIN workspaces w ON w.id = p.workspace_id \
         LEFT JOIN LATERAL ( \
            SELECT b.id, b.status \
            FROM builds b \
            WHERE b.deployment_id = d.id \
              AND b.node_id = a.node_id \
              AND b.status NOT IN ('succeeded', 'failed', 'cancelled') \
            ORDER BY b.updated_at DESC \
            LIMIT 1 \
         ) b ON true \
         ORDER BY a.node_id ASC, kind DESC, a.memory_mb DESC, a.created_at ASC",
    )
    .fetch_all(state.pool())
    .await?;

    let mut workloads_by_node = std::collections::BTreeMap::<String, Vec<AdminNodeWorkload>>::new();
    for row in workload_rows {
        workloads_by_node
            .entry(row.node_id.clone())
            .or_default()
            .push(AdminNodeWorkload::try_from(row)?);
    }

    rows.into_iter()
        .map(|row| {
            let workloads = workloads_by_node.remove(&row.id).unwrap_or_default();
            AdminNode::try_from((row, workloads))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

#[derive(Serialize)]
struct AdminUser {
    id: String,
    email: String,
    display_name: String,
    status: String,
    is_platform_admin: bool,
    created_at: DateTime<Utc>,
}

async fn list_users(
    State(state): State<AppState>,
    auth: AuthUser,
) -> ApiResult<Json<Vec<AdminUser>>> {
    super::require_platform_admin(state.pool(), &auth.user_id).await?;

    #[derive(sea_orm::FromQueryResult)]
    struct Row {
        id: String,
        email: String,
        display_name: String,
        status: String,
        is_platform_admin: bool,
        created_at: DateTime<Utc>,
    }
    // Pending first so the admin sees what needs attention at the top.
    let rows: Vec<Row> = crate::db::query_as(
        "SELECT id, email, display_name, status, is_platform_admin, created_at \
         FROM users \
         ORDER BY CASE status WHEN 'pending' THEN 0 WHEN 'approved' THEN 1 ELSE 2 END, \
                  created_at DESC",
    )
    .fetch_all(state.pool())
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| AdminUser {
                id: r.id,
                email: r.email,
                display_name: r.display_name,
                status: r.status,
                is_platform_admin: r.is_platform_admin,
                created_at: r.created_at,
            })
            .collect(),
    ))
}

async fn approve_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<String>,
) -> ApiResult<()> {
    super::require_platform_admin(state.pool(), &auth.user_id).await?;

    crate::db::query("UPDATE users SET status = 'approved' WHERE id = $1")
        .bind(&user_id)
        .execute(state.pool())
        .await?;
    Ok(())
}

async fn reject_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<String>,
) -> ApiResult<()> {
    super::require_platform_admin(state.pool(), &auth.user_id).await?;

    // Flip status and nuke live sessions so the rejected user gets booted
    // on their next request instead of waiting for cookie expiry.
    crate::db::query("UPDATE users SET status = 'rejected' WHERE id = $1")
        .bind(&user_id)
        .execute(state.pool())
        .await?;
    crate::db::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(&user_id)
        .execute(state.pool())
        .await?;
    Ok(())
}

async fn drain_node(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(node_id): Path<String>,
) -> ApiResult<()> {
    super::require_platform_admin(state.pool(), &auth.user_id).await?;

    let row: Option<(String,)> = crate::db::query_tuple("SELECT id FROM nodes WHERE id = $1")
        .bind(&node_id)
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
    Path(node_id): Path<String>,
) -> ApiResult<Json<crate::agent_updates::AgentUpdateResponse>> {
    super::require_platform_admin(state.pool(), &auth.user_id).await?;
    let workspace_id = workspace_for_node(state.pool(), &node_id).await?;

    crate::agent_updates::check_node_update(state.pool(), state.config(), &workspace_id, &node_id)
        .await
        .map(Json)
}

async fn update_agent(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(node_id): Path<String>,
) -> ApiResult<Json<crate::agent_updates::AgentUpdateResponse>> {
    super::require_platform_admin(state.pool(), &auth.user_id).await?;
    let workspace_id = workspace_for_node(state.pool(), &node_id).await?;

    crate::agent_updates::enqueue_node_update(state.pool(), state.config(), &workspace_id, &node_id)
        .await
        .map(Json)
}

#[derive(Deserialize, Default)]
struct DeleteQuery {
    #[serde(default)]
    force: Option<bool>,
}

async fn delete_node(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(node_id): Path<String>,
    Query(q): Query<DeleteQuery>,
) -> ApiResult<()> {
    super::require_platform_admin(state.pool(), &auth.user_id).await?;

    #[derive(sea_orm::FromQueryResult)]
    struct Node {
        id: String,
        workspace_id: String,
        provider: String,
        hetzner_server_id: Option<i64>,
    }
    let node: Option<Node> = crate::db::query_as(
        "SELECT id, workspace_id, provider, hetzner_server_id FROM nodes WHERE id = $1",
    )
    .bind(&node_id)
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
            "node has {busy} active workloads; pass force=true to delete anyway"
        )));
    }

    if let ("hetzner", Some(server_id)) = (node.provider.as_str(), node.hetzner_server_id) {
        let token = credentials::hetzner_token_for_workspace(
            state.pool(),
            state.config(),
            state.master_key(),
            &node.workspace_id,
        )
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::Validation(
                "managed Hetzner token is not configured; cannot delete provider server".into(),
            )
        })?;
        hetzner_provisioner::terminate(state.pool(), &token, &node.id, server_id)
            .await
            .map_err(ApiError::Internal)?;
        pause_scheduler(state.pool(), &node.workspace_id).await?;
        return Ok(());
    }

    crate::db::query("DELETE FROM nodes WHERE id = $1")
        .bind(&node.id)
        .execute(state.pool())
        .await?;
    Ok(())
}

async fn workspace_for_node(
    pool: &sea_orm::DatabaseConnection,
    node_id: &str,
) -> ApiResult<String> {
    let row: Option<(String,)> =
        crate::db::query_tuple("SELECT workspace_id FROM nodes WHERE id = $1")
            .bind(node_id)
            .fetch_optional(pool)
            .await?;
    row.map(|(workspace_id,)| workspace_id)
        .ok_or(ApiError::NotFound)
}

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
