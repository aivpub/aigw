# aigw SaaS Architecture

## Overview

Multi-tenant SaaS deployment model for aigw, supporting organization-level data
isolation with optional tenant-aware query filtering. The system can run in two
modes: **onprem** (single-tenant, no isolation) and **saas** (multi-tenant,
org-scoped data access).

## Architecture Diagram

```
                       ┌────────────────────┐
                       │   Load Balancer     │
                       │  (nginx / haproxy)  │
                       └──────┬───┬───┬─────┘
                              │   │   │
              ┌───────────────┘   │   └───────────────┐
              v                   v                   v
  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
  │  aigw instance  │  │  aigw instance  │  │  aigw instance  │
  │    (node-1)     │  │    (node-2)     │  │    (node-N)     │
  │                 │  │                 │  │                 │
  │  Instance       │  │  Instance       │  │  Instance       │
  │  Registry Sync  │  │  Registry Sync  │  │  Registry Sync  │
  │  + TenantCtx    │  │  + TenantCtx    │  │  + TenantCtx    │
  └────────┬────────┘  └────────┬────────┘  └────────┬────────┘
           │                    │                    │
           └────────────────────┼────────────────────┘
                                │
                     ┌──────────v──────────┐
                     │   PostgreSQL         │
                     │   (shared database)  │
                     │                      │
                     │   Tables:            │
                     │   - virtual_keys     │
                     │   - spend_logs       │
                     │   - organizations    │
                     │   - teams            │
                     │   - users            │
                     │   - projects         │
                     └──────────────────────┘
```

## Multi-Tenancy Model

### Organization-Based Isolation

Each tenant is represented by an `Organization` with a unique
`organization_id`. All data entities (keys, spend logs, users, teams)
carry an `organization_id` column that links them to their owning tenant.

```
organizations (1) ──→ (N) teams
teams          (1) ──→ (N) users
teams          (1) ──→ (N) virtual_keys
users          (1) ──→ (N) virtual_keys
projects       (1) ──→ (N) virtual_keys
budgets        (1) ──→ (N) virtual_keys
                              │
                    organization_id (FK nullable)
```

### Data Isolation Strategy

- **Shared database with org_id filtering**: All tenants share the same
  database. The `TenantDb` wrapper applies `organization_id` filters to
  queries when running in SaaS mode.
- **Onprem mode bypass**: When `deployment_mode` is `onprem`, the tenant
  context is `None`, and all org filters are skipped — effectively full access.
- **API key scoping**: Keys belong to organizations. Queries for keys are
  scoped to the requesting tenant's `organization_id`.

## Components

### 1. Tenant Context (`crates/aigw-core/src/tenant.rs`)

```rust
pub struct TenantContext {
    pub organization_id: String,
}

pub struct TenantDb<'a> {
    db: &'a Database,
    tenant: Option<&'a TenantContext>,
}
```

- `TenantContext` — carries the tenant's `organization_id` for the request.
- `TenantDb` — wraps `Database` with optional tenant filtering.
- `is_authorized(org_id)` — checks whether a given org_id is authorized
  under the current tenant context.
  - `None` tenant → always `true` (onprem mode)
  - `Some` tenant → `true` only if `org_id` matches

### 2. Instance Registry (`crates/aigw-core/src/instance.rs`)

```rust
pub struct InstanceRegistry {
    instances: RwLock<HashMap<String, InstanceInfo>>,
}
```

Tracks all running aigw instances in a concurrent-safe registry:
- `register(bind_address)` — register a new instance, returns unique UUID
- `heartbeat(instance_id)` — update last_heartbeat, transition Starting → Healthy
- `list_instances()` — get all registered instances
- `drain(instance_id)` — mark instance as draining (pre-shutdown)
- `mark_unhealthy(instance_id)` — mark instance as unhealthy
- `unregister(instance_id)` — remove from registry
- `healthy_count()` — count of healthy/starting instances

### 3. Auth Gateway

- **Master key**: Full cross-org access (admin operations).
- **Org-scoped keys**: Limited to their organization's data.
- **Token validation with org scope check**: Every API key is validated
  and its owning `organization_id` is extracted to build the `TenantContext`.

### 4. Rate Limiting (Per-Tenant)

- Per-organization rate limits using `RateLimiter` with tenant isolation.
- Global rate limits configurable for the entire deployment.
- Token bucket algorithm per organization.

## Deployment Topology

### Single-Tenant (Onprem Mode)

```
User → aigw (single instance) → SQLite / PostgreSQL
```

- `TenantContext` is `None`.
- No org filtering — full access to all data.
- Suitable for self-hosted single-org deployments.
- Database: SQLite (for simplicity) or PostgreSQL.

### Multi-Tenant (SaaS Mode)

```
Users → Load Balancer (nginx/haproxy)
     → aigw-instance-1 ┐
     → aigw-instance-2 ├→ PostgreSQL (shared)
     → aigw-instance-N ┘
```

- `TenantContext` is `Some` with the requester's `organization_id`.
- All data queries filter by `organization_id`.
- Database: PostgreSQL (required for multi-instance access).
- Instances register themselves and maintain heartbeats.

## Configuration

### SaaS Mode

```yaml
deployment_mode: saas
database_url: postgres://aigw:aigw_secret@postgres:5432/aigw
instance:
  bind_address: "0.0.0.0:8000"
  heartbeat_interval_secs: 10
  heartbeat_timeout_secs: 30
```

### Onprem Mode

```yaml
deployment_mode: onprem
database_url: sqlite:/app/data/aigw.db
```

## Security Considerations

1. **All requests require Bearer token authentication**: No unauthenticated
   access to any endpoint.
2. **Organization isolation enforced at query level**: The `TenantDb`
   wrapper applies `organization_id` filters. There is no way for a
   tenant-scoped key to access another organization's data.
3. **Master key has cross-org access**: Admin operations (global spend,
   key management across orgs) require the master key. The master key
   bypasses tenant filtering.
4. **API keys hashed with SHA-256 before storage**: Keys are never stored
   in plaintext.
5. **PostgreSQL connection uses TLS** (recommended for production).
6. **Instance registry is in-process only**: In the current implementation,
   the instance registry is per-process. Cross-instance coordination would
   require a shared backing store (Redis, etcd) in a future iteration.

## Instance Lifecycle

```
Register → Starting → Heartbeat → Healthy
                                  │
                    ┌─────────────┼─────────────┐
                    v             v             v
               Unhealthy      Draining      (keeps going)
                    │             │
                    │  (missed     │  (drain completed)
                    │   heartbeats)│
                    v             v
               Unregister     Unregister
```

1. **Register**: Instance added to registry, status = `Starting`.
2. **Heartbeat**: First heartbeat transitions `Starting` → `Healthy`.
3. **Drain**: Pre-shutdown signal; load balancer stops sending new requests.
4. **Unhealthy**: Missed heartbeats trigger health check to mark as unhealthy.
5. **Unregister**: Instance removed from registry (graceful shutdown or
   manual removal).

## Testing Strategy

- **Unit tests**: `tenant.rs` tests cover authorization logic for all
  combinations (null tenant, same org, different org, null org_id).
- **Unit tests**: `instance.rs` tests cover registration, heartbeat
  transitions, listing, drain flow, and edge cases (nonexistent IDs).
- **Integration tests**: To be added for end-to-end tenant isolation
  verification with actual database queries.
