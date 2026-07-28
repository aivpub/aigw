# aigw -- 下一步行动

**上次更新**: 2026-07-28
**当前阶段**: Phase 33 ✅ aigw↔aigw 多表只读增量同步（`aigw-migrate sync`，Stage 86 完成）

---

## 当前状态：82/86 Stages（Stage 86 ✅，Phase 33 完成）

**下一项工作 — 长期路线触发式 + Phase 30 一并标记 ✅**：Phase 33（Stage 86 `aigw-migrate sync` 子命令）于 2026-07-28 完成。`aigw-migrate` 新增 `sync` 子命令，在两个 aigw DB 实例间（PG↔SQLite 任意组合）只读增量同步——默认全 11 张业务表，`--tables` 选子集，`spend_logs` 支持 `--days` 增量，其他表全量幂等追加，重跑不重复。复用 `SourcePool`/`CursorRange`/`insert_rows_batch`；新增 `build_aigw_cursor_sql`（锚点 `start_time`，不改 litellm `build_cursor_sql` 保零回归）+ `stream_pg_rows_keyset_aigw`（PG keyset `(start_time, call_id)`）；空 overrides direct-match（aigw↔aigw 同 schema）；`credentials`/`proxy_models` 直接复制密文（同 master_key）；config 默认排除（含 master_key）。TDD 8 UT 红绿全过，`cargo test -p aigw-migrate` 全量无回归。**待办**：① Phase 30（Stage 78-81）代码已落地 + Phase 31 修复完成，待一并回写为 ✅（保持审计可追溯）；② TD-006 客户端 call_id 响应头回写；③ 长期路线 LT-BodyMetrics/LT-BodyCompact/LT-BodyLifecycle 视数据量触发。

---

## 上一阶段回顾 — Stage 86（Phase 33 ✅）

Stage 86（`aigw-migrate sync` 子命令 — aigw↔aigw 多表只读增量同步）于 2026-07-28 完成。核心预期：任意两个 aigw DB 实例间（PG↔SQLite 任意组合）一条 CLI 同步数据，默认全 11 张业务表，`--tables` 选子集，`spend_logs` 按"最近 N 天"增量，其他表全量幂等追加，重跑不重复。实现：native.rs `build_aigw_cursor_sql`（锚点 `start_time`，不动 litellm `build_cursor_sql` 保零回归）+ `stream_rows_with_cursor_aigw` + `stream_pg_rows_keyset_aigw`（PG keyset `(start_time, call_id)`）；sync.rs `run_sync` + `SyncStats` + 常量 + `parse_tables`/`resolve_tables`/`resolve_cursor`；main.rs `Sync` 子命令 + short alias（-s/-t/-T/-d/-r/-e/-B/-b）+ env 回退。空 overrides direct-match；加密表直接复制密文；config 默认排除。TDD 8 UT 红绿（全表/子集/`--days 7`/幂等/`--skip-body`/非法表名/config 默认排除+显式不覆盖/DEFAULT_TABLES 契约）。验证：`cargo test -p aigw-migrate` 27+27+8+1 全，无回归；`aigw-migrate sync --help` 输出表清单。Phase 30（Stage 78-81）仍待一并标记 ✅。

### 项目里程碑

```
Phase 0-4:  ████████████████████ 100% (6/6)  ✅
Phase 5:    ████████████████████ 100% (6/6)  ✅
Phase 7:    ████████████████████ 100% (5/5)  ✅
Phase 8:    ████████████████████ 100% (3/3)  ✅
Phase 9:    ████████████████████ 100% (4/4)  ✅
Phase 11:   ████████████████████ 100% (6/6)  ✅
Phase 12:   ████████████████████ 100% (3/3)  ✅
Phase 13:   ████████████████████ 100% (6/6)  ✅
Phase 14:   ████████████████████ 100% (4/4)  ✅
Phase 15:   ████████████████████ 100% (3/3)  ✅
Phase 16:   ████████████████████ 100% (3/3)  ✅
Phase 17:   ████████████████████ 100% (3/3)  ✅
Phase 18:   ████████████████████ 100% (2/2)  ✅
Phase 19:   ████████████████████ 100% (2/2)  ✅
Phase 20:   ████████████████████ 100% (2/2)  ✅
Phase 21:   ████████████████████ 100% (2/2)  ✅
Phase 22:   ████████████████████ 100% (2/2)  ✅
Phase 23:   ████████████████████ 100% (2/2)  ✅
Phase 24:   ████████████████████ 100% (1/1)  ✅
Phase 25:   ████████████████████ 100% (1/1)  ✅
Phase 26:   ████████████████████ 100% (3/3)  ✅
Phase 27:   ████████████████████ 100% (3/3)  ✅ 全栈质量修复 + Usage 图表增强
Phase 28:   ████████████████████ 100% (1/1)  ✅ 安全与质量加固
Phase 29:   ████████████████████ 100% (4/4)  ✅ Cross-DB BDD Hardening
Phase 30:   ░░░░░░░░░░░░░░░░░░░░   0% (0/4)  ⚠️ 代码已落地，Phase 31 修复完成待一并标记 ✅
Phase 31:   ████████████████████ 100% (3/3)  ✅ Stage 82-84 全部完成
Phase 32:   ████████████████████ 100% (1/1)  ✅ request_id → call_id 改名 + 上游对账链路（Stage 85）
Phase 33:   ████████████████████ 100% (1/1)  ✅ aigw↔aigw 多表只读增量同步（Stage 86）
```

