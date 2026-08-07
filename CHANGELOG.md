# 变更日志

## [未发布]

### 新增
- Stage 105: 图片渲染 — Playground user 气泡缩略图 + log-viewer `extractImages`/`ImageThumbnails`（SpendLog 详情图片/`output_text`/Responses output[] block 渲染）
- Stage 105: SpendLog 详情透传 UT × 3（image_url / output_text / Anthropic image block）
- Stage 104: Playground 图片输入 — 上传/粘贴/预览 + 双端点（chat/messages）多模态序列化 + 独立 sessionStorage 持久化（`src/pages/playground/index.tsx`）
- Stage 104: /v1/messages E2E mock（裸 data: JSON SSE）+ 请求体捕获（api-mocks.ts）
- Stage 103: `/v1/models` 暴露 `model_info.mode`（多模态模型可识别）

### 变更
- Stage 103: `openai_message_to_claude` 修 image 转换 bug — data URL 剥离 + media_type 推导（parse_data_url）

### 修复
- Stage 103: `test_activity_reports_timezone_metadata` date-sensitive 修复（固定 start_time 在查询窗口内）

### 技术债
- 引入: TD-009a/b/e（图片压缩 + 超大图 body limit 防御 + 外链渲染）
