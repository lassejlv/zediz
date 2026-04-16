use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use zediz_common::Id;

use crate::auth::AuthUser;
use crate::deployments;
use crate::error::{ApiError, ApiResult};
use crate::projects::validate_slug;
use crate::services::{EnvVars, PortMap, Resources};
use crate::state::AppState;
use crate::workspaces::membership::{self, Role};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/workspaces/:slug/projects/:project_slug/services",
            get(list).post(create),
        )
        .route(
            "/workspaces/:slug/projects/:project_slug/services/:service_slug",
            get(show).patch(update).delete(delete),
        )
        .route(
            "/workspaces/:slug/projects/:project_slug/services/:service_slug/deploy",
            axum::routing::post(deploy),
        )
        .route(
            "/workspaces/:slug/projects/:project_slug/services/:service_slug/deployments",
            get(list_deployments),
        )
}

#[derive(Serialize)]
pub struct ServiceSummary {
    pub id: Id,
    pub slug: String,
    pub name: String,
    pub source: String,
    pub image_ref: Option<String>,
    pub env_vars: EnvVars,
    pub ports: Vec<PortMap>,
    pub resources: Resources,
    pub replicas: i32,
    pub restart_policy: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct ServiceRow {
    id: String,
    slug: String,
    name: String,
    source: String,
    image_ref: Option<String>,
    env_vars: JsonValue,
    ports: JsonValue,
    resources: JsonValue,
    replicas: i32,
    restart_policy: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ServiceRow> for ServiceSummary {
    type Error = ApiError;
    fn try_from(r: ServiceRow) -> Result<Self, ApiError> {
        Ok(Self {
            id: r
                .id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            slug: r.slug,
            name: r.name,
            source: r.source,
            image_ref: r.image_ref,
            env_vars: serde_json::from_value(r.env_vars)
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("env_vars: {e}")))?,
            ports: serde_json::from_value(r.ports)
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("ports: {e}")))?,
            resources: serde_json::from_value(r.resources)
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("resources: {e}")))?,
            replicas: r.replicas,
            restart_policy: r.restart_policy,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

async fn resolve_project(
    pool: &sqlx::PgPool,
    workspace_id: &Id,
    project_slug: &str,
) -> ApiResult<Id> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT id FROM projects WHERE workspace_id = $1 AND slug = $2")
            .bind(workspace_id.to_string())
            .bind(project_slug)
            .fetch_optional(pool)
            .await?;
    let (id,) = row.ok_or(ApiError::NotFound)?;
    id.parse()
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug)): Path<(String, String)>,
) -> ApiResult<Json<Vec<ServiceSummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    let project_id = resolve_project(state.pool(), &ctx.workspace_id, &project_slug).await?;

    let rows: Vec<ServiceRow> = sqlx::query_as(
        "SELECT id, slug, name, source, image_ref, env_vars, ports, resources, \
                replicas, restart_policy, created_at, updated_at \
         FROM services WHERE project_id = $1 ORDER BY created_at DESC",
    )
    .bind(project_id.to_string())
    .fetch_all(state.pool())
    .await?;

    rows.into_iter()
        .map(ServiceSummary::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

#[derive(Deserialize)]
pub struct CreateServiceRequest {
    pub slug: String,
    pub name: String,
    pub image_ref: String,
    #[serde(default)]
    pub env_vars: EnvVars,
    #[serde(default)]
    pub ports: Vec<PortMap>,
    #[serde(default)]
    pub resources: Option<Resources>,
    #[serde(default = "default_replicas")]
    pub replicas: i32,
    #[serde(default = "default_restart_policy")]
    pub restart_policy: String,
}

fn default_replicas() -> i32 {
    1
}
fn default_restart_policy() -> String {
    "on-failure".into()
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug)): Path<(String, String)>,
    Json(req): Json<CreateServiceRequest>,
) -> ApiResult<Json<ServiceSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;
    let project_id = resolve_project(state.pool(), &ctx.workspace_id, &project_slug).await?;

    let service_slug = req.slug.trim().to_lowercase();
    validate_slug(&service_slug)?;
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::Validation("name is required".into()));
    }
    if req.image_ref.trim().is_empty() {
        return Err(ApiError::Validation("image_ref is required".into()));
    }
    if !matches!(req.restart_policy.as_str(), "no" | "on-failure" | "always") {
        return Err(ApiError::Validation("invalid restart_policy".into()));
    }
    if req.replicas < 1 {
        return Err(ApiError::Validation("replicas must be >= 1".into()));
    }

    let resources = req.resources.unwrap_or_default();

    let id = Id::new();
    let inserted: Option<ServiceRow> = sqlx::query_as(
        "INSERT INTO services (id, project_id, slug, name, source, image_ref, env_vars, \
                               ports, resources, replicas, restart_policy) \
         VALUES ($1, $2, $3, $4, 'image', $5, $6, $7, $8, $9, $10) \
         ON CONFLICT (project_id, slug) DO NOTHING \
         RETURNING id, slug, name, source, image_ref, env_vars, ports, resources, \
                   replicas, restart_policy, created_at, updated_at",
    )
    .bind(id.to_string())
    .bind(project_id.to_string())
    .bind(&service_slug)
    .bind(&name)
    .bind(req.image_ref.trim())
    .bind(json!(req.env_vars))
    .bind(json!(req.ports))
    .bind(json!(resources))
    .bind(req.replicas)
    .bind(&req.restart_policy)
    .fetch_optional(state.pool())
    .await?;

    let row = inserted.ok_or_else(|| ApiError::Conflict("slug already taken".into()))?;
    Ok(Json(ServiceSummary::try_from(row)?))
}

