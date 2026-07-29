# Stage 89: 四表软删除 — 前端（删除确认 + 已删除视图 + E2E）

**Phase**: 35 — Core Entity Soft-Delete
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 6h
**前置**: Stage 88（后端归档表 + API 全部就绪）
**参考**: 现有 `keys/index.tsx` 删除确认 Dialog 模式；`models/DeleteConfirm.tsx` 可复用确认组件

---

## 核心预期

**五个管理页面（keys/teams/users/orgs/models）统一增强删除确认弹窗 + 搜索行右侧 Active/Deleted Toggle 切换归档视图。Playwright BDD 覆盖删除 → 已删除的完整交互流。**

---

## 背景

后端 Stage 88 已将四表的 DELETE 操作从硬删除改为 tombstone-then-delete，并提供了 `GET /{entity}/deleted` 归档列表 API。前端需要：
1. 让用户感知删除行为变更为"可追溯"
2. 提供查看已删除记录的入口

现状：`keys/index.tsx` 已有较完善的删除确认 Dialog，`models/DeleteConfirm.tsx` 是可复用组件。teams/users/orgs 页面虽有删除按钮但确认流程不统一。

---

## 实现

### ① 删除确认增强

**统一模式**（适用于全部五个管理页面）：

```
删除按钮 → <DeleteConfirm> Dialog:
  - 标题: "Delete {Entity}"
  - 正文: "确定要删除 {name} 吗？删除后可在'已删除'中查看历史记录。"
  - 取消 / 删除按钮（删除中显示 Spinner）
→ 调用 DELETE API
→ toast.success("{Entity} deleted")
→ queryClient.invalidateQueries 刷新列表
```

**复用策略**：`models/DeleteConfirm.tsx` 已存在且完整，抽到 `components/ui/` 作为通用 `DeleteConfirmDialog`，props 传入 title/description/onConfirm。或者各页面内联实现（模式简单，不到 20 行）。

teams/users/orgs 页面当前缺少确认弹窗的补充。models/keys 已具备，只需微调文案（加上「可在已删除中查看」）。

---

### ② 已删除视图（搜索行右侧 Toggle）

**为什么不用 Tabs：**
- models 页面已有 Model Groups / Credentials / Health 三个 Tab，再嵌套一层 Active/Deleted Tab 会形成**两层 Tab 地狱**，UX 极差
- Active/Deleted 本质是**列表过滤/视图切换**，不是页面导航，Toggle 语义更准确
- 统一模式：五个管理页面搜索行结构一致，都在搜索框所在行最右侧放置切换按钮

**方案**：复用现有 `<ToggleGroup>` 或两个 `<Button variant="outline/ghost">` 做成 Segmented Control，放在搜索行最右侧：

```
models:    [🔍 Search...              ] [Active | Deleted] [Add Model]
teams:     [🔍 Search...              ] [Active | Deleted]
keys:      [🔍 Search...              ] [Active | Deleted]
users:     [🔍 Search...              ] [Active | Deleted]
orgs:      [🔍 Search...              ] [Active | Deleted]
```

- "Active" 选中：展示现有列表（代码完全不动）
- "Deleted" 选中：调用 `apiGet('/{entity}/deleted')` → 只读归档表格，隐藏搜索框和新增按钮

**单个 `<ToggleGroup>` 实现示例**（项目已依赖 Radix ToggleGroup，`components/ui/toggle-group.tsx` 已封装）：

```tsx
const [viewMode, setViewMode] = useState<"active" | "deleted">("active");

// 搜索行
<div className="flex items-center gap-2">
  {viewMode === "active" && (
    <div className="relative flex-1 max-w-sm">
      <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
      <Input placeholder="Search..." value={search} onChange={...} className="pl-9" />
    </div>
  )}
  <div className="flex-1" /> {/* spacer */}
  <ToggleGroup type="single" value={viewMode} onValueChange={(v) => v && setViewMode(v as typeof viewMode)}>
    <ToggleGroupItem value="active" aria-label="Active">Active</ToggleGroupItem>
    <ToggleGroupItem value="deleted" aria-label="Deleted">Deleted</ToggleGroupItem>
  </ToggleGroup>
  {viewMode === "active" && <Button size="sm" onClick={handleAdd}><Plus /> Add</Button>}
</div>
```

