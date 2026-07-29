# 上游模型缓存命中检测与计费调研

> 调研日期：2026-07-28
> 调研范围：litellm 源码 (`~/works/projects/github.com/BerriAI/litellm/`) + aigw 生产 PG 数据库
> 核心问题：aigw 是否感知上游模型服务的 prompt caching？计费是否合理？

## 一、核心发现：litellm 有两套截然不同的"缓存"机制

这是最关键的前提——**`cache_hit` 字段并不代表上游缓存命中**：

| 类型 | cache_hit 值 | 含义 | 计费 | 如何触发 |
|------|-------------|------|------|---------|
| **litellm 自身 Redis 缓存** | `"True"` | 请求在 litellm 层被 cache 拦截，**根本没发到上游** | spend = 0 | `caching_handler.py:L195` 设置 |
| **上游 Provider 侧缓存** | `"False"`（不变） | 请求确实发到了上游，上游 Provider 返回了缓存 token 信息 | 按缓存定价计费 | 从 response `usage` 解析 |

**上游缓存命中的信息不通过 `cache_hit` 字段表达，而是存储在 `response->usage` 和 `metadata->additional_usage_values` 中。**

litellm 源码中 `cache_hit = True` 的设置位置（均为 litellm 内部 cache）：

| 文件 | 行号 | 触发条件 |
|------|------|---------|
| `caching/caching_handler.py` | 195, 320, 482, 1159 | Redis/本地缓存命中 |
| `litellm_core_utils/streaming_handler.py` | 1692, 1880 | `custom_llm_provider == "cached_response"` |
| `responses/streaming_iterator.py` | 810 | Responses API 缓存命中 |

以上**没有任何一处**是基于 provider 返回的 `cached_tokens` 来设置的。

## 二、上游 Provider 返回缓存数据的两种格式

### 1. Anthropic 格式（顶层 `usage` 字段）

```json
{
  "usage": {
    "input_tokens": 100,
    "output_tokens": 200,
    "cache_read_input_tokens": 500,
    "cache_creation_input_tokens": 50
  }
}
```

- **关键**：`input_tokens`（映射为 `prompt_tokens`）**不包含**缓存 token
- `cache_read_input_tokens` 和 `cache_creation_input_tokens` 在顶层，不在 `prompt_tokens_details` 中
- 适用 provider：Anthropic、Bedrock Anthropic Claude 3

### 2. OpenAI 兼容格式（`prompt_tokens_details` 子对象）

```json
{
  "usage": {
    "prompt_tokens": 600,
    "completion_tokens": 200,
    "prompt_tokens_details": {
      "cached_tokens": 500,
      "cache_write_tokens": 50,
      "cache_creation_tokens": 50
    }
  }
}
```

- **关键**：`prompt_tokens` **已包含** `cached_tokens`
- 不同 provider 命名不同：`cached_tokens`（OpenAI）、`cache_write_tokens`（kimi-k2）、`cache_creation_tokens`（通用别名）
- 适用 provider：OpenAI、DeepSeek、GLM、kimi-k2、大多数 OpenAI 兼容服务

### 解析代码入口 (`cost_calculator.py:367-398`)

```python
# 统一两套格式到同一个变量
_cache_read_tokens: float = 0
_cache_creation_tokens: float = 0
_is_anthropic_style = False

# Anthropic 风格: 从 usage 对象顶层读取
_anthropic_read = getattr(usage_object, "cache_read_input_tokens", None)
if _anthropic_read is not None:
    _cache_read_tokens = float(_anthropic_read)
    _is_anthropic_style = True

# OpenAI 风格: 从 prompt_tokens_details 读取
_pt_details = getattr(usage_object, "prompt_tokens_details", None)
if _pt_details is not None:
    _cache_read_tokens = float(getattr(_pt_details, "cached_tokens", 0) or 0)
    _cache_creation_tokens = float(
        getattr(_pt_details, "cache_write_tokens", 0) or
        getattr(_pt_details, "cache_creation_tokens", 0) or 0
    )
```

