# aigw -- 下一步行动

**上次更新**: 2026-10-06
**当前阶段**: **Phase 53 🔄 进行中 — 内建 Web Search（TD-017c）：Stage 134 ✅ 完成，135-138 ⏳；总进度 138**

---

## 当前状态：Phase 53 Stage 134 ✅ 完成，下一步 Stage 135

### Stage 134 交付（2026-10-06）

`aigw-core::websearch` 搜索后端抽象层落地 —— **多 provider 架构，本期唯一实现 SearXNG**，**刻意未接任何请求管线**（接线归 Stage 135），因此本层正确性完全由 UT 锁定、不牵连三条协议路径的回归面。

| 文件 | 交付 |
|------|------|
| `websearch/mod.rs` | `WebSearchRegistry`（provider 级 pick + failover）+ `build_provider`（**唯一 `kind` 分派点**）+ test-only `StubProvider` |
| `websearch/types.rs` | 三值类型 + `Guardrails` + `normalize` 六步流水线（过滤 → 域名 → 去重 → **score 重排** → 截断 → snippet 截断） |
| `websearch/provider.rs` | `SearchProvider` trait（object-safe）+ `SearchError`（`is_retriable` 区分 4xx 不转移 / 5xx·timeout·parse 转移） |
| `websearch/instance.rs` | `pick_instance_with_roll`（纯函数，可注入随机数）+ **`InstancePool`**（规划外新增，见下）+ `report_*`，语义逐字照 `router.rs` |
| `websearch/config.rs` | 三层配置（global → provider → instances）+ `validate()`（未知 kind 报错并列出支持列表）+ `attempt_order()` |
| `websearch/client.rs` | `build_search_client`（代理 ⊕ 重试 ⊕ 超时，组合 `probe.rs` 与 `router.rs` 两边的能力） |
| `websearch/searxng.rs` | `build_url` / `parse_response` / `map_error` 三纯函数 + IO 薄壳；fixture 为 2026-10-06 真实抓取 |
| `config_loader.rs` | `build_websearch_registry`（**只接已解析结构、不读文件** → Stage 138 的 DB 来源零重写复用） |

**验证**: aigw-core UT **530 → 611（+81）**；mock BDD **285 场景（272 pass / 13 skip / 0 fail）——与 Stage 133 基线逐字一致**，零 `.feature` 改动；`task fmt` / `task lint` green。

**规划外的增量（4 项，均非缩减）**:

1. **`InstancePool`** —— 规划只给了自由函数，但 provider 需跨 await 持有可变实例状态；若让每个 provider 自管 `Mutex<Vec<State>>`，「加一家 provider = 1 文件」的承诺就漏掉了实例管理。收进 `instance.rs` 后，新 provider 只需持有 pool 并循环 `pick_excluding` / `report_*`。
2. **`SearchError::EmptyQuery`** —— 实测要求「发请求前就拒绝空 query」，但规划枚举无法表达它（塞进 `Http{400}` 会谎称发生过网络往返）。
3. **`Parse` 判为可转移** —— SearXNG 的「200 + HTML」本质是**单实例配置错误**，兄弟实例可能正常，应换实例而非整体失败。
4. **域名名单从 `SearchRequest` 移除** —— 规划 §3.2 把它放进请求结构，但 §3.5 要求它是客户端不可触及的护栏；放在请求里等于给调用方开了入口。改为只存在于 registry 持有的 `Guardrails`，**结构上无字段可填**，优于靠纪律约束。

**⚠️ 验证方式的替代（须知）**: **未走严格 TDD 红绿**（测试与实现同批编写）。补偿手段是**变异测试** —— 注入 4 个针对性缺陷（删除 score 重排 / 截断先于去重 / 4xx 改为可转移 / snippet 读错键），确认每个都被对应 UT 捕获后还原。这证明断言非同义反复，**但不等同于红绿流程**。另：3 个集成测试最初因「实例选择随机」而 flaky，已改为断言与顺序无关的不变量，连跑 5 次稳定。

### ✅ 工程纪律与构建配置收尾（2026-10-07，用户决策后执行）

Stage 134 执行期间暴露了「Agent 绕过 Taskfile 跑裸命令」的问题，根因是**纪律文本引用了不存在的 task，且两处真实缺口没有 task 覆盖**。三项决策全部落地：

1. **删除 `reqwest` feature（方案 ②「承认现实」）** —— 该 feature 从未真正可选：`alerts.rs` / `claude_oauth.rs` / `probe.rs` 无条件引用 `reqwest::`，`--no-default-features` 有 **27 个既存编译错误**。改动：Cargo.toml 三个依赖去 `optional`、删 `[features] default = ["reqwest"]` 与 `reqwest = [...]`、`aigw-server` 两处 `features = ["reqwest"]` 清理、全库 **22 处 `#[cfg(feature = "reqwest")]` 移除**（`router.rs` 2 / `config_loader.rs` 10 / `websearch/*` 10）。**验证**：`task test` **1052 pass / 0 fail**（与删除前逐字一致）、`task bdd` **285 场景**不变、`task doctor` / `fmt` / `lint` 全绿。
2. **新增 `task test-filter -- <pattern>`** —— 此前跑过滤测试（开发迭代）**只能用裸命令**，这是 Stage 134 违规最集中的地方。现收敛进 Taskfile，并在 CLAUDE.md 写明边界：**过滤测试是迭代工具，不是证据；门禁与交付结论必须以 `task test` 全量结果为准**。
3. **新增 `task fmt-fix`** —— `task fmt` 只做 `--check`（只读），此前「实际格式化代码」没有任何 task 覆盖。现 `fmt` 管检查、`fmt-fix` 管写入，职责分明。

**CLAUDE.md 同步订正**：`task check` → `task doctor`（前者不存在，且被显式列入反例清单）、`task test-bdd` → `task bdd`、补 `test-filter` / `fmt-fix` / `fmt` / `lint`，并把「没有 task 时必须先问用户、不得自作主张跑裸命令」从一句话强化为带实例的明文约束。

### 下一步：Stage 135（prompt 注入接线，12h）

改两处丢弃点（`adapter.rs:2553-2559` 兜底臂、`:2210-2216` 历史 item）+ 三入口触发检测 + 注入最后一条 user 消息 + 搜索失败降级放行；顺带修 **TD-017g**（`ClaudeToolDef.input_schema` 致 HTTP 500）。⚠️ 注意 `responses.rs` 流式管线是**高风险区**（已有三次独立修复 `c3f360c` / `4bd85c7` / `8cf8c12` 落在同一处）。

---

## Phase 53 规划（Stage 135-138 ⏳）

