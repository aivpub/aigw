# Stage 37: Users/Orgs 端到端修复 + Provider 解密

**Phase**: 13 — 前端反馈改进
**状态**: ⏳ 待开始
**预估**: 4.5h

---

## 目标

修复 3 个问题并完成前端适配：Orgs 列表不返回新建组织、Users 列表缺少分页、Provider 字段解密失败。

## 验收标准

### 后端
- [ ] `/org/list` 正确返回新建的 organization
- [ ] `/user/list` 支持 `page` / `page_size` 分页（默认 page=1, page_size=10）
- [ ] `/user/list` 响应包含 `total_count, page, page_size, total_pages`
- [ ] `/spend/providers` 所有 provider 字段正确解密

### 前端
- [ ] 新建 org 后列表正确出现新条目
- [ ] Users 列表分页控件：Previous/Next + Page N of M + page size selector (10/25/50)
- [ ] BDD：org create+list, user list pagination, provider decryption

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/org.rs` | 调查 org list bug |
| `crates/aigw-server/src/routes/user.rs` | 新增分页参数 + 响应格式 |
| `crates/aigw-server/src/routes/spend.rs` | provider 解密修复 |
| `crates/aigw-core/src/db.rs` | 新增 user list 分页查询 + provider 解密调试 |
| `src/pages/users/index.tsx` | 添加分页控件 + 更新 API 调用 |
| `src/pages/orgs/index.tsx` | 验证/修复新建后的刷新 |

## 改动细节

### 1. Orgs List Bug 调查与修复

**可能根因**：
- `organization_id` 使用 `REPLACE(UUID, '-', '')` 生成（无连字符），与预期 UUID 格式不一致
- 数据库事务未提交或 query client 缓存
- 前端 query invalidation 问题

**步骤**：
1. 检查 `org_new()` handler 中的 SQL INSERT，验证返回值
2. 确认 `org_list()` SQL 查询无 WHERE 过滤条件（当前是无条件 `SELECT * ORDER BY`）
3. 若后端无 bug，检查前端 `queryClient.invalidateQueries` 在 create 后是否正确触发

### 2. Users List 分页（后端 + 前端）

**后端 — Query params**:
```rust
pub struct UserListQuery {
    pub organization_id: Option<String>,  // 已有
    pub page: Option<i32>,                // 新增，默认 1
    pub page_size: Option<i32>,           // 新增，默认 10
}
```

**后端 — DB 层**:
```sql
SELECT COUNT(*) as total FROM users [WHERE organization_id = ?]
SELECT ... FROM users [WHERE org_id = ?] ORDER BY user_alias LIMIT ? OFFSET ?
```

**后端 — 响应格式**:
```json
{
  "data": [ ...users ],
  "total_count": 150,
  "page": 1,
  "page_size": 10,
  "total_pages": 15
}
```

**前端 — Users 页面**:
```typescript
// 新增 state
const [page, setPage] = useState(1);
const [pageSize, setPageSize] = useState(10);

// useQuery key 变更
queryKey: ["users", page, pageSize]

// API 调用变更
apiGet(`/user/list?page=${page}&page_size=${pageSize}`)

// 响应接口更新
interface UserListResponse {
  data: UserItem[];
  total_count: number;
  page: number;
  page_size: number;
  total_pages: number;
}
```

**前端 — 分页控件**:
- "Showing X-Y of Z results" + "Page N of M"
- Previous / Next 按钮（到达边界 disabled）
- Page size selector (10/25/50)

### 3. Provider 解密修复

当前流程：
1. SQL: `JSON_EXTRACT(pm.litellm_params, '$.model')` JOIN `spend_logs`
2. 若 `litellm_params` 是加密 blob → `JSON_EXTRACT` 返回 NULL → fallback 到 `sl.model`
3. `build_decrypted_provider_map()` 在内存中解密后覆盖

可能的失败原因：
- `aigw_master_key` 未设置 → 无法解密 → 返回空 map
- `list_models()` 返回的 `litellm_params` 格式异常

修复：
1. 添加 `aigw_master_key` 未设置时的 WARN 日志
2. 增强 `build_decrypted_provider_map()` 的 plaintext JSON fallback 逻辑
3. 解密失败时跳过该条而非 panic

## 依赖

- 无（独立改动）

## 风险

- Orgs bug 可能是 DB 写入问题而非查询问题 — 需要实际复现
- Provider 解密修复后需验证三种场景（master_key 存在/不存在/格式异常）
