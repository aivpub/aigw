# Stage 82: 后端正确性全栈（Step↔Job 联动状态机 + 配置失联 + 假阳性 + 冷回源 + 并发安全 + retry）

**Phase**: 31 — Body Archive 生产化
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 10h
**前置**: Stage 78-81 代码已落地（feat/body-archive 分支）

> 合并自原 Stage 82 + Stage 83（二者同触 `crates/aigw-core/src/engine.rs` 和 `crates/aigw-core/src/body_archive/mod.rs`，合并避免反复改同一文件；工作量按 subagent 并发实测下调）。对应用户反馈 Q1/Q3/Q4。对应审计 P0-1~P0-6 + P1-3~P1-7（审计结论见 stage-roadmap.md Phase 31 背景）。

---

## 背景

审计确认 6 个 P0 + 部分 P1 阻断生产：(1) job 状态机只有 pending→completed，缺 running/failed/partially_failed；(2) 配置三处用 `BodyArchiveConfig::default()` 失联，config.yaml 无 body_archive 段；(3) execute() 未检查 storage_configured 导致 bucket="" 时 0 行返回 Ok 被标 completed；(4) 冷数据回源端点未接通；(5) create_job/claim_next_step 无事务，increment_job_completed 竞态；(6) fail_step 无退避，start_time 列类型偏差。本 Stage 一次性修复后端正确性。

## Step↔Job 联动状态机设计（核心）

Jobs 和 Steps 是**两层联动状态机**：Step 状态机是主/驱动，Job 状态机由 Step 的执行状态和结果**派生聚合**——Job 不独立流转。Step 的状态变化直接驱动 Job：
- 任一 Step 首次进入 running → 触发 Job `pending → running`
- 每个 Step 到终态（completed/failed）→ 原子累加计数并检查 `completed+failed >= total`，是则按 failed 计数判定 Job 三终态
- finalize 失败这个"结果" → 把 Job 从原终态覆盖为 failed

当前实现的 Q1"job 卡 pending"根因正是 Job 状态机残缺（engine.rs 只有 `mark_job_completed` 一个改 job.status 的函数，无 running/failed/partially_failed 转移，且 increment_job_failed 只加计数不触发终态判定）。

### Step 状态机（`async_job_steps.status`）

```
pending ──claim──→ running ──execute Ok──→ completed ✅
                      │
                      ├──execute Err, retry<max──→ pending (+ next_retry_at 退避) 🔁
                      ├──execute Err, retry>=max──→ failed ❌
                      ├──超时 cleanup, retry<max──→ pending (+ retry_count+1) 🔁
                      └──超时 cleanup, retry>=max──→ failed ❌
```

- **回 pending 是 step 级**：单个 step 失败但有 retry 次数，回队列等下次重试，**不打回 job 起点**。
- **退避**：retry<max 回 pending 时设 `next_retry_at = now + 2^retry_count 秒`，`claim_next_step` WHERE 加 `AND (next_retry_at IS NULL OR next_retry_at < now)`，避免立即重试耗尽配额。
- **cleanup**：超时 running 回收时 `retry_count+1`，达 max_retries 直接 failed 不回 pending。

### Job 状态机（`async_jobs.status`，当前残缺，本 Stage 补全）

```
pending ──首次 claim 任一 step──→ running 🔄
                                   │
                                   ├──所有 step 终态且 failed==0────────→ completed ✅
                                   ├──所有 step 终态且 failed==total────→ failed ❌
                                   └──所有 step 终态且 0<failed<total──→ partially_failed ⚠️
                                   （finalize 在三种终态都调用；finalize 失败则标 job failed）
```

**Job 不会回到 pending。** 回 pending 的是单个 step（重试）。Job 整体只会向前走到终态。

### Job 终态判定逻辑

每次 step 进终态（completed 或 failed）时，原子地检查 `completed_steps + failed_steps >= total_steps`，是则判定三态之一：

```rust
// 伪代码：job 终态判定（在 complete_step / fail_step 末尾调用）
fn judge_job_terminal(completed: i32, failed: i32, total: i32) -> JobStatus {
    // 进入此函数前提：completed + failed >= total（所有 step 已到终态）
    if failed == 0          { Completed }        // 全部成功
    else if failed == total { Failed }           // 全部失败
    else                    { PartiallyFailed }  // 部分成功部分失败
}
```

`partially_failed` 是业界常见做法（Airflow/Celery/Temporal 均有）：body_archive 24 小时 step 有 1 个失败时，标 partially_failed 让运维知道"大部分归档成功但有一小时要重跑"，而非把整个 job 标 failed 让人误以为全废。

### 原子化防竞态（Stage 82 并发安全）

`complete_step`/`fail_step` 的 `+1` 与终态判定必须原子，否则两个并发 complete/fail 各自读到旧值都判 true，finalize 调两次。用 `UPDATE ... RETURNING` 一条 SQL 完成"+1 并判定终态"。两侧对称：complete 侧加 completed_steps 并判定，fail 侧加 failed_steps 并判定。

