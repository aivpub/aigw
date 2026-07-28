//! aigw-migrate: Bidirectional migration CLI between litellm and aigw databases.
//!
//! Environment variables (all optional; CLI flags take precedence):
//!   AIGW_UPSTREAM_DB_URL — fallback for --source-url
//!   AIGW_DATABASE_URL    — fallback for --target-url
//!   AIGW_MASTER_KEY      — fallback for --target-master-key
//!
//! Table name mapping (litellm → aigw):
//!   LiteLLM_VerificationToken      → virtual_keys
//!   LiteLLM_SpendLogs              → spend_logs
//!   LiteLLM_OrganizationTable      → organizations
//!   LiteLLM_TeamTable              → teams
//!   LiteLLM_UserTable              → users
//!   LiteLLM_ProjectTable           → projects
//!   LiteLLM_BudgetTable            → budgets
//!   LiteLLM_OrganizationMembership  → organization_memberships
//!   LiteLLM_TeamMembership         → team_memberships

mod export;
mod import;
mod native;
mod pre_check;
mod remote_export;
mod remote_import;
mod sync;
mod verify;

use clap::{Parser, Subcommand};

use aigw_migrate::TABLE_MAPPINGS;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Env-resolve helpers — CLI flag > env var, with clear error if both missing
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn resolve_source_url(cli: Option<String>) -> anyhow::Result<String> {
    cli.or_else(|| std::env::var("AIGW_UPSTREAM_DB_URL").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Source URL required. Provide --source-url or set AIGW_UPSTREAM_DB_URL env var."
            )
        })
}

fn resolve_target_url(cli: Option<String>) -> anyhow::Result<String> {
    cli.or_else(|| std::env::var("AIGW_DATABASE_URL").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Target URL required. Provide --target-url or set AIGW_DATABASE_URL env var."
            )
        })
}

fn resolve_target_master_key(cli: Option<String>) -> anyhow::Result<String> {
    cli.or_else(|| std::env::var("AIGW_MASTER_KEY").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Target master key required. Provide --target-master-key or set AIGW_MASTER_KEY env var."
            )
        })
}

fn resolve_sync_source_url(cli: Option<String>) -> anyhow::Result<String> {
    cli.or_else(|| std::env::var("AIGW_SYNC_SOURCE_URL").ok())
        .or_else(|| std::env::var("AIGW_UPSTREAM_DB_URL").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Source URL required. Provide --source-url or set AIGW_SYNC_SOURCE_URL / AIGW_UPSTREAM_DB_URL env var."
            )
        })
}

