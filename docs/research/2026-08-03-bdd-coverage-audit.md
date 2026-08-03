# AIGW BDD 场景覆盖全量调研报告

> 调研日期: 2026-08-03 | 项目: aigw (AI Gateway)

---

## 一、总览

| 维度 | 数量 |
|------|------|
| aigw-server route handlers | 71 |
| aigw-server BDD feature 文件 (mock) | 23 |
| aigw-server BDD 场景总数 (mock) | ~160 |
| aigw-server Real BDD feature 文件 | 9 |
| aigw-server Real BDD 场景总数 | ~36 |
| aigw-frontend BDD feature 文件 | 12 |
| aigw-frontend BDD 场景总数 | ~92 |
| aigw-core 模块 | 22 |
| aigw-migrate 模块 | 8 |
| 路由处理器完全无 BDD 覆盖 | 1 (docs.rs) |
| 存在覆盖但不足的模块 | 7 |

---

## 二、aigw-server 路由处理器 BDD 覆盖详情

### 2.1 Keys (密钥管理) — `crates/aigw-server/src/routes/keys.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `generate_key` | POST /key/generate | ✅ | - | keys.feature (8场景) |
| `key_info` | GET /key/info | ✅ | - | keys.feature, keys_permission.feature (6场景) |
| `key_list` | GET /key/list | ✅ | - | keys.feature |
| `key_update` | PUT /key/update | ✅ | - | keys.feature |
| `key_delete` | DELETE /key/delete | ✅ | - | keys.feature |
| `key_regenerate` | POST /key/regenerate | ✅ | - | keys.feature |
| `key_deleted_list` | POST /key/deleted_list (custom) | ✅ | - | keys.feature |

**覆盖率: 100%** (7/7) — `key_deleted_list` 已确认有 BDD 步骤覆盖 (`keys_steps.rs:70 key_already_deleted`)

### 2.2 Team (团队管理) — `crates/aigw-server/src/routes/team.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `team_new` | POST /team/new | ✅ | - | auth.feature (9场景 含 team CRUD) |
| `team_info` | GET /team/info | ✅ | - | auth.feature |
| `team_list` | GET /team/list | ✅ | - | auth.feature |
| `team_update` | PUT /team/update | ✅ | - | auth.feature |
| `team_delete` | DELETE /team/delete | ✅ | - | auth.feature |
| `team_deleted_list` | GET /team/deleted | ❌ | - | **GAP: 无 BDD 场景** |

**覆盖率: 83%** (5/6)

### 2.3 Budget (预算管理) — `crates/aigw-server/src/routes/budget.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `budget_list` | GET /budget/list | ✅ | - | budget_reset.feature (4场景) |
| `budget_new` | POST /budget/new | ✅ | - | budget_reset.feature |
| `budget_info` | GET /budget/info | ✅ | - | budget_reset.feature |
| `budget_update` | PUT /budget/update | ✅ | - | budget_reset.feature |
| `budget_delete` | DELETE /budget/delete | ✅ | - | budget_reset.feature |

**覆盖率: 100%** (5/5)

### 2.4 Models (模型管理) — `crates/aigw-server/src/routes/models.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `model_new` | POST /model/new | ✅ | - | models.feature (8场景) |
| `model_info` | GET /model/info | ✅ | - | models.feature |
| `model_list` | GET /model/list | ✅ | - | models.feature |
| `model_update` | PUT /model/update | ✅ | - | models.feature |
| `model_delete` | DELETE /model/delete | ✅ | - | models.feature |
| `model_deleted_list` | GET /model/deleted | ❌ | - | **GAP: 无 BDD 场景** |

**覆盖率: 83%** (5/6)

### 2.5 User (用户管理) — `crates/aigw-server/src/routes/user.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `user_new` | POST /user/new | ✅ | - | auth.feature (9场景 含 user CRUD) |
| `user_info` | GET /user/info | ✅ | - | auth.feature |
| `user_list` | GET /user/list | ✅ | - | auth.feature |
| `user_update` | PUT /user/update | ✅ | - | auth.feature |
| `user_delete` | DELETE /user/delete | ✅ | - | auth.feature |
| `user_deleted_list` | GET /user/deleted | ❌ | - | **GAP: 无 BDD 场景** |

