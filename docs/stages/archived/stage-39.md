# Stage 39: Playground 聊天式对话升级

**Phase**: 13 — 前端反馈改进
**状态**: ✅ 完成
**预估**: 5h

---

## 目标

将 Playground 从单次对话改造成聊天式界面，支持多轮对话和上下文传递。

## 验收标准

- [ ] 聊天 UI 布局：消息列表（气泡） + 底部输入框
- [ ] 侧边栏设置面板（可折叠）
- [ ] 多轮对话：每次发送携带全部历史消息
- [ ] 消息气泡：角色图标 + Markdown 渲染 + 复制按钮
- [ ] Streaming 实时输出支持
- [ ] 编辑已发送消息（修改后重新生成）
- [ ] 删除历史消息
- [ ] New Chat 按钮清空对话
- [ ] System Prompt 支持（作为消息列表首条）
- [ ] 设置面板：Model, Temperature, Max Tokens, Top P, Frequency Penalty, Presence Penalty
- [ ] Token 用量展示（每条 assistant 消息下方）
- [ ] Stop Generation（abort streaming）
- [ ] 移动端适配（堆叠布局）
- [ ] BDD：chat layout, multi-turn, streaming, clear history, model select

## 关键文件

| 文件 | 操作 |
|------|------|
| `src/pages/playground/index.tsx` | 完全重写 |

## 布局设计

```
桌面端（两栏）:
┌──────────────────────────────┬───────────┐
│ Chat Area                    │ Settings  │
│ ┌──────────────────────────┐ │           │
│ │ [System] You are helpful  │ │ Model: ▽  │
│ ├──────────────────────────┤ │ Temp: ═══ │
│ │ [User] What is Rust?     │ │ MaxTok:   │
│ ├──────────────────────────┤ │ Top P:    │
│ │ [Asst] Rust is a systems  │ │ Freq Pen: │
│ │ programming language...   │ │ Pres Pen: │
│ │ [Tokens: 152] [📋 Copy]  │ │           │
│ └──────────────────────────┘ │ [-] Hide  │
│ ┌──────────────────────────┐ │           │
│ │ Type a message...    [⏎]│ │           │
│ └──────────────────────────┘ │           │
└──────────────────────────────┴───────────┘

移动端（堆叠）:
┌──────────────────┐
│ [Settings Bar ▲] │  ← 点击展开设置面板
├──────────────────┤
│ Chat Messages    │
│ (full width)     │
├──────────────────┤
│ [Input Bar]      │
└──────────────────┘
```

## 组件状态

| 状态 | 展示 |
|------|------|
| Empty | 空消息列表 + "Start a conversation" 引导提示 |
| Loading | assistant 气泡: spinner 动画 + "Generating..." |
| Streaming | assistant 气泡: 逐字更新 + "Stop" 按钮 |
| Error | assistant 气泡: 错误信息 + "Retry" 按钮 |
| Idle | assistant 消息: Markdown 渲染 + 复制按钮 + Token 统计 |

## 消息模型

```typescript
interface ChatMessage {
  id: string;                    // UUID v4
  role: "system" | "user" | "assistant";
  content: string;
  timestamp: number;
  tokens?: { prompt: number; completion: number };
  error?: string;
}
```

发送时：
```json
POST /v1/chat/completions
{
  "model": "gpt-4",
  "messages": [
    { "role": "system", "content": "..." },   // 若设置了 system prompt
    { "role": "user", "content": "msg 1" },
    { "role": "assistant", "content": "reply 1" },
    { "role": "user", "content": "msg 2" }     // 当前发送
  ],
  "temperature": 0.7,
  "max_tokens": 1024,
  "stream": true
}
```

## 依赖

- 无（纯前端改造，`/v1/chat/completions` 已在 Stage 3 就绪）

## 风险

- Streaming SSE 解析在 Chat UI 模式下更复杂（需要在气泡内增量更新 Markdown）
- 大段响应时 Markdown 渲染性能
- 移动端弹出设置面板的 UX 设计
