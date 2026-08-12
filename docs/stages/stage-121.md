# Stage 121: 上游模型停用功能接线（Phase 49）

**所属**: Phase 49（模型生命周期完善）
**预估**: 6-10h（schema + 后端 SQL + Resolver + Handler + 前端 + UT/BDD）
**依赖**: 无（独立能力）
**状态**: ⏳ 进行中

---

## 1. 目标

修复用户反馈的"上游模型停用功能完全无效"缺陷（详见 `docs/research/2026-08-13-model-disable-audit.md`）：前端 UI Switch 已提供停用/启用切换，但后端**零处消费** `model_info.mode`，被"停用"的模型仍照常路由到上游。

**引入独立 `enabled: bool` 字段**（方案 B），与业务类别 `model_info.mode`（"embed"/"image"）语义分离，schema 干净、DB 层过滤、避免加载→丢弃的无效 IO。

## 2. 现状证据

| 位置 | 问题 |
|------|------|
| `crates/aigw-core/src/db.rs:4428` | `LIST_MODELS_BY_NAME_SQLITE` 只按 `model_name` 过滤，未看 `model_info.mode` |
| `crates/aigw-core/src/resolver.rs:52-58` | `resolve` 把 `list_models_by_name` 结果全部映射为 `Deployment` |
| `crates/aigw-core/src/deployment.rs:17-71` | `Deployment` 无 `enabled` 字段，仅 `cooldown_until` |
| `crates/aigw-core/src/router.rs:355-357` | `pick_deployment` 只按 cooldown 过滤 |
| `crates/aigw-server/src/routes/models.rs:294-298` | `/model/update` 把 `model_info.mode` merge 存 DB，无人读 |

## 3. 方案

### 3.1 Schema（Migration 026）

`proxy_models` + `deleted_models` 各加一列：

| Backend | 类型 | 默认值 |
|---------|------|--------|
| SQLite | `INTEGER NOT NULL DEFAULT 1` | 1（历史行全部启用） |
| PostgreSQL | `BOOLEAN NOT NULL DEFAULT TRUE` | TRUE |
| MySQL | `TINYINT(1) NOT NULL DEFAULT 1` | 1 |

**为何 `deleted_models` 也加**：`archive_and_delete_model` INSERT INTO deleted_models 需要列对齐（复制所有字段）。

### 3.2 Rust struct

- `ProxyModel.enabled: bool`（`#[serde(default = "default_true")]` 允许旧 JSON payload）
- `UpdateModelRequest.enabled: Option<bool>`
- `ModelResponse.enabled: bool`（透出给前端）
- `DeletedModel.enabled: bool`

### 3.3 SQL（3 端 × ~9 SQL）

- `INSERT INTO proxy_models (..., enabled) VALUES (..., ?)`（默认 TRUE，但显式传方便测试）
- `SELECT model_id, ..., enabled FROM proxy_models WHERE ...`（所有 SELECT 都加 enabled 列）
- `UPDATE proxy_models SET ..., enabled = ? WHERE model_id = ?`
- `LIST_MODELS_BY_NAME` 加 `AND enabled = TRUE`（业务过滤核心点）
- `LIST_MODELS` / `LIST_MODELS_PAGED`：**保留** enabled 行 + disabled 行都返回（admin 视角能看到全部；前端 Switch 显示的正是这个列表）
- `INSERT INTO deleted_models (..., enabled)` + `SELECT ..., enabled FROM deleted_models`

### 3.4 Resolver 层安全防线

`ModelResolver::resolve` **额外做一次 `.filter(|m| m.enabled)`**——即使 SQL 忘了过滤也能兜底。UT 覆盖此路径。

### 3.5 Handler

`models.rs` `/model/update`：读 `body.enabled` 直接赋值到 `model.enabled`；`ModelResponse::from_model` 透出 `enabled`。

### 3.6 前端

