# Stage 72: 安全与质量加固

**Phase**: 28 — Security & Quality Hardening
**状态**: ⏳ 待开始
**预估**: 16h
**依赖**: 无

---

## 目标

基于代码审计发现的一揽子安全/质量问题，一次性修复：

| # | 问题 | 当前状态 | 目标 |
|---|------|---------|------|
| 1 | `OptionalClientIp` 无 fallback | 只有 `X-Forwarded-For`，无 header 时 IP 为空 | 三层 fallback：X-Forwarded-For → X-Real-IP → ConnectInfo |
| 2 | `requester_ip_address` 不序列化 | DB SELECT 了但 JSON 响应无此字段 | `spend.rs` 两处 handler 追加字段 |
| 3 | `/router/settings` 无鉴权 | 4 个 handler 均无 auth extractor | GET 加 `SpendAuth`；PUT/PATCH 加 `SpendAuth` + `require_admin` |
| 4 | 前端 401 不跳转登录 | 显示 toast 不清除 auth 状态 | 全局监听 `auth:unauthenticated` → 自动重定向 `/dash/login` |

三个子任务无硬依赖，可并行开发。

---

## Part A — Client IP Fallback + requester_ip_address 序列化 (6h)

### 当前状态

**`crates/aigw-server/src/routes/ip_extractor.rs`** (27 行):
```rust
pub struct OptionalClientIp(pub Option<RightmostXForwardedFor>);

impl<S> FromRequestParts<S> for OptionalClientIp
where
    S: Send + Sync,
{
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match RightmostXForwardedFor::from_request_parts(parts, state).await {
            Ok(ip) => Ok(OptionalClientIp(Some(ip))),
            Err(_) => Ok(OptionalClientIp(None)),  // ← 直接放弃，无 fallback
        }
    }
}
```

**问题**:
- 无 `X-Forwarded-For` 时直接返回 `None`
- 无 `X-Real-IP` header fallback
- 无 TCP peer address (`ConnectInfo<SocketAddr>`) fallback
- 零 UT 覆盖
- `spend.rs` 的 `spend_logs`（第 288 行）和 `global_spend_logs`（第 520 行）JSON 序列化不包含 `requester_ip_address` 字段

### 修改文件

**1. `crates/aigw-server/src/routes/ip_extractor.rs`**

增加三层 fallback + UT 模块：

```rust
use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum_client_ip::RightmostXForwardedFor;
use std::net::SocketAddr;

pub struct OptionalClientIp(pub Option<RightmostXForwardedFor>);

impl<S> FromRequestParts<S> for OptionalClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Layer 1: X-Forwarded-For (已有)
        if let Ok(ip) = RightmostXForwardedFor::from_request_parts(parts, state).await {
            return Ok(OptionalClientIp(Some(ip)));
        }

        // Layer 2: X-Real-IP header (nginx 惯例)
        if let Some(real_ip) = parts.headers.get("x-real-ip")
            .and_then(|v| v.to_str().ok())
        {
            if let Ok(addr) = real_ip.parse::<std::net::IpAddr>() {
                let sock = SocketAddr::new(addr, 0);
                return Ok(OptionalClientIp(Some(RightmostXForwardedFor(sock))));
            }
        }

        // Layer 3: TCP peer address (ConnectInfo)
        if let Ok(ConnectInfo(addr)) = ConnectInfo::<SocketAddr>::from_request_parts(parts, state).await {
            return Ok(OptionalClientIp(Some(RightmostXForwardedFor(addr))));
        }

        Ok(OptionalClientIp(None))
    }
}

#[cfg(test)]
mod tests {
    // 8 个 UT 用例:
    // 1. X-Forwarded-For 单个 IP → 提取成功
    // 2. X-Forwarded-For 多个 IP → 提取最右端 IP
    // 3. X-Forwarded-For 缺失 + X-Real-IP 存在 → 提取 X-Real-IP
    // 4. X-Forwarded-For 格式错误 → fallback 到 X-Real-IP/ConnectInfo
    // 5. 全部缺失 → None
    // 6. IPv6 地址支持
    // 7. X-Forwarded-For 空字符串 → fallback
    // 8. X-Real-IP 格式错误 + 无 ConnectInfo → None
}
```

