use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::agent::commands::{self, CommandKind};
use crate::agent::routes::{tail_logs, TailQuery};
use crate::auth::AuthUser;
use crate::deployments::{self, DeploymentSummary};
use crate::error::ApiResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/deployments/:id", get(show))
        .route("/deployments/:id/stop", post(stop))
        .route("/deployments/:id/restart", post(restart))
        .route("/deployments/:id/logs", get(logs))
        .route("/deployments/:id/metrics", get(metrics_history))
}

async fn show(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<DeploymentSummary>> {
    let row = deployments::authorize(state.pool(), &id, &auth.user_id).await?;
    Ok(Json(DeploymentSummary::try_from(row)?))
}

async fn stop(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<DeploymentSummary>> {
    let row = deployments::authorize(state.pool(), &id, &auth.user_id).await?;

    if let Some(node_id) = row.node_id.as_deref() {
        commands::enqueue(
            state.pool(),
            node_id,
            Some(&id),
            CommandKind::Stop,
            serde_json::json!({}),
        )
        .await
        .map_err(|e| crate::error::ApiError::Internal(anyhow::anyhow!("{e}")))?;
    } else {
        // Never placed anywhere — just flip the row.
        crate::db::query(
            "UPDATE deployments SET status = 'stopped', stopped_at = now(), \
                                    private_ipv4 = NULL, updated_at = now() \
             WHERE id = $1",
        )
        .bind(&id)
        .execute(state.pool())
        .await?;
        crate::db::query("DELETE FROM node_allocations WHERE deployment_id = $1")
            .bind(&id)
            .execute(state.pool())
            .await?;
    }
    if let Err(e) = crate::private_network::sync_for_service(state.pool(), &row.service_id).await {
        tracing::warn!(error = ?e, service = %row.service_id, "sync private network after stop");
    }

    let updated = deployments::fetch_by_id(state.pool(), &id).await?;
    Ok(Json(DeploymentSummary::try_from(updated)?))
}

async fn restart(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<DeploymentSummary>> {
    let row = deployments::authorize(state.pool(), &id, &auth.user_id).await?;

    if let Some(node_id) = row.node_id.as_deref() {
        commands::enqueue(
            state.pool(),
            node_id,
            Some(&id),
            CommandKind::Remove,
            serde_json::json!({}),
        )
        .await
        .map_err(|e| crate::error::ApiError::Internal(anyhow::anyhow!("{e}")))?;
    }

    crate::db::query(
        "UPDATE deployments SET status = 'pending', container_id = NULL, \
                                node_id = NULL, reason = NULL, \
                                private_ipv4 = NULL, started_at = NULL, \
                                stopped_at = NULL, updated_at = now() \
         WHERE id = $1",
    )
    .bind(&id)
    .execute(state.pool())
    .await?;

    crate::db::query("DELETE FROM node_allocations WHERE deployment_id = $1")
        .bind(&id)
        .execute(state.pool())
        .await?;

    // Restart is an explicit "start over" intent — clear any scheduler pause
    // left over from a node delete so provisioning can kick off immediately.
    crate::db::query(
        "UPDATE workspaces SET scheduler_paused_until = NULL \
         WHERE id = (SELECT p.workspace_id FROM deployments d \
                     JOIN services s ON s.id = d.service_id \
                     JOIN projects p ON p.id = s.project_id \
                     WHERE d.id = $1)",
    )
    .bind(&id)
    .execute(state.pool())
    .await?;

    crate::scheduler::nudge(&state);
    if let Err(e) = crate::private_network::sync_for_service(state.pool(), &row.service_id).await {
        tracing::warn!(error = ?e, service = %row.service_id, "sync private network after restart");
    }

    let updated = deployments::fetch_by_id(state.pool(), &id).await?;
    Ok(Json(DeploymentSummary::try_from(updated)?))
}

async fn logs(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    q: Query<TailQuery>,
) -> ApiResult<impl axum::response::IntoResponse> {
    tail_logs(State(state), auth, Path(id), q).await
}

#[derive(Deserialize, Default)]
struct MetricsQuery {
    /// Window in minutes. Capped at the server's retention (60) so the
    /// agent doesn't accidentally ask for data that no longer exists.
    #[serde(default)]
    minutes: Option<i64>,
}

#[derive(Serialize)]
struct MetricsSample {
    ts: DateTime<Utc>,
    cpu_percent: f32,
    memory_bytes: i64,
    memory_limit_bytes: Option<i64>,
    rx_bytes: i64,
    tx_bytes: i64,
    /// Bytes per second since the previous sample. `None` on the first
    /// row (no prior sample to diff against) and when the counter
    /// resets — the agent reports cumulative values, so container
    /// recreates wrap back to 0.
    rx_rate: Option<f64>,
    tx_rate: Option<f64>,
}

#[derive(sea_orm::FromQueryResult)]
struct MetricsRow {
    ts: DateTime<Utc>,
    cpu_percent: f32,
    memory_bytes: i64,
    memory_limit_bytes: Option<i64>,
    rx_bytes: i64,
    tx_bytes: i64,
    rx_rate: Option<f64>,
    tx_rate: Option<f64>,
}

async fn metrics_history(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<MetricsQuery>,
) -> ApiResult<Json<Vec<MetricsSample>>> {
    deployments::authorize(state.pool(), &id, &auth.user_id).await?;

    let minutes = q.minutes.unwrap_or(60).clamp(1, 60);

    let rows: Vec<MetricsRow> = crate::db::query_as(
        "SELECT ts, cpu_percent, memory_bytes, memory_limit_bytes, rx_bytes, tx_bytes, \
                CASE WHEN prev_rx IS NULL OR rx_bytes < prev_rx OR dt_secs <= 0 \
                     THEN NULL \
                     ELSE (rx_bytes - prev_rx)::float8 / dt_secs END AS rx_rate, \
                CASE WHEN prev_tx IS NULL OR tx_bytes < prev_tx OR dt_secs <= 0 \
                     THEN NULL \
                     ELSE (tx_bytes - prev_tx)::float8 / dt_secs END AS tx_rate \
         FROM ( \
             SELECT ts, cpu_percent, memory_bytes, memory_limit_bytes, \
                    rx_bytes, tx_bytes, \
                    LAG(rx_bytes) OVER w AS prev_rx, \
                    LAG(tx_bytes) OVER w AS prev_tx, \
                    EXTRACT(EPOCH FROM ts - LAG(ts) OVER w) AS dt_secs \
             FROM deployment_metrics \
             WHERE deployment_id = $1 \
               AND ts >= now() - ($2 || ' minutes')::interval \
             WINDOW w AS (ORDER BY ts) \
         ) ordered \
         ORDER BY ts ASC",
    )
    .bind(&id)
    .bind(minutes)
    .fetch_all(state.pool())
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| MetricsSample {
                ts: r.ts,
                cpu_percent: r.cpu_percent,
                memory_bytes: r.memory_bytes,
                memory_limit_bytes: r.memory_limit_bytes,
                rx_bytes: r.rx_bytes,
                tx_bytes: r.tx_bytes,
                rx_rate: r.rx_rate,
                tx_rate: r.tx_rate,
            })
            .collect(),
    ))
}
