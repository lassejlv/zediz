use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use zediz_common::Id;

use crate::auth::AuthUser;
use crate::builds;
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

const SERVICE_COLUMNS: &str = "id, slug, name, source, image_ref, env_vars, ports, resources, \
     replicas, restart_policy, git_repo, git_branch, git_commit, \
     dockerfile_path, root_dir, builder, registry_repo, \
     github_credential_id, registry_credential_id, created_at, updated_at";

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
    pub git_repo: Option<String>,
    pub git_branch: Option<String>,
    pub git_commit: Option<String>,
    pub dockerfile_path: Option<String>,
    pub root_dir: Option<String>,
    pub builder: String,
    pub registry_repo: Option<String>,
    pub github_credential_id: Option<Id>,
    pub registry_credential_id: Option<Id>,
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
    git_repo: Option<String>,
    git_branch: Option<String>,
    git_commit: Option<String>,
    dockerfile_path: Option<String>,
    root_dir: Option<String>,
    builder: String,
    registry_repo: Option<String>,
    github_credential_id: Option<String>,
    registry_credential_id: Option<String>,
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
            git_repo: r.git_repo,
            git_branch: r.git_branch,
            git_commit: r.git_commit,
            dockerfile_path: r.dockerfile_path,
            root_dir: r.root_dir,
            builder: r.builder,
            registry_repo: r.registry_repo,
            github_credential_id: r
                .github_credential_id
                .map(|s| s.parse())
                .transpose()
                .map_err(|e: ulid::DecodeError| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            registry_credential_id: r
                .registry_credential_id
                .map(|s| s.parse())
                .transpose()
                .map_err(|e: ulid::DecodeError| ApiError::Internal(anyhow::anyhow!("{e}")))?,
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

    let rows: Vec<ServiceRow> = sqlx::query_as(&format!(
        "SELECT {cols} FROM services WHERE project_id = $1 ORDER BY created_at DESC",
        cols = SERVICE_COLUMNS,
    ))
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
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub image_ref: Option<String>,
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
    // git source fields
    #[serde(default)]
    pub git_repo: Option<String>,
    #[serde(default)]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub dockerfile_path: Option<String>,
    #[serde(default)]
    pub root_dir: Option<String>,
    #[serde(default)]
    pub builder: Option<String>,
    #[serde(default)]
    pub registry_repo: Option<String>,
    #[serde(default)]
    pub github_credential_id: Option<String>,
    #[serde(default)]
    pub registry_credential_id: Option<String>,
}

fn default_replicas() -> i32 {
    1
}
fn default_restart_policy() -> String {
    "on-failure".into()
}