**2026-10-06（内建 Web Search 调研 + Phase 53 规划）**: Stage 131 对服务端工具采取「丢弃 + 告警」——诚实但**客户端的联网能力实际不可用**。本环境实测：上游 MaaS 对 `web_search` 等服务端工具透传**全部 400**，而 litellm 的「派生 `web_search_options`」路线**被收下但不执行搜索**（模型回复「我无法联网」）。**→ 要真正可用，必须由网关自己执行搜索。**

### 三份调研（13 网关横向 + 20+ 厂商选型 + 代码落点测绘）

| 文档 | 内容 |
|------|------|
| `docs/research/2026-10-06-gateway-websearch-architecture.md` | 13 个网关横向 + litellm 拦截子系统深潜 + 线格式附录（OpenAI `annotations` / Responses `web_search_call` / Anthropic `server_tool_use`）+ **四种 SpendLog 计费方案** |
| `docs/research/2026-10-06-web-search-providers.md` | 20+ 厂商（含 Tavily / Serper / Brave / Exa / Yandex / DDG / SearXNG / Jina / 博查 / 智谱）的 curl + 响应形状 + 免费额度 + 注册方式 + ToS |
| `docs/research/2026-10-06-aigw-websearch-codebase-map.md` | aigw 代码落点（丢弃点 / SpendLog schema / 计费链路 / 配置层 / 流式管线） |

**关键结论**:

- **「网关自执行搜索」已是主流** —— 13 家中 **7 家**有该能力，不止 litellm。
- **三种形态而非两种** —— 除 litellm 的 (A) 短路 与 (B) agentic loop，**Higress `ai-search` 与 Portkey `exa/online` 各自独立实现了 (C)「搜索 → 改写 prompt → 单次调用」**。
- **两个硬约束把 A/B 封顶** —— Anthropic 的 `encrypted_content` 是服务端签发的不透明 blob，网关**无法伪造**，且多轮重放时**逐字节比对**（否则 400）；自造块会**投毒历史**，必须按 id 前缀剥离（sub2api `srvtoolu_ws_` 的血泪教训：「第一轮正常，第二轮整个会话 400」）。**形态 C 完全绕开这两个坑。**
- **三处常识更正** —— OpenAI 现价 **$10/1k**（$25/1k 只残存于 `web_search_preview` + 非推理模型）；OpenRouter 的 `plugins:[{id:"web"}]` 与 `:online` **均已弃用**；旧文档「sub2api 只丢弃」仅对其 Responses 桥接成立——它的 Anthropic 路径有完整模拟子系统（含迁移 `174_group_web_search_price_per_call.sql`，**即按次计费先例**）。
- **厂商四否决**（经实测 / 官方页核实）—— Google Custom Search JSON API **已不对新客户开放且 2027-01-01 停服**；DuckDuckGo **无官方 web-results API**（Instant Answer 仅返消歧义条目；`html.duckduckgo.com` 实测返 **202 + 反爬挑战页**）；Bing Search API v7 **已退役**；替代品 Grounding with Bing **$14/1k、无免费额度、强制展示 citations**。
- **aigw 侧三个缺口** —— ① **agentic loop 完全不存在**（全链唯一 `.send()` 在 `chat.rs:1743`；注释声称的 fallback loop 无对应重发代码）；② **按次（非 token）计费完全不存在**（`calc_spend` 纯 token，`ModalPricing` 实为 per-1M-token）；③ **Anthropic 服务端工具触发 HTTP 500**（`ClaudeToolDef` 的 `input_schema` 非 Option 且无 `type` → 反序列化即炸，已跨三路由核实 → 新增 **TD-017g**）。

### 用户决策

- **架构 = 形态 C（prompt 注入）** —— 上游零能力依赖、流式几无改动、不触碰两个硬约束。
- **Provider = 多 provider 架构 + 本期只实现 SearXNG（支持多实例）** —— Tavily（1000/mo 免费）与博查（中文，Bing 兼容形状）**本期不实现**，抽象与 `kind` 已就位，接入是纯增量；Serper / ScrapingDog / 智谱 / 百度千帆登记为后备候选。**SearXNG 非零成本**，`cost_per_query` 填自建摊销值。
- **新增 Stage 138**：provider / 实例 / 定价的 DB 化 + 管理 UI（对齐模型定价与代理服务的既有治理方式）。

### Phase 53 五 Stage（~49h，**规划态未实施**）

> **范围定调（2026-10-06，两轮用户决策）**：**① 多 provider 架构保留，本期只实现 SearXNG**（Tavily/博查不落地，接入 = 加一文件 + 一个 `kind` 分支；多态/failover/非零计费由 test-only `StubProvider` 证明）；**② 一个 provider 支持多实例**（provider = 逻辑后端 + 定价 → instances[] = 物理端点，实例选择照抄 `router.rs` 的 `cooldown_until` + `weighted_pick`，形成「实例级加权+冷却」与「provider 级 failover」两级结构）；**③ SearXNG 不是零成本** —— `cost_per_query` **缺省 `0.01` USD/次**（= $10/1k，对齐 OpenAI 牌价与 sub2api `174_*.sql`），**缺省即非零故生产默认有真实金额**；但该默认值是牌价占位，自建摊销典型在 `1e-4` 量级，**不改约高估 50 倍**，应按「(服务器+带宽+运维) ÷ 月查询量」改写；**④ 新增 Stage 138** 做 provider/实例/定价的 DB 化与管理 UI（补齐与模型定价、代理服务的治理一致性）。

| Phase | Stage | 主题 | 预估 | 状态 |
|-------|-------|------|------|------|
| **53** | 134 | 搜索后端抽象层（trait + registry + **仅 SearXNG 实现** + 多实例选择 + StubProvider + 配置 + 密钥 + HTTP client） | ~10h | ✅ 完成（2026-10-06，81 UT） |
| **53** | 135 | prompt 注入接线（两处丢弃点 + 三入口触发 + 注入模板 + 降级）+ 修 TD-017g | 12h | ⏳ 规划 |
| **53** | 136 | 按次计费 + SpendLog 独立行（**aigw 首个非 token 计价**）+ usage 回传 | ~8h | ⏳ 规划 |
| **53** | 137 | 调用日志展现（搜索行渲染 + 父子跳转 + 聚合口径审计 + i18n + fe-bdd） | 8h | ⏳ 规划 |
| **53** | 138 | **provider / 实例 / 定价 DB 化 + 管理 UI**（028 三方言两表 + CRUD + 实例子资源 + 连通性探测 + 定价编辑 + 改价不重启生效） | 14h | ⏳ 规划 |

**依赖**：134 ✅ → 135 → 136 → 137 严格串行；**138 依赖 134 + 136，与 137 无依赖可并行**（134 已完成 → 138 现仅等 136）。

**关键设计决策**:

