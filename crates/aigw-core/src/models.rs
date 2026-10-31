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
    #[sqlx(default)]
    pub soft_budget: Option<String>,
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
        self.max_parallel_requests
            .as_deref()
            .and_then(|s| s.parse().ok())
    }

    /// Parse `tpm_limit` as Option<i64> from TEXT-compatible string.
    pub fn tpm_limit_i64(&self) -> Option<i64> {
        self.tpm_limit.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `rpm_limit` as Option<i64> from TEXT-compatible string.
    pub fn rpm_limit_i64(&self) -> Option<i64> {
        self.rpm_limit.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `soft_budget` as Option<f64> from TEXT-compatible string.
    pub fn soft_budget_f64(&self) -> Option<f64> {
        self.soft_budget.as_deref().and_then(|s| s.parse().ok())
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// spend_logs (24 columns, matches LiteLLM_SpendLogs)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Per-modality input pricing (TD-012b), USD per 1M tokens — supports
/// providers like Gemini embeddings that bill image/audio/video differently.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModalPricing {
    /// USD per 1M image input tokens.
    pub image: Option<f64>,
    /// USD per 1M audio input tokens.
    pub audio: Option<f64>,
    /// USD per 1M video input tokens.
    pub video: Option<f64>,
}

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
    /// Image tokens consumed in this request — a subset of prompt_tokens.
    /// Value source: upstream (Qwen/Gemini) → client-side estimate
    /// (OpenAI/Anthropic) → NULL (unknown / text-only).  Stored source tag
    /// ("upstream" | "estimated") lives in metadata.image_tokens_source.
    pub image_tokens: Option<i32>,
}

/// Daily spend pre-aggregation record — maps to one of 6 daily_*_spend tables.
/// The entity_id field maps to user_id/team_id/organization_id/end_user_id/agent_id/tag
/// depending on the target table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailySpendLog {
    pub entity_id: String,
    pub date: String, // YYYY-MM-DD
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
    pub image_tokens: i64,
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
        self.max_parallel_requests
            .as_deref()
            .and_then(|s| s.parse().ok())
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
        self.max_parallel_requests
            .as_deref()
            .and_then(|s| s.parse().ok())
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

    /// Parse `soft_budget` as Option<f64> from TEXT-compatible string.
    pub fn soft_budget_f64(&self) -> Option<f64> {
        self.soft_budget.as_deref().and_then(|s| s.parse().ok())
    }

    /// Parse `max_parallel_requests` as Option<i32> from TEXT-compatible string.
    pub fn max_parallel_requests_i32(&self) -> Option<i32> {
        self.max_parallel_requests
            .as_deref()
            .and_then(|s| s.parse().ok())
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
    /// JSON mode / Structured Output config
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub response_format: Option<ResponseFormat>,
    /// DeepSeek/OpenAI o-series reasoning effort: "low", "medium", "high"
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning_effort: Option<String>,
}

/// Response format config (JSON mode / Structured Output)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<serde_json::Value>,
}

/// OpenAI tool definition (request)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    #[serde(rename = "type", default = "default_tool_type")]
    pub tool_type: String,
    pub function: ToolDefFunction,
}

fn default_tool_type() -> String {
    "function".to_string()
}

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
    /// DeepSeek/OpenAI reasoning content — must be preserved across multi-turn
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning_content: Option<String>,
}

/// OpenAI tool call (function call)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_call_type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

fn default_tool_call_type() -> String {
    "function".to_string()
}

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
    /// System fingerprint for monitoring/tracing
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub system_fingerprint: Option<String>,
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
    /// DeepSeek/OpenAI reasoning content from thinking mode
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning_content: Option<String>,
    /// Safety refusal reason when model blocks a response
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub refusal: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    /// Prompt token details (cached_tokens, audio_tokens)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub prompt_tokens_details: Option<TokenDetails>,
    /// Completion token details (reasoning_tokens, audio_tokens, etc.)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub completion_tokens_details: Option<TokenDetails>,
    /// Server-side tool counters. Without a field here the upstream's
    /// `server_tool_use` is silently dropped on deserialization.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub server_tool_use: Option<ServerToolUse>,
}

/// Server-side tool usage counters (`usage.server_tool_use`).
///
/// Anthropic defines this shape; aigw echoes it on all three surfaces so a
/// client can tell how many searches its request caused.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerToolUse {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub web_search_requests: Option<i32>,
}

