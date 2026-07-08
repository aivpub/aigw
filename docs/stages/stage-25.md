# Stage 25: 前端 BDD 测试基础设施

**创建日期**: 2026-07-08
**状态**: ⏳ 待开始
**优先级**: P0
**前置条件**: Phase 9 完成（Stage 21-24）
**预估**: 4-6h

---

## 1. 目标

建立前端 BDD 端到端测试体系，覆盖全部现有页面（Login、Dashboard、Keys、Models），包括移动端 viewport，确保后续所有前端 Stage 走 R-G-R 开发循环。

---

## 2. 设计决策

### 2.1 测试框架选型

| 方案 | 描述 | 优点 | 缺点 |
|------|------|------|------|
| A: Playwright + cucumber-rust | 复用后端 BDD 框架，.feature + Rust step defs | 与后端一致，共享 BDD 基础设施 | 较重量，step 实现需 Rust |
| B: Playwright + TypeScript | Playwright test runner + Gherkin plugin | 轻量，前端团队更友好 | 与后端 BDD 框架不一致 |

**选择方案 B**：使用 Playwright 原生 test runner（TypeScript），配合 `playwright-bdd` 插件实现 Gherkin .feature 文件支持。理由：前端测试涉及大量 DOM 交互，TypeScript 的 Playwright API 更自然；通过 Gherkin feature 文件保持 BDD 风格与后端一致；测试执行速度更快。

### 2.2 测试策略

```
层级 1: E2E BDD（Playwright + Gherkin）
  ├── 覆盖所有页面 + 移动端 viewport
  ├── Mock 后端 API（MSW 或 Playwright route interception）
  └── R-G-R 循环：写 .feature → 实现 step defs → 运行（红）→ 前端代码（绿）→ 重构

层级 2: 组件测试（Vitest + Testing Library）
  └── 关键组件（AuthProvider、Sidebar、DataTable）
```

### 2.3 Viewport 覆盖

| Viewport | 设备 | 覆盖范围 |
|----------|------|---------|
| 375x667 | iPhone SE | 所有测试 |
| 768x1024 | iPad | 登录 + Dashboard |
| 1280x720 | Laptop | 所有测试 |

---

## 3. 交付

### 3.1 文件结构

```
crates/aigw-frontend/
  tests/
    features/
      login.feature
      keys.feature
      models.feature
      dashboard.feature
      mobile.feature
    steps/
      login.steps.ts
      keys.steps.ts
      models.steps.ts
      dashboard.steps.ts
      mobile.steps.ts
    fixtures/
      api-mocks.ts      # Playwright route interception 模拟 API
    playwright.config.ts
  src/
    __tests__/           # Vitest 组件测试
      auth-provider.test.tsx
      sidebar.test.tsx
```

### 3.2 核心 Feature 文件

**login.feature**:
```gherkin
Feature: 登录认证
  Scenario: 成功登录并跳转到首页
    Given 未认证用户访问 "/dash/home"
    When 重定向到 "/dash/login"
    And 输入用户名 "admin"
    And 输入密码 "sk-master-change-me"
    And 点击 "Sign In"
    Then 跳转到 "/dash/home"
    And 侧边栏可见

  Scenario: 错误的密码显示错误消息
    Given 在登录页面
    When 输入用户名 "admin"
    And 输入密码 "wrong-key"
    And 点击 "Sign In"
    Then 显示错误消息 "Authentication failed"
    And 停留在登录页面

  Scenario: 已认证用户自动跳转
    Given 已登录（cookie 中有有效 token）
    When 访问 "/dash/login"
    Then 自动跳转到 "/dash/home"
```

**keys.feature**:
```gherkin
Feature: Key 管理
  Scenario: 查看 Key 列表
    Given 已登录管理员
    And API 返回 3 个虚拟 Key
    When 访问 "/dash/keys"
    Then 显示 3 个 Key 条目
    And 每个 Key 显示 alias、key_name、models、max_budget

  Scenario: 创建新 Key 并显示 Token
    Given 已登录管理员
    When 点击 "New Key"
    And 填写 alias "test-key"
    And 填写 models ["gpt-4"]
    And 点击 "Create"
    Then 弹出 Token 显示对话框
    And 显示 sk- 开头的 token
    And 关闭对话框的按钮可见
    When 点击 "Copy & Close"
    Then Token 对话框关闭
    And Key 列表刷新

  Scenario: 搜索 Key
    Given 已登录管理员
    And API 返回 5 个 Key
    When 在搜索框输入 "prod"
    Then 列表过滤为仅匹配项

  Scenario: 删除 Key
    Given 已登录管理员
    When 点击某 Key 的删除按钮
    And 确认删除
    Then Key 从列表消失
```

