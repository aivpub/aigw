# Stage 91: UI 多语言基础设施 — i18n 框架 + 后端语言配置 + 浏览器持久化

**Phase**: 38 — UI 多语言 i18n 支持（中文 + English）
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 16h
**前置**: 无（独立前端 + 后端配置改动，无代码交集）

---

## 核心预期

1. **i18n 框架安装与配置**：`react-i18next` + `i18next` + `i18next-browser-languagedetector` 三件套，配置命名空间翻译、语言检测（localStorage → 浏览器语言 → 后端默认）、回退策略。

2. **翻译文件骨架**：`src/i18n/locales/zh-CN.json` + `src/i18n/locales/en.json`，按页面/模块命名空间划分（common/sidebar/dashboard/keys/models/users/orgs/teams/spend-logs/jobs/playground/router-settings/usage/health/login），初始化所有 key 为英文原文。

3. **后端默认语言配置**：`GeneralSettings` 新增 `ui_language` 字段（默认 `"en"`）+ `GET /api/v1/settings/language` 公开端点（无需鉴权，返回当前配置的默认语言）。

4. **浏览器语言持久化**：`i18next-browser-languagedetector` 三级 fallback：localStorage `aigw-language` → `navigator.language` → 后端 `GET /settings/language` 默认值。语言切换时同时写入 localStorage 持久化。

5. **基础组件改造**：Shell（侧边栏 + Header）+ Login 页面作为首批改造目标，验证框架落实。

---

## 背景

当前 aigw 前端所有 UI 文本硬编码为英文（详见 `docs/research/2026-07-31-frontend-i18n-audit.md`），无任何 i18n 框架、翻译文件或语言切换机制。项目使用 React 19 + TypeScript + Vite + Tailwind CSS v4 + Radix UI primitives，UI 组件为自建 `shadcn/ui` 风格。

本次新增中英双语支持，遵循三步走：

1. **Stage 91**（本 Stage）：搭框架 — 安装依赖、配置 i18next、建翻译文件骨架、后端语言端点、浏览器持久化。
2. **Stage 92**：全量文本提取与翻译 — 逐页面把所有硬编码字符串替换为 `t('key')` 调用，中英翻译。
3. **Stage 93**：语言切换器 + E2E 验收 — Header 增加语言下拉、Playwright BDD 双语场景覆盖、文档收尾。

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

const DEFAULT_LANGUAGE = 'en';

async function fetchDefaultLanguage(): Promise<string> {
  try {
    const resp = await fetch('/api/v1/settings/language');
    if (resp.ok) {
      const { language } = await resp.json();
      return language || DEFAULT_LANGUAGE;
    }
  } catch {}
  return DEFAULT_LANGUAGE;
}

export async function initI18n() {
  const backendDefault = await fetchDefaultLanguage();

  await i18n
    .use(LanguageDetector)
    .use(initReactI18next)
    .init({
      resources: {
        en: { translation: en },
        'zh-CN': { translation: zhCN },
      },
      fallbackLng: backendDefault,
      defaultNS: 'translation',
      detection: {
        order: ['localStorage', 'navigator', 'htmlTag'],
        lookupLocalStorage: 'aigw-language',
        caches: ['localStorage'],
      },
      interpolation: {
        escapeValue: false, // React already escapes
      },
      returnObjects: true, // Allow nested key access
    });

  return i18n;
}
```

**关键决策**：
- 语言检测顺序：localStorage（用户显式选择）→ 浏览器语言 → 后端默认（fallbackLng）
- 后端默认语言在 i18n 初始化前异步获取，作为 `fallbackLng`
- `escapeValue: false` — React 已做 XSS 防护
- `returnObjects: true` — 支持 `t('sidebar')` 返回整个命名空间对象

### 5. App.tsx 集成

```typescript
// main.tsx / App.tsx 入口
import { initI18n } from '@/i18n';

// 在渲染前初始化
initI18n().then(() => {
  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  );
});
```

### 6. 后端语言配置端点（`crates/aigw-core/src/config.rs`）

`GeneralSettings` 新增字段：

```rust
/// Default UI language for the frontend.
/// Supported: "en", "zh-CN". Defaults to "en".
#[serde(rename = "ui_language", skip_serializing_if = "Option::is_none")]
pub ui_language: Option<String>,
```

核心原因：SaaS 部署可为不同实例配置不同默认语言（中国区默认中文，海外区默认英文），是「后台配置默认语言」需求的落地点。

**端点**：`GET /api/v1/settings/language`（无需鉴权，在 auth middleware 白名单中）

```rust
// crates/aigw-server/src/routes/settings.rs 新建（或放在现有 routes 中）
pub async fn get_language(State(state): State<AppState>) -> Json<serde_json::Value> {
    let lang = state.config.general_settings
        .as_ref()
        .and_then(|gs| gs.ui_language.as_deref())
        .unwrap_or("en");
    Json(serde_json::json!({ "language": lang }))
}
```

路由注册（`main.rs`）：
```rust
.route("/api/v1/settings/language", get(get_language))
```

### 7. 浏览器持久化流程

```
用户首次访问
  → i18next 检测 localStorage["aigw-language"] → 无
  → 检测 navigator.language → 如 "zh-CN"
  → 加载 zh-CN 翻译
  → 渲染中文 UI

