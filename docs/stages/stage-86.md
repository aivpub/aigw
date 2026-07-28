# Stage 86: aigw-migrate sync 子命令 — aigw↔aigw 多表只读增量同步

**Phase**: 33 — 跨实例数据同步
**优先级**: P2
**状态**: ⏳ 待开始
**预估**: 8h
**前置**: Stage 85（request_id→call_id 改名已完成，两端 schema 一致）；`aigw-migrate` crate 的 `SourcePool` / `CursorRange` / `insert_rows_batch` / `migrate_plain_table` 抽象已就绪

---

## 核心预期

**任意两个 aigw 数据库实例之间（PG↔SQLite 任意组合）能通过一条 CLI 命令，把源库的数据同步到目标库——默认全 11 张业务表，也可用 `--tables` 选子集；`spend_logs` 支持"最近 N 天"增量，其他表全量幂等追加；重跑不重复。**

这是本 Stage 唯一业务目标。多表选择、方向双向、时间过滤、幂等追加均为支撑项。参数范式参考现有 `remote-import`/`remote-export`。

> ⚠️ **边界**: 本 Stage **只读追加**（`INSERT OR IGNORE` / `ON CONFLICT DO NOTHING`），**一次性 CLI 命令**（非常驻、非定时、非 CDC）。源端 UPDATE/DELETE 不传播——符合"只读镜像"诉求。
>
> ⚠️ **加密表假设**: `credentials` / `proxy_models` 两张加密表**直接复制密文**，假设两端共享同一个 `master_key`（同 aigw 集群内）。不支持跨 master_key 同步（会解密失败）。需要跨 key 场景仍用现有 `remote-import`/`remote-export`。
>
> ⚠️ **config 表默认排除**: `config` 表含 `master_key`，默认**不同步**（避免覆盖目标实例鉴权）。如需同步须显式 `--tables config`，走 `INSERT OR IGNORE` 只补齐目标缺失行，不覆盖已有 master_key。

## 背景

现有 `aigw-migrate` 是 **litellm ↔ aigw 异构迁移** CLI，绑死了 litellm 表名（`LiteLLM_SpendLogs`）、camelCase 列名（`startTime`）、以及 `source.request_id → target.call_id` 的列重定向（`remote_import.rs:577`）。它**不能**用于 aigw↔aigw 同构同步，因为两端 schema 已一致（同表名、同 snake_case、同 PK `call_id`），这套 litellm-mapping 反而会找错表、做错列映射。

但 `aigw-migrate` crate 底层抽象与 litellm 假设解耦，可复用：

| 抽象 | 位置 | 复用方式 |
|------|------|----------|
| `SourcePool::connect` | `native.rs:62` | 按 URL scheme 建 PG/SQLite/MySQL 池，source/target 任意组合 |
| `migrate_plain_table` | `remote_import.rs:147` | 全表扫描 + `insert_rows` 幂等写——给 plain 表当模板 |
| `CursorRange` | `native.rs:27` | `resume_after` + `end_before` 时间游标（spend_logs 专用） |
| `stream_rows_with_cursor` | `native.rs:306` | PG keyset / SQLite LIMIT 流式吐 `UnifiedRow`（spend_logs 专用） |
| `insert_rows_batch` | `native.rs:1316` | 目标侧 `INSERT OR IGNORE` / `ON CONFLICT DO NOTHING` 幂等批量写 |
| `conflict_clause` / `insert_prefix` | `native.rs:130-144` | 跨方言幂等 INSERT 子句 |

**需要改的底层点**: `build_cursor_sql`（`native.rs:260-295`）硬编码锚点列为 litellm 的 `"startTime"`，aigw 是 `start_time`。需新增 `build_aigw_cursor_sql`（锚点 `start_time`），**不改** `build_cursor_sql`，保 litellm 迁移零回归。`stream_rows_with_cursor` 的 PG keyset 分页用 `(start_time, call_id)` 而非 `(startTime, request_id)`。

