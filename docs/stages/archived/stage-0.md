# Stage 0：项目初始化与 RDD 框架搭建

**Stage ID**: stage-0
**所属 Phase**: Phase 0 — 项目基础设施
**状态**: ✅ 完成
**开始日期**: 2026-07-03
**目标完成**: 2026-07-03

---

## 目标

完成 aigw 项目的正式初始化，建立 RDD 工作框架，编写完整项目章程，从现有原型代码中提取可复用部分建立基线。

## 交付物清单

- [x] RDD 框架初始化（`rdd init`）
- [x] 项目章程 `docs/01-charter.md`（v2.0，含多租户/双向迁移/长期路线/OpenAPI/部署）
- [x] Stage 路线图 `docs/stages/stage-roadmap.md`
- [x] litellm diff 基线文档 `docs/litellm-diff-baseline.md`（含表名映射 + 双向迁移策略）
- [x] Rust 工程结构定义（Cargo workspace：`crates/aigw-core` + `crates/aigw-server`）
- [x] 数据模型定义 `crates/aigw-core/src/models.rs`（全部 11 张表，aigw 自有表名，litellm 列兼容）
- [x] `Cargo.toml` workspace 定义（含依赖声明）
- [x] `.rdd/config.yml` 项目配置
- [x] `AGENTS.md` / `CLAUDE.md` 项目特定内容
- [x] Git initial commit
- [x] 表名决策落实：aigw 自有表名，`aigw-migrate` 负责双向映射
- [x] `docs/11-next-steps.md` 更新

## 不在此 Stage 范围

- Schema 迁移代码（Stage 1）
- API 端点实现（Stage 2+）
- 任何运行时代码

## 验收标准

- [ ] `docs/01-charter.md` 包含完整的多租户策略、长期路线、OpenAPI/前端规划、部署模式
- [ ] Rust 工程可通过 `cargo check`（即使只有骨架）
- [ ] litellm diff 基线文档完成
- [ ] `task doctor` 通过

## 修订记录

| 版本 | 日期 | 修订 | 修订人 |
|------|------|------|--------|
| v1.0 | 2026-07-03 | 初始版本 | 全栈架构师 |
| v2.0 | 2026-07-03 | 所有交付物完成，Stage 0 收尾 | 全栈架构师 |
