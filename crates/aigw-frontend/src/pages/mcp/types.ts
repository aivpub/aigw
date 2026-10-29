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

export interface McpServer {
  id: number;
  alias: string;
  display_name: string;
  url: string;
  transport: string;
  credential_scope: CredentialScope;
  enabled: boolean;
  protocol_version: string | null;
  tool_count?: number | null;
  created_at: string;
  updated_at: string;
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

export type McpTokenOwner = "server" | "user" | "anonymous";

/** `GET /admin/mcp/calls` row — one-to-one with `mcp_call_log` (stage-144 §3.5). */
export interface McpCallRow {
  call_id: string;
  server_alias: string;
  tool_name: string;
  method: string;
  key_id: string | null;
  user_id: string | null;
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
