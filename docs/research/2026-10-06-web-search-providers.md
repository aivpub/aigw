# Web Search API 供应商调研（aigw `web_search` tool 选型）

> 调研日期：**2026-10-06**　|　用途：aigw 内置 `web_search` tool 的外部搜索后端选型
> 验证方式：本次会话通过 wget / headless browser 直接读取厂商官方定价页与 API 文档。
> 标注约定：`[已验证 <url>]` = 本次在官方页面亲眼确认；`未验证（请复核）` = 未能确认，**不要当作事实使用**。

---

## §0 TL;DR 推荐

### 免费开发/测试 Top 3

| 排名 | 厂商 | 免费额度 | 一句话理由 |
|---|---|---|---|
| 1 | **Tavily** | 1,000 credits/月，**不要信用卡**，每月 1 号重置 | 字段天生就是 `title/url/content/score/published_date`，零映射成本；为 LLM 而生 |
| 2 | **Serper.dev** | 2,500 次（一次性），**不要信用卡** | 付费后 $0.30–1.00/1k 全场最便宜，Google 质量，1–2s 延迟 |
| 3 | **Exa** | $10/月 + $10 注册奖励，**不要支付方式** | 按美元计费、响应内带 `costDollars`，便于网关侧成本归因 |

补充：**博查 Bocha**（1,000 次 / ¥0 / 3 个月有效）是中文场景最佳免费起点，响应**兼容 Bing Web Search 格式**。

