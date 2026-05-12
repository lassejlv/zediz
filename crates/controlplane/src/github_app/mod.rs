pub mod routes;

use anyhow::{anyhow, Context, Result};
use base64::{
    engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD},
    Engine,
};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use rand::RngCore;
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use sha2::Sha256;

use crate::config::{Config, GitHubAppConfig};
use crate::crypto::MasterKey;
use crate::error::{ApiError, ApiResult};

const GITHUB_API: &str = "https://api.github.com";
const GITHUB_WEB: &str = "https://github.com";
const API_VERSION: &str = "2022-11-28";
const STATE_TTL_SECONDS: i64 = 15 * 60;

#[derive(Debug, Clone, Serialize)]
pub struct GitHubInstallationSummary {
    pub installation_id: i64,
    pub account_login: String,
    pub account_type: String,
    pub repository_selection: String,
    pub active: bool,
    pub html_url: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitHubRepositorySummary {
    pub installation_id: i64,
    pub repository_id: i64,
    pub full_name: String,
    pub private: bool,
    pub default_branch: String,
    pub clone_url: String,
    pub html_url: String,
    pub archived: bool,
    pub disabled: bool,
    pub pushed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sea_orm::FromQueryResult)]
struct InstallationRow {
    installation_id: i64,
    account_login: String,
    account_type: String,
    repository_selection: String,
    active: bool,
    html_url: Option<String>,
    updated_at: DateTime<Utc>,
}

#[derive(sea_orm::FromQueryResult)]
struct RepositoryRow {
    installation_id: i64,
    repository_id: i64,
    full_name: String,
    private: bool,
    default_branch: String,
    clone_url: String,
    html_url: String,
    archived: bool,
    disabled: bool,
    pushed_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

impl From<InstallationRow> for GitHubInstallationSummary {
    fn from(row: InstallationRow) -> Self {
        Self {
            installation_id: row.installation_id,
            account_login: row.account_login,
            account_type: row.account_type,
            repository_selection: row.repository_selection,
            active: row.active,
            html_url: row.html_url,
            updated_at: row.updated_at,
        }
    }
}

impl From<RepositoryRow> for GitHubRepositorySummary {
    fn from(row: RepositoryRow) -> Self {
        Self {
            installation_id: row.installation_id,
            repository_id: row.repository_id,
            full_name: row.full_name,
            private: row.private,
            default_branch: row.default_branch,
            clone_url: row.clone_url,
            html_url: row.html_url,
            archived: row.archived,
            disabled: row.disabled,
            pushed_at: row.pushed_at,
            updated_at: row.updated_at,
        }
    }
}

pub fn require_config(config: &Config) -> ApiResult<&GitHubAppConfig> {
    config.github_app.as_ref().ok_or_else(|| {
        ApiError::Validation("GitHub App is not configured on this Driftbase instance".into())
    })
}

pub fn install_url(config: &GitHubAppConfig, state: &str) -> String {
    format!(
        "{GITHUB_WEB}/apps/{}/installations/select_target?state={}",
        percent_encode(&config.slug),
        percent_encode(state)
    )
}

pub fn sign_state(master_key: &MasterKey, workspace_id: &str, slug: &str, user_id: &str) -> String {
    let mut nonce = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut nonce);
    let payload = format!(
        "{}|{}|{}|{}|{}",
        workspace_id,
        slug,
        user_id,
        Utc::now().timestamp(),
        B64.encode(nonce)
    );
    let key = master_key.derive(b"github-app-state");
    let sig = hmac_sha256(&key, payload.as_bytes());
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(payload),
        URL_SAFE_NO_PAD.encode(sig)
    )
}

pub fn verify_state(master_key: &MasterKey, state: &str) -> ApiResult<VerifiedState> {
    let (payload_b64, sig_b64) = state
        .split_once('.')
        .ok_or_else(|| ApiError::Validation("invalid GitHub state".into()))?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| ApiError::Validation("invalid GitHub state".into()))?;
    let sig = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|_| ApiError::Validation("invalid GitHub state".into()))?;
    let key = master_key.derive(b"github-app-state");
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("hmac key");
    mac.update(&payload);
    mac.verify_slice(&sig)
        .map_err(|_| ApiError::Validation("invalid GitHub state".into()))?;

    let payload = String::from_utf8(payload)
        .map_err(|_| ApiError::Validation("invalid GitHub state".into()))?;
    let parts: Vec<&str> = payload.split('|').collect();
    if parts.len() != 5 {
        return Err(ApiError::Validation("invalid GitHub state".into()));
    }
    let issued_at = parts[3]
        .parse::<i64>()
        .map_err(|_| ApiError::Validation("invalid GitHub state".into()))?;
    if Utc::now().timestamp() - issued_at > STATE_TTL_SECONDS {
        return Err(ApiError::Validation("GitHub state expired".into()));
    }
    Ok(VerifiedState {
        workspace_id: parts[0].to_string(),
        workspace_slug: parts[1].to_string(),
        user_id: parts[2].to_string(),
    })
}

