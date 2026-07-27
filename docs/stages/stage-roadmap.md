# aigw — AI Gateway Stage Roadmap

**项目**: aigw (litellm Rust 最小兼容替代)
**最后更新**: 2026-07-27

---

## 当前状态

- **当前 Phase**: Phase 31 — Body Archive 生产化 ✅ (3/3 Stages，Stage 82 ✅，Stage 83 ✅，Stage 84 ✅)
- **状态**: 80/84 Stages 已完成（Stage 82-84 ✅；Stage 78-81 已编码落地，Phase 30 待一并标记 ✅）
- **下一里程碑**: Phase 30 收尾标记 + 长期路线（LT-BodyMetrics / LT-BodyCompact / LT-BodyLifecycle 视数据量触发）

### 整体进度

```
Phase 0-4:  ████████████████████ 100% (6/6 Stages)
Phase 5:    ████████████████████ 100% (6/6 Stages)
Phase 7:    ████████████████████ 100% (5/5 Stages)
Phase 8:    ████████████████████ 100% (3/3 Stages)
Phase 9:    ████████████████████ 100% (4/4 Stages)
Phase 11:   ████████████████████ 100% (6/6 Stages)
Phase 12:   ████████████████████ 100% (3/3 Stages)
Phase 13:   ████████████████████ 100% (6/6 Stages)
Phase 14:   ████████████████████ 100% (4/4 Stages)
Phase 15:   ████████████████████ 100% (3/3 Stages)
Phase 16:   ████████████████████ 100% (3/3 Stages)
Phase 17:   ████████████████████ 100% (3/3 Stages) ✅
Phase 18:   ████████████████████ 100% (2/2 Stages) ✅ Spend Logs & Usage 质量修复
Phase 19:   ████████████████████ 100% (2/2 Stages) ✅ UI Enhancement
Phase 20:   ████████████████████ 100% (2/2 Stages) ✅ 可观测性增强
Phase 21:   ████████████████████ 100% (2/2 Stages) ✅ 协议兼容性修复
Phase 22:   ████████████████████ 100% (2/2 Stages) ✅ Anthropic 原生上游
Phase 23:   ████████████████████ 100% (2/2 Stages) ✅ Router 负载均衡
Phase 24:   ████████████████████ 100% (1/1 Stage)  ✅ 管理控制台完善
Phase 25:   ████████████████████ 100% (1/1 Stage)  ✅ 健康检查 & UX 优化
Phase 26:   ████████████████████ 100% (3/3 Stages) ✅ 可观测性（Prometheus ✅, OTEL ✅, Body 分离 ✅）
Phase 27:   ████████████████████ 100% (3/3 Stages) ✅ 全栈质量修复 + Usage 图表增强
Phase 28:   ████████████████████ 100% (1/1 Stage)  ✅ 安全与质量加固
Phase 29:   ████████████████████ 100% (4/4 Stages) ✅ Cross-DB BDD Hardening
Phase 30:   ░░░░░░░░░░░░░░░░░░░░   0% (0/4 Stages) ⚠️ 代码已落地，Phase 31 修复完成待一并标记 ✅
Phase 31:   ████████████████████ 100% (3/3 Stages) ✅ Body Archive 生产化（Stage 82-84 全部完成）
```

---

## 当前 Phase 详情

### Phase 30：Body Archive 冷存储 ⏳

**背景**: spend_logs 表 messages/response/proxy_server_request 三个 JSON body 字段占 95%+ 存储体积（日均增长 4-5 GB）。Stage 77 已将 body 从列表接口分离，本 Phase 实现 body 的 S3 Parquet 冷存储归档——主库只保留最近 7 天热数据，历史 body 迁移到对象存储并压缩为 Parquet 列式格式（ZSTD 压缩 8-11x），查询时自动路由热/冷数据。

详见设计文档: `docs/plans/2026-07-22-body-archive-s3-parquet.md`

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 78 | ⏳ 待开始 | **AsyncTask + Engine 框架 + Body Archiver 写链路** — Migration 020/021（async_jobs 三张通用表 + spend_logs 加列）；AsyncTask trait（tick + execute + finalize）+ Engine（宿主运行时，spawn tick/exec/cleanup loop）；StorageBackend 多类型适配（S3/COS/R2/MinIO/FS）；BodyArchiver impl AsyncTask（Parquet ZSTD + Bloom filter → 目标存储 + 清理器）。TDD: UT 覆盖 claim_next_step SKIP LOCKED、loop 并发、storage config 解析、parquet 写入 | 后端+测试 | 14h |
| Stage 79 | ⏳ 待开始 | **Query Router + Footer Cache（读链路）** — query_parquet_with_cache(): footer 缓存（moka LRU）→ row group 定位（column statistics + Bloom filter）→ col chunk 读取 → 解码；get_message_body() 热/冷自动路由；详情端点集成存储 fallback。TDD: UT 覆盖路由逻辑、缓存命中/未命中、Parquet 读路径 | 后端+测试 | 10h |
| Stage 80 | ⏳ 待开始 | **Admin API + Col Chunk Cache + 存量归档** — 5 个 admin 端点（trigger/jobs/stats/job_detail/logs）；引擎统计查询（loops/pending/running/stale）；Col chunk 文件系统 LFU 缓存（可选）；通过 API 触发存量归档。TDD: UT 覆盖 admin API 鉴权、Job 生命周期、col chunk 缓存 | 后端+测试 | 12h |
| Stage 81 | ⏳ 待开始 | **前端 Jobs 管理页面** — Settings → Jobs Tab，按 step_type 分 Sub-Tab；统计卡片 + 手动触发 + 通用 JobList/JobDetail（Steps 表格 + logs 过滤 + 自动刷新）。TDD: 4 BDD × 3 viewports | 前端+测试 | 10h |

**依赖关系**: Stage 78 → Stage 79 → Stage 80 → Stage 81（严格串行）
- 78（写链路 + Schema）→ 79 需要已归档的测试数据做读链路验证
- 79（读链路）→ 80 的 admin API 详情端点需要冷查询能力
- 80（Admin API）→ 81 前端页面依赖所有 API 就绪

**Phase 30 合计**: 46h，4 Stages。

**关键决策**:
- **不需要独立 CLI**：存量归档通过 `POST /admin/archive/trigger` API 触发，支持任意日期范围，进度可查询
- **日 compaction 推迟**：小时文件 2-40MB 可接受，日合并作为后续优化
- **监控指标推迟**：所有执行进度和错误信息记录在 `archive_job_logs` 表，可通过 API/前端回溯
- **纯 Rust 栈**：parquet + arrow + object_store + moka，无 DuckDB C++ FFI 依赖
- **默认关闭**：`body_archive.enabled=false`，生产环境显式开启
- **DB 先写后归档**：body 仍然先入 DB，归档是纯后台异步操作，不影响核心写入链路

**设计文档**: `docs/stages/stage-78.md` ~ `docs/stages/stage-81.md`

> ⚠️ **生产审计结论**（2026-07-25）：Phase 30 代码已落地但未通过生产审计，三路 subagent 并行审计确认 6 个 P0 + 10 个 P1 + 12 个 P2 缺陷。Phase 30 状态标记为 ⚠️ 待修复，修复工作转入 Phase 31（Stage 82-84）。

