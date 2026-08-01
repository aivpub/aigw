# Stage 91: UI 多语言基础设施 — i18n 框架 + 浏览器持久化

**Phase**: 38 — UI 多语言 i18n 支持（中文 + English）
**优先级**: P1
**状态**: ✅ 完成
**预估**: 12h
**前置**: 无（纯前端改动，零后端变更）

---

## 核心预期

1. **i18n 框架安装与配置**：`react-i18next` + `i18next` + `i18next-browser-languagedetector` 三件套，配置命名空间翻译、浏览器语言检测（localStorage → navigator.language → 'en'）。

2. **翻译文件骨架**：`src/i18n/locales/zh-CN.json` + `src/i18n/locales/en.json`，按页面/模块命名空间划分（common/sidebar/dashboard/keys/models/users/orgs/teams/spend-logs/jobs/playground/router-settings/usage/health/login），初始化所有 key 为英文原文。

3. **浏览器语言持久化**：`i18next-browser-languagedetector` 两级 fallback：localStorage `aigw-language` → `navigator.language` → 硬编码 `'en'`。用户切换语言时自动写入 localStorage 持久化。

4. **基础组件改造**：Shell（侧边栏 + Header）+ Login 页面作为首批改造目标，验证框架落实。

---

## 背景

当前 aigw 前端所有 UI 文本硬编码为英文，无任何 i18n 框架、翻译文件或语言切换机制。项目使用 React 19 + TypeScript + Vite + Tailwind CSS v4 + Radix UI primitives，UI 组件为自建 `shadcn/ui` 风格。

调研确认 litellm **无任何多语言支持**（`<html lang="en">` 硬编码，零 i18n 依赖，零翻译文件），本次是 net-new 能力。

**管理员配置默认语言**推迟到后续 Phase（需要时在 Router Settings 页加一个下拉即可），当前 Stage 通过浏览器语言自动检测：

- 中文浏览器用户首次访问 → 自动显示中文
- 英文浏览器用户首次访问 → 自动显示英文
- 用户在 Header 切换语言后 → localStorage 持久化，后续访问优先

---

## 设计

### 1. 技术选型

| 库 | 用途 |
|---|---|
| `i18next` ^25.x | 核心 i18n 框架：翻译存储、插值、复数、格式化 |
| `react-i18next` ^16.x | React 绑定：`useTranslation()` hook、`<Trans>` 组件 |
| `i18next-browser-languagedetector` ^8.x | 浏览器语言检测：localStorage → navigator → backend |

**选型理由**：i18next 是 React 生态事实标准（React 官方文档推荐），支持命名空间懒加载、ICU MessageFormat、TypeScript 类型安全，社区成熟度高。不选 `react-intl`（FormatJS）因为其包体积更大且 i18next 在 Tailwind/shadcn 项目中更常见。

### 2. 目录结构

```
crates/aigw-frontend/src/i18n/
├── index.ts              # i18next 初始化 + 配置
├── locales/
│   ├── zh-CN.json        # 中文翻译（命名空间扁平或嵌套结构）
│   └── en.json           # 英文翻译
└── types.ts              # 翻译 key 的 TypeScript 类型定义（可选，增强 DX）
```

### 3. 翻译文件结构（命名空间设计）

不拆分多文件（初期 2 语言 × 多个小文件维护成本高），单 JSON 按命名空间组织：

```json
{
  "common": {
    "save": "Save",
    "cancel": "Cancel",
    "delete": "Delete",
    "create": "Create",
    "edit": "Edit",
    "search": "Search",
    "loading": "Loading...",
    "noResults": "No results found",
    "confirm": "Confirm",
    "close": "Close",
    "back": "Back"
  },
  "sidebar": {
    "usage": "Usage",
    "keys": "API Keys",
    "models": "Models",
    "users": "Users",
    "organizations": "Organizations",
    "teams": "Teams",
    "spendLogs": "Spend Logs",
    "playground": "Playground",
    "jobs": "Jobs",
    "settings": "Settings",
    "routerSettings": "Router Settings"
  },
  ...
}
```

中文对应：

```json
{
  "common": {
    "save": "保存",
    "cancel": "取消",
    "delete": "删除",
    "create": "创建",
    "edit": "编辑",
    "search": "搜索",
    "loading": "加载中...",
    "noResults": "暂无数据",
    "confirm": "确认",
    "close": "关闭",
    "back": "返回"
  },
  "sidebar": {
    "usage": "用量",
    "keys": "API 密钥",
    "models": "模型管理",
    ...
  }
}
```

**关键决策**：JSON key 使用英文 camelCase（而非中文拼音或数字 ID），可读性好、IDE 补全、类型安全。

### 4. i18next 初始化（`src/i18n/index.ts`）

```typescript
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import LanguageDetector from 'i18next-browser-languagedetector';
import en from './locales/en.json';
import zhCN from './locales/zh-CN.json';

// 同步初始化，不阻塞渲染
i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      'zh-CN': { translation: zhCN },
    },
    fallbackLng: 'en',
    defaultNS: 'translation',
    detection: {
      order: ['localStorage', 'navigator'],
      lookupLocalStorage: 'aigw-language',
      caches: ['localStorage'],
    },
    interpolation: {
      escapeValue: false, // React already escapes
    },
    returnObjects: true, // Allow nested key access
  });

export default i18n;
```

