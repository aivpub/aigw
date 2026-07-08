# Stage 26: 登录安全对齐 Litellm

**创建日期**: 2026-07-08
**状态**: ✅ 完成
**优先级**: P0
**前置条件**: Stage 25（前端 BDD 基础设施）
**预估**: 4-6h

---

## 1. 目标

将前端登录从"裸 master key 存 localStorage"改为 litellm 兼容的"用户名+密码 → JWT → HttpOnly Cookie"方案，解决安全风险。

---

## 2. 设计决策

### 2.1 当前问题

| 问题 | 现状 | 风险 |
|------|------|------|
| 存储方式 | `localStorage.setItem("aigw_master_key", raw_key)` | XSS 可读，不设过期 |
| 认证字段 | 仅一个 `sk-` 输入框 | 无法支持多用户 |
| 与 litellm 兼容 | 不一致 | 迁移成本高 |

### 2.2 Litellm vs Aigw 对比

```
litellm 流程：
  /v2/login → 创建临时 sk-xxx（team_id="litellm-dashboard", 24h过期）
           → JWT({ user_id, key: sk-xxx, user_role, ... }, HS256)
           → Set-Cookie: token=<jwt>   ← 无 HttpOnly，JS 可读
           → 响应体也返 token（CDN/反向代理兼容）
  前端：document.cookie → JWT decode → 取 sk-xxx → Authorization: Bearer sk-xxx
  问题：XSS 能读到 cookie 中的 JWT，base64url decode 直接看到 sk

aigw 方案：
  /v2/login → 创建临时 sk-xxx（team_id="litellm-dashboard", 24h过期）
           → JWT({ user_id, key: sk-xxx, user_role, ... }, HS256)
           → Set-Cookie: token=<jwt>; HttpOnly; Secure; SameSite=Lax  ← 核心差异
  前端：fetch(..., { credentials: "include" }) → 浏览器自动带 cookie
  服务端：Auth 中间件从 cookie 读 JWT → 验签 → 解出 sk-xxx → 鉴权
  前端 JS 永远不知道 cookie 内容，XSS 偷不到
```

**核心差异只有一点：cookie 设 `HttpOnly`，sk 的提取从"前端 JS decode JWT"变为"服务端中间件读 cookie"。**

机制完全保留 litellm 的设计：临时 Key + `team_id="litellm-dashboard"` + DB `expires` + 后台清理过期 Key。不需要 session 表，不需要查 DB（JWT 本地验签）。

### 2.3 为什么不是 Session ID 方案

| | Session ID（HashMap/DB） | JWT |
|------|--------------------------|-----|
| 多实例部署 | 需要共享存储（Redis/DB） | 无状态，天然支持 |
| 前后端分离 | 需要跨实例查 session 表 | 无额外依赖 |
| 每请求开销 | 查 session store | 本地验签（无 I/O） |
| 登出 | 删 session 记录 | 删 cookie + 删临时 Key |
| 复杂度 | 需要维护 session 生命周期 | 复用 JWT 标准 |

aigw 单 binary 部署时两种方案没区别，但要为未来前后端分离留空间，JWT 的无状态特性更合适。

---

## 3. 详细设计

### 3.1 POST /v2/login

```
请求: { "username": "admin", "password": "sk-master-change-me" }

认证:
  ├─ username 匹配 UI_USERNAME 环境变量（默认 "admin"）
  │  或 数据库 users 表 user_email 字段
  ├─ password:
  │   管理员路径：匹配 UI_PASSWORD 环境变量或 master_key
  │   数据库用户路径：scrypt hash 验证（格式 "scrypt:base64(salt+dk)"，与 litellm 兼容）
  └─ 失败 → 401 { "error": { "message": "Invalid credentials", "type": "auth_error" } }

认证成功:
  ├─ 创建临时 Key：INSERT INTO verification_tokens
  │   (token, user_id, team_id, expires, user_role, ...)
  │   VALUES (sk-xxx, user_id, "litellm-dashboard", now+24h, role, ...)
  ├─ JWT payload:
  │   {
  │     "user_id": "default_user_id",
  │     "key": "sk-xxx",
  │     "user_email": null,
  │     "user_role": "proxy_admin",
  │     "login_method": "username_password"
  │   }
  ├─ HS256 签名，key = master_key
  ├─ Set-Cookie: token=<jwt>; HttpOnly; Secure; SameSite=Lax; Path=/
  └─ 响应 200: { "user_id": "...", "user_role": "...", "user_email": null }
       ↑ 不再返回 token 字段（JS 读不到 cookie，返回也没用）
```

