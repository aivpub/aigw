# Stage 142: MCP 管理页 `/dash/mcp`（Phase 55）

**所属**: Phase 55（MCP Gateway）
**预估**: ~18h（列表页 + 五对话框 + 路由 + 侧边栏 + i18n + BDD）
**依赖**: Stage 141（`/admin/mcp/*` 全部端点）
**状态**: ✅ 完成（2026-10-29）—— Gate 4 的 2 HIGH + 7 MED 已由 Stage 145 同批落修，主 Agent 逐条复核

**提交**: —

---

## 1. 问题

Stage 141 之后，管理员能用 `curl` 管 server 了，但界面上一片空白。仓库里同类的「管理页」已经有两套成熟模板（`pages/proxies/`、`pages/websearch/`），MCP 页没有理由是第三套范式。

本 Stage 交付一个**与既有两页结构一致**的管理页，重点是让「凭据模式」这一新概念在 UI 上不费解——它是本功能最容易配错的地方。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | `pages/websearch/index.tsx` 的结构可直接照抄（`useQuery` + `useCallback` 变更 + `toast`） | 若它用了本页不适用的东西（如 `reload` 语义），就得另起模板 | 已读该文件：`afterWrite(label) => (reloadOk) => toast...` 是为 websearch 的「保存但未生效」设计的；MCP 无运行期 registry，写成功即生效，**不需要这一层** |
| H2 | 既有 UI 组件（`@/components/ui/*`）覆盖全部所需（表格 / 对话框 / 开关 / 徽章 / 下拉） | 缺组件就得新造，工作量翻倍 | `pages/proxies/index.tsx` 用到 `Table*` / `Checkbox` / `Badge` / `Dialog` / `Switch` —— 与 MCP 需求同构 |
| H3 | i18n 类型由 `scripts/fe-i18n-types` 从 `en.json` 生成，新增 `mcp` 命名空间后必须重跑 | 不重跑则 `t("mcp.xxx")` 类型报错，`fe-lint` 挂 | 读 `src/i18n/resources.d.ts` 头部注释 + `scripts/check-i18n-keys.sh` |
| H4 | 侧边栏分组的既有约定是 `sidebar.groups.settings` | 分错组会让 MCP 出现在不相关的位置 | `src/components/layout/sidebar.tsx:71` 分组、`:78` / `:80-83` 是 proxies / websearch 的落位 |

---

## 2. 边界

**做**：
- `/dash/mcp` 页面（**tab 容器 + 单个「Server 管理」tab**；容器是本 Stage 的交付物，145 只往里加第二个 tab —— ADR-041）
- 四个对话框：`ServerDialog` / `ProbeDialog` / `TokenDialog` / `ToolsDialog` + 一个日志占位按钮（`CallLogDialog` 依赖 144 的 `/admin/mcp/calls`，本 Stage **不做对话框**，只留按钮，见 §9）
- `App.tsx` 路由 + 侧边栏项
- i18n `mcp` 命名空间（en / zh-CN）+ 重生成 `resources.d.ts`
- BDD ≥8 场景 × 3 viewport

**不做**（明确排除）：
- **不做「我的 MCP 凭据」tab** —— 145（依赖 144 的 `/v1/mcp/*`）
- **不做 Playground MCP 标签** —— 145
- **不做调用日志对话框** —— 依赖 144 的 `/admin/mcp/calls`；本 Stage 留占位按钮「日志（待 Stage 144）」
- **不做白名单编辑的完整 UX** —— `ToolsDialog` 本 Stage 只读展示 catalog + 冲突标红；ACL 编辑在 144 之后才有端点

---

## 3. 方案

### 3.1 页面结构

```
/dash/mcp
├── Tabs: [Server 管理]                    ← 容器在本 Stage 交付，145 加「我的凭据」
├── 页头       [新建 Server]  [全部重新探测]
├── 筛选行     状态: 全部 / 启用 / 停用   ·   搜索: alias / 显示名
├── 表格
│   alias | 显示名 | URL | 协议版本 | 工具数 | 凭据模式 | 状态 | 操作
│   操作: 编辑 · 探测 · 工具 · 凭据 · 日志（占位）· 启停 · 删除
└── 分页
```

