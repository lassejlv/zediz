use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::time::Duration;

#[derive(Clone)]
pub struct ControlPlaneClient {
    http: Client,
    base: String,
}

impl ControlPlaneClient {
    pub fn new(base: &str) -> Self {
        Self {
            http: Client::builder()
                .user_agent(concat!("driftbase-agent/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
            base: base.trim_end_matches('/').to_string(),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub async fn register(&self, input: RegisterInput<'_>) -> Result<RegisterResponse> {
        let body = serde_json::json!({
            "bootstrap_token": input.bootstrap_token,
            "hostname": input.hostname,
            "agent_version": env!("CARGO_PKG_VERSION"),
            "agent_image_ref": input.agent_update.image_ref.as_deref(),
            "agent_image_digest": input.agent_update.image_digest.as_deref(),
            "agent_self_update_capable": input.agent_update.self_update_capable,
            "total_cpu_millis": input.cpu,
            "total_memory_mb": input.mem,
            "total_disk_mb": input.disk,
            "private_network_capable": input.private_network.is_some(),
            "wireguard_public_key": input.private_network.map(|id| id.public_key.as_str()),
            "wireguard_listen_port": input.private_network.map(|id| id.listen_port),
        });
        let res = self
            .http
            .post(format!("{}/api/v1/agent/register", self.base))
            .json(&body)
            .send()
            .await?;
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("register: {status}: {text}"));
        }
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn heartbeat(
        &self,
        node_token: &str,
        body: &HeartbeatBody,
    ) -> Result<HeartbeatResponse> {
        let res = self
            .http
            .post(format!("{}/api/v1/agent/heartbeat", self.base))
            .bearer_auth(node_token)
            .json(body)
            .send()
            .await?;
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("heartbeat: {status}: {text}"));
        }
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn report_status(
        &self,
        node_token: &str,
        deployment_id: &str,
        body: &StatusBody,
    ) -> Result<()> {
        let res = self
            .http
            .post(format!(
                "{}/api/v1/agent/deployments/{deployment_id}/status",
                self.base
            ))
            .bearer_auth(node_token)
            .json(body)
            .send()
            .await?;
        if !res.status().is_success() && res.status() != StatusCode::NOT_FOUND {
            let s = res.status();
            let t = res.text().await.unwrap_or_default();
            return Err(anyhow!("status: {s}: {t}"));
        }
        Ok(())
    }

    pub async fn push_logs(
        &self,
        node_token: &str,
        deployment_id: &str,
        lines: Vec<LogLineOut>,
    ) -> Result<()> {
        if lines.is_empty() {
            return Ok(());
        }
        let res = self
            .http
            .post(format!(
                "{}/api/v1/agent/deployments/{deployment_id}/logs",
                self.base
            ))
            .bearer_auth(node_token)
            .json(&serde_json::json!({ "lines": lines }))
            .send()
            .await?;
        if !res.status().is_success() {
            let s = res.status();
            let t = res.text().await.unwrap_or_default();
            return Err(anyhow!("push_logs: {s}: {t}"));
        }
        Ok(())
    }

    pub async fn report_build_status(
        &self,
        node_token: &str,
        build_id: &str,
        body: &BuildStatusBody,
    ) -> Result<()> {
        let res = self
            .http
            .post(format!(
                "{}/api/v1/agent/builds/{build_id}/status",
                self.base
            ))
            .bearer_auth(node_token)
            .json(body)
            .send()
            .await?;
        if !res.status().is_success() && res.status() != StatusCode::NOT_FOUND {
            let s = res.status();
            let t = res.text().await.unwrap_or_default();
            return Err(anyhow!("build status: {s}: {t}"));
        }
        Ok(())
    }
}

pub struct RegisterInput<'a> {
    pub bootstrap_token: &'a str,
    pub hostname: &'a str,
    pub cpu: i32,
    pub mem: i32,
    pub disk: i32,
    pub private_network: Option<&'a crate::private_network::Identity>,
    pub agent_update: &'a crate::self_update::AgentUpdateState,
}

#[derive(Debug, Deserialize)]
pub struct RegisterResponse {
    pub node_id: String,
    pub node_token: String,
}

#[derive(Debug, Serialize, Default)]
pub struct HeartbeatBody {
    pub cpu_used_millis: Option<i32>,
    pub memory_used_mb: Option<i32>,
    pub disk_used_mb: Option<i32>,
    pub load_avg_1m: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_image_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_image_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_self_update_capable: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acks: Vec<CommandAck>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub container_metrics: Vec<ContainerMetricSample>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_network_capable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wireguard_public_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wireguard_listen_port: Option<i32>,
}

/// One live snapshot of a running container's resource usage. Agent
/// reports these alongside heartbeat acks; the CP overwrites the
/// matching `deployments.runtime_metrics` JSONB row.
#[derive(Debug, Serialize)]
pub struct ContainerMetricSample {
    pub deployment_id: String,
    pub ts: DateTime<Utc>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_limit_bytes: Option<u64>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandAck {
    pub command_id: String,
    pub ok: bool,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HeartbeatResponse {
    pub commands: Vec<Command>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Command {
    pub id: String,
    #[serde(default)]
    pub deployment_id: Option<String>,
    pub kind: String,
    pub payload: JsonValue,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct StatusBody {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LogLineOut {
    pub stream: String,
    pub ts: DateTime<Utc>,
    pub line: String,
}

#[derive(Debug, Default, Serialize)]
pub struct BuildStatusBody {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_tag: Option<String>,
}
