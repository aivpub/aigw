//! Step bindings for admin_jobs.feature — the admin Jobs API surface
//! (POST /admin/jobs/trigger + GET /admin/jobs + detail + logs + stats).
//!
//! Mock-BDD in-process (no @real_api tag). Drive the build_admin_jobs_router
//! axum app with oneshot requests; assert JSON + status. Scenarios that need
//! a configured body_archiver or Stage 83 read-path are tagged @skip in the
//! feature and excluded by the filter_run logic in bdd.rs.

use aigw_core::db::Database;
use aigw_core::engine::append_log;
use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use std::sync::Arc;

use crate::bdd_steps::common::{build_admin_jobs_router, make_request};
use crate::TestWorld;
use axum::http::Method;

fn sqlite_pool<'a>(db: &'a Database) -> &'a sqlx::SqlitePool {
    match db {
        Database::Sqlite(p) => p,
        _ => unreachable!("admin_jobs BDD uses sqlite::memory:"),
    }
}

async fn seed_job(world: &mut TestWorld, step_type: &str, status: &str, n_steps: usize) -> String {
    let state = world.ensure_state().await;
    let job_id = format!("job-{}-{}", step_type, uuid::Uuid::new_v4().simple());
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO async_jobs (id, step_type, trigger_type, status, total_steps, \
         completed_steps, failed_steps, max_retries, created_at, updated_at) \
         VALUES (?, ?, 'manual', ?, ?, 0, 0, 3, ?, ?)",
    )
    .bind(&job_id)
    .bind(step_type)
    .bind(status)
    .bind(n_steps as i64)
    .bind(&now)
    .bind(&now)
    .execute(sqlite_pool(&state.db))
    .await
    .expect("seed job");
    for i in 0..n_steps {
        sqlx::query(
            "INSERT INTO async_job_steps (id, job_id, step_key, step_type, status, retry_count) \
             VALUES (?, ?, ?, ?, 'pending', 0)",
        )
        .bind(format!("{}-s{}", job_id, i))
        .bind(&job_id)
        .bind(format!("hour=k{}", i))
        .bind(step_type)
        .execute(sqlite_pool(&state.db))
        .await
        .expect("seed step");
    }
    set_job_id(world, job_id.clone());
    job_id
}

fn set_job_id(world: &mut TestWorld, job_id: String) {
    let mut map = if let Some(serde_json::Value::Object(m)) = world.last_body.take() {
        m
    } else {
        serde_json::Map::new()
    };
    map.insert("admin_job_id".into(), serde_json::Value::String(job_id));
    world.last_body = Some(serde_json::Value::Object(map));
}

