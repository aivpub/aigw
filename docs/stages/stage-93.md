# Stage 93: 语言切换器 + E2E 验收 + 文档收尾

**Phase**: 38 — UI 多语言 i18n 支持（中文 + English）
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 10h
**前置**: Stage 91（框架 + 后端端点）+ Stage 92（全量翻译）

---

## 核心预期

1. **语言切换器 UI**：Header 增加语言下拉选择器（中文/English），切换即时生效，支持键盘无障碍。

2. **全栈端到端验证**：本地启动前端 + 后端，验证中英切换在真实环境下的端到端行为。

3. **Playwright BDD 双语验收**：编写语言切换 + 多语言 UI 验收的 BDD 场景，覆盖桌面/平板/移动端。

4. **文档收尾**：更新 roadmap、next-steps、技术债，登记 ADR，将原 Phase 37 budget reset 及其 stage 文档重命名保存。

---

## 背景

Stage 91 完成了 i18n 框架和后端语言端点，Stage 92 完成了全量页面翻译。本 Stage 提供用户切换语言的能力 + 端到端验收 + 文档闭环，完成 UI 多语言支持的全部交付。

---

## 设计

### 1. 语言切换器（`src/components/layout/Header.tsx`）

**位置**：Header 右侧，在 dark mode toggle 旁边。

**设计**：

```tsx
import { useTranslation } from 'react-i18next';
import { Languages } from 'lucide-react';

function LanguageSwitcher() {
  const { i18n } = useTranslation();
  const currentLang = i18n.language?.startsWith('zh') ? 'zh-CN' : 'en';

  const toggleLanguage = () => {
    const next = currentLang === 'zh-CN' ? 'en' : 'zh-CN';
    i18n.changeLanguage(next);
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" aria-label={t('header.switchLanguage')}>
          <Languages className="h-4 w-4" />
          <span className="ml-1.5">{currentLang === 'zh-CN' ? '中文' : 'EN'}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onClick={() => i18n.changeLanguage('zh-CN')}>
          🇨🇳 中文 {currentLang === 'zh-CN' && '✓'}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => i18n.changeLanguage('en')}>
          🇺🇸 English {currentLang === 'en' && '✓'}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
```

**行为**：
- 点击 → `i18n.changeLanguage(lang)` → i18next 自动写入 `localStorage["aigw-language"]`（Stage 91 配置的 `detection.caches`）
- 当下高亮 ✓
- Lucide `Languages` 图标
- 桌面端显示 "EN"/"中文" 文本，移动端仅图标（节省空间）

### 2. HTML lang 属性同步

语言切换时同步 `<html lang>` 属性，确保无障碍和 SEO：

```typescript
// src/i18n/index.ts 补充
i18n.on('languageChanged', (lng) => {
  document.documentElement.lang = lng;
});
```

### 3. Tailwind/Radix 无障碍

语言下拉使用已有的 `DropdownMenu` 组件（`components/ui/dropdown-menu.tsx`，基于 `@radix-ui/react-dropdown-menu`），天然支持键盘导航（↑↓ Enter Escape）。

---

## TDD

### Playwright BDD（5 场景 × 3 viewports）

`i18n-switcher.feature`：

| # | 场景 | 验证点 |
|---|------|--------|
| 1 | 首次访问英文 | `navigator.language=en-US`，后端默认 `en` → 菜单文本为英文 |
| 2 | 首次访问中文 | `navigator.language=zh-CN`，后端默认 `zh-CN` → 菜单/登录页中文 |
| 3 | 语言切换器切换 | 点击 Header 语言下拉 → 选 "English" → UI 即时变英文。菜单文本变 English |
| 4 | localStorage 持久化 | 切换为中文 → 刷新页面 → 仍为中文（localStorage `aigw-language=zh-CN`） |
| 5 | 中文 > 英文后日期格式 | 切换英文后日期显示 `Jul 31, 2026` 而非 `2026年7月31日` |

### 手动验收 checklist

- [ ] 清除 localStorage，首次访问 → 菜单文本英文
- [ ] 把浏览器语言设为中文，清除 localStorage，首次访问 → 菜单文本中文
- [ ] Header 点击 EN → 选 "中文" → 全站即时中文
- [ ] 刷新 → 保持中文（localStorage 持久化）
- [ ] 再切回英文 → 全站即时英文
- [ ] 键盘 Tab 到语言切换器 → Enter → ↓↑ 选语言 → Enter 确认
- [ ] 移动端（375px）→ 语言切换器为图标仅（无文本）

---

## 文档收尾

### 1. `docs/stages/stage-roadmap.md`

- Phase 38 标记 ✅ 完成
- 进度条更新：89/92 → 92/92

### 2. `docs/11-next-steps.md`

- Phase 38 完成总结（3 Stage 工时 + 核心成果）
- 原 Phase 37 budget reset 推后到 Phase 39（Stages 91-93 重编号为 94-96）
- 长期路线新增 `LT-I18n-Advanced`（RTL 支持、多语言 BDD 矩阵）

### 3. `docs/08-autonomous-decisions.md`

新增 ADR-023：UI 多语言 i18n 选型与架构
- 技术选型：i18next + react-i18next（非 FormatJS）
- 翻译策略：命名空间 + 单 JSON 文件
- 后端语言配置：`GeneralSettings.ui_language`
- 持久化：localStorage → navigator → 后端默认 三级 fallback

### 4. `docs/12-technical-debt.md`

新增 TD-008：i18n 后续改进项

| ID | 条目 | 优先级 |
|----|------|--------|
| TD-008a | 翻译文件懒加载（按命名空间按需加载） | P3 |
| TD-008b | TypeScript 类型安全（从 JSON 生成翻译 key 类型） | P3 |
| TD-008c | 后端 API 错误消息多语言 | P3 |
| TD-008d | RTL 语言支持（阿拉伯语等） | P3 |

### 5. 原 Phase 37 文档重命名

```
docs/stages/stage-91.md → docs/stages/stage-94-budget-reset-backend.md
docs/stages/stage-92.md → docs/stages/stage-95-budget-reset-frontend.md
docs/stages/stage-93.md → docs/stages/stage-96-budget-reset-fullstack.md
```

---

## 验收门禁

- [ ] Playwright BDD `i18n-switcher.feature` 5 场景 × 3 viewports 全绿
- [ ] 全量 BDD 回归通过（无 i18n 引入的断链）
- [ ] `tsc -b` 零错误
- [ ] `cargo check` + `cargo test` 全绿
- [ ] 手动验收 checklist 全部通过
- [ ] 文档收尾 4 项完成（roadmap/next-steps/ADR/tech-debt）

---

## 非目标

- 不做语言选择器的动画/过渡
- 不翻译 BDD 测试框架或 Playwright 报告
- 不做 CMS 式的在线编辑翻译
- 不做 BrowserStack/SauceLabs 跨浏览器验证（手动验收即可）
