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
    /// Optional one-time installer secret required for the first platform-admin
    /// signup. If unset, first-signup bootstrap keeps its historical behavior.
    pub setup_token: Option<String>,
}

pub struct LoadedConfig {
    pub config: Config,
    pub master_key: MasterKey,
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
        let setup_token = optional_env(&["DRIFTBASE_SETUP_TOKEN"]);

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
                setup_token,
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
