# Body Archive: S3 Parquet 冷存储方案（纯 Rust 栈）

**日期**: 2026-07-22
**优先级**: P0（spend_logs 表 body 字段存储膨胀，影响 DB 性能和成本）
**状态**: 方案评审完成，已修正

### 修正记录

| 日期 | 修正内容 | 原因 |
|------|---------|------|
| 2026-07-24 | 废弃 DuckDB 依赖，改用纯 Rust 栈（parquet + arrow + object_store + moka） | 架构评审发现 DuckDB 读路径无法实现 footer/col-chunk 缓存，且引入 C++ FFI 增加编译复杂度。纯 Rust 栈可对 S3 range read 做精确控制，缓存层按需读 footer（~8KB）和 col chunk，避免整文件下载 |
| 2026-07-24 | Footer 缓存从自定义实现改为 moka::sync::Cache | moka 是成熟的 Rust 并发缓存库，提供 LRU/TTL 策略，比手写实现更可靠 |
| 2026-07-24 | 新增 Parquet Schema 设计 | 明确列顺序（过滤列在前、body 列在后）、编码策略、Bloom filter 列选择 |
| 2026-07-24 | 新增 DuckDB CLI 运维数据分析方案 | DuckDB CLI 仅用于人工 ad-hoc SQL 查询，不在应用代码中使用；与读路径分离 |
| 2026-07-24 | Bloom filter 追加 session_id 列；S3 文件布局从文件名分区改为目录分区（兼容 DuckDB hive_partitioning） | session_id 为随机 UUID，column statistics (min/max) 对其无效，需 Bloom filter 加速会话查询；文件名中的 key=value 不被 DuckDB hive_partitioning 识别，需改为目录形式 |
| 2026-07-24 | 修正"不做什么" | 移除"不引入 arrow/parquet crate"，新增"不在读路径使用 DuckDB" |

---

## 一、背景与问题

### 1.1 现状

aigw 每条 LLM 请求完成后，将完整的 `messages`（请求 body）、`response`（响应 body）、`proxy_server_request`（代理请求）三个 JSON 字段写入 `spend_logs` 表：

| 字段 | (PG) | 典型大小 |
|------|-----------|:------:|
| `messages` | JSONB | 200-300 KB（长程对话） |
| `response` | JSONB | 10-100 KB |
| `proxy_server_request` | JSONB | 50-128 KB |

每条 spend_log 记录的这三个字段合计可达 **400-500 KB**。在日均 1 万请求的场景下，spend_logs 表每天增长 **4-5 GB**，每月增长 **120-150 GB**。其中 95%+ 的体积来自这三个 body 字段。

### 1.2 实测数据

基于生产数据库 800,863 条记录、4,095 小时的实测统计：

**请求量分布：**

| 指标 | 值 |
|------|:---|
| 每小时中位数 | **131** |
| 每小时 P95 | **518** |
| 每小时 P99 | **1,095** |
| 每小时历史极值 | 8,866（2026-01-01，小体量时期） |
| 历史日极值 | 57,501（2026-01-01） |
| 近期日请求量 | 5,000-12,000 |

**Body 大小分布（全量）：**

| messages 大小 | 占比 | 说明 |
|:----------:|:----:|------|
| < 1 KB | **99.3%** | 历史数据，短请求为主 |
| 50-200 KB | 0.4% | 长程对话 |
| 200-500 KB | 0.2% | 超大上下文 |

**近期模式已切换**（2026-07-22 17:00 采样，621 条）：
- 96%（596 条）的 messages > 50 KB
- 该小时 messages 总量 104 MB，平均单条 **172 KB**
- 长程对话已成为主要负载

**单次 S3 查询流量预估（基于 ROW_GROUP_SIZE=5000）：**

| 场景 | 请求数/时 | Parquet 压缩后 | Row Groups | Footer | Col Chunk | **S3 总流量** |
|------|:---------:|:------------:|:---------:|:-----:|:---------:|:-----------:|
| 低谷 | 30 | 0.5 MB | 1 | 1 KB | 0.5 MB | **0.5 MB** |
| **中位数** | **131** | **2.3 MB** | 1 | 1 KB | 2.3 MB | **2.3 MB** |
| P95 | 518 | 9 MB | 1 | 1 KB | 9 MB | **9 MB** |
| P99 | 1,095 | 19 MB | 1 | 1 KB | 19 MB | **19 MB** |
| 近期峰值 | 2,210 | 38 MB | 1 | 1 KB | 38 MB | **38 MB** |
| 历史极值 | 8,866 | ~8 MB | 2 | 5 KB | 4 MB | **4 MB** |
| 预估远期极值 | 20,000 | 340 MB | 4 | 20 KB | 85 MB | **85 MB** |

> 历史 8,866/小时的极值对应小体量时期（99% 消息 < 1KB），实际体积极小。远期极值按峰值 20,000 条大 body 估算。常规场景（中位数~P99）都是 1 个 row group，col chunk ≤ 19 MB。

### 1.3 核心矛盾

- **业务需要**：用户需要回查历史对话的完整 messages/response，用于调试、审计、计费争议
- **存储成本**：JSONB 直接存 PG 效率低，列存储 ZSTD 压缩可节省 5-10x 空间
- **查询模式**：大部分查询只关心元数据（model/tokens/spend/time），极少查 body；但 body 拖慢了全表扫描
- **安全边界**：aigw 部署服务器本地磁盘不适合长期存储数百 GB 的 body 数据

### 1.4 目标

1. 主库 `spend_logs` 只保留最近 7 天的 body 热数据，7 天以上的 body 迁移到对象存储
2. 对象存储使用 Parquet + ZSTD 列式压缩，节省 85-90% 存储成本
3. 查询 body 时自动路由：热数据查 DB，冷数据查 S3 Parquet
4. 写入路径零改动：body 仍然先入 DB，归档是纯后台异步操作
5. 支持任意 S3 兼容对象存储（AWS S3 / Cloudflare R2 / 腾讯云 COS / MinIO 等）
6. 提供管理 API 和 admin 界面的手动归档、进度查询、执行日志回溯能力

---

## 二、技术方案

### 2.1 架构概览

```
写路径（零改动）:
  POST /v1/chat → spend_logs INSERT (含 body) → PG/MySQL 持久化

归档器（每小时一次，后台任务）:
  DB 中 1 小时前的数据 → parquet crate 写 → S3 PUT → UPDATE body_archived=TRUE

清理器（归档后）:
  DB 中 7 天前 + body_archived=TRUE → SET messages=NULL, response=NULL, proxy_server_request=NULL

查询 body:
  7 天内的 → 直接查 DB
  7 天外的 → 读 S3 Parquet footer → 定位 row group → 读 col chunk → 解码 JSON body
```

### 2.2 为什么用 DB 当 Buffer

| 方案 | 崩溃安全 | 写路径改动 | S3 故障容忍 | 新增组件 |
|------|:---:|:---:|:---:|:---:|
| Parquet 内存缓写 | ❌ | 大 | ❌ | Parquet writer 热路径 |
| WAL + 异步上传 | ✅ | 大 | ✅（WAL 积压） | WAL + Parquet |
| **DB 直写 + 定期归档** | ✅ | **零** | ✅（等恢复） | Parquet 冷路径 |

DB 本身就是最好的 buffer：ACID 写入保证、崩溃自动恢复、已有查询能力。归档器是可选的后台任务，挂了只影响归档进度，不影响核心功能。

### 2.3 为什么用纯 Rust Parquet 栈（不用 DuckDB）

架构评审发现 DuckDB 在 aigw 场景下有根本性问题：

**问题 1：DuckDB 读路径无法实现缓存。** DuckDB 的 `read_parquet()` 通过 httpfs 扩展读 S3，整个读取链路是 C++ 内部的，Rust 侧无法拦截 S3 range read。这意味着 Plan 第三节设计的 footer 缓存和 col chunk 缓存在 DuckDB 读路径上完全无法工作——每次查询 body 都必须通过 DuckDB 下载完整文件或至少一个 row group。

**问题 2：依赖成本高。** DuckDB `bundled` feature 引入 C++ 编译（DuckDB core + httpfs 扩展），编译时间 5-10 分钟，二进制体积 +20-30 MB。aigw 的核心定位是轻量网关，这个代价不可忽视。

