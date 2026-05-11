use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{routing::get, Json, Router};
use driftbase_common::telemetry;
use serde::Serialize;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

mod admin;
mod agent;
mod agent_updates;
mod auth;
mod builds;
mod config;
mod console;
mod credentials;
mod crypto;
mod db;
mod deployments;
mod domains;
mod entity;
mod error;
mod migration;
mod nodes;
mod private_network;
mod projects;
mod provisioner;
mod rate_limit;
mod registry_proxy;
mod scheduler;
mod services;
mod ssh_keys;
mod state;
mod volumes;
mod workspaces;

use axum::extract::State;

use crate::config::Config;
use crate::state::AppState;

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

/// Non-secret subset of the config exposed to the frontend. Lets the UI show
/// hints that depend on what's configured (e.g. "this credential targets the
/// bundled registry") without hard-coding hostnames.
#[derive(Serialize)]
struct PublicSettings {
    registry_site: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    match dotenvy::dotenv() {
        Ok(path) => eprintln!("loaded env from {}", path.display()),
        Err(e) if e.not_found() => {}
        Err(e) => eprintln!("warning: could not load .env: {e}"),
    }
    telemetry::init("driftbase-controlplane");
    let loaded = Config::from_env().context("loading config")?;
    let config = loaded.config;
    let master_key = loaded.master_key;
    let pool = connect_and_migrate(&config).await?;

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

async fn connect_and_migrate(config: &Config) -> Result<db::Db> {
    match config.database_pooled_url.as_deref() {
        Some(runtime_url) if runtime_url != config.database_url => {
            let migration_pool = db::connect(&config.database_url)
                .await
                .context("connecting migration database")?;
            db::migrate(&migration_pool).await?;
            drop(migration_pool);

            db::connect(runtime_url)
                .await
                .context("connecting pooled runtime database")
        }
        _ => {
            let pool = db::connect(&config.database_url)
                .await
                .context("connecting database")?;
            db::migrate(&pool).await?;
            Ok(pool)
        }
    }
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
        .route(
            "/public-settings",
            get(|State(state): State<AppState>| async move {
                Json(PublicSettings {
                    registry_site: state.config().registry_site.clone(),
                })
            }),
        )
        .nest("/auth", auth::routes::router())
        .merge(admin::routes::router())
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
        .merge(volumes::routes::router())
        .merge(agent::routes::router())
        .merge(console::routes::router());

    // The registry proxy is mounted at the root (not under /api/v1) because
    // docker clients hit `<registry-host>/v2/...` verbatim and we can't
    // change their URL shape. Caddy forwards `{$REGISTRY_SITE}/v2/*` straight
    // to us with the path intact.
    Router::new()
        .nest("/api/v1", api)
        .merge(registry_proxy::router())
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_methods(Any)
                .allow_headers(Any)
                .allow_origin(Any),
        )
        .with_state(state)
}
