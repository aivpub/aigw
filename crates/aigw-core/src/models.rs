//! Data models for aigw — column-compatible with litellm v1.90.0 schema.prisma
//!
//! aigw uses its own table names (see `docs/litellm-diff-baseline.md` §5 for mapping).
//! Fields, types, and defaults match litellm's Prisma schema at the column level,
//! ensuring data portability via the `aigw-migrate` tool.
//!
//! # Multi-tenant hierarchy (FK chain)
//!
//! ```text
//! organizations (1) ──→ (N) teams
//! teams          (1) ──→ (N) users
//! teams          (1) ──→ (N) virtual_keys
//! users          (1) ──→ (N) virtual_keys
//! projects       (1) ──→ (N) virtual_keys
//! budgets        (1) ──→ (N) virtual_keys
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// virtual_keys (55 columns, matches LiteLLM_VerificationToken)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Virtual Key — column-compatible with litellm's `LiteLLM_VerificationToken`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct VirtualKey {
    pub token: String, // SHA256 hex hash, PK
    pub key_name: Option<String>,
    pub key_alias: Option<String>,
    #[sqlx(default)]
    pub soft_budget_cooldown: String,
    pub spend: f64,
    pub expires: Option<DateTime<Utc>>,
    pub models: serde_json::Value,  // String[] → JSON array
    pub aliases: serde_json::Value, // Json @default("{}")
    pub config: serde_json::Value,  // Json @default("{}")
    pub router_settings: Option<serde_json::Value>,
    pub user_id: Option<String>,
    pub team_id: Option<String>,
    pub agent_id: Option<String>,
    pub project_id: Option<String>,
    pub permissions: serde_json::Value, // Json @default("{}")
    pub max_parallel_requests: Option<String>,
    pub metadata: serde_json::Value, // Json @default("{}")
    pub blocked: Option<bool>,
    pub tpm_limit: Option<String>, // BigInt
    pub rpm_limit: Option<String>,
    #[sqlx(default)]
    pub max_budget: Option<String>,
    pub budget_duration: Option<String>,
    pub budget_reset_at: Option<DateTime<Utc>>,
    pub allowed_cache_controls: serde_json::Value, // String[] → JSON
    pub allowed_routes: serde_json::Value,         // String[] → JSON
    pub policies: serde_json::Value,               // String[] → JSON
    pub access_group_ids: serde_json::Value,       // String[] → JSON
    pub model_spend: serde_json::Value,            // Json @default("{}")
    pub model_max_budget: serde_json::Value,       // Json @default("{}")
    pub budget_id: Option<String>,
    pub organization_id: Option<String>,
    pub object_permission_id: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub created_by: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
    pub updated_by: Option<String>,
    pub last_active: Option<DateTime<Utc>>,
    pub rotation_count: Option<i32>,
    #[sqlx(default)]
    pub auto_rotate: Option<String>,
    pub rotation_interval: Option<String>,
    pub last_rotation_at: Option<DateTime<Utc>>,
    pub key_rotation_at: Option<DateTime<Utc>>,
    pub budget_limits: Option<serde_json::Value>,
    /// From LEFT JOIN with LiteLLM_UserTable — not stored on virtual_keys
    #[sqlx(default)]
    #[serde(default)]
    pub user_email: Option<String>,
    /// From LEFT JOIN with LiteLLM_UserTable — not stored on virtual_keys
    #[sqlx(default)]
    #[serde(default)]
    pub user_alias: Option<String>,
}

impl VirtualKey {
    /// Parse `soft_budget_cooldown` as bool from TEXT-compatible string.
    pub fn soft_budget_cooldown_bool(&self) -> bool {
        self.soft_budget_cooldown.eq_ignore_ascii_case("true")
    }

    /// Parse `max_budget` as Option<f64> from TEXT-compatible string.
    pub fn max_budget_f64(&self) -> Option<f64> {
        self.max_budget.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `auto_rotate` as Option<bool> from TEXT-compatible string.
    pub fn auto_rotate_bool(&self) -> Option<bool> {
        self.auto_rotate
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("true"))
    }