1. **注入最后一条 user 消息，不注入 system** —— 决定性理由：system 注入与 Stage 131 建立的「有且仅有一条前导 system」不变量**正面冲突**（`consolidate_system_messages` 按出现顺序 `join("\n\n")`，顺序不可控；注入在 adapt 之后则等于在无人看守的第二处重复实现该不变量）；且 Anthropic 的 `system` 是顶层 untagged 二形态枚举 → **system 注入需 3 条代码路径，user 注入 1 条 `messages.last_mut()` 通吃**。附带收益：system 前缀逐字稳定，不击穿上游 prefix cache。
2. **计费选「独立行」而非「附加费并入同行」** —— 可独立聚合 + 零迁移（`call_type` 已有 `"embedding"` 作为第三取值先例）+ **绕开 `8cf8c12` 修过的「流式按 chunk 重建覆盖落库」陷阱**（搜索行有独立 `call_id`，Phase 2 UPDATE 物理上触不到）+ 未来设计 A（短路）无宿主行可挂，现在选方案 2 等于埋返工。
3. **`model_group` 与 `mcp_namespaced_tool_name` 禁止挪用** —— litellm 把 `model_group` 占用为搜索工具名，但 aigw 的「Spend by Model Group」图直接消费该列；`mcp_namespaced_tool_name` 已是 `daily_spend_queue` 聚合复合键的一部分，填值会裂开日聚合分组。
4. **搜索失败降级放行**（不带搜索继续调上游 + `warn` + metadata 标记），与既有「诚实降级」立场一致。
5. **两级实例/provider 选择照抄 `router.rs`，不自创** —— `:26` `cooldown_until`、`:79-84` 冷却过滤、`:366-377` `has_weight` 判定、`:419` `weighted_pick`。实例级解决「同一后端多端点的可用性与分流」，provider 级解决「换一家厂商」，职责不混。
6. **单价只做全局 provider 级，不做 key/team 覆写** —— 单价是「采购成本」（事实），覆写表达「加价/折扣策略」（产品概念），aigw 目前无售卖加价机制，引入即超范围。但**单价必须可运营修改**（Stage 138）而非改码重启。


**✅ 实施阻塞项已清空（Stage 134 已消费）**:

- **搜索后端**：✅ **SearXNG 已就绪并实测通过** —— `http://30.184.60.216:9099`，`format=json` 已开启，2026-10-06 实测 HTTP 200 / 延迟 2.0–3.1s / 中文可用。真实响应已固化为 fixture：`docs/fixtures/searxng-search-response-2026-10-06.json`（35 条，28KB），Stage 134 的解析 UT 直接内联该文件。**实测推翻 5 项原假设，已全部回写 Stage 134 §3.3.2/§3.4/§8.1**（详见下「实测关键发现」）。**本期不需要任何付费厂商账号。**
- **单价**：`cost_per_query` **缺省 `0.01` USD/次**（= $10/1k 牌价占位），开箱即可跑且金额非零。⚠️ **该默认值不是 SearXNG 的自建成本**——应按「(服务器 + 带宽 + 运维) ÷ 月搜索次数」改写（自建典型 `1e-4` 量级，不改约高估 50 倍）。Stage 134 走 yaml，Stage 138 后可在 UI 改且不重启生效。**不是开工前必须定好的数字。**

**⏳ 已降级为「接入时再处理」（不阻塞本期）**:

- **Tavily / 博查注册** —— 本期不实现其客户端，抽象与 `kind` 已就位，接入 = 加一文件 + 一个 `kind` 分支。
- **博查汇率决策** —— 仅因博查以 CNY 计价（¥36/1k）而 `SpendLog.spend` 恒为 USD 且无货币字段。**博查接入前必须落地该决策，落地前其金额不得用于对外出账**（三候选见 Stage 136 §8）。
- 调研中标 `未验证（请复核）` 的字段（Serper 的 `X-API-KEY` header 名、Jina 的 $/1M token 价、Yandex v2 `rawData` 细节等）—— 对应厂商接入时再实网探测。

**⚠️ 实现期须先核实的两件事**（Stage 138）:

- **`ON DELETE CASCADE` 的三方言一致性** —— SQLite 需 `PRAGMA foreign_keys=ON` 才生效，须先核实仓内连接是否开启；未开启则改应用层级联删除 + UT 锁定。
- **`probe_result` 的 JSONB/JSON 方言** —— Stage 125 已踩过并修过（roadmap v58.0），照 `proxies` 的三方言写法即可。


### SearXNG 实测关键发现（2026-10-06，5 项推翻原假设）

| # | 发现 | 对设计的影响 |
|---|------|------------|
| 1 | ⚠️⚠️ **`results[]` 按「每引擎各自 position」交错，不是全局 score 降序** —— 实测 score 序列 `1.0,1.0,0.5,0.5,0.33,0.33,0.33,0.25,0.25,`**`1.0`**`,…`（第 10 条跳回 1.0 = 第三引擎第 1 名） | **归一化必须「截断前按 score 全局稳定重排」**（Stage 134 §3.4 新增规则 5）。否则 `truncate(5)` 拿到的是「三引擎各自头部混合」，且**第三引擎的最佳结果会被整条丢掉**。纯看文档不可能发现 |
| 2 | ⚠️⚠️ **不支持服务端限条数** —— `&results=5` / `&limit=5` 均被忽略（仍返 31 条） | `max_results` clamp 只能在网关侧做（已覆盖），但**网络与解析成本无法节省**（恒收 ~30KB）。与 Tavily 的 `chunks_per_source`（厂商侧压缩）成本模型完全不同 |
| 3 | ⚠️ **无「零结果」语义** —— 乱码 query 仍返 35 条无关结果（Subway 门店、Google 首页…） | 网关**无法靠结果数判断「搜不到」**。Stage 135 注入模板的「资料与问题无关时直接忽略」由礼貌措辞**升级为必需的防噪音护栏**，不得精简删除 |
| 4 | ⚠️ **`format` 参数静默降级** —— `format=xml` / 省略 → **HTTP 200 + HTML**（非 4xx） | `parse_response` 对「200 但非 JSON」须给带排查提示的 `Parse` 错误，而非裸 serde 报错 |
| 5 | **延迟 2.0–3.1s（mean 2.43s）** —— 要等 bing+yandex+sogou 三引擎全部回包 | 设计 C 下**搜索耗时全额计入 TTFT**。全局 `timeout_ms=5000` 对它偏紧 → 改为 **per-provider `timeout_ms`**（SearXNG 档 8000）。**本期 SearXNG 是唯一后端，故该 TTFT 增量无可替代**；Tavily（~1s）接入后才可用「把它设为 `default_provider`、SearXNG 作 failover」这一手段 |