**问题 3：DuckDB 只用在两个操作上。** 写操作（`COPY TO S3`）和读操作（`read_parquet`）。这两个操作用 parquet + object_store crate 可以实现，且可以拿到更精确的 S3 流量控制。

**纯 Rust 栈的优势：**

- **精确 S3 range read**：`object_store::get_range()` 允许只读 Parquet footer（~8KB）和单个 col chunk（~10 MB），而不是整个 file/row group
- **缓存可控**：footer 解析成 `ParquetMetaData` 后缓存，col chunk 解压后缓存，全部在 Rust 侧实现
- **零 FFI**：纯 Rust 编译，无 C++ 依赖，编译快，二进制体积小
- **Parquet 列式 + ZSTD**：`parquet` crate（arrow-rs）原生支持 ZSTD 压缩，JSON 文本压缩比 8-11x
- **谓词下推 + Bloom Filter + Column Statistics**：`WHERE request_id = 'xxx'` 时先查 footer 的 column statistics（min/max），再查 Bloom filter，只读目标 row group 的 messages 列 chunk
- **ROW_GROUP_SIZE=5000 + ORDER BY request_id**：P99 以内的请求量（≤ 1,095 条/时）都在 1 个 row group 内，查 1 条只需 2 次 S3 range read（footer + 1 个 col chunk ~20 MB）

### 2.4 Parquet Schema 设计

Parquet 文件的列按以下顺序排列（遵循"过滤列在前、大 body 列在后"原则）：

| 列名 | 类型 | 编码 | 说明 |
|------|------|------|------|
| `request_id` | UTF8 | Plain + Bloom filter | **第一列**，点查主键，Bloom filter 加速定位 |
| `start_time` | TimestampMillisecond | Delta encoding | **第二列**，时间范围查询 |
| `model` | UTF8 | Dictionary encoding | 低基数（< 100 个不同值），字典编码节省空间 |
| `status` | UTF8 | Dictionary encoding | 低基数（"success"/"failed"/...），字典编码 |
| `cache_hit` | Boolean | Plain | 布尔值 |
| `session_id` | UTF8 | Plain + Bloom filter (nullable) | 会话分组，可为 null；Bloom filter 加速会话追踪查询 |
| `messages` | UTF8 | ZSTD(3) | 大 JSON，ZSTD 压缩 |
| `response` | UTF8 | ZSTD(3) | 大 JSON |
| `proxy_server_request` | UTF8 | ZSTD(3) | 大 JSON |

**设计理由：**

- **过滤列在前**、body 列在后。Parquet 的 column statistics（min/max）存在 row group 的 footer 中，查询时先检查前几列的 statistics 即可跳过不匹配的 row group，无需读取后续大列。
- **Bloom filter 加在 `request_id` 和 `session_id`**。`request_id` 是唯一的点查键；`session_id` 为随机 UUID，其 column statistics 的 min/max 范围跨度极大，对 row group 裁剪无效，必须依赖 Bloom filter 才能高效定位会话所在 row group。Bloom filter 开销约 30KB/row group（5000 行），仅占总文件大小的 < 0.8%。
- **文件内按 `request_id` 排序**。同一 row group 内的 `request_id` 连续，column statistics 的 min/max 范围精确。
- **`model` 和 `status` 用字典编码**。这两个字段的基数极低（model 不到 100 个，status 不到 10 个），字典编码将重复字符串替换为整数索引，显著减少存储。
- **ZSTD 压缩 level 3**。在压缩率和速度之间的平衡点；body 列的 JSON 文本压缩比约 8-11x。

#### 写入伪代码

```rust
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, Encoding};

let props = WriterProperties::builder()
    .set_compression(Compression::ZSTD(ZstdLevel::try_new(3)?))
    // Bloom filter on request_id (点查主键)
    .set_column_bloom_filter_enabled(
        ColumnPath::from("request_id"), true
    )
    // Bloom filter on session_id (随机 UUID，column statistics 无效)
    .set_column_bloom_filter_enabled(
        ColumnPath::from("session_id"), true
    )
    .build();

let mut writer = ArrowWriter::try_new(
    object_store_sink,  // object_store S3 stream
    schema,
    Some(props),
)?;

writer.write(&record_batch)?;
writer.close()?;
```

#### ROW_GROUP_SIZE 计算

默认 ROW_GROUP_SIZE=5000，基于以下考量：
- 中位数场景（131 条/时）：1 个 row group
- P95（518 条/时）：1 个 row group
- P99（1,095 条/时）：1 个 row group
- 远期极值（20,000 条/时）：4 个 row group

1 个 row group 意味着查询 1 条记录只需读 1 个 col chunk，避免跨 row group 的多次 S3 range read。

### 2.5 为什么每小时一个文件

- Parquet 不可追加（S3 对象不可变），但也不需要追加——每个时段结束一次性导出
- 小时粒度：归档延迟 ≤ 1h、单文件 2-40 MB 大小可控、查询定位只打开 1 个文件
- 可选的日 compaction：每天凌晨合并 24 个小时文件为 1 个日文件，减少碎片

---

## 三、查询缓存层

Parquet 查询一次 body 走两次 S3 请求：读 footer（~1-50 KB），读 col chunk（~2-40 MB）。对于需要反复查同一批历史日志的场景，可以引入两层缓存来减少 S3 流量和延迟。

### 3.1 Footer 缓存

Footer 是 Parquet 文件末尾的元数据块：row group 偏移量、列统计信息（min/max）、Bloom filter。同一个文件在它的生命周期内 footer 不变，是完美的缓存目标。

| 缓存位置 | 适用场景 | 内存占用 | 过期策略 |
|---------|---------|:------:|---------|
| **none** | 低频查询，每次重新读 S3 | 0 | — |
| **mem** | 单节点，footer 量少（< 10K 文件），重启可接受冷启动 | 文件数 × ~50KB | 简单 TTL |
| **redis** | 多节点共享，重启免冷启动，footer 量大 | Redis 内 | TTL + LRU |

```
Footer 体积估算:
  每天 24 个文件 × 7 天保留期内常查 = 168 个活跃文件
  168 × ~50 KB = ~8.4 MB（mem 方案几乎可忽略）

  如果保留 30 天 = 720 个文件 × ~50 KB = ~36 MB（mem 仍可接受）
  如果全部历史 = 4,095 × ~50 KB = ~200 MB（越大越倾向 redis）
```

#### 配置

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FooterCacheMode {
    None,
    Mem,
    Redis,
}
```

#### 内存实现（moka）

Footer 缓存使用 `moka::sync::Cache`——一个成熟的 Rust 并发缓存库，支持 LRU 淘汰和 TTL 过期。

```rust
use moka::sync::Cache;
use std::sync::Arc;
use std::time::Duration;

struct FooterCache {
    // Key: S3 path, Value: 已解析的 ParquetMetaData
    cache: Cache<String, Arc<ParquetMetaData>>,
}

impl FooterCache {
    fn new(max_capacity: u64) -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(Duration::from_secs(3600))
                .build(),
        }
    }

    fn get(&self, s3_path: &str) -> Option<Arc<ParquetMetaData>> {
        self.cache.get(s3_path)
    }

    fn put(&self, s3_path: &str, metadata: Arc<ParquetMetaData>) {
        self.cache.insert(s3_path.to_string(), metadata);
    }
}
```

> 为什么缓存 `ParquetMetaData` 而不是原始 bytes？缓存解析后的元数据避免每次查询都解析 footer；同时 `ParquetMetaData` 包含 row group 偏移量、列统计（min/max）、Bloom filter 位置等，是查询路由的完整输入。

#### Redis 实现

```rust
// Footer key: "aigw:parquet:footer:{s3_path}" → base64(footer_bytes)
// TTL: 24h（文件每天不变，但新 compaction 可能替换）
// 自动过期，无需手动 evict
impl FooterCache {
    fn redis_key(s3_path: &str) -> String {
        format!("aigw:parquet:footer:{}", s3_path)
    }

    async fn get_redis(&self, redis: &redis::Client, s3_path: &str) -> Option<Vec<u8>> {
        let key = Self::redis_key(s3_path);
        let raw: Option<Vec<u8>> = redis.get(&key).await.ok()?;
        raw
    }