> ⚠️ **Tavily 使用前必读**：其 Platform Terms §6.5 明确允许把你的 query 与结果**用于训练 AI 模型** [已验证 https://www.tavily.com/terms]。**仅建议用于 dev/test 与非敏感 query**；生产若会转发用户敏感内容，不要默认选 Tavily。

### 生产默认 Top 2

| 排名 | 厂商 | $/1k | 一句话理由 |
|---|---|---|---|
| 1 | **Serper.dev** | **$1.00 → $0.30** | 成本/质量比最优；50–300 QPS；仅 snippet，context 成本可控 |
| 2 | **Brave Search API** | **$5.00** | 独立索引（不依赖 Google ToS 灰区），50 QPS，ToS 明确允许 AI/LLM 用途 |

中文生产默认建议 **博查 Bocha**（¥36/1k ≈ $5.07/1k，0.15s 延迟，Bing 兼容格式）。

> 合规优先的场景另荐 **Mojeek**（£2/1k ≈ $2.5）：唯一一家**逐条明文授予** AI/LLM 使用权、允许重排、且**无 attribution 要求**的独立索引厂商 [已验证 https://www.mojeek.com/services/api.html]。代价是无自助免费档（需商务联系）、5 QPS、Startup 档仅允许缓存 1 小时。

### 自建 / 无供应商

**SearXNG**（self-hosted metasearch）—— `?format=json` 可直出 JSON，零单次成本、无需注册；代价是**要自己扛上游引擎封锁与 ToS 风险**，仅建议作为 dev/离线兜底，不建议作为生产唯一后端。

### 关键否决项（本次调研推翻了常见认知）

- ❌ **Google Custom Search JSON API**：**已不对新客户开放**，且 **2027-01-01 停服** [已验证 https://developers.google.com/custom-search/v1/overview]。不要再把"100 次/天免费"写进方案。
- ❌ **DuckDuckGo**：无官方 web-results API。Instant Answer API 实测只返回消歧义条目、**无网页结果列表**；`html.duckduckgo.com` 实测返回 **HTTP 202 + `anomaly.js` 反爬挑战页、零结果**（本次均为实际调用验证）。
- ❌ **Bing Search API v7**：**已退役**（文档已迁入 `/previous-versions/`，元数据 `is_retired: true`）[已验证 https://learn.microsoft.com/en-us/previous-versions/bing/search-apis/bing-web-search/overview]。
- ⚠️ **Grounding with Bing**（替代品）：**$14/1k、无免费额度、仅能在 Agent 内调用、且强制向终端用户展示 citations** [已验证 https://www.microsoft.com/en-us/bing/apis/grounding-pricing]。
- ⚠️ **Azure AI Search ≠ web search** —— 它是索引「你自己的数据」的检索服务，别当 SERP 供应商做预算。

---

## §1 总览对比表

> `$/1k` = 每 1,000 次查询美元成本（入门档）。注册难度 1=邮箱即可，5=需企业实体/对公。

| 厂商 | 免费额度 | 需要卡 | $/1k | 内容形态 | 鉴权 | 注册难度 |
|---|---|---|---|---|---|---|
| **Tavily** | 1,000 credits/月 [已验证] | 否 [已验证] | $8.00 [已验证] | snippet（可选全文） | `Authorization: Bearer` | 1 |
| **Serper.dev** | 2,500 次 [已验证] | 否 [已验证] | **$1.00→$0.30** [已验证] | snippet only | `X-API-KEY` 未验证（请复核） | 1 |
| **Brave Search** | $5 credits/月 ≈1,000 次 [已验证] | 未验证（请复核） | $5.00 [已验证] | snippet + extra_snippets | `X-Subscription-Token` [已验证] | 1 |
| **Exa** | $10/月 + $10 一次性 [已验证] | 否 [已验证] | $4.00 起 [已验证] | 全文/highlights | `x-api-key` [已验证] | 1 |
| **SearchApi.io** | 100 次（一次性）[已验证] | 否 [已验证] | $4.00 [已验证] | snippet only | `Authorization: Bearer` 或 `api_key` [已验证] | 1 |
| **SerpApi** | 250 次/月 [已验证] | 未验证（请复核） | $25.00 [已验证] | snippet only | `api_key` query [已验证] | 1 |
| **Perplexity Search** | 未验证（请复核） | 未验证（请复核） | **$5.00 / $1.00 fast** [已验证] | snippet [已验证] | `Authorization: Bearer` [已验证] | 1 |
| **Linkup** | 4,000 次 [已验证] | 未验证（请复核） | $5.00–6.00 [已验证] | 抽取式 content | `Authorization: Bearer` [已验证] | 1 |
| **Parallel AI** | 5,000 次/月 + $5/月 [已验证] | 未验证（请复核） | $1.00（fast）[已验证] | 压缩 excerpts | `x-api-key` [已验证] | 1 |
| **Valyu** | $10（工作邮箱 $20）[已验证] | 否 [已验证] | $1.50 /1k **retrieval** [已验证] | 全文 excerpt | `x-api-key` [已验证] | 1 |
| **Jina `s.jina.ai`** | 10M tokens 一次性 ≈1,000 次 [已验证] | 否 | 未验证（请复核） | **全文 markdown** | `Authorization: Bearer` | 1 |
| **Firecrawl `/v2/search`** | 1,000 credits/月 = 500 次 [已验证] | 否 [已验证] | ≈$1.66（Standard 推算） | snippet +可选 markdown | `Authorization: Bearer` [已验证] | 1 |
| **Mojeek** | 仅"contact us"试用 [已验证] | 需商务洽谈 | **£2 CPM**（≈$2.5）[已验证] | snippet | `api_key` query [已验证] | 3 |
| **ZenSERP** | 50 次/月（free forever）[已验证] | 未验证（请复核） | **$2.00** [已验证] | snippet only | 未验证（请复核） | 1 |
| **Marginalia** | `public` key 免费 [已验证] | 否 | 商业档 未验证（请复核） | snippet | `API-Key` header [已验证] | 1 |
| **SearXNG** | 自建无限 | 否 | $0（自担成本） | snippet | 无 / 自建 | 1（但需运维） |
| **Yandex Cloud v2** | 未验证（请复核） | 需卡+手机 | **$4.00**（夜间 $3.00 / deferred $0.25）[已验证] | XML(base64) 未验证（请复核） | IAM/API key + folderId | 5 |
| **Gemini grounding** | 5,000 次/月（**仅付费档**）[已验证] | 是 [已验证] | **$14.00** [已验证] | 仅 grounded 散文 | `x-goog-api-key` | 2 |
| **Grounding with Bing** | **无** [已验证] | 是 [已验证] | **$14.00** [已验证] | 仅 Agent 内 + 强制署名 | Azure AAD | 4 |
| ~~Bing Web Search v7~~ | ~~F1 1000/月~~ **已退役** [已验证] | — | — | — | — | — |
| ~~Google CSE~~ | ~~100/天~~ **新客户不可用，2027-01-01 停服** [已验证] | — | ~~$5.00~~ | — | — | — |
| **博查 Bocha** | 1,000 次 / ¥0 / 3 月 [已验证] | 否（需手机实名）| **¥36/1k ≈ $5.07** [已验证] | snippet + summary | `Authorization: Bearer` [已验证] | 2 |
| **智谱 Zhipu** | 无常驻免费额度 | **不需实名**（复核后更正） | **¥10/1k（`search_std`）** | snippet（content_size 可调）| `Authorization: Bearer` [已验证] | 2 |
| **百度千帆（纯检索）** | **1,500 次/月（常驻）** | 需百度智能云账号 + 实名 | **¥36/1k** | snippet | `Authorization: Bearer` | 3 |

> **2026-10-06 补充复核**（本表上述两行）：智谱 `search_std` **¥10/千次为三家国内厂商中最低**，且复核认为**不需实名**（与第 76 行原记「需实名」冲突，**以本行为准，实现时再确认**）；百度千帆纯检索是**唯一有常驻月度免费额度**的国内厂商（1,500 次/月）。两者均为独立可调用 + Bearer 鉴权，形态与博查同级。**本期 provider 选型（SearXNG / Tavily / 博查）不变** —— 博查胜在响应 **Bing 兼容**（`webPages.value[]`，映射成本最低）；若后续需降本（¥10 vs ¥36）或要常驻免费额度，这两家是最直接的替换项，已登记 Stage 134 §8.2。

---

## §2 各厂商实现细节

### 2.1 Tavily ⭐ 免费首选

- **Endpoint**：`POST https://api.tavily.com/search` [已验证 https://docs.tavily.com/documentation/api-reference/endpoint/search]
- **Auth**：`Authorization: Bearer tvly-xxx` [已验证 同上]

```bash
curl -X POST https://api.tavily.com/search \
  -H 'Authorization: Bearer $TAVILY_API_KEY' \
  -H 'Content-Type: application/json' \
  -d '{"query":"who is Leo Messi?","search_depth":"basic","max_results":5}'
```

**响应结构** [已验证 同上]

```json
{
  "query": "Who is Leo Messi?",
  "answer": "Lionel Messi, born in 1987, ...",
  "results": [
    {
      "title": "Lionel Messi Facts | Britannica",
      "url": "https://www.britannica.com/facts/Lionel-Messi",
      "content": "Lionel Messi, an Argentine footballer...",
      "score": 0.81025416,
      "raw_content": null,
      "published_date": "Tue, 11 Mar 2025 17:00:00 GMT",
      "favicon": "https://britannica.com/favicon.png"
    }
  ],
  "response_time": "1.67",
  "usage": { "credits": 1 }
}
```

| 字段 | JSON path |
|---|---|
| title | `results[].title` |
| url | `results[].url` |
| snippet | `results[].content` |
| score | `results[].score` |
| published_date | `results[].published_date` |
| 全文 | `results[].raw_content`（需 `include_raw_content: true`）|

- **免费**：1,000 credits/月，**不要信用卡**，每月 1 号重置，用尽即硬停 [已验证 https://www.tavily.com/pricing]
- **付费**：PAYG **$0.008/credit**；basic search = 1 credit，advanced = 2 credits → **$8/1k（basic）/ $16/1k（advanced）** [已验证 同上]
- **Rate limit**：dev key 100 RPM / prod key 1,000 RPM 未验证（请复核）
- **内容形态**：默认 snippet；`include_raw_content: true` 可取全文，**不额外计费**（价格只由 `search_depth` 决定）
- **⚠️ ToS 重点（已逐字核实）**：Platform Terms §6.5 "Customer Input" 原文 —— "**Tavily and its third-party artificial intelligence service providers may use, process, analyze, and retain Customer Input submitted to the AI Functionality and Outputs generated by the AI Functionality for purposes of training, improving, developing, and enhancing artificial intelligence models**, machine learning systems, and related technologies including Tavily's services." 并明确"may incorporate insights derived from your Customer Input and Outputs into their respective **training datasets**" [已验证 https://www.tavily.com/terms，Last updated 2026-05-04]
  - 另 §6.5 附注：某些 Third-Party Service Providers "**may not be required to maintain the confidentiality** of any Customer Input or Output and may retain certain rights to use or disclose…… including to **further train their algorithmic models**" [已验证 同上]
  - **影响**：网关若会转发用户敏感 query（如内部文档检索词、客户名），Tavily 默认条款下这些 query 会进训练集。**敏感场景必须改用提供 ZDR 的厂商**（SerpApi `zero_retention` / SearchApi `zero_retention` / Linkup Enterprise / Valyu Enterprise / Brave Enterprise ZDR），或与 Tavily 谈 Enterprise。

### 2.2 Serper.dev ⭐ 生产首选（成本）

- **Endpoint**：`POST https://google.serper.dev/search`（域名解析存在，本次以 wget 探测返回 403 WAF，证实 endpoint 存在；路径 未验证（请复核））
- **Auth**：`X-API-KEY: <key>` 未验证（请复核 —— docs.serper.dev 被 403 挡住）

```bash
curl -X POST https://google.serper.dev/search \
  -H 'X-API-KEY: <key>' -H 'Content-Type: application/json' \
  -d '{"q":"rust async runtime","gl":"us","hl":"en","num":10}'
```

**响应结构**（首页 live sample 实证）[已验证 https://serper.dev]

```json
{
  "knowledgeGraph": { "title": "...", "type": "...", "description": "..." },
  "organic": [
    {
      "title": "Google",
      "link": "https://www.google.com/",
      "snippet": "Search the world's information...",
      "date": "2 days ago",
      "source": "Wikipedia",
      "position": 1
    }
  ],
  "peopleAlsoAsk": [ { "question": "...", "snippet": "...", "link": "..." } ],
  "relatedSearches": [ { "query": "..." } ]
}
```

| 字段 | JSON path |
|---|---|
| title | `organic[].title` |
| url | `organic[].link` |
| snippet | `organic[].snippet` |
| published_date | `organic[].date`（**非 ISO，相对时间串如 "2 days ago"，需解析**）|
| score | **无**，仅 `organic[].position` |

- **免费**：**2,500 次，不要信用卡** [已验证 https://serper.dev]；是一次性还是周期性 未验证（请复核）
- **付费**（充值制，无订阅）[已验证 https://serper.dev]

| 档位 | 价格 | credits | **$/1k** | QPS | 有效期 |
|---|---|---|---|---|---|
| Starter | $50 | 50k | **$1.00** | 50 | 6 个月 |
| Standard | $375 | 500k | **$0.75** | 100 | 6 个月 |
| Scale | $1,250 | 2.5M | **$0.50** | 200 | 6 个月 |
| Ultimate | $3,750 | 12.5M | **$0.30** | 300 | 6 个月 |

- **⚠️ credits 6 个月过期**，低频网关要注意沉没成本
- **内容形态**：**仅 snippet**，无全文抽取；延迟 1–2s [已验证 https://serper.dev]
- **ToS**：法律页未取到 未验证（请复核）。作为 Google SERP 代理，承担 Google ToS 灰区风险；Serper **不提供** SerpApi 式法律盾。

### 2.3 Brave Search API ⭐ 生产首选（合规）

- **Endpoint**：`GET https://api.search.brave.com/res/v1/web/search` [已验证 https://api-dashboard.search.brave.com/app/documentation/web-search/get-started]
- **Auth**：`X-Subscription-Token: <KEY>` header [已验证 同上]

```bash
curl "https://api.search.brave.com/res/v1/web/search?q=rust+async&count=10&extra_snippets=true" \
  -H "X-Subscription-Token: <YOUR_API_KEY>"
```

**响应结构** [已验证 同上]

```json
{
  "web": {
    "results": [
      {
        "title": "Python Web Frameworks",
        "url": "https://example.com/python-frameworks",
        "description": "Main snippet text...",
        "extra_snippets": ["First additional excerpt...", "Second..."]
      }
    ]
  },
  "query": { "more_results_available": true }
}
```

| 字段 | JSON path |
|---|---|
| title | `web.results[].title` |
| url | `web.results[].url` |
| snippet | `web.results[].description` |
| 额外片段 | `web.results[].extra_snippets[]`（最多 5 条，`extra_snippets=true`）|
| score | **无** |

- **定价（已变化，注意！）**：**$5 per 1,000 requests**，含**每月 $5 免费 credits**（≈1,000 次/月），**50 QPS** [已验证 https://brave.com/search/api/]
  - ⚠️ 常被引用的"免费 2,000 次/月 + 1 QPS"**已不是当前形态**，现在是 credits 制。
- 另有 **Answers** 端点：$4/1k + $5/百万 token，2 QPS，OpenAI SDK 兼容 [已验证 同上]
- 另有专为 agent 设计的 **LLM Context** 端点（文档指向其为 AI 场景首选）[已验证 同上]，具体 schema 未验证（请复核）
- **参数**：`freshness=pd|pw|pm|py` 或自定义日期区间；`count` 最大 20；`offset` 最大 9；支持 Goggles 自定义重排 [已验证 同上]
- **ToS**：独立索引，明确面向 AI/agent/RAG 与基模训练 [已验证 同上]；具体 attribution/caching 条款 未验证（请复核）

### 2.4 Exa

- **Endpoint**：`POST https://api.exa.ai/search` [已验证 https://exa.ai/docs/reference/search]
- **Auth**：`x-api-key: $EXA_API_KEY` [已验证 同上]

```bash
curl -X POST https://api.exa.ai/search \
  -H "x-api-key: $EXA_API_KEY" -H "Content-Type: application/json" \
  -d '{"query":"latest AI research","type":"auto","numResults":10,
       "contents":{"text":true,"highlights":true}}'
```

**响应结构** [已验证 同上]

```json
{
  "results": [
    { "title": "...", "url": "...",
      "publishedDate": "2023-11-07T05:31:56Z",
      "author": "...", "text": "...",
      "highlights": ["..."], "highlightScores": [0.9],
      "summary": "..." }
  ],
  "costDollars": { "total": 0.01, "search": {"neural": 0.005}, "contents": {"text": 0.005} }
}
```

| 字段 | JSON path |
|---|---|
| title / url | `results[].title` / `results[].url` |
| published_date | `results[].publishedDate`（ISO8601，最干净）|
| snippet | `results[].highlights[]`（token 友好）|
| 全文 | `results[].text` |
| score | **仅 `results[].highlightScores[]`**，无统一 `score` |

- **免费**：**$10 credits/月 + $10 注册奖励，不需要支付方式**；月度额度不滚存 [已验证 https://exa.ai/pricing]
- **付费**：Instant Search **$4/1k**，Fast/Auto **$7/1k**，Deep $12–15/1k；Contents **$1/1k pages**；额外结果 +$1/1k [已验证 同上]
- **Rate limit**：10 QPS（Starter/PAYG），Enterprise 可定制 [已验证 同上]
- **亮点**：响应内 `costDollars` 便于网关精确成本归因

### 2.5 Perplexity —— 注意：现在有**原生 Search API** 了

> ⚠️ 旧认知"Perplexity 只返回散文不返回结果列表"**已过时**。Perplexity 现已提供独立的 raw Search API。

- **Endpoint**：`POST https://api.perplexity.ai/search` [已验证 https://docs.perplexity.ai/api-reference/search-post]
- **Auth**：`Authorization: Bearer <token>` [已验证 同上]

```bash
curl -X POST https://api.perplexity.ai/search \
  -H 'Authorization: Bearer <token>' -H 'Content-Type: application/json' \
  -d '{"query":"rust async runtime comparison"}'
```

**响应结构** [已验证 同上]

```json
{
  "results": [
    { "title": "...", "url": "...", "snippet": "...",
      "date": "...", "last_updated": "..." }
  ],
  "id": "...", "server_time": "..."
}
```

| 字段 | JSON path |
|---|---|
| title / url / snippet | `results[].title` / `.url` / `.snippet` |
| published_date | `results[].date`；另有 `results[].last_updated` |
| score | **无** |

- **定价** [已验证 https://docs.perplexity.ai/getting-started/pricing]

| 产品 | $/1k requests |
|---|---|
| **Search API** | **$5.00** |
| **Search API（`search_type:"fast"`）** | **$1.00** |
| Agent API `web_search` tool（standard） | $2.50 |
| Agent API `web_search` tool（fast） | $1.00 |

- **计费单位**：按**每个成功的 `POST /search` 请求**计费，而非请求体里的每个 query [已验证 同上] —— 支持多 query 批量，网关可借此摊薄成本
- **内容形态**：snippet；**另有** Sonar/Chat completions 返回 grounded 散文 + citations（这才是"不是 raw results"的那个产品，别混淆）
- **免费额度**：未验证（请复核）

### 2.6 SearchApi.io

- **Endpoint**：`GET https://www.searchapi.io/api/v1/search?engine=google` [已验证 https://www.searchapi.io/docs/google]
- **Auth**：`Authorization: Bearer <key>` **或** `api_key` query param（二者皆可）[已验证 同上]

```bash
curl 'https://www.searchapi.io/api/v1/search?engine=google&q=chatgpt' \
  -H "Authorization: Bearer $SEARCHAPI_KEY"
```

**响应结构**：SerpApi 风格信封 —— `search_metadata` / `search_parameters` / `search_information` / `organic_results[]`，字段 `title` / `link` / `snippet` / `position` [已验证 同上，完整字段列表 未验证（请复核）]

- **免费**：**100 次（一次性），不要信用卡** [已验证 https://www.searchapi.io/pricing]
- **付费**：Developer $40/10k = **$4.00/1k**；Production $100/35k ≈ **$2.86/1k**；BigData $250/100k = **$2.50/1k**；Scale $500/250k = **$2.00/1k** [已验证 同上]
- **按成功计费**（失败不扣）[已验证 同上]
- **Rate limit**（独特模型）：**每小时最多用掉套餐额度的 20%** [已验证 同上]
- Production 档及以上含 **Legal Protection Guarantee** [已验证 同上]
- **内容形态**：仅 snippet

### 2.7 SerpApi

- **Endpoint**：`GET https://serpapi.com/search?engine=google` [已验证 https://serpapi.com/search-api]
- **Auth**：`api_key` **query 参数**（注意：不是 header，日志脱敏要当心）[已验证 同上]

```bash
curl 'https://serpapi.com/search?engine=google&q=coffee&api_key=$SERPAPI_KEY&num=10'
```

**响应结构** [已验证 同上]：`organic_results[].{position,title,link,snippet,displayed_link,sitelinks,rich_snippet}` + `knowledge_graph` + `search_metadata`
- score **无**；organic 结果无 `published_date`（news 垂类有 `date`）

- **免费**：**250 次/月**，50 次/小时吞吐 [已验证 https://serpapi.com/pricing]；是否需要卡 未验证（请复核）
- **付费**：Starter $25/1k = **$25/1k**；Developer $75/5k = **$15/1k**；Production $150/15k = **$10/1k**；Big Data $275/30k ≈ **$9.17/1k** [已验证 同上]
- **全场最贵**（约为 Serper 的 9–25 倍）；溢价买的是结构化解析 + 法律盾
- **U.S. Legal Shield**：最高 **$200 万**抓取/解析责任覆盖，**Starter 档及以上，Free 档不含** [已验证 同上]
- **ZeroTrace / `zero_retention`**：可要求不留存查询与结果 [已验证 同上]
- **内容形态**：仅 snippet（`raw_html_file` 是 SERP 原始 HTML，非目标页正文）

### 2.8 Linkup

- **Endpoint**：`POST https://api.linkup.so/v1/search` [已验证 https://docs.linkup.so/pages/documentation/api-reference/endpoint/post-search]
- **Auth**：`Authorization: Bearer <token>` [已验证 同上]

```bash
curl -X POST https://api.linkup.so/v1/search \
  -H 'Authorization: Bearer $LINKUP_API_KEY' -H 'Content-Type: application/json' \
  -d '{"q":"Microsoft 2024 revenue","depth":"deep","outputType":"sourcedAnswer"}'
```

**响应结构**（`outputType: "sourcedAnswer"`）[已验证 同上]

```json
{
  "answer": "...",
  "sources": [
    { "name": "Microsoft 2024 Annual Report",
      "url": "https://...",
      "content": "Microsoft Cloud revenue increased 23% to $137.4 billion.",
      "type": "text" }
  ]
}
```

- ⚠️ **title 字段叫 `name` 不叫 `title`**；无 `score`、无 `published_date`
- `outputType` 另支持 `searchResults`（schema 未验证（请复核））
- **免费**：**4,000 次** [已验证 https://www.linkup.so/pricing]；是否需卡/重置周期 未验证（请复核）。另有创业公司 $5,000 赠金计划。
- **付费**：Search $0.005–0.006/次 = **$5–6/1k**；Fetch（取全文）$0.001–0.006/次 [已验证 同上]
- **Rate limit**：未验证（请复核）

### 2.9 Parallel AI

- **Endpoint**：`POST https://api.parallel.ai/v1/search` [已验证 https://docs.parallel.ai/search-api/search-quickstart]
- **Auth**：`x-api-key: $PARALLEL_API_KEY` [已验证 同上]

```bash
curl https://api.parallel.ai/v1/search \
  -H "Content-Type: application/json" -H "x-api-key: $PARALLEL_API_KEY" \
  -d '{"objective":"Find latest info about X","search_queries":["X products"],"mode":"fast"}'
```

**响应结构** [已验证 同上]

```json
{
  "search_id": "search_8a911eb...",
  "results": [
    { "url": "...", "title": "...", "publish_date": null,
      "excerpts": ["...", "..."] }
  ],
  "warnings": []
}
```

- ⚠️ **snippet 是 `excerpts[]`（字符串数组）**，不是单个字符串，映射时需 join
- 请求模型特殊：自然语言 `objective` + 可选 `search_queries[]` 数组
- **免费**：**5,000 次/月** + $5 credits/月 + 注册最高 $80 [已验证 https://parallel.ai/pricing]
- **付费**：Search **$0.001–0.005 / 10 结果** = **$1–5/1k**；`fast` 档 ≈ **$1/1k**（~700ms）[已验证 同上]
- **⚠️ 坑**：`mode` 不传时 API 默认 **`advanced`（最贵档）**，文档推荐却是 `fast` —— **务必显式传 `mode`** [已验证 https://docs.parallel.ai/search-api/processors]
- **Rate limit**：Search **600 req/min** [已验证 https://parallel.ai/pricing]

### 2.10 Valyu

- **Endpoint**：`POST https://api.valyu.ai/v1/search`（注意域名是 `.ai` 不是 `.network`）[已验证 https://docs.valyu.ai/api-reference/endpoint/search]
- **Auth**：`x-api-key: $VALYU_API_KEY` [已验证 同上]

**响应结构** [已验证 同上]

```json
{
  "tx_id": "tx_a1b2...",
  "results": [
    { "title": "...", "url": "...", "content": "...",
      "relevance_score": 0.95, "publication_date": "2024-06-15",
      "price": 0.0015, "length": 15420, "source_type": "website" }
  ],
  "total_deduction_dollars": 0.0075,
  "total_characters": 45230
}
```

| 字段 | JSON path |
|---|---|
| title / url / content | `results[].title` / `.url` / `.content` |
| score | **`results[].relevance_score`** |
| published_date | **`results[].publication_date`** |

- **免费**：**$10 注册赠金（工作邮箱 $20），不需要卡** [已验证 https://www.valyu.ai/pricing]
- **付费**：Web search **$1.50 / 1,000 retrievals** [已验证 https://docs.valyu.ai/pricing]
  - ⚠️ **按"条结果"而非"次查询"计费** —— 10 结果的一次查询 ≈ $0.015，折合 **≈$15/1k 查询**。横向比价时极易算错。
  - 可用 `max_price` 限制单次花费（超限返回 206 部分成功）
- **亮点**：响应内 per-result `price` + `total_deduction_dollars`，成本可观测性全场最佳

### 2.11 Jina AI `s.jina.ai` —— ⚠️ 全文 markdown，且**无法降级为纯 SERP**

- **Endpoint**：`GET/POST https://s.jina.ai/?q=<query>` [已验证 https://jina.ai/api-dashboard/rate-limit]
- **Reader**：`GET/POST https://r.jina.ai/<url>` [已验证 同上]
- **Auth**：`Authorization: Bearer <jina_key>`

```bash
curl "https://s.jina.ai/?q=rust%20async%20runtime" \
  -H "Authorization: Bearer $JINA_API_KEY" \
  -H "Accept: application/json" \
  -H "X-Engine: direct"          # 最快，不跑 JS
  # 可选降 token：X-Retain-Images: none / X-Target-Selector / X-Remove-Selector
```

- **⚠️ 默认返回每条结果的完整页面 markdown**，Search 模式默认 5 条 [已验证 https://jina.ai/reader]。对 LLM context 成本影响巨大。
- **⚠️⚠️ 重要更正（本次实测）**：**不存在 `X-Respond-With: no-content`** —— 该字符串在官方页面出现 **0 次**。`X-Respond-With` 的真实语义是**切换到 `jina-ocr-v1` 把图片转 Markdown，且「Costs 40× more tokens!」** [已验证 https://jina.ai/reader]。误用会让成本暴涨 40 倍而非降低。
  - **结论：`s.jina.ai` 没有「只要 title/url/snippet」的廉价模式。** 一次纯元数据搜索与一次全文搜索**同价**。这是 Jina 与其他厂商最根本的模型差异。
- **真实可用的降 token 手段** [已验证 同上]：`X-Retain-Images`（去图）、`X-Target-Selector` / `X-Remove-Selector`（CSS 取舍）、`X-Token-Budget`（硬上限，超则请求失败）、`X-Engine: direct`（不跑 JS，最快）
- **计费**：按 token 计。`s.jina.ai` **每次请求固定起步 10,000 tokens** [已验证 同上]
- **免费**：新 API key 赠 **10M tokens**（一次性）[已验证 同上] → 10M ÷ 10k = **恰好 1,000 次搜索**
- **Rate limit**（注意：付费**不提升** RPM，只有 Premium 提升）[已验证 https://jina.ai/api-dashboard/rate-limit]

| 端点 | 无 key | Free key | Paid key | Premium | 平均延迟 |
|---|---|---|---|---|---|
| `r.jina.ai` | 20 RPM | 500 RPM | 500 RPM | 5,000 RPM | 7.9s |
| `s.jina.ai` | **blocked** | 100 RPM | 100 RPM | 1,000 RPM | 2.5s |

- **$/1k**：未验证（请复核 —— 单价由 `dash.jina.ai` 运行时加载，静态页只有 Stripe ID）。按单价 `P`（美元/1M tokens）推算：`1,000 次 × 10,000 tokens = 10M tokens → 成本 = 10 × P`。若 P=$0.02 → **$0.20/1k**；P=$0.05 → **$0.50/1k**。
- **⚠️ ToS 竞业条款**：Terms S4.5(iii) 禁止用 Output 构建**与 Jina 竞争**的服务 [已验证 https://jina.ai/legal/] —— **对「转卖搜索能力的网关」而言这是直接相关的风险**，上生产前需法务确认。LLM ingestion 本身明确允许（"claims no rights to such Output"），无署名要求，客户侧缓存不受限。
- Jina AI GmbH 已于 2025-10 被 **Elastic N.V.** 收购 [已验证 同上] —— 产品方向可能变动。
- **响应 JSON 结构**：未验证（请复核 —— `s.jina.ai`/`r.jina.ai` 本次网络不可达，无法实测）

### 2.12 Firecrawl `/v2/search`

- **Endpoint**：`POST https://api.firecrawl.dev/v2/search` [已验证 https://docs.firecrawl.dev/api-reference/endpoint/search]
- **Auth**：`Authorization: Bearer <token>` [已验证 同上]

```bash
curl -X POST https://api.firecrawl.dev/v2/search \
  -H 'Authorization: Bearer $FIRECRAWL_KEY' -H 'Content-Type: application/json' \
  -d '{"query":"rust async","limit":10,"sources":["web"]}'
```

**响应结构** [已验证 同上]

```json
{
  "success": true,
  "data": {
    "web": [
      { "title": "...", "description": "...", "url": "...",
        "markdown": "...", "html": "...", "links": ["..."] }
    ]
  },
  "creditsUsed": 123
}
```

| 字段 | JSON path |
|---|---|
| title | `data.web[].title` |
| url | `data.web[].url` |
| snippet | `data.web[].description` |
| 全文 | `data.web[].markdown`（需 `scrapeOptions`）|

- **免费**：1,000 credits/月，**无需卡** [已验证 https://www.firecrawl.dev/pricing]
- **credit 换算**：**search = 2 credits / 10 结果** → 1,000 credits ≈ **500 次搜索** [已验证 同上]
- **付费**：Hobby $16/月 5k credits（2,500 次）；Standard $83/月 100k credits（50k 次）；Growth $333/月 500k；Scale $599/月 1M [已验证 同上]
  - 折算 ≈ **$1.66/1k 搜索**（Standard：$83 ÷ 50,000 × 1,000）[本人推算]
  - PAYG：$5 增量，Hobby 给 1,000 credits / Standard 2,000 / Growth 2,500 / Scale 5,000 [已验证 同上]
- ⚠️ `scrapeOptions` 用 JSON / Question / Highlight 格式 **每页额外 +4 credits**；Monitor 的确定性抽取引擎按每页每次 7 credits 计 [已验证 同上]

### 2.13 Mojeek —— 独立索引，ToS 最友好 ⭐

- **Endpoint**：`GET https://api.mojeek.com/search` [已验证 https://www.mojeek.com/support/api/search/quickstart.html]
- **Auth**：`api_key` **query 参数**（不是 header —— 日志脱敏注意）[已验证 同上]
- **格式**：`fmt=json` | `fmt=xml`，**默认 xml，必须显式传 `fmt=json`** [已验证 https://www.mojeek.com/support/api/search/request_parameters.html]

```bash
curl "https://api.mojeek.com/search?q=mojeek&api_key=YOUR_API_KEY&fmt=json&lb=EN&lbb=100"
```

**响应结构** [已验证 https://www.mojeek.com/support/api/search/json_response.html]

```
response
├── status            "OK" | "ERROR: Daily Limit Reached"
├── head
│   ├── query, nword, words[] {full, stem, plus, hits}
│   └── timer, start, return, exact, results, rankm, dups
└── results[]
    ├── url           ← response.results[].url
    ├── title         ← response.results[].title
    ├── desc          ← response.results[].desc   （query-dependent snippet）
    └── size, timestamp, date
```

| 字段 | JSON path |
|---|---|
| title | `response.results[].title` |
| url | `response.results[].url` |
| snippet | **`response.results[].desc`** |
| published_date | `response.results[].date` |
| score | 无（自定义档可申请 ranking/authority/semantic 分数）|

- ⚠️ 注意顶层多一层 `response` 包裹，且有 `status` 字段需检查（如 `"ERROR: Daily Limit Reached"`）

- **定价** [已验证 https://www.mojeek.com/services/api.html]

| 档位 | 价格（CPM，不含 VAT）| QPS | 日上限 | 单次最大结果 | Storage Rights |
|---|---|---|---|---|---|
| Startup | **£2 / 千次** | 5 | 100,000/天 | 10 | **仅允许缓存 1 小时** |
| Business | **£3 / 千次** | 10 | 400,000/天 | 40 | **完整存储权** |
| Enterprise | 面议 | 定制 | 无限 | 100 | 可选 |

- £2 CPM ≈ **$2.5–2.6/1k** 未验证（请复核，汇率）
- ⚠️ Startup 比 Business 更便宜 —— 加价买的是 QPS、结果数与**存储权**
- 支付：Stripe，按量预充值；仅 Enterprise 支持开票 [已验证 同上]
- **免费**：**无自助免费档**。仅 "free trial version, with limited queries. Get in touch." —— 需商务联系 [已验证 同上]
- **⭐ ToS 最明确友好** [已验证 同上]：
  - "Can I use the API for AI? **Yes, you can use the API results for AI with all plans.**"
  - "**Usage alongside LLMs and chatbots allowed**"
  - 允许重排（"you are free to re-rank results as you wish"）
  - **无 attribution 要求**；可在结果旁放自己的广告、可与其他来源混排
  - ⚠️ **缓存权分档**：Business 可存储；其他档**仅允许缓存 1 小时** —— 直接约束网关缓存 TTL 设计
  - ⚠️ 版权注意：API 不授予目标网页第三方内容的使用权；若你去抓取这些 URL 的正文，合规责任在你
- 返回：URL + page title + query-dependent snippets，**仅 snippet 无全文** [已验证 同上]

### 2.14 Marginalia Search —— 小众索引，非商业免费

- **Endpoint**：`GET https://api2.marginalia-search.com/search?query=<q>` [已验证 https://about.marginalia-search.com/article/api/]
- **Auth**：`API-Key: <key>` header；**可直接用公共 key `public`**（无需注册，但有较严 rate limit、不能建自定义 filter）[已验证 同上]

```bash
curl -H "API-Key: public" -XGET 'https://api2.marginalia-search.com/search?query=json+api'
```

**响应结构** [已验证 同上]

```json
{
  "query": "...",
  "license": "...",
  "results": [
    { "url": "...", "title": "...", "description": "..." }
  ]
}
```

| 字段 | JSON path |
|---|---|
| title / url | `results[].title` / `results[].url` |
| snippet | `results[].description` |
| published_date / score | **无** |

- ⚠️ 顶层 `license` 字段是**刻意设计的合规信号** —— 若用非商业 key，应把它随结果一起透传/记录。
- 参数：`count`(1–100)、`timeout`(50–250ms)、`dc`(每域名最大结果)、`page`(1-indexed)、`nsfw`(0/1)、`filter` [已验证 同上]
- 另有 filter CRUD：`GET /filter`、`POST /filter/NAME`、`GET /filter/NAME`、`DELETE /filter/NAME`，XML 定义 [已验证 同上]
- **三档访问** [已验证 同上]

| 档位 | 获取方式 | 许可/条款 |
|---|---|---|
| 免费非商业 | 邮件 contact@marginalia-search.com（"necessary to prevent mass signup abuse"）| **CC-BY-NC-SA 4.0** → 需署名 + 非商业 + 相同方式共享 |
| 付费非商业 | 一次性付费跳队列（可选，"will always remain optional"）| CC-BY-NC-SA 4.0 |
| 商业 | 在 `polar.sh/marginalia-search/portal` 买**按量计费**商业 key | **无商业限制、无 attribution 要求** |

- **商业档 $/1k：未验证（请复核）** —— 页面只给 Polar 结账链接，未公布费率；"Other terms are negotiable"
- **Rate limit**：无公布 QPS；`public` key "often hits a rate limit" [已验证 同上]，具体数值 未验证（请复核）
- ⚠️ **免费/付费非商业 key 受 CC-BY-NC-SA 约束 —— 商业产品（含 SaaS 网关）必须买商业 key**
- 文档更新日期 **2025-12-08** [已验证 同上]
- **结论**：索引偏小众/长尾非商业内容，是很好的**多样性/长尾补充后端**（能挖出大厂埋掉的独立站），但索引规模小、无 SLA，**不适合作通用主力**。

### 2.15 SearXNG —— 自建首选

> ✅ **2026-10-06 实测落定**（实例 `http://30.184.60.216:9099`，本节原标「未验证（请复核）」项已全部确认，fixture 固化于 `docs/fixtures/searxng-search-response-2026-10-06.json`）：
>
> | 项 | 实测结果 |
> |----|---------|
> | `format=json` | ✅ HTTP 200；延迟 **2.0–3.1s**（mean 2.43s，5 次连打） |
> | 响应字段 | `results[].{title,url,content,score,publishedDate,engine}` 全部存在；`score` **35/35 非空**、`publishedDate` **仅 7/35 非空**、`content` **33/35 非空**（2 条空） |
> | ⚠️ **排序** | **`results[]` 按「每引擎各自 position」交错，不是全局 score 降序** —— 实测序列 `1.0,1.0,0.5,0.5,0.33,0.33,0.33,0.25,0.25,`**`1.0`**`,…`（第 10 条跳回 1.0 = 第三引擎的第 1 名）。**消费方必须先按 score 全局重排再截断**，否则会丢掉某引擎的最佳结果 |
> | ⚠️ **零结果** | **无空结果语义** —— 乱码 query 仍返 35 条无关结果，`results` 永不为空 → 调用方无法靠条数判断「搜不到」 |
> | ⚠️ **限条数** | `&results=N` / `&limit=N` **均被忽略**；唯一有效的是 `&engines=bing`（→ 10 条）。恒收全量 ~30KB |
> | ⚠️ **format 容错** | `format=xml` 或省略 `format` → **HTTP 200 + HTML**（静默降级，非 4xx）；`format=csv` 真实可用（表头 `title,url,content,host,engine,score,type`） |
> | 空 query | HTTP 400 |
> | 速率限制 | 5 次连打无 429（该实例未启 limiter；limiter 需显式启用 + Valkey） |
> | 中文 | ✅ 可用（`量子计算 最新进展` → 29 条，首条百度百科） |
> | 实测引擎 | bing + yandex + sogou（35 条/次） |
> | 上下文成本 | 全量 4761 字符 ≈ **1.2k tokens**；取前 5 条 ≈ 170 tokens（snippet 天然短，max 184 字符） |

- **Endpoint**：`GET|POST http://<your-instance>/search`（也支持 `/`）[已验证 https://docs.searxng.org/dev/search_api.html]
- **Auth**：无（自建，靠网络隔离/反代保护）

```bash
curl 'http://127.0.0.1:8080/search?q=rust+async&format=json'
# POST 形式
curl -X POST 'http://127.0.0.1:8080/search' -d 'q=rust+async&format=json'
```

- **⚠️ 关键前提**：必须在 `settings.yml` 的 `search:` 段里把 `json` 加进 `formats`，**否则请求 format=json 返回 403 Forbidden** [已验证 https://docs.searxng.org/dev/search_api.html]
  ```yaml
  search:
    formats:
      - html
      - csv
      - json
      - rss
  ```
  可用值为 `html` / `csv` / `json` / `rss`；文档原文："Result formats available from web, **remove format to deny access** (use lower case)" [已验证 https://docs.searxng.org/admin/settings/settings_search.html]
- **⚠️ 公共实例基本都关掉了 JSON** —— 官方文档原文："Be aware that **many public instances have these formats disabled**" [已验证 https://docs.searxng.org/dev/search_api.html]。**必须自建。**
- **limiter / bot detection** [已验证 https://docs.searxng.org/admin/searx.limiter.html]
  - `server:` 下 `limiter: true` 启用，**需要 Valkey（原 Redis）** 连接，如 `valkey://localhost:6379/0`
  - 配置在独立的 **`/etc/searxng/limiter.toml`**；文档提醒"Don't copy all values to your local configuration, just enable what you need"
  - 方法：**`ip_limit`**（`[botdetection.ip_limit]`）与 **`link_token`**
  - 客户端 IP 从 `X-Forwarded-For` / `X-Real-IP` 提取 —— **反代配错会让所有请求看起来同一个 IP，直接触发限流**
  - 支持 **pass-list 网段**（"not monitored by the `ip_limit`"）→ 这是放行 localhost / 网关子网的官方机制
  - 其存在目的是防止 SearXNG 被上游判定为 bot
  - **网关实践建议**：私网部署保持 `limiter: false`（默认），在网络层做访问控制，同时免掉 Valkey 依赖
- **响应结构**：顶层含 `results[]` 以及 `answers` / `corrections` / `suggestions` / `infoboxes` / `number_of_results` [已验证 https://docs.searxng.org/dev/search_api.html]
  - 单条结果字段推测为 `results[].title` / `results[].url` / **`results[].content`（snippet 在 `content` 不在 `snippet`）** / `results[].engine` / `results[].score` / `results[].publishedDate` —— **未验证（请复核）**，由 result-types 索引推断，未捕获真实响应体
- **引擎脆弱性**：文档为 Google / Bing / DuckDuckGo / Brave / Startpage / Qwant / Yandex / Marginalia 等各自维护实现页 [已验证 同上]。Google/Bing 抓取器会随上游 HTML 或反爬变更而碎 —— 这也是 SearXNG 发版极频繁的原因（本次读到的文档构建号为 **2026.10.4**）。**务必显式固定引擎选择并预期频繁升级。** 注意 **Marginalia 是内置引擎**，SearXNG 可直接前置它。
- **成本**：$0 软件成本，只付托管；但**要自己承担上游引擎的封锁与 ToS 风险**。自建意味着上游会对你的 IP 限流/弹验证码 —— 官方文档甚至把 "Answer CAPTCHA from server's IP" 列为一项管理员日常任务 [已验证 同上]
- **结论**：最佳 $0 选项，也是自建 AI 网关的常规选择（docker compose + 开 json + localhost 放行）。代价：崩了你自己修，法律暴露你自己扛，无 SLA。**仅建议作 dev/test/兜底，不建议作生产唯一后端。**

### 2.16 Yandex Cloud Search API v2

- **Endpoint**：`POST https://searchapi.api.cloud.yandex.net/v2/web/search`（同步）；另有 `/v2/web/searchAsync`（deferred）与生成式检索 未验证（请复核，具体路径）
- **Auth**：IAM token（`Authorization: Bearer <iam>`）或 API key，**+ 请求体内必须带 `folderId`** 未验证（请复核）
- **⚠️ 响应形态**：v2 将结果以 **base64 编码的 XML 放在 JSON 的 `rawData` 字段里** 未验证（请复核 —— Yandex 文档页被 bot 验证拦截，仅定价页可读）。若为真，网关需做 **base64 decode → XML parse** 两段额外处理，是本清单里**集成成本最高**的一家。
- **定价（每 1,000 请求，不含 VAT）** [已验证 https://yandex.cloud/en/docs/search-api/pricing]

| 类型 | $/1k |
|---|---|
| 日间同步请求 | **$4.00** |
| 日间 deferred 请求 | **$0.25** |
| 夜间同步（00:00–07:59 UTC+3）| **$3.00** |
| 夜间 deferred | **$0.21** |
| 图片搜索 | $7.50 |
| RU 智能摘要 | $12.30 |
| **生成式回答（neuro）同步** | **$41.64** |

- 计费单位为"请求"；内部错误与鉴权错误不计费 [已验证 同上]
- **地域/实体**：美元价适用于 Iron Hive doo Beograd（塞尔维亚）或 Direct Cursus Technology L.L.C.（迪拜）客户；卢布价适用于 Yandex.Cloud LLC（俄罗斯）客户 [已验证 同上]
- **注册门槛 5/5**：需 Yandex Cloud 组织 + 手机号 + 支付卡；非俄用户需走塞尔维亚/迪拜实体 [已验证 同上（实体部分）]；免费试用赠金 未验证（请复核）
- **结论**：除非明确需要俄语/CIS 市场覆盖，**不建议纳入首批实现**

### 2.17 Google —— 两条路都不推荐

#### (a) Custom Search JSON API（Programmable Search Engine）❌ 已废弃

> **原文**："The following pricing applies only to existing Custom Search JSON API customers until the service **discontinuation on January 1, 2027**. **This API is not available for new customers.**" [已验证 https://developers.google.com/custom-search/v1/overview]
> 另一处："The Custom Search JSON API is **closed to new customers**. Vertex AI Search is a favorable alternative for searching up to 50 domains…… Existing customers have until **January 1, 2027** to transition." [已验证 同上]

历史参数（仅供参考，新项目**无法**使用）—— 四个数字均已核实 [已验证 同上]：

| 项目 | 值 |
|---|---|
| 免费额度 | 100 queries/day |
| 超出单价 | $5 / 1,000 queries |
| 日上限 | 10,000 queries/day |
| 重置 | 按天 |

技术细节（仅对存量 key 有意义）：
- Endpoint：`GET https://www.googleapis.com/customsearch/v1`，**仅 GET** [已验证 https://developers.google.com/custom-search/v1/using_rest]
- Auth：`key` query 参数（API key）+ `cx` query 参数（Search Engine ID）[已验证 同上]
- 遵循 OpenSearch 1.1 规范 [已验证 同上]
- 响应：结果在 `items[]`；官方示例用 `response.items[i].htmlTitle` 并提醒 "Make sure HTML in item.htmlTitle is escaped" [已验证 同上]。`items[].title` / `items[].link` / `items[].snippet` 的确切路径 未验证（请复核）
- **"搜全网" caveat**：PSE 默认只搜你配置的站点；Google 自己的迁移说明确认该产品定位是"searching up to **50 domains**" [已验证 https://developers.google.com/custom-search/v1/overview]。全网搜索是个配置 workaround 而非设计路径，覆盖与质量实质低于 google.com。

- **结论：aigw 不要实现这个 provider。**

#### (b) Gemini Grounding with Google Search

- 形态：**不是 raw SERP**。仅能作为 `generateContent` 调用里的 tool：`"tools": [{"type": "google_search"}]`（旧名 `google_search_retrieval`）[已验证 https://ai.google.dev/gemini-api/docs/grounding]
- **定价**：**5,000 free search requests/月**（Gemini 3.x 系列共享），超出 **$14/1,000 requests** [已验证 https://ai.google.dev/gemini-api/docs/pricing]
- ⚠️ **免费档（Free Tier）标注为 "Not available"** —— 这 5,000 次免费额度属于**付费档**权益 [已验证 同上]
- Grounding with Google Maps：5,000 prompts/月免费，超出 $14/1k [已验证 同上]
- **响应形态**：grounded 文本 + grounding metadata，相关标识含 `google_search_call` / `google_search_result` / `search_suggestions` / `webSearchQueries` / `groundingChunks` / `searchEntryPoint` [已验证 https://ai.google.dev/gemini-api/docs/grounding]
- **⚠️ 展示要求**：`search_suggestions` 是"an HTML snippet for rendering search suggestions in your UI. **Full usage requirements are detailed in the Terms of Service**" [已验证 同上] —— 应视为**强制渲染** Google Search Suggestions
- **形态不匹配**：你拿到的是**模型生成的散文 + citation chunks**，不是 title/url/snippet 排序列表。对需要干净 SERP 行的网关来说，等于花 $14/1k 买了一段还得再拆回去解析的合成文本。
- **结论**：不适合做 aigw 的 `web_search` 后端（形态不匹配 + 贵）

### 2.18 Bing / Azure —— ⚠️ 已退役，替代品昂贵且强制署名

- **Bing Search APIs v7 = 已退役（确证）**
  - 官方文档已迁入 `/previous-versions/` 命名空间，页面元数据带 `is_retired: true` / `is_archived: true`，并标 `ROBOTS: NOINDEX,NOFOLLOW` [已验证 https://learn.microsoft.com/en-us/previous-versions/bing/search-apis/bing-web-search/overview]
  - 文档最后更新 `ms.date: 2025-05-07` [已验证 同上]
  - **精确退役日期 未验证（请复核）**（普遍引用 **2025-08-11**，但 Azure 退役公告页为 JS 渲染，本次未能取得正文确认）。"已退役"这一事实已确证，具体日期未确证。
  - 旧 **F1 免费档（3/sec、1,000 次/月）随整个 API 一并消失** [已验证 同上（由全量退役推定）]

- **替代品 = Grounding with Bing Search**（Microsoft Foundry / Azure AI Foundry）

| 项目 | 值 |
|---|---|
| 价格 | **$14 / 1,000 transactions** [已验证 https://www.microsoft.com/en-us/bing/apis/grounding-pricing] |
| 速率 | **150 transactions/sec**，**1,000,000 transactions/天** [已验证 同上] |
| 变体 | `Grounding with Bing Search` 与 `Grounding with Bing Custom Search`，同价同限 [已验证 同上] |
| 免费额度 | **无**（定价表无任何免费额度）[已验证 同上] |
| 可独立调用？ | **不可以。** 仅作为 Azure AI Foundry 里的 resource/tool，或 Azure AI Search 的 Web Knowledge Source；计费在"模型推理判断需要检索并触发该 tool"时发生 [已验证 同上] |

- **⚠️ 强制署名（硬要求）**：输出"包含所引用站点的 citations 与 Bing 搜索查询链接……**These references must be shown to the end customer**" [已验证 同上]；另需按 Microsoft 提供的**原样形式**展示，遵守 "Use and Display Requirements" [已验证 https://learn.microsoft.com/en-us/azure/ai-foundry/agents/how-to/tools/bing-grounding]
- **⚠️ 无免费订阅资格**："**Only paid and pay-as-you-go Azure subscriptions are eligible**……**Sponsored subscriptions or subscriptions based on free credits are not eligible**" [已验证 同上]
- Agents (classic) 已 deprecated，**2027-03-31 退役** [已验证 同上]
- **⚠️ 常见混淆（已确证）**：微软自己的定价页把 Grounding with Bing 描述为可插入 "**Azure AI Search** and Foundry Knowledge" 的 Web Knowledge Source [已验证 https://www.microsoft.com/en-us/bing/apis/grounding-pricing]。这正是陷阱所在 —— **Azure AI Search 是对「你自己的数据」做向量/全文检索的索引服务，不是 web search API**；网页结果只能通过单独计费的 Grounding with Bing 进来。**不要把 Azure AI Search 当 SERP 供应商做预算。**
- **结论**：aigw **不建议**接 Bing 系 —— $14/1k 偏贵、强制展示条款与"喂给 LLM 自由改写"存在实质冲突、且只能在 Agent 内调用。

### 2.19 DuckDuckGo —— ❌ 诚实结论：不可用于生产

**(a) Instant Answer API** `https://api.duckduckgo.com/?q=<q>&format=json`
- 本次**实际调用验证**（`q=rust`）：返回 `Abstract` / `AbstractURL` / `Heading` / `RelatedTopics[]` / `Infobox`，**完全没有网页搜索结果列表**。`RelatedTopics` 只是维基式消歧义条目（`FirstURL` + `Text`）。
- **结论：无法作为 `web_search` 后端。** 它是"即时答案/消歧义"接口，不是 SERP 接口。

**(b) `/ac/` autocomplete** —— 本次实测可用，但无结果

```bash
curl -A 'Mozilla/5.0' 'https://duckduckgo.com/ac/?q=rust+async&type=list'
```

实测返回（HTTP 200）[已验证 live call]

```json
["rust async",["rust async","rust async trait","rust async book","rust async await",
"rust async closure","rust async_trait","rust async-std","rust async fn"]]
```

免 key 免鉴权，**但只有查询补全词，零 URL 零 snippet** —— 无法作为搜索后端。

**(c) 非官方爬取路径**（`html.duckduckgo.com` / `lite.duckduckgo.com` / `duckduckgo_search`→`ddgs`）—— **本次实测已被封**

实测 `GET https://html.duckduckgo.com/html/?q=test`：
- 返回 **HTTP 202**，14,281 bytes [已验证 live call]
- 正文含 **67 处 `anomaly`、13 处 `challenge`**，**零个** `result__a` / `result__snippet` 结果标记 [已验证 live grep]
- 正文实际是反爬挑战表单 [已验证 live call]：
  ```html
  <form id="challenge-form" action="//duckduckgo.com/anomaly.js?sv=html&cc=sre&st=...&gk=...&p=..." method="POST">
  ```

**这就是"2026 年还能用吗"的确定答案：不能。** HTTP 202 + `anomaly.js` 挑战页取代结果，正是 `ddgs` 库 issue 区的典型报错形态。机房/云 IP 会被立即挑战。

另一个佐证：该库已改名 **`ddgs`**，并**从"DDG 专用"转型为通用 metasearch** —— `text()` 后端现包含 `bing, brave, duckduckgo, google, grokipedia, mojeek, startpage, yandex, yahoo, wikipedia` [已验证 https://pypi.org/project/ddgs/]。作者被迫做多引擎兜底，本身就说明**单靠 DDG 已不可靠**。

**(d) 合法性**

DDG 既无 web-search API 产品，也无对应商业条款可供 opt-in。抓取 `html.duckduckgo.com` 是**绕过技术访问控制措施**（anomaly 挑战），属违反 ToS 且有法律暴露。更关键的是：**根本不存在任何许可授权**，所以也谈不上 attribution 或 caching 权利。具体条款原文 未验证（请复核）。

**结论：aigw 不要实现 DDG provider。** 唯一稳定的端点（`/ac/`）不返回结果；唯一返回结果的端点正被主动封锁且无授权。做"DDG 支持"等于引入一个必然在负载下崩掉的法律与可用性负债。若确有"零成本无 key"诉求，用自建 SearXNG —— 同样是元搜索，但部署与风险在你自己手里、可控、可监控。

### 2.20 其他 SERP 代理 / 抓取平台（仅列有免费档者）

#### ZenSERP

- **Endpoint**：`GET https://app.zenserp.com/api/v2/search?q=...` 未验证（请复核）
- **Auth**：`apikey` header 未验证（请复核）
- **免费**：**50 searches/月，"Free forever"** [已验证 https://zenserp.com/pricing/]
- **付费**：Small **$49.99/月 / 25,000 searches = $2.00/1k** [已验证 同上]
- **按成功计费**（"Only successful requests are billed"）[已验证 同上]
- 覆盖 Google / Bing / Yandex / DuckDuckGo 等多引擎；另有 AI Overview / Answer Engines 垂类 [已验证 同上]
- 50 次/月的免费档对网关联调而言偏小，**不建议作为主力免费档**（Serper 2,500 / Tavily 1,000 更实用）

#### ScrapingDog / ScrapingBee / ScrapingAnt、Apify、Oxylabs、Bright Data

本次未能取得确切官方数字，**全部标记 未验证（请复核）**。通用判断（供排期参考，非已验证事实）：

| 厂商 | 免费档 | 备注 |
|---|---|---|
| ScrapingDog / ScrapingBee / ScrapingAnt | 未验证（请复核） | 通用抓取平台，Google SERP 为其子功能；单次 credit 成本通常高于专用 SERP API |
| Apify Google Search Scraper | 未验证（请复核） | actor 模型有**冷启动延迟**，对同步 `web_search` tool 不友好 |
| Oxylabs SERP | 未验证（请复核） | 企业级，通常需 **KYC / 企业实体**，注册难度 4–5 |
| Bright Data SERP | 未验证（请复核） | 同上，KYC 门槛高 |

> **选型结论**：这一类（通用抓取/代理平台）对 aigw 的 `web_search` 场景**性价比与集成成本均劣于**专用 SERP API（Serper/SearchApi）或 AI-native API（Tavily/Brave）。**不建议纳入首批实现。** 若后续有需求再单独调研。

---

### 2.21 中文/国内选项

#### 博查 Bocha ⭐ 中文首选

- **Endpoint**：`POST https://api.bocha.cn/v1/web-search` [已验证 https://aq6ky2b8nql.feishu.cn/wiki/RXEOw02rFiwzGSkd9mUcqoeAnNK]
  - ⚠️ **双域名并存**：官方文档已换成 `api.bocha.cn`，官网首页示例仍写 `api.bochaai.com`。**以文档的 `bocha.cn` 为准**；控制台 `open.bocha.cn`。
- **Auth**：`Authorization: Bearer {API KEY}` [已验证 同上]

```bash
curl --location 'https://api.bocha.cn/v1/web-search' \
  --header 'Authorization: Bearer BOCHA-API-KEY' \
  --header 'Content-Type: application/json' \
  --data '{"query":"阿里巴巴2024年ESG报告","freshness":"noLimit","count":10,"summary":true}'
```

**响应结构 —— ⭐ 兼容 Bing Web Search API 格式** [已验证 https://open.bochaai.com/ + Feishu 文档]

```json
{
  "_type": "SearchResponse",
  "queryContext": { "originalQuery": "..." },
  "webPages": {
    "webSearchUrl": "https://bochaai.com/search?q=...",
    "totalEstimatedMatches": 606721,
    "value": [
      {
        "id": "...",
        "name": "阿里巴巴发布2024年ESG报告 持续推进减碳与数字化普惠",
        "url": "https://www.alibabagroup.com/document-...",
        "siteName": "阿里巴巴集团",
        "siteIcon": "...",
        "snippet": "阿里巴巴集团发布《2024财年环境、社会和治理（ESG）报告》...",
        "summary": "报告显示，阿里巴巴扎实推进减碳举措...",
        "datePublished": "2024-07-22T00:00:00+08:00"
      }
    ]
  }
}
```

| 字段 | JSON path |
|---|---|
| title | **`webPages.value[].name`**（注意是 `name`）|
| url | `webPages.value[].url` |
| snippet | `webPages.value[].snippet` |
| 长摘要 | `webPages.value[].summary`（需 `summary:true`）|
| published_date | `webPages.value[].datePublished`（ISO8601）|
| score | **无** |

- **免费**：Web Search API **免费试用资源包 1,000 次 / ¥0 / 有效期 3 个月** [已验证 https://aq6ky2b8nql.feishu.cn/wiki/JYSbwzdPIiFnz4kDYPXcHSDrnZb]
- **付费**：目录价 **¥0.036/次 = ¥36/1,000 次** [已验证 同上]
  - 折算美元：¥36 ÷ 7.1 ≈ **$5.07/1k**
  - 资源包：体验包 1,000 次 ¥3.6（¥3.6/千次）；标准包 ¥36/千次 [已验证 同上]
  - 官方对标：微软 Bing ¥108/千次，宣称便宜 67% [已验证 同上]
- **AI Search API**（含大模型总结 + 模态卡）：¥0.060/次 [已验证 同上]
- **性能**：**0.15s 响应延迟**，官方称最高支持 **1000–2000 QPS** [已验证 同上]
- **单次最多 50 条**（`count` 最大 50）[已验证 同上]
- **注册**：国内手机号 + 实名 未验证（请复核，具体实名层级）
- **内容形态**：snippet + 可选 `summary` 长摘要（非全文）
- **结论**：中文场景性价比与易用性最佳；**Bing 兼容格式意味着 aigw 可以用同一个 adapter 同时支持 Bocha 与任何 Bing-shaped API**，这是重要的实现杠杆。

#### 智谱 Zhipu / BigModel —— 有独立搜索接口

- **Endpoint**：`POST https://open.bigmodel.cn/api/paas/v4/web_search` —— **确认存在独立搜索接口**（不必走 chat completions）[已验证 https://docs.bigmodel.cn/api-reference/工具-api/网络搜索]
- **Auth**：`Authorization: Bearer <token>` [已验证 同上]

```bash
curl --request POST \
  --url https://open.bigmodel.cn/api/paas/v4/web_search \
  --header 'Authorization: Bearer <token>' \
  --header 'Content-Type: application/json' \
  --data '{"search_query":"2025年4月的财经新闻","search_engine":"search_std","count":10,
           "search_recency_filter":"noLimit","content_size":"medium"}'
```

**响应结构** [已验证 同上]

```json
{
  "id": "...", "created": 1748261757, "request_id": "...",
  "search_intent": [ { "query": "...", "intent": "SEARCH_ALL", "keywords": "..." } ],
  "search_result": [
    { "title": "2025年5月23日财经早资讯",
      "content": "一、1-4月我国对外直接投资575.4亿美元...",
      "link": "https://www.sohu.com/a/897879632_121123890",
      "media": "搜狐",
      "icon": "...",
      "refer": "ref_1",
      "publish_date": "2025-05-23" }
  ]
}
```

| 字段 | JSON path |
|---|---|
| title | `search_result[].title` |
| url | **`search_result[].link`** |
| snippet | `search_result[].content` |
| published_date | `search_result[].publish_date` |
| 来源媒体 | `search_result[].media` |
| score | **无** |

- **可选引擎**：`search_std`（基础版）/ `search_pro`（高阶）/ `search_pro_sogou`（搜狗）/ `search_pro_quark`（夸克）[已验证 同上]
- **`count`**：1–50，默认 10 [已验证 同上]
- **`content_size`**：`medium`（摘要）/ `high`（最大化上下文）—— **直接影响 LLM context 成本，网关应默认 `medium`** [已验证 同上]
- **`search_recency_filter`**：`oneDay|oneWeek|oneMonth|oneYear|noLimit` [已验证 同上]
- **`search_domain_filter`**：域名白名单 [已验证 同上]
- **`search_intent`**：可开启意图识别，自动判断是否需要检索 [已验证 同上]
- **定价 / 免费额度**：未验证（请复核）
- **注册**：需实名认证 未验证（请复核，层级）

#### 其他国内选项

| 厂商 | 独立可调用？ | 免费额度 | 备注 |
|---|---|---|---|
| **阿里云 IQS / 通义** | 未验证（请复核） | 未验证（请复核） | 若走阿里云 AK/SK 签名，**实现成本显著高于 Bearer token**，需单独评估 |
| **百度 AI 搜索 / 千帆** | 未验证（请复核） | 未验证（请复核） | 需百度智能云账号 + 实名 |
| **腾讯混元 search** | 未验证（请复核） | 未验证（请复核） | 疑似仅能绑在模型调用内，非独立 SERP |
| **火山引擎/方舟、SiliconFlow、秘塔** | 未验证（请复核） | 未验证（请复核） | — |

> 以上四类本次未能在官方页面取得确切数字，**全部标记未验证**。若要纳入选型需补充调研。
> 一个通用提醒：**阿里云/腾讯云系 API 多采用 AK/SK 请求签名**（而非简单 Bearer token），对 Rust 侧实现意味着额外的签名算法工作量，排期时要计入。

---

## §3 抽象设计建议

### 3.1 aigw 内部统一结果形状

```rust
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,              // 统一的摘要文本
    pub published_date: Option<DateTime<Utc>>,
    pub score: Option<f32>,           // 多数厂商没有，保持 Option
    pub source: Option<String>,       // 站点名/媒体名（Bocha siteName / Zhipu media）
    pub raw_content: Option<String>,  // 全文，默认不填充
}

pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub provider: &'static str,
    pub cost_usd: Option<f64>,        // Exa costDollars / Valyu total_deduction_dollars
    pub answer: Option<String>,       // Tavily answer / Linkup answer，可选透传
}
```

### 3.2 映射清洁度分级

**A 级 —— 几乎零成本直映射**

| 厂商 | title | url | snippet | date | score |
|---|---|---|---|---|---|
| **Tavily** | `title` | `url` | `content` | `published_date` | ✅ `score` |
| **Valyu** | `title` | `url` | `content` | `publication_date` | ✅ `relevance_score` |
| **Perplexity Search** | `title` | `url` | `snippet` | `date` | ❌ |
| **Marginalia** | `title` | `url` | `description` | ❌ | ❌ |

**B 级 —— 改个字段名即可**

| 厂商 | 注意点 |
|---|---|
| **Brave** | snippet 在 `description`；结果在 `web.results[]` 而非顶层 |
| **Serper** | url 在 `link`；`date` 是**相对时间串**（"2 days ago"）需额外解析 |
| **SerpApi / SearchApi** | url 在 `link`；结果在 `organic_results[]`；organic 无日期 |
| **Exa** | date 在 `publishedDate`；snippet 用 `highlights[]`（数组需 join）；score 仅 `highlightScores[]` |
| **Firecrawl** | snippet 在 `description`；结果在 `data.web[]` |
| **Mojeek** | snippet 在 **`desc`**；结果在 **`response.results[]`**（多一层 `response` 包裹）；需检查 `response.status`（可能是 `"ERROR: Daily Limit Reached"`）；**必须显式传 `fmt=json`，默认是 XML** |
| **Marginalia** | snippet 在 `description`；顶层 `license` 字段需透传（非商业 key 的合规信号）|
| **博查 Bocha** | **title 在 `name`**；结果在 `webPages.value[]`；有 `summary` 可选长摘要 |
| **智谱 Zhipu** | url 在 `link`；结果在 `search_result[]`；snippet 在 `content` |

**C 级 —— 需要特殊处理**

| 厂商 | 特殊处理 |
|---|---|
| **Parallel AI** | snippet 是 **`excerpts[]` 字符串数组**，需 join；且**必须显式传 `mode`** 否则默认最贵档 |
| **Linkup** | **title 字段名是 `name`**；无 date/score；`sourcedAnswer` 模式返回 answer + sources 混合体 |
| **Jina `s.jina.ai`** | **只有全文 markdown 一种模式** —— ⚠️ **没有** `no-content` 之类的降级开关（`X-Respond-With` 实为 OCR，40× token）。纯元数据与全文**同价**。计费为 token 制（每请求起步 10k tokens），与"按次"模型不可直接比价。只能用 `X-Retain-Images`/`X-Target-Selector`/`X-Token-Budget` 压缩 |
| **Firecrawl（带 scrape）** | `markdown` 字段是全文，走 `raw_content` 而非 `snippet` |
| **Yandex v2** | **`rawData` 是 base64 编码的 XML** 未验证（请复核）—— 需 base64 decode → XML parse 两段处理，集成成本最高 |
| **Perplexity Sonar/Chat** | 返回 **grounded 散文 + citations，不是 result list** —— 不应走 `web_search` tool 路径，若要支持应单列为 `answer` 能力 |
| **Gemini grounding** | 同上，只能拿 `groundingMetadata` 里的引用，形态不匹配 |
| **Grounding with Bing** | 同上 + **强制原样展示要求**，与"喂给 LLM 自由改写"存在条款冲突 |
| **SearXNG** | 自建；`format=json` 需在 `settings.yml` 显式开启，否则 403 |

### 3.3 设计建议

1. **`snippet` 与 `raw_content` 必须分开，且 `raw_content` 默认关闭。** Jina 默认全文、Firecrawl/Exa 可选全文、Tavily `include_raw_content` —— 若不显式控制，单次搜索可能注入几万 token，直接推高下游模型成本。建议 tool 参数暴露 `include_content: bool = false`。
2. **`score` 一律 `Option<f32>`。** 只有 Tavily / Valyu / Exa(highlightScores) 有；其余靠数组顺序表达排序。不要为了"补齐"而伪造分数。
3. **复用 Bing-shaped adapter。** 博查兼容 Bing Web Search 响应格式 —— 一个 `webPages.value[]` 解析器可同时服务博查及任何 Bing 兼容实现，是性价比最高的中文接入方式。
4. **把 `cost_usd` 纳入统一结构。** Exa(`costDollars`)、Valyu(`total_deduction_dollars`)、Firecrawl(`creditsUsed`)、Tavily(`usage.credits`) 都在响应内回传消耗，网关可直接做 SpendLog 归因，无需二次查用量 API。
5. **日期解析要容错。** Serper 给相对时间串、Tavily 给 RFC2822、Exa/博查给 ISO8601、部分厂商干脆没有。统一 `Option<DateTime<Utc>>` + 解析失败降级为 `None`，不要 panic。
6. **Provider trait 建议按"能力"而非"厂商"切分**：`SearchProvider`（返回 results）与 `AnswerProvider`（返回 grounded prose）分离，避免把 Perplexity Sonar / Gemini grounding / Bing grounding 硬塞进 `web_search`。

---

## §4 风险登记册

| 风险 | 等级 | 说明 | 建议 |
|---|---|---|---|
| **Google CSE 2027-01-01 停服 + 新客户不可用** | 🔴 高（已确定） | 已验证官方公告 | **不实现该 provider** |
| **DDG 爬取已被实测封锁 + ToS 违规** | 🔴 高（已实测） | `html.duckduckgo.com` 返回 202 + `anomaly.js` 挑战页、零结果；`ddgs` 已被迫转多引擎兜底 | **不实现 DDG provider**；用自建 SearXNG 替代"零成本"诉求 |
| **Bing v7 已退役；替代品 $14/1k + 无免费 + 强制署名** | 🔴 高（已确定） | 文档 `is_retired: true`；Grounding 仅 Agent 内、强制向终端用户展示 citations | **不接 Bing 系**；若客户强需求，单独评估展示合规 |
| **Mojeek 缓存权分档（Startup 仅 1 小时）** | 🟡 中 | Startup 档只允许缓存 1 小时，完整存储权要 Business | **网关缓存 TTL 必须按档位配置**，别写死 |
| **Marginalia 非商业 key 是 CC-BY-NC-SA** | 🟡 中 | 商业 SaaS 用非商业 key 违规；响应 `license` 字段是合规信号 | 商业用途必须买商业 key；透传 `license` |
| **Brave 定价模型已变** | 🟡 中 | 旧"2,000次/月免费 + 1 QPS"已变成 "$5 credits/月 + $5/1k + 50 QPS" | 文档与代码注释不要再引用旧数字 |
| **Serper credits 6 个月过期** | 🟡 中 | 预充值沉没成本 | 低频场景优先选按量计费厂商 |
| **Tavily 会用你的 query 训练模型** | 🔴 高（已逐字核实） | Platform Terms §6.5 明确可保留并使用 Customer Input/Output 进训练集；部分第三方 provider 无保密义务 | **敏感 query 不走 Tavily**；改用带 ZDR 的厂商或谈 Enterprise。**这点应在 aigw 文档里对用户明示** |
| **Valyu 按"条结果"计费** | 🟡 中 | $1.50/1k retrieval，10 结果 ≈ $15/1k 查询 | 比价时统一折算到"每次查询"；设 `max_price` |
| **Parallel 默认走最贵档** | 🟡 中 | `mode` 不传默认 `advanced` | **代码里必须显式传 `mode`** |
| **Jina 只有全文一种模式，无法降级** | 🟡 中 | `s.jina.ai` 固定 10k tokens 起步，纯元数据与全文同价；**`X-Respond-With` 不是降级开关而是 40× OCR 开关** | 不要把 Jina 当「便宜 SERP」用；仅在确实需要全文时选它。用 `X-Retain-Images`/`X-Target-Selector` 压缩 |
| **Jina ToS 竞业条款** | 🟡 中 | S4.5(iii) 禁止用 Output 构建与 Jina 竞争的服务 —— 网关转卖搜索能力可能触线 | 上生产前法务确认；或改用其他厂商 |
| **SERP 代理的 Google ToS 灰区** | 🟡 中 | Serper/SerpApi/SearchApi 本质是抓 Google | SerpApi/SearchApi 有法律盾（且 SerpApi Free 档不含）；Serper 无。合规敏感场景优先 **Brave/Mojeek 独立索引** |
| **SearXNG 上游封锁 + 运维负担** | 🟡 中 | Google engine 常失效；limiter 需 Valkey；JSON 需手动开（否则 403）；反代配错 `X-Forwarded-For` 会误触限流 | 仅作 dev/兜底；固定引擎选择、监控成功率、私网部署可关 limiter |
| **Yandex 注册门槛与地缘** | 🟠 中低 | 需 Cloud 组织+卡+手机；非俄用户走塞尔维亚/迪拜实体 | 非 CIS 需求不纳入首批 |
| **国内厂商实名门槛** | 🟠 中低 | 博查/智谱等需手机实名；阿里云/腾讯云需 AK/SK 签名（额外工作量） | 文档提示用户自备 key；签名类厂商排期另算 |
| **本报告多项 ToS 未验证** | 🟡 中 | 多数厂商法律页 JS 渲染/被挡，attribution 与 caching 权利未确认 | **上生产前逐家读 ToS**，尤其"能否缓存结果"直接影响网关缓存设计 |

---

## §5 Sources

验证日期统一为 **2026-10-06**。以下为本次实际成功读取的页面。

**AI-native / SERP 代理**
- https://docs.tavily.com/documentation/api-reference/endpoint/search
- https://www.tavily.com/pricing
- https://docs.tavily.com/documentation/api-credits
- https://docs.tavily.com/documentation/rate-limits
- https://www.tavily.com/terms
- https://exa.ai/pricing
- https://exa.ai/docs/reference/search
- https://docs.exa.ai/reference/rate-limits
- https://serper.dev （首页，含完整定价表与 live sample payload）
- https://serpapi.com/pricing
- https://serpapi.com/search-api
- https://www.searchapi.io/pricing
- https://www.searchapi.io/docs/google
- https://www.linkup.so/pricing
- https://docs.linkup.so/pages/documentation/api-reference/endpoint/post-search
- https://parallel.ai/pricing
- https://docs.parallel.ai/search-api/search-quickstart
- https://docs.parallel.ai/search-api/processors
- https://www.valyu.ai/pricing
- https://docs.valyu.ai/pricing
- https://docs.valyu.ai/api-reference/endpoint/search

**Brave**
- https://brave.com/search/api/
- https://api-dashboard.search.brave.com/app/documentation/web-search/get-started

**Perplexity**
- https://docs.perplexity.ai/getting-started/pricing
- https://docs.perplexity.ai/api-reference/search-post

**Reader / Crawler**
- https://jina.ai/reader/
- https://www.firecrawl.dev/pricing
- https://docs.firecrawl.dev/api-reference/endpoint/search
- https://zenserp.com/pricing/

**大厂 / 独立索引 / 自建**
- https://developers.google.com/custom-search/v1/overview （**停服公告 + 四项额度数字**）
- https://developers.google.com/custom-search/v1/using_rest （CSE endpoint/auth）
- https://ai.google.dev/gemini-api/docs/pricing （Gemini grounding 定价）
- https://ai.google.dev/gemini-api/docs/grounding （grounding 形态与展示要求）
- https://learn.microsoft.com/en-us/previous-versions/bing/search-apis/bing-web-search/overview （**`is_retired: true` 元数据，确证退役**）
- https://learn.microsoft.com/en-us/bing/search-apis/bing-web-search/reference/endpoints
- https://www.microsoft.com/en-us/bing/apis/grounding-pricing （**$14/1k、150 tps、无免费档、强制署名**）
- https://learn.microsoft.com/en-us/azure/ai-foundry/agents/how-to/tools/bing-grounding
- https://www.mojeek.com/services/api.html （定价 + ToS/AI 条款）
- https://www.mojeek.com/services/search/web-search-api/
- https://www.mojeek.com/support/api/search/quickstart.html （endpoint + curl）
- https://www.mojeek.com/support/api/search/request_parameters.html （`fmt` 默认 xml）
- https://www.mojeek.com/support/api/search/json_response.html （响应结构）
- https://about.marginalia-search.com/article/api/
- https://docs.searxng.org/dev/search_api.html （构建号 2026.10.4）
- https://docs.searxng.org/admin/settings/settings_search.html （`formats` 可用值）
- https://docs.searxng.org/admin/searx.limiter.html （limiter / Valkey / pass-list）
- https://yandex.cloud/en/docs/search-api/pricing
- https://pypi.org/project/ddgs/
- https://api.duckduckgo.com/?q=rust&format=json&no_html=1 （**实际调用：仅消歧义，无结果列表**）
- https://duckduckgo.com/ac/?q=rust+async&type=list （**实际调用 200：仅补全词**）
- https://html.duckduckgo.com/html/?q=test （**实际调用：202 + `anomaly.js` 挑战页，零结果**）
- https://duckduckgo.com/duckduckgo-help-pages/results/sources/

**中文**
- https://open.bochaai.com/
- https://aq6ky2b8nql.feishu.cn/wiki/RXEOw02rFiwzGSkd9mUcqoeAnNK （博查 Web Search API 文档）
- https://aq6ky2b8nql.feishu.cn/wiki/JYSbwzdPIiFnz4kDYPXcHSDrnZb （博查 API 定价）
- https://aq6ky2b8nql.feishu.cn/wiki/HmtOw1z6vik14Fkdu5uc9VaInBb （博查快速开始）
- https://docs.bigmodel.cn/api-reference/工具-api/网络搜索 （智谱 web_search API reference）
- https://docs.bigmodel.cn/cn/guide/models/web-search （智谱搜索服务概览）

**未能读取（故相关数字标为未验证）**
- `docs.serper.dev/*`（403）—— Serper endpoint path 与 `X-API-KEY` header 名未能官方确认
- `yandex.cloud/en/docs/search-api/quickstart`、`.../concepts/*`（bot 验证拦截）—— v2 `rawData`/base64 细节未确认
- `jina.ai/api-dashboard/pricing` token 价格表（JS 渲染）
- `exa.ai/terms`、`linkup.so/terms-of-service`、`serpapi.com/legal/terms`（JS 渲染或 404）
- 阿里云 IQS / 百度 AI 搜索 / 腾讯混元 search 官方定价页
