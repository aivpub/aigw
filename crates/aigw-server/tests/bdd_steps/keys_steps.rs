//! Step bindings for keys.feature

use axum::http::Method;
use cucumber::gherkin::Step;
use cucumber::{given, then, when};

use super::common::{build_key_router, make_request};
use crate::TestWorld;

/// Helper: create a key and return (status, body)
async fn create_key(
    router: &axum::Router,
    mk: &str,
    alias: &str,
) -> (u16, Option<serde_json::Value>) {
    let body = serde_json::json!({"key_alias": alias, "models": ["gpt-4"]}).to_string();
    make_request(router, Method::POST, "/key/generate", Some(mk), Some(&body)).await
}

/// Helper: create a key with extra fields (key_alias, max_budget, etc.)
async fn create_key_with_fields(
    router: &axum::Router,
    mk: &str,
    alias: &str,
    extra: serde_json::Value,
) -> (u16, Option<serde_json::Value>) {
    let mut body = serde_json::json!({"key_alias": alias, "models": ["gpt-4"]});
    if let Some(obj) = body.as_object_mut() {
        if let Some(extra_obj) = extra.as_object() {
            for (k, v) in extra_obj {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    make_request(router, Method::POST, "/key/generate", Some(mk), Some(&body.to_string())).await
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "已存在 key {string}")]
async fn existing_key(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let (_, body) = create_key(&router, &world.master_key.clone(), &alias).await;
    if let Some(ref b) = body {
        if let Some(raw) = b.get("key").and_then(|v| v.as_str()) {
            world.created_keys.insert(alias, raw.to_string());
        }
    }
}

#[given(expr = "已存在 {int} 个 key")]
async fn existing_n_keys(world: &mut TestWorld, count: usize) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    for i in 0..count {
        let alias = format!("multi-{}", i);
        let (_, body) = create_key(&router, &world.master_key.clone(), &alias).await;
        if let Some(ref b) = body {
            if let Some(raw) = b.get("key").and_then(|v| v.as_str()) {
                world.created_keys.insert(alias, raw.to_string());
            }
        }
    }
}

#[given(expr = "key {string} 已被删除")]
async fn key_already_deleted(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let (_, body) = create_key(&router, &world.master_key.clone(), &alias).await;
    if let Some(ref b) = body {
        if let Some(raw) = b.get("key").and_then(|v| v.as_str()) {
            let uri = format!("/key/delete?key={}", raw);
            let _ =
                make_request(&router, Method::DELETE, &uri, Some(&world.master_key), None).await;
        }
    }
}

#[given(regex = r#"^已存在 key "(.+)" 且 key_alias="(.+)" max_budget=([\d.]+)$"#)]
async fn existing_key_with_fields(
    world: &mut TestWorld,
    alias: String,
    key_alias: String,
    max_budget: f64,
) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let extra = serde_json::json!({"key_alias": &key_alias, "max_budget": max_budget});
    let (_, body) =
        create_key_with_fields(&router, &world.master_key.clone(), &alias, extra).await;
    if let Some(ref b) = body {
        if let Some(raw) = b.get("key").and_then(|v| v.as_str()) {
            world.created_keys.insert(alias, raw.to_string());
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送 POST \\/key\\/generate 请求")]
async fn when_post_key_generate(world: &mut TestWorld, step: &Step) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let body = step
        .docstring
        .as_ref()
        .expect("docstring body not found")
        .to_string();
    let (s, b) = make_request(
        &router,
        Method::POST,
        "/key/generate",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 POST \\/key\\/generate 请求（无认证）")]
async fn when_post_key_generate_noauth(world: &mut TestWorld, step: &Step) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let body = step
        .docstring
        .as_ref()
        .expect("docstring body not found")
        .to_string();
    let (s, b) = make_request(&router, Method::POST, "/key/generate", None, Some(&body)).await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(regex = r"^发送 GET /key/info\?key=(.+)$")]
async fn when_get_key_info(world: &mut TestWorld, key_ref: String) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let raw_key = world.created_keys.get(&key_ref).cloned().unwrap_or(key_ref);
    let uri = format!("/key/info?key={}", raw_key);
    let (s, b) = make_request(
        &router,
        Method::GET,
        &uri,
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "发送 GET \\/key\\/list")]
async fn when_get_key_list(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let (s, b) = make_request(
        &router,
        Method::GET,
        "/key/list",
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(regex = r"^发送 DELETE /key/delete\?key=(.+)$")]
async fn when_delete_key(world: &mut TestWorld, key_ref: String) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let raw_key = world.created_keys.get(&key_ref).cloned().unwrap_or(key_ref);
    let uri = format!("/key/delete?key={}", raw_key);
    let (s, b) = make_request(
        &router,
        Method::DELETE,
        &uri,
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(regex = r"^发送 POST /key/regenerate (.+)$")]
async fn when_post_regenerate(world: &mut TestWorld, body: String) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    // Replace key aliases in body with actual raw keys
    let mut resolved_body = body.clone();
    for (alias, raw) in &world.created_keys {
        // Replace the alias within the JSON
        resolved_body = resolved_body.replace(alias, raw);
    }
    let (s, b) = make_request(
        &router,
        Method::POST,
        "/key/regenerate",
        Some(&world.master_key.clone()),
        Some(&resolved_body),
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[when(expr = "直接查询 virtual_keys 表")]
async fn when_query_virtual_keys(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let (s, b) = make_request(
        &router,
        Method::GET,
        "/key/list",
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "响应包含 key 字段")]
async fn then_has_key(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    assert!(body.get("key").is_some(), "Body missing 'key': {:?}", body);
}

#[then(expr = "key 以 {string} 开头")]
async fn then_key_prefix(world: &mut TestWorld, prefix: String) {
    let body = world.last_body.as_ref().expect("no body");
    let key = body.get("key").and_then(|v| v.as_str()).expect("no key");
    assert!(
        key.starts_with(&prefix),
        "Key '{}' doesn't start with '{}'",
        key,
        prefix
    );
}

#[then(expr = "key 长度为 {int} 字符")]
async fn then_key_len(world: &mut TestWorld, len: usize) {
    let body = world.last_body.as_ref().expect("no body");
    let key = body.get("key").and_then(|v| v.as_str()).expect("no key");
    assert_eq!(
        key.len(),
        len,
        "Key '{}' len={}, expected {}",
        key,
        key.len(),
        len
    );
}

#[then(expr = "key 主体字符集为 base64url")]
async fn then_key_base64url(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no body");
    let key = body.get("key").and_then(|v| v.as_str()).expect("no key");
    let key_body = key.strip_prefix("sk-").expect("must start with sk-");
    for c in key_body.chars() {
        assert!(
            c.is_ascii_alphanumeric() || c == '-' || c == '_',
            "Char '{}' in '{}' not base64url",
            c,
            key_body
        );
    }
}

#[then(expr = "响应包含 key_alias 字段")]
async fn then_has_key_alias(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no body");
    assert!(
        body.get("key_alias").is_some(),
        "Missing key_alias: {:?}",
        body
    );
}

#[then(expr = "响应包含 {int} 个 key")]
async fn then_has_n_keys(world: &mut TestWorld, expected: usize) {
    let body = world.last_body.as_ref().expect("no body");
    let keys = body
        .get("keys")
        .and_then(|v| v.as_array())
        .expect("no keys array");
    assert_eq!(
        keys.len(),
        expected,
        "Expected {} keys, got {}",
        expected,
        keys.len()
    );
}

#[then(expr = "该 key 不再存在")]
async fn then_key_gone(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let (_, body) = make_request(
        &router,
        Method::GET,
        "/key/list",
        Some(&world.master_key.clone()),
        None,
    )
    .await;
    let keys = body
        .and_then(|b| b.get("keys").cloned())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    assert!(
        keys.len() < world.created_keys.len() || world.created_keys.is_empty(),
        "Deleted key still in list, keys.len={}, created={}",
        keys.len(),
        world.created_keys.len()
    );
}

#[then(expr = "返回新 key")]
async fn then_new_key(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no body");
    let key = body.get("key").and_then(|v| v.as_str()).expect("no key");
    assert!(key.starts_with("sk-"), "Not a valid key: {}", key);
}

#[then(expr = "token 列存储的是 SHA256 hash")]
async fn then_token_is_hash(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no body");
    let keys = body
        .get("keys")
        .and_then(|v| v.as_array())
        .expect("no keys array");
    for key_entry in keys {
        let token = key_entry
            .get("token")
            .and_then(|v| v.as_str())
            .expect("no token field");
        assert_eq!(token.len(), 64, "Token '{}' not 64-char hex", token);
        assert!(
            token.chars().all(|c| c.is_ascii_hexdigit()),
            "Token '{}' not hex",
            token
        );
    }
}

#[then(expr = "token 列不等于明文 key")]
async fn then_token_not_plaintext(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no body");
    let keys = body
        .get("keys")
        .and_then(|v| v.as_array())
        .expect("no keys array");
    for key_entry in keys {
        let token = key_entry
            .get("token")
            .and_then(|v| v.as_str())
            .expect("no token");
        for (_, raw) in &world.created_keys {
            assert_ne!(token, raw, "Token equals plaintext key!");
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Partial update (key/update) steps
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送 PUT \\/key\\/update 请求")]
async fn when_put_key_update(world: &mut TestWorld, step: &Step) {
    let state = world.ensure_state().await;
    let router = build_key_router(state);
    let mut body_str = step
        .docstring
        .as_ref()
        .expect("docstring body not found")
        .to_string();
    // Resolve key aliases to actual raw keys
    for (alias, raw) in &world.created_keys {
        body_str = body_str.replace(alias, raw);
    }
    let (s, b) = make_request(
        &router,
        Method::PUT,
        "/key/update",
        Some(&world.master_key.clone()),
        Some(&body_str),
    )
    .await;
    world.last_status = Some(s);
    world.last_body = b;
}

#[then(regex = r#"^列表中的 key "(.+)" blocked 值为 (true|false)$"#)]
async fn then_list_key_blocked_is(world: &mut TestWorld, alias: String, expected: String) {
    let body = world.last_body.as_ref().expect("no body");
    let keys = body
        .get("keys")
        .and_then(|v| v.as_array())
        .expect("no keys array");

    let raw = world
        .created_keys
        .get(&alias)
        .expect("key not in created_keys");
    let hash = aigw_core::crypto::hash_token(raw);

    let key_entry = keys
        .iter()
        .find(|k| k.get("token").and_then(|v| v.as_str()) == Some(hash.as_str()))
        .unwrap_or_else(|| panic!("Key '{}' (raw={}, hash={}) not found in list. List tokens: {:?}",
            alias, raw, hash,
            keys.iter().map(|k| k.get("token").and_then(|v| v.as_str()).unwrap_or("?")).collect::<Vec<_>>()));

    let blocked = key_entry
        .get("blocked")
        .and_then(|v| v.as_bool())
        .expect("missing 'blocked' field");
    let expected_bool = expected == "true";
    assert_eq!(
        blocked, expected_bool,
        "Expected blocked={}, got {}",
        expected, blocked
    );
}

#[then(regex = r#"^列表中 key "(.+)" 的 其他字段不变 原值为 (.+)$"#)]
async fn then_list_key_other_fields_unchanged(
    world: &mut TestWorld,
    alias: String,
    expected_json: String,
) {
    let body = world.last_body.as_ref().expect("no body");
    let keys = body
        .get("keys")
        .and_then(|v| v.as_array())
        .expect("no keys array");

    let raw = world
        .created_keys
        .get(&alias)
        .expect("key not in created_keys");
    let hash = aigw_core::crypto::hash_token(raw);

    let key_entry = keys
        .iter()
        .find(|k| k.get("token").and_then(|v| v.as_str()) == Some(hash.as_str()))
        .unwrap_or_else(|| panic!("Key '{}' (raw={}, hash={}) not found in list", alias, raw, hash));

    let expected: serde_json::Value =
        serde_json::from_str(&expected_json).expect("invalid expected JSON");

    if let Some(expected_obj) = expected.as_object() {
        for (field, expected_val) in expected_obj {
            let actual_val = key_entry
                .get(field)
                .unwrap_or_else(|| {
                    panic!(
                        "Field '{}' not found in key entry: {:?}",
                        field, key_entry
                    )
                });
            let match_found = if expected_val.is_number() && actual_val.is_number() {
                (expected_val.as_f64().unwrap() - actual_val.as_f64().unwrap()).abs()
                    < f64::EPSILON
            } else {
                actual_val == expected_val
            };
            assert!(
                match_found,
                "Field '{}' expected {:?}, got {:?}",
                field, expected_val, actual_val
            );
        }
    }
}
