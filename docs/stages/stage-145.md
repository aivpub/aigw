# Stage 145: 我的 MCP 凭据 + Playground MCP 标签（Phase 55）

**所属**: Phase 55（MCP Gateway）
**预估**: ~8h（tab 骨架 + 自助凭据页 + Playground 标签 + 补齐 142 缺口）
**依赖**: Stage 144（`/v1/mcp/*` + `/admin/mcp/calls` + ACL 端点）、Stage 143（`/mcp-rest/tools/list` 与 `/mcp-rest/tools/call`，Playground 标签的落点）、Stage 142（页面骨架）
**状态**: ✅ 完成（2026-10-29）—— Gate 3 三个前端 task 全绿，142 的 2 HIGH + 7 MED 同批收口

**提交**: `27fd651`（前端我的凭据 + Playground MCP 标签）→ `32e67e1`（前端 BDD）→ `a5fad16`（Phase 55 收官回写），均 `--signoff`，未 push。

---

## 1. 问题

Stage 144 之后后端功能完整了，但两件事在界面上还是黑盒：

1. **普通用户没有地方填自己的 MCP token**。`user_required` 的 server 对他们等于不存在，而他们**不知道原因**——列表里就是不出现，没有任何提示。
2. **排查「模型说没这个工具」没有工具**。这条路径涉及认证、可见性、ACL、前缀、上游可达性五个环节，靠看日志排查很慢。

本 Stage 补这两块，并顺手补掉 Stage 142 留下的两个占位（调用日志对话框、ACL 开关）。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | 普通用户需要看到「哪些 server 对你不可见以及为什么」 | 若只展示已配置的，用户遇到「工具列表少了一半」时无从下手，会当作 bug 报上来 | 设计文档 §8.3：`user_required` 无 token 时列表**静默**剔除——静默是给 agent 的行为，不是给人的行为。给人看的地方必须解释 |
| H2 | `credential_scope` 的可见性对普通用户是「必须展示」而非「可选」 | 不展示的话用户不知道自己填 token 能换来什么，自助率趋近于零 | 三档语义差异正是「填不填 token 有什么后果」的答案 |
| H3 | Playground 已有的双端点序列化与 SSE 渲染能力可复用 | 若不能复用，MCP 标签要另写一套流渲染，工作量翻倍 | `pages/playground/` 已有 streaming 与原始 SSE 展示路径（Phase 42 / 47 的产物） |
| H4 | admin 与普通用户共用同一页面，靠 tab 区分 | 分两个页面会导致导航项对普通用户不可见——反而更要解释「为什么我没有这个菜单」 | 与 `/dash/proxies` 等纯 admin 页不同，MCP 有用户侧内容，所以是**共享页面 + tab 权限**而非**独立页面 + 菜单权限** |

---

## 2. 边界

**做**：
- 在 Stage 142 预留的 tab 容器上加第二个 tab：admin 见「Server 管理」+「我的凭据」；非 admin **只**见「我的凭据」（ADR-041）
- 「我的凭据」页：`server | 凭据模式 | 我的 token | 操作`，`user_required` 且未配置行标黄 + 说明
- `MyTokenDialog`：设置 / 更新 / 删除自己的 token
- Playground 加 MCP 标签页：选 server + 工具 + 参数 JSON → 调 `/mcp-rest/tools/call` → 原始 SSE / JSON 展示
- 补 Stage 142 的两个占位：`CallLogDialog`（接 `/admin/mcp/calls`）、`ToolsDialog` 的 ACL 开关（接 `PUT /admin/mcp/servers/{id}/allowlist`）
- i18n 补 key + `resources.d.ts` 重生成
- BDD ≥6 场景 × 3 viewport

**不做**（明确排除）：
- **不做 playgound 的 MCP 工具参数自动表单** —— 从 `input_schema` 生成表单是独立功能，本 Stage 只给 JSON 文本框
- **不做凭据使用量统计** —— 需要 `mcp_call_log` 聚合，属后续
- **不做 OAuth 授权流程 UI** —— Phase 57

