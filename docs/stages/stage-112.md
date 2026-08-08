# Stage 112: Embeddings 模型接入 + 文档收尾

**所属**: Phase 45（Embeddings API 代理支持）
**预估**: 6h（后端 + 测试 + 文档）
**依赖**: Stage 110 → 111 → 112

---

## 1. 目标

**模型注册增强** + 文档收尾。本 Stage 不新增端点（Stage 110 已落地四端点），核心是把"embedding 模型如何在 aigw 中注册和使用"闭环补齐，并同步全部项目文档。

## 2. 核心设计

### 2.1 模型注册能力（litellm 兼容）

embedding 模型复用**现有** `proxy_models` 表 + `/model/new` 端点（models.rs L148 已接受 model_info 含 `mode` / `input_cost_per_token`）。本 Stage 验证并补测试，确保：

1. **注册**：`/model/new` 支持 `model_info.mode="embed"` + `input_cost_per_token`（litellm 约定）。无 schema 变更。
2. **列表**：`/v1/models` 已列出任意 proxy_models 行（model_info 透传）→ embedding 模型天然可见。
3. **本地 + 托管**：
   - 本地 vLLM：`litellm_params.api_base=http://vllm:8000/v1`，`custom_llm_provider=openai`，`model` 指向 vLLM 的 pooling 模型名 → `/v1/embeddings` 透传
   - 托管 OpenAI：`api_base=https://api.openai.com/v1`，`model=text-embedding-3-small`

### 2.2 补充 BDD：模型注册 + /v1/models 展示

```gherkin
Scenario: 注册 embedding 模型（mode=embed）
  When 创建 model "text-embedding-3-small" 带 model_info.mode="embed"
  Then 模型创建成功
  And GET /v1/models 返回中包含该模型
  And 该模型 model_info.mode 为 "embed"

Scenario: embedding 模型走 /v1/embeddings
  Given 已注册 model "text-embedding-3-small"（mode=embed）
  When 使用 master key 发送 POST /v1/embeddings 请求
  Then 响应状态码为 200
  And SpendLog call_type 为 "embedding"
```

### 2.3 health.rs 探测增强（**用户确认非阻塞 → 记技术债**）

现有 `run_and_save_health_check`（health.rs L266）对所有 OpenAICompatible 模型 POST `{model, messages:[...], max_tokens:1}` 到 `/chat/completions`。embedding-only 模型会 400。**本 Stage 不做**，登记 TD-011（Phase 46 候选）。

## 3. 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `docs/01-charter.md` | 修改 | L91 上游协议加 `+ OpenAI Embeddings API (Passthrough)`；L204 Stage-4 scope 加 `/v1/embeddings` |
| `docs/stages/stage-roadmap.md` | 修改 | Phase 45 登记 + v45.0 |
| `docs/11-next-steps.md` | 修改 | Phase 44/45 todo + 测试目标表更新 |
| `docs/08-autonomous-decisions.md` | 修改 | ADR-026 |
| `docs/12-technical-debt.md` | 修改 | TD-011（health embedding 探测 + 多模态 embedding 计费） |
| `crates/aigw-server/tests/features/models.feature` | 修改 | +2 场景（mode=embed 注册 + /v1/models 展示） |
| `crates/aigw-server/tests/bdd_steps/model_steps.rs` | 修改 | step 绑定 |

## 4. TDD — BDD（+2 mock 场景）

见 2.2。回归：全量 mock BDD 保持绿。

## 5. 验收标准

- [ ] `task bdd` 全量 mock BDD 全绿（含新增 +2 场景）
- [ ] 文档四处（charter/roadmap/next-steps/ADR/tech-debt）全部同步
- [ ] embedding 模型注册→/v1/models→/v1/embeddings 全链路验证通过
