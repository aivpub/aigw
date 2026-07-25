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
import { useState, useEffect, useCallback } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { toast } from "sonner";

// ── Status Badge ──
function StatusBadge({ status, className }: { status: string; className?: string }) {
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
      {status}
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
  if (typeof result.bytes_written === "number" && result.bytes_written > 0) {
    parts.push(formatBytes(result.bytes_written as number));
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

// ── Inline Steps Pagination ──
function StepsPagination({
  page,
  totalPages,
  onPageChange,
}: {
  page: number;
  totalPages: number;
  onPageChange: (p: number) => void;
}) {
  if (totalPages <= 1) return null;
  const pages = Array.from({ length: totalPages }, (_, i) => i + 1);

  return (
    <nav role="navigation" aria-label="steps pagination" className="flex items-center gap-1 mt-2">
      <Button
        variant="ghost"
        size="sm"
        disabled={page <= 1}
        onClick={() => onPageChange(page - 1)}
        aria-label="Previous page"
      >
        ←
      </Button>
      {pages.map((p) => (
        <Button
          key={p}
          variant={p === page ? "default" : "ghost"}
          size="sm"
          onClick={() => onPageChange(p)}
          aria-label={`Page ${p}`}
          aria-current={p === page ? "page" : undefined}
        >
          {p}
        </Button>
      ))}
      <Button
        variant="ghost"
        size="sm"
        disabled={page >= totalPages}
        onClick={() => onPageChange(page + 1)}
        aria-label="Next page"
      >
        →
      </Button>
    </nav>
  );
}

// ── Logs by step ──
function LogsByStep({
  logs,
  steps,
  logFilter,
  setLogFilter,
}: {
  logs: LogEntry[];
  steps: StepItem[];
  logFilter: string;
  setLogFilter: (f: string) => void;
}) {
  const [expandedStep, setExpandedStep] = useState<string | null>(null);

  // Group logs by step_key
  const logsByStep = new Map<string | null, LogEntry[]>();
  for (const log of logs) {
    const key = log.step_key || "__global__";
    if (!logsByStep.has(key)) logsByStep.set(key, []);
    logsByStep.get(key)!.push(log);
  }

  return (
    <div>
      <div className="flex items-center justify-between mb-2">
        <h3 className="text-sm font-medium">Logs</h3>
        <div className="flex gap-2">
          {["all", "info", "warn", "error"].map((l) => (
            <Button
              key={l}
              variant={logFilter === l ? "default" : "outline"}
              size="sm"
              onClick={() => setLogFilter(l)}
            >
              {l}
            </Button>
          ))}
        </div>
      </div>

      {steps.map((step) => {
        const stepLogs = logsByStep.get(step.step_key) || [];
        const isExpanded = expandedStep === step.step_key;

        return (
          <div key={step.step_key} className="border rounded mb-2">
            <div
              className="flex items-center justify-between p-2 cursor-pointer hover:bg-accent"
              onClick={() => setExpandedStep(isExpanded ? null : step.step_key)}
              role="button"
              tabIndex={0}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  setExpandedStep(isExpanded ? null : step.step_key);
                }
              }}
              aria-expanded={isExpanded}
              aria-label={`Logs for step ${step.step_key}`}
            >
              <span className="text-xs font-mono">{step.step_key}</span>
              <span className="text-xs text-muted-foreground">
                {stepLogs.length} logs {isExpanded ? "▲" : "▼"}
              </span>
            </div>
            {isExpanded && (
              <div className="border-t">
                {stepLogs.length === 0 ? (
                  <p className="p-2 text-xs text-muted-foreground">No logs for this step.</p>
                ) : (
                  <table className="w-full text-sm">
                    <tbody>
                      {stepLogs.map((log, i) => (
                        <tr key={i} className="border-t">
                          <td className="p-2 w-16">
                            <Badge
                              variant={
                                log.level === "error"
                                  ? "destructive"
                                  : log.level === "warn"
                                  ? "secondary"
                                  : "outline"
                              }
                            >
                              {log.level}
                            </Badge>
                          </td>
                          <td className="p-2 font-mono text-xs">{log.message}</td>
                          <td className="p-2 text-xs text-muted-foreground w-32">
                            {new Date(log.created_at).toLocaleTimeString()}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </div>
            )}
          </div>
        );
      })}

      {/* Global logs (no step_key) */}
      {(logsByStep.get("__global__") || []).length > 0 && (
        <div className="border rounded p-2">
          <p className="text-xs text-muted-foreground mb-1">General Logs</p>
          <table className="w-full text-sm">
            <tbody>
              {(logsByStep.get("__global__") || []).map((log, i) => (
                <tr key={i} className="border-t">
                  <td className="p-2 w-16">
                    <Badge
                      variant={
                        log.level === "error"
                          ? "destructive"
                          : log.level === "warn"
                          ? "secondary"
                          : "outline"
                      }
                    >
                      {log.level}
                    </Badge>
                  </td>
                  <td className="p-2 font-mono text-xs">{log.message}</td>
                  <td className="p-2 text-xs text-muted-foreground w-32">
                    {new Date(log.created_at).toLocaleTimeString()}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

// ── Job Detail Page Component ──
export function JobDetailPage() {
  const { jobId } = useParams<{ jobId: string }>();
  const navigate = useNavigate();

  const [detail, setDetail] = useState<JobDetailResponse | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [logFilter, setLogFilter] = useState("all");
  const [stepsPage, setStepsPage] = useState(1);
  const [expandedPayload, setExpandedPayload] = useState<string | null>(null);
  const stepsPageSize = 20;

  const loadDetail = useCallback(async () => {
    if (!jobId) return;
    try {
      const data = await fetchJobDetail(jobId);
      setDetail(data);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(`Failed to load job detail: ${msg}`);
    }
  }, [jobId]);

  const loadLogs = useCallback(async () => {
    if (!jobId) return;
    try {
      const data = await fetchJobLogs(jobId, {
        level: logFilter,
        limit: 200,
      });
      setLogs(data.logs || []);
    } catch {
      // logs are optional
    }
  }, [jobId, logFilter]);

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

  // When log filter changes
  useEffect(() => {
    loadLogs();
  }, [logFilter]);

  if (!detail) {
    return <Skeleton className="h-60 w-full" />;
  }

  const {
    job,
    steps,
    summary,
  } = detail;
  const ds = displayJobStatus(job.status, summary);
  const totalSteps = summary.total_steps || steps.length;
  const stepsTotalPages = Math.max(1, Math.ceil(steps.length / stepsPageSize));
  const stepsSlice = steps.slice(
    (stepsPage - 1) * stepsPageSize,
    stepsPage * stepsPageSize
  );

  return (
    <div className="space-y-6">
      {/* Back button */}
      <Button variant="ghost" size="sm" onClick={() => navigate("/dash/jobs")}>
        ← Back to Jobs
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
          <CardTitle className="text-base">Summary</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 md:grid-cols-5 gap-4 text-sm">
            <div>
              <span className="text-muted-foreground">Total:</span>{" "}
              {totalSteps}
            </div>
            <div>
              <span className="text-muted-foreground">Completed:</span>{" "}
              {summary.completed}
            </div>
            <div>
              <span className="text-muted-foreground">Failed:</span>{" "}
              {summary.failed}
            </div>
            <div>
              <span className="text-muted-foreground">Pending:</span>{" "}
              {summary.pending}
            </div>
            <div>
              <span className="text-muted-foreground">Running:</span>{" "}
              {summary.running}
            </div>
          </div>
          {/* Progress bar */}
          {totalSteps > 0 && (
            <div className="mt-4 space-y-1">
              <div className="flex justify-between text-xs text-muted-foreground">
                <span>Progress</span>
                <span>
                  {job.completed_steps}/{totalSteps} ({summary.failed > 0 ? `${summary.failed} failed · ` : ""}
                  {Math.round((job.completed_steps / totalSteps) * 100)}%)
                </span>
              </div>
              <div className="w-full bg-muted rounded-full h-2 overflow-hidden">
                <div
                  className="bg-green-500 h-2 transition-all"
                  style={{ width: `${(job.completed_steps / Math.max(1, totalSteps)) * 100}%` }}
                />
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Steps Table */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Steps</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="border rounded overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead className="bg-muted">
                  <tr>
                    <th className="text-left p-2">Step Key</th>
                    <th className="text-left p-2">Status</th>
                    <th className="text-left p-2">Payload</th>
                    <th className="text-left p-2">Result</th>
                    <th className="text-left p-2">Duration</th>
                  </tr>
                </thead>
                <tbody>
                  {stepsSlice.map((step) => {
                    const duration = step.started_at && step.completed_at
                      ? new Date(step.completed_at).getTime() - new Date(step.started_at).getTime()
                      : null;
                    return (
                      <tr key={step.id} className="border-t">
                        <td className="p-2 font-mono text-xs">{step.step_key}</td>
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
                              aria-label="Expand payload"
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
                    );
                  })}
                </tbody>
              </table>
            </div>
          </div>
          <StepsPagination page={stepsPage} totalPages={stepsTotalPages} onPageChange={setStepsPage} />
        </CardContent>
      </Card>

      {/* Logs by Step */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Execution Logs</CardTitle>
        </CardHeader>
        <CardContent>
          <LogsByStep logs={logs} steps={steps} logFilter={logFilter} setLogFilter={setLogFilter} />
        </CardContent>
      </Card>
    </div>
  );
}