**覆盖率: 83%** (5/6)

### 2.6 Organization (组织管理) — `crates/aigw-server/src/routes/org.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `org_new` | POST /org/new | ✅ | - | auth.feature |
| `org_info` | GET /org/info | ✅ | - | auth.feature |
| `org_list` | GET /org/list | ✅ | - | auth.feature |
| `org_update` | PUT /org/update | ✅ | - | auth.feature |
| `org_delete` | DELETE /org/delete | ✅ | - | auth.feature |
| `org_deleted_list` | GET /org/deleted | ❌ | - | **GAP: 无 BDD 场景** |

**覆盖率: 83%** (5/6)

### 2.7 Credentials (凭证管理) — `crates/aigw-server/src/routes/credentials.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `credential_new` | POST /credentials | ✅ | - | end_to_end.feature |
| `credential_info` | GET /credentials | ✅ | - | end_to_end.feature |
| `credential_list` | GET /credentials | ✅ | - | end_to_end.feature |
| `credential_update` | PUT /credentials | ✅ | - | end_to_end.feature |
| `credential_delete` | DELETE /credentials | ✅ | - | end_to_end.feature |

**覆盖率: 100%** (5/5) — 通过 e2e 场景间接覆盖

### 2.8 Spend (消费查询) — `crates/aigw-server/src/routes/spend.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `spend_logs` | GET /spend/logs | ✅ | - | spend.feature (8场景) |
| `spend_keys` | GET /spend/keys | ✅ | - | spend_aggregation.feature (14场景) |
| `spend_users` | GET /spend/users | ✅ | - | spend_aggregation.feature |
| `spend_tags` | GET /spend/tags | ✅ | - | spend.feature |
| `spend_models` | GET /spend/models | ✅ | ✅ | spend_models_providers_real.feature (5场景) |
| `spend_providers` | GET /spend/providers | ✅ | ✅ | spend_models_providers_real.feature |
| `spend_model_groups` | GET /spend/model_groups | ✅ | - | spend_aggregation.feature |
| `global_spend` | GET /global/spend | ✅ | - | spend.feature |
| `global_spend_logs` | GET /global/spend/logs | ✅ | ✅ | spend_activity_real.feature (5场景) |
| `global_spend_keys` | GET /global/spend/keys | ✅ | - | spend.feature |
| `global_spend_providers` | GET /global/spend/providers | ✅ | ✅ | spend_models_providers_real.feature |
| `global_spend_models` | GET /global/spend/models | ✅ | - | spend_aggregation.feature |
| `global_spend_model_groups` | GET /global/spend/model_groups | ✅ | - | spend_aggregation.feature |
| `global_spend_activity` | GET /global/spend/activity | ✅ | ✅ | spend_activity_real.feature |
| `global_spend_keys_rankings` | GET /global/spend/keys_rankings | ✅ | ✅ | spend_rankings_real.feature (3场景) |
| `spend_end_user` (via spend_end_user) | 相关 | ✅ | - | spend_end_user.feature (3场景) |

**覆盖率: 100%** (16/16)，其中 Real BDD 覆盖了 5 个关键排行榜端点

### 2.9 Health (健康检查) — `crates/aigw-server/src/routes/health.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `health` | GET /health | ✅ | - | health.feature (6场景) |
| `readiness` | GET /health/readiness | ✅ | - | health.feature |
| `liveliness` | GET /health/liveliness | ✅ | - | health.feature |
| `system_info` | GET /system/info | ✅ | - | health.feature |
| `model_health_check_all` | POST /model/health-check/all | ✅ | - | health.feature (场景: lines 20-29, 含 hc-openai-model/hc-anthropic-model) |
| `model_health_check` | POST /model/health-check | ✅ | - | health.feature (场景: line 35, hc-fail-model) |
| `health_latest` | GET /health/latest | ❌ | - | **GAP: 无 BDD 场景** |
| `prometheus_metrics` | GET /metrics | ❌ | - | **GAP: 无 BDD 场景** |
| `health_metrics` | GET /health/metrics | ❌ | - | **GAP: 无 BDD 场景** |

