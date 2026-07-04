//! Step bindings for spend.feature and global.feature

use cucumber::when;
use axum::http::Method;

use super::common::{build_spend_router, make_request};
use crate::TestWorld;

#[when(expr = "发送 GET \\/spend\\/logs 请求（无认证）")]
async fn get_spend_logs_noauth(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let (s, b) = make_request(&app, Method::GET, "/spend/logs", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/spend\\/keys 请求（无认证）")]
async fn get_spend_keys_noauth(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let (s, b) = make_request(&app, Method::GET, "/spend/keys", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/global\\/spend 请求（无认证）")]
async fn get_global_spend_noauth(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let app = build_spend_router(state);
    let (s, b) = make_request(&app, Method::GET, "/global/spend", None, None).await;
    world.last_status = Some(s);
    world.last_body = b;
}