## CLI 设计（参数参考 remote-import/remote-export 风格）

```
aigw-migrate sync -s <URL> -t <URL> [OPTIONS]

  -s, --source-url <URL>           # aigw 源库（必填，或 AIGW_SYNC_SOURCE_URL）
  -t, --target-url <URL>           # aigw 目标库（必填，或 AIGW_SYNC_TARGET_URL）
  -T, --tables <list>              # aigw 表名，逗号分隔；不传=全 11 张业务表
  -d, --days <N>                   # spend_logs 专用：start_time 在最近 N 天（UTC）
  -r, --resume-after <ISO8601>     # spend_logs 精确下界 start_time >= value
  -e, --end-before <ISO8601>       # spend_logs 精确上界 start_time < value
  -B, --skip-body                  # 跳过 spend_logs 的 messages/response/proxy_server_request
  -b, --batch-size <N>             # 默认 10
```

**short alias 分配**（sync 为新命令，新增 short alias 提升易用性；现有 `remote-import`/`remote-export` 仅有 long 不改动，保兼容）：

| long | short | 助记 | 选字母理由 |
|------|-------|------|-----------|
| `--source-url` | `-s` | source | 首字母 |
| `--target-url` | `-t` | target | 首字母 |
| `--tables` | `-T` | Tables | 大写避让小写 `-t`（target） |
| `--days` | `-d` | days | 首字母 |
| `--resume-after` | `-r` | resume | 首字母 |
| `--end-before` | `-e` | end | 首字母 |
| `--skip-body` | `-B` | Body | 大写避让小写 `-b`（batch） |
| `--batch-size` | `-b` | batch | 首字母 |

> clap 写法：`#[arg(long, short = 'T')]`。大小写区分不同 short（`-t` vs `-T`、`-b` vs `-B`），无冲突。

**参数与 remote-import 对照**：

| remote-import 参数 | sync 对应 | 差异说明 |
|---|---|---|
| `--source-url`/`--target-url` | 同名 + `-s`/`-t` | 两端都是 aigw 库 |
| `--source-master-key`/`--target-master-key` | **无** | aigw↔aigw 同 master_key，加密表直接复制密文 |
| `--step-filter` | `--tables`/`-T`（更灵活） | remote-import 按"步骤"选，sync 按 aigw 表名选，支持任意子集 |
| `--spend-log-limit` | 暂不纳入 | --days/--resume-after/--end-before 已够 |
| `--spend-log-resume-after`/`--end-before` | 同名 + `-r`/`-e` | spend_logs 时间游标，直接复用 |
| `--skip-body`/`--skip-columns` | `--skip-body`/`-B` | 同名 |
| `--batch-size` | 同名 + `-b` | 同名 |

## 同步表清单（--tables 可选子集，命令 --help 显示）

默认全 11 张业务表（`config` 默认排除）：

| # | aigw 表名 | 类型 | sync 处理方式 |
|---|-----------|------|---------------|
| 1 | `virtual_keys` | plain | 全量幂等追加 |
| 2 | `spend_logs` | plain + 时间锚点 | 按 --days / --resume-after / --end-before 增量 |
| 3 | `organizations` | plain | 全量幂等追加 |
| 4 | `teams` | plain | 全量幂等追加 |
| 5 | `users` | plain | 全量幂等追加 |
| 6 | `projects` | plain | 全量幂等追加 |
| 7 | `budgets` | plain | 全量幂等追加 |
| 8 | `organization_memberships` | plain | 全量幂等追加 |
| 9 | `team_memberships` | plain | 全量幂等追加 |
| 10 | `credentials` | 加密 | 直接复制密文（同 master_key，当 plain 处理） |
| 11 | `proxy_models` | 加密 | 直接复制密文（同 master_key，当 plain 处理） |
| — | `config` | 含 master_key | **默认排除**；显式 `--tables config` 才同步，走 INSERT OR IGNORE 不覆盖 |

> **表名校验**: `--tables` 解析后校验每个表名在清单内（含 config），非法表名报错退出，不静默忽略。