```sql
-- Postgres/MySQL 示例（complete 侧）
UPDATE async_jobs
   SET completed_steps = completed_steps + 1
 WHERE id = ?
RETURNING
  (SELECT CASE
            -- 先判是否所有 step 已到终态（completed+failed 之和达 total）
            WHEN (completed_steps + 1) + failed_steps >= total_steps THEN
              CASE WHEN failed_steps = 0 THEN 'completed'          -- 无失败
                   ELSE 'partially_failed'                          -- 有失败有成功
              END
            ELSE 'running'                                         -- 还有 step 未到终态
          END) AS terminal;

-- fail 侧对称 increment_job_failed：SET failed_steps = failed_steps + 1 ... RETURNING
-- 判定：(completed) + (failed_steps+1) >= total 时，completed==0 → 'failed'（全失败），否则 'partially_failed'
```

**注意**：complete 侧刚成功一个 step（completed 至少 1），永远不会判出 `failed`，该分支只存在于 fail 侧。两侧都返回 `terminal` 字段，调用方据此决定是否调 `mark_job_*` + `finalize`。SQLite 旧版无 RETURNING，改 `BEGIN IMMEDIATE` 事务内 UPDATE+SELECT 串行 + `WHERE status IN('pending','running')` 乐观锁保护。

## 目标

1. 补全 Step↔Job 联动状态机：Step 状态机加 next_retry_at 退避 + cleanup retry_count+1；Job 状态机新增 mark_job_running/mark_job_failed/mark_job_partially_failed（`WHERE status IN(...)` 乐观锁）
2. 配置单例化：AppState 注入 `body_archiver: Arc<BodyArchiver>`；main.rs 从 config.yaml 解析（AigwConfig 加 `body_archive: Option<BodyArchiveConfig>` 字段）；trigger/archive_stats 改用 state.body_archiver；trigger 端点 enabled=false 返回 409
3. execute() 加 `storage_configured()` 门禁，未配置直接 Err；steps_from_payload 同样拒绝；消除假阳性 completed。**门禁判定需覆盖后端类型**：S3 模式认 `bucket + access_key_id 非空`，FileSystem 模式认 `path 非空`（Stage 83 引入 FS 后端后，FS 模式不能被 S3-only 门禁误挡——本 Stage 预留 `StorageBackend` 枚举分支，FS 实现可先返回 `Err("filesystem backend not yet implemented")`，Stage 83 补真实实现）
4. 冷数据回源：routes/spend.rs detail handler 集成 `state.body_archiver.get_message_body`（body null + body_archived 时查 Parquet）
5. create_job/claim_next_step 事务化（SQLite BEGIN IMMEDIATE）；increment_job_completed/failed 用 UPDATE...RETURNING（PG/MySQL）或事务内 UPDATE+SELECT（SQLite）原子化消竞态
6. finalize 在三种 job 终态（completed/failed/partially_failed）都调用；finalize 失败标 job failed 不静默 completed（修复 P0-3：有 step failed 时 finalize 永不调用导致 body 不清理）
7. fail_step 加 next_retry_at 指数退避（migration 加 next_retry_at 列）；claim_next_step WHERE 加 next_retry_at 条件；cleanup_stale_steps 回收 retry_count+1 用尽直接 failed
8. writer.rs start_time 改 TimestampMillisecond；cache_hit 改 Boolean

## TDD 流程（红→绿）

### Red：先写失败测试，运行确认红

- [ ] `engine.rs` UT：create_job→claim→assert job.status=='running'（当前失败：无 mark_job_running）
- [ ] `engine.rs` UT：fail_step retry 用尽→assert job.status=='failed'（当前失败：无 mark_job_failed）
- [ ] `engine.rs` UT：混合 completed+failed→assert job.status=='partially_failed'
- [ ] `engine.rs` UT：全部 step failed→assert job.status=='failed'
- [ ] `engine.rs` UT：全部 step completed→assert job.status=='completed'
- [ ] `engine.rs` UT：并发 complete_step（2 个）→ finalize 仅调用 1 次（当前失败：竞态）
- [ ] `engine.rs` UT：create_job 部分失败→回滚（模拟 step INSERT 失败，当前失败：无事务）
- [ ] `engine.rs` UT：fail_step 退避间隔递增（当前失败：无 next_retry_at）
- [ ] `engine.rs` UT：job 有 failed step 时 finalize 仍被调用（当前失败：P0-3 finalize 永不调用）
- [ ] `body_archive/mod.rs` UT：bucket="" 时 execute 返回 Err（当前失败：0 行返回 Ok）
- [ ] `body_archive/config.rs` UT：AigwConfig 解析 body_archive 段（当前失败：无字段）
- [ ] `writer.rs` UT：start_time 是 TimestampMillisecond（当前失败：Utf8）
- [ ] `jobs.rs` BDD：trigger enabled=false→409（当前失败：用 default config）
- [ ] `spend.rs` BDD：归档后 body null + body_archived→GET /global/spend/logs/{id} 返回 body（当前失败：未接通）

