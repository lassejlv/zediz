use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};

use super::{list_for_service, BuildSummary};
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::workspaces::membership;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/workspaces/:slug/projects/:project_slug/services/:service_slug/builds",
        get(list),
    )
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug)): Path<(String, String, String)>,
) -> ApiResult<Json<Vec<BuildSummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;

    // Resolve service_id cheaply without pulling the full ServiceSummary.
    let row: Option<(String,)> = crate::db::query_tuple(
        "SELECT s.id FROM services s \
         JOIN projects p ON p.id = s.project_id \
         WHERE p.workspace_id = $1 AND p.slug = $2 AND s.slug = $3",
    )
    .bind(ctx.workspace_id.to_string())
    .bind(&project_slug)
    .bind(&service_slug)
    .fetch_optional(state.pool())
    .await?;
    let (service_id,) = row.ok_or(ApiError::NotFound)?;

    Ok(Json(list_for_service(state.pool(), &service_id).await?))
}
