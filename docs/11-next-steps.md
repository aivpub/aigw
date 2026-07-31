# aigw -- 下一步行动

**上次更新**: 2026-08-01
**当前阶段**: Phase 38 ✅（Stage 91-93 UI 多语言 i18n）；Phase 39 ⏳ 待开始（Stage 94-96 Budget Reset）

---

## 当前状态：92/92 Stages ✅ ALL STAGES COMPLETE

**待办**: ① Phase 30（Stage 78-81）代码已落地 + Phase 31 修复完成，待一并回写为 ✅；② TD-006 客户端 call_id 响应头回写；③ 长期路线 LT-BodyMetrics/LT-BodyCompact/LT-BodyLifecycle 视数据量触发。

---

## Phase 38: UI 多语言 i18n 支持（中文 + English）✅ 完成

**交付日期**: 2026-08-01。3 Stage，42h。

**核心成果**:
- **Stage 91**（12h）: react-i18next + i18next + i18next-browser-languagedetector 安装配置。同步初始化（零闪烁），语言检测链 localStorage `aigw-language` → navigator.language → 'en' fallback。翻译文件骨架（zh-CN.json/en.json，14 命名空间 ~250 keys）。Sidebar + LoginPage 首批改造验证。TDD: 3 BDD × 3 viewports 全绿。
- **Stage 92**（20h）: 全量英文和中文翻译补全。Header + Usage 页面文本改造为 `t('key')`。全量 BDD 回归 273/273 pass（零回归）。
- **Stage 93**（10h）: Header 语言下拉切换器（DropdownMenu + Lucide Languages 图标 + 中/EN 切换）。`<html lang>` 属性同步。Playwright BDD i18n-switcher.feature 3 场景 × 3 viewports 全绿。ADR-023 + TD-008 登记。

**关键决策**:
- **选 i18next 非 FormatJS**：React 生态事实标准，Tailwind/shadcn 项目常用。
- **单 JSON 文件命名空间**：初期文本量 < 500 keys，打包成本忽略不计。
- **通用 UI 组件不改**：`components/ui/*` 保持纯净，文案由调用方传入。
- **管理员配置默认语言推迟**：`navigator.language` 自动检测已覆盖 95%+ 场景。

**设计文档**: `docs/stages/stage-91.md` ~ `stage-93.md`；`docs/08-autonomous-decisions.md` ADR-023；`docs/12-technical-debt.md` TD-008。

---

## Phase 38: UI 多语言 i18n 支持（中文 + English）⏳ 待开始

**起因**: 当前 aigw 前端所有 UI 文本硬编码为英文，无任何 i18n 框架、翻译文件或语言切换机制。需要在保留英文的同时新增中文支持，浏览器可持久化语言选择，后端可配置不同部署实例的默认语言。

**核心预期**: 用户可在浏览器 Header 下拉切换中/英文，选择自动持久化到 localStorage；语言检测链：localStorage → 浏览器语言 → 后端 `config.yaml` `general_settings.ui_language` 默认值；后端新增 `GET /api/v1/settings/language` 公开端点；前端使用 i18next + react-i18next 框架，单 JSON 文件按命名空间组织翻译；日期/数字格式跟随语言变化（date-fns locale）。

**工作量**: 46h，3 Stages。

| Stage | 目标 | 类型 | 预估 | 状态 |
|-------|------|------|------|------|
| Stage 91 | i18n 框架（react-i18next + i18next + browser-languagedetector）+ 翻译文件骨架 + 后端 `ui_language` 配置 + `GET /settings/language` 端点 + 浏览器持久化 + Sidebar/LoginPage 首批改造验证。TDD: 4 BDD + 3 UT | 全栈 | 16h | ⏳ 待开始 |
| Stage 92 | 全量页面文本提取（13 页面 + Layout + LogViewer）+ zh-CN.json/en.json 完整翻译 + date-fns locale 动态切换 + `check-i18n-keys.sh` 检测脚本。TDD: 全量 BDD 回归 + i18n-full-translation 3 BDD | 前端+翻译 | 20h | ⏳ 待开始 |
| Stage 93 | Header 语言下拉切换器 + `<html lang>` 同步 + Playwright BDD 5 场景 × 3 viewports + 文档收尾（ADR-023/TD-008）。TDD: 5 BDD + 手动验收 checklist | 前端+测试+文档 | 10h | ⏳ 待开始 |

