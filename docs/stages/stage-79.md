# Stage 79: Body Archive — Query Router + Footer Cache（读路径）

**Phase**: 30 — Body Archive 冷存储
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 10h
**前置**: Stage 78（迁移就绪 + BodyArchiver 写链路可生成测试数据）

---

## 背景

Stage 78 建立了 AsyncTask + Engine 框架和 Body Archiver 写链路。本阶段实现读路径。

详见：`docs/plans/2026-07-22-body-archive-s3-parquet.md` 第三、四、五节

## 目标

1. 查询路由：`get_message_body()` 热数据查 DB，冷数据查 Parquet
2. Parquet 读路径：footer 缓存 → row group 定位 → col chunk → 过滤
3. Footer 缓存：moka 内存 LRU
4. 详情端点集成存储回源

## 验收标准

- [ ] `get_message_body` — DB 有 body → 直接返回
- [ ] `get_message_body` — DB 无 body + `body_archived=true` → 查 Parquet → 返回
- [ ] `get_message_body` — DB 无 body + `body_archived=false` → None
- [ ] `get_message_body` — 记录不存在 → None
- [ ] Footer 缓存命中 → 跳过 footer 请求
- [ ] Footer 缓存 TTL 过期 → 重新下载
- [ ] `GET /global/spend/logs/{request_id}` — 冷数据自动回源
- [ ] 存储后端不可达 → error，不 crash

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-core/src/body_archive/query.rs` | 新增：Parquet 查询 + 缓存流水线 |
| `crates/aigw-core/src/body_archive/cache.rs` | 新增：FooterCache（moka LRU） |
| `crates/aigw-core/src/body_archive/mod.rs` | 修改：集成 get_message_body |
| `crates/aigw-server/src/routes/spend.rs` | 修改：详情端点存储回源 |

## 技术方案

### 查询路由

```rust
impl BodyArchiver {
    pub async fn get_message_body(&self, db: &Database, request_id: &str)
        -> Result<Option<BodyPayload>>
    {
        let meta = sqlx::query_as::<_, SpendLogMeta>(
            "SELECT messages, response, proxy_server_request,
                    body_archived, parquet_path
             FROM spend_logs WHERE request_id = ?"
        ).bind(request_id).fetch_optional(db).await?;

        match meta {
            Some(m) if m.messages.is_some() => Ok(Some(m.into_body())),
            Some(m) if m.body_archived && m.parquet_path.is_some() =>
                self.query_parquet_with_cache(&m.parquet_path.unwrap(), request_id).await,
            Some(_) => Ok(None),
            None => Ok(None),
        }
    }
}
```

### Parquet 查询流水线

```rust
async fn query_parquet_with_cache(&self, path: &str, request_id: &str)
    -> Result<Option<BodyPayload>>
{
    // 1. footer metadata（缓存命中 → 0 网络请求）
    let meta = match self.footer_cache.get(path) {
        Some(c) => c,
        None => {
            let size = self.object_store.head(path).await?.size;
            let bytes = self.object_store
                .get_range(path, size.saturating_sub(8192)..size).await?;
            let meta = Arc::new(ParquetMetaDataReader::new().decode(&bytes)?);
            self.footer_cache.put(path, meta.clone());
            meta
        }
    };
    // 2. row group 定位（column statistics + Bloom filter）
    let rg = locate_row_group(&meta, request_id)?;
    // 3. 读 col chunk → 解码 → 过滤
    let chunk = self.read_col_chunk(path, &meta, rg, "messages").await?;
    decode_messages_column(&chunk, request_id)
}
```

### Footer 缓存（moka）

缓存解析后的 `ParquetMetaData`（含 row group 偏移量、statistics、Bloom filter 位置）。

### Row Group 定位

ROW_GROUP_SIZE=5000 → P99 场景在 1 个 row group 内。通过 request_id 列的 statistics（min/max）跳过不匹配的 row group。

### 详情端点集成

```rust
pub async fn global_spend_log_detail(/* ... */) -> Result<Json<SpendLogDetail>> {
    require_admin(&auth)?;
    let log = state.db.get_spend_log_by_request_id(&request_id)?;
    match log {
        Some(log) => {
            let body = if log.messages.is_none() && log.body_archived {
                state.body_archiver.get_message_body(&state.db, &request_id).await.ok().flatten()
            } else { None };
            Ok(Json(log.into_detail(body)))
        }
        None => Err(AppError::NotFound),
    }
}
```

## 测试要求

- 热数据命中 → 返回 body，不查存储
- DB body=NULL + body_archived=true → 查 Parquet → 返回
- DB body=NULL + body_archived=false → None
- 不存在 → None
- Footer cache put/get、miss、TTL 过期
- 存储后端 error → 不 crash

## 依赖

Stage 78

## 不做

- Col chunk 缓存（Stage 80）
- Parquet 加密