**2. `crates/aigw-server/src/main.rs`**（第 399-402 行）

```rust
// BEFORE:
axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await?;

// AFTER:
axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
    .with_graceful_shutdown(shutdown_signal())
    .await?;
```

**3. `crates/aigw-server/src/routes/spend.rs`**

两处 JSON 序列化追加 `requester_ip_address`:
- `spend_logs` handler（约第 320 行，`"mcp_namespaced_tool_name"` 之后）
- `global_spend_logs` handler（约第 548 行，同样位置）

```json
"requester_ip_address": log.requester_ip_address,
```

### BDD 回归

新增 3 个 scenario 追加到 `tests/features/spend.feature`:

```gherkin
Scenario: spend logs 返回 requester_ip_address
  Given 一条 spend log 包含 requester_ip_address "192.168.1.1"
  When 使用 master-key 发送 GET /global/spend/logs?page_size=1 请求
  Then 响应状态码为 200
  And 第一条日志的 requester_ip_address 为 "192.168.1.1"

Scenario: v1/messages 通过 X-Forwarded-For 捕获客户端 IP
  Given 一个普通 key "ip-xff" 已生成
  When 使用 key "ip-xff" 且 X-Forwarded-For: "10.0.0.1" 发送 POST /v1/messages 请求
  Then 生成的 spend log 中 requester_ip_address 为 "10.0.0.1"

Scenario: v1/messages 无 X-Forwarded-For 时 fallback 到 ConnectInfo
  Given 一个普通 key "ip-no-xff" 已生成
  When 使用 key "ip-no-xff" 不设置 X-Forwarded-For 发送 POST /v1/messages 请求
  Then 生成的 spend log 中 requester_ip_address 不为空
```

新增 step 定义追加到 `tests/bdd_steps/spend_steps.rs`：

```rust
#[given(regex = r#"一条 spend log 包含 requester_ip_address "(.+)""#)]
async fn given_spend_log_with_ip(world: &mut TestWorld, ip: String) { ... }

#[then(regex = r#"第一条日志的 requester_ip_address 为 "(.+)""#)]
async fn then_first_log_ip_is(world: &mut TestWorld, expected: String) { ... }

#[then(expr = "生成的 spend log 中 requester_ip_address 不为空")]
async fn then_spend_log_ip_not_empty(world: &mut TestWorld) { ... }

#[then(regex = r#"生成的 spend log 中 requester_ip_address 为 "(.+)""#)]
async fn then_spend_log_ip_is(world: &mut TestWorld, expected: String) { ... }
```

### UT

**8 个 `ip_extractor` UT**（如上 [cfg(test)] 模块），run via `cargo test -p aigw-server -- ip_extractor`。

---

## Part B — /router/settings 端点鉴权加固 (4h)

### 当前状态

**`crates/aigw-server/src/routes/router_settings.rs`** 中 4 个 handler 均无 auth：

```rust
pub async fn get_global(State(state): State<SharedState>) -> ...        // ← 无 auth
pub async fn put_global(State(state): State<SharedState>, ...) -> ...   // ← 无 auth
pub async fn patch_key(State(state): State<SharedState>, ...) -> ...    // ← 无 auth
pub async fn patch_team(State(state): State<SharedState>, ...) -> ...   // ← 无 auth
```

对比所有其他 admin 端点（`keys.rs`, `models.rs`, `credentials.rs`, `org.rs`, `team.rs`, `user.rs`），全部使用：

```rust
pub async fn some_handler(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,     // ← auth extractor
    ...
) -> ... {
    require_admin(&auth)?;           // ← admin check
    ...
}
```

### 修改文件

**1. `crates/aigw-server/src/routes/router_settings.rs`**