用户在 Header 切换语言为 "English"
  → i18next.changeLanguage("en")
  → 自动写入 localStorage["aigw-language"] = "en"（detection.caches 配置）
  → UI 即时切换到英文
  → 刷新后从 localStorage 读取，保持英文

用户清除浏览器数据
  → 回到首次访问流程
```

### 8. 首批改造组件

本 Stage 仅改造以下组件验证框架正确性：

| 组件 | 改造内容 |
|------|----------|
| `Shell` / `Sidebar` | 侧边栏菜单项文本 |
| `Header` | 标题/面包屑（不含语言切换器） |
| `LoginPage` | 登录表单所有文本 |

其他页面文本替换留给 Stage 92。

---

## TDD

### 前端 BDD（Playwright + mock API）

- `i18n.feature` 4 场景：
  1. **英文默认**：mock `GET /settings/language` 返回 `en`，断言菜单文本为英文
  2. **中文默认**：mock `GET /settings/language` 返回 `zh-CN`，断言菜单文本为中文
  3. **浏览器语言 > 后端默认**：不设 localStorage，`navigator.language=zh-CN`，后端默认 `en` → 断言中文
  4. **localStorage 持久化**：设置 `aigw-language=en`，后端默认 `zh-CN` → 断言英文

### 后端 UT

- `get_language` 端点 3 个测试：
  1. 配置了 `ui_language: "zh-CN"` → 返回 `{"language": "zh-CN"}`
  2. 未配置 → 返回 `{"language": "en"}`（默认值）
  3. 公开端点，无 token 可访问

---

## 实施步骤

| 步骤 | 内容 | 预估 |
|------|------|------|
| 1 | 安装依赖：`i18next` `react-i18next` `i18next-browser-languagedetector` | 0.5h |
| 2 | 创建 `src/i18n/` 目录结构 + `index.ts` 初始化逻辑 | 1.5h |
| 3 | 创建 `zh-CN.json` + `en.json` 骨架（common + sidebar + login 命名空间，其他留空） | 2h |
| 4 | `App.tsx` 集成：`initI18n` 异步初始化 → 渲染 | 1h |
| 5 | 后端 `ui_language` 配置 + `GET /settings/language` 端点 + UT | 3h |
| 6 | 改造 Sidebar + LoginPage 组件：硬编码 → `t('key')` | 3h |
| 7 | BDD 4 场景编写 + mock API `GET /settings/language` | 3h |
| 8 | 验收：UT 全绿 + BDD 4/4 + `cargo check` + `tsc -b` 零错误 | 2h |

---

## 验收门禁

- [ ] 前端 BDD i18n 4 场景全绿（3 viewports）
- [ ] 后端 UT `get_language` 3 测试全绿
- [ ] `cargo test -p aigw-core` + `cargo test -p aigw-server` 无回归
- [ ] `cargo check` 零错误
- [ ] `tsc -b`（TypeScript noEmit）零错误
- [ ] 手动验证：访问前端 → 菜单/登录页文本显示正确
- [ ] 手动验证：浏览器 DevTools → Application → Local Storage → 修改 `aigw-language` → 刷新 → 语言切换

---

## 非目标

- 不翻译后端 API 错误消息（只影响前端 UI 文本）
- 不翻译 BDD 测试框架文本
- 不翻译日志输出
- 不支持 RTL 语言（阿拉伯语等）
- 不做全量页面翻译（留给 Stage 92）
- 不做语言切换器 UI 组件（留给 Stage 93）

---

## 关键决策

| # | 决策 | 理由 |
|---|------|------|
| 1 | 选 i18next 而非 FormatJS | React 生态事实标准，社区大，Tailwind/shadcn 项目常用 |
| 2 | 单 JSON 文件命名空间而非多文件拆分 | 初期 2 语言 × 少量文本，多文件维护成本 > 收益；命名空间逻辑隔离足够 |
| 3 | JSON key 用英文 camelCase | 可读性好、IDE 补全、类型安全，避免中文 key 编码问题 |
| 4 | `returnObjects: true` | 支持 `t('common')` 获取整个命名空间，减少重复前缀 |
| 5 | 后端端点 `GET /settings/language` 公开 | 语言配置是 UI 层面信息，无需鉴权；鉴权判断应在 auth middleware 做 |
| 6 | `fallbackLng` 用后端配置值 | 满足「后台配置默认语言」需求，语言检测链的最终兜底 |
| 7 | 后端 `ui_language` 字段而非独立配置表 | 简单场景，`GeneralSettings` 即可；不增加新 migration |
| 8 | 翻译文件全部 bundle（不懒加载） | 初期文本量小（< 500 keys），懒加载增加复杂度无实际收益 |
