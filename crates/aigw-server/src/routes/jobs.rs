//! Admin job API endpoints — /admin/jobs/* and /admin/archive/stats
//!
//! Provides trigger, list, detail, stats, and log endpoints for the async job engine.
//! Requires admin authentication.

use aigw_core::async_task::{AsyncTask, JobRecord};
use aigw_core::engine::{create_job, get_job_detail, get_job_logs as engine_get_job_logs, get_job_stats, list_jobs};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::keys::SharedState;
use super::spend::{require_admin, SpendAuth};

/// POST /admin/jobs/trigger request body.
#[derive(Debug, Deserialize)]
pub struct TriggerJobRequest {
    pub step_type: String,
    pub payload: Value,
}

/// POST /admin/jobs/trigger response.
#[derive(Debug, Serialize)]
pub struct TriggerJobResponse {
    pub job_id: String,
    pub status: String,
    pub total_steps: i32,
}

/// Query params for GET /admin/jobs
#[derive(Debug, Deserialize)]
pub struct ListJobsQuery {
    pub step_type: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i32>,
    pub page: Option<i32>,
}

/// Query params for GET /admin/jobs/{id}/logs
#[derive(Debug, Deserialize)]
pub struct JobLogsQuery {
    pub level: Option<String>,
    pub limit: Option<i32>,
    pub page: Option<i32>,
}

/// Job list item in response.
#[derive(Debug, Serialize)]
pub struct JobListItem {
    pub id: String,
    pub step_type: String,
    pub trigger_type: String,
    pub triggered_by: Option<String>,
    pub status: String,
    pub total_steps: i32,
    pub completed_steps: i32,
    pub failed_steps: i32,
    pub created_at: String,
    pub updated_at: String,
}

impl From<JobRecord> for JobListItem {
    fn from(j: JobRecord) -> Self {
        Self {
            id: j.id,
            step_type: j.step_type,
            trigger_type: j.trigger_type,
            triggered_by: j.triggered_by,
            status: j.status,
            total_steps: j.total_steps,
            completed_steps: j.completed_steps,
            failed_steps: j.failed_steps,
            created_at: j.created_at,
            updated_at: j.updated_at,
        }
    }
}

/// Job detail with steps.
#[derive(Debug, Serialize)]
pub struct JobDetailResponse {
    pub job: JobListItem,
    pub steps: Vec<serde_json::Value>,
    pub summary: serde_json::Value,
}

/// Step in job detail.
#[derive(Debug, Serialize)]
pub struct StepResponse {
    pub id: String,
    pub step_key: String,
    pub status: String,
    pub payload: Value,
    pub result: Value,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// POST /admin/jobs/trigger
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Manually trigger a job for a registered step_type.
pub async fn trigger_job(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(req): Json<TriggerJobRequest>,
) -> Result<Json<TriggerJobResponse>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let step_type = req.step_type.as_str();
    let triggered_by = auth.user_id.clone().unwrap_or_else(|| "admin".to_string());

    // Only body_archive is supported for now
    let steps = match step_type {
        "body_archive" => {
            let archiver = state.body_archiver.as_ref()
                .ok_or((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error": "body archiver not configured"})),
                ))?;
            if !archiver.storage_configured() {
                return Err((
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"error": "body archive storage not configured"})),
                ));
            }
            archiver.steps_from_payload(&req.payload).await.map_err(|e| {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()})))
            })?
        }
        _ => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("unknown step_type: {}", step_type)})),
            ));
        }
    };

    let job_id = create_job(&state.db, step_type, "manual", Some(&triggered_by), &steps, 3).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;

    Ok(Json(TriggerJobResponse {
        job_id,
        status: "pending".to_string(),
        total_steps: steps.len() as i32,
    }))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// GET /admin/jobs
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// List all jobs, filtered by step_type and/or status.
pub async fn list_jobs_handler(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(params): Query<ListJobsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let limit = params.limit.unwrap_or(50).min(200);
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * limit;

    let (jobs, total) = list_jobs(&state.db, params.step_type.as_deref(), params.status.as_deref(), limit, offset).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;

    let job_items: Vec<JobListItem> = jobs.into_iter().map(JobListItem::from).collect();

    Ok(Json(serde_json::json!({
        "jobs": job_items,
        "page": page,
        "limit": limit,
        "total": total,
    })))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// GET /admin/jobs/stats
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Get engine stats per step_type.
pub async fn job_stats_handler(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let stats = get_job_stats(&state.db).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;

    Ok(Json(stats))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// GET /admin/jobs/{job_id}
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Get a single job with its steps.
pub async fn job_detail_handler(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(job_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let result = get_job_detail(&state.db, &job_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;

    match result {
        Some((job, steps)) => {
            let steps_json: Vec<Value> = steps.iter().map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "step_key": s.step_key,
                    "status": s.status,
                    "payload": s.payload,
                    "result": s.result,
                    "error_message": s.error_message,
                    "retry_count": s.retry_count,
                    "started_at": s.started_at,
                    "completed_at": s.completed_at,
                })
            }).collect();

            Ok(Json(serde_json::json!({
                "job": JobListItem::from(job),
                "steps": steps_json,
                "summary": {
                    "total_steps": steps.len(),
                    "completed": steps.iter().filter(|s| s.status == "completed").count(),
                    "failed": steps.iter().filter(|s| s.status == "failed").count(),
                    "pending": steps.iter().filter(|s| s.status == "pending").count(),
                    "running": steps.iter().filter(|s| s.status == "running").count(),
                },
            })))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("job not found: {}", job_id)})),
        )),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// GET /admin/jobs/{job_id}/logs
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Get execution logs for a job.
pub async fn job_logs_handler(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(job_id): Path<String>,
    Query(params): Query<JobLogsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let limit = params.limit.unwrap_or(200).min(1000);
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * limit;

    let logs = engine_get_job_logs(&state.db, &job_id, params.level.as_deref(), limit, offset).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;

    let log_entries: Vec<Value> = logs.into_iter().map(|entry| {
        serde_json::json!({
            "step_key": entry.step_key,
            "level": entry.level,
            "message": entry.message,
            "created_at": entry.created_at,
        })
    }).collect();

    Ok(Json(serde_json::json!({
        "logs": log_entries,
        "page": page,
        "limit": limit,
    })))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// GET /admin/archive/stats
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Get body archive specific statistics.
pub async fn archive_stats_handler(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let archiver = state.body_archiver.as_ref()
        .ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "body archiver not configured"})),
        ))?;
    let stats = archiver.get_archive_stats(&state.db).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))))?;

    Ok(Json(stats))
}
