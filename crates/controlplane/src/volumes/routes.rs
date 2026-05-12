use axum::extract::{Path, State};
use axum::routing::{delete, get};
use axum::{Json, Router};
use driftbase_common::Id;
use driftbase_hetzner::{CreateVolumeRequest, HetznerClient};
use serde::Deserialize;
use std::time::Duration;

use crate::auth::AuthUser;
use crate::credentials;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::volumes::{self, VolumeRow, VolumeSummary, VOLUME_COLUMNS};
use crate::workspaces::membership::{self, Role};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces/:slug/volumes", get(list).post(create))
        .route("/workspaces/:slug/volumes/:id", delete(remove))
        .route(
            "/workspaces/:slug/projects/:project_slug/services/:service_slug/volume",
            get(show_for_service)
                .post(attach_to_service)
                .delete(detach_from_service),
        )
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> ApiResult<Json<Vec<VolumeSummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    let rows: Vec<VolumeRow> = crate::db::query_as(format!(
        "SELECT {VOLUME_COLUMNS} FROM volumes \
         WHERE workspace_id = $1 ORDER BY created_at DESC"
    ))
    .bind(ctx.workspace_id.to_string())
    .fetch_all(state.pool())
    .await?;
    Ok(Json(rows.into_iter().map(VolumeSummary::from).collect()))
}

#[derive(Deserialize)]
pub struct CreateVolumeInput {
    pub name: String,
    pub size_gb: u32,
    /// Optional override; defaults to the workspace's `hetzner_location`.
    #[serde(default)]
    pub location: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
    Json(req): Json<CreateVolumeInput>,
) -> ApiResult<Json<VolumeSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;

    let name = req.name.trim().to_string();
    if !(1..=64).contains(&name.len()) {
        return Err(ApiError::Validation(
            "volume name must be 1–64 characters".into(),
        ));
    }
    if !(10..=10240).contains(&req.size_gb) {
        return Err(ApiError::Validation(
            "size_gb must be between 10 and 10240".into(),
        ));
    }

    // Default to the workspace's default region. We always set a
    // location — the UI doesn't expose an override yet.
    let workspace_location: (String,) =
        crate::db::query_tuple("SELECT hetzner_location FROM workspaces WHERE id = $1")
            .bind(ctx.workspace_id.to_string())
            .fetch_one(state.pool())
            .await?;
    let location = req.location.unwrap_or(workspace_location.0);

    // Insert the row first so concurrent deletes / races see our intent.
    let volume_id = Id::new().to_string();
    crate::db::query(
        "INSERT INTO volumes (id, workspace_id, name, size_gb, hetzner_location, status) \
         VALUES ($1, $2, $3, $4, $5, 'creating')",
    )
    .bind(&volume_id)
    .bind(ctx.workspace_id.to_string())
    .bind(&name)
    .bind(req.size_gb as i32)
    .bind(&location)
    .execute(state.pool())
    .await
    .map_err(|e| match e {
        e if crate::db::is_unique_violation(&e) => {
            ApiError::Conflict("a volume with that name already exists".into())
        }
        other => ApiError::Db(other),
    })?;

    // Call Hetzner. On any failure, delete the row so the user can retry.
    let result = async {
        let token = credentials::first_hetzner_token(
            state.pool(),
            state.master_key(),
            &ctx.workspace_id.to_string(),
        )
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::Validation(
                "workspace has no Hetzner API token credential; add one in Credentials".into(),
            )
        })?;
        let client = HetznerClient::new(&token);
        let created = client
            .create_volume(&CreateVolumeRequest {
                name: &format!("driftbase-{}", &volume_id),
                size: req.size_gb,
                location: &location,
                automount: false,
                // Pre-format so the agent just mounts the block device.
                format: "ext4",
            })
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("hetzner create_volume: {e}")))?;

        // Update the row with the real Hetzner id before we wait for
        // the action so a crash mid-wait still leaves us a pointer.
        crate::db::query(
            "UPDATE volumes SET hetzner_volume_id = $1, updated_at = now() WHERE id = $2",
        )
        .bind(created.volume.id)
        .bind(&volume_id)
        .execute(state.pool())
        .await?;

        client
            .wait_for_action(created.action.id, Duration::from_secs(120))
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("hetzner volume action: {e}")))?;

        crate::db::query(
            "UPDATE volumes SET status = 'available', reason = NULL, updated_at = now() \
             WHERE id = $1",
        )
        .bind(&volume_id)
        .execute(state.pool())
        .await?;

        Ok::<_, ApiError>(())
    }
    .await;

    if let Err(e) = result {
        let reason = e.to_string();
        let _ = crate::db::query(
            "UPDATE volumes SET status = 'errored', reason = $1, updated_at = now() \
             WHERE id = $2",
        )
        .bind(&reason)
        .bind(&volume_id)
        .execute(state.pool())
        .await;
        return Err(e);
    }

    let row = volumes::fetch_by_id(state.pool(), &volume_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(VolumeSummary::from(row)))
}

