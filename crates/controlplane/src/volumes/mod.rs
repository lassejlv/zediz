pub mod routes;

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use serde::Serialize;
use std::time::Duration;

use crate::credentials;
use crate::crypto::MasterKey;
use crate::error::{ApiError, ApiResult};

/// Serialized shape for UI + API. `mount_path` and `attached_*` are
/// populated only when `status = 'attached'`.
#[derive(Debug, Serialize)]
pub struct VolumeSummary {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub size_gb: i32,
    pub hetzner_volume_id: Option<i64>,
    pub hetzner_location: String,
    pub attached_node_id: Option<String>,
    pub attached_service_id: Option<String>,
    pub mount_path: Option<String>,
    pub status: String,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, sea_orm::FromQueryResult)]
pub struct VolumeRow {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub size_gb: i32,
    pub hetzner_volume_id: Option<i64>,
    pub hetzner_location: String,
    pub attached_node_id: Option<String>,
    pub attached_service_id: Option<String>,
    pub mount_path: Option<String>,
    pub status: String,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<VolumeRow> for VolumeSummary {
    fn from(r: VolumeRow) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            name: r.name,
            size_gb: r.size_gb,
            hetzner_volume_id: r.hetzner_volume_id,
            hetzner_location: r.hetzner_location,
            attached_node_id: r.attached_node_id,
            attached_service_id: r.attached_service_id,
            mount_path: r.mount_path,
            status: r.status,
            reason: r.reason,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Columns we always pull back. Kept in one place so the struct + list
/// queries don't drift.
pub const VOLUME_COLUMNS: &str = "id, workspace_id, name, size_gb, hetzner_volume_id, \
     hetzner_location, attached_node_id, attached_service_id, mount_path, status, reason, \
     created_at, updated_at";

pub async fn fetch_by_id(pool: &DatabaseConnection, id: &str) -> ApiResult<Option<VolumeRow>> {
    let row: Option<VolumeRow> = crate::db::query_as(format!(
        "SELECT {VOLUME_COLUMNS} FROM volumes WHERE id = $1",
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Load the volume attached to a service, if any. Used by the scheduler
/// and the deploy handler to decide whether to pin placement.
pub async fn fetch_for_service(
    pool: &DatabaseConnection,
    service_id: &str,
) -> ApiResult<Option<VolumeRow>> {
    let row: Option<VolumeRow> = crate::db::query_as(format!(
        "SELECT {VOLUME_COLUMNS} FROM volumes WHERE attached_service_id = $1",
    ))
    .bind(service_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Delete the provider-side volume, then remove the local row.
///
/// If the workspace no longer has a Hetzner token, this preserves the
/// existing API behavior and only removes the local row.
pub async fn delete_backing_volume_and_row(
    pool: &DatabaseConnection,
    master_key: &MasterKey,
    workspace_id: &str,
    row: &VolumeRow,
) -> ApiResult<()> {
    // If physically attached to a node, detach first so Hetzner lets us
    // delete it. Non-fatal if detach returns an error — the delete
    // itself will surface any real problem.
    if let Some(hz_id) = row.hetzner_volume_id {
        let token = credentials::first_hetzner_token(pool, master_key, workspace_id)
            .await
            .map_err(ApiError::Internal)?;
        if let Some(token) = token {
            let client = driftbase_hetzner::HetznerClient::new(&token);
            if row.attached_node_id.is_some() {
                if let Ok(action) = client.detach_volume(hz_id).await {
                    let _ = client
                        .wait_for_action(action.id, Duration::from_secs(60))
                        .await;
                }
            }
            client
                .delete_volume(hz_id)
                .await
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("hetzner delete_volume: {e}")))?;
        }
    }

    crate::db::query("DELETE FROM volumes WHERE id = $1")
        .bind(&row.id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Validate a container-side mount path. Absolute, no `..`, reasonable
/// length. Leaves the specifics of whether the path is safe to the user;
/// we just catch the obvious mistakes.
pub fn validate_mount_path(path: &str) -> ApiResult<()> {
    let p = path.trim();
    if !p.starts_with('/') {
        return Err(ApiError::Validation(
            "mount path must be absolute (start with /)".into(),
        ));
    }
    if p.split('/').any(|seg| seg == ".." || seg == ".") {
        return Err(ApiError::Validation(
            "mount path cannot contain . or .. segments".into(),
        ));
    }
    if p.len() > 200 {
        return Err(ApiError::Validation("mount path too long".into()));
    }
    if matches!(
        p,
        "/" | "/etc" | "/bin" | "/sbin" | "/proc" | "/sys" | "/dev"
    ) {
        return Err(ApiError::Validation(
            "mount path collides with a system directory".into(),
        ));
    }
    Ok(())
}
