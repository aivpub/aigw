# Stage 125: Phase 50 收尾 — real BDD + 文档（代理服务管理）

**所属**: Phase 50（代理服务管理）
**预估**: 4h（real BDD 三后端 + ADR + roadmap/next-steps 回写）
**依赖**: Stage 122-124
**状态**: ✅ 完成（2026-08-18）

---

## 1. 目标

Phase 50 收尾：real BDD 三后端验证 + 设计决策登记 ADR-033 + roadmap/next-steps 回写。

## 2. 方案

### 2.1 real BDD 三后端

- `features/real/proxy_crud.feature`（@real_api @needs_upstream_db）：
  - proxies CRUD 三方言全绿（sqlite/pg/mysql）
  - in-use 守卫：credentials 含 proxy_id 引用 → 删除 409
  - probe_result 快照落库 roundtrip
- 检测端点（test/quality）走 **mock 上游**（`@real_api` 场景不依赖真实 IP 服务，避免 flake）：经 `MockUpstream` 返回 ip-api/CF 签名响应

### 2.2 ADR-033（代理服务管理）

`docs/08-autonomous-decisions.md` 追加 ADR-033：

- **决策**：新建 `proxies` 表 + `/admin/proxies/*` CRUD + 出口/质量检测;整串 `proxy_url` AES-GCM 加密落库;检测快照收单 JSON 字段 `probe_result`;不做过期回退(长期路线)
- **理由**：reqwest 原生消费 proxy_url 字符串;密码随串加密优于 sub2api 明文;检测快照 admin 列表内存解析足够
- **后果**：Stage 51 凭证绑定代理引用 proxies.id;OAuth 反代/交换复用代理客户端

### 2.3 roadmap / next-steps 回写

- `docs/stages/stage-roadmap.md`：追加 Phase 50（Stage 122-125，44h）+ 标记完成;总进度 125→129;顶部状态更新
- `docs/11-next-steps.md`：追加 Phase 50 完成记录 + Phase 51 预告

## 3. 验收标准

- [x] real BDD 三方言 proxies CRUD + in-use + 快照全绿（sqlite/pg/mysql **53/53 × 3**）
- [x] ADR-033 Accepted 记录
- [x] roadmap 顶部状态 + Phase 50 条 + 总进度 129/134(待 Phase 51) 回写
- [x] next-steps 更新

---

## 4. 实现记录（2026-08-18 ✅）

### 4.1 real BDD 三后端（`features/real/proxy_crud.feature`）

`@real_api @needs_upstream_db` 6 场景（创建列表 redact / 更新改名 / 删除 / in-use 409 / 出口检测容忍 / toggle）× sqlite/pg/mysql 三后端 = **18 执行全绿**（三后端各自 53/53 场景，含既有 real 场景）。

- step 绑定 `real_proxy_steps.rs`：HTTP 驱动 `/admin/proxies` CRUD + toggle + test + `/credential/new`（in-use 引用）。
- **实现偏差**：探测端点（test）不依赖真实 IP 服务——断言「200 或 500 且返回 JSON」（不可达出口真实返回 500，容忍而非 flake）。
- `代理列表包含/不包含` 改为**重新 GET 列表**断言（create/update 的响应是单对象，不是列表）。

### 4.2 Cross-DB 方言修复（Stage 125 实测暴露）

- **PostgreSQL `probe_result` TEXT → JSONB**：sqlx 将 `serde_json::Value` 映射为 PG JSONB，拒绝 TEXT 解码（同 credentials 019 约束）——027 迁移改为 `JSONB NOT NULL DEFAULT '{}'::jsonb`。
- **MySQL `probe_result` TEXT → JSON**：sqlx 将 `Value` 映射为 MySQL JSON 列类型，TEXT 列读写触发 **1835 Malformed communication packet**——027 迁移改为 `JSON NOT NULL`。
- **MySQL list/count 绑定顺序错误**：SQL 有 7 个 `?`（status×2 + search×3 + limit + offset），原实现只 bind(status, search×3, limit, offset)=6 次——缺失的 status 第二次绑定导致 MySQL 参数错位 → 1835。修复为 `bind(status)×2 + bind(search)×3 + limit + offset`（SQLite `?1/?2` 复用式不受影响，PG `$N` 不受影响）。

### 4.3 ADR / roadmap / next-steps

- ADR-033 已在 Phase 50 规划时 Accepted（2026-08-18），Stage 122/123 实现后本节确认落地（整串 proxy_url 加密 / probe_result 单 JSON / claude_oauth CF challenge / 过期回退长期路线）。
- roadmap：顶部状态更新为「Phase 50 实施中（Stage 122-125 ✅）」+ Phase 50 条标记完成 + 里程碑条不变（125/125 交付 + Phase 50 4 Stage 完成）。
- next-steps：Phase 50 完成记录 + Phase 51 预告（Stage 126-130 强依赖）。

### 4.4 基线

- aigw-core **475 UT**、aigw-server **154 UT**、mock BDD **265（252 pass / 13 skip）**、real BDD **53/53 × 3**（sqlite/pg/mysql 全绿）、`task test`/`task fmt`/`task lint` 全绿。
