# Stage 105: 图片渲染 + SpendLog 详情增强 + 文档收尾

**Phase**: 42 — Playground 多模态图片能力
**优先级**: P1
**状态**: ✅ Complete
**预估**: 12h
**前置**: Stage 104（Playground 图片数据模型就绪）✅ 完成
**后置**: 无（Phase 42 收尾）

---

## Implementation Notes

### Implementation Differences
- **主路径明确为 utils.ts + OutputCard**：`extractText`/`extractImages`（log-viewer/utils.ts）是 SpendLog drawer 的实际解析器；`OutputCard.parseOutput` 补 `images` 字段 + **Responses API `output[].message.content[]` 分支**（output_text / image_url / function_call）——这是 output_text 场景能渲染的关键（设计仅提 ResponseViewer，spend-logs 实际用 OutputCard）。
- **Responses output 分支（Gate 3 发现）**：spend-logs 的 output_text E2E 失败暴露 OutputCard 无 Responses API 解析——新增 `output[].message.content[]` 分支，output_text 拼接文本 + image_url 收集图片 + function_call 收 toolCalls；finishReason 用 `status` 字段。
- **extractImages 只返回 `data:image/`**（Gate 2）：https 外链不渲染（admin-only 详情仍收窄向量面），TD-009e 登记 raster 白名单 + 外链渲染为后续。
- **缩略图共享组件**：`ImageThumbnails`（log-viewer）+ Playground user 气泡 `msg.images` 渲染。

### Technical Decisions Made
- `extractText` 增 `output_text`/`file`/`text_delta`/`function_call` 分支；`extractImages` 递归处理 OpenAI image_url / Anthropic image block / 嵌套 content（Responses output→message→content）。
- Playground 气泡用 `msg.images`（data URL 数组）直接渲染，不依赖后端转换。
- spend.rs 3 UT 用 `make_detail_state` + `base_detail_log` + `get_detail` helper 抽掉 AppState 样板（~90 行/用例）。

### Testing Evidence
- Rust UT：3 个 detail 透传测试（image_url / output_text / Anthropic image block）全绿；`task test` 全量 20 suite 无失败。
- Playwright BDD：playground + spend-logs 新增 5 场景（user 气泡缩略图 / 详情图片缩略图 / output_text 渲染 / raw tab 保留）× 3 viewports = 15 执行全绿。
- 全量 frontend BDD：**312 passed / 3 skipped**（含 Stage 105 新增）。
- tsc -b 零错误；Gate 2 设计评审 2 High + 1 Medium、Gate 4 代码评审 1 Medium 已修。

---

## 核心预期

1. **Playground 图片气泡渲染**：user 消息带 `images` 时在气泡内渲染缩略图（`<img src={dataUrl}>`）。

2. **log-viewer 多模态 block 渲染**：`extractText`/`OutputCard`/`ResponseViewer`/`MessageBubble`/`InputCard` 补 `output_text`/`input_text`/`image_url`/`image`/`file` block 类型；新增共享 `extractImages()` helper + `ImageThumbnails` 组件，SpendLog 详情 drawer 自然透传图片。

3. **SpendLog 详情 body 透传验证**：3 个后端 UT 断言 detail 端点保留 image_url / output_text / Anthropic image block 原始 JSON。

4. **文档收尾**：ADR-025（Phase 42 决策）+ roadmap Phase 42 段 + 修订记录 v42.0 + next-steps + TD-009。

---

## 背景

Stage 104 让 Playground 能发送图片，但两条渲染链路还不完整：

