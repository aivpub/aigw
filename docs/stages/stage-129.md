# Stage 129: Claude OAuth 凭证 — 前端（Phase 51）

**所属**: Phase 51（Claude OAuth 订阅反代）
**预估**: 8h（CredentialsTab OAuth 入口 + 状态/刷新/Re-auth + i18n + BDD）
**依赖**: Stage 126-128（exchange 端点 + TokenProvider 状态）
**状态**: ⏳ 待开始

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

- `POST /credential/oauth/exchange`（body `{session_key, proxy_id?, inject_prompt?, name}`）
- `POST /credential/refresh`（OAuth 凭证手动刷新——若 Stage 127 未提供独立端点,先做触发式:调反代 401 路径或直接 exchange;优先在 Stage 127 补 `POST /credential/oauth/refresh`）
- `GET /admin/proxies/all`（代理下拉）

> 注：若 Stage 127 未实现独立 refresh 端点，本 Stage 需补 `POST /credential/oauth/refresh {credential_id}`（admin），服务端强制刷新一次并返回新状态。列入 Stage 127 或 129 交付均可,实施时收敛。

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

- [ ] CredentialsTab OAuth 入口（交换 + 状态 + Refresh/Re-auth + 代理绑定 + inject_prompt）可用
- [ ] i18n 中英双语完整（`scripts/fe-i18n-types` 通过）
- [ ] 前端 BDD claude_oauth.feature × 3 viewports 全绿;全量 fe-bdd 回归无退化
- [ ] fe-build + fe-lint green
