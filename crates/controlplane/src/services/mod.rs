pub mod references;
pub mod routes;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resources {
    pub cpu_millis: u32,
    pub memory_mb: u32,
    pub disk_mb: u32,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            cpu_millis: 500,
            memory_mb: 256,
            disk_mb: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMap {
    pub container_port: u16,
    pub host_port: Option<u16>,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_protocol() -> String {
    "tcp".into()
}

pub type EnvVars = BTreeMap<String, String>;
