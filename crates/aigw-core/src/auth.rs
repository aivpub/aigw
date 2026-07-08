//! JWT auth utilities — HS256 token encode/decode using master_key as secret.
//!
//! Compatible with litellm's `/v2/login` JWT payload structure.

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

/// JWT claims matching litellm's /v2/login token payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub user_id: String,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
    pub user_role: String,
    pub login_method: String,
}

/// Encode a JWT using HS256 with master_key as secret.
pub fn encode_jwt(claims: &JwtClaims, master_key: &str) -> Result<String, String> {
    let key = EncodingKey::from_secret(master_key.as_bytes());
    encode(&Header::default(), claims, &key)
        .map_err(|e| format!("JWT encode failed: {}", e))
}

/// Decode and validate a JWT using HS256 with master_key as secret.
pub fn decode_jwt(token: &str, master_key: &str) -> Result<JwtClaims, String> {
    let key = DecodingKey::from_secret(master_key.as_bytes());
    let mut validation = Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.required_spec_claims = std::collections::HashSet::new(); // no exp required (DB-backed expiry)
    decode::<JwtClaims>(token, &key, &validation)
        .map(|data| data.claims)
        .map_err(|e| format!("JWT decode failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_roundtrip() {
        let master_key = "sk-test-master-key";
        let claims = JwtClaims {
            user_id: "default_user_id".to_string(),
            key: "sk-test-session-key-12345".to_string(),
            user_email: None,
            user_role: "proxy_admin".to_string(),
            login_method: "username_password".to_string(),
        };

        let token = encode_jwt(&claims, master_key).unwrap();
        let decoded = decode_jwt(&token, master_key).unwrap();

        assert_eq!(decoded.user_id, claims.user_id);
        assert_eq!(decoded.key, claims.key);
        assert_eq!(decoded.user_role, claims.user_role);
        assert_eq!(decoded.login_method, claims.login_method);
    }

    #[test]
    fn test_jwt_wrong_key() {
        let claims = JwtClaims {
            user_id: "u1".to_string(),
            key: "sk-xxx".to_string(),
            user_email: None,
            user_role: "proxy_admin".to_string(),
            login_method: "username_password".to_string(),
        };

        let token = encode_jwt(&claims, "correct-key").unwrap();
        let result = decode_jwt(&token, "wrong-key");
        assert!(result.is_err());
    }
}
