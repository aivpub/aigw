//! Cryptographic utilities — token hashing, litellm-compatible encryption/decryption
//!
//! Supports NaCl SecretBox (XSalsa20-Poly1305) and AES-256-GCM decryption,
//! compatible with litellm's encrypt_decrypt_utils.py.

use base64::{engine::general_purpose::STANDARD as BASE64_STD, Engine};
use sha2::{Digest, Sha256};

/// Hash a token string using SHA256, returning a hex-encoded string.
///
/// All API keys stored in the database are SHA256-hashed for security.
/// The raw token is only returned on the `/key/generate` or `/key/regenerate` response
/// and is never persisted in plaintext.
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Derive a 32-byte encryption key from a master key string using SHA-256.
fn derive_key(master_key: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(master_key.as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Decryption (litellm-compatible)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Decrypt a litellm-encrypted value.
///
/// Attempts NaCl SecretBox first, then falls back to AES-256-GCM.
///
/// The `v2:gcm:` prefix indicates AES-GCM encrypted values.
/// Values encrypted with NaCl SecretBox use the raw base64-encoded format
/// where the nonce is the first 24 bytes and the ciphertext is the rest.
///
/// AES-GCM format (base64-decoded):
///   salt[0..16] || nonce[16..28] || ciphertext[28..-16] || tag[-16..]
pub fn decrypt_litellm_value(encrypted_b64: &str, master_key: &str) -> Result<String, String> {
    let data = decode_base64_safe(encrypted_b64)
        .map_err(|e| format!("base64 decode failed: {}", e))?;

    // Check for v2:gcm: prefix indicating AES-256-GCM encrypted values
    if encrypted_b64.starts_with("v2:gcm:") {
        return decrypt_gcm(&data, master_key);
    }

    // Try NaCl SecretBox first (default for litellm)
    if let Ok(result) = decrypt_nacl(&data, master_key) {
        return Ok(result);
    }

    // Fallback: try AES-GCM
    decrypt_gcm(&data, master_key)
}

/// NaCl SecretBox decryption.
///
/// Format: nonce[0..24] || ciphertext+tag[24..]
fn decrypt_nacl(data: &[u8], master_key: &str) -> Result<String, String> {
    use crypto_secretbox::{aead::Aead, KeyInit};

    if data.len() < 24 {
        return Err("data too short for NaCl SecretBox".to_string());
    }

    let key = derive_key(master_key);
    let cipher = crypto_secretbox::XSalsa20Poly1305::new_from_slice(&key)
        .map_err(|e| format!("invalid key: {}", e))?;

    let nonce = crypto_secretbox::Nonce::from_slice(&data[..24]);
    let ciphertext = &data[24..];

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("NaCl decrypt failed: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 decode failed: {}", e))
}

/// AES-256-GCM decryption.
///
/// Format: salt[0..16] || nonce[16..28] || ciphertext[28..-16] || tag[-16..]
fn decrypt_gcm(data: &[u8], master_key: &str) -> Result<String, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};

    if data.len() < 44 {
        // 16 (salt) + 12 (nonce) + 16 (tag) minimum
        return Err("data too short for AES-256-GCM".to_string());
    }

    let salt = &data[..16];
    let nonce_bytes = &data[16..28];
    let ciphertext_with_tag = &data[28..];

    // Derive key using PBKDF2 with SHA-256
    let key = derive_gcm_key(master_key, salt);

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("invalid AES key: {}", e))?;

    use aes_gcm::aead::consts::U12;
    let nonce = <&aes_gcm::Nonce<U12>>::try_from(nonce_bytes)
        .map_err(|e| format!("invalid nonce: {:?}", e))?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext_with_tag)
        .map_err(|e| format!("AES-GCM decrypt failed: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 decode failed: {}", e))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Encryption (litellm-compatible)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Encrypt a value using NaCl SecretBox (litellm-compatible).
///
/// The output is base64-encoded: nonce[24] || ciphertext+tag.
/// This matches litellm's default encryption format.
pub fn encrypt_litellm_value(plaintext: &str, master_key: &str) -> Result<String, String> {
    use crypto_secretbox::{aead::Aead, KeyInit};

    let key = derive_key(master_key);
    let cipher = crypto_secretbox::XSalsa20Poly1305::new_from_slice(&key)
        .map_err(|e| format!("invalid key: {}", e))?;

    // Generate a random nonce (24 bytes for XSalsa20)
    let mut nonce_bytes = [0u8; 24];
    getrandom::fill(&mut nonce_bytes).map_err(|e| format!("RNG failed: {}", e))?;
    let nonce = crypto_secretbox::Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("NaCl encrypt failed: {}", e))?;

    // Format: nonce || ciphertext+tag, then base64 encode
    let mut result = Vec::with_capacity(24 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(BASE64_STD.encode(&result))
}

