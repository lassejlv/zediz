use axum::async_trait;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::convert::Infallible;
use std::time::Duration;

use crate::agent::commands::{self, AgentCommand};
use crate::agent::tokens::{self, TokenClaims, TokenKind};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/agent/register", post(register))
        .route("/agent/heartbeat", post(heartbeat))
        .route("/agent/deployments/:id/status", post(deployment_status))
        .route("/agent/deployments/:id/logs", post(push_logs))
        .route("/agent/deployments/:id/log-tail", get(tail_logs))
        .route("/agent/builds/:id/status", post(build_status))
}

#[derive(Deserialize)]
struct RegisterRequest {
    bootstrap_token: String,
    hostname: Option<String>,
    agent_version: Option<String>,
    total_cpu_millis: Option<i32>,
    total_memory_mb: Option<i32>,
    total_disk_mb: Option<i32>,
}

#[derive(Serialize)]
struct RegisterResponse {
    node_id: String,
    workspace_id: String,
    node_token: String,
    heartbeat_interval_seconds: u32,
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> ApiResult<Json<RegisterResponse>> {
    let claims = tokens::verify(
        state.master_key(),
        &req.bootstrap_token,
        TokenKind::Bootstrap,
    )
    .map_err(|_| ApiError::Unauthorized)?;

    let existing: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT workspace_id, status FROM nodes WHERE id = $1")
            .bind(&claims.node_id)
            .fetch_optional(state.pool())
            .await?;

    let (workspace_id, _status) = existing.ok_or(ApiError::NotFound)?;
    if workspace_id != claims.workspace_id {
        return Err(ApiError::Unauthorized);
    }

    let node_token = tokens::mint_node(state.master_key(), &claims.node_id, &claims.workspace_id)
        .map_err(ApiError::Internal)?;
    let node_token_hash = tokens::fingerprint(&node_token);

    sqlx::query(
        "UPDATE nodes SET \
            status = 'ready', \
            node_token_hash = $1, \
            bootstrap_token_hash = NULL, \
            agent_version = COALESCE($2, agent_version), \
            total_cpu_millis = COALESCE($3, total_cpu_millis), \
            total_memory_mb = COALESCE($4, total_memory_mb), \
            total_disk_mb = COALESCE($5, total_disk_mb), \
            registered_at = COALESCE(registered_at, now()), \
            last_seen_at = now() \
         WHERE id = $6",
    )
    .bind(&node_token_hash)
    .bind(req.agent_version.as_deref())
    .bind(req.total_cpu_millis)
    .bind(req.total_memory_mb)
    .bind(req.total_disk_mb)
    .bind(&claims.node_id)
    .execute(state.pool())
    .await?;

    if let Some(name) = req.hostname {
        tracing::info!(node = %claims.node_id, hostname = %name, "agent registered");
    }

    crate::scheduler::nudge(&state);

    Ok(Json(RegisterResponse {
        node_id: claims.node_id,
        workspace_id: claims.workspace_id,
        node_token,
        heartbeat_interval_seconds: 10,
    }))
}

/// Extractor that validates a node token from the `Authorization: Bearer …` header.
pub struct NodeAuth {
    pub claims: TokenClaims,
}

#[async_trait]
impl FromRequestParts<AppState> for NodeAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .ok_or(ApiError::Unauthorized)?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or(ApiError::Unauthorized)?;
        let claims = tokens::verify(state.master_key(), token, TokenKind::Node)
            .map_err(|_| ApiError::Unauthorized)?;

        let fp = tokens::fingerprint(token);
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM nodes WHERE id = $1 AND node_token_hash = $2 AND status <> 'terminated'",
        )
        .bind(&claims.node_id)
        .bind(&fp)
        .fetch_optional(state.pool())
        .await?;
        row.ok_or(ApiError::Unauthorized)?;

        sqlx::query("UPDATE nodes SET last_seen_at = now() WHERE id = $1")
            .bind(&claims.node_id)
            .execute(state.pool())
            .await?;
        Ok(NodeAuth { claims })
    }
}

#[derive(Deserialize)]
struct HeartbeatRequest {
    #[serde(default)]
    cpu_used_millis: Option<i32>,
    #[serde(default)]
    memory_used_mb: Option<i32>,
    #[serde(default)]
    disk_used_mb: Option<i32>,
    #[serde(default)]
    load_avg_1m: Option<f32>,
    #[serde(default)]
    acks: Vec<CommandAck>,
    #[serde(default)]
    container_metrics: Vec<ContainerMetricSample>,
}

