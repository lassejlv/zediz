pub mod routes;

use sqlx::PgPool;
use driftbase_common::Id;

use crate::error::{ApiError, ApiResult};

/// Load `is_platform_admin` for the calling user. Handlers call this at
/// the top to gate /admin endpoints — a tiny extra query (~1 row lookup)
/// in exchange for not needing a custom extractor.
pub async fn require_platform_admin(pool: &PgPool, user_id: &Id) -> ApiResult<()> {
    let row: Option<(bool,)> = sqlx::query_as("SELECT is_platform_admin FROM users WHERE id = $1")
        .bind(user_id.to_string())
        .fetch_optional(pool)
        .await?;
    match row {
        Some((true,)) => Ok(()),
        _ => Err(ApiError::Forbidden(
            "Platform admin access required.".into(),
        )),
    }
}