- `crates/aigw-frontend/src/pages/models/index.tsx:545` Switch 的 `checked` 改成 `model.enabled ?? true`；`onCheckedChange` 调 `apiPut("/model/update", {model_id, enabled: checked})`（不再写 `model_info.mode`）
- `isActive(info)` → `isActive(model)`（读 `model.enabled`）
- ModelDialog 里的 status 显示同理

**model_info.mode 保留原义**（"embed"/"image" 业务类别），本 Stage 不清理历史 mode 字段中的 "inactive"/"disabled" 值（惰性容忍：`isActive` 返回 `enabled`，历史 mode 不再影响判断）。

## 4. TDD 计划

### 4.1 失败先行 UT（`crates/aigw-core/src/resolver.rs`）

- `test_resolve_skips_disabled_model`：插入 `enabled=false` 的 row，调 `resolve("foo")` 期望 `Err(BAD_REQUEST, model_not_found)`
- `test_resolve_returns_enabled_only`：同 model_name 两 row（一 enabled 一 disabled），期望返回 1 个 deployment

### 4.2 Handler UT（`crates/aigw-server/src/routes/models.rs`）

- `test_model_update_enabled_false_persists`：PUT `/model/update {enabled: false}` 后从 DB 读 model.enabled = false
- `test_model_update_preserves_enabled_when_absent`：不传 enabled 时保留原值

### 4.3 迁移测试（既有 real BDD 覆盖）

`bdd-real-sqlite/pg/mysql` 会跑迁移全链，验证 026 兼容。

## 5. 变更清单

| 文件 | 改动 |
|------|------|
| `crates/aigw-core/migrations/sqlite/026_proxy_models_enabled.sql` | ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1 到 proxy_models + deleted_models |
| `crates/aigw-core/migrations/postgres/026_proxy_models_enabled.sql` | ADD COLUMN enabled BOOLEAN NOT NULL DEFAULT TRUE |
| `crates/aigw-core/migrations/mysql/026_proxy_models_enabled.sql` | ADD COLUMN enabled TINYINT(1) NOT NULL DEFAULT 1 |
| `crates/aigw-core/src/models.rs` | ProxyModel + UpdateModelRequest + DeletedModel 加 enabled |
| `crates/aigw-core/src/db.rs` | 3 端 ~30 处 SQL 修改（SQLite const + PG/MySQL inline） |
| `crates/aigw-core/src/resolver.rs` | 加 filter + 2 个 UT |
| `crates/aigw-server/src/routes/models.rs` | /model/update 支持 enabled；ModelResponse 透出 |
| `crates/aigw-frontend/src/pages/models/index.tsx` | Switch 切换到 enabled；isActive 用 model.enabled |
| `crates/aigw-frontend/src/pages/models/ModelDialog.tsx` | status 显示用 enabled |
| `docs/stages/stage-121.md` | 本文件 |
| `docs/research/2026-08-13-model-disable-audit.md` | 调研文档（前置） |
| `docs/11-next-steps.md` | Phase 49 / Stage 121 回写 |

## 6. 回归验证

1. `task test` 全绿（含 4 个新 UT）
2. `task test-bdd` mock BDD 246 场景保持基线
3. `task bdd-real-sqlite/pg/mysql` 迁移全链绿（上游 402 无关失败豁免）
4. `task fmt` / `task lint` / `task build` 全绿

## 7. 门禁

- [ ] 4 个新 UT 先 fail 后 pass
- [ ] `task test` / `task fmt` / `task lint` / `task build` 全绿
- [ ] `docs/11-next-steps.md` 回写 Phase 49
- [ ] git commit（精确 add，不用 `-A`/`.`）

## 8. 不做的事（明确边界）

- **不清理 `model_info.mode` 历史值**（"inactive"/"disabled" 遗留在旧数据里不管；enabled 是新的唯一权威）
- **不做 Router 层的 enabled 字段透传**（SQL 已过滤，Deployment 中不需要这个字段；除非未来需要"看看被停用了但仍在池子里的"debug 场景）
- **不清理 `deleted_models` 中的 mode 语义**（deleted models 只是归档，不会被路由）
- **不动 `status` enum 状态机**（方案 C，长期路线，非本 Stage）
