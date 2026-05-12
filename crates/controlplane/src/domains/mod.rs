pub mod routes;

use anyhow::Result;
use reqwest::redirect::Policy;
use sea_orm::DatabaseConnection;
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::lookup_host;

/// Hostnames currently served by a public edge node. Each route points at the
/// active deployment's project-private IP, so the edge node does not need to be
/// the node that runs the container.
#[derive(Debug, Clone)]
pub struct NodeRoute {
    pub hostname: String,
    pub container_port: u16,
    pub deployment_id: String,
    pub upstream_host: String,
}

/// Fetch the current live edge route set for a node. Every ready public node in
/// the workspace gets every active domain route, and Caddy dials the workload
/// over the project private network.
pub async fn routes_for_node(pool: &DatabaseConnection, node_id: &str) -> Result<Vec<NodeRoute>> {
    let rows: Vec<(String, i32, String, String)> = crate::db::query_tuple(
        "SELECT sd.hostname, sd.container_port, active.id AS deployment_id, active.private_ipv4 \
         FROM nodes edge \
         JOIN projects p ON p.workspace_id = edge.workspace_id \
         JOIN services s ON s.project_id = p.id \
         JOIN service_domains sd ON sd.service_id = s.id \
         JOIN LATERAL ( \
             SELECT d.id, d.private_ipv4 \
             FROM deployments d \
             WHERE d.service_id = s.id \
               AND d.status = 'running' \
               AND d.private_ipv4 IS NOT NULL \
             ORDER BY d.updated_at DESC \
             LIMIT 1 \
         ) active ON TRUE \
         WHERE edge.id = $1 \
           AND edge.status = 'ready' \
           AND edge.public_ipv4 IS NOT NULL \
           AND edge.private_network_capable = TRUE \
         ORDER BY sd.hostname ASC",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(hostname, container_port, deployment_id, upstream_host)| NodeRoute {
                hostname,
                container_port: container_port as u16,
                upstream_host,
                deployment_id,
            },
        )
        .collect())
}

/// Return every public, private-network-capable node in the service workspace.
/// These are the edge nodes whose Caddy route table needs updating when a
/// domain changes or a deployment rolls over.
pub async fn nodes_for_service(pool: &DatabaseConnection, service_id: &str) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = crate::db::query_tuple(
        "SELECT n.id \
         FROM services s \
         JOIN projects p ON p.id = s.project_id \
         JOIN nodes n ON n.workspace_id = p.workspace_id \
         WHERE s.id = $1 \
           AND n.status = 'ready' \
           AND n.public_ipv4 IS NOT NULL \
           AND n.private_network_capable = TRUE \
         ORDER BY n.created_at ASC",
    )
    .bind(service_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(n,)| n).collect())
}

pub async fn edge_ips_for_service(
    pool: &DatabaseConnection,
    service_id: &str,
) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = crate::db::query_tuple(
        "SELECT n.public_ipv4 \
         FROM services s \
         JOIN projects p ON p.id = s.project_id \
         JOIN nodes n ON n.workspace_id = p.workspace_id \
         WHERE s.id = $1 \
           AND n.status = 'ready' \
           AND n.public_ipv4 IS NOT NULL \
           AND n.private_network_capable = TRUE \
         ORDER BY n.created_at ASC",
    )
    .bind(service_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(ip,)| ip).collect())
}

