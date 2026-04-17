use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{routing::get, Json, Router};
use serde::Serialize;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use zediz_common::telemetry;

mod agent;
mod auth;
mod builds;
mod config;
mod credentials;
mod crypto;
mod db;
mod deployments;
mod domains;
mod error;
mod nodes;
mod projects;
mod provisioner;
mod scheduler;
mod services;
mod ssh_keys;
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
    match dotenvy::dotenv() {
        Ok(path) => eprintln!("loaded env from {}", path.display()),
        Err(e) if e.not_found() => {}
        Err(e) => eprintln!("warning: could not load .env: {e}"),
    }
    telemetry::init("zediz-controlplane");
    let loaded = Config::from_env().context("loading config")?;
    let config = loaded.config;
    let master_key = loaded.master_key;
    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;

    let bind: SocketAddr = config.bind_addr;
    let state = AppState::new(pool, config, master_key);
    scheduler::spawn(state.clone());
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
        .merge(workspaces::invites::router())
        .merge(credentials::routes::router())
        .merge(ssh_keys::routes::router())
        .merge(projects::routes::router())
        .merge(services::routes::router())
        .merge(builds::routes::router())
        .merge(nodes::routes::router())
        .merge(deployments::routes::router())
        .merge(domains::routes::router())
        .merge(agent::routes::router());

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
