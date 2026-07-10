# litellm 成本计费链路与 aigw 对应对照

> 基于 litellm `master` 源码原样追踪（2026-07），不含解读、不含猜测。

---

## 1. 架构

```
启动时:
  proxy_models.litellm_params ──注册→ litellm.model_cost (内存字典)
                                      ├── [model_info.id]   → 含自定义定价
                                      └── [provider/model]  → 前缀移除定价字段
请求时:
  /chat/completions → router.acompletion() → litellm_logging._response_cost_calculator()
    → use_custom_pricing_for_model() → custom_pricing=True
    → _select_model_name_for_cost_calc() → model_info.id
    → litellm.model_cost[model_info.id]["input_cost_per_token"]
    → cost_per_token() → response_cost
    → proxy_track_cost_callback → spend_logs.spend
```

**关键事实**: litellm **不在请求时**查询 `LitellM_ProxyModelTable`。定价在启动时注册到全局内存字典，请求时直接从字典查。

---

## 2. 完整调用栈

### 2.1 启动时注册

**文件**: `litellm/router.py`, `_create_deployment()` (line 7272)

```
for field in CustomPricingLiteLLMParams:   # 49个定价字段（7290-7298）
    deployment.litellm_params[field] → _model_info[field]

部署注册两次（7308-7358）:
  litellm.model_cost[model_info.id]          = _model_info       ← 含定价
  litellm.model_cost[provider/model]          = _model_info.strip ← 定价已移除
```

### 2.2 请求时路由注入

**文件**: `litellm/router.py`, `_update_kwargs_with_deployment()` (line 2897)

```
kwargs["metadata"]["model_info"] = deployment["model_info"]
kwargs["model_info"] = deployment["model_info"]
```

这个 `model_info` 目的是传递 `model_info.id`——用于成本计算时从 `model_cost` 中查找。

### 2.3 成本计算

**文件**: `litellm/litellm_core_utils/litellm_logging.py`, `_response_cost_calculator()` (line 1352)

```
custom_pricing = use_custom_pricing_for_model(litellm_params)   # (1410-12)
router_model_id = get_router_model_id()  → metadata.model_info.id  # (513-27)
litellm.response_cost_calculator(model=model, custom_pricing=custom_pricing, router_model_id=router_model_id)
```

**文件**: `litellm/cost_calculator.py`, `_select_model_name_for_cost_calc()` (line 735)

```
if custom_pricing is True:
    if router_model_id is not None and router_model_id in litellm.model_cost:
        return router_model_id   # 使用 model_info.id 作为 model_cost 查找键
```

**文件**: `litellm/cost_calculator.py`, `cost_per_token()` (line 296)

```
model_cost_ref = litellm.model_cost          # (line 422)
model_info = _cached_get_model_info_helper(model=selected_model) # (line 654)
# 普通路径:
prompt_cost = prompt_tokens * model_info["input_cost_per_token"]    # (line 219)
completion_cost = completion_tokens * model_info["output_cost_per_token"]
```

**最后一个 `input_cost_per_token` 读取位置**: `litellm/litellm_core_utils/llm_cost_calc/utils.py`, `_get_cost_per_unit()` (line 378):

```python
return model_info.get(cost_key)   # 就是 litellm.model_cost[model_info.id].get("input_cost_per_token")
```

### 2.4 写入 spend_logs

**文件**: `litellm/proxy/db/db_spend_update_writer.py`, `update_database()` (line 121)

```
payload["spend"] = response_cost or 0.0   # (line 166)
INSERT INTO spend_logs (spend, ...) VALUES (payload["spend"], ...)
```

---

## 3. 为什么定价在两个数据库列中

`LiteLLM_ProxyModelTable.litellm_params` 和 `.model_info` 都包含 `input_cost_per_token`。

**起源 commit**: `86632f6da0`（2024-07-04），标题 `fix(types/router.py): add custom pricing
 info to 'model_info'`。

GitHub issue #4542。用户在 `litellm_params` 中设置了自定义价格，但
cost calculator 仅从 `model_info` 读取 → 定价未生效。

修复：`Deployment.__init__` (`types/router.py:451-454`) 在构造时将定价
从 `litellm_params` 镜像到 `model_info`：

```python
for key in SPECIAL_MODEL_INFO_PARAMS:
    field = getattr(litellm_params, key, None)
    if field is not None:
        setattr(model_info, key, field)
```

镜像的字段（`SPECIAL_MODEL_INFO_PARAMS`，line 422-429）：
- `input_cost_per_token`
- `output_cost_per_token`
- `input_cost_per_character`
- `output_cost_per_character`
- `cache_read_input_token_cost`（2026-05 添加）
- `cache_creation_input_token_cost`（2026-05 添加）

Admin UI 的 PATCH 处理器（`model_management_endpoints.py:126-134`）
实现显式 null 清空，并通过 `SPECIAL_MODEL_INFO_PARAMS` 确保清除在两列之间传播。
提交 `f75a7c6b221`（2026-05-26）中的注释说明了设计理念：

