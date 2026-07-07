# Stage 17 Completion Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Complete Stage 17 remaining 20% — `aigw-migrate pre-check` command + `scripts/rollback.sh` + SOP doc update.

**Architecture:** `pre-check` is a new subcommand in `aigw-migrate` that runs 6 automated checks against source/target DBs. `rollback.sh` is a standalone shell script wrapping `remote-export` + service start/stop.

**Tech Stack:** Rust + sqlx + tokio (pre-check), Bash (rollback script)

---

### Task 1: Add `pre-check` module and subcommand

**Files:**
- Create: `crates/aigw-migrate/src/pre_check.rs`
- Modify: `crates/aigw-migrate/src/lib.rs` — register `pub mod pre_check`
- Modify: `crates/aigw-migrate/src/main.rs` — add `PreCheck` variant and handler

**Step 1: Create `pre_check.rs` with stub function**

```rust
// crates/aigw-migrate/src/pre_check.rs
use sqlx::any::AnyPoolOptions;
use sqlx::{AnyPool, Row};

fn is_pg(url: &str) -> bool {
    url.starts_with("postgres://") || url.starts_with("postgresql://")
}

fn is_mysql(url: &str) -> bool {
    url.starts_with("mysql://") || url.starts_with("mariadb://")
}

fn quote_table(name: &str, db_url: &str) -> String {
    if is_mysql(db_url) {
        format!("`{}`", name)
    } else {
        format!("\"{}\"", name)
    }
}

async fn connect(url: &str) -> anyhow::Result<AnyPool> {
    let pool = AnyPoolOptions::new().max_connections(1).connect(url).await?;
    Ok(pool)
}

pub async fn run(source_url: &str, target_url: &str, target_master_key: &str) -> anyhow::Result<bool> {
    sqlx::any::install_default_drivers();

    let mut passed = 0u32;
    let total = 6u32;

    // Check 1: Source DB connectivity + tables exist
    // Check 2: Source core tables have data
    // Check 3: Target DB connectivity
    // Check 4: Source master_key extractable
    // Check 5: Target master key valid (len >= 32)
    // Check 6: Encryption/decryption spot check

    let all_pass = passed == total;
    println!("{}/{} checks passed", passed, total);
    Ok(all_pass)
}
```

**Step 2: Register module in `lib.rs`**

In `crates/aigw-migrate/src/lib.rs`, add:

```
pub mod pre_check;
```

**Step 3: Add `PreCheck` variant to `main.rs`**

Add to the `Commands` enum:

```rust
/// Pre-migration checks: verify source/target connectivity, keys, and data
PreCheck {
    /// Source database URL (litellm DB)
    #[arg(long)]
    source_url: String,
    /// Target database URL (aigw DB)
    #[arg(long)]
    target_url: String,
    /// Target master key (falls back to AIGW_MASTER_KEY env var)
    #[arg(long = "target-master-key")]
    target_master_key: Option<String>,
},
```

Add to the match block:

```rust
Commands::PreCheck {
    source_url,
    target_url,
    target_master_key,
} => {
    let target_key = target_master_key
        .or_else(|| std::env::var("AIGW_MASTER_KEY").ok())
        .ok_or_else(|| {
            anyhow::anyhow!("Target master key required. Provide --target-master-key or set AIGW_MASTER_KEY env var.")
        })?;
    let all_pass = pre_check::run(&source_url, &target_url, &target_key).await?;
    if all_pass {
        println!("All checks passed. Ready to migrate.");
        std::process::exit(0);
    } else {
        eprintln!("Some checks failed. Fix issues before migrating.");
        std::process::exit(1);
    }
}
```

**Step 4: Build to check compilation**

```bash
cargo check -p aigw-migrate
```

**Step 5: Commit**

```bash
git add crates/aigw-migrate/src/pre_check.rs crates/aigw-migrate/src/lib.rs crates/aigw-migrate/src/main.rs
git commit -m "feat(aigw-migrate): add pre-check subcommand skeleton"
```

