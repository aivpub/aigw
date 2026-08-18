# aigw — AI Gateway Stagemap

**项目**: aigw (litellm Rust 最小兼容替代)
**最后更新**: 2026-08-18

---

## 当前状态

- **当前 Phase**: **Phase 50 ✅ 完成（Stage 122-125）**。Phase 51（Stage 126-130，Claude OAuth 订阅反代）规划完成待实施。
- **状态**: **129/134 Stages 交付（Phase 50 全部完成）**。Stage 122-125（2026-08-18）：proxies 表 CRUD + 出口/质量检测 + ProxiesPage 前端 + real BDD 收尾。验证：aigw-core 475 + aigw-server 154 UT、mock BDD **265（252 pass / 13 skip）**、real BDD **53/53 × 3**（sqlite/pg/mysql）、fe-bdd 372（369 pass / 3 skip）。详见 `docs/stages/stage-122.md` ~ `stage-125.md`。
- **下一里程碑**: Phase 51（Stage 126 凭证交换 → 127 三层自愈 → 128 反代管线 → 129 前端 → 130 收尾）;中期 M1 guardrails / M2 Redis 分布式层。

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
Phase 30:   ████████████████████ 100% (4/4 Stages) ✅ Body Archive 冷存储（Stage 78-81 生产化后回写）
Phase 31:   ████████████████████ 100% (3/3 Stages) ✅ Body Archive 生产化（Stage 82-84 全部完成）
Phase 32:   ████████████████████ 100% (1/1 Stage)  ✅ request_id→call_id 改名 + 上游对账链路（Stage 85）
Phase 33:   ████████████████████ 100% (1/1 Stage)  ✅ aigw↔aigw 多表只读增量同步（Stage 86）
Phase 34:   ████████████████████ 100% (1/1 Stage)  ✅ 售后对账链路收尾（Stage 87）
Phase 35:   ████████████████████ 100% (2/2 Stages) ✅ Core Entity Soft-Delete（Stage 88-89）
Phase 36:   ████████████████████ 100% (1/1 Stage)  ✅ Upstream Cache Detection & Billing（Stage 90）
Phase 38:   ████████████████████ 100% (3/3 Stages) ✅ UI 多语言 i18n 支持 (Stage 91-93)
Phase 39:   ████████████████████ 100% (4/4 Stages) ✅ Budget Reset 周期任务 + 配置
Phase 40:   ████████████████████ 100% (3/3 Stages) ✅ BDD Coverage Enhancement (Stage 98-100)
Phase 41:   ████████████████████ 100% (2/2 Stages) ✅ OpenAI Responses API 接入 (Stage 101-102)
Phase 42:   ████████████████████ 100% (3/3 Stages) ✅ Playground 多模态图片 (Stage 103-105)
Phase 43:   ████████████████████ 100% (3/3 Stages) ✅ Image Token Usage Tracking (Stage 106-108)
Phase 44:   ████████████████████ 100% (3/3 Stages) ✅ OpenAI Embeddings API 代理 (Stage 110-112)
Phase 45:   ████████████████████ 100% (3/3 Stages) ✅ 技术债清理 (Stage 113-115 全部完成)
Phase 46:   ████████████████████ 100% (1/1 Stage)  ✅ 静态配置模型接入 (Stage 116)
Phase 47:   ████████████████████ 100% (3/3 Stages) ✅ A 类接线 + exact-match 缓存 (Stage 117-119)
Phase 48:   ████████████████████ 100% (1/1 Stage)  ✅ GLM5 流式 tool_use 首帧修复 (Stage 120)
Phase 49:   ████████████████████ 100% (1/1 Stage)  ✅ 上游模型停用功能接线 (Stage 121)
Phase 50:   ████████████████████ 100% (4/4 Stages) ✅ 代理服务管理 (Stage 122-125)
```

---

## Phase 50：代理服务管理 ✅（Stage 122-125 全部完成，44h）

**背景**: sub2api 的代理管理（CRUD + 出口/质量检测）已生产验证，是 Claude OAuth 反代的底座。aigw 当前零代理能力——reqwest 客户端无代理配置、无 proxies 表、无检测端点。参考实现调研：`docs/research/2026-08-18-sub2api-proxy-oauth-reference.md`；总体规划：`docs/plans/2026-08-18-claude-oauth-reverse-proxy.md`。

**核心预期**: 系统配置中新增代理服务管理——增删改查 + 代理出口检测（IP/国家/延迟）+ 代理质量检测（多目标 + CF challenge 识别 + 分数/等级），检测快照落 `probe_result` JSON。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| 122 | ✅ 完成（2026-08-18） | **后端 CRUD** — Migration 027 ×3 建 `proxies` 表（整串 proxy_url 加密落库）+ `Proxy` model + db store ×3 方言 + `/admin/proxies/*` 路由 + in-use 守卫（credentials JSON 扫描 proxy_id）+ proxy_url 加密/redact + 创建异步探测预留。TDD: 5 core UT + 3 handler UT + proxies.feature | 后端+测试 | 10h |
| 123 | ✅ 完成（2026-08-18） | **出口+质量检测** — 出口探测（经代理 GET ip-api/ipify）+ 质量目标（openai/anthropic/**claude_oauth(CF challenge 检测)**/gemini/grok）+ 计分/等级（100−warn×10−fail×22−challenge×30）+ 创建/更新异步自动探测 + probe_result 快照 + reqwest `socks` feature + /test /quality /toggle。TDD: 5 core UT + 2 handler UT + proxies.feature 扩展 | 后端+测试 | 12h |
| 124 | ✅ 完成（2026-08-18） | **前端** — ProxiesPage（Settings 分组 /dash/proxies）：表格（出口 IP·国家/延迟/分数等级/状态/到期）+ 创建/编辑对话框 + Test/Quality 按钮 + 逐项展开 + 批量操作 + toggle + i18n 全量。TDD: 6 BDD × 3 viewports | 前端+测试 | 10h |
| 125 | ✅ 完成（2026-08-18） | **收尾** — real BDD 三后端 proxies CRUD + in-use + 快照 + ADR-033 + roadmap/next-steps 回写 | 全栈+文档 | 4h |

**依赖关系**: 122 → 123 → 124（串行）;125 收尾依赖全部。**Phase 51 强依赖本 Phase**（凭证绑代理 + 交换走代理出口 + 反代代理出口 + claude_oauth 质量目标）。

**关键决策**（ADR-033）:
- **整串 `proxy_url` 加密落库，不拆细字段**（用户决策）——reqwest 原生消费；AES-GCM `v2:gcm:` 复用现有 crypto.rs；密码随串加密优于 sub2api 明文。
- **检测快照收单 JSON 字段 `probe_result`**——status 顶层列仅用于过滤；admin 列表内存解析足够。
- **质量检测加 `claude_oauth` 目标**——生产实测 CF 拦截高频；探测 claude.ai/api/organizations 最敏感路径，CF 签名命中 → challenge。
- **不做过期回退**（fallback_mode/backup_proxy_id/expiry_warn_days）——登记长期路线。

---

## Phase 51：Claude OAuth 订阅反代 🔄（规划待实施，Stage 126-130，50h）

**背景**: Anthropic OAuth 凭证（`sk-ant-sid` 订阅 cookie）打 `/v1/messages` 必须 `system[0]` 是 billing 块或身份句，否则 429 拒（身份 gate 实测）。凭证管理需支持 cookie 换 token、绑定代理 IP、模型解析到 OAuth 凭证时经代理出口以 Bearer access_token 访问、默认注入 billing header。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 126 | ⏳ 待开始 | **凭证 + Cookie→Token 交换引擎** — credentials 表 `credential_values` 扩展 OAuth 结构化字段（access/refresh/session_key 加密落库 + proxy_id/inject_prompt/org_uuid 明文）+ `claude_oauth.rs` 3 步交换（PKCE S256，经绑定代理）+ 敏感字段加密/redact + `POST /credential/oauth/exchange` + mock Anthropic OAuth 上游。TDD: 6 core UT + 3 handler UT + claude_oauth.feature | 后端+测试 | 12h |
| Stage 127 | ⏳ 待开始 | **Token 生命周期 + 三层自愈** — 内存缓存 → 临期(3min)刷新 → refresh 失效回退存储 cookie 重走 3 步自愈 → cookie 也失效 needs_reauth + alert_webhook 告警;管线 401 强制刷新重试;进程内锁防并发刷新。TDD: 8 core UT + claude_oauth.feature 扩展 | 后端+测试 | 10h |
| Stage 128 | ⏳ 待开始 | **反代管线** — resolver/Deployment OAuth 识别（type==anthropic_oauth）+ 统一上游 /v1/messages + **billing 块注入（默认最小化，指纹字节对齐 sub2api/Parrot）** + inject_prompt 追加 + CC 伪装头（UA/Stainless/anthropic-beta）+ 代理出口 + chat/responses 转换接线 + count_tokens + embeddings 400 + 401 刷新重试。TDD: 7 core UT + claude_oauth.feature 扩展 | 后端+测试 | 14h |
| Stage 129 | ⏳ 待开始 | **前端** — CredentialsTab OAuth 入口（粘贴 sk-ant-sid + 代理下拉 + inject_prompt + 交换）+ 状态徽章（active/needs_reauth）+ token 到期 + Refresh/Re-auth 按钮 + 敏感字段 redact + i18n。TDD: 6 BDD × 3 viewports | 前端+测试 | 8h |
| Stage 130 | ⏳ 待开始 | **收尾 + 安全审计** — real BDD 三后端 OAuth 凭证 CRUD + in-use + 快照;安全审计 8 项（cookie/token 加密落库、响应/日志 redact、proxy_url 加密）+ ADR-034 + roadmap/next-steps 回写 + 长期路线追加 | 全栈+文档+安全 | 6h |

**依赖关系**: 126 → 127 → 128（串行）;129 依赖 126-128;130 收尾依赖全部。**强依赖 Phase 50**（凭证绑代理 + 交换走代理出口 + 反代代理出口 + claude_oauth 质量目标）。

**关键决策**（ADR-034）:
- **最小化 billing 块默认注入**（用户决策）——`system[0]` = `x-anthropic-billing-header: cc_version={ver}.{fp}; cc_entrypoint=cli;`，0 token 成本、服务端剥离;凭证可配 inject_prompt 追加。
- **三层 token 自愈**（用户决策）——access(8h)+refresh(30 天轮换)+cookie 三者都存;临期优先 refresh，refresh 失效回退 cookie 自愈，cookie 失效 → needs_reauth + 告警;进程内锁 + 内存缓存（分布式锁推迟 M2 Redis）。
- **全协议统一反代**（用户决策）——任何入站协议（messages/chat/responses/count_tokens）解析到 OAuth 凭证即统一走反代管线;非 OAuth 部署原样不动;embeddings → 400。
- **凭证存 `credentials` 表（零新表）**——proxy_models 经现有 `litellm_credential_name` 引用;resolver 判定 type==anthropic_oauth。
- **TLS 指纹模拟推迟**（用户决策）——HTTP 层伪装已够初步可用;uTLS/rquest 登记长期路线。

---

## 当前 Phase 详情

### Phase 38：UI 多语言 i18n 支持（中文 + English）✅ 已完成

**背景**: 当前 aigw 前端所有 UI 文本硬编码为英文，无任何 i18n 框架、翻译文件或语言切换机制。项目使用 React 19 + TypeScript + Vite + Tailwind CSS v4 + Radix UI primitives，UI 组件为自建 shadcn/ui 风格。需增加中英双语支持，浏览器可持久化语言选择，后端可配置默认语言。

**核心预期**: 用户可在浏览器切换中/英文，选择自动持久化到 localStorage；首次访问通过 `navigator.language` 自动检测浏览器语言；语言检测链：localStorage → navigator.language → 'en'。

**拆分**: 3 Stage（91 框架 12h + 92 全量翻译 20h + 93 切换器+E2E+收尾 10h），共 42h。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 91 | ✅ 完成 | **i18n 框架 + 浏览器语言检测 + 持久化** — react-i18next + i18next + i18next-browser-languagedetector 安装配置；翻译文件骨架（zh-CN.json/en.json，命名空间结构）；i18next 同步初始化（localStorage → navigator.language → 'en' 两级 fallback）；Sidebar + LoginPage 首批改造验证。TDD: 3 BDD 场景 | 前端 | 12h |
| Stage 92 | ✅ 完成 | **全量页面文本提取 + 中英翻译** — 13 页面 + Layout 改造中 Header/Usage 页面（硬编码→`t('key')`）；en.json + zh-CN.json 全部翻译条目补全（14 命名空间 ~250 keys）。TDD: 全量 BDD 回归 273 pass | 前端+翻译 | 20h |
| Stage 93 | ✅ 完成 | **语言切换器 + E2E 验收 + 文档收尾** — Header 语言下拉（DropdownMenu + Lucide Languages 图标 + 中/EN 切换）；`<html lang>` 属性同步；Playwright BDD i18n-switcher.feature 3 场景 × 3 viewports；文档收尾（roadmap/next-steps/ADR-023/tech-debt TD-008）。TDD: 3 BDD + 全量回归 273 pass | 前端+测试+文档 | 10h |

**依赖关系**: Stage 91 → 92（翻译依赖框架就绪）；Stage 92 → 93（语言切换器依赖翻译完成）。

**Phase 38 合计**: 42h，3 Stages。纯前端 Phase，零后端变更。

**设计文档**:
- `docs/stages/stage-91.md`（i18n 框架 + 浏览器持久化）
- `docs/stages/stage-92.md`（全量翻译 + 页面改造）
- `docs/stages/stage-93.md`（语言切换器 + E2E + 收尾）

**关键决策**:
- **i18next 而非 FormatJS**：React 生态事实标准，Tailwind/shadcn 项目常用。
- **单 JSON 文件命名空间**：初期文本量 < 500 keys，打包成本忽略不计，懒加载未必要。
- **管理员配置默认语言推迟**：首次访问通过 `navigator.language` 自动检测已覆盖 95%+ 场景，后续 Phase 需要时在 Router Settings 页加下拉即可。
- **zod schema 不在定义时翻译**：语言切换需动态响应，render 时 `t()` 更安全。
- **通用 UI 组件不改**：`components/ui/*` 保持纯净，文案由调用方传入。

