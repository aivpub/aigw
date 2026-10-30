//! Custom outbound header name and extra upstream headers (Phase 56, Stage 146).
//!
//! Adding a `headers.rs` rather than a general-purpose HTTP client is on
//! purpose: the wire-level effect is "resolved token into a differently-named
//! header, plus up to N static headers". Everything here is a total function
//! over `(&str, &str)` pairs, so the same decisions (which names are legal,
//! which are reserved) are made once and enforced by `mcp::probe` and
//! `mcp::session` alike — see [`crate::mcp::session::UpstreamAuth`].
//!
//! Three rules carry weight:
//!
//! 1. **Reserved names are rejected, not overwritten.** `Mcp-Session-Id` is
//!    derived from the session of the moment; letting an administrator pin it
//!    would silently cross sessions between upstreams.
//! 2. **The auth header and the extra-header list may not collide.** Two write
//!    points for one header leaves the winner dependent on injection order,
//!    which is not something an operator can reason about.
//! 3. **Values never leave the gateway.** The store holds them sealed
//!    (`seal_token`) and every read path serialises `***`; the only consumer of
//!    the plaintext is `mcp::session`'s request builder.

use serde::{Deserialize, Serialize};

use crate::mcp::session::UpstreamAuth;

/// Standard headers the gateway writes from protocol/runtime state. An
/// operator overriding any of these can make the gateway emit an invalid MCP
/// request, and the failure shows up on the upstream side where it costs the
/// most to diagnose.
pub const RESERVED_HEADERS: [&str; 6] = [
    "content-type",
    "accept",
    "mcp-protocol-version",
    "mcp-session-id",
    "host",
    "content-length",
];

pub const MAX_HEADER_NAME_LEN: usize = 256;
pub const MAX_HEADER_VALUE_LEN: usize = 4096;
pub const MAX_EXTRA_HEADERS: usize = 32;
/// Serialised `extra_headers` cap. Mirrors the plaintext side; the ciphertext is
/// roughly 1.4x this and the columns are `TEXT`, so nothing overflows — the
/// limit exists so a bad UI cannot stuff megabytes into one row.
pub const MAX_EXTRA_HEADERS_BYTES: usize = 8192;

/// One extra header. The value is plaintext **here** — this type is what
/// crosses the encrypt/decrypt boundary, never what goes into a response body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraHeader {
    pub name: String,
    pub value: String,
}

/// Why a header configuration was rejected. Each variant maps to one error type
/// on the admin API, so the UI can point at the offending control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderError {
    ReservedName(String),
    InvalidName(String),
    ValueHasNewline(String),
    ValueHasInvalidChar(String, char),
    ValueIsRedacted(String),
    NameTooLong(String),
    ValueTooLong(String),
    TooManyHeaders(usize),
    TooLarge(usize),
    DuplicateName(String),
    AuthNameCollision(String),
}

impl HeaderError {
    pub fn kind(&self) -> &'static str {
        match self {
            HeaderError::ReservedName(_) => "invalid_header_name",
            HeaderError::InvalidName(_) => "invalid_header_name",
            HeaderError::NameTooLong(_) => "invalid_header_name",
            HeaderError::ValueHasNewline(_) => "invalid_header_value",
            HeaderError::ValueHasInvalidChar(_, _) => "invalid_header_value",
            HeaderError::ValueIsRedacted(_) => "invalid_header_value",
            HeaderError::ValueTooLong(_) => "invalid_header_value",
            HeaderError::TooManyHeaders(_) => "too_many_headers",
            HeaderError::TooLarge(_) => "extra_headers_too_large",
            HeaderError::DuplicateName(_) => "duplicate_header_name",
            HeaderError::AuthNameCollision(_) => "duplicate_header_name",
        }
    }
}

