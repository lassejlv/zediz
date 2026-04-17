use std::net::SocketAddr;

use anyhow::{anyhow, Context, Result};

use crate::crypto::MasterKey;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub public_url: String,
    pub cookie_secure: bool,
    /// Public hostname of the bundled registry (e.g. `registry.zediz.dev`),
    /// or `None` when the bundled registry is not in use. Used to decide
    /// whether a registry credential's URL points at the bundled registry
    /// (and therefore whether the CP auth proxy should mediate it).
    pub registry_site: Option<String>,
    /// Internal URL the CP uses to reach the registry container. Only
    /// meaningful when `registry_site` is set.
    pub registry_upstream: String,
}

pub struct LoadedConfig {
    pub config: Config,
    pub master_key: MasterKey,
}

impl Config {
    pub fn from_env() -> Result<LoadedConfig> {
        let bind_addr = std::env::var("ZEDIZ_BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".into())
            .parse()
            .context("ZEDIZ_BIND_ADDR")?;
        let database_url = std::env::var("ZEDIZ_DATABASE_URL")
            .map_err(|_| anyhow!("ZEDIZ_DATABASE_URL is required"))?;
        let public_url =
            std::env::var("ZEDIZ_PUBLIC_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        let cookie_secure = std::env::var("ZEDIZ_COOKIE_SECURE")
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);

        let master_key_raw = std::env::var("ZEDIZ_MASTER_KEY")
            .map_err(|_| anyhow!("ZEDIZ_MASTER_KEY is required (base64 of 32 bytes)"))?;
        let master_key =
            MasterKey::from_base64(&master_key_raw).context("loading ZEDIZ_MASTER_KEY")?;

        let registry_site = std::env::var("ZEDIZ_REGISTRY_SITE")
            .ok()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let registry_upstream = std::env::var("ZEDIZ_REGISTRY_UPSTREAM")
            .unwrap_or_else(|_| "http://registry:5000".into());

        Ok(LoadedConfig {
            config: Self {
                bind_addr,
                database_url,
                public_url,
                cookie_secure,
                registry_site,
                registry_upstream,
            },
            master_key,
        })
    }
}
