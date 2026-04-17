use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use sqlx::PgPool;
use zediz_common::Id;

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
    pool: &PgPool,
    node_id: &str,
    deployment_id: Option<&str>,
    kind: CommandKind,
    payload: JsonValue,
) -> Result<Id> {
    let id = Id::new();
    sqlx::query(
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
pub async fn claim_for_node(pool: &PgPool, node_id: &str, limit: i64) -> Result<Vec<AgentCommand>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: String,
        deployment_id: Option<String>,
        kind: String,
        payload: JsonValue,
        created_at: DateTime<Utc>,
    }
    let rows: Vec<Row> = sqlx::query_as(
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
    pool: &PgPool,
    command_id: &str,
    ok: bool,
    result: Option<&str>,
) -> Result<()> {
    let status = if ok { "acked" } else { "errored" };
    sqlx::query(
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

pub fn pull_and_run_payload(
    image: &str,
    env: &serde_json::Value,
    ports: &serde_json::Value,
    cpu_millis: u32,
    memory_mb: u32,
) -> JsonValue {
    json!({
        "image": image,
        "env": env,
        "ports": ports,
        "cpu_millis": cpu_millis,
        "memory_mb": memory_mb,
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
    let registry = p.registry.as_ref().map(|r| {
        json!({ "url": r.url, "username": r.username, "password": r.password })
    });
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
