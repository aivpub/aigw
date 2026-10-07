# Stage 134: 搜索后端抽象层（Phase 53）

**所属**: Phase 53（内建 Web Search / TD-017c）
**预估**: 10h（provider trait + 两级选择：实例级加权/冷却 + provider 级 failover + SearXNG 客户端 + test-only StubProvider + 配置装载 + 密钥解密 + HTTP client + 归一化 + UT）
**依赖**: 无（独立新子系统，不触碰任何既有请求管线）
**状态**: ✅ 完成（2026-10-06）

---

## 1. 目标

新建 `aigw-core::websearch` 模块，交付一个**可独立编译、可独立测试、可独立合并**的搜索后端抽象层：**多 provider 架构 + 本期唯一实现 SearXNG** + 统一结果形状 + 配置装载 + 密钥解密 + 出站 HTTP client。

**本 Stage 只实现 SearXNG 一家** —— 已有可用自建实例（`http://30.184.60.216:9099`，2026-10-06 完成实测，见 §3.3.1），零注册摩擦、零采购成本。Tavily / 博查 Bocha 的 endpoint / auth / 字段映射调研**全部保留为接入参考附录**（§3.3.2 / §3.3.3），但**不写代码**。

**抽象必须是真的，不是 vaporware** —— 一家实现无法证明「多 provider 就绪」，故本 Stage 用一个 **test-only `StubProvider`**（`#[cfg(test)]`，见 §4.1）充当「第二个 provider」，在 UT 层证明 trait 多态、registry 选择、failover 排序、4xx 不转移 / 5xx+timeout 转移、`api_key` 解密、**非零 `cost_per_query`**。加一家 provider 的代价被压到「**新增 1 个文件 + `kind` 枚举加 1 个分支**」，其余零改动。

**两级结构（本次范围修正，§3.8）**：`provider`（逻辑后端，持 `kind` + 定价）**→** `instances[]`（物理端点，各自 `base_url` / `api_key` / `weight` / `enabled`）。SearXNG 的典型部署就是多实例做可用性与分流，单 `base_url` 模型是错的。**实例级 = 加权随机 + 连续失败冷却**（provider 内部，逐字照 `router.rs` 现成实现）；**provider 级 = `failover_order`**（provider 之间）。

**本 Stage 刻意不接任何请求管线** —— 接线（触发检测、prompt 注入）在 Stage 135。这样本层的正确性可以完全用 UT 锁定，不受三条协议路径的回归面牵连。

### 验收标准

- [x] `SearchProvider` trait **object-safe**，可作 `Arc<dyn SearchProvider>` 存入 `WebSearchRegistry`；`SearxngProvider` 实现之，`cargo` 无新 warning
- [x] SearXNG 响应解析 UT 用**录制 fixture**（`docs/fixtures/searxng-search-response-2026-10-06.json`，真实抓取，非实网调用）通过
- [x] `web_search` 顶层 config section 可解析；`providers[]` 支持 **N 元数组**（不是单例特化），`kind` 为判别式；**每个 provider 下 `instances[]` 亦为 N 元数组**；缺省（absent）= 禁用，零行为变化
- [x] **未知 `kind` → 配置校验期报错，且错误信息列出当前支持的 kinds**（本期为 `searxng`）—— 这是「加 provider 是纯增量」的可验证证据
- [x] ⭐ **实例级选择按 `router.rs` 语义工作**：任一实例声明 `weight` → 加权随机（`weight: 0` 排除，全零/缺省 → 均匀随机）；连续失败 ≥ `allowed_fails` → 进入 `cooldown_until` 冷却并被过滤；全部冷却 → 取最早恢复者而非拒服务
- [x] `v2:gcm:` 密文形态的 `api_key` 装载时自动解密（**per-instance**）；明文 key 原样可用（SearXNG 免鉴权，该路径由 StubProvider UT 覆盖）
- [x] per-provider `timeout_ms` 覆盖生效，缺省回落全局值
- [x] `cost_per_query` **缺省默认 `0.01` USD/次**（= $10/1k，对齐 OpenAI 牌价与 sub2api `174_*.sql` 的 `0.01`），语义为「**部署方应按自建摊销成本改写的单价**」；配置注释须写明「该默认值是行业牌价占位，**不是 SearXNG 的真实自建成本**，不改会高估」
- [x] ⭐ **StubProvider UT 证明抽象可用**：trait 多态、registry 默认选择、provider 级 failover 顺序、4xx 不转移、5xx/timeout 转移、非零 `cost_per_query` 携带
- [x] `WebSearchConfig` / `WebSearchProviderConfig` / `WebSearchInstanceConfig` **与配置来源解耦** —— 装载函数接受已解析结构（不自己读文件），使 Stage 138 的 DB + admin CRUD 来源成为纯增量
- [x] 服务端护栏（`max_results` 默认 5 / snippet-only / 单次超时 / 域名白黑名单）在**本层**生效，不依赖调用方自觉
- [x] `task test` / `task fmt` / `task lint` 全绿；`task bdd` 场景数不变（本 Stage 无 BDD 面）

### 明确不做（边界）

- **不实现 Tavily 与博查 Bocha 客户端**（抽象已就位，接入是纯增量：1 个文件 + 1 个 `kind` 分支；endpoint/auth/字段映射调研已固化为 §3.3.2 / §3.3.3 附录，接入时零再调研成本）
- **不改 `adapter.rs`**（工具丢弃点、历史 item 分派全部不动 → Stage 135）
- **不改三条路由**（`chat.rs` / `responses.rs` / `v1_messages.rs` 零改动）
- **不做计费 / SpendLog**（`cost_per_query` 只是**配置字段**，本 Stage 仅解析与透传，不参与任何 spend 计算 → Stage 136）
- **不做前端**（→ Stage 137）
- **不做 agentic loop、不做短路、不合成任何服务端工具块**（设计 A/B，见 §8.2）
- **不做 query 改写**（Higress 的 `searchRewrite` 多一跳 LLM，不在阶段 1）
- **不做配额加权负载均衡 / Redis 配额预留**（sub2api `manager.go` 的全套治理，多 key 场景出现后再补，见 §8.2）
- **不新建 DB 表、不加 migration**（provider 配置走 config.yaml 顶层 section；per-deployment 开关走 `model_info`，均零迁移）。**配置源的 DB 表 + admin CRUD + 前端归 Stage 138**（仿 `proxies` 表 / `027_proxies.sql` / Phase 50），本 Stage 只做 yaml 来源但结构与来源解耦
- **不实现 Serper / Exa / Brave / Perplexity / ScrapingDog 等其余厂商**（登记为后续候选，见 §8.2）

---

## 2. 现状证据

调研全文：`docs/research/2026-10-06-gateway-websearch-architecture.md`（架构选型）、`docs/research/2026-10-06-web-search-providers.md`（厂商线格式与定价）、`docs/research/2026-10-06-aigw-websearch-codebase-map.md`（落地基线）。

### 2.1 搜索后端 — 零存在

全仓无任何搜索厂商 HTTP 客户端。`grep -rn "tavily\|searxng\|bocha\|api.exa.ai\|serper" crates/` 零命中。架构调研 §5.1 的现状表逐字记录「搜索后端：**零**」。

### 2.2 `provider.rs` 的 `ProviderRegistry` 不可复用 — 是遗留 stub

`crates/aigw-core/src/provider.rs:49` 的 `ProviderRegistry` **不在任何活跃请求路径上**：`chat.rs` 走 `deployment.rs` + DB `proxy_models` + `Router`；`ProviderRegistry::default_with_env()`（`provider.rs:69`）还在直读 `OPENAI_API_KEY` 环境变量，是 pre-DB 时期接线。

真正的 provider 分发是 adapter 层二维 match（`crates/aigw-core/src/adapter.rs:75-90`），而 `MessageAdapter` trait 的四个方法（`adapter.rs:34-46`：`client_protocol` / `adapt_request` / `adapt_response` / `stream_adapter`）对搜索厂商**全部不适用**（搜索不收 OpenAI chat 体、不回流式 token）。

**→ 必须新建独立 trait，无现成抽象可实现。** 组织方式照 `crates/aigw-core/src/body_archive/` 的目录化模块（`config.rs` / `storage.rs` / `writer.rs` / `query.rs` / `cache.rs` / `mod.rs`）。

### 2.3 出站 HTTP client — 重试与代理分散在两个互不相干的函数里

| 函数 | 位置 | 有重试 | 有代理 | 超时 |
|------|------|-------|-------|------|
| `Router::build_retry_client` | `crates/aigw-core/src/router.rs:535-548` | ✅ `ExponentialBackoff` + `RetryTransientMiddleware` | ❌ | **硬编码 600s**（`router.rs:542`） |
| `probe::build_proxy_client` | `crates/aigw-core/src/probe.rs:26-33` | ❌ | ✅ `reqwest::Proxy::all` | 调用方给定 |

搜索厂商**同时需要**：国内环境大概率要走代理出网，且公网 API 抖动需要重试，而单次超时必须**远小于** 600s（搜索发生在首字节之前，直接计入 TTFB）。→ 本 Stage 新写一个组合版 `build_search_client(proxy: Option<&str>, timeout: Duration, retries: u32)`。

### 2.4 配置层 — `CacheConfig` 是现成模板，`litellm_settings` 是反面教材

`AigwConfig`（`crates/aigw-core/src/config.rs:30`）现有 8 个顶层字段，`cache: Option<CacheConfig>` 在 `config.rs:60`，`CacheConfig` 本体在 `config.rs:65-78`（每个字段配 `#[serde(default = "...")]` 默认值函数）—— **逐字照此结构新增 `web_search`**。

装载侧：`build_router_config`（`crates/aigw-core/src/config_loader.rs:131`）是纯函数 + 可单测的范式。

**反面教材**：`config_loader.rs:28` 的文件头注释明确记录 `litellm_settings` 被解析但**刻意未接线**，并在 `docs/12-technical-debt.md:140`（TD-013）登记为技术债。本 Stage 若只加结构不加运行期消费，会变成同类死配置 —— 故**必须**在本 Stage 就让 `build_websearch_config` 产出可用的运行期 registry（即便消费者在 Stage 135）。

### 2.5 密钥加密 — `v2:gcm:` 方案已就位，零新机制

| 函数 | 位置 | 说明 |
|------|------|------|
| `encrypt_litellm_value_gcm` | `crates/aigw-core/src/crypto.rs:178` | AES-256-GCM，输出 `v2:gcm:` + Base64(salt[16]‖nonce[12]‖ct+tag)；`crypto.rs:170-177` 注释指明新写入应优先 GCM |
| `decrypt_litellm_value` | `crates/aigw-core/src/crypto.rs:50` | **分发器**，检测 `v2:gcm:` 前缀 → GCM，否则 NaCl |
| `decrypt_json_fields` | `crates/aigw-core/src/crypto.rs:285-300` | 递归遍历 JSON，对每个 string 叶子 try-then-fallback 解密，**非密文静默原样返回** |

`decrypt_litellm_value` 的「非密文安全拒绝」语义（`crypto.rs:280-284` 注释）正是本层需要的 —— 配置里的 `api_key` 可以是明文也可以是密文，统一过一遍分发器即可。

### 2.6 厂商线格式三家实测差异（必须在映射表里显式处理）

来自 `docs/research/2026-10-06-web-search-providers.md`：

| 厂商 | 结果数组路径 | title 键 | snippet 键 | 备注 |
|------|-------------|---------|-----------|------|
| Tavily（§2.1） | `results[]` | `title` | `content` | **唯一自带 `score`**；字段天生贴合内部形状 |
| SearXNG（§2.15） | `results[]` | `title` | `content` | `format=json` 需在 `settings.yml` 显式开启，否则 **403** |
| 博查 Bocha（§2.21） | `webPages.value[]` | **`name`**（不是 `title`） | `snippet` | **Bing Web Search 兼容格式** → 一个解析器可同时服务任何 Bing-shaped API |

### 2.7 已验证的先例：sub2api 的同形抽象

`~/works/play/sub2api/backend/internal/pkg/websearch/` 共 10 文件，是已上线验证的最小切分：

