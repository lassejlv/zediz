use anyhow::{anyhow, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use driftbase_common::Id;

pub const COOKIE_NAME: &str = "driftbase_session";
pub const SESSION_TTL_DAYS: i64 = 30;

pub struct IssuedSession {
    #[allow(dead_code)]
    pub id: Id,
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

pub fn mint_token() -> (String, Vec<u8>) {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    let token = URL_SAFE_NO_PAD.encode(buf);
    let hash = Sha256::digest(token.as_bytes()).to_vec();
    (token, hash)
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub async fn create(
    pool: &PgPool,
    user_id: &Id,
    user_agent: Option<&str>,
    ip: Option<&str>,
) -> Result<IssuedSession> {
    let (token, token_hash) = mint_token();
    let id = Id::new();
    let expires_at = Utc::now() + Duration::days(SESSION_TTL_DAYS);

    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, user_agent, ip, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id.to_string())
    .bind(user_id.to_string())
    .bind(&token_hash)
    .bind(user_agent)
    .bind(ip)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(IssuedSession {
        id,
        token,
        expires_at,
    })
}

pub struct LoadedSession {
    pub user_id: Id,
    pub session_id: Id,
}

pub async fn load(pool: &PgPool, token: &str) -> Result<Option<LoadedSession>> {
    let token_hash = hash_token(token);
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, user_id FROM sessions \
         WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await?;

    let Some((session_id, user_id)) = row else {
        return Ok(None);
    };
    Ok(Some(LoadedSession {
        session_id: session_id.parse().map_err(|e| anyhow!("{e}"))?,
        user_id: user_id.parse().map_err(|e| anyhow!("{e}"))?,
    }))
}

pub async fn revoke(pool: &PgPool, token: &str) -> Result<()> {
    let token_hash = hash_token(token);
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE token_hash = $1")
        .bind(&token_hash)
        .execute(pool)
        .await?;
    Ok(())
}
