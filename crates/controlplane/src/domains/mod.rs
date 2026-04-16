pub mod routes;

use anyhow::Result;
use sqlx::PgPool;

/// Hostnames currently pointing at a given node, derived from service_domains
/// joined to running deployments on that node. One entry per live hostname.
#[derive(Debug, Clone)]
pub struct NodeRoute {
    pub hostname: String,
    pub container_port: u16,
    pub deployment_id: String,
    pub container_name: String,
}

/// Fetch the current live route set for a node: each service_domain whose
/// service has a running deployment pinned to this node.
pub async fn routes_for_node(pool: &PgPool, node_id: &str) -> Result<Vec<NodeRoute>> {
    let rows: Vec<(String, i32, String)> = sqlx::query_as(
        "SELECT sd.hostname, sd.container_port, d.id AS deployment_id \
         FROM service_domains sd \
         JOIN services s ON s.id = sd.service_id \
         JOIN deployments d ON d.service_id = s.id \
         WHERE d.node_id = $1 AND d.status = 'running' \
         ORDER BY sd.hostname ASC",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(hostname, container_port, deployment_id)| NodeRoute {
            hostname,
            container_port: container_port as u16,
            container_name: format!("zediz-{deployment_id}"),
            deployment_id,
        })
        .collect())
}

/// Return every node currently hosting a running deployment for the given
/// service. These are the nodes whose Caddy needs updating when a domain on
/// this service changes.
pub async fn nodes_for_service(pool: &PgPool, service_id: &str) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT d.node_id FROM deployments d \
         WHERE d.service_id = $1 AND d.node_id IS NOT NULL AND d.status = 'running'",
    )
    .bind(service_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(n,)| n).collect())
}

pub fn validate_hostname(h: &str) -> Result<(), String> {
    if h.is_empty() || h.len() > 253 {
        return Err("hostname must be 1–253 chars".into());
    }
    if h.starts_with('.') || h.ends_with('.') || h.contains("..") {
        return Err("hostname has invalid dots".into());
    }
    for label in h.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err("label must be 1–63 chars".into());
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("label has invalid characters".into());
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err("label cannot start or end with '-'".into());
        }
    }
    if !h.contains('.') {
        return Err("hostname must be fully qualified".into());
    }
    Ok(())
}
