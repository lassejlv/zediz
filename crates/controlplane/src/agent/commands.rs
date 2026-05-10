use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use driftbase_common::Id;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use crate::crypto::MasterKey;

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
    CancelBuild,
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
            CommandKind::CancelBuild => "cancel_build",
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
    master_key: &MasterKey,
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
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let mut command = AgentCommand {
            id: r.id,
            deployment_id: r.deployment_id,
            kind: r.kind,
            payload: r.payload,
            created_at: r.created_at,
        };
        if let Err(e) = hydrate_command_payload(pool, master_key, &mut command).await {
            let result = format!("could not prepare command payload: {e:#}");
            tracing::warn!(
                command = %command.id,
                node = %node_id,
                error = ?e,
                "command payload hydration failed",
            );
            mark_acked(pool, node_id, &command.id, false, Some(&result)).await?;
            continue;
        }
        out.push(command);
    }
    Ok(out)
}

pub async fn mark_acked(
    pool: &DatabaseConnection,
    node_id: &str,
    command_id: &str,
    ok: bool,
    result: Option<&str>,
) -> Result<bool> {
    let status = if ok { "acked" } else { "errored" };
    let res = crate::db::query(
        "UPDATE agent_commands SET status = $1, result = $2, acked_at = now(), \
         payload = CASE \
             WHEN kind = 'build' THEN \
                 (CASE \
                     WHEN payload ? 'registry' AND jsonb_typeof(payload->'registry') = 'object' \
                     THEN jsonb_set(payload, '{registry}', (payload->'registry') - 'password', false) \
                     ELSE payload \
                  END) - 'github_pat' \
             WHEN kind = 'pull_and_run' THEN \
                 CASE \
                     WHEN payload ? 'registry' AND jsonb_typeof(payload->'registry') = 'object' \
                     THEN jsonb_set(payload, '{registry}', (payload->'registry') - 'password', false) \
                     ELSE payload \
                 END \
             ELSE payload \
         END \
         WHERE id = $3 AND node_id = $4",
    )
    .bind(status)
    .bind(result)
    .bind(command_id)
    .bind(node_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
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
        .map(|r| json!({ "url": r.url, "username": r.username, "credential_id": r.credential_id }));
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
    pub github_credential_id: Option<&'a str>,
    pub registry: Option<RegistryAuth<'a>>,
}

pub struct RegistryAuth<'a> {
    pub url: &'a str,
    pub username: &'a str,
    pub credential_id: &'a str,
}

pub fn build_payload(p: &BuildPayload<'_>) -> JsonValue {
    let registry = p
        .registry
        .as_ref()
        .map(|r| json!({ "url": r.url, "username": r.username, "credential_id": r.credential_id }));
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
        "github_credential_id": p.github_credential_id,
        "registry": registry,
    })
}

async fn hydrate_command_payload(
    pool: &DatabaseConnection,
    master_key: &MasterKey,
    command: &mut AgentCommand,
) -> Result<()> {
    if !matches!(command.kind.as_str(), "pull_and_run" | "build") {
        return Ok(());
    }

    let registry_credential_id = registry_credential_id(&command.payload);
    let github_credential_id = if command.kind == "build" {
        string_field(&command.payload, "github_credential_id")
    } else {
        None
    };
    if registry_credential_id.is_none() && github_credential_id.is_none() {
        return Ok(());
    }

    let deployment_id = command
        .deployment_id
        .as_deref()
        .or_else(|| {
            command
                .payload
                .get("deployment_id")
                .and_then(|v| v.as_str())
        })
        .ok_or_else(|| anyhow!("command with credential references is missing deployment_id"))?;
    let workspace_id = workspace_for_deployment(pool, deployment_id).await?;

    if let Some(credential_id) = registry_credential_id {
        let credential =
            crate::credentials::fetch_decrypted(pool, master_key, &workspace_id, &credential_id)
                .await?
                .ok_or_else(|| anyhow!("registry credential {credential_id} not found"))?;
        if credential.kind != "registry" {
            return Err(anyhow!(
                "credential {credential_id} is not a registry credential"
            ));
        }
        inject_registry_password(&mut command.payload, &credential.secret)?;
    }

    if let Some(credential_id) = github_credential_id {
        let credential =
            crate::credentials::fetch_decrypted(pool, master_key, &workspace_id, &credential_id)
                .await?
                .ok_or_else(|| anyhow!("github credential {credential_id} not found"))?;
        if credential.kind != "github_pat" {
            return Err(anyhow!("credential {credential_id} is not a github_pat"));
        }
        inject_github_pat(&mut command.payload, &credential.secret)?;
    }

    Ok(())
}

