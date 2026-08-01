# Stage 47: Playground Virtual Key 配置 + Endpoint Type 选择

**Phase**: 16 — Playground 增强
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 3h
**依赖**: Phase 14 `/v1/messages` 修复完成

---

## 目标

Playground 页面 Settings 面板新增两项配置：
1. **Virtual Key**：支持选择"Current UI Session"（默认，通过 cookie）或输入自定义 Virtual Key（SK 密钥）
2. **Endpoint Type**：选择 `/v1/chat/completions`（OpenAI Chat）或 `/v1/messages`（Claude Messages）

## 验收标准

- [ ] Settings 面板新增 "Virtual Key" 配置区域
  - 下拉选择：`Current UI Session`（默认）或 `Custom Virtual Key`
  - 选择 `Custom Virtual Key` 时展开 SK 密钥输入框（password 类型）
  - 选择 `Current UI Session` 时不显示输入框
- [ ] Settings 面板新增 "Endpoint Type" 选择区域
  - 下拉/Radio 选择：`Chat Completions (/v1/chat/completions)` 或 `Claude Messages (/v1/messages)`
  - 默认选中 Chat Completions（保持向后兼容）
- [ ] 当 Virtual Key = Custom 时，fetch 请求带上 `Authorization: Bearer <sk>` header
- [ ] 当 Endpoint Type = Claude Messages 时，请求体格式从 OpenAI Chat 切换为 Claude Messages
- [ ] 当切换到 Claude Messages 时，settings 字段适配（移除不兼容字段如 `frequency_penalty`/`presence_penalty`，改用 Claude 参数如 `max_tokens` 必填）
- [ ] BDD：Virtual Key 选择 + Endpoint Type 切换 + 请求验证

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-frontend/src/pages/playground/index.tsx` | 修改：Settings 面板 + fetch 逻辑 + 请求体构建 |

## 技术方案

### Settings 数据结构扩展

```typescript
interface SettingsData {
  model: string;
  temperature: number;
  maxTokens: number;
  streaming: boolean;
  topP: number;
  freqPenalty: number;
  presPenalty: number;
  // 新增
  virtualKey: "session" | "custom";
  customApiKey: string;        // 仅 virtualKey === "custom" 时使用
  endpointType: "chat" | "messages";  // "chat" = /v1/chat/completions, "messages" = /v1/messages
}
```

### Virtual Key 配置 UI

```tsx
<div className="space-y-2">
  <Label>Virtual Key</Label>
  <Select value={settings.virtualKey} onValueChange={...}>
    <option value="session">Current UI Session (Cookie)</option>
    <option value="custom">Custom Virtual Key (SK)</option>
  </Select>
  {settings.virtualKey === "custom" && (
    <Input
      type="password"
      placeholder="sk-..."
      value={settings.customApiKey}
      onChange={(e) => setSettings({...settings, customApiKey: e.target.value})}
    />
  )}
</div>
```

### Endpoint Type 选择 UI

```tsx
<div className="space-y-2">
  <Label>Endpoint Type</Label>
  <Select value={settings.endpointType} onValueChange={...}>
    <option value="chat">Chat Completions (/v1/chat/completions)</option>
    <option value="messages">Claude Messages (/v1/messages)</option>
  </Select>
  {settings.endpointType === "messages" && (
    <p className="text-xs text-muted-foreground">
      Uses Anthropic Messages API format. Requires anthropic-version header.
    </p>
  )}
</div>
```

### Fetch 逻辑适配

```typescript
const url = settings.endpointType === "chat" 
  ? "/v1/chat/completions" 
  : "/v1/messages";

const headers: Record<string, string> = {
  "Content-Type": "application/json",
};
if (settings.endpointType === "messages") {
  headers["anthropic-version"] = "2023-06-01";
}
if (settings.virtualKey === "custom" && settings.customApiKey) {
  headers["x-api-key"] = settings.customApiKey;
}

const body = settings.endpointType === "chat" 
  ? buildOpenAIBody(messages, settings)
  : buildClaudeBody(messages, settings);
```

### 请求体构建

**Chat 模式**（保持现有逻辑不变）：
```typescript
function buildOpenAIBody(messages, settings) {
  return {
    model: settings.model,
    messages: messages.map(m => ({ role: m.role, content: m.content })),
    stream: settings.streaming,
    temperature: settings.temperature,
    max_tokens: settings.maxTokens,
    top_p: settings.topP,
    frequency_penalty: settings.freqPenalty,
    presence_penalty: settings.presPenalty,
  };
}
```

**Messages 模式**：
```typescript
function buildClaudeBody(messages, settings) {
  const systemMsg = messages.find(m => m.role === "system");
  const conversationMsgs = messages.filter(m => m.role !== "system");
  return {
    model: settings.model,
    messages: conversationMsgs.map(m => ({ role: m.role, content: m.content })),
    max_tokens: settings.maxTokens,
    stream: settings.streaming,
    ...(systemMsg ? { system: systemMsg.content } : {}),
    ...(settings.temperature > 0 ? { temperature: settings.temperature } : {}),
    ...(settings.topP > 0 ? { top_p: settings.topP } : {}),
  };
}
```

### response 解析适配

Messages 模式返回的响应格式是 `{ content: [{ type: "text", text: "..." }], usage: { input_tokens, output_tokens } }`，需要适配：

```typescript
function extractContentFromResponse(data, endpointType) {
  if (endpointType === "chat") {
    return data?.choices?.[0]?.message?.content ?? "";
  }
  // Claude format
  return data?.content?.filter(c => c.type === "text").map(c => c.text).join("") ?? "";
}

function extractTokenUsage(data, endpointType) {
  if (endpointType === "chat") {
    return {
      prompt: data?.usage?.prompt_tokens ?? 0,
      completion: data?.usage?.completion_tokens ?? 0,
    };
  }
  return {
    prompt: data?.usage?.input_tokens ?? 0,
    completion: data?.usage?.output_tokens ?? 0,
  };
}
```

## 风险

- Messages 模式下的 SSE 解析需要适配 Anthropic SSE 格式（与 OpenAI SSE 不同：有 `event:` 字段）
- Settings 持久化：当前 settings 仅存内存中，刷新丢失 → 需确认是否用 localStorage
- Claude Messages API 的参数比 Chat Completions 少（无 `frequency_penalty`, `presence_penalty`），切换时需隐藏不适用的控件