---

## 3. 方案

### 3.1 tab 与权限

```
/dash/mcp
├── [Server 管理]  [我的凭据]        ← admin
└── [我的凭据]                       ← 非 admin
```

判定复用既有 `require_admin` 的语义（`is_master_key || user_role == "proxy_admin"`）。**前端只能看到其中一半**：`useAuth()` 暴露 `userRole` / `userId`（`GET /v2/login/check` 返回 `{user_id, user_role, user_email}`，`crates/aigw-frontend/src/hooks/use-auth.tsx:31`），**没有 `is_master_key`**，而 admin 登录时 `user_role` 恒为 `"proxy_admin"`（`crates/aigw-server/src/routes/login.rs:138-139`）。因此前端判定用 `userRole === "proxy_admin"`，不新增身份端点 —— 会话 cookie 只有一条路径能签发出 `proxy_admin`，这半条语义在前端是冗余的（后端 `require_admin` 仍按完整语义判）。

**默认 tab**：admin 进「Server 管理」（他们的日常工作），非 admin 只有一个 tab。

### 3.2 「我的凭据」页

```
server        | 凭据模式   | 我的 token          | 操作
github        | 共享或用户 | personal  [默认] 启用 | 编辑
internal-wiki | 共享       | — 无需配置          | —
hr-portal     | 必须用户   | ⚠ 未配置            | 设置     ← 整行标黄
```

- 「无需配置」vs「未配置」必须视觉区分：前者灰字（无事可做），后者黄底 + 提示
- 未配置行下方一行小字：**「未配置时，该 server 的工具对你不开放」** —— 这是把「静默剔除」翻译成人话
- 操作列在 `shared` 下不显示（配了也没用，容易误导）；在 `shared_or_user` 下显示「设置/编辑」并附「可选」

### 3.3 `MyTokenDialog`

字段：`token_name`（默认 `personal`）、`token_value`（`type="password"`，不回显）、`expires_at`（可选）。已配置时显示当前元数据 + 「替换」按钮（不是就地编辑明文——明文永不回显，所以「编辑」在语义上只能是「替换」）。

删除需二次确认，文案明确后果：「删除后，该 server 的工具将不再对你开放」。

### 3.4 Playground MCP 标签

```
┌ 聊天 │ MCP ┐
├───────────────────────────────────────────┤
│ Server  [github ▾]                         │
│ Tool    [search ▾]      ← 从 catalog 拉     │
│ Arguments                                   │
│ ┌─────────────────────────────────────────┐ │
│ │ {"query": "rust axum"}                  │ │
│ └─────────────────────────────────────────┘ │
│                        [发送]              │
├───────────────────────────────────────────┤
│ 响应                                        │
│ ┌ 视觉 │ 原始 ┐                             │
│ │  ...（JSON 或 SSE 事件序列）              │
└───────────────────────────────────────────┘
```

调用 `/mcp-rest/tools/call`。**走调试口而非 `/mcp`** —— 调试口无状态、无协商，一次请求就出结果；`/mcp` 要三次握手，排查时反而是噪音。

工具下拉从 `/admin/mcp/servers/{id}/tools` 拉（admin）或从 `POST /mcp-rest/tools/list`（body `{"alias":"..."}`，普通用户）拉 —— 后者是 POST 不是 GET，形状见 stage-143 §3.4。

### 3.5 补齐 Stage 142 的两个占位

- `CallLogDialog`：`server_alias | tool | 身份（server/user/anonymous） | 状态 | 耗时 | 时间`，失败行高亮
- `ToolsDialog` 加 ACL 开关：每工具一行「限制」按钮 → 小表单选 `scope_type` + `scope_id` + `allowed`

---

## 4. 测试

### 4.1 BDD（≥6 场景 × 3 viewport）

