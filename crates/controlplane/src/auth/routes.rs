use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use driftbase_common::Id;

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
    pub is_platform_admin: bool,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum SignupResponse {
    /// Account immediately usable — first-ever user or invite acceptance.
    Active(MeResponse),
    /// Account created but awaiting platform-admin approval. No session
    /// is issued; the client shows a "pending approval" screen.
    Pending { pending: bool, email: String },
}

async fn signup(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<SignupRequest>,
) -> ApiResult<(CookieJar, Json<SignupResponse>)> {
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

    // First signup on this instance becomes the platform admin and is
    // auto-approved so the installer isn't locked out of their own box.
    let existing_count: (i64,) = sqlx::query_as("SELECT COUNT(*)::bigint FROM users")
        .fetch_one(state.pool())
        .await?;
    let is_first_user = existing_count.0 == 0;

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, status, is_platform_admin) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(user_id.to_string())
    .bind(&email_norm)
    .bind(&password_hash)
    .bind(req.display_name.trim())
    .bind(if is_first_user { "approved" } else { "pending" })
    .bind(is_first_user)
    .execute(state.pool())
    .await?;

    // Accept invite if one was supplied. Treat successful acceptance as
    // implicit approval — someone with a workspace already vouched for
    // this person. Failed matches are non-fatal and leave the row
    // pending for the platform admin to review.
    let mut approved_via_invite = false;
    if let Some(token) = req.invite_token.as_deref() {
        if crate::workspaces::invites::accept_for_user(state.pool(), token, &user_id, &email_norm)
            .await
            .is_ok()
        {
            approved_via_invite = true;
            sqlx::query("UPDATE users SET status = 'approved' WHERE id = $1")
                .bind(user_id.to_string())
                .execute(state.pool())
                .await?;
        }
    }

    let active = is_first_user || approved_via_invite;
    if !active {
        return Ok((
            jar,
            Json(SignupResponse::Pending {
                pending: true,
                email: email_norm,
            }),
        ));
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
        is_platform_admin: is_first_user,
    };
    Ok((jar, Json(SignupResponse::Active(me))))
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: String,
    email: String,
    password_hash: String,
    display_name: String,
    created_at: DateTime<Utc>,
    status: String,
    is_platform_admin: bool,
}

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> ApiResult<(CookieJar, Json<MeResponse>)> {
    let email_norm = req.email.trim().to_lowercase();
    let row: Option<UserRow> = sqlx::query_as(
        "SELECT id, email, password_hash, display_name, created_at, status, \
                is_platform_admin \
         FROM users WHERE email = $1",
    )
    .bind(&email_norm)
    .fetch_optional(state.pool())
    .await?;

    let Some(row) = row else {
        return Err(ApiError::Unauthorized);
    };

    let ok = password::verify(&req.password, &row.password_hash).map_err(ApiError::Internal)?;
    if !ok {
        return Err(ApiError::Unauthorized);
    }

    // Waitlist gate: password is right but the account isn't usable yet.
    // 403 (Forbidden) is the right status — the credentials are valid,
    // the account just isn't active. The UI keys off the message.
    match row.status.as_str() {
        "approved" => {}
        "pending" => {
            return Err(ApiError::Forbidden(
                "Account pending approval — an administrator needs to approve your signup.".into(),
            ));
        }
        "rejected" => {
            return Err(ApiError::Forbidden("Account has been rejected.".into()));
        }
        _ => {
            return Err(ApiError::Forbidden("Account is not active.".into()));
        }
    }

    let user_id: Id = row
        .id
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
            email: row.email,
            display_name: row.display_name,
            created_at: row.created_at,
            is_platform_admin: row.is_platform_admin,
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

#[derive(sqlx::FromRow)]
struct MeRow {
    id: String,
    email: String,
    display_name: String,
    created_at: DateTime<Utc>,
    status: String,
    is_platform_admin: bool,
}

async fn me(State(state): State<AppState>, auth: AuthUser) -> ApiResult<Json<MeResponse>> {
    let row: Option<MeRow> = sqlx::query_as(
        "SELECT id, email, display_name, created_at, status, is_platform_admin \
         FROM users WHERE id = $1",
    )
    .bind(auth.user_id.to_string())
    .fetch_optional(state.pool())
    .await?;
    let MeRow {
        id,
        email,
        display_name,
        created_at,
        status,
        is_platform_admin,
    } = row.ok_or(ApiError::Unauthorized)?;
    // A user rejected after logging in should lose access on their next
    // request, not wait for cookie expiry.
    if status != "approved" {
        return Err(ApiError::Unauthorized);
    }
    Ok(Json(MeResponse {
        id: id
            .parse()
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
        email,
        display_name,
        created_at,
        is_platform_admin,
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
