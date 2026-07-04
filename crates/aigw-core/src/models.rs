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
    pub soft_budget_cooldown: bool,
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
    pub max_parallel_requests: Option<i32>,
    pub metadata: serde_json::Value, // Json @default("{}")
    pub blocked: Option<bool>,
    pub tpm_limit: Option<i64>, // BigInt
    pub rpm_limit: Option<i64>,
    pub max_budget: Option<f64>,
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
    pub auto_rotate: Option<bool>,
    pub rotation_interval: Option<String>,
    pub last_rotation_at: Option<DateTime<Utc>>,
    pub key_rotation_at: Option<DateTime<Utc>>,
    pub budget_limits: Option<serde_json::Value>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// spend_logs (24 columns, matches LiteLLM_SpendLogs)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Spend log entry — column-compatible with litellm's `LiteLLM_SpendLogs`
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SpendLog {
    pub request_id: String, // UUID, PK
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
    pub max_budget: Option<f64>,
    pub soft_budget: Option<f64>,
    pub spend: f64,
    pub models: serde_json::Value, // String[]
    pub max_parallel_requests: Option<i32>,
    pub tpm_limit: Option<i64>,
    pub rpm_limit: Option<i64>,
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
    pub max_budget: Option<f64>,
    pub spend: f64,
    pub user_email: Option<String>,
    pub models: serde_json::Value, // String[]
    pub metadata: serde_json::Value,
    pub max_parallel_requests: Option<i32>,
    pub tpm_limit: Option<i64>,
    pub rpm_limit: Option<i64>,
    pub budget_duration: Option<String>,
    pub budget_reset_at: Option<DateTime<Utc>>,
    pub allowed_cache_controls: serde_json::Value, // String[]
    pub policies: serde_json::Value,               // String[]
    pub model_spend: serde_json::Value,
    pub model_max_budget: serde_json::Value,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
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
    pub max_budget: Option<f64>,
    pub soft_budget: Option<f64>,
    pub max_parallel_requests: Option<i32>,
    pub tpm_limit: Option<i64>,
    pub rpm_limit: Option<i64>,
    pub model_max_budget: serde_json::Value,
    pub budget_duration: Option<String>,
    pub budget_reset_at: Option<DateTime<Utc>>,
    pub allowed_models: serde_json::Value, // String[]
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: ChatContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
// Key management request/response types (litellm-compatible)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// /key/generate request body
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateKeyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>, // custom key value (migration-critical)
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