/// Detailed token breakdown (OpenAI/DeepSeek usage details)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_prediction_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_prediction_tokens: Option<i32>,
}

/// SSE stream chunk (delta)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
    /// Usage in final chunk (when stream_options.include_usage is set)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage: Option<Usage>,
    /// System fingerprint
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub system_fingerprint: Option<String>,
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
    /// DeepSeek/OpenAI reasoning delta (thinking mode streaming)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning_content: Option<String>,
    /// Safety refusal delta in streaming
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub refusal: Option<String>,
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
    pub model_id: String,                  // PK, UUID
    pub model_name: String,                // unique human-readable name
    pub litellm_params: serde_json::Value, // JSON: {model, api_base, api_key, rpm, tpm, ...}
    pub model_info: serde_json::Value, // JSON: {id, mode, max_tokens, input_cost_per_token, ...}
    pub created_at: String,
    pub created_by: Option<String>,
    pub updated_at: String,
    pub updated_by: Option<String>,
    /// Stage 121: independent enable/disable switch, separate from
    /// `model_info.mode` (which carries business category like "embed"/"image").
    /// When false, the model is skipped by resolver + router.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
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
    /// Stage 121: independent enable/disable switch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
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
    /// Stage 121: mirrors proxy_models.enabled at the moment of deletion.
    #[serde(default = "default_true")]
    pub enabled: bool,
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
    /// Extended thinking config: {type: "enabled", budget_tokens: N}
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thinking: Option<serde_json::Value>,
}

/// Anthropic tool definition (request)
///
/// Client tools carry `name` + `description` + `input_schema`. Server-side
/// tools (`web_search_20250305`, `code_execution_*`, ...) carry `type` and
/// `max_uses` instead and have no schema at all — hence every field past
/// `name` is optional. Before Phase 53 a missing `input_schema` failed
/// deserialization and surfaced as HTTP 500 on all three routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeToolDef {
    /// Server-tool discriminant. Absent on client tools.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none", default)]
    pub tool_type: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub input_schema: Option<serde_json::Value>,
    /// Search count ceiling on `web_search_20250305`. Read as an upper bound on
    /// the result count (`websearch::trigger::detect_anthropic`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_uses: Option<i64>,
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
    // thinking block fields (Anthropic extended thinking)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub signature: Option<String>,
    // citations on text blocks
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub citations: Option<serde_json::Value>,
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
    /// Anthropic prompt caching: cache read tokens
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cache_read_input_tokens: Option<i32>,
    /// Anthropic prompt caching: cache creation tokens
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cache_creation_input_tokens: Option<i32>,
    /// Server-side tool counters — the native Anthropic field aigw echoes.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub server_tool_use: Option<ServerToolUse>,
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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage: Option<ClaudeUsage>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// proxies — proxy service management (Phase 50, Stage 122)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Proxy — a single outbound proxy service.
///
/// `proxy_url` is stored **encrypted** (AES-GCM `v2:gcm:` with master_key);
/// the response decrypts + redacts the password before exposing it.
/// `probe_result` is a single JSON snapshot (latency/exit/score/grade/items),
/// filled by the Stage 123 probe engine; the top-level `status` column is only
/// used for filtering.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Proxy {
    pub id: i64,
    pub name: String,
    /// Encrypted ciphertext as stored in the DB (`v2:gcm:...`). Handlers
    /// decrypt this before use and redact the password in responses.
    pub proxy_url: String,
    /// active / inactive / expired
    pub status: String,
    pub expires_at: Option<String>,
    #[sqlx(default)]
    pub probe_result: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

/// Request body for POST /admin/proxies (create)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProxyRequest {
    pub name: String,
    pub proxy_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Request body for PUT /admin/proxies/{id} (update)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProxyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Web search provider models (Phase 53, Stage 138)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// One search provider: a logical backend plus its pricing.
///
/// "Which row is the default" is not a column — it is the row with the smallest
/// non-NULL `failover_order` (ties broken by id). One fact, one source.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebSearchProviderRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub cost_per_query: f64,
    pub enabled: bool,
    pub failover_order: Option<i64>,
    pub timeout_ms: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// One physical endpoint of a provider. `api_key` is stored encrypted
