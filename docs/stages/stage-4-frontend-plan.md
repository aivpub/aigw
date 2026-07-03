# aigw Frontend Console — Planning Document

**Stage**: Stage 4 - Frontend Console Planning
**Created**: 2026-07-03
**Status**: Planning

---

## 1. Overview & Goals

### Purpose

A web-based management console for aigw — the AI Gateway. Provides a visual interface for
managing API keys, viewing usage metrics, and monitoring spend across LLM providers.

### Target Users

- **Developers**: Generate and manage API keys for their applications
- **Admins**: Monitor overall spend, configure rate limits, manage providers

### Design Principles

- **Simple**: Minimal dependencies, fast build, easy to self-host
- **Functional first**: Data-centric views, action-oriented navigation
- **Performant**: SPA with client-side caching, minimal API calls
- **Embeddable**: Served from the same Rust binary, no separate deployment

---

## 2. Tech Stack Recommendation

### Framework: React + TypeScript

**Why React:**
- Largest ecosystem of UI components and charting libraries
- Vite build tooling for fast dev iteration and optimal production builds
- React Query (TanStack Query) for server state management — aligns perfectly
  with aigw's REST API
- TypeScript for type safety matching the Rust backend's strict types

**Alternatives considered:**
| Option | Verdict | Reason |
|--------|---------|--------|
| Vue 3 | Rejected | Smaller ecosystem for admin dashboards; fewer charting integrations |
| Svelte 5 | Rejected | Smaller community; harder to find admin templates |
| HTMX + templates | Rejected | Insufficient interactivity for charts and real-time data |

### Build Tooling: Vite

- Fast HMR during development
- Optimized production builds with code splitting
- Built-in TypeScript support
- Proxy configuration for API dev (forward `/key/*`, `/spend/*`, etc. to aigw)

### UI Component Library: shadcn/ui (Radix + Tailwind)

- Accessible, unstyled primitives for full visual control
- Excellent admin dashboard templates available
- Small bundle size (tree-shakeable)
- Consistent with Rust's "pay for what you use" philosophy

### State Management: TanStack Query + Zustand

- **TanStack Query**: Server state (API keys, spend data, models). Handles
  caching, refetching, pagination, and optimistic updates.
- **Zustand**: Client-only state (sidebar collapsed, theme, active filters).
  Lightweight, no boilerplate.

### API Client: fetch + TanStack Query

No need for axios or graphql. Plain `fetch` with TanStack Query's `queryFn`
keeps dependencies minimal. Type-safe response types mirror Rust structs.

### Charting: Recharts

- React-native charting with responsive containers
- Line charts for spend over time, bar charts for per-model usage
- Pie charts for spend distribution by key/team

---

## 3. Page & Route Structure

```
/                          → Dashboard (overview)
/keys                      → Key Management (CRUD)
/keys/:id                  → Key detail / edit
/spend                     → Spend & Usage
/models                    → Models & Providers
/settings                  → Global Settings
/docs                      → API Documentation (Swagger UI)
```

### Route Details

**Dashboard (`/`)**
- Summary cards: total keys, active keys, total spend (today/month)
- Spend trend sparkline (last 30 days)
- Top 5 keys by spend
- Quick actions: generate key, view logs

**Keys Management (`/keys`)**
- Paginated key list with search/filter
- Actions per key: info, edit, block, delete, regenerate
- Create/generate new key form with model access, budget, rate limit fields
- Key detail view showing token (masked), usage stats, config

**Spend & Usage (`/spend`)**
- Filterable spend logs (by key, user, model, date range)
- Aggregated spend charts (daily/weekly/monthly)
- Per-key spend breakdown
- Export CSV capability

**Models & Providers (`/models`)**
- Read-only view of configured models from the provider registry
- Show provider, model name, status, routing info

**Settings (`/settings`)**
- Master key management
- Rate limit defaults
- Deployment mode display
- CORS origin configuration (future)

**API Docs (`/docs`)**
- Embedded Swagger UI rendering the aigw OpenAPI 3.1 spec
- Auto-generated from the `/openapi.json` endpoint

---

## 4. Component Tree

### Layout Components

```
AppLayout
├── Sidebar
│   ├── Logo + AppName
│   ├── NavItem (Dashboard)
│   ├── NavItem (Keys)
│   ├── NavItem (Spend)
│   ├── NavItem (Models)
│   ├── NavItem (Settings)
│   └── NavItem (API Docs)
├── Header
│   ├── Breadcrumb / PageTitle
│   └── UserMenu (auth status, logout)
└── MainContent (outlet)
```

### Page-Specific Components

```
DashboardPage
├── StatCard (Total Keys)
├── StatCard (Active Keys)
├── StatCard (Spend Today)
├── SpendTrendChart
├── TopKeysTable
└── QuickActions

KeysPage
├── KeySearchBar
├── KeyTable (sortable columns)
├── KeyCreateDialog (modal form)
├── KeyEditDialog
└── KeyDetailDrawer

SpendPage
├── DateRangePicker
├── SpendChart
├── SpendBreakdownTable
└── SpendLogTable (paginated)

ModelsPage
├── ProviderCard (per-provider)
└── ModelTable

SettingsPage
└── SettingsForm (key-value pairs)
```

### Shared Components

- `DataTable`: Paginated, sortable, filterable table
- `SearchInput`: Debounced text search
- `ConfirmDialog`: Delete/block confirmation modal
- `StatCard`: Metric display card with trend indicator
- `DateRangePicker`: Calendar-based date range selector
- `StatusBadge`: Active/blocked/expired status indicator
- `CopyButton`: Copy-to-clipboard for tokens and IDs
- `EmptyState`: Placeholder for empty lists

