# aigw — AI Gateway (litellm Rust 最小兼容替代)

> litellm proxy 的 Rust 最小兼容替代版本。
> 保持与 litellm 数据格式、API 接口和部署模式高度兼容，
> 同时提供更低的资源消耗和更高的吞吐性能。

## 快速开始

```bash
# 克隆仓库
git clone https://github.com/aivpub/aigw.git
cd aigw

# 使用 RDD 流程开发
/rdd-stage-auto
```

## 项目结构

```
aigw/
├── crates/
│   ├── aigw-core/          # 核心库：数据模型、数据库、路由、中间件
│   └── aigw-server/        # HTTP 服务二进制
├── docs/
│   ├── 01-charter.md       # 项目章程
│   ├── 11-next-steps.md    # 当前进度
│   ├── stages/             # Stage 文档
│   │   └── stage-roadmap.md
│   └── openapi/            # OpenAPI 规范 (Stage 4)
├── migrations/             # 数据库迁移脚本 (Stage 1)
├── .rdd/                   # RDD 框架配置
└── Cargo.toml              # Rust workspace
```

## 技术栈

- **语言**: Rust 2021
- **Web 框架**: axum 0.8
- **数据库**: SQLite (sqlx 0.8)，兼容 PostgreSQL
- **HTTP 客户端**: reqwest 0.12
- **异步运行时**: tokio

## 部署模式

### 企业自托管 (On-Prem)

```bash
cargo run --release -- --config config.yaml --deployment-mode onprem
```

### 云服务 (SaaS)

```bash
cargo run --release -- --config config.yaml --deployment-mode saas
# 前置 nginx/kong auth request 鉴权网关
```

## 与 litellm 的兼容性

详见 `docs/01-charter.md` 和 `docs/stages/stage-roadmap.md`。

核心承诺：
1. **Schema 100% 兼容** — litellm SQLite DB 可直接导入
2. **API 格式兼容** — /key/* 和 /spend/* 返回值结构一致
3. **多租户最小化兼容** — Org/Team/User/Project 表完整保留
4. **OpenAPI 规范** — Stage 4 输出标准 openapi.yaml
5. **长期路线** — 从最小化逐步演进到完整替代