### Phase 31：Body Archive 生产化 ✅

**背景**: Phase 30（Stage 78-81）编码落地后用户实测发现 8 个问题，居中调度三路 subagent 并行审计确认三大类生产缺陷：(1) 状态正确性——job 卡 pending（状态机缺 running/failed）、steps 假阳性 completed（execute 未检查 storage_configured）、配置失联（三处用 `default()` 导致 Disabled 仍可执行）；(2) 数据可观测性——logs 独立空区块、列表无分页、Steps 无分页；(3) UX/可分享性——tab 含下划线、Manual Trigger 独占行、详情页冗余、子页面不可 URI 直达。本 Phase 严格按审计报告修复，**每个 Stage 强制 TDD 红绿循环（先写失败测试跑红→重构至绿）+ BDD + real BDD 三后端实际执行验证，发现错误及时修复**。


**工作量下调说明**：原规划 4 Stage / 50h，用户反馈偏高 2-5 倍。按"subagent 并发实测 + 同触文件合并避免反复改"原则合并为 3 Stage / 24h：原 Stage 82+83（同触 engine.rs/body_archive/mod.rs）合并为新 82；原 84 重编号为 83；原 85 重编号为 84。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 82 | ✅ 完成（2026-07-27） | **后端正确性全栈（合并 P0+P1）** — 补全 job 状态机（pending→running→completed/failed/partially_failed）；配置单例化（AppState 注入 body_archiver，main.rs 从 config.yaml 解析，AigwConfig 加 body_archive 字段）；execute() 加 storage_configured() 门禁消除假阳性 completed；trigger enabled 检查返回 409；冷数据回源接通（spend detail 集成 get_message_body）；create_job/claim 事务化；increment_job_completed 原子化消竞态；finalize 失败标 failed；fail_step 加 next_retry_at 退避；start_time→TimestampMillisecond。**TDD 红绿**：18 单测 + async_task.feature 15 场景 + admin_jobs.feature 12 场景 + body_archive_admin_real 3 @real_api；三后端 real BDD 全绿。对应 Q1/Q3/Q4 | 后端+测试 | 10h |
| Stage 83 | ✅ 完成（2026-07-27） | **读路径 + 缓存激活 + 凭证安全 + FileSystem 后端** — 实现 query_parquet_with_cache（footer cache → row group 定位 → col chunk range read 三段流水线），激活 FooterCache 死代码消除全文件下载（P99 19MB → 仅 footer + 列块）；read_body_from_storage 区分 NotFound vs 不可达（Err 不吞 None）；S3 凭证支持 ${ENV_VAR} 占位符；StorageBackend::FileSystem 接入 LocalFileSystem（CI 无需 S3）。**TDD 红绿**：10 测试先红后绿；**BDD+real BDD**：本地 FS 全链路 + 三后端回源 | 后端+测试 | 6h |
| Stage 84 | ✅ 完成（2026-07-27） | **前端 Jobs 页面生产化重构** — 路由化（/dash/jobs/:jobId 子路由，useSearchParams，URI 直达/刷新/分享）；Tab 美化（body_archive→"Body Archive"）+ Trigger 同行布局；列表表格化 + 分页（后端 list response 加 total）；详情页去冗余 tab + 标题人类可读 + Steps 分页 + Payload/Result 列；Logs 加 Step Key 列按 step 分组；假阳性 completed 矛盾检测（rows_archived=0 灰色 no-op）；Archive Disabled 真禁用 Trigger；错误 toast 替换 silent fail；a11y 键盘导航。**TDD 红绿**：先修 playwright-bdd bddgen 崩溃（cucumber `/` alternation + `{job_id}` 未注册参数类型 + 重复 step 定义 + 参数个数不匹配），再 11 BDD 场景 × 3 viewports = 81/81 全绿（mock API）。对应 Q2/Q4-Q8 | 前端+测试 | 8h |

**依赖关系**: Stage 82 → 83（后端串行，读路径优化依赖冷回源端点）；Stage 82 → 84（前端兜底 + 矛盾检测依赖后端 summary.running + result 字段，可与 83 部分并行）
- 82（冷回源接通 + 状态机 + 配置）→ 83 的读路径有端点可改
- 82（状态机 + 配置）→ 84 的前端 displayStatus 兜底 + 矛盾检测依赖后端字段

**Phase 31 合计**: 24h，3 Stages。

**关键决策**:
- **TDD 红绿强制**：每个 Stage 先写失败测试（Red）跑红，再重构实现至绿（Green），不直接写实现。
- **BDD + real BDD 实际执行**：mock BDD（`task bdd` / `task fe-bdd` Playwright）+ real BDD（`task bdd-real-sqlite` / `task bdd-real-pg` / `task bdd-real-mysql` 三后端）必须全绿，发现的错误及时修复重跑，不积压。
- **先正确性后性能**：82 修 P0/P1 正确性，83 修读路径性能，84 修前端。不混入新功能。
- **同触文件合并**：原 82+83 同触 engine.rs/body_archive，合并避免反复改同一文件，降低工作量。
- **Phase 30 标记为 ⚠️ 待修复**：不回写 Stage 78-81 为 ✅，保持审计可追溯；Phase 31 完成后一并标记。
- **P2 技术债登记**：panic 容错（P2-2）、shutdown 信号（P2-3）登记到 `docs/12-technical-debt.md`（TD-005），作为 Phase 32 候选。
- **长期路线维持**：LT-BodyMetrics / LT-BodyCompact / LT-BodyLifecycle 优先级不变，Phase 31 后视数据量触发。

**设计文档**: `docs/stages/stage-82.md` ~ `docs/stages/stage-84.md`

### Phase 32：request_id → call_id 改名 + 上游对账链路打通 ⏳

**背景**: 当前 aigw 把自身 UUID v7 存在 `spend_logs.request_id`（PK，语义=网关调用标识），但行业惯例（含 litellm）中 `request_id` 指上游 provider 返回的请求 ID。导致语义混淆 + 售后对账断链（SpendLog 未存上游 ID，退款/排查无法与 provider 对账）。设计文档经 5 轮评审迭代定稿（v5），核验代码后修正 migrate 映射机制描述、补对外协议字段边界（§6.3）、可观测性影响（§10）、失败路径 4xx/5xx 提取（v5 增量）。

**核心预期**: 任意一条 SpendLog 记录都能用上游 `request_id` 去 provider 侧对上账，无论成功还是 4xx/5xx 失败。改名 `call_id`、流式提取、失败路径提取均为支撑项。