async fn show(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug)): Path<(String, String, String)>,
) -> ApiResult<Json<ServiceSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    let project_id = resolve_project(state.pool(), &ctx.workspace_id, &project_slug).await?;

    let row: Option<ServiceRow> = sqlx::query_as(
        "SELECT id, slug, name, source, image_ref, env_vars, ports, resources, \
                replicas, restart_policy, created_at, updated_at \
         FROM services WHERE project_id = $1 AND slug = $2",
    )
    .bind(project_id.to_string())
    .bind(&service_slug)
    .fetch_optional(state.pool())
    .await?;

    let row = row.ok_or(ApiError::NotFound)?;
    Ok(Json(ServiceSummary::try_from(row)?))
}

#[derive(Deserialize)]
pub struct UpdateServiceRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub image_ref: Option<String>,
    #[serde(default)]
    pub env_vars: Option<EnvVars>,
    #[serde(default)]
    pub ports: Option<Vec<PortMap>>,
    #[serde(default)]
    pub resources: Option<Resources>,
    #[serde(default)]
    pub replicas: Option<i32>,
    #[serde(default)]
    pub restart_policy: Option<String>,
}

async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug)): Path<(String, String, String)>,
    Json(req): Json<UpdateServiceRequest>,
) -> ApiResult<Json<ServiceSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;
    let project_id = resolve_project(state.pool(), &ctx.workspace_id, &project_slug).await?;

    if let Some(rp) = &req.restart_policy {
        if !matches!(rp.as_str(), "no" | "on-failure" | "always") {
            return Err(ApiError::Validation("invalid restart_policy".into()));
        }
    }
    if let Some(r) = req.replicas {
        if r < 1 {
            return Err(ApiError::Validation("replicas must be >= 1".into()));
        }
    }

    let row: ServiceRow = sqlx::query_as(
        "UPDATE services SET \
            name = COALESCE($1, name), \
            image_ref = COALESCE($2, image_ref), \
            env_vars = COALESCE($3, env_vars), \
            ports = COALESCE($4, ports), \
            resources = COALESCE($5, resources), \
            replicas = COALESCE($6, replicas), \
            restart_policy = COALESCE($7, restart_policy), \
            updated_at = now() \
         WHERE project_id = $8 AND slug = $9 \
         RETURNING id, slug, name, source, image_ref, env_vars, ports, resources, \
                   replicas, restart_policy, created_at, updated_at",
    )
    .bind(req.name.as_deref())
    .bind(req.image_ref.as_deref())
    .bind(req.env_vars.map(|v| json!(v)))
    .bind(req.ports.map(|v| json!(v)))
    .bind(req.resources.map(|v| json!(v)))
    .bind(req.replicas)
    .bind(req.restart_policy.as_deref())
    .bind(project_id.to_string())
    .bind(&service_slug)
    .fetch_optional(state.pool())
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(ServiceSummary::try_from(row)?))
}

