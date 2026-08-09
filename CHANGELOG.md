# 变更日志

## [未发布]

### 新增
- Stage 113: Async Engine panic 容错（`guarded()` catch_unwind 包裹三 loop 迭代）+ CancellationToken 优雅关闭（`Engine::run_with_cancel`）——TD-005 解决
- Stage 113: `task bdd-coverage` 端点覆盖率报告脚本（`scripts/bdd-coverage`，mock+real feature 解析）——TD-003 解决
- Stage 113: health 探测 embedding-mode 分支 — `model_info.mode="embed"` 模型走 `{api_base}/embeddings` 探针（body `input:["ping"]`）——TD-010a 解决
- Stage 113: 健康检查 embedding 探针 BDD 场景（`/v1/embeddings` 命中 + `input` 字段断言）
- Stage 105: 图片渲染 — Playground user 气泡缩略图 + log-viewer `extractImages`/`ImageThumbnails`（SpendLog 详情图片/`output_text`/Responses output[] block 渲染）
- Stage 105: SpendLog 详情透传 UT × 3（image_url / output_text / Anthropic image block）
- Stage 104: Playground 图片输入 — 上传/粘贴/预览 + 双端点（chat/messages）多模态序列化 + 独立 sessionStorage 持久化（`src/pages/playground/index.tsx`）
- Stage 104: /v1/messages E2E mock（裸 data: JSON SSE）+ 请求体捕获（api-mocks.ts）
- Stage 103: `/v1/models` 暴露 `model_info.mode`（多模态模型可识别）

### 变更
- Stage 113: `Engine::run` 拆出 `run_with_cancel(token)`（保持 `run()` 兼容签名）；health.rs `run_and_save_health_check` 增 `model_info` 参数 + 抽 `build_probe_spec`
- Stage 103: `openai_message_to_claude` 修 image 转换 bug — data URL 剥离 + media_type 推导（parse_data_url）

### 修复
- Stage 113: 后端 loop panic 不再杀死 tokio task（tick/exec/cleanup 静默降级问题）
- Stage 103: `test_activity_reports_timezone_metadata` date-sensitive 修复（固定 start_time 在查询窗口内）

### 技术债
- 解决: TD-005 / TD-003 / TD-010a（Stage 113）
- 引入: 无