    /// Parse `max_parallel_requests` as Option<i32> from TEXT-compatible string.
    pub fn max_parallel_requests_i32(&self) -> Option<i32> {
        self.max_parallel_requests.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `tpm_limit` as Option<i64> from TEXT-compatible string.
    pub fn tpm_limit_i64(&self) -> Option<i64> {
        self.tpm_limit.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `rpm_limit` as Option<i64> from TEXT-compatible string.
    pub fn rpm_limit_i64(&self) -> Option<i64> {
        self.rpm_limit.as_deref().and_then(|s| s.parse().ok())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// spend_logs (24 columns, matches LiteLLM_SpendLogs)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Spend log entry — column-compatible with litellm's `LiteLLM_SpendLogs`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SpendLog {
    pub call_id: String, // aigw gateway UUID v7, PK (renamed from request_id)
    pub call_type: String,
    pub api_key: String, // hashed API token
    pub spend: f64,
    pub total_tokens: i32,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub request_duration_ms: Option<i32>,
    pub completion_start_time: Option<DateTime<Utc>>,
    pub model: String,
    pub model_id: Option<String>,
    pub model_group: Option<String>,
    pub custom_llm_provider: Option<String>,
    pub api_base: Option<String>,
    pub user: Option<String>,
    pub metadata: Option<serde_json::Value>, // project_id stored here
    pub cache_hit: Option<String>,
    pub cache_key: Option<String>,
    pub request_tags: Option<serde_json::Value>,
    pub team_id: Option<String>,
    pub organization_id: Option<String>,
    pub end_user: Option<String>,
    pub requester_ip_address: Option<String>,
    pub messages: Option<serde_json::Value>,
    pub response: Option<serde_json::Value>,
    pub session_id: Option<String>,
    pub status: Option<String>,
    pub mcp_namespaced_tool_name: Option<String>,
    pub agent_id: Option<String>,
    pub proxy_server_request: Option<serde_json::Value>,
    pub body_archived: bool,
    pub parquet_path: Option<String>,
    /// Upstream provider's request id (e.g. Anthropic `msg_xxx`, OpenAI
    /// `chatcmpl-xxx`).  Extracted from the upstream response body / error
    /// body / response headers, for reconciliation against the provider.
    /// Populated at INSERT time for non-streaming success + 4xx/5xx failure
    /// paths, and at streaming Phase 2 UPDATE time for streaming paths.
    pub request_id: Option<String>,
}

/// Daily spend pre-aggregation record — maps to one of 6 daily_*_spend tables.
/// The entity_id field maps to user_id/team_id/organization_id/end_user_id/agent_id/tag
/// depending on the target table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailySpendLog {
    pub entity_id: String,
    pub date: String,           // YYYY-MM-DD
    pub api_key: String,
    pub model: String,
    pub model_group: String,
    pub custom_llm_provider: String,
    pub mcp_namespaced_tool_name: String,
    pub endpoint: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub spend: f64,
    pub api_requests: i64,
    pub successful_requests: i64,
    pub failed_requests: i64,
    /// Which table this belongs to
    pub kind: DailySpendKind,
}

/// Enum identifying which daily_spend table a record targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DailySpendKind {
    User,
    Team,
    Organization,
    EndUser,
    Agent,
    Tag { tag: String, call_id: String },
}

/// Spend aggregation by model
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SpendModelAgg {
    pub model: String,
    pub total_tokens: i64,
    pub total_spend: f64,
    pub requests: i64,
}

/// Spend aggregation by model_group
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SpendModelGroupAgg {
    pub model_group: String,
    pub total_tokens: i64,
    pub total_spend: f64,
    pub requests: i64,
}

/// Spend aggregation by provider (from proxy_models litellm_params JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendProviderAgg {
    pub provider: String,
    pub total_tokens: i64,
    pub total_spend: f64,
    pub requests: i64,
}

