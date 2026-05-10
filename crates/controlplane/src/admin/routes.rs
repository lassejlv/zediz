use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::auth::extractor::AuthUser;
use crate::error::ApiResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/users", get(list_users))
        .route("/admin/users/:id/approve", post(approve_user))
        .route("/admin/users/:id/reject", post(reject_user))
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
