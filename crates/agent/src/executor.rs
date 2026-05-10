use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::time::Duration;

use crate::client::{
    CommandAck, ContainerMetricSample, ControlPlaneClient, HeartbeatBody, StatusBody,
};
use crate::docker::{DockerExec, PortSpec, PrivateNetworkSpec, RegistryAuth, RunSpec, VolumeMount};

/// Upper bound on a single `pull_and_run` so a hung Docker daemon or
/// unreachable registry can't leave the deployment stuck forever. The
/// scheduler has a longer reaper (15 minutes) behind this as a safety net.
const PULL_AND_RUN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub struct Executor {
    client: ControlPlaneClient,
    docker: DockerExec,
    node_token: String,
    #[allow(dead_code)]
    node_id: String,
    private_network: Option<crate::private_network::Identity>,
    agent_update: crate::self_update::AgentUpdateState,
    pending_acks: Vec<CommandAck>,
    restart_after_ack: bool,
    /// deployment_id → last-seen docker log timestamp (unix seconds).
    log_cursors: HashMap<String, i64>,
    /// Deployment ids currently running on this node — we scrape their logs.
    tracked: std::collections::HashSet<String>,
}

impl Executor {
    pub fn new(
        client: ControlPlaneClient,
        docker: DockerExec,
        node_token: String,
        node_id: String,
        private_network: Option<crate::private_network::Identity>,
        agent_update: crate::self_update::AgentUpdateState,
    ) -> Self {
        Self {
            client,
            docker,
            node_token,
            node_id,
            private_network,
            agent_update,
            pending_acks: Vec::new(),
            restart_after_ack: false,
            log_cursors: HashMap::new(),
            tracked: std::collections::HashSet::new(),
        }
    }

    pub async fn tick(&mut self) -> Result<()> {
        let initial_acks = std::mem::take(&mut self.pending_acks);
        let exit_after_initial_ack = self.restart_after_ack && !initial_acks.is_empty();
        let body = self.heartbeat_body(initial_acks.clone(), Vec::new());
        let mut resp = match self.client.heartbeat(&self.node_token, &body).await {
            Ok(resp) => {
                if exit_after_initial_ack {
                    exit_for_update();
                }
                resp
            }
            Err(e) => {
                self.pending_acks.extend(initial_acks);
                return Err(e);
            }
        };
        resp.commands.sort_by_key(|cmd| cmd.created_at);
        let mut metrics_sent = false;
        let mut rounds = 0usize;
        loop {
            let acks = self.execute_commands(resp.commands).await;
            let exit_after_ack = self.restart_after_ack && !acks.is_empty();

            // Reconcile tracked deployments against Docker so a missed status
            // POST (network blip, CP restart) doesn't leave a live container
            // stuck in `pulling` on the CP forever.
            self.reconcile_tracked().await;

            // Sample container stats once per tick. If the follow-up heartbeat
            // returns newly-enqueued commands, we still need to execute them
            // instead of dropping them after the CP marked them `dispatched`.
            let metrics = if metrics_sent {
                Vec::new()
            } else {
                metrics_sent = true;
                self.sample_container_metrics().await
            };

            // Ship log chunks for tracked deployments.
            let tracked_ids: Vec<String> = self.tracked.iter().cloned().collect();
            for deployment_id in tracked_ids {
                if let Err(e) = self.drain_and_push_logs(&deployment_id).await {
                    tracing::warn!(deployment = %deployment_id, error = ?e, "log scrape failed");
                }
            }

            if acks.is_empty() && metrics.is_empty() {
                break;
            }

            let body = self.heartbeat_body(acks, metrics);
            match self.client.heartbeat(&self.node_token, &body).await {
                Ok(mut next) => {
                    if exit_after_ack {
                        exit_for_update();
                    }
                    next.commands.sort_by_key(|cmd| cmd.created_at);
                    if next.commands.is_empty() {
                        break;
                    }
                    resp = next;
                }
                Err(e) => {
                    self.pending_acks.extend(body.acks);
                    tracing::warn!(error = ?e, "follow-up heartbeat failed");
                    break;
                }
            }

            rounds += 1;
            if rounds >= 8 {
                tracing::warn!("stopping command drain after 8 follow-up heartbeat rounds");
                break;
            }
        }
        Ok(())
    }

