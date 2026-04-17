use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const API_BASE: &str = "https://api.hetzner.cloud/v1";

#[derive(Debug, Error)]
pub enum HetznerError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api error {status}: {message}")]
    Api { status: u16, message: String },
    #[error("action {id} failed: {error}")]
    ActionFailed { id: i64, error: String },
    #[error("timed out waiting for action {id}")]
    ActionTimeout { id: i64 },
}

#[derive(Clone)]
pub struct HetznerClient {
    http: Client,
    token: String,
}

impl HetznerClient {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            http: Client::builder()
                .user_agent(concat!("zediz/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
            token: token.into(),
        }
    }

    pub async fn ping(&self) -> Result<(), HetznerError> {
        self.get_json::<serde_json::Value>("/server_types?per_page=1")
            .await
            .map(|_| ())
    }

    pub async fn list_server_types(&self) -> Result<Vec<ServerType>, HetznerError> {
        let res: ServerTypesResponse = self.get_json("/server_types?per_page=50").await?;
        Ok(res.server_types)
    }

    pub async fn list_locations(&self) -> Result<Vec<Location>, HetznerError> {
        let res: LocationsResponse = self.get_json("/locations").await?;
        Ok(res.locations)
    }

    pub async fn list_ssh_keys(&self) -> Result<Vec<SshKey>, HetznerError> {
        let res: SshKeysResponse = self.get_json("/ssh_keys?per_page=50").await?;
        Ok(res.ssh_keys)
    }

    /// Find SSH key by fingerprint or public-key bytes, or upload it. Returns
    /// the Hetzner-side id. Tolerates two Hetzner uniqueness quirks:
    ///   * `name` collision: retry with a fingerprint-suffixed name.
    ///   * `public_key` collision: the key is already there under another
    ///     name (and our fingerprint format didn't match Hetzner's) — re-list
    ///     and return the existing id.
    pub async fn ensure_ssh_key(
        &self,
        name: &str,
        public_key: &str,
        fingerprint: &str,
    ) -> Result<i64, HetznerError> {
        let norm = normalize_public_key(public_key);

        let existing = self.list_ssh_keys().await?;
        if let Some(k) = existing.iter().find(|k| {
            k.fingerprint.eq_ignore_ascii_case(fingerprint)
                || normalize_public_key(&k.public_key) == norm
        }) {
            return Ok(k.id);
        }

        // Try with the user's name.
        match self.post_create_ssh_key(name, public_key).await {
            Ok(id) => return Ok(id),
            Err(HetznerError::Api { status: 409, .. }) => {}
            Err(e) => return Err(e),
        }

        // Retry with a suffixed name (handles name-only collisions).
        let suffix = fingerprint_suffix(fingerprint);
        let unique = format!("{name}-{suffix}");
        match self.post_create_ssh_key(&unique, public_key).await {
            Ok(id) => return Ok(id),
            Err(HetznerError::Api { status: 409, .. }) => {}
            Err(e) => return Err(e),
        }

        // Public-key bytes already exist under yet-another name — find it.
        let fresh = self.list_ssh_keys().await?;
        if let Some(k) = fresh
            .iter()
            .find(|k| normalize_public_key(&k.public_key) == norm)
        {
            return Ok(k.id);
        }

        Err(HetznerError::Api {
            status: 409,
            message: "SSH key conflicts with an existing Hetzner key but could not be located"
                .into(),
        })
    }

    async fn post_create_ssh_key(&self, name: &str, public_key: &str) -> Result<i64, HetznerError> {
        let body = serde_json::json!({
            "name": name,
            "public_key": public_key,
        });
        let created: CreateSshKeyResponse = self.post_json("/ssh_keys", &body).await?;
        Ok(created.ssh_key.id)
    }

    pub async fn create_server(
        &self,
        req: &CreateServerRequest<'_>,
    ) -> Result<CreateServerResponse, HetznerError> {
        self.post_json("/servers", req).await
    }

    pub async fn get_server(&self, id: i64) -> Result<Server, HetznerError> {
        let res: ServerResponse = self.get_json(&format!("/servers/{id}")).await?;
        Ok(res.server)
    }

