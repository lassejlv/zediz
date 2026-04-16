use anyhow::{anyhow, Result};
use argon2::password_hash::{rand_core::OsRng, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Argon2, PasswordHash};

pub fn hash(plain: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow!("argon2 hash: {e}"))
}

pub fn verify(plain: &str, encoded: &str) -> Result<bool> {
    let parsed = PasswordHash::new(encoded).map_err(|e| anyhow!("argon2 parse: {e}"))?;
    Ok(Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies() {
        let h = hash("hunter2hunter2").unwrap();
        assert!(verify("hunter2hunter2", &h).unwrap());
        assert!(!verify("wrong", &h).unwrap());
    }
}