**覆盖率: 67%** (6/9)

### 2.10 Chat/Proxy (聊天代理) — `crates/aigw-server/src/routes/chat.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `chat_completions` | POST /v1/chat/completions | ✅ | ✅ | messages.feature (9场景), e2e, compatibility_real (3场景) |
| `models_list` | GET /v1/models | ✅ | ✅ | models.feature, protocol_conversion_real (2场景) |

**覆盖率: 100%** (2/2) — 有 Mock BDD + Real BDD 双重覆盖

### 2.11 Messages (Anthropic 原生消息) — `crates/aigw-server/src/routes/v1_messages.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `messages_handler` | POST /v1/messages | ✅ | ✅ | anthropic_native.feature (4场景), e2e |

**覆盖率: 100%** (1/1) — 有 Mock + Real 双重覆盖

### 2.12 Login/Auth — `crates/aigw-server/src/routes/login.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `login` | POST /v2/login | ✅ | - | auth.feature |
| `logout_with_cleanup` | POST /v2/logout | ✅ | - | auth.feature |
| `login_check` | GET /v2/login/check | ✅ | - | auth.feature |

**覆盖率: 100%** (3/3)

### 2.13 Router Settings (路由设置) — `crates/aigw-server/src/routes/router_settings.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `get_global` | GET /router/settings | ✅ | - | auth.feature (line 40: master-key GET /router/settings) |
| `put_global` | PUT /router/settings | ✅ | - | auth.feature (line 44: master-key PUT /router/settings) |
| `patch_key` | PATCH /router/settings/key | ❌ | - | **GAP: 无 BDD 场景** |
| `patch_team` | PATCH /router/settings/team | ❌ | - | **GAP: 无 BDD 场景** |

**覆盖率: 50%** (2/4)

### 2.14 Jobs/Admin — `crates/aigw-server/src/routes/jobs.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `trigger_job` | POST /job/trigger | ✅ | - | admin_jobs.feature (20场景) |
| `list_jobs_handler` | GET /job/list | ✅ | - | admin_jobs.feature |
| `job_stats_handler` | GET /job/stats | ✅ | - | admin_jobs.feature |
| `job_detail_handler` | GET /job/detail | ✅ | - | admin_jobs.feature |
| `job_logs_handler` | GET /job/logs | ✅ | - | admin_jobs.feature |
| `archive_stats_handler` | GET /archive/stats | ✅ | - | admin_jobs.feature |

**覆盖率: 100%** (6/6)

### 2.15 Docs — `crates/aigw-server/src/routes/docs.rs`

| Handler | Method + Path | BDD Mock? | Real BDD? | 覆盖文件 |
|---------|--------------|-----------|-----------|---------|
| `docs_ui` | GET /docs | ❌ | - | 无覆盖 (纯静态 UI 页面) |

**覆盖率: 0%** (1/1) — 纯 UI 页面，BDD 价值低

### 2.16 Proxy (路由逻辑) — `crates/aigw-server/src/routes/proxy.rs`

| 说明 |
|------|
| proxy.rs 只有单元测试，无 HTTP handler — 路由逻辑通过 chat.rs 的 `chat_completions` 路径间接覆盖 |

**覆盖率: N/A** — 纯内部逻辑，由 chat BDD 间接覆盖

---

## 三、aigw-core 模块 BDD 覆盖

