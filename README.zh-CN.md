# aigw — AI 网关

> **litellm proxy 的 Rust 精简替代** — 把已投产的 litellm 服务迁移到更小、更快的 Rust 实现，客户端无感、数据无损。

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.88+-orange.svg)](https://www.rust-lang.org)
[![Docker](https://img.shields.io/badge/docker-available-blue.svg)](https://github.com/aivpub/aigw/pkgs/container/aigw)

---

## 项目简介

aigw 是 **litellm proxy 的 Rust 精简替代**，专为已经将 litellm 投入生产一年多的团队设计。用 `aigw-migrate` 一键迁移，保留现有 PostgreSQL 数据库不换，所有客户端（Claude Code、Codex、OpenAI SDK）无需任何改动——除了响应变快了。

**为什么用 Rust？** litellm 空载容器占用 **~1 GB** 内存，Docker 镜像 **1.11 GB**；aigw 空载仅 **~10 MB**，镜像 **129 MB**（均为 macOS arm64 实测）。实际生产环境运行 6 周、处理 31.7 万请求后，litellm 实际消耗 **~3.1 GB**。

<!--
  litellm 空载: ghcr.io/berriai/litellm:main-stable, 全新容器, 0 请求
  litellm 生产: docker.litellm.ai/berriai/litellm:main-latest (2026-03-22),
                6周运行, 317,854 POST 请求, PostgreSQL 后端, 31 进程.
                镜像与空载不同（不同 registry/tag/构建时间）。
  aigw 空载:   aigw:latest (debian:bookworm-slim), release 构建, SQLite
-->

| | litellm 空载 | litellm 生产 | aigw 空载 |
|---|---|---|---|
| 容器 RSS | ~1,007 MB | **~3,111 MB**（6周, 31.7万请求） | **~10 MB** |
| Docker 镜像 | 1.11 GB | 1.89 GB | **129 MB** |
| 制品形态 | Python venv + uvicorn | Python venv + uvicorn | **单个静态二进制 (~20 MB)** |

---

## 核心功能

- **OpenAI Chat Completions API** — `/v1/chat/completions`、`/v1/models`、SSE 流式
- **Anthropic Messages API** — `/v1/messages` 支持双向协议转换（Anthropic ↔ OpenAI）
- **Virtual Key 管理** — 完整 CRUD（`/key/generate`、`/key/info`、`/key/update`、`/key/delete`、`/key/list`），返回值格式兼容 litellm
- **用量统计** — `/spend/logs`、`/spend/keys`、`/spend/users`、`/global/spend/*`，记录每次请求费用
- **多租户数据模型** — Org → Team → User → Key 层级结构，外键完整保留
- **负载均衡路由** — Usage-based、Latency-based、Shuffle 三种策略，含 Cooldown + Fallback
- **速率限制** — RPM/TPM 内存计数限流
- **Web 管理控制台** — React + shadcn/ui 仪表盘，管理 Key/模型/用量/Playground/设置
- **Prometheus 指标** — 14 个指标（Counter/Histogram/Gauge），暴露在 `GET /metrics`
- **多数据库支持** — SQLite（默认）、PostgreSQL、MySQL
- **litellm 迁移工具** — `aigw-migrate` 处理加密导入/导出/校验
- **Docker 部署** — 单容器运行，含健康检查和 Docker Compose

---

## 快速开始

### Docker（推荐）

```bash
docker run -d -p 4000:4000 \
  -e MASTER_KEY=sk-your-secret-key \
  -e OPENAI_API_KEY=sk-openai-xxx \
  ghcr.io/aivpub/aigw:latest
```

测试一下：

```bash
curl http://localhost:4000/v1/models \
  -H "Authorization: Bearer sk-your-secret-key"
```

### Docker Compose

提供三份 Compose 文件适配不同场景：

| 文件 | 数据库 | 适用场景 |
|------|----------|----------|
| `docker-compose.yml` | 外部（PG/MySQL） | **生产环境** — 对接已有数据库 |
| `docker-compose.allinone.yml` | PostgreSQL（内建） | **自托管** — aigw + PG 一键部署 |
| `docker-compose.test.yml` | PG + MySQL（内建） | **测试/CI** — BDD 及跨数据库验证 |

**生产环境：**

```bash
cp .env.example .env
$EDITOR .env   # 设置 MASTER_KEY, DATABASE_URL, API Key
docker compose up -d
```

**All-in-One（含 PostgreSQL）：**

```bash
docker compose -f docker-compose.allinone.yml up -d
```

### 源码构建

**前置条件：** Rust 1.88+、Node 22+

```bash
git clone https://github.com/aivpub/aigw.git
cd aigw

# 启动开发服务器（构建前端 + 启动后端）
task dev

# 或者构建发布版本
task build
```

服务启动在 `http://localhost:4000`，管理控制台在根路径。

### 最小配置

创建 `config.yaml`：

```yaml
general_settings:
  master_key: ${MASTER_KEY:-sk-change-me}

model_list:
  - model_name: gpt-4o
    litellm_params:
      model: gpt-4o
      api_base: https://api.openai.com/v1
      api_key: ${OPENAI_API_KEY:-}
```

完整模板见 [`config.example.yaml`](config.example.yaml)。

---

## 架构概览

```
┌──────────────┐     ┌─────────────────────────────────┐     ┌──────────────┐
│  客户端       │────▶│  aigw 服务 (axum + tokio)       │────▶│  上游 LLM    │
│  (Claude Code │     │                                  │     │  (OpenAI /   │
│   Codex /     │     │  认证 → 解析 → 适配 → 日志     │     │   Anthropic) │
│   OpenAI SDK) │     │                                  │     │              │
└──────────────┘     │  SQLite / PostgreSQL / MySQL     │     └──────────────┘
                     └─────────────────────────────────┘
```

### 项目结构

```
aigw/
├── crates/
│   ├── aigw-core/        # 共享库：数据模型、数据库、路由、认证、适配器
│   ├── aigw-server/      # HTTP 服务二进制（axum）
│   ├── aigw-migrate/     # litellm ↔ aigw 迁移 CLI
│   ├── aigw-frontend/    # React 管理控制台（Vite + shadcn/ui）
│   └── aigw-openapi/     # OpenAPI 3.1 规范生成
├── docs/                 # 章程、Stage 文档、ADR、指南
├── config.example.yaml   # 配置模板
├── docker-compose.yml    # 多服务编排
├── Dockerfile            # 多阶段容器构建
├── Taskfile.yml          # 统一开发工作流（task 运行器）
└── Cargo.toml            # Rust workspace
```

---

## litellm 兼容性矩阵

| 功能 | 状态 | 备注 |
|------|------|-------|
| **Virtual Key CRUD** | ✅ 兼容 | `/key/generate`、`/key/info`、`/key/update`、`/key/delete`、`/key/list` |
| **Spend Logs API** | ✅ 兼容 | `/spend/logs`、`/spend/keys`、`/spend/users`、`/spend/tags`、`/global/spend/*` |
| **Schema（11 张核心表）** | ✅ 兼容 | 列+FK 完整对齐，通过 `aigw-migrate` 双向迁移 |
| `/v1/chat/completions` | ✅ 兼容 | SSE 流式、Function Calling、Tool Use |
| `/v1/messages` | ✅ 兼容 | Anthropic Messages API + 协议转换 |
| `/v1/models` | ✅ 兼容 | 模型列表端点 |
| **多租户 CRUD** | ✅ 兼容 | `/org/*`、`/team/*`、`/user/*`（15 个端点） |
| **JWT 登录** | ✅ 兼容 | `/v2/login`，scrypt + Cookie |
| **速率限制** | ✅ 兼容 | RPM/TPM、max_parallel_requests |
| **路由策略** | ✅ 兼容 | Usage-based、Latency-based、Shuffle + Cooldown + Fallback |
| **Prometheus 指标** | ✅ 兼容 | 14 个指标，`GET /metrics` |
| **OTEL 链路追踪** | 🔄 开发中 | W3C traceparent，5 层跨度 |
| 30+ Provider 适配 | ❌ 不做 | 仅支持 OpenAI 兼容 + Anthropic 原生上游 |

---

## 从 litellm 迁移

```bash
# 1. 导入 litellm 数据库到 aigw（自动处理加密密钥轮换）
aigw-migrate remote-import \
  --from litellm --from-db litellm.db \
  --to aigw --to-db aigw.db

# 2. 校验行数一致
aigw-migrate verify --source litellm.db --target aigw.db

# 3. 用迁移后的数据库启动 aigw
aigw --db aigw.db
```

**回滚** 到 litellm（随时可以）：

```bash
aigw-migrate remote-export \
  --from aigw --from-db aigw.db \
  --to litellm --to-db litellm-restored.db
```

完整生产迁移 SOP： [`docs/migration-sop.md`](docs/migration-sop.md)

---

## 开发指南

### 常用命令

| 命令 | 用途 |
|---------|------|
| `task doctor` | 检查项目健康（编译、clippy、必需文件） |
| `task dev` | 启动开发服务器（前端构建 + 后端运行） |
| `task test` | 运行所有后端单元测试（293 个测试） |
| `task bdd` | 运行 BDD 测试（Mock 上游） |
| `task bdd-real` | 运行 BDD 测试（真实 API） |
| `task fe-bdd` | 运行 Playwright BDD 测试（108 个测试，3 种视口） |
| `task fe-dev` | 启动 Vite 前端开发服务器（API 代理到 :4000） |
| `task lint` | 运行 clippy 代码检查（`-D warnings`） |
| `task build` | 构建发布版本（内嵌前端） |
| `task docker-build` | 构建 Docker 镜像 |
| `task fmt` | 检查代码格式 |

### 测试体系

| 层级 | 框架 | 数量 | 命令 |
|-------|-----------|-------|---------|
| 后端单元测试 | libtest | 293 | `task test` |
| 后端 BDD (Mock) | cucumber-rust | 91 场景 | `task bdd` |
| 后端 BDD (真实) | cucumber-rust | — | `task bdd-real` |
| 前端 BDD | Playwright + playwright-bdd | 108（36×3 视口） | `task fe-bdd` |

### 技术栈

| 组件 | 技术 |
|-----------|------------|
| 语言 | Rust 2021 edition |
| Web 框架 | axum 0.8 |
| 数据库 | SQLite / PostgreSQL / MySQL（sqlx 0.8） |
| HTTP 客户端 | reqwest 0.12 |
| 异步运行时 | tokio |
| 前端 | React 19 + Vite + shadcn/ui |
| 日志 | tracing + tracing-subscriber（JSON 格式） |
| 配置 | YAML（`config.yaml`） |
| 迁移工具 | `aigw-migrate` CLI |

---

## 配置参考

| 变量 | 必需 | 默认值 | 说明 |
|----------|----------|---------|-------------|
| `MASTER_KEY` | 是 | — | 主管理密钥，用于代理认证 |
| `DATABASE_URL` | 否 | `sqlite:aigw.db` | 数据库连接：`sqlite:`、`postgres://` 或 `mysql://` |
| `OPENAI_API_KEY` | 否 | — | 默认 OpenAI API Key |
| `ANTHROPIC_API_KEY` | 否 | — | 默认 Anthropic API Key |
| `RUST_LOG` | 否 | `info` | 日志级别：`trace`、`debug`、`info`、`warn`、`error` |
| `DEPLOYMENT_MODE` | 否 | `onprem` | 部署模式：`onprem`（自托管）或 `saas`（云服务） |
| `SERVER_HOST` | 否 | `0.0.0.0` | 服务绑定地址 |
| `SERVER_PORT` | 否 | `4000` | 服务绑定端口 |

完整 YAML 配置模板见 [`config.example.yaml`](config.example.yaml)。

---

## 健康检查

| 端点 | 用途 |
|----------|---------|
| `GET /health` | 总体健康状态 |
| `GET /health/readiness` | 服务就绪检查（可以接收流量） |
| `GET /health/liveliness` | 服务存活检查（进程存活） |
| `GET /health/metrics` | 数据库连接池状态、运行时间、Key/Model 数量 |
| `GET /metrics` | Prometheus 指标（14 项指标） |

---

## 文档索引

| 文档 | 用途 |
|----------|---------|
| [`docs/01-charter.md`](docs/01-charter.md) | 项目章程 — 愿景、目标、边界、路线图 |
| [`docs/stages/stage-roadmap.md`](docs/stages/stage-roadmap.md) | Stage 路线图 — 65/68 已完成 |
| [`docs/11-next-steps.md`](docs/11-next-steps.md) | 当前进度与后续优先级 |
| [`docs/deployment.md`](docs/deployment.md) | 部署指南 — Docker、Nginx、systemd |
| [`docs/litellm-diff-baseline.md`](docs/litellm-diff-baseline.md) | litellm v1.90.0 vs aigw 差异基线 |
| [`docs/migration-sop.md`](docs/migration-sop.md) | 生产迁移 SOP（litellm → aigw） |
| [`docs/15-bdd-guide.md`](docs/15-bdd-guide.md) | BDD 测试编写指南 |
| [`docs/08-autonomous-decisions.md`](docs/08-autonomous-decisions.md) | 架构决策记录（ADR） |
| [`docs/12-technical-debt.md`](docs/12-technical-debt.md) | 技术债账本 |
| [`docs/virtual-key-spec.md`](docs/virtual-key-spec.md) | Virtual Key 生成规范 |

---

## License

MIT

---

## 社区

- **Issues**：[GitHub Issues](https://github.com/aivpub/aigw/issues)
- **Discussions**：[GitHub Discussions](https://github.com/aivpub/aigw/discussions)