**单 Stage 说明**: 设计文档 §7 总耗时 ~6h 人时（Agent 编码加速），强耦合串行（DB schema 是路由层编译前提、路由层是前端/migrate 前提），拆多 Stage 无并行收益反而增加协调成本，故收敛为 1 Stage / 8h（对齐下限 + 三端联调 buffer）。Stage 内分三阶段：① DB schema+模型层 → ② 路由层+上游 id 全路径提取+前端+migrate（subagent 并行）→ ③ 三端联调+全测试+端到端。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 85 | ⏳ 待开始 | **request_id → call_id 改名 + 上游对账链路打通** — 022 迁移（pg/mysql/sqlite 双重条件幂等 RENAME + ADD COLUMN + 索引）；models.rs/db.rs/body_archive/daily_spend_queue 字段改名（~140 处）；路由层 chat.rs/v1_messages.rs 流式 chunk id 提取 + **4xx/5xx 失败路径提取**（OpenAI error body id / Anthropic error body request_id + 响应头 fallback）+ Phase 2 UPDATE 调用补 upstream_id；main.rs tracing span 字段改名；spend.rs/openapi API 字段拆分；migrate 注入 `request_id→call_id` override 解决存量导入 PK 歧义；前端 3 interface + 展示列 + CSV + 搜索；10 BDD + 5 非 BDD 单测。**核心边界**：HTTP 层 `tower_http::request_id`（§2.2）+ 对外协议响应体 `request_id`（§6.3）+ litellm 源端 SQL（§4.5）三处不改。**TDD 红绿**：失败路径 4xx/5xx/流式部分成功/连接超时 4 BDD 场景先红后绿。**验收三判据**：正向对账 `WHERE request_id=?` 走索引 / 反向追溯 call_id→上游 id 四路径非空 / 覆盖盲区 4xx-5xx 上游 id 非空 | 全栈+测试 | 8h |

**关键决策**:
- **单 Stage 不拆**：工作量 ~6h 低于 8h 下限，强耦合串行无并行收益，收敛为 1 Stage。
- **核心预期驱动**：所有改动服务"打通上游对账链路"这一唯一业务目标，改名/流式提取/失败路径提取是支撑项（设计文档 §1.3 + §9 顶层决策）。
- **失败路径也提取（v5）**：4xx/5xx 从 error body/响应头提取上游 id，让失败请求也能对账——核心预期覆盖盲区。
- **三处不改边界**：HTTP 层 / 对外协议响应体 / litellm 源端 SQL，否则破坏功能或契约。
- **migrate override 必做**：源端单 `request_id` 对目标 `call_id`+`request_id` 双列，不 override 则 PK 为 NULL 插入失败（设计文档 §4.5）。

**设计文档**: `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md`（v5）、`docs/stages/stage-85.md`

### Phase 14：`/v1/messages` 接口修复 ✅ 已完成

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 40 | ✅ 完成 | 复用 `resolve_upstream_params` + Key 校验对齐 | 2026-07-11 |
| Stage 41 | ✅ 完成 | 流式 SSE 格式转换（OpenAI→Anthropic） | 2026-07-11 |
| Stage 42 | ✅ 完成 | SpendLog api_key/user_id 修复 + 错误码修正 | 2026-07-11 |
| Stage 43 | ✅ 完成 | stream_options include_usage + 流式 token 计数 | 2026-07-11 |

参见 `docs/plans/2026-07-11-v1-messages-fix-plan.md`

### Phase 15：第二轮反馈改进 ✅ 已完成

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 44 | ✅ 完成 | Models 页面 Cost 列 | 2026-07-11 |
| Stage 45 | ✅ 完成 | Spend Logs 抽屉完整内容 + CSV 导出 + 布局优化 | 2026-07-11 |
| Stage 46 | ✅ 完成 | aigw-migrate --skip-columns / --skip-body 选择性迁移 | 2026-07-11 |

参见 `docs/plans/2026-07-10-phase-14-feedback-round-2.md`

### Phase 16：Playground 增强 ✅ 已完成

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 47 | ✅ 完成 | Playground Virtual Key 配置 + Endpoint Type 选择 | 2026-07-11 |
| Stage 48 | ✅ 完成 | Playground Clear Session + Get Code（curl/SDK） | 2026-07-11 |
| Stage 49 | ✅ 完成 | Playground Markdown 渲染 + 气泡边框 + 底部统计栏 | 2026-07-11 |

参见 `docs/plans/2026-07-11-phase-16-playground-enhancement.md`

### Phase 17：代理转发架构重构（P1）

> **背景**: `chat.rs` 和 `v1_messages.rs` 各自独立 resolve upstream，逻辑重复 ~230 行；`DefaultAdapter` 写死单一实现；`provider_registry`/`router_state` 在 `AppState` 中定义但从未使用。需要先重构架构再继续功能增强。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 50 | ✅ 完成 | **ModelResolver + Deployment** — 新建 `deployment.rs` + `resolver.rs`，迁移 `resolve_upstream_params` 为 `ModelResolver::resolve() → Vec<Deployment>`，替换 chat.rs 调用点。TDD: UT 覆盖查表/解密/credential/env fallback。门禁：全量 BDD 回归通过 | 后端+测试 | 4h |
| Stage 51 | ✅ 完成 | **MessageAdapter + tool 转换** — 拆分 adapter trait 为 `MessageAdapter` + `StreamAdapter`，实现 `OpenAIPassthrough` + `AnthropicToOpenAI`（含 tool_use/tool_result ↔ tool_calls 双向转换），新增 `select_adapter()`。TDD: UT 覆盖 4 种转换方向 + 流式 tool chunk。BDD: /v1/messages 含 tool_use 场景 | 后端+测试 | 5h |
| Stage 52 | ✅ 完成 | **Handler 瘦身** — chat.rs / v1_messages.rs 通用逻辑下沉，handler 只做：校验→resolve→adapt→upstream call→spend log。清理死代码。门禁：全量 UT+BDD+前端测试回归 | 后端+测试 | 3h |

**依赖关系**: Stage 50 → 51 → 52（串行，渐进式重构）。预估 12h。

**TDD 要求**: 每个 Stage 先写测试（UT + BDD scenario），RED → GREEN → REFACTOR 循环，测试全部通过后才可 commit。

**设计文档**: `docs/plans/2026-07-13-arch-refactor-plan.md`

**新增核心组件**:

| 组件 | 命名 | 职责 |
|------|------|------|
| 模型解析层 | `ModelResolver` | model_name → `Vec<Deployment>`（查 proxy_models、解密、解析 credential、提取定价） |
| 消息格式转换 | `MessageAdapter` trait | OpenAI Chat ↔ Anthropic Messages 双向转换（含 tool_use/tool_result ↔ tool_calls） |
| 上游配置 | `Deployment` | 纯值对象：api_base / api_key / upstream_model / provider_type / 定价 / raw_params（解密后完整 litellm_params） |
| 流式转换器 | `StreamAdapter` trait | SSE chunk 逐块转换（`&mut self` 维护跨 chunk 状态如 tool_use index） |

### Phase 18：Spend Logs & Usage 质量修复（P0）✅ 已完成

> **背景**: Spend Logs 页面时间过滤器和 Usage 页面有 4 个已确认的 bug（详见 `docs/14-spend-logs-usage-bugs.md`），依赖 Phase 17 Handler 瘦身完成后执行。

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 53 | ✅ 完成 | **时间过滤 + Usage 当天数据修复** — 前端 `presetRange()` 改用 `toISOString()`；后端 `query_activity_*` 改用 `date(start_time) >= date(?)`；UTC 日期统一 | 2026-07-17 |
| Stage 54 | ✅ 完成 | **end_user 提取 + requester_ip + CopyButton** — `metadata.user_id` → end_user；JSON 解析 session_id；X-Forwarded-For → requester_ip；useCopyToClipboard hook + CopyButton 组件 | 2026-07-17 |

