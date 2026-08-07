# Stage 104 Review Log

**Review Type**: Design
**Review Date**: 2026-08-07
**Reviewer**: 独立评审（general-purpose × 2）+ 主模型交叉验证
**Stage**: Stage 104 — Playground 图片输入

## Review Summary

设计总体可行，进入实现前需修正 1 个 Critical（前端/后端 data URL 解析语义不一致）+ 明确 1 个 High（/v1/messages mock SSE 格式）。

## Findings

| # | Severity | 文件:行号 | 问题 | 修复 |
|---|----------|-----------|------|------|
| 1 | Critical | stage-104.md L107-112（imageToClaudeBlock） | 设计正则 `^data:(image\/[a-z+.-]+);base64,(.*)$` 要求 `;base64,` 紧跟 media type。但 data URL 可能带参数（`data:image/png;charset=utf-8;base64,xxx`）——前端正则匹配失败，与后端 Stage 103 `parse_data_url`（`mime.split(';').next()` 取第一段）语义不一致。 | 改用与后端一致的解析：`const sep = src.indexOf(";base64,"); media_type = src.slice(5, sep).split(";")[0]; data = src.slice(sep + 8)`（无 `;base64,` 时 fallback）。 |
| 2 | High | stage-104.md L131-135（/v1/messages mock） | 前端 index.tsx L781 对 messages 端点读 `parsed.delta?.text`，且 L767 跳过 `event:` 行只处理 `data:` 行。设计写"Anthropic SSE delta.text"未明确 data 行 JSON 结构。 | mock 流式应发 `data: {"delta":{"text":"..."}}`（裸 JSON 于 data: 后），非 `event: content_block_delta` 包裹格式。非流式回 `{content:[{type:"text",text:"..."}], usage:{input_tokens,output_tokens}}`。 |
| 3 | Medium | stage-104.md L128-135（mock 断言） | /v1/chat/completions mock（api-mocks.ts L327-349）已 parse reqBody（L328）但未暴露给断言。 | 模块级 `let lastChatBody` 在 mock handler 内捕获 + 导出 getter，或场景内 page.route override 捕获 postData。 |
| 4 | Low | stage-104.md L123-126（持久化） | sessionStorage 配额（~5MB），多图 base64 可能溢出；saveToStorage 已有 try/catch 静默忽略（index.tsx L348-354），设计未提。 | 无需额外处理——现有 saveToStorage 已 catch quota exceeded 静默降级。在实现 Notes 记录。 |

## AI Pre-Filter Results

- 已过滤：粘贴事件 focus 冲突（Textarea 是唯一可编辑区，paste 事件默认在此触发，无需额外处理）；assistant 多轮 images 保留（pendingImages 发送后清空、消息内 images 随 ChatMessage 保留在会话，语义正确）。
- 已确认：`e.preventDefault()` 在图片粘贴时必需（阻止 Textarea 默认文本粘贴），设计已含。

## Resolution Summary

- Critical #1 → 已修正设计文档（imageToClaudeBlock 改 indexOf + slice 解析，与后端一致）。
- High #2 → 已修正设计文档（明确 mock SSE data 行结构）。
- Medium #3 → 实现时用模块级 lastChatBody 捕获。
- Low #4 → 记录实现 Notes，无需代码变更。

**All Critical Fixed**: Yes
**All High Priority Addressed**: Yes

---

## Code Review (Stage 104)

**Review Type**: Code
**Review Date**: 2026-08-07
**Reviewer**: 独立评审 + 主模型交叉验证

### Files Reviewed
- `src/pages/playground/index.tsx`（ChatMessage.images / pendingImages / addImageFiles / paste / 预览条 / 序列化 / Claude blocks 转换）
- `tests/steps/api-mocks.ts`（/v1/messages mock + 请求体捕获）
- `tests/steps/playground.steps.ts`（setInputFiles / 合成 paste / 断言）
- `tests/features/playground.feature`（+8 场景）
- `src/i18n/locales/*.json`（+3 keys）

### Findings

| # | Severity | 文件:行号 | 问题 | 处置 |
|---|----------|-----------|------|------|
| 1 | Medium | index.tsx addImageFiles | SVG 等非栅格 MIME 通过 `image/*` 上传进 `<img src>`；SVG data URL 在 `<img>` 默认不执行脚本，但为最小化向量面，收敛到栅格 MIME 白名单（png/jpeg/gif/webp）+ 20MB 上限跳过。 | 已修：RASTER_MIME + MAX_IMAGE_BYTES 守卫。 |
| 2 | Low | index.tsx pendingImages | 多图大 base64 撑爆 sessionStorage（5MB）与后端 32MiB body limit；现有 saveToStorage try/catch 静默。20MB 单图守卫 + TD-009a/b 登记体积压缩/413 友好提示。 | 已修 20MB 单图跳过；TD-009 已登记（Stage 105 文档）。 |
| 3 | OK | index.tsx 序列化 | apiMessages 转 content array（text + image_url）后，messages 端点 map 只转 image_url part，text part 原样透传——无重复无丢失。system 字段从 string content 取（system 消息无 images），无 unknow[] 泄漏。 | 无需改。 |
| 4 | OK | mock 捕获 | exposeCapturedBodies 注册时置初值 + syncCapturedBodies 在断言前显式同步 + waitForTimeout(300)，测试 42 场景全绿验证无竞态。 | 无需改。 |
| 5 | OK | paste 监听 | 仅拦截 image paste 并 preventDefault，纯文本粘贴不受影响（无 image file 时 return）。 | 无需改。 |

### Resolution Summary
- 1 个 Medium（MIME 白名单 + 体积守卫）→ 已修，桌面 14 场景复跑全绿。
- 其余为设计确认（无缺陷）或已登记 tech debt。
- 全量 frontend BDD：300 passed / 3 skipped（含新增 24 执行）。

**All Critical Fixed**: Yes
**All High Priority Addressed**: Yes