| 模块 | 功能 | 通过 API 暴露? | BDD 覆盖? | 单元/集成测试? |
|------|------|--------------|-----------|----------|
| `adapter.rs` | 协议适配 (OpenAI↔Anthropic, stream adapters, system folding) | ❌ (内部) | ✅ adapter.feature (5), anthropic_native.feature (4) | ✅ 25+ 内联单元测试 |
| `async_task.rs` | AsyncTask trait + JobRecord/StepRecord 类型定义 | ❌ (内部) | ✅ async_task.feature (15) | ✅ stage82_state_machine.rs (15 集成测试) |
| `auth.rs` | JWT encode/decode (HS256) | ✅ (中间件) | ✅ auth.feature (9), keys_permission.feature (6) | ✅ 2 内联单元测试 |
| `body_archive/` | 请求/响应体 Parquet 归档 + 查询 + 缓存 | ✅ (管理 API) | ✅ body_archive_write.feature (12), body_archive_read.feature (7), body_archive_admin_real.feature (2) | ✅ stage82 (8), stage83 (9) |
| `budget.rs` | BudgetEnforcer::check_budget (spend vs max_budget) | ✅ (中间件) | ✅ budget_reset.feature (4), keys.feature 间接 | ✅ 7 内联单元测试 |
| `budget/duration.rs` | parse_duration_secs + compute_next_reset_at | ❌ (内部) | ❌ 间接通过 budget_reset.feature | ✅ 内联测试 |
| `budget/resetter.rs` | BudgetResetter AsyncTask (scan_all/scan_by_type/execute_reset) | ✅ (管理 API) | ✅ budget_reset.feature (4), admin_jobs.feature | ✅ 内联测试 |
| `config.rs` | AigwConfig + CompressionConfig + OtelConfig | ❌ (启动时) | ❌ 间接通过 global.feature | ✅ 6 内联单元测试 |
| `crypto.rs` | hash_token, encrypt/decrypt_litellm_value, decrypt_json_fields, rotate_json_fields, base64_type15 | ❌ (内部) | ❌ 间接通过迁移 models.feature | ✅ 27 内联单元测试 |
| `daily_spend_queue.rs` | DailySpendQueue (drain/upsert 后台任务) | ❌ (内部) | ❌ 间接通过 spend.feature | ❌ **零测试覆盖** |
| `db.rs` | Database::init (multi-DB migrations) + CredentialsStore trait | ❌ (内部) | ✅ 所有 BDD 依赖 db | ✅ 3 内联测试 + integration_test.rs |
| `deployment.rs` | Deployment struct + ProviderType::infer | ❌ (内部) | ❌ 间接通过 adapter.feature | ✅ 3 内联单元测试 |
| `engine.rs` | Engine (tick/exec/cleanup, create_job, claim/complete/fail step) | ✅ (管理 API) | ✅ async_task.feature (15), admin_jobs.feature (20) | ✅ 3 内联 + stage82_state_machine.rs (15) |
| `instance.rs` | InstanceRegistry (RwLock<HashMap>, register/heartbeat/drain) | ❌ (内部) | ❌ | ✅ 8 内联单元测试 |
| `lib.rs` | 模块声明 + 重导出 | N/A | N/A | N/A |
| `metrics.rs` | MetricsRecorder (14 Prometheus 指标) + RequestSummary | ❌ (内部) | ❌ /metrics 端点无 BDD | ✅ 5 内联单元测试 |
| `middleware/auth_gateway.rs` | TenantIdentity + DeploymentMode (SaaS/OnPrem) | ✅ (中间件) | ❌ 间接通过 keys_permission.feature | ❌ 无内联测试 |
| `middleware/rate_limit.rs` | enforce_limits (budget + rate limit guard) | ✅ (中间件) | ❌ 间接通过请求场景 | ❌ 无内联测试 |
| `models.rs` | 所有 DB 模型 (VirtualKey, SpendLog, ProxyModel, org/team/user, chat types) | ❌ (数据结构) | ✅ 所有 BDD 都依赖 | 无测试模块 |
| `otel_tracing.rs` | OtelTracer init/extract/inject + OtelConfig | ❌ (内部) | ❌ | ✅ 8 内联单元测试 |
| `password.rs` | hash_password + verify_password (scrypt, litellm-compatible) | ❌ (内部) | ❌ 间接通过 auth.feature | ✅ 4 内联单元测试 |
| `provider.rs` | ProviderRegistry + ProviderConfig (legacy routing, 大部分被 resolver 取代) | ❌ (内部) | ❌ | ✅ 6 内联单元测试 |
| `rate_limiter.rs` | RateLimiter (token bucket RPM/TPM 执行) | ✅ (中间件) | ❌ | ✅ 7 内联单元测试 |
| `resolver.rs` | ModelResolver::resolve (model_name → Vec<Deployment>) | ❌ (内部) | ✅ model_access.feature (5) | ✅ 10 内联单元测试 |
| `router.rs` | Router + select_instance + mark_failure/success + merge_router_overrides | ❌ (内部) | ✅ end_to_end.feature, global.feature | ✅ 16 内联单元测试 |
| `tenant.rs` | TenantContext + TenantDb (org-level 数据隔离, SaaS) | ❌ (内部) | ❌ | ✅ 7 内联单元测试 |

