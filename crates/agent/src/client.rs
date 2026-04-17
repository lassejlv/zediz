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
                .user_agent(concat!("zediz-agent/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
            base: base.trim_end_matches('/').to_string(),
        }
    }

    pub async fn register(
        &self,
        bootstrap_token: &str,
        hostname: &str,
        cpu: i32,
        mem: i32,
        disk: i32,
    ) -> Result<RegisterResponse> {
        let body = serde_json::json!({
            "bootstrap_token": bootstrap_token,
            "hostname": hostname,
            "agent_version": env!("CARGO_PKG_VERSION"),
            "total_cpu_millis": cpu,
            "total_memory_mb": mem,
            "total_disk_mb": disk,
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
    pub acks: Vec<CommandAck>,
}

#[derive(Debug, Serialize)]
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