#[derive(Deserialize, Serialize)]
struct ContainerMetricSample {
    deployment_id: String,
    ts: DateTime<Utc>,
    cpu_percent: f32,
    memory_bytes: i64,
    #[serde(default)]
    memory_limit_bytes: Option<i64>,
    rx_bytes: i64,
    tx_bytes: i64,
}

#[derive(Deserialize)]
struct CommandAck {
    command_id: String,
    ok: bool,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Serialize)]
struct HeartbeatResponse {
    commands: Vec<AgentCommand>,
}

async fn heartbeat(
    State(state): State<AppState>,
    NodeAuth { claims }: NodeAuth,
    Json(req): Json<HeartbeatRequest>,
) -> ApiResult<Json<HeartbeatResponse>> {
    if let (Some(c), Some(m), Some(d)) = (req.cpu_used_millis, req.memory_used_mb, req.disk_used_mb)
    {
        tracing::trace!(
            node = %claims.node_id,
            cpu = c,
            mem = m,
            disk = d,
            load = req.load_avg_1m.unwrap_or(0.0),
            "heartbeat",
        );
    }

    for ack in req.acks {
        commands::mark_acked(
            state.pool(),
            &ack.command_id,
            ack.ok,
            ack.message.as_deref(),
        )
        .await?;
    }

    for sample in req.container_metrics {
        // Scope the write to deployments owned by this node to prevent a
        // compromised agent from poisoning other nodes' rows.
        if let Err(e) = sqlx::query(
            "UPDATE deployments \
             SET runtime_metrics = $1 \
             WHERE id = $2 AND node_id = $3",
        )
        .bind(serde_json::to_value(&sample).unwrap_or(serde_json::Value::Null))
        .bind(&sample.deployment_id)
        .bind(&claims.node_id)
        .execute(state.pool())
        .await
        {
            tracing::warn!(
                deployment = %sample.deployment_id,
                error = ?e,
                "store runtime_metrics",
            );
        }
    }

    let pending = commands::claim_for_node(state.pool(), &claims.node_id, 16).await?;
    Ok(Json(HeartbeatResponse { commands: pending }))
}

