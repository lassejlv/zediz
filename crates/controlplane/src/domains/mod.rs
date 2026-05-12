pub mod routes;

use anyhow::Result;
use reqwest::redirect::Policy;
use sea_orm::DatabaseConnection;
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::lookup_host;

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
pub async fn routes_for_node(pool: &DatabaseConnection, node_id: &str) -> Result<Vec<NodeRoute>> {
    let rows: Vec<(String, i32, String)> = crate::db::query_tuple(
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
            container_name: format!("driftbase-{deployment_id}"),
            deployment_id,
        })
        .collect())
}

/// Return every node currently hosting a running deployment for the given
/// service. These are the nodes whose Caddy needs updating when a domain on
/// this service changes.
pub async fn nodes_for_service(pool: &DatabaseConnection, service_id: &str) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = crate::db::query_tuple(
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

/// Probe a single domain right now and write the result back. Used by the
/// `retry` route so an admin doesn't have to wait for the next scheduler
/// tick to see whether their DNS fix took.
pub async fn probe_one(pool: &DatabaseConnection, domain_id: &str) -> Result<()> {
    let row: Option<(String, bool, Option<String>)> = crate::db::query_tuple(
        "SELECT sd.hostname, \
                EXISTS(SELECT 1 FROM deployments d \
                       WHERE d.service_id = sd.service_id AND d.status = 'running') AS has_running, \
                (SELECT n.public_ipv4 \
                 FROM deployments d \
                 JOIN nodes n ON n.id = d.node_id \
                 WHERE d.service_id = sd.service_id \
                       AND d.status = 'running' \
                       AND n.public_ipv4 IS NOT NULL \
                 ORDER BY d.updated_at DESC \
                 LIMIT 1) AS expected_ip \
         FROM service_domains sd \
         WHERE sd.id = $1",
    )
    .bind(domain_id)
    .fetch_optional(pool)
    .await?;
    let Some((hostname, has_running, expected_ip)) = row else {
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

    let Some(expected_ip) = expected_ip else {
        mark_failed(pool, domain_id, "running deployment has no public node IP").await?;
        return Ok(());
    };

    apply_probe_result(pool, domain_id, &hostname, &expected_ip).await
}

async fn apply_probe_result(
    pool: &DatabaseConnection,
    domain_id: &str,
    hostname: &str,
    expected_ip: &str,
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
            &format!("DNS does not resolve yet; expected A record to {expected_ip}"),
        )
        .await?;
        return Ok(());
    }

    if !resolved_ips.iter().any(|ip| ip == expected_ip) {
        mark_failed(
            pool,
            domain_id,
            &format!(
                "DNS resolves to {}, but this service is running on {expected_ip}",
                resolved_ips.join(", ")
            ),
        )
        .await?;
        return Ok(());
    }

    match probe_hostname(hostname, expected_ip).await {
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
    let rows: Vec<(String, String, bool, Option<String>)> = crate::db::query_tuple(
        "SELECT sd.id, sd.hostname, \
                EXISTS(SELECT 1 FROM deployments d \
                       WHERE d.service_id = sd.service_id AND d.status = 'running') AS has_running, \
                (SELECT n.public_ipv4 \
                 FROM deployments d \
                 JOIN nodes n ON n.id = d.node_id \
                 WHERE d.service_id = sd.service_id \
                       AND d.status = 'running' \
                       AND n.public_ipv4 IS NOT NULL \
                 ORDER BY d.updated_at DESC \
                 LIMIT 1) AS expected_ip \
         FROM service_domains sd \
         ORDER BY sd.created_at ASC",
    )
    .fetch_all(pool)
    .await?;

    for (id, hostname, has_running, expected_ip) in rows {
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

        let expected_ip = match expected_ip {
            Some(ip) => ip,
            None => {
                mark_failed(pool, &id, "running deployment has no public node IP").await?;
                continue;
            }
        };

        apply_probe_result(pool, &id, &hostname, &expected_ip).await?;
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