### 测试目标

| 层 | 框架 | 当前 |
|---|------|------|
| 后端单元 | libtest | ~275 tests（aigw-core 247 + Stage 82 单测 18 + Stage 83 单测 10）|
| 后端 BDD | cucumber-rust | 178 scenarios（mock 163 pass / 15 skip，含 Stage 85 核心预期 2：双列返回 + 双列搜索）|
| 后端 real BDD | cucumber-rust + testcontainers | 36 scenarios × 3 后端（sqlite/pg/mysql 全绿；real 上游 key 失败为环境问题非 Stage 85）|
| 前端 BDD | Playwright + playwright-bdd | 252 tests（含 jobs 81 = 27 scenarios × 3 viewports）|

---

## 优先级排序

| 优先级 | Phase | 目标 | 状态 |
|--------|-------|------|------|
| ✅ | Phase 31 | 后端正确性全栈（Stage 82）| ✅ 完成（2026-07-27）|
| ✅ | Phase 31 | 读路径 + 缓存激活 + 凭证 + FS（Stage 83）| ✅ 完成（2026-07-27）|
| ✅ | Phase 31 | 前端 Jobs 页面生产化重构（Stage 84）| ✅ 完成（2026-07-27）|
| ✅ | Phase 32 | request_id → call_id 改名 + 上游对账链路打通（Stage 85）| ✅ 完成（2026-07-28）|
| ✅ | Phase 33 | aigw↔aigw 多表只读增量同步（Stage 86）| ✅ 完成（2026-07-28）|

---

## Phase 32: request_id → call_id 改名 + 上游对账链路打通 ✅

**起因**: 当前 aigw 把自身 UUID v7 存在 `spend_logs.request_id`（PK，语义=网关调用标识），但行业惯例（含 litellm）中 `request_id` 指上游 provider 返回的请求 ID。导致语义混淆 + 售后对账断链（SpendLog 未存上游 ID，退款/排查无法与 provider 对账）。**核心预期**：任意 SpendLog 都能用上游 `request_id` 与 provider 对账，无论成功还是 4xx/5xx 失败。

**完成情况**: 设计文档经 Gate-2 多模型评审定稿（v6.1，lead 独立 + 3 路 subagent）。评审关键发现：v5 的 COALESCE-UPDATE 方案对失败路径无效（失败行 INSERT-only，核心预期会静默失败）→ 改为 INSERT 时写入 upstream_id。migrate override 方向写反 → 更正。MySQL 索引需前缀长度 128。三处不改边界（HTTP 层 / 对外协议响应体 / litellm 源端 SQL）严格遵守。

**设计文档**: docs/stages/stage-85.md + docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md（v6.1）+ docs/research/2026-07-27-stage-85-design-review-consolidated.md

---

## Phase 31: Body Archive 生产化 ✅

**起因**: Phase 30 代码落地后用户实测 8 问题（job 卡 pending / logs 空 / steps 假阳性 completed / tab 下划线 / Disabled 仍执行 / Manual Trigger 独占行 / 列表无分页 / 详情页冗余 + 子页面不可直达）。**工作量下调**：原 4 Stage/50h 偏高 2-5 倍，按 subagent 并发实测 + 同触文件合并，收敛为 3 Stage/24h。**每个 Stage 强制 TDD 红绿循环 + BDD + real BDD 三后端实际执行验证，发现错误及时修复**。

| Stage | 目标 | 类型 | 预估 | 状态 |
|-------|------|------|------|------|
| Stage 82 | 后端正确性全栈（状态机 running/failed/partially_failed + 配置单例化 + execute storage_configured 门禁 + 冷数据回源接通 + create_job/claim 事务化 + increment 原子化 + finalize 错误传播 + retry 退避 + start_time TimestampMillisecond）。TDD：18 单测 + async_task 15 BDD + admin_jobs 12 BDD + body_archive_admin_real 3 @real_api；real BDD：三后端全绿 | 后端 | 10h | ✅ 完成（2026-07-27）|
| Stage 83 | 读路径 + 缓存激活（query_parquet_with_cache footer cache→row group→col chunk range read）+ read_body 错误区分 + S3 env 凭证 + FileSystem 后端。TDD：10 测试红绿；real BDD：三后端全绿 | 后端 | 6h | ✅ 完成（2026-07-27）|
| Stage 84 | 前端生产化重构（路由化 /dash/jobs/:jobId + Tab 美化 + 列表分页 + 详情去冗余 + Steps 分页 + Logs 按 step + 矛盾检测 + a11y）。TDD：11 BDD 红绿（Playwright mock，3 viewports）；real BDD：分页/trigger 409/冷回源 body | 前端 | 8h | ✅ 完成（2026-07-27）|

