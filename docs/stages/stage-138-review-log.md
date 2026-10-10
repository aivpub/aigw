# Stage 138 Review Log

**Review Type**: Design
**Review Date**: 2026-10-09
**Reviewer**: Claude (main) —— 逐项对照代码核实（未采信设计文档的自述锚点）
**Stage**: Stage 138（搜索 Provider / 实例 / 定价的 DB 化与管理 UI）

---

## 编码期必查项回填（2026-10-10，交付时核对）

| # | 必查项 | 结果 | 载体 |
|---|--------|------|------|
| 1 | F1：确认没有任何全局字段被写进两张表；`Guardrails` 四个字段仍来自 yaml | ✅ | `028_*.sql` 无 `proxy_url` / 域名黑白名单 / `snippet_max_chars` / `retries` / `allowed_fails` / `cooldown_secs` / `max_results` 列；`test_db_rows_do_not_touch_global_policy` |
| 2 | F2：写操作 → 不重启 → 下一次搜索用新单价；36 处字面量全改 | ✅ | `price_change_is_live_without_restart`（handler UT）；**实测**（§7.2）改价 `0.000012 → 0.0005` 后下一次搜索行 spend 即为 `0.0005`。字段类型变更由编译器强制（字面量改不过来就编译失败） |
| 3 | F3：`UNIQUE(name)` 与「默认 = `failover_order` 最小」各一条 UT（含并列、含全 NULL） | ✅ | `test_websearch_provider_name_is_unique` / `test_default_provider_is_min_failover_order`（覆盖无 order → 最小 id、有 order 优先于无 order、更小的 order 胜出、禁用者落选） |
| 4 | F4：级联 UT 三方言各跑一遍，MySQL 是哨兵 | ✅ **三方言均已跑** | `test_websearch_instance_cascade_delete`（sqlite UT）+ real BDD 三驱动各 67/67（2026-10-10 补跑 pg/mysql）。**补跑的价值当场兑现**：首跑即暴露四处方言类型缺陷（`enabled` INTEGER↔i64、MySQL DATETIME↔String、`enabled = 1`、`UPDATE_WSI_PG` 的 `$9`），见 stage-138 §7.4 |
| 5 | F5：探测构造单实例临时池，不复用共享 `InstancePool` | ✅ | `websearch/probe.rs` 自建单实例请求（`test_probe_takes_one_instance_not_a_pool`），不触碰注册表与共享池冷却 |
| 6 | F7：写路径 kind 校验 + loader fail-soft 两者都要；reload 失败保留旧 registry | ✅ | `validate_kind` → 400（`unsupported_kind_is_rejected_with_a_hint`）；loader fail-soft（`test_unbuildable_row_is_skipped_not_fatal`）；`failed_reload_keeps_the_previous_registry` |

**交付时新增的两条口径修正**（设计未预见，已在 stage-138 §7.3 记录）：

1. **「DB 里一行 enabled 都没有」不再是错误** —— `build_websearch_registry_from_rows` 对空 provider 列表返 `Ok(None)` 而非 `Err`。原设计的 fail-soft 只覆盖「构造不出的行」；但「最后一行被 toggle 关掉」同样会走到空列表，返 `Err` 会让 reload 失败、旧 registry 被保留 → **toggle 看起来没生效**。yaml 路径保持「空列表 = 错误」（那是操作员能改的文件）。
2. **in-use 守卫的错误文案** —— type 保留 `WEB_SEARCH_PROVIDER_IN_USE`（与 `proxies` 形态一致），message 明确写「这是默认 provider，请先提升另一个」。判据是 Review F3/F9 已定的「默认那一行」。

---

## Review Summary

设计的主体判断成立：`proxies`（Phase 50）确实是**逐项可照抄**的同构先例，`028` 号可用（迁移目录最大为 `027_proxies.sql`），
三方言目录文件集完全对齐（`sqlite == postgres == mysql`），`WebSearchConfig` 的两层结构（provider / instances）
与两张表的划分一致，且 `k` 内已有「单价由运营在 UI 改」的既定做法（`ModelDialog.tsx:93-96/166-167` 已核实）。

