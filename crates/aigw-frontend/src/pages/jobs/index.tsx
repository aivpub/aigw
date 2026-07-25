import {
  fetchJobStats,
  fetchJobs,
  fetchArchiveStats,
  stepTypeLabel,
  formatCount,
  displayJobStatus,
} from "@/lib/api/jobs";
import type { JobItem, JobStats, ArchiveStats } from "@/lib/api/jobs";
import { useState, useEffect, useCallback } from "react";
import { useSearchParams, useNavigate } from "react-router-dom";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { TriggerDialog } from "./components/trigger-dialog";
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

// ── Inline Pagination ──
function ListPagination({
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
    <nav role="navigation" aria-label="pagination" className="flex items-center gap-1">
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

// ── Job List (shared table) ──
function JobListTable({
  jobs,
  loading,
  total,
  page,
  limit,
  onPageChange,
  onJobClick,
}: {
  jobs: JobItem[];
  loading: boolean;
  total: number;
  page: number;
  limit: number;
  onPageChange: (p: number) => void;
  onJobClick: (id: string) => void;
}) {
  const totalPages = Math.max(1, Math.ceil(total / limit));

  if (loading) return <Skeleton className="h-40 w-full" />;

  return (
    <div className="space-y-2">
      {jobs.length === 0 ? (
        <p className="text-muted-foreground text-center py-4">No jobs found.</p>
      ) : (
        <div className="border rounded overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-muted">
              <tr>
                <th className="text-left p-2">ID</th>
                <th className="text-left p-2">Step Type</th>
                <th className="text-left p-2">Trigger</th>
                <th className="text-left p-2">Status</th>
                <th className="text-left p-2">Progress</th>
                <th className="text-left p-2">Created</th>
              </tr>
            </thead>
            <tbody>
              {jobs.map((job) => {
                const ds = displayJobStatus(job.status);
                return (
                  <tr
                    key={job.id}
                    className="border-t cursor-pointer hover:bg-accent transition-colors"
                    onClick={() => onJobClick(job.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        onJobClick(job.id);
                      }
                    }}
                    tabIndex={0}
                    role="button"
                    aria-label={`Job ${job.id.slice(0, 12)}`}
                  >
                    <td className="p-2 font-mono text-xs truncate max-w-[120px]" title={job.id}>
                      {job.id.length > 20 ? `${job.id.slice(0, 20)}...` : job.id}
                    </td>
                    <td className="p-2">{stepTypeLabel(job.step_type)}</td>
                    <td className="p-2">{job.trigger_type}</td>
                    <td className="p-2"><StatusBadge status={ds} /></td>
                    <td className="p-2 text-xs">
                      {job.completed_steps}/{job.total_steps}
                      {job.failed_steps > 0 && (
                        <span className="text-red-500 ml-1">({job.failed_steps} failed)</span>
                      )}
                    </td>
                    <td className="p-2 text-xs text-muted-foreground">
                      {new Date(job.created_at).toLocaleString()}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span>{total} jobs total</span>
        <ListPagination page={page} totalPages={totalPages} onPageChange={onPageChange} />
      </div>
    </div>
  );
}

// ── Archive Stats Card ──
function ArchiveStatsCard({ archiveStats }: { archiveStats: ArchiveStats }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Archive Overview</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
          <div>
            <div className="text-muted-foreground">Status</div>
            <Badge variant={archiveStats.archive_enabled ? "default" : "secondary"}>
              <span className={`mr-1 inline-block w-2 h-2 rounded-full ${archiveStats.archive_enabled ? "bg-green-500" : "bg-gray-400"}`} />
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
            <div className="text-muted-foreground">Storage</div>
            <div className="font-medium">
              {archiveStats.archive_enabled ? "Configured" : "Not configured"}
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

// ── Budget Reset Placeholder ──
function BudgetResetPlaceholder() {
  return (
    <Card>
      <CardContent className="py-12">
        <p className="text-muted-foreground text-center">No jobs yet</p>
      </CardContent>
    </Card>
  );
}

// ── Overview Stats Cards ──
function OverviewCards({
  stepTypes,
  stats,
  onSelectTab,
}: {
  stepTypes: string[];
  stats: JobStats | null;
  onSelectTab: (st: string) => void;
}) {
  if (!stepTypes.length) {
    return (
      <p className="text-muted-foreground col-span-3 text-center py-8">
        No jobs registered. Start the engine to see stats.
      </p>
    );
  }
  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
      {stepTypes.map((st) => {
        const s = stats![st];
        return (
          <Card
            key={st}
            className="cursor-pointer hover:bg-accent/50 transition-colors"
            onClick={() => onSelectTab(st)}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelectTab(st);
              }
            }}
            aria-label={`${stepTypeLabel(st)} stats`}
          >
            <CardHeader>
              <CardTitle className="text-base">{stepTypeLabel(st)}</CardTitle>
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
    </div>
  );
}

// ── Main Jobs Page Component ──
export function JobsPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const navigate = useNavigate();

  // Parse URL params
  const tab = searchParams.get("tab") || "overview";
  const page = Math.max(1, parseInt(searchParams.get("page") || "1", 10) || 1);
  const statusFilter = searchParams.get("status") || "all";

  // State
  const [stats, setStats] = useState<JobStats | null>(null);
  const [jobs, setJobs] = useState<JobItem[]>([]);
  const [totalJobs, setTotalJobs] = useState(0);
  const [archiveStats, setArchiveStats] = useState<ArchiveStats | null>(null);
  const [loading, setLoading] = useState(false);
  const [triggerOpen, setTriggerOpen] = useState(false);

  const stepTypes = stats ? Object.keys(stats) : [];
  const limit = 50;

  // URL helpers
  const setUrlParam = useCallback(
    (updates: Record<string, string | null>) => {
      const next = new URLSearchParams(searchParams);
      for (const [key, value] of Object.entries(updates)) {
        if (value === null || value === "" || value === "1") {
          next.delete(key);
        } else {
          next.set(key, value);
        }
      }
      setSearchParams(next, { replace: true });
    },
    [searchParams, setSearchParams]
  );

  // Navigate to detail
  const goToDetail = useCallback(
    (jobId: string) => {
      navigate(`/dash/jobs/${jobId}`);
    },
    [navigate]
  );

  // Fetch functions
  const loadStats = useCallback(async () => {
    try {
      const data = await fetchJobStats();
      setStats(data);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(`Failed to load stats: ${msg}`);
    }
  }, []);

  const loadJobs = useCallback(async () => {
    setLoading(true);
    try {
      const params: Record<string, string> = {};
      if (tab !== "overview") params.step_type = tab;
      if (statusFilter !== "all") params.status = statusFilter;
      const data = await fetchJobs({ ...params, page, limit });
      setJobs(data.jobs || []);
      setTotalJobs(data.total || (data.jobs || []).length);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(`Failed to load jobs: ${msg}`);
    }
    setLoading(false);
  }, [tab, statusFilter, page, limit]);

  const loadArchiveStats = useCallback(async () => {
    try {
      const data = await fetchArchiveStats();
      setArchiveStats(data);
    } catch {
      // archive may not be configured — silently ignore
    }
  }, []);

  // Auto-refresh
  useEffect(() => {
    loadStats();
    loadJobs();
    loadArchiveStats();
    const timer = setInterval(() => {
      loadStats();
      loadJobs();
      loadArchiveStats();
    }, 30000);
    return () => clearInterval(timer);
  }, [loadStats, loadJobs, loadArchiveStats]);

  // Re-fetch when tab/status/page changes
  useEffect(() => {
    loadJobs();
  }, [tab, statusFilter, page]);

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Jobs</h1>

      <Tabs
        defaultValue="overview"
        value={tab}
        onValueChange={(v) => {
          setUrlParam({ tab: v, page: null, status: null });
        }}
      >
        <div className="flex items-center justify-between">
          <TabsList className="overflow-x-auto">
            <TabsTrigger value="overview">Overview</TabsTrigger>
            {stepTypes.map((st) => (
              <TabsTrigger key={st} value={st}>
                {stepTypeLabel(st)}
              </TabsTrigger>
            ))}
          </TabsList>
          {/* Trigger button in same row as tabs (Q5) */}
          {tab === "body_archive" && (
            <Button
              onClick={() => setTriggerOpen(true)}
              disabled={archiveStats !== null && !archiveStats.archive_enabled}
              title={archiveStats && !archiveStats.archive_enabled ? "Archive is disabled" : "Trigger Archive"}
              size="sm"
            >
              Trigger Archive
            </Button>
          )}
        </div>

        {/* Overview Tab */}
        <TabsContent value="overview" className="space-y-4 mt-4">
          <OverviewCards stepTypes={stepTypes} stats={stats} onSelectTab={(st) => setUrlParam({ tab: st })} />

          <Card>
            <CardHeader>
              <div className="flex items-center justify-between">
                <CardTitle className="text-base">Recent Jobs</CardTitle>
                <Select value={statusFilter} onValueChange={(v) => setUrlParam({ status: v === "all" ? null : v, page: null })}>
                  <SelectTrigger className="w-32">
                    <SelectValue placeholder="Status" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">All</SelectItem>
                    <SelectItem value="pending">Pending</SelectItem>
                    <SelectItem value="running">Running</SelectItem>
                    <SelectItem value="completed">Completed</SelectItem>
                    <SelectItem value="failed">Failed</SelectItem>
                    <SelectItem value="partially_failed">Partial</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </CardHeader>
            <CardContent>
              <JobListTable
                jobs={jobs}
                loading={loading}
                total={totalJobs}
                page={page}
                limit={limit}
                onPageChange={(p) => setUrlParam({ page: p > 1 ? String(p) : null })}
                onJobClick={goToDetail}
              />
            </CardContent>
          </Card>
        </TabsContent>

        {/* Per-type Tabs */}
        {stepTypes.map((st) => (
          <TabsContent key={st} value={st} className="space-y-4 mt-4">
            {/* body_archive stats */}
            {st === "body_archive" && archiveStats && (
              <ArchiveStatsCard archiveStats={archiveStats} />
            )}

            {/* Budget Reset placeholder */}
            {st === "budget_reset" && <BudgetResetPlaceholder />}

            {/* Filtered Jobs */}
            <Card>
              <CardHeader>
                <div className="flex items-center justify-between">
                  <CardTitle className="text-base">{stepTypeLabel(st)} Jobs</CardTitle>
                  <Select value={statusFilter} onValueChange={(v) => setUrlParam({ status: v === "all" ? null : v, page: null })}>
                    <SelectTrigger className="w-32">
                      <SelectValue placeholder="Status" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">All</SelectItem>
                      <SelectItem value="pending">Pending</SelectItem>
                      <SelectItem value="running">Running</SelectItem>
                      <SelectItem value="completed">Completed</SelectItem>
                      <SelectItem value="failed">Failed</SelectItem>
                      <SelectItem value="partially_failed">Partial</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </CardHeader>
              <CardContent>
                <JobListTable
                  jobs={jobs}
                  loading={loading}
                  total={totalJobs}
                  page={page}
                  limit={limit}
                  onPageChange={(p) => setUrlParam({ page: p > 1 ? String(p) : null })}
                  onJobClick={goToDetail}
                />
              </CardContent>
            </Card>
          </TabsContent>
        ))}
      </Tabs>

      {/* Trigger Dialog */}
      <TriggerDialog
        open={triggerOpen}
        onOpenChange={setTriggerOpen}
        onSuccess={() => {
          loadJobs();
          loadStats();
        }}
      />
    </div>
  );
}
