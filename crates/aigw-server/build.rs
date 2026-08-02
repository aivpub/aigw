//! Build script for aigw-server
//!
//! Captures git version information at compile time and exposes it
//! as environment variables for use via `env!()` in source code.
//!
//! Variables exposed:
//! - `GIT_COMMIT_HASH` — short commit hash (e.g. "abc1234"), or "unknown"
//! - `GIT_DIRTY` — "-dirty" if working tree has uncommitted changes, else ""
//! - `GIT_DESCRIBE` — `git describe --tags --always --dirty` output
//! - `BUILD_DATE` — UTC build date in YYYY-MM-DD format

fn run_cmd(args: &[&str]) -> Option<String> {
    std::process::Command::new(args[0])
        .args(&args[1..])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
}

fn main() {
    // ── Git commit hash (short) ──
    let git_hash =
        run_cmd(&["git", "rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=GIT_COMMIT_HASH={}", git_hash);

    // ── Git dirty status ──
    let git_dirty = std::process::Command::new("git")
        .args(["diff", "--quiet"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(false);
    let dirty_suffix = if git_dirty { "-dirty" } else { "" };
    println!("cargo:rustc-env=GIT_DIRTY={}", dirty_suffix);

    // ── Git describe (tag-based version) ──
    let git_describe = run_cmd(&["git", "describe", "--tags", "--always", "--dirty"])
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));
    println!("cargo:rustc-env=GIT_DESCRIBE={}", git_describe);

    // ── Build date (UTC) ──
    let build_date = run_cmd(&["date", "-u", "+%Y-%m-%d"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_DATE={}", build_date);

    // Re-run build.rs when git HEAD or index changes.
    // Paths are relative to CARGO_MANIFEST_DIR (crates/aigw-server/),
    // so we use ../ to reach the workspace-root .git directory.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
}