但设计在**四处**会直接导致实现返工或线上事故的地方不成立：全局配置字段无归宿（F1）、运行期 reload 与
`AppState` 的实际形态冲突且变更清单漏项（F2）、`is_default` 无唯一性约束（F3）、以及
**一条能被保存的 DB 行可让下次启动 `exit(1)`**（F7，**最高危** —— 服务再也起不来且无 UI 可改）。
另有 3 项 Medium（MySQL 内联 FK 静默失效、探测无单实例入口、in-use 守卫口径变化）
与 3 项 Low（`ALL_TABLES` 漏改、锚点漂移、工作量低估）。

**结论：需先改设计**（F1/F2/F3/F7 四项必修，F4/F5/F9 须写死处置）。

---

## Findings

### F1 — 全局配置字段在 DB 化后**无家可归**，且其中一个（`proxy_url`）是安全控制（High）

设计 §3.1 的两张表只覆盖 provider / instance 两个层级，但 `WebSearchConfig` 还有 **7 个顶层字段没有对应列**：

| 字段 | 定义处 | 语义 | 设计是否提及 |
|------|-------|------|-------------|
| `retries` | `config.rs:54` | 传输层重试 | ❌（全文 0 命中） |
| `snippet_max_chars` | `config.rs:58` | 单条 snippet 截断 | ❌ |
| `allowed_domains` | `config.rs:62` | **域名白名单**（护栏） | ❌ |
| `blocked_domains` | `config.rs:66` | **域名黑名单**（护栏） | ❌ |
| `proxy_url` | `config.rs:70` | **搜索出站代理**（egress 路径） | ❌（3 处命中，全是引用 `proxies` 的写法，非本设计字段） |
| `allowed_fails` | `config.rs:74` | 实例冷却阈值 | ❌ |
| `cooldown_secs` | `config.rs:78` | 实例冷却时长 | ❌ |

`max_results`（`config.rs:46`）与 `timeout_ms`（`config.rs:50`）被设计搬到了 **provider 表**（每行一份），
但这与运行时用法不符：`Guardrails` 是**全局单例**（`config_loader.rs:236-241` 一次性构造，`types.rs:90-99` 定义
`max_results` / `snippet_max_chars` / `allowed_domains` / `blocked_domains` 四字段），而不是 per-provider。

**失败场景**：部署方原有 yaml 配了 `blocked_domains`（护栏）与 `proxy_url`（出站代理）。他在新 UI 里建了 1 个 provider
→ 按 §3.2「DB 有记录 → 用 DB，忽略 yaml」→ **护栏与出站代理同时静默失效**，搜索流量改走直连、被屏蔽域名重新进入
模型上下文。这不是「配置不生效」的可用性问题，是**安全控制静默丢失**。

**处理（必改）**：
1. 明确分工：**全局策略字段（护栏 / 出站代理 / 重试 / 冷却）保持 yaml 唯一来源，不进 DB**——它们是基础设施策略，
   不是 per-provider 数据，放进任何一张按实体建模的表都是错位的。§3.1 的 provider 表**删去 `max_results` /
   `timeout_ms` 之外的全局字段**（这两个 per-provider 覆盖有 `effective_timeout_ms`（`config.rs:257-259`）先例，可留；
   `max_results` 无 per-provider 先例，**应删**，回归 `Guardrails`）。
2. §3.2 的「不做并集」**限定作用域**：它针对的是 **provider 列表**（同一事实的两个来源才是排查地狱）；
   全局字段只有 yaml 一个来源，不存在并集问题。设计需把这句话写清楚，否则实现者会以为整个 block 都要二选一。
3. 启动日志同时打印「providers 来源 = DB|yaml」与「护栏/代理来源 = yaml」。

### F2 — 运行期 reload 与 `AppState` 的实际形态冲突，且 §5 变更清单漏了改动面（High）

§3.6 要求「原子替换 `AppState` 里的 registry」，但实际形态是：

```rust
pub type SharedState = Arc<AppState>;                    // keys.rs:113
pub web_search: Option<Arc<WebSearchRegistry>>,          // keys.rs:77  (字段，非锁)
```

读者是 `state.web_search.as_deref()`（`chat.rs:1164`、`responses.rs:612`、`v1_messages.rs:846`）。
`Arc<AppState>` 内的裸字段**无法在运行期替换** —— 必须把字段改成 `Option<Arc<RwLock<…>>>` 或 `ArcSwap`，
于是**三个读者全部要改**（改为先 clone 内层句柄、再在 await 边界外读），字段声明与 `main.rs:363-406` 的构造也要改。