pub struct VerifiedState {
    pub workspace_id: String,
    pub workspace_slug: String,
    pub user_id: String,
}

#[derive(Serialize)]
struct JwtClaims {
    iat: i64,
    exp: i64,
    iss: String,
}

fn app_jwt(config: &GitHubAppConfig) -> Result<String> {
    let now = Utc::now().timestamp();
    let claims = JwtClaims {
        iat: now - 60,
        exp: now + 9 * 60,
        iss: config.app_id.to_string(),
    };
    let key = EncodingKey::from_rsa_pem(config.private_key.as_bytes())
        .context("loading GitHub App private key")?;
    let mut header = Header::new(Algorithm::RS256);
    header.typ = Some("JWT".into());
    encode(&header, &claims, &key).context("signing GitHub App JWT")
}

fn github_client() -> reqwest::Client {
    reqwest::Client::new()
}

fn github_get(client: &reqwest::Client, url: String, token: &str) -> reqwest::RequestBuilder {
    client
        .get(url)
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", API_VERSION)
        .header(USER_AGENT, "driftbase-controlplane")
        .header(AUTHORIZATION, format!("Bearer {token}"))
}

fn github_post(client: &reqwest::Client, url: String, token: &str) -> reqwest::RequestBuilder {
    client
        .post(url)
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", API_VERSION)
        .header(USER_AGENT, "driftbase-controlplane")
        .header(AUTHORIZATION, format!("Bearer {token}"))
}

#[derive(Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
}

pub async fn exchange_oauth_code(config: &GitHubAppConfig, code: &str) -> Result<OAuthToken> {
    let client = github_client();
    let res = client
        .post(format!("{GITHUB_WEB}/login/oauth/access_token"))
        .header(ACCEPT, "application/json")
        .header(USER_AGENT, "driftbase-controlplane")
        .form(&[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("code", code),
        ])
        .send()
        .await
        .context("exchanging GitHub OAuth code")?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("GitHub OAuth exchange failed ({status}): {body}"));
    }
    res.json().await.context("decoding GitHub OAuth response")
}

#[derive(Debug, Deserialize)]
pub struct GitHubAccount {
    pub id: i64,
    pub login: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Deserialize)]
pub struct GitHubInstallation {
    pub id: i64,
    pub account: GitHubAccount,
    pub repository_selection: String,
    #[serde(default)]
    pub permissions: JsonValue,
    #[serde(default)]
    pub events: JsonValue,
    pub html_url: Option<String>,
    pub suspended_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct InstallationsPage {
    installations: Vec<GitHubInstallation>,
}

pub async fn user_installations(user_token: &str) -> Result<Vec<GitHubInstallation>> {
    let client = github_client();
    let mut page = 1;
    let mut out = Vec::new();
    loop {
        let res = github_get(
            &client,
            format!("{GITHUB_API}/user/installations?per_page=100&page={page}"),
            user_token,
        )
        .send()
        .await
        .context("listing GitHub App installations for user")?;
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!(
                "GitHub installations request failed ({status}): {body}"
            ));
        }
        let body: InstallationsPage = res.json().await?;
        let count = body.installations.len();
        out.extend(body.installations);
        if count < 100 {
            break;
        }
        page += 1;
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
pub struct GitHubRepository {
    pub id: i64,
    pub full_name: String,
    #[serde(default)]
    pub private: bool,
    pub default_branch: Option<String>,
    pub clone_url: String,
    pub html_url: String,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub permissions: JsonValue,
    pub pushed_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct RepositoriesPage {
    repositories: Vec<GitHubRepository>,
}

pub async fn user_installation_repositories(
    user_token: &str,
    installation_id: i64,
) -> Result<Vec<GitHubRepository>> {
    let client = github_client();
    let mut page = 1;
    let mut out = Vec::new();
    loop {
        let res = github_get(
            &client,
            format!(
                "{GITHUB_API}/user/installations/{installation_id}/repositories?per_page=100&page={page}"
            ),
            user_token,
        )
        .send()
        .await
        .context("listing GitHub repositories for user installation")?;
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!(
                "GitHub repositories request failed ({status}): {body}"
            ));
        }
        let body: RepositoriesPage = res.json().await?;
        let count = body.repositories.len();
        out.extend(body.repositories);
        if count < 100 {
            break;
        }
        page += 1;
    }
    Ok(out)
}

