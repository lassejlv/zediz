use anyhow::{anyhow, Context, Result};
use bollard::container::{
    Config, CreateContainerOptions, InspectContainerOptions, RemoveContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::{HostConfig, PortBinding, RestartPolicy, RestartPolicyNameEnum};
use bollard::Docker;
use futures::StreamExt;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::time::Duration;

/// Name of the shared docker network all deployments + the Caddy sidecar join
/// so Caddy can reach containers by name.
pub const NETWORK: &str = "driftbase";
pub const CADDY_CONTAINER: &str = "driftbase-caddy";
pub const CADDY_IMAGE: &str = "caddy:2-alpine";
pub const CADDY_ADMIN_URL: &str = "http://127.0.0.1:2019";

/// Ensure the shared `driftbase` docker network exists.
pub async fn ensure_network(docker: &Docker) -> Result<()> {
    let existing = docker.list_networks::<String>(None).await?;
    if existing.iter().any(|n| n.name.as_deref() == Some(NETWORK)) {
        return Ok(());
    }
    docker
        .create_network(bollard::network::CreateNetworkOptions {
            name: NETWORK.to_string(),
            driver: "bridge".into(),
            ..Default::default()
        })
        .await?;
    Ok(())
}

/// Ensure the Caddy sidecar container is running and joined to the shared
/// network. Idempotent — safe to call on every tick.
pub async fn ensure_running(docker: &Docker) -> Result<()> {
    ensure_network(docker).await?;

    // Already running?
    if let Ok(inspect) = docker
        .inspect_container(CADDY_CONTAINER, None::<InspectContainerOptions>)
        .await
    {
        let running = inspect.state.and_then(|s| s.running).unwrap_or(false);
        if running {
            return Ok(());
        }
        // Dead/stopped — remove and recreate.
        let _ = docker
            .remove_container(
                CADDY_CONTAINER,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;
    }

    // Pull the image (best-effort; swallow known streaming quirks).
    let mut pull = docker.create_image(
        Some(CreateImageOptions {
            from_image: "caddy",
            tag: "2-alpine",
            ..Default::default()
        }),
        None,
        None,
    );
    while let Some(event) = pull.next().await {
        if let Err(e) = event {
            let msg = e.to_string();
            if !msg.contains("expected value at line 1 column 1") {
                return Err(anyhow!("pulling caddy image: {e}"));
            }
        }
    }

    // Publish 80/443 on the host + expose admin API on localhost.
    let mut port_bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
    for p in ["80", "443"] {
        for proto in ["tcp", "udp"] {
            // Only bind UDP on 443 (HTTP/3). 80/udp isn't useful.
            if p == "80" && proto == "udp" {
                continue;
            }
            port_bindings.insert(
                format!("{p}/{proto}"),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".into()),
                    host_port: Some(p.to_string()),
                }]),
            );
        }
    }
    port_bindings.insert(
        "2019/tcp".into(),
        Some(vec![PortBinding {
            host_ip: Some("127.0.0.1".into()),
            host_port: Some("2019".into()),
        }]),
    );

    let mut exposed: HashMap<String, HashMap<(), ()>> = HashMap::new();
    for k in ["80/tcp", "443/tcp", "443/udp", "2019/tcp"] {
        exposed.insert(k.into(), HashMap::new());
    }

    let host_config = HostConfig {
        port_bindings: Some(port_bindings),
        network_mode: Some(NETWORK.into()),
        restart_policy: Some(RestartPolicy {
            name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
            ..Default::default()
        }),
        binds: Some(vec![
            "driftbase-caddy-data:/data".into(),
            "driftbase-caddy-config:/config".into(),
        ]),
        ..Default::default()
    };

    // Bootstrap script: on first boot write a minimal config with the admin
    // API bound to 0.0.0.0 (so Docker port binding can actually reach it);
    // on subsequent boots --resume picks up /config/caddy/autosave.json that
    // Caddy persists every time we POST /load.
    let bootstrap = r#"set -eu
mkdir -p /etc/caddy
cat > /etc/caddy/bootstrap.json <<'JSON'
{"admin":{"listen":"0.0.0.0:2019"},"apps":{"http":{"servers":{"driftbase":{"listen":[":80",":443"],"routes":[]}}}}}
JSON
exec caddy run --config /etc/caddy/bootstrap.json --resume
"#;

    let config: Config<String> = Config {
        image: Some(CADDY_IMAGE.into()),
        exposed_ports: Some(exposed),
        host_config: Some(host_config),
        entrypoint: Some(vec!["sh".into(), "-c".into(), bootstrap.to_string()]),
        ..Default::default()
    };

    // Seed an empty config if the volume is fresh.
    let created = docker
        .create_container(
            Some(CreateContainerOptions {
                name: CADDY_CONTAINER.to_string(),
                platform: None,
            }),
            config,
        )
        .await?;
    docker.start_container::<String>(&created.id, None).await?;

    // Give Caddy a moment to come up before any subsequent /load POST.
    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(())
}

/// Ensure a running deployment container is attached to the shared `driftbase`
/// network. Idempotent.
pub async fn ensure_container_on_network(docker: &Docker, container: &str) -> Result<()> {
    let inspect = match docker
        .inspect_container(container, None::<InspectContainerOptions>)
        .await
    {
        Ok(i) => i,
        Err(_) => return Ok(()), // container doesn't exist — nothing to do
    };
    let already = inspect
        .network_settings
        .as_ref()
        .and_then(|ns| ns.networks.as_ref())
        .map(|nets| nets.contains_key(NETWORK))
        .unwrap_or(false);
    if already {
        return Ok(());
    }
    let _ = docker
        .connect_network(
            NETWORK,
            bollard::network::ConnectNetworkOptions {
                container,
                ..Default::default()
            },
        )
        .await;
    Ok(())
}

pub struct Route {
    pub hostname: String,
    pub container_port: u16,
    pub container_name: String,
}

/// Push a new Caddy config describing the given hostname→container routes.
/// Uses Caddy's JSON API on the admin port. Caddy auto-issues Let's Encrypt
/// certs for every hostname whose DNS already points here.
pub async fn apply_routes(routes: &[Route]) -> Result<()> {
    let http = reqwest::Client::new();
    let cfg = build_config(routes);
    let res = http
        .post(format!("{CADDY_ADMIN_URL}/load"))
        .json(&cfg)
        .send()
        .await
        .context("POSTing to caddy admin /load")?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("caddy /load: {status}: {body}"));
    }
    Ok(())
}

fn build_config(routes: &[Route]) -> JsonValue {
    // One route per hostname, reverse-proxying to the container on the
    // shared docker network by container name at the configured port.
    let routes_json: Vec<JsonValue> = routes
        .iter()
        .map(|r| {
            json!({
                "match": [{ "host": [r.hostname] }],
                "handle": [{
                    "handler": "reverse_proxy",
                    "upstreams": [{
                        "dial": format!("{}:{}", r.container_name, r.container_port)
                    }],
                    "flush_interval": -1,
                }],
                "terminal": true,
            })
        })
        .collect();

    // IMPORTANT: include `admin.listen` in every /load so we don't drop the
    // admin API and lock ourselves out of future route pushes.
    json!({
        "admin": { "listen": "0.0.0.0:2019" },
        "apps": {
            "http": {
                "servers": {
                    "driftbase": {
                        "listen": [":443", ":80"],
                        "routes": routes_json,
                        "automatic_https": {
                            "disable_redirects": false
                        }
                    }
                }
            }
        }
    })
}
