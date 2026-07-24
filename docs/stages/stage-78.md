# Stage 78: Body Archive — AsyncTask + Engine 框架 + 写链路

**Phase**: 30 — Body Archive 冷存储
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 14h
**前置**: 无（独立）

---

## 背景

当前 aigw 只有 1 个周期性后台任务（`DailySpendQueue`，裸 `tokio::spawn`），没有任何多副本协调机制。body_archive 是第一个需要"多副本安全争抢 Step"的异步工作负载。

本 Stage 建立最小化异步任务框架（1 个 trait + Engine），并将 Body Archive 接进去。后续 budget_reset 只需新增一个 AsyncTask impl，框架零改动。

详见：`docs/plans/2026-07-22-body-archive-s3-parquet.md`

---

## 概念模型

### 5 个名字

```
主动实体 (运行时, 干活)        被动实体 (DB 记录, 被操作)
─────────────────────       ─────────────────────
AsyncTask trait             Job    (async_jobs 表)
Engine (宿主运行时)          Step   (async_job_steps 表)
                            Log    (async_job_logs 表)
```

| 名字 | 类型 | 做什么 | 数量 |
|------|------|--------|:---:|
| **AsyncTask** | trait | 一个异步任务类型。`tick()` 发现新工作，`execute()` 执行 Step，`finalize()` 收尾 Job | 每种工作类型 1 个 |
| **Engine** | 实体 | 宿主。spawn tick loop + exec loop + cleanup loop | 每个 Replica 1 个 |
| **Job** | DB `async_jobs` | 一次执行。trigger_type = cron 或 manual | N |
| **Step** | DB `async_job_steps` | 最小执行单元。pending → running → completed/failed | N |
| **Log** | DB `async_job_logs` | 执行日志 | N |

### 层级

```
AsyncTask.tick()                    POST /admin/jobs/trigger
    │                                     │
    ▼                                     ▼
  Step 列表                            Step 列表
    │                                     │
    └──────────────┬──────────────────────┘
                   ▼
           INSERT Job + Steps (pending)
                   │
                   ▼
   ┌──── Engine exec loop ────────────────┐
   │  claim pending Step                 │
   │  → AsyncTask.execute(step)           │
   │  → 写 result, mark completed         │
   │  → 下一个 pending Step              │
   │                                      │
   │  检测到 Job 所有 Step 完成            │
   │  → AsyncTask.finalize(job)           │
   └──────────────────────────────────────┘
```

**两条路径在 Step 入库后汇合**。cron 产生和手动触发的 Step 走完全相同的 `AsyncTask.execute()`。

### Step 执行策略

**所有 Step 并发。** 如需顺序依赖，拆成两个 Job（首版不实现 depends_on）。

---

## AsyncTask trait

```rust
// crates/aigw-core/src/async_task.rs

#[async_trait]
pub trait AsyncTask: Send + Sync + 'static {
    /// 任务类型标识。对应 async_job_steps.step_type
    fn step_type(&self) -> &'static str;

    // ── cron 路径 ──

    /// 周期检查。有发现 → 返回 Step 列表。无 → None
    async fn tick(&self, db: &Database) -> Result<Option<Vec<NewStep>>>;
    fn tick_interval(&self) -> Duration;

    // ── cron + manual 共用 ──

    /// 执行一个 Step
    async fn execute(&self, db: &Database, step: &Step) -> Result<StepOutput>;
    /// Job 全部完成后调用
    async fn finalize(&self, db: &Database, job: &JobRecord) -> Result<()> { Ok(()) }

    // ── 配置 ──

    fn concurrency(&self) -> usize { 1 }

    // ── 手动触发 ──

    /// POST /admin/jobs/trigger 时调用。默认不支持
    async fn steps_from_payload(&self, _payload: &Value) -> Result<Vec<NewStep>> {
        Err("manual trigger not supported for this task".into())
    }
}

pub struct NewStep {
    pub key: String,                 // "hour=2026-07-24T14"
    pub payload: serde_json::Value,
}

pub struct StepOutput {
    pub result: serde_json::Value,
}
```

---

## Engine

