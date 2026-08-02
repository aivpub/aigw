//! Shared step definitions — used by all .feature files
//!
//! Steps defined here MUST NOT be duplicated in any other *_steps.rs file.

use crate::TestWorld;
use cucumber::{given, then};

#[given(expr = "管理员已认证")]
async fn admin_authenticated(_world: &mut TestWorld) {}

/// Format a response diagnostic string: status + pretty-printed JSON body.
fn response_diag(status: &Option<u16>, body: &Option<serde_json::Value>) -> String {
    let status_str = match status {
        Some(s) => format!("status={}", s),
        None => "status=None".to_string(),
    };
    let body_str = match body {
        Some(b) => {
            let pretty = serde_json::to_string_pretty(b).unwrap_or_else(|_| b.to_string());
            format!("\n  body={}", pretty)
        }
        None => "\n  body=None".to_string(),
    };
    format!("{}{}", status_str, body_str)
}

#[then(expr = "响应状态码为 {int}")]
async fn then_status_is(world: &mut TestWorld, expected: u16) {
    let diag = response_diag(&world.last_status, &world.last_body);
    assert_eq!(
        world.last_status,
        Some(expected),
        "Expected status {}, got {:?}\n  ── Full response ──\n  {}",
        expected,
        world.last_status,
        diag
    );
}

#[then(regex = r#"^响应 JSON 包含 "(.+)" 字段$"#)]
async fn then_json_contains_field(world: &mut TestWorld, field: String) {
    let body = world.last_body.as_ref().expect("no response body");
    assert!(
        body.get(&field).is_some(),
        "Expected JSON to contain field '{}', got: {}",
        field,
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

#[then(regex = r#"^响应 JSON 包含 "(.+)" 字段值为 (-?\d+)$"#)]
async fn then_json_contains_field_int(world: &mut TestWorld, field: String, expected: i64) {
    let body = world.last_body.as_ref().expect("no response body");
    let actual = body
        .get(&field)
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| {
            panic!(
                "Expected JSON field '{}' to be integer {}, got: {}",
                field,
                expected,
                serde_json::to_string_pretty(body).unwrap_or_default()
            )
        });
    assert_eq!(
        actual, expected,
        "Expected field '{}' = {}, got {}",
        field, expected, actual
    );
}
