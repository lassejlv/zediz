use anyhow::{anyhow, Result};
use sea_orm::DatabaseConnection;
use serde_json::{json, Value as JsonValue};
use std::collections::BTreeSet;

use crate::agent::commands::{self, CommandKind};
use crate::error::{ApiError, ApiResult};

pub const DOMAIN: &str = "driftbase.internal";
const WG_INTERFACE: &str = "wg0";
const WG_LISTEN_PORT: i32 = 51820;

#[derive(Debug, Clone)]
pub struct DeploymentPrivateNetwork {
    pub network_name: String,
    pub node_subnet: String,
    pub gateway_ip: String,
    pub ip_address: String,
    pub dns_ip: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, sea_orm::FromQueryResult)]
struct ProjectNetworkRow {
    id: String,
    cidr: String,
}

#[derive(Debug, sea_orm::FromQueryResult)]
struct ProjectSubnetRow {
    cidr: String,
    gateway_ip: String,
    dns_ip: String,
}

#[derive(Debug, sea_orm::FromQueryResult)]
struct MeshNodeRow {
    id: String,
    public_ipv4: Option<String>,
    wireguard_public_key: Option<String>,
    wireguard_mesh_ip: Option<String>,
    wireguard_listen_port: i32,
}

#[derive(Debug, sea_orm::FromQueryResult)]
struct ProjectSyncRow {
    project_id: String,
    cidr: String,
    domain: String,
    node_subnet: String,
    gateway_ip: String,
    dns_ip: String,
}

#[derive(Debug, sea_orm::FromQueryResult)]
struct DnsRecordRow {
    service_slug: String,
    private_ipv4: String,
}

pub fn private_hostname(service_slug: &str) -> String {
    format!("{service_slug}.{DOMAIN}")
}

pub fn network_name(project_id: &str) -> String {
    format!("driftbase-pn-{}", project_id.to_ascii_lowercase())
}

pub fn coredns_container_name(project_id: &str) -> String {
    let short = project_id.chars().take(16).collect::<String>();
    format!("driftbase-dns-{}", short.to_ascii_lowercase())
}

pub async fn ensure_project_network(pool: &DatabaseConnection, project_id: &str) -> ApiResult<()> {
    ensure_project_network_row(pool, project_id)
        .await
        .map(|_| ())
}

