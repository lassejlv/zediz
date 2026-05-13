use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Redirect;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::auth::AuthUser;
use crate::builds::BuildTrigger;
use crate::error::{ApiError, ApiResult};
use crate::github_app;
use crate::state::AppState;
use crate::workspaces::membership::{self, Role};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces/:slug/github/start", get(start))
        .route("/github/callback", get(callback))
        .route(
            "/workspaces/:slug/github/installations",
            get(list_installations),
        )
        .route(
            "/workspaces/:slug/github/repositories",
            get(list_repositories),
        )
        .route(
            "/workspaces/:slug/github/installations/:installation_id/sync",
            post(sync_installation),
        )
        .route("/github/webhook", post(webhook))
}

async fn start(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> ApiResult<Redirect> {
    let config = github_app::require_config(state.config())?;
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;
    let signed_state = github_app::sign_state(
        state.master_key(),
        &ctx.workspace_id.to_string(),
        &slug,
        &auth.user_id.to_string(),
    );
    Ok(Redirect::temporary(&github_app::install_url(
        config,
        &signed_state,
    )))
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: String,
    installation_id: Option<i64>,
}

async fn callback(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<CallbackQuery>,
) -> ApiResult<Redirect> {
    let config = github_app::require_config(state.config())?;
    let verified = github_app::verify_state(state.master_key(), &query.state)?;
    if verified.user_id != auth.user_id.to_string() {
        return Err(ApiError::Unauthorized);
    }
    let ctx = membership::resolve(state.pool(), &verified.workspace_slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;
    if ctx.workspace_id.to_string() != verified.workspace_id {
        return Err(ApiError::Unauthorized);
    }

    let Some(expected_installation_id) = query.installation_id else {
        return Err(ApiError::Validation(
            "GitHub did not return an installation id".into(),
        ));
    };

    let installation = if let Some(code) = query.code.as_deref().filter(|c| !c.is_empty()) {
        let token = github_app::exchange_oauth_code(config, code)
            .await
            .map_err(ApiError::Internal)?;
        let installations = github_app::user_installations(&token.access_token)
            .await
            .map_err(ApiError::Internal)?;
        installations
            .into_iter()
            .find(|installation| installation.id == expected_installation_id)
            .ok_or(ApiError::Unauthorized)?
    } else {
        github_app::installation_by_id(config, expected_installation_id)
            .await
            .map_err(ApiError::Internal)?
    };

    github_app::upsert_installation(state.pool(), &ctx.workspace_id.to_string(), &installation)
        .await
        .map_err(ApiError::Internal)?;
    let repos = github_app::installation_repositories(config, installation.id)
        .await
        .map_err(ApiError::Internal)?;
    github_app::sync_repositories(
        state.pool(),
        &ctx.workspace_id.to_string(),
        installation.id,
        &repos,
    )
    .await
    .map_err(ApiError::Internal)?;

    Ok(Redirect::temporary(&format!(
        "{}/w/{}/credentials?github=connected",
        state.config().public_url.trim_end_matches('/'),
        github_app::percent_encode(&verified.workspace_slug)
    )))
}

async fn list_installations(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> ApiResult<Json<Vec<github_app::GitHubInstallationSummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;
    let rows = github_app::list_installations(state.pool(), &ctx.workspace_id.to_string())
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(rows))
}

async fn list_repositories(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(slug): Path<String>,
) -> ApiResult<Json<Vec<github_app::GitHubRepositorySummary>>> {
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Member)?;
    let rows = github_app::list_repositories(state.pool(), &ctx.workspace_id.to_string())
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(rows))
}

async fn sync_installation(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((slug, installation_id)): Path<(String, i64)>,
) -> ApiResult<Json<Vec<github_app::GitHubRepositorySummary>>> {
    let config = github_app::require_config(state.config())?;
    let ctx = membership::resolve(state.pool(), &slug, &auth.user_id).await?;
    membership::require(&ctx, Role::Admin)?;
    let exists: Option<(i64,)> = crate::db::query_tuple(
        "SELECT installation_id FROM github_installations \
         WHERE workspace_id = $1 AND installation_id = $2",
    )
    .bind(ctx.workspace_id.to_string())
    .bind(installation_id)
    .fetch_optional(state.pool())
    .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound);
    }
    let repos = github_app::installation_repositories(config, installation_id)
        .await
        .map_err(ApiError::Internal)?;
    github_app::sync_repositories(
        state.pool(),
        &ctx.workspace_id.to_string(),
        installation_id,
        &repos,
    )
    .await
    .map_err(ApiError::Internal)?;
    let rows = github_app::list_repositories(state.pool(), &ctx.workspace_id.to_string())
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(rows))
}

