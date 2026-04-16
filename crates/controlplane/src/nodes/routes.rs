use axum::extract::{Path, Query, State};
use axum::routing::{delete as delete_route, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use zediz_common::Id;

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
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
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
    last_seen_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl TryFrom<NodeRow> for NodeSummary {
    type Error = ApiError;
    fn try_from(r: NodeRow) -> Result<Self, ApiError> {
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
            last_seen_at: r.last_seen_at,
            created_at: r.created_at,
        })
    }
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> ApiResult<Json<Vec<NodeSummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;

    let rows: Vec<NodeRow> = sqlx::query_as(
        "SELECT n.id, n.name, n.provider, n.status, \
                n.total_cpu_millis, n.total_memory_mb, n.total_disk_mb, \
                COALESCE(SUM(a.cpu_millis), 0)::bigint AS used_cpu_millis, \
                COALESCE(SUM(a.memory_mb), 0)::bigint AS used_memory_mb, \
                COALESCE(SUM(a.disk_mb), 0)::bigint AS used_disk_mb, \
                n.labels, n.last_seen_at, n.created_at \
         FROM nodes n \
         LEFT JOIN node_allocations a ON a.node_id = n.id \
         WHERE n.workspace_id = $1 \
         GROUP BY n.id \
         ORDER BY n.created_at ASC",
    )
    .bind(ctx.workspace_id.to_string())
    .fetch_all(state.pool())
    .await?;

    rows.into_iter()
        .map(NodeSummary::try_from)
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
        sqlx::query_as("SELECT id FROM nodes WHERE id = $1 AND workspace_id = $2")
            .bind(&node_id)
            .bind(ctx.workspace_id.to_string())
            .fetch_optional(state.pool())
            .await?;
    row.ok_or(ApiError::NotFound)?;

    sqlx::query("UPDATE nodes SET status = 'draining' WHERE id = $1")
        .bind(&node_id)
        .execute(state.pool())
        .await?;
    Ok(())
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

    #[derive(sqlx::FromRow)]
    struct Node {
        id: String,
        provider: String,
        status: String,
        hetzner_server_id: Option<i64>,
    }
    let node: Option<Node> = sqlx::query_as(
        "SELECT id, provider, status, hetzner_server_id FROM nodes \
         WHERE id = $1 AND workspace_id = $2",
    )
    .bind(&node_id)
    .bind(ctx.workspace_id.to_string())
    .fetch_optional(state.pool())
    .await?;
    let node = node.ok_or(ApiError::NotFound)?;

    let (busy,): (i64,) =
        sqlx::query_as("SELECT COUNT(*)::bigint FROM node_allocations WHERE node_id = $1")
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
    sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(&node.id)
        .execute(state.pool())
        .await?;
    Ok(())
}

/// After any manual delete/terminate, pause auto-provisioning for 2 minutes so
/// the scheduler doesn't immediately replace the node while the user is
/// investigating. Admins can override by resuming via Settings (future work).
async fn pause_scheduler(pool: &sqlx::PgPool, workspace_id: &str) -> ApiResult<()> {
    sqlx::query(
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

    #[derive(sqlx::FromRow)]
    struct Ws {
        hetzner_location: String,
        default_server_type: Option<String>,
        max_nodes: i32,
    }
    let ws: Ws = sqlx::query_as(
        "SELECT hetzner_location, default_server_type, max_nodes FROM workspaces WHERE id = $1",
    )
    .bind(ctx.workspace_id.to_string())
    .fetch_one(state.pool())
    .await?;

    let (current,): (i64,) = sqlx::query_as(
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

    let ssh_key_ids = ensure_ssh_keys(state.pool(), &ctx.workspace_id.to_string(), &token).await?;

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

async fn ensure_ssh_keys(
    pool: &sqlx::PgPool,
    workspace_id: &str,
    hetzner_token: &str,
) -> ApiResult<Vec<i64>> {
    let keys = crate::ssh_keys::list_for_sync(pool, workspace_id)
        .await
        .map_err(ApiError::Internal)?;
    if keys.is_empty() {
        return Ok(vec![]);
    }
    let client = zediz_hetzner::HetznerClient::new(hetzner_token);
    let mut out = Vec::with_capacity(keys.len());
    for k in keys {
        match client
            .ensure_ssh_key(&k.name, &k.public_key, &k.fingerprint)
            .await
        {
            Ok(id) => out.push(id),
            Err(e) => tracing::warn!(error = ?e, ssh_key = %k.id, "ensure ssh key on hetzner"),
        }
    }
    Ok(out)
}
