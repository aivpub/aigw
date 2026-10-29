//! Outbound credential resolution and token sealing.
//!
//! The gateway never forwards the client's `Authorization` (an aigw virtual
//! key) to an upstream MCP server — the MCP spec forbids it, and doing so would
//! hand the gateway key to every upstream. The only outbound credential is a
//! row from `mcp_server_tokens`, decrypted once at egress.

use chrono::{DateTime, Utc};

use crate::crypto::{decrypt_litellm_value, encrypt_litellm_value_gcm};
use crate::models::{CredentialScope, McpServerTokenRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    User,
    Server,
}

/// The concrete token a request will use, for callers that need more than the
/// source (e.g. `mcp_call_log`'s `token_owner`, or an expiry warning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedToken {
    pub source: TokenSource,
    pub token_id: i64,
    pub expires_at: Option<String>,
}

impl ResolvedToken {
    pub fn new(source: TokenSource, token: &McpServerTokenRow) -> Self {
        ResolvedToken {
            source,
            token_id: token.id,
            expires_at: token.expires_at.clone(),
        }
    }
}

/// "Usable" = enabled and (no expiry or expiry in the future).
///
/// The comparison runs here rather than in SQL: the three dialects spell
/// datetime comparison differently, and a token set is a few rows per user —
/// not worth dialect-specific SQL (stage-140 §8). An unparseable `expires_at`
/// counts as expired: the safe direction is to withhold the credential.
pub fn is_token_usable(token: &McpServerTokenRow, now: DateTime<Utc>) -> bool {
    if !token.enabled {
        return false;
    }
    match &token.expires_at {
        None => true,
        Some(raw) => DateTime::parse_from_rfc3339(raw)
            .map(|expires| expires.with_timezone(&Utc) > now)
            .unwrap_or(false),
    }
}

/// Pick whose token goes outbound.
///
/// Takes only rows from `mcp_server_tokens` — there is no parameter a client
/// credential could occupy, which is the compile-time half of the
/// "never forward the client key" rule.
///
/// `user_required` never falls back to the shared token: falling back would
/// silently downgrade per-person permissions to `shared_or_user`.
pub fn resolve_outbound_token(
    scope: CredentialScope,
    user_token: Option<&McpServerTokenRow>,
    server_token: Option<&McpServerTokenRow>,
) -> Option<TokenSource> {
    let now = Utc::now();
    let user = user_token.filter(|t| is_token_usable(t, now));
    let server = server_token.filter(|t| is_token_usable(t, now));

    match scope {
        CredentialScope::Shared => server.map(|_| TokenSource::Server),
        CredentialScope::SharedOrUser => user
            .map(|_| TokenSource::User)
            .or_else(|| server.map(|_| TokenSource::Server)),
        CredentialScope::UserRequired => user.map(|_| TokenSource::User),
    }
}

/// Encrypt a token for storage. What lands in the DB is always `v2:gcm:`
/// ciphertext; plaintext exists only between this call and the request.
pub fn seal_token(plaintext: &str, master_key: &str) -> Result<String, String> {
    encrypt_litellm_value_gcm(plaintext, master_key)
}

pub fn unseal_token(ciphertext: &str, master_key: &str) -> Result<String, String> {
    decrypt_litellm_value(ciphertext, master_key)
}