/// (`v2:gcm:`) and redacted in responses, same as `proxies.proxy_url`.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebSearchInstanceRow {
    pub id: i64,
    pub provider_id: i64,
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub weight: Option<i64>,
    pub enabled: bool,
    #[sqlx(default)]
    pub probe_result: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// MCP gateway models (Phase 55, Stage 140)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Whose upstream credential a server uses on egress.
///
/// `UserRequired` deliberately never falls back to the shared token: falling
/// back would make it `SharedOrUser`, which is a security-semantics error, not
/// a preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialScope {
    Shared,
    SharedOrUser,
    UserRequired,
}

impl CredentialScope {
    pub const DEFAULT: CredentialScope = CredentialScope::SharedOrUser;

    pub fn as_str(self) -> &'static str {
        match self {
            CredentialScope::Shared => "shared",
            CredentialScope::SharedOrUser => "shared_or_user",
            CredentialScope::UserRequired => "user_required",
        }
    }

    /// Unknown values parse to [`CredentialScope::UserRequired`], not to
    /// [`CredentialScope::DEFAULT`]: a value this binary never wrote means the
    /// row is corrupt or foreign, and the safe direction is to withhold the
    /// shared token instead of silently granting it.
    pub fn parse(value: &str) -> CredentialScope {
        match value {
            "shared" => CredentialScope::Shared,
            "shared_or_user" => CredentialScope::SharedOrUser,
            _ => CredentialScope::UserRequired,
        }
    }
}

/// One registered MCP upstream server. `alias` prefixes its tool names
/// (`{alias}__{tool}`) and is validated by [`crate::mcp::alias::validate_alias`].
///
/// `tool_count` is a projection, not a column: the list/paged reads aggregate
/// `mcp_tool_catalog` (`COALESCE(COUNT(c.id), 0)`). `#[sqlx(default)]` covers the
/// queries that carry no aggregate — `McpServerRow::tool_count` is then 0, not an
/// error. Defaults do **not** cover NULL, which is why the aggregate coalesces.
///
/// `auth_header_name` is `NULL` for the protocol's `Authorization: Bearer`.
/// `auth_header_value` and `extra_headers` hold `v2:gcm:` ciphertext exactly as
/// `mcp_server_tokens.token_value` does — the plaintext exists only between
/// [`crate::mcp::credential::unseal_token`] and the outbound request builder.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct McpServerRow {
    pub id: i64,
    pub alias: String,
    pub display_name: String,
    pub url: String,
    pub transport: String,
    pub credential_scope: String,
    pub enabled: bool,
    pub protocol_version: Option<String>,
    pub auth_header_name: Option<String>,
    pub auth_header_value: Option<String>,
    pub extra_headers: Option<String>,
    #[sqlx(default)]
    pub tool_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl McpServerRow {
    pub fn scope(&self) -> CredentialScope {
        CredentialScope::parse(&self.credential_scope)
    }
}

/// One upstream credential. `token_value` is the `v2:gcm:` ciphertext as
/// stored; plaintext never leaves the decrypt-at-egress path.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct McpServerTokenRow {
    pub id: i64,
    pub server_id: i64,
    pub owner_type: String,
    pub owner_id: String,
    pub token_name: String,
    pub token_value: String,
    pub is_default: bool,
    pub enabled: bool,
    pub expires_at: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

pub const MCP_OWNER_SERVER: &str = "server";
pub const MCP_OWNER_USER: &str = "user";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMcpServerRequest {
    pub alias: String,
    pub display_name: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(default)]
    pub credential_scope: Option<CredentialScope>,
    /// Absent means enabled. The dialog always sends it, and dropping it
    /// silently would create an enabled server behind a switched-off toggle.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// `None` / empty = the protocol's `Authorization: Bearer`.
    #[serde(default)]
    pub auth_header_name: Option<String>,
    /// Plaintext, write-only. Never echoed back.
    #[serde(default)]
    pub auth_header_value: Option<String>,
    #[serde(default)]
    pub extra_headers: Option<Vec<crate::mcp::headers::ExtraHeader>>,
}

