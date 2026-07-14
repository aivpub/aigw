# Roadmap 更新方案 — 架构重构 Stage 替换

**日期**: 2026-07-14
**状态**: 规划中

---

## 1. 背景

当前 Phase 17 与 Stage 50/51 存在不一致：

| 文档 | Phase 17 内容 |
|------|--------------|
| `stage-roadmap.md` | Provider 适配架构 (Stages 50-53) — P3 长期 |
| `11-next-steps.md` | Usage 多视角聚合 (Stages 50-51) — P1 |
| `stage-50.md` | Provider 饼图颜色区分 + key_name 解密 |
| `stage-51.md` | Usage 多视角聚合 (Global/Team/Org/Key 切换) |
| `docs/plans/2026-07-13-arch-refactor-plan.md` | 代理转发架构重构方案（完整设计） |

另外 `11-next-steps.md` 显示 Phase 14-16 均已 100% 完成，但 `stage-roadmap.md` 仍显示 0%。

---

## 2. 变更方案

### 2.1 移除 Stage 50/51（Usage 多视角聚合）

**原因**: Usage 多视角聚合是功能增强项（P2），不是当前阻塞项。在架构层面存在明确技术债（代码重复、adapter 写死）的背景下，优先做架构重构更合理。

### 2.2 新 Phase 17：代理转发架构重构（P1）

基于 `docs/plans/2026-07-13-arch-refactor-plan.md` 的完整设计，将 3 个实施阶段映射为 Stage 50-52：

| Stage | 目标 | 预估 | 类型 |
|-------|------|------|------|
| **Stage 50** | Deployment 抽象 + RouteDispatcher（不破坏现有代码） | 3h | 后端 |
| **Stage 51** | Adapter 重构（trait 拆分 + AdapterRegistry + OpenAIPassthrough + ClaudeToOpenAI） | 3h | 后端 |
| **Stage 52** | Handler 瘦身（chat.rs / v1_messages.rs 通用逻辑下沉） | 2h | 后端 |

**依赖关系**: Stage 50 → 51 → 52（串行）。

### 2.3 更新 stage-roadmap.md

- 修正 Phase 14/15/16 状态为 100%
- 替换 Phase 17 内容为架构重构
- 更新进度条
- 更新修订记录

### 2.4 更新 11-next-steps.md

- 修正 Phase 14/15/16 状态为 ✅ 完成
- 替换 Phase 17 内容
- 更新优先级排序

### 2.5 新增 ADR

- ADR-013: 架构重构优先级决策（为什么优先架构而非功能增强）

---

## 3. 文件变更清单

| 操作 | 文件 | 说明 |
|------|------|------|
| **删除** | `docs/stages/stage-50.md` | 旧 Usage 聚合 Stage |
| **删除** | `docs/stages/stage-51.md` | 旧 Usage 聚合 Stage |
| **修改** | `docs/stages/stage-roadmap.md` | 更新 Phase 14-17 状态 + 新 Stage 50-52 |
| **修改** | `docs/11-next-steps.md` | 更新当前状态 + 新 Phase 17 |
| **新增** | `docs/08-autonomous-decisions.md` | ADR-015: 架构重构优先级决策 |

### 不在本次范围

- 不创建 `stage-50.md` / `stage-51.md` / `stage-52.md` 详细 Stage 文档（等实施阶段再写）
- 不改动 `docs/01-charter.md`（Phase 17 在章程里已有占位）

---

## 4. 实施步骤

1. 删除 `docs/stages/stage-50.md`、`docs/stages/stage-51.md`
2. 修改 `docs/stages/stage-roadmap.md` — 更新 Phase 14-16 的状态 + 替换 Phase 17
3. 修改 `docs/11-next-steps.md` — 同步更新
4. 在 `docs/08-autonomous-decisions.md` 中新增 ADR-015
5. `git add` 精确文件并 commit
