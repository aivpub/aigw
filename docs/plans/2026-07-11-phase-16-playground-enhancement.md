# Phase 16: Playground 增强

> 基于用户使用反馈，Playground 页面的 3 个增强需求。

**日期**: 2026-07-11
**Phase**: 16
**Stage 编号**: 47-49

---

## 需求概览

| # | 需求 | Stage | 预估 |
|---|------|-------|------|
| 1 | Virtual Key 配置 + Endpoint Type 选择 | Stage 47 | 3h |
| 2 | 按钮组：Clear Session + Get Code（curl/OpenAI SDK/Enio） | Stage 48 | 2h |
| 3 | Markdown 渲染 + 气泡边框 + 底部统计栏（原 Stage 41 移入） | Stage 49 | 5h |

**总预估**: 10h（1.5 个工作日）

## 依赖图

```
Phase 14 (/v1/messages 修复) — 先决条件，确保 /v1/messages 端点可用
  └── Phase 16 (Playground 增强)
        ├── Stage 47 (Virtual Key + Endpoint Type)
        │     └── Stage 48 (Get Code / Clear Session) — 依赖 Endpoint Type 影响代码示例
        └── Stage 49 (Markdown + 气泡 + 统计) — 独立，可与 47/48 并行
```

## 与 Phase 15 的关系

Phase 15 和 Phase 16 可在 Phase 14 完成后并行进行。Stage 49（Markdown 气泡）是纯前端改动，不依赖任何后端 stage。

## 详细设计

见各 Stage 文档：
- `docs/stages/stage-47.md` — Virtual Key + Endpoint Type
- `docs/stages/stage-48.md` — Clear Session + Get Code
- `docs/stages/stage-49.md` — Markdown + 气泡边框 + 底部统计
