//! Shared step definitions — used by all .feature files
//!
//! Steps defined here MUST NOT be duplicated in any other *_steps.rs file.

use cucumber::{given, then};
use crate::TestWorld;

#[given(expr = "管理员已认证")]
async fn admin_authenticated(_world: &mut TestWorld) {}

#[then(expr = "响应状态码为 {int}")]
async fn then_status_is(world: &mut TestWorld, expected: u16) {
    assert_eq!(
        world.last_status,
        Some(expected),
        "Expected status {}, got {:?}",
        expected,
        world.last_status
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
async fn then_json_contains_field_int(
    world: &mut TestWorld,
    field: String,
    expected: i64,
) {
    let body = world.last_body.as_ref().expect("no response body");
    let actual = body
        .get(&field)
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| panic!(
            "Expected JSON field '{}' to be integer {}, got: {}",
            field,
            expected,
            serde_json::to_string_pretty(body).unwrap_or_default()
        ));
    assert_eq!(
        actual, expected,
        "Expected field '{}' = {}, got {}",
        field, expected, actual
    );
}