async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    let config = github_app::require_config(state.config())?;
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok());
    if !github_app::verify_webhook_signature(&config.webhook_secret, &body, signature) {
        return Err(ApiError::Unauthorized);
    }

    let event = headers
        .get("x-github-event")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    let delivery_id = headers
        .get("x-github-delivery")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::Validation("missing X-GitHub-Delivery".into()))?
        .to_string();
    let payload: JsonValue = serde_json::from_slice(&body)
        .map_err(|e| ApiError::Validation(format!("invalid GitHub webhook JSON: {e}")))?;
    let installation_id = payload
        .get("installation")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_i64());

    let inserted: Option<(String,)> = crate::db::query_tuple(
        "INSERT INTO github_webhook_deliveries (delivery_id, event, installation_id, status) \
         VALUES ($1, $2, $3, 'processing') \
         ON CONFLICT (delivery_id) DO NOTHING \
         RETURNING delivery_id",
    )
    .bind(&delivery_id)
    .bind(&event)
    .bind(installation_id)
    .fetch_optional(state.pool())
    .await?;
    if inserted.is_none() {
        return Ok(Json(serde_json::json!({ "ok": true, "duplicate": true })));
    }

    let result = match event.as_str() {
        "push" => process_push(&state, &payload, &delivery_id).await,
        "installation" => process_installation(&state, &payload).await,
        "installation_repositories" => process_installation_repositories(&state, &payload).await,
        "ping" => Ok("ignored"),
        _ => Ok("ignored"),
    };

    match result {
        Ok(status) => {
            crate::db::query(
                "UPDATE github_webhook_deliveries \
                 SET status = $1, processed_at = now() \
                 WHERE delivery_id = $2",
            )
            .bind(status)
            .bind(&delivery_id)
            .execute(state.pool())
            .await?;
            Ok(Json(serde_json::json!({ "ok": true, "status": status })))
        }
        Err(error) => {
            let message = error.to_string();
            crate::db::query(
                "UPDATE github_webhook_deliveries \
                 SET status = 'failed', error = $1, processed_at = now() \
                 WHERE delivery_id = $2",
            )
            .bind(&message)
            .bind(&delivery_id)
            .execute(state.pool())
            .await?;
            Err(ApiError::Internal(error))
        }
    }
}

async fn process_push(
    state: &AppState,
    payload: &JsonValue,
    delivery_id: &str,
) -> anyhow::Result<&'static str> {
    let Some(reference) = payload.get("ref").and_then(|v| v.as_str()) else {
        return Ok("ignored");
    };
    let Some(branch) = github_app::branch_from_ref(reference) else {
        return Ok("ignored");
    };
    if payload
        .get("deleted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Ok("ignored");
    }
    let Some(sha) = payload.get("after").and_then(|v| v.as_str()) else {
        return Ok("ignored");
    };
    if sha.chars().all(|ch| ch == '0') {
        return Ok("ignored");
    }
    let Some(installation_id) = payload
        .get("installation")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_i64())
    else {
        return Ok("ignored");
    };
    let Some(repository_id) = payload
        .get("repository")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_i64())
    else {
        return Ok("ignored");
    };

    let services: Vec<(String,)> = crate::db::query_tuple(
        "SELECT s.id \
         FROM services s \
         JOIN projects p ON p.id = s.project_id \
         JOIN github_installations i \
           ON i.workspace_id = p.workspace_id \
          AND i.installation_id = s.github_installation_id \
         WHERE s.source = 'git' \
           AND s.github_installation_id = $1 \
           AND s.github_repository_id = $2 \
           AND s.github_auto_deploy = TRUE \
           AND COALESCE(s.git_branch, 'main') = $3 \
           AND i.active = TRUE",
    )
    .bind(installation_id)
    .bind(repository_id)
    .bind(branch)
    .fetch_all(state.pool())
    .await?;

    for (service_id,) in services {
        let queued = crate::services::routes::queue_git_deployment_for_service(
            state,
            &service_id,
            Some(BuildTrigger {
                trigger_kind: "github_push",
                git_ref: Some(reference),
                git_sha: Some(sha),
                github_delivery_id: Some(delivery_id),
            }),
        )
        .await
        .map_err(|err| anyhow::anyhow!("{err:?}"))?;
        if let Err(err) = github_app::post_commit_status_for_build(
            state.pool(),
            state.config(),
            &queued.build_id,
            "pending",
            "Driftbase build queued",
        )
        .await
        {
            tracing::warn!(error = ?err, build = %queued.build_id, "posting GitHub pending status failed");
        }
        tracing::info!(
            service = %service_id,
            deployment = %queued.deployment_id,
            build = %queued.build_id,
            delivery = %delivery_id,
            "queued GitHub push deployment",
        );
    }

    crate::scheduler::nudge(state);
    Ok("processed")
}

async fn process_installation(
    state: &AppState,
    payload: &JsonValue,
) -> anyhow::Result<&'static str> {
    let action = payload.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let Some(installation_id) = payload
        .get("installation")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_i64())
    else {
        return Ok("ignored");
    };
    match action {
        "deleted" | "suspend" | "suspended" => {
            crate::db::query(
                "UPDATE github_installations \
                 SET active = FALSE, suspended_at = COALESCE(suspended_at, now()), updated_at = now() \
                 WHERE installation_id = $1",
            )
            .bind(installation_id)
            .execute(state.pool())
            .await?;
        }
        "unsuspend" | "unsuspended" => {
            crate::db::query(
                "UPDATE github_installations \
                 SET active = TRUE, suspended_at = NULL, updated_at = now() \
                 WHERE installation_id = $1",
            )
            .bind(installation_id)
            .execute(state.pool())
            .await?;
        }
        _ => {}
    }
    Ok("processed")
}

async fn process_installation_repositories(
    state: &AppState,
    payload: &JsonValue,
) -> anyhow::Result<&'static str> {
    let Some(config) = state.config().github_app.as_ref() else {
        return Ok("ignored");
    };
    let Some(installation_id) = payload
        .get("installation")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_i64())
    else {
        return Ok("ignored");
    };
    let workspaces: Vec<(String,)> = crate::db::query_tuple(
        "SELECT workspace_id FROM github_installations WHERE installation_id = $1",
    )
    .bind(installation_id)
    .fetch_all(state.pool())
    .await?;
    if workspaces.is_empty() {
        return Ok("ignored");
    }
    let repos = github_app::installation_repositories(config, installation_id).await?;
    for (workspace_id,) in workspaces {
        github_app::sync_repositories(state.pool(), &workspace_id, installation_id, &repos).await?;
    }
    Ok("processed")
}
