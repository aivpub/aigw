# Stage 125: Phase 50 收尾 — real BDD + 文档（代理服务管理）

**所属**: Phase 50（代理服务管理）
**预估**: 4h（real BDD 三后端 + ADR + roadmap/next-steps 回写）
**依赖**: Stage 122-124
**状态**: ⏳ 待开始

---

## 1. 目标

Phase 50 收尾：real BDD 三后端验证 + 设计决策登记 ADR-033 + roadmap/next-steps 回写。

## 2. 方案

### 2.1 real BDD 三后端

- `features/real/proxy_crud.feature`（@real_api @needs_upstream_db）：
  - proxies CRUD 三方言全绿（sqlite/pg/mysql）
  - in-use 守卫：credentials 含 proxy_id 引用 → 删除 409
  - probe_result 快照落库 roundtrip
- 检测端点（test/quality）走 **mock 上游**（`@real_api` 场景不依赖真实 IP 服务，避免 flake）：经 `MockUpstream` 返回 ip-api/CF 签名响应

### 2.2 ADR-033（代理服务管理）

`docs/08-autonomous-decisions.md` 追加 ADR-033：

- **决策**：新建 `proxies` 表 + `/admin/proxies/*` CRUD + 出口/质量检测;整串 `proxy_url` AES-GCM 加密落库;检测快照收单 JSON 字段 `probe_result`;不做过期回退(长期路线)
- **理由**：reqwest 原生消费 proxy_url 字符串;密码随串加密优于 sub2api 明文;检测快照 admin 列表内存解析足够
- **后果**：Stage 51 凭证绑定代理引用 proxies.id;OAuth 反代/交换复用代理客户端

### 2.3 roadmap / next-steps 回写

- `docs/stages/stage-roadmap.md`：追加 Phase 50（Stage 122-125，44h）+ 标记完成;总进度 125→129;顶部状态更新
- `docs/11-next-steps.md`：追加 Phase 50 完成记录 + Phase 51 预告

## 3. 验收标准

- [ ] real BDD 三方言 proxies CRUD + in-use + 快照全绿
- [ ] ADR-033 Accepted 记录
- [ ] roadmap 顶部状态 + Phase 50 条 + 总进度 129/134(待 Phase 51) 回写
- [ ] next-steps 更新