async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, id)): Path<(String, String)>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;

    let row: Option<VolumeRow> = crate::db::query_as(format!(
        "UPDATE volumes \
         SET status = 'deleting', reason = NULL, updated_at = now() \
         WHERE id = $1 AND workspace_id = $2 AND attached_service_id IS NULL \
         RETURNING {VOLUME_COLUMNS}"
    ))
    .bind(&id)
    .bind(ctx.workspace_id.to_string())
    .fetch_optional(state.pool())
    .await?;
    let Some(row) = row else {
        let exists: Option<(Option<String>,)> = crate::db::query_tuple(
            "SELECT attached_service_id FROM volumes WHERE id = $1 AND workspace_id = $2",
        )
        .bind(&id)
        .bind(ctx.workspace_id.to_string())
        .fetch_optional(state.pool())
        .await?;
        return match exists {
            Some((Some(_),)) => Err(ApiError::Conflict(
                "detach the volume from its service before deleting".into(),
            )),
            Some((None,)) => Err(ApiError::Conflict(
                "volume is busy; retry deletion in a moment".into(),
            )),
            None => Err(ApiError::NotFound),
        };
    };

    volumes::delete_backing_volume_and_row(
        state.pool(),
        state.master_key(),
        &ctx.workspace_id.to_string(),
        &row,
    )
    .await?;
    Ok(())
}

async fn show_for_service(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug)): Path<(String, String, String)>,
) -> ApiResult<Json<Option<VolumeSummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    let service_id = resolve_service(state.pool(), &ctx, &project_slug, &service_slug).await?;
    let row = volumes::fetch_for_service(state.pool(), &service_id).await?;
    Ok(Json(row.map(VolumeSummary::from)))
}

#[derive(Deserialize)]
pub struct AttachInput {
    pub volume_id: String,
    pub mount_path: String,
}

async fn attach_to_service(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug)): Path<(String, String, String)>,
    Json(req): Json<AttachInput>,
) -> ApiResult<Json<VolumeSummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;
    let service_id = resolve_service(state.pool(), &ctx, &project_slug, &service_slug).await?;

    volumes::validate_mount_path(&req.mount_path)?;

    let row: Option<VolumeRow> = crate::db::query_as(format!(
        "UPDATE volumes \
         SET attached_service_id = $1, mount_path = $2, status = 'attached', \
             updated_at = now() \
         WHERE id = $3 \
           AND workspace_id = $4 \
           AND (attached_service_id IS NULL OR attached_service_id = $1) \
           AND (status = 'available' OR attached_service_id = $1) \
         RETURNING {VOLUME_COLUMNS}"
    ))
    .bind(&service_id)
    .bind(req.mount_path.trim())
    .bind(&req.volume_id)
    .bind(ctx.workspace_id.to_string())
    .fetch_optional(state.pool())
    .await
    .map_err(|e| match e {
        e if crate::db::is_unique_violation(&e) => {
            ApiError::Conflict("another volume is already attached to this service".into())
        }
        other => ApiError::Db(other),
    })?;
    let Some(row) = row else {
        let existing: Option<(String,)> = crate::db::query_tuple(
            "SELECT status FROM volumes WHERE id = $1 AND workspace_id = $2",
        )
        .bind(&req.volume_id)
        .bind(ctx.workspace_id.to_string())
        .fetch_optional(state.pool())
        .await?;
        return match existing {
            Some((status,)) => Err(ApiError::Conflict(format!(
                "volume is {status} — detach it from its current service first"
            ))),
            None => Err(ApiError::NotFound),
        };
    };
    Ok(Json(VolumeSummary::from(row)))
}

async fn detach_from_service(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, project_slug, service_slug)): Path<(String, String, String)>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;
    let service_id = resolve_service(state.pool(), &ctx, &project_slug, &service_slug).await?;

    // Logical detach — the Hetzner-side detach happens the next time the
    // scheduler needs the node for a different volume, or when the user
    // deletes the volume. Keeping the block device mounted on the node
    // in the meantime is harmless.
    crate::db::query(
        "UPDATE volumes \
         SET attached_service_id = NULL, mount_path = NULL, \
             status = CASE WHEN attached_node_id IS NULL THEN 'available' ELSE 'attached' END, \
             updated_at = now() \
         WHERE attached_service_id = $1",
    )
    .bind(&service_id)
    .execute(state.pool())
    .await?;
    Ok(())
}

async fn resolve_service(
    pool: &sea_orm::DatabaseConnection,
    ctx: &membership::WorkspaceContext,
    project_slug: &str,
    service_slug: &str,
) -> ApiResult<String> {
    let row: Option<(String,)> = crate::db::query_tuple(
        "SELECT s.id FROM services s \
         JOIN projects p ON p.id = s.project_id \
         WHERE p.workspace_id = $1 AND p.slug = $2 AND s.slug = $3",
    )
    .bind(ctx.workspace_id.to_string())
    .bind(project_slug)
    .bind(service_slug)
    .fetch_optional(pool)
    .await?;
    row.map(|(id,)| id).ok_or(ApiError::NotFound)
}