### 3.2 Auth 中间件改造

```rust
// 现有逻辑（不变）
async fn authenticate(req: &Request) -> Result<AuthContext, AuthError> {
    // 路径 1: Authorization header → sk 鉴权（CLI、API、SDK）
    if let Some(sk) = extract_bearer_token(req) {
        return verify_and_lookup_key(sk).await;
    }
    // 路径 2: HttpOnly cookie → JWT → 解出 sk → 鉴权（前端页面）
    if let Some(jwt) = extract_cookie(req, "token") {
        let claims = jwt::decode(&jwt, &master_key)?;
        return Ok(AuthContext {
            key: claims.key,          // 从 JWT 解出的 sk-xxx
            user_id: claims.user_id,
            user_role: claims.user_role,
            // ... 后续鉴权路径与路径 1 完全一致
        });
    }
    Err(AuthError::Unauthorized)
}
```

**关键点**：路径 2 从 JWT 解出 `sk-xxx` 之后，走和路径 1 完全一样的 key 验证逻辑（查 `verification_tokens` 表，检查 `expires`、`blocked`、`max_budget` 等）。临时 Key 过期后，JWT 虽然签名仍有效，但 key 在 DB 中已不合法，鉴权自然失败。

### 3.3 过期机制

```
两层保障：
  1. 临时 Key DB 记录: expires = now + LITELLM_UI_SESSION_DURATION（默认 24h）
     → 过期后 key 鉴权失败，JWT 即使签名有效也无用
  2. 后台清理: 定期 DELETE FROM verification_tokens
     WHERE team_id = 'litellm-dashboard' AND expires < now()
     → 清理过期记录，定时任务，间隔可配（默认 24h）

JWT 本身不含 exp 声明（与 litellm 一致），过期完全由 DB 层控制。
好处：延长/缩短 session 可以通过修改 DB 记录实现，不需要重新签发 JWT。
```

### 3.4 前端改造

```
登录页：username + password 双字段（替 换当前单 sk 输入框）

before:
  const token = key.trim();
  fetch("/key/list", { headers: { Authorization: `Bearer ${token}` } })

after:
  const { username, password } = form;
  fetch("/v2/login", { method: "POST", body: JSON.stringify({ username, password }) })
  → 服务端 Set-Cookie（HttpOnly，前端不可见）
  → window.location.href = redirect  // 或 navigate(redirect)

所有 API 调用:
  before: fetch(url, { headers: { Authorization: `Bearer ${localStorage.getItem("key")}` } })
  after:  fetch(url, { credentials: "include" })
          ↑ 只需要这一个变化，不需要手动设 Authorization header
          ↑ 浏览器自动带 HttpOnly cookie，中间件从 cookie 读 JWT 取 sk
```

**useAuth 简化**：
```typescript
// 不再需要存储 accessToken
// 只需要 isAuthenticated 状态（通过 GET /v2/login/check 或读 cookie 存在性判断）
export function AuthProvider({ children }: { children: ReactNode }) {
  const [isAuthenticated, setIsAuthenticated] = useState<boolean | null>(null); // null = loading

  useEffect(() => {
    // 检查 cookie 中是否有有效 session
    fetch("/v2/login/check", { credentials: "include" })
      .then(res => setIsAuthenticated(res.ok))
      .catch(() => setIsAuthenticated(false));
  }, []);

  const login = useCallback(() => setIsAuthenticated(true), []);
  const logout = useCallback(async () => {
    await fetch("/v2/logout", { method: "POST", credentials: "include" });
    setIsAuthenticated(false);
  }, []);

  return (
    <AuthContext.Provider value={{ isAuthenticated, login, logout }}>
      {isAuthenticated === null ? null : children}
    </AuthContext.Provider>
  );
}
```