    async fn put_redis(&self, redis: &redis::Client, s3_path: &str, footer: &[u8]) {
        let key = Self::redis_key(s3_path);
        let _: () = redis.set_ex(&key, footer, 86400).await.ok()?; // 24h TTL
    }
}
```

### 3.2 Col Chunk 缓存

Col chunk 是 row group 中单个列的所有数据（解压后）。一个 col chunk 在 P99 场景下约 19 MB，缓存它可以直接跳过 S3 请求 2，查询延迟从 "2 次 HTTP range" 降到 "0 次网络请求"。

| 缓存位置 | 适用场景 | 存储开销 | 淘汰策略 |
|---------|---------|:---:|---------|
| **none** | 低频查询，S3 出流量可接受 | 0 | — |
| **fs** | col chunk 可复用（重查同一批日志），本地磁盘有空间 | 可配置上限 | LFU + 容量驱逐 |

> 为何不是 mem？col chunk 可能 10-100 MB，放进程内存会挤占热数据缓存（DB 连接池、模型配置、路由表）。文件系统更适合大块冷数据缓存。

> 为何 LFU 而非 LRU？body 查询有明显热点——某次故障后的那次请求可能被反查 10 次，之后再也不查。LRU 会把"刚查过一次但不会再查"的热点踢掉，LFU 按频率更能保留真正的热点。

#### 配置

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ColChunkCacheConfig {
    /// 缓存模式
    #[serde(rename = "mode")]
    pub mode: ColChunkCacheMode,

    /// 缓存目录（mode=fs 时有效）
    pub dir: PathBuf,                    // "/data/aigw/cache/parquet_chunks"

    /// 容量上限（MB）
    pub max_size_mb: usize,              // 默认 1024（1 GB）

    /// 单文件体积上限（MB），超过此大小的 col chunk 不缓存
    pub max_entry_mb: usize,             // 默认 100
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColChunkCacheMode {
    None,
    Fs,
}
```

#### 文件系统实现

```
/data/aigw/cache/parquet_chunks/
├── meta.json              ← 索引：{key → {path, size, access_count, last_access}}
├── chunks/
│   ├── 0001.raw           ← col chunk 原始数据（压缩前）
│   ├── 0002.raw
│   └── ...
```

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::sync::Mutex;

struct ColChunkCache {
    dir: PathBuf,
    max_size: u64,           // 总容量上限（字节）
    max_entry: u64,          // 单条目上限（字节）
    state: Mutex<CacheState>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheState {
    entries: HashMap<String, CacheEntry>,
    total_size: u64,
    next_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    id: u64,                  // 文件名 chunks/{id}.raw
    size: u64,                // 字节
    access_count: u64,        // LFU 计数器
    last_access: i64,         // Unix timestamp
}

impl ColChunkCache {
    /// 缓存的 key = {s3_path}:{row_group}:{column}
    /// 例如 "s3://bucket/logs/.../hour=14/data.parquet:0:messages"
    fn cache_key(s3_path: &str, row_group: usize, column: &str) -> String {
        format!("{}:{}:{}", s3_path, row_group, column)
    }

    async fn get(&self, s3_path: &str, row_group: usize, column: &str) -> Option<Vec<u8>> {
        let key = Self::cache_key(s3_path, row_group, column);
        let mut state = self.state.lock().await;
        if let Some(entry) = state.entries.get_mut(&key) {
            entry.access_count += 1;
            entry.last_access = now_unix();
            let path = self.dir.join("chunks").join(format!("{}.raw", entry.id));
            drop(state);
            fs::read(&path).await.ok()
        } else {
            None
        }
    }

    async fn put(&self, s3_path: &str, row_group: usize, column: &str, data: &[u8]) {
        let size = data.len() as u64;
        let mut state = self.state.lock().await;

        // 跳过超大条目
        if size > self.max_entry { return; }

        // 如果已存在，更新
        let key = Self::cache_key(s3_path, row_group, column);
        if let Some(entry) = state.entries.get_mut(&key) {
            entry.access_count += 1;
            entry.last_access = now_unix();
            // 如果体积变了，删除旧文件重写
            if entry.size != size {
                let old_path = self.dir.join("chunks").join(format!("{}.raw", entry.id));
                fs::remove_file(&old_path).await.ok();
                state.total_size -= entry.size;
                state.total_size += size;
                entry.size = size;
                let new_path = self.dir.join("chunks").join(format!("{}.raw", entry.id));
                fs::write(&new_path, data).await.ok();
            }
            return;
        }

        // LFU 驱逐
        while state.total_size + size > self.max_size && !state.entries.is_empty() {
            // 找 access_count 最小的条目驱逐
            let victim_key = state.entries.iter()
                .min_by_key(|(_, e)| (e.access_count, e.last_access))
                .map(|(k, _)| k.clone());

            if let Some(vk) = victim_key {
                if let Some(ve) = state.entries.remove(&vk) {
                    let path = self.dir.join("chunks").join(format!("{}.raw", ve.id));
                    fs::remove_file(&path).await.ok();
                    state.total_size -= ve.size;
                }
            }
        }

        if state.total_size + size > self.max_size { return; } // 驱逐不够

        let id = state.next_id;
        state.next_id += 1;
        let path = self.dir.join("chunks").join(format!("{}.raw", id));
        state.entries.insert(key.clone(), CacheEntry {
            id,
            size,
            access_count: 1,
            last_access: now_unix(),
        });
        state.total_size += size;
        drop(state);

        fs::create_dir_all(self.dir.join("chunks")).await.ok();
        fs::write(&path, data).await.ok();

        // 异步持久化 meta.json
        self.save_meta().await;
    }

    async fn save_meta(&self) {
        let state = self.state.lock().await;
        let meta = serde_json::to_vec(&*state).unwrap_or_default();
        fs::write(self.dir.join("meta.json"), &meta).await.ok();
    }

