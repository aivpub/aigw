# 变更日志

## [未发布]

### 新增
- Stage 127: OAuth Token 生命周期 + 三层自愈（`claude_token.rs` TokenProvider——内存缓存 + per-credential 锁防并发刷新 + 临期 3min 刷新 + invalid_grant cookie 自愈 + needs_reauth 告警 `dispatch_oauth_reauth_alert` + `invalidate_and_refresh` 管线 401 重试入口）
- Stage 126: Claude OAuth 凭证 + Cookie→Token 3 步交换（`claude_oauth.rs` OauthClient 经代理 + PKCE S256 + fetch_orgs/authorize/exchange_code/refresh + select_org + classify_oauth_error；`build_oauth_credential_values` 敏感字段 AES-GCM 加密；`POST /credential/oauth/exchange` + credential_info/list redact；crypto `redact_oauth_credential_values`）
- Stage 115: Anthropic image token downsizing（`estimate_anthropic` 迭代缩放保比例到 ≤1568 target）——TD-011c 解决
- Stage 115: 多模态按模态计费（`ModalPricing` + `Deployment.modal_pricing` + resolver 提取 + `calc_spend_modal` 纯函数 + 6 UT）——TD-012b 解决（embeddings.rs 接线留待真实负载）
- Stage 115: HEIC/AVIF 前端转码（`compressImage` 检测 heic/avif → Safari 解码转 JPEG；无法解码浏览器 toast 提示）——TD-011b 解决
- Stage 114: Playground 图片上传压缩（`src/lib/image.ts` `compressImage`——canvas 2048px + JPEG 0.8，取「原图 vs 压缩」较小者保真）——TD-009a 解决
- Stage 114: 请求体超限防御（handleSend 预检 ∑ dataUrlBytes > 24MiB → toast + 拒绝）——TD-009b 解决
- Stage 114: i18n 翻译懒加载（en eager + 检测语言 eager + zh-CN 独立 lazy chunk 25kB）——TD-008a 解决
- Stage 114: 翻译 TS 类型（`scripts/fe-i18n-types` 生成 `resources.d.ts` 增广 i18next CustomTypeOptions，t('key') 编译期校验）——TD-008b 解决
- Stage 114: Playground 压缩/超限/小图保真 3 BDD 场景 × 3 viewports
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
- Stage 115: `compressImage` 解码失败返回 null（原返回原图 → caller 无法区分「无法渲染」）；TD-011c 单次缩放 overshoot → 迭代缩放
- Stage 114: i18n 动态 import 归一化（navigator=en-US → en bundle，防 Unknown dynamic import unhandled-rejection）
- Stage 114: 修复 5 个缺失 i18n key（health.min/keys.deletedKeys/keys.tpmLabel+rpmLabel/drawer.tabDescription+tabParams）+ dashboard.spend 拼写错误
- Stage 113: 后端 loop panic 不再杀死 tokio task（tick/exec/cleanup 静默降级问题）
- Stage 103: `test_activity_reports_timezone_metadata` date-sensitive 修复（固定 start_time 在查询窗口内）

### 技术债
- 解决: TD-011b/c + TD-012b（Stage 115）；TD-008a/b + TD-009a/b（Stage 114）；TD-005 / TD-003 / TD-010a（Stage 113）——Phase 45 技术债清理全收官
- 引入: 无（TD-011a 视频输入维持剩余，待真实负载）

