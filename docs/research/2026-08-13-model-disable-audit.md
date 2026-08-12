# 上游模型「停用」功能失效根因调研

**日期**: 2026-08-13
**报告人**: Stage 121 前置调研
**结论**: 前端 Switch 存字段生效，但后端零处消费；停用后模型仍被路由到上游。

---

## 一、现象

- 前端 `crates/aigw-frontend/src/pages/models/index.tsx:545-565` 有 Switch 开关，切「停用」时向 `PUT /model/update` 发 `model_info.mode = "inactive"`；切「启用」发 `mode: undefined`。
- UI 状态确实变化：`isActive(info)` 检查 `mode !== "inactive" && mode !== "disabled"`（`models/index.tsx:67-70`）。
- 但请求走到后端时，**被停用的模型依然照常路由到上游**。

## 二、根因（三处不接线，任意一处补上都不够）

### 2.1 DB 层：`list_models_by_name` SQL 不过滤

`crates/aigw-core/src/db.rs:4428-4430`（SQLite；PG/MySQL 内联版本同样）：

```sql
SELECT model_id, model_name, litellm_params, model_info, ...
FROM proxy_models WHERE model_name = ?
```

**只按 model_name 匹配**，不看 `model_info->>'mode'`，停用/启用的行同样返回。

### 2.2 Resolver 层：`ModelResolver::resolve` 不检查 mode

`crates/aigw-core/src/resolver.rs:40-58` 直接把 `list_models_by_name` 结果全部映射为 `Deployment`。`resolve_one`（120-393 行）**完全不读 `model_info.mode`**。

### 2.3 Router 层：`Deployment` 结构没有停用字段

`crates/aigw-core/src/deployment.rs:17-71` 只有 `cooldown_until`（运行时熔断），没有 `is_active`/`enabled`。
`crates/aigw-core/src/router.rs:347-417` `pick_deployment` 也**只按 cooldown 过滤**。

### 2.4 附：`/model/update` handler 只做 merge

`crates/aigw-server/src/routes/models.rs:294-298` 用 `merge_json` 把 `model_info.mode` 存回 DB，注释里承诺"保留 mode（active/inactive）"——**只是承诺存下来，从不消费**。

## 三、数据链断裂图

```
前端 Switch → PUT /model/update {model_info.mode: "inactive"} → DB proxy_models.model_info
                                                                    │
                                                                    ▼
                                                          (存下来就此躺平,后续无人读)
                                                                    │
                                    ┌───────────────────────────────┘
                                    ▼
用户请求 → ModelResolver.resolve()               ← 不看 mode
       → Router.pick_deployment()              ← 不看 mode,只看 cooldown
       → 停用的 deployment 被选中并调用上游 ❌
```

## 四、字段语义污染

`model_info.mode` 同时承载两个正交概念：

- **启停开关**：`"active" / "inactive" / "disabled"`
- **业务类别**：`"embed" / "image"`（`health.rs:457` 依赖此判 embedding probe；`chat.rs:2898` 是 mock 数据）

同一字段两套语义会给后续接线埋雷。**方案是引入独立 `enabled: bool` 字段**，与业务类别 `mode` 分离。

## 五、修复决策：方案 B（Stage 121 落地）

- Schema：`proxy_models.enabled BOOLEAN` + `deleted_models.enabled BOOLEAN`（Migration 026）
- 后端：3 端 SQL 全部加 enabled 列；`LIST_MODELS_BY_NAME` 增加 `AND enabled = TRUE`；Resolver 额外做一次 `.filter(|m| m.enabled)` 兜底
- API：`/model/update` 接受 `enabled` 参数；`ModelResponse` 透出 `enabled`
- 前端：Switch 改调 `enabled`；`isActive` 从 `model_info.mode` 迁到 `model.enabled`
- `model_info.mode` 保留原义（业务类别），历史 "inactive"/"disabled" 值惰性容忍

详见 `docs/stages/stage-121.md`。