```go
// types.go
type SearchResult struct { URL, Title, Snippet, PageAge string }
type SearchRequest struct { Query string; MaxResults int; ProxyURL string }
type SearchResponse struct { Results []SearchResult; Query string }
const defaultMaxResults = 5

// provider.go
type Provider interface {
    Name() string
    Search(ctx context.Context, req SearchRequest) (*SearchResponse, error)
}
```

两个可直接继承的决策：① `defaultMaxResults = 5`（与 OpenRouter 默认 5、Tavily 默认 5 三方一致，架构调研 §5.5）；② **`http.Client` 由调用方注入**（`NewTavilyProvider(apiKey, httpClient)` 的注释："The caller is responsible for configuring the http.Client with proxy/timeouts"）→ provider 本体不碰代理/超时策略，可测性最佳。

### 2.8 实例级加权与冷却 — `router.rs` 已有完整实现，**逐字照抄不自创**

多实例选择是本仓已上线的能力，`websearch` 不得发明第二套语义：

| 能力 | 位置 | 语义 |
|------|------|------|
| 实例级状态字段 | `crates/aigw-core/src/router.rs:26` `InstanceState.cooldown_until: Option<Instant>`（含 `consecutive_failures` / `total_failures`） | 冷却到期时刻 |
| 冷却过滤 | `router.rs:79-84` `s.cooldown_until.is_none_or(\|t\| now >= t)` | 冷却中的实例不参与选择 |
| 全部冷却的兜底 | `router.rs:366-377` `min_by_key(cooldown_until)` + `tracing::warn!("All deployments in cooldown...")` | **取最早恢复者而非拒服务**（与 §3.8 的「基础设施不可用时放行」同向） |
| 加权判定 | `router.rs:366-377` `has_weight = active.iter().any(\|&i\| deployments[i].weight.is_some_and(\|w\| w > 0))` | **任一实例声明 weight → 走加权**，否则均匀随机 |
| 加权选择 | `router.rs:419` `weighted_pick` | `weight: 0` 排除；pool 为空（全零/缺省）→ 均匀随机 |
| 失败计数 → 冷却 | `router.rs:450-466` `report_failure`：`fail_count += 1`，`>= allowed_fails` → `cooldown_until = now + cooldown_time`；`report_success`（`:470-472`）清零并解冷却 | 连续失败驱动，成功即恢复 |

**→ `websearch` 的实例选择 = 上表的同形移植**（类型名换成 `WebSearchInstanceState`，判定条件逐字一致）。这保证运维心智模型与 `Router` 统一：同一套 weight / cooldown 概念，不需要第二份文档。

---

## 3. 方案

### 3.1 模块布局

照 `body_archive/` 的目录化模块组织，新增 `crates/aigw-core/src/websearch/`，并在 `crates/aigw-core/src/lib.rs`（模块声明区 `:18-48`，按字母序插在 `tenant` 之后）加 `pub mod websearch;`。

| 文件 | 职责 | 对标 |
|------|------|------|
| `websearch/mod.rs` | re-export + `WebSearchRegistry`（provider 实例集合 + **provider 级** 选择/故障转移） | `body_archive/mod.rs` |
| `websearch/types.rs` | `SearchRequest` / `SearchResult` / `SearchResponse` / `DEFAULT_MAX_RESULTS` | sub2api `types.go` |
| `websearch/provider.rs` | `SearchProvider` trait + `SearchError` | sub2api `provider.go` |
| `websearch/instance.rs` | **实例级**选择 — `WebSearchInstanceState`（`cooldown_until` / `consecutive_failures`）+ `pick_instance`（加权随机 + 冷却过滤）+ `report_failure` / `report_success` | **逐字照 `router.rs:26` / `:79-84` / `:366-377` / `:419` / `:450-472`**（§2.8） |
| `websearch/config.rs` | `WebSearchConfig` / `WebSearchProviderConfig` / `WebSearchInstanceConfig` + 校验 | `body_archive/config.rs` |
| `websearch/client.rs` | `build_search_client`（代理 + 重试 + 超时组合） | `probe.rs:26-33` + `router.rs:535-548` |
| `websearch/searxng.rs` | **本期唯一 provider 实现** — SearXNG 请求构造 + 响应解析（多实例由 `instance.rs` 统一处理，provider 只管「给定 base_url 打一次」） | sub2api `tavily.go` 的同形结构 |

> **加一家 provider 的全部代价 = 新增 1 个文件（`websearch/<name>.rs`）+ `kind` 判别式加 1 个分支**。`mod.rs` / `types.rs` / `provider.rs` / `config.rs` / `client.rs` 五个文件**均不需改动** —— 这是本 Stage 抽象设计的核心验收点，由「未知 `kind` 报错列出支持列表」+ StubProvider 的多态 UT 双向证明。
>
> `websearch/tavily.rs` 与 `websearch/bocha.rs` **本期不创建**（映射参考见 §3.3.2 / §3.3.3）。

**provider 文件内部强制切成三段纯函数 + 一层 IO**，这是本 Stage 可测性的关键（新增 provider 照此模板）：

```
build_request(&self, req: &SearchRequest) -> (Method, Url, HeaderMap, Option<Body>)   // 纯
parse_response(bytes: &[u8]) -> Result<SearchResponse, SearchError>                   // 纯，UT 主战场
map_error(status: u16, bytes: &[u8]) -> SearchError                                   // 纯
async fn search(...)                                                                  // 唯一 IO，薄壳
```

### 3.2 类型与 trait

`websearch/types.rs`：

```rust
pub const DEFAULT_MAX_RESULTS: usize = 5;

pub struct SearchRequest {
    pub query: String,
    /// None → DEFAULT_MAX_RESULTS。本层会再按配置上限 clamp（§3.5）。
    pub max_results: Option<usize>,
    /// 服务端护栏，客户端/模型不可注入（§3.5）。
    pub allowed_domains: Vec<String>,
    pub blocked_domains: Vec<String>,
}

pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub published_date: Option<String>,   // 原样字符串，不强行解析（§3.4）
    pub score: Option<f32>,
}

pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub query: String,
    pub provider: String,
    /// 厂商在响应里自报的消耗（如 Tavily `usage.credits`；SearXNG 恒 `None`）。
    /// 本 Stage 仅携带，不参与任何计费计算 —— 计费归 Stage 136。
    pub reported_credits: Option<f64>,
}
```

`websearch/provider.rs`：

```rust
/// **必须 object-safe** —— registry 以 `Arc<dyn SearchProvider>` 存放异构 provider。
/// 故：无泛型方法、无 `Self: Sized` 约束、无关联常量。
#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    /// 稳定标识符（本期 "searxng"；接入后 "tavily" / "bocha"），进日志与 Stage 136 的计费归属。
    fn name(&self) -> &str;
    /// 单次查询单价（SearXNG 恒 0.0）。本 Stage 仅携带，Stage 136 消费。
    fn cost_per_query(&self) -> f64;
    async fn search(&self, req: &SearchRequest) -> Result<SearchResponse, SearchError>;
}

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("{provider}: transport: {0}", .source)]
    Transport { provider: String, source: String },
    #[error("{provider}: HTTP {status}: {body}")]
    Http { provider: String, status: u16, body: String },
    #[error("{provider}: parse: {0}", .detail)]
    Parse { provider: String, detail: String },
    #[error("{provider}: timeout after {ms}ms")]
    Timeout { provider: String, ms: u64 },
    #[error("web search is not configured")]
    NotConfigured,
}
```

`SearchResult.score: Option<f32>` —— 厂商调研 §3.3 第 2 条：只有 Tavily / Valyu / Exa 有分数，**不要为了「补齐」而伪造**。

### 3.3 provider 的请求/响应映射

> **§3.3.1 SearXNG 是本期唯一实现**。§3.3.2 Tavily 与 §3.3.3 博查为**未实现的接入参考附录** —— 其 endpoint / auth / 字段路径调研已核实并保留，接入时可直接照表写代码，零再调研成本。
>
> 以下 endpoint / auth / 字段路径全部取自 `docs/research/2026-10-06-web-search-providers.md`。标 ⚠️ 的项该文档标注为「未验证（请复核）」，**实现时必须先做一次实网探针确认再写死**。

#### 3.3.1 SearXNG（§2.15）—— ✅ 本期实现

> **2026-10-06 实测基线**：`http://30.184.60.216:9099`（自建实例，`format=json` 已开启）。本节所有「[实测]」标记均来自该实例真实响应，下文 §2.15 原「未验证（请复核）」项已全部落定。

| 项 | 值 |
|----|----|
| Endpoint | `GET {base_url}/search?q=<q>&format=json`（**[实测] HTTP 200**） |
| Auth | **无**（自建实例，靠网络隔离/反代保护） |
| 前置条件（硬） | 实例 `settings.yml` 的 `search.formats` 必须含 `json`。**[实测] 该实例已开启**；未开启时为 403 → `map_error` 对 403 返回带「请在 settings.yml 开启 json format」提示的 `SearchError::Http` |
| 成本 | `cost_per_query`：**缺省 `0.01`**（USD/次，= $10/1k 行业牌价占位）→ **应改为自建摊销值**（`(服务器+带宽+运维) / 预估月查询量`，通常远低于 0.01，量级多在 `1e-4`）。留默认即**高估**自建成本 |
| **延迟 [实测]** | **2.0–3.1s（5 次连打 mean ≈ 2.4s）** —— 显著高于 Tavily（~1s 级）。**这是设计 C 的真实 TTFT 增量**（搜索串在上游调用之前）→ `timeout_ms` 默认 5000 对该实例**偏紧**，建议 SearXNG 档位置 **8000**；并在 Stage 135 的降级策略文档里写明「SearXNG 后端下 TTFT 恒增 ~2.5s」 |
| **速率限制 [实测]** | 5 次连续请求**全部 200，无 429、无 `unresponsive_engines`** → 自建实例默认未开 limiter（`searx.limiter` 需显式启用 + Valkey）。**但不可依赖**：limiter 一旦开启会按 IP 封，`map_error` 必须正确处理 429 |
| **中文 [实测]** | 可用。`量子计算 最新进展` → 29 条，首条为百度百科（engine=bing），`content` 为中文 snippet → **中文场景下 SearXNG 可作博查的零成本替代** |
| **聚合引擎 [实测]** | 该实例实际启用 **bing + yandex + sogou** 三引擎（35 条结果中 bing 10 / yandex 15 / sogou 10） |

响应映射（**全部 [实测] 确认**）：

| 内部字段 | JSON path | 实测结论 |
|---------|----------|---------|
| `title` | `results[].title` | 35/35 非空 |
| `url` | `results[].url` | 35/35 非空；**35/35 唯一**（该实例已自行去重，但本层仍须去重——多引擎同 URL 不保证恒不重复） |
| `snippet` | `results[].content`（**在 `content` 不在 `snippet`**） | **35 条中 2 条为空**（sogou 居多）→ **空 snippet 必须过滤或容忍，不可 unwrap** |
| `score` | `results[].score` | **存在，35/35 非空**（原「未验证」→ 已确认）。⚠️ 语义见下「排序陷阱」 |
| `published_date` | `results[].publishedDate` | **存在但 35 条中 28 条为 `null`（80% 缺失）** → `Option<String>` 正确；**前端排序/筛选不可依赖该字段**（已登记 §8.2） |

**五个实测得出的实现要点**（均与原假设不同，按重要性排序）：