/// Response form of a token: never the plaintext, never even the ciphertext.
/// Empty means "not set" so the UI can tell the two apart.
pub fn redact_token(ciphertext: &str) -> String {
    if ciphertext.is_empty() {
        String::new()
    } else {
        "***".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(id: i64, enabled: bool, expires_at: Option<&str>) -> McpServerTokenRow {
        McpServerTokenRow {
            id,
            server_id: 1,
            owner_type: "server".to_string(),
            owner_id: String::new(),
            token_name: format!("t{id}"),
            token_value: "v2:gcm:xxx".to_string(),
            is_default: id == 1,
            enabled,
            expires_at: expires_at.map(str::to_string),
            created_by: String::new(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn future() -> String {
        (Utc::now() + chrono::Duration::hours(1)).to_rfc3339()
    }

    fn past() -> String {
        (Utc::now() - chrono::Duration::hours(1)).to_rfc3339()
    }

    #[test]
    fn resolve_outbound_token_table() {
        let user = token(2, true, None);
        let server = token(1, true, None);

        struct Case {
            name: &'static str,
            scope: CredentialScope,
            user: Option<McpServerTokenRow>,
            server: Option<McpServerTokenRow>,
            expect: Option<TokenSource>,
        }

        let cases = vec![
            Case {
                name: "shared + shared token",
                scope: CredentialScope::Shared,
                user: None,
                server: Some(server.clone()),
                expect: Some(TokenSource::Server),
            },
            Case {
                name: "shared + no token",
                scope: CredentialScope::Shared,
                user: None,
                server: None,
                expect: None,
            },
            Case {
                name: "shared ignores a user token when a shared one exists",
                scope: CredentialScope::Shared,
                user: Some(user.clone()),
                server: Some(server.clone()),
                expect: Some(TokenSource::Server),
            },
            Case {
                name: "shared_or_user + both",
                scope: CredentialScope::SharedOrUser,
                user: Some(user.clone()),
                server: Some(server.clone()),
                expect: Some(TokenSource::User),
            },
            Case {
                name: "shared_or_user + shared only",
                scope: CredentialScope::SharedOrUser,
                user: None,
                server: Some(server.clone()),
                expect: Some(TokenSource::Server),
            },
            Case {
                name: "shared_or_user + neither",
                scope: CredentialScope::SharedOrUser,
                user: None,
                server: None,
                expect: None,
            },
            Case {
                name: "user_required + both",
                scope: CredentialScope::UserRequired,
                user: Some(user.clone()),
                server: Some(server.clone()),
                expect: Some(TokenSource::User),
            },
            Case {
                name: "user_required + shared only must not fall back",
                scope: CredentialScope::UserRequired,
                user: None,
                server: Some(server.clone()),
                expect: None,
            },
            Case {
                name: "user_required + neither",
                scope: CredentialScope::UserRequired,
                user: None,
                server: None,
                expect: None,
            },
            Case {
                name: "disabled user token is absent",
                scope: CredentialScope::UserRequired,
                user: Some(token(3, false, None)),
                server: None,
                expect: None,
            },
            Case {
                name: "disabled token does not win in shared_or_user",
                scope: CredentialScope::SharedOrUser,
                user: Some(token(3, false, None)),
                server: Some(server.clone()),
                expect: Some(TokenSource::Server),
            },
            Case {
                name: "expired user token is absent",
                scope: CredentialScope::UserRequired,
                user: Some(token(3, true, Some(&past()))),
                server: None,
                expect: None,
            },
            Case {
                name: "expired token does not win in shared_or_user",
                scope: CredentialScope::SharedOrUser,
                user: Some(token(3, true, Some(&past()))),
                server: Some(server.clone()),
                expect: Some(TokenSource::Server),
            },
            Case {
                name: "future expiry is still usable",
                scope: CredentialScope::UserRequired,
                user: Some(token(3, true, Some(&future()))),
                server: None,
                expect: Some(TokenSource::User),
            },
        ];

        for case in cases {
            let got = resolve_outbound_token(case.scope, case.user.as_ref(), case.server.as_ref());
            assert_eq!(got, case.expect, "case: {}", case.name);
        }
    }

    #[test]
    fn is_token_usable_rejects_unparseable_expiry() {
        let now = Utc::now();
        assert!(!is_token_usable(&token(1, true, Some("not-a-date")), now));
        assert!(is_token_usable(&token(1, true, None), now));
        assert!(!is_token_usable(&token(1, false, None), now));
    }

    #[test]
    fn seal_and_unseal_roundtrip_yields_gcm_envelope() {
        let sealed = seal_token("s3cret", "master-key").expect("seal");
        assert!(sealed.starts_with("v2:gcm:"), "stored form must be GCM");
        assert_eq!(
            unseal_token(&sealed, "master-key").expect("unseal"),
            "s3cret"
        );
        assert!(unseal_token(&sealed, "wrong-key").is_err());
    }

    #[test]
    fn redact_token_hides_everything_but_presence() {
        assert_eq!(redact_token("v2:gcm:abc"), "***");
        assert_eq!(redact_token(""), "");
    }
}
