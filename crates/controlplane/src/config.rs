use std::net::SocketAddr;

use anyhow::{anyhow, Context, Result};

use crate::crypto::MasterKey;

pub const DEFAULT_AGENT_IMAGE: &str = "ghcr.io/lassejlv/driftbase-agent:latest";

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: SocketAddr,
    /// Direct database connection used for schema migrations.
    pub database_url: String,
    /// Optional pooled database connection used by the app after migrations.
    pub database_pooled_url: Option<String>,
    pub public_url: String,
    pub cookie_secure: bool,
    /// Public hostname of the bundled registry (e.g. `registry.driftbase.app`),
    /// or `None` when the bundled registry is not in use. Used to decide
    /// whether a registry credential's URL points at the bundled registry
    /// (and therefore whether the CP auth proxy should mediate it).
    pub registry_site: Option<String>,
    /// Internal URL the CP uses to reach the registry container. Only
    /// meaningful when `registry_site` is set.
    pub registry_upstream: String,
    /// Desired node-agent image. The node update checker resolves this ref to
    /// a registry digest and compares nodes against it.
    pub agent_image: String,
    /// Preferred Hetzner server type for autoscaled nodes. If the requested
    /// service resources do not fit, provisioning falls back to cheapest-fit.
    pub default_hetzner_server_type: Option<String>,
    /// Optional one-time installer secret required for the first platform-admin
    /// signup. If unset, first-signup bootstrap keeps its historical behavior.
    pub setup_token: Option<String>,
    /// Hetzner API token used for Railway-like managed workspaces. In this
    /// mode users do not connect their own Hetzner account; Driftbase owns
    /// provisioning through this local control-plane secret.
    pub managed_hetzner_api_token: Option<String>,
    /// GitHub App configuration for repository-backed builds. When unset the
    /// legacy GitHub PAT path remains available, but GitHub App connect UI and
    /// webhook processing are disabled.
    pub github_app: Option<GitHubAppConfig>,
}

pub struct LoadedConfig {
    pub config: Config,
    pub master_key: MasterKey,
}

#[derive(Clone, Debug)]
pub struct GitHubAppConfig {
    pub app_id: i64,
    pub client_id: String,
    pub client_secret: String,
    pub private_key: String,
    pub webhook_secret: String,
    pub slug: String,
}

impl Config {
    pub fn from_env() -> Result<LoadedConfig> {
        let bind_addr = std::env::var("DRIFTBASE_BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".into())
            .parse()
            .context("DRIFTBASE_BIND_ADDR")?;
        let database_url = std::env::var("DRIFTBASE_DATABASE_URL")
            .map_err(|_| anyhow!("DRIFTBASE_DATABASE_URL is required"))?;
        let database_pooled_url =
            optional_env(&["DRIFTBASE_DATABASE_POOLED_URL", "DATABASE_POOLED_URL"]);
        let public_url = std::env::var("DRIFTBASE_PUBLIC_URL")
            .unwrap_or_else(|_| "http://localhost:8080".into());
        let cookie_secure = std::env::var("DRIFTBASE_COOKIE_SECURE")
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);

        let master_key_raw = std::env::var("DRIFTBASE_MASTER_KEY")
            .map_err(|_| anyhow!("DRIFTBASE_MASTER_KEY is required (base64 of 32 bytes)"))?;
        let master_key =
            MasterKey::from_base64(&master_key_raw).context("loading DRIFTBASE_MASTER_KEY")?;

        let registry_site = std::env::var("DRIFTBASE_REGISTRY_SITE")
            .ok()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let registry_upstream = std::env::var("DRIFTBASE_REGISTRY_UPSTREAM")
            .unwrap_or_else(|_| "http://registry:5000".into());
        let agent_image = std::env::var("DRIFTBASE_AGENT_IMAGE")
            .map(|s| s.trim().to_string())
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_AGENT_IMAGE.to_string());
        let default_hetzner_server_type = optional_env(&["DRIFTBASE_DEFAULT_HETZNER_SERVER_TYPE"]);
        let setup_token = optional_env(&["DRIFTBASE_SETUP_TOKEN"]);
        let managed_hetzner_api_token = optional_env(&["DRIFTBASE_MANAGED_HETZNER_API_TOKEN"]);
        let github_app = load_github_app_config()?;

        Ok(LoadedConfig {
            config: Self {
                bind_addr,
                database_url,
                database_pooled_url,
                public_url,
                cookie_secure,
                registry_site,
                registry_upstream,
                agent_image,
                default_hetzner_server_type,
                setup_token,
                managed_hetzner_api_token,
                github_app,
            },
            master_key,
        })
    }
}

fn optional_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn load_github_app_config() -> Result<Option<GitHubAppConfig>> {
    let app_id = optional_env(&["DRIFTBASE_GITHUB_APP_ID"]);
    let client_id = optional_env(&["DRIFTBASE_GITHUB_APP_CLIENT_ID"]);
    let client_secret = optional_env(&["DRIFTBASE_GITHUB_APP_CLIENT_SECRET"]);
    let private_key = optional_env(&["DRIFTBASE_GITHUB_APP_PRIVATE_KEY"]);
    let webhook_secret = optional_env(&["DRIFTBASE_GITHUB_APP_WEBHOOK_SECRET"]);
    let slug = optional_env(&["DRIFTBASE_GITHUB_APP_SLUG"]);

    let values = [
        app_id.as_deref(),
        client_id.as_deref(),
        client_secret.as_deref(),
        private_key.as_deref(),
        webhook_secret.as_deref(),
        slug.as_deref(),
    ];
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    if values.iter().any(Option::is_none) {
        return Err(anyhow!(
            "GitHub App config is incomplete; set DRIFTBASE_GITHUB_APP_ID, \
             DRIFTBASE_GITHUB_APP_CLIENT_ID, DRIFTBASE_GITHUB_APP_CLIENT_SECRET, \
             DRIFTBASE_GITHUB_APP_PRIVATE_KEY, DRIFTBASE_GITHUB_APP_WEBHOOK_SECRET, \
             and DRIFTBASE_GITHUB_APP_SLUG"
        ));
    }

    let app_id = app_id
        .expect("checked above")
        .parse::<i64>()
        .context("DRIFTBASE_GITHUB_APP_ID")?;

    Ok(Some(GitHubAppConfig {
        app_id,
        client_id: client_id.expect("checked above"),
        client_secret: client_secret.expect("checked above"),
        private_key: private_key.expect("checked above").replace("\\n", "\n"),
        webhook_secret: webhook_secret.expect("checked above"),
        slug: slug.expect("checked above"),
    }))
}

#[cfg(test)]
mod tests {
    use super::optional_env;

    #[test]
    fn optional_env_ignores_missing_and_blank_values() {
        let key = "DRIFTBASE_TEST_BLANK_OPTIONAL_ENV";
        std::env::set_var(key, "   ");
        assert_eq!(optional_env(&[key]), None);
        std::env::remove_var(key);
    }

    #[test]
    fn optional_env_uses_first_non_empty_value() {
        let first = "DRIFTBASE_TEST_FIRST_OPTIONAL_ENV";
        let second = "DRIFTBASE_TEST_SECOND_OPTIONAL_ENV";
        std::env::set_var(first, " ");
        std::env::set_var(second, " postgres://pooled ");
        assert_eq!(
            optional_env(&[first, second]),
            Some("postgres://pooled".to_string())
        );
        std::env::remove_var(first);
        std::env::remove_var(second);
    }
}
