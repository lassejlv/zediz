use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

#[derive(Clone)]
pub struct MasterKey {
    cipher: Aes256Gcm,
    raw: [u8; KEY_LEN],
}

impl MasterKey {
    pub fn from_base64(encoded: &str) -> Result<Self> {
        let bytes = B64
            .decode(encoded.trim())
            .map_err(|e| anyhow!("DRIFTBASE_MASTER_KEY is not valid base64: {e}"))?;
        if bytes.len() != KEY_LEN {
            return Err(anyhow!(
                "DRIFTBASE_MASTER_KEY must decode to {KEY_LEN} bytes, got {}",
                bytes.len()
            ));
        }
        let key = Key::<Aes256Gcm>::from_slice(&bytes);
        let mut raw = [0u8; KEY_LEN];
        raw.copy_from_slice(&bytes);
        Ok(Self {
            cipher: Aes256Gcm::new(key),
            raw,
        })
    }

    /// Derive a subkey for a named context via HMAC-SHA256(master, context).
    pub fn derive(&self, context: &[u8]) -> [u8; 32] {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.raw).expect("hmac key len");
        mac.update(context);
        let out = mac.finalize().into_bytes();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&out);
        arr
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| anyhow!("encrypt: {e}"))?;
        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(nonce.as_slice());
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    pub fn decrypt(&self, blob: &[u8]) -> Result<Vec<u8>> {
        if blob.len() < NONCE_LEN {
            return Err(anyhow!("ciphertext too short"));
        }
        let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow!("decrypt: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    fn random_key_b64() -> String {
        let mut bytes = [0u8; KEY_LEN];
        rand::thread_rng().fill_bytes(&mut bytes);
        B64.encode(bytes)
    }

    #[test]
    fn roundtrip() {
        let key = MasterKey::from_base64(&random_key_b64()).unwrap();
        let plaintext = b"hello driftbase secret";
        let ct = key.encrypt(plaintext).unwrap();
        assert_ne!(&ct[12..], plaintext);
        let pt = key.decrypt(&ct).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn rejects_wrong_key() {
        let k1 = MasterKey::from_base64(&random_key_b64()).unwrap();
        let k2 = MasterKey::from_base64(&random_key_b64()).unwrap();
        let ct = k1.encrypt(b"secret").unwrap();
        assert!(k2.decrypt(&ct).is_err());
    }

    #[test]
    fn rejects_bad_key_length() {
        let short = B64.encode([0u8; 16]);
        assert!(MasterKey::from_base64(&short).is_err());
    }
}