顶部导入：

```rust
use super::spend::{require_admin, SpendAuth};
```

4 个 handler 签名修改：

| Handler | 改动 |
|---------|------|
| `get_global` | 加 `SpendAuth(_auth): SpendAuth` |
| `put_global` | 加 `SpendAuth(auth): SpendAuth` + `require_admin(&auth)?;` |
| `patch_key` | 加 `SpendAuth(auth): SpendAuth` + `require_admin(&auth)?;` |
| `patch_team` | 加 `SpendAuth(auth): SpendAuth` + `require_admin(&auth)?;` |

**2. UT 模块** — 追加到 `router_settings.rs` `#[cfg(test)]`：

```rust
// 8 个 UT:
// 1. GET /router/settings 无 auth header → 401
// 2. PUT /router/settings 无 auth header → 401
// 3. PATCH /key/{t}/router/settings 无 auth header → 401
// 4. PATCH /team/{id}/router/settings 无 auth header → 401
// 5. GET /router/settings 无效 token → 401
// 6. PUT /router/settings 非 admin key → 403
// 7. GET /router/settings master key → 200
// 8. PUT /router/settings master key → 200
```

### BDD 回归

新增 4 个 scenario 追加到 `tests/features/auth.feature`:

```gherkin
Scenario: 无认证访问路由设置返回 401
  When 不携带 Authorization 发送 GET /router/settings 请求
  Then 响应状态码为 401

Scenario: 普通 key 无法修改路由设置
  Given 一个普通 key "router-regular" 已生成
  When 使用 key "router-regular" 发送 PUT /router/settings 请求
  Then 响应状态码为 403

Scenario: Master key 可以读取路由设置
  When 使用 master-key 发送 GET /router/settings 请求
  Then 响应状态码为 200

Scenario: Master key 可以修改路由设置
  When 使用 master-key 带有效 body 发送 PUT /router/settings 请求
  Then 响应状态码为 200
```

新增/修改文件：

| 文件 | 改动 |
|------|------|
| `tests/bdd_steps/common.rs` | 新增 `build_router_settings_router(state)` 函数 |
| `tests/bdd_steps/router_settings_steps.rs` | **新建** — 4 个 step 定义 |
| `tests/bdd_steps/mod.rs` | 注册 `pub mod router_settings_steps;` |

---

## Part C — 前端 401 全局监听 + 自动跳转 (6h)

### 当前状态

- `handleResponse()` (`lib/api.ts:7-13`) 将所有错误一视同仁
- `AuthProvider` (`use-auth.tsx`) 无机制通知 auth 过期
- `QueryClient` (`main.tsx:8-16`) 无全局 error handler
- `RequireAuth` (`App.tsx:15-28`) 已实现 `isAuthenticated=false → 重定向 /dash/login`，但无人触发

### 设计方案

```
api.ts handleResponse 检测 401
    → dispatchEvent("auth:unauthenticated")
    → throw UnauthorizedError
    ↓
use-auth.tsx 监听 "auth:unauthenticated" 事件
    → setIsAuthenticated(false)
    ↓
App.tsx RequireAuth 渲染
    → <Navigate to="/dash/login?redirect=..." />
```

同时 `main.tsx` 的 `QueryCache.onError` 和 `MutationCache.onError` 作为第二道防线。

### 修改文件

**1. `crates/aigw-frontend/src/lib/api.ts`**

```typescript
export class UnauthorizedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "UnauthorizedError";
  }
}

async function handleResponse(res: Response) {
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    const message = err.error?.message || `API ${res.status}`;
    if (res.status === 401) {
      window.dispatchEvent(new Event("auth:unauthenticated"));
      throw new UnauthorizedError(message);
    }
    throw new Error(message);
  }
  return res.json();
}
```

**2. `crates/aigw-frontend/src/hooks/use-auth.tsx`**