### Phase 39：Budget Reset 周期任务 + 配置 ✅ 已完成（2026-08-02）

**背景**: budgets 表 + 四实体表的 spend/max_budget/budget_duration/budget_reset_at 列 Stage 1 就 schema 对齐但从未实现周期 reset。2026-07-30 深入调研后重写 Phase 39：新增 Stage 94 补实体 spend 写入基础（原规划缺失），Stage 95 并入配额层级约束（施工防历史债务），多级 BudgetEnforcer 集中在 Stage 97。详见 `docs/research/2026-08-01-budget-reset-architecture.md` 和 `docs/08-autonomous-decisions.md` ADR-024。

**核心预期**: 每次请求完成后异步事务更新所有关联实体的 spend；所有 daily_*_spend 维度正确写入；配置写入时强制执行 child.max_budget ≤ parent.max_budget 约束；周期点后 spend 自动清零、budget_reset_at 自动滚动；请求时逐级检查 key→user→team→org。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 94 | ✅ 完成 | **后端** — entity spend 异步增量更新 + DB 层 increment_*_spend × 3 方言 + daily_spend 全5维度 + 失败路径 team_id/org_id 修复 + NaN 防御。TDD: ~22 UT + 6 BDD + real BDD 三后端 | 后端 | 12h |
| Stage 95 | ✅ 完成 | **后端** — duration 解析 + BudgetResetter AsyncTask + 批量 reset × 3 方言 + Budget CRUD + **配额层级约束**（写入时校验 child.max_budget ≤ parent.max_budget）+ backfill + Engine + config。TDD: ~22 UT + 9 BDD + real BDD 三后端 | 后端+测试 | 20h |
| Stage 96 | ✅ 完成 | **前端** — keys/teams/users/orgs 表单内联 budget_duration 下拉 + soft_budget + 列展示；budget_reset Job Tab 补全。TDD: budgets.feature 8 + jobs 增 3 × 3 viewports | 前端+E2E | 16h |
| Stage 97 | ✅ 完成 | **全栈联调** — 多级 BudgetEnforcer（key→user→team→org 逐级）+ soft_budget 记日志 + 历史用量 team/org 聚合补全 + real BDD 三后端 + ADR-024 + TD-007 | 全栈+测试 | 8h |

**Phase 39 合计**: 56h，4 Stages。全部完成 — Stage 94 ✅ / Stage 95 ✅ / Stage 96 ✅ / Stage 97 ✅。

**补充 Stage 109（2026-08-08 ✅）**: 预算重置 cron 界面重构 — `GET /admin/budget-reset/stats` 端点（per-entity ready/total + preview + last_reset + next_tick_at）+ BudgetResetStatsCard（真实待重置数 / 上次重置 / 诚实倒计时）+ BudgetResetPreview（分实体明细 + 即将重置列表）+ BudgetResetTriggerDialog（范围→预览→确认→跳转）+ job 表 trigger 列本地化 + job-detail formatStepResult budget_reset 分支。TDD: 4 core UT + 2 后端 real BDD + 3 前端 BDD × 3 viewports，全绿。详见 `docs/stages/stage-109.md`。

**设计文档**: `docs/stages/stage-94.md` ~ `stage-97.md` / `docs/stages/stage-109.md` / `docs/research/2026-08-01-budget-reset-architecture.md` / `docs/08-autonomous-decisions.md` ADR-024

### Phase 41：OpenAI Responses API 透明桥接 ✅（2026-08-05，22h）

**背景**: OpenAI 于 2025 年推出 Responses API（`POST /v1/responses`）。`/v1/responses` 上游生态极窄（仅 OpenAI + litellm），绝大多数 provider 只支持 `/v1/chat/completions`。分两个 Stage：Stage 101 先做 Passthrough 让端点可用，Stage 102 加 Responses→Chat 协议转换覆盖所有上游。

**拆分**: 2 Stage（Passthrough 8h + Bridge 14h），共 22h，无前端变更。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 101 | ✅ 完成（2026-08-05，b90f42d） | **POST /v1/responses Passthrough** — 新建 `responses.rs` handler（认证→校验 `input` 字段→上游 `{api_base}/responses`→SpendLog）；新增 `ClientProtocol::Responses` 枚举变体 + `select_adapter` arm（实现直接复用 `ResponsesToChatCompletions`，非计划初稿的 `OpenAIPassthrough`，见关键决策修正）；Usage 字段双 fallback（`input_tokens`/`prompt_tokens`）；流式 SSE 透传 + 两阶段 SpendLog。TDD: 6 UT + 6 BDD | 后端+测试 | 8h |
| Stage 102 | ✅ 完成（2026-08-05，6a3ab61） | **Responses→Chat 协议桥接** — 新增 `ResponsesToChatCompletions` 适配器（`MessageAdapter` + `StreamAdapter`）；请求转换（`input→messages`、`instructions→system`、`max_output_tokens→max_tokens`）；响应转换（`choices→output`、`prompt_tokens→input_tokens`）；流式 SSE 事件映射（`delta.content→output_text.delta`、`delta.tool_calls→function_call_arguments.delta`、`finish+usage→response.completed`）；handler 集成。TDD: 6 UT + 5 BDD（适配器级 UT 未单独拆分，桥接逻辑由 5 个新增 BDD 场景覆盖） | 后端+测试 | 14h |

**依赖关系**: Stage 101 → 102（101 落地端点骨架 + `ClientProtocol::Responses`，102 在此基础上加适配器转换，渐进式交付，独立测试验收）。

**Phase 41 合计**: 22h，2 Stages。✅ 全部完成（2026-08-05 代码落地，2026-08-08 文档回写）。

**关键决策**:
- **先 Passthrough 后 Bridge，分开验收**：两个 Stage 独立可测，101 验证端点→认证→SpendLog 链路正确，102 验证协议转换正确。
- **⚠️ 实现修正——101 未复用 `OpenAIPassthrough`**：Stage 101 落地时 `select_adapter(ClientProtocol::Responses, ProviderType::OpenAICompatible)` 直接接线到 `ResponsesToChatCompletions`（Stage 102 的桥接适配器），而非计划初稿的 `OpenAIPassthrough`。因此非流式 `/v1/responses` 请求实际走桥接路径返回 ChatCompletions 格式。流式路径仍为原始 SSE 透传（`stream_adapter` 未接线，见测试缺口）。
- **显式丢弃字段**: `reasoning`（ChatCompletions 无对应）、`previous_response_id`/`conversation`（需服务端会话）、`web_search_preview`/`code_interpreter`/`mcp` 工具（Stage 102 400 拒绝）。

**测试缺口（已记录）**: ① Stage 102 声称的 19 适配器 UT 未落地——adapter.rs 测试模块 68 个 UT 中无 `ResponsesToChatCompletions` 直测，桥接逻辑仅由 5 个 BDD 场景覆盖；② 流式 SSE 桥接（`ResponsesToChatCompletionsStream`）未接入 handler——responses.rs 流式路径转发原始字节、从不调用 `stream_adapter`，流式 SSE 事件转换实际未被执行路径覆盖（mock 上游亦不返回真实 SSE 帧）。

**设计文档**:
- `docs/stages/stage-101.md`（Passthrough + Bridge 两个 Stage）
- `docs/research/2026-08-04-openai-responses-api-support.md`（调研报告）

### Phase 42：Playground 多模态图片 ✅（2026-08-07）

**背景**: 用户要给 Playground 增加图片能力，让 qwen3.5-vl 等多模态模型在 playground 中识别图片。后端多模态转换已部分就绪（`claude_message_to_openai` 正确生成 `data:{media_type};base64,{data}`；`openai_message_to_claude` 反向有 bug），前端 Playground 仅支持纯文本。分 3 Stage 交付：103 后端适配修复 + 模型模式暴露，104 Playground 图片输入（上传/粘贴/预览），105 图片渲染 + SpendLog 详情增强 + 文档收尾。

**拆分**: 3 Stage（Backend 6.5h + Frontend 16h + Render/Docs 12h），共 34.5h。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 103 | ✅ 完成（2026-08-07） | **多模态适配修复 + 模型模式暴露** — 修 `openai_message_to_claude` image 转换 bug（剥离 `data:` 前缀 + 推导 media_type，malformed fallback image/png）；`ModelEntry` 增 `model_info` 可选字段（master 路径透传 `ProxyModel.model_info` 含 mode，向后兼容）；补多模态后端 BDD（chat/messages/anthropic_native 图片透传/转换 + /v1/models mode 字段 + 详情 body 保留 image）。TDD: 8 UT + 6 BDD | 后端+测试 | 6.5h |
| Stage 104 | ✅ 完成（2026-08-07） | **Playground 图片输入（上传+粘贴+预览）** — `ChatMessage.images: string[]` + 隐藏 file input（accept image）+ 剪贴板 paste → FileReader.readAsDataURL → 预览缩略图条 + 删除；多模态序列化（chat 端点 OpenAI content array `image_url` / messages 端点 Claude content blocks `image.source`）；独立 sessionStorage 持久化（pending 条 reload 恢复）；RASTER_MIME 白名单 + 20MB 单图守卫；+3 i18n keys；新增 /v1/messages mock + 请求体捕获。TDD: 8 E2E 场景 × 3 viewports = 24 执行全绿，全量 frontend BDD 300 pass | 前端 | 16h |
| Stage 105 | ✅ 完成（2026-08-07） | **图片渲染 + SpendLog 详情增强 + 文档收尾** — Playground user 气泡图片缩略图；log-viewer `extractText`/`extractImages` 补 `output_text`/`file`/`text_delta`/`function_call` block + `ImageThumbnails` 共享组件 + `OutputCard` Responses API `output[]` 分支（output_text/image_url/function_call）；SpendLog 详情 3 UT 透传断言（image_url/output_text/Anthropic image block）；ADR-025 Accepted + roadmap v42.1 + next-steps + TD-009e。TDD: 3 UT + 5 E2E × 3 viewports = 15 执行全绿，全量 frontend BDD 312 pass | 全栈+文档 | 12h |

**依赖关系**: Stage 103 → 104（104 发送图片依赖反向转换正确 + `/v1/models` 模式字段）；Stage 104 → 105（105 渲染依赖 Playground 图片数据模型就绪）。

**Phase 42 合计**: 34.5h，3 Stages。

**关键决策**:
- **前端始终用 OpenAI content-parts 或 Claude content blocks**：由 `endpointType` 决定，图片在客户端已读为 base64，不走后端转换。
- **后端只修最小缺口**：`openai_message_to_claude` 的 data URL 解析 + `/v1/models` 暴露 model_info.mode；不新增网关图片校验（litellm 亦无，Playground 客户端负责）。
- **log-viewer 共享组件**：`extractImages` + `ImageThumbnails` 供 Playground 与 SpendLog 详情复用，SpendLog drawer 无需改结构。
- **不按 `model_info.mode` 强制过滤附件**：用户可自由给任意模型发图，由上游裁决（litellm 兼容）。

**设计文档**:
- `docs/stages/stage-103.md` / `stage-104.md` / `stage-105.md`

---

### Phase 43：Image Token Usage Tracking ✅（2026-08-08，28h）

**背景**: 多模态模型的 image token 用量数据分布不均：Qwen/DashScope 返回 `prompt_tokens_details.image_tokens`（最完整），OpenAI/Anthropic 不返回。业界网关（litellm/OpenRouter/OneAPI）均不做 image token 客户端预计算。aigw 填补此缺口：上游优先解析 + 对不返回的 provider 做客户端 fallback 估算。image_tokens 是 prompt_tokens 的子集，不改 calc_spend（仅用于分析与对账）。

**核心预期**: 每条多模态请求的 SpendLog 包含 `image_tokens: Option<i32>`（source 标记 upstream/estimated）+ daily 聚合。

**拆分**: 3 Stage（Core Engine 10h + Handler Integration 10h + Frontend/Docs 8h），共 28h。全部完成 — Stage 106 ✅（`45d7323`）/ Stage 107 ✅ / Stage 108 ✅。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 106 | ✅ 完成 | **Image Token Engine (aigw-core)** — `extract_image_tokens_from_usage()` 上游解析器（Qwen OpenAI-compat + DashScope native）；`calculate_image_tokens()` fallback 估算（OpenAI tiling / Qwen ViT factor 28/32 / Anthropic 官方 ⌈w/28⌉×⌈h/28⌉）；Minimal PNG/JPEG/WebP/GIF header parser（零新增 deps）。Auto-sniff 策略（model name 匹配），不依赖 Deployment 配置。TDD: 18 UT。 | 后端 | 10h |
| Stage 107 | ✅ 完成 | **Handler Integration + SpendLog/DailySpendLog + Migration 025 + BDD** — chat.rs / v1_messages.rs 上游取值优先 + fallback 估算；`image_tokens` 字段写入 + metadata `image_tokens_source` 标记；Migration 025（spend_logs + 6 daily_*_spend 加列，×3 方言）；daily_spend_queue 聚合；5 mock BDD 场景 + mock 上游真实 SSE 流式路径。Qwen 路径永远不触发估算（上游返回值优先）。TDD: 4 handler UT。 | 后端+测试 | 10h |
| Stage 108 | ✅ 完成 | **Frontend Display + Real API BDD + Documentation** — SpendLog 抽屉展示 image_tokens + source badge（✓ upstream / ⚠ estimated）；列表 🖼️ 标记（桌面+mobile）；i18n 3 keys；2 E2E 场景 × 3 viewports；ADR-027 + TD-011 + Roadmap/Next-Steps 收尾。 | 全栈+文档 | 8h |

**依赖关系**: Stage 106 → 107（串行，handler 依赖 engine）；Stage 107 → 108（前端依赖 API 字段就绪）。

**Phase 43 合计**: 28h，3 Stages。✅ 全部完成（2026-08-08）。