/// The three header fields follow one rule, and it is the rule that is easiest to
/// get wrong: **absent means "leave alone", empty means "clear"**. A plain
/// `Option<String>` cannot express that, so `auth_header_name` and
/// `auth_header_value` are `Option<Option<String>>` — `None` for absent, `Some("")`
/// for a deliberate clear. Without it an operator could never remove a value they
/// once configured.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMcpServerRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_scope: Option<CredentialScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_header_name: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_header_value: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_headers: Option<Vec<crate::mcp::headers::ExtraHeader>>,
}

/// Token metadata in responses — `token_value` is always `***`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTokenResponse {
    pub id: i64,
    pub server_id: i64,
    pub owner_type: String,
    pub owner_id: String,
    pub token_name: String,
    pub token_redacted: String,
    pub is_default: bool,
    pub enabled: bool,
    pub expires_at: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&McpServerTokenRow> for McpTokenResponse {
    fn from(row: &McpServerTokenRow) -> Self {
        McpTokenResponse {
            id: row.id,
            server_id: row.server_id,
            owner_type: row.owner_type.clone(),
            owner_id: row.owner_id.clone(),
            token_name: row.token_name.clone(),
            token_redacted: crate::mcp::credential::redact_token(&row.token_value),
            is_default: row.is_default,
            enabled: row.enabled,
            expires_at: row.expires_at.clone(),
            created_by: row.created_by.clone(),
            created_at: row.created_at.clone(),
            updated_at: row.updated_at.clone(),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// MCP tool catalog (Phase 55, Stage 141)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// One row of `mcp_tool_catalog`: a tool as the upstream named it, snapshot
/// semantics (a probe replaces the whole set for its server).
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct McpToolRow {
    pub id: i64,
    pub server_id: i64,
    pub tool_name: String,
    pub description: String,
    #[sqlx(default)]
    pub input_schema: serde_json::Value,
    #[sqlx(default)]
    pub annotations: Option<serde_json::Value>,
    pub has_conflict: bool,
    pub fetched_at: String,
}

/// A tool on the wire: the catalog columns without `id`/`server_id`/`fetched_at`,
/// which are storage facts rather than tool facts. Field names are the column
/// names — the frontend reads the same table through the same names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamTool {
    pub tool_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub annotations: Option<serde_json::Value>,
    pub has_conflict: bool,
}

impl UpstreamTool {
    pub fn from_row(row: &McpToolRow) -> Self {
        UpstreamTool {
            tool_name: row.tool_name.clone(),
            description: row.description.clone(),
            input_schema: row.input_schema.clone(),
            annotations: row.annotations.clone(),
            has_conflict: row.has_conflict,
        }
    }
}

/// What one probe learned about a server — the `POST .../probe` body, pinned by
/// Stage 142.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeOutcome {
    pub protocol_version: String,
    pub server_info: Option<serde_json::Value>,
    pub ttl_ms: Option<i64>,
    pub tools: Vec<UpstreamTool>,
    pub conflicts: Vec<String>,
}

/// `GET /admin/mcp/servers/{id}/tools` — the stored snapshot. No snapshot and an
/// empty snapshot are the same answer here; the distinction lives in `fetched_at`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogSnapshot {
    pub tools: Vec<UpstreamTool>,
    pub conflicts: Vec<String>,
    pub fetched_at: Option<String>,
}