## 内部执行节奏（不拆 Stage，分阶段）

```
① cursor 锚点参数化 + sync 模块骨架（串行前置，~2h）
   native.rs: build_aigw_cursor_sql（anchor="start_time"，表名直接用 aigw 表名）
              stream_rows_with_cursor 加 aigw 分支（PG keyset 用 (start_time, call_id)）
   sync.rs: run_sync(source_url, target_url, tables, cursor, skip_body, batch_size)
            — connect source/target
            — 按 tables 列表遍历：plain 表复用 migrate_plain_table 风格全量扫
              spend_logs 走 stream_rows_with_cursor + insert_rows_batch
              credentials/proxy_models 当 plain 复制密文（不调 migrate_credentials）
            — 空 overrides（同 schema direct-match，不做 call_id←request_id 重定向）
        ↓ 编译前提
② CLI 接入 + --days/--tables 解析（~1.5h）
   main.rs: Sync 子命令分支 + 参数定义 + --help 表清单
   --days → CursorRange（chrono UTC，now-Nd ≤ start_time < now）
   --tables → 表名集合 + 校验
        ↓
③ TDD 红绿 + 文档（~4.5h）
   UT 覆盖 7 场景（见下）+ 更新 docs/aigw-migrate.md sync 章节
```

## 关键实现要点

### ① cursor 锚点参数化（不破坏 litellm 迁移）

`build_cursor_sql`（`native.rs:260`）硬编码 `"startTime"`（`native.rs:281,290`）。**新增** `build_aigw_cursor_sql`，锚点列固定 `start_time`，表名直接用 aigw 表名，ORDER BY `start_time` ASC。`stream_rows_with_cursor` 加 aigw 分支，PG keyset 用 `(start_time, call_id)` 而非 `(startTime, request_id)`（`native.rs:579`）。

### ② sync.rs 核心逻辑（复用现有抽象）

```rust
pub async fn run_sync(source_url, target_url, tables, cursor, skip_body, batch_size) -> anyhow::Result<SyncStats> {
    let source = SourcePool::connect(source_url).await?;
    let target = SourcePool::connect(target_url).await?;
    // 按 tables 列表遍历：
    //   - plain 表（含 credentials/proxy_models 当 plain）：全量 read_rows + insert_rows 幂等
    //   - spend_logs：stream_rows_with_cursor("spend_logs", aigw cursor) + insert_rows_batch
    // 空 overrides —— aigw↔aigw 同 schema，direct-match，不重定向
    // 返回 SyncStats { per_table: HashMap<表名, (inserted, ignored)> }
}
```

**关键**: `overrides` 传空 HashMap——aigw↔aigw 同 schema，列名 direct-match，不需要 litellm 的 `call_id←request_id` 重定向。`credentials`/`proxy_models` **不调** `migrate_credentials`/`migrate_proxy_models`（它们做密钥轮转），而是当 plain 表直接 `insert_rows` 复制密文。

### ③ 幂等保证

`insert_rows`（`native.rs:1195`）/ `insert_rows_batch`（`native.rs:1316`）已用 `insert_prefix`（`INSERT OR IGNORE INTO` / `INSERT IGNORE INTO` / `INSERT INTO ... ON CONFLICT DO NOTHING`）+ `conflict_clause`。PK 冲突时整行跳过，重跑只追加新行。统计 `inserted` vs `ignored`（`native.rs:1257-1263` 已有计数逻辑）。

### ④ --days 与显式游标叠加

`--days N` → `resume_after = now - N 天`、`end_before = now`（UTC ISO 8601）。若同时给 `--resume-after`/`--end-before`，取两者更严的边界（max(resume_after), min(end_before)），不冲突报错。仅对 `spend_logs` 生效；其他表忽略时间参数（全量）。

## TDD 计划（先红后绿）