**关键决策**:
- **上游优先 + 客户端 fallback**：Qwen/Gemini 直接解析上游返回值（最精确），OpenAI/Anthropic 做客户端估算。
- **不改 calc_spend**：image_tokens ⊆ prompt_tokens，已包含在现有计费中。字段仅用于分析与对账。
- **`image_token_strategy` 不在 Deployment 上**：auto-sniff model name 已经覆盖 >99% 场景。Qwen 的 ViT 公式仅用于验证/对账，handler 中 Qwen 永远不触发估算。
- **Metadata 存 source，不建独立列**：`image_tokens_source: "upstream" \| "estimated"` 存在已有 metadata JSON 中，只在对账时需要溯源，不需要索引。
- **`image_tokens` 字段名（非 `estimated_image_tokens`）**：因为 Qwen 的值是精确上游返回值，不是估算。
- **Anthropic 用官方公式**：stage-107 初稿写"OpenAI 近似"，实现时改为官方公开精确公式（TD-011c 仍登记 downsizing 规则未模拟）。

**设计文档**:
- `docs/plans/2026-08-07-image-token-estimation.md`（总体规划 + 调研修正记录）
- `docs/stages/stage-106.md` / `stage-107.md` / `stage-108.md`
- ADR-027 / TD-011

---

### Phase 44：OpenAI Embeddings API 代理 ✅（2026-08-09，3 Stage，24h）

**背景**: 用户有部分 Embedding 应用流量并想尝试更多，本地 + 托管 embedding 模型都在用。调研确认（`docs/research/2026-08-08-embedding-proxy-support.md`）LiteLLM（aigw 参照实现）把 `/v1/embeddings` 当一等公民端点，走与 chat 完全相同的 auth→budget→rate-limit→spend-log 管道；Kong/Portkey/new-api 均支持，Cloudflare/Helicone/Azure APIM 缺失（leader parity 非普适 table-stakes）。工程成本低：responses.rs 的非流式克隆，`ChatAuth`/`resolve_key_model_list`/`calc_spend`（prompt-only）/`OpenAIPassthrough`/`ModelResolver` 原样复用，零 schema 变更。

**用户决策**: ① 有流量 → 现在交付；② 本地+托管模型 → 薄 OpenAI-compatible Passthrough 覆盖（`openai/` 前缀 → vLLM/BGE/Qwen3）；③ 排在在途 P1 收尾之后 → 编号 Phase 44；④ 四种端点都需要；⑤ health.rs embedding-mode 探测 → 非阻塞（记 TD-012a）。

**拆分**: 3 Stage（Passthrough 后端 10h + 前端/OpenAPI/real BDD 8h + 模型接入/文档 6h），共 24h。全部完成 — Stage 110 ✅（`41d0223`）/ Stage 111 ✅（`4637062`）/ Stage 112 ✅。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 110 | ✅ 完成 | **POST /v1/embeddings Passthrough（四端点）** — 新建 `routes/embeddings.rs`（responses.rs 非流式子集：ChatAuth 认证→校验 model+input（string/array，400）→resolver+router→**硬选 OpenAIPassthrough**（拒绝 AnthropicNative，防 OpenAIToAnthropic 破坏 embedding body）→上游 `{api_base}/embeddings`→非流式透传→SpendLog `call_type="embedding"` + calc_spend(prompt-only)）；注册 4 端点（`/v1/embeddings` + `/embeddings` 走无 Path wrapper，Azure 别名走 `embeddings_handler_with_path` 提取 `{model}` 入 body）；`extract_prompt_tokens`/`extract_total_tokens` 提升 `pub(crate)`；openapi.rs `embeddings_spec()` + expected_endpoints 18→19。TDD: 6 UT + 11 BDD | 后端+测试 | 10h |
| Stage 111 | ✅ 完成 | **Frontend OutputCard `data[]` 分支 + OpenAPI spec + E2E** — `parseOutput` 加 `object=list` + `data[]` 分支（渲染向量维度 + 8 维截断预览 + usage）；i18n 2 keys；mock 加 EMB_SPEND_ROW + sampleDetailEmbedding；spend-logs.feature 2 E2E × 3 viewports；ADR-026 → Accepted + TD-012a/b 登记。TDD: 2 E2E × 3 viewports = 6 执行 | 前端+测试 | 8h |
| Stage 112 | ✅ 完成 | **Embedding 模型接入验证 + 文档收尾** — models.feature +2 场景（`/model/new` 注册 mode=embed 模型 + `/v1/models` 展示 + `/v1/embeddings` 全链路 SpendLog call_type=embedding）；charter L91/L204 已有 `/v1/embeddings`；roadmap + next-steps + ADR-026 + TD-012a/b 收尾。TDD: +2 BDD 场景 | 全栈+文档 | 6h |

**依赖关系**: Stage 110 → 111（前端依赖端点契约）→ 112（文档依赖全部落地）。

**Phase 44 合计**: 24h，3 Stages。✅ 全部完成（2026-08-09，总进度 116/116 — ALL STAGES COMPLETE）。

**关键决策**:
- **薄 OpenAI-compatible Passthrough，不做协议翻译**：`openai/`-前缀覆盖 OpenAI 托管 + vLLM/BGE/Qwen3 本地；不做 Gemini `:embedContent` / Cohere `/v2/embed` 翻译（Envoy 刚合并的差异化层，等真实 RAG 负载再上）。
- **不加 `ClientProtocol::Embeddings` 变体**：embeddings 的 usage 解析复用现有 `extract_prompt_tokens`/`extract_total_tokens`，上游路径固定 `{api_base}/embeddings`，OpenAIPassthrough 透传足够。
- **硬选 OpenAIPassthrough / 拒绝 AnthropicNative**：`select_adapter(OpenAI, AnthropicNative)` → `OpenAIToAnthropic`（adapter.rs L77）会把 embedding body 当 chat 转换产生垃圾。embedding 模型天然 OpenAI 兼容。
- **`call_type="embedding"`**：对齐 litellm SDK 直调 call_type；aigw 全同步无 async 区分，用 `"embedding"` 语义最贴。
- **计费 prompt-only**：usage `{prompt_tokens, total_tokens}`（completion=0），`calc_spend` 直接复用 → `prompt_tokens × input_cost_per_token`。
- **四端点共用同一 handler**：差异仅路径匹配；Azure 别名的 model 取自 path param 并入 body（axum Path<Option<String>> 在无参数路由会 500 → 拆 wrapper + with_path 两个公开 handler）。
- **前端只渲染向量维度**：不渲染完整 1536 维数组，截断 8 维预览 + usage grid。

**验证**：mock BDD 232 passed（2 新增 Stage 112 场景全绿；仅 pre-existing budget_reset next_tick flake）、aigw-server 135 UT、fe-bdd 333 passed（Stage 111 2 场景 × 3 viewports）、fmt + lint green。

**设计文档**:
- `docs/research/2026-08-08-embedding-proxy-support.md`（调研报告）
- `docs/stages/stage-110.md` / `stage-111.md` / `stage-112.md`

---

### Phase 45：技术债清理 🔄（2026-08-09 规划，3 Stage，28h）

**背景**: Phase 44 + 在途 P1 收尾全部完成后，剩余技术债按风险/依赖/现实性排序收敛。核实确认 TD-004 已修复（`b199000`）；TD-011b（HEIC/AVIF）方案变更为前端转码（后端 ISO-BMFF 解析脆弱 + Chrome 不渲染）；TD-011a（视频）重定义为 Playground 视频输入（token 估算留待真实负载）。总体规划：`docs/plans/2026-08-09-tech-debt-cleanup.md`。

| Stage | 状态 | 目标 |类型 | 预估 |
-------|------|------|------|------|
| Stage 113 | ✅ 完成（2026-08-09） | **后端可靠性加固** — TD-005 Async Engine 三 loop panic 容错（`guarded()` futures catch_unwind + 30s backoff）+ `Engine::run_with_cancel` 优雅关闭；TD-010a health embedding-mode 探测（`build_probe_spec` 按 model_info.mode 分支 `/embeddings`）；TD-003 BDD 覆盖率脚本 + `task bdd-coverage`（60% 回归基线，见 stage-113 实现偏差）。TDD: ~10 UT + 1 BDD | 后端+测试 | 8h |
| Stage 114 | ✅ 完成（2026-08-09） | **前端体验** — TD-009a/b Playground 图片压缩（`compressImage` 取「原图 vs 压缩」较小者）+ body 防御（`∑ >24MiB` toast 拒绝）；TD-008a/b i18n 懒加载（en eager + detected-lang eager + zh-CN lazy chunk）+ 翻译 TS 类型（`resources.d.ts` 增广 CustomTypeOptions）。TDD: 3 fe-bdd × 3 viewports + build 分包 | 前端+测试 | 10h |
| Stage 115 | ✅ 完成（2026-08-09） | **多模态精度** — TD-011b HEIC/AVIF 前端转码（`compressImage` 解码失败 → toast）；TD-011c Anthropic downsizing（迭代缩放 ≤1568）；TD-012b 多模态 embedding 按模态计费（`ModalPricing` + `calc_spend_modal`，接线留待真实负载）；TD-011a 视频输入 SKIPPED（可选标记 + 无真实流量）。TDD: 10 core UT + 1 fe-bdd × 3 viewports | 全栈+测试 | 10h |

**依赖关系**: 113 → 114 → 115（无硬依赖，按价值排序串行；Stage 内子项可并行）。

**Phase 45 合计**: 28h，3 Stages（进度 3/3 — 全部完成 ✅）。

**验证（Stage 113）**: aigw-core 409 + aigw-server 136 UT、mock BDD 233 场景（新增 embed 探针场景通过；仅 pre-existing budget_reset next_tick flake）、bdd-coverage 63% PASS、fmt + lint green。ADR-028 Accepted + TD-003/005/010a/012a Resolved。

**验证（Stage 114）**: 3 新 BDD 场景 × 3 viewports = 9/9、i18n-switcher 9/9、全量 fe-bdd 342 pass（11.6m）、fe-build 分包（zh-CN 25kB lazy chunk）、fe-lint（fe-i18n-types + oxlint + tsc）green。ADR-029 Accepted + TD-008a/b + TD-009a/b Resolved。

**设计文档**:
- `docs/plans/2026-08-09-tech-debt-cleanup.md`
- `docs/stages/stage-113.md`（含 Implementation Notes）/ `stage-114.md` / `stage-115.md`
- `docs/stages/stage-113-review-log.md`

---

### Phase 47：A 类接线 + exact-match 缓存（S1+S2+S3）✅（2026-08-10 交付，3 Stage，40h）

**背景**: 差距调研（`docs/research/2026-08-09-aigw-gap-vs-industry-leaders.md`）确认 aigw 最大欠账是 **A 类「代码在但运行时未接线」**——RPM/TPM 限流、多级预算 `check_budget_multi`、soft_budget 告警、`max_parallel_requests`、Router 智能路由（usage/latency/weighted/cooldown/fallback/merge_overrides）全部已实现且有 UT，但请求路径零调用点。已逐条在仓库代码核实（`enforce_limits` 仅 test 调用、`check_budget_multi` 仅 `#[cfg(test)]`、`select_instance`/`merge_router_overrides` 仅测试模块）。**企业采购 demo 一测即穿**。其次 B 类「缓存=0」：exact-match 响应缓存是全部竞品标配（litellm/Portkey/Cloudflare/Higress），aigw 为零。总体规划：`docs/plans/2026-08-10-phase-47-wiring-cache.md`。

| Stage | 状态 | 目标 |类型 | 预估 |
-------|------|------|------|------|
| Stage 117 | ✅ 完成（`d1000b0`） | **A 类接线核心（S1）** — 4 handler 请求入口挂 `check_request_limits`（复用 `enforce_limits`：多级预算 `check_budget_multi` + RPM/TPM `RateLimiter.check`）；soft_budget 命中 → `alerts::AlertDispatcher` webhook（已实现）；`max_parallel_requests` 每 deployment `tokio::sync::Semaphore`；预算 TOCTOU 竞态文档化 + 事务快照读。**TDD**（单元质量）: core 8-10 UT + handler 4 UT。**BDD**（功能验收）: `rate_limit.feature` HTTP 级扩展（RPM/TPM 429 + x-ratelimit 头 + max_parallel）+ `real/multi_level_budget` 去 @skip（SQLite/PG/MySQL）+ `soft_budget.feature` 新建（webhook + hard-reject） | 后端+测试 | 16h |
| Stage 118 | ✅ 完成（`abad4db`） | **Router 智能路由接线（S2）** — `report_failure/report_success` 接上游返回路径（cooldown 真实推进，429/401/408/404/5xx 才计）；`merge_router_overrides` key>team>global 接请求入口；weighted 路由（按 weight/rpm/tpm 加权随机，对标 litellm simple_shuffle）；usage/latency 变体真实决策；错误类型 priority fallback（429/5xx/context-window/content-policy）；前端 RouterSettings 下拉解锁。**TDD**: router 10-12 UT + handler 2-4 UT。**BDD**: `router.feature`（cooldown 排除 + 400 不计数 weighted 命中率 + usage/latency + priority fallback + overrides）+ `router_settings.feature` 扩展 + fe-bdd 下拉解锁 | 全栈+测试 | 14h |
| Stage 119 | ✅ 完成（`ad981b2`） | **exact-match 响应缓存（S3）** — `aigw_core::cache`（`CacheBackend` trait + moka LRU 内存后端预留 Redis）；cache key = SHA-256(provider+endpoint+model+auth+body)；非流式组装后入缓存 + 流式 `stream_chunk_builder` 组装后入缓存；`cache={"use-cache","no-store","ttl"}` 控制 + `X-Cache-Status: HIT/MISS` 头；cache-hit 计费 0 元（复用 `calc_spend` 三级缓存计费）；config `cache` 块 + boot 注入。**TDD**: cache 8-10 UT + handler 2-4 UT。**BDD**: `cache.feature`（MISS→HIT + no-store + TTL 过期 + cache-hit 零成本计费 + 流式重放 + 禁用 no-op） | 后端+测试 | 10h |

**依赖关系**: 117 → 118（118 依赖 117 的 guard/身份上下文）；119 独立（新能力，可并行）。按「接线优先、缓存次之」价值排序串行。

