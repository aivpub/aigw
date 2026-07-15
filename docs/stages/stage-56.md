# Stage 56: Spend Logs Prompt/Response 结构化可视化

**Phase**: 19 — UI Enhancement（Models CRUD + Spend Logs 可视化）
**状态**: ⏳ 待开始
**预估**: 7-8h
**依赖**: 无（数据已就绪，仅前端改造）

---

## 目标

1. **MessageViewer** — 解析 `messages` 数组按 role 结构化展示
2. **ResponseViewer** — 解析 `response` 对象展示文本回复 + tool calls + usage
3. **DetailDrawer 改造** — 替换 raw JSON `<pre>` 为 Tab 切换（Prompt / Response / Raw）
4. **复制功能** — 每个 Tab 内容独立复制按钮 + 复制反馈动画

## 根因摘要

- `pages/spend-logs/index.tsx:392-403` 中 `messages` 和 `response` 以 `<pre>` + `JSON.stringify` 显示 raw JSON
- 数据已完整存储在 `spend_logs.messages` 和 `spend_logs.response` JSON BLOB 中
- litellm 中 `ui/litellm-dashboard/src/components/view_logs/` 有按 role 渲染气泡的逻辑

## 验收标准

- [ ] messages 按 role（system/user/assistant/tool）结构化展示
- [ ] system 消息：灰色背景区块，默认展开
- [ ] user 消息：气泡风格，含多 part 内容支持（text + image_url）
- [ ] assistant 消息：气泡风格，含 tool_calls 折叠展示（函数名 + 参数 JSON）
- [ ] tool 消息：边框区块，默认折叠
- [ ] response 展示：文本回复 + tool calls + usage + finish_reason
- [ ] Tab 切换：「Prompt」「Response」「Raw」
- [ ] Raw tab 保留原始 JSON（调试用）
- [ ] **每个 Tab 独立复制按钮**：Prompt 复制 messages JSON、Response 复制 response JSON、Raw 复制完整日志 JSON
- [ ] 复制按钮点击后图标从 Copy → Check（绿色），2 秒后恢复
- [ ] 移动端响应式布局
- [ ] **门禁**: 全量 UT + BDD + 前端 Playwright（5 个 scenario × 3 viewports）

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-frontend/src/components/log-viewer/MessageViewer.tsx` | **新建** — messages 按 role 结构化渲染 |
| `crates/aigw-frontend/src/components/log-viewer/ResponseViewer.tsx` | **新建** — response 结构化渲染 |
| `crates/aigw-frontend/src/components/log-viewer/ToolCallBlock.tsx` | **新建** — tool_calls 可折叠组件 |
| `crates/aigw-frontend/src/components/log-viewer/MessageBubble.tsx` | **新建** — 消息气泡组件 |
| `crates/aigw-frontend/src/components/log-viewer/CopyButton.tsx` | **新建** — 统一复制按钮组件（含反馈动画） |
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | **修改** — DetailDrawer 替换 raw JSON 为结构化组件 + Tab |

## 技术方案

### 1. 复制按钮组件（统一定制）

```tsx
import { Copy, Check } from "lucide-react";
import { useCopyToClipboard } from "@/hooks/useCopyToClipboard";

interface CopyButtonProps {
  text: string;
  label?: string;
  className?: string;
}

function CopyButton({ text, label, className }: CopyButtonProps) {
  const { copied, copy } = useCopyToClipboard();

  return (
    <Button
      variant="ghost"
      size="sm"
      className={className}
      onClick={() => copy(text)}
    >
      {copied ? (
        <Check className="h-3.5 w-3.5 text-green-500" />
      ) : (
        <Copy className="h-3.5 w-3.5" />
      )}
      {label && <span className="ml-1 text-xs">{label}</span>}
    </Button>
  );
}
```

### 2. MessageViewer 组件

```tsx
interface MessageViewerProps {
  messages: unknown;  // JSON string or parsed array
}

