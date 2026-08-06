# Stage 104: Playground 图片输入（上传 + 粘贴 + 预览）

**Phase**: 42 — Playground 多模态图片能力
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 16h
**前置**: Stage 103（后端转换修复 + `/v1/models` 模式字段）
**后置**: Stage 105（图片渲染 + SpendLog 详情增强）

---

## 核心预期

1. **图片上传**：Playground 输入区新增图片附件按钮（隐藏 `<input type="file" accept="image/*" multiple>`），选择图片后 `FileReader.readAsDataURL` 读为 base64。

2. **剪贴板粘贴**：输入区监听 `paste` 事件，从 `clipboardData.items` 提取 image file 转 data URL。

3. **预览条**：输入区上方显示缩略图网格（`<img src={dataUrl}>` + 删除按钮），支持多图。

4. **多模态序列化**：发送时自动把图片并入消息 content——`endpointType==='chat'` 用 OpenAI content array（`[{type:'text'},{type:'image_url',image_url:{url:dataUrl}}]`）；`endpointType==='messages'` 用 Claude content blocks（`[{type:'text'},{type:'image',source:{type:'base64',media_type,data}}]`）。

5. **会话持久化**：图片随 `STORAGE_KEY_MESSAGES` 存 sessionStorage；`clearChat`/`clearSession` 清理。

6. **i18n**：新增 ~6 keys（attachImage/removeImage/imagePreview/pasteHint/uploadError）。

---

## 背景

Playground（`crates/aigw-frontend/src/pages/playground/index.tsx`，1162 行）当前只支持纯文本消息：`ChatMessage.content: string`，发送时 `apiMessages` 类型 `{ role, content: string }`。src/ 目录下**无任何 file input / FileReader / readAsDataURL 先例**（仅 spend-logs 用 `URL.createObjectURL` 导出 CSV），图片输入管线要从零写。

后端 Stage 103 已确保：
- `/v1/chat/completions`（OpenAIPassthrough）原样透传 OpenAI content array（含 `image_url`）。
- `/v1/messages`（AnthropicToOpenAI）把 Claude image block 转 OpenAI content-parts。
- `/v1/models` 暴露 `model_info.mode` 可标识多模态模型（本 Stage 不强制按模式过滤附件，留 UI 自由）。

前端无 `react-dropzone` 等依赖（package.json 无），用原生 FileReader 最简。测试侧 api-mocks.ts 目前无 `/v1/messages` mock（messages 端点请求会 `route.continue()` 落到真实后端），需新增。

---

## 设计

### ① 数据模型（`ChatMessage` + `SettingsData`）

```ts
interface ChatMessage {
  id: string;
  role: "system" | "user" | "assistant";
  content: string;
  images?: string[];   // base64 data URLs（仅 user 消息）
  timestamp: number;
  tokens?: { prompt: number; completion: number };
  error?: string;
}
```

- `content` 保持纯文本（气泡渲染、编辑、复制兼容现有逻辑）。
- `images` 仅 user 消息存在；assistant 消息无（上传只在发送侧）。

### ② 附件状态 + 输入管线（`PlaygroundPage`）

```ts
const [pendingImages, setPendingImages] = useState<string[]>([]);
const fileInputRef = useRef<HTMLInputElement>(null);

const addFiles = (files: FileList | File[]) => {
  for (const f of Array.from(files)) {
    if (!f.type.startsWith("image/")) continue;
    const reader = new FileReader();
    reader.onload = () => setPendingImages(prev => [...prev, String(reader.result)]);
    reader.readAsDataURL(f);
  }
};

// 剪贴板粘贴
useEffect(() => {
  const onPaste = (e: ClipboardEvent) => {
    const items = e.clipboardData?.items ?? [];
    const files: File[] = [];
    for (const item of items) {
      if (item.type.startsWith("image/")) {
        const f = item.getAsFile();
        if (f) files.push(f);
      }
    }
    if (files.length) { e.preventDefault(); addFiles(files); }
  };
  window.addEventListener("paste", onPaste);
  return () => window.removeEventListener("paste", onPaste);
}, []);
```

- `pendingImages` 是待发送附件；发送后并入 user 消息的 `images` 并清空。
- 预览条渲染 `pendingImages.map((src, i) => <img src={src} .../><button onClick={remove}>×</button>)`。

