pub mod routes;

use anyhow::Result;
use reqwest::redirect::Policy;
use sqlx::PgPool;
use std::time::Duration;

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

/// Refresh `service_domains.tls_status` by probing each hostname over HTTPS.
/// A domain is only considered probeable once its service has a running
/// deployment; until then it stays `pending`.
pub async fn refresh_tls_statuses(pool: &PgPool) -> Result<()> {
    let rows: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT sd.id, sd.hostname, \
                EXISTS(SELECT 1 FROM deployments d \
                       WHERE d.service_id = sd.service_id AND d.status = 'running') AS has_running \
         FROM service_domains sd \
         ORDER BY sd.created_at ASC",
    )
    .fetch_all(pool)
    .await?;

    let http = reqwest::Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(5))
        .build()?;

    for (id, hostname, has_running) in rows {
        if !has_running {
            sqlx::query(
                "UPDATE service_domains SET \
                    tls_status = 'pending', \
                    last_error = NULL, \
                    updated_at = now() \
                 WHERE id = $1",
            )
            .bind(&id)
            .execute(pool)
            .await?;
            continue;
        }

        match probe_hostname(&http, &hostname).await {
            Ok(()) => {
                sqlx::query(
                    "UPDATE service_domains SET \
                        tls_status = 'active', \
                        last_error = NULL, \
                        last_cert_at = COALESCE(last_cert_at, now()), \
                        updated_at = now() \
                     WHERE id = $1",
                )
                .bind(&id)
                .execute(pool)
                .await?;
            }
            Err(err) => {
                let err = err.to_string();
                sqlx::query(
                    "UPDATE service_domains SET \
                        tls_status = 'failed', \
                        last_error = $2, \
                        updated_at = now() \
                     WHERE id = $1",
                )
                .bind(&id)
                .bind(&err)
                .execute(pool)
                .await?;
            }
        }
    }

    Ok(())
}

async fn probe_hostname(http: &reqwest::Client, hostname: &str) -> Result<()> {
    let url = format!("https://{hostname}");
    let res = http.get(url).send().await?;
    // Any HTTP response means DNS + TCP + TLS + request routing worked.
    let _ = res.status();
    Ok(())
}