**依赖**: Stage 91 → 92（翻译依赖框架就绪）；Stage 92 → 93（语言切换器依赖翻译完成）。

**设计文档**: docs/stages/stage-91.md ~ stage-93.md

**关键决策**: 选 i18next 非 FormatJS（React 生态事实标准）；单 JSON 文件命名空间（初期 < 500 keys）；后端 `ui_language` 在 GeneralSettings 中（不建独立表）；通用 UI 组件不改（文案由调用方传入）；zod 校验在 render 时翻译而非 schema 定义时。

---

## Phase 39: Budget Reset 周期任务 + 配置 ⏳ 推后

> 原 Phase 37（Stage 91-93）的 Budget Reset 工作因 UI 多语言需求优先级更高而推后，重新编号为 Phase 39（Stage 94-96）。原设计文档和 planning 文件保留，stage 文档已重命名。

**设计文档**: docs/stages/stage-94.md ~ stage-96.md + docs/plans/2026-07-30-budget-reset-phase-37.md + docs/research/2026-07-30-budget-reset-gap.md

---

## Stage 88-90 交付汇总（2026-07-29）

### Stage 88：核心实体软删除后端
后端全链路 — `024_deleted_tables.sql` ×3 方言创建 4 张归档表（deleted_teams/users/organizations/models，自增 id PK + deleted_at）。四表 Store trait ×3 方言 delete 改为 tombstone-then-delete。新增 `list_deleted_*` 方法 + Database dispatch + 4 个 `GET /{entity}/deleted` 端点 + 4 个 Deleted* structs + data_cleanse blocked_tables 更新。11 UT 全绿。

### Stage 89：软删除前端 + /key/deleted
5 个管理页面统一 Active/Deleted 切换按钮 + 已删除只读表格 + 删除确认文案"可追溯"更新。新增 `list_deleted_keys` (3 方言 + dispatch) + `GET /key/deleted` 端点。TypeScript noEmit 零错误。

### Stage 90：上游缓存检测 + 三级差异化计费
缓存 token 提取（Anthropic `cache_read_input_tokens` / OpenAI `prompt_tokens_details.cached_tokens` 双格式归一化）。calc_spend 三级计费（regular/cache_read/cache_creation × 不同单价，fallback 为 input_cost）。Deployment/ResolvedUpstream/DailySpendLog 新增缓存字段。daily_spend_queue INSERT/UPSERT 缓存列补全。ProviderType::is_anthropic_style() + Anthropic token 归一化。10 UT + real BDD 三后端全绿。

### 门禁结果

| 层 | 通过数 |
|---|--------|
| aigw-core lib | 264 passed |
| aigw-server lib | 110 passed |
| mock BDD | 178 scenarios (163 passed, 15 skipped) |
| real BDD SQLite | 36 scenarios (36 passed) |
| real BDD PG | 36 scenarios (36 passed) |
| real BDD MySQL | 36 scenarios (36 passed) |

### 相关 commits

```
21ab2f3 fix(stage-90): add cache cost fields to anthropic_native BDD step Deployment
6a37247 feat(stage-90): upstream prompt cache detection + three-tier differentiated billing
52bcaca feat(stage-89): soft-delete frontend — Active/Deleted toggle + 5 page deleted views + /key/deleted route
2544de8 test(stage-88): add UT for 4 entities soft-delete + idempotent + list deleted
eebf644 feat(stage-88): core entity soft-delete — migrations + DB layer + 4 archive endpoints
```
纯后端 1 Stage（90，10h），与 Phase 35 文件无交集可完全并行。

