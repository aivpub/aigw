# Stage 151: `/dash/mcp` 与 Playground 的 tab 进路由（Phase 58）

**所属**: Phase 58（MCP 日志与审计完善）
**预估**: ~8h（路由重构 + 深链 + 行内按钮改 URL + BDD 迁移）
**依赖**: Stage 149（同页改动，避免同一文件两次冲突）、Stage 150（新增第 4 个 tab，本 Stage 之后才有意义）
**状态**: ✅ 完成（2026-10-31）

---

## 1. 问题

`/dash/mcp`（`crates/aigw-frontend/src/pages/mcp/index.tsx:124`）与 `/dash/playground`（`playground/index.tsx:583`）的 tab 都是组件内部 `useState`：

```tsx
const [tab, setTab] = useState(isAdmin ? "servers" : "mine");   // mcp
const [mode, setMode] = useState<"chat" | "mcp">("chat");        // playground
```

后果：

1. **刷新回到默认 tab** —— 在调用日志页按 F5，回到 Server 管理。
2. **链接不能分享** —— 「你看这条调用日志」只能描述「点 /dash/mcp，切到第三个 tab，在 Alias 输 `iwiki`」。运维传问题现场时最需要的就是一条 URL。
3. **行内「调用日志」按钮也是 state 跳转** —— `setTab("calls") + setCallsAlias(s.alias)`（`index.tsx:417`/`:533`），地址栏不变，用户复制 URL 发出去，对方看到的还是默认 tab 且没有 alias 过滤。
4. **浏览器后退无效** —— 从「调用日志」按返回，直接离开页面而不是回到上一个 tab。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | tab 是**路径段**而不是 query 参数 | 用 `?tab=calls` 时，深链的 alias 过滤、时间范围等参数全部堆在同一个 query string 上，且「tab 归 tab、筛选归筛选」的边界在代码里消失（切 tab 时要决定哪些 query 保留） | 路径分段后 `/dash/mcp/calls?alias=iwiki&from=...` 的语义无歧义 |
| H2 | 旧路径 `/dash/mcp` 必须继续能打开 | 生产上已有书签与习惯；直接 404 或白屏是不可接受的退化 | 重定向到角色默认 tab，与现状 `useState(isAdmin ? ...)` 一致 |
| H3 | 权限判定保持在组件内，不搬到路由 | 搬到路由要在 `App.tsx` 里注入 `useAuth`，且「非 admin 访问 /dash/mcp/servers」的正确行为是**重定向到 mine**，不是 403 空白页 | 组件内 `<Navigate to="../mine" replace />` |
| H4 | BDD 的 `tab "servers"` 类步骤可以保持语义，只改实现 | 若步骤必须重写，三条现有场景（`mcp.feature:73`、`:175`、`:180`）会一起失效，回归面变大 | 步骤实现改为「断言 URL 段 + 断言 tab 可见」，文本不变 |

---

## 2. 边界

**做**：
- `/dash/mcp` 改为父路由 + 子路由：
  ```
  /dash/mcp            → 重定向到角色默认（admin: servers，其它: mine）
  /dash/mcp/servers    → Server 管理（非 admin 重定向到 mine）
  /dash/mcp/mine       → 我的凭据
  /dash/mcp/calls      → 调用日志（支持 ?alias=&tool_name=&status=&from=&to=&page=）
  /dash/mcp/rejections → 认证拒绝（Stage 150，admin only）
  ```
- `/dash/playground` 同样：
  ```
  /dash/playground     → 重定向到 /dash/playground/chat
  /dash/playground/chat
  /dash/playground/mcp
  ```
- tab 的 `onValueChange` 改为 `navigate(...)`；`Tabs value` 从 `useLocation().pathname` 的最后一段得出
- 行内「调用日志」按钮改为 `navigate('/dash/mcp/calls?alias=' + encodeURIComponent(s.alias))`
- `CallLogTab` 的 alias / 工具名 / 状态 / 时间 / 页码从 URL query 读写（`useSearchParams`），**这样深链才真的可分享**
- BDD 步骤实现改为读 URL，文本不变；新增「刷新后仍在同一 tab」「深链带 alias 直接过滤」两条场景

