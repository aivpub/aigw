//! TD-020 regression — a MySQL source must round-trip integer columns intact.
//!
//! `try_mysql_get` used to probe `bool` before `i64`. MySQL's `bool` is
//! compatible with every signed integer width and decodes as
//! `<i8>::decode() != 0`, so any value in `1..=127` came back as `Bool(true)`
//! and the migration wrote `1`; negatives lost their sign the same way. Only
//! 0 (coincidentally) and >= 128 (i8 overflow -> fell through to i64) survived.
//!
//! Requires a live MySQL. Started by `task test-db-up`; run via
//! `task test-migrate-mysql`. Ignored by default so `task test` stays offline.

use sqlx::MySqlPool;

const URL_ENV: &str = "AIGW_MIGRATE_MYSQL_URL";

async fn conn() -> MySqlPool {
    let url = std::env::var(URL_ENV)
        .unwrap_or_else(|_| panic!("{URL_ENV} must be set (use `task test-migrate-mysql`)"));
    MySqlPool::connect(&url).await.expect("connect to MySQL")
}

/// Every integer that used to be mangled, plus the two that were not, so the
/// test also pins the boundary rather than only the happy cases.
const INT_VALUES: &[(i64, &str)] = &[
    (0, "zero — the one value that happened to survive"),
    (1, "one — indistinguishable from true"),
    (42, "the realistic token/timing value that became 1"),
    (127, "last value i8 decodes without overflow"),
    (
        128,
        "first value that overflowed i8 and fell through correctly",
    ),
    (9000, "a duration in ms that became 1"),
    (-5, "negative — sign used to be lost"),
    (-1, "-1 must not become 1"),
];

#[tokio::test]
#[ignore = "needs MySQL; run via `task test-migrate-mysql`"]
async fn mysql_integer_columns_round_trip_intact() {
    let pool = conn().await;
    let _ = sqlx::query("DROP TABLE IF EXISTS td020_int_roundtrip")
        .execute(&pool)
        .await;

    // Name columns by index, not by value: `1` and `-1` collide otherwise.
    let col_name = |i: usize| format!("c{i}");
    let cols: Vec<String> = (0..INT_VALUES.len()).map(col_name).collect();
    // INT and BIGINT both, so the test does not depend on which width the
    // source happens to use.
    let ddl = format!(
        "CREATE TABLE td020_int_roundtrip (id INT PRIMARY KEY, int_col INT, bigint_col BIGINT, \
         {int_cols}, {bigint_cols})",
        int_cols = cols
            .iter()
            .map(|c| format!("{c}_int INT"))
            .collect::<Vec<_>>()
            .join(", "),
        bigint_cols = cols
            .iter()
            .map(|c| format!("{c}_big BIGINT"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    sqlx::query(&ddl)
        .execute(&pool)
        .await
        .expect("create table");

    let values: Vec<String> = INT_VALUES.iter().map(|(v, _)| v.to_string()).collect();
    let insert = format!(
        "INSERT INTO td020_int_roundtrip (id, int_col, bigint_col, {int_cols}, {bigint_cols}) \
         VALUES (1, 42, 42, {v}, {v})",
        int_cols = cols
            .iter()
            .map(|c| format!("{c}_int"))
            .collect::<Vec<_>>()
            .join(", "),
        bigint_cols = cols
            .iter()
            .map(|c| format!("{c}_big"))
            .collect::<Vec<_>>()
            .join(", "),
        v = values.join(", "),
    );
    sqlx::query(&insert).execute(&pool).await.expect("insert");

    let source = aigw_migrate::native::SourcePool::connect(&std::env::var(URL_ENV).expect("url"))
        .await
        .expect("source pool");
    let rows = source
        .read_rows_sql("SELECT * FROM td020_int_roundtrip WHERE id = 1")
        .await
        .expect("read row");
    assert_eq!(rows.len(), 1, "expected exactly one row back");

    let read = |col: &str| -> i64 {
        rows[0]
            .iter()
            .find(|(name, _)| name == col)
            .unwrap_or_else(|| panic!("column {col} missing from {rows:?}"))
            .1
            .as_i64()
            .unwrap_or_else(|| {
                panic!(
                    "column {col} did not decode as an integer: {:?}",
                    rows[0].iter().find(|(n, _)| n == col).unwrap().1
                )
            })
    };

    assert_eq!(read("int_col"), 42, "plain INT column");
    assert_eq!(read("bigint_col"), 42, "plain BIGINT column");

    for (i, (v, why)) in INT_VALUES.iter().enumerate() {
        let col = col_name(i);
        assert_eq!(read(&format!("{col}_int")), *v, "INT {v}: {why}");
        assert_eq!(read(&format!("{col}_big")), *v, "BIGINT {v}: {why}");
    }

    let _ = sqlx::query("DROP TABLE IF EXISTS td020_int_roundtrip")
        .execute(&pool)
        .await;
}

/// A `BOOLEAN` column must still read as a bool, not an integer.
#[tokio::test]
#[ignore = "needs MySQL; run via `task test-migrate-mysql`"]
async fn mysql_boolean_column_still_reads_as_bool() {
    let pool = conn().await;
    let _ = sqlx::query("DROP TABLE IF EXISTS td020_bool_roundtrip")
        .execute(&pool)
        .await;
    sqlx::query(
        "CREATE TABLE td020_bool_roundtrip (id INT PRIMARY KEY, flag BOOLEAN, \
         plain_tiny TINYINT, bits BIT(1))",
    )
    .execute(&pool)
    .await
    .expect("create table");
    sqlx::query("INSERT INTO td020_bool_roundtrip VALUES (1, 1, 1, b'1')")
        .execute(&pool)
        .await
        .expect("insert");

    let source = aigw_migrate::native::SourcePool::connect(&std::env::var(URL_ENV).expect("url"))
        .await
        .expect("source pool");
    let rows = source
        .read_rows_sql("SELECT * FROM td020_bool_roundtrip WHERE id = 1")
        .await
        .expect("read row");

    let get = |col: &str| rows[0].iter().find(|(n, _)| n == col).unwrap().1.clone();

    // BOOLEAN is declared `TINYINT(1)`; it is the flag, so it stays a bool.
    assert!(
        matches!(get("flag"), serde_json::Value::Bool(true)),
        "BOOLEAN must decode as bool, got {:?}",
        get("flag")
    );
    // A plain TINYINT is a number, not a flag — this is the distinction the
    // old trial-order probe could not make.
    assert_eq!(get("plain_tiny").as_i64(), Some(1), "TINYINT is an integer");
    assert!(
        matches!(get("bits"), serde_json::Value::Bool(true)),
        "BIT(1) must decode as bool, got {:?}",
        get("bits")
    );

    let _ = sqlx::query("DROP TABLE IF EXISTS td020_bool_roundtrip")
        .execute(&pool)
        .await;
}
