export const CREDENTIAL_SCOPES = [
  "shared",
  "shared_or_user",
  "user_required",
] as const;

export type CredentialScope = (typeof CREDENTIAL_SCOPES)[number];

/** wire value (`shared_or_user`) -> i18n key segment (`sharedOrUser`). */
const SCOPE_I18N_KEY: Record<CredentialScope, "shared" | "sharedOrUser" | "userRequired"> = {
  shared: "shared",
  shared_or_user: "sharedOrUser",
  user_required: "userRequired",
};

export function scopeI18nKey(scope: string): "shared" | "sharedOrUser" | "userRequired" {
  return SCOPE_I18N_KEY[scope as CredentialScope] ?? "shared";
}

export interface McpExtraHeader {
  name: string;
  /** `***` in every response; the plaintext value is write-only. */
  value: string;
}

export interface McpServer {
  id: number;
  alias: string;
  display_name: string;
  url: string;
  transport: string;
  credential_scope: CredentialScope;
  enabled: boolean;
  protocol_version: string | null;
  /** `null` = the protocol's `Authorization: Bearer`. */
  auth_header_name: string | null;
  /** `***` when configured, `null` when not. Never the value. */
  auth_header_value: string | null;
  extra_headers: McpExtraHeader[] | null;
  tool_count?: number | null;
  created_at: string;
  updated_at: string;
}

/** Headers the gateway writes itself; the backend rejects them with 400. */
export const RESERVED_HEADERS = [
  "content-type",
  "accept",
  "mcp-protocol-version",
  "mcp-session-id",
  "host",
  "content-length",
];

const HEADER_NAME_RE = /^[A-Za-z0-9!#$%&'*+\-.^_`|~]+$/;

export function validateHeaderName(
  name: string,
): "reserved" | "charset" | "tooLong" | null {
  const trimmed = name.trim();
  if (trimmed.length > 256) return "tooLong";
  if (!HEADER_NAME_RE.test(trimmed)) return "charset";
  if (RESERVED_HEADERS.includes(trimmed.toLowerCase())) return "reserved";
  return null;
}

/** The wire shape `ExtraHeader` expects: names in clear, values as sent. */
export function extraHeaderPayload(
  rows: { name: string; value: string }[],
): McpExtraHeader[] {
  return rows
    .map((row) => ({ name: row.name.trim(), value: row.value }))
    .filter((row) => row.name.length > 0);
}

export interface McpServerListResponse {
  object?: string;
  data: McpServer[];
  count?: number;
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

export interface McpToken {
  id: number;
  server_id: number;
  owner_type: "server" | "user";
  owner_id: string;
  token_name: string;
  /** `***` when a value is stored, empty string when unset. Never the value. */
  token_redacted: string;
  is_default: boolean;
  enabled: boolean;
  expires_at: string | null;
  created_by: string;
  created_at: string;
  updated_at: string;
}

export interface McpTool {
  tool_name: string;
  description: string | null;
  input_schema?: Record<string, unknown>;
  annotations?: Record<string, unknown> | null;
  has_conflict?: boolean;
}

export interface ProbeOutcome {
  protocol_version: string;
  server_info: Record<string, unknown> | null;
  ttl_ms: number | null;
  tools: McpTool[];
  conflicts: string[];
}

/** `GET /v1/mcp/servers` row — what the current identity can see (stage-144 §3.4). */
export interface McpVisibleServer {
  id: number;
  alias: string;
  display_name: string;
  credential_scope: CredentialScope;
  enabled: boolean;
  has_my_token: boolean;
  token_required: boolean;
}

export interface McpVisibleServerList {
  object?: string;
  data: McpVisibleServer[];
  count?: number;
}

/** `GET|PUT /v1/mcp/servers/{id}/my-token` — metadata only, never the value. */
export interface McpMyToken {
  token_name: string;
  is_default: boolean;
  expires_at: string | null;
  updated_at: string;
}

/** `mixed` means an aggregate listing reached upstreams with different credentials. */
export type McpTokenOwner = "server" | "user" | "anonymous" | "mixed";

/** `GET /admin/mcp/calls` row — one-to-one with `mcp_call_log` (stage-144 §3.5). */
export interface McpCallRow {
  call_id: string;
  server_alias: string;
  tool_name: string | null;
  method: string;
  key_id: string | null;
  user_id: string | null;
  /** Resolved at read time for display; the id above remains the audit fact. */
  key_name?: string | null;
  user_name?: string | null;
  token_owner: McpTokenOwner;
  status: string;
  error_code: string | null;
  latency_ms: number;
  request_bytes: number | null;
  response_bytes: number | null;
  created_at: string;
}

export interface McpCallListResponse {
  object?: string;
  data: McpCallRow[];
  count?: number;
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

/** `GET /admin/mcp/calls/{id}/body` — null halves mean the field was absent
 *  upstream, which is different from a body that was never recorded. */
export interface McpCallBody {
  call_id: string;
  request: string | null;
  response: string | null;
  truncated: boolean;
}

/** Why a request was refused before it could reach any upstream (stage-150). */
export type McpRejectionReason = "missing_header" | "invalid_format" | "invalid_key";

/** `GET /admin/mcp/rejections` row — one-to-one with `mcp_auth_rejection`. */
export interface McpRejectionRow {
  id: string;
  created_at: string;
  path: string;
  http_method: string;
  reason: string;
  /** SHA-256 of what the client presented, never the token itself. */
  token_hash: string | null;
  client_ip: string | null;
  user_agent: string | null;
}

export interface McpRejectionListResponse {
  object?: string;
  data: McpRejectionRow[];
  count?: number;
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

export const ACL_SCOPE_TYPES = ["key", "team", "org"] as const;

export type AclScopeType = (typeof ACL_SCOPE_TYPES)[number];

/** `PUT/GET /admin/mcp/servers/{id}/allowlist` — whole-table replacement. */
export interface McpAllowlistRule {
  tool_name: string;
  scope_type: AclScopeType;
  scope_id: string;
  allowed: boolean;
}

export interface McpAllowlistResponse {
  object?: string;
  data: McpAllowlistRule[];
  count?: number;
}

/** `POST /admin/mcp/reload` — the sweep was accepted and is running. */
export interface ReloadStarted {
  run_id: string;
  total: number;
}

export type ProbeItemState = "pending" | "running" | "ok" | "failed";
export type ProbeRunStatus = "running" | "ok" | "partial";

export interface ProbeRunItem {
  server_id: number;
  alias: string;
  state: ProbeItemState;
  tool_count: number | null;
  protocol_version: string | null;
  error: string | null;
}

/** `GET /admin/mcp/reload/{run_id}` and `/reload/latest`. */
export interface ProbeRun {
  run_id: string;
  total: number;
  done: number;
  status: ProbeRunStatus;
  items: ProbeRunItem[];
}

export function isCallFailure(status: string): boolean {
  return status !== "success";
}

const MAX_ALIAS_LEN = 32;

export function validateAlias(
  alias: string,
): "required" | "doubleUnderscore" | "trailingUnderscore" | "charset" | "tooLong" | null {
  if (!alias) return "required";
  if (alias.length > MAX_ALIAS_LEN) return "tooLong";
  if (alias.includes("__")) return "doubleUnderscore";
  if (alias.endsWith("_")) return "trailingUnderscore";
  if (!/^[A-Za-z0-9_.-]+$/.test(alias)) return "charset";
  return null;
}
