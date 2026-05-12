pub mod routes;

use anyhow::{anyhow, Context, Result};
use sea_orm::DatabaseConnection;
use serde_json::Value as JsonValue;

use crate::config::Config;
use crate::crypto::MasterKey;

pub async fn hetzner_token_for_workspace(
    pool: &DatabaseConnection,
    config: &Config,
    master_key: &MasterKey,
    workspace_id: &str,
) -> Result<Option<String>> {
    let _ = (pool, master_key, workspace_id);
    Ok(config.managed_hetzner_api_token.clone())
}

/// Decrypted view of a stored credential. Caller is expected to use the
/// plaintext immediately (ship it to an agent in-memory) and drop it.
pub struct DecryptedCredential {
    pub kind: String,
    pub secret: String,
    pub metadata: JsonValue,
}

/// Fetch + decrypt a credential by id, scoped to `workspace_id`. Returns None
/// if the credential is missing, the workspace doesn't own it, or the secret
/// isn't valid UTF-8 (all of our kinds store text — tokens, passwords, PATs).
pub async fn fetch_decrypted(
    pool: &DatabaseConnection,
    master_key: &MasterKey,
    workspace_id: &str,
    credential_id: &str,
) -> Result<Option<DecryptedCredential>> {
    let row: Option<(String, Vec<u8>, JsonValue)> = crate::db::query_tuple(
        "SELECT kind, encrypted, metadata FROM credentials \
         WHERE id = $1 AND workspace_id = $2",
    )
    .bind(credential_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    let Some((kind, ct, metadata)) = row else {
        return Ok(None);
    };
    let pt = master_key
        .decrypt(&ct)
        .with_context(|| format!("decrypting credential {credential_id}"))?;
    let secret = String::from_utf8(pt).map_err(|e| anyhow!("credential not utf8: {e}"))?;
    Ok(Some(DecryptedCredential {
        kind,
        secret,
        metadata,
    }))
}

/// Registry-proxy lookup: fetch + decrypt by id without knowing the workspace
/// in advance. Returns the owning workspace id so the caller can check it
/// against the URL path. The registry proxy uses this and then enforces the
/// workspace-scope check itself.
pub async fn fetch_for_proxy(
    pool: &DatabaseConnection,
    master_key: &MasterKey,
    credential_id: &str,
) -> Result<Option<(String, DecryptedCredential)>> {
    let row: Option<(String, String, Vec<u8>, JsonValue)> = crate::db::query_tuple(
        "SELECT workspace_id, kind, encrypted, metadata FROM credentials \
         WHERE id = $1",
    )
    .bind(credential_id)
    .fetch_optional(pool)
    .await?;
    let Some((workspace_id, kind, ct, metadata)) = row else {
        return Ok(None);
    };
    let pt = master_key
        .decrypt(&ct)
        .with_context(|| format!("decrypting credential {credential_id}"))?;
    let secret = String::from_utf8(pt).map_err(|e| anyhow!("credential not utf8: {e}"))?;
    Ok(Some((
        workspace_id,
        DecryptedCredential {
            kind,
            secret,
            metadata,
        },
    )))
}
