# Stage 5: Docker + Deployment

**Status**: Complete (2026-07-03)
**Phase**: Phase 4 -- Production Ready

## Goal

Docker containerization and self-hosted deployment documentation.

## Deliverables

- Multi-stage `Dockerfile` (rust:1.88 builder -> debian:bookworm-slim runtime)
- `docker-compose.yml` with SQLite (default), PostgreSQL, MySQL options
- `.env.example` and `config.example.yaml`
- `docs/deployment.md` with 3 deployment scenarios
- Docker healthcheck with `/health` endpoint
- OCI-compliant image labels

## Verification

- 7 Dockerfile structure tests
- 5 deployment file tests (config, env, compose validation)
- `task docker-build` builds successfully
- 144 total tests pass
