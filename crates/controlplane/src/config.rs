use std::net::SocketAddr;

use anyhow::{anyhow, Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub public_url: String,
    pub cookie_secure: bool,
}

impl Config {
    pub fn from_env() -> Result<Self> {
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

        Ok(Self {
            bind_addr,
            database_url,
            public_url,
            cookie_secure,
        })
    }
}
