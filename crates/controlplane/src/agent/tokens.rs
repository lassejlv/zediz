use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::crypto::MasterKey;

const BOOTSTRAP_CONTEXT: &[u8] = b"driftbase/node-bootstrap/v1";
const NODE_CONTEXT: &[u8] = b"driftbase/node-token/v1";
const DEFAULT_BOOTSTRAP_TTL_MIN: i64 = 60;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TokenKind {
    Bootstrap,
    Node,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    pub kind: TokenKind,
    pub node_id: String,
    pub workspace_id: String,
    pub nonce: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl TokenClaims {
    fn context(&self) -> &'static [u8] {
        match self.kind {
            TokenKind::Bootstrap => BOOTSTRAP_CONTEXT,
            TokenKind::Node => NODE_CONTEXT,
        }
    }
}

pub fn mint_bootstrap(master: &MasterKey, node_id: &str, workspace_id: &str) -> Result<String> {
    let now = Utc::now();
    let claims = TokenClaims {
        kind: TokenKind::Bootstrap,
        node_id: node_id.to_string(),
        workspace_id: workspace_id.to_string(),
        nonce: random_nonce(),
        issued_at: now,
        expires_at: Some(now + Duration::minutes(DEFAULT_BOOTSTRAP_TTL_MIN)),
    };
    encode(master, &claims)
}

pub fn mint_node(master: &MasterKey, node_id: &str, workspace_id: &str) -> Result<String> {
    let claims = TokenClaims {
        kind: TokenKind::Node,
        node_id: node_id.to_string(),
        workspace_id: workspace_id.to_string(),
        nonce: random_nonce(),
        issued_at: Utc::now(),
        expires_at: None,
    };
    encode(master, &claims)
}

pub fn verify(master: &MasterKey, token: &str, expect: TokenKind) -> Result<TokenClaims> {
    let (claims_part, sig_part) = token
        .split_once('.')
        .ok_or_else(|| anyhow!("malformed token"))?;
    let claims_bytes = B64
        .decode(claims_part)
        .map_err(|e| anyhow!("bad claims b64: {e}"))?;
    let claims: TokenClaims =
        serde_json::from_slice(&claims_bytes).map_err(|e| anyhow!("bad claims json: {e}"))?;
    if claims.kind != expect {
        return Err(anyhow!("token kind mismatch"));
    }
    let want_sig = sign_raw(master.derive(claims.context()), claims_part.as_bytes());
    let got_sig = B64
        .decode(sig_part)
        .map_err(|e| anyhow!("bad sig b64: {e}"))?;
    if !constant_eq(&want_sig, &got_sig) {
        return Err(anyhow!("invalid signature"));
    }
    if let Some(exp) = claims.expires_at {
        if Utc::now() > exp {
            return Err(anyhow!("token expired"));
        }
    }
    Ok(claims)
}

/// Returns SHA-256 hex digest suitable for indexing stored tokens.
pub fn fingerprint(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    hex(&h.finalize())
}

fn encode(master: &MasterKey, claims: &TokenClaims) -> Result<String> {
    let claims_json = serde_json::to_vec(claims)?;
    let claims_b64 = B64.encode(&claims_json);
    let sig = sign_raw(master.derive(claims.context()), claims_b64.as_bytes());
    let sig_b64 = B64.encode(sig);
    Ok(format!("{claims_b64}.{sig_b64}"))
}

fn sign_raw(key: [u8; 32], msg: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("hmac key");
    mac.update(msg);
    mac.finalize().into_bytes().to_vec()
}

fn random_nonce() -> String {
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    B64.encode(bytes)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn hex(bytes: &[u8]) -> String {
    const CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(CHARS[(b >> 4) as usize] as char);
        out.push(CHARS[(b & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as B64S;

    fn master() -> MasterKey {
        MasterKey::from_base64(&B64S.encode([7u8; 32])).unwrap()
    }

    #[test]
    fn bootstrap_roundtrip() {
        let m = master();
        let t = mint_bootstrap(&m, "node1", "ws1").unwrap();
        let c = verify(&m, &t, TokenKind::Bootstrap).unwrap();
        assert_eq!(c.node_id, "node1");
        assert_eq!(c.workspace_id, "ws1");
    }

    #[test]
    fn node_roundtrip() {
        let m = master();
        let t = mint_node(&m, "n", "w").unwrap();
        let c = verify(&m, &t, TokenKind::Node).unwrap();
        assert_eq!(c.node_id, "n");
    }

    #[test]
    fn wrong_kind_rejected() {
        let m = master();
        let t = mint_bootstrap(&m, "n", "w").unwrap();
        assert!(verify(&m, &t, TokenKind::Node).is_err());
    }

    #[test]
    fn tampered_rejected() {
        let m = master();
        let t = mint_bootstrap(&m, "n", "w").unwrap();
        let mut bytes = t.into_bytes();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let bad = String::from_utf8(bytes).unwrap();
        assert!(verify(&m, &bad, TokenKind::Bootstrap).is_err());
    }

    #[test]
    fn different_master_rejected() {
        let m1 = master();
        let m2 = MasterKey::from_base64(&B64S.encode([9u8; 32])).unwrap();
        let t = mint_bootstrap(&m1, "n", "w").unwrap();
        assert!(verify(&m2, &t, TokenKind::Bootstrap).is_err());
    }
}