---

### Task 2: Implement Check 1 — Source DB connectivity + tables exist

**Files:**
- Modify: `crates/aigw-migrate/src/pre_check.rs`

**Step 1: Update `run()` to add Check 1**

```rust
// Check 1: Source DB connectivity + tables exist
print!("[ 1/6] Source DB connectivity... ");
let source = match connect(source_url).await {
    Ok(p) => {
        println!("[PASS] connected");
        p
    }
    Err(e) => {
        println!("[FAIL] {}", e);
        println!("{}/{} checks passed", passed, total);
        return Ok(false);
    }
};

print!("       Source tables... ");
let required_tables = [
    "LiteLLM_VerificationToken",
    "LiteLLM_SpendLogs",
    "LiteLLM_OrganizationTable",
    "LiteLLM_TeamTable",
    "LiteLLM_UserTable",
    "LiteLLM_ProjectTable",
    "LiteLLM_BudgetTable",
    "LiteLLM_OrganizationMembership",
    "LiteLLM_TeamMembership",
    "LiteLLM_ProxyModelTable",
    "LiteLLM_Config",
    "LiteLLM_CredentialsTable",
];

let mut missing = Vec::new();
for table in &required_tables {
    let quoted = quote_table(table, source_url);
    let result = sqlx::query(&format!("SELECT 1 FROM {} LIMIT 0", quoted))
        .fetch_optional(&source)
        .await;
    if result.is_err() {
        missing.push(*table);
    }
}
if missing.is_empty() {
    println!("[PASS] all 12 tables present");
    passed += 1;
} else {
    println!("[FAIL] missing: {:?}", missing);
}
```

**Step 2: Build check**

```bash
cargo check -p aigw-migrate
```

**Step 3: Commit**

```bash
git add crates/aigw-migrate/src/pre_check.rs
git commit -m "feat(aigw-migrate): pre-check source DB connectivity + tables"
```

---

### Task 3: Implement Check 2 — Source core tables have data

**Files:**
- Modify: `crates/aigw-migrate/src/pre_check.rs`

**Step 1: Update `run()` to add Check 2**

```rust
// Check 2: Source core tables have data
print!("[ 2/6] Source tables have data... ");
let core_tables = [
    "LiteLLM_VerificationToken",
    "LiteLLM_SpendLogs",
    "LiteLLM_OrganizationTable",
    "LiteLLM_ProxyModelTable",
    "LiteLLM_CredentialsTable",
];
let mut empty_tables = Vec::new();
for table in &core_tables {
    let quoted = quote_table(table, source_url);
    let count: i64 = sqlx::query(&format!("SELECT COUNT(*) FROM {}", quoted))
        .fetch_one(&source)
        .await
        .map(|row| row.get(0))
        .unwrap_or(0);
    if count == 0 {
        empty_tables.push(*table);
    }
}
if empty_tables.is_empty() {
    println!("[PASS]");
    passed += 1;
} else {
    println!("[FAIL] 0 rows: {:?}", empty_tables);
}
```

**Step 2: Build check**

```bash
cargo check -p aigw-migrate
```

**Step 3: Commit**

```bash
git add crates/aigw-migrate/src/pre_check.rs
git commit -m "feat(aigw-migrate): pre-check source core table row counts"
```

---

### Task 4: Implement Check 3 — Target DB connectivity

**Files:**
- Modify: `crates/aigw-migrate/src/pre_check.rs`

**Step 1: Update `run()` to add Check 3**

```rust
// Check 3: Target DB connectivity
print!("[ 3/6] Target DB connectivity... ");
match connect(target_url).await {
    Ok(p) => {
        println!("[PASS] connected");
        passed += 1;
        p.close().await;
    }
    Err(e) => {
        println!("[FAIL] {}", e);
        println!("{}/{} checks passed", passed, total);
        return Ok(false);
    }
}
```

