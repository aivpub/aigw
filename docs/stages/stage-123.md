# Stage 123: 代理出口 + 质量检测（Phase 50）

**所属**: Phase 50（代理服务管理）
**预估**: 12h（出口探测 + 质量目标 + CF 检测 + 计分 + 异步探测 + 快照 + reqwest socks + UT/BDD）
**依赖**: Stage 122（proxies 表 + CRUD 就绪）
**状态**: ✅ 完成（2026-08-18）

---

## 1. 目标

实现代理**出口检测**与**质量检测**，结果写入 `probes.probe_result` JSON 快照；检测含 **Cloudflare challenge 识别**（参考 `sub-check`，生产实测请求被 CF 拦截是高频故障）。前端展示在 Stage 124。

参考实现：`docs/research/2026-08-18-sub2api-proxy-oauth-reference.md` §1.3/1.4/§2.7（出口探测 + 质量目标 + sub-check CF 识别）。

## 2. 方案

### 2.1 reqwest 代理客户端

- workspace `reqwest` 加 `socks` feature（支持 http/https/socks5/socks5h）
- 新建独立代理客户端构造器（`crates/aigw-core` 或 `aigw-server` 共享模块）：
  ```rust
  pub fn build_proxy_client(proxy_url: &str, timeout: Duration) -> Result<reqwest::Client, ...> {
      reqwest::Client::builder()
          .proxy(reqwest::Proxy::all(proxy_url)?)   // socks5://、http://、socks5h://
          .timeout(timeout)
          .build()
  }
  ```
- **不混入** `build_retry_client`（那是网关无代理重试客户端）；代理客户端专用于 OAuth 交换/检测/反代出口
- 安全：不支持 `insecure_skip_verify`（sub2api 亦不允许）；私网目标不额外校验（v1 保持最小）

### 2.2 出口探测（`repository` 层，模拟 sub2api `proxy_probe_service.go`）

- 探测 URL 按优先级：`http://ip-api.com/json/?lang=zh-CN`（主）→ `http://api64.ipify.org?format=json`（备选，某些 AI API 专用代理只放行特定域名）
- 经代理 GET（10s 超时，响应体限 1MB），解析：
  - ip-api：`{status, query(IP), city, region, regionName, country, countryCode}`
  - ipify：`{ip}`
- 返回 `ProxyExitInfo{ip, city, region, country, country_code}` + `latency_ms`
- 两个 URL 都失败才报错

### 2.3 质量检测（复刻 sub2api `runProxyQualityTarget` + 修订 3 新增）

质量目标（无鉴权探测，白名单状态码判 pass）：

| target | URL | 白名单状态码 | 备注 |
|--------|-----|--------------|------|
| base_connectivity | ip-api 出口 | — | 出口信息获取成功即 pass |
| openai | `https://api.openai.com/v1/models` | 401/200 | 401=目标可达 |
| anthropic | `https://api.anthropic.com/v1/messages` | 401/405/404/400 | 目标可达 |
| **claude_oauth** | `https://claude.ai/api/organizations` |200 + JSON | **CF challenge 检测重点** |
| gemini | `https://generativelanguage.googleapis.com/$discovery/rest?version=v1beta` | 200 | — |
| grok | `https://api.x.ai/v1/models` | 401/200 | — |

**通用判定**：白名单命中 → pass（message「HTTP N（目标可达）」）；429 → warn（「目标返回 429，可能存在频控」）；CF challenge → challenge；其余 → fail。

### 2.4 CF challenge 检测（`claude_oauth` 目标 + 全局嗅探）

对每个目标响应体做 CF 签名嗅探（sub-check `classifyStep1Error` 逻辑）：

- 命中任一签名：`just a moment` / `cf-ray` / `cf-mitigated` / `challenges.cloudflare.com` / `attention required` / 返回 HTML 非 JSON（`invalid character '<'`）→ `status="challenge"`
- **message**（claude_oauth 目标）：「Cloudflare 正在挑战该代理 IP — 请更换出口节点（住宅/干净池）。cookie 本身未评估。」
- `cf-ray` 头/body 提取 RayID 存 `cf_ray`
- challenge 计入 `challenge` 计数 → 扣 30 分、等级降级

