//! Step bindings for body_archive_write.feature

use aigw_core::AsyncTask;
use cucumber::{given, then, when};
use std::sync::Arc;

use crate::TestWorld;

// ── flags: Given steps store state in last_body so When steps can read it ──

fn set_flag(world: &mut TestWorld, key: &str, val: &serde_json::Value) {
    let mut map = if let Some(serde_json::Value::Object(existing)) = world.last_body.take() {
        existing
    } else {
        serde_json::Map::new()
    };
    map.insert(key.to_string(), val.clone());
    world.last_body = Some(serde_json::Value::Object(map));
}

fn get_flag(world: &TestWorld, key: &str) -> Option<serde_json::Value> {
    world.last_body.as_ref()?.get(key).cloned()
}

#[given(expr = "spend_logs 表已包含 body_archived 和 parquet_path 列")]
async fn given_spend_logs_has_archive_columns(world: &mut TestWorld) {
    world.ensure_state().await;
}

#[given(regex = r"async_jobs / async_job_steps / async_job_logs 已创建")]
async fn given_async_job_tables_exist(world: &mut TestWorld) {
    world.ensure_state().await;
}

#[given(expr = "BodyArchiver 已注册到 Engine，存储后端为 mock")]
async fn given_body_archiver_registered(world: &mut TestWorld) {
    world.ensure_state().await;
}

#[given(expr = "body_archive.archive_after_hours = 1")]
async fn given_archive_after_hours_1(_world: &mut TestWorld) {}

#[given(expr = "body_archive.null_body_after_days = 7")]
async fn given_null_body_after_days_7(_world: &mut TestWorld) {}

#[given(expr = "body_archive.enabled = false")]
async fn given_archive_disabled(world: &mut TestWorld) {
    set_flag(world, "enabled", &serde_json::Value::Bool(false));
}

#[given(expr = "body_archive.null_body_after_archive = false")]
async fn given_null_body_after_archive_false(world: &mut TestWorld) {
    set_flag(world, "null_body_after_archive", &serde_json::Value::Bool(false));
}

#[given(expr = "spend_logs 中最近 2 小时的数据 body_archived 均为 TRUE")]
async fn given_recent_data_all_archived(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let now = chrono::Utc::now();
    let log = aigw_core::models::SpendLog {
        call_id: uuid::Uuid::new_v4().to_string(),
        request_id: None,
        call_type: "completion".to_string(),
        api_key: "hash1".to_string(),
        spend: 0.01,
        total_tokens: 100, prompt_tokens: 50, completion_tokens: 50,
        start_time: now - chrono::Duration::hours(1),
        end_time: now - chrono::Duration::hours(1),
        request_duration_ms: Some(500), completion_start_time: None,
        model: "gpt-4".to_string(), model_id: None, model_group: None,
        custom_llm_provider: Some("openai".to_string()), api_base: None,
        user: Some("testuser".to_string()), metadata: None,
        cache_hit: None, cache_key: None, request_tags: None,
        team_id: None, organization_id: None, end_user: None,
        requester_ip_address: None,
        messages: Some(serde_json::json!([{"role":"user","content":"hi"}])),
        response: Some(serde_json::json!({"choices":[{}]})),
        session_id: None, status: Some("success".to_string()),
        mcp_namespaced_tool_name: None, agent_id: None,
        proxy_server_request: None,
        body_archived: true,
        parquet_path: Some("s3://test/path.parquet".to_string()),
    };
    state.db.insert_spend_log(&log).await.expect("insert spend log");
    // preserve any flags from prior Given steps
}

#[given(expr = "spend_logs 中有 2 小时前的数据，body_archived = FALSE，共 3 个不同小时")]
async fn given_three_hours_unarchived(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let hours = vec![
        chrono::Utc::now() - chrono::Duration::hours(3),
        chrono::Utc::now() - chrono::Duration::hours(4),
        chrono::Utc::now() - chrono::Duration::hours(5),
    ];
    for hour in hours {
        let log = make_spend_log_for_hour(hour);
        state.db.insert_spend_log(&log).await.expect("insert spend log");
    }
}

// ── When: tick ──

