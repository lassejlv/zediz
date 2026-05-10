use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use driftbase_common::Id;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::projects::{self, validate_slug};
use crate::state::AppState;
use crate::workspaces::membership::{self, Role};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces/:slug/projects", get(list).post(create))
        .route(
            "/workspaces/:slug/projects/:project_slug",
            get(show).delete(delete),
        )
}

#[derive(Serialize)]
pub struct ProjectSummary {
    pub id: Id,
    pub slug: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct ProjectRow {
    id: String,
    slug: String,
    name: String,
    created_at: DateTime<Utc>,
}

impl TryFrom<ProjectRow> for ProjectSummary {
    type Error = ApiError;
    fn try_from(r: ProjectRow) -> Result<Self, ApiError> {
        Ok(Self {
            id: r
                .id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            slug: r.slug,
            name: r.name,
            created_at: r.created_at,
        })
    }
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> ApiResult<Json<Vec<ProjectSummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;

    let rows: Vec<ProjectRow> = sqlx::query_as(
        "SELECT id, slug, name, created_at FROM projects \
         WHERE workspace_id = $1 ORDER BY created_at DESC",
    )
    .bind(ctx.workspace_id.to_string())
    .fetch_all(state.pool())
    .await?;

    rows.into_iter()
        .map(ProjectSummary::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub slug: String,
    pub name: String,
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
    Json(req): Json<CreateProjectRequest>,
) -> ApiResult<Json<ProjectSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;

    let project_slug = req.slug.trim().to_lowercase();
    validate_slug(&project_slug)?;
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::Validation("name is required".into()));
    }

    let id = Id::new();
    let inserted: Option<ProjectRow> = sqlx::query_as(
        "INSERT INTO projects (id, workspace_id, slug, name, created_by) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (workspace_id, slug) DO NOTHING \
         RETURNING id, slug, name, created_at",
    )
    .bind(id.to_string())
    .bind(ctx.workspace_id.to_string())
    .bind(&project_slug)
    .bind(&name)
    .bind(auth.user_id.to_string())
    .fetch_optional(state.pool())
    .await?;

    let row = inserted.ok_or_else(|| ApiError::Conflict("slug already taken".into()))?;
    Ok(Json(ProjectSummary::try_from(row)?))
}

async fn show(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug)): Path<(String, String)>,
) -> ApiResult<Json<ProjectSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;

    let row: Option<ProjectRow> = sqlx::query_as(
        "SELECT id, slug, name, created_at FROM projects \
         WHERE workspace_id = $1 AND slug = $2",
    )
    .bind(ctx.workspace_id.to_string())
    .bind(&project_slug)
    .fetch_optional(state.pool())
    .await?;

    let row = row.ok_or(ApiError::NotFound)?;
    Ok(Json(ProjectSummary::try_from(row)?))
}

async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug)): Path<(String, String)>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    let res = sqlx::query("DELETE FROM projects WHERE workspace_id = $1 AND slug = $2")
        .bind(ctx.workspace_id.to_string())
        .bind(&project_slug)
        .execute(state.pool())
        .await?;

    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

// Silence dead-code warning until /projects/:id/services etc. consume it.
#[allow(dead_code)]
pub(crate) async fn must_resolve(
    pool: &sqlx::PgPool,
    project_id: &str,
) -> ApiResult<projects::ProjectContext> {
    projects::resolve(pool, project_id).await
}
