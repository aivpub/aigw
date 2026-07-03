# aigw Deployment Guide

## Quick Start (Docker Compose)

```bash
cp .env.example .env
# Edit .env with your API keys
docker-compose up -d
```

## Configuration

### Environment Variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | No | `sqlite:aigw.db` | Database connection string |
| `MASTER_KEY` | Yes | — | API key for authenticating proxy requests |
| `RUST_LOG` | No | `info` | Log level: trace, debug, info, warn, error |
| `DEPLOYMENT_MODE` | No | `single` | Deployment mode: single, cluster |
| `OPENAI_API_KEY` | No | — | Default OpenAI API key for proxied requests |
| `ANTHROPIC_API_KEY` | No | — | Default Anthropic API key for proxied requests |
| `SERVER_HOST` | No | `0.0.0.0` | Server bind address |
| `SERVER_PORT` | No | `4000` | Server bind port |

### Database Options

- **SQLite** (default): Simple, single-instance. Good for testing and small deployments.
- **PostgreSQL**: Production, multi-instance support. Set `DATABASE_URL` to `postgres://user:pass@host/db`.
- **MySQL**: Alternative for existing MySQL infrastructure. Set `DATABASE_URL` to `mysql://user:pass@host/db`.

## Deployment Scenarios

### 1. Single Instance (Docker)

```bash
docker build -t aigw:latest .
docker run -d -p 4000:4000 \
  -v $(pwd)/config.yaml:/app/config.yaml \
  -e MASTER_KEY=sk-your-key \
  aigw:latest
```

### 2. Behind Nginx Reverse Proxy

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

### 3. Systemd Service

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

## Logging

JSON structured logging via `tracing-subscriber`. Set `RUST_LOG=debug` for verbose output.

Example log output:

```json
{"timestamp":"2026-07-03T10:00:00Z","level":"INFO","target":"aigw_server","message":"server started","port":4000}
```

## Monitoring

For production, consider:

- **Prometheus metrics** — coming in a future release
- **Log aggregation** — ELK / Loki for centralized log collection
- **Health check monitoring** — poll `/health` endpoint for uptime alerts
