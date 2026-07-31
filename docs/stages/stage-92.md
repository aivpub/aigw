# Stage 92: 全量页面文本提取与中英翻译

**Phase**: 38 — UI 多语言 i18n 支持（中文 + English）
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 20h
**前置**: Stage 91（i18n 框架就绪 + 翻译文件骨架）

---

## 核心预期

1. **全量页面文本提取**：逐一改造所有页面/组件，将硬编码英文字符串替换为 `t('namespace.key')` 调用。覆盖 13 个页面 + Layout + 通用组件。

2. **中英翻译完整**：`zh-CN.json` 和 `en.json` 补全所有翻译条目，英文翻译为原文本身（作为 key 的默认值），中文翻译由人工校对。

3. **组件级别改造**：所有 UI 组件（`components/ui/` 下的通用组件）保持不侵入，仅在使用侧通过 props 或 wrapper 传翻译文本。

4. **零回归保障**：每个页面改造后跑对应的 Playwright BDD 场景（mock API），确保无断链。

---

## 背景

Stage 91 搭建了 i18n 框架、翻译文件骨架，并验证了 Sidebar + LoginPage 两个组件。本 Stage 完成剩余所有页面的文本提取和翻译，使整个前端 100% 可切换中英文。

---

## 设计

### 1. 改造范围（按页面/组件）

| 模块 | 文件 | 文本量（估） | 改造方式 |
|------|------|-------------|----------|
| **Layout** | | | |
| Sidebar | `components/layout/sidebar.tsx` | ~15 | Stage 91 已完成 |
| Header | `components/layout/header.tsx` | ~5 | `useTranslation()` + `t('header.xxx')` |
| Shell | `components/layout/shell.tsx` | ~2 | 如有文本 |
| **页面** | | | |
| Login | `pages/login.tsx` | ~10 | Stage 91 已完成 |
| Usage / Dashboard | `pages/usage/index.tsx` | ~30 | 图表 label、统计卡片、Tab 标签 |
| API Keys | `pages/keys/index.tsx` | ~40 | 表格列头、按钮、表单、状态文本、确认对话框 |
| Models | `pages/models/index.tsx` | ~50 | 表格、表单、Tab 标签、下拉选项、删除确认 |
| Users | `pages/users/index.tsx` | ~25 | 表格、表单、状态文本 |
| Organizations | `pages/orgs/index.tsx` | ~20 | 表格、表单 |
| Teams | `pages/teams/index.tsx` | ~25 | 表格、表单 |
| Spend Logs | `pages/spend-logs/index.tsx` | ~25 | 表格列头、过滤器标签、抽屉面板 |
| Playground | `pages/playground/index.tsx` | ~20 | 输入框 placeholder、按钮、选择器 label |
| Router Settings | `pages/router-settings/index.tsx` | ~15 | Tab、表单字段 label |
| Jobs | `pages/jobs/index.tsx` + `job-detail.tsx` | ~30 | 表格列头、状态标签、Tab、按钮 |
| Health | `pages/health/index.tsx` | ~10 | 统计卡片 |
| **组件** | | | |
| Log Viewer | `components/log-viewer/*` | ~15 | InputCard、MessageBubble、ResponseViewer 等 label |
| UI Components | `components/ui/*` | 0 | **不改** — 通用组件保持无文本侵入，所有文案由调用方传入 |

### 2. 翻译文件设计

`zh-CN.json` 和 `en.json` 按命名空间组织。每个命名空间是扁平 key（不嵌套过深），key 命名约定：`<模块>.<字段>`（如 `keys.tableHeader.keyAlias`）。

**翻译文件命名空间清单**：

```
common       — 通用（save/cancel/delete/create/edit/search/loading/noResults/confirm/close/back/yes/no/enabled/disabled/active/inactive/all/none/unknown）
sidebar      — 侧边栏（usage/keys/models/users/organizations/teams/spendLogs/playground/jobs/routerSettings/health）
header       — 顶栏（title/search/logout/darkMode/lightMode/language）
login        — 登录页（title/username/password/loginButton/loggingIn/wrongCredentials）
dashboard    — 仪表盘（totalSpend/totalRequests/activeKeys/avgLatency）
usage        — 用量页（tabs/charts/trends）
keys         — API 密钥页（table/actions/form/status/delete）
models       — 模型管理等
health       — 健康检查页
users        — 用户管理页
orgs         — 组织管理页
teams        — 团队管理页
spendLogs    — 花费日志页
playground   — 调试页
jobs         — 任务管理页
routerSettings — 路由设置页
logViewer    — 日志查看器组件
```

### 3. 改造模式（以 API Keys 页为例）

**Before**（硬编码）：
```tsx
<Button>Create Key</Button>
<TableHead>Key Alias</TableHead>
<Badge>{status === 'active' ? 'Active' : 'Inactive'}</Badge>
```

**After**（i18n）：
```tsx
import { useTranslation } from 'react-i18next';

function KeysPage() {
  const { t } = useTranslation();

  return (
    <>
      <Button>{t('keys.createKey')}</Button>
      <TableHead>{t('keys.table.keyAlias')}</TableHead>
      <Badge>{status === 'active' ? t('common.active') : t('common.inactive')}</Badge>
    </>
  );
}
```

### 4. 特殊场景处理