async fn workspace_for_deployment(
    pool: &DatabaseConnection,
    deployment_id: &str,
) -> Result<String> {
    let row: Option<(String,)> = crate::db::query_tuple(
        "SELECT p.workspace_id \
         FROM deployments d \
         JOIN services s ON s.id = d.service_id \
         JOIN projects p ON p.id = s.project_id \
         WHERE d.id = $1",
    )
    .bind(deployment_id)
    .fetch_optional(pool)
    .await?;
    row.map(|(workspace_id,)| workspace_id)
        .ok_or_else(|| anyhow!("deployment {deployment_id} not found"))
}

fn registry_credential_id(payload: &JsonValue) -> Option<String> {
    payload
        .get("registry")
        .and_then(|v| v.get("credential_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn string_field(payload: &JsonValue, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn inject_registry_password(payload: &mut JsonValue, password: &str) -> Result<()> {
    let registry = payload
        .get_mut("registry")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| anyhow!("registry credential reference without registry object"))?;
    registry.insert(
        "password".to_string(),
        JsonValue::String(password.to_string()),
    );
    registry.remove("credential_id");
    Ok(())
}

fn inject_github_pat(payload: &mut JsonValue, pat: &str) -> Result<()> {
    let object = payload
        .as_object_mut()
        .ok_or_else(|| anyhow!("build payload is not an object"))
        .context("injecting github credential")?;
    object.insert("github_pat".to_string(), JsonValue::String(pat.to_string()));
    object.remove("github_credential_id");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_and_run_payload_persists_credential_id_not_password() {
        let payload = pull_and_run_payload(&PullAndRunPayload {
            image: "example.com/app:latest",
            env: &json!({}),
            ports: &json!([]),
            cpu_millis: 500,
            memory_mb: 256,
            registry: Some(&RegistryAuth {
                url: "example.com",
                username: "robot",
                credential_id: "cred_123",
            }),
            volume: None,
            private_network: None,
        });

        assert_eq!(payload["registry"]["credential_id"], "cred_123");
        assert!(payload["registry"].get("password").is_none());
    }

    #[test]
    fn build_payload_persists_credential_ids_not_secrets() {
        let payload = build_payload(&BuildPayload {
            build_id: "build_1",
            deployment_id: "dep_1",
            service_id: "svc_1",
            git_repo: "https://github.com/acme/app",
            git_branch: "main",
            builder: "dockerfile",
            dockerfile_path: None,
            root_dir: ".",
            image_tag: "registry/ws/svc:build",
            github_credential_id: Some("github_cred"),
            registry: Some(RegistryAuth {
                url: "registry",
                username: "robot",
                credential_id: "registry_cred",
            }),
        });

        assert_eq!(payload["github_credential_id"], "github_cred");
        assert_eq!(payload["registry"]["credential_id"], "registry_cred");
        assert!(payload.get("github_pat").is_none());
        assert!(payload["registry"].get("password").is_none());
    }

    #[test]
    fn injectors_restore_agent_wire_shape_without_credential_ids() {
        let mut payload = json!({
            "github_credential_id": "github_cred",
            "registry": {
                "url": "registry",
                "username": "robot",
                "credential_id": "registry_cred"
            }
        });

        inject_registry_password(&mut payload, "registry-secret").unwrap();
        inject_github_pat(&mut payload, "github-secret").unwrap();

        assert_eq!(payload["registry"]["password"], "registry-secret");
        assert_eq!(payload["github_pat"], "github-secret");
        assert!(payload["registry"].get("credential_id").is_none());
        assert!(payload.get("github_credential_id").is_none());
    }
}
