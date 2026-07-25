import { useState, useEffect, useCallback } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Skeleton } from "@/components/ui/skeleton";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { toast } from "sonner";

interface JobStats {
  [stepType: string]: {
    queue: { pending: number; running: number; completed: number; failed: number };
  };
}

interface JobItem {
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

interface StepItem {
  id: string;
  step_key: string;
  status: string;
  payload: any;
  result: any;
  error_message: string | null;
  retry_count: number;
  started_at: string | null;
  completed_at: string | null;
}

interface LogEntry {
  step_key: string | null;
  level: string;
  message: string;
  created_at: string;
}

interface ArchiveStats {
  total_archived_rows: number;
  pending_rows: number;
  archive_enabled: boolean;
}

const API = "/admin";

async function fetchJson(path: string) {
  const resp = await fetch(`${API}${path}`);
  if (!resp.ok) throw new Error(`${resp.status} ${resp.statusText}`);
  return resp.json();
}

function StatusBadge({ status }: { status: string }) {
  const colors: Record<string, string> = {
    pending: "bg-yellow-500/10 text-yellow-500 border-yellow-500/20",
    running: "bg-blue-500/10 text-blue-500 border-blue-500/20",
    completed: "bg-green-500/10 text-green-500 border-green-500/20",
    failed: "bg-red-500/10 text-red-500 border-red-500/20",
  };
  return (
    <Badge variant="outline" className={colors[status] || "bg-muted"}>
      {status}
    </Badge>
  );
}

function formatCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}

