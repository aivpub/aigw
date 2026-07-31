import { Page, Route, Request } from "@playwright/test";

interface MockOptions {
  role?: string;
  keyCount?: number;
  modelCount?: number;
}

const baseSpend = {
  total_spend: 42.50,
  spend: 42.50,
};

const sampleKeys = [
  { token: "sk-abc123xxx", key_alias: "prod-gpt-key", key_name: "sk-abc123xxx", models: ["gpt-4"], max_budget: 100, spend: 12.5, team_id: null, expires: null, blocked: false, user_id: "default_user_id", metadata: {}, created_at: "2026-07-01T00:00:00Z", updated_at: "2026-07-05T00:00:00Z" },
  { token: "sk-def456xxx", key_alias: "dev-claude-key", key_name: "sk-def456xxx", models: ["claude-sonnet-4-6"], max_budget: 50, spend: 8.25, team_id: null, expires: null, blocked: false, user_id: "default_user_id", metadata: {}, created_at: "2026-07-02T00:00:00Z", updated_at: "2026-07-06T00:00:00Z" },
  { token: "sk-ghi789xxx", key_alias: "test-key", key_name: "sk-ghi789xxx", models: ["gpt-4o-mini"], max_budget: null, spend: 0.0, team_id: null, expires: null, blocked: false, user_id: "default_user_id", metadata: {}, created_at: "2026-07-03T00:00:00Z", updated_at: "2026-07-03T00:00:00Z" },
];

const sampleModels = [
  { model_id: "m1", model_name: "gpt-4", litellm_params: { model: "openai/gpt-4", api_base: "https://api.openai.com/v1" }, model_info: { id: "gpt-4", mode: "chat", max_tokens: 8192, input_cost_per_token: 0.00003, output_cost_per_token: 0.00006 }, created_at: "2026-07-01T00:00:00Z", created_by: "admin", updated_at: "2026-07-01T00:00:00Z", updated_by: null },
  { model_id: "m2", model_name: "claude-sonnet-4-6", litellm_params: { model: "anthropic/claude-sonnet-4-6", api_base: "https://api.anthropic.com" }, model_info: { id: "claude-sonnet-4-6", mode: "chat", max_tokens: 200000, input_cost_per_token: 0.000003, output_cost_per_token: 0.000015 }, created_at: "2026-07-02T00:00:00Z", created_by: "admin", updated_at: "2026-07-02T00:00:00Z", updated_by: null },
  { model_id: "m3", model_name: "gpt-4o-mini", litellm_params: { model: "openai/gpt-4o-mini", api_base: "https://api.openai.com/v1" }, model_info: { id: "gpt-4o-mini", mode: "chat", max_tokens: 16384, input_cost_per_token: 0.00000015, output_cost_per_token: 0.0000006 }, created_at: "2026-07-03T00:00:00Z", created_by: "admin", updated_at: "2026-07-03T00:00:00Z", updated_by: null },
];

const sampleSpendLogs = [
  { call_id: "req-001", request_id: "chatcmpl-abc123", call_type: "completion", model: "gpt-4", api_key: "sk-abc***", key_name: "prod-gpt-key", total_tokens: 1234, prompt_tokens: 800, completion_tokens: 434, spend: 0.42, start_time: "2026-07-08T10:00:00Z", end_time: "2026-07-08T10:00:05Z", request_duration_ms: 5123, ttft_ms: 234.5, status: "success", custom_llm_provider: "openai", model_group: "gpt-4", user: "test-user", requester_ip_address: "192.168.1.1" },
  { call_id: "req-002", request_id: "msg_xyz789", call_type: "completion", model: "claude-sonnet-4-6", api_key: "sk-def***", key_name: "dev-claude-key", total_tokens: 567, prompt_tokens: 300, completion_tokens: 267, spend: 1.23, start_time: "2026-07-08T10:05:00Z", end_time: "2026-07-08T10:05:03Z", request_duration_ms: 2890, ttft_ms: 456.7, status: "success", custom_llm_provider: "anthropic", model_group: "claude-sonnet-4-6", user: "dev-user" },
];

