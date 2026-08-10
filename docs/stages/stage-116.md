# Stage 116 — 静态配置模型接入（Phase 46）

- **Phase**: 46
- **类型**: 后端 + 测试 + 文档
- **预估**: 16h
- **状态**: ✅ 完成（2026-08-10）
- **前置**: Phase 45 技术债清理收官（116/116 Stages）
- **设计文档**: `docs/stages/stage-116.md` + `docs/08-autonomous-decisions.md` ADR-031

## 背景

代码审计（2026-08-09 env/config 清单核对）发现 `config.yaml` 多个配置块**被解析但从未接线**：

- `AigwConfig.model_list` 解析后丢弃（模型只来自 DB API / aigw-migrate）。
- `router_settings` 只被 serde 解析，运行时恒用 `RouterConfig::default()`；DB `/router/settings` 的 GET/PUT 只读写 config 表，未在请求时应用。
- `environment_variables` 完全死配置（仅 `config.rs:47` 一处定义，零读取）。
- `general_settings.custom_key_generate_length` / `disable_custom_api_keys` / `deployment_mode`（config 版）解析后零消费。

litellm 的核心部署范式是"挂载一个 `config.yaml` 即上线"，aigw 的静态配置路径断裂。

## 交付内容

| 配置块 | 动作 | 落点 |
|---|---|---|
| `model_list` | ✅ seed 到 `proxy_models`（幂等，DB-first） | `aigw_core::config_loader::seed_models_from_config`，`main.rs` boot |
| `environment_variables` | ✅ 启动时注入缺失 env（dotenvy 语义） | `config_loader::apply_environment_variables`，`main.rs`（tracing init 前） |
| `router_settings` | ✅ boot 加载进 Router + seed 到 config 表 | `config_loader::build_router_config` + `router_settings_seed_json` |
| `custom_key_generate_length` | ✅ 接线 `/key/generate` token 长度 | `keys::generate_key_token_with_len`（clamp 16-64） |
| `disable_custom_api_keys` | ✅ 接线非 master 创建 key gate | `keys::generate_key` FORBIDDEN gate |
| `deployment_mode`（config 版） | ✅ config 优先于 CLI 默认 | `main.rs` deployment_mode 解析 |
| `litellm_settings`（drop_params 等） | ⏸ 无对应实现，不接线 | config.example.yaml 注释 |
| config hot-reload / 请求时动态 router 应用 | ⏸ 不在本次范围 | — |

### 优先级规则

- **model_list seed 幂等**：`get_model_by_name` 查重 → 存在跳过（DB-first），不存在 `insert_model`（`created_by: "config"`）。重复启动不重复插入。
- **environment_variables**：`std::env::var_os(name).is_none()` 才 `set_var`，shell env 永远优先。
- **RouterStrategy 扩展**：`FromStr` 新增 `usage-based-routing-v2` / `latency-based-routing` 变体（`pick_deployment` 沿用 shuffle+cooldown 共享路径，实例级负载跟踪留待后续）。

## TDD

- `config_loader` UT ×10：seed 空列表 noop / seed 插入 / seed 幂等 / seed 跳过已有（DB-first）/ env 填补缺失不覆盖已有 / env 非对象 noop / router_config None→默认 / 字段映射 / 负数 clamp / seed_json 保形。
- `keys` UT ×4（新增）：`generate_key_token_with_len` 长度 clamp（16-64-22）/ `/key/generate` 自定义长度 32 / disable_custom_api_keys 非 master 拒绝（JWT user cookie）/ 非禁用 master 允许。
- BDD ×2（新增，mock）：config_loader seed 模型在 `/v1/models` 展示（`model_info.mode=chat` 标注）/ seed 幂等（重复 seed 后 `/model/list` 仍 1 个模型）。
- 顺带修复：`budget_reset_steps` 硬编码 `next_tick_at` 过期 flake（改动态时间戳）；`/v1/models` 对空 `model_info` 补 `mode:"chat"` 标注。

## 验证

- `task test`：aigw-core 425 + aigw-server 144 UT 全绿。
- `task bdd`：254 scenarios（237 passed / 17 skipped，**0 failed**；修复 budget_reset next_tick flake 后 2 个历史失败转绿）。
- `task fmt` + `task lint` green（经 cargo check 无 warning 残留）。

## 相关文档

- `docs/stages/stage-roadmap.md` v53.0（Phase 46 加入，总进度 116/117）
- `docs/11-next-steps.md`（当前阶段 → Phase 46 Stage 116）
- `docs/08-autonomous-decisions.md` ADR-031
- `config.example.yaml`（更新接线注释）

## 不做清单（后续 Stage 候选）

- `litellm_settings`（drop_params/request_timeout/set_verbose）接线 — 需对应实现，已标注。
- `config.yaml` 热重载 / `router_settings` 请求时动态应用 — 需 `Arc<Mutex<Router>>` 改造。
- 多模型 failover 选择逻辑增强（UsageBased/Latency 变体实际负载决策）。