1. **Playground 气泡**：`MessageBubble`（index.tsx L217-219）user 分支只渲染 `msg.content` 文本，`images` 数组无渲染。
2. **log-viewer（SpendLog 详情）**：
   - `extractText`（utils.ts L7-26）只认 `text`/`input_text`，`image_url`/`image` → `"[Image]"` 占位，**缺 `output_text`**（Responses API 输出 block，Stage 101/102 引入）与 `file`。
   - `OutputCard.parseOutput`（L20-118）OpenAI 分支用 `extractText(msg.content)`（无法渲染图片 data URL）；Anthropic 分支只收 `text`/`text_delta`/`tool_use`。
   - `ResponseViewer.extractTextContent`（L98-108）只认 `text`，其他 block 返回裸 `[type]`。
   - `MessageBubble`/`InputCard`（SpendLog 侧）用 `extractText`，图片只显示 `[Image]` 占位。

`spend-logs/index.tsx` 详情 drawer（L858-918）无需改动——`hasPrompt`/`hasResponse` + `InputCard`/`OutputCard` 已接入，图片经 log-viewer 组件自然透传。

---

## 设计

### ① Playground 图片气泡（`pages/playground/index.tsx`）

`MessageBubble` user 分支（L217-219）加图片渲染：

```tsx
{msg.images?.length ? (
  <div className="flex flex-wrap gap-2 mt-2">
    {msg.images.map((src, i) => (
      <img key={i} src={src} alt="attachment" className="max-h-40 max-w-56 rounded-md border object-contain" />
    ))}
  </div>
) : null}
```

### ② log-viewer 多模态支持

**`utils.ts`** — 扩展 `extractText` + 新增 `extractImages`：

```ts
// extractText 增分支
if (part.type === "text" || part.type === "input_text" || part.type === "output_text")
  return String(part.text ?? "");
if (part.type === "image_url") return "[Image]";
if (part.type === "image") return "[Image]";
if (part.type === "file") return `[File: ${String(part.filename ?? "unknown")}]`;

/** 递归提取 content 数组中的图片 data URL（OpenAI image_url / Anthropic image） */
export function extractImages(content: unknown): string[] {
  if (!content) return [];
  if (typeof content === "string") {
    return content.startsWith("data:image/") ? [content] : [];
  }
  if (Array.isArray(content)) {
    return content.flatMap((part: Record<string, unknown>) => {
      if (part.type === "image_url") {
        const url = (part.image_url as Record<string, unknown>)?.url;
        return typeof url === "string" && url.startsWith("data:image/") ? [url] : [];
      }
      if (part.type === "image") {
        const src = (part.source as Record<string, unknown>) ?? {};
        const data = String(src.data ?? "");
        const mt = String(src.media_type ?? "image/png");
        return data ? [`data:${mt};base64,${data}`] : [];
      }
      return extractImages(part as unknown);
    });
  }
  if (typeof content === "object" && content !== null) {
    return extractImages((content as Record<string, unknown>).content);
  }
  return [];
}
```

> **Gate 2 修正**（stage-105-review-log.md）：主路径是 **utils.ts extractText/extractImages + OutputCard.parseOutput**（spend-logs drawer 实际解析器）；ResponseViewer 同步补 output_text 分支（一致性，非阻塞）。**extractImages 只返回 `data:image/` 前缀**——`https://` 外链 image_url 一律不渲染（admin-only 详情仍收窄向量面，TD-009e 登记）。`!content` 守卫防空。

**`ImageThumbnails.tsx`**（新建共享组件）：

```tsx
export function ImageThumbnails({ images, maxH = "h-32" }: { images: string[]; maxH?: string }) {
  if (!images.length) return null;
  return (
    <div className="flex flex-wrap gap-2">
      {images.map((src, i) => (
        <img key={i} src={src} alt="image attachment"
             className={`${maxH} max-w-48 rounded-md border object-contain`} />
      ))}
    </div>
  );
}
```

**接线**（`OutputCard` / `ResponseViewer` / `MessageBubble` / `InputCard`）：
- `OutputCard.parseOutput` 返回 `images: string[]`（OpenAI 分支 `extractImages(msg.content)`，Anthropic 分支收集 `block.type==="image"` 的 `source`），渲染区加 `<ImageThumbnails images={parsed.images} />`。
- `ResponseViewer.extractTextContent` 增 `output_text`/`image` 分支，渲染图片。
- `MessageBubble`（SpendLog 侧）`extractImages(content)` + 缩略图。
- `InputCard` lastMsg 分支同样处理。

