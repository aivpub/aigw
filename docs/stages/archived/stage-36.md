# Stage 36: 前端 Spend Logs 重构

**Phase**: 13 — 前端反馈改进
**状态**: ✅ 完成
**预估**: 5h

---

## 目标

重构 Spend Logs 页面到接近 litellm 体验：Live Tail 实时刷新、时间预设快捷选择、分页导航、请求详情抽屉、增强列表列。

## 验收标准

- [ ] 时间预设按钮组：15 分钟 / 4 小时 / 24 小时 / 7 天 / 自定义
- [ ] 选择"自定义"时展开 start/end date picker
- [ ] 默认选中 24 小时
- [ ] Live Tail 开关（Toggle Switch），开启后 `refetchInterval: 15_000`
- [ ] Live Tail 仅对 page=1 生效
- [ ] 绿色 banner "Auto-refreshing every 15 seconds" + Stop 按钮
- [ ] Live Tail 状态写入 `sessionStorage`
- [ ] Fetch 手动刷新按钮（独立于 Live Tail）
- [ ] Request ID 搜索输入框（300ms debounce）
- [ ] 分页组件："Showing X-Y of Z" + "Page N of M" + Previous/Next
- [ ] Page size selector (30/50/100)
- [ ] 表格列：Time, Type (call_type badge), Status, Session ID, Request ID (copyable), TTFT, Duration, Key Name, Model, Tokens, Cost
- [ ] 点击行 → 右侧 Sheet/Drawer 弹出详情
- [ ] 详情抽屉展示：Request ID, Status, Model, Tokens breakdown, Cost, TTFT, Duration, Start/End Time, Messages JSON, Tags, API Key, User, Team, Org
- [ ] 移动端卡片布局
- [ ] Loading / Empty / Error 三态
- [ ] BDD：time presets, Live Tail toggle, pagination, request_id search, drawer, mobile

## 关键文件

| 文件 | 操作 |
|------|------|
| `src/pages/spend-logs/index.tsx` | 重写 |
| `src/components/spend-logs/` (可选) | 新建子组件目录 |

## 组件结构

```
SpendLogsPage
├── TimePresetBar (预设按钮组 + 自定义日期)
├── Toolbar (Request ID search + Live Tail switch + Fetch btn)
├── PaginationBar (page size + prev/next + 统计)
├── SpendLogsTable
│   ├── Desktop: Table columns (Time/Type/Status/SessionID/RequestID/TTFT/Duration/Key/Model/Tokens/Cost)
│   └── Mobile: Card list
└── LogDetailDrawer (Sheet/Drawer — 右侧弹出)
    ├── BasicInfo: Request ID, Status, Model, Cost, TTFT, Duration
    ├── Timestamps: Start Time, End Time, Completion Start Time
    ├── MessagesView: request/response JSON
    └── MetaInfo: API Key, User, Team, Org, Tags
```

## Table Columns

| 列 | 数据来源 | 格式 |
|----|---------|------|
| Time | `start_time` | `MM-dd HH:mm:ss` |
| Type | `call_type` | Badge (chat/embedding/image/…) |
| Status | `status` | Badge (success=green, failure=red) |
| Session ID | `session_id` | 截断 8 字符 + copy |
| Request ID | `request_id` | 截断 8 字符 + copy |
| TTFT | `ttft_ms` | `123.4ms` 或 `—` (null) |
| Duration | `request_duration_ms` | `1.2s` |
| Key Name | `api_key` | 前 8 字符 |
| Model | `model` | 文本 |
| Tokens | `prompt_tokens`+`completion_tokens` | `1.2K / 3.4K` |
| Cost | `spend` | `$0.0123` |

## 依赖

- Stage 34（后端 streaming + 分页 + TTFT + request_id 过滤）

## 风险

- 抽屉组件状态管理复杂（需要在表格行点击和抽屉间同步数据）
- 移动端卡片布局需要适配所有增强列的信息密度
- Live Tail 和分页的交互：翻到其他页时自动关 Live Tail
