use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::{DateTime, Utc};
use driftbase_common::Id;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::workspaces::membership::{self, Role};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces/:slug/ssh-keys", get(list).post(create))
        .route(
            "/workspaces/:slug/ssh-keys/:id",
            axum::routing::delete(delete),
        )
}

#[derive(Serialize)]
pub struct SshKeySummary {
    pub id: Id,
    pub name: String,
    pub public_key: String,
    pub fingerprint: String,
    pub has_private_key: bool,
    pub hetzner_key_id: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(sea_orm::FromQueryResult)]
struct SshKeyRow {
    id: String,
    name: String,
    public_key: String,
    fingerprint: String,
    has_private_key: bool,
    hetzner_key_id: Option<i64>,
    created_at: DateTime<Utc>,
}

impl TryFrom<SshKeyRow> for SshKeySummary {
    type Error = ApiError;
    fn try_from(r: SshKeyRow) -> Result<Self, ApiError> {
        Ok(Self {
            id: r
                .id
                .parse()
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
            name: r.name,
            public_key: r.public_key,
            fingerprint: r.fingerprint,
            has_private_key: r.has_private_key,
            hetzner_key_id: r.hetzner_key_id,
            created_at: r.created_at,
        })
    }
}

async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> ApiResult<Json<Vec<SshKeySummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;
    require_ssh_keys_unavailable(state.pool(), &ctx.workspace_id.to_string()).await?;

    let rows: Vec<SshKeyRow> = crate::db::query_as(
        "SELECT id, name, public_key, fingerprint, \
         (private_key_encrypted IS NOT NULL) AS has_private_key, \
         hetzner_key_id, created_at \
         FROM ssh_keys WHERE workspace_id = $1 ORDER BY created_at DESC",
    )
    .bind(ctx.workspace_id.to_string())
    .fetch_all(state.pool())
    .await?;

    rows.into_iter()
        .map(SshKeySummary::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

#[derive(Deserialize)]
pub struct CreateSshKeyRequest {
    pub name: String,
    pub public_key: String,
    #[serde(default)]
    pub private_key: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
    Json(req): Json<CreateSshKeyRequest>,
) -> ApiResult<Json<SshKeySummary>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;
    require_ssh_keys_unavailable(state.pool(), &ctx.workspace_id.to_string()).await?;

    let name = req.name.trim().to_string();
    if name.is_empty() || name.len() > 80 {
        return Err(ApiError::Validation("name must be 1–80 chars".into()));
    }

    let public_key = req.public_key.trim().to_string();
    let fingerprint = openssh_fingerprint(&public_key)
        .map_err(|e| ApiError::Validation(format!("invalid OpenSSH public key: {e}")))?;

    let private_key_encrypted = match req.private_key.as_ref() {
        Some(pk) if !pk.trim().is_empty() => Some(
            state
                .master_key()
                .encrypt(pk.trim().as_bytes())
                .map_err(ApiError::Internal)?,
        ),
        _ => None,
    };

    let id = Id::new();
    let inserted: Option<SshKeyRow> = crate::db::query_as(
        "INSERT INTO ssh_keys (id, workspace_id, name, public_key, fingerprint, private_key_encrypted, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (workspace_id, name) DO NOTHING \
         RETURNING id, name, public_key, fingerprint, \
                   (private_key_encrypted IS NOT NULL) AS has_private_key, \
                   hetzner_key_id, created_at",
    )
    .bind(id.to_string())
    .bind(ctx.workspace_id.to_string())
    .bind(&name)
    .bind(&public_key)
    .bind(&fingerprint)
    .bind(private_key_encrypted.as_deref())
    .bind(auth.user_id.to_string())
    .fetch_optional(state.pool())
    .await?;

    let row = inserted
        .ok_or_else(|| ApiError::Conflict("ssh key with that name already exists".into()))?;
    Ok(Json(SshKeySummary::try_from(row)?))
}

async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, id)): Path<(String, String)>,
) -> ApiResult<()> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;
    require_ssh_keys_unavailable(state.pool(), &ctx.workspace_id.to_string()).await?;

    let res = crate::db::query("DELETE FROM ssh_keys WHERE id = $1 AND workspace_id = $2")
        .bind(&id)
        .bind(ctx.workspace_id.to_string())
        .execute(state.pool())
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

async fn require_ssh_keys_unavailable(
    pool: &sea_orm::DatabaseConnection,
    workspace_id: &str,
) -> ApiResult<()> {
    let _ = (pool, workspace_id);
    Err(ApiError::Forbidden(
        "SSH keys are managed by Driftbase".into(),
    ))
}

/// Compute the OpenSSH SHA256 fingerprint of a public key in `ssh-<alg> <base64> [comment]` form.
fn openssh_fingerprint(public_key: &str) -> Result<String, String> {
    let trimmed = public_key.trim();
    let mut parts = trimmed.split_whitespace();
    let alg = parts.next().ok_or("missing algorithm")?;
    let b64_body = parts.next().ok_or("missing key body")?;
    if !alg.starts_with("ssh-") && !alg.starts_with("ecdsa-") && !alg.starts_with("sk-") {
        return Err(format!("unsupported algorithm '{alg}'"));
    }
    let raw = B64
        .decode(b64_body.as_bytes())
        .map_err(|e| format!("base64: {e}"))?;
    let digest = Sha256::digest(&raw);
    let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest);
    Ok(format!("SHA256:{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_known_key() {
        // Sample ed25519 public key from OpenSSH docs
        let key =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICrfzv73pAiXQAlgQuzmfr6+Q1p2MyvpbO+QQvm3RWRB test";
        let fp = openssh_fingerprint(key).unwrap();
        assert!(fp.starts_with("SHA256:"));
        assert_eq!(fp.len(), "SHA256:".len() + 43);
    }

    #[test]
    fn rejects_non_base64() {
        let err = openssh_fingerprint("ssh-ed25519 !!!notbase64!!!").unwrap_err();
        assert!(err.contains("base64"));
    }

    #[test]
    fn rejects_empty() {
        assert!(openssh_fingerprint("").is_err());
    }
}
