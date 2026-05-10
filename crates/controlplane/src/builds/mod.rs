pub mod routes;

use chrono::{DateTime, Utc};
use driftbase_common::Id;
use sea_orm::DatabaseConnection;
use serde::Serialize;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Serialize)]
pub struct BuildSummary {
    pub id: Id,
    pub service_id: Id,
    pub deployment_id: Option<Id>,
    pub node_id: Option<Id>,
    pub status: String,
    pub git_commit: Option<String>,
    pub image_digest: Option<String>,
    pub image_tag: Option<String>,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sea_orm::FromQueryResult)]
pub struct BuildRow {
    pub id: String,
    pub service_id: String,
    pub deployment_id: Option<String>,
    pub node_id: Option<String>,
    pub status: String,
    pub git_commit: Option<String>,
    pub image_digest: Option<String>,
    pub image_tag: Option<String>,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

impl TryFrom<BuildRow> for BuildSummary {
    type Error = ApiError;
    fn try_from(r: BuildRow) -> Result<Self, ApiError> {
        Ok(Self {
            id: r
                .id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            service_id: r
                .service_id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            deployment_id: r
                .deployment_id
                .map(|s| s.parse())
                .transpose()
                .map_err(|e: ulid::DecodeError| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            node_id: r
                .node_id
                .map(|s| s.parse())
                .transpose()
                .map_err(|e: ulid::DecodeError| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            status: r.status,
            git_commit: r.git_commit,
            image_digest: r.image_digest,
            image_tag: r.image_tag,
            reason: r.reason,
            created_at: r.created_at,
            started_at: r.started_at,
            finished_at: r.finished_at,
            updated_at: r.updated_at,
        })
    }
}

/// Insert a `queued` build row for `service_id` + `deployment_id` and return its id.
pub async fn create_queued(
    pool: &DatabaseConnection,
    service_id: &str,
    deployment_id: &str,
) -> ApiResult<Id> {
    let id = Id::new();
    crate::db::query(
        "INSERT INTO builds (id, service_id, deployment_id, status) \
         VALUES ($1, $2, $3, 'queued')",
    )
    .bind(id.to_string())
    .bind(service_id)
    .bind(deployment_id)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn list_for_service(
    pool: &DatabaseConnection,
    service_id: &str,
) -> ApiResult<Vec<BuildSummary>> {
    let rows: Vec<BuildRow> = crate::db::query_as(
        "SELECT id, service_id, deployment_id, node_id, status, git_commit, image_digest, \
                image_tag, reason, created_at, started_at, finished_at, updated_at \
         FROM builds WHERE service_id = $1 ORDER BY created_at DESC LIMIT 50",
    )
    .bind(service_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(BuildSummary::try_from)
        .collect::<Result<Vec<_>, _>>()
}

pub async fn fetch_by_id(pool: &DatabaseConnection, build_id: &str) -> ApiResult<BuildRow> {
    let row: Option<BuildRow> = crate::db::query_as(
        "SELECT id, service_id, deployment_id, node_id, status, git_commit, image_digest, \
                image_tag, reason, created_at, started_at, finished_at, updated_at \
         FROM builds WHERE id = $1",
    )
    .bind(build_id)
    .fetch_optional(pool)
    .await?;
    row.ok_or(ApiError::NotFound)
}
