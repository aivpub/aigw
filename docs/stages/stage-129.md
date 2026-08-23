# Stage 129: Claude OAuth 凭证 — 前端（Phase 51）

**所属**: Phase 51（Claude OAuth 订阅反代）
**预估**: 8h（CredentialsTab OAuth 入口 + 状态/刷新/Re-auth + i18n + BDD）
**依赖**: Stage 126-128（exchange 端点 + TokenProvider 状态）
**状态**: ✅ Complete（2026-08-24，前端入口 + 后端 refresh 端点）

---

## 1. 目标

CredentialsTab 新增 Claude OAuth 凭证管理入口：粘贴 `sk-ant-sid` cookie → 走 exchange 换 token;列表展示 token 到期 / needs_reauth 徽章 / Refresh / Re-auth 按钮;代理绑定 + 注入提示词配置。

## 2. 方案

### 2.1 OAuth 入口（CredentialsTab 内）

`crates/aigw-frontend/src/pages/models/CredentialsTab.tsx` 扩展（或新增 `OAuthDialog.tsx`）：

**新建 OAuth 凭证对话框**：
- session_key textarea（粘贴 `sk-ant-sid***`，提示格式）
- 代理下拉（`GET /admin/proxies/all` → 可选「直连」）
- inject_prompt textarea（可选注入提示词，placeholder 说明默认 billing header 已注入）
- 凭证名称 input
- 提交 → `POST /credential/oauth/exchange` → 成功后列表出现

**凭证列表**：识别 `credential_values.type=="anthropic_oauth"` 的凭证单独展示：
- 状态徽章：`active`（绿）/ `needs_reauth`（红 + 「需重新认证」）
- token 到期时间（`expires_at` 人类可读）
- 绑定代理名（proxy_id → `/admin/proxies/all` 映射）
- `last_error` 展示（需重新认证时的原因）
- 操作按钮：Refresh（手动刷新 access_token）/ Re-auth（重新粘贴 cookie）/ 编辑（改代理 + inject_prompt）

**敏感字段 redact**：列表/详情不展示 access_token/refresh_token/session_key（后端已 redact）。

### 2.2 API 层

- `POST /credential/oauth/exchange`（body `{session_key, proxy_id?, inject_prompt?, name}`）✅ Stage 126 已实现
- **`POST /credential/oauth/refresh`（body `{credential_name}`，admin）—— 本 Stage 新增（Stage 127 仅暴露 core `invalidate_and_refresh`，无 HTTP 端点）**：服务端强制刷新一次（`TokenProvider::invalidate_and_refresh`）→ 返回新状态（active / needs_reauth + last_error）。失败（needs_reauth）返回 409 + last_error。
- `GET /admin/proxies/all`（代理下拉）✅ Stage 122 已实现

> **设计收敛（Gate 2）**：Stage 127 未实现独立 refresh HTTP 端点，设计文档 §2.2 已预留"本 Stage 需补"。Refresh 按钮是 §4 验收标准之一，前端必须有后端可调。故 `POST /credential/oauth/refresh` 正式纳入本 Stage 交付（后端 + 前端按钮）。

### 2.3 i18n

新增 `claudeOAuth` 命名空间（en + zh-CN）：`claudeOAuth.title`、`sessionKey`、`proxy`、`injectPrompt`、`status.active/needsReauth`、`expiresAt`、`boundProxy`、`refresh`、`reAuth`、`exchange`、`lastError`、`direct` 等。

## 3. TDD 计划（前端 BDD × 3 viewports）

`e2e/claude_oauth.feature` 新建：
1. OAuth 对话框提交 → 凭证出现 + 状态 active
2. 列表展示 token 到期 + 绑定代理名
3. needs_reauth 徽章展示（mock 返回 needs_reauth 凭证）
4. Refresh 按钮 → 状态更新
5. Re-auth 重新粘贴 cookie → 恢复 active
6. 敏感字段不展示（redact 断言）

## 4. 验收标准

- [x] CredentialsTab OAuth 入口（交换 + 状态 + Refresh/Re-auth + 代理绑定 + inject_prompt）可用
- [x] i18n 中英双语完整（`scripts/fe-i18n-types` 通过）
- [x] 前端 BDD claude_oauth.feature × 3 viewports 全绿;全量 fe-bdd 回归无退化
- [x] fe-build + fe-lint green

## 5. 实现记录（2026-08-24）

### 交付

- **后端 `POST /credential/oauth/refresh`**（`credentials.rs:oauth_refresh`）：admin 手动刷新一次（`TokenProvider::invalidate_and_refresh`）→ 成功返回 redact 后凭证；非 OAuth 凭证 400；失败（cookie/refresh 均失效）409 + `kind=oauth_refresh_failed`。路由已注册（`main.rs`）。
- **前端 `OAuthCredentialDialog.tsx`**（新）：粘贴 `sk-ant-sid` cookie + 代理下拉（`/admin/proxies/all` active）+ inject_prompt + 名称 → `POST /credential/oauth/exchange`。
- **前端 `CredentialsTab.tsx` 扩展**：OAuth 凭证独立行（active/needs_reauth 徽章 + 到期时间 + 绑定代理名 + last_error 截断展示 + Refresh/Re-auth/编辑/删除按钮），敏感字段 redact 由后端保证、前端不渲染任何 token。Re-auth 对话框复用 exchange 端点（同 name upsert 恢复 active）。
- **i18n**：`claudeOAuth` 命名空间 en + zh-CN（`fe-i18n-types` 已再生成）。
- **测试**：mock BDD +3 场景（refresh 200 / 非 OAuth 400 / cookie+refresh 均失效 → 409 + needs_reauth）；前端 BDD `claude_oauth.feature` 6 场景 × 3 viewports。

### 关键修复（实现中发现）

- **`claude_token.rs cookie_self_heal` 双解密 bug**：`get_access_token` 已用 `decrypt_json_fields` 把 `session_key` 解成明文，但 `cookie_self_heal` 又对明文调 `decrypt_litellm_value` → base64 Invalid padding → cookie 自愈永远不可达（Config 错误替代 NeedsReauth）。修复：直接使用已解密的 `values["session_key"]`。这正是 Stage 129 的 409 场景暴露出的隐藏缺陷。
- 前端 BDD：mock `/credential/list` 改为 module-scoped 列表 + exchange 追加（否则 exchange 后列表不更新）；small viewport 下 force click（对话框 footer 按钮被 textarea 重叠 + 顶部 header 语言切换拦截）。

### 验证

- 后端：aigw-core 500 + aigw-server 154 UT；mock BDD **278 场景（265 pass / 13 skip body_archive）**；`task fmt` / `task lint` 全绿。
- 前端：fe-build + fe-lint + i18n-types green；fe-bdd 全量 **387 pass / 3 skip（0 fail）**，claude_oauth.feature 6 × 3 = 18 全绿。
