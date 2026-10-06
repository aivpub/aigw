# Stage 138: 搜索 Provider / 实例 / 定价的 DB 化与管理 UI（Phase 53）

**所属**: Phase 53（内建 Web Search / TD-017c）
**预估**: 14h（028 三方言 migration + CRUD 端点 + 实例子资源 + 连通性探测 + 前端管理页 + i18n + UT/BDD）
**依赖**: Stage 134（`WebSearchConfig` / `WebSearchProviderConfig` / 实例结构 / `SearchProvider` trait）、Stage 136（`cost_per_query` 的计费消费者）
**状态**: ⏳ 规划

---

## 1. 目标

Stage 134 的搜索配置只能写在 `config.yaml`，**改一次单价或加一个实例都要改文件 + 重启进程**。本 Stage 把它搬进数据库并配上管理 UI，使其与仓内既有的同类资源（模型定价、代理服务）治理方式一致。

三件事：① provider 与实例落库（含 `api_key` 加密）；② admin CRUD + 连通性探测端点；③ 前端管理页（含**定价编辑**）。

### 为什么必须做（两处既有的不一致）

| 资源 | 定价可在 UI 编辑？ | 实例/端点可在 UI 管理？ | 依据 |
|------|------------------|---------------------|------|
| **模型（LLM）** | ✅ `input_cost_per_token` / `output_cost_per_token` | ✅ Models 页 CRUD | `crates/aigw-frontend/src/pages/models/ModelDialog.tsx:166-167`、`:93-96` |
| **代理服务** | — （无定价概念） | ✅ Proxies 页 CRUD + 探测 + 启停 | `crates/aigw-core/migrations/sqlite/027_proxies.sql`、`crates/aigw-server/src/routes/proxies.rs:184-625`、`crates/aigw-frontend/src/pages/proxies/` |
| **搜索 provider（Stage 134 现状）** | ❌ **只能改 yaml + 重启** | ❌ **只能改 yaml + 重启** | 本 Stage 修正 |

**搜索单价是真实成本，不是摆设**：SearXNG 自建**并非零成本**（服务器 + 带宽 + 运维），`cost_per_query` **缺省 `0.01` USD/次**（= $10/1k，牌价占位）**应被改写为自建摊销值**；Tavily/博查接入后更是直接的采购单价。**单价既然会变（换机型、调带宽、厂商调价、汇率波动）、且缺省值本身就需要被改，就必须可运营修改而非改码重启 —— 这是本 Stage 存在的首要理由。**

### 验收标准

- [ ] `028_web_search_providers.sql` × 三方言（sqlite / postgres / mysql）建表并通过 `aigw-migrate` 测试
- [ ] provider CRUD + **实例子资源** CRUD 端点全部可用（含批量删除、启停）
- [ ] `api_key` **加密落库**（`v2:gcm:`），list / info 响应 **redact**（照 `proxies` 的 `proxy_url` 处理）
- [ ] **in-use 守卫**：被 `default_provider` / `failover_order` 引用的 provider 不可删 → 409（照 `proxies` 的 in-use 守卫）
- [ ] 连通性探测端点：对单个**实例**发一次真实搜索，回报 HTTP 状态 / 延迟 / 结果条数 / 解析是否成功，快照存 `probe_result`
- [ ] 前端管理页：provider 列表 + 实例子表 + **定价编辑**（`cost_per_query`）+ 探测按钮 + 启停 + i18n（en / zh-CN）
- [ ] **配置源优先级明确且有 UT**：DB 有记录 → 用 DB；DB 空 → 回落 `config.yaml`；两者皆空 → 禁用（零行为变化）
- [ ] 单价改动后**不重启进程即生效**（运行期 registry 可刷新）
- [ ] `task test` / `task bdd` / `task fmt` / `task lint` / `task fe-build` / `task fe-lint` 全绿

### 明确不做（边界）

- **不改搜索执行逻辑**（Stage 134 的 trait / 归一化 / 护栏零改动）
- **不改注入逻辑**（Stage 135 零改动）
- **不改计费公式**（Stage 136 的 `calc_search_spend` 零改动；本 Stage 只改「单价从哪来」）
- **不做搜索调用日志展现**（Stage 137 已交付）
- **不实现 Tavily / 博查客户端**（抽象与 `kind` 已就位，接入是纯增量；但本 Stage 的 UI **允许录入** `kind=tavily|bocha` 的配置，校验时对未实现的 kind 明确报错）
- **不做配额加权 LB / Redis 配额预留**（sub2api `manager.go` 全套，多 key 场景出现后再议）
- **不做跨租户隔离**（搜索配置是全局基础设施，与 `proxies` 同级）

