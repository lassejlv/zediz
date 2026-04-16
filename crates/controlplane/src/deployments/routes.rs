use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};

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
        sqlx::query(
            "UPDATE deployments SET status = 'stopped', stopped_at = now(), updated_at = now() \
             WHERE id = $1",
        )
        .bind(&id)
        .execute(state.pool())
        .await?;
        sqlx::query("DELETE FROM node_allocations WHERE deployment_id = $1")
            .bind(&id)
            .execute(state.pool())
            .await?;
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

    sqlx::query(
        "UPDATE deployments SET status = 'pending', container_id = NULL, \
                                node_id = NULL, reason = NULL, \
                                started_at = NULL, stopped_at = NULL, updated_at = now() \
         WHERE id = $1",
    )
    .bind(&id)
    .execute(state.pool())
    .await?;

    sqlx::query("DELETE FROM node_allocations WHERE deployment_id = $1")
        .bind(&id)
        .execute(state.pool())
        .await?;

    // Restart is an explicit "start over" intent — clear any scheduler pause
    // left over from a node delete so provisioning can kick off immediately.
    sqlx::query(
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
