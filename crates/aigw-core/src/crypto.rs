//! Cryptographic utilities — token hashing, constant-time comparison

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

#[cfg(test)]
mod tests {
    use super::*;

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
        // SHA256 hex output is always 64 characters
        let result = hash_token("hello-world");
        assert_eq!(result.len(), 64);
    }

    #[test]
    fn test_hash_token_empty() {
        let result = hash_token("");
        assert_eq!(result.len(), 64);
        // Known SHA256 empty string hash
        assert_eq!(
            result,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