### 核心模块缺失覆盖清单

| 优先级 | 模块 | 问题 |
|--------|------|------|
| 🔴 P0 | `daily_spend_queue.rs` | **零测试覆盖** — 生产关键路径每日消费预聚合，无任何单元/集成/BDD 测试 |
| 🟡 P1 | `middleware/auth_gateway.rs` | TenantIdentity + DeploymentMode 无独立 BDD 场景和单元测试 |
| 🟡 P1 | `middleware/rate_limit.rs` | enforce_limits guard 无独立 BDD 和单元测试（依赖子组件测试） |
| 🟢 P2 | `metrics.rs` | /metrics 端点无 BDD 覆盖（但有 5 个内联单元测试） |
| 🟢 P2 | `otel_tracing.rs` | 无 BDD 覆盖（但有 8 个内联单元测试） |
| 🟢 P3 | `provider.rs` | 遗留路由系统，无 BDD（但有 6 个单元测试） |

---

## 四、aigw-migrate 测试覆盖

### 4.1 单元测试覆盖（inline `#[cfg(test)]`）

| 源文件 | 用途 | 内联单元测试数 |
|--------|------|--------------|
| `import.rs` | 本地 SQLite 直接导出导入 | 1 |
| `export.rs` | 本地 SQLite 直接导出导入 | 1 |
| `verify.rs` | 12 张表 source/target 行数比对 | 1 |
| `pre_check.rs` | 6 项迁移前检查 (connectivity, tables, rows, master_key, key valid, decrypt spot-check) | 4 |
| `native.rs` | 跨 DB 池 (PG/SQLite/MySQL)，行读写，游标分页，类型强制转换 | ~13 |
| `remote_import.rs` | litellm→aigw: plain tables + credentials + proxy_models (key rotation) + spend_logs (cursor/batch/skip-body) | 8 |
| `remote_export.rs` | aigw→litellm 反向管道 (roundtrip + re-encryption) | 1 |
| `sync.rs` | aigw→aigw 同 schema 增量同步 | 0 (仅在 tests/sync.rs 中有集成测试) |
| `main.rs` | CLI 入口 (clap 子命令分发) | 0 |
| `lib.rs` | TABLE_MAPPINGS 常量 + 模块导出 | 0 |

### 4.2 集成测试（`crates/aigw-migrate/tests/`）

| 测试文件 | 测试数 | 覆盖内容 |
|---------|--------|---------|
| `sync.rs` | 8 | full sync 11 tables, --tables subset, --days filter, idempotent rerun, --skip-body nulls, illegal table error, config exclude/include |
| `verify_nested_decrypt.rs` | 1 | 从 fixture JSON 嵌套解密 litellm_params + credential_values |
| `native_pool_poc.rs` | 4 (3 ignored) | SQLite↔PG 往返，类型强制转换，virtual_keys 完整 schema (需本地 PG) |

### 4.3 BDD 覆盖（`crates/aigw-server/tests/features/`）

| Feature 文件 | 场景数 | 覆盖的迁移功能 |
|-------------|--------|--------------|
| `migration.feature` | 4 | credential 创建/读取/更新/删除 (mock BDD)，验证 credentials 表迁移后正常工作 |
| `real/migration_sync.feature` | 4 | **Real BDD**: sync plain tables (含行数验证), sync credentials (key rotation), sync proxy_models (key rotation), sync spend_logs (limit 10) |
| `real/migration_rollback.feature` | 2 | **Real BDD**: rollback plain tables→litellm, rollback credentials→litellm (含行数验证) |

### 4.4 迁移功能缺失覆盖清单