1. ⚠️⚠️ **`results[]` 不是全局按 score 降序 —— 是按「每引擎各自的 position」交错排列**。[实测] 实际 score 序列为 `[1.0, 1.0, 0.5, 0.5, 0.33, 0.33, 0.33, 0.25, 0.25, **1.0**, 0.5, 0.25, ...]` —— 第 10 条又跳回 1.0（那是第三个引擎的第 1 名）。**故「取前 N 条」= 取到的是 bing 前几名 + yandex 前几名，而非全局最相关的 N 条**。→ **归一化必须先按 `score` 全局降序重排再截断**（`sort_by(|a,b| b.score.partial_cmp(&a.score))`，`None` 视为 0.0 排最后）。这是本次实测**最有价值的发现**：若照原计划直接 `truncate(5)`，注入给模型的就是「三个引擎各自的头部混合」而非最相关结果，且**第三引擎的最佳结果（score=1.0）会被整条丢掉**。
2. ⚠️⚠️ **SearXNG 不支持服务端限制结果数** —— [实测] `&results=5` 与 `&limit=5` **均被忽略**（仍返回 31 条）。唯一能减少结果的是 `&engines=bing`（→ 10 条）。**故 `max_results` 的 clamp 必须在网关侧做**（本层 §3.4 的截断已覆盖），但**网络与解析成本无法节省**（恒接收全量 ~30KB）。→ 与 Tavily 的 `chunks_per_source`（厂商侧压缩）形成鲜明对比，**应在配置文档里写明两者的成本模型差异**。
3. ⚠️ **零结果查询不返回空列表，而是返回 35 条无关结果**。[实测] `zzqqxk7h3v9nonexistentquerystring42` → 35 条（Subway 门店、Google 首页、bitcoin GitHub issue…）。→ **网关无法靠「结果数为 0」判断「搜不到」**；注入这些噪音会**主动污染模型上下文**。本层不做相关性判断（无可靠信号），但 Stage 135 的注入模板已含「参考资料与问题无关时直接忽略」指引 —— **该指引因此由「礼貌性措辞」升级为「必需的防噪音护栏」**，Stage 135 §3.5 已登记。
4. **`format` 参数无效时静默降级为 HTML，不报错**。[实测] `format=xml` → **HTTP 200 + HTML 页面**（非 JSON、非 4xx）；完全省略 `format` 同样返回 HTML。→ `parse_response` 必须对「200 但 body 非 JSON」给出明确错误（而非 serde 的裸 `expected value`），提示「检查 base_url 是否指向 SearXNG 且 format=json」。另：`format=csv` [实测] 真实可用（返回 `title,url,content,host,engine,score,type` 表头）—— 本层不用，仅说明 `format` 是白名单而非布尔。
5. **空 query 返回 HTTP 400**。[实测] `q=` → 400 → `map_error` 归入 `SearchError::Http`；本层应在发请求**之前**就拒绝空 query（省一次 RTT）。

**上下文成本 [实测]**：35 条 × mean 136 字符 ≈ **4761 字符 ≈ 1.2k tokens**（全量）；经本层 `max_results=5` + score 重排截断后 ≈ **680 字符 ≈ 170 tokens**，**优于 Tavily 默认档**（snippet 天然短：max 184 字符，无需 `snippet_max_chars` 介入）。

**另两个既有注意**（厂商调研 §2.15，保留）：① 公共实例基本都关了 JSON（「many public instances have these formats disabled」）→ 配置文档必须写明**必须自建**；② 引擎脆弱性 —— 抓取器会随上游反爬变更而碎，建议允许 `base_url` 后附 `&engines=...` 固定引擎（本层原样转发调用方给的 query 参数，不做引擎策略）。

#### 3.3.2 Tavily（§2.1）—— ⛔ **未实现，接入参考附录**

> **本 Stage 不写 `websearch/tavily.rs`**。以下表格为已核实的接入资料，保留以便将来「新增 1 个文件 + 1 个 `kind` 分支」即可上线。表中结论未经本仓代码验证。