---

## 2. 现状证据

### 2.1 `proxies` 是逐项可照抄的同构先例（Phase 50 / Stage 122-124）

| 维度 | `proxies` 实现 | 位置 |
|------|---------------|------|
| 建表 | `id` / `name` / **整串加密的 `proxy_url`** / `status` / `expires_at` / **`probe_result` 单 JSON 列** / `created_at` / `updated_at` | `crates/aigw-core/migrations/sqlite/027_proxies.sql:10-20` |
| 加密落库 | `proxy_url` 整串 `v2:gcm:` | `027_proxies.sql:12` 注释 |
| 探测快照 | `probe_result TEXT NOT NULL DEFAULT '{}'`，顶层 `status` 仅用于过滤 | `027_proxies.sql:6-8` 注释 |
| CRUD 端点 | `list` / `list_all` / `create` / `get` / `update` / `delete` / `batch_delete` / `test` / `quality` / `toggle` | `crates/aigw-server/src/routes/proxies.rs:184,239,261,303,328,387,430,557,588,625` |
| 路由注册 | `/admin/proxies` + `/all` + `/batch-delete` + `/:id`（get/put/delete） | `crates/aigw-server/src/main.rs:636-647` |
| 前端 | `index.tsx` + `ProxyDialog.tsx` + `QualityDialog.tsx` + `types.ts` | `crates/aigw-frontend/src/pages/proxies/` |

**→ 本 Stage 的形态就是「`proxies` + 一层实例子资源 + 一个定价字段」。** 不需要任何新机制。

### 2.2 模型定价已可在 UI 编辑 —— 搜索定价没有是明确的不一致

`crates/aigw-frontend/src/pages/models/ModelDialog.tsx`：`:93-96` 读取（`info.input_cost_per_token ?? p.input_cost_per_token`）、`:166-167` 与 `:189` 提交。**即「单价由运营在 UI 改」在本仓已是既定做法**，搜索 provider 理应对齐。

### 2.3 最大 migration 号 = 027

`crates/aigw-core/migrations/sqlite/` 现有最大为 `027_proxies.sql`（另有 `024_deleted_tables` / `025_image_tokens` / `026_proxy_models_enabled`）。→ **本 Stage 用 `028`**。

### 2.4 Stage 134 已为本 Stage 预留接口

Stage 134 §3.6 要求 `build_websearch_registry` **接受已解析好的结构而非自己读文件**，故本 Stage 只需新增「从 DB 行构造同一结构」的装载路径，`websearch` 模块本体零改动。

---

## 3. 方案

### 3.1 表结构（`028_web_search_providers.sql` × 三方言）

**两张表**，对应 Stage 134 的两层模型（provider = 逻辑后端 + 定价；instance = 物理端点）：

```sql
-- 搜索 provider（逻辑后端 + 定价）
CREATE TABLE IF NOT EXISTS web_search_providers (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,                 -- 逻辑名，registry key，唯一
    kind            TEXT NOT NULL,                 -- searxng | tavily | bocha（未实现的 kind 校验时报错）
    cost_per_query  REAL NOT NULL DEFAULT 0.01,    -- USD/次；缺省 0.01(=$10/1k 牌价占位,对齐 sub2api 174_*.sql)；自建应改摊销值(见 §3.5)
    enabled         INTEGER NOT NULL DEFAULT 1,
    is_default      INTEGER NOT NULL DEFAULT 0,    -- 取代 yaml 的 default_provider
    failover_order  INTEGER,                       -- NULL = 不参与故障转移；数字越小越先
    max_results     INTEGER,                       -- NULL = 用全局默认 5
    timeout_ms      INTEGER,                       -- NULL = 用全局默认
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 搜索实例（物理端点，一个 provider 下 N 个）
CREATE TABLE IF NOT EXISTS web_search_instances (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id     INTEGER NOT NULL REFERENCES web_search_providers(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,                 -- 实例标识（便于日志/对账定位）
    base_url        TEXT NOT NULL,
    api_key         TEXT,                          -- v2:gcm: 加密落库；SearXNG 可为 NULL
    weight          INTEGER,                       -- NULL/0 → 不参与加权（照 router.rs 语义）
    enabled         INTEGER NOT NULL DEFAULT 1,
    probe_result    TEXT NOT NULL DEFAULT '{}',    -- 探测快照 JSON（照 proxies）
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
```

