# Stage 83: 读路径 + 缓存激活 + 凭证安全 + FileSystem 后端

**Phase**: 31 — Body Archive 生产化
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 6h
**前置**: Stage 82（冷数据回源端点已接通）

> 原 Stage 84 重编号为 83。工作量按 subagent 并发实测下调。参考审计报告 P1-1、P1-2、P2-10、P2-11、P2-12。

---

## 背景

Stage 79 设计要求读路径为"footer cache → row group 定位 → col chunk range read"三段流水线，实际是全文件下载 + 线性扫描（`query.rs:28-41`），FooterCache（`cache.rs`）死代码从未被调用。归档后查历史 body 每次下载完整 Parquet（P99 19MB），S3 出流量成本不可接受。`read_body_from_storage` 把存储错误吞为 `Ok(None)`，误导排障。S3 凭证明文配置无 env 注入，StorageBackend::FileSystem 无实现。

## 目标

1. 实现 `query_parquet_with_cache`：footer cache → row group 定位（column statistics + Bloom filter）→ col chunk range read
2. 激活 FooterCache（moka LRU），消除重复 footer 请求
3. `read_body_from_storage` 区分 NotFound vs 不可达（Err 不吞 None）
4. S3 凭证支持 `${ENV_VAR}` 占位符解析
5. StorageBackend::FileSystem 接入 `object_store::local::LocalFileSystem`（CI 无需 S3）

## TDD 流程（红→绿）

### Red：先写失败测试

- [ ] `query.rs` UT：footer cache 命中跳过请求（mock store 计数 footer 调用次数=1）（当前失败：无 cache 接入）
- [ ] `query.rs` UT：row group 定位（多 row group parquet，target 在第 2 组，只读第 2 组的 col chunk）（当前失败：全文件读）
- [ ] `query.rs` UT：同文件多次查询 footer 只请求 1 次
- [ ] `mod.rs` UT：read_body_from_storage NotFound→Ok(None)；不可达→Err（当前失败：都返回 Ok(None)）
- [ ] `config.rs` UT：`${VAR}` 占位符解析（当前失败：无解析）
- [ ] `storage.rs` UT：StorageBackend::FileSystem 构造 LocalFileSystem（当前失败：只支持 S3）
- [ ] `storage.rs` UT：FileSystem 归档写入本地文件 → 读回 body 校验一致（当前失败：无 FileSystem 实现）
- [ ] `storage.rs` UT：FileSystem 归档文件路径符合 `year=/month=/day=/hour=/data.parquet` 分区结构（当前失败：无 FileSystem 实现）

运行 `task test`（cargo test --workspace）确认全部红。

### Green：实现至测试通过

- [ ] 新增 `BodyArchiver::query_parquet_with_cache(path, request_id)`：
  1. `footer_cache.get(path)` 命中 → 跳过 footer 请求
  2. 未命中 → `store.get_range(footer_bytes)` 下载 footer → `ParquetMetaDataReader::decode` → `footer_cache.put`
  3. 用 `ParquetMetaData` 的 row group statistics + Bloom filter 定位含 target request_id 的 row group
  4. `store.get_range(col_chunk_bytes)` 仅下载该 row group 的 request_id/messages/response/proxy_server_request 列块
  5. 解码返回 `BodyPayload`
- [ ] `get_message_body` 冷路径改调 `query_parquet_with_cache`
- [ ] `read_body_from_storage` 改：`Err(e) if NotFound => Ok(None), Err(e) => Err(...)`
- [ ] `S3Config` 加 `fn resolve_env(s: &str) -> String` 占位符解析
- [ ] `build_object_store` 接受 `StorageBackend` 枚举；FileSystem 分支用 `LocalFileSystem`

## BDD + real BDD 验证

### BDD（mock，`task bdd`）

- [ ] body_archive feature 加："S3 不可达→GET detail 返回 502"（当前会 404）
- [ ] body_archive feature 加："NotFound→404"
- [ ] body_archive feature 加："本地 FS 归档 + 回源全链路"（用 LocalFileSystem，CI 无需 S3）：
  - Scenario: 本地 FS 归档写入
    - Given body_archive 配置 storage.backend=fs, fs.path=/tmp/aigw-archive
    - When 触发归档 hour=2026-07-25T14
    - Then 文件 `/tmp/aigw-archive/logs/year=2026/month=07/day=25/hour=14/data.parquet` 存在
  - Scenario: 本地 FS 回源
    - Given 上一步归档完成，DB body 已 null + body_archived=true + parquet_path 已记录
    - When GET /global/spend/logs/{request_id}
    - Then 返回 body 内容与归档前一致
- [ ] body_archive feature 加："footer cache 命中跳过请求"（mock store 计数）

> 注：Stage 82 已在 `storage_configured()` 门禁里预留 `StorageBackend` 枚举分支（S3 认 bucket+access_key，FS 认 path），但 FS 分支返回 `Err("not yet implemented")`。本 Stage 补 FS 真实实现后，门禁自动放行 FS 模式——无需再改 Stage 82 的门禁逻辑，只改 `build_object_store` + `storage_configured` 的 FS 分支实现。

### real BDD（三后端）

- [ ] `task bdd-real-sqlite` / `task bdd-real-pg` / `task bdd-real-mysql` 验证回源端点在三后端都工作
- [ ] 本地 FS 后端在真实 server 上归档 + 回源全链路

### 实际执行 + 错误修复

- [ ] `task doctor` 编译 + clippy 无警告
- [ ] `task test` 全绿
- [ ] `task bdd` 全绿
- [ ] `task bdd-real-sqlite` + `task bdd-real-pg` + `task bdd-real-mysql` 全绿
- [ ] 发现的错误及时修复并重跑

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-core/src/body_archive/mod.rs` | 新增 query_parquet_with_cache；read_body_from_storage 错误区分 |
| `crates/aigw-core/src/body_archive/query.rs` | 实现 range read + row group 定位 |
| `crates/aigw-core/src/body_archive/cache.rs` | FooterCache 接入读路径 |
| `crates/aigw-core/src/body_archive/storage.rs` | 接受 StorageBackend 枚举；LocalFileSystem 实现 |
| `crates/aigw-core/src/body_archive/config.rs` | S3Config env 占位符解析 |
| `crates/aigw-server/tests/bdd_steps/body_archive_steps.rs` | 补读路径 BDD step |

## 验收标准

- [ ] Red 阶段 8 个测试全部先红
- [ ] Green 阶段全部转绿
- [ ] 本地 FS 归档读写 BDD 全绿（写入分区路径校验 + 读回 body 一致）
- [ ] mock BDD 全绿
- [ ] real BDD 三后端全绿
- [ ] 发现的错误全部修复