另确认（原标「未验证」）：`score` 35/35 非空、`publishedDate` 仅 7/35 非空（80% 缺失 → 前端排序不可依赖）、`content` 33/35 非空（2 条空 snippet 须容忍不丢弃）、空 query → HTTP 400、未启 limiter、中文可用。


**遗留（后续 Phase 候选）**: 设计 A（短路，**唯一能让 Claude Code 的 WebSearch 按钮真正可用**）；设计 B（agentic loop，硬前提「上游是否认网关**自行注入**的 function tools」**未实测**——Stage 132 只证明了客户端声明的工具可往返）；方案 3（`search_count` 专列）待「上游原生搜索次数」也需落库时启用。

**规划文档**: `docs/stages/stage-134.md` ~ `stage-138.md`

---

## 已完成：Phase 52（Stage 131-133 ✅，总进度 137）

**2026-10-06 计数校正**: commit `c7cd1f4`（Responses 原生直通 + 补齐 Codex 合规 SSE 事件序列）交付时**未写 stage 文档**，导致 `docs/stages/` 缺 133 且总进度计数失真（记 136，实为 **137**）。已补 `stage-133.md`。

**Stage 133 交付**: ① `Deployment.supported_standard_types` + `select_responses_adapter()`（声明含 `responses` → `ResponsesPassthrough` 原生直通，否则回落 Chat 转换）；② SSE 事件序列合规化——实测 **Codex 按 payload 的 `type` 分派、不认 `event:` 行**，且 delta 必须挂在已声明的 item 上，故补 `type` + 单调 `sequence_number` + 完整 item 生命周期。验证：aigw-core **530** UT、mock BDD **285 场景（272 pass / 13 skip）/ 1452 steps**、**真实 Codex 0.160.0 端到端零报错**。遗留：直通路径仅单元层验证（环境无声明该能力的可达上游）。

**注意**: 三次独立修复（`c3f360c` 死循环、`4bd85c7` 提前 `[DONE]`、`8cf8c12` 流式 SpendLog）全部落在 `responses.rs` 同一流式管线 → **高风险区**，Stage 135 已登记该警示。

---

## 已完成：Phase 52 前二 Stage（131-132）

**2026-10-05（Codex 兼容缺陷调研 + Stage 131 规划）**: Codex CLI 0.157.1（`wire_api = "responses"`）接 aigw `/v1/responses` **首个请求即 400**：

```
Unsupported: tool type 'namespace' is not supported in Responses→Chat bridge.
Only 'function' tools are supported.
```

**根因**：`ResponsesToChatCompletions::adapt_request`（`adapter.rs:2005-2019`）对工具是「非 function 一律 `return Err`」硬拒绝，而 Codex 原生发送 `namespace`（`multi_agent_v1`，含 5 个嵌套 function）+ `web_search` + `role="developer"` 消息。

**行业调研结论**（`docs/research/2026-10-05-codex-responses-bridge-gap.md`，直接读源码 + 生产网关实测）：**aigw 是 litellm / new-api / sub2api 四家中唯一「硬拒绝」的**——三家全部「转换或降级」，零失败路径。

**另发现 3 处既存缺口被 mock BDD 掩盖**（实测确认为真实 400，非理论风险）：

| # | 缺口 | 定位 | 实测 |
|---|------|------|------|
| D | 扁平 function 工具**从未转嵌套** | `adapter.rs:2009`（`"function" => {}` 只校验不转换） | 扁平→400 / 嵌套→200 |
| B | `role="developer"` **零处理** | `grep -rn '"developer"' crates/**/*.rs` 无命中 | 单条 developer → 400 |
| E | `input_text` content part 未映射 | `adapter.rs:1974-1980` 原样 `cloned()` | `input_text`→400 / `text`→200 |

**关键交互（设计要点）**：Codex 消息形状恒为 `[system(instructions), developer, user...]`。**朴素 `developer→system` 重命名会产出两个 system**——实测在 deepseek/glm 上游**不报错**（`两个 system 开头 -> 200`），但在 Qwen 严格模板（Phase 21 / Stage 60 已处理过）必 400。**故须合并进首位 system，而非折叠进 user turn（后者会降权为 `<system-reminder>`）。**

**降级 Codex 到 chat wire 已实测排除**：0.92.0（最后一个支持 `wire_api="chat"` 的版本）直连 `/v1/chat/completions` **同样** 400（同根因 `role="developer"`）；官方 discussion #7782 确认 chat wire 2026-02 移除。**→ 修复点必须在网关。**

**服务端工具（web_search 等）处置定案**：调研 `docs/research/2026-10-05-websearch-server-tool-support.md`（litellm 派生 `web_search_options` + 内建 agentic loop 子系统 / sub2api 丢弃 / new-api 透传）。**本环境实测**：`web_search`/`web_search_preview`/`code_interpreter`/`computer_use_preview`/`mcp` 透传**全部 400**；`web_search_options` 参数被收下（200）但**不执行搜索**（模型回复「我无法联网」）。→ 选「**丢弃 + 告警**」（与派生实效相同，但诚实且便于统计）；**内建真实搜索列为独立 Phase**（需搜索后端 + 多轮 agentic loop + SSE 交互）。

**`developer` 映射设计**：**默认启用，但提供 `model_info.developer_role_passthrough` 开关**（与 `chat_template_compat` 同构）——确认后端原生支持 `developer` 的上游可设为 `true` 透传。依据 litellm 已用「provider 覆写」实现同等语义（`translate_developer_role_to_system_role` base 映射、OpenAI/Azure 覆写为不映射）；aigw 无 provider 能力注册表（`ProviderType::infer` 把所有非 `anthropic` 归 `OpenAICompatible`），无法自动区分真 OpenAI，故用显式配置。

| Phase | Stage | 主题 | 预估 | 状态 |
|-------|-------|------|------|------|
| **52** | 131 | Responses→Chat 桥接修复（工具归一化 + role 归一 + part 映射，单轮） | 12h | ✅ 完成（2026-10-05） |
| **52** | 132 | 多轮 tool 历史适配（item 分派 + tool 配对归一，TD-017a） | 6h | ✅ 完成（2026-10-05） |

**Stage 131 交付**: ① 工具归一化（`function` 扁平转嵌套 / `namespace` 拍平 `{ns}__{child}` 含重名报错 / `custom`+`tool_search` 降级 / 服务端工具与 `mcp` 丢弃带告警 / `tool_choice` 同步清理）；② `developer` 与内联 `system` 合并进**唯一首位 system**（默认映射 + `developer_role_passthrough` 开关）；③ `input_text`/`output_text`→`text`；④ **13 个适配器 UT**（含 **Codex 抓包 fixture 端到端回归**）+ BDD 改写 2 条 + 新增 4 条 + mock 请求体断言能力。