**方言差异**（照 `027_proxies.sql` 的三方言惯例）：`INTEGER PRIMARY KEY AUTOINCREMENT` → PG `SERIAL`/`BIGSERIAL`、MySQL `AUTO_INCREMENT`；`REAL` → PG `DOUBLE PRECISION`、MySQL `DOUBLE`；布尔用 `INTEGER 0/1`（与仓内既有列一致，`proxy_models_enabled` 先例）；`probe_result` PG 用 `JSONB`、MySQL 用 `JSON`（**Stage 125 踩过的方言坑**，见 roadmap v58.0「修复 PG/MySQL probe_result JSONB/JSON 方言」）。

> ⚠️ **`ON DELETE CASCADE` 的三方言一致性**必须逐一验证 —— SQLite 需 `PRAGMA foreign_keys=ON` 才生效（仓内连接是否开启须核实；若未开启，改为应用层级联删除 + UT 锁定）。

### 3.2 配置源优先级

```
DB 有 enabled 的 provider 记录  → 用 DB（忽略 yaml 的 web_search 段，启动日志 info 提示）
DB 为空 且 yaml 有 web_search  → 用 yaml（Stage 134 行为，向后兼容）
两者皆空                        → 禁用（零行为变化）
```

**不做合并**（yaml + DB 取并集）—— 两个真相来源会产生「改了 yaml 不生效」的排查地狱。单一来源 + 启动日志明示用了哪个。

### 3.3 端点（照 `proxies.rs` 逐一对应）

| 端点 | 方法 | 对标 |
|------|------|------|
| `/admin/websearch/providers` | GET / POST | `proxies.rs:184` / `:261` |
| `/admin/websearch/providers/:id` | GET / PUT / DELETE | `:303` / `:328` / `:387` |
| `/admin/websearch/providers/batch-delete` | POST | `:430` |
| `/admin/websearch/providers/:id/toggle` | POST | `:625` |
| `/admin/websearch/providers/:id/instances` | GET / POST | 新增（子资源） |
| `/admin/websearch/instances/:id` | GET / PUT / DELETE | 新增 |
| `/admin/websearch/instances/:id/toggle` | POST | 照 `:625` |
| `/admin/websearch/instances/:id/test` | POST | **照 `:557` `test_proxy`** —— 对该实例发一次真实搜索 |
| `/admin/websearch/reload` | POST | 重建运行期 registry（§3.6） |

**安全要求（照 Phase 51 Stage 130 安全审计八项）**：`api_key` 加密落库（`crypto::encrypt_litellm_value_gcm`，`crypto.rs:178`）；list / get 响应 **redact**；日志不打明文；错误只透传 message（`chat::upstream_error_message` 既有收窄）。

### 3.4 连通性探测（`/instances/:id/test`）

对该实例发一次真实搜索（固定 query，如 `"aigw connectivity probe"`），快照存 `probe_result`：

```json
{
  "ok": true, "http_status": 200, "latency_ms": 2430,
  "result_count": 35, "parse_ok": true,
  "probed_at": "2026-10-06T19:00:00Z", "error": null
}
```

**比 `proxies` 的探测更有信息量的一点**：`parse_ok` 与 `result_count` 能抓到「HTTP 200 但响应不是预期形状」——这正是 Stage 134 §3.3.2 实测发现的 SearXNG 陷阱（`format` 参数无效时**静默返回 HTTP 200 + HTML**）。→ **探测必须跑完整的 `parse_response`，不能只看状态码。**

### 3.5 定价的 UI 语义（本 Stage 的核心产品点）

`cost_per_query` 编辑框旁必须有说明文案，三种情形各自给指引：

| provider | 文案要点 |
|----------|---------|
| **SearXNG（自建）** | **缺省 `0.01`（$10/1k）是行业牌价占位，不是你的自建成本。** 请按「(服务器月成本 + 带宽 + 运维摊销) ÷ 月搜索次数」改写（自建典型落在 `1e-4` 量级，**不改约高估 50 倍**）。显式填 0 则成本在账目上不可见（不是「免费」） |
| **Tavily** | 牌价 $8/1k → `0.008`（免费额度期内实际为 0，但建议按牌价计提，与 litellm 一致；差额由财务侧对账吸收） |
| **博查 Bocha** | 牌价 **¥36/1k**（CNY）→ **须自行换算为 USD**（参考 `0.00507`）。⚠️ **汇率决策未落地前，该 provider 的落库金额不得用于对外出账**（Stage 136 §8 登记） |

### 3.6 运行期生效（不重启）

`/admin/websearch/reload` 重新从 DB 装载并原子替换 `AppState` 里的 registry（`Arc<RwLock<Option<WebSearchRegistry>>>` 或 `ArcSwap`）。**provider / 实例 / 单价的任何写操作成功后自动触发一次 reload**，使「UI 改完即生效」成为默认行为（无需用户手动点 reload）。

