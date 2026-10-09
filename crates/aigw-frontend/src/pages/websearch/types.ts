export interface WebSearchInstance {
  id: number;
  provider_id: number;
  name: string;
  base_url: string;
  has_api_key: boolean;
  redacted_api_key: string | null;
  weight: number | null;
  enabled: boolean;
  probe_result: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

export interface WebSearchProvider {
  id: number;
  name: string;
  kind: string;
  cost_per_query: number;
  enabled: boolean;
  /** The row that serves un-named requests: smallest non-null failover_order. */
  is_default: boolean;
  failover_order: number | null;
  timeout_ms: number | null;
  instances: WebSearchInstance[];
  created_at: string;
  updated_at: string;
}

export interface WebSearchProviderListResponse {
  data: WebSearchProvider[];
  count: number;
}

export interface WebSearchReloadOutcome {
  ok: boolean;
  providers?: string[];
  error?: string;
}

export interface ProbeSnapshot {
  ok: boolean;
  http_status: number | null;
  latency_ms: number | null;
  result_count: number | null;
  parse_ok: boolean;
  probed_at: string;
  error: string | null;
}

/** The kinds the backend will accept. Writing anything else is a 400. */
export const SUPPORTED_KINDS = ["searxng"] as const;

/** Recognised but not yet implemented — the backend names these separately. */
export const PLANNED_KINDS = ["tavily", "bocha"] as const;

/**
 * The docs' industry list price is $10/1k = 0.01/query, which is what the
 * column defaults to. A self-hosted SearXNG amortizes nearer 1e-4, so anything
 * above this reads as "you kept the placeholder price".
 */
export const PLACEHOLDER_PRICE_THRESHOLD = 0.001;
