use anyhow::{anyhow, Context, Result};
use driftbase_common::Id;
use reqwest::header::{ACCEPT, WWW_AUTHENTICATE};
use reqwest::{Client, StatusCode};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::agent::commands::CommandKind;
use crate::config::Config;
use crate::error::{ApiError, ApiResult};

const MANIFEST_ACCEPT: &str = concat!(
    "application/vnd.oci.image.index.v1+json, ",
    "application/vnd.oci.image.manifest.v1+json, ",
    "application/vnd.docker.distribution.manifest.list.v2+json, ",
    "application/vnd.docker.distribution.manifest.v2+json"
);

#[derive(Debug, Serialize)]
pub struct AgentUpdateResponse {
    pub status: String,
    pub update_available: bool,
    pub target_image_ref: Option<String>,
    pub target_digest: Option<String>,
    pub error: Option<String>,
    pub command_id: Option<Id>,
}

#[derive(Debug, sea_orm::FromQueryResult)]
struct NodeUpdateRow {
    id: String,
    agent_image_digest: Option<String>,
    agent_self_update_capable: bool,
    agent_update_status: String,
}

#[derive(Debug, Clone)]
pub struct AgentSnapshot<'a> {
    pub version: Option<&'a str>,
    pub image_ref: Option<&'a str>,
    pub image_digest: Option<&'a str>,
    pub self_update_capable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImageRef {
    registry: String,
    repository: String,
    reference: String,
    name_without_reference: String,
    digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedImage {
    digest_ref: String,
    digest: String,
}

pub async fn check_node_update(
    pool: &DatabaseConnection,
    config: &Config,
    workspace_id: &str,
    node_id: &str,
) -> ApiResult<AgentUpdateResponse> {
    let node = load_node_update_row(pool, workspace_id, node_id).await?;
    let resolved = match resolve_agent_image(&config.agent_image).await {
        Ok(resolved) => resolved,
        Err(e) => {
            let msg = e.to_string();
            crate::db::query(
                "UPDATE nodes SET \
                    agent_update_status = 'check_failed', \
                    agent_update_checked_at = now(), \
                    agent_update_target_image_ref = $1, \
                    agent_update_target_digest = NULL, \
                    agent_update_command_id = NULL, \
                    agent_update_error = $2 \
                 WHERE id = $3",
            )
            .bind(&config.agent_image)
            .bind(&msg)
            .bind(&node.id)
            .execute(pool)
            .await?;
            return Ok(AgentUpdateResponse {
                status: "check_failed".into(),
                update_available: false,
                target_image_ref: Some(config.agent_image.clone()),
                target_digest: None,
                error: Some(msg),
                command_id: None,
            });
        }
    };

    let status = classify_update_status(
        node.agent_self_update_capable,
        node.agent_image_digest.as_deref(),
        &resolved.digest,
    );
    crate::db::query(
        "UPDATE nodes SET \
            agent_update_status = $1, \
            agent_update_checked_at = now(), \
            agent_update_target_image_ref = $2, \
            agent_update_target_digest = $3, \
            agent_update_command_id = NULL, \
            agent_update_error = NULL, \
            agent_update_finished_at = CASE WHEN $1 = 'current' THEN now() ELSE agent_update_finished_at END \
         WHERE id = $4",
    )
    .bind(status)
    .bind(&resolved.digest_ref)
    .bind(&resolved.digest)
    .bind(&node.id)
    .execute(pool)
    .await?;

    Ok(AgentUpdateResponse {
        status: status.into(),
        update_available: status == "available",
        target_image_ref: Some(resolved.digest_ref),
        target_digest: Some(resolved.digest),
        error: None,
        command_id: None,
    })
}

pub async fn enqueue_node_update(
    pool: &DatabaseConnection,
    config: &Config,
    workspace_id: &str,
    node_id: &str,
) -> ApiResult<AgentUpdateResponse> {
    let node = load_node_update_row(pool, workspace_id, node_id).await?;
    if !node.agent_self_update_capable {
        return Err(ApiError::Conflict(
            "node agent does not support self updates yet".into(),
        ));
    }
    if matches!(node.agent_update_status.as_str(), "updating" | "restarting") {
        return Err(ApiError::Conflict(
            "agent update already in progress".into(),
        ));
    }

    let resolved = match resolve_agent_image(&config.agent_image).await {
        Ok(resolved) => resolved,
        Err(e) => {
            let msg = e.to_string();
            crate::db::query(
                "UPDATE nodes SET \
                    agent_update_status = 'check_failed', \
                    agent_update_checked_at = now(), \
                    agent_update_target_image_ref = $1, \
                    agent_update_target_digest = NULL, \
                    agent_update_command_id = NULL, \
                    agent_update_error = $2 \
                 WHERE id = $3",
            )
            .bind(&config.agent_image)
            .bind(&msg)
            .bind(&node.id)
            .execute(pool)
            .await?;
            return Ok(AgentUpdateResponse {
                status: "check_failed".into(),
                update_available: false,
                target_image_ref: Some(config.agent_image.clone()),
                target_digest: None,
                error: Some(msg),
                command_id: None,
            });
        }
    };

    let status = classify_update_status(
        node.agent_self_update_capable,
        node.agent_image_digest.as_deref(),
        &resolved.digest,
    );
    if status == "current" {
        crate::db::query(
            "UPDATE nodes SET \
                agent_update_status = 'current', \
                agent_update_checked_at = now(), \
                agent_update_target_image_ref = $1, \
                agent_update_target_digest = $2, \
                agent_update_command_id = NULL, \
                agent_update_error = NULL, \
                agent_update_finished_at = now() \
             WHERE id = $3",
        )
        .bind(&resolved.digest_ref)
        .bind(&resolved.digest)
        .bind(&node.id)
        .execute(pool)
        .await?;
        return Ok(AgentUpdateResponse {
            status: "current".into(),
            update_available: false,
            target_image_ref: Some(resolved.digest_ref),
            target_digest: Some(resolved.digest),
            error: None,
            command_id: None,
        });
    }

    let command_id = Id::new();
    let payload = json!({
        "image_ref": &resolved.digest_ref,
        "source_image_ref": &config.agent_image,
        "target_digest": &resolved.digest,
    });
    crate::db::query(
        "INSERT INTO agent_commands (id, node_id, kind, payload) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(command_id.to_string())
    .bind(&node.id)
    .bind(CommandKind::UpdateAgent.as_str())
    .bind(payload)
    .execute(pool)
    .await?;

    crate::db::query(
        "UPDATE nodes SET \
            agent_update_status = 'updating', \
            agent_update_checked_at = now(), \
            agent_update_target_image_ref = $1, \
            agent_update_target_digest = $2, \
            agent_update_command_id = $3, \
            agent_update_error = NULL, \
            agent_update_started_at = now(), \
            agent_update_finished_at = NULL \
         WHERE id = $4",
    )
    .bind(&resolved.digest_ref)
    .bind(&resolved.digest)
    .bind(command_id.to_string())
    .bind(&node.id)
    .execute(pool)
    .await?;

    Ok(AgentUpdateResponse {
        status: "updating".into(),
        update_available: false,
        target_image_ref: Some(resolved.digest_ref),
        target_digest: Some(resolved.digest),
        error: None,
        command_id: Some(command_id),
    })
}

pub async fn record_agent_snapshot(
    pool: &DatabaseConnection,
    node_id: &str,
    snapshot: AgentSnapshot<'_>,
) -> ApiResult<()> {
    if snapshot.version.is_none()
        && snapshot.image_ref.is_none()
        && snapshot.image_digest.is_none()
        && snapshot.self_update_capable.is_none()
    {
        return Ok(());
    }

    crate::db::query(
        "UPDATE nodes SET \
            agent_version = COALESCE($2, agent_version), \
            agent_image_ref = COALESCE($3, agent_image_ref), \
            agent_image_digest = COALESCE($4, agent_image_digest), \
            agent_self_update_capable = COALESCE($5, agent_self_update_capable), \
            agent_update_status = CASE \
                WHEN $4 IS NOT NULL AND agent_update_target_digest = $4 THEN 'current' \
                WHEN agent_update_status IN ('updating', 'restarting') \
                     AND agent_update_started_at < now() - interval '2 minutes' THEN 'failed' \
                ELSE agent_update_status \
            END, \
            agent_update_error = CASE \
                WHEN $4 IS NOT NULL AND agent_update_target_digest = $4 THEN NULL \
                WHEN agent_update_status IN ('updating', 'restarting') \
                     AND agent_update_started_at < now() - interval '2 minutes' \
                    THEN 'agent restarted without reporting the target image digest' \
                ELSE agent_update_error \
            END, \
            agent_update_finished_at = CASE \
                WHEN $4 IS NOT NULL AND agent_update_target_digest = $4 THEN COALESCE(agent_update_finished_at, now()) \
                WHEN agent_update_status IN ('updating', 'restarting') \
                     AND agent_update_started_at < now() - interval '2 minutes' THEN now() \
                ELSE agent_update_finished_at \
            END, \
            agent_update_command_id = CASE \
                WHEN $4 IS NOT NULL AND agent_update_target_digest = $4 THEN NULL \
                ELSE agent_update_command_id \
            END \
         WHERE id = $1",
    )
    .bind(node_id)
    .bind(snapshot.version)
    .bind(snapshot.image_ref)
    .bind(snapshot.image_digest)
    .bind(snapshot.self_update_capable)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn record_update_ack(
    pool: &DatabaseConnection,
    node_id: &str,
    command_id: &str,
    ok: bool,
    message: Option<&str>,
) -> ApiResult<()> {
    if ok {
        crate::db::query(
            "UPDATE nodes SET \
                agent_update_status = 'restarting', \
                agent_update_error = NULL \
             WHERE id = $1 AND agent_update_command_id = $2",
        )
        .bind(node_id)
        .bind(command_id)
        .execute(pool)
        .await?;
    } else {
        crate::db::query(
            "UPDATE nodes SET \
                agent_update_status = 'failed', \
                agent_update_error = $1, \
                agent_update_finished_at = now() \
             WHERE id = $2 AND agent_update_command_id = $3",
        )
        .bind(message)
        .bind(node_id)
        .bind(command_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn load_node_update_row(
    pool: &DatabaseConnection,
    workspace_id: &str,
    node_id: &str,
) -> ApiResult<NodeUpdateRow> {
    crate::db::query_as(
        "SELECT id, agent_image_digest, agent_self_update_capable, agent_update_status \
         FROM nodes \
         WHERE id = $1 AND workspace_id = $2 AND status <> 'terminated'",
    )
    .bind(node_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?
    .ok_or(ApiError::NotFound)
}

fn classify_update_status(
    self_update_capable: bool,
    current_digest: Option<&str>,
    target_digest: &str,
) -> &'static str {
    if !self_update_capable {
        return "unsupported";
    }
    match current_digest {
        Some(current) if current == target_digest => "current",
        _ => "available",
    }
}

async fn resolve_agent_image(image_ref: &str) -> Result<ResolvedImage> {
    let parsed = parse_image_ref(image_ref)?;
    if let Some(digest) = parsed.digest.clone() {
        return Ok(ResolvedImage {
            digest_ref: format!("{}@{}", parsed.name_without_reference, digest),
            digest,
        });
    }

    let client = Client::builder()
        .user_agent(concat!(
            "driftbase-controlplane/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("building registry client")?;
    let digest = fetch_manifest_digest(&client, &parsed).await?;
    Ok(ResolvedImage {
        digest_ref: format!("{}@{}", parsed.name_without_reference, digest),
        digest,
    })
}

async fn fetch_manifest_digest(client: &Client, image: &ImageRef) -> Result<String> {
    let mut response = request_manifest(client, image, None).await?;
    if response.status() == StatusCode::UNAUTHORIZED {
        let challenge = response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow!("registry requires auth but did not send WWW-Authenticate"))?
            .to_string();
        let token = fetch_bearer_token(client, image, &challenge).await?;
        response = request_manifest(client, image, Some(&token)).await?;
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "registry manifest request failed: {status}: {body}"
        ));
    }

    response
        .headers()
        .get("Docker-Content-Digest")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("registry manifest response did not include Docker-Content-Digest"))
}

async fn request_manifest(
    client: &Client,
    image: &ImageRef,
    bearer: Option<&str>,
) -> Result<reqwest::Response> {
    let url = format!(
        "https://{}/v2/{}/manifests/{}",
        image.registry, image.repository, image.reference
    );
    let mut req = client.get(url).header(ACCEPT, MANIFEST_ACCEPT);
    if let Some(token) = bearer {
        req = req.bearer_auth(token);
    }
    req.send().await.context("requesting registry manifest")
}

#[derive(Deserialize)]
struct TokenResponse {
    token: Option<String>,
    access_token: Option<String>,
}

async fn fetch_bearer_token(client: &Client, image: &ImageRef, challenge: &str) -> Result<String> {
    let params = parse_bearer_challenge(challenge)
        .ok_or_else(|| anyhow!("unsupported registry auth challenge"))?;
    let realm = params
        .iter()
        .find_map(|(k, v)| (k == "realm").then_some(v.as_str()))
        .ok_or_else(|| anyhow!("registry auth challenge missing realm"))?;
    let service = params
        .iter()
        .find_map(|(k, v)| (k == "service").then_some(v));
    let scope = params
        .iter()
        .find_map(|(k, v)| (k == "scope").then_some(v.clone()))
        .unwrap_or_else(|| format!("repository:{}:pull", image.repository));

    let mut req = client.get(realm).query(&[("scope", scope.as_str())]);
    if let Some(service) = service {
        req = req.query(&[("service", service.as_str())]);
    }
    let res = req.send().await.context("requesting registry auth token")?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("registry auth token failed: {status}: {text}"));
    }
    let body: TokenResponse = serde_json::from_str(&text).context("parsing registry auth token")?;
    body.token
        .or(body.access_token)
        .ok_or_else(|| anyhow!("registry auth response did not include a token"))
}

fn parse_bearer_challenge(input: &str) -> Option<Vec<(String, String)>> {
    let rest = input.trim().strip_prefix("Bearer ")?;
    let mut out = Vec::new();
    for part in rest.split(',') {
        let (key, value) = part.trim().split_once('=')?;
        out.push((key.to_string(), value.trim_matches('"').to_string()));
    }
    Some(out)
}

fn parse_image_ref(input: &str) -> Result<ImageRef> {
    let input = input.trim();
    if input.is_empty() {
        return Err(anyhow!("agent image ref is empty"));
    }
    if input.starts_with("http://") || input.starts_with("https://") {
        return Err(anyhow!("agent image ref must not include a URL scheme"));
    }

    let (name_part, digest) = match input.split_once('@') {
        Some((name, digest)) => (name, Some(digest.to_string())),
        None => (input, None),
    };
    let (name_without_reference, reference) = if digest.is_some() {
        (name_part.to_string(), digest.clone().unwrap())
    } else {
        split_tag(name_part)
    };

    let mut parts = name_without_reference.split('/').collect::<Vec<_>>();
    let first = parts
        .first()
        .copied()
        .ok_or_else(|| anyhow!("agent image ref is missing repository"))?
        .to_string();
    let has_registry = first.contains('.') || first.contains(':') || first == "localhost";
    let (registry, repository) = if has_registry {
        parts.remove(0);
        (first, parts.join("/"))
    } else {
        let repo = if parts.len() == 1 {
            format!("library/{}", parts[0])
        } else {
            parts.join("/")
        };
        ("registry-1.docker.io".to_string(), repo)
    };
    if repository.is_empty() {
        return Err(anyhow!("agent image ref is missing repository"));
    }

    Ok(ImageRef {
        registry,
        repository,
        reference,
        name_without_reference,
        digest,
    })
}

fn split_tag(name: &str) -> (String, String) {
    let slash = name.rfind('/').map(|idx| idx + 1).unwrap_or(0);
    if let Some(colon) = name[slash..].rfind(':') {
        let colon = slash + colon;
        (name[..colon].to_string(), name[colon + 1..].to_string())
    } else {
        (name.to_string(), "latest".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ghcr_tag_ref() {
        let parsed = parse_image_ref("ghcr.io/driftbase/agent:latest").unwrap();
        assert_eq!(parsed.registry, "ghcr.io");
        assert_eq!(parsed.repository, "driftbase/agent");
        assert_eq!(parsed.reference, "latest");
        assert_eq!(parsed.name_without_reference, "ghcr.io/driftbase/agent");
    }

    #[test]
    fn parses_digest_ref() {
        let parsed = parse_image_ref("ghcr.io/driftbase/agent@sha256:abc123").unwrap();
        assert_eq!(parsed.registry, "ghcr.io");
        assert_eq!(parsed.reference, "sha256:abc123");
        assert_eq!(parsed.digest.as_deref(), Some("sha256:abc123"));
    }

    #[test]
    fn parses_docker_hub_short_ref() {
        let parsed = parse_image_ref("nginx:1.27").unwrap();
        assert_eq!(parsed.registry, "registry-1.docker.io");
        assert_eq!(parsed.repository, "library/nginx");
        assert_eq!(parsed.reference, "1.27");
    }

    #[test]
    fn classifies_update_state() {
        assert_eq!(
            classify_update_status(false, None, "sha256:a"),
            "unsupported"
        );
        assert_eq!(
            classify_update_status(true, Some("sha256:a"), "sha256:a"),
            "current"
        );
        assert_eq!(
            classify_update_status(true, Some("sha256:b"), "sha256:a"),
            "available"
        );
        assert_eq!(classify_update_status(true, None, "sha256:a"), "available");
    }

    #[test]
    fn parses_bearer_challenge() {
        let parsed = parse_bearer_challenge(
            r#"Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:x/y:pull""#,
        )
        .unwrap();
        assert!(parsed.contains(&("realm".into(), "https://ghcr.io/token".into())));
        assert!(parsed.contains(&("service".into(), "ghcr.io".into())));
    }
}
