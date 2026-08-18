# Stage 123: 代理出口 + 质量检测（Phase 50）

**所属**: Phase 50（代理服务管理）
**预估**: 12h（出口探测 + 质量目标 + CF 检测 + 计分 + 异步探测 + 快照 + reqwest socks + UT/BDD）
**依赖**: Stage 122（proxies 表 + CRUD 就绪）
**状态**: ⏳ 待开始

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

- [ ] 出口探测（ip-api/ipify 经代理）+ 质量目标 + CF 检测 + 计分全绿
- [ ] probe_result 快照落库 × 3 方言
- [ ] 创建/更新异步自动探测接线
- [ ] mock BDD proxies.feature 扩展全绿;fmt + clippy green