```typescript
// AuthState interface 新增 setUnauthenticated
interface AuthState {
  isAuthenticated: boolean;
  isLoading: boolean;
  login: ...;
  logout: ...;
  setUnauthenticated: () => void;  // ← 新增
}

// AuthProvider 内新增 event listener
useEffect(() => {
  const handler = () => setIsAuthenticated(false);
  window.addEventListener("auth:unauthenticated", handler);
  return () => window.removeEventListener("auth:unauthenticated", handler);
}, []);
```

**3. `crates/aigw-frontend/src/main.tsx`**

```typescript
import { QueryCache } from "@tanstack/react-query";
import { UnauthorizedError } from "@/lib/api";

const queryClient = new QueryClient({
  queryCache: new QueryCache({
    onError: (error) => {
      if (error instanceof UnauthorizedError) {
        window.dispatchEvent(new Event("auth:unauthenticated"));
      }
    },
  }),
  defaultOptions: {
    queries: {
      retry: 1,
      staleTime: 10_000,
      refetchOnWindowFocus: false,
    },
  },
});
```

**4. `crates/aigw-frontend/src/App.tsx`** — 无需修改（`RequireAuth` 已正确处理 `isAuthenticated=false`）

### BDD 回归

新增 3 个 scenario 追加到 `tests/features/login.feature`:

```gherkin
Scenario: API 401 triggers redirect to login
  Given I am authenticated and on the usage page
  When the API returns 401 for spend/logs request
  Then I should be redirected to "/dash/login"
  And the sidebar should not be visible

Scenario: 401 redirect preserves current page path
  Given I am authenticated and on "/dash/keys"
  When the API returns 401 for key/list request
  Then I should be redirected to "/dash/login"
  And the URL should contain "redirect=%2Fdash%2Fkeys"

Scenario: Login after 401 redirect returns to original page
  Given I was redirected to "/dash/login?redirect=%2Fdash%2Fmodels"
  When I type "admin" into the username field
  And I type "sk-master-change-me" into the password field
  And I click the Sign In button
  Then I should be redirected to "/dash/models"
```

新增 step 定义追加到 `tests/steps/login.steps.ts`。

### UT

新增前端 UT（需要配置 vitest，如尚未配置则使用 BDD 替代）：
- `UnauthorizedError` 实例检测
- `handleResponse` 401/403/500 分别抛出正确错误类型
- `handleResponse` 401 时派发 `auth:unauthenticated` 事件

---

## BDD 新增汇总

| 子任务 | Feature 文件 | Step 文件 | 新增 Scenario |
|--------|-------------|-----------|---------------|
| A | `tests/features/spend.feature` (追加) | `tests/bdd_steps/spend_steps.rs` (追加) | 3 |
| B | `tests/features/auth.feature` (追加) | `tests/bdd_steps/router_settings_steps.rs` (新建) + `common.rs` (追加) | 4 |
| C | `tests/features/login.feature` (追加) | `tests/steps/login.steps.ts` (追加) | 3 |
| **合计** | 3 文件修改 | 4 文件修改/新建 | **10** |

---

## 依赖关系

```
Part A (IP fallback)  ←→  Part B (router auth)  ←→  Part C (前端 401)
       (可并行)               (可并行)                (可并行)
```

三个 Part 无硬依赖，修改不同文件，可并行开发。

---

## 验证

```bash
# 后端全量
cargo test -p aigw-server                          # UT（含新增 16 个）
cargo test -p aigw-server --test bdd               # BDD（含新增 7 个 scenario）

# 前端全量
cd crates/aigw-frontend && npx playwright test     # BDD（含新增 3 个 scenario）

# 编译
cargo check
cd crates/aigw-frontend && npm run build
```

## 门禁标准

- 所有新增 UT 通过（ip_extractor 8 个 + router_settings 8 个 = 16 个）
- 所有新增 BDD scenario 通过（7 个后端 + 3 个前端 = 10 个）
- 100% 已有测试回归通过
- `cargo check` 无 warning
- 前端 `npm run build` 成功
