# Stage 115 Review Log

**Stage**: Stage 115（多模态精度 — TD-011b HEIC/AVIF 转码 + TD-011c Anthropic downsizing + TD-012b 按模态计费）
**Review Type**: Design + Code
**Review Date**: 2026-08-09
**Reviewer**: lead（RDD Loop 自审）

## Review Summary

设计审查看出 3 个实现风险（1 Medium + 2 Low）全部在实现阶段处置；代码实现后自查无 Critical/High。TD-011a（视频输入）按设计「可选」标记跳过并记录。

## Design Review (Gate 2)

| Sev | Finding | 处置 |
|-----|---------|------|
| Medium | TD-012b 的 per-modal 计费需 per-modal token 计数，但 embeddings input 是 text/string（无 modal 标记）；真实多模态 embedding（gemini-embedding-2）走不同 API | 交付 ModalPricing 数据模型 + resolver 提取 + calc_spend_modal 纯函数 + 4 UT；embeddings.rs 接线留待真实负载（与 TD 描述「等真实负载再评估」一致） |
| Low | estimate_anthropic 单次缩放后 ⌈x/28⌉ 向上取整会 overshoot 1568 cap | 迭代缩放直到 tiled 估算 ≤ target（loop），保比例 |
| Low | HEIC 转码路径浏览器相关（仅 Safari 解码）——Chromium E2E 无法测成功路径 | E2E 测 Chromium reject 路径（toast + 0 preview）；成功路径由 compressImage 逻辑覆盖 |

## Code Review (Gate 4)

| Sev | Finding | 处置 |
|-----|---------|------|
| Low | compressImage 解码失败返回 originalDataUrl → caller 无法区分「无法渲染」| 改为返回 `dataUrl: null`，caller 据此弹 HEIC 不支持 toast |
| Low | scalar input_cost 单位（per-token）与 modal_pricing（per-1M）混用 | calc_spend_modal 仅对 modal_pricing 值 /1e6，scalar 原样使用（UT 校准） |
| Info | calc_spend_modal 当前无调用点 → dead-code | `#[allow(dead_code)]` + 文档注明 wiring 留待真实负载 |

## Verification
- 后端：aigw-core 415（image_tokens 23 含 4 新 anthropic-downsizing + resolver 12 含 2 新 modal）+ aigw-server 140（calc_spend_modal 4 新）全绿；fmt + lint green。
- 前端：playground.feature 57/57（含新 HEIC 场景 × 3 viewports）；fe-build 分包 + tsc 通过。

## Scope Decisions
- **TD-011a（视频输入）SKIPPED**：设计标记「可选（工作量高）」，无真实视频流量；记录为剩余项（TD-011a 原本即注明「留待真实负载」）。

## Resolution Summary
Design: 3（1 Medium + 2 Low）全部解决；Code: 3（2 Low + 1 Info）全部解决。
**All Critical/High Fixed**: Yes
**TD-011a 剩余**: 已记录（见 stage-115.md Known Limitations + tech-debt）。