### ③ 多模态序列化（共享 transform fn）

```ts
type ApiContentPart = { type: "text"; text: string } | { type: "image_url"; image_url: { url: string } };
type ClaudeContentPart = { type: "text"; text: string } | { type: "image"; source: { type: "base64"; media_type: string; data: string } };

function imageToParts(src: string): { type: "image_url"; image_url: { url: string } } {
  return { type: "image_url", image_url: { url: src } };
}
function imageToClaudeBlock(src: string): { type: "image"; source: { type: "base64"; media_type: string; data: string } } {
  const match = /^data:(image\/[a-z+.-]+);base64,(.*)$/s.exec(src);
  return {
    type: "image",
    source: {
      type: "base64",
      media_type: match?.[1] ?? "image/png",
      data: match?.[2] ?? src,
    },
  };
}
```

`handleSend` 中，当消息含图片时：
- **chat 端点**：`content` = `[{ type: "text", text }, ...images.map(imageToParts)]`
- **messages 端点**：`content` = `[{ type: "text", text }, ...images.map(imageToClaudeBlock)]`
- 历史消息（含图片）在 `apiMessages` 中同样按上述规则序列化。

### ④ 会话持久化

- `images` 存入 `STORAGE_KEY_MESSAGES`（已有 effect L576-578），JSON 序列化 data URL 字符串数组无额外处理。
- `clearChat`（L604-614）清 `pendingImages`；`clearSession`（L616-624）同步。

### ⑤ 测试 mock（api-mocks.ts）

新增 `/v1/messages` handler（当前 route.continue 到真实后端）：
- 流式：Anthropic SSE（`event: content_block_delta` + `delta.text`），匹配前端 parse（L781 `parsed.delta?.text`）。
- 非流式：`{content: [{type:"text",text:"..."}], usage: {input_tokens, output_tokens}}`。
- 可选：捕获 chat-completions 请求体供 content-array 断言。

---

## TDD 测试计划

### Playwright BDD（8 场景 × 3 viewports = 24 执行）

`playground.feature`：

| # | 场景 | 验证点 |
|---|------|--------|
| 1 | 上传单张图片 | setInputFiles → 预览缩略图可见 |
| 2 | 上传多张图片 | 2 文件 → 2 缩略图 |
| 3 | 剪贴板粘贴图片 | page.evaluate(new File + DataTransfer) → 缩略图可见 |
| 4 | 预览缩略图 | `<img src^="data:image/` 可见 |
| 5 | 删除附件 | 点击 × → 缩略图消失 |
| 6 | 带图发送（chat 端点） | mock 收到 content array 含 `image_url`（poll captured postData） |
| 7 | 带图发送（messages 端点） | mock 收到 content blocks 含 `image.source.data`（需新 /v1/messages mock） |
| 8 | 会话持久化/清理 | 发送后刷新 → 图片仍在；New Chat → 图片清空 |

`playground.steps.ts` 新增步骤：
- `setInputFiles` with `{name, mimeType, buffer}`（无先例，从零写）。
- 合成 paste 事件（`page.evaluate`）。
- mock 请求体断言（poll captured postData）。

### 门禁

|  | 要求 |
|---|------|
| `npm run build` | tsc -b 零错误 |
| `task fe-bdd` | 8 场景 × 3 viewports 全绿（bddgen 强制重新生成） |

---

## 非目标

- 不做图片压缩/缩放（base64 直传，后端 body limit 默认 32 MiB 有足够余量；超限留 TD）。
- 不做拖拽上传（点击 + 粘贴已覆盖主要路径）。
- 不做图片编辑/标注。
- 不按 `model_info.mode` 强制过滤附件（用户可自由给任意模型发图，由上游裁决）。

## 交付清单

- [ ] `crates/aigw-frontend/src/pages/playground/index.tsx` — ChatMessage.images + pendingImages + file input + paste + 预览条 + 序列化 + 持久化
- [ ] `crates/aigw-frontend/src/i18n/locales/en.json` + `zh-CN.json` — +6 keys
- [ ] `crates/aigw-frontend/tests/features/playground.feature` — +8 场景
- [ ] `crates/aigw-frontend/tests/steps/playground.steps.ts` — 新步骤
- [ ] `crates/aigw-frontend/tests/steps/api-mocks.ts` — /v1/messages mock + 请求体捕获