**依赖关系**: Stage 53 → 54 无硬依赖，可并行；依赖 Phase 17 完成。预估 11h。

**TDD 要求**: UT 先行（RED → GREEN → REFACTOR），BDD feature 补充验收。门禁：全量 UT+BDD+前端测试回归通过。

**设计文档**: `docs/14-spend-logs-usage-bugs.md`

---

### Phase 19：UI Enhancement — Models CRUD + Spend Logs 可视化

**背景**: Models 页面仅有只读列表，缺少增删改查交互（后端 CRUD 已就绪）；Spend Logs 抽屉中 Prompt/Response 以 raw JSON 展示，难以阅读。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 55 | ✅ 完成 | **Models 管理页面完整 CRUD 前端** — 结构化表单（model_name 即 model_group；上游 model 自动跟随 model_name 可编辑；API Key / Credential 二选一 + credential 下拉 + 新建快捷入口；每百万 token 美元定价输入 → 自动转换 per-token 价格；编辑预填反向转换）。TDD: BDD 覆盖创建/编辑/删除/上游联动/auth 切换/定价转换 6 个 scenario × 3 viewports | 前端+BDD | 7-8h | 2026-07-16 |
| Stage 56 | ✅ 完成 | **Spend Logs Prompt/Response 结构化可视化** — 新建 MessageViewer（system/user/assistant/tool 按 role 气泡化）+ ResponseViewer（文本回复/tool_calls/usage/finish_reason）+ DetailDrawer Tab 切换（Prompt/Response/Raw）+ 各 Tab 独立复制按钮 + CopyButton 组件（Copy→Check 反馈动画）。TDD: BDD 覆盖结构化消息/tool_calls 折叠/Raw tab/复制按钮/no-data 占位 5 个 scenario × 3 viewports | 前端+BDD | 7-8h | 2026-07-16 |

**依赖关系**: Stage 55 / 56 无硬依赖，可并行。

**设计文档**: `docs/plans/2026-07-15-phase-19-20-roadmap.md`

---

### Phase 20：Spend Logs 可观测性 — 过滤器增强 + Overhead 评估 + 修复

**背景**: model 过滤器为文本框（不可直观选择）；model_group/custom_llm_provider/model_id 始终为 None（bug）；session_id 有数据但无过滤 UI；user_agent/device_id 缺失；proxy_server_request 始终为 None（无法评估网关 overhead）。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 57 | ✅ 完成 | **下拉过滤器 + model_group 修复 + UA/device_id** — 修复 chat.rs/v1_messages.rs 中 4 个 SpendLog 构造点写入 model_group/custom_llm_provider/model_id；新增 distinct-models/sessions API；Model/Session 过滤器改为 searchable Select；User-Agent 头提取写入 metadata.user_agent；device_id 从 metadata.user_id JSON 解析。TDD: UT 覆盖 model_group 写入/UA 提取/device_id 解析/distinct 查询；BDD 覆盖下拉过滤/UA 展示 4 个 scenario × 3 viewports | 前后端+BDD | 7-8h | 2026-07-16 |
| Stage 58 | ✅ 完成 | **Gateway Overhead 评估与展示**（对齐 litellm）— handler 入口写入 proxy_server_request（url/method/headers/arrival_time）；计算 queue_time；adapter 层记录 upstream_timing（sent_at/first_byte_at/ended_at）；计算 gateway_overhead_ms = total - upstream - queue；前端 TimingBreakdown 水平 bar 可视化。TDD: UT 覆盖 proxy_server_request 写入/queue_time/overhead 计算/adapter timing；BDD 覆盖 timing breakdown/旧日志降级 4 个 scenario × 3 viewports | 前后端+BDD | 7-8h | 2026-07-16 |

**依赖关系**: Stage 57 / 58 无硬依赖，可并行。

**Phase 19 + 20 合计**: 28-32h。

**设计文档**: `docs/plans/2026-07-15-phase-19-20-roadmap.md`

---

### Phase 21：协议兼容性修复 — System Message Normalization + Tool Results

**背景**: Claude Code 实际使用中发现 2 个协议兼容性 bug：(1) 多 tool_result 仅保留第一个，并行工具调用上下文丢失；(2) Anthropic→OpenAI 多 system 消息未归一化，Qwen 系列上游 400 拒收。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 59 | ⏳ 待开始 | **Multi tool_result Discard 修复** — `claude_message_to_openai` 返回值改为 `Vec<ChatMessage>`；tool_result 迭代全部生成多条 `role="tool"` 消息。TDD: 5 UT | 后端+测试 | 4h |
| Stage 60 | ⏳ 待开始 | **System Message Normalization（全栈）** — `ChatTemplateCompat` 枚举 + 嗅探 + 折叠算法；Deployment 增 `chat_template_compat`；前端 ModelDialog 增下拉。TDD: 8 UT + 3 BDD × 3 viewports | 后端+前端+测试 | 8h |

**依赖关系**: 都修改 `adapter.rs` 但不同函数，可并行。

**Phase 21 合计**: 12h。设计文档: `docs/plans/2026-07-16-phase-21-23-roadmap.md`

---

### Phase 22：Anthropic 原生上游适配（LT-Native）

**背景**: `select_adapter` 对 `ProviderType::AnthropicNative` 返回 `None → 400`。需补全 `AnthropicPassthrough`（Anthropic→Anthropic 直通）和 `OpenAIToAnthropic`（OpenAI→Anthropic 转换）。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 61 | ⏳ 待开始 | **AnthropicPassthrough + OpenAIToAnthropic** — 两个新 struct 实现 `MessageAdapter` + `StreamAdapter`；`AnthropicPassthroughStream` 透传，`OpenAIToAnthropicStream`（OpenAI SSE→Anthropic event 方向）。TDD: 10 UT | 后端+测试 | 8h |
| Stage 62 | ⏳ 待开始 | **select_adapter 扩展 + Handler 对接 + 全量回归** — 加两个 arm；MockUpstream 扩展 Anthropic 原生端点；BDD 新增 4 scenarios（直通+转换 × 流式/非流式）。门禁: 93→97 BDD | 后端+测试 | 6h |

**依赖关系**: 61 → 62 串行。

**Phase 22 合计**: 14h。设计文档: `docs/plans/2026-07-16-phase-21-23-roadmap.md`

---

### Phase 23：Router 负载均衡

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 63 | ✅ 完成 | **Schema Repair + Router Core** — Migration 去掉 UNIQUE INDEX；Router struct (SimpleShuffle + cooldown + failure tracking)；10 UT | 后端+测试 | 8h | 2026-07-16 |
| Stage 64 | ✅ 完成 | **三级 router_settings + API + 前端** — GET/PUT /router/settings + PATCH key/team；merge_router_overrides；前端独立页面；4 UT | 全栈+测试 | 8h | 2026-07-16 |

**依赖关系**: 63 → 64 串行。

**Phase 23 合计**: 16h。设计文档: `docs/plans/2026-07-16-phase-21-23-roadmap.md`

---

### Phase 24：管理控制台完善