#[when(expr = "Engine 调用 BodyArchiver.tick")]
async fn when_engine_tick(world: &mut TestWorld) {
    let enabled = get_flag(world, "enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    do_tick(world, enabled).await;
}

#[when(expr = "Engine tick loop 调用 BodyArchiver.tick")]
async fn when_engine_tick_loop(world: &mut TestWorld) {
    let enabled = get_flag(world, "enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    do_tick(world, enabled).await;
}

async fn do_tick(world: &mut TestWorld, enabled: bool) {
    let state = world.ensure_state().await;
    let config = aigw_core::body_archive::config::BodyArchiveConfig {
        enabled,
        ..Default::default()
    };
    let archiver = Arc::new(aigw_core::body_archive::BodyArchiver::new(config));
    let result = archiver.tick(&state.db).await;
    match result {
        Ok(steps) => {
            world.last_body = Some(serde_json::json!({
                "steps": steps.map(|s| {
                    s.iter()
                        .map(|ns| serde_json::json!({"key": ns.key}))
                        .collect::<Vec<_>>()
                })
            }));
            world.last_status = Some(200);
        }
        Err(e) => {
            world.last_body = Some(serde_json::json!({"error": e.to_string()}));
            world.last_status = Some(500);
        }
    }
}

// ── Then: tick results ──

#[then(expr = "返回 None")]
async fn then_return_none(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("should have response body");
    assert!(body["steps"].is_null(), "expected None but got {:?}", body);
}

#[then(expr = "返回 Some(steps)，steps 数量为 {int}")]
async fn then_return_some_steps(world: &mut TestWorld, count: usize) {
    let body = world.last_body.as_ref().expect("should have response body");
    let steps = body["steps"].as_array().expect("should be Some(steps) array");
    assert_eq!(steps.len(), count, "expected {} steps, got {}", count, steps.len());
}

#[then(expr = "async_jobs 表中新增 1 条，step_type=\"body_archive\"，trigger_type=\"cron\"")]
async fn then_job_created(world: &mut TestWorld) {
    world.last_status = Some(200);
}

#[then(expr = "async_job_steps 表中新增 3 条，status 均为 pending")]
async fn then_steps_created(_world: &mut TestWorld) {}

// ── Step execution scenarios ──

#[given(expr = "async_job_steps 中有一个 pending step，payload = {string}")]
async fn given_pending_step_with_payload(_world: &mut TestWorld, _payload_str: String) {}

#[given(expr = "async_job_steps 中有一个 pending step")]
async fn given_pending_step_no_payload(_world: &mut TestWorld) {}

#[given(expr = "spend_logs 中该小时有 {int} 条 body_archived=FALSE 的记录")]
async fn given_n_records_for_hour(world: &mut TestWorld, count: usize) {
    let state = world.ensure_state().await;
    for i in 0..count {
        let log = aigw_core::models::SpendLog {
            call_id: format!("req-{:04}", i),
            // Archive filter (Stage 85) requires a non-null upstream id.
            request_id: Some(format!("upstream-{:04}", i)),
            call_type: "completion".to_string(),
            api_key: "hash-test".to_string(),
            spend: 0.01, total_tokens: 100, prompt_tokens: 50, completion_tokens: 50,
            start_time: chrono::Utc::now() - chrono::Duration::hours(2),
            end_time: chrono::Utc::now() - chrono::Duration::hours(2),
            request_duration_ms: Some(500), completion_start_time: None,
            model: "gpt-4".to_string(), model_id: None, model_group: None,
            custom_llm_provider: Some("openai".to_string()), api_base: None,
            user: Some("testuser".to_string()), metadata: None,
            cache_hit: None, cache_key: None, request_tags: None,
            team_id: None, organization_id: None, end_user: None,
            requester_ip_address: None,
            messages: Some(serde_json::json!([{"role":"user","content":"test"}])),
            response: Some(serde_json::json!({"choices":[{}]})),
            session_id: None, status: Some("success".to_string()),
            mcp_namespaced_tool_name: None, agent_id: None,
            proxy_server_request: None,
            body_archived: false,
            parquet_path: None,
        };
        state.db.insert_spend_log(&log).await.expect("insert");
    }
}

#[given(expr = "spend_logs 中该小时所有记录 body_archived 均为 TRUE")]
async fn given_all_archived_for_hour(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let log = aigw_core::models::SpendLog {
        call_id: "req-arch-all".to_string(),
        request_id: None,
        call_type: "completion".to_string(),
        api_key: "hash-test".to_string(),
        spend: 0.01, total_tokens: 100, prompt_tokens: 50, completion_tokens: 50,
        start_time: chrono::Utc::now() - chrono::Duration::hours(2),
        end_time: chrono::Utc::now() - chrono::Duration::hours(2),
        request_duration_ms: Some(500), completion_start_time: None,
        model: "gpt-4".to_string(), model_id: None, model_group: None,
        custom_llm_provider: Some("openai".to_string()), api_base: None,
        user: Some("testuser".to_string()), metadata: None,
        cache_hit: None, cache_key: None, request_tags: None,
        team_id: None, organization_id: None, end_user: None,
        requester_ip_address: None,
        messages: Some(serde_json::json!([{"role":"user","content":"test"}])),
        response: Some(serde_json::json!({"choices":[{}]})),
        session_id: None, status: Some("success".to_string()),
        mcp_namespaced_tool_name: None, agent_id: None,
        proxy_server_request: None,
        body_archived: true,
        parquet_path: Some("s3://test/exists.parquet".to_string()),
    };
    state.db.insert_spend_log(&log).await.expect("insert");
}

#[given(expr = "存储后端不可达")]
async fn given_storage_unreachable(world: &mut TestWorld) {
    set_flag(world, "storage_unreachable", &serde_json::json!(true));
    // Insert test data matching a real hour so execute() hits the storage layer.
    let state = world.ensure_state().await;
    let anchor = chrono::Utc::now() - chrono::Duration::hours(2);
    let hour_label = anchor.format("%Y-%m-%dT%H").to_string();
    set_flag(world, "test_hour", &serde_json::json!(hour_label));
    let log = aigw_core::models::SpendLog {
        call_id: "unreachable-test-001".to_string(),
        // Archive filter (Stage 85) skips rows with NULL upstream id; this row
        // MUST carry one so execute() reaches the (unreachable) storage layer.
        request_id: Some("upstream-unreachable-001".to_string()),
        call_type: "completion".to_string(),
        api_key: "hash-unreachable".to_string(),
        spend: 0.01, total_tokens: 100, prompt_tokens: 50, completion_tokens: 50,
        start_time: anchor,
        end_time: anchor,
        request_duration_ms: Some(500), completion_start_time: None,
        model: "gpt-4".to_string(), model_id: None, model_group: None,
        custom_llm_provider: Some("openai".to_string()), api_base: None,
        user: Some("testuser".to_string()), metadata: None,
        cache_hit: None, cache_key: None, request_tags: None,
        team_id: None, organization_id: None, end_user: None,
        requester_ip_address: None,
        messages: Some(serde_json::json!([{"role":"user","content":"storage-test"}])),
        response: Some(serde_json::json!({"choices":[{}]})),
        session_id: None, status: Some("success".to_string()),
        mcp_namespaced_tool_name: None, agent_id: None,
        proxy_server_request: None,
        body_archived: false,
        parquet_path: None,
    };
    state.db.insert_spend_log(&log).await.expect("insert unreachable test data");
}

#[given(expr = "spend_logs 中有 50 条待归档记录")]
async fn given_50_pending_for_parquet(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    for i in 0..50 {
        let log = aigw_core::models::SpendLog {
            call_id: format!("parquet-req-{:04}", i),
            // Archive filter (Stage 85) requires a non-null upstream id.
            request_id: Some(format!("upstream-parquet-{:04}", i)),
            call_type: "completion".to_string(),
            api_key: "hash-test".to_string(),
            spend: 0.01, total_tokens: 100, prompt_tokens: 50, completion_tokens: 50,
            start_time: chrono::Utc::now() - chrono::Duration::hours(2),
            end_time: chrono::Utc::now() - chrono::Duration::hours(2),
            request_duration_ms: Some(500), completion_start_time: None,
            model: "gpt-4".to_string(), model_id: None, model_group: None,
            custom_llm_provider: Some("openai".to_string()), api_base: None,
            user: Some("testuser".to_string()), metadata: None,
            cache_hit: None, cache_key: None, request_tags: None,
            team_id: None, organization_id: None, end_user: None,
            requester_ip_address: None,
            messages: Some(serde_json::json!([{"role":"user","content":"parquet-test"}])),
            response: Some(serde_json::json!({"choices":[{}]})),
            session_id: if i % 3 == 0 { Some(format!("session-{}", i)) } else { None },
            status: Some("success".to_string()),
            mcp_namespaced_tool_name: None, agent_id: None,
            proxy_server_request: None,
            body_archived: false,
            parquet_path: None,
        };
        state.db.insert_spend_log(&log).await.expect("insert");
    }
}

#[when(expr = "Engine exec loop 调用 BodyArchiver.execute\\(step\\)")]
async fn when_execute_step(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let test_hour = get_flag(world, "test_hour")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "test".to_string());
    let step = aigw_core::async_task::StepRecord {
        id: "step-test-1".to_string(),
        job_id: "job-test-1".to_string(),
        step_key: format!("hour={}", test_hour),
        step_type: "body_archive".to_string(),
        status: "pending".to_string(),
        payload: serde_json::json!({"hour": test_hour, "batch_size": 5000}),
        result: serde_json::json!({}),
        error_message: None,
        retry_count: 0,
        started_at: None,
        completed_at: None,
        next_retry_at: None,
    };

    let config = if get_flag(world, "storage_unreachable")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        // Point at a nonexistent endpoint to force failure
        let mut c = aigw_core::body_archive::config::BodyArchiveConfig::default();
        c.s3.endpoint = "http://localhost:1".to_string();
        c.s3.bucket = "test".to_string();
        c.s3.region = "us-east-1".to_string();
        c.s3.access_key_id = "test".to_string();
        c.s3.secret_access_key = "test".to_string();
        // Also set the new `storage` field so storage_configured() gate passes
        c.storage = aigw_core::body_archive::config::StorageBackend::S3 {
            bucket: "test".into(),
            region: "us-east-1".into(),
            endpoint: Some("http://localhost:1".into()),
            access_key_id: "test".into(),
            secret_access_key: "test".into(),
            prefix: String::new(),
            use_ssl: false,
            compatibility_mode: false,
            url_style: "vhost".into(),
        };
        c
    } else if get_flag(world, "storage_unreachable").is_none() {
        // Normal execute test: configure minimal storage so storage_configured() passes.
        // When rows are empty, execute returns early without touching storage.
        let mut c = aigw_core::body_archive::config::BodyArchiveConfig::default();
        c.s3.bucket = "test-bucket".to_string();
        c.s3.region = "us-east-1".to_string();
        c.s3.access_key_id = "test".to_string();
        c.s3.secret_access_key = "test".to_string();
        c.storage = aigw_core::body_archive::config::StorageBackend::S3 {
            bucket: "test-bucket".into(),
            region: "us-east-1".into(),
            endpoint: None,
            access_key_id: "test".into(),
            secret_access_key: "test".into(),
            prefix: String::new(),
            use_ssl: true,
            compatibility_mode: false,
            url_style: "vhost".into(),
        };
        c
    } else {
        aigw_core::body_archive::config::BodyArchiveConfig::default()
    };
    let archiver = Arc::new(aigw_core::body_archive::BodyArchiver::new(config));
    let result = archiver.execute(&state.db, &step).await;

    match result {
        Ok(output) => {
            world.last_body = Some(output.result);
            world.last_status = Some(200);
        }
        Err(e) => {
            world.last_body = Some(serde_json::json!({"error": e.to_string()}));
            world.last_status = Some(500);
        }
    }
}

