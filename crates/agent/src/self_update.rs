use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::process::Command;

const DEFAULT_AGENT_ENV_PATH: &str = "/etc/driftbase/agent.env";

#[derive(Debug, Clone)]
pub struct AgentUpdateState {
    pub image_ref: Option<String>,
    pub image_digest: Option<String>,
    pub self_update_capable: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAgentPayload {
    pub image_ref: String,
    #[serde(default)]
    pub target_digest: Option<String>,
    #[serde(default)]
    pub source_image_ref: Option<String>,
}

pub async fn detect() -> AgentUpdateState {
    let image_ref = std::env::var("DRIFTBASE_AGENT_IMAGE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let image_digest = match image_ref.as_deref() {
        Some(image) => match digest_from_ref(image) {
            Some(digest) => Some(digest),
            None => docker_image_digest(image).await.ok().flatten(),
        },
        None => None,
    };
    AgentUpdateState {
        image_ref,
        image_digest,
        self_update_capable: agent_env_path().is_file(),
    }
}

pub async fn persist_node_token(node_token: &str) -> Result<()> {
    let path = agent_env_path();
    if !path.is_file() {
        return Err(anyhow!("{} is not mounted", path.display()));
    }
    rewrite_env_file(&path, node_token, None).await
}

pub async fn prepare_update(node_token: &str, payload: &UpdateAgentPayload) -> Result<()> {
    validate_env_value("DRIFTBASE_AGENT_IMAGE", &payload.image_ref)?;
    if let Some(source) = payload.source_image_ref.as_deref() {
        tracing::info!(source_image = %source, target_image = %payload.image_ref, "preparing agent update");
    }

    pull_image(&payload.image_ref).await?;
    if let Some(target) = payload.target_digest.as_deref() {
        let actual = match digest_from_ref(&payload.image_ref) {
            Some(digest) => Some(digest),
            None => docker_image_digest(&payload.image_ref).await.ok().flatten(),
        }
        .ok_or_else(|| anyhow!("could not determine pulled agent image digest"))?;
        if actual != target {
            return Err(anyhow!(
                "pulled agent image digest mismatch: expected {target}, got {actual}"
            ));
        }
    }

    let path = agent_env_path();
    if !path.is_file() {
        return Err(anyhow!("{} is not mounted", path.display()));
    }
    rewrite_env_file(&path, node_token, Some(&payload.image_ref)).await
}

fn agent_env_path() -> PathBuf {
    std::env::var("DRIFTBASE_AGENT_ENV_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_AGENT_ENV_PATH))
}

async fn pull_image(image_ref: &str) -> Result<()> {
    let out = Command::new("docker")
        .arg("pull")
        .arg(image_ref)
        .output()
        .await
        .with_context(|| format!("spawning docker pull {image_ref}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "docker pull {image_ref} failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

async fn docker_image_digest(image_ref: &str) -> Result<Option<String>> {
    let out = Command::new("docker")
        .arg("image")
        .arg("inspect")
        .arg(image_ref)
        .arg("--format")
        .arg("{{json .RepoDigests}}")
        .output()
        .await
        .with_context(|| format!("spawning docker image inspect {image_ref}"))?;
    if !out.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let repo_digests: Vec<String> = serde_json::from_str(stdout.trim()).unwrap_or_default();
    Ok(repo_digests.iter().find_map(|item| digest_from_ref(item)))
}

fn digest_from_ref(image_ref: &str) -> Option<String> {
    image_ref
        .split_once('@')
        .map(|(_, digest)| digest.to_string())
        .filter(|digest| digest.starts_with("sha256:"))
}

async fn rewrite_env_file(path: &Path, node_token: &str, agent_image: Option<&str>) -> Result<()> {
    validate_env_value("DRIFTBASE_NODE_TOKEN", node_token)?;
    if let Some(image) = agent_image {
        validate_env_value("DRIFTBASE_AGENT_IMAGE", image)?;
    }

    let existing = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    let mut saw_token = false;
    let mut saw_image = false;
    let mut lines = Vec::new();

    for line in existing.lines() {
        if line.starts_with("DRIFTBASE_NODE_TOKEN=") {
            saw_token = true;
            lines.push(format!("DRIFTBASE_NODE_TOKEN={node_token}"));
        } else if line.starts_with("DRIFTBASE_AGENT_IMAGE=") {
            saw_image = true;
            if let Some(image) = agent_image {
                lines.push(format!("DRIFTBASE_AGENT_IMAGE={image}"));
            } else {
                lines.push(line.to_string());
            }
        } else {
            lines.push(line.to_string());
        }
    }

    if !saw_token {
        lines.push(format!("DRIFTBASE_NODE_TOKEN={node_token}"));
    }
    if let Some(image) = agent_image {
        if !saw_image {
            lines.push(format!("DRIFTBASE_AGENT_IMAGE={image}"));
        }
    }

    let content = format!("{}\n", lines.join("\n"));
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    tokio::fs::write(&tmp, content)
        .await
        .with_context(|| format!("writing {}", tmp.display()))?;
    set_secret_permissions(&tmp).await?;
    tokio::fs::rename(&tmp, path)
        .await
        .with_context(|| format!("replacing {}", path.display()))?;
    set_secret_permissions(path).await?;
    Ok(())
}

async fn set_secret_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        tokio::fs::set_permissions(path, perms).await?;
    }
    Ok(())
}

fn validate_env_value(name: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c == '\n' || c == '\r' || c == '\0' || c.is_whitespace())
    {
        return Err(anyhow!(
            "{name} contains characters unsupported by agent.env"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_digest_from_image_ref() {
        assert_eq!(
            digest_from_ref("ghcr.io/driftbase/agent@sha256:abc"),
            Some("sha256:abc".into())
        );
        assert_eq!(digest_from_ref("ghcr.io/driftbase/agent:latest"), None);
    }

    #[tokio::test]
    async fn rewrites_env_file_preserving_other_values() {
        let path = std::env::temp_dir().join(format!(
            "driftbase-agent-env-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        tokio::fs::write(
            &path,
            "DRIFTBASE_CONTROL_PLANE_URL=https://cp.example\nDRIFTBASE_AGENT_IMAGE=old:latest\n",
        )
        .await
        .unwrap();

        rewrite_env_file(
            &path,
            "node.token",
            Some("ghcr.io/driftbase/agent@sha256:new"),
        )
        .await
        .unwrap();

        let updated = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(updated.contains("DRIFTBASE_CONTROL_PLANE_URL=https://cp.example\n"));
        assert!(updated.contains("DRIFTBASE_NODE_TOKEN=node.token\n"));
        assert!(updated.contains("DRIFTBASE_AGENT_IMAGE=ghcr.io/driftbase/agent@sha256:new\n"));
        let _ = tokio::fs::remove_file(&path).await;
    }
}
