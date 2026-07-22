# Migration SOP: litellm → aigw Production Migration

## Overview

This document describes the standard operating procedure for migrating from litellm proxy to aigw (AI Gateway) in a production environment.

### Tools Used

| Tool | Purpose |
|------|---------|
| `aigw-migrate remote-import` | Full litellm → aigw migration with encryption key rotation |
| `aigw-migrate remote-export` | Reverse migration (rollback): aigw → litellm |
| `aigw-migrate verify` | Row count verification between source and target |

### Encryption Key Flow

```
litellm DB                  aigw DB
  (LITELLM_MASTER_KEY)        (AIGW_MASTER_KEY)
        │                           │
        ▼                           ▼
  encrypted fields ──decrypt──► plaintext ──encrypt──► encrypted fields
  (credential_values,           (credential_values,
   litellm_params)               litellm_params)
```

---

## Phase 1: Preparation (Day Before)

### 1.1 Verify Source Database

```bash
# Check source litellm DB is accessible and has expected tables
aigw-migrate verify \
  --source-db /path/to/litellm.db \
  --target-db /path/to/empty-aigw.db
```

Expected output: all row counts for source tables displayed (target will be 0).

### 1.2 Verify Encryption Keys

```bash
# Confirm LITELLM_MASTER_KEY is correct by decrypting a known credential
sqlite3 /path/to/litellm.db \
  "SELECT credential_name, credential_values FROM LiteLLM_CredentialsTable LIMIT 1"
```

Use `aigw-core` crypto tests to verify decryption:
```bash
cargo test -p aigw-core -- crypto::tests
```

### 1.3 Prepare AIGW_MASTER_KEY

Generate or use an existing AIGW master key (32+ characters, stored securely):
```bash
export AIGW_MASTER_KEY="sk-aigw-<random-32-chars>"
```

This key will encrypt all credential and model fields in the aigw database.

### 1.4 Backup

```bash
# Backup source litellm database
cp /path/to/litellm.db /backup/litellm-$(date +%Y%m%d-%H%M%S).db

# Backup target aigw database (if exists)
cp /path/to/aigw.db /backup/aigw-$(date +%Y%m%d-%H%M%S).db
```

### 1.5 Dry Run

```bash
# Test connectivity and key extraction without writing data
# (use --source-master-key to override auto-extraction if preferred;
#  by default the upstream field-encryption key is auto-extracted
#  from LiteLLM_Config, so you rarely need to set it manually)
aigw-migrate remote-import \
  --source-url sqlite:///path/to/litellm-backup.db \
  --target-url sqlite:///tmp/aigw-dryrun.db \
  --target-master-key "$AIGW_MASTER_KEY"
```

### Checklist: Phase 1

- [ ] Source DB accessible and verified
- [ ] LITELLM_MASTER_KEY confirmed working
- [ ] AIGW_MASTER_KEY generated and saved
- [ ] Backup created for both source and target
- [ ] Dry run completed successfully
- [ ] Rollback plan reviewed (Phase 5)

---

## Phase 2: Pre-Check (1 Hour Before)

### 2.1 Stop Write Traffic to litellm

If using a load balancer, redirect write traffic away from litellm. Read-only traffic can continue.

### 2.2 Final Source Verification

```bash
# Record pre-migration row counts
sqlite3 /path/to/litellm.db <<EOF
SELECT 'LiteLLM_VerificationToken', COUNT(*) FROM LiteLLM_VerificationToken
UNION ALL SELECT 'LiteLLM_SpendLogs', COUNT(*) FROM LiteLLM_SpendLogs
UNION ALL SELECT 'LiteLLM_OrganizationTable', COUNT(*) FROM LiteLLM_OrganizationTable
UNION ALL SELECT 'LiteLLM_TeamTable', COUNT(*) FROM LiteLLM_TeamTable
UNION ALL SELECT 'LiteLLM_UserTable', COUNT(*) FROM LiteLLM_UserTable
UNION ALL SELECT 'LiteLLM_ProjectTable', COUNT(*) FROM LiteLLM_ProjectTable
UNION ALL SELECT 'LiteLLM_BudgetTable', COUNT(*) FROM LiteLLM_BudgetTable
UNION ALL SELECT 'LiteLLM_OrganizationMembership', COUNT(*) FROM LiteLLM_OrganizationMembership
UNION ALL SELECT 'LiteLLM_TeamMembership', COUNT(*) FROM LiteLLM_TeamMembership
UNION ALL SELECT 'LiteLLM_ProxyModelTable', COUNT(*) FROM LiteLLM_ProxyModelTable
UNION ALL SELECT 'LiteLLM_Config', COUNT(*) FROM LiteLLM_Config
UNION ALL SELECT 'LiteLLM_CredentialsTable', COUNT(*) FROM LiteLLM_CredentialsTable;
EOF
```

