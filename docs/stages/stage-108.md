# Stage 108: Frontend Display + Real API BDD + Documentation

**Phase**: 43 — Image Token Usage Tracking
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 8h
**前置**: Stage 107（Handler 集成 — image_tokens 字段已在 API 返回中）
**后置**: 无（Phase 收尾）

---

## 核心预期

1. **SpendLog 详情抽屉**展示 image_tokens（区分上游值 vs 估算值）
2. **SpendLog 列表**多模态请求标记 🖼️
3. **i18n** 中英双语 keys（3 个）
4. **Real API BDD** 用 Qwen 验证上游解析正确
5. **Documentation** 收尾：ADR、Stage 文档、Roadmap 更新

---

## 背景

Stage 107 落地后，SpendLog API 返回的 JSON 包含 `image_tokens` 字段（可能为 null/数字），`metadata.image_tokens_source` 标记来源（"upstream"/"estimated"/absent）。前端需要展示这些信息。

**前端不变更计费逻辑**：image_tokens 只展示不换算价格（已在 prompt_tokens 中计费）。

---

## 设计

### 1. 类型定义

```typescript
// crates/aigw-frontend/src/types/spend.ts
interface SpendLog {
  // ... existing fields ...
  image_tokens?: number | null;
}
```

### 2. SpendLog 详情抽屉（detail.tsx）

在 Usage 区域（token 统计卡片旁）增加 `Image Tokens` 行：

```
┌─────────────────────────────────────────┐
│ Usage                                    │
│   Prompt Tokens    1,500                │
│   Completion        200                 │
│   Total            1,700                │
│   ─────────────────────────             │
│   Image Tokens      400  ℹ️              │
│   Source: upstream provider    [Badge]  │
└─────────────────────────────────────────┘
```

- 仅 `image_tokens != null` 时显示此区域
- Tooltip hover ℹ️：
  - source=upstream → "Reported by upstream provider"（上游报告值）
  - source=estimated → "Estimated from image dimensions — provider did not report this breakdown"（基于图片尺寸估算，服务商未返回此分解）
- Source Badge 颜色：
  - `upstream` → green `✓` badge
  - `estimated` → amber `⚠` badge

### 3. SpendLog 列表（index.tsx）

多模态请求行显示 🖼️ emoji（`image_tokens != null` 时）。

### 4. i18n

| Key | English | 中文 |
|-----|---------|------|
| `spend.imageTokens` | Image Tokens | 图片 Token |
| `spend.imageTokensSourceUpstream` | Reported by upstream provider | 上游服务商报告值 |
| `spend.imageTokensSourceEstimated` | Estimated from image dimensions | 基于图片尺寸估算 |

---

## Real API BDD

验证 Qwen 实际上游返回值解析：

```gherkin
@real_api @needs_upstream_qwen
Feature: Image Token Tracking — Real API

  Scenario: Qwen2.5-VL returns image_tokens via OpenAI-compat endpoint
    Given 通过真实 Qwen2.5-VL API 发送含 1 张 base64 图片的 chat 请求
    When 获取 response usage
    Then prompt_tokens_details.image_tokens > 0
    And image_tokens < prompt_tokens

  Scenario: Qwen text-only request has no image_tokens
    Given 通过真实 Qwen API 发送纯文本 chat 请求
    When 获取 response usage
    Then prompt_tokens_details.image_tokens 不存在或为 0
```

可选（需要 Gemini API key）：Gemini `promptTokensDetails[]` 解析验证。

---

## Documentation

| 文件 | 操作 | 说明 |
|------|------|------|
| `docs/08-autonomous-decisions.md` | 修改 | ADR-026 登记（Image Token Estimation Architecture） |
| `docs/12-technical-debt.md` | 修改 | TD-010 登记（视频不支持、HEIC/AVIF 不支持、Anthropic 近似公式） |
| `docs/stages/stage-roadmap.md` | 修改 | Phase 43 完成回写 |
| `docs/11-next-steps.md` | 修改 | 进度同步 |

### ADR-026 摘要

**Title**: Image Token Usage Tracking — Upstream-First with Client-Side Fallback

**Status**: Proposed

**Context**: 多模态模型 image token 用量跟踪。Qwen/DashScope 返回 `image_tokens`，OpenAI/Anthropic 不返回。现有网关（litellm/OpenRouter/OneAPI）均不处理缺失。

**Decision**: 上游优先 + 客户端估算 fallback。Qwen/Gemini 直接解析上游值 → `image_tokens_source: "upstream"`；OpenAI/Anthropic 从 request body 解析 base64 图片宽高 → 按 provider 公式估算 → `image_tokens_source: "estimated"`。image_tokens 不改 calc_spend（是 prompt_tokens 的子集）。Deployment 不承载估算策略（auto-sniff model name 足够）。

**Consequences**: 
- ✅ Qwen users get accurate upstream data
- ✅ OpenAI/Anthropic users get useful estimates
- ⚠️ Anthropic formula not officially published — approximate
- ⚠️ Video, HEIC/AVIF not supported yet

### TD-010 条目

| Item | 描述 | 优先级 | 触发条件 |
|------|------|--------|---------|
| TD-010a | 视频 token 估算不支持（temporal_patch_size + mRoPE） | P2 | 视频多模态请求占比 > 5% |
| TD-010b | HEIC/AVIF 格式不支持 | P3 | Apple 生态用户反馈 |
| TD-010c | Anthropic 估算精度优化（官方公式未公开，当前用 OpenAI 近似） | P3 | Anthropic 公开 image token 公式或用户反馈偏差 > 20% |

---

## 文件变更总览

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-frontend/src/types/spend.ts` | 修改 | `image_tokens?: number \| null` |
| `crates/aigw-frontend/src/pages/spend-logs/detail.tsx` | 修改 | 抽屉展示 image_tokens + source badge |
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | 修改 | 多模态标记 🖼️ |
| `crates/aigw-frontend/src/i18n/locales/en.json` | 修改 | +3 keys |
| `crates/aigw-frontend/src/i18n/locales/zh-CN.json` | 修改 | +3 keys |
| `crates/aigw-frontend/tests/features/spend-logs.feature` | 修改 | +1 E2E scene |
| `docs/08-autonomous-decisions.md` | 修改 | ADR-026 |
| `docs/12-technical-debt.md` | 修改 | TD-010a/b/c |
| `docs/stages/stage-roadmap.md` | 修改 | Phase 43 回写 |
| `docs/11-next-steps.md` | 修改 | 进度同步 |

---

## TDD

前端 E2E：
- Playwright BDD 1 场景 × 3 viewports：SpendLog 详情抽屉显示 image_tokens 行（mock API 返回 `image_tokens: 400, metadata.image_tokens_source: "upstream"`）

后端 BDD（real API）：
- 2 场景（Qwen 图片 + Qwen 纯文本）— `@real_api @needs_upstream_qwen`

---

## Gate 门禁

- `task fe-build` 构建通过
- `task fe-bdd` Playwright BDD 新场景绿 + 全量回归
- `task bdd-real-sqlite` 后端 real BDD 绿（验证 migration + 集成）
- 文档全部更新落盘
