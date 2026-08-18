//! Step bindings for proxies.feature — Stage 122/123 /admin/proxies/* CRUD + probe
//!
//! Uses the shared in-memory SQLite test DB (world.ensure_state()) and a
//! dedicated router with the proxy endpoints. Requires AIGW_MASTER_KEY set in
//! test_state so proxy_url encryption + redact round-trips.

use axum::http::Method;
use axum::Router;
use cucumber::{given, then, when};
use std::sync::Arc;
use tower::ServiceExt;

use crate::TestWorld;

fn build_proxy_router(state: aigw_server::routes::keys::SharedState) -> Router {
    Router::new()
        .route(
            "/admin/proxies",
            axum::routing::get(aigw_server::routes::proxies::list_proxies)
                .post(aigw_server::routes::proxies::create_proxy),
        )
        .route(
            "/admin/proxies/all",
            axum::routing::get(aigw_server::routes::proxies::list_all_proxies),
        )
        .route(
            "/admin/proxies/batch-delete",
            axum::routing::post(aigw_server::routes::proxies::batch_delete_proxies),
        )
        .route(
            "/admin/proxies/{id}",
            axum::routing::get(aigw_server::routes::proxies::get_proxy)
                .put(aigw_server::routes::proxies::update_proxy)
                .delete(aigw_server::routes::proxies::delete_proxy),
        )
        .route(
            "/admin/proxies/{id}/test",
            axum::routing::post(aigw_server::routes::proxies::test_proxy),
        )
        .route(
            "/admin/proxies/{id}/quality",
            axum::routing::post(aigw_server::routes::proxies::quality_proxy),
        )
        .route(
            "/admin/proxies/{id}/toggle",
            axum::routing::post(aigw_server::routes::proxies::toggle_proxy),
        )
        .with_state(state)
}