/// Derive a 32-byte key for AES-256-GCM using PBKDF2-HMAC-SHA256.
///
/// litellm uses: PBKDF2(password=master_key, salt=b"litellm", iterations=600_000, dklen=32)
fn derive_gcm_key(master_key: &str, salt: &[u8]) -> [u8; 32] {
    use hmac::Hmac;
    use pbkdf2::pbkdf2;

    let mut key = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(master_key.as_bytes(), salt, 600_000, &mut key)
        .expect("PBKDF2 derivation should not fail with valid inputs");
    key
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Base64-decode a string, trying multiple variants.
///
/// First tries standard base64, then falls back to URL-safe base64.
/// Also strips any "v2:gcm:" prefix before decoding.
fn decode_base64_safe(input: &str) -> Result<Vec<u8>, String> {
    // Strip v2:gcm: prefix if present
    let encoded = input.strip_prefix("v2:gcm:").unwrap_or(input);

    // Try standard base64 first
    if let Ok(data) = BASE64_STD.decode(encoded) {
        return Ok(data);
    }

    // Try URL-safe base64 (no padding) as fallback
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|e| format!("base64 decode failed: {}", e))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    // ━━━━━━━━━━━ token hashing tests ━━━━━━━━━━━

    #[test]
    fn test_hash_token_deterministic() {
        let input = "sk-abc123-test-key";
        let a = hash_token(input);
        let b = hash_token(input);
        assert_eq!(a, b);
    }

    #[test]
    fn test_hash_token_different_inputs() {
        let a = hash_token("key-one");
        let b = hash_token("key-two");
        assert_ne!(a, b);
    }

    #[test]
    fn test_hash_token_known_length() {
        let result = hash_token("hello-world");
        assert_eq!(result.len(), 64);
    }

    #[test]
    fn test_hash_token_empty() {
        let result = hash_token("");
        assert_eq!(result.len(), 64);
        assert_eq!(
            result,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    // ━━━━━━━━━━━ NaCl SecretBox tests ━━━━━━━━━━━

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let master_key = "sk-test-master-key-12345";
        let plaintext =
            r#"{"api_key":"sk-secret-value","api_base":"https://api.openai.com"}"#;

        let encrypted = encrypt_litellm_value(plaintext, master_key).unwrap();
        let decrypted = decrypt_litellm_value(&encrypted, master_key).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip_empty() {
        let master_key = "sk-test-master-key-12345";
        let plaintext = "";

        let encrypted = encrypt_litellm_value(plaintext, master_key).unwrap();
        let decrypted = decrypt_litellm_value(&encrypted, master_key).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip_unicode() {
        let master_key = "sk-unicode-key-测试";
        let plaintext = "Hello, 世界! 🌍";

        let encrypted = encrypt_litellm_value(plaintext, master_key).unwrap();
        let decrypted = decrypt_litellm_value(&encrypted, master_key).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_wrong_key_fails() {
        let master_key = "sk-test-master-key-12345";
        let wrong_key = "sk-wrong-key-67890";
        let plaintext = "secret data";

        let encrypted = encrypt_litellm_value(plaintext, master_key).unwrap();
        let result = decrypt_litellm_value(&encrypted, wrong_key);

        assert!(result.is_err(), "decrypt with wrong key should fail");
    }

    #[test]
    fn test_encrypt_produces_different_ciphertexts() {
        let master_key = "sk-test-master-key-12345";
        let plaintext = "same data";

        let encrypted1 = encrypt_litellm_value(plaintext, master_key).unwrap();
        let encrypted2 = encrypt_litellm_value(plaintext, master_key).unwrap();

        // Different nonces should produce different ciphertexts
        assert_ne!(encrypted1, encrypted2);

        // But both should decrypt to the same plaintext
        assert_eq!(
            decrypt_litellm_value(&encrypted1, master_key).unwrap(),
            plaintext
        );
        assert_eq!(
            decrypt_litellm_value(&encrypted2, master_key).unwrap(),
            plaintext
        );
    }

    #[test]
    fn test_derive_key_deterministic() {
        let key1 = derive_key("master-key");
        let key2 = derive_key("master-key");
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_derive_key_different() {
        let key1 = derive_key("master-key-1");
        let key2 = derive_key("master-key-2");
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_derive_key_length() {
        let key = derive_key("any-key");
        assert_eq!(key.len(), 32);
    }

    // ━━━━━━━━━━━ AES-256-GCM tests ━━━━━━━━━━━

    #[test]
    fn test_gcm_encrypt_decrypt_roundtrip() {
        use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};

        let master_key = "sk-test-master-key-12345";

        // Build an AES-GCM encrypted value manually to test decrypt
        let mut salt = [0u8; 16];
        getrandom::fill(&mut salt).unwrap();
        let gcm_key = derive_gcm_key(master_key, &salt);

        let cipher = Aes256Gcm::new_from_slice(&gcm_key).unwrap();
        let mut nonce_bytes = [0u8; 12];
        getrandom::fill(&mut nonce_bytes).unwrap();
        use aes_gcm::aead::consts::U12;
        let nonce = <&aes_gcm::Nonce<U12>>::try_from(&nonce_bytes[..]).unwrap();

        let plaintext = b"test plaintext for GCM";
        let ciphertext = cipher.encrypt(nonce, plaintext.as_ref()).unwrap();

        let mut encrypted_data = Vec::new();
        encrypted_data.extend_from_slice(&salt);
        encrypted_data.extend_from_slice(&nonce_bytes);
        encrypted_data.extend_from_slice(&ciphertext);

        let encoded = format!("v2:gcm:{}", BASE64_STD.encode(&encrypted_data));

        let decrypted = decrypt_litellm_value(&encoded, master_key).unwrap();
        assert_eq!(decrypted.as_bytes(), plaintext);
    }

    // ━━━━━━━━━━━ edge case tests ━━━━━━━━━━━

    #[test]
    fn test_decrypt_invalid_base64() {
        let result = decrypt_litellm_value("!!!not-valid-base64!!!", "sk-test");
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_data_too_short() {
        // Encode only 5 bytes - too short for both NaCl (24) and GCM (44)
        let short = BASE64_STD.encode(b"short");
        let result = decrypt_litellm_value(&short, "sk-test");
        assert!(result.is_err());
    }
}