/// Spend aggregation by virtual key (for /global/spend/keys/rankings)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SpendKeyRanking {
    pub api_key: String,
    pub key_alias: Option<String>,
    pub total_spend: f64,
    pub total_requests: i64,
    pub total_tokens: i64,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Multi-tenant tables (minimum compatible — all columns, FK preserved)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Organization — column-compatible with litellm's `LiteLLM_OrganizationTable`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Organization {
    pub organization_id: String,
    pub organization_alias: String,
    pub budget_id: String,
    pub metadata: serde_json::Value,
    pub models: serde_json::Value, // String[]
    pub spend: f64,
    pub model_spend: serde_json::Value,
    pub object_permission_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
}

/// Team — column-compatible with litellm's `LiteLLM_TeamTable`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Team {
    pub team_id: String,
    pub team_alias: Option<String>,
    pub organization_id: Option<String>,
    pub object_permission_id: Option<String>,
    pub admins: serde_json::Value,  // String[]
    pub members: serde_json::Value, // String[]
    pub members_with_roles: serde_json::Value,
    pub metadata: serde_json::Value,
    #[sqlx(default)]
    pub max_budget: Option<String>,
    #[sqlx(default)]
    pub soft_budget: Option<String>,
    pub spend: f64,
    pub models: serde_json::Value, // String[]
    pub max_parallel_requests: Option<String>,
    pub tpm_limit: Option<String>,
    pub rpm_limit: Option<String>,
    pub budget_duration: Option<String>,
    pub budget_reset_at: Option<DateTime<Utc>>,
    pub blocked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub model_spend: serde_json::Value,
    pub model_max_budget: serde_json::Value,
    pub router_settings: Option<serde_json::Value>,
    pub team_member_permissions: serde_json::Value, // String[]
    pub access_group_ids: serde_json::Value,        // String[]
    pub policies: serde_json::Value,                // String[]
    pub default_team_member_models: serde_json::Value,
    pub budget_limits: Option<serde_json::Value>,
    pub model_id: Option<i32>,
    pub allow_team_guardrail_config: bool,
}

