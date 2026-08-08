# Stage 111: Embeddings Frontend + OpenAPI 展示 + real BDD

**所属**: Phase 45（Embeddings API 代理支持）
**预估**: 8h（前端 + 测试 + 文档）
**依赖**: Stage 110（端点就绪后前端按接口契约独立开发）

---

## 1. 目标

前端 SpendLog 详情正确渲染 embedding 响应（当前 `OutputCard.tsx` `parseOutput` 无 `data[]` 分支 → 空态），real API BDD 验证真实 embedding 端点解析正确，openapi.rs 补齐 spec。

## 2. 核心设计

### 2.1 前端：OutputCard `parseOutput` 加 `data[]` 分支

当前 `parseOutput`（OutputCard.tsx L31-167）分支于 `choices[]` / `content[]` / `output[]`，无 `data[]`。Embedding 响应 `{object:"list", data:[{embedding:[...], index, object}], usage:{prompt_tokens,total_tokens}}` 会落到 L164 `return empty` → 详情抽屉空态。

新增分支：
```
if (Array.isArray(obj.data)) → render 向量（embedding.length 维度）+ usage（prompt_tokens/total_tokens）
```

**关键决策**：
- **不渲染完整向量数组**：只显示维度 `[0.1, 0.2, … (1536 dims)]` + 截断预览（前 N 维），避免巨型 JSON 撑爆详情抽屉。
- **SpendLog 列表 badge / token pill 零改动**：`call_type` 原样渲染（`"embedding"` 直接显示），token pill `prompt↑ / completion↓` 中 completion=0 是真实值。

### 2.2 openapi.rs `embeddings_spec()`

镜像 `chat_completions_spec`（L892）：tags + security auth_ref + requestBody（model + input）+ 200/400/401/403/429/502 responses。注册到 `"/v1/embeddings"` path。`expected_endpoints` 18→19 加 `/v1/embeddings`。

### 2.3 real API BDD（可选，`@real_api @needs_upstream_embedding`）

验证真实 embedding 端点解析正确（不验证估算——薄 passthrough 无估算逻辑）：
- 用真实 vLLM/OpenAI-compatible embedding 端点发请求 → 响应 object=list
- SpendLog prompt_tokens>0, completion_tokens=0

## 3. 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-frontend/src/components/log-viewer/OutputCard.tsx` | 修改 | `parseOutput` 加 `data[]` 分支 |
| `crates/aigw-frontend/src/i18n/locales/en.json` | 修改 | +2 keys（embedding.dims / embedding.usage） |
| `crates/aigw-frontend/src/i18n/locales/zh-CN.json` | 修改 | +2 keys |
| `crates/aigw-server/src/openapi.rs` | 修改 | `embeddings_spec()` + expected_endpoints |
| `crates/aigw-server/tests/bdd_steps/embeddings_steps.rs` | 修改 | real BDD step 绑定 |
| `docs/stages/stage-roadmap.md` / `docs/11-next-steps.md` | 修改 | Phase 45 进度回写 |

## 4. TDD — 单元测试（3 UT）

| # | Test | 断言 |
|---|------|------|
| 1 | `test_parse_output_embeddings_data` | `{object:"list", data:[{embedding:[0.1,0.2], index:0}]}` → 渲染"维度" + 不崩溃 |
| 2 | `test_parse_output_embeddings_usage` | usage `{prompt_tokens,total_tokens}` 被提取展示 |
| 3 | `test_openapi_embeddings_spec` | `/v1/embeddings` 在 expected_endpoints 中，post 存在 |

## 5. TDD — 前端 E2E（2 场景 × 3 viewports = 6 执行）

```gherkin
Scenario: SpendLog 详情展示 embedding 响应向量
  Given 一条 call_type=embedding 的 spend log（response 含 data[].embedding）
  When 打开该记录的详情抽屉
  Then 显示 embedding 维度信息而非空态

Scenario: SpendLog 列表 call_type badge 显示 embedding
  Given 一条 call_type=embedding 的 spend log
  Then 列表 Type 列 Badge 显示 "embedding"
```

## 6. 验收标准

- [ ] `task fe-build` + `task fe-bdd` 前端 E2E 全绿
- [ ] `task test` UT 全绿（含 3 新增）
- [ ] `task bdd` mock BDD 全绿（无回归）
- [ ] openapi.json `/v1/embeddings` spec 渲染
- [ ] embedding 响应详情抽屉不再空态