---

## 4. TDD 计划

### 4.1 单元测试

| 组 | UT | 期望 |
|----|----|------|
| migration | `test_028_web_search_tables_created_all_dialects` | 三方言建表成功，列类型符合预期 |
| migration | `test_028_instance_cascade_delete` | 删 provider → 其 instances 一并删除（或应用层级联生效） |
| CRUD | `test_create_provider_encrypts_api_key` | 落库值带 `v2:gcm:` 前缀，非明文 |
| CRUD | `test_list_providers_redacts_api_key` | 响应中 `api_key` 已脱敏 |
| CRUD | `test_delete_provider_in_use_returns_409` | 被 `is_default` / `failover_order` 引用 → 409 |
| CRUD | `test_unknown_kind_rejected_lists_supported` | `kind=exa` → 400 且错误列出 `searxng`（及已实现的 kind） |
| CRUD | `test_unimplemented_kind_tavily_rejected_with_hint` | `kind=tavily` → 400 且提示「抽象已就位，客户端未实现（Stage 134 §8.2）」 |
| 配置源 | `test_db_records_take_precedence_over_yaml` | DB 有记录 → yaml 被忽略 |
| 配置源 | `test_falls_back_to_yaml_when_db_empty` | DB 空 → 用 yaml（Stage 134 行为） |
| 配置源 | `test_disabled_when_both_empty` | 两者皆空 → registry 为 `None`，零行为变化 |
| 探测 | `test_probe_detects_html_body_as_parse_failure` | ⭐ 喂 HTTP 200 + HTML（SearXNG `format` 陷阱）→ `parse_ok: false`、`ok: false` |
| 探测 | `test_probe_records_latency_and_result_count` | `probe_result` 含 `latency_ms` / `result_count` |
| reload | `test_price_change_takes_effect_without_restart` | 改 `cost_per_query` → 后续计费用新单价（配合 Stage 136 的 `calc_search_spend`） |

### 4.2 BDD

`websearch_admin.feature`（mock）：provider 创建 → 加实例 → 探测 → 改单价 → 启停 → in-use 删除被拒 409 → 批量删除。**real BDD 三后端**（`bdd-real-sqlite|pg|mysql`）至少覆盖「创建 + 加密落库 DB 直读断言 + in-use 守卫」三条 —— 照 Stage 130 的 `claude_oauth_crud.feature` / Stage 125 的 `proxy_crud.feature` 形态。

### 4.3 前端 BDD

`websearch-providers.feature` × 3 viewports（照 `crates/aigw-frontend/tests/` 既有约定）：列表渲染 / 新建对话框 / 实例子表展开 / 定价编辑保存 / 探测结果展示 / 启停切换。

---

## 5. 变更清单

| 层 | 文件 | 改动 |
|----|------|------|
| migration | `crates/aigw-core/migrations/{sqlite,postgres,mysql}/028_web_search_providers.sql` | 新增两表 × 三方言 |
| model | `crates/aigw-core/src/models.rs` | `WebSearchProviderRow` / `WebSearchInstanceRow` |
| db | `crates/aigw-core/src/db.rs` | CRUD + in-use 查询（三方言） |
| 装载 | `crates/aigw-core/src/websearch/config.rs` | 新增「从 DB 行构造 `WebSearchConfig`」路径（yaml 路径不动） |
| 探测 | `crates/aigw-core/src/websearch/probe.rs`（新） | 单实例真实搜索 + `parse_response` 校验 + 快照 |
| 路由 | `crates/aigw-server/src/routes/websearch.rs`（新） | 上表全部端点 |
| 注册 | `crates/aigw-server/src/main.rs` | 路由注册（照 `:636-647`） |
| 前端 | `crates/aigw-frontend/src/pages/websearch/{index,ProviderDialog,InstanceDialog,ProbeDialog,types}.tsx` | 管理页（照 `pages/proxies/` 四文件结构） |
| 前端 | `crates/aigw-frontend/src/i18n/locales/{en,zh-CN}.json` | `webSearch.*` 键 |
| 导航 | 侧边栏 SETTINGS 分组 | 新增入口（照 Proxies 位置） |

---

## 6. 回归验证

1. `task test`（含 `aigw-migrate` 三方言）
2. `task bdd` — 新增 `websearch_admin.feature` 场景
3. `task bdd-real-sqlite` / `-pg` / `-mysql` — 加密落库直读 + in-use 守卫
4. `task fe-build` / `task fe-lint` / 前端 BDD × 3 viewports
5. `task fmt` / `task lint`
6. **人工**：对真实 SearXNG 实例（`http://30.184.60.216:9099`）走完「建 provider → 加实例 → 探测 → 填摊销单价 → 发搜索请求 → 核对 SpendLog 金额」全链路