**Step 2: Build check**

```bash
cargo check -p aigw-migrate
```

**Step 3: Commit**

```bash
git add crates/aigw-migrate/src/pre_check.rs
git commit -m "feat(aigw-migrate): pre-check target DB connectivity"
```

---

### Task 5: Implement Check 4 — Source master_key extractable

**Files:**
- Modify: `crates/aigw-migrate/src/pre_check.rs`

**Step 1: Update `run()` to add Check 4**

```rust
// Check 4: Source master_key extractable
print!("[ 4/6] Source master_key... ");
let col = if is_pg(source_url) { "param_value::text" } else { "param_value" };
let master_key_row = sqlx::query(&format!(
    "SELECT {} FROM {} WHERE param_name = 'litellm_master_key'",
    col,
    quote_table("LiteLLM_Config", source_url)
))
.fetch_optional(&source)
.await;

match master_key_row {
    Ok(Some(row)) => {
        let key: String = row.try_get::<String, _>(0).unwrap_or_default();
        if key.is_empty() {
            println!("[FAIL] master_key is empty");
        } else {
            println!("[PASS] found ({} chars)", key.len());
            passed += 1;
        }
    }
    Ok(None) => println!("[FAIL] param_name='litellm_master_key' not found in LiteLLM_Config"),
    Err(e) => println!("[FAIL] {}", e),
}
```

**Step 2: Build check**

```bash
cargo check -p aigw-migrate
```

**Step 3: Commit**

```bash
git add crates/aigw-migrate/src/pre_check.rs
git commit -m "feat(aigw-migrate): pre-check source master_key extraction"
```

---

### Task 6: Implement Check 5 — Target master key valid

**Files:**
- Modify: `crates/aigw-migrate/src/pre_check.rs`

**Step 1: Update `run()` to add Check 5**

```rust
// Check 5: Target master key valid
print!("[ 5/6] Target master key... ");
if target_master_key.len() >= 32 {
    println!("[PASS] {} chars", target_master_key.len());
    passed += 1;
} else {
    println!("[FAIL] too short: {} chars (need >= 32)", target_master_key.len());
}
```

**Step 2: Build check**

```bash
cargo check -p aigw-migrate
```

**Step 3: Commit**

```bash
git add crates/aigw-migrate/src/pre_check.rs
git commit -m "feat(aigw-migrate): pre-check target master key validity"
```

---

### Task 7: Implement Check 6 — Encryption/decryption spot check

**Files:**
- Modify: `crates/aigw-migrate/src/pre_check.rs`

**Step 1: Update `run()` to add Check 6**

Need to decrypt the first credential from source DB using the extracted master_key.

```rust
// Check 6: Encryption/decryption spot check
print!("[ 6/6] Decryption spot check... ");

// Re-extract master_key (needed for decryption test)
let col2 = if is_pg(source_url) { "param_value::text" } else { "param_value" };
let source_key: Option<String> = sqlx::query(&format!(
    "SELECT {} FROM {} WHERE param_name = 'litellm_master_key'",
    col2,
    quote_table("LiteLLM_Config", source_url)
))
.fetch_optional(&source)
.await
.ok()
.flatten()
.and_then(|r| r.try_get::<String, _>(0).ok());

match source_key {
    Some(key) => {
        let cred_row = sqlx::query(&format!(
            "SELECT credential_name, credential_values FROM {} LIMIT 1",
            quote_table("LiteLLM_CredentialsTable", source_url)
        ))
        .fetch_optional(&source)
        .await;

        match cred_row {
            Ok(Some(row)) => {
                let name: String = row.get(0);
                let encrypted: String = row.try_get(1).unwrap_or_default();
                match aigw_core::crypto::decrypt_litellm_value(&encrypted, &key) {
                    Ok(_) => {
                        println!("[PASS] decrypted '{}'", name);
                        passed += 1;
                    }
                    Err(e) => println!("[FAIL] cannot decrypt '{}': {}", name, e),
                }
            }
            Ok(None) => {
                println!("[PASS] no credentials to check (skipped)");
                passed += 1;
            }
            Err(e) => println!("[FAIL] {}", e),
        }
    }
    None => println!("[FAIL] no source master_key available"),
}
```

