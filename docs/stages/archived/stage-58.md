# Stage 58: Gateway Overhead 评估与展示（对齐 litellm proxy_server_request）

**Phase**: 20 — Spend Logs 可观测性（过滤器增强 + Overhead 评估 + 修复）
**状态**: ⏳ 待开始
**预估**: 7-8h
**依赖**: 无硬依赖（可与 Stage 55/56/57 并行）

---

## 目标

1. **proxy_server_request 写入**（对齐 litellm） — 请求入口时记录 url/method/headers/arrival_time
2. **Queue Time 计算** — `processing_start_time - arrival_time`
3. **Upstream Timing 记录** — 上游发送时间、首字节时间、完成时间，用于 overhead 拆分
4. **Timing Breakdown 前端展示** — 可视化 queue + gateway processing + upstream roundtrip 占比

## 根因摘要

- `proxy_server_request` 列 schema 已定义（BLOB），但代码始终写入 `None`
- `request_duration_ms` 为总耗时，无法拆分「网关自身开销」和「上游响应耗时」
- `ttft_ms` 已有（streaming），但无 `queue_time`、无上游发起时间戳
- litellm 的 `proxy_server_request` 在 `litellm_pre_call_utils.py:1350` 中定义为请求入口快照，用于：
  1. Queue time 计算（`common_request_processing.py:1068-1075`）
  2. UI Drawer body 来源（`usage_endpoints.py:498-502`）
  3. Guardrails header 消费

## 验收标准

- [ ] 请求入口时 `proxy_server_request` JSON 写入 SpendLog：url、method、headers（截断版）、arrival_time
- [ ] `queue_time_ms` = processing_start_time - arrival_time，在响应中计算并返回
- [ ] adapter 发送上游时记录 `upstream_sent_at`，首字节时记录 `upstream_first_byte_at`，完成时记录 `upstream_ended_at`
- [ ] `upstream_timing` JSON 写入 SpendLog（或在 proxy_server_request 扩展字段中）
- [ ] API 响应包含 `gateway_overhead_ms`（request_duration_ms - upstream_duration_ms）
- [ ] 前端 DetailDrawer 新增 "Timing Breakdown" 区域：Queue / Gateway Processing / Upstream Roundtrip / Total
- [ ] 水平条形图可视化各部分占比
- [ ] 旧日志（无 proxy_server_request）→ "Timing breakdown not available for this request"
- [ ] **门禁**: 全量 UT + BDD + 前端 Playwright（4 个 scenario × 3 viewports）

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/chat.rs` | **修改** — 请求入口写 proxy_server_request；传递 upstream_timing |
| `crates/aigw-server/src/routes/v1_messages.rs` | **修改** — 同上 |
| `crates/aigw-core/src/adapter.rs` | **修改** — upstream 请求前后记录时间戳；返回 UpstreamTiming |
| `crates/aigw-core/src/models.rs` | **修改** — SpendLog 增加 upstream_timing 字段（或复用 proxy_server_request）|
| `crates/aigw-core/src/db.rs` | **修改** — update_spend_log 增加 proxy_server_request 参数 |
| `crates/aigw-server/src/routes/spend.rs` | **修改** — 响应返回 overhead 计算字段 |
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | **修改** — DetailDrawer 新增 TimingBreakdown 区域 |

## 技术方案

### 1. proxy_server_request 写入（请求入口，对齐 litellm）

在 handler 接收请求后、开始 resolve 之前记录：

```rust
use std::time::{SystemTime, UNIX_EPOCH};

let arrival_time = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs_f64();

let proxy_server_request = json!({
    "url": "/v1/chat/completions",  // 或 /v1/messages
    "method": "POST",
    "headers": {
        "user-agent": user_agent.clone().unwrap_or_default(),
        "x-forwarded-for": requester_ip.clone().unwrap_or_default(),
    },
    "arrival_time": arrival_time,
});
```

### 2. Queue Time 计算

```rust
// 在 handler 中（已有 let start_time = Utc::now()）
let processing_start_ts = start_time.timestamp_millis() as f64 / 1000.0;
let queue_time_ms = ((processing_start_ts - arrival_time) * 1000.0).max(0.0);
```

### 3. Upstream Timing 记录（adapter 层）

```rust
/// 在 adapter 发送请求前后捕获时间
pub struct UpstreamTiming {
    pub sent_at: chrono::DateTime<chrono::Utc>,
    pub first_byte_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
    pub status: u16,
}

