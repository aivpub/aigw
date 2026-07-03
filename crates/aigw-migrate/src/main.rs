//! aigw-migrate: Bidirectional migration CLI between litellm and aigw databases.
//!
//! Supports 3 commands:
//!   import — copy data from litellm table names to aigw table names
//!   export — copy data from aigw table names to litellm table names
//!   verify — compare row counts between the two databases
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
mod verify;

use clap::{Parser, Subcommand};

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
];

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    }

    Ok(())
}