async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug)): Path<(String, String, String)>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;
    let project_id = resolve_project(state.pool(), &ctx.workspace_id, &project_slug).await?;

    let res = sqlx::query("DELETE FROM services WHERE project_id = $1 AND slug = $2")
        .bind(project_id.to_string())
        .bind(&service_slug)
        .execute(state.pool())
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

async fn deploy(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug)): Path<(String, String, String)>,
) -> ApiResult<Json<deployments::DeploymentSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;
    let project_id = resolve_project(state.pool(), &ctx.workspace_id, &project_slug).await?;

    let service: Option<ServiceRow> = sqlx::query_as(
        "SELECT id, slug, name, source, image_ref, env_vars, ports, resources, \
                replicas, restart_policy, created_at, updated_at \
         FROM services WHERE project_id = $1 AND slug = $2",
    )
    .bind(project_id.to_string())
    .bind(&service_slug)
    .fetch_optional(state.pool())
    .await?;
    let service = service.ok_or(ApiError::NotFound)?;
    let summary = ServiceSummary::try_from(service)?;

    let image = summary
        .image_ref
        .clone()
        .ok_or_else(|| ApiError::Validation("service has no image_ref".into()))?;

    // Replace any active deployments for this service — redeploy semantics.
    // Tells the agent to stop the old container (freeing host ports, etc.) and
    // flips the old row to `stopped` so the scheduler won't touch it again.
    retire_active_deployments(state.pool(), &summary.id.to_string()).await?;

    let deployment =
        deployments::create_deployment(state.pool(), &summary, &image, &ctx.workspace_id).await?;

    // Fire-and-forget — scheduler tick will pick it up shortly too.
    crate::scheduler::nudge(&state);

    Ok(Json(deployment))
}

async fn retire_active_deployments(pool: &sqlx::PgPool, service_id: &str) -> ApiResult<()> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, node_id FROM deployments \
         WHERE service_id = $1 \
               AND status IN ('pending', 'placing', 'pulling', 'starting', 'running', 'failing')",
    )
    .bind(service_id)
    .fetch_all(pool)
    .await?;

    for (deployment_id, node_id) in rows {
        if let Some(node_id) = node_id {
            // Best-effort — if the agent is down the row still gets marked stopped.
            let _ = crate::agent::commands::enqueue(
                pool,
                &node_id,
                Some(&deployment_id),
                crate::agent::commands::CommandKind::Stop,
                serde_json::json!({}),
            )
            .await;
        }
        sqlx::query(
            "UPDATE deployments SET status = 'stopped', stopped_at = now(), \
                                    reason = 'replaced by new deployment', updated_at = now() \
             WHERE id = $1",
        )
        .bind(&deployment_id)
        .execute(pool)
        .await?;
        sqlx::query("DELETE FROM node_allocations WHERE deployment_id = $1")
            .bind(&deployment_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn list_deployments(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug)): Path<(String, String, String)>,
) -> ApiResult<Json<Vec<deployments::DeploymentSummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    let project_id = resolve_project(state.pool(), &ctx.workspace_id, &project_slug).await?;

    let service_id: Option<(String,)> =
        sqlx::query_as("SELECT id FROM services WHERE project_id = $1 AND slug = $2")
            .bind(project_id.to_string())
            .bind(&service_slug)
            .fetch_optional(state.pool())
            .await?;
    let (service_id,) = service_id.ok_or(ApiError::NotFound)?;

    let rows = deployments::list_for_service(state.pool(), &service_id).await?;
    Ok(Json(rows))
}