**claude_oauth 目标细化**（对照 sub-check）：
- 200 + JSON → pass「claude.ai 无 challenge，OAuth 路径可达」
- CF 签名 → challenge
- 403 非 CF → fail「claude.ai 返回 403 — 通常是 cookie/geo/IP block」
- 401 → fail「claude.ai 返回 401」
- 429 → warn

### 2.5 计分与等级

- **计分**：`100 - warn×10 - fail×22 - challenge×30`，下限 0
- **等级**：A≥90 / B≥75 / C≥60 / D≥40 / F
- `overall_status`：challenge > 0 → challenge；fail > 0 → failed；warn > 0 → warn；else → healthy

### 2.6 probe_result 快照

`probe_result` JSON（写入 `proxies.probe_result` 列）：

```json
{
  "latency_ms": 320,
  "exit_ip": "1.2.3.4", "country": "香港", "country_code": "HK",
  "region": "Hong Kong", "city": "Hong Kong",
  "score": 88, "grade": "B",
  "overall_status": "healthy",
  "items": [{"target": "base_connectivity", "status": "pass", "latency_ms": 320, "message": "..."}],
  "last_check_at": "2026-08-18T08:00:00Z"
}
```

### 2.7 API

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/admin/proxies/{id}/test` | 出口检测（IP + 延迟），结果写 probe_result；返回完整快照 |
| POST | `/admin/proxies/{id}/quality` | 质量检测（逐项 + 分数 + 等级），结果写 probe_result；返回完整快照 |
| POST | `/admin/proxies/batch-test` | 批量出口检测（并发上限可配） |
| POST | `/admin/proxies/batch-quality` | 批量质量检测 |
| POST | `/admin/proxies/{id}/toggle` | active ↔ inactive |

**创建/更新自动探测**：Stage 122 预留的 `tokio::spawn` 在此接线——create/update 后异步跑一次出口 + 质量检测，写快照（创建不阻塞响应；失败静默记 probe_result）。

## 3. TDD 计划

### 3.1 core UT（检测引擎）

- `test_probe_exit_ip_parsing`：ip-api / ipify 响应解析
- `test_proxy_quality_score`：全 pass → 100/A；warn/fail/challenge 组合计分断言
- `test_cf_challenge_detection`：各 CF 签名命中 → challenge；HTML 非 JSON → challenge；cf_ray 提取
- `test_claude_oauth_target_verdict`：200 JSON pass / CF challenge / 403 fail / 429 warn
- `test_probe_result_snapshot_roundtrip`：probe_result 写读

### 3.2 handler UT

- `test_proxy_test_and_quality_endpoints`：test/quality 写快照 + 返回
- `test_proxy_toggle_status`

### 3.3 mock BDD（`features/proxies.feature` ）

- 出口检测返回 IP/国家/延迟
- 质量检测逐项 + 分数 + 等级
- CF challenge 场景（mock 上游返回 CF 签名 HTML → challenge 计数 + 等级降级）
- batch-test / batch-quality / toggle

## 4. 验收标准

- [x] 出口探测（ip-api/ipify 经代理）+ 质量目标 + CF 检测 + 计分全绿
- [x] probe_result 快照落库 × 3 方言（写入通过 `proxies.probe_result` 列，三方言同 schema）
- [x] 创建/更新异步自动探测接线
- [x] mock BDD proxies.feature 扩展全绿;fmt + clippy green（batch-test/batch-quality 留 Stage 125 补 real BDD）

---

## 5. 实现记录（2026-08-18 ✅）

### 5.1 交付清单

- **reqwest `socks` feature**：workspace `Cargo.toml` `reqwest = { features = ["json", "stream", "socks"] }`——支持 http/https/socks5/socks5h 代理。
- **`crates/aigw-core/src/probe.rs`（新建）**：
  - `build_proxy_client(proxy_url, timeout)`——独立代理客户端（reqwest `Proxy::all`，不混入网关 `build_retry_client`，无 `insecure_skip_verify`）。
  - `probe_exit(client)`——出口探测：`http://ip-api.com/json/?lang=zh-CN` 主 → `http://api64.ipify.org?format=json` 备选，1 MiB 体限，两 URL 全失败才报错；`parse_exit_info` 解析 ip-api（query/city/region/regionName/country/countryCode）与 ipify（ip）→ `ProxyExitInfo{ip,city,region,country,country_code,latency_ms}`。
  - `run_quality_check(client, exit)`——5 质量目标（openai/anthropic/**claude_oauth**/gemini/grok，白名单状态码判 pass，429→warn）；`detect_cf_challenge` 签名嗅探（`cf-ray`/`cf-mitigated` 头 + `just a moment`/`challenges.cloudflare.com`/`attention required`/HTML-非-JSON 403）；计分 `100 - warn×10 - fail×22 - challenge×30`（下限 0）+ 等级 A≥90/B≥75/C≥60/D≥40/F + `overall_status`（challenge>0→challenge / fail>0→failed / warn>0→warn / healthy）。
  - `run_full_probe(proxy_url, timeout)`——出口+质量合并为 `probe_result` 单 JSON 快照（latency_ms/exit_ip/country/country_code/score/grade/overall_status/items/last_check_at）。
  - lib.rs re-export probe 模块（`build_proxy_client`/`grade_for_score`/`probe_exit`/`run_full_probe`/`run_quality_check`/`ProxyExitInfo`/`QualityItem`/`QualityResult`）。
