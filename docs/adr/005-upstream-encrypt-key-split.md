# ADR-005: 拆分上游「字段加密 key」与「API 鉴权 key」环境变量

> 日期：2026-07-22
> 状态：已实施

## 背景

`task bdd-real-pg` 长期有 2 个 scenario 失败（`protocol_conversion_real.feature`
的两个 `/v1/messages` 用例），报：

```
500 Credential 'qUMHBIQqTouetuETdSfqweZwIpnqKX-8Dy3zp6KJJD1_REWSSxcfMVAE31tWyndOLjJfALcFIoqbGf66Zg==' not found
```

## 根因

上游 litellm 部署里存在**两个不同的 master key**：

| key | 值 | 用途 | 来源 |
|-----|----|----|------|
| 字段加密 key | `qogHGRI7JY186o7yhT6QzT8kSo3p2q6H` | 加密 DB 里 `litellm_params`/`credential_values` | `LiteLLM_Config.general_settings.master_key` |
| API 鉴权 key | `sk-ZiYEpatzdI9Enb_L0tujXA` | litellm HTTP API 的 `Authorization: Bearer` | 启动配置 / `OPENAPI_KEY` |

迁移（migrate）需要的是**前者**（解密上游密文、轮转后用 aigw key 重加密）。但原
环境变量 `AIGW_UPSTREAM_MASTER_KEY` 在命名上无法区分两者，配置时误填成 API 鉴权
key（`sk-ZiYE...`），导致 `rotate_json_fields` 用错误 key 解密失败 → **静默保留原
加密串**（`rotate_fields_inner` 的 `Err(_) => Ok(value.clone())` 分支）→ 写入本地库
的 `litellm_credential_name` 仍是上游加密的 base64 串 → 运行时 resolver 用 aigw key
解不出 → `decrypt_json_fields` 返回原值 → `get_credential_by_name(<加密串>)` 返回
None → `not_found`。

### 为何只有 `/v1/messages` 失败、`/v1/chat/completions` 通过

feature 按字母序运行：`end_to_end_real`（chat）在 `migration_sync` 之前跑，此时
`proxy_models` 表为空，`resolver.resolve()` 走 env-var fallback
（`OPENAI_API_KEY`/`OPENAI_BASE_URL`）直连上游成功。`protocol_conversion_real`
（messages）在 `migration_sync` 之后跑，`proxy_models` 已有数据，resolver 命中
`litellm_credential_name` 分支 → 解密失败 → 500。

## 决策

将环境变量 `AIGW_UPSTREAM_MASTER_KEY` 重命名为 `AIGW_UPSTREAM_ENCRYPT_KEY`，语义明确
为「上游 litellm **字段加密 key**」，与 API 鉴权 key（`OPENAPI_KEY`/`OPENAI_API_KEY`）
彻底区分。

### 改动范围

- `crates/aigw-migrate/src/bin/export_fixtures.rs` — 读取 `AIGW_UPSTREAM_ENCRYPT_KEY`
- `crates/aigw-server/tests/bdd_steps/migration_sync_steps.rs` — `upstream_encrypt_key()` 读新变量
- `crates/aigw-server/tests/bdd_steps/migration_rollback_steps.rs` — 同上
- `Taskfile.yml` — `bdd-real-pg`/`sqlite`/`mysql` 三个任务转发新变量
- `.env.example` / `docs/aigw-migrate.md` / `docs/migration-sop.md` — 文档说明 + 故障排查

### 保留的兜底

migrate 的 `extract_source_master_key` 仍会自动从上游 `LiteLLM_Config` 表提取真实
加密 key，即使环境变量留空也能工作。新变量名只是让「需要显式配置」的场景不再
产生语义混淆。

## 验证

修正 `.env` 中 `AIGW_UPSTREAM_ENCRYPT_KEY` 为上游真实字段加密 key 后，
`task bdd-real-pg` 全部 scenario 通过（17/17）。

## 后续

- ADR-004 提到的 `sqlx::AnyPool` 类型擦除问题是另一条独立线索（迁移用例长期失败），
  本次实测显示 AnyPool 路径本身已能完成迁移（12 rows synced [OK]），本 ADR 不涉及。