Save these counts to a file for post-migration verification.

### Checklist: Phase 2

- [ ] Write traffic stopped
- [ ] Pre-migration row counts recorded
- [ ] Team notified of upcoming maintenance window

---

## Phase 3: Execution (During Maintenance Window)

### 3.1 Run Migration

```bash
aigw-migrate remote-import \
  --source-url sqlite:///path/to/litellm.db \
  --target-url sqlite:///path/to/aigw.db \
  --target-master-key "$AIGW_MASTER_KEY"
```

Expected output:
```
Remote import: litellm (...) → aigw (...)
  Extracted master_key from LiteLLM_Config in source DB
Step 1: Source master_key extracted from LiteLLM_Config
Step 2: Importing plain tables...
  LiteLLM_OrganizationTable -> organizations (N rows)
  ...
Step 3: Importing credentials (with key rotation)...
  LiteLLM_CredentialsTable -> credentials (N rows)
Step 4: Importing proxy_models (with key rotation)...
  LiteLLM_ProxyModelTable -> proxy_models (N rows)
Step 5: Importing spend_logs (batch mode)...
Step 6: Verifying row counts...
  LiteLLM_OrganizationTable: src=N tgt=N [OK]
  ...
Remote import complete. All row counts match.
```

### 3.2 Verify Migration

```bash
aigw-migrate verify \
  --source-db /path/to/litellm.db \
  --target-db /path/to/aigw.db
```

All tables should show `[OK]` status (row counts match).

### 3.3 Start aigw Server

```bash
AIGW_MASTER_KEY="$AIGW_MASTER_KEY" \
  cargo run --bin aigw-server
```

Or in production:
```bash
export AIGW_MASTER_KEY="sk-aigw-..."
./aigw-server
```

### 3.4 Smoke Test

```bash
# Test key creation
curl -X POST http://localhost:8000/key/generate \
  -H "Authorization: Bearer sk-master" \
  -H "Content-Type: application/json" \
  -d '{"key_alias": "smoke-test", "models": ["gpt-4"]}'

# Test chat completions through aigw
curl -X POST http://localhost:8000/chat/completions \
  -H "Authorization: Bearer <generated-key>" \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}'
```

### Checklist: Phase 3

- [ ] Migration completed with all [OK]
- [ ] Verification passed
- [ ] aigw server started successfully
- [ ] Smoke test passed (key creation + chat completion)

---

## Phase 4: Monitoring (30 Minutes After Cutover)

### 4.1 Metrics to Watch

| Metric | Expected | Alert If |
|--------|----------|----------|
| HTTP 5xx rate | < 1% | > 5% |
| Chat completion latency | Same as litellm baseline | > 2x baseline |
| Error rate | < 1% | > 5% |
| Spend log writes | Active | No new logs for 5 min |

### 4.2 Spot Check

```bash
# Verify credential resolution works
curl http://localhost:8000/spend/logs \
  -H "Authorization: Bearer sk-master"

# Verify model list
curl http://localhost:8000/model/info \
  -H "Authorization: Bearer sk-master"
```

### 4.3 Restore Write Traffic

Once monitoring confirms stability, redirect write traffic to aigw.

### Checklist: Phase 4

- [ ] Error rates within acceptable range
- [ ] Latency within acceptable range
- [ ] Spend tracking operational
- [ ] Write traffic restored
- [ ] No unexpected errors in logs

---

## Phase 5: Rollback (If Needed)

### 5.1 Trigger Conditions

Rollback is required if:
- Migration verification fails (row count mismatch)
- aigw server fails to start
- Error rate exceeds 10% for > 5 minutes
- Credential/model decryption failures detected at runtime

### 5.2 Rollback Steps