// Detail mocks for the new detail endpoint (GET /global/spend/logs/{call_id})
const sampleDetailLog1 = {
  ...sampleSpendLogs[0],
  messages: [{ role: "user", content: "Hello, how are you?" }],
  response: { id: "chatcmpl-xxx", choices: [{ message: { role: "assistant", content: "I'm doing well, thank you!" } }], usage: { prompt_tokens: 800, completion_tokens: 434, total_tokens: 1234 } },
};
const sampleDetailLog2 = {
  ...sampleSpendLogs[1],
  messages: [{ role: "user", content: "Explain quantum computing" }],
  response: { id: "msg-xxx", content: [{ type: "text", text: "Quantum computing uses qubits..." }], usage: { input_tokens: 300, output_tokens: 267 } },
};

const sampleSpendModels = [
  { model: "gpt-4", total_spend: 25.00, total_tokens: 50000, requests: 12 },
  { model: "claude-sonnet-4-6", total_spend: 17.50, total_tokens: 30000, requests: 8 },
];

const sampleUsers = [
  { user_id: "user-1", user_alias: "Alice", user_email: "alice@example.com", user_role: "proxy_admin", spend: 20.0, organization_id: null, team_id: null },
  { user_id: "user-2", user_alias: "Bob", user_email: "bob@example.com", user_role: "internal_user", spend: 15.0, organization_id: null, team_id: null },
];

const sampleOrgs = [
  { organization_id: "org-1", organization_alias: "Engineering", budget_id: "budget-1", spend: 35.0 },
];

const sampleTeams = [
  { team_id: "team-1", team_alias: "AI Team", organization_id: "org-1", members: ["user-1", "user-2"], admins: ["user-1"], spend: 42.5, blocked: false },
];