**Step 2: Add `aigw-core` dependency to `aigw-migrate` Cargo.toml (if not already present)**

Check `crates/aigw-migrate/Cargo.toml`. If `aigw-core` is not there, add:
```toml
aigw-core = { path = "../aigw-core" }
```

**Step 3: Build check**

```bash
cargo check -p aigw-migrate
```

**Step 4: Commit**

```bash
git add crates/aigw-migrate/src/pre_check.rs crates/aigw-migrate/Cargo.toml
git commit -m "feat(aigw-migrate): pre-check decryption spot check"
```

---

### Task 8: Add pre-check tests

**Files:**
- Modify: `crates/aigw-migrate/src/pre_check.rs`

**Step 1: Add test with in-memory SQLite that creates all required tables**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pre_check_all_pass() {
        sqlx::any::install_default_drivers();
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.db");
        let tgt_path = dir.path().join("tgt.db");
        let src_url = format!("sqlite://{}", src_path.display());
        let tgt_url = format!("sqlite://{}", tgt_path.display());

        // Create source DB with required tables
        {
            let pool = connect(&src_url).await.unwrap();
            for table in &[
                "LiteLLM_VerificationToken",
                "LiteLLM_SpendLogs",
                "LiteLLM_OrganizationTable",
                "LiteLLM_TeamTable",
                "LiteLLM_UserTable",
                "LiteLLM_ProjectTable",
                "LiteLLM_BudgetTable",
                "LiteLLM_OrganizationMembership",
                "LiteLLM_TeamMembership",
                "LiteLLM_ProxyModelTable",
                "LiteLLM_Config",
                "LiteLLM_CredentialsTable",
            ] {
                sqlx::query(&format!(
                    "CREATE TABLE IF NOT EXISTS \"{}\" (id INTEGER PRIMARY KEY, credential_name TEXT, credential_values TEXT, param_name TEXT, param_value TEXT)",
                    table
                ))
                .execute(&pool)
                .await
                .unwrap();
                // Insert a row for row-count check tables
                sqlx::query(&format!(
                    "INSERT INTO \"{}\" (id) VALUES (1)",
                    table
                ))
                .execute(&pool)
                .await
                .unwrap();
            }
            // Insert master_key in LiteLLM_Config
            sqlx::query("INSERT INTO \"LiteLLM_Config\" (param_name, param_value) VALUES ('litellm_master_key', 'sk-test-source-key-for-precheck-32chars')")
                .execute(&pool)
                .await
                .unwrap();
            pool.close().await;
        }

        // Create target DB (empty is fine for connectivity check)
        {
            let pool = connect(&tgt_url).await.unwrap();
            pool.close().await;
        }

        let result = run(&src_url, &tgt_url, "sk-aigw-target-key-for-precheck-32chars+").await.unwrap();
        assert!(result, "All pre-checks should pass");
    }
}
```

**Step 2: Run test**

```bash
cargo test -p aigw-migrate -- pre_check::tests::test_pre_check_all_pass
```

Expected: PASS

**Step 3: Commit**

```bash
git add crates/aigw-migrate/src/pre_check.rs
git commit -m "test(aigw-migrate): pre-check all-pass test with SQLite"
```

---

### Task 9: Create `scripts/rollback.sh`

**Files:**
- Create: `scripts/rollback.sh`

**Step 1: Write rollback script**

```bash
#!/bin/bash
set -euo pipefail