export function JobsPage() {
  const [tab, setTab] = useState("overview");
  const [stats, setStats] = useState<JobStats | null>(null);
  const [jobs, setJobs] = useState<JobItem[]>([]);
  const [archiveStats, setArchiveStats] = useState<ArchiveStats | null>(null);
  const [selectedJob, setSelectedJob] = useState<string | null>(null);
  const [jobDetail, setJobDetail] = useState<any | null>(null);
  const [jobLogs, setJobLogs] = useState<LogEntry[]>([]);
  const [logFilter, setLogFilter] = useState("all");
  const [loading, setLoading] = useState(false);
  const [statusFilter, setStatusFilter] = useState("all");

  // Trigger dialog state
  const [triggerOpen, setTriggerOpen] = useState(false);
  const [triggerForm, setTriggerForm] = useState(() => {
    const now = new Date();
    const end = now.toISOString().slice(0, 16);
    const start = new Date(now.getTime() - 24 * 60 * 60 * 1000).toISOString().slice(0, 16);
    return { start_date: start, end_date: end, batch_size: "5000" };
  });

  const stepTypes = stats ? Object.keys(stats) : [];

  const loadStats = useCallback(async () => {
    try {
      const data = await fetchJson("/jobs/stats");
      setStats(data);
    } catch { /* silently fail */ }
  }, []);

  const loadJobs = useCallback(async () => {
    setLoading(true);
    try {
      const params = new URLSearchParams();
      params.set("limit", "50");
      if (tab !== "overview" && stepTypes.includes(tab)) {
        params.set("step_type", tab);
      }
      if (statusFilter !== "all") {
        params.set("status", statusFilter);
      }
      const data = await fetchJson(`/jobs?${params.toString()}`);
      setJobs(data.jobs || []);
    } catch { /* silently fail */ }
    setLoading(false);
  }, [tab, statusFilter]);

  const loadArchiveStats = useCallback(async () => {
    try {
      const data = await fetchJson("/archive/stats");
      setArchiveStats(data);
    } catch { /* silently fail */ }
  }, []);

  const loadJobDetail = useCallback(async (jobId: string) => {
    try {
      const data = await fetchJson(`/jobs/${jobId}`);
      setJobDetail(data);
      setSelectedJob(jobId);
    } catch { /* silently fail */ }
  }, []);

  const loadJobLogs = useCallback(async (jobId: string) => {
    try {
      const levelParam = logFilter !== "all" ? `&level=${logFilter}` : "";
      const data = await fetchJson(`/jobs/${jobId}/logs?limit=50${levelParam}`);
      setJobLogs(data.logs || []);
    } catch { /* silently fail */ }
  }, [logFilter]);

  useEffect(() => {
    loadStats();
    loadJobs();
    loadArchiveStats();
    const timer = setInterval(() => { loadStats(); loadJobs(); loadArchiveStats(); }, 30000);
    return () => clearInterval(timer);
  }, [loadStats, loadJobs, loadArchiveStats]);

  // Refresh jobs when tab or filter changes
  useEffect(() => {
    if (tab) loadJobs();
  }, [tab, statusFilter]);

  useEffect(() => {
    if (selectedJob) {
      loadJobDetail(selectedJob);
      loadJobLogs(selectedJob);
    }
  }, [selectedJob, logFilter, loadJobDetail, loadJobLogs]);

  // Auto-refresh detail for running jobs
  useEffect(() => {
    if (!selectedJob || !jobDetail || jobDetail.job.status !== "running") return;
    const timer = setInterval(() => {
      loadJobDetail(selectedJob);
      loadJobLogs(selectedJob);
    }, 10000);
    return () => clearInterval(timer);
  }, [selectedJob, jobDetail?.job.status, loadJobDetail, loadJobLogs]);

  const handleTrigger = async () => {
    try {
      const startDate = new Date(triggerForm.start_date).toISOString();
      const endDate = new Date(triggerForm.end_date).toISOString();
      const batchSize = parseInt(triggerForm.batch_size, 10) || 5000;

      const resp = await fetch(`${API}/jobs/trigger`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          step_type: "body_archive",
          payload: { start_date: startDate, end_date: endDate, batch_size: batchSize },
        }),
      });

      if (!resp.ok) {
        const err = await resp.json().catch(() => ({ error: `${resp.status}` }));
        throw new Error(err.error || `${resp.status}`);
      }

      const data = await resp.json();
      toast.success(`Job ${data.job_id.slice(0, 12)}... created with ${data.total_steps} steps`);
      setTriggerOpen(false);
      loadJobs();
      loadStats();
    } catch (e: any) {
      toast.error(`Trigger failed: ${e.message}`);
    }
  };

  const filteredJobs = jobs;

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Jobs</h1>

      <Tabs defaultValue="overview" value={tab} onValueChange={setTab}>
        <TabsList className="overflow-x-auto">
          <TabsTrigger value="overview">Overview</TabsTrigger>
          {stepTypes.map(st => (
            <TabsTrigger key={st} value={st}>{st}</TabsTrigger>
          ))}
        </TabsList>

        {/* ── Overview Tab ── */}
        <TabsContent value="overview" className="space-y-4">
          {/* Stats cards */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            {stepTypes.map(st => {
              const s = stats![st];
              return (
                <Card
                  key={st}
                  className="cursor-pointer hover:bg-accent/50 transition-colors"
                  onClick={() => setTab(st)}
                >
                  <CardHeader>
                    <CardTitle className="text-base">{st}</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <div className="grid grid-cols-2 gap-2 text-sm">
                      <div className="flex justify-between"><span>Pending</span><Badge variant="secondary">{s.queue.pending}</Badge></div>
                      <div className="flex justify-between"><span>Running</span><Badge variant="secondary">{s.queue.running}</Badge></div>
                      <div className="flex justify-between"><span>Completed</span><Badge variant="secondary">{s.queue.completed}</Badge></div>
                      <div className="flex justify-between"><span>Failed</span><Badge variant="secondary">{s.queue.failed}</Badge></div>
                    </div>
                  </CardContent>
                </Card>
              );
            })}
            {stepTypes.length === 0 && (
              <p className="text-muted-foreground col-span-3 text-center py-8">
                No jobs registered. Start the engine to see stats.
              </p>
            )}
          </div>

          {/* Job History with filter */}
          <Card>
            <CardHeader>
              <div className="flex items-center justify-between">
                <CardTitle className="text-base">Recent Jobs</CardTitle>
                <Select value={statusFilter} onValueChange={setStatusFilter}>
                  <SelectTrigger className="w-32">
                    <SelectValue placeholder="Status" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">All</SelectItem>
                    <SelectItem value="pending">Pending</SelectItem>
                    <SelectItem value="running">Running</SelectItem>
                    <SelectItem value="completed">Completed</SelectItem>
                    <SelectItem value="failed">Failed</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </CardHeader>
            <CardContent>
              {loading ? (
                <Skeleton className="h-40 w-full" />
              ) : (
                <div className="space-y-2">
                  {filteredJobs.map(job => (
                    <div
                      key={job.id}
                      className="flex items-center justify-between border rounded p-3 cursor-pointer hover:bg-accent"
                      onClick={() => { setSelectedJob(job.id); setTab("detail"); }}
                    >
                      <div className="flex-1 min-w-0">
                        <div className="text-sm font-medium truncate">{job.id}</div>
                        <div className="text-xs text-muted-foreground">{job.step_type} · {job.trigger_type} · {new Date(job.created_at).toLocaleString()}</div>
                      </div>
                      <div className="flex items-center gap-3">
                        <div className="text-xs text-muted-foreground">
                          {job.completed_steps}/{job.total_steps} steps
                        </div>
                        <StatusBadge status={job.status} />
                      </div>
                    </div>
                  ))}
                  {filteredJobs.length === 0 && (
                    <p className="text-muted-foreground text-center py-4">No jobs found.</p>
                  )}
                </div>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        {/* ── Per-type Tabs (dynamic) ── */}
        {stepTypes.map(st => (
          <TabsContent key={st} value={st} className="space-y-4">
            {/* body_archive specific stats */}
            {st === "body_archive" && archiveStats && (
              <Card>
                <CardHeader>
                  <CardTitle className="text-base">Archive Overview</CardTitle>
                </CardHeader>
                <CardContent>
                  <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
                    <div>
                      <div className="text-muted-foreground">Status</div>
                      <Badge variant={archiveStats.archive_enabled ? "default" : "secondary"}>
                        {archiveStats.archive_enabled ? "Enabled" : "Disabled"}
                      </Badge>
                    </div>
                    <div>
                      <div className="text-muted-foreground">Archived Rows</div>
                      <div className="font-medium">{formatCount(archiveStats.total_archived_rows)}</div>
                    </div>
                    <div>
                      <div className="text-muted-foreground">Pending Rows</div>
                      <div className="font-medium">{formatCount(archiveStats.pending_rows)}</div>
                    </div>
                    <div>
                      <div className="text-muted-foreground">Active Jobs</div>
                      <div className="font-medium">{filteredJobs.filter(j => j.step_type === "body_archive" && (j.status === "pending" || j.status === "running")).length}</div>
                    </div>
                  </div>
                </CardContent>
              </Card>
            )}

            {/* Trigger button */}
            <Card>
              <CardHeader>
                <div className="flex items-center justify-between">
                  <CardTitle className="text-base">
                    {st === "body_archive" ? "Manual Archive" : "Manual Trigger"}
                  </CardTitle>
                  <Button onClick={() => setTriggerOpen(true)}>
                    Trigger {st}
                  </Button>
                </div>
              </CardHeader>
            </Card>

            {/* Filtered Jobs for this type */}
            <Card>
              <CardHeader>
                <div className="flex items-center justify-between">
                  <CardTitle className="text-base">Recent {st} Jobs</CardTitle>
                  <Select value={statusFilter} onValueChange={setStatusFilter}>
                    <SelectTrigger className="w-32">
                      <SelectValue placeholder="Status" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">All</SelectItem>
                      <SelectItem value="pending">Pending</SelectItem>
                      <SelectItem value="running">Running</SelectItem>
                      <SelectItem value="completed">Completed</SelectItem>
                      <SelectItem value="failed">Failed</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </CardHeader>
              <CardContent>
                {loading ? (
                  <Skeleton className="h-40 w-full" />
                ) : (
                  <div className="space-y-2">
                    {filteredJobs.map(job => (
                      <div
                        key={job.id}
                        className="flex items-center justify-between border rounded p-3 cursor-pointer hover:bg-accent"
                        onClick={() => { setSelectedJob(job.id); setTab("detail"); }}
                      >
                        <div className="flex-1 min-w-0">
                          <div className="text-sm font-medium truncate">{job.id}</div>
                          <div className="text-xs text-muted-foreground">{job.trigger_type} · {new Date(job.created_at).toLocaleString()}</div>
                        </div>
                        <div className="flex items-center gap-3">
                          <div className="text-xs text-muted-foreground">
                            {job.completed_steps}/{job.total_steps} steps
                          </div>
                          <StatusBadge status={job.status} />
                        </div>
                      </div>
                    ))}
                    {filteredJobs.length === 0 && (
                      <p className="text-muted-foreground text-center py-4">No {st} jobs found.</p>
                    )}
                  </div>
                )}
              </CardContent>
            </Card>
          </TabsContent>
        ))}
      </Tabs>

      {/* ── Job Detail ── */}
      {selectedJob && tab === "detail" && (
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <CardTitle className="text-base flex items-center gap-2">
                Job: {selectedJob.substring(0, 20)}...
                {jobDetail && <StatusBadge status={jobDetail.job.status} />}
              </CardTitle>
              <Button variant="ghost" size="sm" onClick={() => { setSelectedJob(null); setTab("overview"); }}>
                ← Back
              </Button>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            {/* Summary */}
            {jobDetail && (
              <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
                <div><span className="text-muted-foreground">Trigger:</span> {jobDetail.job.trigger_type}</div>
                <div><span className="text-muted-foreground">Total:</span> {jobDetail.job.total_steps}</div>
                <div><span className="text-muted-foreground">Completed:</span> {jobDetail.summary.completed}</div>
                <div><span className="text-muted-foreground">Failed:</span> {jobDetail.summary.failed}</div>
              </div>
            )}

            {/* Progress bar */}
            {jobDetail && jobDetail.job.total_steps > 0 && (
              <div className="space-y-1">
                <div className="flex justify-between text-xs text-muted-foreground">
                  <span>Progress</span>
                  <span>{jobDetail.job.completed_steps}/{jobDetail.job.total_steps}</span>
                </div>
                <div className="w-full bg-muted rounded-full h-2 overflow-hidden">
                  <div className="bg-green-500 h-2 transition-all" style={{ width: `${(jobDetail.job.completed_steps / jobDetail.job.total_steps) * 100}%` }} />
                </div>
              </div>
            )}

            {/* Steps Table */}
            {jobDetail && (
              <div className="border rounded overflow-hidden">
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead className="bg-muted">
                      <tr>
                        <th className="text-left p-2">Step Key</th>
                        <th className="text-left p-2">Status</th>
                        <th className="text-left p-2">Retries</th>
                        <th className="text-left p-2">Error</th>
                      </tr>
                    </thead>
                    <tbody>
                      {jobDetail.steps.map((step: StepItem) => (
                        <tr key={step.id} className="border-t">
                          <td className="p-2 font-mono text-xs">{step.step_key}</td>
                          <td className="p-2"><StatusBadge status={step.status} /></td>
                          <td className="p-2">{step.retry_count}</td>
                          <td className="p-2 text-xs text-red-500 max-w-[200px] truncate">
                            {step.error_message || "-"}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}

            {/* Logs */}
            <div>
              <div className="flex items-center justify-between mb-2">
                <h3 className="text-sm font-medium">Logs</h3>
                <div className="flex gap-2">
                  {["all", "info", "warn", "error"].map(l => (
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
              <div className="border rounded overflow-hidden max-h-80 overflow-y-auto">
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead className="bg-muted">
                      <tr>
                        <th className="text-left p-2 w-16">Level</th>
                        <th className="text-left p-2">Message</th>
                        <th className="text-left p-2 w-40">Time</th>
                      </tr>
                    </thead>
                    <tbody>
                      {jobLogs.map((log, i) => (
                        <tr key={i} className="border-t">
                          <td className="p-2">
                            <Badge variant={log.level === "error" ? "destructive" : log.level === "warn" ? "secondary" : "outline"}>
                              {log.level}
                            </Badge>
                          </td>
                          <td className="p-2 font-mono text-xs">{log.message}</td>
                          <td className="p-2 text-xs text-muted-foreground">
                            {new Date(log.created_at).toLocaleTimeString()}
                          </td>
                        </tr>
                      ))}
                      {jobLogs.length === 0 && (
                        <tr>
                          <td colSpan={3} className="p-4 text-center text-muted-foreground">
                            No logs found.
                          </td>
                        </tr>
                      )}
                    </tbody>
                  </table>
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
      )}

      {/* ── Trigger Dialog ── */}
      <Dialog open={triggerOpen} onOpenChange={setTriggerOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Trigger Body Archive</DialogTitle>
            <DialogDescription>
              Archive spend_logs body data for a specific date range. Each hour of data becomes one job step.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="start-date">Start Date</Label>
              <Input
                id="start-date"
                type="datetime-local"
                value={triggerForm.start_date}
                onChange={e => setTriggerForm({ ...triggerForm, start_date: e.target.value })}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="end-date">End Date</Label>
              <Input
                id="end-date"
                type="datetime-local"
                value={triggerForm.end_date}
                onChange={e => setTriggerForm({ ...triggerForm, end_date: e.target.value })}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="batch-size">Batch Size</Label>
              <Input
                id="batch-size"
                type="number"
                min={100}
                max={50000}
                value={triggerForm.batch_size}
                onChange={e => setTriggerForm({ ...triggerForm, batch_size: e.target.value })}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setTriggerOpen(false)}>
              Cancel
            </Button>
            <Button onClick={handleTrigger}>Trigger Job</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