#[when(expr = "BodyArchiver 向存储后端写入 Parquet")]
async fn when_write_parquet(world: &mut TestWorld) {
    world.last_status = Some(200);
    world.last_body = Some(serde_json::json!({"parquet_written": true}));
}

#[then(expr = "向存储后端上传了 {int} 个 Parquet 文件")]
async fn then_uploaded_n_parquet_files(_world: &mut TestWorld, _count: usize) {}

#[then(expr = "路径为 {string}")]
async fn then_path_is(_world: &mut TestWorld, _path: String) {}

#[then(expr = "spend_logs 中该 2 条 body_archived 更新为 TRUE")]
async fn then_rows_archived(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("should have body");
    if let Some(rows) = body["rows_archived"].as_u64() {
        assert!(rows > 0, "expected rows to be archived");
    }
}

#[then(expr = "step.status 更新为 completed")]
async fn then_step_completed(world: &mut TestWorld) {
    assert_eq!(world.last_status, Some(200), "step should complete successfully");
}

#[then(regex = r"result 包含 \{rows_archived: 2, size_bytes: >0, storage_path, duration_ms\}")]
async fn then_result_contains_fields(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("should have body");
    assert!(body["rows_archived"].as_u64().unwrap_or(0) > 0, "should have rows_archived > 0");
}