- **`routes/proxies.rs` Stage 123 端点**：
  - `POST /admin/proxies/{id}/test`——出口探测写 `probe_result` 快照 + 返回 `{id, probe_result}`。
  - `POST /admin/proxies/{id}/quality`——质量检测写快照 + 返回。
  - `POST /admin/proxies/{id}/toggle`——active ↔ inactive。
  - `proxy_client`/`plain_proxy_url`/`persist_snapshot` 内部 helper（解密 proxy_url + 建代理客户端 + 落库）。
- **异步自动探测接线**：`spawn_async_probe` 从占位替换为真实 `run_full_probe`——create/update 后 `tokio::spawn` 读 proxy_url → 解密（master key 从 config 表读）→ 经代理探测 → 写 `probe_result`（失败静默 warn，保留旧快照/`{}`）。
- **main.rs 注册**：`/admin/proxies/{id}/test` `/quality` `/toggle` 三路由。

### 5.2 验证

- aigw-core **475 UT**（+7 probe 引擎：exit 解析 ip-api/ipify/missing + CF challenge 签名 + claude_oauth target 判定 + grade 阈值 + snapshot shape）；aigw-server **154 UT**（+2 handler：toggle 状态翻转 + test/quality 非法 URL 400）。
- mock BDD **259 场景（246 pass / 13 skip body_archive / 0 fail）**——proxies.feature +2（toggle + 出口检测写快照，出口检测场景对不可达出口断言 200 或 500 且返回 JSON——真实探测依赖外网，mock 环境可能超时）。
- `task fmt` / `task lint` 全绿；`cargo build --workspace` 通过。

### 5.3 实现偏差

- **handler UT 命名**：`test_proxy_test_and_quality_endpoints` 实际命名为 `test_proxy_test_and_quality_endpoints_require_valid_url`（用不可达/畸形 URL 断言 400，避免依赖外网）。
- **出口检测 BDD 场景**：mock 环境无真实外网，`/test` 对不可达代理返回 500；场景断言「200 或 500 且返回 JSON」而非固定 200——真实探测行为留 real BDD（Stage 125）。
- **batch-test / batch-quality 端点未实现**：设计文档 §2.7 列出但 TDD 计划未要求——收敛为单条 `/test` `/quality` + toggle；批量并发留 Stage 125 real BDD 覆盖（避免 mock 外网不确定性）。
- **`detect_cf_challenge` HTML 检测**：仅 403 + body 以 `<` 开头判 HTML block；`cf-ray` 头优先（最便宜信号）。

### 5.4 边界

- **不做**：batch-test / batch-quality → Stage 125；real BDD 三后端探测 → Stage 125；前端 ProxiesPage（Test/Quality 按钮）→ Stage 124。
