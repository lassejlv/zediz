use anyhow::{Context, Result};
use clap::Parser;
use driftbase_common::telemetry;
use std::time::Duration;

mod build;
mod caddy;
mod cancel;
mod client;
mod docker;
mod executor;
mod private_network;
mod self_update;

use crate::client::{ControlPlaneClient, RegisterInput};
use crate::docker::DockerExec;

#[derive(Parser, Debug)]
#[command(name = "driftbase-agent", version, about = "Driftbase node agent")]
struct Args {
    /// URL of the control plane (e.g. https://cp.driftbase.example).
    #[arg(long, env = "DRIFTBASE_CONTROL_PLANE_URL")]
    control_plane_url: String,

    /// One-shot bootstrap token issued by the control plane at provision time.
    #[arg(long, env = "DRIFTBASE_BOOTSTRAP_TOKEN")]
    bootstrap_token: Option<String>,

    /// Persistent node token (skips registration if supplied).
    #[arg(long, env = "DRIFTBASE_NODE_TOKEN")]
    node_token: Option<String>,

    /// Override reported hostname (defaults to OS hostname).
    #[arg(long, env = "DRIFTBASE_NODE_HOSTNAME")]
    hostname: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    match dotenvy::dotenv() {
        Ok(path) => eprintln!("loaded env from {}", path.display()),
        Err(e) if e.not_found() => {}
        Err(e) => eprintln!("warning: could not load .env: {e}"),
    }
    telemetry::init("driftbase-agent");
    let args = Args::parse();

    let client = ControlPlaneClient::new(&args.control_plane_url);
    let private_network = private_network::load_or_create_identity().await;
    let mut agent_update = self_update::detect().await;

    let (node_token, node_id) = if let Some(tok) = args.node_token.clone() {
        tracing::info!("using pre-issued node token");
        // We still need a node_id to log against; we'll learn it from first heartbeat response,
        // but for now the token itself carries the identity server-side. Pass empty locally.
        (tok, String::new())
    } else {
        let bootstrap = args
            .bootstrap_token
            .clone()
            .context("DRIFTBASE_BOOTSTRAP_TOKEN or DRIFTBASE_NODE_TOKEN is required")?;
        let host = args
            .hostname
            .clone()
            .unwrap_or_else(|| hostname().unwrap_or_else(|| "driftbase-node".into()));
        let specs = host_resources();
        let resp = client
            .register(RegisterInput {
                bootstrap_token: &bootstrap,
                hostname: &host,
                cpu: specs.0,
                mem: specs.1,
                disk: specs.2,
                private_network: private_network.as_ref(),
                agent_update: &agent_update,
            })
            .await
            .context("registering with control plane")?;
        tracing::info!(node = %resp.node_id, "registered");
        (resp.node_token, resp.node_id)
    };

    match self_update::persist_node_token(&node_token).await {
        Ok(()) => {
            agent_update = self_update::detect().await;
        }
        Err(e) => {
            tracing::warn!(error = ?e, "node token was not persisted for agent self-update");
            agent_update.self_update_capable = false;
        }
    }

    let docker = DockerExec::connect().context("connecting to local docker")?;

    // Bring up the Caddy sidecar + shared network once at boot; update_routes
    // commands will keep its config in sync after that.
    if let Err(e) = caddy::ensure_running(&docker.inner()).await {
        tracing::warn!(error = ?e, "caddy sidecar bootstrap failed (will retry on first update_routes)");
    }

    let heartbeat_interval = Duration::from_secs(10);
    let mut exec = executor::Executor::new(
        client,
        docker,
        node_token,
        node_id,
        private_network,
        agent_update,
    );
    loop {
        if let Err(e) = exec.tick().await {
            tracing::warn!(error = ?e, "agent tick failed");
        }
        tokio::time::sleep(heartbeat_interval).await;
    }
}

fn hostname() -> Option<String> {
    std::env::var("HOSTNAME").ok().or_else(|| {
        std::fs::read_to_string("/etc/hostname")
            .ok()
            .map(|s| s.trim().into())
    })
}

/// Returns `(cpu_millis, memory_mb, disk_mb)` derived from the host.
/// Simplified; a real agent reads /proc/meminfo and df.
fn host_resources() -> (i32, i32, i32) {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1) as i32;
    (cpus * 1000, 4096, 50 * 1024)
}