function MessageViewer({ messages }: MessageViewerProps) {
  const parsed = parseMessages(messages);  // handle both string and array

  return (
    <div className="space-y-3">
      {parsed.map((msg, i) => {
        switch (msg.role) {
          case "system":
            return <SystemBlock key={i} content={msg.content} />;
          case "user":
            return <MessageBubble key={i} role="user" content={msg.content} />;
          case "assistant":
            return (
              <div key={i}>
                <MessageBubble role="assistant" content={msg.content} />
                {msg.tool_calls && <ToolCallBlock toolCalls={msg.tool_calls} />}
              </div>
            );
          case "tool":
            return <ToolResultBlock key={i} content={msg.content} />;
          default:
            return <MessageBubble key={i} role={msg.role} content={msg.content} />;
        }
      })}
    </div>
  );
}
```

### 3. ResponseViewer 组件

```tsx
function ResponseViewer({ response }: { response: unknown }) {
  const parsed = parseResponse(response);

  // OpenAI format: choices[0].message.content + tool_calls
  // Anthropic format: content[] blocks (text + tool_use)
  return (
    <div className="space-y-3">
      <TextContent content={parsed.text} />
      {parsed.toolCalls && <ToolCallBlock toolCalls={parsed.toolCalls} />}
      <UsageStats usage={parsed.usage} />
      <FinishReason reason={parsed.finishReason || parsed.stopReason} />
    </div>
  );
}
```

### 4. DetailDrawer 改造（含复制按钮）

```tsx
// 每个 Tab 顶部都有对应的复制按钮
<div>
  <Tabs defaultValue="prompt">
    <TabsList className="h-7">
      <TabsTrigger value="prompt" className="text-xs h-6">Prompt</TabsTrigger>
      <TabsTrigger value="response" className="text-xs h-6">Response</TabsTrigger>
      <TabsTrigger value="raw" className="text-xs h-6">Raw</TabsTrigger>
    </TabsList>

    <TabsContent value="prompt" className="mt-3">
      {log.messages ? (
        <>
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs text-muted-foreground">
              {Array.isArray(parseMessages(log.messages))
                ? `${parseMessages(log.messages).length} messages`
                : "Messages"}
            </span>
            <CopyButton text={safeStringify(log.messages)} label="Copy Prompt" />
          </div>
          <MessageViewer messages={log.messages} />
        </>
      ) : (
        <p className="text-sm text-muted-foreground py-4 text-center">
          No prompt data for this request
        </p>
      )}
    </TabsContent>

    <TabsContent value="response" className="mt-3">
      {log.response ? (
        <>
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs text-muted-foreground">Response</span>
            <CopyButton text={safeStringify(log.response)} label="Copy Response" />
          </div>
          <ResponseViewer response={log.response} />
        </>
      ) : (
        <p className="text-sm text-muted-foreground py-4 text-center">
          No response data for this request
        </p>
      )}
    </TabsContent>

    <TabsContent value="raw" className="mt-3">
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs text-muted-foreground">Raw JSON</span>
        <CopyButton
          text={safeStringify({ messages: log.messages, response: log.response })}
          label="Copy All"
        />
      </div>
      <RawJsonView messages={log.messages} response={log.response} />
    </TabsContent>
  </Tabs>
</div>
```

### 5. 样式（对齐 litellm）

- **System**: `bg-muted/40 border-l-2 border-muted-foreground/30 rounded p-3 text-xs whitespace-pre-wrap`
- **User 气泡**: `bg-blue-50 dark:bg-blue-950 ml-8 rounded-2xl rounded-br-md p-3`
- **Assistant 气泡**: `bg-green-50 dark:bg-green-950 mr-8 rounded-2xl rounded-bl-md p-3`
- **Tool 区块**: `bg-orange-50 dark:bg-orange-950 border border-orange-200 rounded p-2 text-xs`
- **Tool Call 折叠**: Collapsible with `ChevronDown`/`ChevronRight` toggle, 函数名粗体 + 参数 JSON indent
- **复制按钮**: 每个 Tab 顶栏右侧，复制图标 + label 文字，成功后图标变 Check

## TDD 测试用例

### BDD (Gherkin)

```gherkin
Scenario: Spend log drawer shows structured prompt messages
  Given spend logs 页面已加载
  When 点击一条包含 system/user/assistant 消息的日志记录
  And 切换到 "Prompt" tab
  Then system 消息以灰色背景区块显示
  And user 消息以气泡样式显示
  And assistant 回复以气泡样式显示

Scenario: Spend log drawer shows tool calls as collapsible blocks
  Given spend logs 页面已加载
  When 点击一条包含 tool_calls 的日志记录
  And 切换到 "Response" tab
  Then tool_calls 以可折叠的区块显示
  When 点击折叠按钮
  Then tool call 参数 JSON 展开显示

Scenario: Raw tab shows original JSON with copy button
  Given spend logs 详情已打开
  When 切换到 "Raw" tab
  Then 原始 JSON 以 pre 格式显示
  And "Copy All" 按钮可见

Scenario: Copy prompt button copies data and shows feedback
  Given spend logs 详情已打开且 Prompt tab 激活
  When 点击 "Copy Prompt" 按钮
  Then 图标从 Copy 变为 Check（绿色）
  And 2 秒后恢复为 Copy

Scenario: No prompt data shows placeholder
  Given 一条 messages 为 null 的 spend log
  When 在 spend logs 页面点击该日志
  And 切换到 "Prompt" tab
  Then 显示 "No prompt data for this request"
  And 不显示复制按钮
```

## 风险与回滚

| 风险 | 应对 |
|------|------|
| messages 存储格式不一致（有时为 string，有时为已解析 object） | `parseMessages()` 先 typeof 判断，JSON.parse 失败则 fallback 到 Raw tab |
| response 解析失败（非标准格式） | try/catch + fallback 到 Raw tab 显示 |
| 大消息导致渲染性能问题 | messages > 20 条时默认只显示前 20 条 + "Show all N messages" 按钮 |
| Anthropic content 格式与 OpenAI 格式混合 | 每种格式独立 parser，先尝试 OpenAI（choices），再尝试 Anthropic（content[]） |

回滚方式：`git revert` 该 commit。