**mobile.feature**:
```gherkin
Feature: 移动端适配
  Scenario: 移动端侧边栏切换
    Given 移动端 viewport (375x667)
    And 已登录用户访问 "/dash/home"
    Then 侧边栏默认隐藏
    When 点击汉堡菜单按钮
    Then 侧边栏滑出
    When 点击遮罩层
    Then 侧边栏隐藏

  Scenario: 移动端 Key 列表使用卡片布局
    Given 移动端 viewport (375x667)
    And 已登录管理员访问 "/dash/keys"
    Then Key 数据以卡片形式展示（非表格）

  Scenario: 移动端 Dashboard 图表适配
    Given 移动端 viewport (375x667)
    And 已登录管理员访问 "/dash/home"
    Then 图表宽度适配屏幕
    And 统计卡片垂直堆叠
```

### 3.3 API Mock 策略

使用 Playwright `page.route()` 拦截 API 请求并返回 mock 数据：

```typescript
import { defineMockRoutes } from "../fixtures/api-mocks";

// 在每个 scenario 的 Given 步骤中调用
export async function givenAdminLoggedIn(page: Page) {
  await defineMockRoutes(page, { role: "proxy_admin" });
}
```

Mock 数据覆盖的端点：`/key/list`, `/key/generate`, `/key/delete`, `/model/list`, `/spend/*`, `/global/spend`, `/org/list`, `/user/list`（为后续 Stage 预埋）。

### 3.4 失败用例截图 + GIF 录制

为便于人工复查失败用例，Playwright 配置自动截图和 trace：

```typescript
// playwright.config.ts
export default defineConfig({
  use: {
    // 每个失败用例自动截图
    screenshot: "only-on-failure",
    // 保留 trace 以便事后回放
    trace: "retain-on-failure",
    // 录制每一步操作的视频（仅失败时保留，减少磁盘占用）
    video: "retain-on-failure",
  },
  // 截图/trace/video 输出目录
  outputDir: "tests/output/",
});
```

**GIF 生成脚本**（基于截图序列）：

```bash
# tests/scripts/generate-gif.sh
# 将失败用例的截图序列合成为 GIF（需要 ffmpeg）
ffmpeg -framerate 2 -pattern_type glob -i "tests/output/*.png" \
  -vf "scale=640:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse" \
  -loop 0 "tests/output/failure-report.gif"
```

集成到 Taskfile：

```yaml
  fe-bdd:
    desc: "run frontend BDD tests"
    dir: crates/aigw-frontend
    cmds:
      - npx playwright test || (bash tests/scripts/generate-gif.sh && exit 1)
      - echo "fe-bdd passed"

  fe-bdd-report:
    desc: "view Playwright HTML report (with screenshots/trace)"
    dir: crates/aigw-frontend
    cmds:
      - npx playwright show-report
```

**失败时的产出物：**
- `tests/output/*.png` — 每个失败步骤的截图
- `tests/output/*.webm` — 失败用例的完整操作录像
- `tests/output/trace.zip` — Playwright trace（可用 `playwright show-trace` 回放）
- `tests/output/failure-report.gif` — 截图合成的 GIF（可选，需 ffmpeg）

### 3.5 Taskfile 集成

```yaml
  fe-bdd:
    desc: "run frontend BDD tests"
    dir: crates/aigw-frontend
    cmds:
      - npx playwright test
      - echo "fe-bdd passed"

  fe-test:
    desc: "run frontend unit tests"
    dir: crates/aigw-frontend
    cmds:
      - npx vitest run
      - echo "fe-test passed"
```

---

## 4. 门禁

- [ ] `task fe-bdd` 全部 scenario 通过（含移动端 viewport）
- [ ] `task fe-test` 全部 Vitest 组件测试通过
- [ ] Mock API 覆盖所有 4 个页面
- [ ] 3 种 viewport（375/768/1280）全通过
- [ ] Feature 文件文档化在 `tests/features/README.md`
- [ ] 失败用例自动截图 + trace + video（`screenshot: "only-on-failure"`, `trace: "retain-on-failure"`）
- [ ] `task fe-bdd-report` 可查看 HTML 报告
