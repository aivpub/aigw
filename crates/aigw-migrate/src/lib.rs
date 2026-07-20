//! aigw-migrate library — used by the CLI binary and BDD tests.
//!
//! Exports the core migration functions from `remote_import`.

/// Table name pairs for bidirectional migration.
/// Order: (litellm_table_name, aigw_table_name).
pub const TABLE_MAPPINGS: &[(&str, &str)] = &[
    ("LiteLLM_VerificationToken", "virtual_keys"),
    ("LiteLLM_SpendLogs", "spend_logs"),
    ("LiteLLM_OrganizationTable", "organizations"),
    ("LiteLLM_TeamTable", "teams"),
    ("LiteLLM_UserTable", "users"),
    ("LiteLLM_ProjectTable", "projects"),
    ("LiteLLM_BudgetTable", "budgets"),
    ("LiteLLM_OrganizationMembership", "organization_memberships"),
    ("LiteLLM_TeamMembership", "team_memberships"),
    ("LiteLLM_ProxyModelTable", "proxy_models"),
    ("LiteLLM_Config", "config"),
    ("LiteLLM_CredentialsTable", "credentials"),
];

pub mod export;
pub mod import;
pub mod native;
pub mod pre_check;
pub mod remote_export;
pub mod remote_import;
pub mod verify;

pub use native::CursorRange;
pub use remote_export::run as remote_export_run;
pub use remote_import::run_filtered as remote_import_run_filtered;
