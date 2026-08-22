//! OAuth reverse-proxy pipeline (Phase 51, Stage 128).
//!
//! Any inbound protocol that resolves to an `anthropic_oauth` credential flows
//! through this pipeline instead of the normal per-provider adapter path:
//!
//! 1. Resolve a valid access token (TokenProvider: cache → refresh → cookie
//!    self-heal → needs_reauth).
//! 2. Adapt the body to Anthropic Messages format (chat / responses are
//!    converted; messages passes through unchanged).
//! 3. Inject the minimal billing block as `system[0]` (fingerprint byte-aligned
//!    with sub2api/Parrot) + optional credential `inject_prompt` block.
//! 4. Send through the credential's bound proxy (when present) with Claude-Code
//!    disguise headers and `Authorization: Bearer <access_token>`.
//! 5. On 401 → `TokenProvider::invalidate_and_refresh` → retry once.
//!
//! Reference: `docs/research/2026-08-18-sub2api-proxy-oauth-reference.md` §2.4/2.5.

use serde_json::{json, Value};
use sha2::Digest;

use crate::adapter::{
    ClientProtocol, MessageAdapter, OpenAIToAnthropic, ResponsesToChatCompletions,
};
use crate::claude_token::TokenError;
use crate::db::Database;
use crate::deployment::{Deployment, OAuthDeployment};
use crate::probe::build_proxy_client;

/// Claude Code CLI version used for the billing fingerprint + CC disguise
/// headers (aligned with sub2api `CLICurrentVersion`).
pub const CLI_CURRENT_VERSION: &str = "2.1.220";
/// Billing fingerprint salt (aligned with sub2api `gateway_billing_block.go`).
pub const BILLING_SALT: &str = "59cf53e54c78";
/// Anthropic API base for OAuth reverse-proxy targets.
pub const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com";
/// Test override: when `AIGW_ANTHROPIC_MOCK_BASE` is set, the pipeline targets
/// `{mock_base}/v1/messages` so the BDD harness can drive the reverse-proxy
/// against MockUpstream instead of the real api.anthropic.com.
///
/// **Production guard**: in a non-test deployment the env is IGNORED — a stray
/// `AIGW_ANTHROPIC_MOCK_BASE` in production would otherwise silently redirect
/// every OAuth reverse-proxy request (with the Bearer access token) to an
/// attacker-controlled server. The BDD harness sets the env inside the test
/// process and calls handlers via `Router::oneshot`; `cfg!(test)` is the only
/// reliable marker there (integration tests compile the crate's lib, so the
/// `#[cfg(test)]` block is inactive — `cfg!(test)` at the level works
/// for both).
fn target_base() -> String {
    // Only honour the mock override when the `test` feature is enabled (the
    // BDD harness builds aigw-core with that feature). `cfg!(test)` alone is
    // unreliable: aigw-core is compiled without cfg(test) for aigw-server's
    // integration BDD, so the mock remap would silently stop working.
    #[cfg(feature = "test")]
    {
        if let Ok(mock) = std::env::var("AIGW_ANTHROPIC_MOCK_BASE") {
            if !mock.is_empty() {
                return mock.trim_end_matches('/').to_string();
            }
        }
    }
    ANTHROPIC_API_BASE.to_string()
}
/// OAuth + Claude Code beta headers.
pub const ANTHROPIC_BETA: &str = "oauth-2025-04-20, claude-code-20250219";
/// count_tokens endpoint beta (Stage 128 §2.5).
pub const TOKEN_COUNTING_BETA: &str = "token-counting-2024-11-01";