**不做**（明确排除）：
- **不改其他页面的 tab**（Models / Router Settings 等有自己的 state tab；本 Stage 只动点名的那两个页面，避免一次改 6 个页面的回归面）
- **不做 URL 状态的全局同步方案**（不引 `nuqs` 之类的新依赖）
- **不改侧边栏**（`/dash/mcp` 仍是唯一入口）
- **不做浏览器历史的粒度优化**（切 tab 用 `replace` 还是 `push`：用 `push`，后退回到上一个 tab 是期望行为；`CallLogTab` 内部筛选变更用 `replace`，否则每敲一个字符都进历史）

---

## 3. 方案

### 3.1 路由形状

`App.tsx`：

```tsx
<Route path="mcp" element={<McpPage />}>
  <Route index element={<McpIndexRedirect />} />
  <Route path="servers" element={<ServersTab />} />
  <Route path="mine" element={<MyCredentialsTab />} />
  <Route path="calls" element={<CallLogTab />} />
  <Route path="rejections" element={<RejectionsTab />} />
</Route>
<Route path="playground" element={<PlaygroundPage />}>
  <Route index element={<Navigate to="chat" replace />} />
  <Route path="chat" element={<ChatPanel />} />
  <Route path="mcp" element={<McpPanel />} />
</Route>
```

`McpPage` 退化为 layout：渲染 `TabsList` + `<Outlet />`。

**`index.tsx` 的体积问题**：现在 700+ 行，Server 管理 tab 的表格/对话框全在同一个文件。拆出 `ServersTab.tsx` 是本 Stage 的**必要**动作（不是顺手重构）：父组件要变成 layout，内部 state（`serverDialogOpen`、`tokenServer`、`toolsServer` 等 10 个）都属于 Server tab，留在 layout 里就成了「layout 持有子 tab 的 state」的反模式。

`MyCredentialsTab.tsx` / `CallLogTab.tsx` 已是独立文件，直接当 route element。

### 3.2 `CallLogTab` 的 URL 状态

现在这四个筛选是 `useState`。改成 `useSearchParams`：

```tsx
const [params, setParams] = useSearchParams();
const alias = params.get("alias") ?? "";
const status = params.get("status") ?? "";
const from = params.get("from") ?? presetRange("24h").from;
...
const update = (patch: Record<string, string | null>) =>
  setParams(prev => { /* 合并 + 重置 page=1（若改了筛选） */ }, { replace: true });
```

- **默认值不进 URL**：`24h` 是默认，URL 里没有 `from` 就按 24h 算；否则每个链接都带一串默认参数。
- **`page` 留在 URL**：翻页后刷新应停在同一页。
- **`replace: true`**：见 §2 的最后一条。

### 3.3 权限重定向

`ServersTab` 与 `RejectionsTab` 首行：

```tsx
if (!isAdmin) return <Navigate to="../mine" replace />;
```

`McpIndexRedirect`：

```tsx
return <Navigate to={isAdmin ? "servers" : "mine"} replace />;
```

均在 `RequireAuth` 之内，`useAuth()` 已就绪（`isLoading` 为真时 `RequireAuth` 返回 null，所以 `userRole` 一定是已解析的值）。

`TabsTrigger` 从 `<button>` 变为 `<Link>` 包裹或 `onClick={() => navigate(...)}`：保留 `TabsTrigger` 的样式与 `data-testid`（BDD 依赖 `mcp-tab-servers` 等），只换行为。

### 3.4 Playground

`PlaygroundPage` 同样拆 layout + 两个 panel。`McpPanel.tsx` 已是独立文件；chat 部分在 `index.tsx` 内，需要拆出（它的 state 很多——会话、消息、参数面板——拆分时要保证这些 state 留在 `ChatPanel` 而不是 layout，否则切走再切回会丢会话）。

---

## 4. 测试

### 4.1 前端 BDD（≥6 场景 × 3 viewport）

`mcp.feature`：