```bash
# 1. Stop aigw server
kill <aigw-pid>

# 2. Restore litellm from backup (if the original DB was modified)
cp /backup/litellm-<timestamp>.db /path/to/litellm.db

# 3. Restart litellm
cd /path/to/litellm && docker-compose up -d

# 4. Redirect traffic back to litellm

# 5. Verify litellm is working
curl http://litellm:4000/health
```

### 5.3 Remote Export (aigw → litellm)

If changes were made to aigw that need to flow back to litellm:

```bash
aigw-migrate remote-export \
  --source-url sqlite:///path/to/aigw.db \
  --target-url sqlite:///path/to/litellm.db \
  --target-master-key "$LITELLM_MASTER_KEY"
```

This decrypts aigw fields with `AIGW_MASTER_KEY` and re-encrypts with `LITELLM_MASTER_KEY`.

### Checklist: Phase 5

- [ ] aigw server stopped
- [ ] litellm database restored from backup (if modified)
- [ ] litellm server restarted
- [ ] Traffic redirected back to litellm
- [ ] Health check passing

---

## Appendix A: Migration Checklist (Printable)

```
[ ] Phase 1: Preparation
  [ ] Source DB verified
  [ ] LITELLM_MASTER_KEY confirmed
  [ ] AIGW_MASTER_KEY generated
  [ ] Backup created
  [ ] Dry run completed

[ ] Phase 2: Pre-Check
  [ ] Write traffic stopped
  [ ] Row counts recorded
  [ ] Team notified

[ ] Phase 3: Execution
  [ ] Migration completed
  [ ] Verification passed
  [ ] aigw server started
  [ ] Smoke test passed

[ ] Phase 4: Monitoring
  [ ] Error rate OK
  [ ] Latency OK
  [ ] Spend tracking OK
  [ ] Write traffic restored

[ ] Phase 5: Rollback (if needed)
  [ ] aigw stopped
  [ ] litellm restored
  [ ] Traffic redirected
```

## Appendix B: Common Issues & Troubleshooting

### B.1 "Target master key required"

```
Error: Target master key required. Provide --target-master-key or set AIGW_MASTER_KEY env var.
```

**Fix:** Set the `AIGW_MASTER_KEY` environment variable or pass `--target-master-key`:

```bash
export AIGW_MASTER_KEY="sk-aigw-your-key-here"
```

### B.2 "No source master_key found"

```
Error: No source master_key found. Provide --source-master-key or ensure source DB has LiteLLM_Config
```

**Fix:** Either provide `--source-master-key` explicitly or ensure the source litellm DB has a `LiteLLM_Config` table with `param_name='litellm_master_key'`.

### B.3 Decryption failures during migration

```
[WARN] Skipped credential my-creds: decryption failed
```

The row is skipped (not migrated). This happens when the upstream **field-encryption key** doesn't match.

> **Most common cause:** the litellm **API-auth key** (the one used for
> `Authorization: Bearer`, i.e. `OPENAPI_KEY`/`OPENAI_API_KEY`) was mistakenly
> configured as the upstream encryption key. They are **frequently different
> values** in real deployments:
> - field-encryption key → `LiteLLM_Config.general_settings.master_key` (decrypts `litellm_params`/`credential_values`)
> - API-auth key → the `sk-...` used to call litellm's HTTP API
>
> **Fix:** leave `AIGW_UPSTREAM_ENCRYPT_KEY` unset and let migrate auto-extract
> the real key from `LiteLLM_Config`, or set it to the
> `general_settings.master_key` value explicitly.

### B.4 Row count mismatch after migration

Run `aigw-migrate verify` to see which tables mismatch, then re-run the affected migration step manually.

### B.5 Runtime "model not found" after migration

Check that `proxy_models` table was migrated and the model name matches exactly. Verify with:

```bash
sqlite3 /path/to/aigw.db "SELECT model_name FROM proxy_models"
```

### B.6 Runtime `Credential '...' not found` (500) after migration

Symptom: migrated `proxy_models` rows exist, but calling the model returns
`500 Credential '<base64...>' not found`. The `<base64...>` is an **encrypted**
`litellm_credential_name` — meaning the field wasn't decrypted during
resolution, because migration silently skipped key rotation (see B.3).

Root cause: wrong `AIGW_UPSTREAM_ENCRYPT_KEY` (or the env var wasn't forwarded
to the migrate subprocess). Fix the key, re-run migration, and the encrypted
`litellm_credential_name` will be rotated to the aigw master key and decrypt
correctly at runtime.