pub async fn assign_node_mesh_ip(pool: &DatabaseConnection, node_id: &str) -> ApiResult<String> {
    if let Some((Some(ip),)) = crate::db::query_tuple::<(Option<String>,)>(
        "SELECT wireguard_mesh_ip FROM nodes WHERE id = $1",
    )
    .bind(node_id)
    .fetch_optional(pool)
    .await?
    {
        return Ok(ip);
    }

    let used: Vec<(String,)> = crate::db::query_tuple(
        "SELECT wireguard_mesh_ip FROM nodes WHERE wireguard_mesh_ip IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    let used: BTreeSet<String> = used.into_iter().map(|(ip,)| ip).collect();

    for idx in 1..=65_000u32 {
        let ip = mesh_ip_for_index(idx);
        if used.contains(&ip) {
            continue;
        }
        let res = crate::db::query(
            "UPDATE nodes \
             SET wireguard_mesh_ip = COALESCE(wireguard_mesh_ip, $1), \
                 wireguard_listen_port = COALESCE(wireguard_listen_port, $2) \
             WHERE id = $3",
        )
        .bind(&ip)
        .bind(WG_LISTEN_PORT)
        .bind(node_id)
        .execute(pool)
        .await;
        match res {
            Ok(_) => return Ok(ip),
            Err(e) if crate::db::is_unique_violation(&e) => continue,
            Err(e) => return Err(ApiError::Db(e)),
        }
    }

    Err(ApiError::Internal(anyhow!(
        "wireguard mesh IP pool exhausted"
    )))
}

pub async fn ensure_deployment_private_network(
    pool: &DatabaseConnection,
    project_id: &str,
    node_id: &str,
    service_slug: &str,
    deployment_id: &str,
) -> ApiResult<DeploymentPrivateNetwork> {
    let project = ensure_project_network_row(pool, project_id).await?;
    let subnet = ensure_node_subnet(pool, &project, node_id).await?;
    let ip_address = ensure_deployment_ip(pool, deployment_id, &subnet.cidr).await?;
    Ok(DeploymentPrivateNetwork {
        network_name: network_name(project_id),
        node_subnet: subnet.cidr,
        gateway_ip: subnet.gateway_ip,
        ip_address,
        dns_ip: subnet.dns_ip,
        aliases: vec![service_slug.to_string(), private_hostname(service_slug)],
    })
}

pub async fn release_deployment_ip(
    pool: &DatabaseConnection,
    deployment_id: &str,
) -> ApiResult<()> {
    crate::db::query("UPDATE deployments SET private_ipv4 = NULL WHERE id = $1")
        .bind(deployment_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn sync_for_service(pool: &DatabaseConnection, service_id: &str) -> Result<()> {
    let row: Option<(String,)> =
        crate::db::query_tuple("SELECT project_id FROM services WHERE id = $1")
            .bind(service_id)
            .fetch_optional(pool)
            .await?;
    if let Some((project_id,)) = row {
        sync_project(pool, &project_id).await?;
    }
    Ok(())
}

pub async fn sync_project(pool: &DatabaseConnection, project_id: &str) -> Result<()> {
    let row: Option<(String,)> =
        crate::db::query_tuple("SELECT workspace_id FROM projects WHERE id = $1")
            .bind(project_id)
            .fetch_optional(pool)
            .await?;
    if let Some((workspace_id,)) = row {
        sync_workspace(pool, &workspace_id).await?;
    }
    Ok(())
}

pub async fn sync_workspace(pool: &DatabaseConnection, workspace_id: &str) -> Result<()> {
    let nodes = mesh_nodes(pool, workspace_id).await?;
    for node in &nodes {
        let payload = build_sync_payload(pool, workspace_id, node, &nodes).await?;
        commands::enqueue_coalesced(
            pool,
            &node.id,
            None,
            CommandKind::SyncPrivateNetwork,
            payload,
        )
        .await?;
    }
    Ok(())
}

async fn ensure_project_network_row(
    pool: &DatabaseConnection,
    project_id: &str,
) -> ApiResult<ProjectNetworkRow> {
    if let Some(row) = fetch_project_network(pool, project_id).await? {
        return Ok(row);
    }

    let cidr = next_project_cidr(pool).await?;
    let inserted = crate::db::query_as::<ProjectNetworkRow>(
        "INSERT INTO project_networks (id, project_id, cidr, domain) \
         VALUES ($1, $1, $2, $3) \
         ON CONFLICT (project_id) DO NOTHING \
         RETURNING id, cidr",
    )
    .bind(project_id)
    .bind(&cidr)
    .bind(DOMAIN)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = inserted {
        return Ok(row);
    }
    fetch_project_network(pool, project_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow!("project network was not created")))
}

async fn fetch_project_network(
    pool: &DatabaseConnection,
    project_id: &str,
) -> ApiResult<Option<ProjectNetworkRow>> {
    let row = crate::db::query_as::<ProjectNetworkRow>(
        "SELECT id, cidr FROM project_networks WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

async fn next_project_cidr(pool: &DatabaseConnection) -> ApiResult<String> {
    let used: Vec<(String,)> = crate::db::query_tuple("SELECT cidr FROM project_networks")
        .fetch_all(pool)
        .await?;
    let used: BTreeSet<String> = used.into_iter().map(|(cidr,)| cidr).collect();
    for idx in 0..191u16 {
        let cidr = project_cidr_for_index(idx);
        if !used.contains(&cidr) {
            return Ok(cidr);
        }
    }
    Err(ApiError::Internal(anyhow!(
        "project private network address pool exhausted"
    )))
}

async fn ensure_node_subnet(
    pool: &DatabaseConnection,
    project: &ProjectNetworkRow,
    node_id: &str,
) -> ApiResult<ProjectSubnetRow> {
    let existing = crate::db::query_as::<ProjectSubnetRow>(
        "SELECT cidr, gateway_ip, dns_ip \
         FROM project_network_node_subnets \
         WHERE project_network_id = $1 AND node_id = $2",
    )
    .bind(&project.id)
    .bind(node_id)
    .fetch_optional(pool)
    .await?;
    if let Some(row) = existing {
        return Ok(row);
    }

    let used: Vec<(String,)> = crate::db::query_tuple(
        "SELECT cidr FROM project_network_node_subnets WHERE project_network_id = $1",
    )
    .bind(&project.id)
    .fetch_all(pool)
    .await?;
    let used: BTreeSet<String> = used.into_iter().map(|(cidr,)| cidr).collect();

    for idx in 1..=254u8 {
        let (cidr, gateway_ip, dns_ip) = node_subnet_for_index(&project.cidr, idx)?;
        if used.contains(&cidr) {
            continue;
        }
        let inserted = crate::db::query_as::<ProjectSubnetRow>(
            "INSERT INTO project_network_node_subnets \
                (project_network_id, node_id, cidr, gateway_ip, dns_ip) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (project_network_id, node_id) DO NOTHING \
             RETURNING cidr, gateway_ip, dns_ip",
        )
        .bind(&project.id)
        .bind(node_id)
        .bind(&cidr)
        .bind(&gateway_ip)
        .bind(&dns_ip)
        .fetch_optional(pool)
        .await;
        match inserted {
            Ok(Some(row)) => return Ok(row),
            Ok(None) => {
                if let Some(row) = crate::db::query_as::<ProjectSubnetRow>(
                    "SELECT cidr, gateway_ip, dns_ip \
                     FROM project_network_node_subnets \
                     WHERE project_network_id = $1 AND node_id = $2",
                )
                .bind(&project.id)
                .bind(node_id)
                .fetch_optional(pool)
                .await?
                {
                    return Ok(row);
                }
            }
            Err(e) if crate::db::is_unique_violation(&e) => continue,
            Err(e) => return Err(ApiError::Db(e)),
        }
    }

    Err(ApiError::Internal(anyhow!(
        "project private network node subnet pool exhausted"
    )))
}

async fn ensure_deployment_ip(
    pool: &DatabaseConnection,
    deployment_id: &str,
    subnet_cidr: &str,
) -> ApiResult<String> {
    if let Some((Some(ip),)) = crate::db::query_tuple::<(Option<String>,)>(
        "SELECT private_ipv4 FROM deployments WHERE id = $1",
    )
    .bind(deployment_id)
    .fetch_optional(pool)
    .await?
    {
        return Ok(ip);
    }

    let base = subnet_base(subnet_cidr)?;
    let prefix = format!("{}.{}.{}.", base[0], base[1], base[2]);
    let used: Vec<(String,)> =
        crate::db::query_tuple("SELECT private_ipv4 FROM deployments WHERE private_ipv4 LIKE $1")
            .bind(format!("{prefix}%"))
            .fetch_all(pool)
            .await?;
    let used: BTreeSet<String> = used.into_iter().map(|(ip,)| ip).collect();

    for host in 10..=254u8 {
        let ip = format!("{prefix}{host}");
        if used.contains(&ip) {
            continue;
        }
        let res = crate::db::query(
            "UPDATE deployments \
             SET private_ipv4 = COALESCE(private_ipv4, $1) \
             WHERE id = $2",
        )
        .bind(&ip)
        .bind(deployment_id)
        .execute(pool)
        .await;
        match res {
            Ok(_) => return Ok(ip),
            Err(e) if crate::db::is_unique_violation(&e) => continue,
            Err(e) => return Err(ApiError::Db(e)),
        }
    }

    Err(ApiError::Internal(anyhow!(
        "deployment private IP pool exhausted"
    )))
}

async fn mesh_nodes(pool: &DatabaseConnection, workspace_id: &str) -> Result<Vec<MeshNodeRow>> {
    let rows = crate::db::query_as::<MeshNodeRow>(
        "SELECT id, public_ipv4, wireguard_public_key, wireguard_mesh_ip, wireguard_listen_port \
         FROM nodes \
         WHERE workspace_id = $1 \
           AND status = 'ready' \
           AND private_network_capable = TRUE \
           AND wireguard_public_key IS NOT NULL \
           AND wireguard_mesh_ip IS NOT NULL \
         ORDER BY created_at ASC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn build_sync_payload(
    pool: &DatabaseConnection,
    workspace_id: &str,
    target: &MeshNodeRow,
    nodes: &[MeshNodeRow],
) -> Result<JsonValue> {
    let subnet_rows: Vec<(String, String)> =
        crate::db::query_tuple("SELECT node_id, cidr FROM project_network_node_subnets")
            .fetch_all(pool)
            .await?;

    let peers = nodes
        .iter()
        .filter(|node| node.id != target.id)
        .filter_map(|node| {
            let public_key = node.wireguard_public_key.as_deref()?;
            let mesh_ip = node.wireguard_mesh_ip.as_deref()?;
            let public_ipv4 = node.public_ipv4.as_deref()?;
            let mut allowed_ips = vec![format!("{mesh_ip}/32")];
            allowed_ips.extend(
                subnet_rows
                    .iter()
                    .filter(|(node_id, _)| node_id == &node.id)
                    .map(|(_, cidr)| cidr.clone()),
            );
            Some(json!({
                "node_id": node.id,
                "public_key": public_key,
                "endpoint": format!("{public_ipv4}:{}", node.wireguard_listen_port),
                "allowed_ips": allowed_ips,
                "persistent_keepalive_seconds": 25,
            }))
        })
        .collect::<Vec<_>>();

    let projects: Vec<ProjectSyncRow> = crate::db::query_as(
        "SELECT p.id AS project_id, pn.cidr, pn.domain, \
                pns.cidr AS node_subnet, pns.gateway_ip, pns.dns_ip \
         FROM project_network_node_subnets pns \
         JOIN project_networks pn ON pn.id = pns.project_network_id \
         JOIN projects p ON p.id = pn.project_id \
         WHERE p.workspace_id = $1 AND pns.node_id = $2 \
         ORDER BY p.created_at ASC",
    )
    .bind(workspace_id)
    .bind(&target.id)
    .fetch_all(pool)
    .await?;

    let mut project_payloads = Vec::with_capacity(projects.len());
    for project in projects {
        let records: Vec<DnsRecordRow> = crate::db::query_as(
            "SELECT s.slug AS service_slug, d.private_ipv4 \
             FROM deployments d \
             JOIN services s ON s.id = d.service_id \
             WHERE s.project_id = $1 \
               AND d.status = 'running' \
               AND d.private_ipv4 IS NOT NULL \
             ORDER BY s.slug ASC, d.created_at DESC",
        )
        .bind(&project.project_id)
        .fetch_all(pool)
        .await?;

        project_payloads.push(json!({
            "project_id": project.project_id,
            "network_name": network_name(&project.project_id),
            "cidr": project.cidr,
            "node_subnet": project.node_subnet,
            "gateway_ip": project.gateway_ip,
            "dns_ip": project.dns_ip,
            "domain": project.domain,
            "dns_container_name": coredns_container_name(&project.project_id),
            "records": records.into_iter().map(|r| json!({
                "name": r.service_slug,
                "fqdn": private_hostname(&r.service_slug),
                "ip": r.private_ipv4,
            })).collect::<Vec<_>>(),
        }));
    }

    Ok(json!({
        "interface": {
            "name": WG_INTERFACE,
            "address": target.wireguard_mesh_ip,
            "listen_port": target.wireguard_listen_port,
        },
        "peers": peers,
        "projects": project_payloads,
    }))
}

fn mesh_ip_for_index(idx: u32) -> String {
    let host = ((idx - 1) % 254) + 1;
    let third = ((idx - 1) / 254) % 256;
    format!("10.255.{third}.{host}")
}

fn project_cidr_for_index(idx: u16) -> String {
    format!("10.{}.0.0/16", 64 + idx)
}

fn node_subnet_for_index(project_cidr: &str, idx: u8) -> ApiResult<(String, String, String)> {
    let base = subnet_base(project_cidr)?;
    let cidr = format!("{}.{}.{}.0/24", base[0], base[1], idx);
    let gateway_ip = format!("{}.{}.{}.1", base[0], base[1], idx);
    let dns_ip = format!("{}.{}.{}.2", base[0], base[1], idx);
    Ok((cidr, gateway_ip, dns_ip))
}

fn subnet_base(cidr: &str) -> ApiResult<[u8; 4]> {
    let addr = cidr.split_once('/').map(|(addr, _)| addr).unwrap_or(cidr);
    let parts: Vec<u8> = addr
        .split('.')
        .map(str::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::Internal(anyhow!("invalid CIDR {cidr}: {e}")))?;
    let [a, b, c, d]: [u8; 4] = parts
        .try_into()
        .map_err(|_| ApiError::Internal(anyhow!("invalid CIDR {cidr}")))?;
    Ok([a, b, c, d])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_cidr_allocation_uses_private_pool() {
        assert_eq!(project_cidr_for_index(0), "10.64.0.0/16");
        assert_eq!(project_cidr_for_index(2), "10.66.0.0/16");
    }

    #[test]
    fn node_subnet_allocation_uses_project_second_octet() {
        let (cidr, gateway, dns) = node_subnet_for_index("10.72.0.0/16", 3).unwrap();
        assert_eq!(cidr, "10.72.3.0/24");
        assert_eq!(gateway, "10.72.3.1");
        assert_eq!(dns, "10.72.3.2");
    }

    #[test]
    fn dns_names_are_project_scoped_domain_names() {
        assert_eq!(private_hostname("api"), "api.driftbase.internal");
    }
}