各 provider 在 SDK 层的解析入口：

| Provider | 解析文件 | 关键字段 |
|----------|---------|---------|
| Anthropic | `llms/anthropic/chat/transformation.py:2072-2168` | `cache_read_input_tokens`, `cache_creation_input_tokens`, `ephemeral_5m/1h_input_tokens` |
| Bedrock Converse | `llms/bedrock/chat/converse_transformation.py:1736-1765` | `cacheReadInputTokens`, `cacheWriteInputTokens` |
| Bedrock Claude 3 | `llms/bedrock/messages/invoke_transformations/anthropic_claude3_transformation.py:793-845` | SSE `message_start`/`message_stop` 事件提取 |
| OpenAI | SDK 原生 `Usage.prompt_tokens_details.cached_tokens` | 无需额外解析 |
| DeepSeek | `types/utils.py:1596-1600` | `prompt_cache_hit_tokens` → `cached_tokens` |

### Usage 对象内置的归一化 (`types/utils.py:1602-1647`)

```python
## ANTHROPIC MAPPING ##
params["cache_read_input_tokens"] → _prompt_tokens_details.cached_tokens, self._cache_read_input_tokens
params["cache_creation_input_tokens"] → _prompt_tokens_details.cache_creation_tokens, self._cache_creation_input_tokens

## DEEPSEEK MAPPING ##
params["prompt_cache_hit_tokens"] → prompt_tokens_details.cached_tokens, self._cache_read_input_tokens
```

## 三、三级差异化计费逻辑

### 核心计费函数 (`cost_calculator.py:180-223`, `_cost_per_token_custom_pricing_helper`)

```python
def _cost_per_token_custom_pricing_helper(
    prompt_tokens,           # 总 prompt tokens（已含缓存，为统一约定）
    cached_tokens,           # = cache_read_input_tokens（命中缓存的 token）
    cache_creation_tokens,   # = cache_creation_input_tokens（新写入缓存的 token）
    custom_cost_per_token,   # 模型定价
):
    # 提取三级不同价格
    input_cost_per_token             # 常规 prompt token 价格
    cache_read_input_token_cost      # 缓存读取价格（通常为常规 10%-50%）
    cache_creation_input_token_cost  # 缓存写入价格（通常比常规高 ~25%）

    # 计算纯非缓存 prompt tokens
    regular_prompt_tokens = max(prompt_tokens - cached_tokens - cache_creation_tokens, 0)

    # 三类 token 各自按不同价格计费
    spend = (
        regular_prompt_tokens * input_cost_per_token
        + cached_tokens * cache_read_input_token_cost
        + cache_creation_tokens * cache_creation_input_token_cost
    ) + completion_tokens * output_cost_per_token
```

### 实际价格示例（Anthropic Claude 3.5 Sonnet）

| Token 类型 | 价格 (每 1M tokens) | 相对常规价格 |
|-----------|---------------------|-------------|
| 常规 input | $3.00 | 基准 |
| Cache read | $0.30 | **10%** |
| Cache write | $3.75 | **125%** |

### Anthropic 特殊归一化 (`cost_calculator.py:400-404`)

Anthropic 返回的 `input_tokens` **不包含**缓存 token，需要先加回再传入 helper：

```python
_normalized_prompt_tokens = float(prompt_tokens)
if _is_anthropic_style:
    _normalized_prompt_tokens += _cache_read_tokens + _cache_creation_tokens
```

### Anthropic 专属计费 (`llms/anthropic/cost_calculation.py`)

Anthropic 还有 geo/speed 路由 multiplier 的额外处理——先算出纯缓存成本，再只对非缓存部分乘以 multiplier，缓存部分保持不变：