| 场景 | 处理方式 |
|------|----------|
| 拼接文本 | `t('jobs.rowsArchived', { count: 42 })` → `"已归档 42 行"` |
| 带链接的文本 | `<Trans i18nKey="keys.copyWarning">...</Trans>` |
| 日期格式化 | 用 `date-fns` 已有的 `format()` + `locale` 参数（`zhCN` from `date-fns/locale`） |
| 数字格式化 | 用 JS 原生 `toLocaleString()` 已支持，无需改动 |
| 图表 label | Recharts 的 `label` prop 传 `t('usage.dailySpend')` |
| Toast/Sonner 通知 | `toast.success(t('common.saved'))` |
| 表单校验错误 | zod schema 的 `.min(1, t('common.required'))` — **注意**：zod error message 在 render 时翻译，不在 schema 定义时（否则语言切换不生效） |

### 5. date-fns 本地化

`date-fns` 已安装，日期格式化需配合语言切换动态加载 locale：

```typescript
import { enUS, zhCN } from 'date-fns/locale';
import { format } from 'date-fns';

const localeMap = { en: enUS, 'zh-CN': zhCN };

function useDateFormat(date: Date, fmt: string) {
  const { i18n } = useTranslation();
  return format(date, fmt, { locale: localeMap[i18n.language] || enUS });
}
```

---

## TDD

每个页面改造完成后跑对应 BDD 场景：

| 页面 | BDD feature | 场景数 | Viewports |
|------|-------------|--------|-----------|
| Keys | `keys.feature` | ~8 | 3 |
| Models | `models.feature` | ~8 | 3 |
| Users | `management.feature` (users) | ~6 | 3 |
| Orgs | `management.feature` (orgs) | ~4 | 3 |
| Teams | `management.feature` (teams) | ~4 | 3 |
| Spend Logs | `spend-logs.feature` | ~6 | 3 |
| Usage | `dashboard.feature` | ~8 | 3 |
| Jobs | `jobs.feature` | ~10 | 3 |
| Playground | `playground.feature` | ~5 | 3 |
| Router Settings | `management.feature` (router) | ~3 | 3 |
| Health | `management.feature` (health) | ~3 | 3 |

新增 BDD 场景 `i18n-full-translation.feature` 3 场景：
1. 中文语言下所有页面关键文本为中文（选取每个页面 2-3 个关键字符串验证）
2. 英文语言下所有页面关键文本为英文
3. 中文下 number/date 格式符合中文习惯

### 翻译完整性检测

编写脚本 `scripts/check-i18n-keys.sh`：
1. `jq` 遍历 `en.json` 所有 key
2. 检测 `zh-CN.json` 是否有缺失 key
3. 检测 `zh-CN.json` 是否有英文 key 多余的 key（可能拼写错误）

```
npm run check:i18n  # TS 类型检查 + JSON key 对齐检查
```

---

## 实施步骤

| 步骤 | 内容 | 预估 |
|------|------|------|
| 1 | 补全 `en.json` 所有命名空间 + 所有 key（英文=原文）— 先行驱动 | 2h |
| 2 | `zh-CN.json` 中文翻译（全部条目） | 3h |
| 3 | Common 通用组件层：所有页面引用的 `cn()`/`toast` 全局改写 | 1h |
| 4 | Layout 改造（Header） | 0.5h |
| 5 | 页面改造：Login → Dashboard → Keys → Models | 4h |
| 6 | 页面改造：Users → Orgs → Teams → Spend Logs | 3h |
| 7 | 页面改造：Playground → Router Settings → Jobs → Health | 3h |
| 8 | Log Viewer 组件改造 | 1h |
| 9 | `check-i18n-keys.sh` 检测脚本 | 0.5h |
| 10 | BDD 回归 + `i18n-full-translation.feature` 3 场景 + 翻译完整性检测通过 | 2h |

---

## 验收门禁

- [ ] `node scripts/check-i18n-keys.sh` 输出 `OK: all keys aligned`
- [ ] `tsc -b` 零错误
- [ ] 全量 Playwright BDD 回归通过（mock API，3 viewports）
- [ ] `i18n-full-translation.feature` 3 场景（中英文本 + 日期数字格式）全绿
- [ ] 手动验证：所有页面切换中/英 → 菜单/按钮/表格/表单/提示文本正确
- [ ] 手动验证：数字格式（千分位）、日期格式在两种语言下符合习惯

---

## 非目标

- 不翻译后端 API 响应中的英文错误消息
- 不提供 BDD 场景级别的双语断言（每个 scenario 只验证一种语言）
- 不处理 emoji/特殊字符（中文已覆盖）
- 不做语言切换的动画/过渡效果

---

## 关键决策

| # | 决策 | 理由 |
|---|------|------|
| 1 | UI 通用组件不改文本 | 保持组件纯净，通过调用方传 props 控制文案，符合单一职责 |
| 2 | zod schema 不在定义时做 `t()` | 语言切换后 schema 缓存的 message 不会更新，render 时翻译更安全 |
| 3 | date-fns locale 动态切换 | `useTranslation()` 的 `i18n.language` 配合 `date-fns/locale/*` 实现 |
| 4 | 不拆分多 JSON 文件（懒加载） | 翻译总量 < 500 key，打包在一个 bundle 成本可忽略；懒加载增加复杂度 |
| 5 | 英文 JSON key 即为英文翻译 | 避免重复维护，英文翻译直接用 key 本身（英文作为 source of truth） |
