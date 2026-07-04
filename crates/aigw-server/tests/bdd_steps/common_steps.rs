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