**核对 §5 变更清单**：列了 migration / models.rs / db.rs / config.rs / probe.rs / 新路由 / main.rs 路由注册 / 前端五文件 / i18n / 侧边栏
—— **没有任何一行提到 `keys.rs`、`chat.rs`、`responses.rs`、`v1_messages.rs`**。门禁「改价不重启生效」按此清单**做不出来**。

**处理（必改）**：§3.6 写死机制（建议 `Arc<ArcSwap<Option<WebSearchRegistry>>>`，读路径无锁），
并把 `keys.rs` 字段类型 + 3 个读者 + `main.rs` 构造列进 §5。
> 补充核实：`web_search: None` 的结构体字面量全仓 **36 处**（`keys.rs` 6 处 + 测试 30 处）——若字段类型仍为
> `Option<T>`，这些字面量**无需改动**（`None` 对任意 `T` 合法）。这降低但不是消除改动面；设计不应因此略过读者侧。

### F3 — `is_default` 无唯一性约束，与 yaml 的「名字」语义不等价（High）

`WebSearchConfig.default_provider: String` 是**名字**，`failover_order: Vec<String>` 也是名字列表，
二者由 `attempt_order()`（`config.rs:244-252`）合成：default 打头 + failover 去重追加。
设计的 DDL 同时给出 `is_default INTEGER` 与 `failover_order INTEGER` 两列，但：

- **无任何约束防止两行 `is_default=1`** → reload 时「默认 provider 是谁」不确定；
- **无规定 0 行 `is_default=1` 时的行为**（若 DB 非空却无默认，`attempt_order()` 就没有打头项）；
- `failover_order` 并列（同值）时的次序未定义。

**处理（必改）**：三选一并写进 §3.1 + §4.1 UT：
① `is_default` 唯一性由**写路径**保证（任一行置 1 时在同一事务内清其它行），加载时 >1 报错、=0 回落 `failover_order` 最小者；
② 或**删掉 `is_default`**，只用 `failover_order`（`0` = 默认），一位信息一个来源（更符合奥卡姆）；
③ 或**删掉 `failover_order`**，默认由 `is_default` 决定、故障转移顺序按 `id`。
建议 ②。

### F4 — SQLite 级联：**方向反了** —— 会坏的是 MySQL，不是 SQLite（Medium）

**我先前的判断（本 log 初稿）错了，此处更正。** 初稿写「SQLite 的 `PRAGMA foreign_keys` 未开启 → 级联静默失效」，
依据是「全仓无 `PRAGMA foreign_keys`」。该依据不成立：**sqlx-sqlite 默认就开启它**。

实测（`Cargo.lock:4648` 锁 **sqlx-sqlite 0.8.6**；源码 `~/.cargo/registry/.../sqlx-sqlite-0.8.6/src/options/mod.rs:183-185`）：

```rust
// We choose to enable foreign key enforcement by default, though SQLite normally
// leaves it off for backward compatibility
pragmas.insert("foreign_keys".into(), Some("ON".into()));
```

且 `FromStr`（`options/parse.rs:18` `let mut options = Self::default();`）与 `impl ConnectOptions`（`options/connect.rs:36-39`
每条连接执行 `pragma_string()`）都从 `default()` 出发 → **`Database::init("sqlite:…")` 建出的每一条池连接都开着 FK**。
（顺带纠正另一处：`db.rs:138` 的 `init_pool` 走字符串 `connect(url)`，但那是 sqlx 的 URL-解析路径，同样落到 `SqliteConnectOptions`，
结论相同；且该函数**全仓无调用点**，是死代码。）

**真正会坏的是 MySQL**：设计 §3.1 给出一份内联 DDL 片段
（`provider_id INTEGER NOT NULL REFERENCES web_search_providers(id) ON DELETE CASCADE`）并标注「三方言一致性须核实」，
但**MySQL/InnoDB 会解析并忽略列的 `REFERENCES` 子句** —— 它只认表级的 `FOREIGN KEY (…) REFERENCES …`。
仓内自己的先例正是这么写的（`mysql/020_async_jobs.sql:37,48`）：

```sql
CONSTRAINT fk_async_job_steps_job FOREIGN KEY (job_id) REFERENCES async_jobs(id) ON DELETE CASCADE
```