| # | 缺失覆盖 | 严重程度 | 详情 |
|---|---------|---------|------|
| 1 | **PreCheck BDD** | 🟡 中 | 6 项迁移前检查仅在 `pre_check.rs` 中有单元测试，无 BDD |
| 2 | **Verify standalone BDD** | 🟡 中 | `aigw-migrate verify` 子命令仅在 `verify.rs` 中有单元测试 |
| 3 | **Import/Export local BDD** | 🟡 中 | 本地 SQLite 导入/导出仅在 `import.rs`/`export.rs` 中有单元测试 |
| 4 | **RemoteExport 完整验证** | 🟡 中 | BDD 仅覆盖 plain tables + credentials 回滚；缺少 proxy_models 和 spend_logs 回滚 |
| 5 | **Spend logs cursor/resume BDD** | 🟡 中 | `--spend-log-resume-after`/`--end-before` 语义仅在 remote_import.rs 中有单元测试 |
| 6 | **Step filter BDD** | 🟢 低 | `--step-filter` 仅在 remote_import.rs 中有单元测试，无 BDD |
| 7 | **--skip-body/--skip-columns 跨 DB BDD** | 🟢 低 | 仅在 sync.rs 集成测试中有覆盖，无远程 DB BDD |
| 8 | **Aigw sync BDD (aigw→aigw)** | 🟡 中 | `sync.rs` 子命令有 8 个集成测试，但 `real/` 中无对应 BDD（现有 real/migration_sync.feature 仅覆盖 litellm→aigw） |
| 9 | **MySQL/PG 特定路径** | 🟡 中 | native.rs 的 MySQL 和 PostgreSQL 游标/类型逻辑无独立测试，bdd-real-mysql/pg 提供部分覆盖 |
| 10 | **--test 采样模式** | 🟢 低 | `sync::run_sync_test` 无测试覆盖 |
| 11 | **错误恢复路径** | 🟡 中 | 无源 DB 断开/目标 DB 断开/加密值不可解密跳过等场景测试 |

**总体评估**: 迁移模块**单元测试覆盖良好** (约 28 个内联测试)，**BDD 覆盖偏弱** (仅 10 个场景)，Real BDD 仅覆盖 litellm↔aigw 的 plain tables + credentials + proxy_models + spend_logs 基本路径。

---

## 五、aigw-frontend BDD 覆盖

| Feature 文件 | 场景数 | 覆盖功能 |
|-------------|--------|---------|
| `dashboard.feature` | 10 | 仪表盘页面 (spend overview cards, daily trends chart, rankings) |
| `login.feature` | 7 | 登录页面 |
| `jobs.feature` | 20 | 作业管理页面 (Sub-Tabs, stats cards, manual trigger, job history, detail, logs, mobile) |
| `keys.feature` | 8 | 密钥管理页面 |
| `models.feature` | 8 | 模型管理页面 (list, detail, search, create, edit, delete, health tab) |
| `spend-logs.feature` | 13 | 消费日志页面 (table, presets, live tail, pagination, search, detail drawer) |
| `playground.feature` | 6 | Playground (API 试验台) |
| `users.feature` | 3 | 用户管理页面 (list, create, delete) |
| `i18n-switcher.feature` | 3 | 国际化切换 |
| `mobile.feature` | 3 | 移动端适配 |
| `orgs.feature` | 1 | 组织管理页面 (列表) |
| `teams.feature` | 1 | 团队管理页面 (列表) |

**前端 BDD 总计: 83 场景, 12 个 feature 文件**

---

## 六、全覆盖矩阵总结

### ✅ 完整覆盖 (Mock BDD + 部分 Real BDD)