| 项 | 值 |
|----|----|
| Endpoint | `POST https://api.tavily.com/search`（[已验证]） |
| Auth | `Authorization: Bearer tvly-xxx`（[已验证]） |
| 请求体 | `{"query": <q>, "search_depth": "basic", "max_results": <n>, "chunks_per_source": <c>, "include_raw_content": false}` |
| 成本档位 | `search_depth=basic` = 1 credit = $0.008；`advanced` = 2 credits = $0.016 → **固定 `basic`**，不暴露给客户端 |
| ⭐ `chunks_per_source` | **默认 3，范围 1-3，每 chunk 硬上限 500 字符**（[已验证] [docs.tavily.com/documentation/api-reference/endpoint/search](https://docs.tavily.com/documentation/api-reference/endpoint/search)）—— **这是全样本中唯一有文档化「每条结果字符硬上限」的厂商参数**。即 Tavily 可在**厂商侧**把单条压到 ≤1500 字符，优于本层的 `snippet_max_chars` 事后截断（后者已付完整响应的网络与解析成本）。**建议实现时置 `chunks_per_source: 1`**（≤500 字符/条），`snippet_max_chars` 退化为跨 provider 的统一兜底 |
| 上下文成本上界 | 按 Tavily 默认（10 条 × 3 chunks × 500 字符）≈ **15 KB ≈ 4k tokens/次搜索** —— 本层 `max_results=5` + `chunks_per_source=1` 可压到 **≈2.5 KB ≈ 650 tokens** |
| ⚠️ `include_raw_content` | **必须保持 `false`** —— `true` / `"markdown"` 返回整页清洗正文，是 token 爆炸的主要来源，且与 §3.5 的 snippet-only 护栏冲突 |
| ⚠️ API 默认 vs wrapper 默认 | Tavily **API** 的 `max_results` 默认是 **10**（非 5）；常见的「5」是 **LangChain `TavilySearchResults` wrapper** 的默认值。本层**恒显式下发** `max_results`，故不依赖任何一侧默认——但**不要**在文档或注释里把 5 写成「Tavily 默认」 |
| ⚠️ Rate limit | dev key 100 RPM / prod key 1000 RPM — 文档标「未验证（请复核）」 |

响应映射：

| 内部字段 | JSON path |
|---------|----------|
| `title` | `results[].title` |
| `url` | `results[].url` |
| `snippet` | `results[].content` |
| `score` | `results[].score`（**三家中唯一有**） |
| `published_date` | `results[].published_date`（RFC2822，如 `"Tue, 11 Mar 2025 17:00:00 GMT"`） |
| `reported_credits` | `usage.credits` |

**不读 `results[].raw_content`**（需 `include_raw_content: true` 才有值，本层硬编码 `false` —— snippet-only 护栏，§3.5）。
**不读 `answer`**（Tavily 的 LLM 总结；阶段 1 只要 results 列表，避免把厂商的综合结果当事实注入）。

#### 3.3.3 博查 Bocha（§2.21）—— ⛔ **未实现，接入参考附录**

> **本 Stage 不写 `websearch/bocha.rs`**。以下为已核实的接入资料（含双域名待确认项），保留以便将来纯增量接入。


| 项 | 值 |
|----|----|
| Endpoint | ⚠️ **双域名并存** —— 官方文档为 `POST https://api.bocha.cn/v1/web-search`（[已验证]），官网首页示例仍写 `https://api.bochaai.com/v1/web-search`。调研结论「以文档的 `bocha.cn` 为准」→ **代码默认 `api.bocha.cn`，`base_url` 可覆盖**，实现时两个域名各打一次探针确认 |
| Auth | `Authorization: Bearer {API KEY}`（[已验证]） |
| 请求体 | `{"query": <q>, "freshness": "noLimit", "count": <n>, "summary": false}` |
| `count` 上限 | 50（[已验证]）→ 与配置 `max_results` 一起 clamp |
| 成本 | ¥0.036/次 = ¥36/1k ≈ **$5.07/1k**（[已验证]） |

响应映射（**Bing Web Search 兼容** → 解析器命名为 `parse_bing_shaped`，可复用于任何 Bing-shaped API，厂商调研 §3.3 第 3 条称其为「性价比最高的实现杠杆」）：

| 内部字段 | JSON path |
|---------|----------|
| `title` | **`webPages.value[].name`**（⚠️ 是 `name` 不是 `title`） |
| `url` | `webPages.value[].url` |
| `snippet` | `webPages.value[].snippet` |
| `published_date` | `webPages.value[].datePublished`（ISO8601） |
| `score` | **无** → 恒 `None` |

**不读 `summary`**（需 `summary: true`，是长摘要，会显著推高注入 token）→ 请求体硬编码 `summary: false`，snippet-only 护栏。

### 3.4 归一化规则（provider 共用）

| # | 规则 | 做法 | 依据 |
|---|------|------|------|
| 1 | `published_date` 不解析 | 保留**原样字符串**，`Option<String>` | 厂商日期形态三分裂（Tavily RFC2822 / Bocha ISO8601 / **SearXNG ISO8601 但 [实测] 80% 为 null** / Serper 相对时间串）。厂商调研 §3.3 第 5 条要求「解析失败降级为 None，不要 panic」—— 本层直接不解析，把解析留给展示侧，彻底消除 panic 面 |
| 2 | 空 `url` / 空 `title` 的结果丢弃 | 过滤 | 无 URL 的结果无法产引用，注入后模型会编造链接 |
| 3 | **空 `snippet` 容忍（不丢弃）** | `snippet` 为空串时保留该条，不 panic、不过滤 | **[实测] SearXNG 35 条中 2 条 `content` 为空**（sogou 居多）。title + url 本身已有信息量，丢弃会无谓减少条数 |
| 4 | `url` 去重 | 按 `url` 保序去重 | Higress `ai-search` 按 `result.Link` 去重（架构调研 §1.2）。[实测] SearXNG 该实例自身已去重（35/35 唯一），但多引擎聚合不保证恒如此 |
| 5 | ⭐ **按 `score` 全局降序重排** | `sort_by(\|a,b\| b.score.unwrap_or(0.0).partial_cmp(&a.score.unwrap_or(0.0)))`，**稳定排序**保留同分原序 | ⚠️ **[实测] SearXNG 的 `results[]` 是「每引擎各自 position」交错排列，不是全局降序**（score 实测序列 `1.0,1.0,0.5,0.5,0.33,0.33,0.33,0.25,0.25,`**`1.0`**`,...`——第 10 条跳回 1.0）。**不重排就截断 = 丢掉第三个引擎的最佳结果（score=1.0），注入给模型的是「各引擎头部混合」而非最相关结果。** Tavily/Bocha 本身有序，重排对它们是幂等的（故接入时可共用同一归一化） |
| 6 | 结果条数截断 | 重排后 `truncate(effective_max_results)` | **顺序（强制）：解析 → 过滤(2,3) → 去重(4) → 重排(5) → 截断(6)**。重排必须在截断**之前**（否则等于没排），去重必须在重排**之前**（否则重复项占据名额） |
| 7 | `snippet` 单条字符上限 | 截断到 `snippet_max_chars`（默认 2000） | OpenRouter 的 Exa highlights 实测 ~2000-4000 字符/结果（架构调研 §5.5）。[实测] SearXNG snippet 天然短（max 184 字符）→ 该规则对 SearXNG 实际不触发，主要服务 Tavily/Bocha |

> **规则 5 的 `score` 语义警告**：SearXNG 的 score 是**引擎内 `1/position` 量级**（1.0 / 0.5 / 0.33 / 0.25…），不是跨引擎可比的相关性分。重排后「bing 第 1」与「yandex 第 1」并列 1.0，**同分顺序由稳定排序保留为原始引擎顺序** —— 这是可接受的近似（无更好信号），但**不要把它当真实相关性**，也不要在前端展示为「相关度」。

### 3.5 服务端护栏（客户端与模型均不可覆盖）


**设计原则照抄 Vercel**：server tool 的 `config` 字段是 **developer 默认值，覆盖模型生成的值**，官方说明这是**防 prompt 注入**措施（架构调研 §1.3 两处设计点第 1 条 / §5.6 末）；litellm 同款思路是把 `max_agentic_loops` 放进 untrusted-field list（架构调研 §2.2）。

| 护栏 | 默认 | 实现位置 | 客户端能否改 |
|------|------|---------|------------|
| `max_results` | **5** | `WebSearchConfig.max_results`，本层对入参 clamp | ❌ 只能在配置上限内**下调**（Stage 135 的 `search_context_size` 映射），不能上调 |
| snippet-only（禁全文） | 强制 | provider 请求体硬编码不要全文（SearXNG 无全文开关，天然满足；接入 Tavily/Bocha 时为 `include_raw_content:false` / `summary:false`），且解析器不读全文字段 | ❌ 无入口 |
| 单次搜索超时 | **5000ms** | `build_search_client` 的 `reqwest` timeout | ❌ 无入口 |
| `allowed_domains` / `blocked_domains` | 空 | 配置项；厂商不支持该参数时在本层**后置过滤**结果 | ❌ 无入口 |
| `snippet_max_chars` | 2000 | §3.4 截断 | ❌ 无入口 |

**`allowed_domains` 与 `blocked_domains` 同时非空 → 配置校验期报错拒绝启动**（与 Anthropic 官方约束一致：两者同时传 → 400，架构调研 §4.4）。校验在 `WebSearchConfig::validate()` 里做，照 litellm「配置在加载期校验，非法值直接拒绝启动而不是运行时才炸」（架构调研 §2.2）。

### 3.6 配置 schema

`AigwConfig` 新增字段（插在 `crates/aigw-core/src/config.rs:60` 的 `cache` 之后）：

```rust
    /// 内建 web search（Phase 53）。缺省（absent）→ 禁用，零行为变化。
    #[serde(rename = "web_search", skip_serializing_if = "Option::is_none")]
    pub web_search: Option<WebSearchConfig>,
```

`config.example.yaml` 注释态示例块（照 `cache` / `body_archive` 的注释态惯例）：

```yaml
# web_search:
#   enabled: true
#   default_provider: searxng          # 必须出现在 providers[].name 中
#   failover_order: [searxng]          # provider 之间的转移链；单 provider 时为 no-op(见下注)，仍被解析与校验
#   max_results: 5                     # 服务端上限，客户端不可上调
#   timeout_ms: 5000                   # 全局默认单次搜索超时
#   retries: 1                         # 传输层重试次数
#   snippet_max_chars: 2000
#   allowed_domains: []                # 与 blocked_domains 互斥
#   blocked_domains: []
#   proxy_url: ""                      # 空 → 直连；非空 → 走代理出网
#   allowed_fails: 3                   # 实例级:连续失败达此数进入冷却(照 router.rs:455)
#   cooldown_secs: 60                  # 实例级:冷却时长(照 router.rs:456)
#   providers:                         # 逻辑后端 N 元数组；kind 为判别式,未知 kind → 启动失败并列出支持列表
#     - name: searxng
#       kind: searxng                  # 当前支持：searxng
#       # cost_per_query 缺省 0.01 USD/次(= $10/1k,OpenAI 牌价占位)。
#       # 该默认值【不是 SearXNG 的真实自建成本】——自建 ≠ 零成本,但通常
#       # 远低于 0.01。请按 (服务器 + 带宽 + 运维) / 预估月查询量 摊销后改写
#       # (自建量级多在 1e-4,如 0.0002)。不改则【高估】成本。
#       # 显式填 0.0 → 金额为 0,但行与预算调用照样发生(Stage 136 §3.9a)。
#       cost_per_query: 0.0002
#       timeout_ms: 8000               # 覆盖全局：SearXNG 实测 2.0-3.1s，5000 偏紧
#       instances:                     # 物理端点 N 元数组：可用性 + 分流
#         - base_url: http://searxng-a.internal:8080
#           weight: 2                  # 任一实例声明 weight → 全部走加权随机；weight:0 = 排除
#           enabled: true
#         - base_url: http://searxng-b.internal:8080
#           weight: 1
#           enabled: true
#           # api_key: v2:gcm:...      # per-instance；SearXNG 免鉴权故留空
#     # —— 以下为未实现 provider 的接入形态示意(Stage 134 不支持这些 kind) ——
#     # - name: tavily
#     #   kind: tavily
#     #   cost_per_query: 0.008
#     #   instances:
#     #     - base_url: https://api.tavily.com
#     #       api_key: v2:gcm:...      # 明文 / v2:gcm: 密文 均可
#     # - name: bocha
#     #   kind: bocha
#     #   cost_per_query: 0.00507
#     #   instances:
#     #     - base_url: https://api.bocha.cn
#     #       api_key: v2:gcm:...
```

> **两级结构（provider → instances）是本次范围修正的核心。** 原先「每个 provider 一个 `base_url`」无法表达 SearXNG 的典型部署（多实例做可用性与分流）。现在：`provider` 承载**逻辑属性**（`kind` / 定价 / 超时），`instance` 承载**物理属性**（`base_url` / `api_key` / `weight` / `enabled`）。选择逻辑见 §3.8。
>
> **`failover_order` 本期是 provider 级 no-op，但仍必须解析 + 校验。** 只配了 SearXNG 一家时转移链长度为 1，**provider 级故障转移在生产中永不触发**；而**实例级冷却与加权在生产中是真实生效的**（配了 2 个实例即生效）。保留 `failover_order` 是为了让「加第二家 provider」只改 YAML 不改代码；其正确性由 §4.1 的 StubProvider UT 独立证明（§3.8）。
>
> **`api_key` 本期无真实消费者。** SearXNG 免鉴权，per-instance `api_key: Option<String>` + `decrypt_litellm_value` 链路保留（不删），但只有 StubProvider UT 走过 —— 风险已登记 §8.1。
>
> **`kind` 未知值必须报错并列出支持列表**，例如 `unsupported provider kind "tavily" for provider "x"; supported kinds: searxng`。这使「接入新 provider」成为纯增量改动（枚举加一个分支，错误信息自动包含它）。
>
> **配置源与结构解耦（Stage 138 前置）**：`WebSearchConfig` / `WebSearchProviderConfig` / `WebSearchInstanceConfig` 只是**普通可反序列化结构**，`build_websearch_registry` 接受**已解析好的结构**而非自己读文件 → Stage 138 从 DB 行构造同样的结构即可复用全部校验与装载逻辑，零重写。

> **`timeout_ms` 需支持 per-provider 覆盖（本次实测新增的要求）**：[实测] SearXNG `30.184.60.216:9099` 延迟 **2.0–3.1s**（5 次连打 mean ≈ 2.4s），因为它要等 bing+yandex+sogou 三个引擎全部回包；而 Tavily 是 ~1s 级。单一全局 5000ms 会让「SearXNG 偶发慢查询」被误判为超时并触发不必要的 failover。→ `WebSearchProviderConfig` 加 `timeout_ms: Option<u64>`，缺省回落全局值。


**`cost_per_query` 在本 Stage 只被解析与存入运行期 registry，不参与任何计算** —— 计费接线归 Stage 136。放在本 Stage 是为了避免 Stage 136 再动一次配置结构。其语义是**部署方应按自建摊销成本改写的单价**，**缺省为 `0.01` USD/次**（= $10/1k，对齐 OpenAI 牌价与 sub2api `174_group_web_search_price_per_call.sql` 的 `0.01` 默认）。因缺省即非零，**非零计费路径在生产默认就可达**（不是「UT-only」）；但反过来要注意：**该默认值是牌价占位而非 SearXNG 真实自建成本，不改会高估**（§8.1）。

**装载**：`config_loader.rs` 新增纯函数 `build_websearch_registry(cfg: &Option<WebSearchConfig>, master_key: &str) -> Option<WebSearchRegistry>`，照 `build_router_config`（`config_loader.rs:131`）的写法。**该函数只接受已解析好的结构、不读文件** → Stage 138 从 DB 行构造同一结构即可零重写复用：

1. `None` 或 `enabled: false` → 返回 `None`（整层零成本，照 `CacheConfig.enabled` 的「master switch」语义，`config.rs:67-69`）；
2. **每个 instance** 的 `api_key` 过一遍 `crypto::decrypt_litellm_value`（`crypto.rs:50`）—— 密文解密、明文原样（该函数对非密文安全拒绝，`crypto.rs:280-284`）；
3. `validate()`：`default_provider` 必须存在于 `providers[]`、`failover_order` 每项必须存在、**`kind` 必须是已支持值（否则错误信息列出支持列表）**、**每个 provider 的 `instances[]` 非空且至少一个 `enabled`**、`searxng` 的每个 instance 必须有 `base_url`、域名白黑名单互斥；任一不满足 → `Err` → **启动失败**；
4. 为每个 provider 构造一个共享的 `reqwest::Client`（`build_search_client`，§3.7）+ 一份 `WebSearchInstanceState` 表（§2.8），并装箱为 `Arc<dyn SearchProvider>`。

### 3.7 HTTP client

`websearch/client.rs`：

```rust
/// 组合 `probe::build_proxy_client`（代理）与 `Router::build_retry_client`（重试），
/// 超时由调用方给定（搜索发生在首字节之前，不能用 router 的 600s）。
pub fn build_search_client(
    proxy_url: Option<&str>,
    timeout: Duration,
    retries: u32,
) -> Result<reqwest_middleware::ClientWithMiddleware, String>
```

- 代理分支逐字复用 `probe.rs:27` 的 `reqwest::Proxy::all(proxy_url)`（同样**不开** `insecure_skip_verify`）；
- 重试逐字复用 `router.rs:540-547` 的 `ExponentialBackoff::builder().build_with_max_retries(retries)` + `RetryTransientMiddleware`（仅 5xx/网络错误重试，4xx 不重试）；
- **client 在 registry 构造期创建一次并共享**，不像 `build_retry_client` 那样每请求新建（`router.rs:535` 的已知缺陷，搜索路径在首字节前，连接复用对 TTFB 直接可见）；
- 置于 `#[cfg(feature = "reqwest")]` 之下，与 `router.rs:534` 同款（`reqwest` 是 `aigw-core` 的 default feature，见 `crates/aigw-core/Cargo.toml` `[features]`）。

### 3.8 两级选择：provider 级 failover ⊕ 实例级加权与冷却

```
┌─ provider 级（WebSearchRegistry，mod.rs）────────────────────────────┐
│ pick provider: 调用方指定 name（Stage 135 从 model_info 读）          │
│   → 不存在/未指定 → config.default_provider                          │
│   → 该 provider 彻底失败（Transport / Http 5xx / Timeout）            │
│       → 按 failover_order 顺序试下一个 provider                      │
│   → 全部失败 → Err(最后一个错误)，每次失败记 tracing::warn            │
│                                                                      │
│   └─ 实例级（instance.rs，provider 内部）────────────────────────┐    │
│      pick instance: 过滤 !enabled                                │    │
│        → 过滤 cooldown_until > now          (router.rs:79-84)    │    │
│        → 若全部冷却 → 取 cooldown_until 最早者 + warn             │    │
│                                             (router.rs:366-377)  │    │
│        → has_weight(任一 weight>0) ? 加权随机 : 均匀随机          │    │
│                                             (router.rs:366/419)  │    │
│      执行后：成功 → report_success(清零 + 解冷却)                 │    │
│              失败 → report_failure(++fail_count；≥allowed_fails  │    │
│                     → cooldown_until = now + cooldown_secs)      │    │
│                                             (router.rs:450-472)  │    │
│      实例耗尽（全部失败）→ 向上抛，触发 provider 级 failover        │    │
│   └──────────────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────────┘
```

| 层 | 职责 | 粒度 | 本期是否在生产生效 | 由谁证明 |
|----|------|------|------------------|---------|
| **provider 级** | 跨逻辑后端的能力兜底（SearXNG 挂 → 转 Tavily） | `failover_order` 的 provider 名 | ❌ 只配 1 家 → 链长 1，**永不触发** | **仅 StubProvider UT**（§4.1） |
| **实例级** | 同一后端的多物理端点可用性与分流 | `instances[]` 的 `base_url` | ✅ 配 2 个 SearXNG 实例即真实生效 | SearXNG 本地 stub 集成测试 + 纯函数 UT |

- 两级**语义不重叠**：实例级处理「同一家的某台机器挂了」（换机器重试，语义等价），provider 级处理「这一家整体不可用」（换厂商，语义可能降级）；
- 实例级逻辑**逐字照 `router.rs`**（§2.8 对照表），不自创 —— 运维对 `weight` / `cooldown` 的心智模型与 `Router` 完全一致；
- 兜底链形态照 litellm 的三级兜底（`handler.py:1638-1672`：指定名 → router 第一个 → `perplexity`），但**不内置任何「兜底厂商」硬编码**（litellm 兜底到 `perplexity` 是因它自带 21 个后端；aigw 只有配置里声明的）；
- **`Http 4xx` 两级均不转移**（配额耗尽/key 失效换机器或换厂商都救不了当次请求语义，且会放大账单）—— 只有 `Transport` / `Timeout` / `Http 5xx` 触发转移与冷却计数（照 `router.rs:451` 的 `is_cooldown_status` 门禁）；
- `failover_order` 为空 → provider 级不转移，单后端直返错误（阶段 1 最小可用形态，架构调研 §5.8 末「阶段 1 不必做全套」）；
- **不做配额加权 LB / Redis 预留**（sub2api `manager.go` 的 `selectByQuotaWeight` + Lua `quotaIncrScript`），登记 §8.2。注意本层的 `weight` 是**流量分配权重**（`router.rs` 语义），不是配额感知权重。但 sub2api 的一条取舍**本 Stage 就定下来**：**基础设施不可用时放行而非拒服务**（架构调研 §5.8 末）→ 本层无 Redis 依赖、且「全部冷却取最早恢复者」同向，天然满足。

---

## 4. TDD 计划

### 4.1 单元测试（`crates/aigw-core/src/websearch/` 各文件内 inline `#[cfg(test)] mod tests`）

遵循仓库约定（`aigw-core` 全部 inline test mod，见落地基线 §E3）。**SearXNG 解析 UT 用录制 fixture 常量，零实网调用** —— fixture 取 `docs/fixtures/searxng-search-response-2026-10-06.json`（真实抓取）内联为 `const SEARXNG_FIXTURE: &str = ...;`，与 `adapter.rs` 的 Codex 抓包 fixture 常量同款（Stage 131 §5）。

> ⭐ **`StubProvider` 是本 Stage 抽象可信度的唯一证据来源。** 定义在 `websearch/mod.rs` 的 `#[cfg(test)] mod tests` 内，实现 `SearchProvider`（含 `name()` / `cost_per_query()` / `search()`），按脚本返回预设 `Ok` / `Http{4xx}` / `Http{5xx}` / `Timeout` 并记录调用次数，不起 HTTP。它充当「**第二个 provider**」—— 替代被移出本 Stage 的 Tavily/Bocha，用来覆盖**只有多 provider 才能触达的分支**：trait 多态（`Arc<dyn SearchProvider>` 异构存放）、provider 级 failover 顺序、4xx 不转移、`api_key` 解密的真实消费、**非零 `cost_per_query`**。同时 `StubProvider` 挂多个 instance 用来测实例级加权与冷却。
>
> **凡「SearXNG 单家无法构造」的分支，一律由 `StubProvider` 覆盖，不得以「本期不会发生」为由跳过** —— 否则抽象就是 vaporware。

**SearXNG 响应解析与请求构造（7 条，`websearch/searxng.rs`）**

| UT | 输入 | 期望 |
|----|------|------|
| `test_searxng_parse_response_snippet_from_content` | SearXNG json fixture（**2026-10-06 自 `30.184.60.216:9099` 真实抓取固化**） | `snippet`←`results[].content`（**不是 `snippet` 键**） |
| `test_searxng_parse_response_missing_optional_fields` | 结果项仅有 `title`/`url`/`content` | `score`/`published_date` 均 `None`，不报错 |
| `test_searxng_parse_response_tolerates_empty_content` | **[实测] 真实 fixture 中含 2 条 `content:""`** | 该条**保留**（不丢弃、不 panic），`snippet` 为空串 |
| `test_searxng_parse_response_null_published_date` | **[实测] 真实 fixture 中 80% 为 `publishedDate:null`** | `published_date == None`，不报错 |
| `test_searxng_map_error_403_hints_json_format` | status 403 | 错误 message 含「settings.yml」「json」提示串 |
| `test_searxng_parse_response_rejects_html_body` | HTML 文本（**[实测] `format=xml` 或省略 `format` 时实际返回 HTTP 200 + HTML**） | `SearchError::Parse`，且 message 含「base_url / format=json」排查提示；**非 panic、非裸 serde 错误** |
| `test_searxng_build_request_format_json` | — | `GET {base_url}/search`，query 含 `format=json`；**无** `Authorization` header；**不下发 `results`/`limit`**（[实测] 被忽略，发了是噪音） |

**归一化（9 条，`websearch/types.rs` 或 `mod.rs`）**

| UT | 输入 | 期望 |
|----|------|------|
| `test_normalize_default_max_results_is_five` | `SearchRequest{max_results:None}` + 10 条结果 | 返回 5 条 |
| `test_normalize_dedup_by_url_preserves_order` | 同 url 出现在 1/3 位 | 保留第 1 条，顺序不变 |
| `test_normalize_drops_results_without_url` | 1 条无 `url` | 该条被丢 |
| `test_normalize_keeps_results_with_empty_snippet` | 1 条 `snippet:""`，title/url 齐全 | **该条保留**（规则 3） |
| `test_normalize_truncates_snippet_to_limit` | snippet 长 5000 | 长度 == `snippet_max_chars`（2000） |
| `test_normalize_dedup_before_truncate` | 7 条含 3 组重复、`max_results=5` | 去重后 4 条全部返回（证明顺序是去重先于截断） |
| ⭐ `test_normalize_sorts_by_score_desc_before_truncate` | **[实测] 真实 SearXNG 交错序列** score `[1.0,1.0,0.5,0.5,0.33,0.33,0.33,0.25,0.25,1.0,...]`、`max_results=5` | 返回的 5 条 score 为 `[1.0,1.0,1.0,0.5,0.5]` —— **即第 10 条（第三引擎的 score=1.0）必须出现在结果里**。这是本 UT 的全部意义：锁住「不重排就会丢掉它」的回归 |
| `test_normalize_sort_is_stable_for_equal_scores` | 3 条同 score=1.0，engine 顺序 bing/yandex/sogou | 重排后顺序不变（稳定排序，不引入随机性） |
| `test_normalize_sort_treats_none_score_as_lowest` | 混合 `Some(0.5)` 与 `None` | `None` 项全部排在末尾，不 panic（`partial_cmp` 对 `None` 的处理） |


**护栏（4 条）**

| UT | 期望 |
|----|------|
| `test_guardrail_clamps_caller_max_results_to_config` | 入参 `max_results=50`、配置 `max_results=5` → 实际 5（**客户端不能上调**） |
| `test_guardrail_allows_caller_to_lower_max_results` | 入参 3、配置 5 → 3 |
| `test_guardrail_blocked_domain_filtered_post_hoc` | 结果含 `reddit.com` 且 `blocked_domains=["reddit.com"]` | 该条被过滤（含子域） |
| `test_guardrail_allowed_domain_whitelist_only` | `allowed_domains=["example.com"]` | 仅保留 example.com 及子域 |

**配置解析与校验（8 条，`websearch/config.rs` + `config_loader.rs`）**

| UT | 期望 |
|----|------|
| `test_websearch_config_absent_disables_layer` | `AigwConfig` 无 `web_search` → `build_websearch_registry` 返回 `None` |
| `test_websearch_config_parse_full_yaml` | §3.6 完整 YAML → 字段逐项等值；`providers.len()==1`、`providers[0].instances.len()==2` |
| `test_websearch_config_defaults_applied` | 仅给 `enabled`/`default_provider`/`providers` → `max_results==5`、`timeout_ms==5000`、`snippet_max_chars==2000`、`allowed_fails==3`、`cooldown_secs==60` |
| `test_websearch_config_validate_unknown_default_provider` | `default_provider` 不在 `providers[]` | `Err`（启动失败） |
| `test_websearch_config_validate_domain_lists_mutually_exclusive` | allowed 与 blocked 同时非空 | `Err` |
| ⭐ `test_websearch_config_unknown_kind_error_lists_supported_kinds` | `kind: tavily`（本期未实现） | `Err`，message **同时含** `"tavily"` 与支持列表 `"searxng"` —— 锁住「加 provider 是纯增量」的契约：新增枚举分支时该消息自动包含它 |
| `test_websearch_config_validate_rejects_empty_instances` | 某 provider 的 `instances: []`（或全部 `enabled: false`） | `Err`（启动失败，不留运行期空池） |
| `test_websearch_config_provider_timeout_overrides_global` | 全局 5000 + provider 8000 | 该 provider 生效 8000，未覆盖者回落 5000 |

**实例级选择与冷却（7 条，`websearch/instance.rs`，纯函数 + 注入时钟）**

> 语义逐字对照 §2.8 的 `router.rs` 实现；UT 用 `StubProvider` 挂多实例驱动。

| UT | 期望 |
|----|------|
| `test_pick_instance_skips_disabled` | 2 实例其一 `enabled: false` | 恒选另一个 |
| `test_pick_instance_filters_cooldown` | 实例 A `cooldown_until = now + 60s` | 恒选 B（照 `router.rs:79-84`） |
| `test_pick_instance_all_cooldown_returns_earliest_recovery` | A/B 均在冷却，A 更早恢复 | **返回 A 而非 `None`**（照 `router.rs:366-377`：放行而非拒服务），并记 `warn` |
| `test_pick_instance_uniform_when_no_weight` | 均无 `weight` | 两实例均可被选到（多次采样覆盖） |
| `test_pick_instance_weighted_when_any_weight_declared` | A `weight:2` / B `weight:1` | `has_weight` 为真 → 加权随机；大样本下 A 命中显著多于 B（照 `router.rs:419`） |
| `test_pick_instance_excludes_zero_weight` | A `weight:0` / B `weight:1` | 恒选 B（`weight:0` 排除，照 `weighted_pick`） |
| `test_report_failure_enters_cooldown_after_allowed_fails` | `allowed_fails=3`，连续 3 次 5xx | 第 3 次后 `cooldown_until` 被置；中途 `report_success` 清零且解冷却（照 `router.rs:450-472`）；**4xx 不计数**（`is_cooldown_status` 门禁，`router.rs:451`） |

**provider 级故障转移与抽象多态（6 条，`websearch/mod.rs`，全部基于 `StubProvider`）**

| UT | 期望 |
|----|------|
| `test_registry_selects_default_provider` | registry 含 2 个异构 `Arc<dyn SearchProvider>`，未指定 name | 选中 `default_provider`；**同时证明 trait object-safe 且可异构存放** |
| `test_registry_failover_on_5xx_and_timeout` | 首选返 `Http{5xx}`（另一例返 `Timeout`），次选成功 | 返回次选结果，`response.provider == 次选名`；首选被调用恰 1 次 |
| `test_registry_no_failover_on_4xx` | 首选返 `Http{status:401}` | 直接 `Err`，次选**调用计数为 0** |
| `test_registry_all_providers_failed` | 全部 `Timeout` | `Err(Timeout)`，错误来自 `failover_order` 最后一个 |
| ⭐ `test_provider_api_key_decrypted_from_v2_gcm` | stub provider 的 instance `api_key` = `encrypt_litellm_value_gcm("sk-test-xxx", mk)` 的产物；另一 instance 为明文 | registry 内分别为 `"sk-test-xxx"` 与原明文 —— **SearXNG 免鉴权，这是该解密链路唯一的消费者** |
| ⭐ `test_calc_cost_with_nonzero_cost_per_query_via_stub` | stub `cost_per_query() == 0.008`，执行 2 次查询 | 累计携带金额 `== 0.016`（本 Stage 只断言**携带与读取正确**，不接 SpendLog → Stage 136）；此 UT 独立于 provider 默认值，锁住「单价 × 次数」的算术与携带 |

**合计新增 UT：41 个**（7 SearXNG 解析+构造 + 9 归一化 + 4 护栏 + 8 配置 + 7 实例级 + 6 provider 级/多态）；**加 §4.3 的 4 个集成 stub 测试 = 新增测试 45 个**。

> 对比原规划（33 UT，含 Tavily/Bocha）：移出两家厂商的 10 个解析/构造 UT，新增 2 配置 + 7 实例级 + 3 provider 级/多态 + 1 归一化 —— **净增 8**。覆盖面不降反升：被移除的是「第二、三家的字段映射」（重复性最高），新增的是「抽象与选择逻辑」（风险最高）。

### 4.2 BDD

**本 Stage 无 BDD 面** —— 没有请求管线接线，没有可从 HTTP 端点观察的行为变化。`task bdd` 的场景数必须**不变**（这本身是一条门禁：证明新模块对既有行为零影响）。

**mock 搜索厂商方案在此定稿，代码随 Stage 135 落地**（Stage 135 才有消费者）：

现有 `MockUpstream`（`crates/aigw-server/tests/bdd_support/mock_upstream.rs:189`）是「单 axum server + **按 HTTP path 分发**的罐头响应表」：

- 路由注册在 `mock_upstream.rs:202-210` 的 `Router::new()` 链；
- 罐头响应注册 `MockState::set_response(path, response)`（`mock_upstream.rs:145-149`）；
- **分发纯按 path，零 model 路由** → 搜索厂商各占一个 path，不需要任何新机制。

因此方案是：在 `mock_upstream.rs:202-210` 的链上追加路由（**本期只需 SearXNG 一条**），handler 照抄 `openai_handler` 的结构（先 record request，再查 `responses` map）：

| 厂商 | mock path | 方法 |
|------|-----------|------|
| SearXNG（本期） | `/search` + `format=json` query | `get` |
| ~~Tavily~~ | ~~`/search`~~ | 未实现，接入时再加 |
| ~~博查~~ | ~~`/v1/web-search`~~ | 未实现，接入时再加 |

场景里把 `web_search.providers[].instances[].base_url` 指向 `mock.url()` 即可。已有的 `set_response_first_n`（`mock_upstream.rs:298`）可直接用于「首个实例失败、转移到下一实例」的 BDD 场景（Stage 135）。

### 4.3 集成验证

1. **本地 stub HTTP 往返（`#[tokio::test]`，不出网）** —— `websearch/mod.rs` 的 test mod 内起一个临时 axum server（`aigw-core` 已直接依赖 `axum`，见 `crates/aigw-core/Cargo.toml`），绑 `127.0.0.1:0`，回放 §4.1 的 fixture，驱动真实的 `SearchProvider::search`（含 `build_search_client`）跑一遍。断言：endpoint path 命中、query 含 `format=json`、解析结果与纯函数 UT 一致。**这是唯一验证「IO 薄壳 + 纯函数」接缝的测试**：
   - `test_searxng_search_end_to_end_against_local_stub`
   - ⭐ `test_searxng_multi_instance_failover_against_local_stubs` —— 起**两个** stub server，第一个恒 500，断言请求落到第二个且第一个进入冷却（这是**实例级**逻辑的端到端证据，本期生产会真实走到）
   - `test_searxng_all_instances_down_returns_last_error` —— 两个 stub 均 500，断言 `Err` 且两者均被尝试
2. **超时路径** —— stub 故意 sleep 超过 `timeout_ms`，断言返回 `SearchError::Timeout` 而非挂死：`test_search_timeout_returns_timeout_error`。
3. **人工实网探针** —— SearXNG **已于 2026-10-06 完成**（见下），本期无其他待做项：
   - ~~SearXNG 自建实例的 `results[].score` / `publishedDate` 是否真实存在~~ → ✅ **已实测**：两者均存在，`score` 35/35 非空、`publishedDate` 80% 为 `null`。另发现 5 项与原假设不符的行为，已全部回写 §3.3.1 与 §3.4。**fixture 已固化**：`docs/fixtures/searxng-search-response-2026-10-06.json`（真实响应，35 条，28KB）—— 实现时直接内联为 `SEARXNG_FIXTURE` 常量，**不要再手写假数据**（手写的交错 score 序列无法复现「第 10 条跳回 1.0」这一真实形态，而那正是重排 UT 的核心断言）。
   - **未实现，接入时再做**：博查 `api.bocha.cn` 与 `api.bochaai.com` 两个域名各自可达性与响应一致性；
   - **未实现，接入时再做**：Tavily dev key 的真实 RPM（文档未验证）+ `chunks_per_source=1` 的实际字符上限是否确为 ≤500。

**SearXNG 实测记录（2026-10-06，`http://30.184.60.216:9099`）**

| 项 | 结果 |
|----|------|
| 可达性 / JSON | ✅ HTTP 200，`format=json` 已开启 |
| 延迟（5 次连打） | 3.14 / 2.06 / 2.08 / 2.15 / 2.70 s（mean **2.43s**） |
| 速率限制 | 5 次连打无 429、`unresponsive_engines` 恒空 → 该实例未启 limiter |
| 启用引擎 | bing(10) + yandex(15) + sogou(10) = **35 条/次** |
| 字段完备性（35 条） | `title` 35/35、`url` 35/35 且唯一、`content` **33/35**（2 条空）、`score` 35/35、`publishedDate` **7/35** |
| 中文 | ✅ `量子计算 最新进展` → 29 条，首条百度百科 |
| 零结果行为 | ❌ **无空结果语义** —— 乱码 query 仍返 35 条无关结果 |
| 服务端限条数 | ❌ `&results=5` / `&limit=5` **均被忽略**；`&engines=bing` → 10 条 |
| `format` 容错 | ❌ `format=xml` / 省略 → **HTTP 200 + HTML**（静默降级）；`format=csv` 真实可用 |
| 空 query | HTTP 400 |
| 上下文成本 | 全量 4761 字符 ≈ 1.2k tokens；截断后 ≈ 170 tokens |


---

## 5. 变更清单

| 文件 | 改动 |
|------|------|
| `crates/aigw-core/src/websearch/mod.rs` | **新增** — `WebSearchRegistry`（provider 集合 + provider 级 pick/failover）+ re-export + **test-only `StubProvider`** + provider 级/多态 UT 6 个 + 集成 stub 测试 4 个 |
| `crates/aigw-core/src/websearch/types.rs` | **新增** — `SearchRequest` / `SearchResult` / `SearchResponse` / `DEFAULT_MAX_RESULTS=5` + 归一化 9 UT + 护栏 4 UT |
| `crates/aigw-core/src/websearch/provider.rs` | **新增** — `SearchProvider` trait（`async_trait`，object-safe，含 `cost_per_query()`）+ `SearchError`（`thiserror`） |
| `crates/aigw-core/src/websearch/instance.rs` | **新增** — `WebSearchInstanceState` + `pick_instance`（加权随机 + 冷却过滤 + 全冷却取最早恢复）+ `report_failure` / `report_success`，**逐字照 `router.rs:26/79-84/366-377/419/450-472`** + 实例级 UT 7 个 |
| `crates/aigw-core/src/websearch/config.rs` | **新增** — `WebSearchConfig` / `WebSearchProviderConfig` / `WebSearchInstanceConfig` + `validate()`（含未知 `kind` 列出支持列表、`instances[]` 非空）+ 默认值函数 + 配置 UT 8 个 |
| `crates/aigw-core/src/websearch/client.rs` | **新增** — `build_search_client`（代理 ⊕ 重试 ⊕ 超时），`#[cfg(feature = "reqwest")]` |
| `crates/aigw-core/src/websearch/searxng.rs` | **新增** — `SearxngProvider` + 真实 fixture（`docs/fixtures/searxng-search-response-2026-10-06.json`）+ 7 UT |
| `crates/aigw-core/src/lib.rs` | `pub mod websearch;`（模块声明区 `:18-48`，字母序置于 `tenant` 之后） |
| `crates/aigw-core/src/config.rs` | `AigwConfig` 新增 `web_search: Option<WebSearchConfig>`（紧随 `cache`，`:60` 之后） |
| `crates/aigw-core/src/config_loader.rs` | 新增 `build_websearch_registry`（纯函数，仿 `build_router_config`，`:131`）；文件头模块注释补一条装载原语说明（与 `:14-27` 的三原语并列） |
| `config.example.yaml` | 新增 `web_search:` 注释态示例块（§3.6） |
| `docs/stages/stage-134.md` | 本文件 |
| `docs/stages/stage-roadmap.md` | 新增 Phase 53 段落 + 进度条行 |
| `docs/11-next-steps.md` | Phase 53 / Stage 134 回写 |
| `docs/12-technical-debt.md` | TD-017c 状态更新为「进行中（Phase 53）」；新增 §8.2 的遗留条目 |

> `crates/aigw-core/Cargo.toml` **无需改动** —— `async-trait` / `thiserror` / `reqwest` / `reqwest-middleware` / `reqwest-retry` / `serde_json` / `axum`（dev 用）均已在依赖列表中。

---

## 6. 回归验证

1. `task test` 全绿，`aigw-core` UT **530 → 611（净增 81）**（规划 45，超额 36：多出的主要是错误分类、池行为、`config.example.yaml` 漂移防护与 4 个额外的集成 stub 场景）
2. `task bdd` — 实测 **285 场景 / 272 pass / 13 skip / 0 fail**，与 **Stage 133** 基线逐字一致（本文件原写 Stage 132 的 284，那是 Stage 133 交付前的数字）。零 `.feature` 文件改动，证明新模块对既有行为零影响
3. `task fmt` / `task lint` green，无新 clippy warning
4. `task doctor`（`cargo check --workspace` + clippy）通过。⚠️ **原文此处写 `task check`，该 task 不存在于 Taskfile** —— 已于 2026-10-07 订正 CLAUDE.md 与本文件。`--no-default-features` 门禁项**已随 `reqwest` feature 删除而失效**（见下 §7a 收尾）
5. `task doctor` 无新告警
6. 人工：SearXNG 实网探针**已完成**（§4.3 第 3 条），无其余待做项（Tavily/博查 探针随接入 Stage 再做）

---

## 7. 门禁

- [⚠️] **实际交付 81 个新 UT（规划 41 + §4.3 的 4 = 45，超额 36）**。⚠️ **未走严格 TDD 红绿**：测试与实现同批编写，改用**变异测试**反向证明断言有效——逐一注入 4 个缺陷（移除 score 重排 / 截断先于去重 / 4xx 改为可转移 / snippet 读 `snippet` 键而非 `content`），确认每个都被对应 UT 捕获后还原。该替代手段证明了断言非同义反复，但**不等于红绿流程**，如实记录。
- [x] SearXNG 的 fixture 解析 UT 通过，且 fixture 逐字取自 `docs/fixtures/searxng-search-response-2026-10-06.json`（真实抓取，非手写）
- [x] ⭐ **`StubProvider` UT 全部通过** —— trait 多态 / provider 级 failover / 4xx 不转移 / `v2:gcm:` 解密 / 非零 `cost_per_query`，即「多 provider 就绪」的可验证证据
- [x] ⭐ **实例级 7 个 UT 通过**，且语义与 §2.8 的 `router.rs` 对照表逐项一致（加权判定 / `weight:0` 排除 / 冷却过滤 / 全冷却取最早恢复 / 4xx 不计数）
- [x] 未知 `kind` 的配置错误信息**同时包含非法值与支持列表**
- [x] 集成 stub 测试 **7 个**（规划 4）：端到端 / 多实例转移 / 全实例挂 / 4xx 不外扩 / HTML-200 转移 / 超时 / 空 query 短路。⚠️ 其中 3 个最初因「实例选择是随机的」而 flaky（断言了「每实例恰一次」，但健康实例先被选中时故障实例根本不会被访问）——已改为断言**与顺序无关的不变量**（多轮循环 + 「故障实例确实在池中」+ 「每轮都由健康实例服务完成」），连跑 5 次稳定。
- [x] 配置缺省（absent）时 `build_websearch_registry` 返回 `None`，`task bdd` 场景数不变
- [x] `v2:gcm:` 密钥解密 UT 通过；明文 key 不被破坏
- [x] 配置非法值（未知 default_provider / 未知 kind / 空 instances / 域名列表互斥冲突）在加载期报错，不留到运行时
- [x] `build_websearch_registry` 只接受已解析结构、不读文件（Stage 138 的 DB 来源可零重写复用）
- [x] `task test`（**611 pass**，aigw-core 530→611）/ `task bdd`（**285 场景 272 pass 13 skip，与 Stage 133 基线逐字一致**）/ `task fmt` / `task lint` / `task doctor` 全绿。⚠️ **`task check` 在 Taskfile 中不存在**（CLAUDE.md 与本文件 §6 曾引用）→ **2026-10-07 已订正 CLAUDE.md**，并新增 `task test-filter` / `task fmt-fix` 补齐两处真实缺口。
- [~] `--no-default-features` 门禁项 **已作废** —— 2026-10-07 用户决策删除 `reqwest` feature（它从未真正可选：`alerts.rs` / `claude_oauth.rs` / `probe.rs` 无条件引用 `reqwest::`，该 profile 有 27 个既存错误）。现 `reqwest` 为必选依赖，22 处 `#[cfg(feature = "reqwest")]` 全部移除，该 flag 成为 no-op。
- [x] `config.example.yaml` 的 `cost_per_query` 注释写明「摊销单价而非采购价，默认值高估约 50 倍需改写，留 0 则成本不可见」。**另加一条 UT** `test_config_example_yaml_block_parses_into_this_struct`——把注释块反注释后真实反序列化进 `WebSearchConfig` 并跑 `validate()`，防止文档示例与结构漂移（运维唯一的拷贝来源若不可用，比没有更糟）。
- [x] `docs/11-next-steps.md` + `stage-roadmap.md` + `docs/12-technical-debt.md`（TD-017c）回写
- [x] git commit（精确 add；`--signoff`）

---

## 7a. 实施记录（2026-10-06 完成）

### 交付物

| 文件 | 行为 | UT |
|------|------|----|
| `websearch/mod.rs` | `WebSearchRegistry`（provider 级 pick + failover）+ `build_provider`（**唯一的 `kind` 分派点**）+ test-only `StubProvider` | 13 + 7 集成 |
| `websearch/types.rs` | 三个值类型 + `Guardrails` + `normalize`（6 步流水线） | 16 |
| `websearch/provider.rs` | `SearchProvider` trait（object-safe）+ `SearchError`（含 `is_retriable` / `provider` / `status`） | 2 |
| `websearch/instance.rs` | `pick_instance_with_roll`（纯函数）+ `InstancePool`（共享状态封装）+ `report_*` | 14 |
| `websearch/config.rs` | 三层配置结构 + `validate()` + `attempt_order()` | 12 |
| `websearch/client.rs` | `build_search_client`（代理 ⊕ 重试 ⊕ 超时） | 3 |
| `websearch/searxng.rs` | `build_url` / `parse_response` / `map_error` 三纯函数 + IO 薄壳 | 10 |
| `config_loader.rs` | `build_websearch_registry`（校验 + per-instance 解密 + client + pool） | 6 |

### 与规划的偏差（均为增量，无缩减）

1. **新增 `InstancePool`（规划未设计）** —— 规划只给了 `pick_instance` / `report_*` 自由函数，但 provider 需要**跨 await 持有可变实例状态**。若让每个 provider 自己管 `Mutex<Vec<State>>`，「加一家 provider = 1 文件」的承诺就会漏掉实例管理这块。`InstancePool` 把它收进 `instance.rs`，新 provider 只需持有一个 pool 并循环 `pick_excluding` / `report_*`。
2. **新增 `pick_excluding(tried)`** —— 规划的两级图里，「实例耗尽 → 向上抛触发 provider 级 failover」没有说明单次查询如何遍历多个实例。该方法让一次查询走完所有健康实例后才交给 provider 级。
3. **`SearchError` 多一个 `EmptyQuery` 变体** —— §3.3.1 实测要求「发请求**之前**拒绝空 query（省一次 RTT）」，但规划的枚举里没有能表达它的变体（硬塞进 `Http{400}` 会谎称发生过网络往返）。
4. **`SearchError::Parse` 判定为可转移** —— 规划未明确。理由：SearXNG 的「200 + HTML」本质是**该实例配置错误**（`format=json` 未开），兄弟实例可能是对的，所以应当换实例而非整体失败。已由 `test_searxng_html_200_is_a_parse_error_then_tries_next` 锁定。
5. **`SearchRequest` 不含 `allowed_domains` / `blocked_domains`** —— 规划 §3.2 把它们放进请求结构，但 §3.5 同时要求这两项是**客户端不可触及的服务端护栏**。放在请求里等于给了调用方一个入口，与护栏意图冲突。改为只存在于 `Guardrails`（由 registry 持有），请求结构里**没有这个字段可填**——结构性地不可绕过，优于靠纪律。
6. **`urlencode` / `url_host` 手写而非引入 `url` crate** —— 原始理由是「需在 `--no-default-features` 下编译」，该理由已随 feature 删除而失效；但保留手写仍然合理（`reqwest` 不导出独立 parser/encoder，两个函数各约 10 行且各有 UT，不值得加直接依赖）。注释已据实改写。

### 两个新发现（规划未预见）

| # | 发现 | 处理 |
|---|------|------|
| 1 | ⚠️ **`task check` 在 `Taskfile.yml` 中不存在** —— CLAUDE.md 的「纪律红线」与本文件 §6/§7 都引用了它，照做会直接失败。实际等价物是 `task doctor` | ✅ **2026-10-07 已收尾**：CLAUDE.md 订正为 `task doctor`，并把 `task check` 写进反例清单。另发现两处真实缺口并补齐：**`task test-filter -- <pattern>`**（过滤测试，此前只能裸命令）与 **`task fmt-fix`**（实际格式化，`task fmt` 只做 `--check`）。纪律文本同时补上「过滤结果不得充当交付依据」的明文边界 |
| 2 | ⚠️ **`cargo check -p aigw-core --no-default-features` 早已损坏** —— `alerts.rs` / `claude_oauth.rs` / `probe.rs` 共 **27 个** `unresolved crate reqwest` 错误，即 `reqwest` 已是事实必选依赖 | ✅ **2026-10-07 已收尾（用户决策：删除 feature）**：`aigw-core/Cargo.toml` 的 `reqwest`/`reqwest-middleware`/`reqwest-retry` 从 `optional` 改为必选，删掉 `[features] default = ["reqwest"]` 与 `reqwest = [...]`；`aigw-server` 两处 `features = ["reqwest"]` 依赖声明清理；全库 **22 处 `#[cfg(feature = "reqwest")]` 移除**（`router.rs` 2 / `config_loader.rs` 10 / `websearch/*` 10），`#[cfg(all(test, feature="reqwest"))]` → `#[cfg(test)]`。验证：`task test` **1052 pass / 0 fail**（与删除前逐字一致）、`task bdd` **285 场景**不变、`task doctor`/`fmt`/`lint` 全绿 |

### 验证方式的替代（须知）

**未执行严格的 TDD 红绿**：测试与实现同批编写。为补偿，做了**变异测试**——注入 4 个针对性缺陷并确认各自被捕获：

| 注入的缺陷 | 被捕获的 UT |
|-----------|-----------|
| 删除 `normalize` 的 score 重排 | `test_normalize_sorts_by_score_desc_before_truncate` + `..._none_score_as_lowest` |
| 把 `truncate` 提到去重之前 | `test_normalize_dedup_before_truncate` + 上条 |
| 把 4xx 改为可转移 | 4 个（`provider` / `registry_no_failover_on_4xx` / `searxng_4xx_*` / `map_error_403`） |
| `snippet` 改读 `snippet` 键而非 `content` | `test_searxng_parse_response_snippet_from_content` + `..._tolerates_empty_content` |

全部还原后复跑绿。这证明断言**非同义反复**，但**不等同于红绿流程**。

---

## 8. 风险与遗留

### 8.1 风险

| 风险 | 缓解 |
|------|------|
| ~~**SearXNG 的 `score` / `publishedDate` 字段未经证实**~~ | ✅ **2026-10-06 实测消除** —— 两者均真实存在；`score` 35/35 非空、`publishedDate` 仅 7/35 非空。解析器仍按 `Option` 处理（正确）。**但实测暴露了一个比字段缺失更严重的问题，见下一行。** |
| ⚠️⚠️ **SearXNG 的 `results[]` 按引擎交错而非全局 score 降序** —— 直接 `truncate(N)` 会把某个引擎的最佳结果（score=1.0）整条丢掉，注入给模型的是「各引擎头部混合」 | §3.4 规则 5 强制「截断前按 score 全局稳定重排」；`test_normalize_sorts_by_score_desc_before_truncate` 用**真实 fixture** 锁回归。**这是本次实测最有价值的发现**——纯看文档不可能发现 |
| ⚠️ **SearXNG 无「零结果」语义** —— 乱码 query 实测仍返 35 条无关结果（Subway 门店、Google 首页…），网关无法靠条数判断「搜不到」 | 本层不做相关性判断（无可靠信号）；依赖 Stage 135 注入模板里的「资料与问题无关时直接忽略」指引 —— 该指引因此**由礼貌性措辞升级为必需的防噪音护栏**（Stage 135 §3.5 已同步） |
| ⚠️ **SearXNG `format` 参数静默降级** —— `format=xml` / 省略时实测返回 **HTTP 200 + HTML** 而非 4xx | `parse_response` 对「200 但非 JSON」给出带排查提示的 `SearchError::Parse`（含 base_url / format=json），`test_searxng_parse_response_rejects_html_body` 覆盖 |
| ~~**博查双域名并存**~~（文档 `api.bocha.cn` vs 官网示例 `api.bochaai.com`） | **本 Stage 不适用** —— 博查未实现。资料保留于 §3.3.3，接入 Stage 须两域名各打探针确认后再写死默认值 |
| **SearXNG 公共实例普遍关闭 JSON format**（官方原话「many public instances have these formats disabled」）→ 403 | `map_error` 对 403 给出明确的 settings.yml 提示串；配置文档写明**必须自建**。[实测] 目标实例已开启 |
| **SearXNG 上游引擎脆弱**（抓取器随反爬变更而碎，官方把「Answer CAPTCHA from server's IP」列为管理员日常任务） | **本期靠实例级多端点兜底**（§3.8：多个 SearXNG 实例 + 加权 + 冷却）—— provider 级 failover 到 Tavily/博查**本期不可用**（未实现）。[实测] 当次 `unresponsive_engines` 为空，但该字段**值得记入 `tracing::debug`** 以便观测引擎逐步失效 |
| ⚠️ **`api_key` + `v2:gcm:` 解密链路本期无真实消费者** —— SearXNG 免鉴权，该路径**只有 StubProvider UT 覆盖**，生产从未端到端跑过。风险形态：配置看起来能用（字段存在、UT 绿）但首次接入付费 provider 时才暴露问题（如 master key 轮转、per-instance 覆盖语义、明文/密文混配） | 保留完整链路而非删字段（删了接入时要重做）；**首次接入付费 provider 的 Stage 必须把「实网验证 `v2:gcm:` 密钥端到端生效」列为门禁项**，不得沿用「UT 已绿」作为证据 |
| ⚠️ **`cost_per_query` 缺省 `0.01` 是行业牌价占位，不是 SearXNG 的真实自建成本 → 不改会系统性高估** —— $10/1k 是 OpenAI 的**对外售价**；自建摊销通常在 `1e-4` 量级（相差约 50 倍）。部署方若不改默认值，SpendLog 金额与预算扣减会被放大 | ① §3.6 配置注释显式标注「该默认值不是自建成本，请改写」并给摊销算法与典型量级；② 选 `0.01` 而非 `0.0` 的理由是**宁可高估也不要静默为 0**（成本可见 > 成本精确，且 0 会让「搜索花了多少」永远无法回答）；③ Stage 138 的 UI 在 SearXNG 项旁给同款提示与算例；④ 考虑在 Stage 138 加输入软校验（自建 kind 填 > `0.001` 时提示确认） |
| **provider 级 failover 本期永不触发** —— 只配 1 家 provider，转移链长 1。逻辑已实现但生产零覆盖 | 由 StubProvider UT 独立证明（§4.1 的 6 条）；**实例级转移在生产中真实生效**（配 2 个 SearXNG 实例即可），且有本地双 stub 集成测试（§4.3）—— 两级中至少一级有真实覆盖 |
| ~~**Tavily ToS §6.5 明确允许用 query 与结果训练 AI 模型**~~（厂商调研 §2.1 已逐字核实） | **本 Stage 不适用** —— Tavily 未实现，本期无用户可见合规提示义务。**该条款在 Tavily 接入 Stage 升为门禁**：必须在 `config.example.yaml` 的 Tavily 示例块旁写明，让管理员知情；敏感 query 场景建议配 SearXNG 自建或谈 Enterprise。已转记 §8.2 |
| ~~**Serper credits 6 个月过期 / Tavily 用尽即硬停**~~ | **本 Stage 不适用**（两者均未实现）。`cost_per_query` 与配额治理不在本 Stage；实例级冷却提供同一后端内的可用性兜底 |
| **搜索超时直接计入 TTFB**（设计 C 的搜索发生在首字节之前，架构调研 §5.4 末） | [实测] **SearXNG 实测 2.0–3.1s，是真实且不可忽略的 TTFT 增量**（它要等三引擎全部回包）→ per-provider `timeout_ms`（SearXNG 档建议 8000，§3.6）；共享 `reqwest::Client` 复用连接；超时返回 `SearchError::Timeout` 供 Stage 135 降级而非挂死。**本期唯一后端是 SearXNG，故该 ~2.5s 增量是生产常态，无法靠换 provider 规避** —— Stage 135 的降级策略文档必须写明 |
| **SearXNG 无法服务端限条数** —— `&results` / `&limit` 实测被忽略，恒收全量 ~30KB | 网关侧截断（§3.4 规则 6）保证注入量可控；但**网络与解析成本不可省** → 配置文档写明「SearXNG 省的是单次采购费，不是带宽与延迟」 |
| **新 config section 变成死配置**（`litellm_settings` 同类事故，`config_loader.rs:28` / TD-013） | 本 Stage 就交付可用的 `build_websearch_registry`（含校验 + 密钥解密 + 实例选择 + client 构造），不只加结构 |
| **配置结构将来要同时服务 yaml 与 DB 两种来源**（Stage 138），若本期把文件读取耦进装载函数则届时要重写 | `build_websearch_registry` 只接受已解析结构（§3.6 装载），不读文件、不碰 `AigwConfig` 以外的来源 → Stage 138 从 DB 行构造同一结构即可，门禁已登记 |


### 8.2 遗留（本 Stage 不做，登记 TD-017c 子项）

**首批待接入 provider（抽象已就位，接入 = 新增 1 文件 + 1 个 `kind` 分支）**：

| provider | 定价 | 独有价值 / 接入注意 |
|---------|------|-------------------|
| **Tavily** | **$8/1k**（`search_depth=basic` = 1 credit = $0.008） | ⭐ **`chunks_per_source`（默认 3，范围 1-3，每 chunk ≤500 字符硬上限）是全样本中唯一的「厂商侧压缩」参数** —— 可在厂商侧把单条压到 ≤500 字符，优于本层 `snippet_max_chars` 的事后截断（后者已付完整响应的网络与解析成本）。另：**三家中唯一自带 `score`**，延迟 ~1s 级（远优于 SearXNG 实测 2.4s），接入后**适合作 `default_provider` 以降 TTFT**。映射表见 §3.3.2。⚠️ **接入门禁**：ToS §6.5 明确允许用 query 与结果训练 AI 模型（已逐字核实）→ 必须在 `config.example.yaml` 的 Tavily 示例块旁写明该条款让管理员知情；敏感 query 场景建议留在 SearXNG 自建或谈 Enterprise。另须实网验证 dev key RPM（文档未验证）与 `chunks_per_source=1` 的实际字符上限 |
| **博查 Bocha** | ¥36/1k ≈ **$5.07/1k** | ⭐ **Bing Web Search 兼容形状** → 解析器 `parse_bing_shaped` 可复用于任何 Bing-shaped API（厂商调研称其为「性价比最高的实现杠杆」）；**中文检索质量最佳**。映射表见 §3.3.3。⚠️ **接入注意**：双域名并存（文档 `api.bocha.cn` vs 官网示例 `api.bochaai.com`），须两域名各打探针确认后再写死默认值 |

> **两者的接入成本已被本 Stage 的抽象压到最低**：§3.3.2 / §3.3.3 已固化 endpoint / auth / 请求体 / 字段映射 / 定价，零再调研；代码面只需 `websearch/<name>.rs` + `kind` 枚举一个分支（§3.1）。**首个付费 provider 接入时必须实网验证 `v2:gcm:` 密钥端到端生效**（§8.1 该路径本期仅 UT 覆盖）。

**后续候选搜索厂商**（数据取自 `docs/research/2026-10-06-web-search-providers.md`）：

| 厂商 | 定价 | 为何延后 / 特别事项 |
|------|------|-------------------|
| **Serper.dev** | **$0.30–1.00/1k**（Starter $1.00 → Ultimate $0.30，[已验证]）—— 全场最便宜 | 本 Stage 不做。⚠️ 其 endpoint path 与 `X-API-KEY` header 名在调研中标「未验证（请复核）」（`docs.serper.dev` 403）；`published_date` 是**相对时间串**（"2 days ago"）需额外解析；credits **6 个月过期**；作为 Google SERP 代理承担 Google ToS 灰区风险且**不提供** SerpApi 式法律盾 |
| **ScrapingDog** | **$0.333/1k** —— **是全样本中唯一带明确数据授权（data-license）ToS 条款的厂商** | 本 Stage 不做。⚠️ 厂商调研 §2.20 把 ScrapingDog/ScrapingBee/ScrapingAnt 整类标为「未验证（请复核）」，上述价格与 ToS 结论**须先独立复核**再立项；该类通用抓取平台的集成成本与性价比均劣于专用 SERP API |
| Brave Search | $5.00/1k，独立索引，ToS 明确允许 AI/LLM | 合规优先场景的候选 |
| Exa | $4–7/1k，响应带 `costDollars` | 成本归因最佳，但 snippet 走 `highlights[]` 数组需 join |
| Mojeek | £2/1k ≈ $2.5，**唯一逐条明文授予 AI/LLM 使用权且无 attribution 要求** | ⚠️ Startup 档**仅允许缓存 1 小时** → 直接约束网关缓存 TTL 设计，接入时必须按档位配置 |
| **智谱 Zhipu（`search_std`）** | **¥10/1k —— 国内三家最低**（博查 ¥36/1k 的 28%） | **博查的直接降本替换项**。独立可调用 + `Authorization: Bearer`，形态与博查同级；响应 `search_result[]`，url 在 `link`、snippet 在 `content`（映射成本略高于博查的 Bing 兼容形状）。2026-10-06 复核认为**不需实名**（与厂商调研 §1 表原记「需实名」冲突，实现时须确认） |
| **百度千帆（纯检索）** | ¥36/1k，**但有常驻 1,500 次/月免费额度** | **唯一有常驻月度免费额度的国内厂商**（博查的 1,000 次是一次性 3 个月）。若需长期零成本的中文兜底，优先于博查；代价是需百度智能云账号 + 实名，注册摩擦最高（难度 3） |

**法律警示（若将来接入 Jina）**：`s.jina.ai` 的 Terms **S4.5(iii) 禁止用其 Output 构建与 Jina 竞争的服务**（[已验证 https://jina.ai/legal/]）—— **对「转卖搜索能力的 AI 网关」而言这是直接相关的风险，上生产前必须法务确认**。另：Jina **只有全文 markdown 一种模式，无法降级为纯 SERP**（`X-Respond-With` 不是降级开关，而是 40× token 的 OCR 开关），与本层的 snippet-only 护栏（§3.5）根本冲突。

**本 Stage 不做的设计形态**（架构调研 §5.3，均属后续 Phase）：

| 形态 | 说明 | 阻塞项 |
|------|------|-------|
| **设计 A（短路）** | tools 只含 web_search 时不调模型，网关搜完直接合成 `server_tool_use` + `web_search_tool_result` + `text` 返回。**唯一能让 Claude Code 的 WebSearch 真正可用**（那是独立的 `/v1/messages` 子请求，设计 C 对它不适用） | 必须合成 Anthropic 服务端工具块 → 踩 `encrypted_content` 不可伪造（调研 §4.4）+ 历史投毒须按 id 前缀剥离（§5.7）两个坑 |
| **设计 B（agentic loop）** | 把 web_search 换成内部 function 工具 → 模型回 tool_call → 搜索 → 回灌 → 再请求，上限 3 轮 | **硬前提未实测：上游 MaaS 是否支持网关*注入*的 function 工具往返**（调研 §5.9 第 1 项 / §6.3 第 7 项）。Stage 132 只验证了「Codex 客户端声明的工具」可用，**网关注入**的工具是否被模型正确调用**尚无证据** → 立项前必须先做这一项实测 |

**本 Stage 发现的既存缺陷 —— ✅ 均已于 2026-10-07 收尾（用户决策）**：

- ✅ **`reqwest` feature 已删除** —— 原问题：`--no-default-features` 有 27 个编译错误（`alerts.rs` / `claude_oauth.rs` / `probe.rs` 无条件引用 `reqwest::`），即该 feature 的可选性只是名义上的。采纳方案 ②「承认现实」：Cargo.toml 三个依赖改必选、删 `[features] default`/`reqwest`、`aigw-server` 依赖声明清理、全库 22 处 `#[cfg(feature = "reqwest")]` 移除。**不保留该 profile 的理由**：没有消费者在构建无 reqwest 的 aigw-core，维护一个没人跑且早已损坏的 profile 是净成本。验证：`task test` 1052 pass/0 fail 与删除前逐字一致，`task bdd` 285 场景不变。
- ✅ **Taskfile 补齐两个真实缺口 + CLAUDE.md 订正** —— `task check` 不存在（已订正为 `task doctor`，并列入反例）；新增 **`task test-filter -- <pattern>`**（此前跑过滤测试只能用裸命令）与 **`task fmt-fix`**（`task fmt` 只做 `--check`，此前格式化只能用裸命令）。纪律文本补上明文边界：**过滤测试是迭代工具，门禁与交付结论必须以 `task test` 全量结果为准**。

**本层的功能性遗留**：

- ⭐ **配置源从 config.yaml 扩展为 DB 表 + admin CRUD + 前端（→ Stage 138）** —— 仿 `proxies` 表 / `027_proxies.sql` / Phase 50 的既有形态。本 Stage 已做的前置：`WebSearchConfig` / `WebSearchProviderConfig` / `WebSearchInstanceConfig` 是**普通可反序列化结构**，`build_websearch_registry` 只接受已解析结构而非读文件 → Stage 138 从 DB 行构造同一结构即可复用全部校验、密钥解密、实例选择与 client 构造逻辑，**零重写**
- **配额加权负载均衡 + 配额原子预留/回滚**（sub2api `manager.go` 的 `selectByQuotaWeight` + Redis Lua `quotaIncrScript`）—— 注意本 Stage 的实例级 `weight` 是**流量分配权重**（`router.rs` 语义），不是配额感知权重；多搜索 key 场景出现后再补，届时沿用 sub2api 的取舍「Redis 不可用时放行而非拒服务」（调研 §5.8）
- **query 改写 / 是否需要搜索的前置判断**（Higress `searchRewrite` 先调一次 LLM）—— 多一跳 LLM，成本与延迟都翻倍，阶段 1 不做
- **`published_date` 的统一解析**（本层只存原样字符串）—— 若 Stage 137 前端要排序/筛选，再加容错解析层（厂商调研 §3.3 第 5 条：解析失败降级 `None`，禁 panic）
- **`AnswerProvider` 能力分离**（Perplexity Sonar / Gemini grounding / Bing grounding 返回 grounded 散文而非 result list，形态不匹配 `SearchProvider`）—— 厂商调研 §3.3 第 6 条建议按「能力」而非「厂商」切 trait；本 Stage 只做 `SearchProvider`，不预留 `AnswerProvider`（无需求时预留即过度设计）
- **搜索结果缓存** —— aigw 已有 `CacheConfig`（`config.rs:65-78`）与 `cache` 模块，但搜索结果缓存受厂商 ToS 约束（Mojeek Startup 档仅 1 小时），接入前须逐家读 ToS，不在本 Stage
