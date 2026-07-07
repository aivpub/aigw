# Stage 20: 健康检查增强

**创建日期**: 2026-07-08
**状态**: ⏳ 待开始
**优先级**: P2
**前置条件**: Stage 18 完成
**预估**: 1-2h

---

## 1. 目标

新增 `/health/metrics` 端点，提供 DB 连接池状态、uptime、key 数量等运维指标。

---

## 2. 交付

### 2.1 `/health/metrics` 端点

```json
{
  "status": "healthy",
  "uptime_seconds": 86400,
  "db": {
    "connected": true,
    "pool_size": 5,
    "idle": 2
  },
  "counts": {
    "virtual_keys": 42,
    "proxy_models": 12,
    "organizations": 3,
    "teams": 8,
    "users": 25
  },
  "version": "0.1.0"
}
```

### 2.2 实现要点

- server 启动时记录 `std::time::Instant` 作为启动时间
- 通过 `sqlx::Pool::size()` 和 `available()` 获取连接池信息
- 每个 `counts` 字段通过 `SELECT COUNT(*)` 获取
- 仅管理员可访问（master_key 鉴权）
- 保留现有 `/health` 轻量端点不变

---

## 3. 门禁

- `/health/metrics` 返回 200 + JSON
- `uptime_seconds` 单调递增
- `db.pool_size` 与实际连接池配置一致
- 普通 key 访问返回 403
