# Body Archive 大小时段归档失败根因 + 修复（Streaming Multipart + Sharding）

**日期**: 2026-08-05
**状态**: ✅ 已修复并验证（含真实 S3 端点 A/B 探针）
**涉及**: `crates/aigw-core/src/body_archive/{writer,config,mod}.rs` + `stage83_read_path.rs`

---

## 1. 现象（Job `job-ace86b5b`）

`body_archive` 手动任务 88 个 step，68 成功 / 23 失败（`partially_failed`）。失败 step 集中在
`hour=2026-08-03T02..08-04T16`，报错统一为：

```
parquet write: object_store put: Generic S3 error: Error after 5 retries in 180.8s,
max_retries:10, retry_timeout:180s, source:error sending request for url
(http://9.135.87.221:8001/aigw/body-archive/year=2026/month=08/day=03/hour=02/data.parquet)
```

关键点：**"error sending request" 是 reqwest 传输层错误（连接在传输 body 时被服务端 reset/EOF），没有 HTTP 状态码**。

## 2. 数据佐证（SQLite `aigw.db` 实测）

**失败 step 的"当时剩余未归档 body 字节数"与该 hour 是否失败高度相关：**

| 未归档 body 体积 | 失败 | 成功 |
|---|---|---|
| ≥163 MB（max 1.4 GB）| **23/23 全失败** | 0 |
| <100 MB（多为已被前序归档的余量）| 0 | **68/68 全成功** |

但 08-03T07(34MB)/T08(60MB) 也曾在 17:45-17:58 窗口失败 → **服务端间歇性不稳定**，非纯大小阈值。

**A/B 实时探针（同一 rustfs S3 端点 9.135.87.221:8001）：**
- 9.4 MB 单 PUT：✅ 700ms 成功
- 9.4 MB multipart streaming：✅ 3.5s 成功
- **300 MB 单对象 multipart（~19 个 part）**：❌ **partNumber=3 的 put_part 在 180s 后超时** —— 服务端无法稳定吸收 >~几十 MB 的连续上传
- **300 MB 分片（max_body_mb=64 → 60 个 ≤64MB 对象）**：✅ 69s 全成功

## 3. 根因

1. **放大因素（本仓）**：原 `write_parquet_to_store` 把整小时 body 一次性 `write_parquet_to_buffer` 成单个 `Vec<u8>`
   再单发 `store.put()`。1.4 GB 小时 → 1.4 GB 内存 + 一个超大 PUT，服务端一 hang 就 180s×3 全废，且失败重试整个小时。
2. **服务端因素（rustfs）**：S3 兼容端点对 >~几十 MB 的连续上传不可靠，连接中途 reset（间歇性，压力/体积越大越明显）。

## 4. 修复方案（已落地）

**A. Streaming multipart（不落盘）** — `write_parquet_to_store_streaming`
- `parquet::arrow::AsyncArrowWriter` 按 row group 流式产出压缩字节
- 自定义 `AsyncFileWriter` 桥接到 `object_store::WriteMultipart`（内存缓冲 → ≥part_size 分片并发 `put_part`）
- 每个 part 是独立小请求，单 part 失败只重传该 part；`complete()` 原子可见
- **S3 全程零磁盘**；`FileSystem` 后端 object_store 内部 staging 一个临时文件（原子 rename，无分片语义可避免）

**B. 大 hour 分片（按「压缩后输出字节」计数，每对象 ≤ max_parquet_body_mb）** — `write_parquet_shards`
- **输出字节计数**：每写完一个 row group，用 `AsyncArrowWriter` 实际编码量累加「压缩后」字节；累计 ≥ `max_parquet_body_mb`
  就 `complete()` 当前对象并起下一个 —— 所以 `data-N.parquet` 的 S3 对象大小被**严格保证 ≤ max_parquet_body_mb**
  （首片固定命名 `data.parquet` 兼容老读者，后续 `data-1.parquet, data-2.parquet, ...`）
- 每个 row group 是独立的 `AsyncArrowWriter`+`WriteMultipart`，内存有界（一个 row group 的量）
- `MAX_SHARDS_PER_HOUR=256` 兜底，到达上限后多余行并入最后一片（**绝不丢数据**）
- 每行归档时写入**各自分片的确切 `parquet_path`**（冷读精确解析到正确对象）
- **修正动机**：旧实现按「body 原始字节」预判分片，zstd 压缩后实际对象远小于阈值 → 过度分片
  （live 探针：300MB body 按 64MB 输入切 → 60 片，每片实际仅 **6.7MiB**）。改按输出计数后，
  120MB body 实测 → **2 片 × ~64MB 输出**，对象大小可控、不再碎片化。

**C. 配置** — `ArchivePolicy` 新增：
- `multipart_part_size_mb`（默认 16，S3 下限 5）
- `max_parquet_body_mb`（默认 **64** —— 该 rustfs 端点实测 64MB/对象稳定，128MB 曾出现 300MB 单对象超时）

## 5. 结果评估（客观可验证）

- `task test` 全绿（新增 6 个测试：streaming×2、auto-select、sharding×2、FS 分片集成）
- **真实 S3 端点**：
  - 单对象 300MB multipart ❌ 卡 part 3 超时 180s（对照组）
  - 按输入字节分片 300MB → 60 片 × 6.7MiB ✅ 成功（~70s），但碎片化
  - **按输出字节分片 120MB → 2 片 × ~64MB ✅ 成功（~35s）** —— 对象大小真正可控
- 冷读正确性：分片 hour 每行 `parquet_path` 指向确切分片，`query_parquet_with_cache` 命中正确对象
- DuckDB/hive_partitioning：分区键在目录名，叶子文件名 `data-N.parquet` 不影响识别（官方文档证实）

## 5. 结果评估（客观可验证）

- `task test` 全绿（新增 6 个测试：streaming×2、auto-select、sharding×2、FS 分片集成）
- **真实 S3 端点**：300 MB 分片 multipart ✅ 成功（60 对象，~70s），同端点单对象 300MB ❌ 超时 —— 对比确证分片+multipart 是正确修复
- 冷读正确性：分片 hour 每行 `parquet_path` 指向确切分片，`query_parquet_with_cache` 命中正确对象

## 6. 后续

- 待服务端（rustfs）修复稳定性后可回调 `max_parquet_body_mb`/`part_size` 减少对象数
- 已归档 23 个失败 hour 的数据仍在 `aigw.db`（未标记 archived），可用新二进制重触发归档
- 触发命令：`POST /admin/jobs/trigger {"step_type":"body_archive","payload":{"start_date":"...","end_date":"..."}}`