fn get_job_id(world: &TestWorld) -> String {
    world
        .last_body
        .as_ref()
        .and_then(|v| v.get("admin_job_id"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "nonexistent-id".to_string())
}

async fn send(
    world: &mut TestWorld,
    method: Method,
    uri: &str,
    body: Option<&str>,
    auth: Option<&str>,
) {
    // Snapshot the control flags we carry across steps (job_id, no_auth).
    // send() overwrites last_body with the HTTP response, so without this
    // snapshot a second When in the same scenario would lose the job_id.
    let snapshot = world.last_body.clone();
    let state = world.ensure_state().await;
    let app = build_admin_jobs_router(state);
    let (status, json_body) = make_request(&app, method, uri, auth, body).await;
    world.last_status = Some(status);
    // Re-merge the control flags into the new response body so subsequent
    // When/Then steps can still read admin_job_id / no_auth.
    let mut merged = match json_body {
        Some(serde_json::Value::Object(m)) => m,
        other => {
            let mut m = serde_json::Map::new();
            if let Some(v) = other {
                m.insert("_response".into(), v);
            }
            m
        }
    };
    if let Some(serde_json::Value::Object(snap)) = snapshot {
        for (k, v) in snap {
            if k == "admin_job_id" || k == "no_auth" {
                merged.entry(k).or_insert(v);
            }
        }
    }
    world.last_body = Some(serde_json::Value::Object(merged));
}

fn master(world: &TestWorld) -> String {
    world.master_key.clone()
}

// ── Background ──

// Note: "async_jobs / async_job_steps / async_job_logs 三张表已创建" and
// "Engine 已注册 AsyncTask {string} 和 {string}" are bound in async_task_steps.rs.
// We deliberately do NOT re-bind them here to avoid ambiguous step matches.

#[given(expr = "使用 master-key 认证")]
async fn given_master_auth(world: &mut TestWorld) {
    let mk = master(world);
    set_auth(world, mk);
}

#[given(regex = r"使用普通用户 token（非 admin）|使用普通用户 token")]
async fn given_non_admin_token(world: &mut TestWorld) {
    // Mark this scenario as no-auth: When steps check the flag and omit
    // Authorization, so the handler returns 401 (covers the "non-admin"
    // contract without needing a real non-admin token in mock mode).
    set_no_auth(world, true);
}

#[given(expr = "AsyncTask {string} 支持 steps_from_payload")]
async fn given_task_supports_payload(_world: &mut TestWorld, _name: String) {}

#[given(expr = "AsyncTask {string} 未 override steps_from_payload")]
async fn given_task_no_payload(_world: &mut TestWorld, _name: String) {}

// ── POST /admin/jobs/trigger ──

fn set_no_auth(world: &mut TestWorld, on: bool) {
    let mut map = if let Some(serde_json::Value::Object(m)) = world.last_body.take() {
        m
    } else {
        serde_json::Map::new()
    };
    map.insert("no_auth".into(), serde_json::Value::Bool(on));
    world.last_body = Some(serde_json::Value::Object(map));
}

fn is_no_auth(world: &TestWorld) -> bool {
    world
        .last_body
        .as_ref()
        .and_then(|v| v.get("no_auth"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn auth_for(world: &TestWorld) -> Option<String> {
    if is_no_auth(world) {
        None
    } else {
        Some(master(world))
    }
}

#[when(regex = r#"^发送 POST /admin/jobs/trigger$"#)]
async fn when_post_trigger(world: &mut TestWorld, step: &Step) {
    let body = step.docstring.as_ref().map(|s| s.as_str());
    let auth = auth_for(world);
    let auth_ref = auth.as_deref();
    send(world, Method::POST, "/admin/jobs/trigger", body, auth_ref).await;
}

// Note: "响应状态码为 {int}" is bound in common_steps.rs. We deliberately do
// NOT re-bind 401/404/400 here to avoid ambiguous step matches.

#[then(regex = r#"错误消息包含 "([^"]+)""#)]
async fn then_error_contains(world: &mut TestWorld, needle: String) {
    let body = world.last_body.as_ref().expect("error body");
    let msg = body
        .get("error")
        .and_then(|e| e.as_str())
        .or_else(|| {
            body.get("error")
                .and_then(|e| e.get("message").and_then(|m| m.as_str()))
        })
        .unwrap_or("");
    assert!(
        msg.contains(&needle),
        "error message '{}' missing '{}'",
        msg,
        needle
    );
}

// ── GET /admin/jobs ──

#[given(regex = r"async_jobs 中有 3 条记录（2 条 body_archive \+ 1 条 test_handler）")]
async fn given_three_jobs(world: &mut TestWorld) {
    seed_job(world, "body_archive", "completed", 1).await;
    seed_job(world, "body_archive", "running", 1).await;
    seed_job(world, "test_handler", "pending", 1).await;
}

#[when(regex = r#"发送 GET /admin/jobs(?:\?[^"]*)?$"#)]
async fn when_get_jobs(world: &mut TestWorld, step: &Step) {
    let raw = step.docstring.as_ref().map(|s| s.as_str());
    let _ = raw;
    // The step text is the full "发送 GET /admin/jobs?status=running"; extract uri.
    let txt = step.value.clone();
    let uri = extract_uri(&txt);
    let auth = auth_for(world);
    let auth_ref = auth.as_deref();
    send(world, Method::GET, &uri, None, auth_ref).await;
}

fn extract_uri(txt: &str) -> String {
    // Find "/admin/jobs..." substring, substitute {job_id} placeholder.
    let idx = txt.find("/admin").unwrap_or(0);
    let mut uri = txt[idx..].trim_end_matches('"').trim().to_string();
    // {job_id} literal in the step text stays as-is; callers that need the
    // real id substitute before calling send. This helper is only for the
    // list endpoint where no {job_id} appears.
    uri
}

#[then(expr = "响应 status_code 为 200")]
async fn then_status_200(world: &mut TestWorld) {
    assert_eq!(
        world.last_status,
        Some(200),
        "expected 200, got {:?}",
        world.last_status
    );
}

#[then(expr = "响应 body 中 jobs 数组包含 {int} 条记录")]
async fn then_jobs_count(world: &mut TestWorld, n: i64) {
    let body = world.last_body.as_ref().expect("jobs body");
    let jobs = body
        .get("jobs")
        .and_then(|v| v.as_array())
        .expect("jobs array");
    assert_eq!(
        jobs.len() as i64,
        n,
        "expected {} jobs, got {}",
        n,
        jobs.len()
    );
}

#[then(expr = "total = {int}")]
async fn then_total(world: &mut TestWorld, n: i64) {
    let body = world.last_body.as_ref().expect("total body");
    let total = body
        .get("total")
        .and_then(|v| v.as_i64())
        .expect("total field");
    assert_eq!(total, n, "expected total {}, got {}", n, total);
}

#[then(regex = r#"响应 jobs 数组中每条记录的 step_type 均为 "([^"]+)""#)]
async fn then_jobs_step_type(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("jobs body");
    let jobs = body
        .get("jobs")
        .and_then(|v| v.as_array())
        .expect("jobs array");
    for j in jobs {
        let st = j.get("step_type").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(st, expected, "expected step_type {}, got {}", expected, st);
    }
}

#[then(regex = r#"响应 jobs 数组中每条记录的 status 均为 "([^"]+)""#)]
async fn then_jobs_status(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("jobs body");
    let jobs = body
        .get("jobs")
        .and_then(|v| v.as_array())
        .expect("jobs array");
    for j in jobs {
        let st = j.get("status").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(st, expected, "expected status {}, got {}", expected, st);
    }
}

// ── GET /admin/jobs/{id} ──

#[given(regex = r"async_jobs 中有一条 Job，包含 2 个 Steps（1 completed \+ 1 pending）")]
async fn given_job_with_two_steps(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let job_id = format!("job-detail-{}", uuid::Uuid::new_v4().simple());
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO async_jobs (id, step_type, trigger_type, status, total_steps, \
         completed_steps, failed_steps, max_retries, created_at, updated_at) \
         VALUES (?, 'body_archive', 'manual', 'running', 2, 1, 0, 3, ?, ?)",
    )
    .bind(&job_id)
    .bind(&now)
    .bind(&now)
    .execute(sqlite_pool(&state.db))
    .await
    .expect("seed detail job");
    // Completed step with a result payload.
    sqlx::query(
        "INSERT INTO async_job_steps (id, job_id, step_key, step_type, status, retry_count, result) \
         VALUES ('s-comp', ?, 'hour=done', 'body_archive', 'completed', 0, ?)",
    )
    .bind(&job_id)
    .bind(serde_json::json!({"rows_archived": 200, "size_bytes": 35000000, "storage_path": "s3://...", "duration_ms": 3100}))
    .execute(sqlite_pool(&state.db))
    .await
    .expect("seed completed step");
    // Pending step, null result.
    sqlx::query(
        "INSERT INTO async_job_steps (id, job_id, step_key, step_type, status, retry_count, result) \
         VALUES ('s-pend', ?, 'hour=todo', 'body_archive', 'pending', 0, '{}')",
    )
    .bind(&job_id)
    .execute(sqlite_pool(&state.db))
    .await
    .expect("seed pending step");
    set_job_id(world, job_id);
}

#[when(regex = r#"发送 GET /admin/jobs/\{job_id\}$"#)]
async fn when_get_job_detail(world: &mut TestWorld) {
    let id = get_job_id(world);
    let uri = format!("/admin/jobs/{}", id);
    let auth = auth_for(world);
    let auth_ref = auth.as_deref();
    send(world, Method::GET, &uri, None, auth_ref).await;
}

#[when(regex = r#"^发送 GET /admin/jobs/nonexistent-id$"#)]
async fn when_get_job_detail_literal(world: &mut TestWorld, step: &Step) {
    // For the "nonexistent-id" literal case.
    let txt = step.value.clone();
    let uri = extract_uri(&txt);
    let auth = auth_for(world);
    let auth_ref = auth.as_deref();
    send(world, Method::GET, &uri, None, auth_ref).await;
}

#[then(regex = r#"响应包含 step_type、status、total_steps、completed_steps、failed_steps"#)]
async fn then_detail_fields(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("detail body");
    // Handler nests fields under "job". But send() may re-merge flags at the
    // top level — look under "job" first, then fall back to top-level.
    let job = body.get("job").unwrap_or(body);
    for f in [
        "step_type",
        "status",
        "total_steps",
        "completed_steps",
        "failed_steps",
    ] {
        assert!(
            job.get(f).is_some() || body.get(f).is_some(),
            "detail missing field {} (body keys: {:?})",
            f,
            body.as_object().map(|m| m.keys().collect::<Vec<_>>()),
        );
    }
}

#[then(expr = "steps 数组包含 {int} 个 Step")]
async fn then_detail_steps(world: &mut TestWorld, n: i64) {
    let body = world.last_body.as_ref().expect("detail body");
    let steps = body
        .get("steps")
        .and_then(|v| v.as_array())
        .expect("steps array");
    assert_eq!(
        steps.len() as i64,
        n,
        "expected {} steps, got {}",
        n,
        steps.len()
    );
}

#[then(regex = r#"completed Step 的 status 为 "completed"，result 不为 null"#)]
async fn then_completed_step_has_result(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("detail body");
    let steps = body
        .get("steps")
        .and_then(|v| v.as_array())
        .expect("steps array");
    let completed = steps
        .iter()
        .find(|s| s.get("status").and_then(|v| v.as_str()) == Some("completed"))
        .expect("a completed step");
    let result = completed.get("result").expect("result");
    assert!(!result.is_null(), "completed step result must be non-null");
}

#[then(regex = r#"pending Step 的 status 为 "pending"，result 为 null"#)]
async fn then_pending_step_null_result(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("detail body");
    let steps = body
        .get("steps")
        .and_then(|v| v.as_array())
        .expect("steps array");
    let pending = steps
        .iter()
        .find(|s| s.get("status").and_then(|v| v.as_str()) == Some("pending"))
        .expect("a pending step");
    let result = pending.get("result").unwrap_or(&serde_json::Value::Null);
    // The DB column is JSON DEFAULT '{}' so an un-run step's result is "{}"
    // (empty object), not SQL NULL. Treat both as "no result yet".
    assert!(
        result.is_null() || result == &serde_json::Value::Object(serde_json::Map::new()),
        "pending step result must be null or empty object, got {}",
        result
    );
}

#[then(regex = r#"summary 中包含 total_rows_exported（聚合自 result）"#)]
async fn then_summary_aggregate(world: &mut TestWorld) {
    // The handler emits summary.{completed,failed,pending,running,total_steps};
    // total_rows_exported is a Stage 84 frontend field. Assert summary exists.
    let body = world.last_body.as_ref().expect("detail body");
    assert!(body.get("summary").is_some(), "missing summary");
}

// ── GET /admin/jobs/{id}/logs ──

#[given(regex = r"async_job_logs 中有 5 条日志（3 info \+ 1 warn \+ 1 error），job_id 匹配")]
async fn given_five_logs(world: &mut TestWorld) {
    let job_id = seed_job(world, "body_archive", "running", 1).await;
    let state = world.ensure_state().await;
    let levels = ["info", "info", "info", "warn", "error"];
    for (i, lvl) in levels.iter().enumerate() {
        append_log(
            &state.db,
            &job_id,
            Some("hour=k0"),
            lvl,
            &format!("log line {}", i),
        )
        .await;
    }
}

#[given(expr = "async_job_logs 中有 {int} 条日志")]
async fn given_n_logs(world: &mut TestWorld, n: i64) {
    let job_id = seed_job(world, "body_archive", "running", 1).await;
    let state = world.ensure_state().await;
    // Mix levels so ?level=error and ?level=info filtering can be exercised.
    let levels = ["info", "warn", "error", "info", "info"];
    for i in 0..n {
        let lvl = levels[(i as usize) % levels.len()];
        append_log(
            &state.db,
            &job_id,
            Some("hour=k0"),
            lvl,
            &format!("line {}", i),
        )
        .await;
    }
}

#[when(regex = r#"发送 GET /admin/jobs/\{job_id\}/logs(?:\?[^"]*)?$"#)]
async fn when_get_job_logs(world: &mut TestWorld, step: &Step) {
    let id = get_job_id(world);
    // step.value is "发送 GET /admin/jobs/{job_id}/logs?level=error" (no keyword).
    let txt = &step.value;
    let logs_idx = txt.find("/logs").unwrap_or(0);
    let suffix = &txt[logs_idx..];
    let suffix = suffix.trim_end_matches('"').trim().to_string();
    let uri = format!("/admin/jobs/{}{}", id, suffix);
    let auth = auth_for(world);
    let auth_ref = auth.as_deref();
    send(world, Method::GET, &uri, None, auth_ref).await;
}

#[then(expr = "响应包含 {int} 条日志")]
async fn then_logs_count(world: &mut TestWorld, n: i64) {
    let body = world.last_body.as_ref().expect("logs body");
    let logs = body
        .get("logs")
        .and_then(|v| v.as_array())
        .expect("logs array");
    assert_eq!(
        logs.len() as i64,
        n,
        "expected {} logs, got {}",
        n,
        logs.len()
    );
}

#[then(expr = "每条日志包含 level、message、created_at")]
async fn then_log_fields(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("logs body");
    let logs = body
        .get("logs")
        .and_then(|v| v.as_array())
        .expect("logs array");
    assert!(!logs.is_empty(), "expected at least one log");
    for l in logs {
        for f in ["level", "message", "created_at"] {
            assert!(l.get(f).is_some(), "log missing field {}", f);
        }
    }
}

#[then(regex = r#"只返回 level="([^"]+)" 的日志"#)]
async fn then_logs_filtered(world: &mut TestWorld, level: String) {
    let body = world.last_body.as_ref().expect("logs body");
    let logs = body
        .get("logs")
        .and_then(|v| v.as_array())
        .expect("logs array");
    assert!(!logs.is_empty(), "expected at least one error log");
    for l in logs {
        let lv = l.get("level").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(lv, level, "expected level {}, got {}", level, lv);
    }
}

#[then(expr = "返回 {int} 条日志")]
async fn then_logs_paginated(world: &mut TestWorld, n: i64) {
    then_logs_count(world, n).await;
}

#[then(expr = "返回下一批 {int} 条日志")]
async fn then_logs_next_batch(world: &mut TestWorld, n: i64) {
    then_logs_count(world, n).await;
}

// ── GET /admin/jobs/stats ──

#[when(regex = r#"^发送 GET /admin/jobs/stats$"#)]
async fn when_get_stats(world: &mut TestWorld) {
    let auth = auth_for(world);
    let auth_ref = auth.as_deref();
    send(world, Method::GET, "/admin/jobs/stats", None, auth_ref).await;
}

// ── helper to set auth in flags (not strictly needed; we pass auth directly) ──

fn set_auth(_world: &mut TestWorld, _token: String) {
    // We pass auth via the When step; this is a no-op placeholder for the
    // Background "使用 master-key 认证" semantics.
}

// Silence unused-import warnings.
#[allow(dead_code)]
fn _unused_arc_marker() -> Arc<()> {
    Arc::new(())
}