### 3.5 Key 列表的可见性

临时 Key 标记 `team_id = "litellm-dashboard"`，服务端 `/key/list` 无条件过滤掉：

```rust
// key_list handler
let where_clause = format!(
    "WHERE (team_id IS NULL OR team_id != '{}')",
    UI_SESSION_TOKEN_TEAM_ID  // "litellm-dashboard"
);
```

与 litellm 行为一致：用户在 Key 管理页面看不到 session Key。

---

## 4. 交付

### 4.1 后端新增/修改

```
crates/aigw-core/src/
  auth.rs                          # [NEW] JWT encode/decode (jsonwebtoken crate)
  password.rs                      # [NEW] scrypt hash/verify (scrypt crate)

crates/aigw-server/src/
  routes/login.rs                  # [NEW] POST /v2/login, POST /v2/logout, GET /v2/login/check
  routes/keys.rs                   # [MODIFY] key_list 过滤 team_id="litellm-dashboard"
  middleware/auth.rs               # [MODIFY] cookie JWT 分支
  main.rs                          # [MODIFY] 注册 /v2/login 路由
```

### 4.2 前端修改

```
crates/aigw-frontend/src/
  hooks/use-auth.tsx               # [REWRITE] HttpOnly cookie 模式，不再存 sk
  pages/login.tsx                  # [REWRITE] username + password 双字段
  lib/api.ts                       # [MODIFY] 请求加 credentials: "include"
```

### 4.3 新增路由

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/v2/login` | 用户名密码登录，返回 JWT（HttpOnly cookie） |
| POST | `/v2/logout` | 清除 cookie + 删除临时 Key |
| GET | `/v2/login/check` | 检查当前 cookie 是否有效（返回 200 或 401） |

### 4.4 环境变量/配置

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `UI_USERNAME` | `"admin"` | 管理员用户名 |
| `UI_PASSWORD` | master_key 的值 | 管理员密码（不设则等于 master_key） |
| `LITELLM_UI_SESSION_DURATION` | `"24h"` | Session 临时 Key 有效时长 |

### 4.5 依赖（Cargo.toml 新增）

```toml
jsonwebtoken = "9"     # JWT encode/decode (HS256)
scrypt = "0.11"        # password hashing
```

---

## 5. 门禁

- [ ] `POST /v2/login` 管理员登录成功 → Set-Cookie `token`（HttpOnly）+ 200 响应
- [ ] `POST /v2/login` 错误密码 → 401，不设 cookie
- [ ] 数据库用户 `user_email` + `password`（scrypt）登录成功
- [ ] 登录后访问 `/key/list`（`credentials: "include"`）→ auth 中间件从 cookie JWT 解 sk 鉴权成功
- [ ] 临时 Key 在 DB 中 `team_id = "litellm-dashboard"`，不在 `/key/list` 中显示
- [ ] 临时 Key 过期后 API 请求返回 401
- [ ] `POST /v2/logout` → 清除 cookie + 删除临时 Key
- [ ] `GET /v2/login/check` 有效 cookie → 200，无效 → 401
- [ ] cookie 设置 `HttpOnly; Secure; SameSite=Lax`
- [ ] [R-G-R] BDD login.feature 场景全部通过（含 JWT + HttpOnly cookie 流程）
- [ ] 前端不再有 localStorage 中的 raw key
- [ ] 与 litellm `/v2/login` 协议兼容（字段和流程一致，仅 cookie 属性不同）
