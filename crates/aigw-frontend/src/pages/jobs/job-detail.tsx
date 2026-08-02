import { useTranslation } from "react-i18next";
import {
  fetchJobDetail,
  fetchJobLogs,
  stepTypeLabel,
  formatCount,
  formatBytes,
  formatDuration,
  displayJobStatus,
} from "@/lib/api/jobs";
import type { JobDetailResponse, JobItem, StepItem, LogEntry } from "@/lib/api/jobs";
import React, { useState, useEffect, useCallback, useMemo } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { toast } from "sonner";

// ── Status Badge ──
function StatusBadge({ status, className }: { status: string; className?: string }) {
  const { t } = useTranslation();
  const colors: Record<string, string> = {
    pending: "bg-yellow-500/10 text-yellow-500 border-yellow-500/20",
    running: "bg-blue-500/10 text-blue-500 border-blue-500/20",
    completed: "bg-green-500/10 text-green-500 border-green-500/20",
    failed: "bg-red-500/10 text-red-500 border-red-500/20",
    partially_failed: "bg-orange-500/10 text-orange-500 border-orange-500/20",
  };
  return (
    <Badge
      variant="outline"
      className={colors[status] || "bg-muted"}
      aria-label={`Status: ${status}`}
    >
      {status === "running" && <span className="mr-1 inline-block w-2 h-2 rounded-full bg-blue-500 animate-pulse" />}
      {status === "pending" ? t("jobs.status.pending") : status === "running" ? t("jobs.status.running") : status === "completed" ? t("jobs.status.completed") : status === "failed" ? t("jobs.status.failed") : status === "partially_failed" ? t("jobs.status.partiallyFailed") : status}
    </Badge>
  );
}

// ── Step Status Icon ──
function StepStatus({ status, result }: { status: string; result: Record<string, unknown> | null }) {
  // Q3: completed + rows_archived=0 → no-op
  if (status === "completed" && result && (result.rows_archived === 0 || result.rows_archived === "0")) {
    return (
      <Badge variant="outline" className="bg-gray-100 text-gray-500 border-gray-300" aria-label="completed no-op">
        completed (no-op)
      </Badge>
    );
  }
  const icons: Record<string, string> = {
    pending: "⏳",
    running: "🔄",
    completed: "✅",
    failed: "❌",
  };
  return (
    <Badge variant="outline" className={(status === "failed" ? "text-red-500" : "") || ""} aria-label={status}>
      {icons[status] || ""} {status}
    </Badge>
  );
}

// ── Format step result ──
function formatStepResult(result: Record<string, unknown> | null): string {
  if (!result) return "-";
  const parts: string[] = [];
  if (typeof result.rows_archived === "number") {
    parts.push(formatCount(result.rows_archived) + " rows");
  }
  if (typeof result.size_bytes === "number" && result.size_bytes > 0) {
    parts.push(formatBytes(result.size_bytes as number));
  }
  if (typeof result.bytes_written === "number" && result.bytes_written > 0) {
    parts.push(formatBytes(result.bytes_written as number));
  }
  if (typeof result.rows_exported === "number") {
    parts.push(String(result.rows_exported));
  }
  if (typeof result.duration_ms === "number" && result.duration_ms > 0) {
    parts.push(formatDuration(result.duration_ms as number));
  }
  if (typeof result.storage_path === "string") {
    const p = result.storage_path;
    parts.push(p.length > 40 ? "..." + p.slice(-37) : p);
  }
  if (result.message && typeof result.message === "string") {
    parts.push(result.message);
  }
  return parts.length > 0 ? parts.join(" · ") : JSON.stringify(result).slice(0, 60);
}