#[derive(Deserialize)]
struct StatusUpdate {
    status: String,
    #[serde(default)]
    container_id: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

async fn deployment_status(
    State(state): State<AppState>,
    NodeAuth { claims }: NodeAuth,
    Path(deployment_id): Path<String>,
    Json(update): Json<StatusUpdate>,
) -> ApiResult<()> {
    if !matches!(
        update.status.as_str(),
        "pending" | "pulling" | "starting" | "running" | "failing" | "stopped" | "errored"
    ) {
        return Err(ApiError::Validation("invalid status".into()));
    }

    let row: Option<(String,)> = sqlx::query_as(
        "SELECT d.service_id FROM deployments d \
         JOIN services s ON s.id = d.service_id \
         JOIN projects p ON p.id = s.project_id \
         WHERE d.id = $1 AND (d.node_id = $2 OR p.workspace_id = $3)",
    )
    .bind(&deployment_id)
    .bind(&claims.node_id)
    .bind(&claims.workspace_id)
    .fetch_optional(state.pool())
    .await?;
    let service_id = row.ok_or(ApiError::NotFound)?.0;

    let started_at = if update.status == "running" {
        Some(Utc::now())
    } else {
        None
    };
    let stopped_at = if matches!(update.status.as_str(), "stopped" | "errored") {
        Some(Utc::now())
    } else {
        None
    };

    sqlx::query(
        "UPDATE deployments SET \
            status = $1, \
            container_id = COALESCE($2, container_id), \
            reason = $3, \
            started_at = COALESCE($4, started_at), \
            stopped_at = COALESCE($5, stopped_at), \
            node_id = COALESCE(node_id, $6), \
            updated_at = now() \
         WHERE id = $7",
    )
    .bind(&update.status)
    .bind(update.container_id.as_deref())
    .bind(update.reason.as_deref())
    .bind(started_at)
    .bind(stopped_at)
    .bind(&claims.node_id)
    .bind(&deployment_id)
    .execute(state.pool())
    .await?;

    if matches!(update.status.as_str(), "stopped" | "errored") {
        sqlx::query("DELETE FROM node_allocations WHERE deployment_id = $1")
            .bind(&deployment_id)
            .execute(state.pool())
            .await?;
    }

    // Any status change on a deployment might alter which hostnames this
    // node should serve. On `running` we also retire the previous running
    // deployment (if any) of the same service — that is the moment of
    // cut-over, and waiting until now keeps the old upstream live for the
    // whole image-pull window so Caddy never sees an empty route set.
    let mut affected_nodes: BTreeSet<String> = BTreeSet::new();
    if update.status == "running" {
        match crate::deployments::retire_superseded_running(
            state.pool(),
            &service_id,
            &deployment_id,
        )
        .await
        {
            Ok(nodes) => affected_nodes.extend(nodes),
            Err(e) => {
                tracing::warn!(
                    error = ?e,
                    service = %service_id,
                    "retire_superseded_running",
                );
            }
        }
    }
    if matches!(
        update.status.as_str(),
        "running" | "stopped" | "errored" | "failing"
    ) {
        affected_nodes.insert(claims.node_id.clone());
    }
    for node_id in affected_nodes {
        if let Err(e) = crate::scheduler::push_routes_for_node(state.pool(), &node_id).await {
            tracing::warn!(error = ?e, node = %node_id, "push_routes_for_node");
        }
    }

    Ok(())
}

#[derive(Deserialize)]
struct PushLogsRequest {
    lines: Vec<LogLineIn>,
}

#[derive(Deserialize)]
struct LogLineIn {
    stream: String,
    ts: DateTime<Utc>,
    line: String,
}

async fn push_logs(
    State(state): State<AppState>,
    NodeAuth { claims }: NodeAuth,
    Path(deployment_id): Path<String>,
    Json(req): Json<PushLogsRequest>,
) -> ApiResult<()> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT d.id FROM deployments d \
         WHERE d.id = $1 AND ( \
             d.node_id = $2 OR \
             EXISTS ( \
                 SELECT 1 FROM node_allocations a \
                 WHERE a.deployment_id = d.id AND a.node_id = $2 \
             ) OR \
             EXISTS ( \
                 SELECT 1 FROM builds b \
                 WHERE b.deployment_id = d.id AND b.node_id = $2 \
             ) \
         )",
    )
    .bind(&deployment_id)
    .bind(&claims.node_id)
    .fetch_optional(state.pool())
    .await?;
    row.ok_or(ApiError::NotFound)?;

    for l in req.lines {
        if !matches!(l.stream.as_str(), "stdout" | "stderr") {
            continue;
        }
        sqlx::query(
            "INSERT INTO deployment_logs (deployment_id, stream, ts, line) VALUES ($1, $2, $3, $4)",
        )
        .bind(&deployment_id)
        .bind(&l.stream)
        .bind(l.ts)
        .bind(&l.line)
        .execute(state.pool())
        .await?;
    }

    // Trim: keep ~2000 most recent lines per deployment.
    sqlx::query(
        "DELETE FROM deployment_logs WHERE deployment_id = $1 AND id NOT IN ( \
            SELECT id FROM deployment_logs WHERE deployment_id = $1 ORDER BY id DESC LIMIT 2000 \
         )",
    )
    .bind(&deployment_id)
    .execute(state.pool())
    .await?;

    Ok(())
}

#[derive(Deserialize)]
pub struct TailQuery {
    #[serde(default)]
    pub after_id: Option<i64>,
}