fn resolve_sync_target_url(cli: Option<String>) -> anyhow::Result<String> {
    cli.or_else(|| std::env::var("AIGW_SYNC_TARGET_URL").ok())
        .or_else(|| std::env::var("AIGW_DATABASE_URL").ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Target URL required. Provide --target-url or set AIGW_SYNC_TARGET_URL / AIGW_DATABASE_URL env var."
            )
        })
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// CLI
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Parser)]
#[command(
    name = "aigw-migrate",
    about = "Bidirectional migration between litellm and aigw databases",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Import data from litellm DB into aigw DB
    Import {
        /// Path to the litellm SQLite database (source)
        #[arg(long)]
        source: String,
        /// Path to the aigw SQLite database (target)
        #[arg(long)]
        target: String,
    },
    /// Export data from aigw DB into litellm DB
    Export {
        /// Path to the aigw SQLite database (source)
        #[arg(long)]
        source: String,
        /// Path to the litellm SQLite database (target)
        #[arg(long)]
        target: String,
    },
    /// Verify row counts between litellm and aigw databases
    Verify {
        /// Path to the litellm SQLite database
        #[arg(long = "source-db")]
        source_db: String,
        /// Path to the aigw SQLite database
        #[arg(long = "target-db")]
        target_db: String,
    },
    /// Full migration: litellm → aigw with encryption key rotation
    RemoteImport {
        /// Source database URL (falls back to AIGW_UPSTREAM_DB_URL env var)
        #[arg(long)]
        source_url: Option<String>,
        /// Target database URL or file path (falls back to AIGW_DATABASE_URL env var)
        #[arg(long)]
        target_url: Option<String>,
        /// Source master key (optional; auto-extracted from LiteLLM_Config if not provided)
        #[arg(long = "source-master-key")]
        source_master_key: Option<String>,
        /// Target master key (falls back to AIGW_MASTER_KEY env var)
        #[arg(long = "target-master-key")]
        target_master_key: Option<String>,
        /// Limit spend_logs rows per batch (None = import all, ordered by startTime ASC)
        #[arg(long = "spend-log-limit")]
        spend_log_limit: Option<usize>,
        /// Resume spend_logs from this timestamp (ISO 8601, inclusive).
        /// `WHERE "startTime" >= value ORDER BY "startTime" ASC`.
        /// Same-second overlap is harmless — target writes are idempotent on request_id.
        #[arg(long = "spend-log-resume-after")]
        spend_log_resume_after: Option<String>,
        /// Stop spend_logs before this timestamp (ISO 8601, exclusive).
        /// `WHERE "startTime" < value`.
        #[arg(long = "spend-log-end-before")]
        spend_log_end_before: Option<String>,
        /// Run only a single migration step: 2=plain, 3=credentials, 4=proxy_models, 5=spend_logs
        #[arg(long = "step-filter")]
        step_filter: Option<u8>,
        /// Skip large body columns (messages, response) in spend_logs
        #[arg(long = "skip-body", default_value = "false")]
        skip_body: bool,
        /// Comma-separated table.column pairs to skip during import
        #[arg(long = "skip-columns", value_delimiter = ',')]
        skip_columns: Vec<String>,
        /// Rows per target-side INSERT transaction for spend_logs (default 10).
        /// Larger = fewer commits but bigger memory / WAL spikes.
        /// Smaller = smoother progress, tighter memory ceiling.
        #[arg(long = "batch-size", default_value_t = 10)]
        batch_size: usize,
    },
    /// Pre-migration checks: verify source/target connectivity, keys, and data
    PreCheck {
        /// Source database URL (falls back to AIGW_UPSTREAM_DB_URL env var)
        #[arg(long)]
        source_url: Option<String>,
        /// Target database URL (falls back to AIGW_DATABASE_URL env var)
        #[arg(long)]
        target_url: Option<String>,
        /// Target master key (falls back to AIGW_MASTER_KEY env var)
        #[arg(long = "target-master-key")]
        target_master_key: Option<String>,
    },
    /// Full reverse migration: aigw → litellm with encryption key rotation
    RemoteExport {
        /// Source database URL (falls back to AIGW_DATABASE_URL env var)
        #[arg(long)]
        source_url: Option<String>,
        /// Target database URL (falls back to AIGW_UPSTREAM_DB_URL env var)
        #[arg(long)]
        target_url: Option<String>,
        /// Source master key (aigw key; falls back to AIGW_MASTER_KEY env var)
        #[arg(long = "source-master-key")]
        source_master_key: Option<String>,
        /// Target master key (litellm key; auto-extracted from LiteLLM_Config if not provided)
        #[arg(long = "target-master-key")]
        target_master_key: Option<String>,
    },
    /// aigw-to-aigw multi-table read-only incremental sync (same master_key cluster).
    ///
    /// Copies data between two aigw database instances (PG/SQLite/MySQL any
    /// combination).  Same schema on both ends — no litellm table-name or
    /// column-redirect mapping.  Default: all 11 business tables; `config`
    /// excluded (holds master_key).  `spend_logs` supports `--days` / cursor
    /// incremental; other tables are full-table idempotent copies
    /// (`INSERT OR IGNORE` / `ON CONFLICT DO NOTHING`).  Encrypted tables
    /// (`credentials` / `proxy_models`) copy ciphertext verbatim — both ends
    /// must share the same master_key.  One-shot CLI, not a daemon.
    Sync {
        /// Source aigw database URL (or AIGW_SYNC_SOURCE_URL / AIGW_UPSTREAM_DB_URL)
        #[arg(long, short = 's')]
        source_url: Option<String>,
        /// Target aigw database URL (or AIGW_SYNC_TARGET_URL / AIGW_DATABASE_URL)
        #[arg(long, short = 't')]
        target_url: Option<String>,
        /// Comma-separated aigw table names; omit for all 11 business tables.
        /// `config` is known but excluded by default — pass it explicitly to
        /// sync (INSERT OR IGNORE only fills missing rows, never overwrites).
        #[arg(long, short = 'T')]
        tables: Option<String>,
        /// spend_logs only: sync rows with `start_time` within the last N days (UTC).
        /// Other tables are full-table copies and ignore this flag.
        #[arg(long, short = 'd')]
        days: Option<i64>,
        /// spend_logs only: precise lower bound `start_time >= value` (ISO 8601).
        /// Combined with --days by taking the stricter (later) bound.
        #[arg(long = "resume-after", short = 'r')]
        resume_after: Option<String>,
        /// spend_logs only: precise upper bound `start_time < value` (ISO 8601).
        /// Combined with --days by taking the stricter (earlier) bound.
        #[arg(long = "end-before", short = 'e')]
        end_before: Option<String>,
        /// Skip spend_logs body columns (messages, response, proxy_server_request).
        #[arg(long, short = 'B', default_value_t = false)]
        skip_body: bool,
        /// Rows per target-side INSERT transaction (default 10).
        #[arg(long = "batch-size", short = 'b', default_value_t = 10)]
        batch_size: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env from cwd (falls back to parent dirs) so all vars can be set
    // in a .env file instead of shell exports.
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    match cli.command {
        Commands::Import { source, target } => {
            println!("Importing: litellm ({source}) → aigw ({target})");
            import::run(&source, &target).await?;
            println!("Import complete.");
        }
        Commands::Export { source, target } => {
            println!("Exporting: aigw ({source}) → litellm ({target})");
            export::run(&source, &target).await?;
            println!("Export complete.");
        }
        Commands::Verify {
            source_db,
            target_db,
        } => {
            println!("Verifying: litellm ({source_db}) ↔ aigw ({target_db})");
            let all_match = verify::run(&source_db, &target_db).await?;
            if all_match {
                println!("All tables match.");
                std::process::exit(0);
            } else {
                eprintln!("Mismatch detected in one or more tables.");
                std::process::exit(1);
            }
        }
        Commands::PreCheck {
            source_url,
            target_url,
            target_master_key,
        } => {
            let source_url = resolve_source_url(source_url)?;
            let target_url = resolve_target_url(target_url)?;
            let target_key = resolve_target_master_key(target_master_key)?;
            let all_pass = pre_check::run(&source_url, &target_url, &target_key).await?;
            if all_pass {
                println!("All checks passed. Ready to migrate.");
                std::process::exit(0);
            } else {
                eprintln!("Some checks failed. Fix issues before migrating.");
                std::process::exit(1);
            }
        }
        Commands::RemoteImport {
            source_url,
            target_url,
            source_master_key,
            target_master_key,
            spend_log_limit,
            spend_log_resume_after,
            spend_log_end_before,
            step_filter,
            skip_body,
            skip_columns,
            batch_size,
        } => {
            let source_url = resolve_source_url(source_url)?;
            let target_url = resolve_target_url(target_url)?;
            let target_key = resolve_target_master_key(target_master_key)?;

            if skip_body {
                println!("  --skip-body: will skip messages, response, proxy_server_request columns in spend_logs");
            }
            if !skip_columns.is_empty() {
                println!("  --skip-columns: {:?}", skip_columns);
            }

            // Build skip set from --skip-body and --skip-columns
            let mut skip_columns_set: std::collections::HashSet<(String, String)> =
                std::collections::HashSet::new();
            if skip_body {
                skip_columns_set.insert(("spend_logs".to_string(), "messages".to_string()));
                skip_columns_set.insert(("spend_logs".to_string(), "response".to_string()));
                skip_columns_set.insert(("spend_logs".to_string(), "proxy_server_request".to_string()));
            }
            for spec in &skip_columns {
                if let Some((table, col)) = spec.split_once('.') {
                    skip_columns_set.insert((table.to_string(), col.trim().to_string()));
                }
            }

            let cursor = native::CursorRange {
                resume_after: spend_log_resume_after,
                end_before: spend_log_end_before,
            };

            if let Some(ref t) = cursor.resume_after {
                println!("  --spend-log-resume-after: \"{t}\"");
            }
            if let Some(ref t) = cursor.end_before {
                println!("  --spend-log-end-before: \"{t}\"");
            }
            if let Some(limit) = spend_log_limit {
                println!(
                    "Remote import: litellm ({source_url}) → aigw ({target_url}) [spend_logs limit={limit}]"
                );
            } else {
                println!(
                    "Remote import: litellm ({source_url}) → aigw ({target_url})"
                );
            }
            let all_match = remote_import::run_filtered(
                &source_url,
                &target_url,
                source_master_key.as_deref(),
                &target_key,
                spend_log_limit,
                &cursor,
                step_filter,
                skip_body,
                &skip_columns_set,
                batch_size,
            )
            .await?;
            if all_match {
                println!("Remote import complete. All row counts match.");
            } else {
                eprintln!("Remote import complete, but some row counts MISMATCH.");
                std::process::exit(1);
            }
        }
        Commands::RemoteExport {
            source_url,
            target_url,
            source_master_key,
            target_master_key,
        } => {
            let source_url = resolve_target_url(source_url)?;
            let target_url = resolve_source_url(target_url)?;
            let source_key = resolve_target_master_key(source_master_key)?;

            println!("Remote export: aigw ({source_url}) → litellm ({target_url})");
            let all_match = remote_export::run(
                &source_url,
                &target_url,
                &source_key,
                target_master_key.as_deref(),
            )
            .await?;
            if all_match {
                println!("Remote export complete. All row counts match.");
            } else {
                eprintln!("Remote export complete, but some row counts MISMATCH.");
                std::process::exit(1);
            }
        }
        Commands::Sync {
            source_url,
            target_url,
            tables,
            days,
            resume_after,
            end_before,
            skip_body,
            batch_size,
        } => {
            let source_url = resolve_sync_source_url(source_url)?;
            let target_url = resolve_sync_target_url(target_url)?;
            let tables = sync::resolve_tables(tables.as_deref())?;
            let cursor = sync::resolve_cursor(days, resume_after, end_before)?;

            if skip_body {
                println!("  --skip-body: will null messages/response/proxy_server_request in spend_logs");
            }
            if let Some(ref t) = cursor.resume_after {
                println!("  --resume-after: \"{}\" (spend_logs only)", t);
            }
            if let Some(ref t) = cursor.end_before {
                println!("  --end-before: \"{}\" (spend_logs only)", t);
            }
            if let Some(n) = days {
                println!("  --days: {} (spend_logs only, UTC)", n);
            }
            println!(
                "Sync: aigw ({}) -> aigw ({}) [{} tables{}]",
                source_url,
                target_url,
                tables.len(),
                if tables.len() == 1 && tables[0] == "config" { " (config explicit)" } else { "" }
            );
            println!("  tables: {:?}", tables);

            let stats = sync::run_sync(
                &source_url,
                &target_url,
                &tables,
                &cursor,
                skip_body,
                batch_size,
            )
            .await?;
            println!(
                "Sync complete: inserted={} ignored={}",
                stats.total_inserted(),
                stats.total_ignored()
            );
            for (tbl, s) in &stats.per_table {
                println!("  {}: inserted={} ignored={}", tbl, s.inserted, s.ignored);
            }
        }
    }

    Ok(())
}