```python
def _compute_cache_only_cost(model_info, usage, service_tier):
    """提取纯缓存成本（cache read + cache write），不受 geo/speed multiplier 影响"""
    prompt_tokens_details = _parse_prompt_tokens_details(usage)
    cache_read_tokens = prompt_tokens_details["cache_hit_tokens"]
    cache_creation_tokens = prompt_tokens_details["cache_creation_tokens"]
    # ...
    return cache_read_tokens * cache_read_cost + cache_creation_tokens * cache_creation_cost

def cost_per_token(model, usage, service_tier):
    prompt_cost, completion_cost = generic_cost_per_token(model, usage, "anthropic", service_tier)
    # geo/speed multiplier 只应用于非缓存部分
    cache_cost = _compute_cache_only_cost(model_info, usage, service_tier)
    prompt_cost = (prompt_cost - cache_cost) * multiplier + cache_cost
    completion_cost *= multiplier
    return prompt_cost, completion_cost
```

### 定价数据的来源

缓存价格定义在 litellm 的 `model_cost` registry 中（每个模型的 JSON 配置），例如：

```json
{
  "claude-3-5-sonnet-20241022": {
    "input_cost_per_token": 3e-6,
    "output_cost_per_token": 1.5e-5,
    "cache_read_input_token_cost": 3e-7,
    "cache_creation_input_token_cost": 3.75e-6
  }
}
```

## 四、缓存数据写入 DB 的路径

### 4.1 spend_logs 表：仅 `cache_hit` 和 `cache_key` 列

litellm 的 `LiteLLM_SpendLogs` 模型 (`models/spend_logs.py`)：

```python
class LiteLLM_SpendLogs:
    cache_hit: Optional[str] = "False"   # 仅 litellm 内部缓存命中时为 "True"
    cache_key: Optional[str] = None       # 缓存键
    metadata: Optional[Json] = {}         # 包含 additional_usage_values
    # 注意：没有 cache_read_input_tokens / cache_creation_input_tokens 列！
```

**上游缓存 token 数只写入 `metadata.additional_usage_values` JSON**：

```json
{
  "additional_usage_values": {
    "prompt_tokens_details": {"cached_tokens": 500},
    "cache_read_input_tokens": 500,
    "cache_creation_input_tokens": 50
  }
}
```

写入代码 (`spend_tracking_utils.py:361-368`)：

```python
# 遍历 usage.items()，把非基础字段全部放入 additional_usage_values
for key, value in usage.items():
    if key not in ["completion_tokens", "prompt_tokens", "total_tokens"]:
        additional_usage_values[key] = value
```

### 4.2 daily_*_spend 日聚合表：有专用列

写入细节 (`db_spend_update_writer.py:72-94, 1853-1854`)：

```python
def _extract_cache_read_tokens(usage_obj: dict) -> int:
    """Anthropic: 顶层 cache_read_input_tokens
       OpenAI:   prompt_tokens_details.cached_tokens"""
    explicit = usage_obj.get("cache_read_input_tokens", 0) or 0
    if explicit: return int(explicit)
    details = usage_obj.get("prompt_tokens_details") or {}
    return int(details.get("cached_tokens", 0) or 0)

def _extract_cache_creation_tokens(usage_obj: dict) -> int:
    """Anthropic: 顶层 cache_creation_input_tokens
       OpenAI:   prompt_tokens_details.cache_write_tokens / cache_creation_tokens"""
    explicit = usage_obj.get("cache_creation_input_tokens", 0) or 0
    if explicit: return int(explicit)
    details = usage_obj.get("prompt_tokens_details") or {}
    return int(details.get("cache_write_tokens", 0) or details.get("cache_creation_tokens", 0) or 0)

# 写入 daily 事务
daily_transaction = BaseDailySpendTransaction(
    # ... 常规字段 ...
    cache_read_input_tokens=_extract_cache_read_tokens(usage_obj),
    cache_creation_input_tokens=_extract_cache_creation_tokens(usage_obj),
)
```

### 4.3 整体数据流

