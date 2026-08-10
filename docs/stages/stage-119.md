# Stage 119: exact-match 响应缓存（S3）

**所属**: Phase 47（A 类接线 + 缓存）
**预估**: 10h（后端 + 测试）
**依赖**: 无（独立新能力；`calc_spend` 三级缓存计费可复用）
**状态**: ⏳ 待开始

---

## 1. 目标

落地 **exact-match 响应缓存**——差距报告确认的全部竞品标配能力（litellm caching 矩阵 / Portkey / Cloudflare 边缘 / Higress ai-cache），aigw 当前为 0：

1. **内存缓存层**（moka LRU）+ `CacheBackend` trait 预留 Redis
2. **缓存读写**：非流式响应组装后入缓存；流式组装后入缓存；TTL 可配
3. **cache 控制**：`cache={"use-cache","no-store","ttl"}` 解析 + `X-Cache-Status: HIT/MISS` 头
4. **cache-hit 计费 0 元**：命中时 `response_cost=0`（复用 `calc_spend` 缓存计费逻辑）
5. **config 接线**：`config.yaml` 增 `cache` 块，boot 注入 AppState

## 2. 现状证据

| 项 | 现状 | 证据 |
|----|------|------|
| 缓存层 | 无（=0） | 无 cache 模块 |
| cache tokens 计费 | 已支持三级缓存差异化计费 | `calc_spend`（Stage 36） |
| 流式组装 | `stream_chunk_builder` 存在 | 可复用做流式缓存 |

## 3. 方案

### 3.1 缓存层（新模块 `aigw_core::cache`）

```rust
pub trait CacheBackend: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<CachedResponse>>;
    async fn put(&self, key: &str, resp: &CachedResponse, ttl: Duration) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
}
pub struct MemoryCache { store: moka::sync::Cache<String, CachedResponse> } // LRU
pub struct CachedResponse { status: u16, headers: HeaderMap, body: Bytes }
```

- **cache key** = SHA-256(provider + endpoint + model + auth + canonical body)（对标 litellm `Cache.get_cache_key`）。
- **TTL 默认 60s**（`cache={"ttl"}` 请求级覆盖；config 全局默认可调）。

### 3.2 读写路径（chat.rs / v1_messages.rs）

- **非流式**：响应组装后入缓存（`X-Cache-Status: MISS`）；下次命中直接返回缓存 body（`X-Cache-Status: HIT`）+ 不调上游。
- **流式**：`stream_chunk_builder` 组装完整流后入缓存（对标 litellm `_add_streaming_response_to_cache`）；流式命中时重放缓存 body 为 SSE。
- **缓存对象不含 request_id/call_id**（每个请求独立生成响应头），body 透传。

### 3.3 cache 控制

- 请求 `cache` 字段解析：`{"use-cache": bool, "no-store": bool, "ttl": seconds}`（OpenAI Chat Completions cache 扩展，对标 litellm/Cloudflare）。
- `no-store` / `use-cache: false` → 绕过缓存层；命中但 `no-store` → 照常回上游。

### 3.4 cache-hit 计费 0 元

- 缓存命中路径：SpendLog `response_cost=0` + `cached=1` 标记（litellm cache_hit 行为）；prompt/completion token 记实际缓存响应 usage。
- `calc_spend` 的 cache 三级计费逻辑复用（缓存命中时总 cost 归零）。

### 3.5 config 接线

```yaml
cache:
  enabled: true
  backend: memory      # memory | redis(预留)
  ttl_seconds: 60
  max_entries: 1000
```

- `config.rs` 解析 + `main.rs` boot 构建 `MemoryCache` 注入 AppState（`state.cache: Option<Arc<dyn CacheBackend>>`，`enabled=false` 时 None）。

## 4. 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-core/src/cache/mod.rs` | 新增 | `CacheBackend` trait + `CachedResponse` + 缓存 key 构造 |
| `crates/aigw-core/src/cache/memory.rs` | 新增 | moka LRU 后端 |
| `crates/aigw-core/src/cache.rs` | 修改 | 模块导出 |
| `crates/aigw-core/src/config.rs` | 修改 | `cache` 配置块 |
| `crates/aigw-server/src/main.rs` | 修改 | boot 构建注入 AppState |
| `crates/aigw-server/src/routes/chat.rs` | 修改 | 缓存读写 + X-Cache-Status + cache-hit 计费 |
| `crates/aigw-server/src/routes/v1_messages.rs` | 修改 | 同上 |
| `crates/aigw-server/src/routes/spend.rs`（如需要）| 修改 | cached 标记透传 |

## 5. TDD

- **cache UT**（8-10）：key 构造确定性 / get miss→put→get hit / TTL 过期 / LRU 淘汰 / no-store 绕过 / 缓存 body 保真（headers+body）。
- **handler UT**（2-4）：非流式 MISS→HIT、流式组装后入缓存、cache-hit 计费 0 元。
- **mock BDD**（4-5）：`X-Cache-Status: HIT/MISS` / no-store 绕过 / TTL 过期后 MISS / cache-hit 计费 0 元（SpendLog response_cost=0 + cached=1）。

## 6. 验收标准

- [ ] `task test` / `task bdd` / `task bdd-real-*` 全绿
- [ ] `task fmt` / `task lint` 全绿
- [ ] `X-Cache-Status: HIT/MISS` 头正确；no-store 绕过；TTL 过期后 MISS
- [ ] cache-hit 请求 SpendLog `response_cost=0` + `cached=1`（BDD 断言）
- [ ] config `cache.enabled=false` 时缓存层零开销（None 路径）

## 7. 参考实现

- litellm `caching/caching.py` + `caching_handler.py`（key 构造 + 流式缓存）
- litellm cache_hit → `response_cost=0.0`
- Higress ai-cache（GJSON PATH 取末条 user 消息做 key）
- Cloudflare AI Gateway（`cf-aig-cache-status` HIT/MISS 头）
