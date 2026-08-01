# Stage 70: 前端页面修复 — Models / Keys / Users / Spend Logs

**Phase**: 27 — 全栈质量修复 + Usage 页面图表增强
**状态**: ✅ 完成
**预估**: 8h
**完成日期**: 2026-07-22
**依赖**: Stage 69 ✅

---

## 目标

修复 4 个管理页面的 UI 缺陷 + Spend Logs 补全，每个页面独立可测。

| 页面 | 问题 | 目标 |
|------|------|------|
| Models | Provider 不对、无截断、Status 无 toggle | Provider 用 custom_llm_provider + 截断 + Switch |
| API Keys | 无 Expires 列、Status 无 toggle | Expires 列 + Switch |
| Users | 无 User ID 复制、无 Key 数量 | User ID 列 + CopyButton + virtual_keys_count |
| Spend Logs | 无 requester_ip 列 | IP 列 + 复制按钮 |

---

## Part A — Models 页面 (2.5h)

**`crates/aigw-frontend/src/pages/models/index.tsx`**

### 1.1 Provider 修正

```typescript
function extractProvider(params: Record<string, unknown>): string {
  if (typeof params.custom_llm_provider === "string") {
    return params.custom_llm_provider;  // ← 优先使用 custom_llm_provider
  }
  if (typeof params.model === "string") {
    const parts = params.model.split("/");
    return parts.length > 1 ? parts[0] : params.model;  // fallback
  }
  return "—";
}
```

### 1.2 截断

- Model Name: `max-w-[180px] truncate`
- Upstream Model: `max-w-[160px] truncate` + tooltip 显示完整名

### 1.3 Status toggle

Badge + Switch（shadcn/ui）:
- onChange → PUT `/model/update` → model_info.mode: "active" / "inactive"
- 需确认 `/model/update` 端点是否已支持 mode 修改

### 1.4 TDD (2 BDD × 3 viewports)

| # | 场景 |
|---|------|
| 1 | Provider 使用 custom_llm_provider 显示 |
| 2 | Status toggle 切换 → PUT 调用 + 状态变更 |

---

## Part B — API Keys 页面 (2h)

**`crates/aigw-frontend/src/pages/keys/index.tsx`**

### 2.1 Expires 列

```tsx
<TableCell className="text-xs">
  {item.expires ? new Date(item.expires).toLocaleDateString() : "∞"}
</TableCell>
```

列位: Budget → **Expires** → Status

### 2.2 Expires 加入表单

- Create Dialog / Edit Dialog: 新增 `type="date"` Input
- `buildCreateBody()` / `handleEdit()` 传入 `expires`

### 2.3 Status toggle

Badge(blocked/active) + Switch → `POST /block/key` 或 `POST /unblock/key`
（需检查后端是否有这些端点，如无则新增）

### 2.4 TDD (2 BDD × 3 viewports)

| # | 场景 |
|---|------|
| 1 | Expires 列可见（日期或 ∞） |
| 2 | Status toggle 切换 blocked/active |

---

## Part C — Users 页面 (2h)

**`crates/aigw-frontend/src/pages/users/index.tsx`**

### 3.1 User ID 列 + CopyButton

```tsx
<TableCell className="text-xs font-mono flex items-center gap-1">
  <span className="truncate max-w-[100px]">{user.user_id.slice(0, 12)}…</span>
  <CopyButton text={user.user_id} />
</TableCell>
```

### 3.2 Virtual Keys 数量

**后端先行**: **`crates/aigw-server/src/routes/user.rs`** list handler 中加子查询:
```sql
(SELECT COUNT(*) FROM virtual_keys vk WHERE vk.user_id = u.user_id) as virtual_keys_count
```

**前端**: 新增列 + 可点击跳转到 Keys 页面

### 3.3 TDD (2 BDD × 3 viewports)

| # | 场景 |
|---|------|
| 1 | User ID 可见 + 复制按钮可用 |
| 2 | Virtual Keys 数量可见 |

### 3.4 后端 UT (1 个)

验证 `virtual_keys_count` 在 `/user/list` 响应中正确返回

---

## Part D — Spend Logs requester_ip 列 (1.5h)

**`crates/aigw-frontend/src/pages/spend-logs/index.tsx`**

### 4.1 新增 IP 列

```tsx
<TableCell className="text-xs font-mono">
  <span>{log.requester_ip_address ?? "—"}</span>
  {log.requester_ip_address && <CopyButton text={log.requester_ip_address} />}
</TableCell>
```

列位: end_user → **IP** → session_id

移动端卡片同理

### 4.2 TDD (1 BDD × 3 viewports)

| # | 场景 |
|---|------|
| 1 | requester_ip_address 列可见 + 复制 |

---

## 测试汇总

| 层级 | # | 场景 |
|------|---|------|
| 后端 UT | 1 | user/list 返回 virtual_keys_count |
| 后端 BDD | 0 | — |
| 前端 BDD | 7 | Models×2 + Keys×2 + Users×2 + Spend Logs×1 |

7 scenarios × 3 viewports = 21 new frontend tests

---

## 门禁

- [ ] `npm run build` 前端通过
- [ ] `cargo test --workspace` 全量通过
- [ ] 前端 BDD: 108 → 129 tests
- [ ] 手动验收: 4 页面新列/功能