/// Errors surfaced by the OAuth pipeline.
#[derive(Debug)]
pub enum PipelineError {
    /// Token acquisition / self-heal failure (404/needs_reauth/config).
    Token(TokenError),
    /// Request/body adaptation failure.
    Adapt(String),
    /// Upstream request failure (network / timeout / transport).
    Upstream(String),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineError::Token(e) => write!(f, "token: {}", e),
            PipelineError::Adapt(e) => write!(f, "adapt: {}", e),
            PipelineError::Upstream(e) => write!(f, "upstream: {}", e),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Billing fingerprint (byte-aligned with sub2api/Parrot)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Claude Code billing fingerprint for a request body.
///
/// `computeClaudeCodeFingerprint` (sub2api `gateway_billing_block.go`, byte-
/// aligned with Parrot `src/transform/cc_mimicry.py`):
///
/// 1. Take the **first** `role=user` message's pure text (first block `text`).
/// 2. `chars = text[4], text[7], text[20]` (each missing char → `'0'`).
/// 3. `SHA256("59cf53e54c78" + chars + version)` → hex, first 3 chars.
pub fn compute_claude_code_fingerprint(body: &Value, version: &str) -> String {
    let text = first_user_text(body);
    let chars = (0..3)
        .map(|i| {
            let idx = match i {
                0 => 4,
                1 => 7,
                _ => 20,
            };
            text.chars().nth(idx).unwrap_or('0')
        })
        .collect::<String>();
    let mut hasher = sha2::Sha256::new();
    hasher.update(BILLING_SALT.as_bytes());
    hasher.update(chars.as_bytes());
    hasher.update(version.as_bytes());
    let digest = hasher.finalize();
    hex::encode(digest)[..3].to_string()
}

/// Extract the pure text of the first `role=user` message (first block `text`).
fn first_user_text(body: &Value) -> String {
    let Some(messages) = body.get("messages").and_then(|v| v.as_array()) else {
        return String::new();
    };
    for m in messages {
        if m.get("role").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let content = m.get("content");
        return match content {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(blocks)) => blocks
                .iter()
                .find(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
                .and_then(|b| b.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            _ => String::new(),
        };
    }
    String::new()
}

/// Build the billing block `{type:"text", text:...}`.
fn billing_block(version: &str, fp: &str) -> Value {
    json!({
        "type": "text",
        "text": format!(
            "x-anthropic-billing-header: cc_version={}.{}; cc_entrypoint=cli;",
            version, fp
        )
    })
}

/// Inject the billing block as `system[0]` (unconditionally — sub2api aligns
/// the client's CC system to the billing header; a system without billing
/// block is judged third-party by the identity gate). The client's original
/// system blocks are preserved after; a credential `inject_prompt` is appended
/// as an extra block (the gate only inspects block[0]).
pub fn inject_billing_block(body: &mut Value, oauth: &OAuthDeployment) {
    let version = CLI_CURRENT_VERSION;
    let fp = compute_claude_code_fingerprint(body, version);
    let billing = billing_block(version, &fp);

    // Rewrite system[0] to the billing block; keep everything else.
    let mut new_system: Vec<Value> = vec![billing];
    if let Some(system) = body.get("system") {
        match system {
            Value::String(s) => {
                if !s.trim().is_empty() {
                    new_system.push(json!({"type": "text", "text": s}));
                }
            }
            Value::Array(blocks) => {
                for b in blocks {
                    if b.get("type").and_then(|v| v.as_str()) != Some("text") {
                        continue;
                    }
                    if b.get("text")
                        .and_then(|v| v.as_str())
                        .map(|t| t.starts_with("x-anthropic-billing-header:"))
                        .unwrap_or(false)
                    {
                        continue; // drop any pre-existing billing block
                    }
                    new_system.push(b.clone());
                }
            }
            _ => {}
        }
    }
    // Append the credential-level inject_prompt (after billing, gate only
    // inspects block[0]).
    if let Some(p) = oauth.inject_prompt.as_deref() {
        if !p.trim().is_empty() {
            new_system.push(json!({"type": "text", "text": p}));
        }
    }
    if let Some(obj) = body.as_object_mut() {
        obj.insert("system".to_string(), Value::Array(new_system));
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Protocol adaptation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Adapt an inbound body to Anthropic Messages format.
///
/// - `Anthropic` → passthrough (native protocol).
/// - `OpenAI` → `OpenAIToAnthropic`.
/// - `Responses` → `ResponsesToChatCompletions` → `OpenAIToAnthropic`.
pub fn adapt_to_anthropic(
    protocol: ClientProtocol,
    body: Value,
    deployment: &Deployment,
) -> Result<Value, String> {
    match protocol {
        ClientProtocol::Anthropic => Ok(body),
        ClientProtocol::OpenAI => OpenAIToAnthropic
            .adapt_request(body, deployment)
            .map_err(|e| e.to_string()),
        ClientProtocol::Responses => {
            let chat = ResponsesToChatCompletions
                .adapt_request(body, deployment)
                .map_err(|e| e.to_string())?;
            OpenAIToAnthropic
                .adapt_request(chat, deployment)
                .map_err(|e| e.to_string())
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// CC disguise headers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Apply the Claude-Code disguise header set (sub2api §2.5 minimal set).
/// The OAuth pipeline does **not** forward client `x-stainless-*`/UA headers —
/// they would conflict with the injected values and mark the request third-party.
pub fn apply_cc_headers(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    req.header(
        reqwest::header::USER_AGENT,
        format!("claude-cli/{} (external, cli)", CLI_CURRENT_VERSION),
    )
    .header("x-app", "cli")
    .header("X-Stainless-Lang", "js")
    .header("X-Stainless-Package-Version", "0.94.0")
    .header("X-Stainless-OS", "Linux")
    .header("X-Stainless-Arch", "arm64")
    .header("X-Stainless-Runtime", "node")
    .header("X-Stainless-Runtime-Version", "v24.3.0")
    .header("X-Stainless-Retry-Count", "0")
    .header("X-Stainless-Timeout", "600")
    .header(reqwest::header::ACCEPT, "application/json")
    .header("anthropic-version", "2023-06-01")
    .header("anthropic-beta", ANTHROPIC_BETA)
    .header("anthropic-dangerous-direct-browser-access", "true")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Send helper (proxy egress + Bearer + 401 refresh-retry)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Endpoint targets of the OAuth pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OauthTarget {
    /// `POST https://api.anthropic.com/v1/messages`
    Messages,
    /// `POST https://api.anthropic.com/v1/messages/count_tokens`
    CountTokens,
}

impl OauthTarget {
    fn path(&self) -> &'static str {
        match self {
            OauthTarget::Messages => "/v1/messages",
            OauthTarget::CountTokens => "/v1/messages/count_tokens",
        }
    }

    /// count_tokens uses the token-counting beta (no identity gate, but kept in
    /// the pipeline for consistency — Stage 128 §2.5).
    fn beta(&self) -> &'static str {
        match self {
            OauthTarget::Messages => ANTHROPIC_BETA,
            OauthTarget::CountTokens => TOKEN_COUNTING_BETA,
        }
    }
}

/// Send an already-adapted + billing-injected body through the OAuth pipeline.
///
/// Resolves the access token, applies CC disguise headers, egresses through the
/// credential's bound proxy (when present), and on a 401 invalidates the cached
/// token and retries once (Stage 127 `invalidate_and_refresh`).
pub async fn send(
    provider: &crate::claude_token::TokenProvider,
    db: &Database,
    master_key: &str,
    oauth: &OAuthDeployment,
    body: Value,
    target: OauthTarget,
) -> Result<reqwest::Response, PipelineError> {
    let token = provider
        .get_access_token(db, &oauth.credential_id, master_key)
        .await
        .map_err(PipelineError::Token)?;

    let client = match oauth.proxy_url.as_deref() {
        Some(proxy) => build_proxy_client(proxy, std::time::Duration::from_secs(600))
            .map_err(PipelineError::Upstream)?,
        None => reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| PipelineError::Upstream(format!("build client: {}", e)))?,
    };

    let url = format!("{}{}", target_base(), target.path());
    let build = |token: &str, beta: &str| {
        let mut req = client.post(&url).json(&body);
        req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {}", token));
        req = req.header("anthropic-beta", beta);
        apply_cc_headers(req)
    };

    let resp = build(&token, target.beta())
        .send()
        .await
        .map_err(|e| PipelineError::Upstream(format!("OAuth upstream request failed: {}", e)))?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        // Invalidate the cached token and force a refresh; retry once with the
        // fresh token. If the refresh chain fails → propagate.
        let fresh = provider
            .invalidate_and_refresh(db, &oauth.credential_id, master_key)
            .await
            .map_err(PipelineError::Token)?;
        let retry = build(&fresh, target.beta())
            .send()
            .await
            .map_err(|e| PipelineError::Upstream(format!("OAuth retry failed: {}", e)))?;
        // Retry STILL 401 → the account is rejected upstream (scope stripped /
        // revoked / region-blocked). The refresh path only self-heals when the
        // refresh_token itself is invalid; a valid token that the upstream
        // rejects means the account needs manual re-auth. Mark needs_reauth so
        // ops is alerted instead of the request silently failing forever.
        if retry.status() == reqwest::StatusCode::UNAUTHORIZED {
            if let Ok(Some(cred)) = db.get_credential_by_name(&oauth.credential_id).await {
                provider
                    .mark_needs_reauth(
                        db,
                        &cred,
                        master_key,
                        "OAuth upstream rejected access token after refresh (401)",
                    )
                    .await;
            }
        }
        return Ok(retry);
    }
    Ok(resp)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    fn oauth(inject: Option<&str>) -> OAuthDeployment {
        OAuthDeployment {
            credential_id: "cred-1".to_string(),
            proxy_url: None,
            inject_prompt: inject.map(String::from),
        }
    }

    /// Stage 128 §3.1 — billing fingerprint byte-aligned with sub2api/Parrot.
    #[test]
    fn test_billing_fingerprint_byte_aligned() {
        // Known vector: user text "0123456789..." → chars[4]='4', chars[7]='7',
        // chars[20]='0' (padded, text too short).
        let body = json!({
            "messages": [{"role": "user", "content": "0123456789"}]
        });
        let fp = compute_claude_code_fingerprint(&body, CLI_CURRENT_VERSION);
        assert_eq!(fp.len(), 3, "fingerprint must be 3 hex chars");
        // Deterministic: same input → same output.
        let again = compute_claude_code_fingerprint(&body, CLI_CURRENT_VERSION);
        assert_eq!(fp, again);
        // Padding: a too-short user text still yields a 3-char fingerprint.
        let short = json!({
            "messages": [{"role": "user", "content": "hi"}]
        });
        let fp2 = compute_claude_code_fingerprint(&short, CLI_CURRENT_VERSION);
        assert_eq!(fp2.len(), 3);
        assert_ne!(fp, fp2, "different text → different fingerprint");
        // Version changes the fingerprint.
        let fp3 = compute_claude_code_fingerprint(&body, "2.1.221");
        assert_ne!(fp, fp3);
    }

    /// Stage 128 §3.1 — billing block becomes system[0]; original system kept
    /// after; inject_prompt appended.
    #[test]
    fn test_billing_block_injected_first() {
        let mut body = json!({
            "system": [{"type": "text", "text": "original instructions"}],
            "messages": [{"role": "user", "content": "0123456789"}]
        });
        let oauth = oauth(Some("please follow my rules"));
        inject_billing_block(&mut body, &oauth);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 3, "billing + original + inject_prompt");
        let first = &system[0];
        assert_eq!(first["type"], "text");
        let text = first["text"].as_str().unwrap();
        assert!(
            text.starts_with("x-anthropic-billing-header: cc_version="),
            "billing block must be system[0], got: {}",
            text
        );
        assert!(text.contains("cc_entrypoint=cli;"));
        // Original system preserved after billing.
        assert_eq!(system[1]["text"], "original instructions");
        // inject_prompt appended last.
        assert_eq!(system[2]["text"], "please follow my rules");
    }

    /// inject_prompt None → only billing + original system.
    #[test]
    fn test_inject_prompt_appended_only_when_present() {
        let mut body = json!({
            "system": [{"type": "text", "text": "original"}],
            "messages": [{"role": "user", "content": "0123456789"}]
        });
        let oauth = oauth(None);
        inject_billing_block(&mut body, &oauth);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 2, "billing + original (no inject_prompt)");
    }

    /// A pre-existing billing block from the client must be dropped (we always
    /// compute our own to match our UA version).
    #[test]
    fn test_client_billing_block_replaced() {
        let mut body = json!({
            "system": [
                {"type": "text", "text": "x-anthropic-billing-header: cc_version=1.0.0.abc; cc_entrypoint=cli;"},
                {"type": "text", "text": "keep me"}
            ],
            "messages": [{"role": "user", "content": "0123456789"}]
        });
        let oauth = oauth(None);
        inject_billing_block(&mut body, &oauth);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 2);
        assert!(
            system[0]["text"]
                .as_str()
                .unwrap()
                .starts_with("x-anthropic-billing-header:"),
            "billing block must be regenerated first"
        );
        assert_eq!(system[1]["text"], "keep me");
    }

    /// Fingerprint from the first user block text (content array → first text block).
    #[test]
    fn test_fingerprint_uses_first_user_block_text() {
        let body = json!({
            "messages": [
                {"role": "system", "content": "ignored"},
                {"role": "user", "content": [{"type": "text", "text": "0123456789"}]}
            ]
        });
        let fp = compute_claude_code_fingerprint(&body, CLI_CURRENT_VERSION);
        // Same text as a plain-string user message → same fingerprint.
        let plain = json!({
            "messages": [{"role": "user", "content": "0123456789"}]
        });
        assert_eq!(
            fp,
            compute_claude_code_fingerprint(&plain, CLI_CURRENT_VERSION)
        );
    }

    /// Stage 128 §3.1 — chat → Anthropic conversion + billing injection.
    #[test]
    fn test_adapt_chat_to_anthropic() {
        let dep = Deployment {
            api_base: ANTHROPIC_API_BASE.to_string(),
            api_key: None,
            upstream_model: "claude-sonnet-5".to_string(),
            provider_type: crate::deployment::ProviderType::AnthropicNative,
            input_cost_per_token: None,
            output_cost_per_token: None,
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: json!({}),
            model_id: None,
            model_group: None,
            custom_llm_provider: Some("anthropic".to_string()),
            chat_template_compat: None,
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: Some(oauth(None)),
        };
        let body = json!({
            "model": "claude-sonnet-5",
            "messages": [{"role": "user", "content": "0123456789"}],
            "max_tokens": 100
        });
        let adapted = adapt_to_anthropic(ClientProtocol::OpenAI, body, &dep).unwrap();
        assert!(
            adapted.get("messages").is_some(),
            "chat→anthropic produces messages"
        );
        assert_eq!(adapted["model"], "claude-sonnet-5");
        // Responses → chat → anthropic chain.
        let resp = json!({
            "model": "claude-sonnet-5",
            "input": "0123456789",
            "max_output_tokens": 100
        });
        let adapted2 = adapt_to_anthropic(ClientProtocol::Responses, resp, &dep).unwrap();
        assert!(adapted2.get("messages").is_some());
        assert_eq!(adapted2["max_tokens"], 100);
    }

    /// Stage 128 §3.1 — messages protocol passes through unchanged.
    #[test]
    fn test_adapt_messages_passthrough() {
        let dep = Deployment {
            api_base: ANTHROPIC_API_BASE.to_string(),
            api_key: None,
            upstream_model: "claude-sonnet-5".to_string(),
            provider_type: crate::deployment::ProviderType::AnthropicNative,
            input_cost_per_token: None,
            output_cost_per_token: None,
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: json!({}),
            model_id: None,
            model_group: None,
            custom_llm_provider: Some("anthropic".to_string()),
            chat_template_compat: None,
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: Some(oauth(None)),
        };
        let body = json!({
            "model": "claude-sonnet-5",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100
        });
        let adapted = adapt_to_anthropic(ClientProtocol::Anthropic, body.clone(), &dep).unwrap();
        assert_eq!(adapted, body, "native messages passes through unchanged");
    }
}
