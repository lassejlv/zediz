use anyhow::{anyhow, Context, Result};
use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions};
use bollard::image::CreateImageOptions;
use bollard::models::{
    EndpointIpamConfig, EndpointSettings, HostConfig, Ipam, IpamConfig, RestartPolicy,
    RestartPolicyNameEnum,
};
use bollard::network::CreateNetworkOptions;
use bollard::Docker;
use futures::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const DEFAULT_NETWORK_DIR: &str = "/var/lib/driftbase/network";
const PRIVATE_KEY_FILE: &str = "wg0.key";
const WG_CONFIG_FILE: &str = "wg0.conf";
const COREDNS_IMAGE: &str = "coredns/coredns:1.11.3";

#[derive(Debug, Clone)]
pub struct Identity {
    pub public_key: String,
    pub listen_port: i32,
}

#[derive(Debug, Deserialize)]
pub struct SyncSpec {
    pub interface: WireGuardInterface,
    #[serde(default)]
    pub peers: Vec<WireGuardPeer>,
    #[serde(default)]
    pub projects: Vec<ProjectNetworkSpec>,
}

#[derive(Debug, Deserialize)]
pub struct WireGuardInterface {
    pub name: String,
    pub address: String,
    pub listen_port: i32,
}

#[derive(Debug, Deserialize)]
pub struct WireGuardPeer {
    pub public_key: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub persistent_keepalive_seconds: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectNetworkSpec {
    pub project_id: String,
    pub network_name: String,
    pub node_subnet: String,
    pub gateway_ip: String,
    pub dns_ip: String,
    pub domain: String,
    pub dns_container_name: String,
    #[serde(default)]
    pub records: Vec<DnsRecord>,
}

#[derive(Debug, Deserialize)]
pub struct DnsRecord {
    pub fqdn: String,
    pub ip: String,
}

pub async fn load_or_create_identity() -> Option<Identity> {
    match load_or_create_identity_inner().await {
        Ok(identity) => Some(identity),
        Err(e) => {
            tracing::warn!(error = ?e, "private networking disabled");
            None
        }
    }
}

pub async fn sync(docker: Docker, payload: serde_json::Value) -> Result<()> {
    let spec: SyncSpec = serde_json::from_value(payload)
        .map_err(|e| anyhow!("bad sync_private_network payload: {e}"))?;
    let private_key_path = network_dir().join(PRIVATE_KEY_FILE);
    let private_key = tokio::fs::read_to_string(&private_key_path)
        .await
        .with_context(|| format!("reading {}", private_key_path.display()))?;

    configure_wireguard(&spec, private_key.trim()).await?;
    configure_forwarding().await?;
    for project in &spec.projects {
        ensure_project_network(&docker, project).await?;
        ensure_dns_sidecar(&docker, project).await?;
    }
    Ok(())
}

async fn load_or_create_identity_inner() -> Result<Identity> {
    let dir = network_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("creating {}", dir.display()))?;
    let private_key_path = dir.join(PRIVATE_KEY_FILE);
    let private_key = match tokio::fs::read_to_string(&private_key_path).await {
        Ok(key) => key,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = command_output("wg", &["genkey"]).await?;
            write_private_key(&private_key_path, key.trim()).await?;
            key
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", private_key_path.display())),
    };
    let public_key = wg_pubkey(private_key.trim()).await?;
    Ok(Identity {
        public_key,
        listen_port: 51820,
    })
}

async fn write_private_key(path: &Path, key: &str) -> Result<()> {
    tokio::fs::write(path, format!("{key}\n"))
        .await
        .with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        tokio::fs::set_permissions(path, perms)
            .await
            .with_context(|| format!("chmod 0600 {}", path.display()))?;
    }
    Ok(())
}

