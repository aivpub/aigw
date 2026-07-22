#!/usr/bin/env python3
"""One-shot rename: AIGW_UPSTREAM_MASTER_KEY -> AIGW_UPSTREAM_ENCRYPT_KEY.

Also renames the Rust helper fn `upstream_master_key` -> `upstream_encrypt_key`
in migration_sync_steps.rs (rollback uses an inline var name, no fn).

Run from repo root. Idempotent: skips files already migrated.
"""
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Rust source files that reference the env var.
RUST_FILES = [
    "crates/aigw-migrate/src/bin/export_fixtures.rs",
    "crates/aigw-server/tests/bdd_steps/migration_sync_steps.rs",
    "crates/aigw-server/tests/bdd_steps/migration_rollback_steps.rs",
]

# Other config/doc files.
OTHER_FILES = [
    "Taskfile.yml",
    ".env.example",
]


def replace_in_file(rel, old, new, label):
    p = REPO / rel
    if not p.exists():
        print(f"  SKIP (missing): {rel}")
        return 0
    s = p.read_text()
    n = s.count(old)
    if n == 0:
        print(f"  SKIP (no occurrence): {rel} [{label}]")
        return 0
    s = s.replace(old, new)
    p.write_text(s)
    print(f"  {rel}: replaced {n}x [{label}]")
    return n


total = 0
print("== Rust: env var AIGW_UPSTREAM_MASTER_KEY -> AIGW_UPSTREAM_ENCRYPT_KEY ==")
for f in RUST_FILES:
    total += replace_in_file(f, "AIGW_UPSTREAM_MASTER_KEY", "AIGW_UPSTREAM_ENCRYPT_KEY", "env var")

print("== Rust: helper fn upstream_master_key -> upstream_encrypt_key (sync steps) ==")
# Only migration_sync_steps.rs defines and calls this helper.
total += replace_in_file(
    "crates/aigw-server/tests/bdd_steps/migration_sync_steps.rs",
    "upstream_master_key",
    "upstream_encrypt_key",
    "fn name",
)

print("== Config: Taskfile.yml env var ==")
total += replace_in_file("Taskfile.yml", "AIGW_UPSTREAM_MASTER_KEY", "AIGW_UPSTREAM_ENCRYPT_KEY", "taskfile")

print(f"TOTAL replacements: {total}")