| # | 测试场景 | 验证点 |
|---|----------|--------|
| 1 | SQLite→SQLite 全表同步 | 两 tempdir 库，源灌多表数据→目标，每表行数 + 内容一致 |
| 2 | `--tables` 选子集 | `--tables spend_logs,teams` 只同步这两张，其他表目标为空 |
| 3 | `--days 7` 时间过滤 | 源 spend_logs 灌 10 行（3 行 7 天内），目标只得 3 行；其他表全量不受影响 |
| 4 | 幂等重跑 | 同步一次后再同步一次，`ignored = inserted`，目标行数不变 |
| 5 | `--skip-body` | 目标 spend_logs 的 messages/response/proxy_server_request 为 NULL，其他列有值 |
| 6 | 非法表名报错 | `--tables foo` 报错退出，不静默忽略 |
| 7 | config 默认排除 | 不传 --tables 时 config 表不同步；显式 `--tables config` 才同步（INSERT OR IGNORE 不覆盖目标 master_key） |

> **PG 跨方言测试**: 项目已有 testcontainers（`bdd-real-pg`/`bdd-real-mysql` task，Phase 29）。UT 优先 SQLite→SQLite（快、无外部依赖），PG→SQLite 作为可选集成测试复用容器启动逻辑。若 testcontainers 在 UT 环境不稳定，记为技术债 TD，BDD real 路径补齐。

> **BDD**: 本 Stage 是 CLI 工具，无 HTTP 接口，不新增 .feature。UT + 手工端到端验证即可（对齐 `aigw-migrate` 现有测试风格，见 `crates/aigw-migrate/tests/`）。

## 交付清单

- [ ] `crates/aigw-migrate/src/native.rs`: `build_aigw_cursor_sql` + `stream_rows_with_cursor` aigw 分支
- [ ] `crates/aigw-migrate/src/sync.rs`: `run_sync` + `SyncStats` + 表清单常量 + 表名校验
- [ ] `crates/aigw-migrate/src/lib.rs`: 导出 `sync` 模块
- [ ] `crates/aigw-migrate/src/main.rs`: `Sync` 子命令 + `--days`/`--tables` 解析 + `--help` 表清单
- [ ] `crates/aigw-migrate/tests/sync.rs`: 7 个 UT 场景
- [ ] `docs/aigw-migrate.md`: 新增 `sync` 子命令章节（含表清单 + 参数对照）
- [ ] `docs/11-next-steps.md` + `docs/stages/stage-roadmap.md`: Phase 33 规划同步

## 风险与决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 命令定位 | 新建 `sync`（aigw↔aigw），参数参考 remote-import | 用户明确：参数范式参考 remote-import，但两端是 aigw 同构 |
| 默认表范围 | 全 11 张业务表，config 默认排除 | config 含 master_key，避免覆盖目标鉴权 |
| 锚点列改造 | 新增 `build_aigw_cursor_sql`，不改 `build_cursor_sql` | 保 litellm 迁移不回归 |
| 列重定向 | 不做（空 overrides） | aigw↔aigw 同 schema，direct-match |
| 加密表 | 直接复制密文（同 master_key） | aigw 集群内同 key；跨 key 用 remote-import |
| `--days` 时区 | UTC | `start_time` 存 UTC，避免跨天错位 |
| `--tables` 表名 | aigw 表名（如 `spend_logs`） | aigw↔aigw 同构，命令 --help 给出清单 |
| PG 集成测试 | 复用 testcontainers，可选 | 不阻塞 UT，不稳定则记 TD |
| 是否常驻 | 否，一次性 CLI | 符合用户诉求，简单可落 |

## 不做的事（边界）

- ❌ 不做 UPDATE/DELETE 传播（只读追加）
- ❌ 不做定时/常驻同步（一次性 CLI）
- ❌ 不做 CDC / 逻辑复制
- ❌ 不改 litellm↔aigw 迁移路径（`remote_import`/`remote_export` 原样）
- ❌ 不支持跨 master_key 同步加密表（需跨 key 用 remote-import）
- ❌ config 表默认不同步（显式 --tables config 才同步）