**验证**: aigw-core **517** UT（+13）、mock BDD **283 场景（270 pass / 13 skip）/ 1436 steps**、`task fmt`/`task lint` green；真实上游形态验证 → **200**（此前 400）。**未提交/未部署**（交用户）。

**Stage 132 交付（多轮）**: `input_to_messages` 拆为 `items_to_messages`（按 `type` 分派：`message` / `reasoning`（暂存附到下条 assistant）/ `function_call`+`custom_tool_call`+`tool_search_call`（→ assistant `tool_calls`，并行合并、`namespace` 按请求侧同规则拍平、`custom` free-form `input` 包成 `{"input":...}`）/ `*_output`（→ `role="tool"` + `tool_call_id`，对象输出字符串化）/ 裸 content part / 未知类型跳过）+ `normalize_tool_pairing`（未应答调用剪除、孤立 tool 回复丢弃、回复紧随其 assistant）。**端到端**：假 Responses-SSE 上游驱动 Codex 0.160.0 走完真实一轮 tool 往返（`exec_command` → `echo hello` → 回填 → 收尾），round-2 body 固化为 UT fixture；变换后打真实上游 **200**（模型正确读到 tool 结果）。验证：aigw-core **527** UT（+10）、mock BDD **284 场景（271 pass / 13 skip）/ 1443 steps**、fmt+lint green。

**遗留**: `tool_choice` 的 `{type:"namespace"}` 形态（TD-017b）；内建搜索执行（TD-017c）；streaming SSE 事件映射 UT（TD-017d 剩余）。

**规划文档**: `docs/stages/stage-131.md` / `stage-132.md` + `docs/research/2026-10-05-codex-responses-bridge-gap.md` + `docs/research/2026-10-05-websearch-server-tool-support.md`

---

## 已完成：Phase 51（Stage 126-130 ✅，134/134）

**2026-08-24（Stage 130 收尾 + 安全审计 ✅，134/134）**: real BDD 三后端 OAuth 凭证 CRUD + 加密落库直读断言 + in-use 守卫（**58/58 × 3 全绿**）。**安全审计 8 项全部通过**。两个关键修复：
- **`/credential/new` OAuth 凭证逐字段加密落库**：`anthropic_oauth` 凭证的 access/refresh/session_key 之前以明文经通用创建路径落库（exchange 已加密但 new 遗漏）——新增 `aigw-core::crypto::encrypt_litellm_value_gcm`（AES-256-GCM `v2:gcm:`）+ `credential_new` 加密接线 + 2 UT。real BDD DB 直读探针三后端验证无明文。
- **TD-015a 全库收窄**：`chat::upstream_error_message`（只提取上游 `error.message`，parse 失败退化 `Upstream returned HTTP {status}`）接线 chat/v1_messages/responses/embeddings 四处 handler——所有上游错误不再暴露原始 body。

**验证**: aigw-core **502** + aigw-server **157** + aigw-migrate 27 UT、mock BDD **278（265 pass / 13 skip body_archive）**、real BDD 三端 **58/58 × 3**、`task fmt`/`task lint` green。设计文档：`docs/stages/stage-130.md` + `stage-130-review-log.md`；TD-015a Resolved；ADR-034 收尾；长期路线追加 LT-OAuthResponseAdapt / LT-CountTokensAuth。

**基线更新（Stage 130 后）**: aigw-core **502 UT**、aigw-server **157 UT**、mock BDD **278（265 pass / 13 skip body_archive）**、real BDD 三端 **58/58 × 3**、`task fmt`/`task lint` green。

| Phase | Stage | 主题 | 预估 | 状态 |
|-------|-------|------|------|------|
| **51** | 126 | 凭证扩展 + Cookie→Token 3 步交换（PKCE） | 12h | ✅ 完成 |
| **51** | 127 | Token 生命周期 + 三层自愈（缓存→刷新→cookie→告警） | 10h | ✅ 完成 |
| **51** | 128 | 反代管线（billing 注入 + 全协议转换 + 代理出口） | 14h | ✅ 完成 |
| **51** | 129 | CredentialsTab OAuth 入口前端 | 8h | ✅ 完成（2026-08-24） |
| **51** | 130 | 收尾：real BDD + 安全审计 + ADR-034 | 6h | ✅ 完成（2026-08-24） |

**后续候选（OAuth）**: TD-015d 响应侧协议转换（LT-OAuthResponseAdapt）/ TD-015e count_tokens 双认证（LT-CountTokensAuth）/ TD-016a/b 前端 refresh 语义;中期 M1 guardrails / M2 Redis 分布式层。

**依赖**: Phase 51 强依赖 Phase 50（凭证绑代理 + 交换走代理出口 + 反代代理出口 + claude_oauth 质量目标）。

**规划文档**:
- `docs/plans/2026-08-18-claude-oauth-reverse-proxy.md`（总体规划）
- `docs/research/2026-08-18-sub2api-proxy-oauth-reference.md`（参考实现调研）
- `docs/stages/stage-122.md` ~ `stage-130.md`（9 份 Stage 设计文档）
- ADR-033 / ADR-034

---

## 已完成 Phase 回顾（节选最新）

### Phase 49 Stage 121 ✅（上游模型停用功能接线）

**2026-08-13（Stage 121 ✅）**: 修复"上游模型停用功能完全无效"缺陷。前端 Switch 之前只写 `model_info.mode="inactive"` 到 DB，后端**零处消费**该字段（DB SQL 只过 model_name / Resolver 不看 mode / Deployment 结构无 disabled 字段 / Router 只按 cooldown 过滤）。同一 `model_info.mode` 还兼载业务类别 "embed"/"image"，语义污染。**方案 B 落地**：独立 `enabled: bool` 列。

| Stage | 交付 |
|-------|------|
| **121** | Migration 026 三端 `ALTER TABLE proxy_models/deleted_models ADD COLUMN enabled` (BOOLEAN/INTEGER/TINYINT DEFAULT TRUE)；`ProxyModel` + `DeletedModel` + `UpdateModelRequest` + `ModelResponse` 加 `enabled` 字段；3 端 SQL 全部加 `enabled` 列，`LIST_MODELS_BY_NAME` 追加 `AND enabled=TRUE`；`ModelResolver::resolve` 加 `.filter(|m| m.enabled)` 防御式兜底；`/model/update` 读 `body.enabled`；前端 `ModelItem` 加 `enabled`、`isActive` 迁到 `model.enabled`、Switch onChange 调 `{enabled}` 而非 `{model_info.mode}`；+3 UT（resolver 跳过 disabled + 同 name 两 row 只返 enabled + db 层 list_models_by_name 过滤） |

**基线（Stage 121 后）**: aigw-core UT **458**（Stage 120 → 455 + Stage 121 → 458）、mock BDD 246 保持基线、`task test/fmt/lint/build/fe-lint` 全绿。设计文档：`docs/stages/stage-121.md`；根因调研：`docs/research/2026-08-13-model-disable-audit.md`。

