use anyhow::{anyhow, Context, Result};
use driftbase_common::Id;
use driftbase_hetzner::{pick_server_type, CreateServerRequest, HetznerClient, ServerType};
use sea_orm::DatabaseConnection;
use std::time::Duration;

use crate::agent::tokens;
use crate::config::Config;
use crate::crypto::MasterKey;
use crate::provisioner::cloud_init;
use crate::services::Resources;

/// Default Hetzner image; cloud-init inside installs docker + agent.
const DEFAULT_IMAGE: &str = "debian-12";

/// Minimum headroom multiplier: provisioned node must fit 120% of need.
const HEADROOM: f32 = 1.2;

pub struct ProvisionResult {
    pub node_id: Id,
    pub hetzner_server_id: i64,
}

/// How the caller wants the server type chosen.
pub enum NodeSize<'a> {
    /// Pick the cheapest server type that fits `need` + HEADROOM.
    Fit(&'a Resources),
    /// Use a named server type verbatim (e.g. "cx22").
    Explicit(&'a str),
}

/// Provision a new Hetzner VM, insert a `provisioning` node row, kick off the
/// create action, and return identifiers. Agent registration flips it to `ready`.
#[allow(clippy::too_many_arguments)]
pub async fn provision(
    pool: &DatabaseConnection,
    config: &Config,
    master_key: &MasterKey,
    hetzner_token: &str,
    workspace_id: &str,
    location: &str,
    size: NodeSize<'_>,
    ssh_key_ids: Vec<i64>,
) -> Result<ProvisionResult> {
    let client = HetznerClient::new(hetzner_token);
    let types = client
        .list_server_types()
        .await
        .context("listing Hetzner server types")?;

    let st: ServerType = match size {
        NodeSize::Fit(need) => {
            let cpu_need = (need.cpu_millis as f32 * HEADROOM) as u32;
            let mem_need = (need.memory_mb as f32 * HEADROOM) as u32;
            let disk_need = (need.disk_mb as f32 * HEADROOM) as u32;
            let picked = pick_server_type(&types, location, cpu_need, mem_need, disk_need)
                .ok_or_else(|| {
                    anyhow!("no Hetzner server type fits requested resources in {location}")
                })?;
            ServerType {
                id: picked.id,
                name: picked.name.clone(),
                description: picked.description.clone(),
                cores: picked.cores,
                memory: picked.memory,
                disk: picked.disk,
                prices: Vec::new(),
            }
        }
        NodeSize::Explicit(name) => {
            let found = types
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| anyhow!("unknown Hetzner server type: {name}"))?;
            if !found.prices.iter().any(|p| p.location == location) {
                return Err(anyhow!("server type {name} is not available in {location}"));
            }
            ServerType {
                id: found.id,
                name: found.name.clone(),
                description: found.description.clone(),
                cores: found.cores,
                memory: found.memory,
                disk: found.disk,
                prices: Vec::new(),
            }
        }
    };

    let node_id = Id::new();
    let bootstrap = tokens::mint_bootstrap(master_key, &node_id.to_string(), workspace_id)
        .context("minting bootstrap token")?;

    let total_cpu_millis = (st.cores * 1000) as i32;
    let total_memory_mb = (st.memory * 1024.0) as i32;
    let total_disk_mb = (st.disk * 1024) as i32;

    let name = format!("driftbase-{}", &node_id.to_string()[..8]);
    let user_data = cloud_init::render(
        &config.public_url,
        &bootstrap,
        &config.agent_image,
        &node_id.to_string(),
        workspace_id,
    );

    crate::db::query(
        "INSERT INTO nodes (id, workspace_id, name, provider, status, \
                            total_cpu_millis, total_memory_mb, total_disk_mb, \
                            bootstrap_token_hash, hetzner_location, hetzner_server_type) \
         VALUES ($1, $2, $3, 'hetzner', 'provisioning', $4, $5, $6, $7, $8, $9)",
    )
    .bind(node_id.to_string())
    .bind(workspace_id)
    .bind(&name)
    .bind(total_cpu_millis)
    .bind(total_memory_mb)
    .bind(total_disk_mb)
    .bind(tokens::fingerprint(&bootstrap))
    .bind(location)
    .bind(&st.name)
    .execute(pool)
    .await?;

    let req = CreateServerRequest {
        name: &name,
        server_type: &st.name,
        image: DEFAULT_IMAGE,
        location,
        ssh_keys: ssh_key_ids,
        user_data: &user_data,
        start_after_create: true,
        labels: Some(serde_json::json!({
            "driftbase.workspace_id": workspace_id,
            "driftbase.node_id": node_id.to_string(),
        })),
    };

    let created = match client.create_server(&req).await {
        Ok(r) => r,
        Err(e) => {
            crate::db::query("DELETE FROM nodes WHERE id = $1")
                .bind(node_id.to_string())
                .execute(pool)
                .await
                .ok();
            return Err(anyhow!("hetzner create_server: {e}"));
        }
    };

    let public_ipv4 = created
        .server
        .public_net
        .ipv4
        .as_ref()
        .map(|v| v.ip.clone());

    crate::db::query("UPDATE nodes SET hetzner_server_id = $1, public_ipv4 = $2 WHERE id = $3")
        .bind(created.server.id)
        .bind(public_ipv4.as_deref())
        .bind(node_id.to_string())
        .execute(pool)
        .await?;

    if created.action.id > 0 {
        let client_for_bg = client.clone();
        let action_id = created.action.id;
        tokio::spawn(async move {
            if let Err(e) = client_for_bg
                .wait_for_action(action_id, Duration::from_secs(120))
                .await
            {
                tracing::warn!(action = action_id, error = ?e, "hetzner create action");
            }
        });
    }

    Ok(ProvisionResult {
        node_id,
        hetzner_server_id: created.server.id,
    })
}

/// Tear down a Hetzner node. Marks it terminated first (so the scheduler stops
/// dispatching to it), then deletes the VM. On Hetzner success the row is
/// removed entirely. On Hetzner failure the tombstone remains and the caller
/// gets an error to retry.
pub async fn terminate(
    pool: &DatabaseConnection,
    hetzner_token: &str,
    node_id: &str,
    hetzner_server_id: i64,
) -> Result<()> {
    // Stop scheduling to this node immediately.
    crate::db::query(
        "UPDATE nodes SET status = 'terminated', node_token_hash = NULL WHERE id = $1",
    )
    .bind(node_id)
    .execute(pool)
    .await?;
    crate::db::query("DELETE FROM node_allocations WHERE node_id = $1")
        .bind(node_id)
        .execute(pool)
        .await?;

    let client = HetznerClient::new(hetzner_token);
    // delete_server returns Ok for 404 (already gone), so a real error here
    // is worth surfacing — the VM may still exist and could bill.
    client
        .delete_server(hetzner_server_id)
        .await
        .map_err(|e| anyhow!("hetzner delete_server {hetzner_server_id}: {e}"))?;

    // VM confirmed gone — drop the row.
    crate::db::query("DELETE FROM nodes WHERE id = $1")
        .bind(node_id)
        .execute(pool)
        .await?;
    Ok(())
}
