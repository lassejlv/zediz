use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{routing::get, Json, Router};
use serde::Serialize;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use zediz_common::telemetry;

mod auth;
mod config;
mod db;
mod error;
mod state;
mod workspaces;

use crate::config::Config;
use crate::state::AppState;

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init("zediz-controlplane");
    let config = Config::from_env().context("loading config")?;
    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;

    let bind: SocketAddr = config.bind_addr;
    let state = AppState::new(pool, config);
    let app = router(state);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    tracing::info!(addr = %bind, "control plane listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn router(state: AppState) -> Router {
    let api = Router::new()
        .route(
            "/healthz",
            get(|| async {
                Json(Health {
                    status: "ok",
                    version: env!("CARGO_PKG_VERSION"),
                })
            }),
        )
        .nest("/auth", auth::routes::router())
        .merge(workspaces::routes::router())
        .merge(workspaces::invites::router());

    Router::new()
        .nest("/api/v1", api)
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_methods(Any)
                .allow_headers(Any)
                .allow_origin(Any),
        )
        .with_state(state)
}