```rust
// crates/aigw-core/src/engine.rs

pub struct EngineConfig {
    pub max_loops: usize,           // 全局 exec loop 上限。默认 8
    pub poll_interval: Duration,    // 无 Step 时的休眠间隔。默认 10s
    pub cleanup_interval: Duration, // 超时 Step 回收间隔。默认 30s
    pub step_timeout: Duration,     // Step 超时。默认 10min
}

pub struct Engine {
    db: Arc<Database>,
    config: EngineConfig,
    tasks: Vec<Arc<dyn AsyncTask>>,
}

impl Engine {
    pub fn new(db: Arc<Database>, config: EngineConfig) -> Self;
    pub fn register(&mut self, task: Arc<dyn AsyncTask>);
    pub async fn run(&self) -> !;
}
```

**`run()` 内部：**

```
run():
  1. 每个 task 一个 tick loop（调用 task.tick()）
  2. 每个 task N 个 exec loop（N = task.concurrency()，受 max_loops 限制）
     分配：先每人 1，剩余按 concurrency 比例分配
  3. 一个全局 cleanup loop
  4. join_all 保活
```

**Exec loop：**

```rust
loop {
    match claim_next_step(&db, task.step_type()) {
        Some(step) => {
            match task.execute(&db, &step).await {
                Ok(output) => complete_step(&db, &step, output, task).await,
                Err(e) => fail_step(&db, &step, e, task).await,
            }
        }
        None => sleep(config.poll_interval),
    }
}
```

**核心 SQL（Engine 内部）：**

- `claim_next_step(db, step_type)` — `SELECT … FOR UPDATE SKIP LOCKED`
- `complete_step(db, step, output, task)` — 写 result + 检测 Job 完成 + `task.finalize()`
- `fail_step(db, step, error, task)` — retry < max → pending；retry ≥ max → failed
- `cleanup_stale_steps(db)` — 超时 running → pending

---

## DB 表

### Migration 020 — 三张表

```sql
CREATE TABLE async_jobs (
    id TEXT PRIMARY KEY,
    step_type TEXT NOT NULL,
    trigger_type TEXT NOT NULL,        -- "cron" | "manual"
    triggered_by TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    total_steps INTEGER NOT NULL DEFAULT 0,
    completed_steps INTEGER NOT NULL DEFAULT 0,
    failed_steps INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    max_retries INTEGER NOT NULL DEFAULT 3,
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE async_job_steps (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES async_jobs(id),
    step_key TEXT NOT NULL,
    step_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    payload JSONB DEFAULT '{}',
    result JSONB DEFAULT '{}',
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    started_at TEXT,
    completed_at TEXT,
    UNIQUE(job_id, step_key)
);

CREATE TABLE async_job_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL REFERENCES async_jobs(id),
    step_key TEXT,
    level TEXT NOT NULL DEFAULT 'info',
    message TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_async_jobs_status ON async_jobs(status);
CREATE INDEX idx_async_jobs_type ON async_jobs(step_type, status);
CREATE INDEX idx_async_job_steps_claim ON async_job_steps(step_type, status, step_key);
CREATE INDEX idx_async_job_logs_job ON async_job_logs(job_id, created_at);
```

### Migration 021 — spend_logs 加列

```sql
-- SQLite
ALTER TABLE spend_logs ADD COLUMN body_archived INTEGER NOT NULL DEFAULT 0;
ALTER TABLE spend_logs ADD COLUMN parquet_path TEXT;

-- MySQL
ALTER TABLE spend_logs ADD COLUMN body_archived TINYINT(1) NOT NULL DEFAULT 0;
ALTER TABLE spend_logs ADD COLUMN parquet_path TEXT;

-- PostgreSQL
ALTER TABLE spend_logs ADD COLUMN body_archived BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE spend_logs ADD COLUMN parquet_path TEXT;
```

部分索引：
```sql
CREATE INDEX idx_spend_logs_archive
  ON spend_logs(body_archived, start_time)
  WHERE messages IS NOT NULL;
```

---

## Body Archiver 接入

```rust
// crates/aigw-core/src/body_archive/mod.rs

pub struct BodyArchiver {
    config: BodyArchiveConfig,
    object_store: Arc<dyn ObjectStore>,
    footer_cache: FooterCache,         // (Stage 79)
    col_cache: Option<ColChunkCache>,  // (Stage 80)
}

impl AsyncTask for BodyArchiver {
    fn step_type(&self) -> &'static str { "body_archive" }

    // ── cron ──
    async fn tick(&self, db: &Database) -> Result<Option<Vec<NewStep>>> { /* 查未归档小时 */ }
    fn tick_interval(&self) -> Duration { Duration::from_secs(300) }

    // ── execute ──
    async fn execute(&self, db: &Database, step: &Step) -> Result<StepOutput> {
        // step.payload 读 hour → SELECT WHERE body_archived=FALSE
        // → ArrowWriter (ZSTD 3, Bloom filter, ROW_GROUP 5000)
        // → 存储上传 → UPDATE body_archived=TRUE
    }
    fn concurrency(&self) -> usize { 2 }

    // ── finalize ──
    async fn finalize(&self, db: &Database, _job: &JobRecord) -> Result<()> {
        // null_body_after_days 清理
    }

    // ── manual ──
    async fn steps_from_payload(&self, payload: &Value) -> Result<Vec<NewStep>> {
        // {start_date, end_date} → hour 序列
    }
}
```