```
上游 Provider API response
    │
    ├── Anthropic: usage.cache_read_input_tokens, cache_creation_input_tokens
    └── OpenAI:   usage.prompt_tokens_details.cached_tokens, cache_write_tokens
          │
          ▼
    Usage.__init__() 归一化 (types/utils.py:1602-1647)
          │
          ├──► cost_per_token() → 三级差异化计费 → spend
          │
          └──► spend_tracking_utils.get_logging_payload()
               │
               ├──► spend_logs.cache_hit = "True"/"False"
               ├──► spend_logs.metadata.additional_usage_values (含缓存 token 明细)
               ├──► spend_logs.spend (已按缓存定价计算的最终费用)
               │
               └──► db_spend_update_writer
                    ├──► daily_*_spend.cache_read_input_tokens
                    └──► daily_*_spend.cache_creation_input_tokens
```

## 五、aigw DB 生产数据核实

### 5.1 `spend_logs` 表结构

当前 `spend_logs` 有 35 列，缓存相关仅 2 列：

| 列名 | 类型 | 说明 |
|------|------|------|
| `cache_hit` | TEXT | "True"/"False"/None |
| `cache_key` | TEXT | 缓存键 |

**缺失的关键字段**（litellm 也没有，仅在 `response` JSON 中）：

| 说明 | 数据位置 |
|------|---------|
| 上游缓存读入 token 数 | `response->usage->prompt_tokens_details->cached_tokens` |
| 上游缓存写入 token 数 | `response->usage->prompt_tokens_details->cache_write_tokens` |
| Anthropic 缓存读入 token | `response->usage->cache_read_input_tokens` |
| Anthropic 缓存写入 token | `response->usage->cache_creation_input_tokens` |

### 5.2 数据分布（共 850,751 条记录）

| cache_hit 值 | 记录数 | 总消费($) | 平均消费 | 来源 |
|---|---|---|---|---|
| `"True"` | 16,462 | 0.00 | 0.00 | litellm 导入（Redis 缓存命中） |
| `"False"` | 586,036 | 18,609.00 | 0.032 | litellm 导入（含上游缓存命中） |
| `None` | 190,497 | 2,697.62 | 0.014 | aigw 自身写入 |

### 5.3 上游缓存命中数据详情

在 `response->usage->prompt_tokens_details->cached_tokens` 中检测到大量上游缓存命中。主要分布：

| 模型 | 记录数 | 平均缓存 token/次 | 提供商 |
|------|--------|-------------------|--------|
| `deepseek-v4-flash-202605` | 78,947 | 53,577 | deepseek |
| `deepseek-v4-pro-202606` | 29,241 | 122,614 | deepseek |
| `tke/deepseek-v4-flash` | 16,862 | 37,013 | deepseek |
| `ep-iswqr9k0` | 16,188 | 113,184 | deepseek |
| `ep-s55rmple` | 15,269 | 120,866 | deepseek |
| `glm-5.2` | 2,316 | 185,431 | deepseek |
| `glm-5.1` | 249 | 57,260 | hosted_vllm |

Anthropic 格式的 `cache_read_input_tokens` 仅 2 条记录，可忽略。

### 5.4 计费核实

- `cache_hit='True'` 的记录 spend 全部为 0：**正确**（litellm 内部缓存命中不收费）
- `cache_hit='False'` 但有上游缓存命中的记录：spend 由 litellm 在导入时计算，已按缓存定价区分计费：**正确**
- `cache_hit=None` 的记录（aigw 自身写入）：aigw 不使用缓存定价，但目前这批记录的 prompt 缓存触发量极少：**影响较小**

## 六、aigw 与 litellm 的差距

