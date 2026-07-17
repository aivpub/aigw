# Stage 66: 模型健康检查 + Usage 页面优化 + Spend Logs 过滤

**Phase**: 25 — 健康检查 & 用户体验优化
**状态**: ⏳ 待开始
**预估**: 7h
**依赖**: 无

---

## 目标

1. **模型健康检查体系** — `health_checks` 表 + 即时 ping + 历史读取，纯手动触发（对齐 litellm）
2. **Usage 页面重构** — 布局紧凑化、过滤器位置优化、图表 Y 轴 Tab 切换（费用/token）
3. **Spend Logs 过滤增强** — 新增 status、token 用量范围过滤

---

## Part A — DB Schema: `health_checks` 表 (0.5h)

migration `017_health_checks.sql`（3 数据库），对齐 `LiteLLM_HealthCheckTable`：

```sql
CREATE TABLE IF NOT EXISTS health_checks (
    health_check_id TEXT NOT NULL PRIMARY KEY,
    model_name      TEXT NOT NULL,
    model_id        TEXT,
    status          TEXT NOT NULL,           -- 'healthy' | 'unhealthy'
    healthy_count   INTEGER NOT NULL DEFAULT 0,
    unhealthy_count INTEGER NOT NULL DEFAULT 0,
    error_message   TEXT,
    response_time_ms REAL,
    details         TEXT NOT NULL DEFAULT '{}',  -- JSON
    checked_by      TEXT,
    checked_at      TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_health_checks_model_name ON health_checks(model_name);
CREATE INDEX IF NOT EXISTS idx_health_checks_checked_at ON health_checks(checked_at);
CREATE INDEX IF NOT EXISTS idx_health_checks_status ON health_checks(status);
```

> `proxy_models` **不需要加列**。健康状态通过 `health_checks` 表关联查询获得，前端调 `/health/latest` 即可。

---

## Part B — 后端: Health Check (1.5h)

### 2.1 `crates/aigw-core/src/health.rs`

```rust
pub struct HealthCheckResult {
    pub model_name: String,
    pub model_id: Option<String>,
    pub status: String,        // "healthy" | "unhealthy"
    pub response_time_ms: f64,
    pub error_message: Option<String>,
    pub checked_at: String,
}
```

`ping_upstream(base_url, api_key?)` — GET `{base_url}/v1/models`，200=healthy，其他=unhealthy。

### 2.2 DB 方法

```rust
// INSERT INTO health_checks
async fn insert_health_check(&self, result: &HealthCheckResult) -> Result<()>;

// latest per model_name (DISTINCT ON + ORDER BY checked_at DESC)
async fn get_latest_health_checks(&self) -> Result<Vec<HealthCheckResult>>;
```

### 2.3 端点 (`routes/health.rs`)

| Method | Path | 功能 |
|--------|------|------|
| `POST` | `/model/health-check?model_id=xxx` | 单个模型 ping，写 DB |
| `POST` | `/model/health-check/all` | 并发 ping 全部模型，全量写 DB |
| `GET` | `/health/latest` | 读每个 model 最新一条 check 结果 |

---

## Part C — 后端: Spend Logs 过滤增强 (0.7h)

`routes/spend.rs` 的 `spend_logs` 和 `global_spend_logs` handler 新增 query 参数：

| 参数 | 类型 | 功能 |
|------|------|------|
| `status` | `Option<String>` | 过滤 `success` / `failure:N` / `streaming` |
| `min_tokens` | `Option<i32>` | `total_tokens >= min_tokens` |
| `max_tokens` | `Option<i32>` | `total_tokens <= max_tokens` |

DB 方法 `query_spend_logs` 追加 WHERE 条件。

---

## Part D — 前端: Health Tab (1h)

```
┌─ Models ──────────────────────────────────────────────────┐
│ [Model Groups] [Credentials] [Health]                      │
├────────────────────────────────────────────────────────────┤
│  [🔄 Check All Models]     Last run: 14:32 (2 min ago)     │
│                                                            │
│  Model      │ Status   │ Latency  │ Error      │ Checked   │
│  gpt-4      │ 🟢 ok   │ 42ms     │            │ 14:32:05  │
│  deepseek   │ 🟢 ok   │ 128ms    │            │ 14:32:05  │
│  broken-mdl │ 🔴 fail │ 5001ms   │ 502 Bad GW │ 14:25:00  │
└────────────────────────────────────────────────────────────┘
```

- 页面加载 `GET /health/latest` → 表格
- Check All `POST /model/health-check/all` → 完成后 refetch

---

## Part E — 前端: Usage 页面优化 (2h)

### 5.1 布局紧凑化

6 个数值卡缩小：去掉多余 `p-6` → `p-4`，字号 `text-3xl` → `text-2xl`，图标缩小。

### 5.2 过滤器迁移

日期选择器和 model/key 过滤器从卡片内部移到页面顶部 toolbar：

```
[📅 Last 30 days ▾]  [Model: All ▾]  [Key: All ▾]
```

### 5.3 图表 Y 轴 Tab Switch

三张图表（Total Spend、Spend by Model、Spend by Key）顶部增加 Tabs：

```
[💰 Spend] [📊 Tokens]
```

- Spend Tab: Y 轴显示费用 (USD)，tooltip 显示 spend
- Tokens Tab: Y 轴显示 token 数量，tooltip 显示 prompt_tokens / completion_tokens / total_tokens
- 两张 Tab 共用相同 X 轴和时间粒度，仅 scale 切换

### 5.4 Tooltip 增强

当前 tooltip 只显示费用。增加：

```
14:00
  Spend:     $0.42
  Requests:  12
  Tokens:    8,200 (p: 5,100 / c: 3,100)
```

---

## Part F — 前端: Spend Logs 过滤增强 (0.8h)

`pages/spend-logs/index.tsx` 顶部 toolbar 新增：

```
[📅 30 min ▾]  [Status: All ▾]  [Tokens: — to —]
```

- Status Select: All / Success / Failure (4xx/5xx) / Streaming
- Token 范围: 两个 NumberInput (min/max)

---

## 测试

| 类型 | # | 场景 |
|------|---|------|
| UT | 1 | `ping_upstream` 200 → healthy |
| UT | 2 | `ping_upstream` 超时 → unhealthy |
| UT | 3 | `insert_health_check` + `get_latest_health_checks` |
| UT | 4 | Spend Logs status 过滤 |
| UT | 5 | Spend Logs token 范围过滤 |
| BDD | 1 | Health Tab 展示健康/不健康模型 |
| BDD | 2 | Usage 图表切换 Spend/Tokens Tab |
| 手动 | — | 新页面布局全 viewport 验收 |

---

## 门禁

- [ ] `cargo test` 全量通过（206 → 211 UT）
- [ ] BDD 回归通过（97 → 99 scenarios）
- [ ] `npm run build` 前端通过
- [ ] Usage 页面三种 viewport 布局验收
