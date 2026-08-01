# Stage 64: 三级 router_settings + API + 前端

**Phase**: 23 — Router 负载均衡
**状态**: ⏳ 待开始
**预估**: 8h
**依赖**: Stage 63

---

## 目标

1. **三级配置读取** — 启动时读 Global `router_settings` → 请求时 Key/Team 级覆盖合并
2. **REST API** — 读写 Global / Key / Team 三级的 `router_settings`
3. **前端 RouterSettingsAccordion** — ModelDialog / CreateKey / TeamInfo 嵌入路由配置

---

## Part A — 三级配置读取 (3h)

### 1.1 RouterConfig

`crates/aigw-core/src/router.rs`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct RouterConfig {
    #[serde(default = "default_routing_strategy")]
    pub routing_strategy: String,          // "simple-shuffle"
    #[serde(default)]
    pub num_retries: u32,                  // 0
    #[serde(default = "default_allowed_fails")]
    pub allowed_fails: u32,                // 3
    #[serde(default = "default_cooldown_time")]
    pub cooldown_time: f64,                // 5.0
    #[serde(default)]
    pub model_group_alias: HashMap<String, String>,
}

fn default_routing_strategy() -> String { "simple-shuffle".into() }
fn default_allowed_fails() -> u32 { 3 }
fn default_cooldown_time() -> f64 { 5.0 }
```

### 1.2 启动时加载

`crates/aigw-server/src/main.rs` 或 `AppState` 构建时：

```rust
// 从 config 表读 router_settings
let router_config: RouterConfig = match db.query_config("router_settings").await {
    Some(json_str) => serde_json::from_str(&json_str)
        .unwrap_or_else(|e| {
            tracing::warn!(error=%e, "Failed to parse router_settings, using defaults");
            RouterConfig::default()
        }),
    None => {
        tracing::info!("No router_settings in config table, using defaults");
        RouterConfig::default()
    }
};
let router = Arc::new(RwLock::new(Router::from_config(&router_config)));
```

### 1.3 请求时三级合并

```rust
/// Merge Key > Team > Global router settings into an effective override.
/// Returns None if no overrides present (use Router as-is).
fn merge_router_overrides(
    key_settings: Option<&Value>,
    team_settings: Option<&Value>,
    global: &RouterConfig,
) -> RouterConfig {
    let mut merged = global.clone();

    // Layer 2: Team override
    if let Some(ts) = team_settings {
        apply_override(&mut merged, ts);
    }

    // Layer 1: Key override (highest priority)
    if let Some(ks) = key_settings {
        apply_override(&mut merged, ks);
    }

    merged
}

fn apply_override(config: &mut RouterConfig, overrides: &Value) {
    if let Some(v) = overrides.get("allowed_fails").and_then(|v| v.as_u64()) {
        config.allowed_fails = v as u32;
    }
    if let Some(v) = overrides.get("cooldown_time").and_then(|v| v.as_f64()) {
        config.cooldown_time = v;
    }
    if let Some(v) = overrides.get("num_retries").and_then(|v| v.as_u64()) {
        config.num_retries = v as u32;
    }
    if let Some(v) = overrides.get("routing_strategy").and_then(|v| v.as_str()) {
        config.routing_strategy = v.to_string();
    }
}
```

Handler 调用：
```rust
let effective = merge_router_overrides(
    key.router_settings.as_ref(),
    team.as_ref().and_then(|t| t.router_settings.as_ref()),
    &state.router_config,
);
let temp_router = Router::from_config(&effective);
// 在 retry loop 中用 temp_router 的 allowed_fails / cooldown_time / num_retries
```

---

## Part B — REST API (2h)

### 2.1 路由定义

`crates/aigw-server/src/routes/router_settings.rs`:

```rust
pub fn router() -> Router {
    Router::new()
        .route("/router/settings", get(get_global).put(put_global))
        .route("/key/{token}/router/settings", patch(patch_key))
        .route("/team/{id}/router/settings", patch(patch_team))
}
```

### 2.2 Handlers

**`GET /router/settings`**:
```rust
async fn get_global(State(state): State<Arc<AppState>>) -> Result<Json<Value>> {
    let val = state.db.get_config("router_settings").await?;
    let json: Value = serde_json::from_str(&val).unwrap_or(json!({}));
    Ok(Json(json))
}
```

**`PUT /router/settings`**:
```rust
async fn put_global(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    // 校验字段名合法性
    validate_router_settings_keys(&body)?;
    // 写入 config 表
    state.db.upsert_config("router_settings", &body.to_string()).await?;
    // 热更新 Router
    let new_config: RouterConfig = serde_json::from_value(body.clone())?;
    *state.router.write().await = Router::from_config(&new_config);
    tracing::info!("Router settings updated (hot reload)");
    Ok(Json(body))
}
```

**`PATCH /key/{token}/router/settings`**:
```rust
async fn patch_key(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    validate_router_settings_keys(&body)?;
    state.db.update_key_router_settings(&token, &body).await?;
    Ok(Json(body))
}
```

**`PATCH /team/{id}/router/settings`**:
```rust
async fn patch_team(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    validate_router_settings_keys(&body)?;
    state.db.update_team_router_settings(&id, &body).await?;
    Ok(Json(body))
}
```

### 2.3 字段白名单校验

```rust
const VALID_ROUTER_SETTINGS_KEYS: &[&str] = &[
    "routing_strategy", "num_retries", "allowed_fails",
    "cooldown_time", "retry_after", "fallbacks",
    "model_group_alias", "routing_groups",
];