pub async fn tail_logs(
    State(state): State<AppState>,
    auth: crate::auth::AuthUser,
    Path(deployment_id): Path<String>,
    Query(q): Query<TailQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    crate::deployments::authorize(state.pool(), &deployment_id, &auth.user_id).await?;

    struct TailState {
        pool: sqlx::PgPool,
        deployment_id: String,
        buffer: std::collections::VecDeque<(i64, String, DateTime<Utc>, String)>,
        last_id: i64,
        first_poll: bool,
    }

    let init = TailState {
        pool: state.pool().clone(),
        deployment_id,
        buffer: std::collections::VecDeque::new(),
        last_id: q.after_id.unwrap_or(0),
        first_poll: true,
    };

    let stream = futures::stream::unfold(init, |mut s| async move {
        loop {
            if let Some((id, stream, ts, line)) = s.buffer.pop_front() {
                let ev = log_event(id, &stream, &ts, &line);
                return Some((Ok::<_, Infallible>(ev), s));
            }

            if !s.first_poll {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            s.first_poll = false;

            let rows: Vec<(i64, String, DateTime<Utc>, String)> = sqlx::query_as(
                "SELECT id, stream, ts, line FROM deployment_logs \
                 WHERE deployment_id = $1 AND id > $2 ORDER BY id ASC LIMIT 500",
            )
            .bind(&s.deployment_id)
            .bind(s.last_id)
            .fetch_all(&s.pool)
            .await
            .unwrap_or_default();

            for row in rows {
                s.last_id = row.0;
                s.buffer.push_back(row);
            }
        }
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(Deserialize)]
struct BuildStatusUpdate {
    status: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    git_commit: Option<String>,
    #[serde(default)]
    image_digest: Option<String>,
    #[serde(default)]
    image_tag: Option<String>,
}

/// Agent → CP build progress. Accepted transitions:
///   queued → cloning → building → pushing → succeeded
///                                         → failed (from any)
/// On `succeeded` we write the pushed tag back to the service + deployment and
/// flip the deployment to `pending` so the scheduler dispatches `pull_and_run`.
async fn build_status(
    State(state): State<AppState>,
    NodeAuth { claims }: NodeAuth,
    Path(build_id): Path<String>,
    Json(update): Json<BuildStatusUpdate>,
) -> ApiResult<()> {
    if !matches!(
        update.status.as_str(),
        "cloning" | "building" | "pushing" | "succeeded" | "failed"
    ) {
        return Err(ApiError::Validation("invalid build status".into()));
    }

    let build = crate::builds::fetch_by_id(state.pool(), &build_id).await?;
    // Enforce that the reporting node is the one the build was dispatched to.
    if build.node_id.as_deref() != Some(&claims.node_id) {
        return Err(ApiError::Unauthorized);
    }

    let terminal = matches!(update.status.as_str(), "succeeded" | "failed");
    sqlx::query(
        "UPDATE builds SET \
            status = $1, \
            reason = COALESCE($2, reason), \
            git_commit = COALESCE($3, git_commit), \
            image_digest = COALESCE($4, image_digest), \
            image_tag = COALESCE($5, image_tag), \
            finished_at = CASE WHEN $6 THEN now() ELSE finished_at END, \
            updated_at = now() \
         WHERE id = $7",
    )
    .bind(&update.status)
    .bind(update.reason.as_deref())
    .bind(update.git_commit.as_deref())
    .bind(update.image_digest.as_deref())
    .bind(update.image_tag.as_deref())
    .bind(terminal)
    .bind(&build_id)
    .execute(state.pool())
    .await?;

    let Some(deployment_id) = build.deployment_id.clone() else {
        return Ok(());
    };

    match update.status.as_str() {
        "succeeded" => {
            let image_tag = update
                .image_tag
                .or(build.image_tag.clone())
                .ok_or_else(|| {
                    ApiError::Validation("succeeded build must include image_tag".into())
                })?;
            if let Some(commit) = update.git_commit.as_deref() {
                sqlx::query(
                    "UPDATE services SET git_commit = $1, updated_at = now() WHERE id = $2",
                )
                .bind(commit)
                .bind(&build.service_id)
                .execute(state.pool())
                .await?;
            }
            sqlx::query("UPDATE services SET image_ref = $1, updated_at = now() WHERE id = $2")
                .bind(&image_tag)
                .bind(&build.service_id)
                .execute(state.pool())
                .await?;
            sqlx::query(
                "UPDATE deployments SET image_ref = $1, status = 'pending', \
                                        reason = 'build succeeded', updated_at = now() \
                 WHERE id = $2",
            )
            .bind(&image_tag)
            .bind(&deployment_id)
            .execute(state.pool())
            .await?;
            // Release the small build reservation so pull_and_run can pick a runtime node.
            sqlx::query("DELETE FROM node_allocations WHERE deployment_id = $1")
                .bind(&deployment_id)
                .execute(state.pool())
                .await?;
            crate::scheduler::nudge(&state);
        }
        "failed" => {
            let reason = update.reason.unwrap_or_else(|| "build failed".to_string());
            sqlx::query(
                "UPDATE deployments SET status = 'errored', reason = $1, \
                                        stopped_at = now(), updated_at = now() \
                 WHERE id = $2",
            )
            .bind(&reason)
            .bind(&deployment_id)
            .execute(state.pool())
            .await?;
            sqlx::query("DELETE FROM node_allocations WHERE deployment_id = $1")
                .bind(&deployment_id)
                .execute(state.pool())
                .await?;
        }
        _ => {}
    }

    Ok(())
}

fn log_event(id: i64, stream: &str, ts: &DateTime<Utc>, line: &str) -> Event {
    Event::default()
        .id(id.to_string())
        .event("log")
        .json_data(serde_json::json!({
            "id": id,
            "stream": stream,
            "ts": ts,
            "text": line,
        }))
        .unwrap_or_else(|_| Event::default().data(""))
}