**归档表格设计**：

```
┌────────────┬──────────┬──────────┬──────────────────┐
│ Name/Alias │ ID       │ ...      │ Deleted At       │
├────────────┼──────────┼──────────┼──────────────────┤
│ my-team    │ tm_123   │ ...      │ 2026-07-28 14:30 │
│ old-user   │ usr_456  │ ...      │ 2026-07-27 09:15 │
└────────────┴──────────┴──────────┴──────────────────┘
```

每列映射：

| 实体 | 展示列 |
|------|--------|
| Teams | team_alias / team_id / organization_id / spend / deleted_at |
| Users | user_alias (or user_email) / user_id / team_id / spend / deleted_at |
| Orgs | organization_alias / organization_id / spend / deleted_at |
| Models | model_name / model_id / litellm_params.provider / deleted_at |

只读展示，不提供编辑/恢复按钮（恢复功能留到后续 Phase）。空态文案：「暂无已删除记录。」

---

### ③ 实现细节

**models 页面特殊处理**：

models 的搜索行和 Active/Deleted 切换**仅在 model-groups Tab 内生效**：
- 搜索行和 Toggle 放在 `<TabsContent value="model-groups">` 内部
- 切换到 Deleted 时隐藏搜索框和 Add 按钮，替换为归档表格
- Credentials / Health Tab 完全不受影响

**新增 query hooks**（每页面一个）：

```tsx
// 示例: teams
const { data: deletedTeams = [], isLoading } = useQuery({
  queryKey: ["deleted-teams"],
  queryFn: () => apiGet<DeletedTeam[]>("/team/deleted"),
});
```

**Toggle 状态管理**：

```tsx
const [viewMode, setViewMode] = useState<"active" | "deleted">("active");

// 列表区
{viewMode === "active" ? (
  <>{/* 现有列表（代码不动） */}</>
) : (
  <Card>
    <CardHeader><CardTitle>Deleted Records ({deletedItems.length})</CardTitle></CardHeader>
    <CardContent>
      {/* 归档只读表格 */}
    </CardContent>
  </Card>
)}
```

---

### ④ E2E 验证

Playwright BDD 场景（新增 `.feature` 文件，或嵌入现有 step 文件）：

```
Feature: Soft Delete — Deleted View
  Scenario: Delete a team and verify it appears in Deleted view
    Given I am logged in as admin
    And a team "test-team" exists
    When I click delete on team "test-team"
    And I confirm the deletion
    Then I should see a success toast "Team deleted"
    When I toggle to the "Deleted" view
    Then I should see "test-team" in the deleted teams table
    And the deleted_at column should show a timestamp

  Scenario: Delete a non-existent entity is idempotent
    Given I am logged in as admin
    When I call DELETE /team/delete for "non-existent-team"
    Then the response should be 200 OK

  Scenario: Delete same entity twice is idempotent
    Given I am logged in as admin
    And a team "double-delete-test" exists
    When I delete team "double-delete-test"
    And I delete team "double-delete-test" again
    Then both deletions should succeed
    And the Deleted view should show "double-delete-test" exactly once
```

---

## 涉及文件

| # | 文件 | 操作 |
|---|------|------|
| 1 | `crates/aigw-frontend/src/pages/teams/index.tsx` | 删除确认 Dialog + 已删除 Tab |
| 2 | `crates/aigw-frontend/src/pages/users/index.tsx` | 删除确认 Dialog + 已删除 Tab |
| 3 | `crates/aigw-frontend/src/pages/orgs/index.tsx` | 删除确认 Dialog + 已删除 Tab |
| 4 | `crates/aigw-frontend/src/pages/models/index.tsx` | 已删除 Tab（复用 `DeleteConfirm.tsx`） |
| 5 | `crates/aigw-frontend/src/pages/keys/index.tsx` | 已删除 Tab + 文案微调 |
| 6 | Playwright BDD 场景文件 | 新增 E2E 场景 |

---

## 门禁

- `npm run build` 无错误
- `npm run lint` 无警告
- Playwright BDD 场景全部通过
- 五个管理页面均可切换"已删除"Tab，归档记录正确展示
