import { apiGet, apiPost } from "@/lib/api";
import i18n from "@/i18n";

// ── Types ──

export interface JobStats {
  [stepType: string]: {
    queue: {
      pending: number;
      running: number;
      completed: number;
      failed: number;
    };
  };
}

export interface JobItem {
  id: string;
  step_type: string;
  trigger_type: string;
  triggered_by: string | null;
  status: string;
  total_steps: number;
  completed_steps: number;
  failed_steps: number;
  created_at: string;
  updated_at: string;
}

export interface StepItem {
  id: string;
  step_key: string;
  status: string;
  payload: Record<string, unknown>;
  result: Record<string, unknown> | null;
  error_message: string | null;
  retry_count: number;
  started_at: string | null;
  completed_at: string | null;
}

export interface LogEntry {
  step_key: string | null;
  level: string;
  message: string;
  created_at: string;
}

export interface ArchiveStats {
  total_archived_rows: number;
  pending_rows: number;
  auto_archive: boolean;
  storage_configured: boolean;
}

export interface JobListResponse {
  jobs: JobItem[];
  page: number;
  limit: number;
  total: number;
}

export interface JobDetailResponse {
  job: JobItem;
  steps: StepItem[];
  summary: {
    total_steps: number;
    completed: number;
    failed: number;
    pending: number;
    running: number;
  };
}

export interface JobLogsResponse {
  logs: LogEntry[];
  page: number;
  limit: number;
}

export interface TriggerJobResponse {
  job_id: string;
  status: string;
  total_steps: number;
}

// ── API functions ──

export function fetchJobStats(): Promise<JobStats> {
  return apiGet<JobStats>("/admin/jobs/stats");
}

export function fetchJobs(params: {
  step_type?: string;
  status?: string;
  page?: number;
  limit?: number;
}): Promise<JobListResponse> {
  const searchParams = new URLSearchParams();
  if (params.step_type) searchParams.set("step_type", params.step_type);
  if (params.status) searchParams.set("status", params.status);
  if (params.page) searchParams.set("page", String(params.page));
  if (params.limit) searchParams.set("limit", String(params.limit));
  const qs = searchParams.toString();
  return apiGet<JobListResponse>(`/admin/jobs${qs ? `?${qs}` : ""}`);
}

export function fetchJobDetail(jobId: string): Promise<JobDetailResponse> {
  return apiGet<JobDetailResponse>(`/admin/jobs/${jobId}`);
}

export function fetchJobLogs(
  jobId: string,
  params: { page?: number; limit?: number; level?: string },
): Promise<JobLogsResponse> {
  const searchParams = new URLSearchParams();
  if (params.page) searchParams.set("page", String(params.page));
  if (params.limit) searchParams.set("limit", String(params.limit));
  if (params.level && params.level !== "all")
    searchParams.set("level", params.level);
  const qs = searchParams.toString();
  return apiGet<JobLogsResponse>(
    `/admin/jobs/${jobId}/logs${qs ? `?${qs}` : ""}`,
  );
}

export function fetchArchiveStats(): Promise<ArchiveStats> {
  return apiGet<ArchiveStats>("/admin/archive/stats");
}

// ── Budget reset stats ──

export interface BudgetResetPreviewItem {
  entity_type: string;
  entity_id: string;
  alias: string;
  spend: number;
  max_budget?: number | null;
  budget_duration: string;
  budget_reset_at?: string | null;
}

export interface BudgetResetEntityCount {
  ready: number;
  total: number;
}

export interface BudgetResetLastReset {
  job_id: string;
  trigger_type: string;
  status: string;
  started_at?: string | null;
  completed_at?: string | null;
  total_steps: number;
  completed_steps: number;
  failed_steps: number;
}

export interface BudgetResetStats {
  tick_interval_sec: number;
  next_tick_at: string;
  counts: Record<string, BudgetResetEntityCount>;
  ready_total: number;
  preview: BudgetResetPreviewItem[];
  last_reset: BudgetResetLastReset | null;
}

export function fetchBudgetResetStats(): Promise<BudgetResetStats> {
  return apiGet<BudgetResetStats>("/admin/budget-reset/stats");
}

export function triggerJob(payload: {
  step_type: string;
  payload: Record<string, unknown>;
}): Promise<TriggerJobResponse> {
  return apiPost<TriggerJobResponse>("/admin/jobs/trigger", payload);
}

// ── Helpers ──

/** Map step_type to human-readable i18n key. Use stepTypeLabel() instead. */
const STEP_LABELS: Record<string, string> = {
  body_archive: "jobs.subTabs.bodyArchive",
  budget_reset: "jobs.subTabs.budgetReset",
};

/** Localize the raw `trigger_type` value ("cron" / "manual"). */
export function triggerTypeLabel(trigger: string): string {
  if (trigger === "cron") return i18n.t("jobs.trigger.cron");
  if (trigger === "manual") return i18n.t("jobs.trigger.manual");
  return trigger;
}

/**
 * Step types the frontend always advertises as tabs, independent of job history.
 *
 * The backend `GET /admin/jobs/stats` derives step_types from `SELECT DISTINCT step_type FROM
 * async_job_steps`, so on a fresh DB it returns `{}` and no task tab renders — hiding the
 * implemented AsyncTask UI. These are the registered async-task types the UI knows about; seeding
 * them here ensures the tabs (and the Body Archive page) are reachable even before any job runs.
 * Any additional step_type the backend reports is still appended (see JobsPage stepTypes merge).
 */
export const KNOWN_STEP_TYPES: string[] = ["body_archive", "budget_reset"];

export function stepTypeLabel(st: string): string {
  const key = STEP_LABELS[st];
  return key ? i18n.t(key as never) : st.replace(/_/g, " ");
}

/** Format a number with K/M suffixes. */
export function formatCount(n: number): string {
  if (n >= 1_000_000) {
    const v = n / 1_000_000;
    return `${Number.isInteger(v) ? v : v.toFixed(1)}M`;
  }
  if (n >= 1_000) {
    const v = n / 1_000;
    return `${Number.isInteger(v) ? v : v.toFixed(1)}K`;
  }
  return String(n);
}

/** Format bytes to human-readable size. */
export function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "0 B";
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
  if (bytes >= 1_024) return `${(bytes / 1_024).toFixed(1)} KB`;
  return `${bytes} B`;
}

/** Format duration in ms to human-readable. */
export function formatDuration(ms: number): string {
  if (!ms || ms <= 0) return "-";
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  return `${ms}ms`;
}

/**
 * Compute display status for a job, considering running/pending step counts.
 * If summary says there are running steps, show "running" even if job.status says "pending".
 * This handles the Q1 pending bug: the job may be pending at DB level but has running steps.
 */
export function displayJobStatus(
  jobStatus: string,
  summary?: { running: number; pending: number },
): string {
  if (summary?.running && summary.running > 0) return "running";
  return jobStatus;
}