**合计**: 24h，3 Stages

**依赖**: Stage 82 → 83（后端串行）；Stage 82 → 84（前端可与 83 部分并行）

**设计文档**: `docs/stages/stage-82.md` ~ `docs/stages/stage-84.md`

## Phase 30: Body Archive 冷存储 ⚠️ 待修复

> Stage 78-81 已编码落地但未通过生产审计，详见 Phase 31。原设计文档 `stage-78~81.md` 保留作为实现参考。

## 需求对齐总结

| 问题 | 决策 |
|------|------|
| 是否需要独立 CLI 批量归档存量数据？ | **不需要**，`POST /admin/archive/trigger` API 已支持任意日期范围批量归档（Stage 80） |
| 日 compaction 要纳入首批吗？ | **推迟到后续优化**，小时文件 2-40MB 可接受 |
| 监控指标要纳入首批吗？ | **推迟**，执行进度和错误记录在 `archive_job_logs` 表，可通过 API/前端查看 |
| 交付顺序？ | **写链路 → 读链路 → API → 前端**（严格串行，每 Stage 独立可测） |

## 后续路线

| ID | 主题 | 优先级 | 状态 |
|----|------|--------|------|
| LT-BodyCompact | Body Archive 日 compaction | P2 | 小时文件碎片过多时 |
| LT-BodyLifecycle | S3 生命周期自动删除 | P2 | 冷数据积累 > 100GB |
| LT-BodyMetrics | Body Archive 监控指标 | P2 | 生产运维需要 |
| LT-Redis | Redis 缓存 | P2 | QPS > 1000 |
| LT-PG | PostgreSQL 生产级 | P2 | 多实例 + 高可用 |
| LT-SSO | SSO/OAuth | P3 | 企业客户需求 |
| LT-K8s | Kubernetes Operator | P3 | 云原生客户需求 |

> **已消化**: LT-Native → Phase 22, LT-Router → Phase 23, LT-Settings → Phase 24, LT-Usage → Phase 27, LT-CrossDB → Phase 29, LT-BodyArchive → Phase 30

## ADR 记录

| 编号 | 决策 | 日期 |
|------|------|------|
| ADR-013 | /v1/messages 接口审计 — 7 bugs（2 CRITICAL）| 2026-07-11 |
| ADR-014 | 当前无 Provider 适配架构 — 仅单一 DefaultAdapter | 2026-07-11 |
| ADR-015 | 架构重构优先于功能增强 | 2026-07-14 |
| ADR-016 | System Message Normalization (chat_template_compat) | 2026-07-16 |
| ADR-017 | model_group 语义对齐 litellm: model_name 而非 litellm_params.model | 2026-07-21 |
| ADR-018 | HTTP 层重试选用 reqwest-middleware + reqwest-retry, 单条 spend_logs 记录重试次数 | 2026-07-21 |
| ADR-019 | Phase 31 完成 — Body Archive 生产化（Stage 82-84，前端 Jobs 页面路由化 + 分页 + 矛盾检测 + a11y）| 2026-07-27 |
| ADR-020 | Phase 32 完成 — request_id→call_id 改名 + 上游对账链路。网关调用 ID 改名 call_id（PK），上游 provider 返回 ID 存为 request_id（可空+索引）。核心预期：任意 SpendLog 都能用上游 request_id 与 provider 对账（成功+4xx/5xx）。Gate-2 评审关键决策：失败路径 upstream_id 走 INSERT 非 UPDATE（COALESCE-UPDATE 不覆盖失败行）；migrate override key=target/value=source；三处不改（HTTP 层 / 对外协议响应体 / litellm 源端 SQL）。TD-006（客户端 call_id 响应头回写）留作后续 | 2026-07-28 |
| ADR-021 | Phase 33 完成 — aigw↔aigw 多表只读增量同步（`aigw-migrate sync` 子命令）。aigw↔aigw 同构同步（同表名/同 snake_case/同 PK `call_id`），空 overrides direct-match，不做 litellm 的 `call_id←request_id` 重定向。新增 `build_aigw_cursor_sql`（锚点 `start_time`，不改 litellm `build_cursor_sql` 保零回归）+ `stream_pg_rows_keyset_aigw`（PG keyset `(start_time, call_id)`）。加密表 `credentials`/`proxy_models` 直接复制密文（同 master_key，不调密钥轮转）；config 默认排除（含 master_key）。只读追加（`INSERT OR IGNORE`/`ON CONFLICT DO NOTHING`），非常驻/非 CDC。TDD 8 UT。 | 2026-07-28 |
