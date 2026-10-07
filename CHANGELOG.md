# 变更日志

## [未发布]

### 新增
- Stage 135: 内建 web search 接线 —— 三入口触发检测 + prompt 注入（`aigw-core::websearch::trigger` / `inject` + `aigw-server::routes::web_search_wire`）：Chat `web_search_options`（消费并移除，堵住静默透传给上游）/ Responses `web_search(`_preview`)` / Anthropic `web_search_20250305`；结果按 Higress 模板注入**最后一条 user 消息**（追加而非替换，保住 tool_result blocks）；`search_context_size` low/medium/high → 1/3/5；搜索失败降级放行；触发搜索的请求绕过 exact-match 缓存；原生直通上游（`AnthropicNative` / 声明 `responses`）豁免以免双重计费
- Stage 135: 修复 `ClaudeToolDef`（`models.rs`）—— `input_schema` 改 `Option` + 新增 `tool_type`（`#[serde(rename="type")]`）与 `max_uses`；此前带 `web_search_20250305` 的 Anthropic 请求在反序列化即失败 → 三条路由均 **HTTP 500**（TD-017g 解决）；服务端工具现按 Responses 侧同款「丢弃 + warn」处理
- Stage 135: 响应 metadata 标记 `{"aigw":{"web_search":{"status":"ok|degraded|not_configured|no_target","provider","results"}}}`（仅非流式）
- Stage 135: `web_search.feature` 7 场景（三入口触发 + 未配置降级 + 500/超时降级 + 缓存 bypass）
- Stage 130: `/credential/new` OAuth 凭证逐字段 AES-256-GCM 加密落库（`encrypt_oauth_credential_values` 经 `aigw-core::crypto::encrypt_litellm_value_gcm`——`v2:gcm:` 信封，与 exchange 一致）——安全审计第 1/2 项
- Stage 130: `aigw-core::crypto::encrypt_litellm_value_gcm`（litellm `v2:gcm:` AES-256-GCM 加密信封，PBKDF2-HMAC-SHA256 600k） + roundtrip UT
- Stage 130: `chat::upstream_error_message`（只提取上游 `error.message`，parse 失败退化 `Upstream returned HTTP {status}`）接线 chat/v1_messages/responses/embeddings 四处 handler——TD-015a 解决
- Stage 130: real BDD `claude_oauth_crud.feature`（三后端 OAuth 凭证 CRUD + 加密落库 DB 直读断言 + in-use 409 + 探测）——`real_oauth_steps.rs`
- Stage 129: CredentialsTab OAuth 入口（`OAuthCredentialDialog.tsx` 粘贴 `sk-ant-sid` cookie + 代理下拉 + inject_prompt → exchange；OAuth 独立行 active/needs_reauth 徽章 + 到期时间 + 绑定代理 + last_error + Refresh/Re-auth/编辑/删除）
- Stage 129: `claudeOAuth` i18n 命名空间（en + zh-CN，`fe-i18n-types` 已再生成）
- Stage 129: 修复 `claude_token.rs cookie_self_heal` 对已解密 `session_key` 二次解密（base64 Invalid padding）→ 自愈永远不可达，改用已解密明文
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
- Stage 130: chat/v1_messages/responses/embeddings 客户端上游错误从「拼原始 body」收窄为 `upstream_error_message`（TD-015a，全库模式）
- Stage 113: `Engine::run` 拆出 `run_with_cancel(token)`（保持 `run()` 兼容签名）；health.rs `run_and_save_health_check` 增 `model_info` 参数 + 抽 `build_probe_spec`
- Stage 103: `openai_message_to_claude` 修 image 转换 bug — data URL 剥离 + media_type 推导（parse_data_url）

### 修复
- Stage 130: `/credential/new` OAuth 凭证明文落库（token 三件套经 exchange 已加密但通用创建路径遗漏）——real BDD DB 直读探针暴露并修复
- Stage 129: `claude_token.rs cookie_self_heal` 对已解密 `session_key` 二次 `decrypt_litellm_value` → base64 Invalid padding → cookie 自愈永远不可达（409 场景暴露）；改用已解密明文
- Stage 115: `compressImage` 解码失败返回 null（原返回原图 → caller 无法区分「无法渲染」）；TD-011c 单次缩放 overshoot → 迭代缩放
- Stage 114: i18n 动态 import 归一化（navigator=en-US → en bundle，防 Unknown dynamic import unhandled-rejection）
- Stage 114: 修复 5 个缺失 i18n key（health.min/keys.deletedKeys/keys.tpmLabel+rpmLabel/drawer.tabDescription+tabParams）+ dashboard.spend 拼写错误
- Stage 113: 后端 loop panic 不再杀死 tokio task（tick/exec/cleanup 静默降级问题）
- Stage 103: `test_activity_reports_timezone_metadata` date-sensitive 修复（固定 start_time 在查询窗口内）

### 技术债
- 解决: TD-015a（Stage 130，`upstream_error_message` 全库收窄）；TD-011b/c + TD-012b（Stage 115）；TD-008a/b + TD-009a/b（Stage 114）；TD-005 / TD-003 / TD-010a（Stage 113）——Phase 45 技术债清理全收官
- 引入: 无（TD-015b/c/d/e/f + TD-016a/b 维持记录；TD-015d/e 转长期路线 LT-OAuthResponseAdapt / LT-CountTokensAuth）

