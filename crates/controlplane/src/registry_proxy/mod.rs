//! Bundled Docker Registry auth proxy.
//!
//! Caddy forwards `{$REGISTRY_SITE}/*` to `controlplane:8080`. This module
//! handles `/v2/...` requests: it validates Basic auth against the
//! `credentials` table, enforces that the authed credential's workspace owns
//! the image path (`/v2/<workspace_id>/...`), and stream-proxies to the
//! registry container.
//!
//! The registry container itself has no auth and no exposed host ports — it's
//! only reachable from the internal docker network, and the CP is its sole
//! client.
//!
//! Per-workspace scoping is the core isolation: two workspaces with valid
//! credentials cannot pull each other's images because the URL path's first
//! segment must match the authed credential's workspace.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use base64::prelude::{Engine, BASE64_STANDARD};
use futures::TryStreamExt;
use std::sync::LazyLock;

use crate::credentials;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    // `any` matches all HTTP methods — docker pushes use GET/HEAD/PUT/PATCH/POST/DELETE.
    // Docker clients hit the discovery endpoint as `/v2/` (with trailing
    // slash); axum's `/v2/*path` wildcard requires at least one character
    // after the slash, so the bare `/v2/` needs its own route or else it
    // falls through to 404 and `docker login` dies with "login attempt to
    // https://.../v2/ failed with status: 404 Not Found".
    Router::new()
        .route("/v2", any(handle))
        .route("/v2/", any(handle))
        .route("/v2/*path", any(handle))
}

static HOP_BY_HOP: LazyLock<[HeaderName; 7]> = LazyLock::new(|| {
    [
        header::CONNECTION,
        HeaderName::from_static("keep-alive"),
        HeaderName::from_static("proxy-authenticate"),
        HeaderName::from_static("proxy-authorization"),
        HeaderName::from_static("te"),
        HeaderName::from_static("trailers"),
        header::TRANSFER_ENCODING,
    ]
});

async fn handle(State(state): State<AppState>, req: Request) -> Response {
    let Some(_registry_site) = state.config().registry_site.as_deref() else {
        return error(
            StatusCode::NOT_FOUND,
            "bundled registry proxy not configured",
        );
    };

    let path = req.uri().path().to_string();

    // Authenticate. Discovery endpoints (`/v2` and `/v2/`) are workspace-agnostic
    // but still require a valid credential — that's what `docker login` relies
    // on to decide whether the login was accepted.
    let basic = match parse_basic(req.headers()) {
        Some(b) => b,
        None => return unauthorized(),
    };

    let (creds_workspace_id, cred) = match credentials::fetch_for_proxy(
        state.pool(),
        state.master_key(),
        &basic.user,
    )
    .await
    {
        Ok(Some(v)) => v,
        Ok(None) => return unauthorized(),
        Err(e) => {
            tracing::warn!(error = ?e, user = %basic.user, "registry proxy credential lookup failed");
            return unauthorized();
        }
    };
    if cred.kind != "registry" {
        return unauthorized();
    }
    if !constant_time_eq(cred.secret.as_bytes(), basic.pass.as_bytes()) {
        return unauthorized();
    }

    // Enforce the path-scope check for anything past the discovery endpoint.
    // `/v2`, `/v2/`, `/v2/_catalog` are the ones without a `<name>` segment.
    if let Some(path_workspace) = workspace_from_path(&path) {
        if !path_workspace.eq_ignore_ascii_case(&creds_workspace_id) {
            return unauthorized();
        }
    }

    // Build the upstream request.
    let upstream_base = state.config().registry_upstream.trim_end_matches('/');
    let upstream_url = match build_upstream_url(upstream_base, req.uri()) {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(error = ?e, path = %path, "registry proxy bad upstream url");
            return error(StatusCode::BAD_GATEWAY, "bad upstream url");
        }
    };

    let method = match reqwest::Method::from_bytes(req.method().as_str().as_bytes()) {
        Ok(m) => m,
        Err(_) => return error(StatusCode::METHOD_NOT_ALLOWED, "unsupported method"),
    };

    let (parts, body) = req.into_parts();
    let mut upstream_headers = reqwest::header::HeaderMap::with_capacity(parts.headers.len());
    for (name, value) in &parts.headers {
        if HOP_BY_HOP.iter().any(|h| h == name) {
            continue;
        }
        if name == header::HOST || name == header::AUTHORIZATION || name == header::CONTENT_LENGTH {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            reqwest::header::HeaderName::from_bytes(name.as_ref()),
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            upstream_headers.append(n, v);
        }
    }

    let body_stream = body.into_data_stream().map_err(std::io::Error::other);
    let upstream_body = reqwest::Body::wrap_stream(body_stream);

    let client = reqwest::Client::builder()
        // Docker pushes can take a while. Leave the overall request timeout
        // unset instead of using `Duration::ZERO`, which causes immediate
        // request timeouts.
        .build()
        .expect("reqwest client");

    let upstream = client
        .request(method, upstream_url)
        .headers(upstream_headers)
        .body(upstream_body)
        .send()
        .await;
    let upstream = match upstream {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = ?e, "registry upstream request failed");
            return error(StatusCode::BAD_GATEWAY, "upstream registry unreachable");
        }
    };

    // Relay the response.
    let mut builder = Response::builder().status(
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
    );
    if let Some(headers) = builder.headers_mut() {
        for (name, value) in upstream.headers() {
            if HOP_BY_HOP.iter().any(|h| h.as_str() == name.as_str()) {
                continue;
            }
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(name.as_ref()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                headers.append(n, v);
            }
        }
    }
    let resp_stream = upstream.bytes_stream();
    match builder.body(Body::from_stream(resp_stream)) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = ?e, "registry proxy could not build response");
            error(StatusCode::BAD_GATEWAY, "response build failed")
        }
    }
}