运行 `task test`（cargo test --workspace）+ `task bdd` 确认上述全部红。

### Green：实现至测试通过

- [ ] 新增 mark_job_running/mark_job_failed/mark_job_partially_failed（engine.rs，`WHERE status IN(...)` 乐观锁）
- [ ] exec_loop claim 后调 mark_job_running
- [ ] complete_step/fail_step 用 UPDATE...RETURNING（或 SQLite 事务）原子 +1 并判定终态；到终态时调对应 mark_job_* + finalize
- [ ] finalize 在 completed/failed/partially_failed 三种终态都调用；finalize Err 时调 mark_job_failed
- [ ] create_job_sqlite/mysql/pg 用事务包裹 INSERT job + 所有 INSERT steps
- [ ] SQLite claim_next_step 改 BEGIN IMMEDIATE 事务
- [ ] BodyArchiver 加 storage_configured()；execute/steps_from_payload 开头门禁
- [ ] AigwConfig 加 body_archive 字段；main.rs 解析并注入 Engine + AppState
- [ ] routes/jobs.rs trigger/archive_stats 改用 state.body_archiver；trigger enabled 检查返回 409
- [ ] routes/spend.rs detail handler 集成 get_message_body 回源
- [ ] writer.rs start_time→TimestampMillisecond；cache_hit→Boolean
- [ ] migration 022 加 next_retry_at 列（sqlite/mysql/postgres 三套）
- [ ] config.example.yaml 加 body_archive 示例段（credentials 用 ${ENV} 占位符）

## BDD + real BDD 验证

### BDD（mock，`task bdd`）

- [ ] jobs.feature 加："trigger disabled 返回 409"
- [ ] body_archive feature 加："冷数据回源返回 body"（归档后 body null → GET detail 返回 body）
- [ ] body_archive feature 加："job 状态机 running/failed/partially_failed 三态"
- [ ] body_archive feature 加："job 有 failed step 时 finalize 仍清理 body"

### real BDD（`task bdd-real-sqlite` / `task bdd-real-pg` / `task bdd-real-mysql`，三后端）

- [ ] 跨 DB 验证状态机迁移 SQL：PG/MySQL 的 UPDATE...RETURNING、SQLite 的 BEGIN IMMEDIATE 在三后端都通过
- [ ] 验证 async_job_steps.next_retry_at 列迁移在三后端生效
- [ ] 验证 trigger enabled=false→409 在真实 server + 真实 DB
- [ ] 验证冷数据回源在真实归档数据上工作
- [ ] 验证 job 三态判定在真实并发下不重复 finalize

### 实际执行 + 错误修复

- [ ] `task doctor` 编译 + clippy 无警告
- [ ] `task test` 全绿
- [ ] `task bdd` 全绿
- [ ] `task bdd-real-sqlite` + `task bdd-real-pg` + `task bdd-real-mysql` 全绿
- [ ] 发现的编译错误/测试失败/迁移错误及时修复并重跑，不积压

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-core/src/engine.rs` | 新增 mark_job_running/failed/partially_failed；事务化 create_job/claim；原子化 increment；finalize 三终态调用 + 错误传播；cleanup retry_count |
| `crates/aigw-core/src/body_archive/mod.rs` | storage_configured()；execute/steps_from_payload 门禁 |
| `crates/aigw-core/src/config.rs` | AigwConfig 加 body_archive 字段 |
| `crates/aigw-core/src/body_archive/writer.rs` | start_time→TimestampMillisecond；cache_hit→Boolean |
| `crates/aigw-server/src/main.rs` | 解析 config.body_archive → 单例 BodyArchiver → 注入 Engine + AppState |
| `crates/aigw-server/src/routes/keys.rs` | AppState 加 body_archiver 字段 |
| `crates/aigw-server/src/routes/jobs.rs` | trigger/archive_stats 用 state.body_archiver；trigger enabled 检查 |
| `crates/aigw-server/src/routes/spend.rs` | detail handler 集成 get_message_body 回源 |
| `crates/aigw-core/migrations/{sqlite,mysql,postgres}/022_*.sql` | 加 next_retry_at 列 |
| `config.example.yaml` | 加 body_archive 示例段（${ENV} 占位符）|
| `crates/aigw-server/tests/bdd_steps/body_archive_steps.rs` | 补状态机 + 冷回源 BDD step |

## 验收标准

- [ ] Red 阶段 14 个测试全部先红
- [ ] Green 阶段全部转绿
- [ ] 联动状态机实现：Step 可回退重试 + 退避；Job 由 Step 驱动三终态 + 不回退
- [ ] finalize 在三种 job 终态都调用（P0-3 修复）
- [ ] mock BDD 全绿
- [ ] real BDD 三后端（sqlite/pg/mysql）全绿
- [ ] 发现的错误全部修复，无遗留编译警告
