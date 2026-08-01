# Stage 80: Admin API + Col Chunk Cache + 存量归档

**Phase**: 30 — Body Archive 冷存储
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 12h
**前置**: Stage 79

---

## 背景

Stage 78/79 完成了 AsyncTask + Engine 框架和 Body Archive 读写链路。本阶段补全运维能力。

## 目标

1. 通用 Admin API：`POST /admin/jobs/trigger`、`GET /admin/jobs`、`GET /admin/jobs/stats`、`GET /admin/jobs/{id}`、`GET /admin/jobs/{id}/logs`
2. Body Archive 特有查询：`GET /admin/archive/stats`
3. Col Chunk 缓存（可选）：FS LFU
4. 存量归档：通过 API 触发
5. 权限：admin role

### API 端点

```
POST   /admin/jobs/trigger      手动创建 Job（step_type + payload）
GET    /admin/jobs               所有 Job（按 step_type/status 过滤）
GET    /admin/jobs/stats         引擎统计（每 step_type loop 数 + queue）
GET    /admin/jobs/{job_id}      Job 详情（Steps + result）
GET    /admin/jobs/{job_id}/logs 执行日志（level 过滤 + 分页）
GET    /admin/archive/stats      body_archive 专属统计
```

## 验收标准

### 通用 Admin API

- [ ] `POST /admin/jobs/trigger {step_type, payload}` → `{job_id, status, total_steps}`
- [ ] 无 admin → 401；未知 step_type → 404；不支持手动 → 400
- [ ] `GET /admin/jobs` 按 step_type/status 过滤
- [ ] `GET /admin/jobs/stats` → loops + queue
- [ ] `GET /admin/jobs/{id}` → Steps + result（completed 有值, running null）
- [ ] `GET /admin/jobs/{id}/logs` → level 过滤 + 分页

### Col Chunk 缓存

- [ ] mode=fs → LFU、容量/单条目限制、命中跳过存储、满驱逐、restore
- [ ] mode=none → 无缓存

### Body Archive 统计

- [ ] `GET /admin/archive/stats` → 实时查 spend_logs

### 存量归档

- [ ] `POST /admin/jobs/trigger {step_type:"body_archive", payload:{start_date, end_date}}` → Job + Steps → Engine 接管

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-core/src/engine.rs` | 修改：list_jobs、job_detail、job_logs、stats |
| `crates/aigw-core/src/body_archive/cache.rs` | 修改：ColChunkCache |
| `crates/aigw-core/src/body_archive/query.rs` | 修改：集成 col chunk 缓存 |
| `crates/aigw-server/src/routes/jobs.rs` | 新增：通用 admin job handler |
| `crates/aigw-server/src/routes/archive.rs` | 新增：archive stats |

## 技术方案

### POST /admin/jobs/trigger

接收 `{step_type, payload}` → 调用注册的 AsyncTask 的 `steps_from_payload()` → INSERT Job + Steps → 返回 job_id。Engine exec loop 自动接管。

### GET /admin/jobs/stats

查询 async_job_steps 聚合：

```json
{
  "body_archive": {
    "loops": { "allocated": 2, "cluster_estimate": 6 },
    "queue": { "pending": 3, "running": 2, "stale": 0,
               "completed_24h": 48, "failed_24h": 1 }
  }
}
```

### GET /admin/jobs/{job_id}

返回 Steps + result + summary。summary 按 step_type 分派 builder 聚合 result JSONB。通用透传不解析。

### Col Chunk 缓存

FS LFU，目录 `/data/aigw/cache/parquet_chunks/`。Key: `{path}:{rg}:{column}`。

## 测试要求

- trigger_job：无 admin→401、正常→job_id、未知 step_type→404、不支持→400
- list_jobs：过滤
- stats：loops + queue
- job_detail：completed result 有值、running null
- job_logs：level 过滤 + 分页
- archive_stats：数值正确
- ColChunkCache：put/get、LFU、restore

## 依赖

Stage 79（query_parquet_with_cache 就绪）+ Stage 78（Engine 就绪）

## 不做

- 日 compaction
- Prometheus 指标
- Parquet 文件浏览器