**收尾（2026-08-17 ✅）**: 三路 subagent 核实禁用能力完备性 + 补 3 个端到端 BDD 场景（`models.feature`：停用→chat 断言 400 `model_not_found` / 管理列表仍可见 / 重新启用→200）。核实结论——四个转发入口（chat/v1_messages/embeddings/responses）全部经 resolver 过滤（SQL `AND enabled` + resolver `.filter` 双层），`Router::pick_deployment` 只操作已过滤 vec 无 DB 重查/retry 绕过；`/model/update` 省略 `enabled` 保留原值、新建默认 true、迁移 026 三端历史行默认启用。**基线更新**: mock BDD **249 场景（236 pass / 13 @skip body_archive / 0 fail）**、fmt + clippy green；real BDD 三端 sqlite/pg/mysql **47/47 × 3 全绿**（首跑 pg/mysql 各有 1-4 例 429 为上游 tokenhub 真实限流偶发，重跑转绿）。收尾缺口登记 **TD-014a/b/c**。

**边界（不做的事）**: 不清理历史 `model_info.mode` 里的 "inactive"/"disabled" 值（`isActive` 现只看 enabled，历史值惰性容忍）；不引入 `status enum` 状态机（方案 C，长期路线）。

---

## Phase 48 Stage 120 ✅（流式 tool_use 精度修复）

**2026-08-12（Stage 120 ✅）**: 修复 aigw 转发 GLM5（tokenhub 上游）到 Claude Code 出现 `Invalid tool parameters` / `__unparsedToolInput` 的问题。根因**订正**：不是 `partial_json` 累积语义差异（Anthropic 官方规范里 partial_json 就是纯增量碎片），而是 `AnthropicToOpenAIStream::next` 与 `OpenAIToAnthropicStream::next` 的 tool_calls 分支 early-return **丢掉与首帧 `id` 同帧的 `arguments="{\""`**。

| Stage | 交付 |
|-------|------|
| **120** | `crates/aigw-core/src/adapter.rs`：`AnthropicToOpenAIStream::next` + `OpenAIToAnthropicStream::next` tool_calls 分支重构（early-return → local buffer 累积后统一返回,共 ~60 行);3 个新 UT（`test_stage120_glm5_first_chunk_id_and_args` / `test_stage120_glm5_reverse_first_chunk_id_and_args` / `test_stage120_multiple_arg_frags_accumulate`）；`docs/16-glm-stream-delta-analysis.md` 五/六节订正；`docs/stages/stage-120.md` |

**基线（Stage 120 后）**: aigw-core UT 458（+3 Stage 120 = 455 → 458）、mock BDD 246 场景（233 pass / 13 @skip / 0 fail）保持、`task fmt` + `task lint` 全绿。

**收益**: tokenhub GLM-5.2（逐 token 增量首帧 `id + "{\""`)、MAAS GLM-5（词组增量首帧空 arguments）、DeepSeek/OpenAI 全上游流式 tool_use 首帧不再丢帧;下游 Claude Code 累积得到的 partial JSON 完整合法。

---

## 当前状态：Phase 47 全部完成（Stage 117/118/119 ✅）+ BDD 基线锁定

**2026-08-10（Phase 47 交付 ✅）**: 差距调研（`docs/research/2026-08-09-aigw-gap-vs-industry-leaders.md`）确认的 **A 类「代码在但运行时未接线」** 全部接线 + **B 类「缓存=0」** 补齐，三 Stage 交付：

| Stage | Commit | 交付 |
|-------|--------|------|
| **117** | `d1000b0` | 4 handler 入口挂 `check_request_limits`（多级预算 key→user→team→org + RPM/TPM + soft_budget webhook + max_parallel Semaphore）；`LimitError::IntoResponse` 带 `x-ratelimit-limit/remaining` + `Retry-After`；`real/multi_level_budget` 去 @skip 4 场景 |
| **118** | `abad4db` | Router 智能路由真实决策：weighted（weight 加权随机）+ usage（max remaining rpm）+ latency（min EWMA）+ cooldown 分类计数（429/401/408/404/5xx，400 业务错不计）+ priority fallback 顺序 + key>team>global `merge_router_overrides` 接入请求路径 |
| **119** | `ad981b2` | exact-match 响应缓存：`aigw_core::cache`（moka LRU + 手动 TTL + SHA-256 key + canonical body）+ `X-Cache-Status: HIT/MISS` + cache-hit 计费 0 元（`cached=1`）+ no-store 绕过 + `CacheControl`（use-cache/ttl） |

**基线（Phase 47 收尾）**: aigw-core 432 + aigw-server 145+152 UT（**合计 861**）、mock BDD **246 场景（233 pass / 13 @skip body_archive / 0 fail）**、real BDD sqlite/pg/mysql **47/47 × 3**、fmt + clippy `-D warnings` green。ADR-032 Accepted（Stage 117-119 落地）。roadmap v55.0。

**顺带修复**: ① BDD chat 步骤缺 request-id layer（所有 mock chat 请求 call_id="unknown" 撞 spend_logs.call_id UNIQUE，缓存写第二条时暴露）→ e2e + cache 步骤补 `SetRequestIdLayer`（UUID-v7）；② alerts.rs 测试 flake（`dispatch_soft_budget_alert` 的 `tokio::spawn` 在同步 `#[test]` 无 reactor panic）→ 改 `#[tokio::test]`。

**收尾（✅ 全部完成）**: ① 前端 RouterSettings 下拉解锁 usage/latency（`9fe6329`，weight/rpm/tpm 输入留后续模型页）；② config `cache` 块解析 + boot 注入（`9fe6329`）；③ max_parallel 从 key/budget 表字段层级接线（`cada57b`，key→team→org-budget→deployment）。

**后续候选**（中期 3-6 月，视人力）:
4. **M1** guardrails 最小集（regex 提示注入 + PII 脱敏 + Moderation hook，P0，4-5 pw）
5. **M2** Redis 分布式层（跨实例限流/预算 counter/共享缓存，P0，4-5 pw）
6. **M3** MCP 最小透传 + WebSocket（P1，3-4 pw）
7. **M4** 审计日志 + RBAC 矩阵（P1，5-7 pw）
8. **M5** `gen_ai.*` 可观测语义 + 指标扩展（P2，2-3 pw）

**按使用量触发**: litellm_settings 接线、config 热重载、TD-008c/d、TD-009e、TD-011a 视频 token 估算、长期路线 LT-*

---

## Phase 40: BDD Coverage Enhancement ✅ 完成

**2026-08-05 回写**: 以下 git log 确认 Stage 98-100 全部实际完成，roadmap 此前未同步——本次文档修正。

