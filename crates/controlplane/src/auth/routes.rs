use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use zediz_common::Id;

use crate::auth::{extractor::AuthUser, password, session};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/signup", post(signup))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
}

#[derive(Deserialize)]
pub struct SignupRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub invite_token: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct MeResponse {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
}

async fn signup(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<SignupRequest>,
) -> ApiResult<(CookieJar, Json<MeResponse>)> {
    validate_email(&req.email)?;
    validate_password(&req.password)?;
    if req.display_name.trim().is_empty() {
        return Err(ApiError::Validation("display_name is required".into()));
    }

    let email_norm = req.email.trim().to_lowercase();
    let password_hash = password::hash(&req.password).map_err(ApiError::Internal)?;
    let user_id = Id::new();

    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(&email_norm)
        .fetch_optional(state.pool())
        .await?;
    if row.is_some() {
        return Err(ApiError::Conflict("email already registered".into()));
    }

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name) VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id.to_string())
    .bind(&email_norm)
    .bind(&password_hash)
    .bind(req.display_name.trim())
    .execute(state.pool())
    .await?;

    // Accept invite if one was supplied. Non-fatal if it fails to match.
    if let Some(token) = req.invite_token.as_deref() {
        let _ =
            crate::workspaces::invites::accept_for_user(state.pool(), token, &user_id, &email_norm)
                .await;
    }

    let issued = session::create(state.pool(), &user_id, None, None)
        .await
        .map_err(ApiError::Internal)?;
    let jar = jar.add(build_cookie(
        &issued.token,
        issued.expires_at,
        state.config().cookie_secure,
    ));

    let me = MeResponse {
        id: user_id,
        email: email_norm,
        display_name: req.display_name.trim().to_string(),
        created_at: Utc::now(),
    };
    Ok((jar, Json(me)))
}

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> ApiResult<(CookieJar, Json<MeResponse>)> {
    let email_norm = req.email.trim().to_lowercase();
    let row: Option<(String, String, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, email, password_hash, display_name, created_at FROM users WHERE email = $1",
    )
    .bind(&email_norm)
    .fetch_optional(state.pool())
    .await?;

    let Some((id, email, hash, display_name, created_at)) = row else {
        return Err(ApiError::Unauthorized);
    };

    let ok = password::verify(&req.password, &hash).map_err(ApiError::Internal)?;
    if !ok {
        return Err(ApiError::Unauthorized);
    }

    let user_id: Id = id
        .parse()
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?;
    let issued = session::create(state.pool(), &user_id, None, None)
        .await
        .map_err(ApiError::Internal)?;
    let jar = jar.add(build_cookie(
        &issued.token,
        issued.expires_at,
        state.config().cookie_secure,
    ));

    Ok((
        jar,
        Json(MeResponse {
            id: user_id,
            email,
            display_name,
            created_at,
        }),
    ))
}

async fn logout(State(state): State<AppState>, jar: CookieJar) -> ApiResult<CookieJar> {
    if let Some(c) = jar.get(session::COOKIE_NAME) {
        let _ = session::revoke(state.pool(), c.value()).await;
    }
    let jar = jar.remove(Cookie::from(session::COOKIE_NAME));
    Ok(jar)
}

async fn me(State(state): State<AppState>, auth: AuthUser) -> ApiResult<Json<MeResponse>> {
    let row: Option<(String, String, String, DateTime<Utc>)> =
        sqlx::query_as("SELECT id, email, display_name, created_at FROM users WHERE id = $1")
            .bind(auth.user_id.to_string())
            .fetch_optional(state.pool())
            .await?;
    let (id, email, display_name, created_at) = row.ok_or(ApiError::Unauthorized)?;
    Ok(Json(MeResponse {
        id: id
            .parse()
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
        email,
        display_name,
        created_at,
    }))
}

fn build_cookie(token: &str, expires_at: DateTime<Utc>, secure: bool) -> Cookie<'static> {
    let expires = OffsetDateTime::from_unix_timestamp(expires_at.timestamp())
        .unwrap_or(OffsetDateTime::now_utc());
    Cookie::build((session::COOKIE_NAME, token.to_string()))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .expires(expires)
        .build()
}

fn validate_email(email: &str) -> ApiResult<()> {
    let trimmed = email.trim();
    if trimmed.len() < 3 || !trimmed.contains('@') || trimmed.len() > 254 {
        return Err(ApiError::Validation("invalid email".into()));
    }
    Ok(())
}

fn validate_password(pw: &str) -> ApiResult<()> {
    if pw.len() < 8 {
        return Err(ApiError::Validation(
            "password must be at least 8 characters".into(),
        ));
    }
    if pw.len() > 256 {
        return Err(ApiError::Validation("password too long".into()));
    }
    Ok(())
}