| 维度 | litellm | aigw 当前 |
|------|---------|----------|
| **litellm 自身 Redis 缓存** | ✅ 支持 | ❌ 不支持（`cache_hit: None`） |
| **解析上游 `cached_tokens`** | ✅ `prompt_tokens_details.cached_tokens` | ❌ 存在 but 未使用 |
| **解析上游 `cache_write_tokens`** | ✅ `prompt_tokens_details.{cache_write\|cache_creation}_tokens` | ❌ |
| **解析 Anthropic `cache_read_input_tokens`** | ✅ 顶层字段 | ❌ |
| **解析 Anthropic `cache_creation_input_tokens`** | ✅ 顶层字段 | ❌ |
| **三级差异化计费** | ✅ regular / cache_read / cache_write | ❌ `calc_spend` 仅用单一 `input_cost_per_token` |
| **Anthropic prompt_tokens 归一化** | ✅ 补回 cache token | ❌ |
| **写入 `daily_*` 缓存列** | ✅ 有实际数据写入 | ❌ 列已建，无写入逻辑 |
| **模型定价含缓存价格** | ✅ `cache_read_input_token_cost` 等 | ❌ 无 |

aigw 代码确认：

- `calc_spend` (`chat.rs:69-73`) 仅做 `prompt_tokens * input_cost + completion_tokens * output_cost`，不区分缓存 token
- 所有 `SpendLog` 构造点均硬编码 `cache_hit: None`（`chat.rs:1026,1140,1213,1527,1594`；`v1_messages.rs:406,590,702,765,1054`）
- 无代码解析上游 response 的 `prompt_tokens_details` 或 `cache_read_input_tokens`
- `daily_*_spend` 表的 `cache_read_input_tokens` / `cache_creation_input_tokens` 列存在但始终为 0

## 七、需要补齐的具体内容（若要做）

### 7.1 上游 response 解析

解析 `response` JSONB 中的 `usage` 子对象，提取两类格式的缓存 token：

```
OpenAI 格式: usage.prompt_tokens_details.cached_tokens
             usage.prompt_tokens_details.cache_write_tokens
             usage.prompt_tokens_details.cache_creation_tokens
Anthropic 格式: usage.cache_read_input_tokens
               usage.cache_creation_input_tokens
```

### 7.2 模型定价结构扩展

在 model pricing 中增加两个可选字段：

```rust
cache_read_input_token_cost: Option<f64>,     // 缓存读取价格
cache_creation_input_token_cost: Option<f64>, // 缓存写入价格
```

从 litellm 的 `model_cost` registry 或 aigw 自身的 `proxy_models.litellm_params` 获取。

### 7.3 计费逻辑改造 (`calc_spend`)

```rust
fn calc_spend(
    prompt_tokens: i32,
    completion_tokens: i32,
    input_cost: Option<f64>,
    output_cost: Option<f64>,
    // 新增：
    cache_read_tokens: i32,
    cache_creation_tokens: i32,
    cache_read_cost: Option<f64>,
    cache_creation_cost: Option<f64>,
    is_anthropic_style: bool,  // Anthropic 需归一化
) -> f64 {
    // 归一化：Anthropic 的 prompt_tokens 不含缓存 token
    let effective_prompt = if is_anthropic_style {
        prompt_tokens + cache_read_tokens + cache_creation_tokens
    } else {
        prompt_tokens
    };

    let regular_prompt = 0.max(effective_prompt - cache_read_tokens - cache_creation_tokens) as f64;
    let input_cost = input_cost.unwrap_or(0.0);
    let read_cost = cache_read_cost.unwrap_or(input_cost);      // 默认同常规价
    let create_cost = cache_creation_cost.unwrap_or(input_cost); // 默认同常规价

    regular_prompt * input_cost
        + cache_read_tokens as f64 * read_cost
        + cache_creation_tokens as f64 * create_cost
        + completion_tokens as f64 * output_cost.unwrap_or(0.0)
}
```

### 7.4 写入 `daily_*_spend` 的缓存列

在 `DailySpendLog` 写入逻辑中增加两个字段的填充：

```rust
pub struct DailySpendLog {
    // ... 现有字段 ...
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
}
```

### 7.5 `SpendLog` 可选的扩展字段

