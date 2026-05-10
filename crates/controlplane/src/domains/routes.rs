use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use driftbase_common::Id;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::domains::{self, validate_hostname};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::workspaces::membership::{self, Role};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/workspaces/:slug/projects/:project_slug/services/:service_slug/domains",
            get(list).post(create),
        )
        .route(
            "/workspaces/:slug/projects/:project_slug/services/:service_slug/domains/:id",
            axum::routing::patch(update).delete(delete),
        )
        .route(
            "/workspaces/:slug/projects/:project_slug/services/:service_slug/domains/:id/retry",
            post(retry),
        )
}

#[derive(Serialize)]
pub struct DomainSummary {
    pub id: Id,
    pub service_id: Id,
    pub hostname: String,
    pub container_port: i32,
    pub tls_status: String,
    pub last_error: Option<String>,
    pub last_cert_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(sea_orm::FromQueryResult)]
struct DomainRow {
    id: String,
    service_id: String,
    hostname: String,
    container_port: i32,
    tls_status: String,
    last_error: Option<String>,
    last_cert_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl TryFrom<DomainRow> for DomainSummary {
    type Error = ApiError;
    fn try_from(r: DomainRow) -> Result<Self, ApiError> {
        Ok(Self {
            id: r
                .id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            service_id: r
                .service_id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            hostname: r.hostname,
            container_port: r.container_port,
            tls_status: r.tls_status,
            last_error: r.last_error,
            last_cert_at: r.last_cert_at,
            created_at: r.created_at,
        })
    }
}

async fn resolve_service(
    pool: &sea_orm::DatabaseConnection,
    workspace_id: &Id,
    project_slug: &str,
    service_slug: &str,
) -> ApiResult<(Id, i32)> {
    #[derive(sea_orm::FromQueryResult)]
    struct Row {
        id: String,
        first_port: Option<i32>,
    }
    let row: Option<Row> = crate::db::query_as(
        "SELECT s.id, \
                (SELECT (p.value->>'container_port')::int \
                 FROM jsonb_array_elements(s.ports) p LIMIT 1) AS first_port \
         FROM services s \
         JOIN projects p ON p.id = s.project_id \
         WHERE p.workspace_id = $1 AND p.slug = $2 AND s.slug = $3",
    )
    .bind(workspace_id.to_string())
    .bind(project_slug)
    .bind(service_slug)
    .fetch_optional(pool)
    .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let id: Id = row
        .id
        .parse()
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?;
    Ok((id, row.first_port.unwrap_or(80)))
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug)): Path<(String, String, String)>,
) -> ApiResult<Json<Vec<DomainSummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    let (service_id, _) = resolve_service(
        state.pool(),
        &ctx.workspace_id,
        &project_slug,
        &service_slug,
    )
    .await?;

    let rows: Vec<DomainRow> = crate::db::query_as(
        "SELECT id, service_id, hostname, container_port, tls_status, \
                last_error, last_cert_at, created_at \
         FROM service_domains WHERE service_id = $1 ORDER BY created_at ASC",
    )
    .bind(service_id.to_string())
    .fetch_all(state.pool())
    .await?;

    rows.into_iter()
        .map(DomainSummary::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

#[derive(Deserialize)]
pub struct CreateDomainRequest {
    pub hostname: String,
    #[serde(default)]
    pub container_port: Option<i32>,
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug)): Path<(String, String, String)>,
    Json(req): Json<CreateDomainRequest>,
) -> ApiResult<Json<DomainSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;
    let (service_id, default_port) = resolve_service(
        state.pool(),
        &ctx.workspace_id,
        &project_slug,
        &service_slug,
    )
    .await?;

    let hostname = req.hostname.trim().to_lowercase();
    validate_hostname(&hostname).map_err(ApiError::Validation)?;

    let container_port = req.container_port.unwrap_or(default_port);
    if !(1..=65535).contains(&container_port) {
        return Err(ApiError::Validation("container_port out of range".into()));
    }

    let id = Id::new();
    let inserted: Option<DomainRow> = crate::db::query_as(
        "INSERT INTO service_domains (id, service_id, hostname, container_port) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (hostname) DO NOTHING \
         RETURNING id, service_id, hostname, container_port, tls_status, \
                   last_error, last_cert_at, created_at",
    )
    .bind(id.to_string())
    .bind(service_id.to_string())
    .bind(&hostname)
    .bind(container_port)
    .fetch_optional(state.pool())
    .await?;
    let row = inserted.ok_or_else(|| ApiError::Conflict("hostname already in use".into()))?;

    crate::scheduler::push_routes_for_service(state.pool(), &service_id.to_string())
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(DomainSummary::try_from(row)?))
}

