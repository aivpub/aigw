//! Cryptographic utilities — token hashing, litellm-compatible encryption/decryption
//!
//! Supports NaCl SecretBox (XSalsa20-Poly1305) and AES-256-GCM decryption,
//! compatible with litellm's encrypt_decrypt_utils.py.

use base64::{engine::general_purpose::STANDARD as BASE64_STD, Engine};
use serde_json::Value;
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
/// Uses the `v2:gcm:` prefix to disambiguate: GCM-encrypted values always
/// carry this prefix; all other values use NaCl SecretBox.
///
/// NaCl SecretBox format (base64-decoded):
///   nonce[0..24] || ciphertext+tag[24..]
///
/// AES-GCM format (base64-decoded):
///   salt[0..16] || nonce[16..28] || ciphertext[28..-16] || tag[-16..]
///
/// No silent fallback from NaCl to GCM: litellm always prefixes GCM values
/// with `v2:gcm:`.  Falling through to GCM when NaCl fails only wastes
/// 600 000 PBKDF2 iterations (several seconds in debug builds) on data that
/// was never GCM-encrypted.
pub fn decrypt_litellm_value(encrypted_b64: &str, master_key: &str) -> Result<String, String> {
    let data =
        decode_base64_safe(encrypted_b64).map_err(|e| format!("base64 decode failed: {}", e))?;

    // Check for v2:gcm: prefix indicating AES-256-GCM encrypted values.
    // litellm always writes this prefix for GCM-encrypted values.
    if encrypted_b64.starts_with("v2:gcm:") {
        return decrypt_gcm(&data, master_key);
    }

    // No prefix → NaCl SecretBox only.  GCM fallback is intentionally
    // skipped: it costs PBKDF2 600 000 iterations per call (~2 s in
    // debug builds) and was never reached for correctly-prefixed data.
    decrypt_nacl(&data, master_key)
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

    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("invalid AES key: {}", e))?;

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
// proxy_url encryption (Phase 50, Stage 122)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Encrypt a whole proxy URL string (`scheme://user:pass@host:port`) for
/// `proxies.proxy_url` storage. Same strength as litellm credentials.
pub fn encrypt_proxy_url(proxy_url: &str, master_key: &str) -> Result<String, String> {
    encrypt_litellm_value(proxy_url, master_key)
}

/// Decrypt a `proxies.proxy_url` ciphertext back to the plaintext URL.
pub fn decrypt_proxy_url(encrypted: &str, master_key: &str) -> Result<String, String> {
    decrypt_litellm_value(encrypted, master_key)
}

/// Redact the password portion of a plaintext proxy URL for admin responses:
/// `scheme://user:***@host:port`. If the URL has no `@` (no credentials), it is
/// returned unchanged. Malformed URLs (no `://`) are returned as-is.
pub fn redact_proxy_url(url: &str) -> String {
    let Some((scheme_rest, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    // rest = [user:pass@]host:port — redact everything between start and the
    // last '@' (only the credential part), keeping host:port intact.
    let Some(at_idx) = rest.rfind('@') else {
        return url.to_string();
    };
    let credentials = &rest[..at_idx];
    let host_part = &rest[at_idx + 1..];
    // Split credentials into user[:pass] — user is safe to keep, pass is masked.
    let (user, _pass) = credentials.split_once(':').unwrap_or((credentials, ""));
    format!("{}://{}:***@{}", scheme_rest, user, host_part)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Nested field-level decryption
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Recursively walk a JSON value and attempt to decrypt every string leaf.
///
/// This handles the common litellm pattern where individual fields inside
/// a `litellm_params` or `credential_values` JSON object are separately
/// encrypted (e.g. `api_key`, `api_base`, `litellm_credential_name`).
///
/// `decrypt_litellm_value` safely rejects non-encrypted strings (wrong
/// format / UTF-8 validation), so we can try-then-fallback without
/// false positives.
pub fn decrypt_json_fields(value: &Value, master_key: &str) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), decrypt_json_fields(v, master_key));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| decrypt_json_fields(v, master_key))
                .collect(),
        ),
        Value::String(s) if !s.is_empty() && !s.starts_with('{') => {
            match decrypt_litellm_value(s, master_key) {
                Ok(decrypted) => {
                    serde_json::from_str(&decrypted).unwrap_or(Value::String(decrypted))
                }
                Err(_) => value.clone(),
            }
        }
        _ => value.clone(),
    }
}