**Phase 47 合计**: 40h，3 Stages（进度 3/3 — 全部完成 ✅）。

**验证（Phase 47）**: aigw-core 432 + aigw-server 145+152 UT（合计 861）、mock BDD **246 场景（233 pass / 13 @skip body_archive / 0 fail）**、real BDD sqlite/pg/mysql **47/47 × 3**、fmt + clippy `-D warnings` green。ADR-032 Accepted。顺带修复：BDD chat 步骤补 request-id layer（UUID-v7 call_id，避免 spend_logs.call_id UNIQUE 冲突）、alerts.rs 测试 flake（`tokio::spawn` 需 reactor → `#[tokio::test]`）。

**后续收尾（✅ 已全部完成）**: 前端 RouterSettings 下拉解锁 usage/latency（`9fe6329`，Stage 118 §3.6）；config `cache` 块解析 + boot 注入（`9fe6329`，Stage 119 §3.5）；max_parallel 从 key/budget 表字段接线（`cada57b`，key→team→org-budget→deployment 层级取最严限制，+4 UT）。

**设计文档**:
- `docs/plans/2026-08-10-phase-47-wiring-cache.md`（总体规划）
- `docs/stages/stage-117.md` / `stage-118.md` / `stage-119.md`

---

### Phase 48：GLM5 流式 tool_use 首帧丢帧修复（S1）✅（2026-08-12 交付，1 Stage）

**背景**: 用户反馈 aigw 转发 GLM5（tokenhub 上游）到 Claude Code 反复出现 `Invalid tool parameters` / `__unparsedToolInput` 错误。前期文档（`docs/16-glm-stream-delta-analysis.md` 首版，2026-07-15）错误地把根因归为 `partial_json` 累积语义差异——本次交叉 Anthropic 官方 SDK 文档确认 `partial_json` **本身就是纯增量碎片**（客户端负责累积），aigw 直接透传语义正确。真正 bug：`AnthropicToOpenAIStream::next` + `OpenAIToAnthropicStream::next` 首个带 `id` 的 chunk 若同时携带 `arguments`（tokenhub GLM-5.2 首帧就是 `id + "{\""`），代码 emit `content_block_start` 后 `return`，**丢掉首帧 arguments**——下游 Claude Code 累积得到的 partial JSON 永远缺开头 `{"`。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 120 | ✅ 完成（`332fa08`） | **首帧丢帧修复** — `AnthropicToOpenAIStream::next` + `OpenAIToAnthropicStream::next` tool_calls 分支：early-return → 本地 buffer 累积 SSE frame，循环末尾统一返回；同 chunk `content_block_start` + `input_json_delta` 两帧一起发出；订正 `docs/16-glm-stream-delta-analysis.md` 五/六节根因。**TDD**: 3 UT（首帧同时含 id+args 正/反向对称 + 后续多个纯 args 增量帧顺序透传，先 fail 后 pass） | 后端+测试 | 4-6h |

**Phase 48 合计**: 4-6h，1 Stage（进度 1/1 — 完成 ✅）。

**验证（Phase 48）**: aigw-core 455 UT（+3 Stage 120）、mock BDD 246 保持基线、real BDD sqlite/pg/mysql 通过（4 例失败均为上游 tokenhub 402 免费额度耗尽外部依赖）、前端 fe-lint pass、fmt + clippy `-D warnings` green。

**设计文档**: `docs/stages/stage-120.md`；根因订正：`docs/16-glm-stream-delta-analysis.md` §五/六（2026-08-12 版）。

---

### Phase 49：上游模型停用功能接线（S1）✅（2026-08-13 交付，1 Stage）

**背景**: 用户反馈"上游模型停用功能完全无效"。调研确认（`docs/research/2026-08-13-model-disable-audit.md`）：前端 UI Switch 有开关，切「停用」时 `PUT /model/update {model_info.mode:"inactive"}`——但**后端零处消费**该字段，SQL 只按 model_name 过滤，`ModelResolver.resolve` 不看 mode，Deployment 结构无 disabled 字段，Router 只按 cooldown 过滤。同一 `model_info.mode` 还兼载业务类别（"embed"/"image"），语义污染。方案 B（推荐）落地：独立 `enabled: bool` 列 + 3 端 SQL 加过滤 + resolver 兜底 filter + `/model/update` 接受 `enabled` 参数 + 前端切换到 `model.enabled`。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 121 | ✅ 完成 | **停用功能接线** — Migration 026 三端 `ALTER TABLE proxy_models/deleted_models ADD COLUMN enabled` (BOOLEAN/INTEGER/TINYINT DEFAULT TRUE)；`ProxyModel` + `DeletedModel` + `UpdateModelRequest` + `ModelResponse` 加 `enabled` 字段；3 端 SQL（SQLite const + PG/MySQL inline）全部加 `enabled` 列，`LIST_MODELS_BY_NAME` 追加 `AND enabled=TRUE`；`ModelResolver::resolve` 加 `.filter(|m| m.enabled)` 防御式兜底；`/model/update` 读 `body.enabled`；前端 `types.ts.ModelItem` 加 `enabled`、`isActive` 从 `model_info.mode` 迁到 `model.enabled`、Switch onChange 调 `{enabled}` 而非 `{model_info.mode}`。**TDD**: 3 UT（resolver 跳过 disabled + 同 name 两 row 只返 enabled + db 层 list_models_by_name 过滤，先 fail 后 pass） | 全栈+测试 | 6-10h |

**Phase 49 合计**: 6-10h，1 Stage（进度 1/1 — 完成 ✅）。

**验证（Phase 49）**: aigw-core 458 UT（+3 Stage 121）、mock BDD 246 保持基线、`task test/fmt/lint/build/fe-lint` 全绿。

**收尾（2026-08-17 ✅）**: 三路 subagent 核实禁用能力完备性 + 补 3 个端到端 BDD 场景（`models.feature`：停用→chat 断言 400 `model_not_found` / 管理列表仍可见 / 重新启用→200）。核实的四个转发入口（chat/v1_messages/embeddings/responses）全部经 resolver 过滤（SQL `AND enabled` + resolver `.filter` 双层），`pick_deployment` 无重查 DB / 无 retry 绕过；`/model/update` 省略 `enabled` 保留原值、新建默认 true、迁移 026 历史行默认启用。**基线更新**: mock BDD **249 场景（236 pass / 13 @skip body_archive / 0 fail）**、fmt + clippy green；real BDD 三端 sqlite/pg/mysql 重跑 **47/47 × 3 全绿**（首跑 pg/mysql 各有 1-4 例 429 为上游 tokenhub 真实限流偶发，重跑转绿）。收尾缺口登记 **TD-014a/b/c**（`@real_api` 三端覆盖缺失 / env 回退兜底仍转发禁用模型名 / config.yaml `model_list` 不支持 `enabled:false`）。

**不做的事**：不清理历史 `model_info.mode` 里的 "inactive"/"disabled" 值（`isActive` 现只看 enabled，历史值惰性容忍）；不引入 `status enum` 状态机（方案 C，长期路线）。

**设计文档**: `docs/stages/stage-121.md`；根因调研：`docs/research/2026-08-13-model-disable-audit.md`。

---

### Phase 36：Upstream Prompt Cache Detection & Differentiated Billing ✅ 已完成

**背景**: 调研确认（`docs/research/2026-07-28-upstream-prompt-cache-detection-and-billing.md`）litellm 对上游 provider 的 prompt caching 有两套解析（Anthropic 顶层字段 + OpenAI `prompt_tokens_details`）和三级差异化计费（regular / cache_read / cache_creation），而 aigw 当前 `calc_spend` 对所有 prompt token 使用同一单价，`Deployment` 不含缓存定价字段。核心目标：补齐上游缓存 token 解析 → 三级计费 → daily 聚合表写入的完整链路。

**拆分**：1 Stage（Stage 90 — 后端全栈，10h），无前端变更。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 90 | ✅ 完成 | **calc_spend 三级缓存差异化计费 + upstream response 缓存 token 解析** — ① `Deployment` + `ResolvedUpstream` 增 `cache_read_input_token_cost`/`cache_creation_input_token_cost` 字段，`extract_pricing()` 从 `model_info`→`litellm_params` 两级 fallback 提取；② `calc_spend` 重构为三级计费（`regular = prompt - cache_read - cache_creation`，分别乘不同单价），fallback 策略：缓存定价缺失时回退 `input_cost_per_token`；③ 流式 & 非流式路径从 `response->usage` 提取 `cache_read_input_tokens`/`prompt_tokens_details.cached_tokens` 和 `cache_creation_input_tokens`/`cache_write_tokens`，Anthropic 归一化（`prompt_tokens += cache_read + cache_creation`）；④ `DailySpendLog` struct 补全缓存字段 + daily_spend_queue 写入；⑤ 10 UT + 2 BDD spec。 | 后端+测试 | 10h |

**依赖关系**: 无前向依赖（独立改动 chat.rs / v1_messages.rs / deployment.rs / calc_spend）。与 Phase 35（Stage 88-89）修改文件无交集，可完全并行。

**Phase 36 合计**: 10h，1 Stage。

**关键决策**:
- **三级计费 fallback 用 input_cost 而非 0**：对齐 litellm `_cost_per_token_custom_pricing_helper`（缓存价格缺失时回退到常规 `input_cost_per_token`），不溢出也不欠费。
- **Anthropic 归一化在调用侧做**：`calc_spend` 签名保持纯粹——传入的 `prompt_tokens` 是已归一的，不做 provider-type 分支。
- **不在 spend_logs 加缓存 token 列**：litellm 也没有，数据已在 `response` JSONB + `daily_*_spend` 汇总表中。
- **daily_*_spend 列已建**（015 migration），本 Stage 仅补 Rust struct 和写入逻辑，无需新 migration。

**设计文档**:
- `docs/stages/stage-90.md`（后端全链路）
- `docs/research/2026-07-28-upstream-prompt-cache-detection-and-billing.md`（调研报告）

---

### Phase 35：Core Entity Soft-Delete ✅ 已完成

**背景**: 仅 `virtual_keys` 有软删除（`deleted_keys` 归档表 + tombstone-then-delete），`teams`/`users`/`organizations`/`proxy_models` 四表全部硬删除，无审计追溯能力。参考 litellm `LiteLLM_DeletedTeamTable` / `LiteLLM_DeletedVerificationToken` 独立归档表模式，扩展 aigw 现有 `deleted_keys` 实现到全部核心实体。

**拆分**：后端全链路 1 Stage（Stage 88 — 迁移 + DB 层 + API + 测试，12h）+ 前端 1 Stage（Stage 89 — 删除确认 + 已删除视图 + E2E，6h）。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 88 | ✅ 完成 | **后端** — `024_deleted_tables.sql` 三方言迁移（4 归档表自增 id PK + 源 ID 索引 + `deleted_at`）；DB 层 4 个 Store trait × 3 方言 tombstone-then-delete 改造（SELECT→INSERT archive→DELETE）+ `list_deleted_*` 方法；4 新增 API 端点（`GET /{entity}/deleted`）；UT + BDD 测试覆盖。 | 后端+测试 | 12h |
| Stage 89 | ✅ 完成 | **前端** — 5 管理页面（keys/teams/users/orgs/models）统一删除确认增强 + "已删除"Tab（Tabs 切换 + 归档表格 + `deleted_at` 列）；Playwright BDD E2E。 | 前端+测试 | 6h |

**依赖关系**: Stage 88 → Stage 89（后端 API 先就绪，前端按接口契约独立开发）。

**Phase 35 合计**: 18h，2 Stages。

**关键决策**:
- **独立归档表而非源表加列**：物理隔离活跃数据与归档数据，查询无需额外 `WHERE deleted_at IS NULL`，对齐 litellm 和现有 `deleted_keys` 模式。
- **归档表 PK 用自增 `id` 而非源 ID**：team_id/user_id 可能被删后重建再删，源 ID 做主键会冲突（`deleted_keys` 的 token hash 天然唯一，其他实体不成立）。litellm 的 `DeletedTeamTable` 正是 `id String @id @default(uuid())`。
- **幂等不报 404**：行不存在时返回 Ok，与 `delete_key` 一致。
- **恢复功能留到后续 Phase**：需冲突处理（源 ID 已存在）、UI 交互、权限考量，独立交付。
- **`deleted_by` 审计列延后**：需 auth middleware 注入用户信息，本 Phase 先建表不加此列。

**设计文档**:
- `docs/stages/stage-88.md`（后端全链路）
- `docs/stages/stage-89.md`（前端 + E2E）
- `docs/plans/2026-07-28-soft-delete-archive-tables.md`（总体规划）

### Phase 34：售后对账链路收尾 ✅ 已完成

**背景**: Stage 85 把 `spend_logs.request_id`（PK）改名 `call_id` + 新增可空 `request_id`（上游 provider id）后，三个实测缺口：(1) 历史迁移行 `request_id` 为 NULL，对账断链——需回填成功行；(2) 列表 `call_id` 在第 8 列不易定位，要放最左；(3) 抽屉只显示 `call_id`，`request_id` 未渲染，两者混淆；(4) 搜索 `?request_id=` 走精确等值，不支持模糊匹配。

**核心预期**: ① 成功历史行 `request_id` 回填为 `call_id`（SQL SOP 文档，不写代码）；② Spend Logs 列表 `call_id` 放最左、抽屉双 id 显著区分、搜索框对 call_id/request_id 模糊匹配。

**拆分**：回填用 SQL SOP 文档（`docs/request-id-backfill-sop.md`，不占 Stage 编号——一行 SQL 三方言通用，写 crate 代码过度工程）；UI + 模糊搜索 1 Stage（Stage 87）。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 87 | ✅ 完成 | **Spend Logs UI 双 id + 双列模糊搜索** — ① 前端列重排（`call_id`/`Upstream ID` 移到 `Time` 之前成最左两列）；② 抽屉双 id Badge 显著区分（call_id 用 `variant=default`、request_id 用 `variant=secondary` + 文字标签，NULL 显灰 `—`）；③ 后端 db.rs 5 处 `=`→`LIKE '%X%'` 模糊匹配（SQLite query/count :1521/:1551、PG 内存 :1828、PG count :1854、PG status_filter :4243，含 LIKE 通配符转义 `ESCAPE '\'`）；④ BDD 3 新场景（call_id 最左列、抽屉双 id、模糊搜索）+ mock 按 query param 过滤。TDD 红绿 + 三后端 real BDD。 | 全栈+测试 | 5h |