#[derive(Deserialize)]
pub struct UpdateDomainRequest {
    #[serde(default)]
    pub container_port: Option<i32>,
}

async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug, id)): Path<(String, String, String, String)>,
    Json(req): Json<UpdateDomainRequest>,
) -> ApiResult<Json<DomainSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;
    let (service_id, _) = resolve_service(
        state.pool(),
        &ctx.workspace_id,
        &project_slug,
        &service_slug,
    )
    .await?;

    if let Some(p) = req.container_port {
        if !(1..=65535).contains(&p) {
            return Err(ApiError::Validation("container_port out of range".into()));
        }
    }

    let row: Option<DomainRow> = crate::db::query_as(
        "UPDATE service_domains SET \
            container_port = COALESCE($1, container_port), \
            updated_at = now() \
         WHERE id = $2 AND service_id = $3 \
         RETURNING id, service_id, hostname, container_port, tls_status, \
                   last_error, last_cert_at, created_at",
    )
    .bind(req.container_port)
    .bind(&id)
    .bind(service_id.to_string())
    .fetch_optional(state.pool())
    .await?;
    let row = row.ok_or(ApiError::NotFound)?;

    crate::scheduler::push_routes_for_service(state.pool(), &service_id.to_string())
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(DomainSummary::try_from(row)?))
}

async fn retry(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug, id)): Path<(String, String, String, String)>,
) -> ApiResult<Json<DomainSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;
    let (service_id, _) = resolve_service(
        state.pool(),
        &ctx.workspace_id,
        &project_slug,
        &service_slug,
    )
    .await?;

    // Reset to pending up-front so the row reflects the in-flight retry even
    // if the synchronous probe takes a moment.
    let row: Option<DomainRow> = crate::db::query_as(
        "UPDATE service_domains SET \
            tls_status = 'pending', \
            last_error = NULL, \
            updated_at = now() \
         WHERE id = $1 AND service_id = $2 \
         RETURNING id, service_id, hostname, container_port, tls_status, \
                   last_error, last_cert_at, created_at",
    )
    .bind(&id)
    .bind(service_id.to_string())
    .fetch_optional(state.pool())
    .await?;
    let row = row.ok_or(ApiError::NotFound)?;

    // Re-push routes — clears any stale Caddy config that might have been
    // mid-failover when the prior probe ran.
    crate::scheduler::push_routes_for_service(state.pool(), &service_id.to_string())
        .await
        .map_err(ApiError::Internal)?;

    // Probe synchronously so the response reflects the live result.
    domains::probe_one(state.pool(), &id)
        .await
        .map_err(ApiError::Internal)?;

    let refreshed: Option<DomainRow> = crate::db::query_as(
        "SELECT id, service_id, hostname, container_port, tls_status, \
                last_error, last_cert_at, created_at \
         FROM service_domains WHERE id = $1",
    )
    .bind(&id)
    .fetch_optional(state.pool())
    .await?;

    DomainSummary::try_from(refreshed.unwrap_or(row)).map(Json)
}

async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug, id)): Path<(String, String, String, String)>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;
    let (service_id, _) = resolve_service(
        state.pool(),
        &ctx.workspace_id,
        &project_slug,
        &service_slug,
    )
    .await?;

    let res = crate::db::query("DELETE FROM service_domains WHERE id = $1 AND service_id = $2")
        .bind(&id)
        .bind(service_id.to_string())
        .execute(state.pool())
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }

    crate::scheduler::push_routes_for_service(state.pool(), &service_id.to_string())
        .await
        .map_err(ApiError::Internal)?;

    // Clean up node-level state we no longer need.
    let _ = domains::nodes_for_service(state.pool(), &service_id.to_string()).await;

    Ok(())
}
