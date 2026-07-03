//! Auth middleware — Virtual Key + Master Key authentication
//!
//! Extracts Bearer token from Authorization header, hashes it,
//! and looks up in LiteLLM_VerificationToken table.
//! Falls back to master key check.

use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use std::fmt;

/// Extracted key identity after auth
#[derive(Debug, Clone)]
pub struct KeyIdentity {
    pub token_hash: String,
    pub key_alias: Option<String>,
    pub user_id: Option<String>,
    pub team_id: Option<String>,
    pub organization_id: Option<String>,
    pub is_master_key: bool,
}

/// Auth extraction error
#[derive(Debug)]
pub enum AuthError {
    MissingHeader,
    InvalidFormat,
    TokenNotFound,
    TokenExpired,
    TokenBlocked,
}

impl axum::response::IntoResponse for AuthError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            AuthError::MissingHeader => (StatusCode::UNAUTHORIZED, "Missing Authorization header"),
            AuthError::InvalidFormat => (StatusCode::UNAUTHORIZED, "Invalid Authorization format"),
            AuthError::TokenNotFound => (StatusCode::UNAUTHORIZED, "Invalid API key"),
            AuthError::TokenExpired => (StatusCode::UNAUTHORIZED, "API key expired"),
            AuthError::TokenBlocked => (StatusCode::FORBIDDEN, "API key blocked"),
        };
        let body = serde_json::json!({ "error": { "message": message, "type": "auth_error" } });
        (status, axum::Json(body)).into_response()
    }
}

/// Placeholder: Extract the Bearer token and validate
pub async fn authenticate(
    master_key_hash: &str,
    _db: &sqlx::SqlitePool,
    header: Option<&str>,
) -> std::result::Result<KeyIdentity, AuthError> {
    let header = header.ok_or(AuthError::MissingHeader)?;

    let token = header
        .strip_prefix("Bearer ")
        .ok_or(AuthError::InvalidFormat)?;

    // Master key check
    // TODO (Stage 2): use constant-time comparison
    if token == master_key_hash {
        return Ok(KeyIdentity {
            token_hash: master_key_hash.to_string(),
            key_alias: Some("master".to_string()),
            user_id: None,
            team_id: None,
            organization_id: None,
            is_master_key: true,
        });
    }

    // Hash and lookup in DB
    // TODO (Stage 2): SHA256 hash, DB lookup, expiry/blocked check
    Err(AuthError::TokenNotFound)
}