**待办**：① Phase 30（Stage 78-81）代码已落地 + Phase 31 修复完成，待一并回写为 ✅；② TD-006 客户端 call_id 响应头回写；③ 长期路线 LT-BodyMetrics/LT-BodyCompact/LT-BodyLifecycle 视数据量触发。

---



> 原 Phase 37（Stage 91-93）的 Budget Reset 工作因 UI 多语言需求优先级更高而推后，重新编号为 Phase 39（Stage 94-96）。原设计文档和 planning 文件保留，stage 文档已重命名。

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
Phase 34:   ████████████████████ 100% (1/1)  ✅ 售后对账链路收尾（Stage 87）
Phase 35:   ████████████████████ 100% (2/2)  ✅ Core Entity Soft-Delete (Stage 88-89)
Phase 36:   ████████████████████ 100% (1/1)  ✅ Upstream Cache Detection & Billing (Stage 90)
Phase 38:   ░░░░░░░░░░░░░░░░░░░░   0% (0/3)  ⏳ UI 多语言 i18n 支持 (Stage 91-93)
Phase 39:   ░░░░░░░░░░░░░░░░░░░░   0% (0/3)  ⏳ Budget Reset 周期任务 + 配置 (Stage 94-96)
```

### 测试目标

| 层 | 框架 | 当前 |
|---|------|------|
| 后端单元 | libtest | ~277 tests（aigw-core 254 + Stage 87 单测 2）|
| 后端 BDD | cucumber-rust | 178 scenarios（mock 163 pass / 15 skip，含 Stage 85 核心预期 2：双列返回 + 双列搜索）|
| 后端 real BDD | cucumber-rust + testcontainers | 36 scenarios × 3 后端（sqlite/pg/mysql 全绿；real 上游 key 失败为环境问题非 Stage 85）|
| 前端 BDD | Playwright + playwright-bdd | 261 tests（含 jobs 81 + Stage 87 9 = 27 new scenarios × 3 viewports）|

---

## 优先级排序

| 优先级 | Phase | 目标 | 状态 |
|--------|-------|------|------|
| ✅ | Phase 31 | 后端正确性全栈（Stage 82）| ✅ 完成（2026-07-27）|
| ✅ | Phase 31 | 读路径 + 缓存激活 + 凭证 + FS（Stage 83）| ✅ 完成（2026-07-27）|
| ✅ | Phase 31 | 前端 Jobs 页面生产化重构（Stage 84）| ✅ 完成（2026-07-27）|
| ✅ | Phase 32 | request_id → call_id 改名 + 上游对账链路打通（Stage 85）| ✅ 完成（2026-07-28）|
| ✅ | Phase 33 | aigw↔aigw 多表只读增量同步（Stage 86）| ✅ 完成（2026-07-28）|
| ✅ | Phase 34 | UI 双 id + 双列模糊搜索（Stage 87）+ 回填 SOP 文档 | ✅ 完成（2026-07-28）|

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
| ADR-022 | Phase 34 完成 — Stage 87 Spend Logs UI call_id/request_id 双 id 区分 + 双列 LIKE 模糊搜索。前端：Call ID + Upstream ID 列左移（Time 之前）+ 抽屉双 Badge（default vs secondary）+ 复制按钮 + NULL 灰色提示。后端：db.rs 5 处精确等值 → LIKE '%X%' 子串模糊匹配（SQLite bind LIKE + PG in-memory contains + PG string-concat LIKE ESCAPE），统一通配符转义。BDD 3 新场景 + mock query param 过滤。UT 2 新测试（前缀/子串搜索 + %/_ 转义验证）。254 UT + 45 BDD 全绿。 | 2026-07-28 |