**依赖关系**: Stage 87 基于 Stage 85 schema 已落地。回填 SOP 与 Stage 87 解耦，运维可独立执行。

**Phase 34 合计**: 5h，1 Stage + 1 SOP 文档。

**关键决策**:
- **回填不写 CLI 子命令也不占 Stage**：一行 SQL `UPDATE spend_logs SET request_id=call_id WHERE request_id IS NULL AND status='success'` 三方言通用，crate 代码+UT 过度工程；一次性运维操作非产品功能。改为 `docs/request-id-backfill-sop.md` SQL 手册，运维在 DB 执行。
- **成功判定 `status='success'` 精确**：失败/流式/timeout 行无上游 id，保持 NULL 语义正确。
- **回填值用 `call_id`**：历史成功行无真实上游 id，call_id 是最佳对账锚点（次优但可用）。
- **列重排只动桌面表格**：移动端 card 空间有限只显示 call_id，双 id 在抽屉看。
- **模糊粒度子串 `LIKE '%X%'`**：用户要「输半段能搜到」；前缀不够。LIKE 通配符 `%`/`_` 转义防注入。
- **5 处 SQL 必须同时改**：列表（query）与计数（count）不匹配会分页错乱。
- **MySQL 大表循环 YAGNI**：当前数据量未达阈值，SOP 备用不实现。

**设计文档**: `docs/stages/stage-87.md`（UI + 模糊搜索）、`docs/request-id-backfill-sop.md`（回填 SQL 手册）

### Phase 33：aigw↔aigw 多表只读增量同步 ✅ 已完成

**背景**: 用户诉求——在 aigw 内部不同数据库实例之间（PG↔SQLite 任意组合）同步数据，参数范式参考现有 `remote-import`/`remote-export`，支持全表同步或 `--tables` 选子集；`spend_logs` 可按"最近 N 天"增量，其他表全量幂等追加；只读、一次性 CLI。现有 `aigw-migrate` 是 litellm↔aigw **异构**迁移（绑死 litellm 表名/camelCase 列/`call_id←request_id` 重定向），覆盖不了 aigw↔aigw **同构**同步。但底层 `SourcePool`/`CursorRange`/`insert_rows_batch`/`migrate_plain_table` 抽象与 litellm 假设解耦，可复用——只需新写一个不走 litellm-mapping 的上层 `sync` 命令。

**核心预期**: 任意两个 aigw 数据库实例之间（PG↔SQLite 任意组合）能通过一条 CLI 命令，把源库数据同步到目标库——默认全 11 张业务表，也可用 `--tables` 选子集；`spend_logs` 支持"最近 N 天"增量，其他表全量幂等追加；重跑不重复。

**单 Stage 说明**: 改动集中在 `aigw-migrate` crate（cursor 锚点参数化 + 新 `sync` 模块 + CLI 接入 + UT），不动 `aigw-core`，工作量 ~8h。Stage 内分三阶段：① cursor 锚点参数化 + sync 模块骨架 → ② CLI 接入 + `--days`/`--tables` 解析 → ③ TDD 红绿 + 文档。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 86 | ✅ 完成（2026-07-28） | **`aigw-migrate sync` 子命令** — 参数参考 remote-import/remote-export 风格。复用 `SourcePool`/`CursorRange`/`insert_rows_batch`/`migrate_plain_table`；新增 `build_aigw_cursor_sql`（锚点 `start_time`，不动 litellm 的 `build_cursor_sql`）+ `stream_rows_with_cursor_aigw` + `stream_pg_rows_keyset_aigw`（PG keyset 用 `(start_time, call_id)`）；`sync.rs::run_sync`（source/target 任意 PG/SQLite 组合，按 `--tables` 遍历：plain 表全量幂等 + spend_logs 流式时间游标 + insert_rows_batch，空 overrides 同 schema direct-match）。CLI `Sync` 子命令 + `--tables`（默认全 11 张业务表，config 默认排除）+ `--days N`（chrono UTC 转 CursorRange）+ `--resume-after`/`--end-before`/`--skip-body`/`--batch-size` + short alias（-s/-t/-T/-d/-r/-e/-B/-b）。`credentials`/`proxy_models` 直接复制密文（同 master_key，当 plain 处理，不调 migrate_credentials）。**TDD 红绿**：8 UT（全表同步、`--tables` 选子集、`--days 7` 过滤、幂等重跑、`--skip-body`、非法表名报错、config 默认排除+显式不覆盖、DEFAULT_TABLES 契约）。只读追加（`INSERT OR IGNORE`/`ON CONFLICT DO NOTHING`），非常驻。 | 后端+测试 | 8h |

**依赖关系**: 无前向依赖（Stage 85 已让两端 schema 一致）；本 Stage 与 Phase 30/31 Body Archive、长期路线均解耦。

**Phase 33 合计**: 8h，1 Stage。

**关键决策**:
- **参数范式参考 remote-import**: `--source-url`/`--target-url`/`--resume-after`/`--end-before`/`--skip-body`/`--batch-size` 同名；`--step-filter` → `--tables`（按 aigw 表名选，更灵活）；无 master-key 参数（aigw↔aigw 同 key）。
- **只复用底层抽象**: `SourcePool`/`CursorRange`/`insert_rows_batch` 直接用，不碰 `remote_import`/`remote_export` 的 litellm-mapping 路径。
- **锚点参数化而非改原函数**: 新增 `build_aigw_cursor_sql`，保 litellm 迁移零回归。
- **不做列重定向**: aigw↔aigw 同 schema，空 overrides direct-match。
- **加密表直接复制密文**: 同 aigw 集群内共享 master_key；跨 key 场景用 remote-import。
- **config 默认排除**: 含 master_key，避免覆盖目标鉴权；显式 `--tables config` 才同步（INSERT OR IGNORE 不覆盖）。
- **`--days` 用 UTC**: `start_time` 存 UTC，避免本地时区跨天错位。
- **只读追加边界**: 仅 INSERT，不传播 UPDATE/DELETE；非常驻、非 CDC。符合"只读镜像"诉求。

**设计文档**: `docs/stages/stage-86.md`

### Phase 30：Body Archive 冷存储 ✅（2026-08-08 生产化后回写）

**背景**: spend_logs 表 messages/response/proxy_server_request 三个 JSON body 字段占 95%+ 存储体积（日均增长 4-5 GB）。Stage 77 已将 body 从列表接口分离，本 Phase 实现 body 的 S3 Parquet 冷存储归档——主库只保留最近 7 天热数据，历史 body 迁移到对象存储并压缩为 Parquet 列式格式（ZSTD 压缩 8-11x），查询时自动路由热/冷数据。

> **2026-08-08 回写**：代码自 2026-07-27 起落地（Phase 31 Stage 82-84 完成生产化修复），roadmap 保持 ⚠️ 待修复标记直至审计缺陷核实完毕。本次回写前逐条核对 `docs/research/2026-07-25-body-archive-production-audit.md` 全部 28 项缺陷：**6 P0 + 10 P1 全部修复**（状态机、配置单例化、storage_configured 门禁、冷回源、读路径缓存、事务化、前端生产化），**P2 10/12 修复**；剩余 2 项（P2-2 Engine panic 容错、P2-3 无 shutdown 信号）明确登记 TD-005 技术债，不阻塞生产。至此 Phase 30 回写 ✅。

详见设计文档: `docs/plans/2026-07-22-body-archive-s3-parquet.md`

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 78 | ✅ 完成（2026-07-27 落地） | **AsyncTask + Engine 框架 + Body Archiver 写链路** — Migration 020/021（async_jobs 三张通用表 + spend_logs 加列）；AsyncTask trait（tick + execute + finalize）+ Engine（宿主运行时，spawn tick/exec/cleanup loop）；StorageBackend 多类型适配（S3/COS/R2/MinIO/FS）；BodyArchiver impl AsyncTask（Parquet ZSTD + Bloom filter → 目标存储 + 清理器）。TDD: UT 覆盖 claim_next_step SKIP LOCKED、loop 并发、storage config 解析、parquet 写入 | 后端+测试 | 14h |
| Stage 79 | ✅ 完成（2026-07-27 落地） | **Query Router + Footer Cache（读链路）** — query_parquet_with_cache(): footer 缓存（moka LRU）→ row group 定位（column statistics + Bloom filter）→ col chunk 读取 → 解码；get_message_body() 热/冷自动路由；详情端点集成存储 fallback。TDD: UT 覆盖路由逻辑、缓存命中/未命中、Parquet 读路径 | 后端+测试 | 10h |
| Stage 80 | ✅ 完成（2026-07-27 落地） | **Admin API + Col Chunk Cache + 存量归档** — 5 个 admin 端点（trigger/jobs/stats/job_detail/logs）；引擎统计查询（loops/pending/running/stale）；Col chunk 文件系统 LFU 缓存（可选）；通过 API 触发存量归档。TDD: UT 覆盖 admin API 鉴权、Job 生命周期、col chunk 缓存 | 后端+测试 | 12h |
| Stage 81 | ✅ 完成（2026-07-27 落地） | **前端 Jobs 管理页面** — Settings → Jobs Tab，按 step_type 分 Sub-Tab；统计卡片 + 手动触发 + 通用 JobList/JobDetail（Steps 表格 + logs 过滤 + 自动刷新）。TDD: 4 BDD × 3 viewports | 前端+测试 | 10h |

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

> ✅ **生产审计闭环**（2026-07-25 审计 → 2026-08-08 回写）：Phase 30 代码落地后经生产审计确认 6 P0 + 10 P1 + 12 P2 缺陷，修复工作转入 Phase 31（Stage 82-84）。2026-08-08 逐条核对审计报告：6 P0 + 10 P1 全部修复（状态机、配置单例化、storage_configured 门禁、冷回源、读路径缓存、事务化、前端生产化），P2 10/12 修复；剩余 P2-2（Engine panic 容错）/P2-3（无 shutdown 信号）登记 TD-005，不阻塞生产。审计闭环，Phase 30 回写 ✅。

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

### Phase 32：request_id → call_id 改名 + 上游对账链路打通 ✅ 已完成

**背景**: 当前 aigw 把自身 UUID v7 存在 `spend_logs.request_id`（PK，语义=网关调用标识），但行业惯例（含 litellm）中 `request_id` 指上游 provider 返回的请求 ID。导致语义混淆 + 售后对账断链（SpendLog 未存上游 ID，退款/排查无法与 provider 对账）。设计文档经 Gate-2 多模型评审定稿（v6.1，lead 独立 + 3 路 subagent），修正 migrate 映射机制描述、补对外协议字段边界（§6.3）、可观测性影响（§10）、失败路径 4xx/5xx 提取（v5 增量）、migrate override 方向（v6）、失败路径走 INSERT 非 UPDATE（v6.1 §11.2 核心预期关键修正）。

**核心预期**: 任意一条 SpendLog 记录都能用上游 `request_id` 去 provider 侧对上账，无论成功还是 4xx/5xx 失败。改名 `call_id`、流式提取、失败路径提取均为支撑项。

**单 Stage 说明**: 强耦合串行（DB schema 是路由层编译前提、路由层是前端/migrate 前提），收敛为 1 Stage / 8h。Stage 内分三阶段（每阶段独立 git commit）：① DB schema+模型/DB/body_archive 层 → ② 路由层+上游 id 全路径提取+前端+migrate → ③ 测试同步+核心预期 BDD+三端联调。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 85 | ✅ 完成（2026-07-28） | **request_id → call_id 改名 + 上游对账链路打通** — 023 迁移（pg/mysql/sqlite 双重条件幂等 RENAME + ADD COLUMN + 索引，MySQL 索引加前缀长度 128）；models.rs/db.rs/body_archive/daily_spend_queue 字段改名；路由层 chat.rs/v1_messages.rs 流式 chunk id 提取 + **4xx/5xx 失败路径提取**（v6.1 关键：走 INSERT 非 UPDATE）+ Phase 2 UPDATE 调用补 upstream_id（COALESCE 保护）+ Anthropic 流式提取位置修正（choices 分支前 borrow）+ 响应头预提取 request-id；main.rs tracing span 字段改名；spend.rs/openapi API 字段拆分 + URL `/{call_id}`；migrate 注入 `call_id→request_id` override + export 源行剥离 request_id 击败 direct-match；前端 3 interface + 展示列 + CSV + 搜索；body_archive 归档过滤加 `request_id IS NOT NULL`（失败请求跳过归档）。**TDD**：核心预期 2 BDD 场景（双列返回 + 双列搜索）。**验收**：mock BDD 163/163 + aigw-core lib 247/247 + aigw-migrate 27/27（含 override 方向断言）+ frontend build green；PG/MySQL 迁移应用通过 | 全栈+测试 | 8h |

**关键决策**:
- **单 Stage 不拆**：工作量 ~6h 低于 8h 下限，强耦合串行无并行收益，收敛为 1 Stage。
- **核心预期驱动**：所有改动服务"打通上游对账链路"这一唯一业务目标，改名/流式提取/失败路径提取是支撑项（设计文档 §1.3 + §9 顶层决策）。
- **失败路径也提取（v5）**：4xx/5xx 从 error body/响应头提取上游 id，让失败请求也能对账——核心预期覆盖盲区。
- **三处不改边界**：HTTP 层 / 对外协议响应体 / litellm 源端 SQL，否则破坏功能或契约。
- **migrate override 必做**：源端单 `request_id` 对目标 `call_id`+`request_id` 双列，不 override 则 PK 为 NULL 插入失败（设计文档 §4.5）。

**设计文档**: `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md`（v6.1）、`docs/stages/stage-85.md`

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

### Phase 21：协议兼容性修复 — System Message Normalization + Tool Results ✅ 已完成（2026-07-16）