fn trim_opt(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
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
    // Reserved because each maps to a literal frontend route under
    // `/w/<ws>/projects/<p>/<here>` — a service with one of these
    // slugs would shadow the route.
    if matches!(service_slug.as_str(), "new" | "templates") {
        return Err(ApiError::Validation(format!(
            "slug '{service_slug}' is reserved — try another"
        )));
    }
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::Validation("name is required".into()));
    }
    if !matches!(req.restart_policy.as_str(), "no" | "on-failure" | "always") {
        return Err(ApiError::Validation("invalid restart_policy".into()));
    }
    if req.replicas < 1 {
        return Err(ApiError::Validation("replicas must be >= 1".into()));
    }

    let source = req.source.as_deref().unwrap_or("image");
    if !matches!(source, "image" | "git") {
        return Err(ApiError::Validation(
            "source must be 'image' or 'git'".into(),
        ));
    }

    // Branch on source to pick which fields are required and which are rejected.
    let (
        image_ref,
        git_repo,
        git_branch,
        dockerfile_path,
        root_dir,
        builder,
        registry_repo,
        github_credential_id,
        registry_credential_id,
    );
    match source {
        "image" => {
            let img = trim_opt(req.image_ref)
                .ok_or_else(|| ApiError::Validation("image_ref is required".into()))?;
            if req.git_repo.is_some()
                || req.git_branch.is_some()
                || req.dockerfile_path.is_some()
                || req.root_dir.is_some()
                || req.builder.is_some()
                || req.registry_repo.is_some()
                || req.github_credential_id.is_some()
                || req.registry_credential_id.is_some()
            {
                return Err(ApiError::Validation(
                    "git fields not allowed for image services".into(),
                ));
            }
            image_ref = Some(img);
            git_repo = None;
            git_branch = None;
            dockerfile_path = None;
            root_dir = None;
            builder = "dockerfile".to_string();
            registry_repo = None;
            github_credential_id = None;
            registry_credential_id = None;
        }
        "git" => {
            git_repo = Some(
                trim_opt(req.git_repo)
                    .ok_or_else(|| ApiError::Validation("git_repo is required".into()))?,
            );
            git_branch = Some(trim_opt(req.git_branch).unwrap_or_else(|| "main".into()));
            let chosen_builder = req.builder.as_deref().unwrap_or("dockerfile").to_string();
            if !matches!(chosen_builder.as_str(), "dockerfile" | "railpack") {
                return Err(ApiError::Validation(
                    "builder must be 'dockerfile' or 'railpack'".into(),
                ));
            }
            dockerfile_path = if chosen_builder == "dockerfile" {
                Some(trim_opt(req.dockerfile_path).unwrap_or_else(|| "Dockerfile".into()))
            } else {
                // dockerfile_path is ignored for Railpack; store NULL so it
                // doesn't look meaningful in the UI.
                None
            };
            builder = chosen_builder;
            root_dir = Some(trim_opt(req.root_dir).unwrap_or_else(|| ".".into()));
            registry_repo = normalize_registry_repo(req.registry_repo);
            github_credential_id = trim_opt(req.github_credential_id);
            registry_credential_id = trim_opt(req.registry_credential_id);
            // image_ref starts NULL; filled in by the first successful build.
            image_ref = None;
        }
        _ => unreachable!(),
    }

    let resources = req.resources.unwrap_or_default();

    let id = Id::new();
    let insert_sql = format!(
        "INSERT INTO services ( \
            id, project_id, slug, name, source, image_ref, env_vars, ports, resources, \
            replicas, restart_policy, git_repo, git_branch, dockerfile_path, \
            root_dir, builder, registry_repo, github_credential_id, registry_credential_id \
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19) \
         ON CONFLICT (project_id, slug) DO NOTHING \
         RETURNING {cols}",
        cols = SERVICE_COLUMNS,
    );
    let inserted: Option<ServiceRow> = sqlx::query_as(&insert_sql)
        .bind(id.to_string())
        .bind(project_id.to_string())
        .bind(&service_slug)
        .bind(&name)
        .bind(source)
        .bind(image_ref.as_deref())
        .bind(json!(req.env_vars))
        .bind(json!(req.ports))
        .bind(json!(resources))
        .bind(req.replicas)
        .bind(&req.restart_policy)
        .bind(git_repo.as_deref())
        .bind(git_branch.as_deref())
        .bind(dockerfile_path.as_deref())
        .bind(root_dir.as_deref())
        .bind(&builder)
        .bind(registry_repo.as_deref())
        .bind(github_credential_id.as_deref())
        .bind(registry_credential_id.as_deref())
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

    let row: Option<ServiceRow> = sqlx::query_as(&format!(
        "SELECT {cols} FROM services WHERE project_id = $1 AND slug = $2",
        cols = SERVICE_COLUMNS,
    ))
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
    #[serde(default)]
    pub git_repo: Option<String>,
    #[serde(default)]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub dockerfile_path: Option<String>,
    #[serde(default)]
    pub root_dir: Option<String>,
    #[serde(default)]
    pub builder: Option<String>,
    #[serde(default)]
    pub registry_repo: Option<String>,
    #[serde(default)]
    pub github_credential_id: Option<String>,
    #[serde(default)]
    pub registry_credential_id: Option<String>,
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
    if let Some(b) = &req.builder {
        if !matches!(b.as_str(), "dockerfile" | "railpack") {
            return Err(ApiError::Validation(
                "builder must be 'dockerfile' or 'railpack'".into(),
            ));
        }
    }

    let registry_repo = normalize_registry_repo(req.registry_repo);

    let row: ServiceRow = sqlx::query_as(&format!(
        "UPDATE services SET \
            name = COALESCE($1, name), \
            image_ref = COALESCE($2, image_ref), \
            env_vars = COALESCE($3, env_vars), \
            ports = COALESCE($4, ports), \
            resources = COALESCE($5, resources), \
            replicas = COALESCE($6, replicas), \
            restart_policy = COALESCE($7, restart_policy), \
            git_repo = COALESCE($8, git_repo), \
            git_branch = COALESCE($9, git_branch), \
            dockerfile_path = COALESCE($10, dockerfile_path), \
            root_dir = COALESCE($11, root_dir), \
            builder = COALESCE($12, builder), \
            registry_repo = COALESCE($13, registry_repo), \
            github_credential_id = COALESCE($14, github_credential_id), \
            registry_credential_id = COALESCE($15, registry_credential_id), \
            updated_at = now() \
         WHERE project_id = $16 AND slug = $17 \
         RETURNING {cols}",
        cols = SERVICE_COLUMNS,
    ))
    .bind(req.name.as_deref())
    .bind(req.image_ref.as_deref())
    .bind(req.env_vars.map(|v| json!(v)))
    .bind(req.ports.map(|v| json!(v)))
    .bind(req.resources.map(|v| json!(v)))
    .bind(req.replicas)
    .bind(req.restart_policy.as_deref())
    .bind(req.git_repo.as_deref())
    .bind(req.git_branch.as_deref())
    .bind(req.dockerfile_path.as_deref())
    .bind(req.root_dir.as_deref())
    .bind(req.builder.as_deref())
    .bind(registry_repo.as_deref())
    .bind(req.github_credential_id.as_deref())
    .bind(req.registry_credential_id.as_deref())
    .bind(project_id.to_string())
    .bind(&service_slug)
    .fetch_optional(state.pool())
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(ServiceSummary::try_from(row)?))
}