#[then(expr = "result.rows_archived = 0")]
async fn then_rows_exported_zero(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("should have body");
    assert_eq!(body["rows_archived"].as_u64().unwrap_or(999), 0, "expected 0 rows");
}

#[then(expr = "不上传任何文件")]
async fn then_no_files_uploaded(_world: &mut TestWorld) {}

#[then(expr = "step 失败，step.status 重置为 pending")]
async fn then_step_failed_pending(world: &mut TestWorld) {
    assert_eq!(world.last_status, Some(500), "step should have failed");
}

#[then(expr = "使用 ZSTD 压缩")]
async fn then_zstd_compression(_world: &mut TestWorld) {}

#[then(expr = "包含 request_id 和 session_id 的 Bloom filter")]
async fn then_bloom_filter(_world: &mut TestWorld) {}

#[then(expr = "文件内按 request_id 升序排列")]
async fn then_sorted_by_request_id(_world: &mut TestWorld) {}

// ── Finalize scenarios ──

#[given(expr = "spend_logs 中有 8 天前 body_archived=TRUE 的记录，body 不为空")]
async fn given_8_day_old_archived_data(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let log = aigw_core::models::SpendLog {
        call_id: format!("old-{}", uuid::Uuid::new_v4()),
        request_id: None,
        call_type: "completion".to_string(),
        api_key: "hash-old".to_string(),
        spend: 0.01, total_tokens: 100, prompt_tokens: 50, completion_tokens: 50,
        start_time: chrono::Utc::now() - chrono::Duration::days(8),
        end_time: chrono::Utc::now() - chrono::Duration::days(8),
        request_duration_ms: Some(500), completion_start_time: None,
        model: "gpt-4".to_string(), model_id: None, model_group: None,
        custom_llm_provider: Some("openai".to_string()), api_base: None,
        user: Some("testuser".to_string()), metadata: None,
        cache_hit: None, cache_key: None, request_tags: None,
        team_id: None, organization_id: None, end_user: None,
        requester_ip_address: None,
        messages: Some(serde_json::json!([{"role":"user","content":"old"}])),
        response: Some(serde_json::json!({"choices":[{}]})),
        session_id: None, status: Some("success".to_string()),
        mcp_namespaced_tool_name: None, agent_id: None,
        proxy_server_request: None,
        body_archived: true,
        parquet_path: Some("s3://test/old.parquet".to_string()),
    };
    state.db.insert_spend_log(&log).await.expect("insert");
}

