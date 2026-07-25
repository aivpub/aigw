import { useState, useEffect, useCallback } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Skeleton } from "@/components/ui/skeleton";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

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

export function JobsPage() {
  const [tab, setTab] = useState("overview");
  const [stats, setStats] = useState<JobStats | null>(null);
  const [jobs, setJobs] = useState<JobItem[]>([]);
  const [selectedJob, setSelectedJob] = useState<string | null>(null);
  const [jobDetail, setJobDetail] = useState<any | null>(null);
  const [jobLogs, setJobLogs] = useState<LogEntry[]>([]);
  const [logFilter, setLogFilter] = useState("all");
  const [loading, setLoading] = useState(false);
  const [triggerPayload, setTriggerPayload] = useState(
    '{"start_date":"2026-07-22T00:00:00+08:00","end_date":"2026-07-23T00:00:00+08:00"}'
  );

  const loadStats = useCallback(async () => {
    try {
      const data = await fetchJson("/jobs/stats");
      setStats(data);
    } catch (e) { /* silently fail */ }
  }, []);

  const loadJobs = useCallback(async () => {
    setLoading(true);
    try {
      const data = await fetchJson("/jobs?limit=50");
      setJobs(data.jobs || []);
    } catch (e) { /* silently fail */ }
    setLoading(false);
  }, []);

  const loadJobDetail = useCallback(async (jobId: string) => {
    try {
      const data = await fetchJson(`/jobs/${jobId}`);
      setJobDetail(data);
      setSelectedJob(jobId);
    } catch (e) { /* silently fail */ }
  }, []);

  const loadJobLogs = useCallback(async (jobId: string) => {
    try {
      const levelParam = logFilter !== "all" ? `&level=${logFilter}` : "";
      const data = await fetchJson(`/jobs/${jobId}/logs?limit=50${levelParam}`);
      setJobLogs(data.logs || []);
    } catch (e) { /* silently fail */ }
  }, [logFilter]);

  useEffect(() => {
    loadStats();
    loadJobs();
    const timer = setInterval(() => { loadStats(); loadJobs(); }, 30000);
    return () => clearInterval(timer);
  }, [loadStats, loadJobs]);

  useEffect(() => {
    if (selectedJob) {
      loadJobDetail(selectedJob);
      loadJobLogs(selectedJob);
    }
  }, [selectedJob, logFilter, loadJobDetail, loadJobLogs]);

  const triggerJob = async () => {
    try {
      const payload = JSON.parse(triggerPayload);
      const resp = await fetch(`${API}/jobs/trigger`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ step_type: "body_archive", payload }),
      });
      if (!resp.ok) throw new Error(`${resp.status}`);
      const data = await resp.json();
      alert(`Job created: ${data.job_id} (${data.total_steps} steps)`);
      loadJobs();
    } catch (e: any) {
      alert(`Trigger failed: ${e.message}`);
    }
  };

  const stepTypes = stats ? Object.keys(stats) : [];

  return (
    <div className="space-y-6 p-6">
      <h1 className="text-2xl font-bold">Jobs</h1>

      <Tabs defaultValue="overview" value={tab} onValueChange={setTab}>
        <TabsList>
          <TabsTrigger value="overview">Overview</TabsTrigger>
          {stepTypes.map(st => (
            <TabsTrigger key={st} value={st}>{st}</TabsTrigger>
          ))}
        </TabsList>

        {/* Overview Tab */}
        <TabsContent value="overview" className="space-y-4">
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            {stepTypes.map(st => {
              const s = stats![st];
              return (
                <Card key={st}>
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
                No jobs registered.
              </p>
            )}
          </div>

          {/* Manual Trigger */}
          <Card>
            <CardHeader><CardTitle className="text-base">Manual Trigger</CardTitle></CardHeader>
            <CardContent className="space-y-3">
              <div className="space-y-1">
                <Label>Payload (JSON)</Label>
                <textarea
                  className="w-full min-h-[80px] font-mono text-sm border rounded p-2"
                  value={triggerPayload}
                  onChange={e => setTriggerPayload(e.target.value)}
                />
              </div>
              <Button onClick={triggerJob}>Trigger body_archive</Button>
            </CardContent>
          </Card>

          {/* Job History */}
          <Card>
            <CardHeader><CardTitle className="text-base">Recent Jobs</CardTitle></CardHeader>
            <CardContent>
              {loading ? (
                <Skeleton className="h-40 w-full" />
              ) : (
                <div className="space-y-2">
                  {jobs.map(job => (
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
                  {jobs.length === 0 && (
                    <p className="text-muted-foreground text-center py-4">No jobs found.</p>
                  )}
                </div>
              )}
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>

      {/* Job Detail Modal */}
      {selectedJob && tab === "detail" && (
        <Card className="mt-4">
          <CardHeader>
            <div className="flex items-center justify-between">
              <CardTitle className="text-base">
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
              <div className="grid grid-cols-4 gap-4 text-sm">
                <div><span className="text-muted-foreground">Trigger:</span> {jobDetail.job.trigger_type}</div>
                <div><span className="text-muted-foreground">Total Steps:</span> {jobDetail.job.total_steps}</div>
                <div><span className="text-muted-foreground">Completed:</span> {jobDetail.summary.completed}</div>
                <div><span className="text-muted-foreground">Failed:</span> {jobDetail.summary.failed}</div>
              </div>
            )}

            {/* Steps Table */}
            {jobDetail && (
              <div className="border rounded overflow-hidden">
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
          </CardContent>
        </Card>
      )}
    </div>
  );
}