### main.rs

```rust
let archiver = Arc::new(BodyArchiver::new(config, object_store));

let mut engine = Engine::new(db.clone(), EngineConfig { max_loops: 8, ..Default::default() });
engine.register(archiver);
engine.run().await;
```

---

## 存储后端

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StorageBackend {
    S3 { bucket, region, endpoint?, access_key_id, secret_access_key, prefix, use_ssl, compatibility_mode, url_style },
    #[serde(rename = "fs")]
    FileSystem { path },
}
```

支持：AWS S3 / COS / R2 / MinIO / OSS / 本地文件系统。

---

## 新增依赖

```toml
parquet = { version = "54", features = ["arrow", "zstd"] }
arrow = { version = "54" }
object_store = { version = "0.11", features = ["aws"] }
moka = { version = "0.12", features = ["sync"] }
```

**Engine 纯 sqlx，约 300 行，零外部框架。**

---

## 模块结构

```
crates/aigw-core/src/
├── async_task.rs        ← AsyncTask trait
├── engine.rs            ← Engine + claim / complete / fail / cleanup
├── body_archive/
│   ├── mod.rs           ← BodyArchiver (impl AsyncTask)
│   ├── config.rs        ← BodyArchiveConfig + StorageBackend
│   ├── storage.rs       ← build_object_store (S3 + FS)
│   ├── writer.rs        ← Parquet 写入
│   ├── query.rs         ← (Stage 79)
│   └── cache.rs         ← (Stage 79/80)
```

---

## 多副本协调

| 机制 | 作用 |
|------|------|
| `UNIQUE(job_id, step_key)` | 多 tick 同时 INSERT 同一 Step，只一条成功 |
| `SELECT … FOR UPDATE SKIP LOCKED` | 多 exec loop 同时 claim，各自拿不同 Step |
| `cleanup_stale_steps()` | 超时 running → pending（崩溃恢复） |
| `WHERE body_archived = FALSE` | 业务幂等 |

---

## 验收标准

### 框架

- [ ] `claim_next_step` 原子领取、并发不重复、全部 running 返回 None
- [ ] `complete_step` 写 result + Job 完成检测 + 调 `finalize()`
- [ ] `fail_step` retry < max → pending；retry ≥ max → failed + job progress
- [ ] `cleanup_stale_steps` 超时回收 / 未超时保留
- [ ] `AsyncTask::concurrency()` 控制 exec loop 数，`max_loops` 全局上限
- [ ] `AsyncTask::steps_from_payload()` 默认返回 unsupported

### Body Archiver

- [ ] `BodyArchiveConfig` 含 `StorageBackend`（S3 + FS）完整解析
- [ ] `tick()` → 未归档小时 → Steps；无 → None
- [ ] `execute()` → Parquet ZSTD → 存储上传 → body_archived=TRUE
- [ ] `finalize()` → 过期 body 清空
- [ ] `concurrency = 2`
- [ ] `steps_from_payload(start, end)` → 手动存量归档

---

## 测试要求

- [ ] `claim_next_step` — 多 pending 拿一行；全部 running → None；并发隔离
- [ ] `complete_step` — step done → job completed → finalize 调用
- [ ] `fail_step` — retry < max → pending；≥ max → failed
- [ ] `cleanup_stale` — 超时回收 / 未超时保留
- [ ] `tick` — 有 → Steps / 无 → None
- [ ] `execute` — 0 行 / 正常 / 存储不可达
- [ ] `finalize` — null_body_after_days 边界
- [ ] `StorageBackend` 反序列化 — S3 / FS

---

## 不做

- Footer/Col 缓存 + 查询路由（Stage 79）
- Admin API（Stage 80）
- 前端（Stage 81）
- 日 compaction（长期）
- Job 间 depends_on（未来）