### ③ SpendLog 详情后端 UT（`routes/spend.rs`）

复用 `test_global_spend_log_detail_found`（L1521-1611，~90 行含 AppState 样板）模板，抽 `make_detail_state(db)` helper 缩减。3 UT：

| # | 测试 | 断言 |
|---|------|------|
| 1 | `test_detail_preserves_openai_image_url` | `messages` 含 `[{type:"image_url",image_url:{url:"data:image/png;base64,..."}}]` 原样返回 |
| 2 | `test_detail_preserves_output_text` | `response` 含 `content:[{type:"output_text",text:"..."}]` 原样返回 |
| 3 | `test_detail_preserves_anthropic_image_block` | `messages` 含 `{type:"image",source:{type:"base64",media_type:"image/png",data:"..."}}` 原样返回 |

### ④ 文档收尾

| 文档 | 改动 |
|------|------|
| `docs/08-autonomous-decisions.md` | ADR-025：Phase 42 决策（前端 base64 直传、OpenAI content-parts 双端点、log-viewer 共享组件、模型模式感知推迟） |
| `docs/stages/stage-roadmap.md` | Phase 42 段（3 Stage 表 + 合计 34.5h）+ 进度条 + 修订记录 v42.0 |
| `docs/11-next-steps.md` | Phase 42 段 + 当前阶段更新 |
| `docs/12-technical-debt.md` | TD-009：图片 base64 体积/压缩/缩放、超大图 body limit、图片放大查看 |

---

## TDD 测试计划

### Playwright BDD（4 场景 × 3 viewports = 12 执行）

| 文件 | 场景 | 验证点 |
|------|------|--------|
| `playground.feature` | 图片气泡渲染 | 发送含 image_url 消息 → user 气泡 `img[src^="data:image/"]` 可见 |
| `spend-logs.feature` | 详情图片缩略图 | 打开详情 → 图片缩略图可见 |
| `spend-logs.feature` | output_text 响应渲染 | 详情 → `output_text` 文本渲染（非 `[output_text]`） |
| `spend-logs.feature` | raw tab 保留原始 JSON | raw tab 显示含 `image_url` 的原始 JSON |

### Rust UT（3）

见设计 ③。

### 门禁

|  | 要求 |
|---|------|
| `task check` | 无编译错误 |
| `task test` | aigw-server lib 全绿（新增 3 UT） |
| `task fe-bdd` | 4 场景 × 3 viewports 全绿 + 全量回归 |
| `task test-bdd` | 后端 mock BDD 全绿 |

---

## 非目标

- 不做图片点击放大/灯箱（TD-009 登记）。
- 不做 SpendLog 详情 drawer 结构改动（现有 hasPrompt/hasResponse + InputCard/OutputCard 已够）。
- 不做 base64 体积统计/告警（TD-009 登记）。
- 不做 `/v1/models` 模式标签 UI（Playground 模型下拉暂不加多模态图标，Stage 104 已定）。

## 交付清单

- [ ] `pages/playground/index.tsx` — user 气泡图片渲染
- [ ] `components/log-viewer/utils.ts` — extractText 扩展 + extractImages
- [ ] `components/log-viewer/ImageThumbnails.tsx` — 新建共享组件
- [ ] `components/log-viewer/OutputCard.tsx` + `ResponseViewer.tsx` + `MessageBubble.tsx` + `InputCard.tsx` — 接线
- [ ] `i18n/locales/en.json` + `zh-CN.json` — 图片 i18n keys
- [ ] `routes/spend.rs` — 3 UT（detail 透传）
- [ ] E2E × 4 场景
- [ ] 文档：ADR-025 + roadmap v42.0 + next-steps + TD-009 + stage-105.md 收尾