**失败场景**：把 SQLite 那份内联 DDL 抄给 MySQL → 无级联 → 删 provider 后实例行成孤儿；而 real BDD 在 MySQL 上
跑「删 provider → 实例一并消失」会失败（或更糟：测试没覆盖 MySQL 的这一条，缺陷静默上线）。

**处理（改设计）**：§3.1 **写出三份具体的 DDL 体**（不是一份片段加映射注释）：
① SQLite / PG 用内联 `REFERENCES … ON DELETE CASCADE`（sqlx 已开 FK，PG 原生强制）；
② **MySQL 必须用表级 `CONSTRAINT … FOREIGN KEY (…) REFERENCES … ON DELETE CASCADE`**。
③ 级联的 UT 须在**三方言各跑一遍**（`test_028_instance_cascade_delete`），MySQL 那条是真正的哨兵。
④ 应用层级联可保留为纵深防御，但**不再作为 SQLite 的替代方案**（SQLite 本来就工作）。
### F5 — 探测缺「指定单个实例」的入口，设计的「对该实例发一次真实搜索」需要绕道（Medium）

`SearxngProvider::search` 内部恒走 `self.pool.pick_excluding(&tried)`（`searxng.rs` 的 `search` 实现），
**无法指定某个实例** —— 它只认池内的选择算法。设计 §3.4 说「对该实例发一次真实搜索」，但没说明怎么绕过选择器。

好消息：`InstancePool::new`（`instance.rs:183`）、`SearxngProvider::new`（`searxng.rs:51`）、
`SearxngProvider::parse_response_from`（`searxng.rs:114`）、`client::build_search_client`（`client.rs:21`）
**全是 `pub`** → 探测可以**构造一个只含目标实例的临时池**，复用整套逻辑，**无需改 `mod.rs` / `provider.rs`**。

**处理**：§3.4 / §5 补一句「探测构造单实例临时 `InstancePool` 复用 `SearxngProvider`，不触碰选择器与注册表」，
使「新增 `probe.rs` 一个文件即够」这一判断**有据可依**（现无据）。另：探测不得写回 `InstancePool` 的冷却状态
（它是临时池，天然满足），但设计应显式说明，避免实现者误用共享池。

### F6 — `ALL_TABLES` 漏改会让「全部表已建」的迁移测试失去覆盖（Low）

`db.rs:8344` 的 `const ALL_TABLES`（注明「All 23 tables」）被 `db.rs:8441-8458` 的迁移测试逐表断言存在。
新增两张表若不进去，该测试**照常通过**（它只遍历清单，不做全表 diff）→ 覆盖率静默倒退。

**处理**：§5 变更清单加一行「`db.rs` 的 `ALL_TABLES` 补两张表，注释 23 → 25」。
（另核实：`aigw-migrate` 的 `remote_import.rs:852` / `remote_export.rs:529` 两张表清单是 **litellm 互操作**专用，
只覆盖 litellm 侧同名表，**不需要**加 aigw 独有表 —— 设计不提它是正确的，此处仅记明以绝后患。）

### F7 — `kind=tavily|bocha` 的「允许录入」表述不成立，且**可保存的行会让下次启动 `exit(1)`**（High）

§8.1 写「UI 允许录入 `kind=tavily|bocha` 的配置（校验报『客户端未实现』）」——用「允许录入」来验证加密/redact 链路。
但加载路径是硬的：

```rust
// mod.rs:194  build_provider()
searxng::KIND => …,                                    // 只有这一个分支
other => Err(format!("unsupported provider kind …")),   // 其余一律 Err
// config_loader.rs:186  build_websearch_registry() → cfg.validate()? / build_provider(...)?
// main.rs:381-384
Err(e) => { tracing::error!(…); std::process::exit(1); }   // ← 启动即退出
```

**失败场景**：运营在 UI 里存了一个 `kind=tavily` 的 provider（正如 §8.1 所鼓励）→ 该行落库 →
进程重启 → reload 构造 registry 时 `build_provider` 返回 `Err` → `build_websearch_registry` 返回 `Err` →
`main.rs` **`exit(1)`** → **服务再也起不来**，且现场没有 UI 可改（服务没起来）。这是「UI 能写、loader 不认」的自锁。