impl Team {
    /// Parse `max_budget` as Option<f64> from TEXT-compatible string.
    pub fn max_budget_f64(&self) -> Option<f64> {
        self.max_budget.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `soft_budget` as Option<f64> from TEXT-compatible string.
    pub fn soft_budget_f64(&self) -> Option<f64> {
        self.soft_budget.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `max_parallel_requests` as Option<i32> from TEXT-compatible string.
    pub fn max_parallel_requests_i32(&self) -> Option<i32> {
        self.max_parallel_requests.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `tpm_limit` as Option<i64> from TEXT-compatible string.
    pub fn tpm_limit_i64(&self) -> Option<i64> {
        self.tpm_limit.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `rpm_limit` as Option<i64> from TEXT-compatible string.
    pub fn rpm_limit_i64(&self) -> Option<i64> {
        self.rpm_limit.as_deref().and_then(|s| s.parse().ok())
    }
}

/// User — column-compatible with litellm's `LiteLLM_UserTable`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub user_id: String,
    pub user_alias: Option<String>,
    pub team_id: Option<String>,
    pub sso_user_id: Option<String>,
    pub organization_id: Option<String>,
    pub object_permission_id: Option<String>,
    pub password: Option<String>,
    pub teams: serde_json::Value, // String[]
    pub user_role: Option<String>,
    #[sqlx(default)]
    pub max_budget: Option<String>,
    pub spend: f64,
    pub user_email: Option<String>,
    pub models: serde_json::Value, // String[]
    pub metadata: serde_json::Value,
    pub max_parallel_requests: Option<String>,
    pub tpm_limit: Option<String>,
    pub rpm_limit: Option<String>,
    pub budget_duration: Option<String>,
    pub budget_reset_at: Option<DateTime<Utc>>,
    pub allowed_cache_controls: serde_json::Value, // String[]
    pub policies: serde_json::Value,               // String[]
    pub model_spend: serde_json::Value,
    pub model_max_budget: serde_json::Value,
    #[sqlx(default)]
    pub virtual_keys_count: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl User {
    /// Parse `max_budget` as Option<f64> from TEXT-compatible string.
    pub fn max_budget_f64(&self) -> Option<f64> {
        self.max_budget.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `max_parallel_requests` as Option<i32> from TEXT-compatible string.
    pub fn max_parallel_requests_i32(&self) -> Option<i32> {
        self.max_parallel_requests.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `tpm_limit` as Option<i64> from TEXT-compatible string.
    pub fn tpm_limit_i64(&self) -> Option<i64> {
        self.tpm_limit.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `rpm_limit` as Option<i64> from TEXT-compatible string.
    pub fn rpm_limit_i64(&self) -> Option<i64> {
        self.rpm_limit.as_deref().and_then(|s| s.parse().ok())
    }
}

/// Project — column-compatible with litellm's `LiteLLM_ProjectTable`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub project_id: String,
    pub project_alias: Option<String>,
    pub description: Option<String>,
    pub team_id: Option<String>,
    pub budget_id: Option<String>,
    pub metadata: serde_json::Value,
    pub models: serde_json::Value, // String[]
    pub spend: f64,
    pub model_spend: serde_json::Value,
    pub model_rpm_limit: serde_json::Value,
    pub model_tpm_limit: serde_json::Value,
    pub blocked: bool,
    pub object_permission_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
}

/// Budget — column-compatible with litellm's `LiteLLM_BudgetTable`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Budget {
    pub budget_id: String,
    #[sqlx(default)]
    pub max_budget: Option<String>,
    #[sqlx(default)]
    pub soft_budget: Option<String>,
    pub max_parallel_requests: Option<String>,
    pub tpm_limit: Option<String>,
    pub rpm_limit: Option<String>,
    pub model_max_budget: serde_json::Value,
    pub budget_duration: Option<String>,
    pub budget_reset_at: Option<DateTime<Utc>>,
    pub allowed_models: serde_json::Value, // String[]
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
}

impl Budget {
    /// Parse `max_budget` as Option<f64> from TEXT-compatible string.
    pub fn max_budget_f64(&self) -> Option<f64> {
        self.max_budget.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `max_parallel_requests` as Option<i32> from TEXT-compatible string.
    pub fn max_parallel_requests_i32(&self) -> Option<i32> {
        self.max_parallel_requests.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `tpm_limit` as Option<i64> from TEXT-compatible string.
    pub fn tpm_limit_i64(&self) -> Option<i64> {
        self.tpm_limit.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `rpm_limit` as Option<i64> from TEXT-compatible string.
    pub fn rpm_limit_i64(&self) -> Option<i64> {
        self.rpm_limit.as_deref().and_then(|s| s.parse().ok())
    }
}

/// Organization Membership — column-compatible with litellm's `LiteLLM_OrganizationMembership`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct OrganizationMembership {
    pub user_id: String,
    pub organization_id: String,
    pub user_role: Option<String>,
    pub spend: Option<f64>,
    pub budget_id: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Team Membership — column-compatible with litellm's `LiteLLM_TeamMembership`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TeamMembership {
    pub user_id: String,
    pub team_id: String,
    pub spend: f64,
    pub total_spend: f64,
    pub budget_id: Option<String>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OpenAI-compatible request/response types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// OpenAI Chat Completions request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

/// OpenAI tool definition (request)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    #[serde(rename = "type", default = "default_tool_type")]
    pub tool_type: String,
    pub function: ToolDefFunction,
}

fn default_tool_type() -> String { "function".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefFunction {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: ChatContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// OpenAI tool call (function call)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_call_type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

fn default_tool_call_type() -> String { "function".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String, // JSON string
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ImageUrl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

/// OpenAI Chat Completions response (non-streaming)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: i32,
    pub message: AssistantMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

/// SSE stream chunk (delta)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkChoice {
    pub index: i32,
    pub delta: Delta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChunkToolCall>>,
}

/// Tool call delta in streaming chunks.
/// Differs from ToolCall: `id` and `arguments` are `Option` since they arrive
/// incrementally across SSE chunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkToolCall {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none", default)]
    pub call_type: Option<String>,
    #[serde(default)]
    pub function: ChunkToolCallFunction,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub index: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChunkToolCallFunction {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: String,
}

/// /v1/models response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelListResponse {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// proxy_models — model deployment configuration (litellm ProxyModelTable)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// ProxyModel — column-compatible with litellm's `LiteLLM_ProxyModelTable`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProxyModel {
    pub model_id: String, // PK, UUID
    pub model_name: String, // unique human-readable name
    pub litellm_params: serde_json::Value, // JSON: {model, api_base, api_key, rpm, tpm, ...}
    pub model_info: serde_json::Value,     // JSON: {id, mode, max_tokens, input_cost_per_token, ...}
    pub created_at: String,
    pub created_by: Option<String>,
    pub updated_at: String,
    pub updated_by: Option<String>,
}

/// Request body for /model/new (litellm-compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddModelRequest {
    pub model_name: String,
    pub litellm_params: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_info: Option<serde_json::Value>,
}

/// Request body for /model/update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateModelRequest {
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub litellm_params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_info: Option<serde_json::Value>,
}

/// Request body for /model/delete
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteModelRequest {
    pub model_id: String,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Deleted entities — archive tables for soft-delete (tombstone-then-delete)
// Each mirrors the source table columns plus an auto-increment `id` PK
// and a `deleted_at` timestamp.
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Archived organization — mirror of organizations with id + deleted_at
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeletedOrganization {
    pub id: i64,
    pub organization_id: String,
    pub organization_alias: String,
    pub budget_id: String,
    pub metadata: serde_json::Value,
    pub models: serde_json::Value,
    pub spend: f64,
    pub model_spend: serde_json::Value,
    pub object_permission_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
    pub deleted_at: DateTime<Utc>,
}

/// Archived team — mirror of teams with id + deleted_at
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeletedTeam {
    pub id: i64,
    pub team_id: String,
    pub team_alias: Option<String>,
    pub organization_id: Option<String>,
    pub object_permission_id: Option<String>,
    pub admins: serde_json::Value,
    pub members: serde_json::Value,
    pub members_with_roles: serde_json::Value,
    pub metadata: serde_json::Value,
    #[sqlx(default)]
    pub max_budget: Option<String>,
    #[sqlx(default)]
    pub soft_budget: Option<String>,
    pub spend: f64,
    pub models: serde_json::Value,
    pub max_parallel_requests: Option<String>,
    pub tpm_limit: Option<String>,
    pub rpm_limit: Option<String>,
    pub budget_duration: Option<String>,
    pub budget_reset_at: Option<DateTime<Utc>>,
    pub blocked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub model_spend: serde_json::Value,
    pub model_max_budget: serde_json::Value,
    pub router_settings: Option<serde_json::Value>,
    pub team_member_permissions: serde_json::Value,
    pub access_group_ids: serde_json::Value,
    pub policies: serde_json::Value,
    pub default_team_member_models: serde_json::Value,
    pub budget_limits: Option<serde_json::Value>,
    pub model_id: Option<i32>,
    pub allow_team_guardrail_config: bool,
    pub deleted_at: DateTime<Utc>,
}

/// Archived user — mirror of users with id + deleted_at
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeletedUser {
    pub id: i64,
    pub user_id: String,
    pub user_alias: Option<String>,
    pub team_id: Option<String>,
    pub sso_user_id: Option<String>,
    pub organization_id: Option<String>,
    pub object_permission_id: Option<String>,
    pub password: Option<String>,
    pub teams: serde_json::Value,
    pub user_role: Option<String>,
    #[sqlx(default)]
    pub max_budget: Option<String>,
    pub spend: f64,
    pub user_email: Option<String>,
    pub models: serde_json::Value,
    pub metadata: serde_json::Value,
    pub max_parallel_requests: Option<String>,
    pub tpm_limit: Option<String>,
    pub rpm_limit: Option<String>,
    pub budget_duration: Option<String>,
    pub budget_reset_at: Option<DateTime<Utc>>,
    pub allowed_cache_controls: serde_json::Value,
    pub policies: serde_json::Value,
    pub model_spend: serde_json::Value,
    pub model_max_budget: serde_json::Value,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted_at: DateTime<Utc>,
}

/// Archived model — mirror of proxy_models with id + deleted_at
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeletedModel {
    pub id: i64,
    pub model_id: String,
    pub model_name: String,
    pub litellm_params: serde_json::Value,
    pub model_info: serde_json::Value,
    pub created_at: String,
    pub created_by: Option<String>,
    pub updated_at: String,
    pub updated_by: Option<String>,
    pub deleted_at: DateTime<Utc>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// credentials — credential storage (litellm LiteLLM_CredentialsTable)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Credential — column-compatible with litellm's `LiteLLM_CredentialsTable`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Credential {
    pub credential_id: String,
    pub credential_name: String,
    pub credential_values: serde_json::Value, // JSON, encrypted
    pub credential_info: serde_json::Value,   // JSON
    pub created_at: String,
    pub created_by: Option<String>,
    pub updated_at: String,
    pub updated_by: Option<String>,
}

/// Request body for /credential/new
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddCredentialRequest {
    pub credential_name: String,
    pub credential_values: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_info: Option<serde_json::Value>,
}

/// Request body for /credential/update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCredentialRequest {
    pub credential_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_values: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_info: Option<serde_json::Value>,
}

/// Request body for /credential/delete
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteCredentialRequest {
    pub credential_name: String,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Key management request/response types (litellm-compatible)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// /key/generate request body
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateKeyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>, // custom key value (migration-critical)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_budget: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_duration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_reset_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tpm_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpm_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_parallel_requests: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_rotate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation_interval: Option<String>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Anthropic/Claude API message types (for /v1/messages endpoint)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Anthropic Messages API request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMessageRequest {
    pub model: String,
    pub messages: Vec<ClaudeMessage>,
    pub max_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<ClaudeSystemMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ClaudeToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

/// Anthropic tool definition (request)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeToolDef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// System message content type — string or structured content blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ClaudeSystemMessage {
    Text(String),
    Blocks(Vec<ClaudeContentBlock>),
}

/// Claude message (user/assistant)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMessage {
    pub role: String,
    pub content: ClaudeContent,
}

/// Claude content — string or content blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ClaudeContent {
    Text(String),
    Blocks(Vec<ClaudeContentBlock>),
}

/// Claude content block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ClaudeImageSource>,
    // tool_use fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    // tool_result fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
}

/// Claude image source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

/// Anthropic Messages API response (non-streaming)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMessageResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub role: String,
    pub content: Vec<ClaudeContentBlock>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: ClaudeUsage,
}

/// Claude usage info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
}

/// Claude SSE stream event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<ClaudeDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_block: Option<ClaudeContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<ClaudeMessageResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ClaudeUsage>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Health Check models
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Health check result — aligned with LiteLLM_HealthCheckTable
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct HealthCheck {
    pub health_check_id: String,
    pub model_name: String,
    pub model_id: Option<String>,
    pub status: String,
    pub healthy_count: i32,
    pub unhealthy_count: i32,
    pub error_message: Option<String>,
    pub response_time_ms: Option<f64>,
    pub details: String,
    pub checked_by: Option<String>,
    pub checked_at: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Claude delta for streaming text/content block events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeDelta {
    #[serde(rename = "type")]
    pub delta_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Used for input_json_delta — Anthropic requires "partial_json", not "text"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_json: Option<String>,
}