impl std::fmt::Display for HeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeaderError::ReservedName(n) => write!(
                f,
                "header \"{n}\" is written by the gateway itself and cannot be configured"
            ),
            HeaderError::InvalidName(n) => {
                write!(f, "header name \"{n}\" is not a valid HTTP token")
            }
            HeaderError::NameTooLong(n) => write!(
                f,
                "header name \"{n}\" exceeds {MAX_HEADER_NAME_LEN} characters"
            ),
            HeaderError::ValueHasNewline(n) => {
                write!(f, "value of header \"{n}\" must not contain CR or LF")
            }
            HeaderError::ValueHasInvalidChar(n, c) => write!(
                f,
                "value of header \"{n}\" contains {c:?}, which is not a printable ASCII character"
            ),
            HeaderError::ValueIsRedacted(n) => write!(
                f,
                "value of header \"{n}\" is the redaction placeholder `{REDACTED}`, not a real value"
            ),
            HeaderError::ValueTooLong(n) => write!(
                f,
                "value of header \"{n}\" exceeds {MAX_HEADER_VALUE_LEN} characters"
            ),
            HeaderError::TooManyHeaders(n) => write!(
                f,
                "{n} extra headers exceeds the limit of {MAX_EXTRA_HEADERS}"
            ),
            HeaderError::TooLarge(n) => write!(
                f,
                "extra headers serialise to {n} bytes, over the limit of {MAX_EXTRA_HEADERS_BYTES}"
            ),
            HeaderError::DuplicateName(n) => {
                write!(f, "header \"{n}\" appears more than once")
            }
            HeaderError::AuthNameCollision(n) => write!(
                f,
                "header \"{n}\" is the auth header and must not also appear in extra headers"
            ),
        }
    }
}

/// RFC 7230 `token` (the characters a header name may use).
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c)
}

/// One header name: syntax, length, reserved list. `name` is returned unchanged
/// — case is preserved for display, while every comparison below folds case.
pub fn validate_name(name: &str) -> Result<(), HeaderError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_HEADER_NAME_LEN {
        return Err(HeaderError::NameTooLong(trimmed.to_string()));
    }
    if !trimmed.chars().all(is_token_char) {
        return Err(HeaderError::InvalidName(trimmed.to_string()));
    }
    if RESERVED_HEADERS
        .iter()
        .any(|r| r.eq_ignore_ascii_case(trimmed))
    {
        return Err(HeaderError::ReservedName(trimmed.to_string()));
    }
    Ok(())
}

/// One header value: no CR/LF (header injection), printable ASCII only, a
/// length cap, and not the redaction sentinel.
///
/// The ASCII rule is not cosmetic: `http::HeaderValue` rejects `0x00..0x1f`,
/// `0x7f` and everything above `0x7f`, and `reqwest` defers that rejection to
/// `.send()`. A value that passes here but fails there is stored as `***` and
/// then fails every probe and call with a transport error naming neither the
/// header nor the configuration.
///
/// `***` is refused because it is what `server_wire` puts in place of a secret.
/// A client that round-trips a GET response back into a PUT would otherwise seal
/// three asterisks over the real credential and see nothing wrong.
pub fn validate_value(name: &str, value: &str) -> Result<(), HeaderError> {
    if value.contains('\r') || value.contains('\n') {
        return Err(HeaderError::ValueHasNewline(name.to_string()));
    }
    if let Some(c) = value.chars().find(|&c| !is_printable_ascii(c)) {
        return Err(HeaderError::ValueHasInvalidChar(name.to_string(), c));
    }
    if value.trim() == REDACTED {
        return Err(HeaderError::ValueIsRedacted(name.to_string()));
    }
    if value.chars().count() > MAX_HEADER_VALUE_LEN {
        return Err(HeaderError::ValueTooLong(name.to_string()));
    }
    Ok(())
}

/// Printable ASCII plus tab — exactly what `HeaderValue::from_str` accepts.
fn is_printable_ascii(c: char) -> bool {
    (' '..='~').contains(&c) || c == '\t'
}

/// The header the protocol credential rides when no custom name is configured.
const PROTOCOL_AUTH_HEADER: &str = "Authorization";

