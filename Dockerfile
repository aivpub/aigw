# =============================================================================
# aigw — AI Gateway Docker Image
#
# Optimized with cargo-chef for Docker layer caching:
# - Dependency-only layer cached unless Cargo.toml/Cargo.lock changes
# - BuildKit cache mounts for cargo registry and target/ (reusable across builds)
# - Source changes → only recompile workspace crates (seconds, not minutes)
#
# Build:
#   task docker-build           # native arch
#   task docker-build-amd64     # linux/amd64 (from arm64 mac)
#   task docker-build-arm64     # linux/arm64
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

# Stage 1: Frontend build
FROM node:22-slim AS frontend-builder

WORKDIR /app/crates/aigw-frontend
COPY crates/aigw-frontend/package.json crates/aigw-frontend/package-lock.json ./
RUN npm ci --silent

COPY crates/aigw-frontend/ ./
RUN npm run build

# Stage 2: Rust build — dependency layer (cached unless Cargo.toml changes)
FROM rust:1.88-slim-bookworm AS planner

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-chef for dependency recipe generation
RUN cargo install cargo-chef --locked

WORKDIR /app

# Copy only metadata needed for dependency resolution
COPY Cargo.toml Cargo.lock ./
COPY crates/aigw-core/Cargo.toml      crates/aigw-core/
COPY crates/aigw-server/Cargo.toml    crates/aigw-server/
COPY crates/aigw-migrate/Cargo.toml   crates/aigw-migrate/

# Minimal stub source files so cargo chef can analyze the workspace
# (these are just markers; the real source is copied later in the builder stage)
RUN mkdir -p crates/aigw-core/src crates/aigw-server/src crates/aigw-server/tests crates/aigw-migrate/src
RUN echo '' > crates/aigw-core/src/lib.rs
RUN echo 'fn main() {}' > crates/aigw-server/src/main.rs
RUN echo 'fn main() {}' > crates/aigw-server/tests/bdd.rs
RUN echo 'fn main() {}' > crates/aigw-migrate/src/main.rs

# Generate the dependency recipe
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Cook dependencies (cached layer — only rebuilds when recipe changes)
FROM rust:1.88-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-chef in this stage too (needed for cook)
RUN cargo install cargo-chef --locked

WORKDIR /app

# Copy the recipe from the planner stage
COPY --from=planner /app/recipe.json recipe.json
COPY --from=planner /app/Cargo.toml /app/Cargo.lock ./
COPY --from=planner /app/crates/ crates/

# Cook dependencies — this layer is cached until Cargo.toml/Cargo.lock changes.
# NO cache mount for target/ — Docker layer caching preserves the pre-built deps.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo chef cook --release --recipe-path recipe.json

# Copy real source and migrations (only this layer rebuilds on most code changes)
COPY crates/aigw-core/src/      crates/aigw-core/src/
COPY crates/aigw-core/migrations/ crates/aigw-core/migrations/
COPY crates/aigw-server/src/    crates/aigw-server/src/
COPY crates/aigw-migrate/src/   crates/aigw-migrate/src/

# Copy pre-built frontend dist/ for rust-embed
COPY --from=frontend-builder /app/crates/aigw-frontend/dist/ crates/aigw-frontend/dist/

# Build the application — only workspace crates are recompiled.
# NO cache mount for target/ here: the cook step preserves target/ in the Docker
# layer so `cargo build` sees all pre-compiled dependencies and only rebuilds app code.
ARG GIT_COMMIT_HASH
ARG GIT_DESCRIBE
ARG GIT_DIRTY
ARG GIT_BRANCH
ARG BUILD_DATE
ARG RUST_VERSION
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release

# Collect binaries from cache — BuildKit doesn't preserve target/ naturally,
# so we copy them to a known location before the next stage
RUN mkdir -p /out && \
    cp target/release/aigw /out/aigw && \
    cp target/release/aigw-migrate /out/aigw-migrate

# Stage 4: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    wget \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /out/aigw /usr/local/bin/aigw
COPY --from=builder /out/aigw-migrate /usr/local/bin/aigw-migrate
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
    CMD wget -qO- http://localhost:4000/health || exit 1

ENTRYPOINT ["aigw"]
CMD ["--bind", "0.0.0.0:4000"]
