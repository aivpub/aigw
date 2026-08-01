# Stage 33: Playground Chat 调试页

**Phase**: 12 — 前端导航重构 + Playground
**状态**: ✅ 完成
**预估**: 2.5h

---

## 目标

新增 `/dash/playground` Chat 调试页面，用于测试模型效果。

## 验收标准

- [x] `/dash/playground` 页面加载成功
- [x] 模型选择 dropdown（从 `/v1/models` 拉取）
- [x] System Prompt 输入区（可选 textarea）
- [x] User Message 输入区（textarea，必填）
- [x] Temperature slider (0-2) + Max Tokens input
- [x] Send 按钮调用 `/v1/chat/completions`
- [x] Response 展示区（Markdown 渲染）
- [x] Streaming toggle + 流式输出支持
- [x] 移动端堆叠布局
- [x] Loading / Empty / Error 三态覆盖
- [x] BDD: page load, send message, streaming response, model select, empty state

## API

```
POST /v1/chat/completions
{
  "model": "gpt-4",
  "messages": [
    {"role": "system", "content": "..."},
    {"role": "user", "content": "..."}
  ],
  "temperature": 0.7,
  "max_tokens": 1024,
  "stream": true|false
}
```

鉴权：使用 admin session (JWT Cookie，已由 RequireAuth 保护)。

## 页面布局

```
桌面端（两栏）:                      移动端（堆叠）:
┌──────────────────┬────────┐       ┌──────────────────┐
│ System Prompt    │ Model  │       │ Model Select     │
│ (textarea)       │ Select │       ├──────────────────┤
│                  │        │       │ System Prompt    │
│ User Message     │ Temp   │       ├──────────────────┤
│ (textarea)       │ MaxTok │       │ User Message     │
│                  │        │       ├──────────────────┤
│ [Send] [Stream]  │        │       │ Temp / MaxTok    │
├──────────────────┴────────┤       ├──────────────────┤
│ Response Area             │       │ [Send] [Stream]  │
│ (Markdown)                │       ├──────────────────┤
│                           │       │ Response         │
└───────────────────────────┘       └──────────────────┘
```

## 组件状态

| 状态 | 展示 |
|------|------|
| Loading | Spinner + "Sending..." |
| Empty | "Enter a message and click Send to test" |
| Error | "Request failed: {message}" + Retry |
| Success | Markdown rendered response |

## 依赖

- Stage 31（路由 + 侧边栏）
- 后端 `/v1/chat/completions`（已就绪）
- 后端 `/v1/models`（已就绪）

## 输出

- [ ] `src/pages/playground/index.tsx`
- [ ] 若需要，`src/components/playground/` 子组件
- [ ] BDD feature + steps