**没有多选列** —— 本 Stage 不做批量操作（`POST /batch-delete` 无前端消费者），勾选框只会让人以为能批量删。这与 stage-141 §2 的说明一致。

**凭据模式**列渲染为徽章：

| 值 | 文案（zh-CN） | 配色 |
|---|---|---|
| `shared` | 共享 | 灰 |
| `shared_or_user` | 共享或用户 | 蓝 |
| `user_required` | 必须用户 | 橙 |

**协议版本**列：`NULL` 显示「未探测」（灰），有值显示 `2026-07-28` 之类等宽字体。

### 3.2 `ServerDialog`

字段：`alias`（实时校验，失焦触发 `/admin/mcp/servers/all` 的本地查重 + 本地校验 `__` / 下划线结尾）、`display_name`、`url`、`credential_scope`（三选一 radio，每项一行说明）、`enabled`。

`credential_scope` 的三行说明文案直接决定这一档会不会被配错：

- **共享** —— 「所有用户共用同一个 token；没配 token 就匿名访问」
- **共享或用户** —— 「用户填了自己的就用他的，没填用共享的」
- **必须用户** —— 「只认用户自己的 token；没填的用户看不到这个 server 的工具」

### 3.3 `TokenDialog`

```
┌ 凭据 —— github ────────────────────────────────┐
│ 凭据模式：共享或用户                              │
│                                                │
│ 共享 token                                      │
│ ┌──────────────────────────────────────────┐   │
│ │ svc-account-2026   [默认]  启用  08-28 更新 │   │
│ └──────────────────────────────────────────┘   │
│ [+ 新建共享 token]                              │
│                                                │
│ 用户 token（3 位用户已配置）                     │
│ ┌──────────────────────────────────────────┐   │
│ │ alice   personal   [默认]  启用           │   │
│ │ bob     personal            停用           │   │
│ └──────────────────────────────────────────┘   │
└────────────────────────────────────────────────┘
```

- 值恒为 `***`，**永不回显**
- 「新建」弹小表单：`token_name` + `token_value`（`type="password"`）+ `expires_at`（可选）+ 「设为默认」勾选
- 行操作：`设为默认` / `停用` / `删除`
- 用户级 token 分区对 admin 只读（可代建、可删、可置默认，但不能读值）

### 3.4 `ProbeDialog`

展示 `ProbeOutcome`：

```
协议版本   2026-07-28
Server     modern v0.0.0
缓存       ttlMs = 60000
工具数     12

⚠ 冲突工具（2）
  - search      与 wiki 冲突
  - get_issue   与 internal-gh 冲突
```

冲突工具行**标红**，并给出「改 alias」的建议文案。

### 3.5 `ToolsDialog`

只读表格：工具名 / 描述（`line-clamp-2`）/ 是否有冲突 / 探测时间。本 Stage 不做 ACL 开关（端点未存在）。

### 3.6 i18n

新增顶层命名空间 `mcp`，结构照 `webSearch`：

```jsonc
"mcp": {
  "title": "MCP Servers",
  "subtitle": "...",
  "table": { "alias": "Alias", "displayName": "...", "url": "...", "protocol": "...",
             "toolCount": "...", "credentialScope": "...", "status": "...", "actions": "..." },
  "scope": { "shared": "Shared", "sharedOrUser": "Shared or User", "userRequired": "User Required" },
  "scopeHint": { "shared": "...", "sharedOrUser": "...", "userRequired": "..." },
  "dialog": { ... },
  "token": { ... },
  "probe": { ... },
  "tools": { ... },
  "toast": { "created": "...", "deleted": "...", "probed": "...", "probeFailed": "..." },
  "empty": "..."
}
```

`zh-CN.json` 同步补齐；两文件 key 集必须完全一致（`scripts/check-i18n-keys.sh` 会校验）。

### 3.7 路由与侧边栏

- `App.tsx` 的 `/dash` 子路由块（`:71-93`）追加 `<Route path="mcp" element={<McpPage />} />`
- `sidebar.tsx` 在 `sidebar.groups.settings` 组内、`websearch` 之后追加一项，`sidebar.nav.mcp`

---

## 4. 测试

### 4.1 BDD（≥8 场景 × 3 viewport）