# Usage message
usage() {
    cat <<EOF
Usage: rollback.sh --aigw-url <URL> --litellm-url <URL> [OPTIONS]

Rollback aigw → litellm: export aigw data back to litellm, stop aigw, start litellm.

Required:
  --aigw-url <URL>         Source aigw database URL
  --litellm-url <URL>      Target litellm database URL
  --aigw-master-key <KEY>  AIGW_MASTER_KEY for decryption

Optional:
  --litellm-master-key <KEY>  LITELLM_MASTER_KEY (auto-extracted if omitted)
  --stop-cmd <CMD>            Command to stop aigw (default: "kill \$(pgrep aigw-server)")
  --start-cmd <CMD>           Command to start litellm (default: "docker-compose up -d")
  --health-url <URL>          litellm health check URL (default: "http://localhost:4000/health")
  --dry-run                   Print commands without executing

Example:
  rollback.sh \\
    --aigw-url "postgres://user:pass@host/aigw" \\
    --litellm-url "postgres://user:pass@host/litellm" \\
    --aigw-master-key "sk-aigw-xxx"
EOF
    exit 1
}

# Defaults
STOP_CMD="kill \$(pgrep aigw-server 2>/dev/null) 2>/dev/null || true"
START_CMD="docker-compose up -d"
HEALTH_URL="http://localhost:4000/health"
DRY_RUN=false
AIGW_MASTER_KEY=""
LITELLM_MASTER_KEY=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --aigw-url) AIGW_URL="$2"; shift 2 ;;
        --litellm-url) LITELLM_URL="$2"; shift 2 ;;
        --aigw-master-key) AIGW_MASTER_KEY="$2"; shift 2 ;;
        --litellm-master-key) LITELLM_MASTER_KEY="$2"; shift 2 ;;
        --stop-cmd) STOP_CMD="$2"; shift 2 ;;
        --start-cmd) START_CMD="$2"; shift 2 ;;
        --health-url) HEALTH_URL="$2"; shift 2 ;;
        --dry-run) DRY_RUN=true; shift ;;
        *) echo "Unknown option: $1"; usage ;;
    esac
done

# Validate required args
if [[ -z "${AIGW_URL:-}" ]] || [[ -z "${LITELLM_URL:-}" ]] || [[ -z "$AIGW_MASTER_KEY" ]]; then
    echo "ERROR: --aigw-url, --litellm-url, and --aigw-master-key are required"
    usage
fi

echo "=== aigw Rollback: aigw → litellm ==="
echo ""

# Step 1: Stop aigw
echo "Step 1: Stopping aigw..."
if $DRY_RUN; then
    echo "  [DRY RUN] $STOP_CMD"
else
    eval "$STOP_CMD"
    echo "  aigw stopped."
fi

# Step 2: Export aigw → litellm
echo "Step 2: Exporting aigw → litellm..."

EXPORT_CMD="aigw-migrate remote-export \
  --source-url \"$AIGW_URL\" \
  --target-url \"$LITELLM_URL\" \
  --source-master-key \"$AIGW_MASTER_KEY\""

if [[ -n "$LITELLM_MASTER_KEY" ]]; then
    EXPORT_CMD="$EXPORT_CMD --target-master-key \"$LITELLM_MASTER_KEY\""
fi

if $DRY_RUN; then
    echo "  [DRY RUN] $EXPORT_CMD"
else
    eval "$EXPORT_CMD" || {
        echo "ERROR: Export failed!"
        exit 1
    }
    echo "  Export complete."
fi

# Step 3: Start litellm
echo "Step 3: Starting litellm..."
if $DRY_RUN; then
    echo "  [DRY RUN] $START_CMD"
else
    eval "$START_CMD"
    echo "  litellm started."
fi

# Step 4: Health check
echo "Step 4: Health check litellm..."
if $DRY_RUN; then
    echo "  [DRY RUN] curl -sf $HEALTH_URL"
