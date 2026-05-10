use axum::extract::{Path, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use driftbase_common::Id;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::projects::validate_slug;
use crate::state::AppState;
use crate::workspaces::membership::{self, Role};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces", post(create).get(list))
        .route("/workspaces/:slug", get(show).patch(update))
        .route("/workspaces/:slug/members", get(list_members))
        .route(
            "/workspaces/:slug/members/:user_id",
            patch(update_member).delete(remove_member),
        )
}

#[derive(Deserialize)]
pub struct CreateWorkspaceRequest {
    pub slug: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct WorkspaceSummary {
    pub id: Id,
    pub slug: String,
    pub name: String,
    pub role: Role,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub hetzner_location: Option<String>,
    #[serde(default)]
    pub default_server_type: Option<String>,
    #[serde(default)]
    pub max_nodes: Option<i32>,
    #[serde(default)]
    pub max_monthly_euro: Option<i32>,
    #[serde(default)]
    pub autoscale_idle_ttl_seconds: Option<i32>,
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateWorkspaceRequest>,
) -> ApiResult<Json<WorkspaceSummary>> {
    let slug = req.slug.trim().to_lowercase();
    validate_slug(&slug)?;
    // Reserved workspace slugs that would collide with literal app routes
    // under /w/. Keep this list tight — only add entries that actually
    // conflict with something in web/src/routes.
    if matches!(slug.as_str(), "new") {
        return Err(ApiError::Validation(
            "slug 'new' is reserved — try another".into(),
        ));
    }
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::Validation("name is required".into()));
    }

    let mut tx = state.pool().begin().await?;

    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM workspaces WHERE slug = $1")
        .bind(&slug)
        .fetch_optional(&mut *tx)
        .await?;
    if exists.is_some() {
        return Err(ApiError::Conflict("slug already taken".into()));
    }

    let id = Id::new();
    sqlx::query("INSERT INTO workspaces (id, slug, name, owner_user_id) VALUES ($1, $2, $3, $4)")
        .bind(id.to_string())
        .bind(&slug)
        .bind(&name)
        .bind(auth.user_id.to_string())
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(id.to_string())
    .bind(auth.user_id.to_string())
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(WorkspaceSummary {
        id,
        slug,
        name,
        role: Role::Owner,
        created_at: Utc::now(),
        hetzner_location: Some("nbg1".into()),
        default_server_type: None,
        max_nodes: Some(3),
        max_monthly_euro: Some(50),
        autoscale_idle_ttl_seconds: Some(600),
    }))
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> ApiResult<Json<Vec<WorkspaceSummary>>> {
    let rows: Vec<(String, String, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT w.id, w.slug, w.name, m.role, w.created_at \
         FROM workspaces w \
         JOIN workspace_members m ON m.workspace_id = w.id \
         WHERE m.user_id = $1 \
         ORDER BY w.created_at ASC",
    )
    .bind(auth.user_id.to_string())
    .fetch_all(state.pool())
    .await?;

    Ok(Json(
        rows.into_iter()
            .filter_map(|(id, slug, name, role, created_at)| {
                Some(WorkspaceSummary {
                    id: id.parse().ok()?,
                    slug,
                    name,
                    role: role.parse().ok()?,
                    created_at,
                    hetzner_location: None,
                    default_server_type: None,
                    max_nodes: None,
                    max_monthly_euro: None,
                    autoscale_idle_ttl_seconds: None,
                })
            })
            .collect(),
    ))
}

#[derive(sqlx::FromRow)]
struct WorkspaceRow {
    id: String,
    slug: String,
    name: String,
    role: String,
    created_at: DateTime<Utc>,
    hetzner_location: String,
    default_server_type: Option<String>,
    max_nodes: i32,
    max_monthly_euro: i32,
    autoscale_idle_ttl_seconds: i32,
}

impl WorkspaceRow {
    fn into_summary(self) -> ApiResult<WorkspaceSummary> {
        Ok(WorkspaceSummary {
            id: self
                .id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            slug: self.slug,
            name: self.name,
            role: self.role.parse().map_err(ApiError::Validation)?,
            created_at: self.created_at,
            hetzner_location: Some(self.hetzner_location),
            default_server_type: self.default_server_type,
            max_nodes: Some(self.max_nodes),
            max_monthly_euro: Some(self.max_monthly_euro),
            autoscale_idle_ttl_seconds: Some(self.autoscale_idle_ttl_seconds),
        })
    }
}

async fn show(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> ApiResult<Json<WorkspaceSummary>> {
    let row: Option<WorkspaceRow> = sqlx::query_as(
        "SELECT w.id, w.slug, w.name, m.role, w.created_at, \
                w.hetzner_location, w.default_server_type, w.max_nodes, \
                w.max_monthly_euro, w.autoscale_idle_ttl_seconds \
         FROM workspaces w \
         JOIN workspace_members m ON m.workspace_id = w.id \
         WHERE w.slug = $1 AND m.user_id = $2",
    )
    .bind(&slug)
    .bind(auth.user_id.to_string())
    .fetch_optional(state.pool())
    .await?;

    let row = row.ok_or(ApiError::NotFound)?;
    Ok(Json(row.into_summary()?))
}

#[derive(Deserialize)]
pub struct UpdateWorkspaceRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub hetzner_location: Option<String>,
    #[serde(default)]
    pub default_server_type: Option<String>,
    #[serde(default)]
    pub max_nodes: Option<i32>,
    #[serde(default)]
    pub max_monthly_euro: Option<i32>,
    #[serde(default)]
    pub autoscale_idle_ttl_seconds: Option<i32>,
}

