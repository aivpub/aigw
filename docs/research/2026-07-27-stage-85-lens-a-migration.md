# Stage 85 Reviewer Lens A — Migration Safety + DB Schema

Review the DESIGN (not code) for aigw Stage 85. Read first:
- `docs/research/2026-07-27-stage-85-design-review-brief.md` (brief)
- `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md` (design v5)
- `docs/stages/stage-85.md` (stage)

## Your lens: migration safety + DB schema correctness across SQLite/MySQL/PostgreSQL

Verify these design claims against the actual codebase:

1. **§3.1 PG idempotency**: 022 uses `EXISTS(request_id) AND NOT EXISTS(call_id)`.
   - Read the migration runner in `crates/aigw-core` (search for how `migrations/*.sql` are applied — is there a version/schema_migrations table? does it skip already-applied migrations?).
   - Determine: if the runner applies each file once via a version table, the SQL-level idempotency is belt-and-suspenders (fine). If it re-runs files, does the double-condition actually hold for existing/fresh/re-run DBs?
   - Check `migrations/` dir layout: are there per-DB subdirs (pg/mysql/sqlite)? Confirm 022 would be added as 3 files.

2. **§3.1 MySQL**: `INFORMATION_SCHEMA + PREPARE` for all 4 phases; `ALTER TABLE ... RENAME COLUMN`.
   - Check the minimum MySQL version aigw targets (README / docker-compose / CI). `RENAME COLUMN` needs MySQL 8.0+. Read existing MySQL migrations in `migrations/` to confirm `RENAME COLUMN` is already used (or if they use `CHANGE COLUMN` instead).
   - Confirm `PREPARE`/`EXECUTE`/`DEALLOCATE` style is consistent with existing migrations.

3. **§3.1 SQLite**: no conditional RENAME; design says runner does `PRAGMA table_info` probing.
   - Does such probing exist in the migration runner? Or does SQLite just run the SQL and rely on the version table to never re-run? If the latter, flag that the design's claim "迁移器侧 PRAGMA table_info 探测" may be aspirational — and assess whether that matters given version-table semantics.

4. **§3.2**: 002/015 NOT modified. Confirm `migrations/` contains 002_spend_logs and 015_daily_spend with `request_id`.

5. **§3.3 index**: new `idx_spend_logs_request_id`. Check existing indexes on spend_logs (PK + any others) for conflict. Is the PK implicitly indexed after RENAME (yes, but confirm no orphaned index name).

6. **§4.4 parquet compat**: reader maps old `request_id` column → `call_id`. Read `crates/aigw-core/src/body_archive/query.rs` and `mod.rs` to see how parquet rows are deserialized into `BodyRow`. Is column-name-based mapping feasible, or does it use positional/struct deserialization that would break? Assess feasibility.

## Output
Report ONLY design-level defects that would cause implementation failure or contract breaks. For each: `file:line` anchor, concrete failure scenario, severity (Critical/High/Medium/Low), fix. If sound, say so with evidence. No stylistic nits.