**背景**: 侧边栏缺少 SETTINGS 分组；Router Settings 仅 Global Tab 不完整；Credential 后端 CRUD 已有但缺少前端管理页；Health 页面代码存在但路由/侧边栏未注册。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 65 | ✅ 完成 | **SETTINGS 分组 + Router 三 Tab + Models 多 Tab + Credential 前端 + Health Tab** | 前端+测试 | 5h | 2026-07-16 |

**Phase 24 合计**: 5h。独立 Stage，无后端变更（所有 API 已就绪）。

---

### Phase 25：健康检查 & UX 优化 ✅ 已完成

**背景**: litellm 有 `LiteLLM_HealthCheckTable` + `/health/latest` 用于模型健康检查（纯手动触发）；Usage 页面布局松散、图表只显示费用；Spend Logs 缺少 status/token 过滤器。

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 66 | ✅ 完成 | **健康检查 + Usage 重构 + Spend Logs 过滤** — `health_checks` 表 + `POST /model/health-check` ping + `GET /health/latest` + HealthTab 前端；Usage 布局紧凑化 + 图表 Spend/Tokens/Requests Tab 切换 + 增强 tooltip；Spend Logs 新增 status（All/Success/Failure/Streaming）+ token 范围过滤 | 2026-07-17 |

**Phase 25 合计**: 7h。独立 Stage，全栈完成。

---

### Phase 26：可观测性 (Observability) 🔄

**背景**: 对齐 litellm PrometheusLogger（14 指标）+ OTEL traces（5 层 span）+ Spend Logs Body 字段分离。

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 67 | ✅ 完成 | **Prometheus Metrics** — 14 指标（Counter/Histogram/Gauge），namespace `aigw`，`GET /metrics` 端点，handler 注入（chat.rs + v1_messages.rs） | 2026-07-16 |
| Stage 68 | ✅ 完成 | **OTEL Traces 链路追踪** — W3C traceparent 提取/注入，5 层 span（chat_completions/auth/resolve/adapt/upstream_call），tracing-opentelemetry bridge，OTEL exporter 配置化（config.yaml `general_settings.otel`），`otel_active` 标志位禁用时零开销。10 UT 覆盖 extract/inject/config。handler 中 `tracing::info_span!` 5 层 span + record 属性 | 2026-07-23 |
| Stage 77 | ✅ 完成 | **Spend Logs Body 字段分离** — `/spend/logs` 和 `/global/spend/logs` 永久移除 `messages`/`response`；新增 `GET /global/spend/logs/{request_id}` 详情端点；前端抽屉按需 fetch body，Skeleton 加载 + error/retry 状态。UT + BDD 全覆盖 | 2026-07-23 |

**依赖**: 67 已完成，68 和 77 独立可并行，均已完成。

**Phase 26 合计**: 17h ✅ 全部完成。3 Stage（67 ✅, 68 ✅, 77 ✅）。

---

### Phase 27：全栈质量修复 + Usage 页面图表增强 ✅

**背景**: 用户反馈 6 类问题：(1) model_group 语义错误 — 记录为上游模型名而非部署名称；(2) 无 HTTP 层重试机制；(3) requester_ip 手动解析需标准化；(4) Models/Keys/Users 页面表格有缺陷；(5) Usage 页面缺少 token/request 堆叠分解和 Top Keys/Models 排行榜；(6) Spend Logs 未展示客户端 IP。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 69 | ✅ 完成 | **后端质量修复 + Usage 数据增强** — model_group 语义修正（→ model_name）+ reqwest-retry HTTP 层重试 + axum-client-ip 中间件 + query_activity_daily 8 字段扩展 + aggregate_spend_by_keys + GET /global/spend/keys/rankings。TDD: 9 UT + 2 BDD。闭环：后端 API 就绪，可直接 curl 验证所有端点 | 后端 | 8h | 2026-07-22 |
| Stage 70 | ✅ 完成 | **前端页面修复** — Models: Provider 用 custom_llm_provider + 截断 + Status toggle；Keys: Expires 列 + Status toggle + Expires 写入 create/edit form；Users: User ID 列 + CopyButton + virtual_keys_count（含后端 user.rs 子查询）；Spend Logs: requester_ip 列。TDD: 1 UT + 8 BDD × 3 viewports。闭环：4 页面独立可测，可逐页验收 | 全栈 | 8h | 2026-07-22 |
| Stage 71 | ✅ 完成 | **Usage 页面图表增强** — Daily Trend token (prompt/completion) + request (success/failed) 堆叠 bar；Top Virtual Keys 排行榜卡片（排名 + 迷你进度条 + spend/tokens/requests Tab）；Top Models Chart/Rank 双模式切换；图表 Tab 状态独立化 + 响应式布局调整。TDD: 5 BDD × 3 viewports。闭环：Usage 页面功能完整，可独立验收 | 前端 | 8h | 2026-07-22 |

**依赖关系**: Stage 69（数据层 + 端点）→ Stage 70（表格修复，依赖 API）和 Stage 71（图表，依赖新端点）。70 和 71 可并行。

**Phase 27 合计**: 24h，3 Stages。

**设计文档**: `docs/stages/stage-69.md`, `docs/stages/stage-70.md`, `docs/stages/stage-71.md`

**关键决策**:
- model_group → proxy_models.model_name（对齐 litellm）
- 重试 → reqwest-middleware + reqwest-retry HTTP 层，单条 spend_logs
- 客户端 IP → axum-client-ip 中间件
- Top Keys → LEFT JOIN virtual_keys ON token

**Phase 25-27 总汇总**:

| Phase | Stages | 工时 | 主题 |
|-------|--------|------|------|
| 25 | 66 | 7h | 健康检查 & UX 优化 |
| 26 | 67-68 | 12h | 可观测性 (Metrics + Traces) |
| 27 | 69-71 | 24h | 全栈质量修复 + Usage 图表增强 |
| **合计** | **3 Stages** | **~43h** | |

---

### Phase 28：安全与质量加固 ⏳

**背景**: 代码审计发现的 4 个安全/质量问题：OptionalClientIp 无 fallback、requester_ip_address 不序列化、/router/settings 无鉴权、前端 401 不跳转。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 72 | ✅ 完成 | **安全与质量加固** — Part A: `OptionalClientIp` 三层 fallback (X-Forwarded-For → X-Real-IP → ConnectInfo) + `requester_ip_address` JSON 序列化修复；Part B: `/router/settings` 4 handler 加 `SpendAuth` + `require_admin`；Part C: 前端 `handleResponse` 检测 401 → 全局事件 `auth:unauthenticated` → `RequireAuth` 自动重定向。TDD: 16 UT + 10 BDD。三个子任务可并行 | 全栈+测试 | 16h | 2026-07-23 |

**依赖**: 无。Part A/B/C 修改不同文件，可并行开发。

**Phase 28 合计**: 16h，1 Stage。✅ 完成

**设计文档**: `docs/stages/stage-72.md`

---

### Phase 29：Cross-DB BDD Hardening ✅ 完成

