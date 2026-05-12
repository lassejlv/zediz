use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{DateTime, Utc};
use driftbase_common::Id;
use sea_orm::TransactionTrait;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::auth::{extractor::AuthUser, password, session};
use crate::error::{ApiError, ApiResult};
use crate::rate_limit::RateLimitOutcome;
use crate::state::AppState;
use std::time::Duration;

const FIRST_USER_BOOTSTRAP_LOCK: i64 = 7_413_640_021_337;
const LOGIN_FAILURE_LIMIT: u32 = 10;
const LOGIN_FAILURE_WINDOW: Duration = Duration::from_secs(5 * 60);
const SIGNUP_ATTEMPT_LIMIT: u32 = 10;
const SIGNUP_ATTEMPT_WINDOW: Duration = Duration::from_secs(10 * 60);

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
    #[serde(default)]
    pub setup_token: Option<String>,
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
    let signup_key = rate_key("signup", &email_norm);
    if state
        .rate_limiter()
        .record_attempt(&signup_key, SIGNUP_ATTEMPT_LIMIT, SIGNUP_ATTEMPT_WINDOW)
        == RateLimitOutcome::Limited
    {
        return Err(ApiError::RateLimited);
    }

    let password_hash = password::hash(&req.password).map_err(ApiError::Internal)?;
    let user_id = Id::new();

    let tx = state.pool().begin().await?;
    crate::db::query("SELECT pg_advisory_xact_lock($1)")
        .bind(FIRST_USER_BOOTSTRAP_LOCK)
        .execute(&tx)
        .await?;

    let row: Option<(String,)> = crate::db::query_tuple("SELECT id FROM users WHERE email = $1")
        .bind(&email_norm)
        .fetch_optional(&tx)
        .await?;
    if row.is_some() {
        return Err(ApiError::Conflict("email already registered".into()));
    }

    // First signup on this instance becomes the platform admin and is
    // auto-approved so the installer isn't locked out of their own box.
    let existing_count: (i64,) = crate::db::query_tuple("SELECT COUNT(*)::bigint FROM users")
        .fetch_one(&tx)
        .await?;
    let is_first_user = existing_count.0 == 0;
    if is_first_user {
        if let Some(expected) = state.config().setup_token.as_deref() {
            let supplied = req.setup_token.as_deref().unwrap_or("");
            if !constant_time_eq(expected.as_bytes(), supplied.as_bytes()) {
                return Err(ApiError::Forbidden("invalid setup token".into()));
            }
        }
    }

    crate::db::query(
        "INSERT INTO users (id, email, password_hash, display_name, status, is_platform_admin) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(user_id.to_string())
    .bind(&email_norm)
    .bind(&password_hash)
    .bind(req.display_name.trim())
    .bind(if is_first_user { "approved" } else { "pending" })
    .bind(is_first_user)
    .execute(&tx)
    .await?;
    tx.commit().await?;

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
            crate::db::query("UPDATE users SET status = 'approved' WHERE id = $1")
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

#[derive(sea_orm::FromQueryResult)]
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
    let login_key = rate_key("login", &email_norm);
    if state
        .rate_limiter()
        .check(&login_key, LOGIN_FAILURE_LIMIT, LOGIN_FAILURE_WINDOW)
        == RateLimitOutcome::Limited
    {
        return Err(ApiError::RateLimited);
    }

    let row: Option<UserRow> = crate::db::query_as(
        "SELECT id, email, password_hash, display_name, created_at, status, \
                is_platform_admin \
         FROM users WHERE email = $1",
    )
    .bind(&email_norm)
    .fetch_optional(state.pool())
    .await?;

    let Some(row) = row else {
        state
            .rate_limiter()
            .record_failure(&login_key, LOGIN_FAILURE_WINDOW);
        return Err(ApiError::Unauthorized);
    };

    let ok = password::verify(&req.password, &row.password_hash).map_err(ApiError::Internal)?;
    if !ok {
        state
            .rate_limiter()
            .record_failure(&login_key, LOGIN_FAILURE_WINDOW);
        return Err(ApiError::Unauthorized);
    }
    state.rate_limiter().clear(&login_key);

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

#[derive(sea_orm::FromQueryResult)]
struct MeRow {
    id: String,
    email: String,
    display_name: String,
    created_at: DateTime<Utc>,
    status: String,
    is_platform_admin: bool,
}

async fn me(State(state): State<AppState>, auth: AuthUser) -> ApiResult<Json<MeResponse>> {
    let row: Option<MeRow> = crate::db::query_as(
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

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let max_len = a.len().max(b.len());
    let mut diff = a.len() ^ b.len();
    for i in 0..max_len {
        let av = a.get(i).copied().unwrap_or(0);
        let bv = b.get(i).copied().unwrap_or(0);
        diff |= usize::from(av ^ bv);
    }
    diff == 0
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

fn rate_key(scope: &str, email: &str) -> String {
    format!("auth:{scope}:{}", email.trim().to_ascii_lowercase())
}
