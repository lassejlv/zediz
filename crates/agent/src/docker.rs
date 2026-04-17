use anyhow::{anyhow, Result};
use bollard::auth::DockerCredentials;
use bollard::container::{
    Config, CreateContainerOptions, LogOutput, LogsOptions, RemoveContainerOptions,
    StopContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::{HostConfig, PortBinding, RestartPolicy, RestartPolicyNameEnum};
use bollard::Docker;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;

use crate::client::LogLineOut;

/// Registry credentials carried inside build / pull-and-run command payloads.
/// Shared by `crates/agent/src/build.rs` (for `docker login` during push) and
/// `pull_and_run` (passed to bollard so the daemon pulls from the bundled
/// registry with auth).
#[derive(Debug, Clone, Deserialize)]
pub struct RegistryAuth {
    pub url: String,
    pub username: String,
    pub password: String,
}

impl RegistryAuth {
    /// Bollard's `DockerCredentials` uses `serveraddress` for the registry
    /// host. Our URL may have a scheme — strip it.
    pub fn to_bollard(&self) -> DockerCredentials {
        let host = self
            .url
            .trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        DockerCredentials {
            username: Some(self.username.clone()),
            password: Some(self.password.clone()),
            serveraddress: Some(host),
            ..Default::default()
        }
    }
}

const PREFIX: &str = "zediz-";

#[derive(Clone)]
pub struct DockerExec {
    docker: Docker,
}

impl DockerExec {
    pub fn connect() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    pub fn inner(&self) -> Docker {
        self.docker.clone()
    }

    pub fn container_name(deployment_id: &str) -> String {
        format!("{PREFIX}{deployment_id}")
    }

    pub async fn pull_and_run(&self, spec: RunSpec) -> Result<String> {
        let (from_image, tag) = split_image_tag(&spec.image);
        let credentials = spec.registry.as_ref().map(RegistryAuth::to_bollard);
        let mut stream = self.docker.create_image(
            Some(CreateImageOptions {
                from_image,
                tag,
                ..Default::default()
            }),
            None,
            credentials,
        );
        while let Some(event) = stream.next().await {
            if let Err(e) = event {
                let msg = e.to_string();
                if msg.contains("expected value at line 1 column 1") {
                    continue;
                }
                return Err(anyhow!("pulling {}: {e}", spec.image));
            }
        }

        let env_vec: Vec<String> = spec.env.iter().map(|(k, v)| format!("{k}={v}")).collect();

        let mut port_bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
        let mut exposed: HashMap<String, HashMap<(), ()>> = HashMap::new();
        for p in &spec.ports {
            let proto = p.protocol.clone().unwrap_or_else(|| "tcp".into());
            let key = format!("{}/{}", p.container_port, proto);
            exposed.insert(key.clone(), HashMap::new());
            if let Some(host) = p.host_port {
                port_bindings.insert(
                    key,
                    Some(vec![PortBinding {
                        host_ip: Some("0.0.0.0".into()),
                        host_port: Some(host.to_string()),
                    }]),
                );
            }
        }

        let mut labels = HashMap::new();
        labels.insert("zediz.deployment_id".into(), spec.deployment_id.clone());
        labels.insert("zediz.managed".into(), "true".into());

        let host_config = HostConfig {
            port_bindings: if port_bindings.is_empty() {
                None
            } else {
                Some(port_bindings)
            },
            memory: Some(i64::from(spec.memory_mb) * 1024 * 1024),
            nano_cpus: Some(i64::from(spec.cpu_millis) * 1_000_000),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                ..Default::default()
            }),
            // Join the shared `zediz` network so Caddy can reach this
            // container by name for domain routing.
            network_mode: Some(crate::caddy::NETWORK.into()),
            ..Default::default()
        };

        let config: Config<String> = Config {
            image: Some(spec.image.clone()),
            env: if env_vec.is_empty() {
                None
            } else {
                Some(env_vec)
            },
            exposed_ports: if exposed.is_empty() {
                None
            } else {
                Some(exposed)
            },
            labels: Some(labels),
            host_config: Some(host_config),
            ..Default::default()
        };

        let name = Self::container_name(&spec.deployment_id);

        let existing = self
            .docker
            .inspect_container(&name, None::<bollard::container::InspectContainerOptions>)
            .await;
        if existing.is_ok() {
            let _ = self
                .docker
                .remove_container(
                    &name,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
        }

        let created = self
            .docker
            .create_container(
                Some(CreateContainerOptions {
                    name: name.clone(),
                    platform: None,
                }),
                config,
            )
            .await?;

        self.docker
            .start_container::<String>(&created.id, None)
            .await?;

        Ok(created.id)
    }

    /// Inspect the zediz-managed container for a deployment. Returns
    /// `Some(container_id)` only if the container exists *and* is running.
    /// Missing, exited, or paused containers — and 404s from the daemon —
    /// all map to `Ok(None)` so callers can treat this as a predicate.
    pub async fn running_container_id(&self, deployment_id: &str) -> Result<Option<String>> {
        let name = Self::container_name(deployment_id);
        let res = self
            .docker
            .inspect_container(&name, None::<bollard::container::InspectContainerOptions>)
            .await;
        match res {
            Ok(inspect) => {
                let running = inspect
                    .state
                    .as_ref()
                    .and_then(|s| s.running)
                    .unwrap_or(false);
                if running {
                    Ok(inspect.id)
                } else {
                    Ok(None)
                }
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn stop_by_deployment(&self, deployment_id: &str) -> Result<()> {
        let name = Self::container_name(deployment_id);
        let res = self
            .docker
            .stop_container(&name, Some(StopContainerOptions { t: 10 }))
            .await;
        match res {
            Ok(()) => Ok(()),
            Err(bollard::errors::Error::DockerResponseServerError { status_code, .. })
                if status_code == 304 || status_code == 404 =>
            {
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn remove_by_deployment(&self, deployment_id: &str) -> Result<()> {
        let name = Self::container_name(deployment_id);
        let res = self
            .docker
            .remove_container(
                &name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;
        match res {
            Ok(()) => Ok(()),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Drain new log lines since `since` (RFC3339). Returns lines + new since.
    pub async fn drain_logs(
        &self,
        deployment_id: &str,
        since: i64,
    ) -> Result<(Vec<LogLineOut>, i64)> {
        let name = Self::container_name(deployment_id);
        let options = LogsOptions::<String> {
            follow: false,
            stdout: true,
            stderr: true,
            timestamps: true,
            since,
            tail: "all".into(),
            ..Default::default()
        };

        let mut stream = self.docker.logs(&name, Some(options));
        let mut out: Vec<LogLineOut> = Vec::new();
        let mut max_ts = since;
        while let Some(event) = stream.next().await {
            match event {
                Ok(LogOutput::StdOut { message }) => {
                    for line in parse_timestamped(&message, "stdout") {
                        max_ts = max_ts.max(line.ts.timestamp());
                        out.push(line);
                    }
                }
                Ok(LogOutput::StdErr { message }) => {
                    for line in parse_timestamped(&message, "stderr") {
                        max_ts = max_ts.max(line.ts.timestamp());
                        out.push(line);
                    }
                }
                Ok(_) => {}
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                }) => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok((out, max_ts))
    }
}

fn split_image_tag(full: &str) -> (&str, &str) {
    if let Some(at) = full.find('@') {
        return (&full[..at], &full[at + 1..]);
    }
    if let Some(colon) = full.rfind(':') {
        let after = &full[colon + 1..];
        if !after.contains('/') {
            return (&full[..colon], after);
        }
    }
    (full, "latest")
}

fn parse_timestamped(raw: &[u8], stream: &str) -> Vec<LogLineOut> {
    let text = String::from_utf8_lossy(raw);
    text.split_inclusive('\n')
        .filter_map(|chunk| {
            let chunk = chunk.trim_end_matches('\n');
            if chunk.is_empty() {
                return None;
            }
            let (ts_str, body) = match chunk.split_once(' ') {
                Some((t, b)) => (t, b),
                None => ("", chunk),
            };
            let ts = ts_str
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now());
            Some(LogLineOut {
                stream: stream.into(),
                ts,
                line: body.into(),
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct RunSpec {
    pub deployment_id: String,
    pub image: String,
    pub env: std::collections::BTreeMap<String, String>,
    pub ports: Vec<PortSpec>,
    pub cpu_millis: u32,
    pub memory_mb: u32,
    /// Private-registry auth for the pull. Only the bundled registry needs
    /// this today (external registries for image services are still public).
    pub registry: Option<RegistryAuth>,
}

#[derive(Debug, Clone)]
pub struct PortSpec {
    pub container_port: u16,
    pub host_port: Option<u16>,
    pub protocol: Option<String>,
}
