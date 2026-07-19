# aigw Deployment Guide

## Compose Files

| File | Database | Use Case |
|------|----------|----------|
| `docker-compose.yml` | External (PG/MySQL) | **Production** — point `DATABASE_URL` to your existing DB |
| `docker-compose.allinone.yml` | PostgreSQL (included) | **Self-hosted** — aigw + PG in one command |
| `docker-compose.test.yml` | PG + MySQL (included) | **Testing/CI** — BDD and cross-DB verification |

## Quick Start

### Production

```bash
cp .env.example .env
$EDITOR .env   # set MASTER_KEY, DATABASE_URL, API keys
docker compose up -d
```

### All-in-One (with PostgreSQL)

```bash
docker compose -f docker-compose.allinone.yml up -d
```

### Test / CI

```bash
docker compose -f docker-compose.test.yml up -d
task bdd-real-pg
task bdd-real-mysql
```

## Configuration

### Environment Variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | Yes (prod) | — | `postgres://user:pass@host/db` or `mysql://user:pass@host/db` |
| `MASTER_KEY` | Yes | — | Master admin key for proxy authentication |
| `RUST_LOG` | No | `info` | Log level: `trace`, `debug`, `info`, `warn`, `error` |
| `DEPLOYMENT_MODE` | No | `onprem` | `onprem` or `saas` |
| `OPENAI_API_KEY` | No | — | Default OpenAI API key for proxied requests |
| `ANTHROPIC_API_KEY` | No | — | Default Anthropic API key for proxied requests |
| `SERVER_PORT` | No | `4000` | Server bind port |

### Database Options

- **SQLite**: Simple, single-instance. Good for testing and small deployments.
- **PostgreSQL**: Production, multi-instance support. Set `DATABASE_URL` to `postgres://user:pass@host/db`.
- **MySQL**: Alternative for existing MySQL infrastructure. Set `DATABASE_URL` to `mysql://user:pass@host/db`.

## Deployment Scenarios

### 1. Docker Compose (Production)

```bash
cp .env.example .env
$EDITOR .env
docker compose up -d
```

### 2. Single Binary (Docker)

```bash
docker run -d -p 4000:4000 \
  -v $(pwd)/config.yaml:/app/config.yaml \
  -e MASTER_KEY=sk-your-key \
  -e DATABASE_URL=postgres://user:pass@host:5432/aigw \
  ghcr.io/aivpub/aigw:latest
```

### 3. Behind Nginx Reverse Proxy

```nginx
location /v1/ {
    proxy_pass http://localhost:4000;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_read_timeout 600s;
    proxy_buffering off;
}
```

### 4. Systemd Service

```ini
[Unit]
Description=aigw AI Gateway
After=network.target

[Service]
Type=simple
User=aigw
ExecStart=/usr/local/bin/aigw --config /etc/aigw/config.yaml
Restart=always
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

## Health Checks

| Endpoint | Purpose |
|---|---|
| `GET /health` | Overall health status |
| `GET /health/readiness` | Service readiness (ready to serve traffic) |
| `GET /health/liveliness` | Service liveness (process is alive) |
| `GET /health/metrics` | DB pool stats, uptime, key/model counts |
| `GET /metrics` | Prometheus metrics |

## Logging

JSON structured logging via `tracing-subscriber`. Set `RUST_LOG=debug` for verbose output.

Example log output:

```json
{"timestamp":"2026-07-03T10:00:00Z","level":"INFO","target":"aigw_server","message":"server started","port":4000}
```

## Monitoring

For production, consider:

- **Prometheus metrics** — available on `GET /metrics`
- **Log aggregation** — ELK / Loki for centralized log collection
- **Health check monitoring** — poll `/health` endpoint for uptime alerts