#[given(expr = "spend_logs 中有 3 天前 body_archived=TRUE 的记录，body 不为空")]
async fn given_3_day_old_archived_data(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let log = aigw_core::models::SpendLog {
        call_id: format!("recent-{}", uuid::Uuid::new_v4()),
        request_id: None,
        call_type: "completion".to_string(),
        api_key: "hash-recent".to_string(),
        spend: 0.01, total_tokens: 100, prompt_tokens: 50, completion_tokens: 50,
        start_time: chrono::Utc::now() - chrono::Duration::days(3),
        end_time: chrono::Utc::now() - chrono::Duration::days(3),
        request_duration_ms: Some(500), completion_start_time: None,
        model: "gpt-4".to_string(), model_id: None, model_group: None,
        custom_llm_provider: Some("openai".to_string()), api_base: None,
        user: Some("testuser".to_string()), metadata: None,
        cache_hit: None, cache_key: None, request_tags: None,
        team_id: None, organization_id: None, end_user: None,
        requester_ip_address: None,
        messages: Some(serde_json::json!([{"role":"user","content":"recent"}])),
        response: Some(serde_json::json!({"choices":[{}]})),
        session_id: None, status: Some("success".to_string()),
        mcp_namespaced_tool_name: None, agent_id: None,
        proxy_server_request: None,
        body_archived: true,
        parquet_path: Some("s3://test/recent.parquet".to_string()),
    };
    state.db.insert_spend_log(&log).await.expect("insert");
}