async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
    Json(req): Json<UpdateWorkspaceRequest>,
) -> ApiResult<Json<WorkspaceSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    if let Some(n) = req.max_nodes {
        if !(1..=100).contains(&n) {
            return Err(ApiError::Validation("max_nodes must be 1..=100".into()));
        }
    }
    if let Some(e) = req.max_monthly_euro {
        if e < 0 {
            return Err(ApiError::Validation("max_monthly_euro must be >= 0".into()));
        }
    }
    if let Some(t) = req.autoscale_idle_ttl_seconds {
        if !(60..=86_400).contains(&t) {
            return Err(ApiError::Validation(
                "autoscale_idle_ttl_seconds must be 60..=86400".into(),
            ));
        }
    }

    sqlx::query(
        "UPDATE workspaces SET \
            name = COALESCE($1, name), \
            hetzner_location = COALESCE($2, hetzner_location), \
            default_server_type = COALESCE($3, default_server_type), \
            max_nodes = COALESCE($4, max_nodes), \
            max_monthly_euro = COALESCE($5, max_monthly_euro), \
            autoscale_idle_ttl_seconds = COALESCE($6, autoscale_idle_ttl_seconds) \
         WHERE id = $7",
    )
    .bind(req.name.as_deref())
    .bind(req.hetzner_location.as_deref())
    .bind(req.default_server_type.as_deref())
    .bind(req.max_nodes)
    .bind(req.max_monthly_euro)
    .bind(req.autoscale_idle_ttl_seconds)
    .bind(ctx.workspace_id.to_string())
    .execute(state.pool())
    .await?;

    let row: WorkspaceRow = sqlx::query_as(
        "SELECT w.id, w.slug, w.name, m.role, w.created_at, \
                w.hetzner_location, w.default_server_type, w.max_nodes, \
                w.max_monthly_euro, w.autoscale_idle_ttl_seconds \
         FROM workspaces w \
         JOIN workspace_members m ON m.workspace_id = w.id \
         WHERE w.id = $1 AND m.user_id = $2",
    )
    .bind(ctx.workspace_id.to_string())
    .bind(auth.user_id.to_string())
    .fetch_one(state.pool())
    .await?;
    Ok(Json(row.into_summary()?))
}

#[derive(Serialize)]
pub struct MemberRow {
    pub user_id: Id,
    pub email: String,
    pub display_name: String,
    pub role: Role,
    pub joined_at: DateTime<Utc>,
}

async fn list_members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> ApiResult<Json<Vec<MemberRow>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;

    let rows: Vec<(String, String, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT u.id, u.email, u.display_name, m.role, m.joined_at \
         FROM workspace_members m \
         JOIN users u ON u.id = m.user_id \
         WHERE m.workspace_id = $1 \
         ORDER BY m.joined_at ASC",
    )
    .bind(ctx.workspace_id.to_string())
    .fetch_all(state.pool())
    .await?;

    Ok(Json(
        rows.into_iter()
            .filter_map(|(user_id, email, display_name, role, joined_at)| {
                Some(MemberRow {
                    user_id: user_id.parse().ok()?,
                    email,
                    display_name,
                    role: role.parse().ok()?,
                    joined_at,
                })
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct UpdateMemberRequest {
    pub role: Role,
}

async fn update_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, target_user_id)): Path<(String, String)>,
    Json(req): Json<UpdateMemberRequest>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    if matches!(req.role, Role::Owner) {
        return Err(ApiError::Validation(
            "use ownership transfer to assign owner".into(),
        ));
    }
    if target_user_id == auth.user_id.to_string() {
        return Err(ApiError::Validation("cannot change your own role".into()));
    }

    let current: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(ctx.workspace_id.to_string())
    .bind(&target_user_id)
    .fetch_optional(state.pool())
    .await?;
    let (current,) = current.ok_or(ApiError::NotFound)?;
    if current == "owner" {
        return Err(ApiError::Forbidden(String::new()));
    }

    sqlx::query("UPDATE workspace_members SET role = $1 WHERE workspace_id = $2 AND user_id = $3")
        .bind(req.role.as_str())
        .bind(ctx.workspace_id.to_string())
        .bind(&target_user_id)
        .execute(state.pool())
        .await?;

    Ok(())
}

async fn remove_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, target_user_id)): Path<(String, String)>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;

    // Members can remove themselves; admins/owners can remove others (but not the owner).
    let is_self = target_user_id == auth.user_id.to_string();
    if !is_self {
        membership::require(&ctx, Role::Admin)?;
    }

    let target: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(ctx.workspace_id.to_string())
    .bind(&target_user_id)
    .fetch_optional(state.pool())
    .await?;
    let (target_role,) = target.ok_or(ApiError::NotFound)?;
    if target_role == "owner" {
        return Err(ApiError::Forbidden(String::new()));
    }

    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
        .bind(ctx.workspace_id.to_string())
        .bind(&target_user_id)
        .execute(state.pool())
        .await?;
    Ok(())
}