/// Validate the whole extra-header list in the context of the auth header name.
///
/// Cross-entry rules live here rather than in the per-header validators
/// because they are about the *set*: duplicate names (case-insensitive) and a
/// collision with the auth header.
///
/// An empty `auth_header_name` still means `Authorization: Bearer`, so the
/// collision check compares against `Authorization` in that case: allowing it
/// through would put two `Authorization` lines on the wire — `reqwest` appends
/// rather than replaces — and which one an upstream honours is undefined.
pub fn validate_extra_headers(
    headers: &[ExtraHeader],
    auth_header_name: Option<&str>,
) -> Result<(), HeaderError> {
    if headers.len() > MAX_EXTRA_HEADERS {
        return Err(HeaderError::TooManyHeaders(headers.len()));
    }
    let auth = auth_header_name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(PROTOCOL_AUTH_HEADER)
        .to_ascii_lowercase();

    let mut seen: Vec<String> = Vec::with_capacity(headers.len());
    for header in headers {
        let name = header.name.trim();
        validate_name(name)?;
        validate_value(name, &header.value)?;
        let folded = name.to_ascii_lowercase();
        if folded == auth {
            return Err(HeaderError::AuthNameCollision(name.to_string()));
        }
        if seen.contains(&folded) {
            return Err(HeaderError::DuplicateName(name.to_string()));
        }
        seen.push(folded);
    }

    let encoded = serde_json::to_vec(headers).map_err(|_| HeaderError::TooLarge(headers.len()))?;
    if encoded.len() > MAX_EXTRA_HEADERS_BYTES {
        return Err(HeaderError::TooLarge(encoded.len()));
    }
    Ok(())
}

/// Trim names/values into the shape that is stored and sent.
pub fn normalize(headers: &[ExtraHeader]) -> Vec<ExtraHeader> {
    headers
        .iter()
        .map(|h| ExtraHeader {
            name: h.name.trim().to_string(),
            value: h.value.trim().to_string(),
        })
        .collect()
}

/// Parse the decrypted `extra_headers` blob. A malformed blob yields an empty
/// list rather than an error: the alternative is a server that becomes
/// unreachable because one stored byte is off, and the value that would be
/// dropped is the one an administrator just configured — silence is worse than
/// a missing header, and the admin API validates on write.
pub fn parse_extra_headers(raw: Option<&str>) -> Vec<ExtraHeader> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    serde_json::from_str(raw).unwrap_or_default()
}

/// Decrypt the stored blob, then parse it. A missing or unreadable blob is an
/// empty list for the same reason [`parse_extra_headers`] swallows malformed
/// JSON: the call path must still reach an upstream that only needs the auth
/// header.
pub fn decrypt_extra_headers(sealed: Option<&str>, master_key: &str) -> Vec<ExtraHeader> {
    try_decrypt_extra_headers(sealed, master_key).unwrap_or_default()
}

/// The same read, but a blob that will not decrypt is an error rather than an
/// empty list. The admin edit path needs this: it feeds the result back into a
/// whole-list replacement, so "cannot read" would become "wrote empty" and the
/// ciphertext would be gone for good.
pub fn try_decrypt_extra_headers(
    sealed: Option<&str>,
    master_key: &str,
) -> Result<Vec<ExtraHeader>, String> {
    let Some(sealed) = sealed.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    let plain = crate::mcp::credential::unseal_token(sealed, master_key)?;
    let plain = plain.trim();
    if plain.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(plain)
        .map_err(|e| format!("stored extra headers are not a valid header list: {e}"))
}

/// Decrypt the stored single auth-header value. Like the extras, an unreadable
/// or absent value is `None` rather than an error: the call path must still
/// reach an upstream that binds its credential elsewhere.
pub fn unseal_value(sealed: Option<&str>, master_key: &str) -> Option<String> {
    let sealed = sealed.map(str::trim).filter(|s| !s.is_empty())?;
    crate::mcp::credential::unseal_token(sealed, master_key)
        .map_err(|e| {
            tracing::warn!(
                "the stored static auth header value will not decrypt ({e}); the request goes out without it"
            );
            e
        })
        .ok()
}

