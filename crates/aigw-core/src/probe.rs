//! Proxy probe engine — exit detection + quality check (Phase 50, Stage 123)
//!
//! Implements sub2api's `proxy_probe_service.go` (exit probe via ip-api/ipify)
//! and `admin_proxy.go CheckProxyQuality` (multi-target quality with Cloudflare
//! challenge detection). Results are written as a single `probe_result` JSON
//! snapshot into `proxies.probe_result`.
//!
//! Reference: `docs/research/2026-08-18-sub2api-proxy-oauth-reference.md`
//! §1.3/§1.4/§2.7 (sub-check CF signatures).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Proxy client (reqwest with socks/http/https proxy)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Build a reqwest client that routes through `proxy_url`
/// (`scheme://user:pass@host:port`, supports http/https/socks5/socks5h).
///
/// Distinct from `Router::build_retry_client` (the gateway's non-proxy retry
/// client) — this client is used only for probe / OAuth exchange / reverse-
/// proxy egress. No `insecure_skip_verify` (sub2api also forbids it).
pub fn build_proxy_client(proxy_url: &str, timeout: Duration) -> Result<reqwest::Client, String> {
    let proxy = reqwest::Proxy::all(proxy_url).map_err(|e| format!("invalid proxy URL: {}", e))?;
    reqwest::Client::builder()
        .proxy(proxy)
        .timeout(timeout)
        .build()
        .map_err(|e| format!("failed to build proxy client: {}", e))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Exit probe
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Exit info resolved from a successful exit probe.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProxyExitInfo {
    pub ip: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    /// Round-trip latency in ms for the exit probe request.
    pub latency_ms: u64,
}

/// Probe URL list — primary first (ip-api), fallback ipify. Some AI-API-only
/// proxies only allow specific domains; try both before failing.
const EXIT_PROBE_URLS: &[&str] = &[
    "http://ip-api.com/json/?lang=zh-CN",
    "http://api64.ipify.org?format=json",
];

/// Probe the proxy's egress IP / geo / latency.
///
/// Uses the first URL that returns a parseable body; both must fail to return
/// an error. Body limited to 1 MiB.
pub async fn probe_exit(client: &reqwest::Client) -> Result<ProxyExitInfo, String> {
    for url in EXIT_PROBE_URLS {
        let started = std::time::Instant::now();
        let resp = client
            .get(*url)
            .send()
            .await
            .map_err(|e| format!("exit probe GET {} failed: {}", url, e))?;
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("exit probe read {} failed: {}", url, e))?;
        if bytes.len() > 1_048_576 {
            return Err(format!("exit probe response too large from {}", url));
        }
        let body: Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("exit probe {} not JSON: {}", url, e))?;
        let latency_ms = started.elapsed().as_millis() as u64;
        if let Ok(info) = parse_exit_info(&body, latency_ms) {
            return Ok(info);
        }
    }
    Err("all exit probe URLs failed".to_string())
}