export async function defineMockRoutes(route: Route, request: Request) {
  const url = new URL(request.url());

  // ── Admin Jobs API mocks ──
  const sampleJobs = [
    { id: "job-abc123-4567", step_type: "body_archive", trigger_type: "manual", triggered_by: "admin", status: "running", total_steps: 24, completed_steps: 8, failed_steps: 0, created_at: "2026-07-25T14:00:00Z", updated_at: "2026-07-25T14:05:00Z" },
    { id: "job-def456-7890", step_type: "body_archive", trigger_type: "cron", triggered_by: null, status: "completed", total_steps: 1, completed_steps: 1, failed_steps: 0, created_at: "2026-07-25T13:00:00Z", updated_at: "2026-07-25T13:02:00Z" },
  ];
  const sampleJobDetail = {
    job: sampleJobs[0],
    steps: [
      { id: "step-1", step_key: "hour=2026-07-25T14", step_type: "body_archive", status: "completed", payload: { hour: "2026-07-25T14" }, result: { rows_archived: 200, bytes_written: 35000000, storage_path: "s3://bucket/logs/year=2026/..." }, error_message: null, retry_count: 0, started_at: "2026-07-25T14:00:01Z", completed_at: "2026-07-25T14:00:04Z" },
      { id: "step-2", step_key: "hour=2026-07-25T15", step_type: "body_archive", status: "running", payload: { hour: "2026-07-25T15" }, result: {}, error_message: null, retry_count: 0, started_at: "2026-07-25T14:00:05Z", completed_at: null },
    ],
    summary: { total_steps: 24, completed: 8, failed: 0, pending: 15, running: 1 },
  };
  const sampleJobLogs = [
    { step_key: "hour=2026-07-25T14", level: "info", message: "step started", created_at: "2026-07-25T14:00:01Z" },
    { step_key: "hour=2026-07-25T14", level: "info", message: "queried 200 rows", created_at: "2026-07-25T14:00:02Z" },
    { step_key: "hour=2026-07-25T14", level: "info", message: "parquet written 35MB", created_at: "2026-07-25T14:00:03Z" },
    { step_key: null, level: "info", message: "all steps completed, finalizing", created_at: "2026-07-25T14:05:00Z" },
  ];
  const sampleArchiveStats = {
    total_archived_rows: 450000,
    pending_rows: 800,
    auto_archive: true,
    storage_configured: true,
  };

  if (url.pathname === "/admin/jobs/stats") {
    return route.fulfill({ status: 200, json: { body_archive: { queue: { pending: 3, running: 2, completed: 148, failed: 1 } }, budget_reset: { queue: { pending: 0, running: 0, completed: 0, failed: 0 } } } });
  }
  if (url.pathname === "/admin/jobs") {
    const st = url.searchParams.get("step_type") || "";
    const filtered = st ? sampleJobs.filter(j => j.step_type === st) : sampleJobs;
    // Return total: 120 to trigger pagination when requested, but only 2 items for page 1
    return route.fulfill({ status: 200, json: { jobs: filtered, page: 1, limit: 50, total: 120 } });
  }
  if (url.pathname === "/admin/jobs/trigger" && route.request().method() === "POST") {
    return route.fulfill({ status: 200, json: { job_id: "job-new123-4567", status: "pending", total_steps: 3 } });
  }
  if (url.pathname.match(/^\/admin\/jobs\/[^/]+\/logs$/)) {
    return route.fulfill({ status: 200, json: { logs: sampleJobLogs, page: 1, limit: 50 } });
  }
  if (url.pathname.match(/^\/admin\/jobs\/[^/]+$/)) {
    return route.fulfill({ status: 200, json: sampleJobDetail });
  }
  if (url.pathname === "/admin/archive/stats") {
    return route.fulfill({ status: 200, json: sampleArchiveStats });
  }

  // Key management — frontend expects { keys: [...] } not { data: [...] }
  if (url.pathname === "/key/list") {
    return route.fulfill({ status: 200, json: { keys: sampleKeys, total_count: sampleKeys.length } });
  }
  if (url.pathname === "/key/generate" && route.request().method() === "POST") {
    return route.fulfill({ status: 200, json: { token: "sk-new123xxx", key: "sk-new123xxx", key_name: "sk-new123xxx", key_alias: "new-key", models: ["gpt-4"], max_budget: 100 } });
  }
  if (url.pathname === "/key/delete" && route.request().method() === "DELETE") {
    return route.fulfill({ status: 200, json: { message: "Key deleted" } });
  }
  if (url.pathname === "/key/info") {
    return route.fulfill({ status: 200, json: sampleKeys[0] });
  }
  if (url.pathname === "/key/update" || url.pathname === "/key/regenerate") {
    return route.fulfill({ status: 200, json: sampleKeys[0] });
  }

  // Model management — frontend expects { data: ModelItem[] }
  if (url.pathname === "/model/list") {
    return route.fulfill({ status: 200, json: { object: "list", data: sampleModels } });
  }
  if (url.pathname === "/model/info") {
    return route.fulfill({ status: 200, json: sampleModels[0] });
  }
  if (url.pathname === "/model/new" && route.request().method() === "POST") {
    return route.fulfill({ status: 200, json: { model_name: "new-model", litellm_params: {} } });
  }
  if (url.pathname === "/model/update" && route.request().method() === "PUT") {
    return route.fulfill({ status: 200, json: { message: "Model updated" } });
  }
  if (url.pathname === "/model/delete" && route.request().method() === "DELETE") {
    return route.fulfill({ status: 200, json: { message: "Model deleted" } });
  }

  // Credentials list (for ModelDialog credential dropdown)
  if (url.pathname === "/credential/list") {
    return route.fulfill({
      status: 200,
      json: { data: [{ credential_name: "prod-openai" }, { credential_name: "dev-anthropic" }] },
    });
  }

  // Spend
  if (url.pathname === "/spend/logs") {
    return route.fulfill({ status: 200, json: { data: sampleSpendLogs } });
  }
  if (url.pathname.startsWith("/global/spend/logs/") && url.pathname !== "/global/spend/logs") {
    // Detail endpoint: extract call_id from path
    const cid = url.pathname.replace("/global/spend/logs/", "");
    const detail = cid === "req-001" ? sampleDetailLog1 : cid === "req-002" ? sampleDetailLog2 : null;
    if (detail) {
      return route.fulfill({ status: 200, json: detail });
    }
    return route.fulfill({ status: 200, json: sampleDetailLog1 });
  }
  if (url.pathname === "/spend/models") {
    return route.fulfill({ status: 200, json: { data: sampleSpendModels } });
  }
  if (url.pathname === "/global/spend") {
    return route.fulfill({ status: 200, json: baseSpend });
  }
  if (url.pathname === "/global/spend/keys") {
    return route.fulfill({ status: 200, json: [{ api_key: "sk-abc***", spend: 12.5 }, { api_key: "sk-def***", spend: 8.25 }] });
  }
  if (url.pathname === "/global/spend/keys/rankings") {
    return route.fulfill({
      status: 200,
      json: [
        { api_key: "sk-abc***", key_alias: "prod-gpt-key", total_spend: 12.50, total_requests: 85, total_tokens: 30000 },
        { api_key: "sk-def***", key_alias: "dev-claude-key", total_spend: 8.25, total_requests: 42, total_tokens: 18000 },
        { api_key: "sk-ghi***", key_alias: "test-key", total_spend: 3.10, total_requests: 15, total_tokens: 5000 },
        { api_key: "sk-jkl***", key_alias: null, total_spend: 1.50, total_requests: 8, total_tokens: 2000 },
        { api_key: "sk-mno***", key_alias: null, total_spend: 0.80, total_requests: 4, total_tokens: 900 },
      ],
    });
  }
  if (url.pathname === "/spend/keys") {
    return route.fulfill({ status: 200, json: [{ api_key: "sk-abc***", spend: 12.5 }] });
  }
  if (url.pathname === "/spend/tags") {
    return route.fulfill({ status: 200, json: [{ tag: "prod", spend: 42.5 }] });
  }
  if (url.pathname === "/spend/providers") {
    return route.fulfill({ status: 200, json: { data: [{ provider: "openai", total_spend: 25.0, total_tokens: 50000, requests: 12 }, { provider: "anthropic", total_spend: 17.5, total_tokens: 30000, requests: 8 }], count: 2 } });
  }
  if (url.pathname === "/global/spend/logs") {
    // Apply fuzzy search filter if ?request_id= query param present
    const q = url.searchParams.get("request_id");
    if (q) {
      const filtered = sampleSpendLogs.filter(log => log.call_id.includes(q) || (log.request_id ?? "").includes(q));
      return route.fulfill({ status: 200, json: { data: filtered, count: filtered.length, total_count: filtered.length, page: 1, page_size: 30, total_pages: 1 } });
    }
    return route.fulfill({ status: 200, json: { data: sampleSpendLogs, count: sampleSpendLogs.length, total_count: sampleSpendLogs.length, page: 1, page_size: 30, total_pages: 1 } });
  }
  if (url.pathname === "/global/spend/activity") {
    return route.fulfill({
      status: 200,
      json: {
        metadata: {
          total_spend: 42.50,
          total_requests: 423,
          successful_requests: 401,
          failed_requests: 22,
          total_tokens: 2300000,
          prompt_tokens: 1400000,
          completion_tokens: 900000,
        },
        daily: [
          { date: "2026-07-08", spend: 12.5, tokens: 500000, requests: 100, prompt_tokens: 300000, completion_tokens: 200000, successful_requests: 95, failed_requests: 5 },
          { date: "2026-07-09", spend: 18.3, tokens: 800000, requests: 150, prompt_tokens: 500000, completion_tokens: 300000, successful_requests: 142, failed_requests: 8 },
          { date: "2026-07-10", spend: 11.7, tokens: 1000000, requests: 173, prompt_tokens: 600000, completion_tokens: 400000, successful_requests: 164, failed_requests: 9 },
        ],
      },
    });
  }
  if (url.pathname === "/global/spend/models") {
    return route.fulfill({ status: 200, json: { data: sampleSpendModels, count: sampleSpendModels.length } });
  }
  if (url.pathname === "/global/spend/providers") {
    return route.fulfill({ status: 200, json: { data: [{ provider: "openai", total_spend: 25.0, total_tokens: 50000, requests: 12 }, { provider: "anthropic", total_spend: 17.5, total_tokens: 30000, requests: 8 }], count: 2 } });
  }
  if (url.pathname === "/spend/model-groups" || url.pathname === "/global/spend/model-groups") {
    return route.fulfill({ status: 200, json: { data: [{ model_group: "gpt-4-group", total_spend: 25.0, total_tokens: 50000, requests: 12 }, { model_group: "claude-group", total_spend: 17.5, total_tokens: 30000, requests: 8 }], count: 2 } });
  }

  // User management (for Stage 29)
  if (url.pathname === "/user/list") {
    return route.fulfill({ status: 200, json: { data: sampleUsers } });
  }
  if (url.pathname === "/user/info") {
    return route.fulfill({ status: 200, json: sampleUsers[0] });
  }
  if (url.pathname === "/user/new" && route.request().method() === "POST") {
    return route.fulfill({ status: 200, json: sampleUsers[0] });
  }
  if (url.pathname === "/user/delete" && route.request().method() === "DELETE") {
    return route.fulfill({ status: 200, json: { message: "User deleted" } });
  }
  if (url.pathname === "/user/update" && route.request().method() === "PUT") {
    return route.fulfill({ status: 200, json: sampleUsers[0] });
  }

  // Org management
  if (url.pathname === "/org/list") {
    return route.fulfill({ status: 200, json: { data: sampleOrgs } });
  }
  if (url.pathname === "/org/new" && route.request().method() === "POST") {
    return route.fulfill({ status: 200, json: sampleOrgs[0] });
  }
  if (url.pathname === "/org/delete" && route.request().method() === "DELETE") {
    return route.fulfill({ status: 200, json: { message: "Organization deleted" } });
  }
  if (url.pathname === "/org/update" && route.request().method() === "PUT") {
    return route.fulfill({ status: 200, json: sampleOrgs[0] });
  }

  // Team management
  if (url.pathname === "/team/list") {
    return route.fulfill({ status: 200, json: { data: sampleTeams } });
  }
  if (url.pathname === "/team/new" && route.request().method() === "POST") {
    return route.fulfill({ status: 200, json: sampleTeams[0] });
  }
  if (url.pathname === "/team/delete" && route.request().method() === "DELETE") {
    return route.fulfill({ status: 200, json: { message: "Team deleted" } });
  }
  if (url.pathname === "/team/update" && route.request().method() === "PUT") {
    return route.fulfill({ status: 200, json: sampleTeams[0] });
  }

  // Login (for Stage 26)
  if (url.pathname === "/v2/login") {
    return route.fulfill({ status: 200, json: { user_id: "default_user_id", user_role: "proxy_admin", user_email: null } });
  }
  if (url.pathname === "/v2/login/check") {
    return route.fulfill({ status: 200, json: { user_id: "default_user_id", user_role: "proxy_admin" } });
  }

  // Health
  if (url.pathname === "/health/metrics") {
    return route.fulfill({ status: 200, json: { status: "ok", db: "connected", uptime_seconds: 3600, key_count: 3, model_count: 3 } });
  }
  // Model health-check latest results — HealthTab polls this on load.
  if (url.pathname === "/health/latest") {
    const checkedAt = new Date().toISOString();
    return route.fulfill({
      status: 200,
      json: {
        data: sampleModels.map((m) => ({
          model_name: m.model_name,
          model_id: m.model_id,
          status: "healthy",
          response_time_ms: 42.5,
          error_message: null,
          checked_at: checkedAt,
        })),
        count: sampleModels.length,
        last_success: Object.fromEntries(sampleModels.map((m) => [m.model_name, checkedAt])),
      },
    });
  }
  // Trigger model health checks — HealthTab POSTs these; respond immediately
  // (the probe is async on the real backend, and the next /health/latest poll
  // reflects the fresh "healthy" results above).
  if (url.pathname === "/model/health-check/all" && route.request().method() === "POST") {
    return route.fulfill({ status: 200, json: { status: "dispatched", models: sampleModels.length } });
  }
  if (url.pathname === "/model/health-check" && route.request().method() === "POST") {
    return route.fulfill({ status: 200, json: { status: "checking", model_id: url.searchParams.get("model_id") } });
  }

  // v1 endpoints
  if (url.pathname === "/v1/models") {
    return route.fulfill({ status: 200, json: { data: [{ id: "gpt-4" }, { id: "claude-sonnet-4-6" }] } });
  }
  if (url.pathname === "/v1/chat/completions" && route.request().method() === "POST") {
    const reqBody = JSON.parse(request.postData() ?? "{}");
    const isStream = reqBody.stream === true;
    if (isStream) {
      // Return multiple SSE chunks as a single body; the frontend ReadableStream reader
      // will process them as individual data: lines.
      const sseBody = [
        `data: ${JSON.stringify({ choices: [{ delta: { content: "Hello" } }] })}\n\n`,
        `data: ${JSON.stringify({ choices: [{ delta: { content: " from" } }] })}\n\n`,
        `data: ${JSON.stringify({ choices: [{ delta: { content: " mock!" } }] })}\n\n`,
        "data: [DONE]\n\n",
      ].join("");
      return route.fulfill({
        status: 200,
        headers: { "content-type": "text/event-stream" },
        body: sseBody,
      });
    }
    return route.fulfill({
      status: 200,
      json: { choices: [{ message: { content: "Mock response: I am doing well!" } }] },
    });
  }

  // Fallback: pass through
  return route.continue();
}