`crates/aigw-frontend/tests/features/mcp.feature`：

| # | 场景 | 断言 |
|---|---|---|
| 1 | 打开页面 | 表格渲染、页头两个按钮存在 |
| 2 | 新建 server（合法 alias） | toast 成功 + 行出现 |
| 3 | 新建 server 用含 `__` 的 alias | 内联错误，提交按钮不可用 |
| 4 | 新建 server alias 重复 | 内联错误提示重复 |
| 5 | 筛选「启用」 | 停用行消失 |
| 6 | 点「探测」 | `ProbeDialog` 打开，显示协议版本与工具数 |
| 7 | 冲突工具 | `ProbeDialog` 内冲突区块可见且标红 |
| 8 | 打开「凭据」 | token 值显示 `***`，页面上 grep 不到明文 |
| 9 | 置第二个 token 为默认 | 第一个的「默认」徽章消失 |
| 10 | 停用 → 删除 | 行消失，toast 成功 |

### 4.2 单测

无新增后端单测。前端无单测框架（本仓前端测试走 BDD）。

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-frontend/src/pages/mcp/index.tsx` | 新增（含 `Tabs` 容器 + `Server 管理` tab + 日志占位按钮） |
| `crates/aigw-frontend/src/pages/mcp/ServerDialog.tsx` | 新增 |
| `crates/aigw-frontend/src/pages/mcp/ProbeDialog.tsx` | 新增 |
| `crates/aigw-frontend/src/pages/mcp/TokenDialog.tsx` | 新增 |
| `crates/aigw-frontend/src/pages/mcp/ToolsDialog.tsx` | 新增 |
| `crates/aigw-frontend/src/pages/mcp/types.ts` | 新增 |
| `crates/aigw-frontend/src/App.tsx` | 加路由 |
| `crates/aigw-frontend/src/components/layout/sidebar.tsx` | 加导航项 |
| `crates/aigw-frontend/src/i18n/locales/{en,zh-CN}.json` | 加 `mcp` 命名空间 + `sidebar.nav.mcp` |
| `crates/aigw-frontend/src/i18n/resources.d.ts` | 重生成（`sh scripts/fe-i18n-types`；Taskfile 无独立 task，`task fe-lint` 会先跑它） |
| `crates/aigw-frontend/tests/features/mcp.feature` + `tests/steps/mcp.steps.ts` | 新增 |
| `crates/aigw-frontend/tests/steps/api-mocks.ts` | 加 `mcp` 路由（`/admin/mcp/*`），照 websearch 段（`:697-840`）的形状；注意 `mockAllApis` 的 `WeakSet` 守卫 —— 场景细化路由必须**后注册**才生效 |

---

## 6. 回归验证

- `task fe-bdd`（新增 8+ 场景 × 3 viewport，全量不回归）
- `task fe-lint`（含 i18n key 对称性与 TS 类型）
- `task fe-build`

---

## 7. 门禁

- [x] 页面在 3 个 viewport（桌面 / 平板 / 手机）无横向滚动、表格不溢出 —— `/tmp/fe-bdd-145-r3.log` 三 viewport 各跑一遍；MCP `TabsList` 与 playground 头部按钮行的溢出由 Stage 145 修掉（只改容器，不改内容）
- [x] token 明文在 DOM 与网络响应里都不可见 —— 145 的场景断言明文既不在 `page.content()`、又确实在 PUT body 里（M7 的真断言）
- [x] `en.json` / `zh-CN.json` key 集完全一致 —— `scripts/check-i18n-keys.sh` 输出 `✅ All i18n keys aligned`，已并入 `task fe-lint`
- [x] `resources.d.ts` 已重生成，`fe-lint` green —— `task fe-lint` EXIT=0
- [x] `task fe-bdd` 全量通过 —— `/tmp/fe-bdd-145-r3.log` **582 passed / 3 skipped / 0 failed**（24.7m，`✘`=0）

---

## 8. 风险与遗留

**Gate 4 结论：BLOCK（HIGH 2 / MED 7 / LOW 7，2026-10-29，独立 reviewer）** → **已全部收口**。整改清单与 145 的同一批文件合并执行 —— 两处 HIGH 都在 `TokenDialog` / `ServerDialog` 的 key 与 `expires_at` 上，而 145 的「我的凭据」tab 复用了这同一批组件，另起 agent 会与 145 抢文件。

**Gate 3 最终证据**：`task fe-bdd` 全量 `/tmp/fe-bdd-145-r3.log` **582 passed / 3 skipped / 0 failed**（24.7m，`✘`=0；3 个 skip 是 `login.feature` 既有 `@skip`）、`task fe-lint` EXIT=0（含新接入的 `scripts/check-i18n-keys.sh`）、`task fe-build` EXIT=0。

逐条：

| # | 级别 | 缺陷 | 落地位置 |
|---|---|---|---|
| H1 | HIGH | `expires_at` 由 `<input type="date">` 直接 `new Date(x).toISOString()`，`YYYY-MM-DD` 在部分实现下解析为非法 → 后端 RFC 3339 解析必拒 → token 永久 unusable | ✅ `TokenDialog.tsx:139` / `MyTokenDialog.tsx:93`：`new Date(\`${expiresAt}T00:00:00Z\`).toISOString()` |
| H2 | HIGH | `TokenDialog` / `ServerDialog` 的 `key` 让对话框中正在输入的密文跟着切换的 server 走 | ✅ `index.tsx:648` / `:657`（`token-closed` / `tools-closed`）/ `:625`（`{id}-{open}`） |
| M1 | MED | 移动端无分页控件 | ✅ `index.tsx:327` `PaginationBar` 已移出 `hidden md:block`（:340） |
| M2 | MED | 「全部重新探测」部分失败无提示 | ✅ `index.tsx:211` `toast.warning(reloadPartial)` |
| M3 | MED | 探测后未失效工具缓存 | ✅ `index.tsx:171` `invalidateQueries(["mcp-server-tools", id])` |
| M4 | MED | 删除无二次确认 | ✅ `index.tsx:102` `mcp-delete-confirm` |
| M5 | MED | 新建对话框跨 server 复用状态 | ✅ `index.tsx:625` `key={editing?.id ?? "new"}-{open}` |
| M6 | MED | 「不发送创建请求」是空断言 | ✅ `mcp-my-credentials.steps.ts:119` `getMcpCreateRequestCount()` |
| M7 | MED | 「明文发到 API」是空断言 | ✅ `mcp-my-credentials.steps.ts:112` `getLastMcpMyTokenPutBody()` |

- **`CallLogDialog` 与 ACL 编辑已补齐**（Stage 145 同批落地）：`CallLogDialog.tsx` 接 `GET /admin/mcp/calls`，`ToolsDialog` 的 ACL 接 `GET/PUT /admin/mcp/servers/{id}/allowlist`，两条路由已在 `main.rs:779` / `:795` 注册。145 一度报「mock-only，服务端未挂载」，主 Agent 复核为误判（见 stage-145 §8）。
- **补丁期间的 `saveAcl` 数据丢失**（Stage 142 Gate 3 之后发现）：原 `filter(r => r.tool_name !== aclTool)` 会连带删掉同一 tool 其它 scope 的规则，现按 `(tool_name, scope_type, scope_id)` 三元组去重（`ToolsDialog.tsx:96-115`），新场景 `Adding a rule for a second scope keeps the first` 三 viewport 全绿。
- **`alias` 查重的本地实现** —— 用 `/admin/mcp/servers/all` 拉全量在内存里比对。server 数量预期是「几十」，不值得为它加一个专门的校验端点；若将来超过 500 个需改为服务端校验。
- **`credential_scope` 的说明文案是本 Stage 最重要的产品决策** —— 三档名字本身不给提示，「必须用户」被误解成「必须配共享 token」的成本远高于多写三行说明。

---

## 9. 对后续 Stage 的影响

- **144** 落地后需回头补 `CallLogDialog` 与 `ToolsDialog` 的 ACL 开关。**这是一处已知的返工**，接受它的理由：把前端排在 141 之后是因为管理端点已完备；等 144 一起做会让前端 Stage 空转两个后端 Stage 的时间。
- **145** 在本页基础上加 tab，因此本 Stage 的页面骨架要预留 tab 容器（即使目前只有一个 tab）。