**背景**: `GET /global/spend/keys/rankings` 在 PostgreSQL 部署报错 `column "vk.key_alias" must appear in the GROUP BY clause`（commit `29168b5` 已修复）。根因 SQL 的 `SELECT vk.key_alias` 不在 `GROUP BY` —— SQLite/MySQL 宽松只在 PG 暴露。这暴露一个系统性缺口：mock BDD 默认跑 SQLite（`bdd.rs:46` `sqlite::memory:`），**无法发现跨 DB SQL 方言差异**。已有 DB 层 testcontainers 回归测试，但接口层（路由/鉴权/HTTP 响应）无多 DB 覆盖。

**调研**（见各 stage-7X 附录 A）：13 个 spend 接口中 4 个零 BDD 覆盖（`/spend/users`、`/spend/tags`、`/global/spend/activity`、`/global/spend/keys/rankings`），后两个方言代码最多、风险极高。按"每 Stage 8-16h"拆成 4 个 Stage，复用现成 `bdd-real-pg/mysql/sqlite` task 基础设施，端到端覆盖 11/13 spend 接口。

| Stage | 状态 | 目标 | 类型 | 预估 | 完成日期 |
|-------|------|------|------|------|----------|
| Stage 73 | ✅ 完成 | **基础设施 + keys/rankings** — 提取 `pub(crate)` helper + 封装可复用 `real_db_seed` 灌数据工具（供 74-76 复用）；新增 `@real_api @needs_upstream_db` 场景覆盖 `/global/spend/keys/rankings`（唯一 LEFT JOIN，极高，已修）；红→绿复现 42803。SQLite/PG/MySQL 三 DB 一致 | 后端+测试 | 10h | 2026-07-23 |
| Stage 74 | ✅ 完成 | **activity 覆盖** — `/global/spend/activity`（方言代码量全模块第一，三 DB 占位符 `$N`/`?` + 日期转换 `CAST AS CHAR`/`::TEXT`/`DATE()` + `build_activity_filter` 动态过滤，零覆盖，极高）；metadata 7 字段 + daily 分组 + user_id/team_id 过滤三 DB 一致 | 后端+测试 | 12h | 2026-07-23 |
| Stage 75 | ✅ 完成 | **models + providers 覆盖** — `/spend/{models,providers}` + `/global/spend/{models,providers}`（GROUP BY，PG 版日期 `::TIMESTAMPTZ` 内联 vs SQLite/MySQL bind，高）；重点验证日期过滤三 DB 一致 + 空 provider 兜底 unknown | 后端+测试 | 10h | 2026-07-23 |
| Stage 76 | ✅ 完成 | **SUM 聚合簇 + 应用层 keys** — `/spend/{keys,users,tags}` + `/global/spend` + `/global/spend/keys`；重点 `/spend/users` 的 `"user"` 引号列名 + `/spend/tags` 的 LIKE 转义（三 DB 差异，零覆盖，高）；红→绿验证引号/cast | 后端+测试 | 12h | 2026-07-23 |

**依赖**: Stage 73（基础设施）→ 74/75/76（可并行，均复用 73 的 `real_db_seed`）。

**Phase 29 合计**: 44h，4 Stage。✅ 完成。覆盖 11/13 spend 接口（全部 5 聚合高风险 + 2 极高）；明细 logs 低风险不纳入。

**设计文档**: `docs/stages/stage-73.md` ~ `stage-76.md`

**关键决策**:
- 方向选 B（打真实服务器）而非改 mock 多 DB 化 —— 复用 `bdd-real-*` task，不改 mock SQLite 快测
- 灌数据走 `SourcePool::execute_raw` + `time_literal()` 跨方言，不写方言分支
- Stage 73 先建 `real_db_seed` 可复用工具，74-76 直接复用，避免重复造轮子
- 每 Stage 8-16h：73=10h、74=12h、75=10h、76=12h
- 拆分维度按方言风险聚类：73(LEFT JOIN/极高) → 74(activity/极高) → 75(GROUP BY/高) → 76(SUM+引号/高)

---

## 已完成 Phase 回顾

### Phase 0：项目基础设施

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 0 | ✅ 完成 | RDD 初始化、章程编写、代码基线建立、表名决策、双向迁移策略 | 2026-07-03 |

