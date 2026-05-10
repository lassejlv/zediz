pub mod routes;

use anyhow::Result;
use sea_orm::DatabaseConnection;

pub struct SshKeyForSync {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub fingerprint: String,
}

pub async fn list_for_sync(
    pool: &DatabaseConnection,
    workspace_id: &str,
) -> Result<Vec<SshKeyForSync>> {
    let rows: Vec<(String, String, String, String)> = crate::db::query_tuple(
        "SELECT id, name, public_key, fingerprint FROM ssh_keys WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, public_key, fingerprint)| SshKeyForSync {
            id,
            name,
            public_key,
            fingerprint,
        })
        .collect())
}

/// Sync every workspace SSH key to the Hetzner account, returning the Hetzner
/// key ids that succeeded. Individual failures are logged and skipped so a
/// single bad key never blocks provisioning.
pub async fn ensure_on_hetzner(
    pool: &DatabaseConnection,
    workspace_id: &str,
    hetzner_token: &str,
) -> Result<Vec<i64>> {
    let keys = list_for_sync(pool, workspace_id).await?;
    if keys.is_empty() {
        return Ok(vec![]);
    }
    let client = driftbase_hetzner::HetznerClient::new(hetzner_token);
    let mut out = Vec::with_capacity(keys.len());
    for k in keys {
        match client
            .ensure_ssh_key(&k.name, &k.public_key, &k.fingerprint)
            .await
        {
            Ok(id) => out.push(id),
            Err(e) => tracing::warn!(error = ?e, ssh_key = %k.id, "ensure ssh key on hetzner"),
        }
    }
    Ok(out)
}