/// The `(name, value)` pairs the request builder attaches.
pub fn to_pairs(headers: &[ExtraHeader]) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|h| (h.name.clone(), h.value.clone()))
        .collect()
}

/// A configured auth header name, normalised: an empty or blank value means
/// "use the protocol's `Authorization: Bearer`", the same as `NULL`.
pub fn auth_header_name(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
}

/// Serialise for sealing. An empty list stores `NULL` rather than `[]`, so
/// "not configured" and "configured empty" are one state.
pub fn encode_extra_headers(headers: &[ExtraHeader]) -> Option<String> {
    if headers.is_empty() {
        return None;
    }
    serde_json::to_string(headers).ok()
}

/// The outbound headers of one server, decrypted and ready to put on the wire.
///
/// This is the single construction point for what a request carries. The probe,
/// `/mcp/{alias}`, `/mcp-rest/tools/call` and `/mcp-rest/health` all build it
/// here — a probe that assembles its own headers reports "unreachable" for
/// exactly the upstream whose configuration is the only thing worth checking.
#[derive(Debug, Clone, Default)]
pub struct OutboundHeaders {
    /// `None` means the protocol's `Authorization: Bearer`.
    pub auth_header_name: Option<String>,
    /// Plaintext credential. `None` when the server has no usable credential.
    pub auth_header_value: Option<String>,
    pub extra_headers: Vec<(String, String)>,
}

impl OutboundHeaders {
    /// `credential` is the token [`crate::mcp::credential::resolve_outbound_token`]
    /// picked, already unsealed. It wins over the server's own
    /// `auth_header_value` because a token row is the more specific statement:
    /// `user_required` means "this identity's credential", and a per-user token
    /// must never be displaced by the static one. The static value is the
    /// fallback — the TAPD case, where the operator types the credential into
    /// the server dialog and there is no token row at all.
    ///
    /// Without a master key neither secret can be read, so the extras are
    /// dropped and the auth header goes out only if a token row supplied it —
    /// a gateway that cannot decrypt stays reachable rather than silent.
    pub fn resolve(
        server: &crate::models::McpServerRow,
        credential: Option<&str>,
        master_key: Option<&str>,
    ) -> Self {
        let auth_header_name = auth_header_name(server.auth_header_name.as_deref());
        let extra_headers = master_key
            .map(|mk| decrypt_extra_headers(server.extra_headers.as_deref(), mk))
            .map(|headers| to_pairs(&headers))
            .unwrap_or_default();
        let static_value =
            master_key.and_then(|mk| unseal_value(server.auth_header_value.as_deref(), mk));
        OutboundHeaders {
            auth_header_value: credential.map(str::to_string).or(static_value),
            auth_header_name,
            extra_headers,
        }
    }

    pub fn auth(&self) -> Option<UpstreamAuth<'_>> {
        let value = self.auth_header_value.as_deref()?;
        Some(match self.auth_header_name.as_deref() {
            Some(name) => UpstreamAuth::Header { name, value },
            None => UpstreamAuth::Bearer(value),
        })
    }

    pub fn extra(&self) -> &[(String, String)] {
        &self.extra_headers
    }
}

/// Wire shape of a header in an API response: name in clear, value redacted.
#[derive(Debug, Clone, Serialize)]
pub struct HeaderWire {
    pub name: String,
    pub value: &'static str,
}

pub const REDACTED: &str = "***";