/// Send an HTTP request to the proxy router, storing status + body on world.
async fn send_request(
    world: &mut TestWorld,
    method: Method,
    uri: &str,
    auth: Option<&str>,
    body: Option<&str>,
) {
    let state = world.ensure_state().await;
    let app = build_proxy_router(state);
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json");
    if let Some(token) = auth {
        req = req.header("Authorization", format!("Bearer {}", token));
    }
    let req = req
        .body(axum::body::Body::from(body.unwrap_or("").to_string()))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

/// Track the most recently created proxy id per scenario.
/// We reuse world.created_keys with a reserved key "proxy:latest".
fn store_proxy_id(world: &mut TestWorld, id: i64) {
    world
        .created_keys
        .insert("proxy:latest".to_string(), id.to_string());
}

fn latest_proxy_id(world: &mut TestWorld) -> String {
    world
        .created_keys
        .get("proxy:latest")
        .expect("no proxy created yet")
        .clone()
}

/// Create a proxy directly via the DB store (returns its id).
async fn db_create_proxy(world: &mut TestWorld, name: &str) -> i64 {
    let state = world.ensure_state().await;
    // Encrypt the URL with the test master key so the response redact works.
    let encrypted =
        aigw_core::crypto::encrypt_proxy_url("http://user:secret@1.2.3.4:8080", "bdd-master-key")
            .unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    let p = aigw_core::models::Proxy {
        id: 0,
        name: name.to_string(),
        proxy_url: encrypted,
        status: "active".to_string(),
        expires_at: None,
        probe_result: serde_json::json!({}),
        created_at: now.clone(),
        updated_at: now,
    };
    state.db.create_proxy(&p).await.expect("create proxy")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// The shared test DB already runs migrations and has no master key in
/// AppState.aigw_master_key by default — this step is a no-op marker that
/// ensures the world's master key (sk-master-test) is set. Our proxy router
/// uses the shared test state, which carries aigw_master_key=None; the create
/// handler therefore fails encryption. So for these scenarios we override the
/// world state with one that HAS aigw_master_key.
#[given(expr = "数据库已初始化且已配置 master key")]
async fn given_db_ready_with_master_key(world: &mut TestWorld) {
    // Replace world.state with a state that has aigw_master_key set, so
    // encrypt/decrypt of proxy_url works through the HTTP handlers.
    let db = aigw_core::db::Database::init("sqlite::memory:")
        .await
        .expect("db init");
    let state: aigw_server::routes::keys::SharedState =
        Arc::new(aigw_server::routes::keys::AppState {
            resolver: aigw_core::resolver::ModelResolver::new(db.clone(), None, "onprem"),
            router: aigw_core::router::Router::default(),
            db,
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: Some("bdd-master-key".to_string()),
            key_generate_length: aigw_server::routes::keys::DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
            provider_registry: aigw_core::provider::ProviderRegistry::new(),
            router_state: aigw_core::router::RouterState::default(),
            rate_limiter: Arc::new(aigw_core::rate_limiter::RateLimiter::new()),
            deployment_mode: "test".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            metrics: None,
        });
    world.master_key = "sk-master-test".to_string();
    world.state = Some(state);
}

#[given(expr = "已创建代理 {string}")]
async fn given_proxy_created(world: &mut TestWorld, name: String) {
    let id = db_create_proxy(world, &name).await;
    store_proxy_id(world, id);
    // Also store under proxy:{name} so batch-delete can reference by name.
    world
        .created_keys
        .insert(format!("proxy:{}", name), id.to_string());
}

/// Proxy with a non-empty probe_result snapshot (simulates Stage 123 results).
#[given(expr = "已创建代理 {string} 带探测快照")]
async fn given_proxy_with_snapshot(world: &mut TestWorld, name: String) {
    let state = world.ensure_state().await;
    let encrypted =
        aigw_core::crypto::encrypt_proxy_url("http://user:secret@1.2.3.4:8080", "bdd-master-key")
            .unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    let p = aigw_core::models::Proxy {
        id: 0,
        name: name.to_string(),
        proxy_url: encrypted,
        status: "active".to_string(),
        expires_at: None,
        probe_result: serde_json::json!({
            "exit_ip": "1.2.3.4",
            "country": "香港",
            "latency_ms": 120,
            "score": 88,
            "grade": "B",
        }),
        created_at: now.clone(),
        updated_at: now,
    };
    let id = state.db.create_proxy(&p).await.expect("create proxy");
    store_proxy_id(world, id);
    world
        .created_keys
        .insert(format!("proxy:{}", name), id.to_string());
}

/// Insert a credential whose credential_values.proxy_id references the latest
/// created proxy (in-use guard).
#[given(expr = "已存在凭证 {string} 引用该代理")]
async fn given_credential_referencing_proxy(world: &mut TestWorld, cred_name: String) {
    let state = world.ensure_state().await;
    let id: i64 = latest_proxy_id(world).parse().unwrap();
    let cred = aigw_core::models::Credential {
        credential_id: uuid::Uuid::new_v4().to_string(),
        credential_name: cred_name,
        credential_values: serde_json::json!({"proxy_id": id}),
        credential_info: serde_json::json!({}),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: None,
    };
    state
        .db
        .insert_credential(&cred)
        .await
        .expect("insert credential");
}

/// Create a non-master key for the 403 scenario.
#[given(expr = "已生成普通 key {string}（代理场景）")]
async fn given_regular_key(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let raw = format!("sk-{}", uuid::Uuid::new_v4());
    let key = aigw_core::models::VirtualKey {
        token: aigw_core::crypto::hash_token(&raw),
        key_name: Some(alias.clone()),
        key_alias: Some(alias.clone()),
        soft_budget_cooldown: "false".to_string(),
        spend: 0.0,
        expires: None,
        models: serde_json::json!([]),
        aliases: serde_json::json!({}),
        config: serde_json::json!({}),
        router_settings: None,
        user_id: None,
        team_id: None,
        agent_id: None,
        project_id: None,
        permissions: serde_json::json!({}),
        max_parallel_requests: None,
        metadata: serde_json::json!({}),
        blocked: None,
        tpm_limit: None,
        rpm_limit: None,
        max_budget: None,
        soft_budget: None,
        budget_duration: None,
        budget_reset_at: None,
        allowed_cache_controls: serde_json::json!([]),
        allowed_routes: serde_json::json!([]),
        policies: serde_json::json!([]),
        access_group_ids: serde_json::json!([]),
        model_spend: serde_json::json!({}),
        model_max_budget: serde_json::json!({}),
        budget_id: None,
        organization_id: None,
        object_permission_id: None,
        created_at: Some(chrono::Utc::now()),
        created_by: Some("test".to_string()),
        updated_at: Some(chrono::Utc::now()),
        updated_by: Some("test".to_string()),
        last_active: None,
        rotation_count: None,
        auto_rotate: None,
        rotation_interval: None,
        last_rotation_at: None,
        key_rotation_at: None,
        budget_limits: None,
        user_email: None,
        user_alias: None,
    };
    state.db.insert_key(&key).await.expect("insert key");
    world.created_keys.insert(alias, raw);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送 POST \\/admin\\/proxies 请求创建代理")]
async fn when_create_proxy(world: &mut TestWorld, step: &cucumber::gherkin::Step) {
    let body = step.docstring.as_ref().expect("docstring body").to_string();
    send_request(
        world,
        Method::POST,
        "/admin/proxies",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    // Store the created id so detail/update/delete steps can reference it.
    if let Some(ref b) = world.last_body {
        if let Some(id) = b.get("id").and_then(|v| v.as_i64()) {
            store_proxy_id(world, id);
        }
    }
}

#[when(expr = "发送 GET \\/admin\\/proxies 请求")]
async fn when_list_proxies(world: &mut TestWorld) {
    send_request(
        world,
        Method::GET,
        "/admin/proxies",
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "发送 GET \\/admin\\/proxies\\/\\{id\\} 请求")]
async fn when_get_proxy(world: &mut TestWorld) {
    let id = latest_proxy_id(world);
    let uri = format!("/admin/proxies/{}", id);
    send_request(
        world,
        Method::GET,
        &uri,
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "发送 PUT \\/admin\\/proxies\\/\\{id\\} 请求更新名称为 updated-name")]
async fn when_update_proxy(world: &mut TestWorld) {
    let id = latest_proxy_id(world);
    let uri = format!("/admin/proxies/{}", id);
    let body = serde_json::json!({"name": "updated-name"}).to_string();
    send_request(
        world,
        Method::PUT,
        &uri,
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
}

#[when(expr = "发送 DELETE \\/admin\\/proxies\\/\\{id\\} 请求")]
async fn when_delete_proxy(world: &mut TestWorld) {
    let id = latest_proxy_id(world);
    let uri = format!("/admin/proxies/{}", id);
    send_request(
        world,
        Method::DELETE,
        &uri,
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "使用普通 key {string} 发送 GET \\/admin\\/proxies 请求")]
async fn when_list_proxies_with_key(world: &mut TestWorld, alias: String) {
    let token = world.created_keys.get(&alias).expect("key").clone();
    send_request(world, Method::GET, "/admin/proxies", Some(&token), None).await;
}

#[when(expr = "发送 POST \\/admin\\/proxies\\/batch-delete 请求删除两个代理")]
async fn when_batch_delete(world: &mut TestWorld) {
    let a: i64 = world
        .created_keys
        .get("proxy:batch-a")
        .expect("batch-a created")
        .parse()
        .expect("batch-a id");
    let b: i64 = world
        .created_keys
        .get("proxy:batch-b")
        .expect("batch-b created")
        .parse()
        .expect("batch-b id");
    let body = serde_json::json!({"ids": [a, b]}).to_string();
    send_request(
        world,
        Method::POST,
        "/admin/proxies/batch-delete",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 123 when steps — toggle / test
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送 POST \\/admin\\/proxies\\/\\{id\\}\\/toggle 请求")]
async fn when_toggle_proxy(world: &mut TestWorld) {
    let id = latest_proxy_id(world);
    let uri = format!("/admin/proxies/{}/toggle", id);
    send_request(
        world,
        Method::POST,
        &uri,
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

#[when(expr = "发送 POST \\/admin\\/proxies\\/\\{id\\}\\/test 请求")]
async fn when_test_proxy(world: &mut TestWorld) {
    let id = latest_proxy_id(world);
    let uri = format!("/admin/proxies/{}/test", id);
    send_request(
        world,
        Method::POST,
        &uri,
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "响应包含 name 字段值为 {string}")]
async fn then_proxy_name_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no name field: {}", body));
    assert_eq!(name, expected);
}

#[then(expr = "响应 proxy_url 字段已 redact 不包含明文密码")]
async fn then_proxy_url_redacted(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    let url = body
        .get("proxy_url")
        .and_then(|v| v.as_str())
        .expect("proxy_url in response");
    assert!(
        url.contains(":***@"),
        "proxy_url should be masked, got: {}",
        url
    );
    assert!(
        !url.contains("secret"),
        "plaintext password leaked: {}",
        url
    );
}

#[then(expr = "响应中的 data 包含 {int} 个代理")]
async fn then_data_has_n_proxies(world: &mut TestWorld, expected: usize) {
    let body = world.last_body.as_ref().expect("no response body");
    let data = body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("data array");
    assert_eq!(data.len(), expected);
}

#[then(expr = "响应包含 status 字段值为 {string}")]
async fn then_proxy_status_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let status = body
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no status field: {}", body));
    assert_eq!(status, expected);
}

#[then(expr = "响应错误 type 为 {string}")]
async fn then_error_type_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let ty = body
        .get("error")
        .and_then(|e| e.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no error.type: {}", body));
    assert_eq!(ty, expected);
}

#[then(expr = "批量删除结果中包含 {int} 个已删除 id")]
async fn then_batch_deleted_count(world: &mut TestWorld, expected: usize) {
    let body = world.last_body.as_ref().expect("no response body");
    let deleted = body
        .get("deleted_ids")
        .and_then(|v| v.as_array())
        .expect("deleted_ids array");
    assert_eq!(deleted.len(), expected);
}

#[then(expr = "响应 probe_result 包含 exit_ip 字段")]
async fn then_probe_has_exit_ip(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    let pr = body
        .get("probe_result")
        .unwrap_or_else(|| panic!("no probe_result in response: {}", body));
    let exit_ip = pr
        .get("exit_ip")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("probe_result missing exit_ip: {}", body));
    assert!(!exit_ip.is_empty());
}

#[then(expr = "响应状态码为 200 或 500 且返回 JSON")]
async fn then_status_200_or_500(world: &mut TestWorld) {
    match world.last_status {
        Some(200) | Some(500) => {}
        other => panic!("expected status 200 or 500, got {:?}", other),
    }
    assert!(
        world.last_body.is_some(),
        "expected JSON body for probe response"
    );
}