1. `/dash/mcp` 对 admin → URL 变为 `/dash/mcp/servers`
2. `/dash/mcp` 对非 admin → `/dash/mcp/mine`
3. 非 admin 直接访问 `/dash/mcp/servers` → 重定向到 `/dash/mcp/mine`（不白屏、不 403）
4. 深链 `/dash/mcp/calls?alias=iwiki` → 表格已按 alias 过滤（**不经任何点击**）
5. 在调用日志 tab 按 F5（`page.reload()`）→ 仍在 `/dash/mcp/calls`
6. 行内「调用日志」按钮 → URL 变为 `/dash/mcp/calls?alias=<该 server>`
7. 从 servers 切到 calls 再按后退 → 回到 servers

`playground.feature`（新增或补充）：

8. `/dash/playground` → `/dash/playground/chat`；切到 mcp → `/dash/playground/mcp`；刷新后仍在 mcp

现有三条 tab 场景（`mcp.feature:73`、`:175`、`:180`）的文本不变，步骤实现改读 URL。

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-frontend/src/App.tsx` | 两条父路由 + 子路由 |
| `crates/aigw-frontend/src/pages/mcp/index.tsx` | 退化为 layout（TabsList + Outlet + 角色重定向） |
| `crates/aigw-frontend/src/pages/mcp/ServersTab.tsx` | 新增（从 index.tsx 拆出） |
| `crates/aigw-frontend/src/pages/mcp/CallLogTab.tsx` | 四个筛选改 `useSearchParams`；`replace: true` |
| `crates/aigw-frontend/src/pages/mcp/MyCredentialsTab.tsx` | 基本不动（去掉父组件传的 props） |
| `crates/aigw-frontend/src/pages/playground/index.tsx` | 退化为 layout；chat 部分拆出 `ChatPanel.tsx` |
| `crates/aigw-frontend/tests/steps/mcp.steps.ts` 等 | tab 步骤改读 URL |
| `crates/aigw-frontend/tests/features/{mcp,playground}.feature` | 新增场景 |

---

## 6. 回归验证

- `task fe-bdd` / `task fe-lint` / `task fe-build`
- `task test`（前端改动不影响 Rust，但门禁以全量为准）
- `task fmt` / `task lint` / `task doctor`
- 手工：生产 `/dash/mcp` 各子路径逐个打开；320px 下 tab 不溢出

---

## 7. 门禁

- [x] 五个 `/dash/mcp/*` 路径与两个 `/dash/playground/*` 路径都能直接打开（刷新不丢）—— `App.tsx:101` 的父路由下挂 `servers`/`mine`/`calls`/`rejections` 四个子路由，`App.tsx:83` 的 playground 父路由下挂 `chat`/`mcp`；BDD `Each tab has its own address` 与 `The MCP tab has its own address and survives a reload` 都夹了 `I reload the page`
- [x] 非 admin 访问 admin-only 子路径得到重定向，不是空白页 —— `mcp/index.tsx:17`：`tabs` 按角色取 `ADMIN_TABS` / `USER_TABS`，`!tabs.includes(tab)` 即 `<Navigate to="/dash/mcp/{mine|servers}" replace />`；BDD `A non-admin deep link to an admin-only tab lands somewhere they can read` 断言落到 `/dash/mcp/mine`
- [x] `/dash/mcp/calls?alias=x` 深链直接生效（不经点击）—— `CallLogTab` 的 `readFilters` 从 `useSearchParams` 读五个筛选；BDD `A deep link applies its filter without any clicking`（深链后 2 条，不等用户点）
- [x] 行内按钮产生的 URL 可分享（复制到新标签页得到同一过滤）—— `ServersTab` 的日志按钮 `navigate(\`/dash/mcp/calls?alias=${alias}\`)`；BDD `The row's log button produces a shareable URL` 断言整条 URL
- [x] 后退能回到上一个 tab —— tab 切换走 `navigate`（push），筛选走 `setParams(..., { replace: true })`；BDD `Going back returns to the previous tab`
- [x] 320px 下 tab 与筛选行不横向溢出 —— `TabsList` 为 `h-auto min-h-9 max-w-full flex-wrap`（`mcp/index.tsx:33`）；BDD `the MCP tabs and call log filters fit inside the screen` 在 320px 视口跑
- [x] `task fe-bdd` 全量通过（含迁移后的三条既有 tab 场景）—— 数字见 §7.1

### 7.1 门禁证据（最终树复跑）

- `task fe-bdd`：**702 passed / 3 skipped / 0 failed / 0 flaky（27.7m）**，EXIT=0（`/tmp/aigw-fe-bdd-152c.log`）。含 `playground.feature` 的三条 Stage 151 新场景（bare 路径落 chat、mcp tab 刷新仍在、切回 chat 会话还在）与 `mcp.feature` 的四条（bare 路径落 servers、每 tab 有地址、深链生效、行内按钮 URL、后退）。上一次跑（`/tmp/aigw-fe-bdd-152b.log`）有 1 条 `websearch.feature:31` flaky，这次整轮零 flaky。
- `task fe-lint`：EXIT=0（`/tmp/aigw-fe-lint-152.log`；`oxlint` 只剩 `ChatPanel.tsx` 两条既有 `react-hooks/exhaustive-deps` warning，`tsc -b` 通过）。
- `task fe-build`：EXIT=0（`/tmp/aigw-fe-build-152.log`）。
- `task test`（全量，门禁只认这个）：**1716 passed / 0 failed / 13 ignored**，见 `stage-152.md` §7.1 —— 本 Stage 只改前端，与 Stage 152 同一棵最终树。
- `task fmt` / `task lint` / `task doctor`：见 `stage-152.md` §7.1，同一棵树。

---

## 8. 风险与遗留

- **拆 `index.tsx` 是本次最大的回归面** —— 700+ 行的状态提升/下沉若做错，表现为「切 tab 后对话框还开着」或「切回来数据丢了」。缓解：state 明确归属到各自 tab 组件，layout 只持有角色与当前 tab。
- **Playground 的 chat state 拆分更危险** —— 会话与消息如果提到 layout，切到 mcp 再切回来会**保留会话**（这听起来不错），但如果留在 `ChatPanel` 且组件因路由切换被卸载，就会**丢失会话**。必须二选一并在 BDD 里钉死。**已决策：状态留在 `ChatPanel`，由于消息本就持久化在 `sessionStorage`，切 tab 不会丢会话**，见 §9。
- **URL 里的 `from`/`to` 是 RFC 3339 字符串** —— 用户手改时格式非法会被后端 400。前端已在 `fromDatetimeLocalValue` 里归一化，但 URL 是可任意的，需在读取时校验并回落默认值。
- **旧书签 `/dash/mcp` 无 query** —— 重定向后 query 丢失。现状本来也没有 query，无退化。

---

## 9. Playground 的 chat state 归属（开工前决策）

**决策：状态留在 `ChatPanel`，不上提到 layout。**

理由不是「卸载即丢可接受」，而是**卸载后并不会丢**：`index.tsx:583` 的 `messages` 与 `settings` 初值都来自 `loadFromStorage`（`messages` 落 `sessionStorage`、`settings` 落 `localStorage`，`:620`/`:624` 两个 effect 每次变更即写）。组件卸载只清掉 React state，重挂载时初值再读一次存储 —— 会话、消息、附件条（`aigw-playground-pending-images`）都回来。

于是三种做法里它最省：

| 做法 | 会话跨 tab | 代价 |
|---|---|---|
| state 留在 `ChatPanel` | **保留**（经 sessionStorage） | 零改动 |
| 上提到 layout | 保留（在内存里） | ChatPanel 的十几个 state 与其中对 `sending`/`abortRef` 的引用要一路传 props，且切走时进行中的请求仍在跑但界面不可见 |
| 卸载即丢 + 提示 | 丢失 | 要新增文案与一条 BDD，且丢的是用户真的在写的输入（`input` **不**持久化，只有已发送的 `messages` 持久化） |

**代价（必须写进 BDD 钉死）**：`input` 草稿与 `sending` 中的请求在切 tab 时丢 —— 前者是既有行为（草稿本来就不持久化），后者切回来时看不到结果（但 `messages` 里没有半条记录，因为只有成功响应才 append）。BDD 场景加入 `playground.feature`：切到 mcp 再切回 chat，`messages` 条数不变、内容一致。
