# request_id 历史行回填 SOP

> Stage 85 把 spend_logs.request_id（PK）改名 call_id + 新增可空 request_id（上游 provider id）。历史迁移行 request_id 为 NULL——本 SOP 用一条 SQL 把成功行的 call_id 回填到 request_id，失败行保持 NULL。**一次性运维操作，非产品功能，不写代码。**

## 成功判定

回填**仅对 status = 'success' 的行**（精确匹配）。失败/流式/超时行无上游 id，保持 NULL：

| status | 处理 |
|--------|------|
| success | ✅ 回填 request_id = call_id |
| failure:NNN / timeout:upstream / streaming / NULL | ❌ 保持 NULL |

> 历史成功行没有真实上游 id，用 call_id（网关调用 id）顶上作为对账兜底——次优但可用。

---

## 通用步骤（三方言一致）

### 1. 回填前 COUNT 看影响范围

```sql
SELECT COUNT(*) FROM spend_logs
  WHERE request_id IS NULL AND status = 'success';
```

### 2. 执行回填

```sql
UPDATE spend_logs
  SET request_id = call_id
  WHERE request_id IS NULL AND status = 'success';
```

- WHERE request_id IS NULL 保证**幂等**：重跑只回填新增的 NULL 成功行，已有上游 id 的行不覆盖。
- status = 'success' 精确匹配，不含 failure:* / timeout:upstream / streaming。

### 3. 验证（回填后应只剩失败/流式行 NULL）

```sql
SELECT status, COUNT(*) FROM spend_logs
  WHERE request_id IS NULL
  GROUP BY status;
-- 期望：只有 failure:*/timeout:upstream/streaming/NULL，没有 success
```

---

## 各数据库注意事项

### PostgreSQL

- 单语句原子执行（MVCC，不锁表，不阻塞读写）。
- 命令（psql）：
  - psql "$DATABASE_URL" -c "UPDATE spend_logs SET request_id = call_id WHERE request_id IS NULL AND status = 'success';"
- 大表（>50 万 NULL 成功行）：PG 不支持 UPDATE ... LIMIT，但 MVCC 下并发不受影响，可直跑；若担心 WAL 暴涨，分批用子查询循环：
  - 重复执行直到 affected_rows = 0：
    UPDATE spend_logs SET request_id = call_id
      WHERE call_id IN (
        SELECT call_id FROM spend_logs
        WHERE request_id IS NULL AND status = 'success'
        LIMIT 10000
      );

### SQLite

- 单语句执行（整库写锁，aigw 数据量下毫秒级）。
- 命令（sqlite3）：
  - sqlite3 /path/to/aigw.db "UPDATE spend_logs SET request_id = call_id WHERE request_id IS NULL AND status = 'success';"
- 大表：SQLite 不支持 UPDATE ... LIMIT，同 PG 用 call_id IN (SELECT ... LIMIT N) 子查询循环。
- 建议**停服或低峰期**执行（写锁会阻塞其他写）。

### MySQL

- 单语句可执行，InnoDB 加行锁；小表直跑。
- 命令（mysql）：
  - mysql -h <host> aigw -e "UPDATE spend_logs SET request_id = call_id WHERE request_id IS NULL AND status = 'success';"
- **大表分批（MySQL 原生支持 UPDATE ... LIMIT）**：
  - 重复执行直到 affected_rows = 0：
    UPDATE spend_logs
      SET request_id = call_id
      WHERE request_id IS NULL AND status = 'success'
      LIMIT 10000;
- 大表建议低峰期分批，避免长行锁。

---

## 回滚（误回填时）

仅清「被本脚本回填」的行（request_id == call_id 的成功行）：

```sql
UPDATE spend_logs SET request_id = NULL
  WHERE status = 'success' AND request_id = call_id;
```

不会误清 Stage 85 之后新写入、有真实上游 id 的行（那些行 request_id != call_id）。

---

## 边界

- ❌ 不写 CLI 子命令（一行 SQL 三方言通用，过度工程）。
- ❌ 不回填失败行（无上游 id，语义不对）。
- ❌ 不回填 daily_tag_spend（该表只有 call_id，没有 request_id 列，见 023 迁移 Phase 3）。
- ❌ 不做定时/常驻回填（一次性运维）。