async fn wg_pubkey(private_key: &str) -> Result<String> {
    let mut child = Command::new("wg")
        .arg("pubkey")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("spawning wg pubkey")?;
    let mut stdin = child.stdin.take().context("opening wg pubkey stdin")?;
    stdin.write_all(private_key.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    drop(stdin);
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        return Err(anyhow!(
            "wg pubkey failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn configure_wireguard(spec: &SyncSpec, private_key: &str) -> Result<()> {
    let config = render_wg_config(spec, private_key);
    let config_path = network_dir().join(WG_CONFIG_FILE);
    tokio::fs::write(&config_path, config)
        .await
        .with_context(|| format!("writing {}", config_path.display()))?;

    let _ = command_status(
        "ip",
        &["link", "add", &spec.interface.name, "type", "wireguard"],
    )
    .await;
    command_status(
        "wg",
        &[
            "setconf",
            &spec.interface.name,
            config_path.to_str().unwrap_or_default(),
        ],
    )
    .await?;
    let address = format!("{}/32", spec.interface.address);
    command_status(
        "ip",
        &["address", "replace", &address, "dev", &spec.interface.name],
    )
    .await?;
    command_status("ip", &["link", "set", "up", "dev", &spec.interface.name]).await?;

    for peer in &spec.peers {
        for allowed in &peer.allowed_ips {
            command_status(
                "ip",
                &["route", "replace", allowed, "dev", &spec.interface.name],
            )
            .await?;
        }
    }
    Ok(())
}

fn render_wg_config(spec: &SyncSpec, private_key: &str) -> String {
    let mut out = format!(
        "[Interface]\nPrivateKey = {private_key}\nListenPort = {}\n\n",
        spec.interface.listen_port
    );
    for peer in &spec.peers {
        out.push_str("[Peer]\n");
        out.push_str(&format!("PublicKey = {}\n", peer.public_key));
        if let Some(endpoint) = peer.endpoint.as_deref() {
            out.push_str(&format!("Endpoint = {endpoint}\n"));
        }
        if !peer.allowed_ips.is_empty() {
            out.push_str(&format!("AllowedIPs = {}\n", peer.allowed_ips.join(", ")));
        }
        if let Some(keepalive) = peer.persistent_keepalive_seconds {
            out.push_str(&format!("PersistentKeepalive = {keepalive}\n"));
        }
        out.push('\n');
    }
    out
}

async fn configure_forwarding() -> Result<()> {
    let _ = command_status("sysctl", &["-w", "net.ipv4.ip_forward=1"]).await;
    ensure_iptables_rule(&["FORWARD", "-i", "wg0", "-j", "ACCEPT"]).await;
    ensure_iptables_rule(&["FORWARD", "-o", "wg0", "-j", "ACCEPT"]).await;
    Ok(())
}

async fn ensure_iptables_rule(args: &[&str]) {
    let check = Command::new("iptables").arg("-C").args(args).output().await;
    if matches!(check, Ok(out) if out.status.success()) {
        return;
    }
    let _ = Command::new("iptables").arg("-I").args(args).output().await;
}

async fn ensure_project_network(docker: &Docker, project: &ProjectNetworkSpec) -> Result<()> {
    let existing = docker.list_networks::<String>(None).await?;
    if existing
        .iter()
        .any(|n| n.name.as_deref() == Some(&project.network_name))
    {
        return Ok(());
    }

    let mut labels = HashMap::new();
    labels.insert("driftbase.managed".to_string(), "true".to_string());
    labels.insert(
        "driftbase.project_id".to_string(),
        project.project_id.clone(),
    );

    docker
        .create_network(CreateNetworkOptions {
            name: project.network_name.clone(),
            check_duplicate: true,
            driver: "bridge".to_string(),
            internal: false,
            attachable: false,
            ingress: false,
            ipam: Ipam {
                driver: Some("default".to_string()),
                config: Some(vec![IpamConfig {
                    subnet: Some(project.node_subnet.clone()),
                    gateway: Some(project.gateway_ip.clone()),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            enable_ipv6: false,
            options: HashMap::new(),
            labels,
        })
        .await
        .with_context(|| format!("creating docker network {}", project.network_name))?;
    Ok(())
}

async fn ensure_dns_sidecar(docker: &Docker, project: &ProjectNetworkSpec) -> Result<()> {
    write_coredns_files(project).await?;
    pull_image(docker, "coredns/coredns", "1.11.3").await?;

    let _ = docker
        .remove_container(
            &project.dns_container_name,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;

    let config_dir = dns_dir(&project.project_id);
    let binds = vec![format!("{}:/etc/coredns:ro", config_dir.display())];
    let host_config = HostConfig {
        network_mode: Some(project.network_name.clone()),
        binds: Some(binds),
        restart_policy: Some(RestartPolicy {
            name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mut endpoints = HashMap::new();
    endpoints.insert(
        project.network_name.clone(),
        EndpointSettings {
            ipam_config: Some(EndpointIpamConfig {
                ipv4_address: Some(project.dns_ip.clone()),
                ..Default::default()
            }),
            aliases: Some(vec!["dns".to_string()]),
            ..Default::default()
        },
    );

    let config: Config<String> = Config {
        image: Some(COREDNS_IMAGE.to_string()),
        cmd: Some(vec![
            "-conf".to_string(),
            "/etc/coredns/Corefile".to_string(),
        ]),
        host_config: Some(host_config),
        networking_config: Some(bollard::container::NetworkingConfig {
            endpoints_config: endpoints,
        }),
        ..Default::default()
    };

    let created = docker
        .create_container(
            Some(CreateContainerOptions {
                name: project.dns_container_name.clone(),
                platform: None,
            }),
            config,
        )
        .await
        .with_context(|| format!("creating {}", project.dns_container_name))?;
    docker
        .start_container::<String>(&created.id, None)
        .await
        .with_context(|| format!("starting {}", project.dns_container_name))?;
    Ok(())
}

async fn write_coredns_files(project: &ProjectNetworkSpec) -> Result<()> {
    let dir = dns_dir(&project.project_id);
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("creating {}", dir.display()))?;
    let corefile = format!(
        ".:53 {{\n    errors\n    hosts /etc/coredns/hosts {domain} {{\n        ttl 5\n        fallthrough\n    }}\n    forward . /etc/resolv.conf\n}}\n",
        domain = project.domain
    );
    let mut hosts = String::new();
    for record in &project.records {
        hosts.push_str(&format!("{} {}\n", record.ip, record.fqdn));
    }
    tokio::fs::write(dir.join("Corefile"), corefile).await?;
    tokio::fs::write(dir.join("hosts"), hosts).await?;
    Ok(())
}

async fn pull_image(docker: &Docker, image: &str, tag: &str) -> Result<()> {
    let mut pull = docker.create_image(
        Some(CreateImageOptions {
            from_image: image,
            tag,
            ..Default::default()
        }),
        None,
        None,
    );
    while let Some(event) = pull.next().await {
        if let Err(e) = event {
            let msg = e.to_string();
            if !msg.contains("expected value at line 1 column 1") {
                return Err(anyhow!("pulling {image}:{tag}: {e}"));
            }
        }
    }
    Ok(())
}

fn network_dir() -> PathBuf {
    std::env::var("DRIFTBASE_PRIVATE_NETWORK_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_NETWORK_DIR))
}

fn dns_dir(project_id: &str) -> PathBuf {
    network_dir().join("dns").join(project_id)
}

async fn command_output(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .await
        .with_context(|| format!("spawning {program}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "command failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

async fn command_status(program: &str, args: &[&str]) -> Result<()> {
    let out = Command::new(program)
        .args(args)
        .output()
        .await
        .with_context(|| format!("spawning {program}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "command failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_wireguard_peer_allowed_ips() {
        let spec = SyncSpec {
            interface: WireGuardInterface {
                name: "wg0".to_string(),
                address: "10.255.0.1".to_string(),
                listen_port: 51820,
            },
            peers: vec![WireGuardPeer {
                public_key: "peer-key".to_string(),
                endpoint: Some("203.0.113.10:51820".to_string()),
                allowed_ips: vec!["10.255.0.2/32".to_string(), "10.64.1.0/24".to_string()],
                persistent_keepalive_seconds: Some(25),
            }],
            projects: Vec::new(),
        };
        let rendered = render_wg_config(&spec, "private-key");
        assert!(rendered.contains("PrivateKey = private-key"));
        assert!(rendered.contains("AllowedIPs = 10.255.0.2/32, 10.64.1.0/24"));
        assert!(rendered.contains("PersistentKeepalive = 25"));
    }

    #[test]
    fn parses_project_dns_records() {
        let payload = serde_json::json!({
            "interface": {"name": "wg0", "address": "10.255.0.1", "listen_port": 51820},
            "peers": [],
            "projects": [{
                "project_id": "p1",
                "network_name": "driftbase-pn-p1",
                "node_subnet": "10.64.1.0/24",
                "gateway_ip": "10.64.1.1",
                "dns_ip": "10.64.1.2",
                "domain": "driftbase.internal",
                "dns_container_name": "driftbase-dns-p1",
                "records": [{"fqdn": "api.driftbase.internal", "ip": "10.64.1.10"}]
            }]
        });
        let spec: SyncSpec = serde_json::from_value(payload).unwrap();
        assert_eq!(spec.projects[0].records[0].fqdn, "api.driftbase.internal");
    }
}
