# Stage 124: 代理服务管理 — 前端（Phase 50）

**所属**: Phase 50（代理服务管理）
**预估**: 10h（ProxiesPage + CRUD + test/quality + 批量 + sidebar + i18n + BDD）
**依赖**: Stage 122/123（后端 API 就绪）
**状态**: ⏳ 待开始

---

## 1. 目标

前端新增「Proxies」管理页（`/dash/proxies`，Settings 分组），覆盖：列表 + 创建/编辑/删除 + 出口检测 + 质量检测（分数徽章 + 逐项展开）+ 批量操作 + 状态 toggle + 快照列展示；中英双语。

## 2. 方案

### 2.1 路由与侧边栏

- `App.tsx` 新增 `<Route path="proxies" element={<ProxiesPage />} />`
- `sidebar.tsx` Settings 分组新增「Proxies」项（icon `Network`，新增 i18n key `sidebar.nav.proxies`）

### 2.2 页面结构（`crates/aigw-frontend/src/pages/proxies/`）

```
proxies/
  index.tsx          ← ProxiesPage 主页面
  ProxyDialog.tsx    ← 创建/编辑对话框(名称 + proxy_url + expires_at)
  QualityDialog.tsx  ← 质量检测逐项展开(分数/等级/逐项状态 + cf_ray)
  types.ts           ← Proxy/ProxyQualityResult/ProxyExitInfo 接口
```

**列表表格列**（参考 sub2api ProxiesView + aigw models 页风格）：
| 名称 | 出口 IP · 国家 | 延迟 | 分数/等级 | 状态 | 到期 | 操作 |
|------|--------------|------|-----------|------|------|------|
| name | exit_ip (country_code) | latency_ms | score + grade 徽章 | status 徽章 + toggle | expires_at | Test / Quality / 编辑 / 删除 |

- `proxy_url` 显示 redact 形态（`scheme://user:***@host`），password 掩码
- status 徽章：active 绿 / inactive 灰 / expired 红；expired 由 expires_at 派生
- grade 徽章：A 绿 / B 蓝 / C 黄 / D 橙 / F 红
- 批量：checkbox 选择 → 批量测试 / 批量质量 / 批量删除（in-use 跳过 → toast 汇总）
- 搜索框 + status 下拉过滤 + 分页

### 2.3 API 层（`crates/aigw-frontend/src/lib/api.ts` 或页面内 apiGet/apiPost/apiPut/apiDelete）

- `GET /admin/proxies?page=&page_size=&status=&search=` 列表
- `GET /admin/proxies/all` 下拉
- `POST /admin/proxies` / `GET /admin/proxies/{id}` / `PUT /admin/proxies/{id}` / `DELETE /admin/proxies/{id}`
- `POST /admin/proxies/{id}/test` / `{id}/quality` / `{id}/toggle` / `batch-test` / `batch-quality` / `batch-delete`

### 2.4 检测交互

- Test 按钮 → POST test → 更新快照列（exit_ip/延迟）+ toast
- Quality 按钮 → POST quality → QualityDialog 展示分数/等级/逐项（target/status/latency/message/cf_ray）
- 批量测试/质量 → 全部完成后刷新列表 + 汇总 toast（成功/失败数）
- 创建后自动探测（后端异步）：列表轮询或一次性刷新可见快照

### 2.5 i18n

- 新增 `proxies` 命名空间（en.json + zh-CN.json）：`proxies.title`、`proxies.name`、`proxies.proxyUrl`、`proxies.expiresAt`、`proxies.status.*`、`proxies.grade.*`、`proxies.exitIp`、`proxies.latency`、`proxies.score`、`proxies.test`、`proxies.quality`、`proxies.toggle`、`proxies.batchTest/Quality/Delete`、`proxies.create/edit/delete`、`proxies.inUse` 等
- `sidebar.nav.proxies`

## 3. TDD 计划（前端 BDD × 3 viewports）

`e2e/proxies.feature` 新建：
1. 代理列表展示（名称/出口 IP/延迟/分数/等级/状态）
2. 创建代理 → 列表出现 + proxy_url redact
3. 编辑代理
4. 删除代理 + 确认
5. Test 按钮 → 快照列更新
6. Quality 按钮 → 逐项对话框 + 分数/等级
7. 批量删除 in-use 跳过 → 汇总 toast
8. 非 admin → 403 / 重定向

## 4. 验收标准

- [ ] ProxiesPage 全功能（CRUD + test/quality + 批量 + toggle）可用
- [ ] i18n 中英双语完整（`scripts/fe-i18n-types` 通过）
- [ ] 前端 BDD proxies.feature × 3 viewports 全绿;全量 fe-bdd 回归无退化
- [ ] fe-build + fe-lint green
