# Stage 140 Gate 4 遗留（MED/LOW）处置记录

**日期**: 2026-10-29
**来源**: Stage 140 独立 reviewer 结论 `PASS — BLOCKER 0 / HIGH 0 / MEDIUM 3 / LOW 4`
**处置者**: 主 Agent（Phase 55 loop，Stage 141 Gate 3 期间顺手收口，因为三处都在 `db.rs` / `models.rs` 且与 141 的 token 路径共用）

## 已修

| # | 原问题 | 处置 |
|---|---|---|
| M1 | `SET_MCP_TOKEN_DEFAULT_*` 只有 `WHERE id = ?`，CLEAR 却是 owner 作用域的；配对错了会给别人的行打 default | 三个方言的 SET 都加上 `server_id / owner_type / owner_id`，并在 `rows_affected() == 0` 时返回 `DbError::NotFound`（事务回滚，不会留下「已清空但没设上」的半态）。新增 UT `test_mcp_token_default_rejects_foreign_token` |
| M2 | `is_default` 有三个写点（INSERT / UPDATE / set_default），trait doc 却声称只有一个 | 收敛成单一写者：INSERT 与 UPDATE 的 SQL 与绑定都删掉这一列，trait doc 明写「只有 `set_default_mcp_token` 写」。handler 本来就先插 false 再 promote，行为不变 |
| M3 | `CredentialScope::parse` 把未知值映射到 `DEFAULT`（= `SharedOrUser`，最宽松的一个） | 未知值改为映射到 `UserRequired`（收紧方向），并显式认 `shared_or_user`。理由写进 doc：这一列只由本二进制写，出现别的值说明行被改过或来自别处，此时不该静默把共享 token 发出去 |
| L3 | `CreateMcpTokenRequest`（core 里）无人使用，且带明文 `token_value` 字段 | 删除。handler 用的是自己模块内的 `CreateMcpTokenBody` |

## 未修（有意留）

| # | 问题 | 为什么不修 |
|---|---|---|
| L1 | `default_mcp_token` 不过滤 `enabled` | 过滤发生在 Rust 侧（`is_token_usable`），这是 Stage 140 §8 定的设计：三方言的 datetime 比较写法不同，且 token 是每 owner 几行的小集合。SQL 里再过滤一次等于两处判断同一件事 |
| L2 | `delete_mcp_server` 不检查 `rows_affected` | handler 先 `fetch_server(&state, id).await?` 拿 404，删不到行的情况已经在上游被拦。store 层再抛一次只会让批量删除的语义变复杂 |
| L4 | PG / MySQL 的 store 覆盖只有 real BDD 覆盖一部分 | 由 Stage 141 的 `real/mcp_server_crud_real.feature` 承担（5 场景在 pg 与 mysql 上都真跑过，含 JSON 列往返与表级 FK 级联）。store 级的逐函数单测在 SQLite 上有，跨方言靠 real BDD，不重复建一套 |

## 顺带发现（不是 Stage 140 的问题）

MySQL 后端跑 real BDD 时出现 `tracing-subscriber` `sharded.rs` panic（`tried to clone a span that already closed` / `refs left=1 right=0`），导致当时正在进行的请求收到 `hyper::Error(IncompleteMessage)`。**这是既有已知 flake**：`crates/aigw-server/src/routes/chat.rs:1013-1021` 的注释早已写明（连报错串都一样），机制是 `Span::enter()` 的 guard 活过 `await`、被 tokio 多线程运行时在另一个 worker 上 drop。`rg '\.enter\(\)' crates/aigw-core/src/mcp crates/aigw-server/src/routes/mcp.rs` 零命中，MCP 代码没有新增任何跨 await 的 guard，因此**不归 Phase 55 修**，也不新开 TD 条目（既有注释即账面）。