如果希望在其他地方（如 UI、报表）直接查询缓存 token 数，可以在 `spend_logs` 表上增加列：

```sql
ALTER TABLE spend_logs ADD COLUMN cache_read_input_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE spend_logs ADD COLUMN cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0;
```

但这需要评估必要性——litellm 选择了只放在 `daily_*_spend` 和 `metadata` JSON 里，`spend_logs` 表本身不存这些字段。

### 7.6 实施优先级建议

| 优先级 | 项目 | 影响面 |
|--------|------|--------|
| P0 | 解析上游 response 的缓存 token 字段 | 所有 provider 调用 |
| P0 | `calc_spend` 增加缓存差异化计费 | spend 准确性 |
| P1 | `daily_*_spend` 写入缓存列 | 日聚合报表 |
| P2 | 模型定价增加缓存价格字段 | 需要 sync 上游定价 |
| P3 | 写入 `spend_logs.cache_hit = "False"`（发到上游但上游命中缓存） | 数据完整性 |

## 附录：关键文件索引

### litellm 源码

| 文件 | 内容 |
|------|------|
| `litellm/cost_calculator.py` | 核心计费逻辑：`cost_per_token()`、`response_cost_calculator()`、`_cost_per_token_custom_pricing_helper()` |
| `litellm/litellm_core_utils/llm_cost_calc/utils.py` | `_parse_prompt_tokens_details()`、`_get_token_base_cost()`、`get_billable_input_tokens()`、`calculate_cache_writing_cost()` |
| `litellm/litellm_core_utils/litellm_logging.py` | `success_handler()`、`_response_cost_calculator()`、`_success_handler_helper_fn()` |
| `litellm/llms/anthropic/cost_calculation.py` | Anthropic 专属计价：`cost_per_token()`、`_compute_cache_only_cost()` |
| `litellm/llms/anthropic/chat/transformation.py` | Anthropic response `usage` 解析：`cache_read_input_tokens`、`cache_creation_input_tokens`、`ephemeral_*` |
| `litellm/llms/bedrock/chat/converse_transformation.py` | Bedrock Converse `usage` 解析：`cacheReadInputTokens`、`cacheWriteInputTokens` |
| `litellm/models/spend_logs.py` | `LiteLLM_SpendLogs` 模型定义 |
| `litellm/proxy/db/db_spend_update_writer.py` | DB 写入：`_extract_cache_read_tokens()`、`_extract_cache_creation_tokens()`、`_common_add_spend_log_transaction_to_daily_transaction()` |
| `litellm/proxy/spend_tracking/spend_tracking_utils.py` | `get_logging_payload()`：组装 spend log payload |
| `litellm/types/utils.py` | `Usage.__init__()`：Anthropic/DeepSeek 缓存字段归一化 |
| `litellm/caching/caching_handler.py` | litellm 内部 Redis 缓存命中检测 |

### aigw 源码

| 文件 | 内容 |
|------|------|
| `crates/aigw-core/src/models.rs:115-158` | `SpendLog` struct |
| `crates/aigw-core/migrations/postgres/002_spend_logs.sql` | `spend_logs` CREATE TABLE |
| `crates/aigw-core/migrations/postgres/015_daily_spend.sql` | `daily_*_spend` CREATE TABLE（含 `cache_read_input_tokens`/`cache_creation_input_tokens`） |
| `crates/aigw-server/src/routes/chat.rs:69-73` | `calc_spend()` 函数 |
| `crates/aigw-server/src/routes/chat.rs:1026,1140,1213,1527,1594` | `SpendLog` 构造点（全部 `cache_hit: None`） |
| `crates/aigw-server/src/routes/v1_messages.rs:406,590,702,765,1054` | `SpendLog` 构造点（全部 `cache_hit: None`） |
| `crates/aigw-core/src/db.rs:1262-1924` | `INSERT_SPEND_LOG_*` SQL 语句 |