    fn heartbeat_body(
        &self,
        acks: Vec<CommandAck>,
        container_metrics: Vec<ContainerMetricSample>,
    ) -> HeartbeatBody {
        let mut body = HeartbeatBody {
            acks,
            container_metrics,
            ..Default::default()
        };
        body.agent_version = Some(env!("CARGO_PKG_VERSION").to_string());
        body.agent_image_ref = self.agent_update.image_ref.clone();
        body.agent_image_digest = self.agent_update.image_digest.clone();
        body.agent_self_update_capable = Some(self.agent_update.self_update_capable);
        if let Some(identity) = self.private_network.as_ref() {
            body.private_network_capable = Some(true);
            body.wireguard_public_key = Some(identity.public_key.clone());
            body.wireguard_listen_port = Some(identity.listen_port);
        }
        body
    }

    async fn execute_commands(&mut self, commands: Vec<crate::client::Command>) -> Vec<CommandAck> {
        let mut acks = Vec::with_capacity(commands.len());
        for cmd in commands {
            acks.push(self.execute(cmd).await);
        }
        acks
    }

    async fn sample_container_metrics(&self) -> Vec<ContainerMetricSample> {
        let tracked: Vec<String> = self.tracked.iter().cloned().collect();
        if tracked.is_empty() {
            return Vec::new();
        }
        let docker = self.docker.clone();
        let futures = tracked.into_iter().map(|id| {
            let d = docker.clone();
            async move {
                let s = d.sample_stats(&id).await;
                (id, s)
            }
        });
        let results = futures::future::join_all(futures).await;

        let mut out = Vec::with_capacity(results.len());
        for (deployment_id, sample) in results {
            match sample {
                Ok(Some(s)) => out.push(ContainerMetricSample {
                    deployment_id,
                    ts: Utc::now(),
                    cpu_percent: s.cpu_percent,
                    memory_bytes: s.memory_bytes,
                    memory_limit_bytes: s.memory_limit_bytes,
                    rx_bytes: s.rx_bytes,
                    tx_bytes: s.tx_bytes,
                }),
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(
                        deployment = %deployment_id,
                        error = ?e,
                        "sample_stats",
                    );
                }
            }
        }
        out
    }

    /// For each deployment we think is live, check Docker's actual state
    /// and tell the control plane. Running containers get a fresh
    /// `running` report; exited or missing ones get `stopped` and are
    /// dropped from tracked so we stop scraping their logs.
    async fn reconcile_tracked(&mut self) {
        let snapshot: Vec<String> = self.tracked.iter().cloned().collect();
        for deployment_id in snapshot {
            match self.docker.running_container_id(&deployment_id).await {
                Ok(Some(container_id)) => {
                    if let Err(e) = self
                        .client
                        .report_status(
                            &self.node_token,
                            &deployment_id,
                            &StatusBody {
                                status: "running".into(),
                                container_id: Some(container_id),
                                reason: None,
                            },
                        )
                        .await
                    {
                        tracing::warn!(
                            deployment = %deployment_id,
                            error = ?e,
                            "reconcile report_status running",
                        );
                    }
                }
                Ok(None) => {
                    if let Err(e) = self
                        .client
                        .report_status(
                            &self.node_token,
                            &deployment_id,
                            &StatusBody {
                                status: "stopped".into(),
                                container_id: None,
                                reason: None,
                            },
                        )
                        .await
                    {
                        tracing::warn!(
                            deployment = %deployment_id,
                            error = ?e,
                            "reconcile report_status stopped",
                        );
                    }
                    self.tracked.remove(&deployment_id);
                    self.log_cursors.remove(&deployment_id);
                }
                Err(e) => {
                    tracing::warn!(
                        deployment = %deployment_id,
                        error = ?e,
                        "reconcile docker inspect",
                    );
                }
            }
        }
    }

    async fn execute(&mut self, cmd: crate::client::Command) -> CommandAck {
        let id = cmd.id.clone();
        match cmd.kind.as_str() {
            "pull_and_run" => match self.handle_pull_and_run(cmd).await {
                Ok(()) => ok(id),
                Err(e) => err(id, e.to_string()),
            },
            "stop" => match self.handle_stop(cmd).await {
                Ok(()) => ok(id),
                Err(e) => err(id, e.to_string()),
            },
            "remove" | "restart" => match self.handle_remove(cmd).await {
                Ok(()) => ok(id),
                Err(e) => err(id, e.to_string()),
            },
            "update_routes" => match self.handle_update_routes(cmd).await {
                Ok(()) => ok(id),
                Err(e) => err(id, e.to_string()),
            },
            "sync_private_network" => match self.handle_sync_private_network(cmd).await {
                Ok(()) => ok(id),
                Err(e) => err(id, e.to_string()),
            },
            "update_agent" => match self.handle_update_agent(cmd).await {
                Ok(()) => ok(id),
                Err(e) => err(id, e.to_string()),
            },
            "build" => match self.handle_build(cmd).await {
                Ok(()) => ok(id),
                Err(e) => err(id, e.to_string()),
            },
            other => err(id, format!("unsupported command kind: {other}")),
        }
    }

    async fn handle_build(&mut self, cmd: crate::client::Command) -> anyhow::Result<()> {
        let spec: crate::build::BuildSpec = serde_json::from_value(cmd.payload)
            .map_err(|e| anyhow::anyhow!("bad build payload: {e}"))?;
        crate::build::run_build(&self.client, &self.node_token, spec).await
    }

    async fn handle_sync_private_network(&mut self, cmd: crate::client::Command) -> Result<()> {
        crate::private_network::sync(self.docker.inner(), cmd.payload).await
    }

    async fn handle_update_agent(&mut self, cmd: crate::client::Command) -> Result<()> {
        let payload: crate::self_update::UpdateAgentPayload =
            serde_json::from_value(cmd.payload)
                .map_err(|e| anyhow::anyhow!("bad update_agent payload: {e}"))?;
        crate::self_update::prepare_update(&self.node_token, &payload).await?;
        self.restart_after_ack = true;
        Ok(())
    }

    async fn handle_update_routes(&mut self, cmd: crate::client::Command) -> Result<()> {
        let raw = cmd
            .payload
            .get("routes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let docker = self.docker.inner();
        crate::caddy::ensure_running(&docker).await?;

        let mut routes = Vec::with_capacity(raw.len());
        for item in &raw {
            let hostname = item
                .get("hostname")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("route missing hostname"))?
                .to_string();
            let container_port = item
                .get("container_port")
                .and_then(|v| v.as_u64())
                .and_then(|n| n.try_into().ok())
                .unwrap_or(80u16);
            let container_name = item
                .get("container_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("route missing container_name"))?
                .to_string();

            // Make sure the target container is attached to the shared
            // `driftbase` network so Caddy can dial it by name.
            crate::caddy::ensure_container_on_network(&docker, &container_name).await?;

            routes.push(crate::caddy::Route {
                hostname,
                container_port,
                container_name,
            });
        }

        crate::caddy::apply_routes(&routes).await?;
        Ok(())
    }

    async fn handle_pull_and_run(&mut self, cmd: crate::client::Command) -> Result<()> {
        let deployment_id = cmd
            .deployment_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("pull_and_run missing deployment_id"))?;

        // Tell the CP we're starting. These intermediate status reports are
        // best-effort — if the POST fails we still attempt the pull, and the
        // final `running` report (or the errored path below) brings the
        // deployment row back in sync. Log so silent POST failures don't
        // leave us debugging in the dark.
        if let Err(e) = self
            .client
            .report_status(
                &self.node_token,
                &deployment_id,
                &StatusBody {
                    status: "pulling".into(),
                    container_id: None,
                    reason: None,
                },
            )
            .await
        {
            tracing::warn!(deployment = %deployment_id, error = ?e, "report_status pulling");
        }

        let spec = parse_run_spec(&deployment_id, &cmd.payload)?;
        if let Err(e) = self
            .client
            .report_status(
                &self.node_token,
                &deployment_id,
                &StatusBody {
                    status: "starting".into(),
                    container_id: None,
                    reason: None,
                },
            )
            .await
        {
            tracing::warn!(deployment = %deployment_id, error = ?e, "report_status starting");
        }

        let pull_result =
            tokio::time::timeout(PULL_AND_RUN_TIMEOUT, self.docker.pull_and_run(spec)).await;
        let container_id = match pull_result {
            Ok(Ok(id)) => id,
            Ok(Err(e)) => {
                // Make the failure visible on the deployment row, not just on
                // the agent_commands ack that the UI never shows.
                if let Err(post_err) = self
                    .client
                    .report_status(
                        &self.node_token,
                        &deployment_id,
                        &StatusBody {
                            status: "errored".into(),
                            container_id: None,
                            reason: Some(e.to_string()),
                        },
                    )
                    .await
                {
                    tracing::warn!(
                        deployment = %deployment_id,
                        error = ?post_err,
                        "report_status errored",
                    );
                }
                return Err(e);
            }
            Err(_elapsed) => {
                let msg = format!(
                    "pull_and_run timed out after {}s",
                    PULL_AND_RUN_TIMEOUT.as_secs()
                );
                if let Err(post_err) = self
                    .client
                    .report_status(
                        &self.node_token,
                        &deployment_id,
                        &StatusBody {
                            status: "errored".into(),
                            container_id: None,
                            reason: Some(msg.clone()),
                        },
                    )
                    .await
                {
                    tracing::warn!(
                        deployment = %deployment_id,
                        error = ?post_err,
                        "report_status errored after timeout",
                    );
                }
                return Err(anyhow::anyhow!(msg));
            }
        };

        self.tracked.insert(deployment_id.clone());
        self.log_cursors.insert(deployment_id.clone(), 0);

        self.client
            .report_status(
                &self.node_token,
                &deployment_id,
                &StatusBody {
                    status: "running".into(),
                    container_id: Some(container_id),
                    reason: None,
                },
            )
            .await?;
        Ok(())
    }

    async fn handle_stop(&mut self, cmd: crate::client::Command) -> Result<()> {
        let deployment_id = cmd
            .deployment_id
            .ok_or_else(|| anyhow::anyhow!("stop missing deployment_id"))?;
        self.docker.stop_by_deployment(&deployment_id).await?;
        self.tracked.remove(&deployment_id);
        self.client
            .report_status(
                &self.node_token,
                &deployment_id,
                &StatusBody {
                    status: "stopped".into(),
                    container_id: None,
                    reason: None,
                },
            )
            .await?;
        Ok(())
    }

    async fn handle_remove(&mut self, cmd: crate::client::Command) -> Result<()> {
        let deployment_id = cmd
            .deployment_id
            .ok_or_else(|| anyhow::anyhow!("remove missing deployment_id"))?;
        let _ = self.docker.stop_by_deployment(&deployment_id).await;
        self.docker.remove_by_deployment(&deployment_id).await?;
        self.tracked.remove(&deployment_id);
        Ok(())
    }

    async fn drain_and_push_logs(&mut self, deployment_id: &str) -> Result<()> {
        let cursor = *self.log_cursors.get(deployment_id).unwrap_or(&0);
        let (lines, new_cursor) = self.docker.drain_logs(deployment_id, cursor).await?;
        if lines.is_empty() {
            return Ok(());
        }
        self.client
            .push_logs(&self.node_token, deployment_id, lines)
            .await?;
        self.log_cursors.insert(deployment_id.into(), new_cursor);
        Ok(())
    }
}

