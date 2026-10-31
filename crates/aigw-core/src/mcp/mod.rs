//! MCP gateway foundation (Phase 55, Stages 140/141).
//!
//! Data layer, pure logic, upstream probing and the process-level catalog cache.
//! No HTTP handlers — those live in `aigw-server::routes::mcp`.
//!
//! The credential direction is fixed: the client's `Authorization` is an aigw
//! virtual key and is never forwarded upstream. The only thing that can go
//! outbound is a row from `mcp_server_tokens`, and
//! [`credential::resolve_outbound_token`]'s signature gives a client credential
//! nowhere to enter — that is a compile-time guarantee, not a convention.

pub mod acl;
pub mod alias;
pub mod body;
pub mod catalog;
pub mod catalog_cache;
pub mod credential;
pub mod headers;
pub mod probe;
pub mod probe_runs;
pub mod session;

pub use alias::validate_alias;
pub use credential::{
    is_token_usable, redact_token, resolve_outbound_token, seal_token, unseal_token, ResolvedToken,
    TokenSource,
};
pub use headers::{
    validate_extra_headers, validate_name, validate_value, ExtraHeader, HeaderError,
};
pub use probe::{probe_server, read_jsonrpc_response, read_jsonrpc_response_for, ProbeError};
pub use probe_runs::{
    registry as probe_run_registry, ItemState, ProbeRun, ProbeRunItem, RunInProgress, RunStatus,
};
