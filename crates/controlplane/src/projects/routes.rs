use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use driftbase_common::Id;
use serde::{Deserialize, Serialize};

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
    pub hetzner_location: String,
    pub private_network_enabled: bool,
    pub private_network_domain: String,
    pub created_at: DateTime<Utc>,
}

#[derive(sea_orm::FromQueryResult)]
struct ProjectRow {
    id: String,
    slug: String,
    name: String,
    hetzner_location: String,
    private_network_domain: Option<String>,
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
            hetzner_location: r.hetzner_location,
            private_network_enabled: true,
            private_network_domain: r
                .private_network_domain
                .unwrap_or_else(|| crate::private_network::DOMAIN.to_string()),
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

    let rows: Vec<ProjectRow> = crate::db::query_as(
        "SELECT p.id, p.slug, p.name, p.hetzner_location, \
                pn.domain AS private_network_domain, p.created_at \
         FROM projects p \
         LEFT JOIN project_networks pn ON pn.project_id = p.id \
         WHERE p.workspace_id = $1 ORDER BY p.created_at DESC",
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
    pub hetzner_location: String,
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
    let hetzner_location = req.hetzner_location.trim().to_lowercase();
    validate_hetzner_location(&hetzner_location)?;

    let id = Id::new();
    let inserted: Option<ProjectRow> = crate::db::query_as(
        "INSERT INTO projects (id, workspace_id, slug, name, created_by, hetzner_location) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (workspace_id, slug) DO NOTHING \
         RETURNING id, slug, name, hetzner_location, NULL::text AS private_network_domain, created_at",
    )
    .bind(id.to_string())
    .bind(ctx.workspace_id.to_string())
    .bind(&project_slug)
    .bind(&name)
    .bind(auth.user_id.to_string())
    .bind(&hetzner_location)
    .fetch_optional(state.pool())
    .await?;

    let row = inserted.ok_or_else(|| ApiError::Conflict("slug already taken".into()))?;
    crate::private_network::ensure_project_network(state.pool(), &row.id).await?;
    Ok(Json(ProjectSummary::try_from(row)?))
}

async fn show(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug)): Path<(String, String)>,
) -> ApiResult<Json<ProjectSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;

    let row: Option<ProjectRow> = crate::db::query_as(
        "SELECT p.id, p.slug, p.name, p.hetzner_location, \
                pn.domain AS private_network_domain, p.created_at \
         FROM projects p \
         LEFT JOIN project_networks pn ON pn.project_id = p.id \
         WHERE p.workspace_id = $1 AND p.slug = $2",
    )
    .bind(ctx.workspace_id.to_string())
    .bind(&project_slug)
    .fetch_optional(state.pool())
    .await?;

    let row = row.ok_or(ApiError::NotFound)?;
    Ok(Json(ProjectSummary::try_from(row)?))
}

pub(crate) fn validate_hetzner_location(location: &str) -> ApiResult<()> {
    if matches!(location, "nbg1" | "fsn1" | "hel1" | "ash" | "hil" | "sin") {
        return Ok(());
    }
    Err(ApiError::Validation("unsupported Hetzner location".into()))
}

async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug)): Path<(String, String)>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    let res = crate::db::query("DELETE FROM projects WHERE workspace_id = $1 AND slug = $2")
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
    pool: &sea_orm::DatabaseConnection,
    project_id: &str,
) -> ApiResult<projects::ProjectContext> {
    projects::resolve(pool, project_id).await
}
