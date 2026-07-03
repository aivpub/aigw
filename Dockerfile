# =============================================================================
# aigw — AI Gateway Docker Image
#
# Build:
#   docker build -t aigw:latest .
#
# Run (SQLite, default):
#   docker run -p 4000:4000 -v $(pwd)/data:/app/data aigw:latest
#
# Run (PostgreSQL):
#   docker run -p 4000:4000 \
#     -e DATABASE_URL=postgres://user:pass@host:5432/aigw \
#     aigw:latest
#
# Run with config:
#   docker run -p 4000:4000 -v $(pwd)/config.yaml:/app/config.yaml aigw:latest
# =============================================================================

# Stage 1: Build
FROM rust:1.88-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy full workspace for single-pass build (avoids dummy-src caching issues
# with sqlx::migrate! macros and multi-crate workspace deps)
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/aigw /usr/local/bin/aigw
COPY --from=builder /app/target/release/aigw-migrate /usr/local/bin/aigw-migrate
COPY --from=builder /app/crates/aigw-core/migrations /app/migrations

EXPOSE 4000

RUN mkdir -p /app/data

ENV DATABASE_URL=sqlite:/app/data/aigw.db
ENV RUST_LOG=info

LABEL org.opencontainers.image.title="aigw"
LABEL org.opencontainers.image.description="AI Gateway — litellm-compatible LLM proxy in Rust"
LABEL org.opencontainers.image.source="https://github.com/aivpub/aigw"
LABEL org.opencontainers.image.version="0.1.0"
LABEL org.opencontainers.image.licenses="MIT"

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD which wget >/dev/null && wget --no-verbose --tries=1 --spider http://localhost:4000/health || \
        which curl >/dev/null && curl --fail --silent http://localhost:4000/health || \
        exit 1

ENTRYPOINT ["aigw"]
CMD ["--bind", "0.0.0.0:4000"]
