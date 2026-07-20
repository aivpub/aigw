//! aigw-server library — re-exports for integration tests

// ── Build-time version info (injected by build.rs) ──
/// Full version string: `0.1.0 (abc1234)` or `0.1.0 (abc1234-dirty)`
pub const VERSION_INFO: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("GIT_COMMIT_HASH"),
    env!("GIT_DIRTY"),
    ")"
);
pub const BUILD_DATE: &str = env!("BUILD_DATE");
pub const GIT_COMMIT_HASH: &str = env!("GIT_COMMIT_HASH");
pub const GIT_DESCRIBE: &str = env!("GIT_DESCRIBE");

pub mod openapi;
pub mod routes;