/// Recursively walk a JSON value and rotate individually encrypted string leaves
/// from `source_key` to `target_key`.
///
/// Used during `remote-import` when `litellm_params` or `credential_values` is a JSON
/// object whose individual fields (like `api_key`, `api_base`, `litellm_credential_name`)
/// are separately encrypted rather than the whole value being a single blob.
///
/// Plain-text strings are left untouched.  The resulting serialised JSON string is
/// returned so the caller can re-encrypt it as a single blob (matching the existing
/// storage format).
pub fn rotate_json_fields(
    value: &Value,
    source_key: &str,
    target_key: &str,
) -> Result<String, String> {
    let rotated = rotate_fields_inner(value, source_key, target_key)?;
    Ok(serde_json::to_string(&rotated).unwrap_or_else(|_| "{}".to_string()))
}

fn rotate_fields_inner(value: &Value, source_key: &str, target_key: &str) -> Result<Value, String> {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), rotate_fields_inner(v, source_key, target_key)?);
            }
            Ok(Value::Object(out))
        }
        Value::Array(arr) => {
            let rotated: Result<Vec<Value>, String> = arr
                .iter()
                .map(|v| rotate_fields_inner(v, source_key, target_key))
                .collect();
            Ok(Value::Array(rotated?))
        }
        Value::String(s) if !s.is_empty() && !s.starts_with('{') => {
            // Try decrypting with source key; if it succeeds, re-encrypt with target key.
            // If decrypt fails, it's plain text — leave untouched.
            match decrypt_litellm_value(s, source_key) {
                Ok(plaintext) => encrypt_litellm_value(&plaintext, target_key).map(Value::String),
                Err(_) => Ok(value.clone()),
            }
        }
        _ => Ok(value.clone()),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Decode a `base64:type15:<encoded>` envelope produced by litellm on MySQL
/// deployments.
///
/// Unlike `v2:gcm:` / NaCl envelopes (which carry *encrypted* bytes), the
/// `base64:type15:` envelope wraps a **JSON plaintext** object — litellm
/// base64-encodes the whole `litellm_params` JSON so MySQL's JSON validator
/// never sees the raw text. The inner fields (e.g. `custom_llm_provider`) may
/// still be individually NaCl-encrypted; those are handled separately by
/// `rotate_json_fields` / `decrypt_json_fields`.
///
/// Returns the decoded UTF-8 plaintext on success, so callers can route the
/// value through the same JSON-object rotation path used for SQLite/PG.
pub fn decode_base64_type15(envelope: &str) -> Result<String, String> {
    let rest = envelope
        .strip_prefix("base64:type15:")
        .ok_or_else(|| "not a base64:type15: envelope".to_string())?;
    let cleaned: String = rest.replace(['\n', '\r'], "");
    let data = decode_base64_safe(&cleaned)?;
    String::from_utf8(data).map_err(|e| format!("UTF-8 decode failed: {}", e))
}