## 7. 门禁

- [ ] 028 migration 三方言通过
- [ ] CRUD + 实例子资源 + 探测 + 启停端点全部可用
- [ ] `api_key` 加密落库 + 响应 redact + in-use 守卫 409
- [ ] 配置源优先级三种情形各有 UT
- [ ] 探测能识别「HTTP 200 + HTML」为失败
- [ ] 改价不重启生效
- [ ] 前端管理页 + i18n（en/zh-CN）+ 前端 BDD × 3 viewports
- [ ] `task test` / `bdd` / `bdd-real-*` / `fmt` / `lint` / `fe-build` / `fe-lint` 全绿
- [ ] git commit（精确 add；`--signoff`）

## 8. 风险与遗留

### 8.1 风险

| 风险 | 缓解 |
|------|------|
| **双配置源导致「改了不生效」的排查地狱** | §3.2 定为**单一来源 + 优先级**（DB 优先，不做并集）+ 启动日志明示当前来源 + 三条优先级 UT |
| **`ON DELETE CASCADE` 三方言行为不一致**（SQLite 需 `PRAGMA foreign_keys=ON`） | 实现前先核实仓内连接是否开启；未开启则改应用层级联 + UT 锁定。**不可假定 SQLite 默认开启** |
| **`probe_result` 的 JSONB/JSON 方言坑** | Stage 125 已踩过并修过（roadmap v58.0）→ 直接照 `proxies` 的三方言写法 |
| **探测只看状态码会漏掉 SearXNG 的静默降级** | §3.4 强制探测跑完整 `parse_response`；`test_probe_detects_html_body_as_parse_failure` 锁定 |
| **单价误填导致账目失真** | 两类：① 把 ¥36/1k 当 USD 填成 `0.036`（放大 7 倍）；② **沿用 SearXNG 的缺省 `0.01` 不改**（自建高估约 50 倍，**这是最可能发生的一类**）。缓解：UI 文案按 provider 给换算指引与典型量级（§3.5）；博查项标注「汇率决策未落地前不得对外出账」；**`kind=searxng` 且单价 > `0.001` 时提示「这看起来是牌价而非自建摊销成本，确认？」**；加输入范围软校验（> $0.1/次 时提示确认） |
| **`api_key` 加密链路在 SearXNG 下仍无真实消费者** | 本 Stage 的 UI 允许录入 `kind=tavily|bocha` 配置（校验报「客户端未实现」）→ 加密/redact 链路可在不实现客户端的前提下被 CRUD 层端到端验证 |
| **UI 允许录入未实现的 kind 会让用户困惑** | `test_unimplemented_kind_tavily_rejected_with_hint` 要求错误文案明确区分「不支持的 kind」与「已规划但未实现的 kind」 |

### 8.2 遗留

| 条目 | 说明 |
|------|------|
| **Tavily / 博查客户端实现** | 抽象（trait / `kind` / 配置 / 定价 / 实例）在 Stage 134 与本 Stage 均已就位 → 接入 = 加一个 provider 文件 + 一个 `kind` 分支，**零架构改动**。Tavily 的 `chunks_per_source`（厂商侧 ≤500 字符压缩）是其独有优势；博查胜在 Bing 兼容形状与中文质量 |
| **博查 CNY→USD 汇率决策** | 三候选见 Stage 136 §8。**本 Stage 只做 UI 提示，不做换算机制**；决策落地前博查金额不得对外出账 |
| **key/team 级单价覆写（加价/折扣）** | Stage 136 §3.3 已论证：单价是采购成本（事实），覆写是售卖策略（产品概念），超出 Phase 53 范围。若引入，照 sub2api `web_search_price_per_call DECIMAL(20,8)`（`~/works/play/sub2api/backend/migrations/174_group_web_search_price_per_call.sql`）的列形态补 |
| **配额加权 LB + Redis 配额预留** | sub2api `manager.go` 的 `selectByQuotaWeight` + Lua `quotaIncrScript`；多搜索 key 场景出现后再补，沿用「Redis 不可用时放行而非拒服务」 |
| **按实例的花费下钻图表** | Stage 137 只做抽屉可见 `api_base`；若要「按实例聚合花费」图表需新查询 |
| **设计 A（短路）/ 设计 B（agentic loop）** | 后续 Phase，见 Stage 135 §8.2。B 的硬前提（上游是否认网关**注入**的 function tools）仍未实测 |