### Phase 1：数据兼容（核心基础）

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 1 | ✅ 完成 | Schema 100% 对齐（11 张表，SQLite/MySQL/PostgreSQL）+ aigw-migrate 双向迁移工具 | 2026-07-03 |
| Stage 2 | ✅ 完成 | Key API CRUD + SpendLog 读写 + /spend/* 端点 | 2026-07-03 |

### Phase 2：功能对等

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 3 | ✅ 完成 | Chat Completions /v1/chat/completions + /v1/models + Router + Budget/Rate Limit | 2026-07-03 |

### Phase 3：接口规范化

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 4 | ✅ 完成 | OpenAPI 3.1 规范 + Swagger UI + 前端控制台技术选型与规划 | 2026-07-03 |

### Phase 4：部署就绪

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 5 | ✅ 完成 | Docker 化 + Docker Compose + 自托管部署文档 | 2026-07-03 |
| Stage 6 | ✅ 完成 | 云服务 SaaS 架构支持（鉴权网关 + 多实例 + 数据隔离） | 2026-07-03 |

### Phase 5：最小化后端完整版 + BDD 测试（RGR 驱动）

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 7 | ✅ 完成 | BDD 框架搭建 + 既有功能 .feature | 2026-07-04 |
| Stage 8 | ✅ 完成 | 模型管理 CRUD（BDD 驱动） | 2026-07-04 |
| Stage 9 | ✅ 完成 | Provider 适配转换层（BDD 驱动） | 2026-07-04 |
| Stage 10 | ✅ 完成 | Claude /v1/messages 端点 + SSE Streaming（BDD 驱动） | 2026-07-04 |
| Stage 11 | ✅ 完成 | Usage 用量查询增强（BDD 驱动） | 2026-07-04 |
| Stage 12 | ✅ 完成 | BDD 全量覆盖 + 集成测试体系 | 2026-07-05 |

### Phase 7：生产 litellm 迁移到 aigw

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 13 | ✅ 完成 | credentials 表 + CredentialsStore + 全量 Store PG/MySQL 补齐 | 2026-07-06 |
| Stage 14 | ✅ 完成 | NaCl 加密/解密 Rust 库 + aigw-migrate PostgreSQL 源 + master_key 提取 | 2026-07-06 |
| Stage 15 | ✅ 完成 | aigw-migrate 全量迁移（解密 litellm → 重加密 aigw）+ 端到端验证 | 2026-07-06 |
| Stage 16 | ✅ 完成 | aigw 运行时解密 + 凭证引用解析（litellm_credential_name） | 2026-07-07 |
| Stage 17 | ✅ 完成 | 生产迁移 SOP + pre-check 预检 + rollback.sh 回滚脚本 | 2026-07-08 |

### Phase 8：生产化基础

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 18 | ✅ 完成 | 结构化日志 — tracing + tracing-subscriber + JSON 格式 + request_id | 2026-07-08 |
| Stage 19 | ✅ 完成 | 多租户管理 API — /org/* /team/* /user/* CRUD（15 端点，BDD 驱动） | 2026-07-08 |
| Stage 20 | ✅ 完成 | 健康检查增强 — /health/metrics（DB 连接池、uptime、key/model 计数） | 2026-07-08 |

### Phase 9：前端管理控制台

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 21 | ✅ 完成 | 前端工程搭建 — Vite + React + shadcn/ui + rust-embed 集成 | 2026-07-08 |
| Stage 22 | ✅ 完成 | Key 管理页面 — 列表/搜索/创建/编辑/删除/复制 API key | 2026-07-08 |
| Stage 23 | ✅ 完成 | 用量 Dashboard — 支出卡片 + 图表 + spend logs 表格 + 日期筛选 | 2026-07-08 |
| Stage 24 | ✅ 完成 | Model 管理页面 — proxy_models 列表 + 详情展开 | 2026-07-08 |

### Phase 11：前端质量加固 + 安全达标

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 25 | ✅ 完成 | 前端 BDD 测试基础设施 — Playwright + Gherkin + 截图/GIF + Mock API | 2026-07-08 |
| Stage 26 | ✅ 完成 | 登录安全对齐 Litellm — `/v2/login` JWT + Cookie + scrypt + 数据库用户认证 | 2026-07-08 |
| Stage 27 | ✅ 完成 | 移动端适配 — 全页面响应式改造 | 2026-07-08 |
| Stage 28 | ✅ 完成 | Key 创建 UX 修复 — Token 展示对话框 + 复制确认 + 一次性提示 | 2026-07-08 |
| Stage 29 | ✅ 完成 | 用户/组织/团队管理前端页面 | 2026-07-08 |
| Stage 30 | ✅ 完成 | Dashboard 数据接入 + 移动端图表 | 2026-07-08 |

### Phase 12：前端导航重构 + Playground（对齐 litellm）

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 31 | ✅ 完成 | 侧边栏分组重构 + Usage 重命名 — litellm 5 组结构 | 2026-07-08 |
| Stage 32 | ✅ 完成 | Spend Logs 独立页面 — 日期筛选 + 移动端 card list + 30s 自动刷新 | 2026-07-09 |
| Stage 33 | ✅ 完成 | Playground Chat 调试页 — 模型选择 + System/User 消息 + Temperature/MaxTokens + Streaming + SSE mock | 2026-07-09 |

### Phase 13：前端反馈改进 + SSE Streaming + TTFT

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 34 | ✅ 完成 | SSE Streaming + completion_start_time + Spend Logs 增强（分页+request_id+TTFT） | 2026-07-10 |
| Stage 35 | ✅ 完成 | daily_spend 聚合表迁移 + 定时写入 | 2026-07-10 |
| Stage 36 | ✅ 完成 | 前端 Spend Logs 重构（Live Tail+时间预设+分页+细节抽屉） | 2026-07-10 |
| Stage 37 | ✅ 完成 | Users/Orgs 端到端修复 + Provider 解密 | 2026-07-10 |
| Stage 38 | ✅ 完成 | Usage 聚合端点 + 前端 Global 视图重构 | 2026-07-10 |
| Stage 39 | ✅ 完成 | Playground 聊天式多轮对话升级 | 2026-07-10 |

---

## 长期路线

| ID | 主题 | 优先级 | 触发条件 |
|----|------|--------|---------|
| LT-Redis | Redis 缓存 + 性能优化 | P2 | QPS > 1000 |
| LT-SSO | SSO/OAuth 鉴权 | P3 | 企业客户需求 |
| LT-PG | PostgreSQL 生产级支持 + 迁移工具 | P2 | 多实例 + 高可用 |
| LT-K8s | Kubernetes Operator + Helm Chart | P3 | 云原生客户需求 |
| LT-BodyCompact | Body Archive 日 compaction（合并 24 小时文件为日文件） | P2 | 小时文件碎片过多 |
| LT-BodyLifecycle | S3 生命周期自动删除（90 天自动过期） | P2 | 冷数据积累 > 100GB |
| LT-BodyMetrics | Body Archive 监控指标（Prometheus） | P2 | 生产运维需要 |

> **已消化**: LT-Router → Phase 23, LT-Native → Phase 22, LT-Usage → Phase 27, LT-CrossDB → Phase 29, LT-BodyArchive → Phase 30

### 状态图标说明

- ✅ 完成 - Stage 已完成所有验收标准
- 🔄 进行中 - Stage 正在开发中
- ⏳ 待开始 - Stage 尚未开始
- ❌ 已取消 - Stage 被取消

---

## 修订记录

| 版本 | 日期 | 修订内容 |
|------|------|----------|
| v1.0-v14.0 | 2026-07-03~11 | 初始版本 ~ Phase 17 规划 |
| v15.0 | 2026-07-14 | **架构重构规划**：修正 Phase 14-16 状态为已完成；移除旧 Stage 50-51（Usage 多视角聚合移入长期路线）；Phase 17 替换为代理转发架构重构（Stage 50-52: ModelResolver + MessageAdapter + Handler 瘦身）；每个 Stage 内置 TDD+BDD 测试；Stage 51 新增 tool_use/tool_calls 双向转换 |
| v16.0 | 2026-07-15 | **Spend Logs & Usage 质量修复规划**：Phase 17 Stage 50-52 已全部完成，状态更新为 ✅；新增 Phase 18（Stage 53: 时间过滤+Usage 当天数据修复，Stage 54: end_user 提取+复制按钮反馈），共 2 Stage，预估 11h |
| v17.0 | 2026-07-16 | **Phase 19-20 完成 + Phase 21 规划**：Phase 19-20 (Stages 55-58) 全部完成（Models CRUD、Prompt 可视化、过滤器、Overhead）；新增 Phase 21（Stages 59-60，共 2 Stage，预估 12h）：Multi tool_result 修复、System Message Normalization。总进度 58/60 |
| v18.0 | 2026-07-16 | **Phase 21-23 拉通规划**：新增 Phase 22（Stages 61-62, Anthropic 原生上游, 14h）+ Phase 23（Stages 63-64, Router 负载均衡, 16h）。总进度 58/64，消化 LT-Native + LT-Router。6 Stage 细节文档就绪：`stage-59~64.md` |
| v19.0 | 2026-07-16 | **Phase 23 完成 + Phase 24 规划**：Stages 63-64 完成；新增 Phase 24（Stage 65，管理控制台完善, 5h）：SETTINGS 分组 + Router Settings 三 Tab + Models 多 Tab + Credential 管理前端 + Health Tab 集成。总进度 64/65。|
| v20.0 | 2026-07-21 | **Phase 27 规划（第二版）**：合并为 3 Stage（69-71），每个 Stage 8h 闭环。Stage 69 后端质量修复+数据增强（model_group 修正+重试+IP中间件+Daily分解+Top Keys端点）；Stage 70 前端页面修复（Models/Keys/Users/SpendLogs 表格补全）；Stage 71 Usage 图表增强（堆叠 bar+Top Keys/Models 排行榜）。消化 LT-Usage。
| v21.0 | 2026-07-22 | **Stage 69 完成**：model_group 语义修复、reqwest-retry HTTP 重试、axum-client-ip 提取器、Daily trends 8 字段扩展、Top Keys 聚合端点。总进度 69/71，Phase 27 进度 1/3。|
| v22.0 | 2026-07-22 | **Phase 27 全部完成**：Stage 70 前端页面修复（Expires 表单字段、virtual_keys_count 子查询）+ Stage 71 Usage 图表增强（堆叠 bar、Top Keys/Models 排行榜）。总进度 71/71。✅ Phase 27 闭环交付。|
| v23.0 | 2026-07-22 | **Phase 28 规划**：新增 Phase 28（Stage 72，安全与质量加固, 16h）：OptionalClientIp 三层 fallback + requester_ip_address 序列化修复 + /router/settings 鉴权加固 + 前端 401 自动跳转。3 子任务可并行。设计文档：`docs/stages/stage-72.md`。总进度 71/72。|
| v24.0 | 2026-07-22 | **Phase 29 规划（待命，4 Stage）**：起因 `/global/spend/keys/rankings` 在 PG 报错（commit `29168b5` 已修复），暴露 mock BDD 跑 SQLite 无法发现跨 DB 方言差异。调研 13 个 spend 接口（4 个零覆盖）后按"每 Stage 8-16h"拆 4 Stage：73 基础设施+keys/rankings(10h)、74 activity(12h)、75 models+providers(10h)、76 SUM 簇+应用层(12h)，共 44h 覆盖 11/13 接口。复用 `bdd-real-pg/mysql/sqlite` task，仅文档就绪待实施。设计文档：`stage-73~76.md`。|
| v25.0 | 2026-07-23 | **Stage 77 规划 + 进度同步**：新增 Stage 77（Spend Logs Body 字段分离，5h）；修正 roadmap：Stage 72（安全加固）和 Stage 73-76（Cross-DB BDD）已通过 commit `263e1b0` 和 `4bbacb7` 完成但 roadmap 未更新，一并修正为 ✅。总进度 76/77。设计文档：`docs/stages/stage-77.md`。|
| v26.0 | 2026-07-23 | **Stage 68 完成**：OTEL Traces 链路追踪 — tracing-opentelemetry 0.33 bridge + `tracing_subscriber::registry()` 组合层 + `tracing::info_span!` 5 层 span（chat_completions/auth/resolve/adapt/upstream_call）+ W3C traceparent 提取/注入改造 + `config.yaml` `general_settings.otel` 配置化 + `otel_active` 标志位零开销禁用 + 10 UT 覆盖 extract/inject/config/deserialization。总进度 77/77。|
| v27.0 | 2026-07-23 | **Stage 77 完成**：Spend Logs Body 字段分离 — DB 层 `get_spend_log_by_request_id` (trait + 3 后端 impl + Database dispatch)；`GET /global/spend/logs/{request_id}` 详情端点；列表接口移除 `messages`/`response`；前端 `SpendLogDetail` 接口 + on-demand fetch + Skeleton + error/retry。4 UT + 7 BDD (4 后端 + 3 前端)。Phase 26 全部完成，全部 77/77 Stages ✅。|
| v28.0 | 2026-07-24 | **Phase 30 规划**：新增 Phase 30（Stages 78-81，Body Archive 冷存储，共 46h）：78=DB Schema+Core Archiver(12h)、79=Query Router+Footer Cache(10h)、80=Admin API+Col Chunk Cache+存量归档(14h)、81=前端管理页面(10h)。按需求对齐：不需要独立 CLI（admin API 覆盖存量归档）；日 compaction/监控指标推迟；写→读→API→前端串行交付。设计文档：`stage-78~81.md` + `docs/plans/2026-07-22-body-archive-s3-parquet.md`。总进度 77/81。|
| v29.0 | 2026-07-25 | **Phase 31 规划 + Phase 30 生产审计**：用户实测发现 8 问题，三路 subagent 并行审计（后端 AsyncTask+Engine / 后端 BodyArchive / 前端 Jobs UI）确认 Phase 30 代码已落地但未达生产预期——6 P0 + 10 P1 + 12 P2 缺陷。Phase 30 标记为 ⚠️ 待修复，修复转入 Phase 31。**工作量下调**：用户反馈原 4 Stage/50h 偏高 2-5 倍，按 subagent 并发实测 + 同触文件合并，收敛为 3 Stage/24h：82=后端正确性全栈(状态机+配置失联+假阳性completed+冷回源+并发安全+retry+schema，10h)、83=读路径+缓存激活+凭证+FS后端(6h)、84=前端生产化重构(8h)。**每个 Stage 强制 TDD 红绿循环（先写失败测试跑红→重构至绿）+ BDD + real BDD 三后端实际执行验证，发现错误及时修复**。设计文档：`stage-82~84.md`。总进度 77/84（Stage 78-81 编码完成但待修复验收）。|
| v30.0 | 2026-07-27 | **Stage 82 完成**：恢复 dangling commit 链 f6089fd（含 Stage 78-81 + Stage 82 P0 修复）到 feat/body-archive，rebase --onto + cherry-pick HEAD 的 6 个 BDD/migrate 修复。实现：mark_job_running/failed/partially_failed 三态 + storage_configured 门禁 + 配置单例化 + 冷回源端点 + create_job/claim 事务化 + fail_step 退避。验证：aigw-core lib 247/247、Stage 82 单测 18/18（`stage82_state_machine.rs`）、mock BDD 169（含 async_task 15 + admin_jobs 12）、三后端 real BDD 全绿。drive-by：migration 021 `body_archived` BOOLEAN（PG/MySQL）、`JobLogEntry.id` i64。Phase 31 进度 1/3。总进度 78/84。|
| v31.0 | 2026-07-27 | **Stage 83 完成**：读路径 + 缓存激活 + 凭证安全 + FileSystem 后端。实现 `query_parquet_with_cache`（parquet `async`+`object_store` feature，`ParquetObjectReader`+`ArrowReaderMetadata::load_async` 拉 footer，`FooterCache` 命中跳过，4 列投影 stream）；`read_body_from_storage` 区分 NotFound→`Ok(None)` vs 不可达→`Err`；`resolve_env_placeholders` 解析 `${ENV_VAR}`；`build_object_store_for_backend` 接 `LocalFileSystem::new_with_prefix`；`write_parquet_to_store` 改 async 去掉 `block_in_place`。TDD 10 测试红→绿（`stage83_read_path.rs`）；mock BDD 176（161 pass / 15 skip）；real BDD 36/36 × sqlite/pg/mysql 全绿。aigw-core lib 247 + Stage 82 18 不变。Phase 31 进度 2/3，总进度 79/84。|
| v32.0 | 2026-07-27 | **Phase 32 规划**：新增 Phase 32（Stage 85，request_id → call_id 改名 + 上游对账链路打通，8h）。核心预期：任意 SpendLog 能用上游 request_id 与 provider 对账，无论成功还是 4xx/5xx 失败。基于设计文档 `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md`（v5，5 轮评审迭代定稿）。**单 Stage 不拆**：工作量 ~6h 低于 8h 下限，强耦合串行无并行收益。v5 增量：失败路径 4xx/5xx 也提取并存储上游 id（覆盖对账盲区）。**三处不改边界**：HTTP 层 `tower_http::request_id`（§2.2）+ 对外协议响应体 `request_id`（§6.3）+ litellm 源端 SQL（§4.5）。设计文档：`stage-85.md`。总进度 79/85。|
