# Stage 63: Schema 修复 + Router Core

**Phase**: 23 — Router 负载均衡
**状态**: ✅ 完成
**耗時**: 8h
**依賴**: 無（獨立於 Phase 21-22）

---

## 目标

1. **Schema 修复** — 去掉 `proxy_models.model_name` UNIQUE INDEX，放开同名多 deployment
2. **aigw-migrate 补齐** — 完整迁移多 deployment + `config.router_settings` 行
3. **Router Core** — 实现 `Router` 策略引擎：shuffle pick + cooldown + failure tracking + retry loop

---

## Part A — Schema 修复 (3h)

### 1.1 Migration

新增 `migrations/sqlite/014_proxy_models_non_unique.sql`:

```sql
DROP INDEX IF EXISTS idx_proxy_models_model_name;
CREATE INDEX IF NOT EXISTS idx_proxy_models_model_name ON proxy_models(model_name);
```

新增 `migrations/postgres/014_proxy_models_non_unique.sql`:

```sql
DROP INDEX IF EXISTS idx_proxy_models_model_name;
CREATE INDEX IF NOT EXISTS idx_proxy_models_model_name ON proxy_models(model_name);
```

新增 `migrations/mysql/014_proxy_models_non_unique.sql`:

```sql
DROP INDEX idx_proxy_models_model_name ON proxy_models;
CREATE INDEX idx_proxy_models_model_name ON proxy_models(model_name);
```

### 1.2 aigw-migrate 修改

**A. 放开同名 model_name 限制**

当前 `migrate_plain_table` 对 proxy_models 使用 `INSERT OR REPLACE`（基于 model_name 唯一），改为：
- 目标表先清空 + 全量 `INSERT INTO`（不做 REPLACE），因为 model_name 不再是唯一 key

```rust
// Before: INSERT OR REPLACE INTO proxy_models ...
// After: 先 DELETE FROM proxy_models; 再 INSERT INTO proxy_models ... (model_id 是实际唯一标识)
```

**B. 补齐 config 表 `router_settings` 行**

`migrate_plain_table` 已覆盖 `LiteLLM_Config → config`，但之前的 UNIQUE 约束（`param_name`）可能导致旧 run 中 router_settings 行因冲突未迁入。改为先清空再全量 INSERT。

验证：
```bash
aigw-migrate import --source-url=... --target-url=...
sqlite3 aigw.db "SELECT param_name FROM config"
# 期望输出包含: router_settings, general_settings, litellm_settings, ...
```

---

## Part B — Router Core (5h)

### 2.1 新增文件

`crates/aigw-core/src/router.rs`

### 2.2 Router 结构

```rust
use rand::seq::SliceRandom;
use std::time::Instant;

#[derive(Debug, Clone)]
pub enum RouterStrategy {
    SimpleShuffle,
    // Future: LeastBusy, UsageBasedRouting, LatencyBased
}

impl RouterStrategy {
    pub fn from_str(s: &str) -> Self {
        match s {
            "simple-shuffle" => Self::SimpleShuffle,
            other => {
                tracing::warn!(strategy=%other, "unknown routing strategy, fallback to simple-shuffle");
                Self::SimpleShuffle
            }
        }
    }
}

pub struct Router {
    strategy: RouterStrategy,
    pub allowed_fails: u32,
    pub cooldown_time: f64, // seconds
    pub num_retries: u32,
}

impl Router {
    pub fn new(strategy: RouterStrategy, allowed_fails: u32, cooldown_time: f64, num_retries: u32) -> Self {
        Self { strategy, allowed_fails, cooldown_time, num_retries }
    }

    pub fn from_config(cfg: &RouterConfig) -> Self { ... }

    /// Pick one deployment index from the candidates.
    /// Returns None only if the input slice is empty.
    pub fn pick_deployment(&self, deployments: &mut [Deployment]) -> Option<usize> {
        let now = Instant::now();

        // 1. Filter out cooldown deployments
        let active: Vec<usize> = (0..deployments.len())
            .filter(|&i| {
                deployments[i].cooldown_until
                    .map_or(true, |t| now >= t)
            })
            .collect();

        if active.is_empty() {
            // All are in cooldown — return the one that recovers earliest
            tracing::warn!("All deployments in cooldown, picking earliest recovery");
            return (0..deployments.len()).min_by_key(|&i| {
                deployments[i].cooldown_until.unwrap_or(Instant::now())
            });
        }

        // 2. Shuffle and pick
        let mut rng = rand::thread_rng();
        let mut picked = active.to_vec();
        picked.shuffle(&mut rng);
        let idx = picked[0];

        Some(idx)
    }

    /// Report a failure on a deployment.
    pub fn report_failure(&mut self, deployment: &mut Deployment) {
        deployment.fail_count += 1;
        if deployment.fail_count >= self.allowed_fails {
            let cooldown = std::time::Duration::from_secs_f64(self.cooldown_time);
            deployment.cooldown_until = Some(Instant::now() + cooldown);
            tracing::warn!(
                model_name=%deployment.model_name,
                fail_count=%deployment.fail_count,
                cooldown_secs=self.cooldown_time,
                "Deployment entering cooldown"
            );
        }
    }

    /// Clear failure state on success.
    pub fn report_success(&mut self, deployment: &mut Deployment) {
        deployment.fail_count = 0;
        deployment.cooldown_until = None;
    }
}
```