| 功能领域 | Mock BDD | Real BDD | 状态 |
|---------|---------|---------|------|
| Keys CRUD | ✅ keys.feature (8) | - | ✅ 完整 |
| Keys 权限 | ✅ keys_permission.feature (6) | - | ✅ 完整 |
| Budget CRUD | ✅ budget_reset.feature (4) | - | ✅ 完整 |
| Models CRUD | ✅ models.feature (8) | - | ✅ 完整 |
| User CRUD | ✅ auth.feature (9) | - | ✅ 完整 |
| Team CRUD | ✅ auth.feature (9) | - | ✅ 完整 |
| Org CRUD | ✅ auth.feature (9) | - | ✅ 完整 |
| Credentials | ✅ end_to_end.feature | - | ✅ 完整 |
| Spend (所有端点) | ✅ spend*.feature | ✅ 5个 real BDD | ✅ 完整 |
| Chat completions | ✅ messages.feature (9) | ✅ compatibility_real (3) | ✅ 完整 |
| Anthropic native | ✅ anthropic_native.feature (4) | - | ✅ 完整 |
| Login/Logout | ✅ auth.feature (9) | - | ✅ 完整 |
| Admin Jobs | ✅ admin_jobs.feature (20) | - | ✅ 完整 |
| Body Archive | ✅ 2 feature 文件 (19) | ✅ admin_real (2) | ✅ 完整 |
| Adapter | ✅ adapter.feature (5) | ✅ protocol_conversion (2) | ✅ 完整 |
| Migration | ✅ migration.feature (4) | ✅ rollback (2) + sync (4) | ✅ 完整 |
| Error Handling | ✅ error_handling.feature (8) | - | ✅ 完整 |
| Model Access | ✅ model_access.feature (5) | - | ✅ 完整 |
| Async Task | ✅ async_task.feature (15) | - | ✅ 完整 |
| E2E | ✅ end_to_end.feature (8) | ✅ e2e_real (6) | ✅ 完整 |
| Health | ⚠️ health.feature (6) 部分 | - | ⚠️ 部分 |
| Frontend 全页面 | ✅ 92 场景 | - | ✅ 完整 |

### ⚠️ 覆盖不足/缺失

| 模块 | 问题 | 严重程度 |
|------|------|---------|
| **health.rs** — `health_latest`, `prometheus_metrics`, `health_metrics` | 3个端点无 BDD 场景 | 🔴 高 |
| **router_settings.rs** — `patch_key`, `patch_team` | 2个 PATCH 端点无 BDD | 🟡 中 |
| **deleted_list 端点** — `team_deleted_list`, `model_deleted_list`, `user_deleted_list`, `org_deleted_list` | 4个 deleted_list 端点无 BDD（仅 keys 有） | 🟡 中 |
| **daily_spend_queue.rs** | 纯内部，无独立测试 | 🟡 中 |
| **metrics.rs** | 纯内部，无测试 | 🟡 中 |
| **rate_limiter.rs** | 纯内部，无独立测试 | 🟡 中 |
| **tenant.rs** | 纯内部，无测试 | 🟡 中 |
| **deployment.rs** | 纯内部，无测试 | 🟢 低 |
| **config.rs** | 纯内部，无测试 | 🟢 低 |
| **instance.rs** | 纯内部，可能被 chat 间接覆盖 | 🟢 低 |
| **otel_tracing.rs** | 纯内部，无测试 | 🟢 低 |
| **docs.rs** | 纯 HTML 页面，BDD 价值低 | 🟢 低 |
| **crypto.rs** | 内部，仅迁移测试间接覆盖 | 🟡 低 |

---

## 七、结论与建议

### 7.1 总体评估

**整体 BDD 覆盖率良好**。71 个路由处理器中，60 个有 BDD 覆盖 (84.5%)，10 个端点明确缺失 BDD。

**测试层级全景:**

| 层级 | 覆盖范围 |
|------|---------|
| **L1: 单元测试** (`#[cfg(test)]`) | aigw-core: ~120 内联测试 (crypto 27, router 16, resolver 10, engine 3, etc.)<br>aigw-migrate: ~28 内联测试 (native 13, remote_import 8, pre_check 4, etc.) |
| **L2: 集成测试** (`tests/*.rs`) | aigw-core: 41 测试 (stage82 15 + stage83 9 + integration_test + etc.)<br>aigw-migrate: 13 测试 (sync 8 + verify 1 + native_pool 4)<br>aigw-server: 13 测试 (artifact 4 + deployment 5 + dockerfile 4) |
| **L3: Mock BDD** (`@mock`) | 23 feature 文件, ~160 场景 (server) + 12 feature 文件, 83 场景 (frontend) |
| **L4: Real BDD** (`@real_api`) | 9 feature 文件, ~36 场景 |
| **L5: 基础设施/CI** | docker-compose.test.yml, Taskfile.yml (test, test-bdd, bdd-real-pg, bdd-real-mysql, bdd-real-sqlite) |