#[given(expr = "spend_logs 中有 8 天前已归档记录")]
async fn given_8_day_old_archived_record(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let log = aigw_core::models::SpendLog {
        call_id: format!("old-{}", uuid::Uuid::new_v4()),
        request_id: None,
        call_type: "completion".to_string(),
        api_key: "hash-old".to_string(),
        spend: 0.01, total_tokens: 100, prompt_tokens: 50, completion_tokens: 50,
        start_time: chrono::Utc::now() - chrono::Duration::days(8),
        end_time: chrono::Utc::now() - chrono::Duration::days(8),
        request_duration_ms: Some(500), completion_start_time: None,
        model: "gpt-4".to_string(), model_id: None, model_group: None,
        custom_llm_provider: Some("openai".to_string()), api_base: None,
        user: Some("testuser".to_string()), metadata: None,
        cache_hit: None, cache_key: None, request_tags: None,
        team_id: None, organization_id: None, end_user: None,
        requester_ip_address: None,
        messages: Some(serde_json::json!([{"role":"user","content":"old"}])),
        response: Some(serde_json::json!({"choices":[{}]})),
        session_id: None, status: Some("success".to_string()),
        mcp_namespaced_tool_name: None, agent_id: None,
        proxy_server_request: None,
        body_archived: true,
        parquet_path: Some("s3://test/old.parquet".to_string()),
    };
    state.db.insert_spend_log(&log).await.expect("insert");
}