else
    sleep 3
    if curl -sf "$HEALTH_URL" > /dev/null 2>&1; then
        echo "  litellm health check PASSED."
    else
        echo "  WARNING: litellm health check FAILED. Check logs."
        exit 1
    fi
fi

echo ""
echo "=== Rollback complete ==="
```

**Step 2: Make executable**

```bash
chmod +x scripts/rollback.sh
```

**Step 3: Commit**

```bash
git add scripts/rollback.sh
git commit -m "feat: add rollback.sh script for aigw → litellm emergency rollback"
```

---

### Task 10: Update `docs/migration-sop.md` with pre-check + rollback references

**Files:**
- Modify: `docs/migration-sop.md`

**Step 1: Update Phase 1 (Preparation) sections to reference `pre-check`**

In Phase 1.1 "Verify Source Database", replace the manual `aigw-migrate verify` and `sqlite3` commands with:

```markdown
### 1.1 Run Pre-Check

```bash
aigw-migrate pre-check \
  --source-url postgres://user:pass@host/litellm \
  --target-url postgres://user:pass@host/aigw \
  --target-master-key "$AIGW_MASTER_KEY"
```

This automated check verifies:
- Source DB connectivity and all 12 required tables
- Row counts in core tables (VerificationToken, SpendLogs, etc.)
- Target DB connectivity
- LITELLM_MASTER_KEY extractable from LiteLLM_Config
- AIGW_MASTER_KEY valid (>= 32 chars)
- Encryption/decryption spot check on first credential

All 6 checks must show `[PASS]` before proceeding.
```

**Step 2: Update Phase 5 (Rollback) to reference `scripts/rollback.sh`**

Replace manual Step 5.2 rollback steps with:

```markdown
### 5.2 Automated Rollback

Use `scripts/rollback.sh`:

```bash
scripts/rollback.sh \
  --aigw-url "postgres://user:pass@host/aigw" \
  --litellm-url "postgres://user:pass@host/litellm" \
  --aigw-master-key "$AIGW_MASTER_KEY"
```

This automatically: stops aigw → exports aigw→litellm → starts litellm → health checks.
```

**Step 3: Commit**

```bash
git add docs/migration-sop.md
git commit -m "docs: update migration SOP with pre-check + rollback.sh references"
```

---

### Task 11: Update Stage 17 status to complete

**Files:**
- Modify: `docs/stages/stage-17.md`
- Modify: `docs/stages/stage-roadmap.md`
- Modify: `docs/11-next-steps.md`

**Step 1: Mark Stage 17 as complete in stage-17.md**

Change status line from `🔄 进行中（~80%）` to `✅ 完成` and add completion date.

**Step 2: Update stage-roadmap.md**

Change Stage 17 status to `✅ 完成` with completion date 2026-07-08. Add Phase 8 and Phase 9 stages referencing the design doc.

**Step 3: Update 11-next-steps.md**

Replace "立即行动" items with Phase 8/9 first steps.

**Step 4: Commit**

```bash
git add docs/stages/stage-17.md docs/stages/stage-roadmap.md docs/11-next-steps.md
git commit -m "docs: mark Stage 17 complete, add Phase 8-9 to roadmap"
```

---

### Task 12: Final integration test — pre-check against real PG

**Files:**
- No new files

**Step 1: Run pre-check against the BDD test PG (if AIGW_UPSTREAM_DB_URL is set)**

```bash
AIGW_MASTER_KEY="sk-test-aigw-master-key-for-testing-32chars+" \
  cargo run --bin aigw-migrate pre-check \
    --source-url "$AIGW_UPSTREAM_DB_URL" \
    --target-url "$AIGW_TEST_DB_URL"
```

Expected: All 6 checks pass, or clear error messages for any that fail.

**Step 2: Run all existing tests to verify no regressions**

```bash
cargo check
cargo test
```

Expected: All pass.

**Step 3: Commit any final fixes**

```bash
git add -u
git commit -m "chore: final Stage 17 integration testing fixes"
```
