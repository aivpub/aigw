//! Claude OAuth client — cookie (`sk-ant-sid`) → access/refresh token 3-step
//! exchange (Phase 51, Stage 126).
//!
//! Implements sub2api's `claude_oauth_service.go` (reference:
//! `docs/research/2026-08-18-sub2api-proxy-oauth-reference.md` §2.1/2.2):
//!
//! 1. `GET claude.ai/api/organizations` with cookie `sessionKey=<sk-ant-sid>` →
//!    org list; multi-org prefers the `team` raven_type.
//! 2. `POST claude.ai/v1/oauth/{org}/authorize` (PKCE S256 challenge) → parse
//!    `code` + `state` from the returned `redirect_uri`.
//! 3. `POST platform.claude.com/v1/oauth/token` (authorization_code) → token pair.
//!
//! All three steps run through the proxy client (`build_proxy_client`, Stage 123)
//! when a proxy is bound. Refresh uses the same token endpoint with
//! `grant_type=refresh_token` (wired in Stage 127).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OAuth constants (aligned with sub2api `oauth.go`)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
pub const SCOPE_API: &str =
    "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
/// access_token TTL returned by the token endpoint (28800 s = 8 h).
pub const DEFAULT_EXPIRES_IN: i64 = 28800;

/// Test override: when `AIGW_OAUTH_MOCK_BASE` is set, the OAuth client rewrites
/// its orgs/authorize/token endpoints to `{mock_base}/api/organizations`,
/// `{mock_base}/v1/oauth/{org}/authorize`, `{mock_base}/v1/oauth/token` so the
/// BDD harness can drive the 3-step exchange against MockUpstream instead of
/// the real claude.ai. **Never set in production** — only the BDD harness sets
/// it (claude_oauth_steps.rs). This avoids touching the public URL constants in
/// the request path.
fn endpoint(base: &str, path: &str) -> String {
    match std::env::var("AIGW_OAUTH_MOCK_BASE") {
        Ok(mock) if !mock.is_empty() => format!("{}/{}", mock.trim_end_matches('/'), path),
        _ => format!("{}{}", base, path),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// An org entry returned by Step 1 (`GET /api/organizations`).
#[derive(Debug, Clone, Deserialize)]
pub struct OauthOrg {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub raven_type: Option<String>,
}

/// A successful Step 3 token response.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    pub refresh_token: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub organization: Option<Value>,
    #[serde(default)]
    pub account: Option<Value>,
}

/// Structured error classification (sub-check `classifyStep1Error`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OauthError {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cf_ray: Option<String>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// PKCE
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Generate a PKCE S256 verifier (32 random bytes → base64url-nopad, 43 chars)
/// and its SHA-256 challenge.
pub fn pkce_s256() -> (String, String) {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("RNG for PKCE verifier");
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let digest = hasher.finalize();
    let challenge = URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Error classification
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Cloudflare challenge signatures (sub-check `classifyStep1Error`, same set as
/// `aigw_core::probe::detect_cf_challenge`).
const CF_SIGNATURES: &[&str] = &[
    "just a moment",
    "cf-ray",
    "cf-mitigated",
    "challenges.cloudflare.com",
    "attention required",
];

/// Classify a Step-1/authorize HTTP error into a structured `OauthError`.
///
/// Order matters (sub-check classifies CF **before** the 403 branch): CF
/// signatures first, then the known Anthropic error bodies.
pub fn classify_oauth_error(
    status: u16,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
) -> OauthError {
    // Cloudflare challenge — check headers + body signatures first.
    let ray = headers
        .get("cf-ray")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .or_else(|| {
            if headers.contains_key("cf-mitigated") {
                Some("cf-mitigated".to_string())
            } else {
                None
            }
        });
    if let Some(ray_id) = ray {
        return OauthError {
            kind: "cf_challenge".to_string(),
            message:
                "Cloudflare 正在挑战该代理 IP — 请更换出口节点（住宅/干净池）。cookie 本身未评估。"
                    .to_string(),
            cf_ray: Some(ray_id),
        };
    }
    let text = String::from_utf8_lossy(body).to_lowercase();
    if CF_SIGNATURES.iter().any(|s| text.contains(s)) {
        return OauthError {
            kind: "cf_challenge".to_string(),
            message: "Cloudflare challenge detected on OAuth endpoint".to_string(),
            cf_ray: None,
        };
    }
    // 403 with HTML (non-JSON) is CF/HTML block even without a signature match.
    if status == 403 && text.trim_start().starts_with('<') {
        return OauthError {
            kind: "cf_challenge".to_string(),
            message: "HTML block on OAuth endpoint (Cloudflare)".to_string(),
            cf_ray: None,
        };
    }
    // Anthropic error bodies (lowercased body searched for known markers).
    if text.contains("account_session_invalid") {
        return OauthError {
            kind: "account_session_invalid".to_string(),
            message: "cookie 已吊销 — 请重新获取 sk-ant-sid".to_string(),
            cf_ray: None,
        };
    }
    if text.contains("account_disabled")
        || text.contains("account_suspended")
        || text.contains("account_terminated")
    {
        return OauthError {
            kind: "account_blocked".to_string(),
            message: "账号被封禁/停用".to_string(),
            cf_ray: None,
        };
    }
    match status {
        401 => OauthError {
            kind: "unauthorized".to_string(),
            message: "cookie 已过期或无效 (401)".to_string(),
            cf_ray: None,
        },
        403 => OauthError {
            kind: "forbidden".to_string(),
            message: "cookie/geo/IP block (403)".to_string(),
            cf_ray: None,
        },
        429 => OauthError {
            kind: "rate_limited".to_string(),
            message: "限流 (429) — 稍后重试".to_string(),
            cf_ray: None,
        },
        s if s >= 500 => OauthError {
            kind: "upstream_error".to_string(),
            message: format!("上游不稳定 ({})", s),
            cf_ray: None,
        },
        _ => OauthError {
            kind: "unknown".to_string(),
            message: format!("未知错误 ({})", status),
            cf_ray: None,
        },
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Exchange client
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Client for the 3-step cookie→token exchange. Holds an optional proxy URL so
/// every HTTP call goes through the bound proxy (or direct when None).
#[derive(Debug, Clone)]
pub struct OauthClient {
    client: reqwest::Client,
}

impl OauthClient {
    /// Build an OAuth client. `proxy_url` may be `None` for direct connection
    /// (the exchange endpoint validates proxy existence first; here we just
    /// build the client, failing fast on a malformed proxy URL).
    pub fn new(proxy_url: Option<&str>) -> Result<Self, String> {
        let client = match proxy_url {
            Some(url) => crate::probe::build_proxy_client(url, std::time::Duration::from_secs(60))?,
            None => reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .map_err(|e| format!("build direct OAuth client: {}", e))?,
        };
        Ok(Self { client })
    }

    /// Step 1: list organizations with the session cookie. Returns the org list.
    async fn fetch_orgs(&self, session_key: &str) -> Result<Vec<OauthOrg>, OauthError> {
        let resp = self
            .client
            .get(endpoint("https://claude.ai", "api/organizations"))
            .header(
                reqwest::header::COOKIE,
                format!("sessionKey={}", session_key),
            )
            .header(reqwest::header::USER_AGENT, OAUTH_UA)
            .send()
            .await
            .map_err(|e| OauthError {
                kind: "network".to_string(),
                message: format!("orgs request failed: {}", e),
                cf_ray: None,
            })?;
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| OauthError {
                kind: "network".to_string(),
                message: format!("orgs body read failed: {}", e),
                cf_ray: None,
            })?
            .to_vec();
        if status >= 400 {
            return Err(classify_oauth_error(status, &headers, &bytes));
        }
        serde_json::from_slice::<Vec<OauthOrg>>(&bytes).map_err(|_| OauthError {
            kind: "parse".to_string(),
            message: "orgs response not a JSON array".to_string(),
            cf_ray: None,
        })
    }

    /// Step 2: authorize for the selected org → parse `code` + `state` from the
    /// returned `redirect_uri`.
    async fn authorize(
        &self,
        session_key: &str,
        org_uuid: &str,
        verifier: &str,
        challenge: &str,
        state: &str,
    ) -> Result<(String, String), OauthError> {
        let url = endpoint(
            "https://claude.ai",
            &format!("v1/oauth/{}/authorize", org_uuid),
        );
        let body = json!({
            "response_type": "code",
            "client_id": CLIENT_ID,
            "organization_uuid": org_uuid,
            "redirect_uri": REDIRECT_URI,
            "scope": SCOPE_API,
            "state": state,
            "code_verifier": verifier,
            "code_challenge": challenge,
            "code_challenge_method": "S256",
        });
        let resp = self
            .client
            .post(url)
            .header(
                reqwest::header::COOKIE,
                format!("sessionKey={}", session_key),
            )
            .header(reqwest::header::USER_AGENT, OAUTH_UA)
            .json(&body)
            .send()
            .await
            .map_err(|e| OauthError {
                kind: "network".to_string(),
                message: format!("authorize request failed: {}", e),
                cf_ray: None,
            })?;
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| OauthError {
                kind: "network".to_string(),
                message: format!("authorize body read failed: {}", e),
                cf_ray: None,
            })?
            .to_vec();
        if status >= 400 {
            return Err(classify_oauth_error(status, &headers, &bytes));
        }
        // sub2api parses the redirect_uri field out of the JSON response.
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| OauthError {
            kind: "parse".to_string(),
            message: "authorize response not JSON".to_string(),
            cf_ray: None,
        })?;
        let redirect = value
            .get("redirect_uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OauthError {
                kind: "parse".to_string(),
                message: "authorize response missing redirect_uri".to_string(),
                cf_ray: None,
            })?;
        parse_redirect_code(redirect, state)
    }

    /// Step 3: exchange the authorization code for a token pair.
    async fn exchange_code(
        &self,
        code: &str,
        state: &str,
        verifier: &str,
    ) -> Result<TokenResponse, OauthError> {
        let body = json!({
            "code": code,
            "grant_type": "authorization_code",
            "client_id": CLIENT_ID,
            "redirect_uri": REDIRECT_URI,
            "code_verifier": verifier,
            "state": state,
        });
        self.post_token(&body).await
    }

    /// Refresh grant (wired by Stage 127; exposed here so the token lifecycle
    /// can reuse the same endpoint + client).
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenResponse, OauthError> {
        let body = json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": CLIENT_ID,
        });
        self.post_token(&body).await
    }

    /// POST to the token endpoint with a JSON body (shared by code + refresh).
    async fn post_token(&self, body: &Value) -> Result<TokenResponse, OauthError> {
        let resp = self
            .client
            .post(endpoint("https://platform.claude.com", "v1/oauth/token"))
            .header(reqwest::header::USER_AGENT, OAUTH_UA)
            .json(body)
            .send()
            .await
            .map_err(|e| OauthError {
                kind: "network".to_string(),
                message: format!("token request failed: {}", e),
                cf_ray: None,
            })?;
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| OauthError {
                kind: "network".to_string(),
                message: format!("token body read failed: {}", e),
                cf_ray: None,
            })?
            .to_vec();
        if status >= 400 {
            return Err(classify_oauth_error(status, &headers, &bytes));
        }
        serde_json::from_slice::<TokenResponse>(&bytes).map_err(|_| OauthError {
            kind: "parse".to_string(),
            message: "token response not JSON".to_string(),
            cf_ray: None,
        })
    }

    /// Full 3-step cookie→token exchange.
    ///
    /// Returns the `TokenResponse` plus the selected `org_uuid` (so the caller
    /// can persist it in the OAuth credential).
    pub async fn exchange(&self, session_key: &str) -> Result<(TokenResponse, String), OauthError> {
        let orgs = self.fetch_orgs(session_key).await?;
        let org = select_org(&orgs)?;
        let (verifier, challenge) = pkce_s256();
        let state = pkce_state();
        let (code, _) = self
            .authorize(session_key, &org.uuid, &verifier, &challenge, &state)
            .await?;
        let token = self.exchange_code(&code, &state, &verifier).await?;
        Ok((token, org.uuid))
    }
}

