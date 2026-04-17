use anyhow::Result;
use std::collections::HashMap;
use std::time::Duration;

use crate::client::{CommandAck, ControlPlaneClient, HeartbeatBody, StatusBody};
use crate::docker::{DockerExec, PortSpec, RegistryAuth, RunSpec};

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
    ) -> Self {
        Self {
            client,
            docker,
            node_token,
            node_id,
            log_cursors: HashMap::new(),
            tracked: std::collections::HashSet::new(),
        }
    }

    pub async fn tick(&mut self) -> Result<()> {
        let body = HeartbeatBody::default();
        let mut resp = self.client.heartbeat(&self.node_token, &body).await?;
        resp.commands.sort_by_key(|cmd| cmd.created_at);

        let mut acks: Vec<CommandAck> = Vec::new();
        for cmd in resp.commands {
            let ack = self.execute(cmd).await;
            acks.push(ack);
        }

        // Reconcile tracked deployments against Docker so a missed status
        // POST (network blip, CP restart) doesn't leave a live container
        // stuck in `pulling` on the CP forever.
        self.reconcile_tracked().await;

        // Ship log chunks for tracked deployments.
        let tracked_ids: Vec<String> = self.tracked.iter().cloned().collect();
        for deployment_id in tracked_ids {
            if let Err(e) = self.drain_and_push_logs(&deployment_id).await {
                tracing::warn!(deployment = %deployment_id, error = ?e, "log scrape failed");
            }
        }

        if !acks.is_empty() {
            let body = HeartbeatBody {
                acks,
                ..Default::default()
            };
            let _ = self.client.heartbeat(&self.node_token, &body).await;
        }
        Ok(())
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
            // `zediz` network so Caddy can dial it by name.
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

    Ok(RunSpec {
        deployment_id: deployment_id.into(),
        image,
        env,
        ports,
        cpu_millis,
        memory_mb,
        registry,
    })
}