    pub async fn delete_server(&self, id: i64) -> Result<Action, HetznerError> {
        let res = self
            .http
            .delete(format!("{API_BASE}/servers/{id}"))
            .bearer_auth(&self.token)
            .send()
            .await?;
        let status = res.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(Action {
                id: 0,
                status: "success".into(),
                error: None,
            });
        }
        let text = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(HetznerError::Api {
                status: status.as_u16(),
                message: text,
            });
        }
        let parsed: DeleteServerResponse =
            serde_json::from_str(&text).map_err(|e| HetznerError::Api {
                status: status.as_u16(),
                message: format!("decode: {e}: {text}"),
            })?;
        Ok(parsed.action)
    }

    pub async fn create_volume(
        &self,
        req: &CreateVolumeRequest<'_>,
    ) -> Result<CreateVolumeResponse, HetznerError> {
        self.post_json("/volumes", req).await
    }

    pub async fn get_volume(&self, id: i64) -> Result<Volume, HetznerError> {
        let res: VolumeResponse = self.get_json(&format!("/volumes/{id}")).await?;
        Ok(res.volume)
    }

    /// Idempotent: treats 404 as success so callers can retry safely.
    pub async fn delete_volume(&self, id: i64) -> Result<(), HetznerError> {
        let res = self
            .http
            .delete(format!("{API_BASE}/volumes/{id}"))
            .bearer_auth(&self.token)
            .send()
            .await?;
        let status = res.status();
        if status == StatusCode::NOT_FOUND || status.is_success() {
            return Ok(());
        }
        let text = res.text().await.unwrap_or_default();
        Err(HetznerError::Api {
            status: status.as_u16(),
            message: text,
        })
    }

    pub async fn attach_volume(
        &self,
        volume_id: i64,
        server_id: i64,
        automount: bool,
    ) -> Result<Action, HetznerError> {
        let body = serde_json::json!({
            "server": server_id,
            "automount": automount,
        });
        let res: ActionResponse = self
            .post_json(&format!("/volumes/{volume_id}/actions/attach"), &body)
            .await?;
        Ok(res.action)
    }

    pub async fn detach_volume(&self, volume_id: i64) -> Result<Action, HetznerError> {
        let body = serde_json::json!({});
        let res: ActionResponse = self
            .post_json(&format!("/volumes/{volume_id}/actions/detach"), &body)
            .await?;
        Ok(res.action)
    }

    pub async fn get_action(&self, id: i64) -> Result<Action, HetznerError> {
        let res: ActionResponse = self.get_json(&format!("/actions/{id}")).await?;
        Ok(res.action)
    }

    /// Polls an action until it succeeds, fails, or the deadline elapses.
    pub async fn wait_for_action(
        &self,
        id: i64,
        timeout: Duration,
    ) -> Result<Action, HetznerError> {
        let start = std::time::Instant::now();
        loop {
            let action = self.get_action(id).await?;
            match action.status.as_str() {
                "success" => return Ok(action),
                "error" => {
                    return Err(HetznerError::ActionFailed {
                        id,
                        error: action.error.unwrap_or_else(|| "unknown".into()),
                    });
                }
                _ => {}
            }
            if start.elapsed() >= timeout {
                return Err(HetznerError::ActionTimeout { id });
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, HetznerError> {
        let res = self
            .http
            .get(format!("{API_BASE}{path}"))
            .bearer_auth(&self.token)
            .send()
            .await?;
        self.parse(res).await
    }

    async fn post_json<B: Serialize + ?Sized, T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, HetznerError> {
        let res = self
            .http
            .post(format!("{API_BASE}{path}"))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?;
        self.parse(res).await
    }

    async fn parse<T: for<'de> Deserialize<'de>>(
        &self,
        res: reqwest::Response,
    ) -> Result<T, HetznerError> {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(HetznerError::Api {
                status: status.as_u16(),
                message: text,
            });
        }
        serde_json::from_str(&text).map_err(|e| HetznerError::Api {
            status: status.as_u16(),
            message: format!("decode: {e}: {text}"),
        })
    }
}

// ---------- types ----------

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerType {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub cores: u32,
    /// RAM in GiB (Hetzner returns a float).
    pub memory: f32,
    /// Disk in GB.
    pub disk: u32,
    #[serde(default)]
    pub prices: Vec<ServerTypePrice>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerTypePrice {
    pub location: String,
    pub price_hourly: Price,
    pub price_monthly: Price,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Price {
    pub net: String,
    pub gross: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Location {
    pub id: i64,
    pub name: String,
    pub country: String,
    pub city: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SshKey {
    pub id: i64,
    pub name: String,
    pub fingerprint: String,
    pub public_key: String,
}

#[derive(Debug, Serialize)]
pub struct CreateServerRequest<'a> {
    pub name: &'a str,
    pub server_type: &'a str,
    pub image: &'a str,
    pub location: &'a str,
    pub ssh_keys: Vec<i64>,
    pub user_data: &'a str,
    pub start_after_create: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateServerResponse {
    pub server: Server,
    pub action: Action,
    #[serde(default)]
    pub root_password: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Server {
    pub id: i64,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub public_net: PublicNet,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PublicNet {
    #[serde(default)]
    pub ipv4: Option<PublicNetIpv4>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PublicNetIpv4 {
    pub ip: String,
}

#[derive(Debug)]
pub struct Action {
    pub id: i64,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateVolumeRequest<'a> {
    pub name: &'a str,
    pub size: u32,
    pub location: &'a str,
    pub automount: bool,
    /// Tell Hetzner to run mkfs at volume creation so the agent just
    /// has to mount the block device at runtime. "ext4" or "xfs".
    pub format: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct CreateVolumeResponse {
    pub volume: Volume,
    pub action: Action,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Volume {
    pub id: i64,
    pub name: String,
    pub size: u32,
    pub status: String,
    /// Populated once attached.
    #[serde(default)]
    pub server: Option<i64>,
    /// `/dev/disk/by-id/scsi-0HC_Volume_<id>` — only set while attached.
    #[serde(default)]
    pub linux_device: Option<String>,
    pub location: Location,
}

#[derive(Deserialize)]
struct VolumeResponse {
    volume: Volume,
}

#[derive(Deserialize)]
struct ServerResponse {
    server: Server,
}

#[derive(Deserialize)]
struct DeleteServerResponse {
    action: Action,
}

#[derive(Deserialize)]
struct ActionResponse {
    action: Action,
}

#[derive(Deserialize)]
struct ServerTypesResponse {
    server_types: Vec<ServerType>,
}

#[derive(Deserialize)]
struct LocationsResponse {
    locations: Vec<Location>,
}

#[derive(Deserialize)]
struct SshKeysResponse {
    ssh_keys: Vec<SshKey>,
}

#[derive(Deserialize)]
struct CreateSshKeyResponse {
    ssh_key: SshKey,
}

impl<'de> Deserialize<'de> for Action {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Hetzner's "error" field is either null or an object `{code, message}`.
        #[derive(Deserialize)]
        struct Raw {
            id: i64,
            status: String,
            error: Option<RawError>,
        }
        #[derive(Deserialize)]
        struct RawError {
            #[serde(default)]
            code: Option<String>,
            #[serde(default)]
            message: Option<String>,
        }
        let raw = Raw::deserialize(d)?;
        let error = raw.error.map(|e| {
            format!(
                "{}: {}",
                e.code.unwrap_or_else(|| "error".into()),
                e.message.unwrap_or_default()
            )
        });
        Ok(Action {
            id: raw.id,
            status: raw.status,
            error,
        })
    }
}

impl Serialize for Action {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Action", 3)?;
        st.serialize_field("id", &self.id)?;
        st.serialize_field("status", &self.status)?;
        st.serialize_field("error", &self.error)?;
        st.end()
    }
}

/// Pick the cheapest server_type that meets the required resources in the given location.
/// Returns the server_type name (e.g. "cx22").
pub fn pick_server_type<'a>(
    server_types: &'a [ServerType],
    location: &str,
    required_cpu_millis: u32,
    required_memory_mb: u32,
    required_disk_mb: u32,
) -> Option<&'a ServerType> {
    let required_cores = (required_cpu_millis as f32 / 1000.0).ceil() as u32;
    let required_mem_gib = (required_memory_mb as f32 / 1024.0).ceil();
    let required_disk_gb = (required_disk_mb as f32 / 1024.0).ceil() as u32;

    let mut candidates: Vec<(&ServerType, f32)> = server_types
        .iter()
        .filter(|t| {
            t.cores >= required_cores.max(1)
                && t.memory >= required_mem_gib.max(0.5)
                && t.disk >= required_disk_gb.max(10)
                && t.prices.iter().any(|p| p.location == location)
        })
        .filter_map(|t| {
            let price = t.prices.iter().find(|p| p.location == location)?;
            let monthly: f32 = price.price_monthly.gross.parse().ok()?;
            Some((t, monthly))
        })
        .collect();

    candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    candidates.into_iter().next().map(|(t, _)| t)
}

fn fingerprint_suffix(fingerprint: &str) -> String {
    fingerprint
        .rsplit(':')
        .next()
        .unwrap_or(fingerprint)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect()
}

/// Reduce an OpenSSH public key to `"<alg> <base64-body>"` so we can compare
/// two strings byte-for-byte regardless of comment or trailing whitespace.
fn normalize_public_key(key: &str) -> String {
    let mut parts = key.split_whitespace();
    let alg = parts.next().unwrap_or("");
    let body = parts.next().unwrap_or("");
    format!("{alg} {body}")
}