    /// 启动时从 meta.json 恢复，但清空 access_count（重启后重新统计）
    async fn restore(&self) {
        if let Ok(data) = fs::read(self.dir.join("meta.json")).await {
            if let Ok(mut state) = serde_json::from_slice::<CacheState>(&data) {
                for entry in state.entries.values_mut() {
                    entry.access_count = 0; // 重置计数器
                }
                *self.state.lock().await = state;
            }
        }
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
```

### 3.3 缓存查询流水线

读路径使用纯 Rust 栈：object_store 做 S3 range read，parquet crate 解析 footer/metrics，arrow 层解码 RecordBatch。

```rust
impl BodyArchiver {
    async fn query_s3_with_cache(
        &self,
        parquet_path: &str,
        request_id: &str,
    ) -> Result<Option<BodyPayload>> {
        // Step 1: 获取 footer metadata（缓存命中则 0 S3 请求）
        let metadata = match self.footer_cache.get(parquet_path) {
            Some(cached) => cached,
            None => {
                // 读 Parquet 文件末尾 ~8KB（footer + metadata）
                let file_size = self.s3.head(parquet_path).await?.size;
                let footer_bytes = self.s3.get_range(parquet_path, file_size - 8192..file_size).await?;
                let metadata = Arc::new(ParquetMetaDataReader::new()
                    .decode(&footer_bytes)?);
                self.footer_cache.put(parquet_path, metadata.clone());
                metadata
            }
        };

        // Step 2: 遍历 row groups → 检查 column statistics + Bloom filter 定位目标 row group
        let row_group = locate_row_group(&metadata, request_id)?;

        // Step 3: 读 messages 列 chunk（缓存命中则 0 S3 请求）
        let col_key = ColChunkCache::cache_key(parquet_path, row_group, "messages");
        let chunk = match self.col_cache.get(&col_key).await {
            Some(cached) => cached,
            None => {
                let (offset, size) = get_col_chunk_offset(&metadata, row_group, "messages")?;
                let fresh = self.s3.get_range(parquet_path, offset..offset + size).await?;
                self.col_cache.put(&col_key, &fresh).await;
                fresh
            }
        };

        // Step 4: parquet::ArrowReader 解码 col chunk → 扫描 request_id → 反序列化 body
        let body = decode_messages_column(&chunk, request_id)?;
        Ok(body)
    }
}
```

### 3.4 缓存效果总结

| 缓存配置 | 首次查询 | 第二次查同一文件 | 适用部署 |
|---------|:---:|:---:|---------|
| footer=none, col=none | 2 次 S3 | 2 次 S3 | 极低频 |
| **footer=mem, col=none** | 2 次 S3 | **1 次 S3**（跳过 footer） | 单节点，推荐默认 |
| footer=redis, col=none | 2 次 S3 | **1 次 S3**（跨节点共享） | 多节点 |
| **footer=mem, col=fs** | 2 次 S3 | **0 次 S3**（全缓存） | 单节点，查同一批日志多 |
| footer=redis, col=fs | 2 次 S3 | **0 次 S3**（跨节点共享） | 多节点高复用 |

**推荐默认**：`footer=mem` + `col=none`。footer 缓存代价极低（~10 MB 内存），col chunk 缓存按需开启（需要本地磁盘空间）。

---

## 四、主库 Schema 变更

### 4.1 spend_logs 新增字段

```sql
-- Migration 020: body_archive_support
-- 所有三个后端（SQLite / MySQL / PostgreSQL）都需要

ALTER TABLE spend_logs
  ADD COLUMN body_archived BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE spend_logs
  ADD COLUMN parquet_path TEXT;  -- S3 路径，归档后才填充

-- 部分索引：仅覆盖未归档 + 有 body 的行，加速归档查询
CREATE INDEX idx_spend_logs_archive
  ON spend_logs(body_archived, start_time)
  WHERE messages IS NOT NULL;
```

| 后端 | BOOLEAN 实现 | TEXT 实现 | 部分索引 |
|------|------------|----------|:---:|
| SQLite | `INTEGER NOT NULL DEFAULT 0` | `TEXT` | ✅ 支持 |
| PostgreSQL | `BOOLEAN NOT NULL DEFAULT false` | `TEXT` | ✅ 支持 |
| MySQL | `TINYINT(1) NOT NULL DEFAULT 0` | `TEXT` | ❌ 不支持 WHERE 子句，改为普通索引 |

### 4.2 SpendLog 模型

```rust
// models.rs — 新增两个字段
pub struct SpendLog {
    // ... 现有字段不变 ...
    pub body_archived: bool,            // 新增，默认 false
    pub parquet_path: Option<String>,   // 新增，默认 None
}
```

### 4.3 backlog 处理

迁移后的存量数据 `body_archived = FALSE`，归档器首次运行会自动处理历史数据——从最早的记录开始，每批 5000 条导出到 Parquet。

---

## 五、Body Archiver 设计

### 5.1 模块结构

```
crates/aigw-core/src/body_archiver.rs   ← 新增模块
├── S3Config            — 对象存储配置
├── ParquetSchema       — Parquet 列定义与编码策略
├── ArchivePolicy       — 归档策略（延迟时间、保留天数、批次大小）
├── FooterCache         — Footer 缓存（moka LRU，key: S3 path → Arc<ParquetMetaData>）
├── ColChunkCache       — Col chunk 缓存配置（none/fs，LFU 淘汰）
├── BodyArchiver        — 归档器主逻辑
└── BodyArchiverState   — 状态追踪（可选，用于监控）
```

### 5.2 完整配置

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct BodyArchiveConfig {
    pub enabled: bool,                       // 默认 false，显式开启
    pub s3: S3Config,
    pub archive: ArchivePolicy,
    pub footer_cache: FooterCacheConfig,
    pub col_chunk_cache: ColChunkCacheConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct S3Config {
    pub endpoint: String,                    // "" = AWS 默认
    pub region: String,
    pub bucket: String,
    pub prefix: String,                      // "logs"
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub url_style: String,                   // "vhost" | "path"
    pub compatibility_mode: bool,            // true for COS/R2
    pub use_ssl: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArchivePolicy {
    pub archive_after_hours: u32,        // 默认 1
    pub null_body_after_days: u32,       // 默认 7
    pub batch_size: usize,               // 默认 5000
    pub row_group_size: usize,           // 默认 5000（Parquet ROW_GROUP_SIZE）
    pub check_interval_secs: u64,        // 默认 300
    /// 归档后是否清空主库 body 字段（messages/response/proxy_server_request）
    /// true  → 归档后清空 body，释放 DB 空间（生产默认）
    /// false → 保留 body 不清理，方便测试验证（归档前后对比 hashes）
    pub null_body_after_archive: bool,   // 默认 true
    pub vacuum_after_null: bool,         // 默认 true（仅 SQLite）
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum FooterCacheConfig {
    None,
    Mem { max_capacity: u64, ttl_secs: u64 },  // 默认 capacity=10000, ttl=3600
    Redis {
        url: String,                     // redis://localhost:6379
        ttl_secs: u64,                   // 默认 86400
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ColChunkCacheConfig {
    None,
    Fs {
        dir: PathBuf,                    // "/data/aigw/cache/parquet_chunks"
        max_size_mb: usize,              // 默认 1024
        max_entry_mb: usize,             // 默认 100
    },
}
```

### 5.3 核心逻辑

```
run_loop() 循环:
    loop
        sleep(check_interval_secs)
        archive_hour(1小时前的数据)

archive_hour(target_hour):
    1. 计算时间范围 [target_hour, target_hour + 1h)
    2. 统计待归档行数
    3. 按 batch_size 分批次:
        a. 从主库读 body 列（sqlx 查询）
        b. 用 parquet crate 写 Parquet，通过 object_store 上传到 S3
           (ArrowWriter, COMPRESSION ZSTD, ROW_GROUP_SIZE {row_group_size},
            Bloom filter on request_id, ORDER BY request_id)
        c. 主库 UPDATE SET body_archived=TRUE, parquet_path='s3://...'
    4. 如果 null_body_after_archive = true，清理 target_hour - null_body_after_days 的小时数据:
        UPDATE SET messages=NULL, response=NULL, proxy_server_request=NULL
        WHERE body_archived=TRUE AND start_time IN [清理窗口]
       如果 null_body_after_archive = false，跳过清理（保留 body 用于测试验证）
    5. (SQLite) VACUUM 回收空间（仅当步骤4执行了清理后）
```

### 5.4 查询路由

```rust
impl Database {
    pub async fn get_message_body(&self, request_id: &str) -> Result<Option<BodyPayload>> {
        let meta = sqlx::query_as::<_, SpendLogMeta>(
            "SELECT messages, response, proxy_server_request,
                    body_archived, parquet_path
             FROM spend_logs WHERE request_id = ?"
        ).bind(request_id).fetch_optional(self).await?;

        match meta {
            // 热数据：直接返回
            Some(m) if m.messages.is_some() => Ok(Some(m.into_body())),
            // 冷数据：从 S3 Parquet 查（带缓存）
            Some(m) if m.body_archived && m.parquet_path.is_some() => {
                self.body_archiver.query_s3_with_cache(&m.parquet_path.unwrap(), request_id)
            }
            _ => Ok(None),
        }
    }
}
```

---

## 六、手动归档 & 运维接口

除了自动定时归档，还需要提供手动触发和运维管理能力。

### 6.1 API 端点

```
POST   /admin/archive/trigger          手动触发归档（指定日期范围）
GET    /admin/archive/jobs             归档任务列表
GET    /admin/archive/jobs/{job_id}    单个归档任务详情（进度、日志、统计）
GET    /admin/archive/stats            归档全局统计
GET    /admin/archive/logs/{job_id}    归档执行日志（分页下载）
```

#### 6.1.1 POST /admin/archive/trigger — 手动触发归档

```json
// Request
{
  "start_date": "2026-07-22T00:00:00+08:00",   // 必填
  "end_date": "2026-07-23T00:00:00+08:00",     // 可选，默认 = start_date + 1 天
  "mode": "hourly",                             // "hourly"(默认) | "daily" | "full_range"
  "dry_run": false                              // true = 仅计算不实际执行
}

// Response
{
  "job_id": "archive-20260722-a1b2c3",
  "status": "accepted",
  "estimated_hours": 24,
  "estimated_rows": 5000,
  "estimated_size_mb": 800
}
```

触发后立即返回 `job_id`，任务在后台异步执行。如果同一时间范围已有进行中的 job，返回 409 Conflict 并给出已有 job_id。

#### 6.1.2 GET /admin/archive/jobs — 任务列表

```json
// Response
{
  "jobs": [
    {
      "job_id": "archive-20260722-a1b2c3",
      "trigger": "manual",                      // "scheduled" | "manual"
      "triggered_by": "admin@example.com",      // 手动触发时的操作者
      "time_range": {
        "start": "2026-07-22T00:00:00+08:00",
        "end": "2026-07-23T00:00:00+08:00"
      },
      "status": "running",                      // "pending" | "running" | "completed" | "failed" | "cancelled"
      "progress": {
        "hours_done": 15,
        "hours_total": 24,
        "rows_done": 3200,
        "rows_total": 5000,
        "bytes_uploaded": 512000000,
        "bytes_total_estimated": 800000000
      },
      "created_at": "2026-07-22T17:30:00+08:00",
      "updated_at": "2026-07-22T17:35:00+08:00"
    }
  ],
  "total": 42
}
```

#### 6.1.3 GET /admin/archive/jobs/{job_id} — 任务详情

除 `GET /jobs` 的字段外，额外包含：

```json
{
  // ... 基础字段同上 ...
  "phases": [
    {
      "hour": "2026-07-22T14:00:00+08:00",
      "status": "completed",                    // "pending" | "running" | "completed" | "failed"
      "rows_exported": 450,
      "size_mb": 78.3,
      "s3_path": "s3://.../hour=14/data.parquet",
      "db_updated": true,                       // body_archived + parquet_path 已写入主库
      "body_nullified": false,                  // DB body 是否已清空（取决于 null_body_after_archive）
      "duration_ms": 4200,
      "error": null
    }
  ],
  "summary": {
    "total_hours": 24,
    "hours_completed": 15,
    "hours_failed": 0,
    "total_rows": 5000,
    "total_bytes_uploaded": 800000000,
    "total_duration_ms": 120000
  }
}
```

#### 6.1.4 GET /admin/archive/stats — 全局统计

```json
{
  "total_archived_rows": 450000,
  "total_archived_bytes": 75000000000,
  "total_parquet_files": 8760,
  "db_body_freed_bytes": 120000000000,
  "last_archive_at": "2026-07-22T17:00:00+08:00",
  "last_archive_status": "completed",
  "pending_rows": 800,
  "archive_enabled": true
}
```

### 6.2 数据库表：archive_jobs

```sql
-- Migration 021: archive_jobs
CREATE TABLE IF NOT EXISTS archive_jobs (
    job_id TEXT PRIMARY KEY,                     -- "archive-20260722-a1b2c3"
    trigger_type TEXT NOT NULL,                  -- "scheduled" | "manual"
    triggered_by TEXT,                           -- 手动触发时的用户名
    time_start TIMESTAMPTZ NOT NULL,             -- 归档时间范围起点
    time_end TIMESTAMPTZ NOT NULL,               -- 归档时间范围终点
    mode TEXT NOT NULL,                          -- "hourly" | "daily" | "test_hour"
    dry_run BOOLEAN NOT NULL DEFAULT FALSE,
    status TEXT NOT NULL DEFAULT 'pending',      -- "pending" | "running" | "completed" | "failed" | "cancelled"
    total_hours INTEGER NOT NULL DEFAULT 0,
    hours_completed INTEGER NOT NULL DEFAULT 0,
    hours_failed INTEGER NOT NULL DEFAULT 0,
    total_rows INTEGER NOT NULL DEFAULT 0,
    total_bytes_uploaded BIGINT NOT NULL DEFAULT 0,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS archive_job_phases (
    id SERIAL PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES archive_jobs(job_id),
    hour_start TIMESTAMPTZ NOT NULL,             -- 该小时的时间起点
    status TEXT NOT NULL DEFAULT 'pending',
    rows_exported INTEGER NOT NULL DEFAULT 0,
    size_bytes BIGINT NOT NULL DEFAULT 0,
    s3_path TEXT,
    db_updated BOOLEAN NOT NULL DEFAULT FALSE,   -- body_archived 是否已写
    body_nullified BOOLEAN NOT NULL DEFAULT FALSE, -- DB body 是否已清空
    error_message TEXT,
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    duration_ms INTEGER
);

CREATE TABLE IF NOT EXISTS archive_job_logs (
    id SERIAL PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES archive_jobs(job_id),
    hour_start TIMESTAMPTZ,                      -- 可为 NULL（job 级别的日志）
    level TEXT NOT NULL DEFAULT 'info',          -- "debug" | "info" | "warn" | "error"
    message TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_archive_jobs_status ON archive_jobs(status);
CREATE INDEX idx_archive_jobs_created ON archive_jobs(created_at);
CREATE INDEX idx_archive_job_phases_job ON archive_job_phases(job_id);
CREATE INDEX idx_archive_job_logs_job ON archive_job_logs(job_id, created_at);
```

### 6.3 Rust 实现

```rust
// crates/aigw-core/src/archive_job.rs — 新增模块

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ArchiveJob {
    pub job_id: String,
    pub trigger_type: String,        // "scheduled" | "manual"
    pub triggered_by: Option<String>,
    pub time_start: DateTime<Utc>,
    pub time_end: DateTime<Utc>,
    pub mode: String,                // "hourly" | "daily" | "test_hour"
    pub dry_run: bool,
    pub status: String,
    pub total_hours: i32,
    pub hours_completed: i32,
    pub hours_failed: i32,
    pub total_rows: i32,
    pub total_bytes_uploaded: i64,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl BodyArchiver {
    /// 手动触发归档 — 返回 job_id，异步执行
    pub async fn trigger_manual(
        &self,
        start_date: DateTime<Utc>,
        end_date: DateTime<Utc>,
        mode: &str,
        dry_run: bool,
        triggered_by: &str,
    ) -> Result<String> {
        let job_id = format!("archive-{}-{}",
            start_date.format("%Y%m%d"),
            &uuid::Uuid::new_v4().to_string()[..6]
        );

        // 检查冲突
        if let Some(existing) = self.find_running_job(start_date, end_date).await? {
            return Err(ArchiveError::Conflict(existing.job_id));
        }

        // 计算预估
        let (hours, rows, size) = self.estimate_range(start_date, end_date).await?;

        // 插入 job 记录
        sqlx::query(
            "INSERT INTO archive_jobs (job_id, trigger_type, triggered_by, time_start, time_end, mode, dry_run, total_hours, total_rows)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&job_id).bind("manual").bind(triggered_by)
        .bind(start_date).bind(end_date).bind(mode).bind(dry_run)
        .bind(hours as i32).bind(rows as i32)
        .execute(&self.db).await?;

        // 生成 phases（每小时一条）
        self.create_phases(&job_id, start_date, end_date).await?;
        self.append_log(&job_id, None, "info", &format!("Job created by {}", triggered_by)).await?;

        // 异步执行
        let archiver = self.clone();
        let jid = job_id.clone();
        tokio::spawn(async move {
            if dry_run {
                let _ = archiver.dry_run_job(&jid).await;
            } else {
                let _ = archiver.execute_job(&jid).await;
            }
        });

        Ok(job_id)
    }

    /// 执行一个归档 job
    async fn execute_job(&self, job_id: &str) -> Result<()> {
        self.update_job_status(job_id, "running").await?;
        self.append_log(job_id, None, "info", "Archive job started").await?;

        let phases = self.list_pending_phases(job_id).await?;
        for phase in phases {
            let phase_start = Instant::now();

            match self.archive_hour_in_job(job_id, &phase).await {
                Ok(stats) => {
                    self.update_phase_completed(job_id, &phase, &stats, phase_start.elapsed()).await?;
                    self.increment_job_progress(job_id, stats.rows, stats.bytes).await?;
                }
                Err(e) => {
                    self.update_phase_failed(job_id, &phase, &e.to_string()).await?;
                    self.append_log(job_id, Some(phase.hour_start), "error", &e.to_string()).await?;
                }
            }
        }

        self.update_job_status(job_id, "completed").await?;
        self.append_log(job_id, None, "info", "Archive job completed").await?;
        Ok(())
    }

    /// 追加执行日志
    async fn append_log(&self, job_id: &str, hour: Option<DateTime<Utc>>, level: &str, msg: &str) {
        sqlx::query(
            "INSERT INTO archive_job_logs (job_id, hour_start, level, message) VALUES (?, ?, ?, ?)"
        )
        .bind(job_id).bind(hour).bind(level).bind(msg)
        .execute(&self.db).await.ok();
    }
}
```

### 6.4 路由注册

```rust
// crates/aigw-server/src/routes/archive.rs — 新增模块

use axum::{extract::State, Json};
use aigw_core::body_archiver::BodyArchiver;

pub async fn trigger_archive(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TriggerArchiveRequest>,
) -> Result<Json<TriggerArchiveResponse>, AppError> {
    state.require_admin_role(&req.user).await?;

    let job_id = state.body_archiver.trigger_manual(
        req.start_date,
        req.end_date.unwrap_or(req.start_date + chrono::Duration::days(1)),
        &req.mode.unwrap_or("hourly".into()),
        req.dry_run.unwrap_or(false),
        &req.user,
    ).await?;

    Ok(Json(TriggerArchiveResponse { job_id, status: "accepted".into() }))
}

pub async fn list_jobs(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ArchiveJobListResponse>, AppError> {
    let jobs = state.body_archiver.list_jobs().await?;
    Ok(Json(ArchiveJobListResponse { jobs, total: jobs.len() as i32 }))
}

pub async fn job_detail(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(job_id): axum::extract::Path<String>,
) -> Result<Json<ArchiveJobDetail>, AppError> {
    let detail = state.body_archiver.get_job_detail(&job_id).await?;
    Ok(Json(detail))
}

pub async fn archive_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ArchiveStats>, AppError> {
    let stats = state.body_archiver.get_global_stats().await?;
    Ok(Json(stats))
}

pub async fn job_logs(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(job_id): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<JobLogsQuery>,
) -> Result<Json<Vec<ArchiveJobLog>>, AppError> {
    let logs = state.body_archiver.get_job_logs(
        &job_id, params.level.as_deref(), params.limit.unwrap_or(200), params.offset.unwrap_or(0)
    ).await?;
    Ok(Json(logs))
}
```

```rust
// crates/aigw-server/src/main.rs — 新增路由
.route("/admin/archive/trigger", axum::routing::post(archive::trigger_archive))
.route("/admin/archive/jobs", get(archive::list_jobs))
.route("/admin/archive/jobs/{job_id}", get(archive::job_detail))
.route("/admin/archive/stats", get(archive::archive_stats))
.route("/admin/archive/logs/{job_id}", get(archive::job_logs))
```

### 6.5 前端页面

在 admin settings 下新增 "Archive" 页面：`crates/aigw-frontend/src/pages/settings/archive/index.tsx`。

#### 页面布局

```
┌─────────────────────────────────────────────────────────────┐
│  Archive Manager                               [Settings ←] │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─ Status ──────────────────────────────────────────────┐ │
│  │  Status: ● Enabled    Last Archive: 2026-07-22 17:00  │ │
│  │  Total Archived: 450K rows / 75 GB                    │ │
│  │  DB Space Freed: 120 GB   Pending: 800 rows           │ │
│  └───────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌─ Manual Trigger ──────────────────────────────────────┐ │
│  │  Start Date: [2026-07-22]  End Date: [2026-07-22]    │ │
│  │  Mode: [hourly ▾]   ☐ Dry Run                        │ │
│  │  Estimated: 24 hours / 5,000 rows / ~800 MB          │ │
│  │  [Trigger Archive]                                    │ │
│  └───────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌─ Job History ─────────────────────────────────────────┐ │
│  │  ┌──────────────────────────────────────────────────┐ │ │
│  │  │ Job ID              │ Status    │ Progress │ Time │ │ │
│  │  │ archive-20260722... │ running   │ 15/24h   │ ...  │ │ │
│  │  │ archive-20260722... │ completed │ 24/24h   │ ...  │ │ │
│  │  │ archive-20260721... │ failed    │ 3/24h    │ ...  │ │ │
│  │  └──────────────────────────────────────────────────┘ │ │
│  │  [View Detail] [Download Logs]                         │ │
│  └───────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌─ Job Detail (展开) ───────────────────────────────────┐ │
│  │  Summary: 24h total, 15 completed, 0 failed, ...     │ │
│  │  Phase Details:                                       │ │
│  │  ┌────────────────────────────────────────────────┐   │ │
│  │  │ Hour          │ Status │ Rows │ Size  │ Time   │   │ │
│  │  │ 2026-07-22 00 │ ✅     │ 200  │ 35MB  │ 3.2s   │   │ │
│  │  │ 2026-07-22 01 │ ✅     │ 150  │ 28MB  │ 2.1s   │   │ │
│  │  │ 2026-07-22 02 │ 🔄     │ -    │  -    │ -      │   │ │
│  │  │ 2026-07-22 03 │ ⏳     │ -    │  -    │ -      │   │ │
│  │  └────────────────────────────────────────────────┘   │ │
│  │  Logs (最新 50 条):                                     │ │
│  │  [info] 17:30:01 Archive job created by admin@...      │ │
│  │  [info] 17:30:02 Phase 2026-07-22T00 started          │ │
│  │  [info] 17:30:05 Copied 200 rows → s3://.../hour=00/data.parquet   │ │
│  │  [info] 17:30:05 Phase 2026-07-22T00 completed        │ │
│  │  [warn] 17:33:12 S3 upload retry (attempt 2/3)        │ │
│  └───────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

#### 前端组件

```typescript
// crates/aigw-frontend/src/pages/settings/archive/index.tsx

function ArchivePage() {
  return (
    <div className="space-y-6">
      {/* Status Card */}
      <ArchiveStatusCard stats={stats} />

      {/* Manual Trigger Card */}
      <ManualTriggerCard
        onTrigger={(req) => triggerArchive(req)}
        onEstimate={(req) => estimateArchive(req)}
      />

      {/* Job History Card */}
      <JobHistoryCard
        jobs={jobs}
        onSelect={(jobId) => setSelectedJob(jobId)}
      />

      {/* Job Detail Panel (expands below selected job) */}
      {selectedJob && (
        <JobDetailCard
          jobId={selectedJob}
          detail={jobDetail}
          logs={jobLogs}
        />
      )}
    </div>
  );
}
```

### 6.6 执行日志留存策略

| 日志级别 | 示例 | 保留策略 |
|---------|------|:---:|
| `info` | "Phase 2026-07-22T14 completed, 450 rows, 78MB" | 30 天 |
| `warn` | "S3 upload retry (attempt 2/3)" | 90 天 |
| `error` | "Phase 2026-07-22T15 failed: S3 connection timeout" | 90 天 |
| `debug` | "Footer cache hit count: 15" | 7 天 |

日志通过 `archive_job_logs` 表存储，定期清理过期日志。前端展示时默认按 `created_at DESC`，支持按 level 过滤和分页。

### 6.7 权限控制

所有 `/admin/archive/*` 端点需要 admin role 认证（与现有 `/admin/` 端点相同的 auth middleware）。

---

## 七、S3 兼容配置矩阵

### 7.1 各厂商配置

| 厂商 | endpoint | url_style | compatibility_mode |
|------|----------|:---------:|:------------------:|
| **AWS S3** | (空，用默认) | `vhost` | false |
| **Cloudflare R2** | `<accountid>.r2.cloudflarestorage.com` | `vhost` | false |
| **腾讯云 COS** | `cos.<region>.myqcloud.com` | `vhost` | **true** |
| **阿里云 OSS** | `oss-<region>.aliyuncs.com` | `vhost` | false |
| **MinIO** | `localhost:9000` | `path` | false |
| **Ceph RGW** | `ceph-rgw:7480` | `path` | false |
| **华为云 OBS** | `obs.<region>.myhuaweicloud.com` | `vhost` | false |

### 7.2 本地开发（MinIO）

```yaml
# docker-compose.db.yml 追加
minio:
  image: minio/minio:latest
  command: server /data --console-address ":9001"
  environment:
    MINIO_ROOT_USER: minioadmin
    MINIO_ROOT_PASSWORD: minioadmin
  ports:
    - "9000:9000"
    - "9001:9001"
```

```toml
[body_archive]
enabled = false

[body_archive.s3]
bucket = "aigw-test"
endpoint = "localhost:9000"
region = "us-east-1"
url_style = "path"
compatibility_mode = false
use_ssl = false
access_key_id = "minioadmin"
secret_access_key = "minioadmin"

[body_archive.archive]
archive_after_hours = 1
null_body_after_days = 7
null_body_after_archive = false  # 测试模式：归档后不清空 DB body，方便验证归档前后一致性
batch_size = 5000
row_group_size = 5000

[body_archive.footer_cache]
mode = "mem"
max_capacity = 10000
ttl_secs = 3600

[body_archive.col_chunk_cache]
mode = "none"
```

### 7.3 生产配置（腾讯云 COS）

```toml
[body_archive]
enabled = true

[body_archive.s3]
bucket = "aigw-logs-1234567890"
endpoint = "cos.ap-guangzhou.myqcloud.com"
region = "ap-guangzhou"
url_style = "vhost"
compatibility_mode = true
use_ssl = true
access_key_id = "${S3_ACCESS_KEY}"
secret_access_key = "${S3_SECRET_KEY}"

[body_archive.archive]
archive_after_hours = 1
null_body_after_days = 7
null_body_after_archive = true   # 生产模式：归档后清空 DB body，释放空间
batch_size = 5000
row_group_size = 5000
check_interval_secs = 300

[body_archive.footer_cache]
mode = "mem"
max_capacity = 10000
ttl_secs = 3600

[body_archive.col_chunk_cache]
mode = "fs"
dir = "/data/aigw/cache/parquet_chunks"
max_size_mb = 1024
max_entry_mb = 100
```

---

## 八、Cargo 依赖

### 8.1 新增依赖

```toml
# crates/aigw-core/Cargo.toml
[dependencies]
# Parquet 读写
parquet = { version = "54", features = ["arrow", "zstd"] }
arrow = { version = "54", features = ["prettyprint"] }
# S3 对象存储访问
object_store = { version = "0.11", features = ["aws"] }
# Footer 缓存（内存 LRU）
moka = { version = "0.12", features = ["sync"] }
# Redis footer 缓存（可选）
redis = { version = "0.25", features = ["tokio-comp"], optional = true }
```

| 依赖 | 何时需要 | 作用 |
|------|---------|------|
| `parquet` (arrow-rs) | 始终 | Parquet 文件读写、Bloom filter、ZSTD 压缩 |
| `arrow` | 始终 | 内存列式格式 RecordBatch，作为 parquet write/read 中间格式 |
| `object_store` | 始终 | S3 对象存储访问（含 range read），支持 AWS S3/COS/R2/MinIO |
| `moka` | 始终 | 内存 LRU 缓存，存储解析后的 ParquetMetaData |
| `redis` | `footer_cache.mode = "redis"` 时 | Footer 跨节点共享 |

### 8.2 不再需要的依赖

| 原方案依赖 | 移除原因 |
|-----------|---------|
| `duckdb` (bundled) | C++ FFI 开销大（编译 5-10 min，二进制 +20-30 MB）；读路径无法实现 footer/col-chunk 缓存；写/读操作均可由 parquet + object_store 替代 |

### 8.3 Feature flags

```toml
[features]
default = []
reqwest = ["dep:reqwest", "dep:reqwest-middleware", "dep:reqwest-retry"]
integration = []
redis-cache = ["dep:redis"]  # 新增：启用 Redis footer 缓存
```

### 8.4 离线部署

纯 Rust 栈不依赖外部 C++ 库或运行时扩展，离线部署无需额外步骤。所有依赖在 `cargo build` 时已静态链接。

---

## 九、S3 文件布局与 Parquet 写入参数

### 9.1 文件布局

```
s3://aigw-logs-1234567890/logs/
├── year=2026/
│   └── month=07/
│       └── day=22/
│           ├── hour=00/
│           │   └── data.parquet
│           ├── hour=01/
│           │   └── data.parquet
│           ├── ...
│           └── hour=23/
│               └── data.parquet
```

> **Hive 分区兼容**：DuckDB 的 `hive_partitioning=true` 只能识别 **目录名** 中的 `key=value` 模式，无法识别文件名中的 `key=value`（如 `hour=14.parquet`）。因此将小时分区从文件名改为子目录，确保 `year`、`month`、`day`、`hour` 四个分区键全部被 DuckDB 自动检测，实现分区裁剪——查询时直接跳过不相关目录，无需打开文件 footer。

### 9.2 Parquet 写入参数

写入参数见「2.4 Parquet Schema 设计」节。核心要点：
- 列顺序：过滤列在前（request_id, start_time, model, status, cache_hit, session_id），body 列在后（messages, response, proxy_server_request）
- 文件内按 request_id 排序
- ROW_GROUP_SIZE=5000
- Bloom filter 加在 request_id 和 session_id 列
- ZSTD 压缩 level 3

写入使用 `parquet::arrow::ArrowWriter`，通过 `object_store` 直接上传到 S3（`put` 操作），不使用 DuckDB `COPY TO`。

### 9.3 可选日 compaction

Compaction 使用 parquet crate 读取多个小时文件，合并后写回日文件：

```rust
// 每天凌晨 2 点，合并前一天 24 个小时文件
async fn compact_daily(&self, date: NaiveDate) -> Result<()> {
    let prefix = format!("logs/year={}/month={:02}/day={:02}/",
        date.year(), date.month(), date.day());
    let hour_dirs = self.s3.list_with_prefix(&prefix).await?; // 列出 hour=00/, hour=01/, ...

    // 读取所有小时文件的 RecordBatch
    let mut batches = Vec::new();
    for dir in &hour_dirs {
        let file_path = format!("{}/data.parquet", dir.path);
        let data = self.s3.get(&file_path).await?;
        let reader = ParquetRecordBatchReader::try_new(data, 8192)?;
        for batch in reader {
            batches.push(batch?);
        }
    }

    // 排序（按 request_id）后写入日文件
    let merged = concat_and_sort_by_request_id(batches)?;
    let day_path = format!("{}data.parquet", prefix);
    let mut writer = ArrowWriter::try_new(
        self.s3.put_sink(&day_path),
        schema.clone(),
        Some(writer_props()),
    )?;
    for batch in &merged {
        writer.write(batch)?;
    }
    writer.close()?;

    // 删除旧小时目录
    for dir in &hour_dirs {
        self.s3.delete_dir(&dir.path).await?;
    }
    Ok(())
}
```

---

## 十、容错与边界情况

### 10.1 归档器故障

| 故障 | 影响 | 恢复 |
|------|------|------|
| 归档器进程崩溃 | 当前批次未完成，重跑 | 下次 tick 重新处理（`body_archived=FALSE` 的行仍在 DB） |
| S3 不可达（网络/鉴权） | 归档暂停 | 等 S3 恢复后继续；数据安全在 DB 中 |
| Parquet OOM（批次太大） | 当前批次失败 | 减小 `batch_size`，重新处理 |
| 主库连接断开 | 归档暂停 | 等连接恢复；归档器通过 sqlx 连 DB，连接池自动重连 |

### 10.2 缓存故障

| 故障 | 影响 | 降级行为 |
|------|------|---------|
| Redis 不可达（footer=redis） | 每次读 footer 走 S3 | 自动降级到 none |
| 本地磁盘满（col=fs） | 写缓存失败，跳过 | 自动降级到 none |
| 缓存数据损坏 | get 返回空 | 从 S3 重新拉取 |
| 进程重启 | meta.json 恢复 + access_count 归零 | 重新统计 LFU |

### 10.3 数据一致性

- `parquet` S3 PUT 和 `UPDATE body_archived=TRUE` **不在同一事务**（跨 DB + S3），但无影响：
  - PUT 成功但 UPDATE 失败 → 下次 re-export 同批数据，Parquet 覆盖写入（幂等）
  - UPDATE 成功但 PUT 未完成 → 不会发生（PUT 是同步的，失败抛错）
- Parquet 写 S3 通过 `object_store::put()` 上传，上传完成后才可见

### 10.4 查询边界

- 查询 `request_id` 时：
  1. DB 有 body → 直接返回（7 天内）
  2. DB 无 body + `parquet_path` 有值 → 读 S3 Parquet（缓存层自动介入）
  3. DB 无 body + `parquet_path` 无值 → 正在归档中，重试或返回空
  4. 记录不存在 → 404

### 10.5 清理安全网

```sql
-- 只清空 body，永远不删整行
-- 仅当 null_body_after_archive = true 时执行
UPDATE spend_logs
SET messages = NULL, response = NULL, proxy_server_request = NULL
WHERE body_archived = TRUE          -- 先已归档
  AND start_time < NOW() - '7 days' -- 够老
  AND messages IS NOT NULL;         -- 还有 body 可清
```

**测试验证模式**：设置 `null_body_after_archive = false`，归档后 body 仍保留在 DB 中，可以：
- 对比 DB body 与 S3 Parquet 内容的一致性
- 在从 DB 切到 S3 查 body 之前，确保归档链路完整
- 验证归档器的 `body_archived` 和 `parquet_path` 写入正确

**环境变量**：`AIGW_BODY_ARCHIVE_NULL_BODY_AFTER_ARCHIVE=true|false`，覆盖配置文件中的 `null_body_after_archive` 设置，方便在 CI/测试环境中动态切换。

---

## 十一、监控指标

| 指标 | 含义 | 告警阈值 |
|------|------|:------:|
| `body_archive_pending_count` | 待归档行数 | > 50000 |
| `body_archive_last_success_ts` | 上次成功归档时间 | > 2h 无更新 |
| `body_archive_bytes_uploaded` | 本次上传字节数 | — |
| `body_archive_errors_total` | 归档失败次数 | > 3 |
| `body_archive_s3_latency_ms` | S3 上传延迟 | > 30000 |
| `body_cache_footer_hit_rate` | Footer 缓存命中率 | < 80% |
| `body_cache_col_hit_rate` | Col chunk 缓存命中率 | — |
| `body_cache_col_size_mb` | Col chunk 缓存当前占用 | > max_size_mb × 0.9 |
| `body_cache_col_evictions` | Col chunk 驱逐次数 | 增长过快 |

---

## 十二、安全考量

1. **S3 凭证管理**：`access_key_id` / `secret_access_key` 从环境变量注入，不落地配置文件
2. **传输加密**：`s3_use_ssl = true`（生产环境强制）
3. **存储加密**：建议 S3 bucket 开启服务端加密（SSE-S3 或 SSE-COS）
4. **访问控制**：body 包含用户对话数据，S3 bucket 应为私有，仅 aigw 服务器有读写权限
5. **数据留存**：Parquet 文件按分区存储，便于按日期设置生命周期策略（如 90 天后自动删除）

---

## 十三、不做什么

1. **不改写路径**：`insert_spend_log` / `update_spend_log` 接口签名不变
2. **不做实时归档**：不做每个请求完成后立即写 Parquet——增加延迟，且小时文件更高效
3. **不做 S3 文件追加**：Parquet 不支持追加，也不需要——每个小时一个完整文件
4. **不做 Parquet 加密**：依赖 S3 bucket 级别的 SSE，不在应用层加密
5. **不删除 spend_logs 行**：只清空 body 字段为 NULL，永远不删整行
6. **不在读路径使用 DuckDB**：DuckDB C++ 读路径无法实现 S3 range read 粒度的缓存控制；应用内读路径全部使用 parquet + object_store crate
7. **不在主库存 offset/row_group**：ROW_GROUP_SIZE=5000 让 P99 场景都在 1 个 row group 内，不需要精确行定位
8. **不用 DuckDB 作为应用依赖**：DuckDB CLI 仅用于运维数据分析（见下方 13.1），不在 aigw 二进制中链接

### 13.1 DuckDB CLI 运维数据分析

> DuckDB CLI 是一个无需服务端的命令行工具（单个约 50MB 二进制），可以直接对 S3 上的 Parquet 文件执行 SQL 查询。使用场景：运维人员需要按模型统计请求量、追踪长会话、分析错误率趋势等。**不需要在应用代码中集成 DuckDB**，CLI 工具完全独立。

#### 前置配置

DuckDB 通过 httpfs 扩展连接 S3。Rust 应用生成的数据文件使用 Hive 分区目录布局，`hive_partitioning=true` 可以自动检测 `year`、`month`、`day`、`hour` 分区键。

```sql
INSTALL httpfs;
LOAD httpfs;
SET s3_region='ap-guangzhou';
SET s3_endpoint='cos.ap-guangzhou.myqcloud.com';
SET s3_use_ssl=true;
SET s3_url_style='vhost';
```

#### S3 凭证配置

支持两种方式：

**方式一：CREATE SECRET（推荐，DuckDB 内管理）**

```sql
CREATE SECRET s3_credentials (
    TYPE S3,
    KEY_ID 'xxx',
    SECRET 'xxx',
    REGION 'ap-guangzhou',
    ENDPOINT 'cos.ap-guangzhou.myqcloud.com',
    URL_STYLE 'vhost',
    USE_SSL true
);
```

**方式二：环境变量**

```bash
export AWS_ACCESS_KEY_ID=xxx
export AWS_SECRET_ACCESS_KEY=xxx
export S3_ENDPOINT=cos.ap-guangzhou.myqcloud.com
export S3_USE_SSL=true
export S3_URL_STYLE=vhost
```

#### 查询示例

```sql
-- 按模型统计请求量（利用分区裁剪，仅扫描匹配的时间范围）
SELECT model, COUNT(*) as requests
FROM read_parquet(
    's3://aigw-logs/logs/**/*.parquet',
    hive_partitioning = true
)
WHERE year = 2026 AND month = 7
GROUP BY model ORDER BY requests DESC;

-- 按小时聚合请求量
SELECT year, month, day, hour, COUNT(*) as requests
FROM read_parquet(
    's3://aigw-logs/logs/**/*.parquet',
    hive_partitioning = true
)
WHERE start_time > '2026-07-01'
GROUP BY year, month, day, hour ORDER BY year, month, day, hour;

-- 追踪特定会话
SELECT request_id, start_time, model, status,
       length(messages) as msg_len, length(response) as resp_len
FROM read_parquet(
    's3://aigw-logs/logs/**/*.parquet',
    hive_partitioning = true
)
WHERE session_id = 'some-session-uuid'
ORDER BY start_time;
```

#### 缓存优化

DuckDB 内置 `enable_object_cache` pragma 可以缓存 S3 对象元数据，减少重复的 HEAD 请求：

```sql
PRAGMA enable_object_cache;
```

> **警告**：`enable_object_cache` 是进程内存缓存，DuckDB CLI 退出后缓存即丢失。每次启动 CLI 都需要重新设置。另外，DuckDB v1.1 在远端 parquet 场景下启用对象缓存可能在高并发或大文件场景下导致 OOM；如果遇到内存问题，关闭此 pragma 并使用 S3 range read 模式（默认行为）更安全。

> DuckDB CLI 的使用不影响 aigw 应用代码编译和部署，是独立的运维工具。

---

## 十四、依赖关系

```
本方案依赖:
  - spend_logs 表（已存在，不需要前置改动）
  - parquet crate v54+ (arrow-rs)，features: arrow + zstd（新增）
  - arrow crate v54+（新增）
  - object_store crate v0.11+，features: aws（新增）
  - moka crate v0.12+，features: sync（新增）
  - redis crate v0.25（可选，仅 footer_cache=redis 时需要）

本方案不阻塞:
  - 任何现有功能
  - 任何现有 API

后续可扩展:
  - Parquet → Iceberg/Delta Lake 表格式（如果需要 schema 演进）
  - 日 compaction（减少小时文件碎片）
  - S3 生命周期自动删除 90 天前的 Parquet


## 十五、参考链接

| 资源 | URL |
|------|-----|
| DuckDB httpfs S3 API | https://duckdb.org/docs/current/core_extensions/httpfs/s3api |
| DuckDB Pragmas (enable_object_cache) | https://duckdb.org/docs/current/configuration/pragmas.html |
| OpenObserve Cargo.toml | https://raw.githubusercontent.com/openobserve/openobserve/main/Cargo.toml |
| OpenObserve architecture | https://openobserve.ai/docs/architecture/ |
| parquet crate (arrow-rs) | https://crates.io/crates/parquet |
| object_store crate | https://crates.io/crates/object_store |
| moka cache crate | https://crates.io/crates/moka |