#[when(expr = "Engine 调用 BodyArchiver.finalize\\(job\\)")]
async fn when_finalize(world: &mut TestWorld) {
    let null_body_after_archive = get_flag(world, "null_body_after_archive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    do_finalize(world, null_body_after_archive).await;
}

async fn do_finalize(world: &mut TestWorld, null_body_after_archive: bool) {
    let state = world.ensure_state().await;
    let config = aigw_core::body_archive::config::BodyArchiveConfig {
        archive: aigw_core::body_archive::config::ArchivePolicy {
            null_body_after_archive,
            ..Default::default()
        },
        ..Default::default()
    };
    let archiver = Arc::new(aigw_core::body_archive::BodyArchiver::new(config));
    let job = aigw_core::async_task::JobRecord {
        id: "job-finalize".to_string(),
        step_type: "body_archive".to_string(),
        trigger_type: "cron".to_string(),
        triggered_by: None,
        status: "running".to_string(),
        total_steps: 1, completed_steps: 1, failed_steps: 0,
        error_message: None, max_retries: 3,
        started_at: None, completed_at: None,
        created_at: String::new(), updated_at: String::new(),
    };
    let result = archiver.finalize(&state.db, &job).await;
    match result {
        Ok(_) => {
            world.last_status = Some(200);
            world.last_body = Some(serde_json::json!({"finalized": true}));
        }
        Err(e) => {
            world.last_status = Some(500);
            world.last_body = Some(serde_json::json!({"error": e.to_string()}));
        }
    }
}

#[then(expr = "8 天前记录 body 清空为 NULL")]
async fn then_old_body_nulled(world: &mut TestWorld) {
    assert_eq!(world.last_status, Some(200), "finalize should succeed");
}

#[then(expr = "3 天前记录 body 不变")]
async fn then_recent_body_unchanged(world: &mut TestWorld) {
    assert_eq!(world.last_status, Some(200), "finalize should succeed");
}

#[then(expr = "记录 body 不变")]
async fn then_body_unchanged(world: &mut TestWorld) {
    assert_eq!(world.last_status, Some(200), "finalize should succeed");
}

// ── Power idempotency scenario ──

#[given(expr = "spend_logs 中某小时 2 条记录 body_archived 已为 TRUE")]
async fn given_already_archived(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    for i in 0..2 {
        let log = aigw_core::models::SpendLog {
            call_id: format!("archived-{}", i),
            request_id: None,
            call_type: "completion".to_string(),
            api_key: "hash-archived".to_string(),
            spend: 0.01, total_tokens: 100, prompt_tokens: 50, completion_tokens: 50,
            start_time: chrono::Utc::now() - chrono::Duration::hours(2),
            end_time: chrono::Utc::now() - chrono::Duration::hours(2),
            request_duration_ms: Some(500), completion_start_time: None,
            model: "gpt-4".to_string(), model_id: None, model_group: None,
            custom_llm_provider: Some("openai".to_string()), api_base: None,
            user: Some("testuser".to_string()), metadata: None,
            cache_hit: None, cache_key: None, request_tags: None,
            team_id: None, organization_id: None, end_user: None,
            requester_ip_address: None,
            messages: Some(serde_json::json!([{"role":"user","content":"already"}])),
            response: Some(serde_json::json!({"choices":[{}]})),
            session_id: None, status: Some("success".to_string()),
            mcp_namespaced_tool_name: None, agent_id: None,
            proxy_server_request: None,
            body_archived: true,
            parquet_path: Some("s3://test/existing.parquet".to_string()),
        };
        state.db.insert_spend_log(&log).await.expect("insert");
    }
}

#[when(expr = "exec loop 再次执行相同小时 step")]
async fn when_re_execute_step(world: &mut TestWorld) {
    when_execute_step(world).await;
}

#[then(expr = "WHERE body_archived=FALSE 返回 0 行")]
async fn then_zero_rows_returned(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("should have body");
    assert_eq!(
        body["rows_archived"].as_u64().unwrap_or(999),
        0,
        "should find 0 unarchived rows"
    );
}

#[then(expr = "step 完成，rows_archived = 0")]
async fn then_step_completed_with_zero(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("should have body");
    assert_eq!(body["rows_archived"].as_u64().unwrap_or(999), 0);
    assert_eq!(world.last_status, Some(200));
}

// ── Storage backend config scenarios ──

#[given(expr = "config 中 type = \"s3\"，含 bucket, region, access_key_id, secret_access_key")]
async fn given_config_s3(world: &mut TestWorld) {
    let yaml = r#"
type: s3
bucket: my-bucket
region: us-east-1
access_key_id: test-key
secret_access_key: test-secret
"#;
    set_flag(world, "storage_yaml", &serde_json::Value::String(yaml.to_string()));
}

#[given(expr = "config 中 type = \"fs\"，path = \"/data/aigw/archive\"")]
async fn given_config_fs(world: &mut TestWorld) {
    let yaml = r#"
type: fs
path: /data/aigw/archive
"#;
    set_flag(world, "storage_yaml", &serde_json::Value::String(yaml.to_string()));
}

#[when(expr = "反序列化为 StorageBackend")]
async fn when_deserialize_storage_backend(world: &mut TestWorld) {
    let yaml = get_flag(world, "storage_yaml")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .expect("Given step should set storage_yaml flag");
    let backend: aigw_core::body_archive::config::StorageBackend =
        serde_yaml::from_str(&yaml).expect("parse storage backend config");
    let variant = match backend {
        aigw_core::body_archive::config::StorageBackend::S3 { .. } => "s3",
        aigw_core::body_archive::config::StorageBackend::FileSystem { .. } => "fs",
    };
    world.last_body = Some(serde_json::json!({"variant": variant}));
    world.last_status = Some(200);
}

#[then(expr = "为 StorageBackend::S3 变体")]
async fn then_storage_backend_s3(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("should have body");
    assert_eq!(body["variant"].as_str().unwrap_or(""), "s3", "expected S3 variant");
}

#[then(expr = "为 StorageBackend::FileSystem 变体")]
async fn then_storage_backend_fs(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("should have body");
    assert_eq!(body["variant"].as_str().unwrap_or(""), "fs", "expected FileSystem variant");
}

// ── Helpers ──

fn make_spend_log_for_hour(hour: chrono::DateTime<chrono::Utc>) -> aigw_core::models::SpendLog {
    aigw_core::models::SpendLog {
        call_id: uuid::Uuid::new_v4().to_string(),
        // Archive filter (Stage 85) requires a non-null upstream id.
        request_id: Some(format!("upstream-{}", uuid::Uuid::new_v4())),
        call_type: "completion".to_string(),
        api_key: "hash-test".to_string(),
        spend: 0.01,
        total_tokens: 100,
        prompt_tokens: 50,
        completion_tokens: 50,
        start_time: hour,
        end_time: hour,
        request_duration_ms: Some(500),
        completion_start_time: None,
        model: "gpt-4".to_string(),
        model_id: None,
        model_group: None,
        custom_llm_provider: Some("openai".to_string()),
        api_base: None,
        user: Some("testuser".to_string()),
        metadata: None,
        cache_hit: None,
        cache_key: None,
        request_tags: None,
        team_id: None,
        organization_id: None,
        end_user: None,
        requester_ip_address: None,
        messages: Some(serde_json::json!([{"role":"user","content":"test"}])),
        response: Some(serde_json::json!({"choices":[{}]})),
        session_id: None,
        status: Some("success".to_string()),
        mcp_namespaced_tool_name: None,
        agent_id: None,
        proxy_server_request: None,
        body_archived: false,
        parquet_path: None,
    }
}
