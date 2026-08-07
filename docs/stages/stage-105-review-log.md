# Stage 105 Review Log

**Review Type**: Design
**Review Date**: 2026-08-07
**Reviewer**: 独立评审（general-purpose）+ 主模型交叉验证
**Stage**: Stage 105 — 图片渲染 + SpendLog 详情增强 + 文档收尾

## Review Summary

设计总体可行。确认 2 个 High（output_text block 结构 + https URL 渲染）需在设计层面明确，1 个 Medium（extractImages 递归 depth）加边界。进入实现前修正设计文档。

## Findings

| # | Severity | 文件:行号 | 问题 | 修复 |
|---|----------|-----------|------|------|
| 1 | High | stage-105.md L100-105（ResponseViewer） | `extractTextContent`（ResponseViewer L98-110）只认顶层 `text`/`[type]`。Responses API 的 `output_text` block 嵌在 `output[].message.content[]`（`{type:"output_text",text}`），现有函数对嵌套 array 只取第一层 `part.type`，`text` 分支命中但 `output_text` 返回 `[output_text]`。设计未区分 ResponseViewer（未在 spend-logs 引用）vs OutputCard（实际使用）——OutputCard 才是 spend-logs drawer 的解析器。 | OutputCard.parseOutput 是主改点（OpenAI 分支 extractText 已走 utils 会受益）；ResponseViewer 同步加 output_text 分支但非阻塞（未引用）。明确设计主路径是 OutputCard + utils.ts。 |
| 2 | High | stage-105.md L72-82（extractImages） | 设计只提 `data:image/` data URL 提取。SpendLog 里可能存 `https://` image_url（客户端传 URL 而非 data URL）——admin 详情渲染任意 https URL 有 SSRF/隐私面（虽然 admin-only）。 | extractImages 只返回 `data:image/` 前缀的 URL；https 非 data 一律不渲染（返回空），缩略图组件同理。TD 登记 https 外链渲染为后续项。 |
| 3 | Medium | stage-105.md L59-70（extractImages 递归） | 递归 `extractImages(content.content)` 对深层嵌套（Responses output→message→content array）正确，但需防循环引用（JSON 来自后端不可能自引用，安全）。对非 array 的 `content` 字段（string）返回空——正确。 | 无需改，实现加 `if (!content) return []` 守卫。 |

## AI Pre-Filter Results

- 已过滤：OutputCard 与 ResponseViewer 重复解析（spend-logs 只用 OutputCard/InputCard/parseMessages；ResponseViewer 未引用，同步改保持一致性即可，非阻塞）。
- 已过滤：SectionHeader HistoryTree 图片缩略图进 history（history 是折叠摘要，保持 extractText 文本预览足够，图片缩略图只在主气泡渲染）。

## Resolution Summary

- High #1 → 设计主路径明确为 **utils.ts extractText/extractImages + OutputCard.parseOutput**（spend-logs 实际解析器）；ResponseViewer 同步补 output_text 分支（一致性，非阻塞）。
- High #2 → extractImages 只返回 `data:image/`；https 外链不渲染（admin-only 详情，但收窄向量面），登记 TD-009e。
- Medium #3 → 实现加 `!content` 守卫。

**All Critical Fixed**: Yes
**All High Priority Addressed**: Yes

---

## Code Review (Stage 105)

**Review Type**: Code
**Review Date**: 2026-08-07
**Reviewer**: 独立评审 + 主模型交叉验证

### Files Reviewed
- `components/log-viewer/utils.ts`（extractText output_text/file + extractImages 递归）
- `components/log-viewer/ImageThumbnails.tsx`（新建）
- `components/log-viewer/OutputCard.tsx`（images + Responses output[] output_text 分支）
- `components/log-viewer/ResponseViewer.tsx` / `MessageBubble.tsx` / `InputCard.tsx`
- `pages/playground/index.tsx`（user 气泡图片渲染）
- `routes/spend.rs`（3 UT：detail 保留 image_url/output_text/Anthropic image block）
- `tests/steps/api-mocks.ts`（sampleDetailImage + IMG_SPEND_ROW）

### Findings

| # | Severity | 文件:行号 | 问题 | 处置 |
|---|----------|-----------|------|------|
| 1 | Medium | InputCard.tsx L79-80 | `extractImages(lastMsg.content)` 调用两次（条件 + 渲染）。 | 已修：IIFE 缓存单次调用；ResponseViewer 同理（L154-155）。 |
| 2 | OK | utils.ts extractImages | `data:image/svg+xml` 也以 `data:image/` 开头，但 `<img>` 内 SVG 不执行脚本；`<image xlink:href>` 外部实体在 `<img>` 上下文不加载。保持现状，TD-009e 补充登记 raster 白名单。 | 无需改（风险面已收窄，登记 TD-009e）。 |
| 3 | OK | OutputCard Responses 分支 | `output[].message.content[]` 的 image_url block 经 extractImages 递归可达；function_call toolCalls 结构 ToolCallBlock 兼容（name/arguments/call_id）；finishReason 用 `status` 字段合理。 | 无需改。 |
| 4 | OK | spend.rs UT | `get_detail` 用 `app.clone().oneshot`（Router Clone 轻量）；base_detail_log start_time=Utc::now() 但 detail 端点不按日期过滤，安全。 | 无需改。 |
| 5 | OK | playground bubble | `msg.images` 在 user（isUser）分支渲染，assistant 无 images 字段，条件正确。 | 无需改。 |
| 6 | OK | api-mocks.ts | /spend/logs 与 /global/spend/logs 均含 IMG_SPEND_ROW，count 逻辑各自一致。 | 无需改。 |

### Resolution Summary
- 1 个 Medium（重复 extractImages 调用）→ 已修，tsc clean。
- 其余为设计确认（无缺陷）或登记 TD-009e。
- 全量 frontend BDD：**312 passed / 3 skipped**（含新增 15 执行）；后端 3 UT + 全量 task test 绿。

**All Critical Fixed**: Yes
**All High Priority Addressed**: Yes
