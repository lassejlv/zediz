use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::oneshot;
use ulid::Ulid;

use crate::agent::commands::{self, CommandKind};
use crate::agent::routes::NodeAuth;
use crate::auth::AuthUser;
use crate::console::bridge;
use crate::deployments;
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, ConsoleAgentConnection};

const SESSION_TIMEOUT: Duration = Duration::from_secs(30);
const CONTAINER_PREFIX: &str = "driftbase-";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/deployments/:id/console/ws", get(open_browser_ws))
        .route("/agent/console/:session_id/ws", get(open_agent_ws))
}

#[derive(Deserialize)]
struct OpenQuery {
    #[serde(default)]
    cols: Option<u16>,
    #[serde(default)]
    rows: Option<u16>,
}

async fn open_browser_ws(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(deployment_id): Path<String>,
    Query(q): Query<OpenQuery>,
    ws: WebSocketUpgrade,
) -> ApiResult<axum::response::Response> {
    let row = deployments::authorize_admin(state.pool(), &deployment_id, &auth.user_id).await?;

    if row.status != "running" {
        return Err(ApiError::Validation(format!(
            "deployment is not running (status={})",
            row.status
        )));
    }
    let node_id = row.node_id.clone().ok_or_else(|| {
        ApiError::Validation("deployment has not been placed on a node yet".into())
    })?;
    let cols = q.cols.unwrap_or(80).clamp(2, 1000);
    let rows = q.rows.unwrap_or(24).clamp(2, 1000);
    let container_name = format!("{CONTAINER_PREFIX}{deployment_id}");
    let session_id = Ulid::new().to_string();

    let (tx, rx) = oneshot::channel::<ConsoleAgentConnection>();
    state.console_sessions().insert(session_id.clone(), tx);

    commands::enqueue(
        state.pool(),
        &node_id,
        Some(&deployment_id),
        CommandKind::OpenConsole,
        json!({
            "session_id": session_id,
            "container_name": container_name,
            "cols": cols,
            "rows": rows,
        }),
    )
    .await
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?;

    let user_id = auth.user_id.to_string();
    tracing::info!(
        %user_id,
        %deployment_id,
        %session_id,
        "console session opened",
    );

    let response = ws
        .max_message_size(8 * 1024 * 1024)
        .on_upgrade(move |browser_ws| async move {
            let _guard = SessionGuard {
                registry: state.console_sessions().clone(),
                session_id: session_id.clone(),
            };
            match tokio::time::timeout(SESSION_TIMEOUT, rx).await {
                Ok(Ok(ConsoleAgentConnection::Connected(agent_ws))) => {
                    bridge::run(browser_ws, *agent_ws).await;
                }
                Ok(Ok(ConsoleAgentConnection::Failed(reason))) => {
                    let _ = close_with_reason(browser_ws, &reason).await;
                }
                Ok(Err(_)) => {
                    let _ = close_with_reason(browser_ws, "agent did not connect").await;
                }
                Err(_) => {
                    let _ =
                        close_with_reason(browser_ws, "timed out waiting for the agent to connect")
                            .await;
                }
            }
            tracing::info!(
                %user_id,
                %deployment_id,
                %session_id,
                "console session closed",
            );
        });
    Ok(response)
}

async fn close_with_reason(mut ws: WebSocket, reason: &str) -> Result<(), axum::Error> {
    ws.send(Message::Text(
        json!({ "type": "error", "message": reason }).to_string(),
    ))
    .await?;
    ws.send(Message::Close(Some(axum::extract::ws::CloseFrame {
        code: 1011,
        reason: reason.to_string().into(),
    })))
    .await
}

struct SessionGuard {
    registry: std::sync::Arc<dashmap::DashMap<String, oneshot::Sender<ConsoleAgentConnection>>>,
    session_id: String,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.registry.remove(&self.session_id);
    }
}

async fn open_agent_ws(
    State(state): State<AppState>,
    NodeAuth { .. }: NodeAuth,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> ApiResult<axum::response::Response> {
    let entry = state
        .console_sessions()
        .remove(&session_id)
        .ok_or(ApiError::NotFound)?;
    let sender = entry.1;
    Ok(ws
        .max_message_size(8 * 1024 * 1024)
        .on_upgrade(move |agent_ws| async move {
            if sender
                .send(ConsoleAgentConnection::Connected(Box::new(agent_ws)))
                .is_err()
            {
                tracing::debug!(
                    %session_id,
                    "browser side dropped before agent connected",
                );
            }
        }))
}

pub fn fail_session(state: &AppState, session_id: &str, reason: String) {
    if let Some((_, sender)) = state.console_sessions().remove(session_id) {
        let _ = sender.send(ConsoleAgentConnection::Failed(reason));
    }
}
