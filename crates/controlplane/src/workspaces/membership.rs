use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use zediz_common::Id;

use crate::error::{ApiError, ApiResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Admin,
    Member,
    Viewer,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
            Role::Viewer => "viewer",
        }
    }

    /// Hierarchy: owner > admin > member > viewer.
    pub fn rank(self) -> u8 {
        match self {
            Role::Owner => 4,
            Role::Admin => 3,
            Role::Member => 2,
            Role::Viewer => 1,
        }
    }

    pub fn at_least(self, required: Role) -> bool {
        self.rank() >= required.rank()
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "owner" => Ok(Role::Owner),
            "admin" => Ok(Role::Admin),
            "member" => Ok(Role::Member),
            "viewer" => Ok(Role::Viewer),
            other => Err(format!("unknown role: {other}")),
        }
    }
}

pub struct WorkspaceContext {
    pub workspace_id: Id,
    #[allow(dead_code)]
    pub slug: String,
    pub role: Role,
}

pub async fn resolve(pool: &PgPool, slug: &str, user_id: &Id) -> ApiResult<WorkspaceContext> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT w.id, w.slug, m.role \
         FROM workspaces w \
         JOIN workspace_members m ON m.workspace_id = w.id \
         WHERE w.slug = $1 AND m.user_id = $2",
    )
    .bind(slug)
    .bind(user_id.to_string())
    .fetch_optional(pool)
    .await?;

    let (id, slug, role) = row.ok_or(ApiError::NotFound)?;
    Ok(WorkspaceContext {
        workspace_id: id
            .parse()
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?,
        slug,
        role: role.parse().map_err(ApiError::Validation)?,
    })
}

pub fn require(ctx: &WorkspaceContext, minimum: Role) -> ApiResult<()> {
    if ctx.role.at_least(minimum) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}