| # | 场景 | 断言 |
|---|---|---|
| 1 | 以 admin 登录 | 两个 tab 都可见 |
| 2 | 以普通用户登录 | 只有「我的凭据」tab |
| 3 | `user_required` + 未配置 | 行标黄，含「对你不开放」文案 |
| 4 | 设置 token 后 | 行变正常，「未配置」消失 |
| 5 | `shared` 的 server | 操作列无「设置」 |
| 6 | Playground MCP：选 server/tool 发送 | 响应区显示结果 |
| 7 | 调用日志对话框 | 行渲染，失败行高亮 |

> **Stage 147 后续注记（2026-10-31）**：`CallLogDialog` 已被 `/dash/mcp` 的第三个 tab（`CallLogTab`）取代，行内「调用日志」按钮改为切 tab 并预填 alias。上表 #7 的**断言意图不变**（行渲染 + 失败行高亮），承载场景迁到 `mcp.feature` 的「The call log renders every call and highlights failures」；`stage-147.md` §7 的「`rg CallLogDialog` 为空」是本表格失效的判据。

### 4.2 无新增后端单测

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-frontend/src/pages/mcp/index.tsx` | 加第二个 tab（容器已在 142 预留） |
| `crates/aigw-frontend/src/pages/mcp/MyCredentialsTab.tsx` | 新增 |
| `crates/aigw-frontend/src/pages/mcp/MyTokenDialog.tsx` | 新增 |
| `crates/aigw-frontend/src/pages/mcp/CallLogDialog.tsx` | 新增（替代 142 的占位） |
| `crates/aigw-frontend/src/pages/mcp/ToolsDialog.tsx` | 加 ACL 开关 |
| `crates/aigw-frontend/src/pages/playground/` | 加 MCP 标签（`McpPanel.tsx`） |
| `crates/aigw-frontend/src/i18n/locales/{en,zh-CN}.json` | 补 key |
| `crates/aigw-frontend/src/i18n/resources.d.ts` | 重生成 |
| `crates/aigw-frontend/tests/features/mcp.feature` + `tests/steps/mcp.steps.ts` | 追加场景 |
| `crates/aigw-frontend/tests/features/playground.feature` + `tests/steps/playground.steps.ts` | 追加 MCP 标签场景 |
| `crates/aigw-frontend/tests/steps/api-mocks.ts` | 加 `/v1/mcp/*` 路由 + `/mcp-rest/tools/{list,call}` 路由；**并让 `mockAllApis(page, opts)` 真正 honour `opts.role`** —— 现在 `MockOptions.role`（`:309-313`）声明了却从未被读，`/v2/login*` 三处（`:1386`、`:1389`、`:1566`）硬编码 `proxy_admin`，非 admin 场景没有构造手段，只能写成空步骤「假通过」。已有的「以非 admin 用户登录」步骤（`jobs.steps.ts:142`）就是空体，不要照抄 |

---

## 6. 回归验证

- `task fe-bdd` 全量（含 142 的既有场景不回归）
- `task fe-lint` / `task fe-build`
- 手工：真实 CC 配 `/mcp`，验证「用户未配 token → 工具不出现 → 配了之后出现」的完整闭环

---

## 7. 门禁

- [x] 非 admin 看不到「Server 管理」tab —— `mcp-my-credentials.feature` 的非 admin 场景断言只有一个 tab
- [x] token 明文在 DOM / 网络响应 / Playground 响应区都不可见 —— 场景断言明文既不在 `page.content()` 里、又在 `getLastMcpMyTokenPutBody()` 里（M7）
- [x] `user_required` 未配置行标黄且有明确说明 —— `data-blocked="true"` + 提示文案
- [x] 142 的两个占位已替换为真实现 —— `CallLogDialog.tsx` 接 `/admin/mcp/calls`、`ToolsDialog` 接 `/admin/mcp/servers/{id}/allowlist`
- [x] `en.json` / `zh-CN.json` key 集合一致 —— `scripts/check-i18n-keys.sh` 已并入 `task fe-lint`，输出 `✅ All i18n keys aligned`
- [x] `task fe-bdd` 全量通过 —— Gate 4 整改（HIGH#1 改读 `tool_name` + mock 契约同步）后的收口复跑：`/tmp/p55-fe-bdd.log` **585 passed / 3 skipped / 0 failed**（24.6m，`FE_BDD_EXIT=0`），3 个 skip 是 `login.feature` 既有 `@skip`；新增场景 `A non-admin sees the tools of the server it selected` 在三个 viewport 各自通过（`:172` desktop / `:337` mobile / `:545` tablet）。此前 `/tmp/fe-bdd-145-r3.log` 的 582 passed 是整改前的基线

**证据汇总**：`task fe-lint` EXIT=0、`task fe-build` EXIT=0、`task fe-bdd` 585/3/0（`/tmp/p55-fe-bdd.log`）。

---

## 8. 风险与遗留

**Gate 4 批次结论（同日，与 143/144 同一份独立复查）**：本 Stage 的六条必检项里「前端 token 处理」PASS —— `saveAcl` 三元组去重只移除命中的 `(tool_name, scope_type, scope_id)`、token 明文只在 React state 与 `type="password"` 输入框、全目录无 `localStorage`/`sessionStorage`/`console` 输出、非 admin 的 `/dash/mcp` 只渲染 `MyCredentialsTab`（真正的闸门是服务端 `require_admin`）。HIGH#1 落在本 Stage 的 Playground 消费侧：`McpPanel.tsx` 非 admin 分支读 `tk.name` 而 `/mcp-rest/tools/list` 序列化的是 `tool_name` → 非 admin 的工具下拉恒空。已改为读 `tool_name`，mock 同步改契约形状，并新增场景 `A non-admin sees the tools of the server it selected`。LOW#10（`MyTokenDialog` 的 queryFn 吞错返回 `null`）登记 TD-026 #5。

- **一次误判已纠正** —— 本 Stage 收尾时报告「`/admin/mcp/calls`、`/admin/mcp/servers/{id}/allowlist`、`/v1/mcp/*` 四组接口 mock-only，服务端未挂载」。主 Agent 复核 `main.rs:779` / `:795` / `:809` 与 `:811` 后确认三组路由均已注册（`routes/mod.rs:19` 有 `pub mod mcp_user;`），该结论不成立，不作遗留。**教训**：`grep 'route("/admin/mcp'` 的单行匹配会漏掉跨多行写的 `.route(...)` 链，判断「路由是否存在」要看 `main.rs` 的整段 router 构造。
- **两处移动端横向溢出** —— 本 Stage 的「无横向滚动」门禁暴露了 MCP `TabsList`（两个长标签，320px 视口下 328px）与 playground 头部按钮行（right=619）。只改容器（`flex-wrap`、`max-w-full overflow-x-auto`）不改内容，320/375/768/1280 四档实测溢出 0。
- **`startAcl` 从 `rulesOf(toolName)[0]` 预填** —— 编辑**另一个** scope 时会覆盖第一条规则，但行上显示全部规则、且新场景已证明多规则集合能存活一次保存。属 UX 粗糙而非数据丢失，接受。
- **普通用户看到的 server 列表与 agent 看到的不一致** —— agent 看到的是「静默剔除后」的，用户看到的是「全部 + 为什么」。这是**刻意的**：给人看的要有解释，给模型看的不要噪音。若有人误以为两者不一致是 bug，需要在 UI 上加一句说明。
- **Playground 的 MCP 标签是调试工具，不是产品功能** —— 它的存在是为了让「模型说没这个工具」这类问题能在 30 秒内定位。因此它刻意不做参数自动表单、不做历史记录、不做结果美化。
- **`MyTokenDialog` 只能「替换」不能「编辑」** —— 明文永不回显决定的。这与用户对「编辑」的直觉有落差，所以按钮文案用「替换」而不是「编辑」。

---

## 9. 对后续 Stage 的影响

- Phase 55 至此收官。Phase 57 的 OAuth 发现需要在前端加「连接并授权」按钮，落点在本 Stage 的 `MyTokenDialog` 同位置（OAuth 与手动 token 是同一件事的两种获取方式）。
- 凭据使用量统计、按 provider/实例聚合的趋势，都是 Phase 57 之后的候选。