```python
# Honor explicit-null clears LAST, after both merges, so a model_info blob
# the UI passes through (which today re-sends the OLD pricing on every save)
# cannot silently undo a litellm_params clear via .update().
#
# Restricted to SPECIAL_MODEL_INFO_PARAMS … so this path cannot be used
# to null out privileged model_info fields like team_id or access groups.
```

### `model_info` 是权威来源

成本计算器从 `model_info` 读取。`litellm_params` 包含定价仅因为
用户习惯在此定义，而镜像确保两个位置保持一致。
请求时成本计算使用 `litellm.model_cost`（内存字典），
非数据库，不再次从数据库读取。

---

## 4. `cleanedLitellmParams`（前端专用）

仅在 UI 层存在。定义在 `modelDataTransformer.ts:49-53`：

```typescript
cleanedLitellmParams = Object.fromEntries(
    Object.entries(curr_model?.litellm_params).filter(
        ([key]) => key !== "model" && key !== "api_base"
    ),
);
```

`litellm_params` 去除 `model` 和 `api_base`——这两个字段已有独立的 UI 列。
仅用于前端显示；不存储在数据库中，也不参与后端成本计算。

---

## 5. aigw 的对应实现

| 关注点 | litellm 做法 | aigw 做法 |
|--------|------------|----------|
| 定价来源 | `litellm.model_cost`（全局内存字典，启动时从 `proxy_models` 注册） | `proxy_models.model_info`（主）→ `litellm_params` 解密后（回退） |
| 读取时机 | 启动时注册，请求时查找内存字典 | 请求时检测 `resolve_upstream_params()`，`params_json` 已解密 |
| 成本公式 | `prompt_tokens * input_cost + completion_tokens * output_cost` | 相同 |
| 写入目的地 | `spend_logs.spend` | 相同 |

aigw 不需要 `litellm.model_cost` 字典，因为每个请求只与一个模型交互。
提取路径（`chat.rs:39-52`）：

```
model_info.input_cost_per_token  （优先——litellm 标准）
    → params_json.input_cost_per_token  （回退——迁移时缺少 model_info 的部署）
    → 0.0  （无定价时的安全默认值）
```

---

## 6. 源码引用索引

| 关注点 | 文件 | 行号 | 用途 |
|---------|------|------|------|
| 成本计算入口 | `litellm_core_utils/litellm_logging.py` | 1352 | `_response_cost_calculator()` |
| 自定义定价标志 | `litellm_core_utils/litellm_logging.py` | 572-574 | `custom_pricing = True` |
| 模型 ID 提取 | `litellm_core_utils/litellm_logging.py` | 513-527 | `get_router_model_id()` |
| 定价检测 | `litellm_core_utils/litellm_logging.py` | 4417-4444 | `use_custom_pricing_for_model()` |
| 模型名选择 | `cost_calculator.py` | 735-795 | `_select_model_name_for_cost_calc()` |
| 单token成本 | `cost_calculator.py` | 296-695 | `cost_per_token()` |
| 完成成本 | `cost_calculator.py` | 1106-1677 | `completion_cost()` |
| 请求成本计算 | `cost_calculator.py` | 1705-1772 | `response_cost_calculator()` |
| 部署注册 | `router.py` | 7272-7358 | `_create_deployment()` |
| 部署注入 | `router.py` | 2897-2955 | `_update_kwargs_with_deployment()` |
| 成本写入 | `proxy/db/db_spend_update_writer.py` | 121-179 | `update_database()` |
| 回调调度 | `proxy/hooks/proxy_track_cost_callback.py` | 38-507 | `_PROXY_track_cost_callback()` |
| `model_info` 镜像 | `types/router.py` | 439-461 | `Deployment.__init__` |
| PATCH 处理器 | `management_endpoints/model_management_endpoints.py` | 99-165 | `update_db_model()` |
| `cleanedLitellmParams` | `ui/.../modelDataTransformer.ts` | 49-53 | 仅前端 |

---

## 7. 关键提交

| 提交 | 日期 | 说明 |
|--------|------|------|
| `86632f6da0` | 2024-07-04 | 首次双存储：将 `SPECIAL_MODEL_INFO_PARAMS` 从 `litellm_params` 镜像到 `model_info` |
| `0dbd663877` | 2025-04-09 | 将自定义定价处理扩展到部署层级路由 |
| `e4411e4815c` | 2025-02-08 | 引入 `update_db_model()` 合并更新 |
| `5655cb87fc` | 2026-03-02 | 将 `register_model` 中的 3 个硬编码字段扩展为完整 `CustomPricingLiteLLMParams` |
| `f75a7c6b221` | 2026-05-26 | 为通配符模型添加自定义定价清除支持；添加缓存成本字段 |
