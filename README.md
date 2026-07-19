# aigw — AI Gateway

> **Drop-in replacement for litellm proxy** — migrate your existing litellm deployment to a smaller, faster Rust service without breaking clients or data.

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.88+-orange.svg)](https://www.rust-lang.org)
[![Docker](https://img.shields.io/badge/docker-available-blue.svg)](https://github.com/aivpub/aigw/pkgs/container/aigw)

---

## What is aigw?

aigw is a **drop-in replacement for litellm proxy**, written in Rust. It's built for teams that already run litellm in production and want to cut resource consumption without touching clients, data, or workflows. Migrate with `aigw-migrate`, keep your existing PostgreSQL database, and your users (Claude Code, Codex, OpenAI SDKs) won't notice the difference — except that everything is faster.

**Why Rust?** In an idle Docker container, litellm consumes **~1 GB** RAM with a **1.11 GB** image. aigw idles at **~10 MB** with a **129 MB** image (both measured on macOS arm64). In our production reference deployment — 6 weeks uptime, 317k+ requests served — litellm runs at **~3.1 GB**.

<!--
  litellm idle:  ghcr.io/berriai/litellm:main-stable, fresh container, 0 requests
  litellm prod:  docker.litellm.ai/berriai/litellm:main-latest (2026-03-22),
                 6-week uptime, 317,854 POST requests, PostgreSQL-backed,
                 31 processes. Image differs from idle (different registry/tag/date).
  aigw idle:     aigw:latest (debian:bookworm-slim), release build, SQLite
-->

| | litellm idle | litellm prod | aigw idle |
|---|---|---|---|
| Container RSS | ~1,007 MB | **~3,111 MB** (6wk, 317k req) | **~10 MB** |
| Docker image | 1.11 GB | 1.89 GB | **129 MB** |
| Artifact | Python venv + uvicorn | Python venv + uvicorn | **single static binary (~20 MB)** |

---

## Core Features

- **OpenAI Chat Completions API** — `/v1/chat/completions`, `/v1/models`, streaming SSE
- **Anthropic Messages API** — `/v1/messages` with bidirectional protocol conversion (Anthropic ↔ OpenAI)
- **Virtual Key Management** — Full CRUD (`/key/generate`, `/key/info`, `/key/update`, `/key/delete`, `/key/list`) with litellm-compatible response shapes
- **Spend Tracking** — `/spend/logs`, `/spend/keys`, `/spend/users`, `/global/spend/*` with per-request cost recording
- **Multi-Tenant Data Model** — Org → Team → User → Key hierarchy with foreign keys preserved
- **Load-Balanced Routing** — Usage-based, latency-based, and shuffle routing with cooldown + fallback
- **Rate Limiting** — RPM/TPM throttling with in-memory counters
- **Web Admin Console** — React + shadcn/ui dashboard for keys, models, spend logs, playground, and settings
- **Prometheus Metrics** — 14 metrics (counters, histograms, gauges) on `GET /metrics`
- **Multi-Database Support** — SQLite (default), PostgreSQL, MySQL
- **litellm Migration Tool** — `aigw-migrate` handles encrypted import/export/verify between litellm and aigw databases
- **Docker Deployment** — Single-container with health checks and Docker Compose

---

## Quick Start

### Docker (recommended)

```bash
docker run -d -p 4000:4000 \
  -e MASTER_KEY=sk-your-secret-key \
  -e OPENAI_API_KEY=sk-openai-xxx \
  ghcr.io/aivpub/aigw:latest
```

Test it:

```bash
curl http://localhost:4000/v1/models \
  -H "Authorization: Bearer sk-your-secret-key"
```

### Docker Compose

Three Compose files for different scenarios:

| File | Database | Use case |
|------|----------|----------|
| `docker-compose.yml` | External (PG/MySQL) | **Production** — connect to your existing DB |
| `docker-compose.allinone.yml` | PostgreSQL (included) | **Self-hosted** — aigw + PG in one command |
| `docker-compose.test.yml` | PG + MySQL (included) | **Testing/CI** — BDD and cross-DB verification |

**Production:**

```bash
cp .env.example .env
$EDITOR .env   # set MASTER_KEY, DATABASE_URL, API keys
docker compose up -d
```

**All-in-One (with PostgreSQL):**

```bash
docker compose -f docker-compose.allinone.yml up -d
```

### Build from Source

**Prerequisites:** Rust 1.88+, Node 22+

```bash
git clone https://github.com/aivpub/aigw.git
cd aigw

# Start dev server (builds frontend + launches backend)
task dev

# Or build release binary
task build
```

The server starts at `http://localhost:4000` with the admin console at the root path.

### Minimal Configuration

Create `config.yaml`:

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

See [`config.example.yaml`](config.example.yaml) for the full template.

---

## Architecture

```
┌──────────────┐     ┌─────────────────────────────────┐     ┌──────────────┐
│  Client      │────▶│  aigw Server (axum + tokio)      │────▶│  Upstream    │
│  (Claude Code│     │                                  │     │  (OpenAI /   │
│   Codex /    │     │  Auth → Resolve → Adapt → Log    │     │   Anthropic) │
│   OpenAI SDK)│     │                                  │     │              │
└──────────────┘     │  SQLite / PostgreSQL / MySQL     │     └──────────────┘
                     └─────────────────────────────────┘
```

### Project Structure

```
aigw/
├── crates/
│   ├── aigw-core/        # Shared library: models, DB, routing, auth, adapters
│   ├── aigw-server/      # HTTP server binary (axum)
│   ├── aigw-migrate/     # litellm ↔ aigw migration CLI
│   ├── aigw-frontend/    # React admin console (Vite + shadcn/ui)
│   └── aigw-openapi/     # OpenAPI 3.1 spec generation
├── docs/                 # Charter, stages, ADRs, guides
├── config.example.yaml   # Configuration template
├── docker-compose.yml    # Multi-service orchestration
├── Dockerfile            # Multi-stage container build
├── Taskfile.yml          # Unified dev workflow (task runner)
└── Cargo.toml            # Rust workspace
```

---

## litellm Compatibility Matrix

| Area | Status | Notes |
|------|--------|-------|
| **Virtual Key CRUD** | ✅ Compatible | `/key/generate`, `/key/info`, `/key/update`, `/key/delete`, `/key/list` |
| **Spend Logs API** | ✅ Compatible | `/spend/logs`, `/spend/keys`, `/spend/users`, `/spend/tags`, `/global/spend/*` |
| **Schema (11 core tables)** | ✅ Compatible | Full column + FK alignment, bidirectional migration via `aigw-migrate` |
| `/v1/chat/completions` | ✅ Compatible | Streaming SSE, function calling, tool use |
| `/v1/messages` | ✅ Compatible | Anthropic Messages API with protocol conversion |
| `/v1/models` | ✅ Compatible | Model list endpoint |
| **Multi-Tenant CRUD** | ✅ Compatible | `/org/*`, `/team/*`, `/user/*` (15 endpoints) |
| **JWT Login** | ✅ Compatible | `/v2/login` with scrypt + Cookie |
| **Rate Limiting** | ✅ Compatible | RPM/TPM, max_parallel_requests |
| **Router** | ✅ Compatible | Usage-based, latency-based, shuffle + cooldown + fallback |
| **Prometheus Metrics** | ✅ Compatible | 14 metrics on `GET /metrics` |
| **OTEL Tracing** | 🔄 In Progress | W3C traceparent, 5 span layers |
| 30+ Provider-specific handlers | ❌ Not planned | OpenAI-compatible + Anthropic native upstreams only |

---

## Migrating from litellm

```bash
# 1. Import litellm database into aigw (handles encryption key rotation)
aigw-migrate remote-import \
  --from litellm --from-db litellm.db \
  --to aigw --to-db aigw.db

# 2. Verify row counts match
aigw-migrate verify --source litellm.db --target aigw.db

# 3. Start aigw with the migrated database
aigw --db aigw.db
```

**Rollback** to litellm at any point:

```bash
aigw-migrate remote-export \
  --from aigw --from-db aigw.db \
  --to litellm --to-db litellm-restored.db
```

Full production migration SOP: [`docs/migration-sop.md`](docs/migration-sop.md)

---

## Development

### Quick Commands

| Command | Purpose |
|---------|---------|
| `task doctor` | Check project health (compile, clippy, required files) |
| `task dev` | Start dev server (frontend build + backend run) |
| `task test` | Run all backend unit tests (293 tests) |
| `task bdd` | Run BDD tests with mock upstream |
| `task bdd-real` | Run BDD tests against real API endpoints |
| `task fe-bdd` | Run Playwright BDD tests (108 tests, 3 viewports) |
| `task fe-dev` | Start Vite dev server (proxies API to :4000) |
| `task lint` | Run clippy with `-D warnings` |
| `task build` | Build release binary with embedded frontend |
| `task docker-build` | Build Docker image |
| `task fmt` | Check code formatting |

### Test Suite

| Layer | Framework | Count | Command |
|-------|-----------|-------|---------|
| Backend Unit | libtest | 293 | `task test` |
| Backend BDD (mock) | cucumber-rust | 91 scenarios | `task bdd` |
| Backend BDD (real) | cucumber-rust | — | `task bdd-real` |
| Frontend BDD | Playwright + playwright-bdd | 108 (36×3 viewports) | `task fe-bdd` |

### Key Technologies

| Component | Technology |
|-----------|------------|
| Language | Rust 2021 edition |
| Web Framework | axum 0.8 |
| Database | SQLite / PostgreSQL / MySQL (sqlx 0.8) |
| HTTP Client | reqwest 0.12 |
| Async Runtime | tokio |
| Frontend | React 19 + Vite + shadcn/ui |
| Logging | tracing + tracing-subscriber (JSON format) |
| Config | YAML (`config.yaml`) |
| Migration | `aigw-migrate` CLI tool |

---

## Configuration Reference

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `MASTER_KEY` | Yes | — | Master admin key for proxy authentication |
| `DATABASE_URL` | No | `sqlite:aigw.db` | `sqlite:`, `postgres://`, or `mysql://` |
| `OPENAI_API_KEY` | No | — | Default OpenAI API key for proxied requests |
| `ANTHROPIC_API_KEY` | No | — | Default Anthropic API key |
| `RUST_LOG` | No | `info` | Log level: `trace`, `debug`, `info`, `warn`, `error` |
| `DEPLOYMENT_MODE` | No | `onprem` | `onprem` or `saas` |
| `SERVER_HOST` | No | `0.0.0.0` | Server bind address |
| `SERVER_PORT` | No | `4000` | Server bind port |

See [`config.example.yaml`](config.example.yaml) for the full YAML configuration schema.

---

## Health Endpoints

| Endpoint | Purpose |
|----------|---------|
| `GET /health` | Overall health status |
| `GET /health/readiness` | Service readiness (ready to serve traffic) |
| `GET /health/liveliness` | Service liveness (process is alive) |
| `GET /health/metrics` | DB pool stats, uptime, key/model counts |
| `GET /metrics` | Prometheus metrics (14 indicators) |

---

## Documentation Index

| Document | Purpose |
|----------|---------|
| [`docs/01-charter.md`](docs/01-charter.md) | Project charter — vision, goals, boundaries, roadmap |
| [`docs/stages/stage-roadmap.md`](docs/stages/stage-roadmap.md) | Stage roadmap — 65/68 stages completed |
| [`docs/11-next-steps.md`](docs/11-next-steps.md) | Current progress and upcoming priorities |
| [`docs/deployment.md`](docs/deployment.md) | Deployment guide — Docker, Nginx, systemd |
| [`docs/litellm-diff-baseline.md`](docs/litellm-diff-baseline.md) | litellm v1.90.0 vs aigw diff baseline |
| [`docs/migration-sop.md`](docs/migration-sop.md) | Production migration SOP (litellm → aigw) |
| [`docs/15-bdd-guide.md`](docs/15-bdd-guide.md) | BDD testing guide (how to write .feature files) |
| [`docs/08-autonomous-decisions.md`](docs/08-autonomous-decisions.md) | Architecture Decision Records (ADR) |
| [`docs/12-technical-debt.md`](docs/12-technical-debt.md) | Technical debt ledger |
| [`docs/virtual-key-spec.md`](docs/virtual-key-spec.md) | Virtual key generation specification |

---

## License

MIT

---

## Community

- **Issues**: [GitHub Issues](https://github.com/aivpub/aigw/issues)
- **Discussions**: [GitHub Discussions](https://github.com/aivpub/aigw/discussions)
