# Stage 28: Key 创建 UX 修复

**创建日期**: 2026-07-08
**状态**: ⏳ 待开始
**优先级**: P0（Bug 修复）
**前置条件**: Stage 25（BDD 基础设施）
**预估**: 1-2h

---

## 1. 目标

修复 Key 创建流程中的交互 Bug：创建成功后应先展示 Token 供用户复制保存，关闭对话框后再刷新列表。

---

## 2. 当前 Bug

`src/pages/keys/index.tsx` 中 `createMutation.onSuccess`：

```typescript
onSuccess: (resp) => {
  queryClient.invalidateQueries({ queryKey: ["virtual-keys"] });
  setCreateOpen(false);       // BUG: 先关对话框
  setGeneratedToken(resp.key ?? null);  // Token 已经无法看到了
  toast.success("Key created successfully");
},
```

问题：`setCreateOpen(false)` 在 `setGeneratedToken(...)` 之前执行。关闭对话框后用户永远看不到生成的 Token — 而这个 Token 只在创建时返回一次。

---

## 3. 设计方案

### 3.1 修正后的交互流程

```
点击 "Create Key"
  → API 返回 { key: "sk-xxx", ... }
  → 显示 Token 展示对话框（独立于创建表单）
  → 用户可见 sk-xxx、可一键复制
  → 用户点击 "I've saved my key" 确认
  → 关闭 Token 对话框
  → 刷新 Key 列表
```

### 3.2 Token 展示对话框

```tsx
// New component: TokenRevealDialog
function TokenRevealDialog({ token, open, onClose }: Props) {
  const [copied, setCopied] = useState(false);

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Save Your API Key</DialogTitle>
          <DialogDescription className="text-destructive font-medium">
            This key will only be shown once. Copy it now and store it securely.
          </DialogDescription>
        </DialogHeader>
        <div className="flex items-center gap-2 rounded-md bg-muted p-3">
          <code className="flex-1 break-all text-sm">{token}</code>
          <Button size="icon" variant="ghost" onClick={() => {
            navigator.clipboard.writeText(token);
            setCopied(true);
          }}>
            {copied ? <Check className="h-4 w-4 text-green-500" /> : <Copy className="h-4 w-4" />}
          </Button>
        </div>
        <Button onClick={onClose} className="w-full">
          I've saved my key
        </Button>
      </DialogContent>
    </Dialog>
  );
}
```

### 3.3 修正后的 onSuccess

```typescript
onSuccess: (resp) => {
  // 先展示 Token 对话框
  setGeneratedToken(resp.key ?? null);
  // 关闭创建表单对话框（不是 Token 对话框）
  setCreateOpen(false);
  // 不立即刷新列表 — Token 对话框还在
  toast.success("Key created. Please save your API key.");
},
```

Token 对话框关闭时再刷新列表：

```typescript
function handleTokenDialogClose() {
  setGeneratedToken(null);
  queryClient.invalidateQueries({ queryKey: ["virtual-keys"] });
}
```

---

## 4. 交付

### 4.1 文件修改

| 文件 | 改动 |
|------|------|
| `src/pages/keys/index.tsx` | 修复 `onSuccess` 逻辑；添加 TokenRevealDialog |
| `src/components/ui/token-reveal.tsx` | [NEW] Token 展示+复制组件 |
| `tests/features/keys.feature` | 添加 "创建 Key 后展示 Token" scenario |

### 4.2 组件功能

- Token 展示框：`monospace` 字体，`break-all`，深色背景
- 复制按钮：点击后图标从 `Copy` 变为 `Check`（2 秒后恢复）
- 安全提示：红色警告文字 "only shown once"
- 关闭按钮文字："I've saved my key"（明确用户操作意图）

---

## 5. 门禁

- [ ] 创建 Key 后弹出 Token 展示对话框
- [ ] Token 可一键复制，复制后有视觉确认
- [ ] 关闭 Token 对话框后列表自动刷新
- [ ] [R-G-R] keys.feature "创建新 Key 并显示 Token" scenario 通过
- [ ] 关闭 Token 对话框后无法再次查看 Token