fn validate_router_settings_keys(body: &Value) -> Result<()> {
    if let Some(obj) = body.as_object() {
        for key in obj.keys() {
            if !VALID_ROUTER_SETTINGS_KEYS.contains(&key.as_str()) {
                return Err(AppError::bad_request(
                    format!("Invalid router_settings key: '{}'. Valid keys: {:?}", key, VALID_ROUTER_SETTINGS_KEYS)
                ));
            }
        }
    }
    Ok(())
}
```

---

## Part C — 前端 RouterSettingsAccordion (3h)

### 3.1 新建组件

```
crates/aigw-frontend/src/components/router-settings/
├── RouterSettingsAccordion.tsx    # 折叠面板容器
├── RoutingStrategySelector.tsx    # 路由策略下拉
└── ReliabilityRetriesSection.tsx  # 重试/冷却/失败阈值 输入
```

### 3.2 RouterSettingsAccordion

对外接口：
```tsx
interface RouterSettingsAccordionValue {
    routing_strategy?: string;
    num_retries?: number;
    allowed_fails?: number;
    cooldown_time?: number;
}

interface RouterSettingsAccordionProps {
    defaultValue?: RouterSettingsAccordionValue;
    onChange?: (value: RouterSettingsAccordionValue) => void;
    // ref 方式暴露 getValue() 供 CreateKey/TeamInfo 在保存时读取
    ref?: React.Ref<RouterSettingsAccordionRef>;
}
```

展开时显示三个配置区：
1. **路由策略**: Select dropdown — `simple-shuffle` (默认) / `least-busy` / `usage-based-routing` / `latency-based-routing`（后三个 grayed + "即将推出" 标记）
2. **重试次数** (`num_retries`): NumberInput 0-10，默认 0
3. **可靠性设置**: `allowed_fails` (1-100, 默认 3) + `cooldown_time` 秒 (1-3600, 默认 5)

底部有 "恢复默认" 按钮。

### 3.3 嵌入位置

| 页面 | 组件 | 嵌入方式 |
|------|------|---------|
| ModelDialog (Global) | ModelDialog model_info 编辑区 | 作为独立 section，标题 "路由配置 (Router Settings)"，保存到 `model_info.router_settings`（不对 — Router Global 设置不在 model_info 里） |
| 独立页面 `/dash/router-settings` | 独立页面 | 对标 litellm `router_settings/index.tsx`，读/写 `GET/PUT /router/settings` |
| CreateKey | CreateKeyButton 折叠区 | 在 existing routerSettings state 中读取，保存时 PUT 到 Key |
| TeamInfo | TeamInfo 编辑区 | 在 existing routerSettingsRef 中读取，保存时 PUT 到 Team |

**修正**: Global router_settings **不在 model_info 里**——它存在 `config` 表。所以 Global 设置是一个独立页面 `/dash/router-settings`，不嵌入 ModelDialog。

### 3.4 独立页面 `/dash/router-settings`

```
Route: /dash/router-settings
Component: src/pages/router-settings/index.tsx
```

- `useQuery` GET /router/settings → 表单默认值
- `useMutation` PUT /router/settings → 保存后 toast "路由设置已更新（热生效）"
- 复用 `RouterSettingsAccordion` 组件
- Sidebar 路由列表新增 "Router Settings" 项（ACCESS CONTROL 组下）

### 3.5 侧边栏新增路由

```tsx
// sidebar.tsx — ACCESS CONTROL group
{ label: "Router Settings", path: "/dash/router-settings", icon: Shuffle }
```

---

## 单元测试（4）

| # | 场景 | 验证点 |
|---|------|--------|
| UT-1 | merge: Key 覆盖 Team 覆盖 Global | 三层合并后取最高优先级值 |
| UT-2 | merge: 空覆盖退化 | Key=None Team=None → 返回 Global 原值 |
| UT-3 | merge: 非法值容错 | `cooldown_time: -1` / `num_retries: "abc"` → 用 Global 默认值 |
| UT-4 | update_settings: 热更新 | PUT 后 Router 读出新 config |

---

## BDD 新增（5 × 3 viewports）

| # | 场景 | 验证点 |
|---|------|--------|
| 1 | GET/PUT /router/settings CRUD | 读空 → 写 → 读返回写入值 |
| 2 | 错误 key 名被拒绝 | PUT 含 `"invalid_key": 1` → 400 + 提示合法 key 列表 |
| 3 | 前端 RouterSettings 页面表单交互 | 修改 → 保存 → toast → 重开值保持 |
| 4 | Key 级设置优先于 Global | Global allowed_fails=3, Key allowed_fails=1 → 请求按 1 处理 |
| 5 | 无 Key/Team 覆盖时 Global 默认 | 空 router_settings → 按 Global 配置运行 |

---

## 门禁

- [ ] `cargo test` 全量通过（含新增 4 UT）
- [ ] BDD: 97 → 102 scenarios 全部通过
- [ ] 前端 BDD: 114 tests 全部通过
- [ ] 手动验证：修改 `/dash/router-settings` → PUT 成功 → 热生效
