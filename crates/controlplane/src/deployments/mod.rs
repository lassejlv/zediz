pub mod routes;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value as JsonValue};
use sqlx::PgPool;
use zediz_common::Id;

use crate::error::{ApiError, ApiResult};
use crate::services::routes::ServiceSummary;

#[derive(Debug, Serialize)]
pub struct DeploymentSummary {
    pub id: Id,
    pub service_id: Id,
    pub node_id: Option<Id>,
    pub status: String,
    pub image_ref: String,
    pub container_id: Option<String>,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub struct DeploymentRow {
    pub id: String,
    pub service_id: String,
    pub node_id: Option<String>,
    pub status: String,
    pub image_ref: String,
    #[allow(dead_code)]
    pub env_vars: JsonValue,
    #[allow(dead_code)]
    pub ports: JsonValue,
    #[allow(dead_code)]
    pub resources: JsonValue,
    pub container_id: Option<String>,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

impl TryFrom<DeploymentRow> for DeploymentSummary {
    type Error = ApiError;
    fn try_from(r: DeploymentRow) -> Result<Self, ApiError> {
        Ok(Self {
            id: r
                .id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            service_id: r
                .service_id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            node_id: r
                .node_id
                .map(|s| s.parse())
                .transpose()
                .map_err(|e: ulid::DecodeError| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            status: r.status,
            image_ref: r.image_ref,
            container_id: r.container_id,
            reason: r.reason,
            created_at: r.created_at,
            started_at: r.started_at,
            stopped_at: r.stopped_at,
            updated_at: r.updated_at,
        })
    }
}

pub async fn create_deployment(
    pool: &PgPool,
    service: &ServiceSummary,
    image: &str,
    _workspace_id: &Id,
) -> ApiResult<DeploymentSummary> {
    let id = Id::new();
    let row: DeploymentRow = sqlx::query_as(
        "INSERT INTO deployments (id, service_id, status, image_ref, env_vars, ports, resources) \
         VALUES ($1, $2, 'pending', $3, $4, $5, $6) \
         RETURNING id, service_id, node_id, status, image_ref, env_vars, ports, resources, \
                   container_id, reason, created_at, started_at, stopped_at, updated_at",
    )
    .bind(id.to_string())
    .bind(service.id.to_string())
    .bind(image)
    .bind(json!(service.env_vars))
    .bind(json!(service.ports))
    .bind(json!(service.resources))
    .fetch_one(pool)
    .await?;

    DeploymentSummary::try_from(row)
}

pub async fn list_for_service(
    pool: &PgPool,
    service_id: &str,
) -> ApiResult<Vec<DeploymentSummary>> {
    let rows: Vec<DeploymentRow> = sqlx::query_as(
        "SELECT id, service_id, node_id, status, image_ref, env_vars, ports, resources, \
                container_id, reason, created_at, started_at, stopped_at, updated_at \
         FROM deployments WHERE service_id = $1 ORDER BY created_at DESC",
    )
    .bind(service_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(DeploymentSummary::try_from)
        .collect::<Result<Vec<_>, _>>()
}

pub async fn fetch_by_id(pool: &PgPool, deployment_id: &str) -> ApiResult<DeploymentRow> {
    let row: Option<DeploymentRow> = sqlx::query_as(
        "SELECT id, service_id, node_id, status, image_ref, env_vars, ports, resources, \
                container_id, reason, created_at, started_at, stopped_at, updated_at \
         FROM deployments WHERE id = $1",
    )
    .bind(deployment_id)
    .fetch_optional(pool)
    .await?;
    row.ok_or(ApiError::NotFound)
}

/// Retire any still-`running` deployments of this service that aren't `winner`.
/// For each: enqueue a Stop to its node, mark the row `stopped`, release its
/// allocation. Returns the node IDs whose route set may have changed.
///
/// This is what makes a redeploy rolling: the old container keeps serving
/// traffic until the new one reaches `running`; only then do we cut over.
pub async fn retire_superseded_running(
    pool: &PgPool,
    service_id: &str,
    winner_deployment_id: &str,
) -> ApiResult<Vec<String>> {
    use crate::agent::commands::{self, CommandKind};
    use std::collections::BTreeSet;

    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, node_id FROM deployments \
         WHERE service_id = $1 AND status = 'running' AND id <> $2",
    )
    .bind(service_id)
    .bind(winner_deployment_id)
    .fetch_all(pool)
    .await?;

    let mut nodes: BTreeSet<String> = BTreeSet::new();
    for (deployment_id, node_id) in rows {
        if let Some(ref node_id) = node_id {
            let _ = commands::enqueue(
                pool,
                node_id,
                Some(&deployment_id),
                CommandKind::Stop,
                json!({}),
            )
            .await;
            nodes.insert(node_id.clone());
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
    Ok(nodes.into_iter().collect())
}

/// Resolve `(workspace_id, service_id, project_id)` for a deployment, enforcing the
/// caller is at least a viewer in the owning workspace.
pub async fn authorize(
    pool: &PgPool,
    deployment_id: &str,
    user_id: &Id,
) -> ApiResult<DeploymentRow> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT w.id, m.role \
         FROM deployments d \
         JOIN services s ON s.id = d.service_id \
         JOIN projects p ON p.id = s.project_id \
         JOIN workspaces w ON w.id = p.workspace_id \
         JOIN workspace_members m ON m.workspace_id = w.id AND m.user_id = $1 \
         WHERE d.id = $2",
    )
    .bind(user_id.to_string())
    .bind(deployment_id)
    .fetch_optional(pool)
    .await?;
    row.ok_or(ApiError::NotFound)?;
    fetch_by_id(pool, deployment_id).await
}