// adapter 返回值扩展：
pub struct UpstreamResponse {
    pub status: u16,
    pub body: ...,
    pub timing: UpstreamTiming,
}
```

UpstreamTiming 写入 SpendLog（可单独字段或放 proxy_server_request 扩展）：

```json
{
  "upstream_sent_at": "2026-07-15T10:30:00.123Z",
  "upstream_first_byte_at": "2026-07-15T10:30:01.456Z",
  "upstream_ended_at": "2026-07-15T10:30:03.789Z",
  "upstream_status": 200
}
```

### 4. Overhead 计算

```rust
fn calculate_gateway_overhead(
    total_ms: f64,           // request_duration_ms
    upstream_duration_ms: f64, // upstream_ended_at - upstream_sent_at
    queue_time_ms: f64,
) -> GatewayOverhead {
    let gateway_overhead_ms = (total_ms - upstream_duration_ms - queue_time_ms).max(0.0);
    GatewayOverhead {
        queue_time_ms,
        gateway_overhead_ms,
        upstream_duration_ms,
        total_ms,
        overhead_percent: if total_ms > 0.0 {
            (gateway_overhead_ms / total_ms * 100.0 * 100.0).round() / 100.0
        } else {
            0.0
        },
    }
}
```

API 响应中返回：
```json
{
  "queue_time_ms": 2.3,
  "gateway_overhead_ms": 15.7,
  "upstream_duration_ms": 3210.5,
  "total_duration_ms": 3228.5,
  "overhead_percent": 0.49
}
```

### 5. DB 更新

```rust
// update_spend_log 扩展
pub async fn update_spend_log(
    &self,
    request_id: &str,
    spend: f64, total_tokens: i32, prompt_tokens: i32, completion_tokens: i32,
    end_time: DateTime<Utc>, request_duration_ms: i32,
    completion_start_time: DateTime<Utc>,
    response: Value, status: &str,
    proxy_server_request: Option<Value>,   // NEW
) -> Result<()>;
```

### 6. 前端 Timing Breakdown

```tsx
// DetailDrawer 中新增区域
function TimingBreakdown({ log }: { log: SpendLog }) {
  if (!log.proxy_server_request) {
    return (
      <div className="text-xs text-muted-foreground italic">
        Timing breakdown not available for this request
      </div>
    );
  }
  const { queue_time_ms, gateway_overhead_ms, upstream_duration_ms,
          total_duration_ms, overhead_percent } = log;
  const total = total_duration_ms || log.request_duration_ms || 1;

  return (
    <div className="space-y-2">
      <Label className="text-xs text-muted-foreground">Timing Breakdown</Label>
      <div className="space-y-1.5">
        <TimingBar label="Queue" ms={queue_time_ms} total={total}
                   color="bg-yellow-400" />
        <TimingBar label="Gateway" ms={gateway_overhead_ms} total={total}
                   color="bg-purple-400" />
        <TimingBar label="Upstream" ms={upstream_duration_ms} total={total}
                   color="bg-blue-400" />
      </div>
      <div className="text-xs text-muted-foreground">
        Gateway overhead: {overhead_percent}% ({gateway_overhead_ms}ms)
      </div>
    </div>
  );
}

function TimingBar({ label, ms, total, color }: {
  label: string; ms: number; total: number; color: string;
}) {
  const pct = total > 0 ? (ms / total * 100) : 0;
  return (
    <div className="flex items-center gap-2 text-xs">
      <span className="w-16 text-muted-foreground">{label}</span>
      <div className="flex-1 h-3 bg-muted rounded-full overflow-hidden">
        <div className={`h-full ${color} rounded-full`}
             style={{ width: `${Math.min(pct, 100)}%` }} />
      </div>
      <span className="w-14 text-right font-mono">{ms.toFixed(0)}ms</span>
    </div>
  );
}
```

## TDD 测试用例

### UT (Rust)

```rust
#[test]
fn test_proxy_server_request_written_on_entry() {
    // handler 接收请求后
    // assert: spend_log.proxy_server_request.is_some()
    // assert: proxy_server_request["arrival_time"] > 0
}

#[test]
fn test_queue_time_calculation() {
    // arrival_time = T, processing_start = T + 0.05s
    // assert: queue_time_ms ≈ 50.0
}

#[test]
fn test_gateway_overhead_calculation() {
    // total_ms = 1000, upstream_ms = 950, queue_ms = 5
    // assert: gateway_overhead_ms = 45
    // assert: overhead_percent = 4.5
}

#[test]
fn test_old_log_without_proxy_server_request_handled() {
    // spend_log.proxy_server_request = None
    // assert: gateway_overhead_ms = null
    // assert: frontend shows "not available"
}

#[test]
fn test_upstream_timing_recorded() {
    // adapter 发送后
    // assert: upstream_timing.sent_at <= upstream_timing.first_byte_at
    // assert: upstream_timing.first_byte_at <= upstream_timing.ended_at
}
```

### BDD (Gherkin)

```gherkin
Scenario: New request has proxy_server_request with arrival_time
  Given 发送 /v1/chat/completions 请求
  When 请求成功完成
  Then SpendLog 中 proxy_server_request 不为空
  And proxy_server_request.arrival_time 存在且大于 0

Scenario: Spend log detail shows timing breakdown bar
  Given 一条有 proxy_server_request 数据的 spend log
  When 在 spend logs 页面点击该日志
  Then 详情显示 "Timing Breakdown" 区域
  And 包含 Queue / Gateway / Upstream 三个分段的水平 bar
  And 显示 gateway overhead 百分比

Scenario: Old spend log shows timing unavailable
  Given 一条没有 proxy_server_request 数据的旧记录
  When 在 spend logs 页面点击该日志
  Then 显示 "Timing breakdown not available for this request"

Scenario: Upstream duration is correctly recorded
  Given 发送一个耗时约 3 秒的 /v1/chat/completions 请求
  When 请求完成
  Then upstream_duration_ms > 0
  And gateway_overhead_ms < total_duration_ms
```

## 风险与回滚

| 风险 | 应对 |
|------|------|
| `SystemTime::now()` 在不同平台精度不一致 | 使用 `as_secs_f64()` 统一为浮点秒 |
| adapter 多款实现在不同位置发请求 | 在 `send_request` trait 方法中统一记录 |
| 旧日志无 overhead 数据 | 前端优雅降级（"not available"），不崩溃 |
| proxy_server_request.headers 敏感信息泄露 | 仅存储 user-agent、x-forwarded-for，不存储 authorization |
| overhead 计算可能与实际不符（异步调度延迟） | 标注为 "approximate"，添加注释说明误差来源 |

回滚方式：`git revert` 该 commit。
