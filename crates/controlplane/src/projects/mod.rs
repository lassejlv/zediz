pub mod routes;

use anyhow::anyhow;
use sqlx::PgPool;
use driftbase_common::Id;

use crate::error::{ApiError, ApiResult};

#[allow(dead_code)]
pub struct ProjectContext {
    pub project_id: Id,
    pub workspace_id: Id,
    pub slug: String,
}

pub async fn resolve(pool: &PgPool, project_id: &str) -> ApiResult<ProjectContext> {
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT id, workspace_id, slug FROM projects WHERE id = $1")
            .bind(project_id)
            .fetch_optional(pool)
            .await?;
    let (id, workspace_id, slug) = row.ok_or(ApiError::NotFound)?;
    Ok(ProjectContext {
        project_id: id.parse().map_err(|e| ApiError::Internal(anyhow!("{e}")))?,
        workspace_id: workspace_id
            .parse()
            .map_err(|e| ApiError::Internal(anyhow!("{e}")))?,
        slug,
    })
}

pub fn validate_slug(s: &str) -> ApiResult<()> {
    if !(2..=40).contains(&s.len()) {
        return Err(ApiError::Validation("slug must be 2–40 chars".into()));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ApiError::Validation(
            "slug: lowercase letters, digits, dashes only".into(),
        ));
    }
    if s.starts_with('-') || s.ends_with('-') {
        return Err(ApiError::Validation(
            "slug cannot start or end with '-'".into(),
        ));
    }
    Ok(())
}