// ── Steps Pagination (SpendLogs-style) ──
function StepsPagination({
  page,
  pageSize,
  totalCount,
  totalPages,
  onPage,
  onPageSize,
}: {
  page: number;
  pageSize: number;
  totalCount: number;
  totalPages: number;
  onPage: (p: number) => void;
  onPageSize: (s: number) => void;
}) {
  const { t: _t } = useTranslation();
  const from = totalCount === 0 ? 0 : (page - 1) * pageSize + 1;
  const to = Math.min(page * pageSize, totalCount);
  return (
    <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-2 mt-2">
      <div className="flex items-center gap-3">
        <span className="text-xs text-muted-foreground">{_t('pagination.showing', { from, to, total: totalCount })}</span>
        <span className="text-xs text-muted-foreground">{_t('pagination.pageInfo', { page, total: Math.max(totalPages, 1) })}</span>
      </div>
      <div className="flex items-center gap-2">
        <Select value={String(pageSize)} onValueChange={(v) => onPageSize(Number(v))}>
          <SelectTrigger className="h-7 w-[70px] text-xs"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="20">20</SelectItem>
            <SelectItem value="50">50</SelectItem>
            <SelectItem value="100">100</SelectItem>
          </SelectContent>
        </Select>
        <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => onPage(page - 1)} className="h-7 px-2">
          <ChevronLeft className="h-3.5 w-3.5" />
        </Button>
        <Button variant="outline" size="sm" disabled={page >= totalPages || totalPages === 0} onClick={() => onPage(page + 1)} className="h-7 px-2">
          <ChevronRight className="h-3.5 w-3.5" />
        </Button>
      </div>
    </div>
  );
}