**背景**: Claude Code 实际使用中发现 2 个协议兼容性 bug：(1) 多 tool_result 仅保留第一个，并行工具调用上下文丢失；(2) Anthropic→OpenAI 多 system 消息未归一化，Qwen 系列上游 400 拒收。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 59 | ✅ 完成 | **Multi tool_result Discard 修复** — `claude_message_to_openai` 返回值改为 `Vec<ChatMessage>`；tool_result 迭代全部生成多条 `role="tool"` 消息，text/image parts 单独发 user 消息保留。TDD: 5 UT（单/双/三 tool_result、mixed、empty 边界） | 后端+测试 | 4h | 2026-07-16（`49a5f1c`） |
| Stage 60 | ✅ 完成 | **System Message Normalization（全栈）** — `ChatTemplateCompat` 枚举（Auto/Strict/Loose）+ resolve/sniff + `<system-reminder>` 折叠算法；Deployment 增 `chat_template_compat`；前端 ModelDialog 增下拉。TDD: 8 UT（real body/multi-system/tail/adjacent/no-user fallback/loose/sniff/override） | 后端+前端+测试 | 8h | 2026-07-16（`f385bc0`） |

**依赖关系**: 都修改 `adapter.rs` 但不同函数，可并行。

**Phase 21 合计**: 12h，✅ 完成（2026-07-16）。设计文档: `docs/plans/2026-07-16-phase-21-23-roadmap.md`

---

### Phase 22：Anthropic 原生上游适配（LT-Native）✅ 已完成（2026-07-16）

**背景**: `select_adapter` 对 `ProviderType::AnthropicNative` 返回 `None → 400`。需补全 `AnthropicPassthrough`（Anthropic→Anthropic 直通）和 `OpenAIToAnthropic`（OpenAI→Anthropic 转换）。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 61 | ✅ 完成 | **AnthropicPassthrough + OpenAIToAnthropic** — 两个新 struct 实现 `MessageAdapter` + `StreamAdapter`；`AnthropicPassthroughStream` 透传，`OpenAIToAnthropicStream`（OpenAI SSE→Anthropic event 方向）。TDD: 10 UT | 后端+测试 | 8h | 2026-07-16（`b892fc4`） |
| Stage 62 | ✅ 完成 | **select_adapter 扩展 + Handler 对接 + 全量回归** — 加两个 arm 覆盖 2×2 矩阵；v1_messages/chat handler 动态上游 URL path + Anthropic 头注入（x-api-key + anthropic-version）；MockUpstream 扩展 Anthropic 原生端点；BDD 新增 4 scenarios（适配器选择+直通）。门禁: 93→97 BDD ✅ | 后端+测试 | 6h | 2026-07-16（`b892fc4`） |

**依赖关系**: 61 → 62 串行。