更糟：§4.1 的 UT 列表**要求写路径拒绝** `kind=tavily`（`test_unimplemented_kind_tavily_rejected_with_hint`），
与 §8.1 的「允许录入」**直接矛盾** —— 同一份设计两处互斥。

**处理（必改）**：取 §4.1 的严格口径，**写路径拒绝**（400，`SUPPORTED_KINDS` 之外一律拒；错误文案区分
「不支持的 kind」与「已规划未实现的 kind」），**删去 §8.1 的「允许录入」**及其「用于验证 redact」的理由
（redact 链路可由 `searxng` + `api_key` 非空的行覆盖，SearXNG 的反向代理可能带 key，见 `searxng.rs` 的
`bearer_auth` 分支 —— 无需引入非法 kind）。
**并且**：loader 侧改为**fail-soft**（遇到无法构造的行 → `warn` + 跳过该 provider，而非 `Err` 整链退出），
使「DB 里存在一行坏数据」**永远不能**阻止服务启动。这一条独立于 kind 校验，是纵深防御。

### F8 — 锚点漂移 2 处（Low）

抽查的锚点中，以下两处与实际不符（设计写于 2026-10-06，其后代码有改动）：

| 设计引用 | 实际 | 判定 |
|---------|------|------|
| `crates/aigw-server/src/main.rs:636-647`（proxies 路由注册） | 实际在 **662-690**；636-647 是 `/spend/model-groups` 区段 | ❌ 漂移 |
| `crates/aigw-core/src/router.rs:26`（`cooldown_until`） | 实际在 **:25**（`:26` 是 `total_requests`） | ❌ 漂移 1 行 |

核实**准确**的锚点：`027_proxies.sql:10-20`（sqlite 建表）、`proxies.rs:184`（`list_proxies`）、`proxies.rs:261`（`create_proxy`）、
`ModelDialog.tsx:93-96` / `:166-167` / `:189`、`crypto.rs:178`（`encrypt_litellm_value_gcm`）、
`instance.rs:89`（`has_weight`，与设计引的 `router.rs:366-377` 对应）、`router.rs:419`（`weighted_pick`）、
`types.rs:90-99`（`Guardrails`）、`config.rs:25`（`SUPPORTED_KINDS = &["searxng"]`）、`keys.rs:77`/`:113`（`AppState` 形态）。
外部引用 `~/works/play/sub2api/backend/migrations/174_group_web_search_price_per_call.sql` **存在**。

**处理**：修上述两处；并在 §2 加一句「锚点为 2026-10-06 快照，以符号名称为准」（其余 Stage 的惯例）。

### F9 — in-use 守卫的口径比 `proxies` 弱，且「删光所有 provider」无定义（Medium）

§1 验收「被 `default_provider` / `failover_order` 引用的 provider 不可删」。但 DB 化之后这两个概念**变成了
provider 表自己的列**（`is_default` / `failover_order`），不再是「别的表引用了它」。
于是它等价于「**默认 provider 不可删**」这一条 —— 比 `proxies` 的「被 credentials 引用」（跨表）弱得多，
而设计沿用了 `proxies` 的 409 + `referenced_by` 响应形状（`proxies.rs:387-428`），会给出一个语义不相称的 API。

**未定义**：删光所有 provider 之后怎么办？（registry 变空 → 回落 yaml？还是禁用？若 yaml 也没有 → 禁用，可接受，
但设计未言明，且 `§3.2` 的「DB 非空」判据在**删空**后又变回 yaml，属**运行期来源反转**，须显式定义。）

**处理**：§3.1 / §3.3 写死：① in-use 判据 = `is_default=1`（其余行可删）；② 删空后回落 yaml（与「DB 空」同一分支），
并在响应/日志中提示来源已切换；③ 若 yaml 也空 → 禁用（零行为变化）。

### F10 — 14h 工作量偏乐观（Low）

§5 的改动面：3 份 migration + `models.rs` 两结构体 + `db.rs` 三方言 CRUD（含 in-use 查询）+ 新 `probe.rs`
+ 新 `websearch.rs`（9 个端点）+ `main.rs` 路由 + **F2 的运行期 reload 重构（含 keys.rs + 3 读者）**
+ 前端 5 文件 + i18n（en/zh-CN）+ 13 组 UT + mock BDD + real BDD 三后端 + 前端 BDD × 3 viewport。
可对标：`proxies`（Phase 50）是 **4 个 Stage / 44h** 交付的同等表面（无实例子资源、无 reload 重构、无定价语义）。
即便本 Stage 复用率更高，**14h 仍偏紧**。

