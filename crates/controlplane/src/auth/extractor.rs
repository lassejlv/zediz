use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::cookie::CookieJar;
use zediz_common::Id;

use crate::auth::session;
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Clone, Debug)]
pub struct AuthUser {
    pub user_id: Id,
    #[allow(dead_code)] // used later for session revocation
    pub session_id: Id,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(session::COOKIE_NAME)
            .map(|c| c.value().to_string())
            .ok_or(ApiError::Unauthorized)?;

        let loaded = session::load(state.pool(), &token)
            .await
            .map_err(ApiError::Internal)?
            .ok_or(ApiError::Unauthorized)?;

        Ok(AuthUser {
            user_id: loaded.user_id,
            session_id: loaded.session_id,
        })
    }
}
