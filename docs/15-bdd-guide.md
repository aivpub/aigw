# BDD Testing Guide

> How to write, organize, and run Behaviour-Driven Development tests in the aigw project using cucumber-rust.

## Table of Contents

1. [What is BDD and Why We Use It](#what-is-bdd-and-why-we-use-it)
2. [Project BDD Structure](#project-bdd-structure)
3. [Running Tests](#running-tests)
4. [The TestWorld Struct](#the-testworld-struct)
5. [How to Write a `.feature` File](#how-to-write-a-feature-file)
6. [How to Write Step Bindings](#how-to-write-step-bindings)
7. [Router Builders](#router-builders)
8. [The `make_request()` Helper](#the-make_request-helper)
9. [Mock Upstream Server](#mock-upstream-server)
10. [`@mock` vs `@real_api` Tags](#mock-vs-real_api-tags)
11. [Adding a New Feature File (Step by Step)](#adding-a-new-feature-file-step-by-step)
12. [Adding New Step Bindings (Step by Step)](#adding-new-step-bindings-step-by-step)
13. [Common Pitfalls](#common-pitfalls)
14. [File Reference](#file-reference)

---

## What is BDD and Why We Use It

BDD (Behaviour-Driven Development) is a testing approach where you describe the **behaviour** of your system in plain language, then write automated tests that verify that behaviour. The plain language descriptions live in `.feature` files and use the Gherkin syntax: `Given` / `When` / `Then`.

In aigw, BDD tests serve three purposes:

1. **Executable specification.** A new developer can read a `.feature` file and understand what the system is supposed to do without reading code.
2. **Regression safety net.** Every API endpoint, error path, and cross-protocol integration is covered. Changes that break behaviour fail the build.
3. **Living documentation.** Feature files are versioned alongside the code. Outdated feature files are as visible as outdated code.

We use the **cucumber-rust** crate, which parses `.feature` files and dispatches each step to a Rust function annotated with `#[given]`, `#[when]`, or `#[then]`.

---

## Project BDD Structure

All BDD test code lives under `crates/aigw-server/tests/`. The entry point is `bdd.rs`.

```
crates/aigw-server/tests/
  bdd.rs                        # Test entry point + TestWorld + main()
  features/                     # .feature files (Gherkin scenarios)
    end_to_end.feature
    error_handling.feature
    auth.feature
    keys.feature
    models.feature
    messages.feature
    health.feature
    spend.feature
    spend_aggregation.feature
    adapter.feature
    global.feature
    real/                       # @real_api scenarios (manual opt-in only)
  bdd_steps/                    # Step binding implementations (Rust)
    mod.rs                      # Module declarations
    common.rs                   # Router builders + make_request() helper
    common_steps.rs             # Shared step definitions
    keys_steps.rs               # Steps for keys.feature
    e2e_steps.rs                # Steps for end_to_end.feature
    error_steps.rs              # Steps for error_handling.feature + auth.feature
    health_steps.rs             # Steps for health.feature
    messages_steps.rs           # Steps for messages.feature
    model_steps.rs              # Steps for models.feature
    adapter_steps.rs            # Steps for adapter.feature
    spend_steps.rs              # Steps for spend.feature + spend_aggregation.feature
  bdd_support/                  # Test infrastructure
    mod.rs
    mock_upstream.rs            # In-memory OpenAI + Claude mock server
```

### Key Files Explained

| File | Purpose |
|------|---------|
| `bdd.rs` | Contains the `TestWorld` struct, `Default` impl, `ensure_state()`, and `main()` that launches cucumber-rust. |
| `features/*.feature` | Gherkin scenario files. Each file covers one domain (keys, auth, models, etc.). |
| `bdd_steps/common.rs` | Router builder functions (`build_key_router`, `build_health_router`, `build_spend_router`) and the `make_request()` helper used by most step files. |
| `bdd_steps/common_steps.rs` | Step definitions shared across multiple feature files: `响应状态码为 {int}`, `响应 JSON 包含 "..." 字段`, etc. |
| `bdd_steps/<domain>_steps.rs` | One file per feature domain. Each file registers its own `#[given]`, `#[when]`, `#[then]` functions. |
| `bdd_support/mock_upstream.rs` | An in-memory HTTP server that simulates OpenAI and Claude upstream APIs with configurable responses and request recording. |

---

## Running Tests

### Basic Commands

```bash
# Run BDD tests (mock scenarios only, the default)
cargo test --test bdd -p aigw-server

# Via Taskfile
task bdd
```

### Real API Tests

Real API tests target live OpenAI/Claude endpoints. They require API key environment variables. These tests are **not** run in CI by default.

```bash
# Run only @real_api scenarios
AIGW_REAL_API=1 cargo test --test bdd -p aigw-server -- --tags @real_api

# Via Taskfile
task bdd-real
```

### Full Suite

```bash
# Run all scenarios (mock, then real, in sequence)
task bdd-all
```

### Taskfile Commands

From `Taskfile.yml`:

```yaml
bdd:
  desc: 运行 BDD 测试（仅 @mock）
  cmds:
    - cargo test --test bdd -p aigw-server
    - echo "bdd passed"

bdd-real:
  desc: 运行 BDD 测试（仅 @real_api，需 API key）
  cmds:
    - AIGW_REAL_API=1 cargo test --test bdd -p aigw-server
    - echo "bdd-real passed"

bdd-all:
  desc: 运行全部 BDD 测试（mock + real）
  cmds:
    - cargo test --test bdd -p aigw-server
    - AIGW_REAL_API=1 cargo test --test bdd -p aigw-server
    - echo "bdd-all passed"
```

### Filtering by Tag

cucumber-rust supports filtering by tag:

```bash
# Run only scenarios tagged @mock
cargo test --test bdd -p aigw-server -- --tags @mock

# Run scenarios tagged @mock but NOT @wip (work-in-progress)
cargo test --test bdd -p aigw-server -- --tags @mock and not @wip
```

---

## The TestWorld Struct

`TestWorld` is the shared state object passed to every step binding function. It persists across all steps within a single Scenario and is re-created (via `Default`) for each new Scenario.

**Defined in:** `crates/aigw-server/tests/bdd.rs`

```rust
#[derive(Debug, Clone, cucumber::World)]
pub struct TestWorld {
    /// The shared app state (DB, master_key, etc.). Lazily initialized.
    #[world(skip)]
    pub state: Option<aigw_server::routes::keys::SharedState>,

    /// Master key for admin auth — initialized by Default, reset per scenario.
    pub master_key: String,

    /// Last HTTP response status code.
    #[world(skip)]
    pub last_status: Option<u16>,

    /// Last HTTP response body as JSON.
    #[world(skip)]
    pub last_body: Option<serde_json::Value>,

    /// Created keys by alias -> raw token. Also used for model IDs
    /// with a "model:" prefix convention.
    #[world(skip)]
    pub created_keys: std::collections::HashMap<String, String>,
}
```

### The `#[world(skip)]` Attribute

Fields marked `#[world(skip)]` are **not** reset between scenarios. Their values persist and must be managed explicitly:
- `state` is lazily initialized once via `ensure_state()`.
- `last_status` and `last_body` are set by "When" steps and read by "Then" steps.
- `created_keys` accumulates across scenarios unless explicitly cleared.

Fields **without** `#[world(skip)]` (like `master_key`) are initialized by the `Default` impl at the start of each scenario.

### Default Implementation

```rust
impl Default for TestWorld {
    fn default() -> Self {
        Self {
            state: None,
            master_key: "sk-master-test".to_string(),
            last_status: None,
            last_body: None,
            created_keys: std::collections::HashMap::new(),
        }
    }
}
```

### ensure_state()

The `ensure_state()` method lazily initializes the shared application state (in-memory SQLite DB, provider registry, rate limiter, etc.) on first access:

```rust
impl TestWorld {
    pub async fn ensure_state(&mut self) -> SharedState {
        if self.state.is_none() {
            let db = aigw_core::db::Database::init("sqlite::memory:")
                .await
                .expect("db init");
            let mk = "sk-master-test".to_string();
            let state: SharedState = Arc::new(
                aigw_server::routes::keys::AppState {
                    db,
                    master_key: Some(mk.clone()),
                    provider_registry: aigw_core::provider::ProviderRegistry::new(),
                    router_state: aigw_core::router::RouterState::default(),
                    rate_limiter: Arc::new(aigw_core::rate_limiter::RateLimiter::new()),
                    deployment_mode: "test".to_string(),
                },
            );
            self.master_key = mk;
            self.state = Some(state.clone());
            state
        } else {
            self.state.as_ref().unwrap().clone()
        }
    }
}
```

Every step binding that needs a database or router **must** call `world.ensure_state().await` before constructing a router. See examples below.

### The `created_keys` Map Convention

Keys are stored as `"alias" -> "raw-token"`. Models are stored with a `"model:"` prefix:

```rust
// Key storage
world.created_keys.insert("e2e-user".to_string(), "sk-abc123...".to_string());

// Model ID storage
world.created_keys.insert("model:gpt-4-mock".to_string(), "uuid-xxx".to_string());
```

---

## How to Write a `.feature` File

### Basic Structure

Feature files use the Gherkin language. In this project, we write them in **Chinese** for readability by the team.

```gherkin
@mock
Feature: <Short description of what this file tests>

  Scenario: <One sentence describing this specific scenario>
    Given <precondition>
    And <additional precondition>
    When <action>
    Then <expected outcome>
    And <additional assertion>
```

### Tag Convention

Every file starts with a tag on the first line:

```gherkin
@mock
Feature: ...
```

Supported tags:
- `@mock` -- uses in-memory mock upstream; runs in every CI build; no network required
- `@real_api` -- hits real OpenAI/Claude APIs; only runs when `AIGW_REAL_API=1`

### Examples from the Codebase

**Simple scenario -- no Given, just When/Then (auth.feature):**

```gherkin
@mock
Feature: 授权 (Authorization)
  作为 API 网关
  我需要验证请求的授权
  以确保只有合法用户能访问受保护的资源

  Scenario: 无 Bearer Token 返回 401
    When 不携带 Authorization 发送 GET /key/list 请求
    Then 响应状态码为 401

  Scenario: 无效 Token 返回 401
    When 使用 invalid key 发送 GET /key/list 请求
    Then 响应状态码为 401

  Scenario: Master key 拥有完整访问权限
    When 使用 master-key 发送 GET /key/list 请求
    Then 响应状态码为 200
```

**Full Given/When/Then with mock upstream (end_to_end.feature):**

```gherkin
@mock
Feature: 端到端调用链路（mock）

  Scenario: OpenAI 协议调用 mock 上游返回成功
    Given mock 上游已启动
    And 已配置 model "gpt-4-mock" 指向 mock 上游
    And 一个普通 key "e2e-user" 已生成
    When 使用 key "e2e-user" 发送 POST /chat/completions 请求
    Then 响应状态码为 200
    And mock 上游收到请求

  Scenario: 上游错误码透传
    Given mock 上游已启动
    And mock 上游 "/v1/chat/completions" 返回状态码 500
    And 一个普通 key "e2e-error-user" 已生成
    When 使用 key "e2e-error-user" 发送 POST /chat/completions 请求
    Then 响应状态码为 500 或 502
```

**Scenario using DocStrings (keys.feature):**

```gherkin
  Scenario: 成功生成新 key
    When 发送 POST /key/generate 请求
      """
      {"key_alias": "new-key", "models": ["gpt-4"]}
      """
    Then 响应状态码为 200
    And 响应包含 key 字段
```

**Error scenarios with custom step text (error_handling.feature):**

```gherkin
  Scenario: 请求缺少 model 字段返回 400
    Given 一个普通 key "err-no-model" 已生成
    When 使用 key "err-no-model" 发送 POST /chat/completions 缺少 model
    Then 响应状态码为 400

  Scenario: upstream 500 error passthrough
    Given mock 上游已启动
    And mock 上游 "/v1/chat/completions" 返回状态码 500
    And 一个普通 key "err-upstream" 已生成
    When 使用 key "err-upstream" 发送 POST /chat/completions 请求
    Then 响应状态码为 500 或 502
```

### Best Practices

1. **One feature per file.** `keys.feature`, `models.feature`, `auth.feature`, etc.
2. **Keep scenarios independent.** Each scenario sets up its own state. No scenario depends on another scenario's side effects.
3. **Use descriptive aliases.** Key aliases like `"e2e-user"` or `"err-no-model"` make test output traces easy to read.
4. **Use the DocString syntax** (triple quotes) for request bodies in the feature file.

---

## How to Write Step Bindings

Step bindings connect the Gherkin text to Rust functions. cucumber-rust matches step text using either `expr` (exact match with placeholders) or `regex`.

### The Two Matching Modes

#### `expr` (Exact String Match with Placeholders)

Use this when the step text is a fixed string, possibly with typed placeholders.

| Placeholder | Matches | Rust Type |
|-------------|---------|-----------|
| `{string}` | A double-quoted string like `"hello"` | `String` |
| `{int}` | An integer literal like `42` | `u16`, `usize`, `i32`, etc. |
| `{word}` | A single word (no spaces, no quotes) | `String` |

**Examples from the codebase:**

```rust
// Matches: 响应状态码为 200
// Where 200 is captured as the u16 parameter
#[then(expr = "响应状态码为 {int}")]
async fn then_status_is(world: &mut TestWorld, expected: u16) {
    assert_eq!(world.last_status, Some(expected));
}

// Matches: 已配置 model "gpt-4-mock" 指向 mock 上游
// Where "gpt-4-mock" (without quotes) is captured as the String parameter
#[given(expr = "已配置 model {string} 指向 mock 上游")]
async fn given_model_points_to_mock(world: &mut TestWorld, name: String) {
    // name == "gpt-4-mock"
}

// Matches: 响应包含 3 个 key
// Where 3 is captured as the usize parameter
#[then(expr = "响应包含 {int} 个 key")]
async fn then_has_n_keys(world: &mut TestWorld, expected: usize) {
    // ...
}
```

#### `regex` (Regular Expression Match)

Use this when the step text has dynamic parts that `{string}` and `{int}` cannot express, such as query parameters or variable text inside double quotes.

```rust
// Matches: 错误 type 为 "server_error"
// Capture group (.+) extracts the quoted value
#[then(regex = r#"^错误 type 为 "(.+)"$"#)]
async fn then_error_type_is(world: &mut TestWorld, expected: String) {
    // expected == "server_error"
}

// Matches: 响应中的 data 包含 5 个模型
// Capture group (\d+) extracts the digit — must be double-escaped in Rust raw string
#[then(regex = r#"^响应中的 data 包含 (\d+) 个模型$"#)]
async fn then_data_has_n_models(world: &mut TestWorld, expected: usize) {
    // expected == 5
}

// Matches: 发送 GET /key/info?key=sk-abc123
// Note the escaped ? in the regex
#[when(regex = r"^发送 GET /key/info\?key=(.+)$")]
async fn when_get_key_info(world: &mut TestWorld, key_ref: String) {
    // key_ref == "sk-abc123"
}

// Matches: OpenAI 响应的 object 为 "chat.completion"
#[then(regex = r#"^OpenAI 响应的 object 为 "(.+)"$"#)]
async fn then_openai_object_is(world: &mut TestWorld, expected: String) {
    // ...
}
```

### The `\/` Escape Rule (Critical)

**In `expr` mode, forward slashes in step text must be escaped as `\/`.** This is a cucumber-rust expr parser requirement because `/` is treated as a step separator in Gherkin syntax.

In your Rust source, you write `\\/` to produce the literal string `\/`:

```rust
// CORRECT — the expr parser sees: 发送 POST \/chat\/completions 请求
#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions 请求")]

// CORRECT — the expr parser sees: 发送 POST \/v1\/messages 请求
#[when(expr = "发送 POST \\/v1\\/messages 请求")]

// CORRECT — same pattern for all path segments
#[when(expr = "发送 GET \\/key\\/list 请求")]
#[when(expr = "发送 GET \\/health\\/readiness 请求")]
#[when(expr = "发送 POST \\/model\\/new 请求")]

// WRONG — will not match, or will cause a parse error
#[when(expr = "发送 POST /chat/completions 请求")]
```

**In `regex` mode, slashes in the pattern itself do not need `\/` escaping.** Only escaping that the regex engine requires (like `\(`, `\)`, `\?`) applies:

```rust
// Correct in regex mode — no \/ needed
#[when(regex = r"^发送 GET /key/info\?key=(.+)$")]
```

**Feature files do NOT use `\/` escaping.** In `.feature` files, just write paths normally:

```gherkin
When using key "x" send POST /chat/completions without model
When using invalid key send GET /key/list request
```

The `\/` is only needed in the Rust `#[given]` / `#[when]` / `#[then]` attribute strings.

### DocString Body Extraction

When a step expects a DocString body (the `"""..."""` block in the feature file), access it via `step.docstring`:

```rust
#[when(expr = "发送 POST \\/key\\/generate 请求")]
async fn when_post_key_generate(world: &mut TestWorld, step: &Step) {
    let body = step.docstring.as_ref().expect("docstring body not found").to_string();
    let (s, b) = make_request(
        &router,
        Method::POST,
        "/key/generate",
        Some(&world.master_key),
        Some(&body),
    ).await;
    world.last_status = Some(s);
    world.last_body = b;
}
```

**Rule:** If you need a `{string}` param plus the docstring, the `&Step` parameter **must come first** in the function signature:

```rust
#[when(expr = "发送 POST \\/v1\\/messages 请求带认证 model={string}")]
async fn when_post_messages_with_model(world: &mut TestWorld, step: &Step, model_name: String) {
    let body = step.docstring.as_ref().expect("docstring body").to_string();
    // model_name == "gpt-4"
}
```

### Complete Step Binding File Example

Here is `health_steps.rs` -- a clean, self-contained step file:

```rust
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
    assert!(body.get("status").is_some());
}
```

### Shared Steps in common_steps.rs

Steps used by more than one feature file are defined once in `common_steps.rs` and **must not** be duplicated in any domain-specific step file. Duplicate definitions cause a runtime panic.

```rust
//! Shared step definitions — used by all .feature files
//! Steps defined here MUST NOT be duplicated in any other *_steps.rs file.

use cucumber::{given, then};
use crate::TestWorld;

#[given(expr = "管理员已认证")]
async fn admin_authenticated(_world: &mut TestWorld) {}

#[then(expr = "响应状态码为 {int}")]
async fn then_status_is(world: &mut TestWorld, expected: u16) {
    assert_eq!(world.last_status, Some(expected));
}

#[then(regex = r#"^响应 JSON 包含 "(.+)" 字段$"#)]
async fn then_json_contains_field(world: &mut TestWorld, field: String) {
    let body = world.last_body.as_ref().expect("no response body");
    assert!(body.get(&field).is_some());
}
```

---

## Router Builders

Router builders create minimal axum `Router` instances containing only the routes needed for a given test. Using a focused router (rather than the full application router) makes tests faster and isolates the code under test.

### Shared Builders in common.rs

These are used by multiple step modules:

```rust
/// Key management: /key/generate, /key/info, /key/list, /key/update, /key/delete, /key/regenerate
pub fn build_key_router(state: SharedState) -> Router {
    Router::new()
        .route("/key/generate", axum::routing::post(keys::generate_key))
        .route("/key/info", axum::routing::get(keys::key_info))
        .route("/key/list", axum::routing::get(keys::key_list))
        .route("/key/update", axum::routing::put(keys::key_update))
        .route("/key/delete", axum::routing::delete(keys::key_delete))
        .route("/key/regenerate", axum::routing::post(keys::key_regenerate))
        .with_state(state)
}

/// Health check: /health, /health/readiness, /health/liveliness
pub fn build_health_router() -> Router {
    Router::new()
        .route("/health", axum::routing::get(health::health))
        .route("/health/readiness", axum::routing::get(health::readiness))
        .route("/health/liveliness", axum::routing::get(health::liveliness))
}

/// Spend routes: /spend/logs, /spend/keys, /spend/users, /global/spend, etc.
pub fn build_spend_router(state: SharedState) -> Router {
    Router::new()
        .route("/spend/logs", axum::routing::get(spend::spend_logs))
        .route("/spend/keys", axum::routing::get(spend::spend_keys))
        .route("/spend/users", axum::routing::get(spend::spend_users))
        // ... more routes ...
        .with_state(state)
}
```

### Per-Module Inline Routers

Some step files define their own router locally, rather than adding to `common.rs`. This is preferred when a router is only used by one feature's steps.

**From `messages_steps.rs`:**

```rust
fn build_messages_router(state: SharedState) -> Router {
    Router::new()
        .route("/v1/messages", axum::routing::post(messages_handler))
        .with_state(state)
}
```

**From `model_steps.rs`:**

```rust
fn build_model_router(state: SharedState) -> Router {
    Router::new()
        .route("/model/new", axum::routing::post(model_new))
        .route("/model/info", axum::routing::get(model_info))
        .route("/model/list", axum::routing::get(model_list))
        .route("/model/update", axum::routing::put(model_update))
        .route("/model/delete", axum::routing::delete(model_delete))
        .with_state(state)
}
```

### Inline (One-Off) Routers in Steps

For steps that only need one route, construct the `Router` inline in the step function (as done in `error_steps.rs` and `e2e_steps.rs`):

```rust
#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions 请求")]
async fn when_post_chat_completions(world: &mut TestWorld, alias: String) {
    let state = world.ensure_state().await;

    let app = Router::new()
        .route("/chat/completions", axum::routing::post(chat_completions))
        .with_state(state);

    let token = world.created_keys.get(&alias).expect("key not found");
    let body = serde_json::json!({
        "model": "gpt-4-mock",
        "messages": [{"role": "user", "content": "hi"}]
    }).to_string();

    let req = Request::builder()
        .method(Method::POST)
        .uri("/chat/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::from(body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    // ... extract body ...
}
```

---

## The `make_request()` Helper

`make_request()` is the standard way to send an HTTP request through an axum `Router` and obtain the status and JSON body. It is used by most step files.

**Defined in:** `bdd_steps/common.rs`

```rust
/// Helper: send an HTTP request and return (status, json_body)
pub async fn make_request(
    app: &Router,
    method: Method,
    uri: &str,
    auth: Option<&str>,        // If Some, adds "Authorization: Bearer <value>" header
    body: Option<&str>,        // If Some, sets this as the JSON request body
) -> (u16, Option<serde_json::Value>) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");

    if let Some(token) = auth {
        req = req.header(header::AUTHORIZATION, format!("Bearer {}", token));
    }

    let req_body = body
        .map(|b| Body::from(b.to_string()))
        .unwrap_or(Body::empty());

    let request = req.body(req_body).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();

    let json_body = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
        Ok(bytes) => serde_json::from_slice(&bytes).ok(),
        Err(_) => None,
    };

    (status, json_body)
}
```

### Usage Patterns

**No auth, no body (GET health check):**

```rust
let app = build_health_router();
let (status, body) = make_request(&app, Method::GET, "/health", None, None).await;
```

**With auth, no body (GET with master key):**

```rust
let (status, body) = make_request(
    &router,
    Method::GET,
    "/key/list",
    Some(&world.master_key),
    None,
).await;
```

**With auth and body (POST key generate):**

```rust
let body = serde_json::json!({"key_alias": "my-key", "models": ["gpt-4"]}).to_string();
let (status, body) = make_request(
    &router,
    Method::POST,
    "/key/generate",
    Some(&world.master_key),
    Some(&body),
).await;
```

### When NOT to Use make_request()

If your step needs custom headers beyond `Authorization: Bearer` and `Content-Type` (for example, `anthropic-version` or `x-api-key` for the Claude Messages endpoint), build the request manually using `axum::http::Request::builder()` and `tower::util::ServiceExt::oneshot()`. See `messages_steps.rs` for examples of this pattern.

---

## Mock Upstream Server

The mock upstream server simulates OpenAI and Claude APIs without requiring real API keys or network access. It is defined in `bdd_support/mock_upstream.rs`.

### Architecture

```
Test Scenario
  |-> MockUpstream::start()          # Starts on ephemeral port (127.0.0.1:0)
  |     |-> /v1/chat/completions     # Simulates OpenAI
  |     |-> /v1/messages              # Simulates Claude
  |
  |-> MockUpstream::set_response()   # Configure what the mock returns
  |-> [test runs...]                 # The gateway calls the mock upstream
  |-> MockUpstream::request_count()  # Verify the gateway made the expected calls
  |-> [drop]                         # Auto-shutdown via Drop impl
```

### Key Types

```rust
pub struct MockUpstream {
    pub base_url: String,            // e.g. "http://127.0.0.1:54321"
    shutdown_tx: Option<oneshot::Sender<()>>,
    state: Arc<MockState>,
}

pub struct MockState {
    pub requests: Arc<Mutex<Vec<RecordedRequest>>>,   // All received requests
    pub responses: Arc<Mutex<HashMap<String, MockResponse>>>, // Configured responses
}

pub struct RecordedRequest {
    pub path: String,                // e.g. "/v1/chat/completions"
    pub headers: HashMap<String, String>,
    pub body: Value,
}

pub struct MockResponse {
    pub status: u16,
    pub body: Value,
    pub headers: HashMap<String, String>,
}
```

### API

```rust
impl MockUpstream {
    /// Start a mock upstream server on an ephemeral port.
    pub async fn start() -> Self;

    /// Returns the base URL (e.g. "http://127.0.0.1:54321").
    pub fn url(&self) -> &str;

    /// Configure the response for a specific path (e.g. "/v1/chat/completions").
    pub fn set_response(&self, path: &str, status: u16, body: Value);

    /// Return all recorded requests for inspection in Then steps.
    pub fn recorded_requests(&self) -> Vec<RecordedRequest>;

    /// Number of requests received by the mock server.
    pub fn request_count(&self) -> usize;

    /// Reset custom responses to defaults (for scenario isolation between tests).
    pub fn reset_responses(&self);
}
```

The **default response** (when no custom response is configured) returns a valid OpenAI chat completion with:

```json
{
  "id": "chatcmpl-mock",
  "object": "chat.completion",
  "model": "gpt-4",
  "choices": [{
    "index": 0,
    "message": {"role": "assistant", "content": "Mock response from upstream"},
    "finish_reason": "stop"
  }],
  "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
}
```

### Global Singleton Pattern in Step Files

Because the mock upstream must outlive individual scenarios, it is stored as a global singleton using `OnceLock`:

```rust
// In e2e_steps.rs
static MOCK_UPSTREAM: std::sync::OnceLock<Arc<tokio::sync::Mutex<Option<MockUpstream>>>> =
    std::sync::OnceLock::new();

fn mock_upstream() -> &'static Arc<tokio::sync::Mutex<Option<MockUpstream>>> {
    MOCK_UPSTREAM.get_or_init(|| Arc::new(tokio::sync::Mutex::new(None)))
}
```

The singleton is initialized on first use and reused across scenarios within a test run.

### Usage in Steps

```rust
#[given(expr = "mock 上游已启动")]
async fn given_mock_upstream_started(_world: &mut TestWorld) {
    let mu = mock_upstream();
    let mut guard = mu.lock().await;
    if guard.is_none() {
        let upstream = MockUpstream::start().await;
        // Tell the chat handler to use our mock upstream
        std::env::set_var("UPSTREAM_LLM_URL", format!("{}/v1", upstream.url()));
        *guard = Some(upstream);
    } else {
        // Reset mock responses for scenario isolation
        guard.as_mut().unwrap().reset_responses();
    }
}

#[given(expr = "mock 上游 {string} 返回状态码 {int}")]
async fn given_mock_returns_status(_world: &mut TestWorld, path: String, status: u16) {
    let mu = mock_upstream().lock().await;
    let upstream = mu.as_ref().expect("mock upstream not started");
    upstream.set_response(
        &path,
        status,
        serde_json::json!({"error": {"message": "mock error", "type": "server_error"}}),
    );
}

#[then(expr = "mock 上游收到请求")]
async fn then_mock_received_request(_world: &mut TestWorld) {
    let mu = mock_upstream().lock().await;
    let count = mu.as_ref().unwrap().request_count();
    assert!(count > 0, "Expected mock upstream to receive at least 1 request, got 0");
}
```

The `UPSTREAM_LLM_URL` env var tells the chat handler where to proxy requests. It must include the `/v1` suffix because the handler appends `/chat/completions` to it.

---

## `@mock` vs `@real_api` Tags

| | `@mock` | `@real_api` |
|---|---|---|
| Upstream | In-memory mock server | Real OpenAI/Claude API |
| CI runs it? | Yes, always | No (disabled by default) |
| Cost | Free | Incurs API costs |
| Speed | Instant | Network latency |
| Deterministic? | Yes | No (model outputs vary) |
| Requires API keys? | No | Yes (`OPENAI_API_KEY` or `ANTHROPIC_API_KEY`) |

### How Tag Filtering Works

The tag on the first line of each `.feature` file determines which group the scenarios belong to:

```gherkin
@mock
Feature: ...

@real_api
Feature: ...
```

cucumber-rust respects the `--tags` filter. When you run `cargo test --test bdd -- --tags @mock`, only scenarios tagged `@mock` are executed. Real API scenarios are not compiled into the binary (they are in separate feature files under `features/real/`).

### Why We Skip `@real_api` in CI

1. **Cost control.** Real API calls cost money on every run.
2. **Determinism.** Model outputs vary; assertions on real content would be flaky.
3. **Speed.** Network calls slow down CI.

Real API scenarios exist for manual verification. Run them locally with `task bdd-real` when making changes to adapter or protocol conversion logic.

---

## Adding a New Feature File (Step by Step)

Suppose you want to add BDD coverage for a new domain -- say "usage tracking."

### Step 1: Create the Feature File

Create `crates/aigw-server/tests/features/usage.feature`:

```gherkin
@mock
Feature: 用量追踪

  Scenario: 记录成功调用的 token 用量
    Given 一个普通 key "usage-basic" 已生成
    When 使用 key "usage-basic" 发送 POST /chat/completions 请求
    Then 响应状态码为 200
    And 用量记录包含本次调用

  Scenario: 无调用时用量为空
    Given 一个普通 key "usage-empty" 已生成
    When 查询该 key 的用量
    Then 用量为 0
```

### Step 2: Create the Step Bindings File

Create `crates/aigw-server/tests/bdd_steps/usage_steps.rs`:

```rust
use cucumber::{then, when};
use axum::http::Method;
use super::common::{build_spend_router, make_request};
use crate::TestWorld;

#[then(expr = "用量记录包含本次调用")]
async fn then_spend_log_contains_call(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = build_spend_router(state);
    let (status, body) = make_request(
        &router,
        Method::GET,
        "/spend/logs",
        Some(&world.master_key),
        None,
    ).await;

    assert_eq!(status, 200);
    let data = body
        .and_then(|b| b.get("data").cloned())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    assert!(!data.is_empty(), "Expected at least one spend log entry");
}

#[when(expr = "查询该 key 的用量")]
async fn when_query_key_usage(world: &mut TestWorld) {
    // Look up the key and query /spend/keys
}

#[then(expr = "用量为 {int}")]
async fn then_usage_is(world: &mut TestWorld, expected: u64) {
    // Assert token count
}
```

### Step 3: Register the Module

Add the new module to `crates/aigw-server/tests/bdd_steps/mod.rs`:

```rust
pub mod adapter_steps;
pub mod common;
pub mod common_steps;
pub mod e2e_steps;
pub mod error_steps;
pub mod health_steps;
pub mod keys_steps;
pub mod messages_steps;
pub mod model_steps;
pub mod spend_steps;
pub mod usage_steps;           // <-- ADD THIS LINE
```

### Step 4: Run and Verify

```bash
cargo test --test bdd -p aigw-server
```

If any step text in the feature file has no matching Rust function, cucumber-rust will report `Step is not matched` with a hint about which step is missing. Add bindings for each missing step and re-run until green.

---

## Adding New Step Bindings (Step by Step)

### Step 1: Identify the Step Text

Read the feature file and find the unmatched step. For example:

```gherkin
Then 用量记录包含本次调用
```

### Step 2: Choose the Matching Mode

- If the step text is entirely fixed (no dynamic parts), use `#[then(expr = "...")]`.
- If the step text has dynamic parts not expressible as `{string}` or `{int}` (e.g. query parameters, quoted identifiers), use `#[then(regex = r"...")]`.

For `"用量记录包含本次调用"`, the step is entirely fixed, so use `expr`.

### Step 3: Write the Function

Add the function to the appropriate `*_steps.rs` file. Use the pattern:

```rust
#[then(expr = "用量记录包含本次调用")]
async fn then_spend_log_contains_call(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let router = super::common::build_spend_router(state);
    let (status, body) = super::common::make_request(
        &router,
        Method::GET,
        "/spend/logs",
        Some(&world.master_key),
        None,
    ).await;

    assert_eq!(status, 200);
    let data = body
        .and_then(|b| b.get("data").cloned())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    assert!(!data.is_empty(), "Expected at least one spend log entry");
}
```

### Step 4: Check for Duplicate Step Definitions

Before finalizing, verify that no other `*_steps.rs` file already defines the same step text. **Duplicate step bindings cause a runtime panic** when cucumber-rust starts.

If the step is already defined elsewhere, either:
- Reuse the existing definition (don't change anything).
- Move the shared step to `common_steps.rs` and remove duplicates from domain-specific files.

### Step 5: Run and Verify

```bash
cargo test --test bdd -p aigw-server
```

---

## Common Pitfalls

### 1. Double `Bearer` Prefix

The `make_request()` helper **automatically** prepends `Bearer ` to the auth token. Do not include `Bearer` in the value you pass:

```rust
// CORRECT — make_request adds "Authorization: Bearer sk-master-test"
make_request(&router, Method::GET, "/key/list", Some("sk-master-test"), None).await;

// WRONG — Results in "Authorization: Bearer Bearer sk-master-test"
make_request(&router, Method::GET, "/key/list", Some("Bearer sk-master-test"), None).await;
```

### 2. `\/` Escaping in `expr` Mode (See Section Above)

Every path segment boundary needs `\/` in `expr` mode. Feature files use plain `/`. This is the single most common cause of "Step is not matched" errors.

### 3. `max_concurrent_scenarios(1)` is Mandatory

In `bdd.rs`, we force sequential scenario execution:

```rust
TestWorld::cucumber()
    .max_concurrent_scenarios(1)   // DO NOT REMOVE
    .run("tests/features")
    .await;
```

This is required because:
- The in-memory SQLite database is shared across scenarios within a test run.
- The mock upstream server is stored as a global singleton.
- Concurrent scenarios would race on shared state.

### 4. Duplicate Step Definitions

If two `*_steps.rs` files register the same step text, cucumber-rust panics at startup with an "ambiguous step" error. Move shared steps to `common_steps.rs` and remove duplicates.

### 5. Missing `ensure_state()` Call

If your step binding touches the database or any state-dependent router, call `world.ensure_state().await` first. Omitting this call may work when another step in the same scenario has already initialized the state, but it is not guaranteed.

### 6. DocString Extraction Must Use `&Step` as First Param

When a step expects a DocString body (`"""..."""` in the feature file), the `&Step` parameter must appear **first** in the function signature, before any typed params:

```rust
// Correct — &Step first
#[when(expr = "发送 POST \\/v1\\/messages 请求带认证 model={string}")]
async fn when_post_messages_with_model(world: &mut TestWorld, step: &Step, model_name: String) { ... }
```

### 7. The `UPSTREAM_LLM_URL` `/v1` Suffix

When setting the upstream URL for mock tests, include the `/v1` suffix because the handler appends `/chat/completions` to it:

```rust
std::env::set_var("UPSTREAM_LLM_URL", format!("{}/v1", upstream.url()));
```

The final proxy URL becomes `http://127.0.0.1:54321/v1/chat/completions`.

### 8. Forgetting to Reset Mock Responses

When the mock upstream already exists (second scenario onwards), call `reset_responses()` to clear custom responses from the previous scenario:

```rust
if guard.is_none() {
    let upstream = MockUpstream::start().await;
    // ...
    *guard = Some(upstream);
} else {
    guard.as_mut().unwrap().reset_responses();  // <-- Important for isolation
}
```

---

## File Reference

| File | Lines | What It Contains |
|------|-------|-----------------|
| `tests/bdd.rs` | 77 | TestWorld struct, Default impl, ensure_state(), main() with max_concurrent_scenarios(1) |
| `tests/bdd_steps/mod.rs` | 13 | Module declarations for all step files |
| `tests/bdd_steps/common.rs` | 137 | build_key_router, build_health_router, build_spend_router, make_request |
| `tests/bdd_steps/common_steps.rs` | 32 | Shared steps: 响应状态码为 {int}, 响应 JSON 包含 "..." 字段 |
| `tests/bdd_steps/e2e_steps.rs` | 145 | Mock upstream Given/When/Then, global singleton pattern |
| `tests/bdd_steps/error_steps.rs` | 243 | Chat error steps (missing model, invalid JSON) + auth variants |
| `tests/bdd_steps/keys_steps.rs` | 320 | Key CRUD steps with docstring bodies and regex query params |
| `tests/bdd_steps/messages_steps.rs` | 254 | /v1/messages steps with anthropic-version, x-api-key headers |
| `tests/bdd_steps/model_steps.rs` | 298 | Model CRUD steps with model_id lookups |
| `tests/bdd_steps/health_steps.rs` | 42 | Health check endpoint steps |
| `tests/bdd_steps/adapter_steps.rs` | 235 | Protocol adapter unit-test style steps |
| `tests/bdd_support/mod.rs` | 3 | Support module declarations |
| `tests/bdd_support/mock_upstream.rs` | 276 | MockUpstream, MockState, RecordedRequest, MockResponse |
| `tests/features/end_to_end.feature` | 31 | E2E scenarios with mock upstream |
| `tests/features/error_handling.feature` | 44 | Error handling scenarios |
| `tests/features/auth.feature` | 29 | Authorization scenarios |
| `features/keys.feature` | -- | Key management scenarios |
| `features/models.feature` | -- | Model management scenarios |
| `features/messages.feature` | -- | /v1/messages endpoint scenarios |
| `features/health.feature` | -- | Health check scenarios |
| `features/spend.feature` | -- | Spend tracking scenarios |
| `features/adapter.feature` | -- | Protocol adapter conversion scenarios |