| Stage | 目标 | 类型 | 预估 | 状态 |
|-------|------|------|------|------|
| Stage 98 | 路由端点 BDD 补全 — health 追加 5 场景 + router_settings.feature 新建 4 场景 + deleted_list.feature 新建 4 场景。共 13 mock BDD，3 个 feature 文件 | 测试 | 12h | ✅ 完成 |
| Stage 99 | 内部模块 + middleware 补测 — daily_spend_queue UT ×7 + rate_limiter 429 BDD ×3 + auth_gateway UT ×4 + rate_limit middleware UT ×5。共 19 测试 | 测试 | 14h | ✅ 完成 |
| Stage 100 | aigw-migrate 高级功能 BDD — precheck.feature ×4 + verify.feature ×2 + advanced.feature ×3 + cursor.feature ×2。共 11 real BDD（SQLite 全量 + PG/MySQL 选 5） | 测试 | 10h | ✅ 完成 |

**相关 commits**:
```
46e4c32 test(migrate): Stage 100 aigw-migrate pre-check + verify UT 6 tests
0888185 test(core): Stage 99 Part C+D — auth_gateway already has 6 UT + rate_limit 5 UT
3069dfd test(bdd): Stage 99 Part B — rate_limiter BDD 3 scenarios
8ccbba6 test(core): Stage 99 Part A — daily_spend_queue 7 UT
f191758 test(bdd): Stage 98 路由端点 BDD 补全 — 13 new mock BDD scenarios
2d31e97 docs(phase-40): add BDD coverage enhancement Stage 98-100 design docs
```

**设计文档**: `docs/plans/2026-08-03-bdd-coverage-enhancement-phase-39.md`、`docs/stages/stage-98~100.md`、`docs/research/2026-08-03-bdd-coverage-audit.md`

---

## Phase 41: OpenAI Responses API 接入 ✅（22h，2 Stages，2026-08-05 完成）

**背景**: OpenAI 于 2025 年推出 Responses API（`POST /v1/responses`）。`/v1/responses` 上游生态极窄（仅 OpenAI + litellm），绝大多数 provider 只支持 `/v1/chat/completions`。分两阶段渐进交付：101 先做 Passthrough 让端点可用，102 加 Responses→Chat 协议转换覆盖所有上游。

| Stage | 目标 | 类型 | 预估 | 状态 |
|-------|------|------|------|------|
| Stage 101 | POST /v1/responses Passthrough — 新建 handler + 路由注册 + ClientProtocol::Responses + Usage 双 fallback + 流式透传。TDD: 6 UT + 6 BDD | 后端+测试 | 8h | ✅ 完成（b90f42d） |
| Stage 102 | Responses→Chat 协议桥接 — ResponsesToChatCompletions 适配器（MessageAdapter + StreamAdapter）+ 非流式字段映射 + 流式 SSE 事件映射 + handler 集成。TDD: 5 BDD（适配器级 UT 未单独拆分） | 后端+测试 | 14h | ✅ 完成（6a3ab61） |

**依赖**: Stage 101 → 102（101 落地端点骨架 + ClientProtocol::Responses，102 在此基础上加适配器转换，独立测试验收）。

**关键决策**:
- **先 Passthrough 后 Bridge，分开验收**：两个 Stage 独立可测，101 验证端点→认证→SpendLog 链路正确，102 验证协议转换正确。
- **⚠️ 实现修正**：Stage 101 实际 `select_adapter(ClientProtocol::Responses, ...)` 直接接线 `ResponsesToChatCompletions`（非计划初稿的 `OpenAIPassthrough`）；流式路径保持原始 SSE 透传。
- **显式丢弃字段**：`reasoning`、`previous_response_id`/`conversation`、非 function 工具（400 拒绝）。

**测试缺口（已记录）**: ① 计划声称的 19 适配器 UT 未落地，桥接逻辑由 5 个 BDD 场景覆盖；② `ResponsesToChatCompletionsStream` 未接入 handler 流式路径，流式 SSE 事件转换未被执行覆盖。

**设计文档**: `docs/stages/stage-101.md` + `docs/research/2026-08-04-openai-responses-api-support.md`

---