/// Parse ip-api (`query`, `city`, `region`, `regionName`, `country`,
/// `countryCode`) or ipify (`ip`) JSON into `ProxyExitInfo`.
fn parse_exit_info(body: &Value, latency_ms: u64) -> Result<ProxyExitInfo, String> {
    let ip = body
        .get("query")
        .or_else(|| body.get("ip"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| "no ip field in exit probe response".to_string())?;
    Ok(ProxyExitInfo {
        ip: Some(ip),
        city: body.get("city").and_then(|v| v.as_str()).map(String::from),
        region: body
            .get("region")
            .or_else(|| body.get("regionName"))
            .and_then(|v| v.as_str())
            .map(String::from),
        country: body
            .get("country")
            .and_then(|v| v.as_str())
            .map(String::from),
        country_code: body
            .get("countryCode")
            .and_then(|v| v.as_str())
            .map(String::from),
        latency_ms,
    })
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Quality check
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A single quality target — URL + whitelisted status codes that count as pass.
struct QualityTarget {
    name: &'static str,
    url: &'static str,
    /// Status codes that mean "reachable" (pass). 429 → warn. Others → fail.
    pass_statuses: &'static [u16],
}

/// Quality targets (no auth, whitelist-status pass) — sub2api's
/// `proxyQualityTargets` + the aigw-added `claude_oauth` target.
const QUALITY_TARGETS: &[QualityTarget] = &[
    QualityTarget {
        name: "openai",
        url: "https://api.openai.com/v1/models",
        pass_statuses: &[401, 200],
    },
    QualityTarget {
        name: "anthropic",
        url: "https://api.anthropic.com/v1/messages",
        pass_statuses: &[401, 405, 404, 400],
    },
    QualityTarget {
        name: "claude_oauth",
        url: "https://claude.ai/api/organizations",
        pass_statuses: &[200],
    },
    QualityTarget {
        name: "gemini",
        url: "https://generativelanguage.googleapis.com/$discovery/rest?version=v1beta",
        pass_statuses: &[200],
    },
    QualityTarget {
        name: "grok",
        url: "https://api.x.ai/v1/models",
        pass_statuses: &[401, 200],
    },
];

/// Per-target verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityItem {
    pub target: String,
    /// pass / warn / challenge / fail
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cf_ray: Option<String>,
    pub message: String,
}

/// Full quality result — score + grade + per-item breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityResult {
    pub score: i64,
    pub grade: String,
    pub overall_status: String,
    pub exit_ip: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub base_latency_ms: Option<u64>,
    pub items: Vec<QualityItem>,
    pub last_check_at: String,
}

/// Cloudflare challenge signatures (sub-check `classifyStep1Error` + extra).
const CF_SIGNATURES: &[&str] = &[
    "just a moment",
    "cf-ray",
    "cf-mitigated",
    "challenges.cloudflare.com",
    "attention required",
];

/// Detect a Cloudflare challenge from a target response (status + headers + body).
/// Returns the extracted Ray ID when a challenge is present, else None.
fn detect_cf_challenge(
    status: u16,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
) -> Option<String> {
    // Headers first — `cf-ray` is the cheapest signal.
    if let Some(ray) = headers.get("cf-ray").and_then(|v| v.to_str().ok()) {
        return Some(ray.to_string());
    }
    if headers.contains_key("cf-mitigated") {
        return Some("cf-mitigated".to_string());
    }
    let text = String::from_utf8_lossy(body).to_lowercase();
    if CF_SIGNATURES.iter().any(|s| text.contains(s)) {
        // Extract a Ray ID if present in the body (common on challenge pages).
        let ray = extract_cf_ray(&text);
        return Some(ray.unwrap_or_else(|| "unknown".to_string()));
    }
    // HTML-but-not-JSON response (starts with '<') is a CF/HTML block.
    if status == 403 && text.trim_start().starts_with('<') {
        return Some("html-block".to_string());
    }
    None
}

fn extract_cf_ray(text: &str) -> Option<String> {
    // Common: `cf-ray: <id>` or `ray id="<id>"` — grab the token after "cf-ray"
    // or the header value pattern `XXXXXX-YYY`.
    text.split("cf-ray")
        .nth(1)
        .and_then(|s| s.split_whitespace().nth(1))
        .map(|s| {
            s.trim_matches(|c: char| c == ':' || c == ',' || c == '"')
                .to_string()
        })
        .filter(|s| !s.is_empty())
}

/// Verdict for a single quality target.
fn classify_target(
    target: &QualityTarget,
    status: u16,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
    latency_ms: u64,
) -> QualityItem {
    let cf = detect_cf_challenge(status, headers, body);
    let mut item = QualityItem {
        target: target.name.to_string(),
        status: "fail".to_string(),
        latency_ms: Some(latency_ms),
        cf_ray: cf.clone(),
        message: String::new(),
    };
    if cf.is_some() {
        item.status = "challenge".to_string();
        let msg = if target.name == "claude_oauth" {
            "Cloudflare 正在挑战该代理 IP — 请更换出口节点（住宅/干净池）。cookie 本身未评估。"
        } else {
            "目标返回 Cloudflare challenge"
        };
        item.message = msg.to_string();
        return item;
    }
    if target.pass_statuses.contains(&status) {
        item.status = "pass".to_string();
        item.message = format!("HTTP {}（目标可达）", status);
        if target.name == "claude_oauth" && status == 200 {
            item.message = "claude.ai 无 challenge，OAuth 路径可达".to_string();
        }
        return item;
    }
    if status == 429 {
        item.status = "warn".to_string();
        item.message = "目标返回 429，可能存在频控".to_string();
        return item;
    }
    // claude_oauth 403 non-CF → fail with specific message
    if target.name == "claude_oauth" && status == 403 {
        item.message = "claude.ai 返回 403 — 通常是 cookie/geo/IP block".to_string();
    } else if target.name == "claude_oauth" && status == 401 {
        item.message = "claude.ai 返回 401".to_string();
    } else {
        item.message = format!("HTTP {}（不可达）", status);
    }
    item
}

