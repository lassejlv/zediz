use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use driftbase_common::Id;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentMessage {
    Heartbeat(Heartbeat),
    DeploymentStatus(DeploymentStatusUpdate),
    LogChunk(LogChunk),
    Ack {
        command_id: Id,
        result: CommandResult,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlCommand {
    PullAndRun(PullAndRun),
    Stop {
        command_id: Id,
        deployment_id: Id,
    },
    Restart {
        command_id: Id,
        deployment_id: Id,
    },
    AttachVolume {
        command_id: Id,
        volume_id: Id,
        mount_path: String,
        deployment_id: Id,
    },
    ReloadProxy {
        command_id: Id,
    },
    Drain {
        command_id: Id,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub cpu_used_millis: u32,
    pub memory_used_mb: u32,
    pub disk_used_mb: u32,
    pub load_avg_1m: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentStatusUpdate {
    pub deployment_id: Id,
    pub status: DeploymentStatus,
    pub container_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Pending,
    Pulling,
    Starting,
    Running,
    Failing,
    Stopped,
    Errored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogChunk {
    pub deployment_id: Id,
    pub stream: LogStream,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub line: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullAndRun {
    pub command_id: Id,
    pub deployment_id: Id,
    pub image: String,
    pub digest: Option<String>,
    pub env: BTreeMap<String, String>,
    pub ports: Vec<PortMap>,
    pub resources: Resources,
    pub mounts: Vec<Mount>,
    pub registry_auth: Option<RegistryAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMap {
    pub container_port: u16,
    pub host_port: Option<u16>,
    pub protocol: Protocol,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resources {
    pub cpu_millis: u32,
    pub memory_mb: u32,
    pub disk_mb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mount {
    pub host_path: String,
    pub container_path: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryAuth {
    pub username: String,
    pub password: String,
    pub server: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandResult {
    Ok,
    Err { message: String },
}
