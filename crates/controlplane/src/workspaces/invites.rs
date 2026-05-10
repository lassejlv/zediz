use anyhow::anyhow;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use driftbase_common::Id;
use rand::RngCore;
use sea_orm::{DatabaseConnection, TransactionTrait};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::workspaces::membership::{self, Role};

pub const INVITE_TTL_DAYS: i64 = 14;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/workspaces/:slug/invites",
            post(create_invite).get(list_invites),
        )
        .route(
            "/workspaces/:slug/invites/:id",
            axum::routing::delete(revoke_invite),
        )
        .route("/invites/:token/accept", post(accept_invite))
}

fn mint_token() -> (String, Vec<u8>) {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    let token = URL_SAFE_NO_PAD.encode(buf);
    let hash = Sha256::digest(token.as_bytes()).to_vec();
    (token, hash)
}

fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

#[derive(Deserialize)]
pub struct CreateInviteRequest {
    pub email: String,
    pub role: Role,
}

#[derive(Serialize)]
pub struct InviteSummary {
    pub id: Id,
    pub email: String,
    pub role: Role,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct CreatedInvite {
    #[serde(flatten)]
    pub summary: InviteSummary,
    pub token: String,
    pub accept_url: String,
}

async fn create_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
    Json(req): Json<CreateInviteRequest>,
) -> ApiResult<Json<CreatedInvite>> {
    if matches!(req.role, Role::Owner) {
        return Err(ApiError::Validation("cannot invite as owner".into()));
    }
    let email_norm = req.email.trim().to_lowercase();
    if !email_norm.contains('@') {
        return Err(ApiError::Validation("invalid email".into()));
    }

    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    let id = Id::new();
    let (token, token_hash) = mint_token();
    let expires_at = Utc::now() + Duration::days(INVITE_TTL_DAYS);

    crate::db::query(
        "INSERT INTO invites (id, workspace_id, email, role, token_hash, invited_by, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id.to_string())
    .bind(ctx.workspace_id.to_string())
    .bind(&email_norm)
    .bind(req.role.as_str())
    .bind(&token_hash)
    .bind(auth.user_id.to_string())
    .bind(expires_at)
    .execute(state.pool())
    .await
    .map_err(|e| match &e {
        e if crate::db::is_unique_violation(e) => {
            ApiError::Conflict("invite already pending for this email".into())
        }
        _ => ApiError::Db(e),
    })?;

    let accept_url = format!(
        "{}/invite/{}",
        state.config().public_url.trim_end_matches('/'),
        token
    );

    Ok(Json(CreatedInvite {
        summary: InviteSummary {
            id,
            email: email_norm,
            role: req.role,
            expires_at,
            created_at: Utc::now(),
            accepted_at: None,
        },
        token,
        accept_url,
    }))
}

#[derive(sea_orm::FromQueryResult)]
struct InviteRow {
    id: String,
    email: String,
    role: String,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    accepted_at: Option<DateTime<Utc>>,
}

async fn list_invites(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> ApiResult<Json<Vec<InviteSummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    let rows: Vec<InviteRow> = crate::db::query_as(
        "SELECT id, email, role, expires_at, created_at, accepted_at \
         FROM invites WHERE workspace_id = $1 AND revoked_at IS NULL \
         ORDER BY created_at DESC",
    )
    .bind(ctx.workspace_id.to_string())
    .fetch_all(state.pool())
    .await?;

    Ok(Json(
        rows.into_iter()
            .filter_map(|r| {
                Some(InviteSummary {
                    id: r.id.parse().ok()?,
                    email: r.email,
                    role: r.role.parse().ok()?,
                    expires_at: r.expires_at,
                    created_at: r.created_at,
                    accepted_at: r.accepted_at,
                })
            })
            .collect(),
    ))
}

async fn revoke_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, invite_id)): Path<(String, String)>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;

    let res = crate::db::query(
        "UPDATE invites SET revoked_at = now() \
         WHERE id = $1 AND workspace_id = $2 AND accepted_at IS NULL AND revoked_at IS NULL",
    )
    .bind(&invite_id)
    .bind(ctx.workspace_id.to_string())
    .execute(state.pool())
    .await?;

    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

async fn accept_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(token): Path<String>,
) -> ApiResult<Json<InviteAccepted>> {
    // Load the user's email so we can match the invite.
    let email: Option<(String,)> = crate::db::query_tuple("SELECT email FROM users WHERE id = $1")
        .bind(auth.user_id.to_string())
        .fetch_optional(state.pool())
        .await?;
    let Some((email,)) = email else {
        return Err(ApiError::Unauthorized);
    };

    let ctx = accept_for_user(state.pool(), &token, &auth.user_id, &email).await?;
    Ok(Json(InviteAccepted {
        workspace_slug: ctx.slug,
    }))
}

#[derive(Serialize)]
pub struct InviteAccepted {
    pub workspace_slug: String,
}

/// Idempotently consume an invite on behalf of `user_id` whose email matches.
/// Returns the workspace context so callers can redirect.
pub async fn accept_for_user(
    pool: &DatabaseConnection,
    token: &str,
    user_id: &Id,
    user_email: &str,
) -> ApiResult<AcceptedContext> {
    let token_hash = hash_token(token);

    let tx = pool.begin().await?;

    let row: Option<InviteLockedRow> = crate::db::query_as(
        "SELECT id, workspace_id, email, role, expires_at, accepted_at, revoked_at \
         FROM invites WHERE token_hash = $1 FOR UPDATE",
    )
    .bind(&token_hash)
    .fetch_optional(&tx)
    .await?;

    let r = row.ok_or(ApiError::NotFound)?;

    if r.revoked_at.is_some() {
        return Err(ApiError::Validation("invite revoked".into()));
    }
    if r.accepted_at.is_some() {
        return Err(ApiError::Validation("invite already accepted".into()));
    }
    if r.expires_at < Utc::now() {
        return Err(ApiError::Validation("invite expired".into()));
    }
    if r.email.to_lowercase() != user_email.to_lowercase() {
        return Err(ApiError::Forbidden(String::new()));
    }

    let invite_id = r.id;
    let workspace_id = r.workspace_id;
    let role = r.role;

    crate::db::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3) \
         ON CONFLICT (workspace_id, user_id) DO NOTHING",
    )
    .bind(&workspace_id)
    .bind(user_id.to_string())
    .bind(&role)
    .execute(&tx)
    .await?;

    crate::db::query("UPDATE invites SET accepted_at = now(), accepted_by = $1 WHERE id = $2")
        .bind(user_id.to_string())
        .bind(&invite_id)
        .execute(&tx)
        .await?;

    let slug: (String,) = crate::db::query_tuple("SELECT slug FROM workspaces WHERE id = $1")
        .bind(&workspace_id)
        .fetch_one(&tx)
        .await?;

    tx.commit().await?;

    Ok(AcceptedContext {
        workspace_id: workspace_id
            .parse()
            .map_err(|e| ApiError::Internal(anyhow!("{e}")))?,
        slug: slug.0,
    })
}

pub struct AcceptedContext {
    #[allow(dead_code)]
    pub workspace_id: Id,
    pub slug: String,
}

#[derive(sea_orm::FromQueryResult)]
struct InviteLockedRow {
    id: String,
    workspace_id: String,
    email: String,
    role: String,
    expires_at: DateTime<Utc>,
    accepted_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}