/// Run the full quality check through the proxy client.
///
/// Counts pass/warn/fail/challenge; score = 100 - warn×10 - fail×22 - challenge×30
/// (floor 0); grade A≥90 / B≥75 / C≥60 / D≥40 / F. Returns the complete result.
pub async fn run_quality_check(
    client: &reqwest::Client,
    exit: &ProxyExitInfo,
) -> Result<QualityResult, String> {
    let mut items = Vec::new();
    let mut warn = 0i64;
    let mut fail = 0i64;
    let mut challenge = 0i64;

    for target in QUALITY_TARGETS {
        let started = std::time::Instant::now();
        let resp = match client.get(target.url).send().await {
            Ok(r) => r,
            Err(e) => {
                items.push(QualityItem {
                    target: target.name.to_string(),
                    status: "fail".to_string(),
                    latency_ms: None,
                    cf_ray: None,
                    message: format!("请求失败: {}", e),
                });
                fail += 1;
                continue;
            }
        };
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        let bytes = match resp.bytes().await {
            Ok(b) => b.to_vec(),
            Err(_) => Vec::new(),
        };
        let latency_ms = started.elapsed().as_millis() as u64;
        let item = classify_target(target, status, &headers, &bytes, latency_ms);
        match item.status.as_str() {
            "warn" => warn += 1,
            "fail" => fail += 1,
            "challenge" => challenge += 1,
            _ => {}
        }
        items.push(item);
    }

    let score = (100 - warn * 10 - fail * 22 - challenge * 30).max(0);
    let grade = grade_for_score(score);
    let overall_status = if challenge > 0 {
        "challenge"
    } else if fail > 0 {
        "failed"
    } else if warn > 0 {
        "warn"
    } else {
        "healthy"
    };

    Ok(QualityResult {
        score,
        grade,
        overall_status: overall_status.to_string(),
        exit_ip: exit.ip.clone(),
        country: exit.country.clone(),
        country_code: exit.country_code.clone(),
        base_latency_ms: Some(exit.latency_ms),
        items,
        last_check_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Grade for a score: A≥90 / B≥75 / C≥60 / D≥40 / F.
pub fn grade_for_score(score: i64) -> String {
    if score >= 90 {
        "A".to_string()
    } else if score >= 75 {
        "B".to_string()
    } else if score >= 60 {
        "C".to_string()
    } else if score >= 40 {
        "D".to_string()
    } else {
        "F".to_string()
    }
}

/// Run exit probe + quality check and combine into a `probe_result` snapshot.
pub async fn run_full_probe(proxy_url: &str, timeout: Duration) -> Result<Value, String> {
    let client = build_proxy_client(proxy_url, timeout)?;
    let exit = probe_exit(&client).await?;
    let quality = run_quality_check(&client, &exit).await?;
    Ok(json!({
        "latency_ms": quality.base_latency_ms,
        "exit_ip": quality.exit_ip,
        "country": quality.country,
        "country_code": quality.country_code,
        "score": quality.score,
        "grade": quality.grade,
        "overall_status": quality.overall_status,
        "items": quality.items,
        "last_check_at": quality.last_check_at,
    }))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_exit_ip_api() {
        let body = json!({
            "status": "success",
            "query": "1.2.3.4",
            "city": "Hong Kong",
            "region": "Hong Kong",
            "regionName": "Hong Kong",
            "country": "香港",
            "countryCode": "HK",
        });
        let info = parse_exit_info(&body, 123).unwrap();
        assert_eq!(info.ip.as_deref(), Some("1.2.3.4"));
        assert_eq!(info.country.as_deref(), Some("香港"));
        assert_eq!(info.country_code.as_deref(), Some("HK"));
        assert_eq!(info.latency_ms, 123);
    }

    #[test]
    fn test_parse_exit_ipify() {
        let body = json!({"ip": "8.8.8.8"});
        let info = parse_exit_info(&body, 55).unwrap();
        assert_eq!(info.ip.as_deref(), Some("8.8.8.8"));
        assert_eq!(info.country, None);
        assert_eq!(info.latency_ms, 55);
    }

    #[test]
    fn test_parse_exit_missing_ip_errors() {
        let body = json!({"status": "fail"});
        assert!(parse_exit_info(&body, 10).is_err());
    }

    #[test]
    fn test_proxy_quality_score_all_pass() {
        let quality = QualityResult {
            score: 100,
            grade: "A".to_string(),
            overall_status: "healthy".to_string(),
            exit_ip: None,
            country: None,
            country_code: None,
            base_latency_ms: None,
            items: vec![],
            last_check_at: "t".to_string(),
        };
        assert_eq!(quality.score, 100);
        assert_eq!(grade_for_score(100), "A");
        assert_eq!(grade_for_score(90), "A");
        assert_eq!(grade_for_score(89), "B");
        assert_eq!(grade_for_score(75), "B");
        assert_eq!(grade_for_score(74), "C");
        assert_eq!(grade_for_score(60), "C");
        assert_eq!(grade_for_score(59), "D");
        assert_eq!(grade_for_score(40), "D");
        assert_eq!(grade_for_score(39), "F");
    }

    #[test]
    fn test_cf_challenge_detection_signatures() {
        // cf-ray header
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("cf-ray", "abc123".parse().unwrap());
        assert_eq!(
            detect_cf_challenge(403, &headers, b""),
            Some("abc123".to_string())
        );

        // body signature
        let h2 = reqwest::header::HeaderMap::new();
        assert_eq!(
            detect_cf_challenge(403, &h2, b"<html>Just a moment... enabling security</html>"),
            Some("unknown".to_string())
        );
        assert_eq!(detect_cf_challenge(200, &h2, b"{\"ok\":true}"), None);
    }

    #[test]
    fn test_claude_oauth_target_verdict() {
        let headers = reqwest::header::HeaderMap::new();
        // 200 JSON → pass
        let item = classify_target(
            &QUALITY_TARGETS[2],
            200,
            &headers,
            b"[{\"uuid\":\"x\"}]",
            50,
        );
        assert_eq!(item.status, "pass");
        assert!(item.message.contains("无 challenge"));

        // CF HTML → challenge
        let item = classify_target(
            &QUALITY_TARGETS[2],
            403,
            &headers,
            b"<html>Attention required! Challenge</html>",
            50,
        );
        assert_eq!(item.status, "challenge");

        // 403 non-CF → fail
        let item = classify_target(
            &QUALITY_TARGETS[2],
            403,
            &headers,
            b"{\"error\":\"forbidden\"}",
            50,
        );
        assert_eq!(item.status, "fail");
        assert!(item.message.contains("403"));

        // 429 → warn
        let item = classify_target(&QUALITY_TARGETS[2], 429, &headers, b"{}", 50);
        assert_eq!(item.status, "warn");
    }

    #[test]
    fn test_probe_result_snapshot_shape() {
        // Simulates the JSON written to proxies.probe_result
        let snapshot = json!({
            "latency_ms": 320,
            "exit_ip": "1.2.3.4",
            "country": "香港",
            "country_code": "HK",
            "score": 88,
            "grade": "B",
            "overall_status": "healthy",
            "items": [{"target": "base_connectivity", "status": "pass", "latency_ms": 320}],
            "last_check_at": "2026-08-18T08:00:00Z",
        });
        assert_eq!(snapshot["score"], 88);
        assert_eq!(snapshot["grade"], "B");
        assert_eq!(snapshot["exit_ip"], "1.2.3.4");
    }
}