struct Basic {
    user: String,
    pass: String,
}

fn parse_basic(headers: &HeaderMap) -> Option<Basic> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let b64 = raw.strip_prefix("Basic ")?.trim();
    let decoded = BASE64_STANDARD.decode(b64).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (user, pass) = text.split_once(':')?;
    Some(Basic {
        user: user.to_string(),
        pass: pass.to_string(),
    })
}

/// First segment of the path after `/v2/`. Returns `None` for the discovery
/// endpoints (`/v2`, `/v2/`, `/v2/_catalog`), which are workspace-agnostic.
fn workspace_from_path(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/v2/")?;
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() || rest == "_catalog" {
        return None;
    }
    let first = rest.split('/').next()?;
    if first.is_empty() || first.starts_with('_') {
        return None;
    }
    Some(first)
}

fn build_upstream_url(base: &str, incoming: &Uri) -> Result<reqwest::Url, reqwest::Error> {
    // `incoming.path_and_query()` gives us `/v2/...?pagination=...` exactly as
    // the client sent it; we want to append that to the upstream base.
    let tail = incoming
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/v2");
    let joined = format!("{base}{tail}");
    // reqwest::Url is re-exported from the `url` crate; we parse via the
    // blanket impl and let reqwest's error type surface any problem.
    reqwest::Client::new()
        .get(&joined)
        .build()
        .map(|r| r.url().clone())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn unauthorized() -> Response {
    let mut r = (StatusCode::UNAUTHORIZED, "unauthorized\n").into_response();
    r.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static(r#"Basic realm="driftbase-registry""#),
    );
    r
}

fn error(code: StatusCode, msg: &str) -> Response {
    (code, format!("{msg}\n")).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_scope_extraction() {
        assert_eq!(workspace_from_path("/v2"), None);
        assert_eq!(workspace_from_path("/v2/"), None);
        assert_eq!(workspace_from_path("/v2/_catalog"), None);
        assert_eq!(
            workspace_from_path("/v2/ws_abc/svc/manifests/tag"),
            Some("ws_abc")
        );
        assert_eq!(
            workspace_from_path("/v2/ws_abc/svc/blobs/sha256:def"),
            Some("ws_abc")
        );
        assert_eq!(workspace_from_path("/v2/ws_abc"), Some("ws_abc"));
    }

    #[test]
    fn basic_parse() {
        let mut h = HeaderMap::new();
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic dXNlcjpwYXNz"),
        );
        let b = parse_basic(&h).unwrap();
        assert_eq!(b.user, "user");
        assert_eq!(b.pass, "pass");
    }
}