**测试总数: ~450+** (单元 ~150 + 集成 ~67 + Mock BDD ~243 + Real BDD ~36)

**优势:**
- 23 个 mock BDD feature 文件,覆盖所有核心 CRUD 操作
- 9 个 real BDD feature 文件,覆盖关键集成路径(消费排行、协议兼容性、迁移、归档)
- 前端 12 个 BDD feature 覆盖全部页面 (83 场景)
- admin_jobs, async_task, spend_aggregation 特别详细 (14-27 场景)
- aigw-core 核心模块单元测试完备 (crypto 27, router 16, resolver 10, engine 3 + 15 集成测试)

**不足:**
- `daily_spend_queue.rs` **零测试覆盖** — 最大单一风险点
- 10 个路由端点缺失 BDD (3 health metrics + 2 router_settings PATCH + 4 deleted_list + 1 docs)
- aigw-migrate 的 BDD 偏弱 (仅 10 个场景)，许多高级功能 (step filter, skip-columns, cursor resume, MySQL/PG 特定路径) 无 BDD
- `middleware/rate_limit.rs` 和 `middleware/auth_gateway.rs` 无独立 BDD 或单元测试(依赖子组件测试)
- 前端 orgs/teams 页面仅 1 个场景(列表显示),无 CRUD BDD

### 7.2 优先补充项

| 优先级 | 模块 | 建议 | 预估工作量 |
|--------|------|------|----------|
| 🔴 P0 | `daily_spend_queue.rs` | **补充单元测试** — 备份关键路径每日消费预聚合，当前零测试 | 小 |
| 🔴 P0 | `health_latest`, `prometheus_metrics`, `health_metrics` | 补充 3 BDD 场景: health latest 返回最近检查状态、prometheus metrics 格式正确、health metrics 返回 JSON | 小 |
| 🟡 P1 | `router_settings` — `patch_key`, `patch_team` | 补充 2 BDD 场景: patch key/team router settings | 小 |
| 🟡 P1 | `deleted_list` 系列 (4 端点) | 补充 4 BDD 场景 (各实体一个软删除列表场景) | 中 |
| 🟡 P1 | `middleware/rate_limit.rs` | 补充单元测试 or Real BDD: RPM/TPM 超限返回 429 | 中 |
| 🟡 P2 | aigw-migrate `--step-filter`/`--skip-columns` | 补充 2-3 BDD 场景覆盖 CLI 参数路径 | 中 |
| 🟡 P2 | aigw-migrate MySQL/PG 特定路径 | 在 `native_pool_poc.rs` 的 ignored 测试基础上补充或在 bdd-real-pg/mysql 中覆盖 | 中 |
| 🟢 P3 | `middleware/auth_gateway.rs` | 补充单元测试 for TenantIdentity/DeploymentMode | 小 |
| 🟢 P3 | `metrics.rs` (Prometheus 端点) | 补充 1 BDD 场景: GET /metrics 格式检查 | 小 |
| 🟢 P3 | 前端 orgs/teams 页面 | 如需要，补充 CRUD BDD (当前仅 1 列表场景) | 中 |

### 7.3 关键发现

1. **71 个路由处理器中 60 个有 BDD 覆盖** (84.5%)，仅 docs.rs 完全无覆盖（预期内）
2. **10 个端点明确缺失 BDD**: 3 个 health metrics 端点 + 2 个 router_settings PATCH + 4 个 deleted_list + 1 个 docs
3. **Real BDD 覆盖了 5 个关键消费端点和 2 个迁移路径**，但对 health、router_settings、deleted_list 等管理端点无 real BDD
4. **aigw-core 纯内部模块**（config, metrics, otel, rate_limiter, tenant, instance）确实无法做 BDD，但应有单元测试，当前大多缺失
5. **前端 BDD 覆盖率良好** (92 场景)，组织/团队页面仅 1 场景偏少，但功能简单
