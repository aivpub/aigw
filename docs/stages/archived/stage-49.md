# Stage 49: Playground Markdown 渲染 + 消息气泡边框 + 底部统计栏

> **原 Stage 41，移入 Phase 16 Stage 49**。与 Playground 增强（Stage 47-48）一起交付。

**Phase**: 16 — Playground 增强
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 5h
**依赖**: 无（独立前端改动）

---

## 目标

1. 将响应内容从纯文本升级为 Markdown 渲染（代码块、粗体、列表等）
2. Streaming 模式下 Markdown 随 chunk 到达实时增量更新
3. 消息气泡增加边框效果（不同角色不同颜色）
4. 气泡底部显示统计信息（token 费用、消耗量）和操作按钮（复制、重新生成、删除）

## 验收标准

- [ ] 响应内容使用 Markdown 渲染（代码块语法高亮、粗体/斜体、列表、链接）
- [ ] Streaming 模式下 Markdown 内容随 chunk 逐段更新（throttle 100ms）
- [ ] 消息气泡带 `rounded-lg` 边框（system=紫色、user=蓝色、assistant=绿色）
- [ ] 每个 assistant 气泡底部显示 token 计数和费用
- [ ] 底部栏操作按钮：复制内容、重新生成、删除
- [ ] 移动端底部栏适配
- [ ] BDD：Markdown 渲染验证、streaming Markdown、气泡颜色、底部统计

## 关键技术决策

- Markdown 库：`react-markdown` + `remark-gfm` + `rehype-highlight`
- Streaming 渲染：简化为直接 re-render（react-markdown 纯函数，无副作用）
- 代码高亮：`rehype-highlight` + highlight.js CSS 主题

## 与 Stage 47 的交互

Stage 47 引入了 `Endpoint Type` 和 `Virtual Key` — 本 Stage 的气泡 UI 和 Markdown 渲染与 Endpoint Type 无关。响应内容无论来自 `/v1/chat/completions` 还是 `/v1/messages`，都经过统一的 `extractContentFromResponse()` 提取后渲染。

（其余内容与原 Stage 41 文档相同，见 `docs/plans/2026-07-10-phase-14-feedback-round-2.md` 中的详细设计）
