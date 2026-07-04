//! Step bindings for health.feature

use cucumber::{then, when};
use axum::http::Method;

use super::common::{build_health_router, make_request};
use crate::TestWorld;

#[when(expr = "发送 GET \\/health 请求")]
async fn get_health(world: &mut TestWorld) {
    let app = build_health_router();
    let (s, b) = make_request(&app, Method::GET, "/health", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/health\\/liveliness 请求")]
async fn get_liveliness(world: &mut TestWorld) {
    let app = build_health_router();
    let (s, b) = make_request(&app, Method::GET, "/health/liveliness", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/health\\/readiness 请求")]
async fn get_readiness(world: &mut TestWorld) {
    let app = build_health_router();
    let (s, b) = make_request(&app, Method::GET, "/health/readiness", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[then(expr = "响应包含 status 字段")]
async fn response_has_status(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    assert!(
        body.get("status").is_some(),
        "Response missing status field: {:?}",
        body
    );
}