impl CatalogSnapshot {
    pub fn from_rows(rows: &[McpToolRow]) -> Self {
        CatalogSnapshot {
            tools: rows.iter().map(UpstreamTool::from_row).collect(),
            conflicts: conflict_names(rows),
            // One snapshot is written with one timestamp, but the rows arrive
            // `ORDER BY tool_name` — `first()` would make the answer depend on
            // alphabetical order rather than on the write.
            fetched_at: rows.iter().map(|r| r.fetched_at.clone()).max(),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// MCP call log (Phase 55, Stage 143)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// One row of `mcp_call_log` — written whether the call succeeded or failed.
///
/// `method` is the MCP method name, not the HTTP verb. `key_id` is a
/// `token_hash` and is always populated (master key requests carry the literal
/// `*master*`); only `user_id` can be absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct McpCallLogRow {
    pub call_id: String,
    pub server_alias: String,
    pub tool_name: Option<String>,
    pub method: String,
    pub key_id: Option<String>,
    pub user_id: Option<String>,
    pub token_owner: String,
    pub status: String,
    pub error_code: Option<String>,
    pub latency_ms: i64,
    pub request_bytes: i64,
    pub response_bytes: i64,
    pub created_at: String,
}

pub const MCP_CALL_STATUS_SUCCESS: &str = "success";
pub const MCP_CALL_STATUS_FAILED: &str = "failed";

/// One row of `mcp_call_body` — the request and response behind a call-log row
/// (Stage 152).
///
/// Both bodies are already bounded by `mcp_log_retention.max_body_kib` when they
/// reach here; `truncated` says whether the stored text is only a prefix. The
/// row is absent (not empty) when body recording is switched off, so a 404 from
/// the detail endpoint is the honest answer rather than two `null`s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct McpCallBodyRow {
    pub call_id: String,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub truncated: bool,
    pub created_at: String,
}

/// One row of `mcp_auth_rejection`: a request turned away by the auth
/// extractor, before any handler ran.
///
/// `token_hash` is the SHA-256 of whatever token was presented (NULL when none
/// was), which is what lets an operator find "which key was sent here by
/// mistake" without the log ever holding a credential.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct McpAuthRejectionRow {
    pub id: String,
    pub created_at: String,
    pub path: String,
    pub http_method: String,
    pub reason: String,
    pub token_hash: Option<String>,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
}

/// Values `McpAuthRejectionRow::reason` can take. `TokenExpired` /
/// `TokenBlocked` are absent because `SpendAuth` never produces them.
pub const MCP_REJECTION_MISSING_HEADER: &str = "missing_header";
pub const MCP_REJECTION_INVALID_FORMAT: &str = "invalid_format";
pub const MCP_REJECTION_INVALID_KEY: &str = "invalid_key";

fn conflict_names(rows: &[McpToolRow]) -> Vec<String> {
    let mut names: Vec<String> = rows
        .iter()
        .filter(|r| r.has_conflict)
        .map(|r| r.tool_name.clone())
        .collect();
    names.sort();
    names.dedup();
    names
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

#[cfg(test)]
mod credential_scope_tests {
    use super::CredentialScope;

    #[test]
    fn unknown_scope_parses_to_the_restrictive_variant() {
        assert_eq!(CredentialScope::parse("shared"), CredentialScope::Shared);
        assert_eq!(
            CredentialScope::parse("shared_or_user"),
            CredentialScope::SharedOrUser
        );
        assert_eq!(
            CredentialScope::parse("user_required"),
            CredentialScope::UserRequired
        );
        assert_eq!(
            CredentialScope::parse("nonsense"),
            CredentialScope::UserRequired
        );
        assert_eq!(CredentialScope::parse(""), CredentialScope::UserRequired);
    }
}

#[cfg(test)]
mod catalog_snapshot_tests {
    use super::{CatalogSnapshot, McpToolRow};
    use serde_json::json;

    fn row(tool_name: &str, fetched_at: &str, has_conflict: bool) -> McpToolRow {
        McpToolRow {
            id: 0,
            server_id: 1,
            tool_name: tool_name.to_string(),
            description: String::new(),
            input_schema: json!({}),
            annotations: None,
            has_conflict,
            fetched_at: fetched_at.to_string(),
        }
    }

    #[test]
    fn fetched_at_is_the_snapshot_write_not_the_alphabetically_first_row() {
        let rows = vec![
            row("aaa", "2026-01-01T00:00:00Z", false),
            row("zzz", "2026-05-05T00:00:00Z", false),
        ];
        assert_eq!(
            CatalogSnapshot::from_rows(&rows).fetched_at.as_deref(),
            Some("2026-05-05T00:00:00Z")
        );
    }

    #[test]
    fn an_empty_snapshot_has_no_timestamp_and_no_conflicts() {
        let snapshot = CatalogSnapshot::from_rows(&[]);
        assert!(snapshot.tools.is_empty());
        assert!(snapshot.conflicts.is_empty());
        assert_eq!(snapshot.fetched_at, None);
    }

    #[test]
    fn conflicts_are_the_sorted_deduplicated_flagged_names() {
        let rows = vec![
            row("b", "2026-01-01T00:00:00Z", true),
            row("a", "2026-01-01T00:00:00Z", true),
            row("c", "2026-01-01T00:00:00Z", false),
        ];
        assert_eq!(CatalogSnapshot::from_rows(&rows).conflicts, vec!["a", "b"]);
    }
}
