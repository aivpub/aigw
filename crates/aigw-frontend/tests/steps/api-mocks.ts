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
  { request_id: "req-001", call_type: "completion", model: "gpt-4", api_key: "sk-abc***", key_name: "prod-gpt-key", total_tokens: 1234, prompt_tokens: 800, completion_tokens: 434, spend: 0.42, start_time: "2026-07-08T10:00:00Z", end_time: "2026-07-08T10:00:05Z", request_duration_ms: 5123, ttft_ms: 234.5, status: "success", custom_llm_provider: "openai", model_group: "gpt-4", user: "test-user", requester_ip_address: "192.168.1.1" },
  { request_id: "req-002", call_type: "completion", model: "claude-sonnet-4-6", api_key: "sk-def***", key_name: "dev-claude-key", total_tokens: 567, prompt_tokens: 300, completion_tokens: 267, spend: 1.23, start_time: "2026-07-08T10:05:00Z", end_time: "2026-07-08T10:05:03Z", request_duration_ms: 2890, ttft_ms: 456.7, status: "success", custom_llm_provider: "anthropic", model_group: "claude-sonnet-4-6", user: "dev-user" },
];

// Detail mocks for the new detail endpoint (GET /global/spend/logs/{request_id})
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
    // Detail endpoint: extract request_id from path
    const rid = url.pathname.replace("/global/spend/logs/", "");
    const detail = rid === "req-001" ? sampleDetailLog1 : rid === "req-002" ? sampleDetailLog2 : null;
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

export async function mockAllApis(page: Page, _opts?: MockOptions) {
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