pub async fn edge_ips_for_workspace(
    pool: &DatabaseConnection,
    workspace_id: &str,
) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = crate::db::query_tuple(
        "SELECT public_ipv4 \
         FROM nodes \
         WHERE workspace_id = $1 \
           AND status = 'ready' \
           AND public_ipv4 IS NOT NULL \
           AND private_network_capable = TRUE \
         ORDER BY created_at ASC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(ip,)| ip).collect())
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

/// Probe a single domain right now and write the result back. Used by the
/// `retry` route so an admin doesn't have to wait for the next scheduler
/// tick to see whether their DNS fix took.
pub async fn probe_one(pool: &DatabaseConnection, domain_id: &str) -> Result<()> {
    let row: Option<(String, String, bool, bool)> = crate::db::query_tuple(
        "SELECT sd.hostname, \
                sd.service_id, \
                EXISTS(SELECT 1 FROM deployments d \
                       WHERE d.service_id = sd.service_id AND d.status = 'running') AS has_running, \
                EXISTS(SELECT 1 FROM deployments d \
                       WHERE d.service_id = sd.service_id \
                         AND d.status = 'running' \
                         AND d.private_ipv4 IS NOT NULL) AS has_private_running \
         FROM service_domains sd \
         WHERE sd.id = $1",
    )
    .bind(domain_id)
    .fetch_optional(pool)
    .await?;
    let Some((hostname, service_id, has_running, has_private_running)) = row else {
        return Ok(());
    };

    if !has_running {
        crate::db::query(
            "UPDATE service_domains SET \
                tls_status = 'pending', \
                last_error = NULL, \
                updated_at = now() \
             WHERE id = $1",
        )
        .bind(domain_id)
        .execute(pool)
        .await?;
        return Ok(());
    }

    if !has_private_running {
        mark_failed(
            pool,
            domain_id,
            "running deployment has no private network IP",
        )
        .await?;
        return Ok(());
    }

    let expected_ips = edge_ips_for_service(pool, &service_id).await?;
    if expected_ips.is_empty() {
        mark_failed(pool, domain_id, "no ready edge node public IP").await?;
        return Ok(());
    }

    apply_probe_result(pool, domain_id, &hostname, &expected_ips).await
}

async fn apply_probe_result(
    pool: &DatabaseConnection,
    domain_id: &str,
    hostname: &str,
    expected_ips: &[String],
) -> Result<()> {
    let resolved_ips = match resolve_hostname(hostname).await {
        Ok(ips) => ips,
        Err(err) => {
            mark_failed(pool, domain_id, &format!("DNS lookup failed: {err}")).await?;
            return Ok(());
        }
    };
    if resolved_ips.is_empty() {
        mark_failed(
            pool,
            domain_id,
            &format!(
                "DNS does not resolve yet; expected A record to {}",
                expected_ips.join(" or ")
            ),
        )
        .await?;
        return Ok(());
    }

    let matched_ip = resolved_ips
        .iter()
        .find(|ip| expected_ips.iter().any(|expected| expected == *ip));
    let Some(matched_ip) = matched_ip else {
        mark_failed(
            pool,
            domain_id,
            &format!(
                "DNS resolves to {}, but this workspace edge is {}",
                resolved_ips.join(", "),
                expected_ips.join(" or ")
            ),
        )
        .await?;
        return Ok(());
    };

    match probe_hostname(hostname, matched_ip).await {
        Ok(()) => {
            crate::db::query(
                "UPDATE service_domains SET \
                    tls_status = 'active', \
                    last_error = NULL, \
                    last_cert_at = COALESCE(last_cert_at, now()), \
                    updated_at = now() \
                 WHERE id = $1",
            )
            .bind(domain_id)
            .execute(pool)
            .await?;
        }
        Err(err) => mark_failed(pool, domain_id, &format!("HTTPS probe failed: {err}")).await?,
    }
    Ok(())
}

/// Refresh `service_domains.tls_status` by probing each hostname over HTTPS.
/// A domain is only considered probeable once its service has a running
/// deployment; until then it stays `pending`.
pub async fn refresh_tls_statuses(pool: &DatabaseConnection) -> Result<()> {
    let rows: Vec<(String, String, String, bool, bool)> = crate::db::query_tuple(
        "SELECT sd.id, sd.hostname, \
                sd.service_id, \
                EXISTS(SELECT 1 FROM deployments d \
                       WHERE d.service_id = sd.service_id AND d.status = 'running') AS has_running, \
                EXISTS(SELECT 1 FROM deployments d \
                       WHERE d.service_id = sd.service_id \
                         AND d.status = 'running' \
                         AND d.private_ipv4 IS NOT NULL) AS has_private_running \
         FROM service_domains sd \
         ORDER BY sd.created_at ASC",
    )
    .fetch_all(pool)
    .await?;

    for (id, hostname, service_id, has_running, has_private_running) in rows {
        if !has_running {
            crate::db::query(
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

        if !has_private_running {
            mark_failed(pool, &id, "running deployment has no private network IP").await?;
            continue;
        }

        let expected_ips = edge_ips_for_service(pool, &service_id).await?;
        if expected_ips.is_empty() {
            mark_failed(pool, &id, "no ready edge node public IP").await?;
            continue;
        }

        apply_probe_result(pool, &id, &hostname, &expected_ips).await?;
    }

    Ok(())
}

async fn mark_failed(pool: &DatabaseConnection, id: &str, err: &str) -> Result<()> {
    crate::db::query(
        "UPDATE service_domains SET \
            tls_status = 'failed', \
            last_error = $2, \
            updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(err)
    .execute(pool)
    .await?;
    Ok(())
}

async fn resolve_hostname(hostname: &str) -> Result<Vec<String>> {
    let addrs = lookup_host((hostname, 443)).await?;
    let mut uniq = BTreeSet::<String>::new();
    for addr in addrs {
        let ip: IpAddr = addr.ip();
        uniq.insert(ip.to_string());
    }
    Ok(uniq.into_iter().collect())
}

async fn probe_hostname(hostname: &str, expected_ip: &str) -> Result<()> {
    let expected_ip: IpAddr = expected_ip.parse()?;
    let http = reqwest::Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(5))
        .resolve(hostname, SocketAddr::new(expected_ip, 443))
        .build()?;
    let url = format!("https://{hostname}");
    let res = http.get(url).send().await?;
    // Any HTTP response means DNS + TCP + TLS + request routing worked.
    let _ = res.status();
    Ok(())
}