// ── Step Logs (inline expandable) ──
function StepLogRows({ logs }: { logs: LogEntry[] }) {
  const { t } = useTranslation();
  if (logs.length === 0) {
    return <p className="p-2 text-xs text-muted-foreground">{t("jobs.noLogs")}</p>;
  }
  return (
    <table className="w-full text-sm">
      <thead className="bg-muted">
        <tr>
          <th className="text-left p-2 w-16">{t("jobs.level")}</th>
          <th className="text-left p-2">{t("jobs.message")}</th>
          <th className="text-left p-2 w-32">{t("jobs.logs.timestamp")}</th>
        </tr>
      </thead>
      <tbody>
        {logs.map((log, i) => (
          <tr key={i} className="border-t">
            <td className="p-2">
              <Badge
                variant={
                  log.level === "error" ? "destructive"
                  : log.level === "warn" ? "secondary"
                  : "outline"
                }
              >
                {log.level}
              </Badge>
            </td>
            <td className="p-2 font-mono text-xs">{log.message}</td>
            <td className="p-2 text-xs text-muted-foreground">
              {new Date(log.created_at).toLocaleTimeString()}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

// ── Error Boundary ──
interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends React.Component<
  { children: React.ReactNode; fallbackLabel?: string },
  ErrorBoundaryState
> {
  constructor(props: { children: React.ReactNode; fallbackLabel?: string }) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("JobDetailPage render error:", error, info.componentStack);
  }

  render() {
    if (this.state.hasError) {
      return (
        <Card className="border-red-500/30">
          <CardHeader>
            <CardTitle className="text-lg text-red-600 dark:text-red-400">
              {this.props.fallbackLabel || "Something went wrong"}
            </CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-sm text-muted-foreground mb-2">
              An unexpected error occurred while rendering this page.
            </p>
            {this.state.error && (
              <pre className="text-xs font-mono bg-muted p-2 rounded overflow-auto max-h-40">
                {this.state.error.message}
              </pre>
            )}
            <Button
              variant="outline"
              size="sm"
              className="mt-3"
              onClick={() => this.setState({ hasError: false, error: null })}
            >
              Try again
            </Button>
          </CardContent>
        </Card>
      );
    }

    return this.props.children;
  }
}

// ── Job Detail Page Component ──
export function JobDetailPage() {
  const { t } = useTranslation();
  const { jobId } = useParams<{ jobId: string }>();
  const navigate = useNavigate();

  const [detail, setDetail] = useState<JobDetailResponse | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [logFilter, setLogFilter] = useState("all");
  const [stepsPage, setStepsPage] = useState(1);
  const [stepsPageSize, setStepsPageSize] = useState(20);
  const [expandedPayload, setExpandedPayload] = useState<string | null>(null);
  const [expandedLogStep, setExpandedLogStep] = useState<string | null>(null);

  const loadDetail = useCallback(async () => {
    if (!jobId) return;
    try {
      const data = await fetchJobDetail(jobId);
      setDetail(data);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(`${t("jobs.toast.loadDetailFailed")}: ${msg}`);
    }
  }, [jobId]);

  const loadLogs = useCallback(async () => {
    if (!jobId) return;
    try {
      const data = await fetchJobLogs(jobId, {
        limit: 10000, // fetch all logs in one request
      });
      setLogs(data.logs || []);
    } catch {
      // logs are optional
    }
  }, [jobId]);

  // Group logs by step_key (stable across re-renders)
  const logsByStep = useMemo(() => {
    const map = new Map<string, LogEntry[]>();
    for (const log of logs) {
      // Apply log level filter
      if (logFilter !== "all" && log.level !== logFilter) continue;
      const key = log.step_key || "__global__";
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(log);
    }
    return map;
  }, [logs, logFilter]);

  useEffect(() => {
    loadDetail();
    loadLogs();
  }, [loadDetail, loadLogs]);

  // Auto-refresh for running jobs
  useEffect(() => {
    if (!detail) return;
    const ds = displayJobStatus(detail.job.status, detail.summary);
    if (ds !== "running") return;
    const timer = setInterval(() => {
      loadDetail();
      loadLogs();
    }, 10000);
    return () => clearInterval(timer);
  }, [detail, loadDetail, loadLogs]);

  if (!detail) {
    return <Skeleton className="h-60 w-full" />;
  }

  const {
    job,
    steps: rawSteps = [],
    summary: rawSummary,
  } = detail;
  const steps = rawSteps ?? [];
  const summary = rawSummary ?? { total_steps: steps.length, completed: 0, failed: 0, pending: 0, running: 0 };
  const ds = displayJobStatus(job.status, summary);
  const totalSteps = summary.total_steps || steps.length;
  const stepsTotalPages = Math.max(1, Math.ceil(steps.length / stepsPageSize));
  const stepsSlice = steps.slice(
    (stepsPage - 1) * stepsPageSize,
    stepsPage * stepsPageSize
  );

  // Global logs
  const globalLogs = logsByStep.get("__global__") || [];

  return (
    <div className="space-y-6">
      {/* Back button */}
      <Button variant="ghost" size="sm" onClick={() => navigate(-1)}>
        ← {t("common.back")}
      </Button>

      {/* Title */}
      <div className="flex items-center gap-3">
        <h1 className="text-2xl font-bold">
          {stepTypeLabel(job.step_type)} · {job.trigger_type}
        </h1>
        <StatusBadge status={ds} />
        <span className="text-sm text-muted-foreground">
          {new Date(job.created_at).toLocaleString()}
        </span>
      </div>

      {/* Summary */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("jobs.summary")}</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 md:grid-cols-5 gap-4 text-sm">
            <div>
              <span className="text-muted-foreground">{t("jobs.total")}:</span>{" "}
              {totalSteps}
            </div>
            <div>
              <span className="text-muted-foreground">{t("jobs.status.completed")}:</span>{" "}
              {summary.completed}
            </div>
            <div>
              <span className="text-muted-foreground">{t("jobs.status.failed")}:</span>{" "}
              {summary.failed}
            </div>
            <div>
              <span className="text-muted-foreground">{t("jobs.status.pending")}:</span>{" "}
              {summary.pending}
            </div>
            <div>
              <span className="text-muted-foreground">{t("jobs.status.running")}:</span>{" "}
              {summary.running}
            </div>
          </div>
          {/* Progress bar */}
          {totalSteps > 0 && (() => {
            const done = summary.completed + summary.failed;
            const pct = Math.round((done / totalSteps) * 100);
            return (
            <div className="mt-4 space-y-1">
              <div className="flex justify-between text-xs text-muted-foreground">
                <span>{t("jobs.progress")}</span>
                <span>
                  {done}/{totalSteps} ({summary.failed > 0 ? `${summary.failed} failed · ` : ""}
                  {pct}%)
                </span>
              </div>
              <div className="w-full bg-muted rounded-full h-2 overflow-hidden">
                <div
                  className="bg-green-500 h-2 transition-all"
                  style={{ width: `${Math.min(pct, 100)}%` }}
                />
              </div>
            </div>
            );
          })()}
        </CardContent>
      </Card>

      {/* Steps Table */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("jobs.steps")}</CardTitle>
        </CardHeader>
        <CardContent>
          <StepsPagination
            page={stepsPage}
            pageSize={stepsPageSize}
            totalCount={steps.length}
            totalPages={stepsTotalPages}
            onPage={(p) => setStepsPage(p)}
            onPageSize={(s) => { setStepsPageSize(s); setStepsPage(1); }}
          />
          <div className="border rounded overflow-hidden mt-2">
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead className="bg-muted">
                  <tr>
                    <th className="text-left p-2">{t("jobs.steps.stepKey")}</th>
                    <th className="text-left p-2">{t("jobs.steps.status")}</th>
                    <th className="text-left p-2">{t("jobs.steps.payload")}</th>
                    <th className="text-left p-2">{t("jobs.steps.result")}</th>
                    <th className="text-left p-2">{t("jobs.steps.duration")}</th>
                  </tr>
                </thead>
                <tbody>
                  {stepsSlice.map((step) => {
                    const duration = step.started_at && step.completed_at
                      ? new Date(step.completed_at).getTime() - new Date(step.started_at).getTime()
                      : null;
                    const stepLogs = logsByStep.get(step.step_key) || [];
                    const isLogExpanded = expandedLogStep === step.step_key;
                    return (
                      <React.Fragment key={step.id}>
                      <tr
                        className={`border-t cursor-pointer hover:bg-accent/50 ${isLogExpanded ? "bg-accent/30" : ""}`}
                        onClick={() => setExpandedLogStep(isLogExpanded ? null : step.step_key)}
                        tabIndex={0}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            setExpandedLogStep(isLogExpanded ? null : step.step_key);
                          }
                        }}
                        aria-expanded={isLogExpanded}
                      >
                        <td className="p-2 font-mono text-xs">
                          {step.step_key}
                          {stepLogs.length > 0 && (
                            <span className="ml-1 text-muted-foreground">
                              {isLogExpanded ? "▲" : "▼"}
                            </span>
                          )}
                        </td>
                        <td className="p-2">
                          <StepStatus status={step.status} result={step.result} />
                        </td>
                        <td className="p-2 font-mono text-xs max-w-[200px]">
                          {expandedPayload === step.id ? (
                            <pre className="whitespace-pre-wrap text-xs break-all">
                              {JSON.stringify(step.payload, null, 2)}
                            </pre>
                          ) : (
                            <span
                              className="cursor-pointer text-blue-500 hover:underline"
                              onClick={() => setExpandedPayload(step.id)}
                              role="button"
                              tabIndex={0}
                              onKeyDown={(e) => {
                                if (e.key === "Enter" || e.key === " ") {
                                  e.preventDefault();
                                  setExpandedPayload(step.id);
                                }
                              }}
                              aria-label={t("jobs.steps.expandPayload")}
                            >
                              {JSON.stringify(step.payload).slice(0, 40)}...
                            </span>
                          )}
                        </td>
                        <td className="p-2 font-mono text-xs">
                          {formatStepResult(step.result)}
                        </td>
                        <td className="p-2 text-xs text-muted-foreground">
                          {duration !== null ? formatDuration(duration) : "-"}
                        </td>
                      </tr>
                      {/* Expandable log rows */}
                      {isLogExpanded && (
                        <tr key={`${step.id}-logs`} className="border-t bg-muted/20">
                          <td colSpan={5} className="p-0">
                            <StepLogRows logs={stepLogs} />
                          </td>
                        </tr>
                      )}
                      </React.Fragment>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </div>
          <StepsPagination
            page={stepsPage}
            pageSize={stepsPageSize}
            totalCount={steps.length}
            totalPages={stepsTotalPages}
            onPage={(p) => setStepsPage(p)}
            onPageSize={(s) => { setStepsPageSize(s); setStepsPage(1); }}
          />
        </CardContent>
      </Card>
    </div>
  );
}