// Idempotent guard: `mockAllApis` is called multiple times per test (Background's
// "API endpoints are mocked" + "I am logged in as admin", and scenarios often re-declare both).
// Playwright runs the LAST-registered `**/*` handler first, so re-registering the broad mock
// AFTER a per-scenario override (e.g. "API detail endpoints return error" → 500) silently
// defeats the override. Guarding with a WeakSet ensures the broad `**/*` handler is registered
// exactly once per page, so any override registered later is guaranteed to win.
const mockedPages = new WeakSet<Page>();

export async function mockAllApis(page: Page, _opts?: MockOptions) {
  if (mockedPages.has(page)) return;
  mockedPages.add(page);
  await page.route("**/*", defineMockRoutes);
}

function unauthenticatedHandler(route: Route, request: Request) {
  const url = new URL(request.url());
  // Return 401 for auth check so login page renders
  if (url.pathname === "/v2/login/check") {
    return route.fulfill({ status: 401 });
  }
  // Login: mock wrong-password as 401, everything else as success
  if (url.pathname === "/v2/login") {
    const body = request.postData() ?? "";
    if (body.includes("wrong-password")) {
      return route.fulfill({ status: 401, json: { error: { message: "Invalid credentials" } } });
    }
    return route.fulfill({ status: 200, json: { user_id: "default_user_id", user_role: "proxy_admin", user_email: null } });
  }
  // Fallback to standard mock routes for all other endpoints
  return defineMockRoutes(route, request);
}

export async function mockApisUnauthenticated(page: Page) {
  await page.route("**/*", unauthenticatedHandler);
}