**关键决策**：
- 同步初始化，无 `await`，React 立即渲染——零闪烁
- 语言检测顺序：localStorage（用户显式选择）→ `navigator.language`（浏览器语言）→ `'en'`（硬编码兜底）
- `escapeValue: false` — React 已做 XSS 防护
- `returnObjects: true` — 支持 `t('sidebar')` 返回整个命名空间对象

### 5. App.tsx 集成

```typescript
// main.tsx 入口 — 直接渲染，无需异步等待
import '@/i18n'; // side-effect import, i18next already initialized

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
```

### 6. 浏览器持久化流程

```
用户首次访问（中文浏览器，zh-CN）
  → i18next 检测 localStorage["aigw-language"] → 无
  → 检测 navigator.language → "zh-CN"
  → 加载 zh-CN 翻译
  → 渲染中文 UI（同步完成，无闪烁）

用户首次访问（英文浏览器，en-US）
  → i18next 检测 localStorage["aigw-language"] → 无
  → 检测 navigator.language → "en-US" → 只匹配前两位 "en"
  → 加载 en 翻译
  → 渲染英文 UI

用户在 Header 切换语言为 "English"
  → i18next.changeLanguage("en")
  → 自动写入 localStorage["aigw-language"] = "en"（detection.caches 配置）
  → UI 即时切换到英文
  → 刷新后从 localStorage 读取，保持英文

用户清除浏览器数据
  → localStorage 丢失 → 回到首次访问流程
```

### 7. 首批改造组件

本 Stage 仅改造以下组件验证框架正确性：

| 组件 | 改造内容 |
|------|----------|
| `Shell` / `Sidebar` | 侧边栏菜单项文本 |
| `Header` | 标题/面包屑（不含语言切换器） |
| `LoginPage` | 登录表单所有文本 |

其他页面文本替换留给 Stage 92。

---

## TDD

### 前端 BDD（Playwright）

- `i18n.feature` 3 场景：
  1. **中文浏览器首次访问**：不设 localStorage，`navigator.language=zh-CN` → 断言菜单文本为中文
  2. **英文浏览器首次访问**：不设 localStorage，`navigator.language=en-US` → 断言菜单文本为英文
  3. **localStorage 优先于浏览器语言**：设置 `aigw-language=en`，`navigator.language=zh-CN` → 断言英文

---

## 实施步骤

| 步骤 | 内容 | 预估 |
|------|------|------|
| 1 | 安装依赖：`i18next` `react-i18next` `i18next-browser-languagedetector` | 0.5h |
| 2 | 创建 `src/i18n/index.ts` 同步初始化 + `zh-CN.json`/`en.json` 骨架 | 2h |
| 3 | `main.tsx` 集成：`import '@/i18n'` side-effect import，直接渲染 | 0.5h |
| 4 | 改造 Sidebar + LoginPage 组件：硬编码 → `t('key')` | 3h |
| 5 | BDD 3 场景编写（Playwright `navigator.language` 通过 `browser.newContext({ locale })` 设置） | 3h |
| 6 | 验收：BDD 3/3 + `tsc -b` 零错误 + 手动验证双语切换 | 3h |

---

## 验收门禁

- [ ] 前端 BDD i18n 3 场景全绿（3 viewports）
- [ ] `tsc -b`（TypeScript noEmit）零错误
- [ ] 手动验证：中文浏览器首次访问 → 菜单/登录页显示中文
- [ ] 手动验证：英文浏览器首次访问 → 菜单/登录页显示英文
- [ ] 手动验证：DevTools → Application → Local Storage → 修改 `aigw-language` → 刷新 → 语言切换

---

## 非目标

- 不翻译后端 API 错误消息（只影响前端 UI 文本）
- 不翻译 BDD 测试框架文本
- 不翻译日志输出
- 不支持 RTL 语言（阿拉伯语等）
- 不做全量页面翻译（留给 Stage 92）
- 不做语言切换器 UI 组件（留给 Stage 93）
- 不做管理员配置默认语言（推迟到后续 Phase，需要时在 Router Settings 页加下拉）

---

## 关键决策

| # | 决策 | 理由 |
|---|------|------|
| 1 | 选 i18next 而非 FormatJS | React 生态事实标准，社区大，Tailwind/shadcn 项目常用 |
| 2 | 单 JSON 文件命名空间而非多文件拆分 | 初期 2 语言 × 少量文本，多文件维护成本 > 收益 |
| 3 | JSON key 用英文 camelCase | 可读性好、IDE 补全、类型安全 |
| 4 | `returnObjects: true` | 支持 `t('common')` 获取整个命名空间，减少重复前缀 |
| 5 | 同步初始化，不阻塞渲染 | 零闪烁；语言检测在 `<1ms` 内完成 |
| 6 | 检测链：localStorage → navigator → 'en' | 用户选择 > 浏览器语言 > 英文兜底，覆盖 95%+ 场景 |
| 7 | 翻译文件全部 bundle（不懒加载） | 初期文本量小（< 500 keys），懒加载增加复杂度无实际收益 |
| 8 | 放弃管理员配置默认语言 | litellm 也没有；浏览器语言检测已覆盖首次访问场景，后续需要时再加 |
