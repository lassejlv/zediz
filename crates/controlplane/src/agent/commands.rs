use anyhow::Result;
use chrono::{DateTime, Utc};
use driftbase_common::Id;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

/// Kinds of commands enqueued for an agent to execute.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    PullAndRun,
    Stop,
    Restart,
    Remove,
    Drain,
    Prune,
    UpdateRoutes,
    Build,
    SyncPrivateNetwork,
    UpdateAgent,
}

impl CommandKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CommandKind::PullAndRun => "pull_and_run",
            CommandKind::Stop => "stop",
            CommandKind::Restart => "restart",
            CommandKind::Remove => "remove",
            CommandKind::Drain => "drain",
            CommandKind::Prune => "prune",
            CommandKind::UpdateRoutes => "update_routes",
            CommandKind::Build => "build",
            CommandKind::SyncPrivateNetwork => "sync_private_network",
            CommandKind::UpdateAgent => "update_agent",
        }
    }
}

/// Row shape visible to the agent over the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommand {
    pub id: String,
    pub deployment_id: Option<String>,
    pub kind: String,
    pub payload: JsonValue,
    pub created_at: DateTime<Utc>,
}

pub async fn enqueue(
    pool: &DatabaseConnection,
    node_id: &str,
    deployment_id: Option<&str>,
    kind: CommandKind,
    payload: JsonValue,
) -> Result<Id> {
    let id = Id::new();
    crate::db::query(
        "INSERT INTO agent_commands (id, node_id, deployment_id, kind, payload) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id.to_string())
    .bind(node_id)
    .bind(deployment_id)
    .bind(kind.as_str())
    .bind(payload)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Returns the up-to-`limit` pending commands for a node and marks them `dispatched`.
pub async fn claim_for_node(
    pool: &DatabaseConnection,
    node_id: &str,
    limit: i64,
) -> Result<Vec<AgentCommand>> {
    #[derive(sea_orm::FromQueryResult)]
    struct Row {
        id: String,
        deployment_id: Option<String>,
        kind: String,
        payload: JsonValue,
        created_at: DateTime<Utc>,
    }
    let rows: Vec<Row> = crate::db::query_as(
        "UPDATE agent_commands SET status = 'dispatched', dispatched_at = now() \
         WHERE id IN ( \
             SELECT id FROM agent_commands \
             WHERE node_id = $1 AND status = 'pending' \
             ORDER BY created_at ASC LIMIT $2 FOR UPDATE SKIP LOCKED \
         ) \
         RETURNING id, deployment_id, kind, payload, created_at",
    )
    .bind(node_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AgentCommand {
            id: r.id,
            deployment_id: r.deployment_id,
            kind: r.kind,
            payload: r.payload,
            created_at: r.created_at,
        })
        .collect())
}

pub async fn mark_acked(
    pool: &DatabaseConnection,
    command_id: &str,
    ok: bool,
    result: Option<&str>,
) -> Result<()> {
    let status = if ok { "acked" } else { "errored" };
    crate::db::query(
        "UPDATE agent_commands SET status = $1, result = $2, acked_at = now() \
         WHERE id = $3",
    )
    .bind(status)
    .bind(result)
    .bind(command_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub struct VolumeMount<'a> {
    /// `/dev/disk/by-id/scsi-0HC_Volume_<hetzner_volume_id>` — the agent
    /// mounts this block device at `host_path` before starting the
    /// container.
    pub device_path: &'a str,
    /// Deterministic path on the node where the block device gets
    /// mounted. Conventionally `/var/lib/driftbase/volumes/<volume_id>`.
    pub host_path: &'a str,
    /// Where the container sees the volume.
    pub container_path: &'a str,
}

pub struct PrivateNetwork<'a> {
    pub network_name: &'a str,
    pub ip_address: &'a str,
    pub dns_ip: &'a str,
    pub aliases: &'a [String],
}

pub struct PullAndRunPayload<'a> {
    pub image: &'a str,
    pub env: &'a serde_json::Value,
    pub ports: &'a serde_json::Value,
    pub cpu_millis: u32,
    pub memory_mb: u32,
    pub registry: Option<&'a RegistryAuth<'a>>,
    pub volume: Option<&'a VolumeMount<'a>>,
    pub private_network: Option<&'a PrivateNetwork<'a>>,
}

pub fn pull_and_run_payload(p: &PullAndRunPayload<'_>) -> JsonValue {
    let registry = p
        .registry
        .map(|r| json!({ "url": r.url, "username": r.username, "password": r.password }));
    let volume = p.volume.map(|v| {
        json!({
            "device_path": v.device_path,
            "host_path": v.host_path,
            "container_path": v.container_path,
        })
    });
    let private_network = p.private_network.map(|n| {
        json!({
            "network_name": n.network_name,
            "ip_address": n.ip_address,
            "dns_ip": n.dns_ip,
            "aliases": n.aliases,
        })
    });
    json!({
        "image": p.image,
        "env": p.env,
        "ports": p.ports,
        "cpu_millis": p.cpu_millis,
        "memory_mb": p.memory_mb,
        "registry": registry,
        "volume": volume,
        "private_network": private_network,
    })
}

pub struct BuildPayload<'a> {
    pub build_id: &'a str,
    pub deployment_id: &'a str,
    pub service_id: &'a str,
    pub git_repo: &'a str,
    pub git_branch: &'a str,
    pub builder: &'a str,
    /// Only meaningful when `builder == "dockerfile"`.
    pub dockerfile_path: Option<&'a str>,
    pub root_dir: &'a str,
    pub image_tag: &'a str,
    pub github_pat: Option<&'a str>,
    pub registry: Option<RegistryAuth<'a>>,
}

pub struct RegistryAuth<'a> {
    pub url: &'a str,
    pub username: &'a str,
    pub password: &'a str,
}

pub fn build_payload(p: &BuildPayload<'_>) -> JsonValue {
    let registry = p
        .registry
        .as_ref()
        .map(|r| json!({ "url": r.url, "username": r.username, "password": r.password }));
    json!({
        "build_id": p.build_id,
        "deployment_id": p.deployment_id,
        "service_id": p.service_id,
        "git_repo": p.git_repo,
        "git_branch": p.git_branch,
        "builder": p.builder,
        "dockerfile_path": p.dockerfile_path,
        "root_dir": p.root_dir,
        "image_tag": p.image_tag,
        "github_pat": p.github_pat,
        "registry": registry,
    })
}
