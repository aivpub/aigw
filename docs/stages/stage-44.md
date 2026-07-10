# Stage 44: Models 页面 Cost 列

**Phase**: 15 — 第二轮反馈改进
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 2h

---

## 目标

在 Models 页面表格中新增一列 Cost，展示 Input / Output Per Million Tokens 定价信息。

## 验收标准

- [ ] 表格新增 Cost 列，渲染为两行：Input Cost（上）、Output Cost（下）
- [ ] 单位为 Per 1M Tokens（`× 1,000,000`），格式化为 `$x.xxxx`
- [ ] 定价缺失或为 0 时显示 "—"
- [ ] 移动端卡片布局同步展示 Cost 信息
- [ ] BDD：Models 页面可见 Cost 列及定价值

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-frontend/src/pages/models/index.tsx` | 修改：新增 Cost 列渲染 |

## 数据来源

从 `model_info` JSON 字段读取（litellm 标准定价位置）：

```typescript
interface ModelItem {
  model_info: Record<string, unknown>;  // { input_cost_per_token, output_cost_per_token }
}
```

## 呈现格式

```
列头: Cost (Per 1M)
行内容:
  $4.xxxx Input
  $2.xxxx Output
```

计算: `cost_per_token × 1_000_000`，保留精度至 4 位小数 + `$` 前缀。

无定价或值为 0 时显示 `—`（emo dash）。

## 依赖

- 无（独立前端改动）

## 风险

- `model_info` 可能不包含定价字段 → 已处理（fallback 为 "—"）
- 移动端信息密度增加 → card 布局添加一行额外显示