## 项目里程碑

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
Phase 30:   ████████████████████ 100% (4/4)  ✅ Body Archive 冷存储（Stage 78-81 生产化后回写）
Phase 31:   ████████████████████ 100% (3/3)  ✅ Stage 82-84 全部完成
Phase 32:   ████████████████████ 100% (1/1)  ✅ request_id → call_id 改名 + 上游对账链路（Stage 85）
Phase 33:   ████████████████████ 100% (1/1)  ✅ aigw↔aigw 多表只读增量同步（Stage 86）
Phase 34:   ████████████████████ 100% (1/1)  ✅ 售后对账链路收尾（Stage 87）
Phase 35:   ████████████████████ 100% (2/2)  ✅ Core Entity Soft-Delete (Stage 88-89)
Phase 36:   ████████████████████ 100% (1/1)  ✅ Upstream Cache Detection & Billing (Stage 90)
Phase 38:   ████████████████████ 100% (3/3)  ✅ UI 多语言 i18n 支持 (Stage 91-93)
Phase 39:   ████████████████████ 100% (4/4)  ✅ Budget Reset 周期任务 + 配置 (Stage 94-97)
Phase 40:   ████████████████████ 100% (3/3)  ✅ BDD Coverage Enhancement (Stage 98-100)
Phase 41:   ████████████████████ 100% (2/2)  ✅ OpenAI Responses API 接入 (Stage 101-102)
Phase 42:   ████████████████████ 100% (3/3)  ✅ Playground 多模态图片 (Stage 103-105)
Phase 43:   ████████████████████ 100% (3/3)  ✅ Image Token Usage Tracking (Stage 106-108)
Phase 44:   ████████████████████ 100% (3/3)  ✅ OpenAI Embeddings API 代理 (Stage 110-112)
Phase 45:   ████████████████████ 100% (3/3)  ✅ 技术债清理 (Stage 113-115 全部完成)
```

---

## Phase 42: Playground 多模态图片 ✅（2026-08-07，34.5h）

**背景**: 用户要给 Playground 增加图片能力，让 qwen3.5-vl 等多模态模型在 playground 中识别图片。代码审计确认后端多模态转换部分就绪（`claude_message_to_openai` 正确生成 `data:{media_type};base64,{data}`），但 `openai_message_to_claude` 反向有 bug（硬编码 image/jpeg + 完整 data URL 塞入 data 字段），前端 Playground 仅纯文本。三路 subagent 并发实测收敛为 3 Stage。

| Stage | 目标 | 类型 | 预估 | 状态 |
|-------|------|------|------|------|
| Stage 103 | 多模态适配修复（`openai_message_to_claude` data URL 解析 + `/v1/models` 暴露 model_info.mode）+ 6 BDD。TDD: 8 UT + 6 BDD | 后端+测试 | 6.5h | ✅ 完成（cd576dc） |
| Stage 104 | Playground 图片输入（上传/粘贴/预览 + 双端点多模态序列化 + 独立 sessionStorage 持久化 + RASTER_MIME 守卫）+ 新增 /v1/messages mock + 请求体捕获。TDD: 8 E2E × 3 viewports = 24 执行 | 前端 | 16h | ✅ 完成 |
| Stage 105 | 图片渲染（Playground 气泡 + log-viewer extractImages/ImageThumbnails + OutputCard Responses output[] 分支）+ SpendLog 详情 3 UT + 文档收尾。TDD: 3 UT + 5 E2E × 3 viewports = 15 执行 | 全栈+文档 | 12h | ✅ 完成 |

**依赖**: Stage 103 → 104（发送图片依赖反向转换正确 + 模式字段）；Stage 104 → 105（渲染依赖图片数据模型就绪）。

**设计文档**: `docs/stages/stage-103.md` / `stage-104.md` / `stage-105.md`

---

## 测试目标

| 层 | 当前 |
|---|------|
| 后端单元 | aigw-core **611**（Stage 134 后：530 + 81 websearch）；aigw-server 160+167；全 workspace **1052 pass / 0 fail** |
| mock BDD | **285 scenarios（272 pass / 13 @skip body_archive / 0 fail）** — Stage 133 起基线，Stage 134 未改变 |
| 前端 BDD | ≥ 342 passed（Stage 114 全量回归；含 3 压缩场景 × 3 viewports + i18n-switcher 9） |
| real BDD | ≥ 47 SQLite / ≥ 47 PG / ≥ 47 MySQL（Phase 47 三后端全绿） |

---

## 优先级排序

| 优先级 | 目标 | 状态 |
|--------|------|------|
| ✅ | Phase 40 BDD Coverage Enhancement | ✅ 完成（2026-08-03） |
| ✅ | Phase 41 Stage 101 POST /v1/responses Passthrough | ✅ 完成（b90f42d） |
| ✅ | Phase 41 Stage 102 Responses→Chat 协议桥接 | ✅ 完成（6a3ab61） |
| ✅ | Phase 42 Stage 103 多模态适配修复 + 模型模式暴露 | ✅ 完成（cd576dc） |
| ✅ | Phase 42 Stage 104 Playground 图片输入 | ✅ 完成 |
| ✅ | Phase 42 Stage 105 图片渲染 + SpendLog 详情 + 文档 | ✅ 完成 |
| ✅ | Phase 39 补充 Stage 109 预算重置 cron 界面重构 | ✅ 完成（2026-08-08） |
| ✅ | Phase 43 Stage 106-108 Image Token Usage Tracking | ✅ 完成（2026-08-08） |
| ✅ | Phase 44 Stage 110-112 OpenAI Embeddings API | ✅ 完成（2026-08-09，116/116 ALL COMPLETE） |
| ✅ | 在途 P1 收尾：Responses 稳定（适配器 UT + 流式 SSE）+ TD-006 x-call-id + TD-007 webhook | ✅ 完成（2026-08-09） |
| ✅ | TD-004 BDD @real_api 键泄漏 | ✅ 已修复（b199000，2026-07-20） |
| ✅ | Phase 45 Stage 113 后端可靠性加固（TD-005 + TD-010a + TD-003） | ✅ 完成（2026-08-09） |
| ✅ | Phase 45 Stage 114 前端体验（TD-009a/b 图片压缩 + TD-008a/b i18n 懒加载） | ✅ 完成（2026-08-09） |
| ✅ | Phase 45 Stage 115 多模态精度（TD-011b/c + TD-012b + TD-011a 可选） | ✅ 完成（2026-08-09，TD-011a 视频 SKIPPED） |
| ✅ | Phase 46 Stage 116 静态配置模型接入（config.yaml model_list/router/env + key gates） | ✅ 完成（2026-08-10，ADR-031 Accepted） |
| ✅ | BDD 漏洞审计（4 静默跳过洞 + 流式 x-call-id 头） | ✅ 完成（2026-08-10，484ea70，mock BDD 254 基线） |
| ✅ | Phase 47 Stage 117 A 类接线核心（限流 + 多级预算 + soft_budget 告警 + max_parallel） | ✅ 完成（2026-08-10，d1000b0） |
| ✅ | Phase 47 Stage 118 Router 智能路由接线（cooldown/weighted/usage/latency/fallback） | ✅ 完成（2026-08-10，abad4db） |
| ✅ | Phase 47 Stage 119 exact-match 响应缓存（moka LRU + X-Cache-Status + 计费 0 元） | ✅ 完成（2026-08-10，ad981b2） |
| ✅ | Phase 47 收尾：前端 RouterSettings 下拉解锁 + config cache 块 + max_parallel key/budget 表字段 | ✅ 完成（2026-08-10，9fe6329 / cada57b） |
| ✅ | **Phase 50 代理服务管理（Stage 122-125 全部完成）**（proxies 表 + in-use 守卫 + proxy_url 加密 + 出口/质量检测 + ProxiesPage 前端 + real BDD 三后端） | ✅ 完成（2026-08-18） |
| ✅ | **Phase 51 Stage 126-128 完成**（凭证扩展 + Cookie→Token 交换 + Token 三层自愈 + 反代管线） | ✅ 完成（2026-08-20，11 commits beffd97~2fc1d89） |
| ✅ | **Phase 51 Stage 129 前端 OAuth 入口 + 手动 refresh** | ✅ 完成（2026-08-24，ADR-035 + TD-016a/b） |
| ✅ | **Phase 51 Stage 130 收尾 + 安全审计（134/134 ALL STAGES COMPLETE）** | ✅ 完成（2026-08-24，real BDD 58/58 × 3 + 审计 8 项 + TD-015a Resolved + ADR-034 收尾） |
| ✅ | **Phase 53 Stage 134 搜索后端抽象层**（`aigw-core::websearch` 六文件 + 装载 + 配置 section + 81 UT） | ✅ 完成（2026-10-06） |
| P1 | **Phase 53 Stage 135 prompt 注入接线** + 修 TD-017g | 下一步 |
| ✅ | 工程纪律与构建配置收尾：删 `reqwest` feature + 新增 `task test-filter` / `task fmt-fix` + CLAUDE.md 订正 | ✅ 完成（2026-10-07） |
| P2 | TD-008c/d 后端错误多语言 + RTL、TD-009e 外链缩略图、TD-011a 视频 token 估算（剩余） | 待处理（视使用量） |
| P2 | Phase 41 测试缺口（适配器 UT + 流式接线） | ✅ 关闭（2026-08-09） |