pub async fn installation_repositories(
    config: &GitHubAppConfig,
    installation_id: i64,
) -> Result<Vec<GitHubRepository>> {
    let token = create_installation_token(config, installation_id, None, None).await?;
    let client = github_client();
    let mut page = 1;
    let mut out = Vec::new();
    loop {
        let res = github_get(
            &client,
            format!("{GITHUB_API}/installation/repositories?per_page=100&page={page}"),
            &token.token,
        )
        .send()
        .await
        .context("listing GitHub repositories for installation")?;
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!(
                "GitHub installation repositories request failed ({status}): {body}"
            ));
        }
        let body: RepositoriesPage = res.json().await?;
        let count = body.repositories.len();
        out.extend(body.repositories);
        if count < 100 {
            break;
        }
        page += 1;
    }
    Ok(out)
}

pub async fn upsert_installation(
    pool: &DatabaseConnection,
    workspace_id: &str,
    installation: &GitHubInstallation,
) -> Result<()> {
    crate::db::query(
        "INSERT INTO github_installations (id, workspace_id, installation_id, account_login, \
            account_id, account_type, repository_selection, permissions, events, html_url, \
            active, suspended_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
         ON CONFLICT (workspace_id, installation_id) DO UPDATE SET \
            account_login = EXCLUDED.account_login, \
            account_id = EXCLUDED.account_id, \
            account_type = EXCLUDED.account_type, \
            repository_selection = EXCLUDED.repository_selection, \
            permissions = EXCLUDED.permissions, \
            events = EXCLUDED.events, \
            html_url = EXCLUDED.html_url, \
            active = EXCLUDED.active, \
            suspended_at = EXCLUDED.suspended_at, \
            updated_at = now()",
    )
    .bind(driftbase_common::Id::new().to_string())
    .bind(workspace_id)
    .bind(installation.id)
    .bind(&installation.account.login)
    .bind(installation.account.id)
    .bind(&installation.account.kind)
    .bind(&installation.repository_selection)
    .bind(&installation.permissions)
    .bind(&installation.events)
    .bind(installation.html_url.as_deref())
    .bind(installation.suspended_at.is_none())
    .bind(installation.suspended_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn sync_repositories(
    pool: &DatabaseConnection,
    workspace_id: &str,
    installation_id: i64,
    repositories: &[GitHubRepository],
) -> Result<()> {
    crate::db::query(
        "DELETE FROM github_repositories WHERE workspace_id = $1 AND installation_id = $2",
    )
    .bind(workspace_id)
    .bind(installation_id)
    .execute(pool)
    .await?;

    for repo in repositories {
        crate::db::query(
            "INSERT INTO github_repositories (workspace_id, installation_id, repository_id, \
                full_name, private, default_branch, clone_url, html_url, archived, disabled, \
                permissions, pushed_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             ON CONFLICT (workspace_id, installation_id, repository_id) DO UPDATE SET \
                full_name = EXCLUDED.full_name, \
                private = EXCLUDED.private, \
                default_branch = EXCLUDED.default_branch, \
                clone_url = EXCLUDED.clone_url, \
                html_url = EXCLUDED.html_url, \
                archived = EXCLUDED.archived, \
                disabled = EXCLUDED.disabled, \
                permissions = EXCLUDED.permissions, \
                pushed_at = EXCLUDED.pushed_at, \
                updated_at = now()",
        )
        .bind(workspace_id)
        .bind(installation_id)
        .bind(repo.id)
        .bind(&repo.full_name)
        .bind(repo.private)
        .bind(repo.default_branch.as_deref().unwrap_or("main"))
        .bind(&repo.clone_url)
        .bind(&repo.html_url)
        .bind(repo.archived)
        .bind(repo.disabled)
        .bind(&repo.permissions)
        .bind(repo.pushed_at)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn list_installations(
    pool: &DatabaseConnection,
    workspace_id: &str,
) -> Result<Vec<GitHubInstallationSummary>> {
    let rows: Vec<InstallationRow> = crate::db::query_as(
        "SELECT installation_id, account_login, account_type, repository_selection, active, \
                html_url, updated_at \
         FROM github_installations \
         WHERE workspace_id = $1 \
         ORDER BY account_login ASC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn list_repositories(
    pool: &DatabaseConnection,
    workspace_id: &str,
) -> Result<Vec<GitHubRepositorySummary>> {
    let rows: Vec<RepositoryRow> = crate::db::query_as(
        "SELECT r.installation_id, r.repository_id, r.full_name, r.private, r.default_branch, \
                r.clone_url, r.html_url, r.archived, r.disabled, r.pushed_at, r.updated_at \
         FROM github_repositories r \
         JOIN github_installations i \
           ON i.workspace_id = r.workspace_id AND i.installation_id = r.installation_id \
         WHERE r.workspace_id = $1 AND i.active = TRUE \
         ORDER BY r.full_name ASC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn repository_for_service_selection(
    pool: &DatabaseConnection,
    workspace_id: &str,
    installation_id: i64,
    repository_id: i64,
) -> Result<Option<GitHubRepositorySummary>> {
    let row: Option<RepositoryRow> = crate::db::query_as(
        "SELECT installation_id, repository_id, full_name, private, default_branch, clone_url, \
                html_url, archived, disabled, pushed_at, updated_at \
         FROM github_repositories \
         WHERE workspace_id = $1 AND installation_id = $2 AND repository_id = $3",
    )
    .bind(workspace_id)
    .bind(installation_id)
    .bind(repository_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

pub struct InstallationToken {
    pub token: String,
}

#[derive(Deserialize)]
struct InstallationTokenResponse {
    token: String,
}

pub async fn create_installation_token(
    config: &GitHubAppConfig,
    installation_id: i64,
    repository_id: Option<i64>,
    permissions: Option<JsonValue>,
) -> Result<InstallationToken> {
    let jwt = app_jwt(config)?;
    let mut body = json!({});
    if let Some(repository_id) = repository_id {
        body["repository_ids"] = json!([repository_id]);
    }
    if let Some(permissions) = permissions {
        body["permissions"] = permissions;
    }
    let client = github_client();
    let res = github_post(
        &client,
        format!("{GITHUB_API}/app/installations/{installation_id}/access_tokens"),
        &jwt,
    )
    .json(&body)
    .send()
    .await
    .context("creating GitHub installation token")?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!(
            "GitHub installation token request failed ({status}): {body}"
        ));
    }
    let body: InstallationTokenResponse = res.json().await?;
    Ok(InstallationToken { token: body.token })
}

pub async fn clone_token_for_repository(
    config: &Config,
    installation_id: i64,
    repository_id: i64,
) -> Result<String> {
    let github = config
        .github_app
        .as_ref()
        .ok_or_else(|| anyhow!("GitHub App is not configured"))?;
    let token = create_installation_token(
        github,
        installation_id,
        Some(repository_id),
        Some(json!({ "contents": "read" })),
    )
    .await?;
    Ok(token.token)
}

pub async fn post_commit_status_for_build(
    pool: &DatabaseConnection,
    config: &Config,
    build_id: &str,
    state: &str,
    description: &str,
) -> Result<()> {
    #[derive(sea_orm::FromQueryResult)]
    struct Row {
        git_sha: Option<String>,
        git_commit: Option<String>,
        service_slug: String,
        project_slug: String,
        workspace_slug: String,
        github_installation_id: Option<i64>,
        github_repository_id: Option<i64>,
        github_repository_full_name: Option<String>,
        github_statuses_enabled: bool,
    }

    let Some(github) = config.github_app.as_ref() else {
        return Ok(());
    };
    let row: Option<Row> = crate::db::query_as(
        "SELECT b.git_sha, b.git_commit, s.slug AS service_slug, p.slug AS project_slug, \
                w.slug AS workspace_slug, s.github_installation_id, s.github_repository_id, \
                s.github_repository_full_name, s.github_statuses_enabled \
         FROM builds b \
         JOIN services s ON s.id = b.service_id \
         JOIN projects p ON p.id = s.project_id \
         JOIN workspaces w ON w.id = p.workspace_id \
         WHERE b.id = $1",
    )
    .bind(build_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(());
    };
    if !row.github_statuses_enabled {
        return Ok(());
    }
    let (Some(installation_id), Some(repository_id), Some(full_name)) = (
        row.github_installation_id,
        row.github_repository_id,
        row.github_repository_full_name,
    ) else {
        return Ok(());
    };
    let Some(sha) = row.git_sha.or(row.git_commit) else {
        return Ok(());
    };
    let target_url = format!(
        "{}/w/{}/projects/{}/{}",
        config.public_url.trim_end_matches('/'),
        percent_encode(&row.workspace_slug),
        percent_encode(&row.project_slug),
        percent_encode(&row.service_slug)
    );
    post_commit_status(
        github,
        CommitStatusRequest {
            installation_id,
            repository_id,
            full_name: &full_name,
            sha: &sha,
            state,
            description,
            target_url: &target_url,
            context: &format!("driftbase/{}", row.service_slug),
        },
    )
    .await
}

pub struct CommitStatusRequest<'a> {
    pub installation_id: i64,
    pub repository_id: i64,
    pub full_name: &'a str,
    pub sha: &'a str,
    pub state: &'a str,
    pub description: &'a str,
    pub target_url: &'a str,
    pub context: &'a str,
}

pub async fn post_commit_status(
    config: &GitHubAppConfig,
    request: CommitStatusRequest<'_>,
) -> Result<()> {
    let token = create_installation_token(
        config,
        request.installation_id,
        Some(request.repository_id),
        Some(json!({ "statuses": "write" })),
    )
    .await?;
    let client = github_client();
    let res = github_post(
        &client,
        format!(
            "{GITHUB_API}/repos/{}/statuses/{}",
            request.full_name, request.sha
        ),
        &token.token,
    )
    .json(&json!({
        "state": request.state,
        "description": request.description,
        "target_url": request.target_url,
        "context": request.context,
    }))
    .send()
    .await
    .context("posting GitHub commit status")?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("GitHub status request failed ({status}): {body}"));
    }
    Ok(())
}

pub fn verify_webhook_signature(secret: &str, body: &[u8], signature: Option<&str>) -> bool {
    let Some(signature) = signature else {
        return false;
    };
    let Some(hex) = signature.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = decode_hex(hex) else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

pub fn branch_from_ref(reference: &str) -> Option<&str> {
    reference.strip_prefix("refs/heads/")
}

fn hmac_sha256(key: &[u8], body: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("hmac key");
    mac.update(body);
    let out = mac.finalize().into_bytes();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

fn decode_hex(input: &str) -> Result<Vec<u8>, ()> {
    if !input.len().is_multiple_of(2) {
        return Err(());
    }
    let mut out = Vec::with_capacity(input.len() / 2);
    for chunk in input.as_bytes().chunks_exact(2) {
        let hi = hex_value(chunk[0])?;
        let lo = hex_value(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_value(b: u8) -> Result<u8, ()> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(()),
    }
}

pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    fn test_key() -> MasterKey {
        MasterKey::from_base64(&STANDARD.encode([7u8; 32])).unwrap()
    }

    #[test]
    fn webhook_signature_validates_hmac_sha256() {
        let secret = "webhook-secret";
        let body = br#"{"zen":"Keep it logically awesome."}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let digest = mac.finalize().into_bytes();
        let signature = format!(
            "sha256={}",
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );

        assert!(verify_webhook_signature(secret, body, Some(&signature)));
        assert!(!verify_webhook_signature("wrong", body, Some(&signature)));
        assert!(!verify_webhook_signature(secret, body, None));
    }

    #[test]
    fn branch_parser_only_accepts_branch_refs() {
        assert_eq!(branch_from_ref("refs/heads/main"), Some("main"));
        assert_eq!(branch_from_ref("refs/heads/feature/x"), Some("feature/x"));
        assert_eq!(branch_from_ref("refs/tags/v1"), None);
    }

    #[test]
    fn state_roundtrips_and_tampering_fails() {
        let key = test_key();
        let state = sign_state(&key, "ws_1", "acme", "user_1");
        let verified = verify_state(&key, &state).unwrap();
        assert_eq!(verified.workspace_id, "ws_1");
        assert_eq!(verified.workspace_slug, "acme");
        assert_eq!(verified.user_id, "user_1");

        let tampered = format!("{state}a");
        assert!(verify_state(&key, &tampered).is_err());
    }
}