/// Base64-decode a string, trying multiple variants.
///
/// First tries standard base64, then falls back to URL-safe base64
/// (with and without padding).  Finally tries normalizing URL-safe
/// characters (`-` → `+`, `_` → `/`) inside a standard-base64 string.
/// Also strips any "v2:gcm:" prefix before decoding.
fn decode_base64_safe(input: &str) -> Result<Vec<u8>, String> {
    // Strip v2:gcm: prefix if present (AES-256-GCM encrypted values)
    let encoded = input.strip_prefix("v2:gcm:").unwrap_or(input);
    // Strip base64:type15: prefix — used by litellm postgres deployments
    // (stores encrypted JSON as "base64:type15:<encoded>")
    let encoded = encoded.strip_prefix("base64:type15:").unwrap_or(encoded);
    // Strip embedded newlines — MySQL JSON columns store literal \n in strings
    let encoded: String = encoded.replace(['\n', '\r'], "");

    // 1. Try standard base64 first
    if let Ok(data) = BASE64_STD.decode(&encoded) {
        return Ok(data);
    }

    // 2. Try URL-safe base64 (with padding)
    use base64::engine::general_purpose::URL_SAFE;
    if let Ok(data) = URL_SAFE.decode(&encoded) {
        return Ok(data);
    }

    // 3. Try URL-safe base64 (no padding)
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    if let Ok(data) = URL_SAFE_NO_PAD.decode(&encoded) {
        return Ok(data);
    }

    // 4. Handle mixed encoding: some litellm deployments mix standard
    //    and URL-safe characters in the same base64 string.  Normalize
    //    URL-safe chars → standard, then decode with standard base64.
    let normalized: String = encoded
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            _ => c,
        })
        .collect();
    BASE64_STD
        .decode(&normalized)
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
        let plaintext = r#"{"api_key":"sk-secret-value","api_base":"https://api.openai.com"}"#;

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

    // ━━━━━━━━━━━ decrypt_json_fields tests ━━━━━━━━━━━

    fn make_encrypted(value: &str) -> String {
        encrypt_litellm_value(value, "test-master-key").unwrap()
    }

    #[test]
    fn test_decrypt_json_fields_nested_encrypted() {
        let plain_api_base = "https://api.openai.com";
        let plain_api_key = "sk-secret-key";
        let params = serde_json::json!({
            "model": "gpt-4",
            "api_base": make_encrypted(plain_api_base),
            "api_key": make_encrypted(plain_api_key),
            "custom_llm_provider": "openai",
        });
        let result = decrypt_json_fields(&params, "test-master-key");
        assert_eq!(result["model"], serde_json::json!("gpt-4"));
        assert_eq!(result["api_base"], serde_json::json!(plain_api_base));
        assert_eq!(result["api_key"], serde_json::json!("sk-secret-key"));
        assert_eq!(result["custom_llm_provider"], serde_json::json!("openai"));
    }

    #[test]
    fn test_decrypt_json_fields_plaintext_untouched() {
        let params = serde_json::json!({
            "model": "gpt-4",
            "rpm": 100,
            "tpm": 2000,
            "api_base": "https://plain.example.com",
        });
        let result = decrypt_json_fields(&params, "test-master-key");
        assert_eq!(result["model"], serde_json::json!("gpt-4"));
        assert_eq!(result["rpm"], serde_json::json!(100));
        assert_eq!(result["tpm"], serde_json::json!(2000));
        assert_eq!(
            result["api_base"],
            serde_json::json!("https://plain.example.com")
        );
    }

    #[test]
    fn test_decrypt_json_fields_empty_string_passes_through() {
        let params = serde_json::json!({"model": "gpt-4", "api_base": ""});
        let result = decrypt_json_fields(&params, "test-master-key");
        assert_eq!(result["api_base"], serde_json::json!(""));
    }

    #[test]
    fn test_decrypt_json_fields_recursive_in_object() {
        let plain_deployment = "us-east-1";
        let params = serde_json::json!({
            "model": "bedrock",
            "litellm_params": {
                "deployment": make_encrypted(plain_deployment),
                "region": "us-east-1",
            },
        });
        let result = decrypt_json_fields(&params, "test-master-key");
        assert_eq!(result["model"], serde_json::json!("bedrock"));
        assert_eq!(
            result["litellm_params"]["deployment"],
            serde_json::json!(plain_deployment)
        );
        assert_eq!(
            result["litellm_params"]["region"],
            serde_json::json!("us-east-1")
        );
    }

    #[test]
    fn test_decrypt_json_fields_in_arrays() {
        let plain1 = "model-a";
        let plain2 = "model-b";
        let params = serde_json::json!({
            "fallbacks": [
                make_encrypted(plain1),
                make_encrypted(plain2),
            ],
        });
        let result = decrypt_json_fields(&params, "test-master-key");
        assert_eq!(result["fallbacks"][0], serde_json::json!(plain1));
        assert_eq!(result["fallbacks"][1], serde_json::json!(plain2));
    }

    #[test]
    fn test_decrypt_json_fields_credential_name_encrypted() {
        // Simulates the bug scenario: litellm_credential_name is individually encrypted
        let plain_cred_name = "my-credential-123";
        let params = serde_json::json!({
            "model": "gpt-4",
            "litellm_credential_name": make_encrypted(plain_cred_name),
        });
        let result = decrypt_json_fields(&params, "test-master-key");
        assert_eq!(
            result["litellm_credential_name"],
            serde_json::json!(plain_cred_name)
        );
    }

    #[test]
    fn test_decrypt_json_fields_whole_encrypted_object_value() {
        // Simulate whole model params encrypted as a single string
        let plain = r#"{"model":"gpt-4","api_base":"https://api.openai.com"}"#;
        let encrypted = make_encrypted(plain);
        let result = decrypt_json_fields(&serde_json::json!(encrypted), "test-master-key");
        assert_eq!(result["model"], serde_json::json!("gpt-4"));
        assert_eq!(
            result["api_base"],
            serde_json::json!("https://api.openai.com")
        );
    }

    // ━━━━━━━━━━━ rotate_json_fields tests ━━━━━━━━━━━

    const SOURCE_KEY: &str = "sk-source-key-abc";
    const TARGET_KEY: &str = "sk-target-key-xyz";

    fn make_encrypted_with_key(value: &str, key: &str) -> String {
        encrypt_litellm_value(value, key).unwrap()
    }

    #[test]
    fn test_rotate_json_fields_nested() {
        let plain_api_key = "sk-secret-123";
        let plain_api_base = "https://api.openai.com";
        let params = serde_json::json!({
            "model": "gpt-4",
            "api_key": make_encrypted_with_key(plain_api_key, SOURCE_KEY),
            "api_base": make_encrypted_with_key(plain_api_base, SOURCE_KEY),
            "custom_llm_provider": "openai",
        });

        let rotated_str = rotate_json_fields(&params, SOURCE_KEY, TARGET_KEY).unwrap();
        let rotated: Value = serde_json::from_str(&rotated_str).unwrap();

        // The encrypted fields now decrypt with TARGET_KEY
        assert_eq!(rotated["custom_llm_provider"], serde_json::json!("openai"));
        let api_key_enc = rotated["api_key"].as_str().unwrap();
        let decrypted_key = decrypt_litellm_value(api_key_enc, TARGET_KEY).unwrap();
        assert_eq!(decrypted_key, plain_api_key);
        // And they should NOT decrypt with the old SOURCE_KEY
        assert!(decrypt_litellm_value(api_key_enc, SOURCE_KEY).is_err());
    }

    #[test]
    fn test_rotate_json_fields_plaintext_untouched() {
        let params = serde_json::json!({
            "model": "gpt-4",
            "rpm": 100,
            "api_base": "https://plain.example.com",
        });
        let rotated_str = rotate_json_fields(&params, SOURCE_KEY, TARGET_KEY).unwrap();
        assert_eq!(rotated_str, params.to_string());
    }

    #[test]
    fn test_rotate_json_fields_empty_string() {
        let params = serde_json::json!({"model": "gpt-4", "api_base": ""});
        let rotated_str = rotate_json_fields(&params, SOURCE_KEY, TARGET_KEY).unwrap();
        let rotated: Value = serde_json::from_str(&rotated_str).unwrap();
        assert_eq!(rotated["api_base"], serde_json::json!(""));
    }

    #[test]
    fn test_rotate_json_fields_decryptable_with_decrypt_json_fields() {
        // After rotation, decrypt_json_fields should produce the original plaintext
        let plain_api_key = "sk-secret-456";
        let plain_api_base = "https://example.com/v1";
        let params = serde_json::json!({
            "api_key": make_encrypted_with_key(plain_api_key, SOURCE_KEY),
            "api_base": make_encrypted_with_key(plain_api_base, SOURCE_KEY),
        });

        let rotated_str = rotate_json_fields(&params, SOURCE_KEY, TARGET_KEY).unwrap();
        let rotated: Value = serde_json::from_str(&rotated_str).unwrap();
        let decrypted = decrypt_json_fields(&rotated, TARGET_KEY);

        assert_eq!(decrypted["api_key"], serde_json::json!(plain_api_key));
        assert_eq!(decrypted["api_base"], serde_json::json!(plain_api_base));
    }

    #[test]
    fn test_rotate_json_fields_credential_name() {
        // Simulates credential_values with individually encrypted fields
        let plain_cred_name = "my-openai-credential";
        let params = serde_json::json!({
            "litellm_credential_name": make_encrypted_with_key(plain_cred_name, SOURCE_KEY),
        });

        let rotated_str = rotate_json_fields(&params, SOURCE_KEY, TARGET_KEY).unwrap();
        let rotated: Value = serde_json::from_str(&rotated_str).unwrap();

        let name_enc = rotated["litellm_credential_name"].as_str().unwrap();
        assert_eq!(
            decrypt_litellm_value(name_enc, TARGET_KEY).unwrap(),
            plain_cred_name,
        );
        assert!(decrypt_litellm_value(name_enc, SOURCE_KEY).is_err());
    }

    #[test]
    fn test_decode_base64_type15_returns_json_plaintext() {
        // litellm MySQL wraps the whole litellm_params JSON in base64:type15:.
        // The payload is JSON plaintext, not a NaCl-encrypted blob.
        let plain = r#"{"custom_llm_provider":"abc","model":"gpt-4"}"#;
        let encoded = BASE64_STD.encode(plain.as_bytes());
        let envelope = format!("base64:type15:{}", encoded);

        let decoded = decode_base64_type15(&envelope).unwrap();
        assert_eq!(decoded, plain);
    }

    #[test]
    fn test_decode_base64_type15_strips_embedded_newlines() {
        // MySQL JSON columns can inject literal newlines into the base64 body.
        let plain = r#"{"model":"gpt-4"}"#;
        let encoded = BASE64_STD.encode(plain.as_bytes());
        // Insert newlines mid-string the way a JSON column might.
        let with_newlines = format!(
            "{}\n{}",
            &encoded[..encoded.len() / 2],
            &encoded[encoded.len() / 2..]
        );
        let envelope = format!("base64:type15:{}", with_newlines);

        let decoded = decode_base64_type15(&envelope).unwrap();
        assert_eq!(decoded, plain);
    }

    #[test]
    fn test_decode_base64_type15_rejects_non_envelope() {
        assert!(decode_base64_type15("v2:gcm:abc").is_err());
        assert!(decode_base64_type15("{\"model\":\"gpt-4\"}").is_err());
    }

    // ━━━━━━━━━━━ proxy_url encrypt/decrypt/redact (Stage 122) ━━━━━━━━━━━

    const PROXY_KEY: &str = "sk-proxy-master-key";

    #[test]
    fn test_proxy_url_encrypt_decrypt_roundtrip() {
        let url = "http://user:pass@1.2.3.4:8080";
        let encrypted = encrypt_proxy_url(url, PROXY_KEY).unwrap();
        // Ciphertext must not leak the plaintext
        assert!(
            !encrypted.contains("user"),
            "ciphertext must not contain user"
        );
        assert!(
            !encrypted.contains("1.2.3.4"),
            "ciphertext must not contain host"
        );
        assert_eq!(decrypt_proxy_url(&encrypted, PROXY_KEY).unwrap(), url);
    }

    #[test]
    fn test_proxy_url_encrypt_wrong_key_fails() {
        let encrypted = encrypt_proxy_url("socks5://u:p@h:1080", PROXY_KEY).unwrap();
        assert!(decrypt_proxy_url(&encrypted, "wrong-key").is_err());
    }

    #[test]
    fn test_redact_proxy_url_with_password() {
        assert_eq!(
            redact_proxy_url("http://user:secret@1.2.3.4:8080"),
            "http://user:***@1.2.3.4:8080"
        );
        assert_eq!(
            redact_proxy_url("socks5://proxyuser@h:1080"),
            "socks5://proxyuser:***@h:1080"
        );
    }

    #[test]
    fn test_redact_proxy_url_no_credentials() {
        assert_eq!(
            redact_proxy_url("http://1.2.3.4:8080"),
            "http://1.2.3.4:8080"
        );
    }

    #[test]
    fn test_redact_proxy_url_malformed() {
        assert_eq!(redact_proxy_url("not-a-url"), "not-a-url");
    }
}