**处理**：预估上调至 **~20h**，或在 §1 显式切出「运行期 reload」为独立小 Stage/后续项
（若切出，门禁「改价不重启生效」须同步降级为「重启生效」，并说明取舍）。

---

## AI Pre-Filter Results

- 子 agent 报「`ON DELETE CASCADE` 会破坏 `aigw-migrate` 的同步」——**过滤（不成立）**：`aigw-migrate` 的两张表清单
  只含 litellm 互操作表（`remote_import.rs:852`），不含任何 aigw 独有表。
- 子 agent 报「`probe_result` 应放 provider 表才对」——**过滤（不成立）**：探测对象是**物理端点**，
  与 `proxies`（一行一端点）一致；provider 健康度是其实例的聚合，不是独立事实。
- 「`SUPPORTED_KINDS` 是运行时可变列表，UI 可动态取」——**过滤（不成立）**：它是 `pub const &[&str]`（`config.rs:25`），
  前端无法运行期获取，UI 的 kind 下拉应为**静态枚举**。

## Rule Filtering

- **Memory bias**：F1 与 F8 同源 —— 设计把注意力全放在「provider / instance 两层」这个**新**结构上，
  对**既有**顶层字段做了「不存在」的默认假设（`grep` 七个字段名全文 0 命中，是最直接的证据）。
- **Logical fallacy**：§8.1 的「允许录入未实现的 kind」是**为验证一个测试链路而放松生产约束**（手段绑架目的），
  且与 §4.1 自己的 UT 要求互斥 —— 已升级为 F8(High)。
- **Scope creep**：本 Stage 不含 scope creep；相反，F10 指出它**低估**了自身范围（运行期 reload 是隐藏的大块）。

## Resolution Summary

| # | 严重度 | 处置 |
|---|-------|------|
| F1 | High | **改设计**：全局策略字段留 yaml；provider 表删 `max_results`；§3.2 限定「不做并集」的作用域 |
| F2 | High | **改设计**：§3.6 写死机制；§5 补 `keys.rs` + 3 读者 + `main.rs` 构造 |
| F3 | High | **改设计**：`is_default` 唯一性写路径保证（建议改删该列、只用 `failover_order`） |
| F4 | Medium | **改设计**：三方言各写具体 DDL（MySQL 用表级 CONSTRAINT）；级联 UT 三方言各跑一遍 |
| F5 | Medium | **改设计**：§3.4 补「单实例临时池」实现路径 |
| F6 | Low | **改设计**：§5 补 `ALL_TABLES` |
| F7 | High | **改设计**：写路径拒非法 kind；删 §8.1「允许录入」；loader 改 fail-soft |
| F8 | Low | **改设计**：修 2 处漂移锚点 + 加「以符号名为准」注 |
| F9 | Medium | **改设计**：in-use 判据 = `is_default=1`；删空后回落 yaml 并入同分支 |
| F10 | Low | **改设计**：预估 14h → ~20h（或切出 reload） |

**All Critical Fixed**: 是（无 Critical）
**All High Priority Addressed**: 是（F1/F2/F3/F7 已改设计）


## 编码期必查项（Design Review 后置）

1. **F1**：实现前先 `grep` 确认**没有任何**全局字段被写进两张表；`Guardrails` 的四个字段必须仍来自 yaml。
2. **F2**：reload 落地后须有一条 UT 断言「写操作 → 不重启 → 下一次搜索用新单价」；
   且 `web_search: None` 的 36 处字面量全部改为 `RwLock::new(None)`（类型变了，漏一处即编译失败 —— 这正好是安全的）。
3. **F3**：`UNIQUE(name)` 与「默认 = `failover_order` 最小」各要一条 UT（含并列、含全 NULL）。
4. **F4**：`test_028_instance_cascade_delete` **三方言各跑一遍** —— MySQL 那条是哨兵（内联 FK 会被 InnoDB 忽略）。
5. **F5**：探测构造单实例临时池，**不得**复用共享 `InstancePool`（会污染冷却状态）。
6. **F7**：写路径 kind 校验 + loader fail-soft，**两者都要**且各有 UT；
   reload 失败必须保留旧 registry（不得写入 `None`）。
