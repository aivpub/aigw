//! Password hashing utilities — scrypt hash/verify compatible with litellm.
//!
//! Litellm format: `scrypt:base64(salt(16)||dk(64))` where salt is 16 bytes and derived key is 64 bytes.

use rand::RngCore;
use scrypt::{scrypt, Params};
use std::sync::LazyLock;

/// scrypt parameters matching litellm's default (N=16384, r=8, p=1, dkLen=64).
static SCRYPT_PARAMS: LazyLock<Params> =
    LazyLock::new(|| Params::new(14, 8, 1, 64).expect("valid scrypt params"));

/// Hash a password using scrypt (litellm-compatible format).
///
/// Returns `"scrypt:base64(salt||dk)"` where salt is 16 random bytes and dk is 64 bytes.
pub fn hash_password(password: &str) -> Result<String, String> {
    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);

    let mut dk = [0u8; 64];
    scrypt(password.as_bytes(), &salt, &SCRYPT_PARAMS, &mut dk)
        .map_err(|e| format!("scrypt hash failed: {}", e))?;

    let mut combined = Vec::with_capacity(16 + 64);
    combined.extend_from_slice(&salt);
    combined.extend_from_slice(&dk);

    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    Ok(format!("scrypt:{}", BASE64.encode(&combined)))
}

/// Verify a password against a litellm-compatible scrypt hash.
///
/// The hash must be in format `"scrypt:base64(salt(16)||dk(64))"`.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, String> {
    let encoded = hash
        .strip_prefix("scrypt:")
        .ok_or_else(|| "invalid hash format: missing scrypt: prefix".to_string())?;

    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    let combined = BASE64
        .decode(encoded)
        .map_err(|e| format!("base64 decode failed: {}", e))?;

    if combined.len() != 80 {
        return Err(format!(
            "invalid hash: expected 80 bytes, got {}",
            combined.len()
        ));
    }

    let salt = &combined[..16];
    let expected = &combined[16..];

    let mut dk = [0u8; 64];
    scrypt(password.as_bytes(), salt, &SCRYPT_PARAMS, &mut dk)
        .map_err(|e| format!("scrypt verify failed: {}", e))?;

    Ok(dk.as_slice() == expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify() {
        let password = "my-secret-password";
        let hash = hash_password(password).unwrap();

        assert!(hash.starts_with("scrypt:"));
        assert!(verify_password(password, &hash).unwrap());
    }

    #[test]
    fn test_wrong_password() {
        let hash = hash_password("correct-password").unwrap();
        assert!(!verify_password("wrong-password", &hash).unwrap());
    }

    #[test]
    fn test_different_salts() {
        let pw = "same-password";
        let h1 = hash_password(pw).unwrap();
        let h2 = hash_password(pw).unwrap();
        assert_ne!(h1, h2); // different salts
        assert!(verify_password(pw, &h1).unwrap());
        assert!(verify_password(pw, &h2).unwrap());
    }

    #[test]
    fn test_invalid_format() {
        assert!(verify_password("pw", "not-valid").is_err());
        assert!(verify_password("pw", "scrypt:too-short").is_err());
    }
}