/// Browser-like UA for claude.ai (Cloudflare-facing endpoints are strict).
const OAUTH_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// Random state parameter (opaque, base64url).
pub fn pkce_state() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("RNG for OAuth state");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Select the org to authorize against: prefer `raven_type == "team"`, else the
/// first org. Error when the list is empty.
pub fn select_org(orgs: &[OauthOrg]) -> Result<OauthOrg, OauthError> {
    orgs.iter()
        .find(|o| o.raven_type.as_deref() == Some("team"))
        .cloned()
        .or_else(|| orgs.first().cloned())
        .ok_or_else(|| OauthError {
            kind: "no_org".to_string(),
            message: "账号下没有可用组织".to_string(),
            cf_ray: None,
        })
}

/// Parse `code` + `state` out of a redirect URI like
/// `https://platform.claude.com/oauth/code/callback?code=...&state=...`.
///
/// Returns `(code, state)`; state mismatch is an error.
pub fn parse_redirect_code(
    redirect: &str,
    expected_state: &str,
) -> Result<(String, String), OauthError> {
    let (_, query) = redirect.split_once('?').ok_or_else(|| OauthError {
        kind: "parse".to_string(),
        message: "redirect_uri missing query string".to_string(),
        cf_ray: None,
    })?;
    let params: std::collections::HashMap<String, String> = query
        .split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect();
    let code = params.get("code").cloned().ok_or_else(|| OauthError {
        kind: "parse".to_string(),
        message: "redirect_uri missing code".to_string(),
        cf_ray: None,
    })?;
    let state = params.get("state").cloned().unwrap_or_default();
    if !state.is_empty() && state != expected_state {
        return Err(OauthError {
            kind: "state_mismatch".to_string(),
            message: "OAuth state mismatch".to_string(),
            cf_ray: None,
        });
    }
    Ok((code, state))
}

