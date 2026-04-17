use anyhow::Result;
use std::collections::HashMap;

use crate::client::{CommandAck, ControlPlaneClient, HeartbeatBody, StatusBody};
use crate::docker::{DockerExec, PortSpec, RunSpec};

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
        let resp = self.client.heartbeat(&self.node_token, &body).await?;

        let mut acks: Vec<CommandAck> = Vec::new();
        for cmd in resp.commands {
            let ack = self.execute(cmd).await;
            acks.push(ack);
        }

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

        // Tell the CP we're starting.
        let _ = self
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
            .await;

        let spec = parse_run_spec(&deployment_id, &cmd.payload)?;
        let _ = self
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
            .await;

        let container_id = match self.docker.pull_and_run(spec).await {
            Ok(id) => id,
            Err(e) => {
                // Make the failure visible on the deployment row, not just on
                // the agent_commands ack that the UI never shows.
                let _ = self
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
                    .await;
                return Err(e);
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

    Ok(RunSpec {
        deployment_id: deployment_id.into(),
        image,
        env,
        ports,
        cpu_millis,
        memory_mb,
    })
}