---

## 5. Data Flow

### API Communication

All frontend API calls target the aigw backend REST API directly:

```
Browser → aigw HTTP API (localhost:4000 or reverse proxy)
```

**Endpoints consumed by the frontend:**

| Frontend Feature | API Endpoint | Method |
|-----------------|-------------|--------|
| Generate key | `/key/generate` | POST |
| Key info | `/key/info` | GET |
| Key list | `/key/list` | GET |
| Update key | `/key/update` | PUT |
| Delete key | `/key/delete` | DELETE |
| Regenerate key | `/key/regenerate` | POST |
| Spend logs | `/spend/logs` | GET |
| Spend per key | `/spend/keys` | GET |
| Spend per user | `/spend/users` | GET |
| Global spend | `/global/spend` | GET |
| Global spend logs | `/global/spend/logs` | GET |
| Health check | `/health` | GET |
| OpenAPI spec | `/openapi.json` | GET |

### Authentication Flow

1. User enters aigw server URL and master key on first visit
2. Master key stored in `sessionStorage` (cleared on tab close)
3. All API calls include `Authorization: Bearer <master_key>` header
4. Future: add dedicated UI login with session tokens

### Real-time Updates (Phase 4)

- Polling with TanStack Query `refetchInterval` for spend and key status
- Future: WebSocket or SSE endpoint for live spend events

---

## 6. Implementation Phases

### Phase 1: MVP — Dashboard + Key Management

- Vite + React + TypeScript project scaffold
- Layout shell (sidebar, header, routing)
- Dashboard with stat cards (hardcoded initially)
- Key management CRUD pages
- Authentication (master key input)
- Static file serving from aigw Rust binary
- CORS middleware on backend

**Deliverable**: Manage keys and view basic metrics from a web UI

### Phase 2: Spend Visualization

- Spend charts with Recharts
- Spend log filtering and pagination
- Per-key spend breakdown
- Models page (read-only provider list)
- CSV export for spend logs

**Deliverable**: Visual spend analytics and log browsing

### Phase 3: Advanced Features

- Settings page (global config management)
- Enhanced key management (model access, budget, rate limits)
- Team/user management (if multi-tenant APIs exist)
- Dark mode theme support

**Deliverable**: Full management console parity with litellm UI

### Phase 4: Real-time & Polish

- SSE or WebSocket for live spend monitoring
- Dashboard auto-refresh with live data
- Responsive mobile layout
- Keyboard shortcuts
- Performance optimization (code splitting, lazy routes)

**Deliverable**: Production-grade admin console

---

## 7. Integration Points

### Static File Serving Strategy

**Recommended: Embedded in Rust binary (Phase 1)**

The Vite production build outputs static files (HTML, JS, CSS). These are
embedded into the aigw Rust binary using `rust-embed` at compile time:

```rust
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
struct FrontendAssets;
```

The axum router serves these assets at `/*` as a fallback route, after all
API routes. SPA routing works by serving `index.html` for unknown paths.

**Rationale:**
- Single binary deployment (no nginx, no separate static server)
- Consistent with litellm's approach
- No CORS issues in production

**Alternative (future): Separate deployment**
- Frontend deployed to CDN / nginx
- Reverse proxy routes `/api/*` to aigw
- Better for horizontal scaling and CDN caching

### CORS Configuration

For development (Vite dev server on `localhost:5173` talking to aigw on
`localhost:4000`), aigw must return CORS headers:

```
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS
Access-Control-Allow-Headers: Authorization, Content-Type
```

The axum middleware layer in `crates/aigw-server/src/routes/cors_layer.rs`
handles this. For production, the origin can be restricted to the aigw
domain when static files are served from the same origin.

### API Type Sharing

The Rust API types (request/response structs) should be mirrored in
TypeScript. Options:

1. **Manual**: Write TypeScript interfaces by hand (Phase 1)
2. **Codegen**: Use `ts-rs` crate to generate TypeScript types from Rust
   structs (Phase 2+)
3. **OpenAPI**: Generate types from the OpenAPI spec using `openapi-typescript`

Recommended: manual for MVP, OpenAPI-based codegen for Phase 2+.

---

## 8. Project Structure (Frontend)

```
frontend/
├── index.html
├── package.json
├── tsconfig.json
├── vite.config.ts
├── tailwind.config.ts
├── src/
│   ├── main.tsx                # Entry point
│   ├── App.tsx                  # Router + providers
│   ├── api/
│   │   ├── client.ts           # fetch wrapper + auth header
│   │   ├── keys.ts             # /key/* API functions
│   │   ├── spend.ts            # /spend/* API functions
│   │   └── health.ts           # /health check
│   ├── components/
│   │   ├── layout/             # Sidebar, Header, AppLayout
│   │   └── shared/             # DataTable, StatCard, etc.
│   ├── pages/
│   │   ├── Dashboard.tsx
│   │   ├── Keys.tsx
│   │   ├── Spend.tsx
│   │   ├── Models.tsx
│   │   ├── Settings.tsx
│   │   └── ApiDocs.tsx
│   ├── hooks/                  # Custom hooks
│   ├── stores/                 # Zustand stores
│   └── types/                  # TypeScript interfaces
└── dist/                       # Vite build output (gitignored)
```

---

## Revision History

| Version | Date | Changes |
|---------|------|---------|
| v1.0 | 2026-07-03 | Initial frontend console plan |