**Phase 22 合计**: 14h，✅ 完成（2026-07-16）。设计文档: `docs/plans/2026-07-16-phase-21-23-roadmap.md`

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
| LT-TLSFingerprint | Claude OAuth TLS 指纹模拟（uTLS/rquest ClientHello 伪装） | P2 | OAuth 反代遭遇 CF 拦截 |
| LT-OAuthMimicry | Claude OAuth 完整伪装链（tool 混淆/dateline 归一化/1h cache TTL/metadata.user_id/完整三块） | P2 | 最小 billing 注入被上游判定第三方 |
| LT-ProxyFallback | 代理过期回退（fallback_mode/backup_proxy_id/expiry_warn_days） | P2 | 代理大量过期需要自动切换 |
| LT-OAuthTokenWarmup | OAuth refresh_token 接近轮换点主动 cookie 预热 | P3 | refresh 轮换失败率上升 |

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
| v33.0 | 2026-07-28 | **Stage 85 完成（Phase 32 ✅）**：Gate-2 多模型评审（lead 独立 + 3 路 subagent：migration / migrate-frontend-tracing-tests / extraction-protocol）发现 v5 设计 3 Critical + 3 High + 4 Medium 缺陷，全部修正至 v6.1。**关键修正**：① 迁移号 022→023（Stage 82 占用 022_next_retry_at）；② migrate import override 方向写反→`overrides["call_id"]="request_id"`（key=target, value=source）；③ **失败路径 upstream_id 走 INSERT 非 UPDATE**（v5 COALESCE-UPDATE 不覆盖失败行，核心预期静默失败）；④ export override 被 direct-match 抢占→源行剥离 request_id；⑤ Anthropic 流式提取位置（choices 分支前 borrow）；⑥ 响应头预提取 request-id；⑦ MySQL 索引前缀长度 128；⑧ body_archive 归档过滤 `request_id IS NOT NULL`（失败请求跳过归档）。实现 023 迁移（pg/mysql/sqlite）+ models/db/body_archive/daily_spend_queue 全链路改名 + 路由层 4 路径上游 id 提取 + migrate override + 前端 3 interface。验证：aigw-core lib 247/247 + aigw-server lib 100/100 + mock BDD 163/163（15 @skip，含新增核心预期 2 场景：双列返回 + 双列搜索）+ aigw-migrate 27/27（含 import override PK 非空断言 + export reverse-override 击败 direct-match 断言）+ frontend build green；PG/MySQL 023 迁移应用通过。总进度 81/85。|
| v34.0 | 2026-07-28 | **Phase 33 规划**：新增 Phase 33（Stage 86，`aigw-migrate sync` 子命令 — aigw↔aigw 多表只读增量同步，8h）。用户诉求：在 aigw 内部不同 DB 实例间（PG↔SQLite 任意组合）同步数据，参数范式参考现有 `remote-import`/`remote-export`，支持全表同步或 `--tables` 选子集；`spend_logs` 按"最近 N 天"增量，其他表全量幂等追加；只读、一次性 CLI。现有 `aigw-migrate` 是 litellm↔aigw 异构迁移，覆盖不了 aigw↔aigw 同构同步；但底层 `SourcePool`/`CursorRange`/`insert_rows_batch`/`migrate_plain_table` 抽象与 litellm 假设解耦可复用。新增 `build_aigw_cursor_sql`（锚点 `start_time`，不改 litellm 的 `build_cursor_sql`）+ `sync.rs::run_sync`（空 overrides 同 schema direct-match）+ CLI `--tables`（默认全 11 张业务表，config 默认排除）+ `--days N`（chrono UTC）。`credentials`/`proxy_models` 直接复制密文（同 master_key）。只读追加（`INSERT OR IGNORE`/`ON CONFLICT DO NOTHING`），非常驻/非 CDC。TDD 7 UT。设计文档：`stage-86.md`。总进度 81/86。|
| v35.0 | 2026-07-28 | **Stage 86 完成（Phase 33 ✅）**：实现 `aigw-migrate sync` 子命令——aigw↔aigw 同构只读增量同步。native.rs 新增 `build_aigw_cursor_sql`（锚点 `start_time`，不改 litellm `build_cursor_sql` 保零回归）+ `stream_rows_with_cursor_aigw` dispatch + `stream_pg_rows_keyset_aigw`（PG keyset 用 `(start_time, call_id)` 而非 `(startTime, request_id)`）。sync.rs 新增 `run_sync` + `SyncStats`/`TableSyncStats` + `ALL_AIGW_TABLES`/`DEFAULT_TABLES`/`SPEND_LOGS_BODY_COLUMNS` 常量 + `parse_tables`/`resolve_tables`/`resolve_cursor`（表名校验、`--days` UTC 转 CursorRange、与显式 `--resume-after`/`--end-before` 取更严边界）。main.rs `Sync` 子命令 + short alias（-s/-t/-T/-d/-r/-e/-B/-b）+ env 回退（`AIGW_SYNC_SOURCE_URL`/`AIGW_SYNC_TARGET_URL`）。空 overrides direct-match（aigw↔aigw 同 schema，不做 `call_id←request_id` 重定向）；`credentials`/`proxy_models` 当 plain 复制密文不调 migrate_credentials；config 默认排除。TDD 8 UT 红绿（`tests/sync.rs`：全表同步/`--tables` 子集/`--days 7` 过滤/幂等重跑/`--skip-body`/非法表名报错/config 默认排除+显式 INSERT OR IGNORE 不覆盖/DEFAULT_TABLES 契约）。验证：`cargo test -p aigw-migrate` 全量通过（27+27+8+1，无回归）+ `aigw-migrate sync --help` 输出表清单。总进度 82/86。|
| v37.0 | 2026-07-28 | **Stage 87 完成 + Phase 35 规划**：Stage 87 ✅（Spend Logs UI 双 id + 模糊搜索全部落地，Phase 34 ✅）。新增 Phase 35（Core Entity Soft-Delete，Stages 88-89，共 18h）：独立归档表模式扩展 teams/users/orgs/models 四表软删除。Stage 88=后端全链路（迁移+DB层+API+测试，12h）；Stage 89=前端（删除确认+已删除视图+E2E，6h）。设计文档：`stage-88.md`、`stage-89.md`、`docs/plans/2026-07-28-soft-delete-archive-tables.md`。总进度 85/88（Stage 87 ✅、Stage 88-89 待开始）。|
| v38.0 | 2026-07-28 | **Phase 36 规划**：新增 Phase 36（Upstream Prompt Cache Detection & Differentiated Billing，Stage 90，10h）。基于 `docs/research/2026-07-28-upstream-prompt-cache-detection-and-billing.md` 调研结果——上游 provider 缓存 token 解析 + calc_spend 三级差异化计费 + Deployment 缓存定价字段 + daily_*_spend 缓存列写入。单 Stage 纯后端，与 Phase 35 并行。设计文档：`stage-90.md`。总进度 83/89（Stage 90 待开始）。|
| v39.0 | 2026-07-30 | **Phase 37 规划**：新增 Phase 37（Budget Reset 周期任务 + 配置，Stages 91-93，共 40h）。基于 docs/research/2026-07-30-budget-reset-gap.md 调研——budgets 表 + 四实体表 budget 列 Stage 1 就 schema 对齐但从未实现周期 reset，budget_duration/budget_reset_at 字段被写入却不消费。复用 Stage 82-84 的 AsyncTask+Engine 框架新增 BudgetResetter（step_type=budget_reset，tick 扫过期记录批量 UPDATE spend=0 + 标准化对齐重算 reset_at）。3 Stage：91 后端（duration 解析+resetter+Budget CRUD+backfill+config，16h）、92 前端（实体表单内联 budget_duration/soft_budget+Jobs Tab 补全，16h）、93 全栈联调（soft/hard 双轨+real BDD 三后端+收尾，8h）。soft_budget 告警通道登记 TD-007。设计文档：stage-91~93.md + plans/2026-07-30-budget-reset-phase-37.md。总进度 89/92（Stage 91-93 待开始）。 |
| v40.0 | 2026-08-02 | **Phase 39 完成（Stage 94-97 ✅）**：Budget Reset 完整交付（56h，4 Stage，8/1~8/2）。核心成果：① entity spend 异步事务增量更新（key/user/team/org 四实体 × 3 方言） + daily_spend 5 维全量补全 + 失败路径 team_id/org_id 修复（Stage 94）；② BudgetResetter AsyncTask（标准化对齐 UTC 0 点/周一/月初）+ 配额层级约束写入校验（child.max_budget ≤ parent.max_budget）+ backfill（Stage 95）；③ 前端 entity 表单 budget_duration 下拉 + soft_budget + Job Tab budget_reset（Stage 96）；④ 多级 BudgetEnforcer（key→user→team→org）+ real BDD 三后端全绿 + NaN 防御（Stage 97）。ADR-024 Approved→Accepted + TD-007 更新（Prometheus Alertmanager 候选）。总进度 96/96 — ALL STAGES COMPLETE。 |
| v41.0 | 2026-08-05 | **Phase 40 完成回写 + Phase 41 两阶段规划**：git log 确认 Phase 40（Stage 98-100 BDD Coverage Enhancement）全部实际完成（`f191758`/`8ccbba6`/`3069dfd`/`0888185`/`46e4c32`），roadmap/nnext-steps 积欠同步。Phase 41 拆为两 Stage 渐进交付：Stage 101 Passthrough（8h）— 客户端 `/v1/responses` → 上游 `/v1/responses`，新建 handler + `ClientProtocol::Responses` + 复用 `OpenAIPassthrough`，TDD 6 UT + 6 BDD；Stage 102 Bridge（14h，依赖 101）— 新增 `ResponsesToChatCompletions` 适配器（`MessageAdapter` + `StreamAdapter`）+ 流式 SSE 事件映射（output_text.delta/function_call_arguments.delta/response.completed）+ handler 集成，TDD 19 UT + 6 BDD。Phase 41 合计 22h。总进度 99/102。设计文档：`stage-101.md` + `docs/research/2026-08-04-openai-responses-api-support.md`。 |
| v42.0 | 2026-08-07 | **Phase 42 规划**：Playground 多模态图片能力，3 Stage 共 34.5h。基于代码审计确认后端多模态转换部分就绪（`claude_message_to_openai` 已正确生成 `data:{media_type};base64,{data}`；`openai_message_to_claude` 反向硬编码 `image/jpeg` + 完整 data URL 塞入 data 字段是 bug），前端 Playground 仅纯文本、src/ 无 file input 先例。三路 subagent 并发实测：Stage 103 后端适配修复 + `/v1/models` 暴露 model_info.mode + 6 BDD（6.5h）；Stage 104 Playground 图片输入（上传/粘贴/预览 + 双端点序列化 + 8 E2E × 3 viewports，16h）；Stage 105 图片渲染 + log-viewer output_text/image block + SpendLog 详情 + 文档收尾（12h）。总进度 99/105。设计文档：`stage-103~105.md`。 |
| v42.1 | 2026-08-07 | **Phase 42 完成**（Stage 103-105 ✅，总进度 103/105）：Stage 103 `cd576dc` 后端适配修复（parse_data_url + model_info.mode + 8 UT + 6 BDD）；Stage 104 `0f4868f` Playground 图片输入（上传/粘贴/预览 + 双端点序列化 + 8 E2E × 3 viewports + 全量 300 pass）；Stage 105 图片渲染（log-viewer extractImages/ImageThumbnails + OutputCard Responses output[] 分支 + SpendLog 详情 3 UT + 5 E2E × 3 viewports + 全量 312 pass）。ADR-025 Approved→Accepted，TD-009 a/b/e 登记。 |
| v43.0 | 2026-08-07 | **Phase 43 规划**：Image Token Usage Tracking — 上游优先解析 + 客户端 fallback 估算。3 Stage（106-108）共 28h。基于多轮调研（Qwen/VL config + 阿里云文档 + litellm/OpenRouter/OneAPI 源码）确认 Qwen/DashScope 返回 image_tokens（最完整），OpenAI/Anthropic 不返回；主流网关均不做预计算（aigw 将是行业差异化功能）。设计文档：`docs/stages/stage-106.md`~`stage-108.md` + `docs/plans/2026-08-07-image-token-estimation.md`。总进度 103/108。 |
| v45.0 | 2026-08-08 | **Embeddings 代理规划（原编号 Phase 45，2026-08-08 重编号为 Phase 44）**：OpenAI Embeddings API 代理（Stage 110-112，3 Stage 共 24h）。基于 6 路 subagent 调研（`docs/research/2026-08-08-embedding-proxy-support.md`）：LiteLLM 把 `/v1/embeddings` 当一等公民端点（四路径 + 与 chat 相同管道 + call_type=embedding + prompt-only 计费）；Kong/Portkey/new-api 均支持（leader parity）；用户确认 ① 有流量想多尝试 ② 本地+托管模型 ③ **排在在途 P1 收尾之后** ④ **四端点都需要** ⑤ health 探测非阻塞。Stage 110 后端 Passthrough 四端点（10h，6 UT + 11 BDD）；Stage 111 前端 OutputCard `data[]` 分支 + OpenAPI spec + real BDD（8h，3 UT + 2 E2E）；Stage 112 模型接入验证 + 文档收尾（6h，+2 BDD）。硬选 OpenAIPassthrough 拒绝 AnthropicNative（防 OpenAIToAnthropic 破坏 body）；薄 OpenAI-compatible 透传不做协议翻译。ADR-026 + TD-011 登记。总进度 104/111。 |
| v46.0 | 2026-08-08 | **Phase 41 完成回写（Stage 101-102 ✅）+ 测试缺口登记**：代码审计确认 Phase 41 两 Stage 已 2026-08-05 落地（`b90f42d` Stage 101 / `6a3ab61` Stage 102），roadmap/next-steps 此前积欠未同步——本次回写为 ✅，总进度 106/111。**实现修正**：Stage 101 `select_adapter(Responses, OpenAICompatible)` 实际接线 `ResponsesToChatCompletions`（非计划初稿的 `OpenAIPassthrough`），非流式 `/v1/responses` 实际返回 ChatCompletions 格式。**测试缺口登记**：① 计划声称的 19 适配器 UT 未落地（adapter.rs 无 `ResponsesToChatCompletions` 直测，桥接由 5 个 BDD 场景覆盖）；② `ResponsesToChatCompletionsStream` 未接入 handler 流式路径（流式路径转发原始 SSE 字节，mock 亦不返回真实 SSE 帧）——登记到 next-steps 待办，待后续补测。 |
| v46.1 | 2026-08-08 | **Phase 44/45 重编号**：原 "Phase 44 在途 P1 收尾" 是无 Stage 的待办桶（Responses 稳定 + Image Token + TD-006/TD-007），非真实 Phase——降级为无 Phase 号的 next-steps 待办项。Embeddings 代理（Stage 110-112）由 Phase 45 **重编号为 Phase 44**（3 Stage 真实功能 Phase）。Stage 号不变，Phase 号只作分组标签。ADR-026 同步。 |
| v47.0 | 2026-08-08 | **Phase 30 完成回写（Stage 78-81 ✅，总进度 110/111）**：Phase 30 代码自 2026-07-27 落地并经 Phase 31（Stage 82-84）生产化修复，roadmap 保持 ⚠️ 待修复直至审计缺陷核实完毕。本次回写前逐条核对 `docs/research/2026-07-25-body-archive-production-audit.md` 全部 28 项缺陷：**6 P0 + 10 P1 全部修复**（状态机 mark_job_*、配置单例化 config.rs body_archive 字段 + main.rs 注入、storage_configured 门禁 ×3、冷回源 spend.rs 详情端点、read_body_from_storage Err/None 区分、query_parquet_with_cache + FooterCache 激活、create_job/claim 事务化、前端路由化/分页/toast），**P2 10/12 修复**；剩余 P2-2（Engine panic 容错）/P2-3（shutdown 信号）登记 TD-005 不阻塞生产。Stage 78-81 全部回写 ✅，审计闭环，里程碑条 0%→100%。 |
| v48.0 | 2026-08-08 | **Phase 43 完成（Stage 106-108 ✅，总进度 113/114）**：Image Token Usage Tracking 全部交付（28h，3 Stage）。Stage 106 引擎（`45d7323`）：零依赖 PNG/JPEG/WebP/GIF header parser + model-name auto-sniff 公式（OpenAI tiling 85+170×tiles / Qwen2.5-VL factor 28 / Qwen3-VL factor 32 / Anthropic 官方 ⌈w/28⌉×⌈h/28⌉）+ extract_image_tokens_from_usage 上游解析器，18 UT。Stage 107 handler+迁移：Migration 025（spend_logs + 6 daily_*_spend 加 image_tokens × 3 方言）+ SpendLog/DailySpendLog 字段 + chat.rs/v1_messages.rs 集成（上游优先 + fallback，streaming Phase 2 UPDATE）+ daily_spend_queue 聚合 mock 上游真实 SSE 流式 + 5 BDD + 4 handler UT。Stage 108 前端+文档：SpendLog 详情 image_tokens + source badge（✓/⚠）+ 列表 🖼️ 标记 + i18n 3 keys + ADR-027 + TD-011。验证：aigw-core 391 + aigw-server 129 UT、mock BDD 219 pass（1 pre-existing budget_reset next_tick flake）、real sqlite 43/43、frontend BDD 327 pass。ADR-027 Accepted + TD-011a/b/c 登记。 |
| v49.0 | 2026-08-09 | **Phase 44 完成（Stage 110-112 ✅，总进度 116/116 — ALL STAGES COMPLETE）**：OpenAI Embeddings API 代理全部交付（24h，3 Stage）。Stage 110（`41d0223`）后端四端点：`routes/embeddings.rs`（responses.rs 非流式子集）+ 硬选 OpenAIPassthrough（拒绝 AnthropicNative）+ call_type=embedding + prompt-only 计费 + openapi embeddings_spec（18→19 端点）+ mock /v1/embeddings handler；**实现修正**：axum `Path<Option<String>>` 在无参数路由会 500 → 拆 `embeddings_handler`（无 Path wrapper）+ `embeddings_handler_with_path`（Azure 别名 Path<String>）两个公开 handler，共享 `embeddings_handler_inner`。TDD 6 UT + 11 BDD。Stage 111（`4637062`）前端：OutputCard `data[]` 分支（向量维度 + 8 维截断预览 + usage）+ i18n 2 keys + 2 E2E × 3 viewports（fe-bdd 333 pass）+ ADR-026 Accepted + TD-012a/b。Stage 112 模型接入 + 文档：models.feature +2 BDD（/model/new 注册 mode=embed + /v1/models 展示 + /v1/embeddings SpendLog call_type=embedding）+ roadmap/next-steps/ADR/TD 收尾。验证：aigw-server 135 UT、mock BDD 232 pass（2 新增全绿，仅 pre-existing budget_reset flake）、fmt + lint green。 |
| v49.1 | 2026-08-09 | **在途 P1 收尾完成（Responses 稳定 + TD-006/TD-007 ✅）+ Phase 45 规划**：① Responses 适配器级 UT 7 个直测 + stream tool-call args 修复（`caae61f`）；② 流式 SSE 接线（responses.rs 接 stream_adapter 转 Responses SSE 事件，mock 上游真实 Chat SSE 帧，BDD 断言三事件）（`361b99d`）；③ TD-006 `x-call-id` 响应头回写（`b485f30`，头名改 `x-call-id` `6b6822c`）——`aigw_core::request_id::UuidV7RequestId` + main.rs PropagateRequestIdLayer 覆盖全路由 + BDD 匹配 SpendLog.call_id；④ TD-007 soft_budget webhook 告警（`6e7a58c`）——`aigw_core::alerts` dispatcher + `general_settings.alert_webhook` + 3 UT + 2 config UT。⑤ 核实 TD-004 已修复（`b199000`）并补标 Resolved（`cb84341`）。⑥ **Phase 45 规划**：技术债清理 3 Stage（Stage 113 后端可靠性 TD-005+TD-010a+TD-003 8h / Stage 114 前端 TD-009ab+TD-008ab 10h / Stage 115 多模态 TD-011 重定义+TD-012b 10h），HEIC/AVIF 改前端转码、视频重定义为 Playground 输入。设计文档：`docs/plans/2026-08-09-tech-debt-cleanup.md` + `stage-113~115.md`。验证：workspace 796 UT、mock BDD 233 pass（新增 SSE-events + x-call-id 场景，仅 pre-existing budget_reset flake）。 |
| v50.0 | 2026-08-09 | **Phase 45 Stage 113 完成（总进度 117/117）**
| v51.0 | 2026-08-09 | **Phase 45 Stage 114 完成（总进度 118/118）**
| v52.0 | 2026-08-09 | **Phase 45 Stage 115 完成 + Phase 45 收官（总进度 119/119 — ALL STAGES + Phase 45 COMPLETE）**：多模态精度三项落地。① TD-011c Anthropic downsizing——`estimate_anthropic` 迭代缩放保比例到 ≤1568（⌈x/28⌉ 向上取整 overshoot 修正为迭代法），4 UT。② TD-012b 多模态按模态计费——`ModalPricing{image,audio,video}`（USD/1M）+ `Deployment.modal_pricing` + resolver `extract_modal_pricing` + `calc_spend_modal` 纯函数（modal ÷1e6 vs scalar per-token 单位校准）+ 4 UT + resolver 2 UT；**embeddings.rs 接线留待真实 per-modal input 流量**。③ TD-011b HEIC/AVIF 前端转码——`compressImage` 检测 heic/avif → Safari 解码转 JPEG；无法解码浏览器 toast（解码失败返回 null 使 caller 可区分，修 compressImage 原返回原图 bug）；E2E 验证 Chromium reject 路径。**TD-011a 视频输入 SKIPPED**（设计可选 + 无真实流量，记录剩余）。验证：aigw-core 415 + aigw-server 140 UT、fmt + lint green、playground.feature 57/57（含 HEIC 场景 × 3 viewports）。ADR-030 Accepted + TD-011b/c/012b Resolved。**Phase 45 技术债清理全收官**（Stage 113-115；TD-003/005/008a/b/009a/b/010a/011b/c/012a/b 全 Resolved）。 |
：前端体验四项技术债落地。① TD-009a Playground 图片压缩——`src/lib/image.ts` `compressImage`（canvas 2048px + JPEG 0.8，取「原图 vs 压缩」较小者保真，小图原样 PNG）+ 上传/粘贴统一走压缩；E2E 2400x2400 照片压缩后 <2MB。② TD-009b 请求体超限防御——handleSend 预检 `∑ dataUrlBytes > 24MiB` → toast + 拒绝（`window.__AIGW_MAX_IMAGE_BODY__` 测试 override）。③ TD-008a i18n 懒加载——en 同步 eager + 检测语言 eager（zh-CN 首访首帧中文）+ 另一语言动态 `import()` 独立 chunk（zh-CN 25kB lazy）；修复 en-US 归一化（防 Unknown dynamic import）。④ TD-008b 翻译 TS 类型——`scripts/fe-i18n-types` 生成 `resources.d.ts`（增广 i18next CustomTypeOptions，不翻转全局 strict）；暴露并修复 5 个缺失 key + 1 拼写错误（dashboard.spend→spendLogs）。验证：3 新 BDD 场景 × 3 viewports = 9/9、i18n-switcher 9/9、全量 fe-bdd 342 pass、fe-build 分包、fe-lint + tsc 通过。ADR-029 Accepted + TD-008a/b + TD-009a/b Resolved。 |：后端可靠性加固三项落地。① TD-005 Engine panic 容错 + 优雅关闭——`guarded()`（futures `catch_unwind` + `AssertUnwindSafe`）包裹三 loop 单次迭代（panic → log + 30s backoff + continue）+ `Engine::run_with_cancel(CancellationToken)`（in-flight step 先完成再退出，`run()` 兼容包装保留）+ main.rs axum 优雅关闭后 cancel；3 新 UT（`test_run_with_cancel_returns_on_cancel` / `test_tick_loop_panic_keeps_task_alive` / `test_guarded_recovers_panic`）。**实现偏差**：std catch_unwind 无法 await async → futures 组合子；tokio-util 不加 sync feature（不存在）；exec 取消检查放迭代后。② TD-010a health embedding 探测——`run_and_save_health_check` 增 `model_info` 参数（设计初稿读 `raw_params["model_info"]` 会静默失效，resolver raw_params 只含 litellm_params）+ `build_probe_spec` 分支 embed 走 `{api_base}/embeddings` body `input:["ping"]`；1 UT + 1 BDD。③ TD-003 `scripts/bdd-coverage` + `task bdd-coverage`——解析 mock+real feature（`发送 METHOD /path` + `并查询 /path`）对照内嵌路由表；实测 63%（55/87），**门禁 60% 回归基线**（admin-CRUD/login/key-deleted/model-groups/system-info 无 mock-BDD step，预置缺口列 NOT covered）。验证：aigw-core 409 + aigw-server 136 UT、mock BDD 233 场景（仅 pre-existing budget_reset flake）、bdd-coverage PASS、fmt + lint green。ADR-028 Accepted + TD-003/005/010a/012a Resolved。 |
| v53.0 | 2026-08-10 | **Phase 46 规划 + Stage 116 完成（总进度 120/120 — ALL STAGES + Phase 46 COMPLETE）**：静态配置模型接入——`config.yaml` 的 `model_list` / `router_settings` / `environment_variables` 启动时真正生效（此前解析后丢弃/零消费），并接线三个 `general_settings` 死字段。新增 `aigw_core::config_loader`（`seed_models_from_config` 幂等 DB-first / `apply_environment_variables` dotenvy 语义 / `build_router_config` 映射 + `router_settings_seed_json`）+ 10 UT；`keys.rs` `generate_key_token_with_len`（clamp 16-64）+ `/key/generate` disable_custom_api_keys gate + 4 UT；`main.rs` boot 接线（env 注入在 tracing init 前、model_list seed 在 Database::init 后、router_config 替换 `::default()`、deployment_mode config 优先、config 表 seed router_settings）；RouterStrategy 扩展 usage/latency 变体。BDD ×2（config seed 展示 + 幂等）+ 修复 budget_reset next_tick 硬编码 flake；`/v1/models` 空 model_info 补 `mode:"chat"`。验证：aigw-core 425 + aigw-server 144 UT、`task bdd` 254 场景（237 pass / 17 skip / 0 fail）、cargo check 无 warning。ADR-031 Accepted。设计文档：`stage-116.md`。 |
| v54.0 | 2026-08-10 | **Phase 47 规划（Stage 117-119，A 类接线 + exact-match 缓存，40h）**：基于差距调研（`docs/research/2026-08-09-aigw-gap-vs-industry-leaders.md`，litellm v1.97.0 源码深读 + 国际/国内/Rust 生态 10 篇笔记）确认 aigw 最大欠账是 **A 类「代码在但运行时未接线」**——RPM/TPM 限流、多级预算 `check_budget_multi`、soft_budget 告警、`max_parallel_requests`、Router 智能路由（usage/latency/weighted/cooldown/fallback/merge_overrides）全部已实现且有 UT 但请求路径零调用点（已核实 `enforce_limits` 仅 test、`check_budget_multi` 仅 `#[cfg(test)]`、`select_instance`/`merge_router_overrides` 仅测试）。**B 类「缓存=0」**：exact-match 响应缓存为全部竞品标配（litellm/Portkey/Cloudflare/Higress）。3 Stage 串行：Stage 117 A 类接线核心（4 handler 入口挂 `check_request_limits`：多级预算 + RPM/TPM + soft_budget 告警 webhook + max_parallel Semaphore，16h）；Stage 118 Router 智能路由接线（report_*/cooldown 真实推进 + merge_overrides + weighted + usage/latency 变体 + 错误类型 priority fallback + 前端下拉解锁，14h）；Stage 119 exact-match 缓存（`aigw_core::cache` moka LRU + cache key SHA-256 + 流式组装缓存 + `X-Cache-Status` + cache-hit 计费 0 元 + config 接线，10h）。设计文档：`docs/plans/2026-08-10-phase-47-wiring-cache.md` + `stage-117~119.md`。总进度 120/123（Stage 117-119 待开始）。 |
| v54.1 | 2026-08-10 | **BDD 漏洞审计（484ea70，总进度 120/123 不变）**：multi-agent 全量审计 + real BDD（sqlite/pg/mysql）确认 **0 失败但 4 个静默跳过洞 + 1 响应头缺口**——① `models.feature:54` 双空格 typo 使 /model/list 解密字段场景静默跳过（对齐注册 step 后真正执行）；② `end_to_end.feature` 失败场景断言 model_not_found（client 400 永不写 spend_log）→ 改写为 mock 上游 500 断言真实 failure spend_log；③ `responses.feature:20` 原始转义引号 `{expr}` 永不匹配 cucumber token → 改 `{string}`，流式 SSE 场景真正执行（**证明流式桥接路径真实运行**）；④ e2e spend_logs then-step 无注册 → 补通用 then-step；⑤ **responses.rs 流式路径缺 TD-006 `x-call-id` 头**（仅非流式有）→ 流式 SSE 现携带同一对账头（stream_request_id copy 防 move-after-use）。**基线锁定**：aigw-core 425 + aigw-server 144 UT、mock BDD **254 场景（241 pass / 13 @skip body_archive / 0 fail）**、fmt + clippy `-D warnings` green、real BDD 三后端 43/43。Phase 47（Stage 117-119）待开始。
| v55.0 | 2026-08-10 | **Phase 47 完成（Stage 117-119 ✅，总进度 123/123 — ALL STAGES COMPLETE）**：A 类「代码在但运行时未接线」全接线 + B 类 exact-match 缓存补齐。Stage 117（`d1000b0`）：4 handler 入口挂 `check_request_limits`（多级预算 key→user→team→org + RPM/TPM + soft_budget webhook + max_parallel Semaphore）；`LimitError::IntoResponse` 带 `x-ratelimit-limit/remaining` + `Retry-After`；`real/multi_level_budget` 去 @skip 4 场景 + `soft_budget.feature` 新建。Stage 118（`abad4db`）：Router weighted/usage/latency 真实决策 + cooldown 分类计数（429/401/408/404/5xx，400 不计）+ priority fallback + key>team>global `merge_router_overrides`；`router.feature` 新建。Stage 119（`ad981b2`）：`aigw_core::cache`（moka LRU + TTL + SHA-256 key + canonical body）+ `X-Cache-Status` + cache-hit 计费 0 元 + no-store；`cache.feature` 新建。验证：aigw-core 432 + aigw-server 145+152 UT（合计 861）、mock BDD 246（233 pass / 13 @skip / 0 fail）、real BDD sqlite/pg/mysql 47/47×3、fmt + clippy green。ADR-032 Accepted。顺带修复：BDD chat 步骤补 request-id layer（UUID-v7 call_id 防 spend_logs.call_id UNIQUE 冲突）、alerts.rs 测试 flake。后续收尾（非阻塞）：Stage 118 前端下拉解锁、Stage 119 config cache 块、max_parallel key/budget 表字段。
| v55.1 | 2026-08-10 | **Phase 47 收尾全完成（总进度 123/123 — ALL STAGES COMPLETE）**：补齐剩余三项非阻塞收尾。① 前端 RouterSettings 下拉解锁 usage-based-routing-v2 / latency-based-routing（Stage 118 §3.6，`9fe6329`，原 disabled "coming soon"——二者已是 Stage 118 真实路由决策）；② config `cache` 块解析 + boot 注入（Stage 119 §3.5，`9fe6329`，CacheConfig {enabled,backend,ttl_seconds,max_entries} + Default，main.rs 按 enabled 构建 MemoryCache / 禁用；config.example.yaml 补文档；+2 config UT）；③ max_parallel 从 key/budget 表字段层级接线（`cada57b`，resolve_effective_max_parallel：key→team→org-budget→deployment 取最严限制，master key 只套 deployment 上限，+4 UT）。验证：aigw-core 871 UT、mock BDD 246（233 pass / 13 @skip）、real BDD sqlite/pg/mysql 47/47×3、前端 fe-build/lint/bddgen 全绿、fmt + clippy green。
| v56.0 | 2026-08-12 | **Phase 48 完成（Stage 120，总进度 124/124）**：GLM5 流式 tool_use 首帧丢帧修复（`332fa08`）。用户反馈 aigw 转发 GLM5 到 Claude Code 反复 `Invalid tool parameters`。调研订正——前期把根因归为 `partial_json` 累积语义差异是错的：Anthropic 官方规范里 `partial_json` **本身就是纯增量碎片**（SDK 文档明确"客户端负责累积"）。真正 bug：`AnthropicToOpenAIStream::next` + `OpenAIToAnthropicStream::next` 首个带 `id` 的 chunk 若同时携带 `arguments`（tokenhub GLM-5.2 首帧就是 `id + "{\""`），代码 emit `content_block_start` 后 `return`，丢掉首帧 arguments。修复：tool_calls 分支 early-return → 本地 buffer 累积 SSE frame，循环末尾统一返回；同 chunk `content_block_start` + `input_json_delta` 两帧一起发出。+3 UT（正/反向对称 + 后续多个纯 args 增量顺序透传）。订正 `docs/16-glm-stream-delta-analysis.md` 五/六节根因。验证：aigw-core 455 UT、mock BDD 246、real BDD sqlite/pg/mysql pass（4 失败均上游 tokenhub 402 免费额度耗尽外部依赖）、fe-lint pass、fmt + clippy green。 |
| v57.0 | 2026-08-13 | **Phase 49 完成（Stage 121，总进度 125/125）**：上游模型停用功能接线。用户反馈"上游模型停用完全无效"。调研（`docs/research/2026-08-13-model-disable-audit.md`）确认——前端 Switch 只写 `model_info.mode="inactive"` 到 DB，后端**零处消费**该字段（DB SQL 只过 model_name / Resolver 不看 mode / Deployment 结构无 disabled 字段 / Router 只按 cooldown 过滤 / `/model/update` handler 仅 merge_json 存下来注释还标"保留"）。同一 `model_info.mode` 还兼载业务类别 "embed"/"image"，语义污染。方案 B 落地：Migration 026 三端 `ALTER TABLE proxy_models/deleted_models ADD COLUMN enabled` (BOOLEAN/INTEGER/TINYINT DEFAULT TRUE)；`ProxyModel` + `DeletedModel` + `UpdateModelRequest` + `ModelResponse` 加 `enabled` 字段；3 端 SQL（SQLite const + PG/MySQL inline）全部加 `enabled` 列，`LIST_MODELS_BY_NAME` 追加 `AND enabled=TRUE`；`ModelResolver::resolve` 加 `.filter(|m| m.enabled)` 防御式兜底；`/model/update` 读 `body.enabled`；前端 `ModelItem` 加 `enabled`、`isActive` 从 `mode` 迁到 `model.enabled`、Switch onChange 调 `{enabled}`。+3 UT（resolver 跳过 disabled + 同 name 两 row 只返 enabled + db 层 list_models_by_name 过滤）。验证：aigw-core 458 UT、mock BDD 246 保持、fmt + clippy + fe-lint + build green。设计文档：`docs/stages/stage-121.md`。 |
| v57.2 | 2026-08-17 | **Stage 121 收尾（总进度 125/125）**：三路 subagent 核实禁用能力完备性 + 补 3 个端到端 BDD 场景（`models.feature`：停用→chat 断言 400 `model_not_found` / 管理列表仍可见 / 重新启用→200）。核实结论——四个转发入口（chat/v1_messages/embeddings/responses）全部经 resolver 过滤（SQL `AND enabled` + resolver `.filter` 双层），`Router::pick_deployment` 只操作已过滤 vec 无 DB 重查/retry 绕过；`/model/update` 省略 `enabled` 保留原值、新建默认 true、迁移 026 三端历史行默认启用；写路径 + admin 展示（list + Switch）正确。**基线更新**：mock BDD **249 场景（236 pass / 13 @skip body_archive / 0 fail）**、fmt + clippy green；real BDD 三端 sqlite/pg/mysql **47/47 × 3 全绿**（首跑 pg/mysql 各有 1-4 例 429 为上游 tokenhub 真实限流偶发，重跑转绿）。收尾缺口登记 **TD-014a/b/c**（`@real_api` 三端覆盖缺失 / env 回退兜底仍转发禁用模型名 / config.yaml `model_list` 不支持 `enabled:false`）。 |
| v57.3 | 2026-08-18 | **Phase 50/51 规划（新增 Stage 122-134，规划态待实施）**：代理服务管理 + Claude OAuth 订阅反代两 Phase 规划完成（仅文档，不实施）。Phase 50（Stage 122-125，44h）：`proxies` 表（整串 proxy_url 加密落库）+ CRUD + 出口/质量检测（含 claude_oauth CF challenge 目标）+ 前端 + 收尾。Phase 51（Stage 126-130，50h）：凭证扩展 + Cookie→Token 3 步交换（PKCE）+ 三层 token 自愈 + 反代管线（最小化 billing 注入 + 全协议转换）+ 前端 + 收尾/安全审计。用户决策：最小化 billing 块默认注入、三层 token 自愈、全协议统一反代、凭证存 credentials 表、TLS 指纹推迟、proxies 表整串 proxy_url + probe_result 单 JSON。ADR-033/034 Accepted。规划文档：`docs/plans/2026-08-18-claude-oauth-reverse-proxy.md` + `docs/stages/stage-122.md` ~ `stage-130.md`。总进度 125 交付 + 9 规划（Stage 122-130）。 |
| v58.0 | 2026-08-18 | **Phase 50 完成（Stage 122-125 ✅，总进度 129/134）**：代理服务管理全部交付。Stage 122 后端 CRUD（`99ad254`）：proxies 表 + in-use 守卫 + proxy_url 加密。Stage 123 出口/质量检测（`0949890`）：`aigw_core::probe` 引擎 + /test /quality /toggle + 异步自动探测 + req socks。Stage 124 前端（`099d3eb`）：ProxiesPage + 对话框 + i18n + 6 BDD × 3 viewports。Stage 125 收尾：real BDD 三后端 proxy_crud.feature（53/53 × 3）+ 修复 PG/MySQL probe_result JSONB/JSON 方言 + MySQL list 绑定顺序 1835。基线：aigw-core 475 + aigw-server 154 UT、mock BDD 265（252 pass / 13 skip）、real BDD 53/53 × 3、fe-bdd 372（369 pass / 3 skip）。ADR-033 落地确认。下一里程碑 Phase 51（Stage 126-130）。 |
| v57.1 | 2026-08-16 | **文档写回修正（总进度 125/125 不变）**：Phase 21（Stage 59-60）与 Phase 22（Stage 61-62）明细表此前仍标 `⏳ 待开始`，git log 取证确认 4 个 Stage 代码早已落在 main（`49a5f1c` Stage 59 multi tool_result 修复 + `f385bc0` Stage 60 System Message Normalization + `b892fc4` Stage 61-62 AnthropicPassthrough/OpenAIToAnthropic，均 2026-07-16 交付；adapter.rs 中 `ChatTemplateCompat`/`AnthropicPassthrough` 现存）。本次仅修正状态为 ✅ 并补 commit 哈希与完成日期，无代码变更，顶部 "125/125" 计数原本即正确。 |
