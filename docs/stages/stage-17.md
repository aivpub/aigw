# Stage 17: 生产迁移 SOP + 回滚方案

**创建日期**: 2026-07-06
**完成日期**: 2026-07-08
**状态**: ✅ 完成
**优先级**: P0
**前置条件**: Stage 16 完成

---

## 1. 目标

建立完整的生产迁移标准操作流程（SOP），确保从 litellm 到 aigw 的切换可安全执行、可快速回滚。同时增强 `aigw-migrate export`，支持回写到 litellm 作为回滚路径。

---

## 2. 交付

### 2.1 迁移 SOP 文档（`docs/migration-sop.md`）

```
Phase 1: 准备（停机窗口前 1 天）
  1. 确认 litellm 版本、数据库类型、数据量
  2. 确认目标 aigw 实例已部署并可通过 DB 访问
  3. 设置 AIGW_MASTER_KEY（新建，不与 litellm master_key 相同）
  4. 通知用户计划停机窗口

Phase 2: 预检（停机前 1 小时）
  1. aigw-migrate verify --source-url ... --target-url ...（对比行数）
  2. 抽样验证：手动解密 3-5 个关键模型确认密钥有效
  3. aigw 健康检查：GET /health

Phase 3: 执行（停机窗口内）
  1. 停止 litellm 写入（只读模式或停止服务）
  2. aigw-migrate remote-import（全量导入）
  3. 验证行数一致性
  4. aigw 启动 + 健康检查
  5. 关键模型端到端测试（curl 调用 /v1/chat/completions）
  6. DNS/负载均衡切换到 aigw

Phase 4: 监控（切换后 30 分钟）
  1. 错误率 < 1%
  2. P99 延迟 < litellm 基线 + 20%
  3. spend_logs 写入正常

Phase 5: 回滚（如需）
  1. DNS/负载均衡回切到 litellm
  2. 停止 aigw
  3. aigw-migrate export（将 aigw 运行期间数据回写到 litellm）
```

### 2.2 `aigw-migrate export` 增強

当前 `export` 已在 9 张表上工作。Stage 17 增强：

- 支持 PostgreSQL 目标（当前仅 SQLite）
- 新增 `credentials` 和 `proxy_models` 导出（从 aigw 写到 litellm）
- 导出时自动用 litellm 密钥重加密（aigw 密文 → 解密 → litellm 密钥加密 → 写入 litellm DB）
- 支持 `--source-url` / `--target-url` 参数（与 import 格式一致）

### 2.3 回滚流程 BDD 测试

```gherkin
Scenario: aigw 运行后回滚到 litellm 数据完整
  Given aigw 已从 litellm 迁移完成并运行
  And aigw 在运行期间产生了新的 spend_logs
  When 运行 aigw-migrate export 回写到 litellm DB
  And 启动 litellm 加载回写后的 DB
  Then litellm 能正常启动
  And litellm /v1/models 包含迁移前 + aigw 运行期间新增的模型
  And 新的 spend_logs 在 litellm 中可查

Scenario: 完整迁移 + 回滚往返测试
  Given 原始 litellm DB 有完整数据
  When 执行 remote-import → export 往返
  Then 往返后 litellm DB 数据与原始一致（行数对比）
```

### 2.4 生产迁移检查清单

作为 `docs/migration-sop.md` 的附录，一份可打印/勾选的检查清单：

- [ ] 源 litellm PostgreSQL 可访问
- [ ] 目标 aigw PostgreSQL 已初始化（migration 已执行）
- [ ] `AIGW_MASTER_KEY` 已生成并安全存储
- [ ] 已确认 `LiteLLM_Config.general_settings.master_key` 可提取
- [ ] 预检行数对比通过
- [ ] 关键模型解密验证通过
- [ ] 迁移完成行数验证通过
- [ ] 端到端请求测试通过
- [ ] 监控告警已配置
- [ ] 回滚方案已演练
- [ ] 回滚联系人已知

---

## 3. 门禁

- 迁移 SOP 对 litellm 测试实例执行，全部步骤通过
- BDD 迁移 + 回滚测试覆盖全流程
- `export` 增强支持 PostgreSQL 目标 + credentials/proxy_models
- 往返测试：litellm → aigw → litellm 数据一致