fn normalize_registry_repo(repo: Option<String>) -> Option<String> {
    trim_opt(repo).map(|value| value.to_ascii_lowercase())
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

    let service: Option<ServiceRow> = sqlx::query_as(&format!(
        "SELECT {cols} FROM services WHERE project_id = $1 AND slug = $2",
        cols = SERVICE_COLUMNS,
    ))
    .bind(project_id.to_string())
    .bind(&service_slug)
    .fetch_optional(state.pool())
    .await?;
    let service = service.ok_or(ApiError::NotFound)?;
    let summary = ServiceSummary::try_from(service)?;

    // Rolling cutover works when the old and new containers can coexist
    // on the node: Caddy routes via the internal network by container
    // name, so domain-only services don't conflict. But:
    //   - A published host_port is a singleton on the host — Docker
    //     rejects the second bind with "port is already allocated".
    //   - A Hetzner volume can only be attached to one server at a
    //     time, so overlapping deployments can't both mount it.
    // Either case falls back to the pre-rolling flow: stop the old
    // deployment before scheduling the new one.
    let uses_host_ports = summary.ports.iter().any(|p| p.host_port.is_some());
    let has_volume = crate::volumes::fetch_for_service(state.pool(), &summary.id.to_string())
        .await?
        .is_some();
    cancel_pre_deploy(
        state.pool(),
        &summary.id.to_string(),
        uses_host_ports || has_volume,
    )
    .await?;

    let deployment = match summary.source.as_str() {
        "image" => {
            let image = summary
                .image_ref
                .clone()
                .ok_or_else(|| ApiError::Validation("service has no image_ref".into()))?;
            deployments::create_deployment(state.pool(), &summary, &image, &ctx.workspace_id)
                .await?
        }
        "git" => {
            if summary.git_repo.is_none() {
                return Err(ApiError::Validation(
                    "git service missing git_repo — fix it in Settings".into(),
                ));
            }
            // image_ref is filled in by the build; the deployment holds a
            // placeholder until then so the row insert (which NOT NULLs
            // image_ref) succeeds. The scheduler won't try to run it while
            // status = 'building'.
            let d = deployments::create_deployment(
                state.pool(),
                &summary,
                "pending-build",
                &ctx.workspace_id,
            )
            .await?;
            sqlx::query(
                "UPDATE deployments SET status = 'building', reason = 'awaiting build', \
                                        updated_at = now() \
                 WHERE id = $1",
            )
            .bind(d.id.to_string())
            .execute(state.pool())
            .await?;
            builds::create_queued(state.pool(), &summary.id.to_string(), &d.id.to_string()).await?;
            d
        }
        other => return Err(ApiError::Validation(format!("unknown source '{other}'"))),
    };

    crate::scheduler::nudge(&state);

    Ok(Json(deployment))
}

/// Retire deployments that are about to be superseded by a new deploy.
///
/// `include_running=false` (the default flow for domain-routed services):
/// skip the currently-running deployment so it keeps serving traffic
/// until `deployments::retire_superseded_running` cuts over atomically.
///
/// `include_running=true` (services that publish a host port): retire
/// the running deployment too so Docker can bind that port for the new
/// container. Enqueued Stop runs before the new PullAndRun on the agent
/// because commands are dispatched in `created_at` order.
async fn cancel_pre_deploy(
    pool: &sqlx::PgPool,
    service_id: &str,
    include_running: bool,
) -> ApiResult<()> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, node_id FROM deployments \
         WHERE service_id = $1 \
           AND (status IN ('pending','building','placing','pulling','starting','failing') \
                OR ($2 AND status = 'running'))",
    )
    .bind(service_id)
    .bind(include_running)
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
        // Abandon any in-flight builds tied to this deployment.
        sqlx::query(
            "UPDATE builds SET status = 'cancelled', \
                               reason = COALESCE(reason, 'superseded'), \
                               finished_at = now(), updated_at = now() \
             WHERE deployment_id = $1 AND status NOT IN ('succeeded','failed','cancelled')",
        )
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