/// Build an OAuth credential `credential_values` object with the sensitive
/// token fields individually encrypted (`v2:gcm:`).
///
/// Returns the JSON to store in `credentials.credential_values` (the encrypted
/// strings are opaque to the admin response until decrypted server-side).
pub fn build_oauth_credential_values(
    session_key: &str,
    token: &TokenResponse,
    org_uuid: &str,
    proxy_id: Option<i64>,
    inject_prompt: Option<&str>,
    master_key: &str,
) -> Result<Value, String> {
    let enc_session = crate::crypto::encrypt_litellm_value(session_key, master_key)?;
    let enc_access = crate::crypto::encrypt_litellm_value(&token.access_token, master_key)?;
    let enc_refresh = crate::crypto::encrypt_litellm_value(&token.refresh_token, master_key)?;
    let expires_at =
        chrono::Utc::now().timestamp() + token.expires_in.unwrap_or(DEFAULT_EXPIRES_IN);
    let account_uuid = token
        .account
        .as_ref()
        .and_then(|a| a.get("uuid"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let email_address = token
        .account
        .as_ref()
        .and_then(|a| a.get("email_address"))
        .and_then(|v| v.as_str())
        .map(String::from);
    Ok(json!({
        "type": "anthropic_oauth",
        "access_token": enc_access,
        "refresh_token": enc_refresh,
        "session_key": enc_session,
        "expires_at": expires_at,
        "proxy_id": proxy_id,
        "inject_prompt": inject_prompt,
        "org_uuid": org_uuid,
        "account_uuid": account_uuid,
        "email_address": email_address,
        "status": "active",
        "last_error": null,
    }))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_verifier_challenge() {
        let (verifier, challenge) = pkce_s256();
        // 32 bytes base64url-nopad → 43 chars.
        assert_eq!(verifier.len(), 43);
        // challenge must equal SHA256(verifier) base64url-nopad.
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let digest = hasher.finalize();
        assert_eq!(challenge, URL_SAFE_NO_PAD.encode(digest));
    }

    #[test]
    fn test_select_org_single() {
        let orgs = vec![OauthOrg {
            uuid: "org-1".to_string(),
            name: "Personal".to_string(),
            raven_type: None,
        }];
        let selected = select_org(&orgs).unwrap();
        assert_eq!(selected.uuid, "org-1");
    }

    #[test]
    fn test_select_org_prefers_team() {
        let orgs = vec![
            OauthOrg {
                uuid: "org-personal".to_string(),
                name: "Personal".to_string(),
                raven_type: None,
            },
            OauthOrg {
                uuid: "org-team".to_string(),
                name: "Work".to_string(),
                raven_type: Some("team".to_string()),
            },
        ];
        let selected = select_org(&orgs).unwrap();
        assert_eq!(selected.uuid, "org-team");
    }

    #[test]
    fn test_select_org_empty_errors() {
        let err = select_org(&[]).unwrap_err();
        assert_eq!(err.kind, "no_org");
    }

    #[test]
    fn test_parse_redirect_code_success() {
        let redirect = "https://platform.claude.com/oauth/code/callback?code=abc123&state=xyz789";
        let (code, state) = parse_redirect_code(redirect, "xyz789").unwrap();
        assert_eq!(code, "abc123");
        assert_eq!(state, "xyz789");
    }

    #[test]
    fn test_parse_redirect_code_state_mismatch() {
        let redirect = "https://platform.claude.com/oauth/code/callback?code=abc123&state=wrong";
        let err = parse_redirect_code(redirect, "expected").unwrap_err();
        assert_eq!(err.kind, "state_mismatch");
    }

    #[test]
    fn test_classify_cf_challenge() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("cf-ray", "ray123".parse().unwrap());
        let err = classify_oauth_error(403, &headers, b"{}");
        assert_eq!(err.kind, "cf_challenge");
        assert_eq!(err.cf_ray.as_deref(), Some("ray123"));
    }

    #[test]
    fn test_classify_account_session_invalid() {
        let headers = reqwest::header::HeaderMap::new();
        let err = classify_oauth_error(401, &headers, br#"{"error":"account_session_invalid"}"#);
        assert_eq!(err.kind, "account_session_invalid");
    }

    #[test]
    fn test_classify_rate_limited() {
        let headers = reqwest::header::HeaderMap::new();
        let err = classify_oauth_error(429, &headers, b"{}");
        assert_eq!(err.kind, "rate_limited");
    }

    #[test]
    fn test_build_oauth_credential_encrypts_sensitive() {
        let token = TokenResponse {
            access_token: "sk-ant-access-secret".to_string(),
            token_type: Some("Bearer".to_string()),
            expires_in: Some(28800),
            refresh_token: "sk-ant-refresh-secret".to_string(),
            scope: Some("user:inference".to_string()),
            organization: Some(json!({"uuid": "org-1"})),
            account: Some(json!({"uuid": "acc-1", "email_address": "a@b.c"})),
        };
        let values = build_oauth_credential_values(
            "sk-ant-sid-123",
            &token,
            "org-1",
            Some(3),
            Some("please follow instructions"),
            "sk-test-master",
        )
        .unwrap();
        assert_eq!(values["type"], "anthropic_oauth");
        assert_eq!(values["proxy_id"], 3);
        assert_eq!(values["org_uuid"], "org-1");
        assert_eq!(values["account_uuid"], "acc-1");
        assert_eq!(values["email_address"], "a@b.c");
        assert_eq!(values["status"], "active");
        // Sensitive fields must be encrypted ciphertext, not plaintext.
        let access = values["access_token"].as_str().unwrap();
        assert!(!access.contains("sk-ant-access-secret"));
        // NaCl SecretBox ciphertext is base64 — must not contain the plaintext.
        assert!(!access.starts_with('{'));
        let refresh = values["refresh_token"].as_str().unwrap();
        assert!(!refresh.contains("sk-ant-refresh-secret"));
        let session = values["session_key"].as_str().unwrap();
        assert!(!session.contains("sk-ant-sid"));
        // Decrypt roundtrip proves the master key recovers them.
        let dec_access = crate::crypto::decrypt_litellm_value(access, "sk-test-master").unwrap();
        assert_eq!(dec_access, "sk-ant-access-secret");
    }
}