fn ok(id: String) -> CommandAck {
    CommandAck {
        command_id: id,
        ok: true,
        message: None,
    }
}

fn err(id: String, msg: String) -> CommandAck {
    CommandAck {
        command_id: id,
        ok: false,
        message: Some(msg),
    }
}

fn exit_for_update() -> ! {
    tracing::info!("agent update acknowledged; exiting for systemd restart");
    std::process::exit(0);
}

fn parse_run_spec(deployment_id: &str, payload: &serde_json::Value) -> Result<RunSpec> {
    let image = payload
        .get("image")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing image"))?
        .to_string();

    let env: std::collections::BTreeMap<String, String> = payload
        .get("env")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let ports: Vec<PortSpec> = payload
        .get("ports")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let container_port = item.get("container_port")?.as_u64()?.try_into().ok()?;
                    let host_port = item
                        .get("host_port")
                        .and_then(|v| v.as_u64())
                        .and_then(|n| n.try_into().ok());
                    let protocol = item
                        .get("protocol")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    Some(PortSpec {
                        container_port,
                        host_port,
                        protocol,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let cpu_millis = payload
        .get("cpu_millis")
        .and_then(|v| v.as_u64())
        .unwrap_or(500) as u32;
    let memory_mb = payload
        .get("memory_mb")
        .and_then(|v| v.as_u64())
        .unwrap_or(256) as u32;

    let registry: Option<RegistryAuth> = payload
        .get("registry")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok());

    let volume = payload.get("volume").and_then(|v| {
        let device_path = v.get("device_path")?.as_str()?.to_string();
        let host_path = v.get("host_path")?.as_str()?.to_string();
        let container_path = v.get("container_path")?.as_str()?.to_string();
        Some(VolumeMount {
            device_path,
            host_path,
            container_path,
        })
    });

    let private_network = payload.get("private_network").and_then(|v| {
        let network_name = v.get("network_name")?.as_str()?.to_string();
        let ip_address = v.get("ip_address")?.as_str()?.to_string();
        let dns_ip = v.get("dns_ip")?.as_str()?.to_string();
        let aliases = v
            .get("aliases")
            .and_then(|aliases| aliases.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Some(PrivateNetworkSpec {
            network_name,
            ip_address,
            dns_ip,
            aliases,
        })
    });

    Ok(RunSpec {
        deployment_id: deployment_id.into(),
        image,
        env,
        ports,
        cpu_millis,
        memory_mb,
        registry,
        volume,
        private_network,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_private_network_payload() {
        let spec = parse_run_spec(
            "dep1",
            &serde_json::json!({
                "image": "nginx:latest",
                "env": {},
                "ports": [],
                "cpu_millis": 500,
                "memory_mb": 256,
                "private_network": {
                    "network_name": "driftbase-pn-project",
                    "ip_address": "10.64.1.10",
                    "dns_ip": "10.64.1.2",
                    "aliases": ["api", "api.driftbase.internal"]
                }
            }),
        )
        .unwrap();
        let private = spec.private_network.unwrap();
        assert_eq!(private.network_name, "driftbase-pn-project");
        assert_eq!(private.ip_address, "10.64.1.10");
        assert_eq!(private.aliases[1], "api.driftbase.internal");
    }

    #[test]
    fn old_payloads_without_private_network_still_parse() {
        let spec = parse_run_spec(
            "dep1",
            &serde_json::json!({
                "image": "nginx:latest",
                "env": {},
                "ports": [],
                "cpu_millis": 500,
                "memory_mb": 256
            }),
        )
        .unwrap();
        assert!(spec.private_network.is_none());
    }
}