pub fn redact(headers: &[ExtraHeader]) -> Vec<HeaderWire> {
    headers
        .iter()
        .map(|h| HeaderWire {
            name: h.name.clone(),
            value: REDACTED,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(name: &str, value: &str) -> ExtraHeader {
        ExtraHeader {
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn reserved_names_are_rejected_case_insensitively() {
        for name in [
            "Mcp-Session-Id",
            "CONTENT-TYPE",
            "accept",
            "Host",
            "mcp-protocol-version",
            "Content-Length",
        ] {
            assert!(
                matches!(validate_name(name), Err(HeaderError::ReservedName(_))),
                "{name} should be reserved"
            );
        }
    }

    #[test]
    fn a_token_name_is_accepted() {
        for name in ["X-Tapd-Access-Token", "PRIVATE-TOKEN", "X_Project.Id~1"] {
            assert!(validate_name(name).is_ok(), "{name} should pass");
        }
    }

    #[test]
    fn non_token_characters_are_rejected() {
        for name in ["X Header", "X:Y", "X\r\n-Injected", "X-Ünicode"] {
            assert!(
                matches!(validate_name(name), Err(HeaderError::InvalidName(_))),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn overlong_names_are_rejected() {
        let long = format!("X-{}", "a".repeat(MAX_HEADER_NAME_LEN));
        assert!(matches!(
            validate_name(&long),
            Err(HeaderError::NameTooLong(_))
        ));
    }

    #[test]
    fn values_may_not_contain_crlf() {
        assert!(matches!(
            validate_value("X-Safe", "abc\r\ndef"),
            Err(HeaderError::ValueHasNewline(_))
        ));
        assert!(validate_value("X-Safe", "abc def").is_ok());
    }

    #[test]
    fn values_must_be_printable_ascii() {
        for value in ["口令-123", "a\u{0}b", "a\u{1b}b", "a\u{7f}b"] {
            assert!(
                matches!(
                    validate_value("X-Safe", value),
                    Err(HeaderError::ValueHasInvalidChar(_, _))
                ),
                "{value:?} should be rejected"
            );
        }
        assert!(validate_value("X-Safe", "a\tb").is_ok());
        assert!(validate_value("X-Safe", "~!@#$%^&*()_+").is_ok());
    }

    #[test]
    fn the_redaction_placeholder_is_not_a_value() {
        assert!(matches!(
            validate_value("X-Safe", REDACTED),
            Err(HeaderError::ValueIsRedacted(_))
        ));
        assert!(matches!(
            validate_value("X-Safe", " *** "),
            Err(HeaderError::ValueIsRedacted(_))
        ));
    }

    #[test]
    fn the_strict_extras_read_reports_an_unreadable_blob() {
        let mk = "sk-master-test";
        let sealed = crate::mcp::credential::seal_token("[]", mk).expect("seal");
        assert!(try_decrypt_extra_headers(Some(&sealed), mk).is_ok());
        assert!(try_decrypt_extra_headers(Some(&sealed), "sk-other").is_err());
        assert_eq!(
            decrypt_extra_headers(Some(&sealed), "sk-other"),
            Vec::<ExtraHeader>::new()
        );
    }

    #[test]
    fn overlong_values_are_rejected() {
        let long = "v".repeat(MAX_HEADER_VALUE_LEN + 1);
        assert!(matches!(
            validate_value("X-Safe", &long),
            Err(HeaderError::ValueTooLong(_))
        ));
    }

    #[test]
    fn duplicate_names_are_rejected_case_insensitively() {
        let list = vec![h("X-A", "1"), h("x-a", "2")];
        assert!(matches!(
            validate_extra_headers(&list, None),
            Err(HeaderError::DuplicateName(_))
        ));
    }

    #[test]
    fn colliding_with_the_auth_header_is_rejected() {
        let list = vec![h("X-Tapd-Access-Token", "t")];
        assert!(matches!(
            validate_extra_headers(&list, Some("x-tapd-access-token")),
            Err(HeaderError::AuthNameCollision(_))
        ));
    }

    #[test]
    fn an_unset_auth_name_still_owns_the_authorization_header() {
        let list = vec![h("authorization", "t")];
        assert!(matches!(
            validate_extra_headers(&list, None),
            Err(HeaderError::AuthNameCollision(_))
        ));
        assert!(matches!(
            validate_extra_headers(&list, Some("  ")),
            Err(HeaderError::AuthNameCollision(_))
        ));
    }

    #[test]
    fn a_custom_auth_name_frees_the_authorization_header() {
        let list = vec![h("Authorization", "t")];
        assert!(validate_extra_headers(&list, Some("PRIVATE-TOKEN")).is_ok());
    }

    #[test]
    fn too_many_headers_are_rejected() {
        let list: Vec<ExtraHeader> = (0..=MAX_EXTRA_HEADERS)
            .map(|i| h(&format!("X-H{i}"), "v"))
            .collect();
        assert!(matches!(
            validate_extra_headers(&list, None),
            Err(HeaderError::TooManyHeaders(_))
        ));
    }

    #[test]
    fn an_oversized_blob_is_rejected() {
        let list: Vec<ExtraHeader> = (0..MAX_EXTRA_HEADERS)
            .map(|i| h(&format!("X-H{i}"), &"v".repeat(MAX_HEADER_VALUE_LEN)))
            .collect();
        assert!(matches!(
            validate_extra_headers(&list, None),
            Err(HeaderError::TooLarge(_))
        ));
    }

    #[test]
    fn round_trip_through_json() {
        let list = vec![h("X-A", "1"), h("X-B", "2")];
        let encoded = serde_json::to_string(&list).expect("encode");
        assert_eq!(parse_extra_headers(Some(&encoded)), list);
    }

    #[test]
    fn a_malformed_blob_is_empty_not_an_error() {
        assert!(parse_extra_headers(Some("{not json")).is_empty());
        assert!(parse_extra_headers(Some("")).is_empty());
        assert!(parse_extra_headers(None).is_empty());
    }

    #[test]
    fn redaction_keeps_names_and_hides_values() {
        let wire = redact(&[h("X-A", "secret")]);
        assert_eq!(wire[0].name, "X-A");
        assert_eq!(wire[0].value, REDACTED);
    }

    #[test]
    fn normalize_trims_both_sides() {
        let out = normalize(&[h(" X-A ", " v ")]);
        assert_eq!(out[0].name, "X-A");
        assert_eq!(out[0].value, "v");
    }

    #[test]
    fn error_kinds_map_to_stable_api_codes() {
        assert_eq!(
            HeaderError::ReservedName("Host".into()).kind(),
            "invalid_header_name"
        );
        assert_eq!(
            HeaderError::ValueHasNewline("X".into()).kind(),
            "invalid_header_value"
        );
        assert_eq!(
            HeaderError::AuthNameCollision("X".into()).kind(),
            "duplicate_header_name"
        );
    }

    #[test]
    fn a_blank_auth_name_means_the_protocol_bearer() {
        assert_eq!(auth_header_name(None), None);
        assert_eq!(auth_header_name(Some("")), None);
        assert_eq!(auth_header_name(Some("   ")), None);
        assert_eq!(
            auth_header_name(Some(" PRIVATE-TOKEN ")),
            Some("PRIVATE-TOKEN".to_string())
        );
    }

    #[test]
    fn an_empty_list_encodes_to_nothing() {
        assert_eq!(encode_extra_headers(&[]), None);
        let encoded = encode_extra_headers(&[h("X-A", "1")]).expect("encoded");
        assert_eq!(parse_extra_headers(Some(&encoded)), vec![h("X-A", "1")]);
    }

    #[test]
    fn sealed_extras_round_trip_and_never_hold_plaintext() {
        let key = "0123456789abcdef0123456789abcdef";
        let plain = encode_extra_headers(&[h("X-Api-Key", "s3cret")]).expect("plain");
        let sealed = crate::mcp::credential::seal_token(&plain, key).expect("seal");
        assert!(sealed.starts_with("v2:gcm:"));
        assert!(!sealed.contains("s3cret"));
        assert_eq!(
            decrypt_extra_headers(Some(&sealed), key),
            vec![h("X-Api-Key", "s3cret")]
        );
    }

    #[test]
    fn an_unreadable_blob_yields_no_headers_instead_of_failing() {
        assert!(decrypt_extra_headers(None, "key").is_empty());
        assert!(decrypt_extra_headers(Some(""), "key").is_empty());
        assert!(decrypt_extra_headers(Some("v2:gcm:not-base64"), "key").is_empty());
    }

    fn row(name: Option<&str>, sealed_extras: Option<&str>) -> crate::models::McpServerRow {
        row_with_value(name, None, sealed_extras)
    }

    fn row_with_value(
        name: Option<&str>,
        sealed_value: Option<&str>,
        sealed_extras: Option<&str>,
    ) -> crate::models::McpServerRow {
        crate::models::McpServerRow {
            id: 1,
            alias: "tapd".into(),
            display_name: "TAPD".into(),
            url: "https://example.invalid/mcp".into(),
            transport: "streamable_http".into(),
            credential_scope: "shared".into(),
            enabled: true,
            protocol_version: None,
            auth_header_name: name.map(str::to_string),
            auth_header_value: sealed_value.map(str::to_string),
            extra_headers: sealed_extras.map(str::to_string),
            tool_count: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn a_configured_name_puts_the_bare_credential_in_that_header() {
        let headers = OutboundHeaders::resolve(&row(Some("PRIVATE-TOKEN"), None), Some("tk"), None);
        assert_eq!(
            headers.auth(),
            Some(UpstreamAuth::Header {
                name: "PRIVATE-TOKEN",
                value: "tk"
            })
        );
    }

    #[test]
    fn no_configured_name_keeps_the_protocol_bearer() {
        let headers = OutboundHeaders::resolve(&row(None, None), Some("tk"), None);
        assert_eq!(headers.auth(), Some(UpstreamAuth::Bearer("tk")));
    }

    #[test]
    fn no_credential_means_no_auth_whatever_the_name_says() {
        let headers = OutboundHeaders::resolve(&row(Some("PRIVATE-TOKEN"), None), None, None);
        assert_eq!(headers.auth(), None);
    }

    #[test]
    fn a_missing_master_key_drops_extras_but_keeps_the_auth_header() {
        let headers = OutboundHeaders::resolve(&row(None, Some("v2:gcm:x")), Some("tk"), None);
        assert!(headers.extra().is_empty());
        assert_eq!(headers.auth(), Some(UpstreamAuth::Bearer("tk")));
    }

    /// The TAPD case: no token row at all, the credential lives in the server's
    /// own `auth_header_value`.
    #[test]
    fn a_stored_header_value_is_the_credential_when_no_token_exists() {
        let key = "0123456789abcdef0123456789abcdef";
        let sealed = crate::mcp::credential::seal_token("tapd-secret", key).expect("seal");
        let headers = OutboundHeaders::resolve(
            &row_with_value(Some("X-Tapd-Access-Token"), Some(&sealed), None),
            None,
            Some(key),
        );
        assert_eq!(
            headers.auth(),
            Some(UpstreamAuth::Header {
                name: "X-Tapd-Access-Token",
                value: "tapd-secret"
            })
        );
    }

    /// 工蜂: the shared token goes out under a differently named header. A token
    /// row is the more specific statement and wins over the dialog value.
    #[test]
    fn a_resolved_token_wins_over_the_stored_header_value() {
        let key = "0123456789abcdef0123456789abcdef";
        let sealed = crate::mcp::credential::seal_token("typo", key).expect("seal");
        let headers = OutboundHeaders::resolve(
            &row_with_value(Some("PRIVATE-TOKEN"), Some(&sealed), None),
            Some("rotated"),
            Some(key),
        );
        assert_eq!(
            headers.auth(),
            Some(UpstreamAuth::Header {
                name: "PRIVATE-TOKEN",
                value: "rotated"
            })
        );
    }

    #[test]
    fn a_stored_header_value_is_unreadable_without_the_master_key() {
        let key = "0123456789abcdef0123456789abcdef";
        let sealed = crate::mcp::credential::seal_token("tapd-secret", key).expect("seal");
        let headers = OutboundHeaders::resolve(
            &row_with_value(Some("X-Tapd-Access-Token"), Some(&sealed), None),
            None,
            None,
        );
        assert_eq!(headers.auth(), None);
    }
}