### 2.3 Deployment 扩展

`crates/aigw-core/src/deployment.rs`:

```rust
pub struct Deployment {
    // ... existing fields ...
    /// Runtime cooldown tracking (not persisted)
    #[serde(skip)]
    pub fail_count: u32,
    #[serde(skip)]
    pub cooldown_until: Option<std::time::Instant>,
}
```

### 2.4 Handler 改造

两个 handler（`chat.rs` + `v1_messages.rs`）：

```rust
// Before
let deployments = state.resolver.resolve(_model).await?;
let deployment = deployments.into_iter().next().ok_or_else(|| { ... })?;

// After — with retry loop
let mut last_err = None;
let deployments_snapshot = state.resolver.resolve(_model).await?;

for attempt in 0..=router.num_retries {
    let mut deployments = deployments_snapshot.clone();
    // Each attempt: re-pick from the full list (cooldown filter applies inside pick_deployment)
    let idx = router.pick_deployment(&mut deployments).ok_or_else(|| { ... })?;
    let mut deployment = deployments.remove(idx);

    match make_upstream_call(&deployment, ...).await {
        Ok(resp) => {
            router.report_success(&mut deployment);
            return Ok(resp);
        }
        Err(e) if is_retryable(&e) && attempt < router.num_retries => {
            router.report_failure(&mut deployment);
            tracing::warn!(attempt, "Retryable failure, retrying...");
            last_err = Some(e);
            continue;
        }
        Err(e) => {
            router.report_failure(&mut deployment);
            return Err(e);
        }
    }
}
Err(last_err.unwrap())
```

**注意**: `Router` 需要放在 `Arc<RwLock<Router>>` 或 `Arc<Mutex<Router>>` 中，因为 `report_failure` 和 `report_success` 会修改内部状态。放入 `AppState`。

### 2.5 可重试错误判断

```rust
fn is_retryable(e: &AppError) -> bool {
    match e.status() {
        StatusCode::TOO_MANY_REQUESTS  // 429
        | StatusCode::BAD_GATEWAY       // 502
        | StatusCode::SERVICE_UNAVAILABLE // 503
        | StatusCode::GATEWAY_TIMEOUT   // 504
        | 0 => true,  // 网络超时/连接错误
        _ => false,
    }
}
```

---

## 单元测试（8）

| # | 场景 | 验证点 |
|---|------|--------|
| 1 | pick: 单 deployment | 返回 index 0 |
| 2 | pick: 多 deployment 随机 | 两次 pick 结果可能不同（概率测试，跑 100 次至少两种不同 index） |
| 3 | pick: cooldown 过滤 | 一个 cooldown → 只选另一个 |
| 4 | pick: 全部 cooldown | 返回最早恢复的 |
| 5 | report_failure: 未达阈值 | fail_count++ 但不设 cooldown_until |
| 6 | report_failure: 达阈值 | cooldown_until 被设为 now + cooldown_time |
| 7 | report_success: 清除 | fail_count 归零，cooldown_until = None |
| 8 | Resolver: 多 deployment resolve | 去掉 UNIQUE 后，同名 model_name 返回多个 Deployment |

---

## 门禁

- [ ] `cargo test` 全量通过（含新增 8 UT）
- [ ] `task migrate` 集成测试：同名多 deployment 完整迁入
- [ ] BDD 回归通过（97 scenarios）
