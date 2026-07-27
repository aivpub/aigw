# aigw -- 下一步行动

**上次更新**: 2026-07-27
**当前阶段**: Phase 31 ✅ Body Archive 生产化完成（Stage 82 ✅，Stage 83 ✅，Stage 84 ✅）

---

## 当前状态：80/84 Stages（Stage 82-84 ✅，Phase 31 进度 3/3）

Phase 30（Stage 78-81）代码已编码落地（feat/body-archive 分支），2026-07-25 审计确认未达生产预期（6 P0 + 10 P1 + 12 P2）。修复转入 Phase 31（Stage 82-84，3 Stage / 24h）。**Stage 82 已于 2026-07-27 完成**：恢复 dangling commit 链 f6089fd + cherry-pick HEAD 修复，实现 P0 全栈。**Stage 83 已于 2026-07-27 完成**：读路径 + 缓存激活 + FileSystem 后端，`query_parquet_with_cache`（footer cache → row group → col chunk range read），激活 FooterCache 死代码，`read_body_from_storage` 区分 NotFound vs 不可达，S3 `${ENV_VAR}` 占位符，`StorageBackend::FileSystem` 接 `LocalFileSystem`。**Stage 84 已于 2026-07-27 完成**：前端 Jobs 页面生产化重构 — 删除 602 行单文件巨石 `jobs.tsx`，启用 `pages/jobs/` 目录（index + job-detail + components/trigger-dialog + lib/api/jobs）；路由化 `/dash/jobs/:jobId` 子路由 + `useSearchParams` 驱动 tab/page/status；`STEP_LABELS` 美化（body_archive → "Body Archive"）+ fallback `replace(/_/g," ")`；Manual Trigger 按钮挪到 `TabsList` 右侧同行；Archive Disabled 联动 `disabled` + tooltip；列表表格化 + 分页（`ListPagination`，后端 `jobs.rs` list response 含 `total`）；详情页独立路由去冗余 + Steps 分页 pageSize=20 + Payload/Result/Duration 列；Logs 按 `step_key` 分组折叠；矛盾检测 `displayJobStatus`（summary.running>0 → running）+ completed+rows_archived=0 → 灰色 "completed (no-op)" badge + 错误 toast + a11y（tabIndex/onKeyDown/aria-label）。TDD 红绿：先修 playwright-bdd bddgen 崩溃（cucumber 表达式 `/` alternation + `{job_id}` 未注册参数类型 + 重复 step 定义 + 参数个数不匹配），再 11 个 Stage 84 新场景 × 3 viewports = 81/81 全绿（mock API）。验证：aigw-core lib 247/247、Stage 82 单测 18/18、Stage 83 单测 10/10、mock BDD 全绿（含 jobs 81）、三后端 real BDD 36/36 全绿、fe-build dist 嵌入 rust-embed、fe-lint 无 error（仅 warning）。Phase 30 待 Stage 84 收尾后一并标记 ✅。

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
Phase 32:   ░░░░░░░░░░░░░░░░░░░░   0% (0/1)  ⏳ request_id → call_id 改名 + 上游对账链路（Stage 85）
```

### 测试目标

| 层 | 框架 | 当前 |
|---|------|------|
| 后端单元 | libtest | ~275 tests（aigw-core 247 + Stage 82 单测 18 + Stage 83 单测 10）|
| 后端 BDD | cucumber-rust | 176 scenarios（mock 161 pass / 15 skip，含 async_task 15 + admin_jobs 12 + body_archive_read 7 @skip）|
| 后端 real BDD | cucumber-rust + testcontainers | 36 scenarios × 3 后端（sqlite/pg/mysql 全绿）|
| 前端 BDD | Playwright + playwright-bdd | 252 tests（含 jobs 81 = 27 scenarios × 3 viewports）|

---

## 优先级排序

| 优先级 | Phase | 目标 | 状态 |
|--------|-------|------|------|
| ✅ | Phase 31 | 后端正确性全栈（Stage 82）| ✅ 完成（2026-07-27）|
| ✅ | Phase 31 | 读路径 + 缓存激活 + 凭证 + FS（Stage 83）| ✅ 完成（2026-07-27）|
| ✅ | Phase 31 | 前端 Jobs 页面生产化重构（Stage 84）| ✅ 完成（2026-07-27）|
| ⏳ | Phase 32 | request_id → call_id 改名 + 上游对账链路打通（Stage 85）| ⏳ 待开始（2026-07-27）|

---

## Phase 32: request_id → call_id 改名 + 上游对账链路打通 ⏳

**起因**: 当前 aigw 把自身 UUID v7 存在 `spend_logs.request_id`（PK，语义=网关调用标识），但行业惯例（含 litellm）中 `request_id` 指上游 provider 返回的请求 ID。导致语义混淆 + 售后对账断链（SpendLog 未存上游 ID，退款/排查无法与 provider 对账）。**核心预期**：任意 SpendLog 都能用上游 `request_id` 与 provider 对账，无论成功还是 4xx/5xx 失败。

**单 Stage 说明**: 设计文档 `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md`（v5，5 轮评审定稿）总耗时 ~6h，强耦合串行无并行收益，收敛为 1 Stage / 8h。v5 增量：失败路径 4xx/5xx 也提取并存储上游 id（覆盖对账盲区）。**三处不改边界**：HTTP 层 `tower_http::request_id` + 对外协议响应体 `request_id`（Anthropic/OpenAI 契约）+ litellm 源端 SQL。

**设计文档**: docs/stages/stage-85.md + docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md

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
