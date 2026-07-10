# Stage 48: Playground 按钮组：Clear Session + Get Code 弹窗

**Phase**: 16 — Playground 增强
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 2h
**依赖**: Stage 47（Endpoint Type 影响代码示例）

---

## 目标

"New Chat" 按钮右侧新增按钮组：
1. **Clear Session** — 清理会话历史
2. **Get Code** — 弹窗展示多种代码示例（curl / OpenAI SDK / Enio）

## 验收标准

- [ ] "New Chat" 右侧新增 "Clear Session" 按钮
  - 点击后清空所有消息 + 重置 settings（保留 Virtual Key/Endpoint Type）
  - 与"New Chat"的区别：New Chat = 全部重置，Clear Session = 保留配置仅清消息
- [ ] "Get Code" 按钮弹出 Dialog/Sheet
  - Tab 切换：curl / OpenAI SDK (Python) / Enio Framework
  - curl 示例包含完整的请求参数（根据当前 settings 动态生成）
  - SDK 代码示例使用当前 settings 的参数
  - Enio 框架代码展示 Enio 框架的调用方式
  - 每个 Tab 有"Copy Code"按钮
- [ ] 弹窗适配移动端
- [ ] BDD：Clear Session 行为验证、Get Code 弹窗内容验证

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-frontend/src/pages/playground/index.tsx` | 修改：Header 按钮组 + GetCodeDialog 组件 |

## 技术方案

### Header 按钮组布局

```
[🔄 New Chat]  [🧹 Clear Session]  [📋 Get Code]        [⚙ Settings]
```

- `New Chat`：调用 `clearChat()`（已有，完全重置）
- `Clear Session`：新增 `clearSession()`，清空 `messages` 但保留 settings
- `Get Code`：打开 `GetCodeDialog`

### Clear Session 函数

```typescript
const clearSession = useCallback(() => {
  setMessages([]);
  setInput("");
}, []);
```

### Get Code Dialog 组件

```tsx
function GetCodeDialog({ open, onClose, settings, messages }) {
  const [tab, setTab] = useState<"curl" | "openai" | "enio">("curl");

  const endpointType = settings.endpointType ?? "chat";
  const endpoint = endpointType === "chat" 
    ? "/v1/chat/completions" 
    : "/v1/messages";

  const buildCurl = () => {
    const body = endpointType === "chat"
      ? buildOpenAIBody(messages, settings)
      : buildClaudeBody(messages, settings);
    
    let curl = `curl -X POST http://localhost:3000${endpoint} \\\n`;
    curl += `  -H "Content-Type: application/json" \\\n`;
    if (endpointType === "messages") {
      curl += `  -H "anthropic-version: 2023-06-01" \\\n`;
    }
    curl += `  -H "x-api-key: sk-xxx" \\\n`;
    curl += `  -d '${JSON.stringify(body, null, 2)}'`;
    return curl;
  };

  const buildOpenAISDK = () => {
    const body = buildOpenAIBody(messages, settings);
    return `from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:3000/v1",
    api_key="sk-xxx",
)

response = client.chat.completions.create(
    model="${settings.model}",
    messages=${JSON.stringify(body.messages, null, 8)},
    temperature=${settings.temperature},
    max_tokens=${settings.maxTokens},
    stream=${settings.streaming},
)
print(response.choices[0].message.content)`;
  };

  const buildEnioCode = () => {
    const body = endpointType === "chat"
      ? buildOpenAIBody(messages, settings)
      : buildClaudeBody(messages, settings);
    
    return `import { EnioAI } from "enio";

const enio = new EnioAI({
    baseURL: "http://localhost:3000/v1",
    apiKey: "sk-xxx",
});

const response = await enio.chat.completions.create({
    model: "${settings.model}",
    messages: ${JSON.stringify(body.messages, null, 8)},
    temperature: ${settings.temperature},
    maxTokens: ${settings.maxTokens},
});

console.log(response.content);`;
  };

  const codeMap = { curl: buildCurl(), openai: buildOpenAISDK(), enio: buildEnioCode() };

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>Get Code</DialogTitle>
        </DialogHeader>
        <Tabs value={tab} onValueChange={setTab}>
          <TabsList>
            <TabsTrigger value="curl">curl</TabsTrigger>
            <TabsTrigger value="openai">OpenAI SDK</TabsTrigger>
            <TabsTrigger value="enio">Enio Framework</TabsTrigger>
          </TabsList>
        </Tabs>
        <div className="relative">
          <pre className="bg-muted p-4 rounded-md text-xs overflow-auto max-h-96">
            <code>{codeMap[tab]}</code>
          </pre>
          <Button
            variant="outline" size="sm"
            className="absolute top-2 right-2"
            onClick={() => { navigator.clipboard.writeText(codeMap[tab]); toast("Copied!"); }}
          >
            <Copy className="h-3.5 w-3.5" /> Copy
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
```

### 代码示例中的动态内容

- settings 中的 `temperature`、`maxTokens`、`stream`、`topP` 等实时反映
- messages 为当前会话历史（便于用户直接复制完整的多轮对话）
- API key 占位符 `sk-xxx`
- URL 使用相对路径（用户自行替换为实际部署地址）

## 依赖

- Stage 47：Endpoint Type 影响 Get Code 中生成的代码（Chat 格式 vs Messages 格式）

## 风险

- Get Code 弹窗中 messages 内容可能很大（多轮对话）→ 限制显示的消息数量或截断长内容
- Enio 框架的 API 调用方式可能与 OpenAI SDK 不同 → 需要确认 Enio 的实际 SDK 接口
