import { useTranslation } from "react-i18next";
import {
  fetchJobStats,
  fetchJobs,
  fetchArchiveStats,
  fetchBudgetResetStats,
  stepTypeLabel,
  triggerTypeLabel,
  formatCount,
  displayJobStatus,
  KNOWN_STEP_TYPES,
} from "@/lib/api/jobs";
import type {
  JobItem,
  JobStats,
  ArchiveStats,
  BudgetResetStats,
} from "@/lib/api/jobs";
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
import { ChevronLeft, ChevronRight, RefreshCw } from "lucide-react";
import { TriggerDialog } from "./components/trigger-dialog";
import { BudgetResetTriggerDialog } from "./components/budget-reset-trigger-dialog";
import { BudgetResetStatsCard } from "./components/budget-reset-stats-card";
import { BudgetResetPreview } from "./components/budget-reset-preview";
import { toast } from "sonner";

// ── Status Badge ──
function StatusBadge({
  status,
  className,
}: {
  status: string;
  className?: string;
}) {
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
      {status === "running" && (
        <span className="mr-1 inline-block w-2 h-2 rounded-full bg-blue-500 animate-pulse" />
      )}
      {status === "pending"
        ? t("jobs.status.pending")
        : status === "running"
          ? t("jobs.status.running")
          : status === "completed"
            ? t("jobs.status.completed")
            : status === "failed"
              ? t("jobs.status.failed")
              : status === "partially_failed"
                ? t("jobs.status.partiallyFailed")
                : status}
    </Badge>
  );
}

// ── PaginationBar (SpendLogs-style: prev/next + page size + showing X-Y of Z) ──
function PaginationBar({
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
    <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-2">
      <div className="flex items-center gap-3">
        <span className="text-xs text-muted-foreground">
          {_t("pagination.showing", { from, to, total: totalCount })}
        </span>
        <span className="text-xs text-muted-foreground">
          {_t("pagination.pageInfo", { page, total: Math.max(totalPages, 1) })}
        </span>
      </div>
      <div className="flex items-center gap-2">
        <Select
          value={String(pageSize)}
          onValueChange={(v) => onPageSize(Number(v))}
        >
          <SelectTrigger className="h-7 w-[70px] text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="30">30</SelectItem>
            <SelectItem value="50">50</SelectItem>
            <SelectItem value="100">100</SelectItem>
          </SelectContent>
        </Select>
        <Button
          variant="outline"
          size="sm"
          disabled={page <= 1}
          onClick={() => onPage(page - 1)}
          className="h-7 px-2"
        >
          <ChevronLeft className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={page >= totalPages || totalPages === 0}
          onClick={() => onPage(page + 1)}
          className="h-7 px-2"
        >
          <ChevronRight className="h-3.5 w-3.5" />
        </Button>
      </div>
    </div>
  );
}

// ── Job List (shared table) ──
function JobListTable({
  jobs,
  loading,
  total,
  page,
  limit,
  onPage,
  onPageSize,
  onJobClick,
}: {
  jobs: JobItem[];
  loading: boolean;
  total: number;
  page: number;
  limit: number;
  onPage: (p: number) => void;
  onPageSize: (s: number) => void;
  onJobClick: (id: string) => void;
}) {
  const { t } = useTranslation();
  const totalPages = Math.max(1, Math.ceil(total / limit));

  if (loading) return <Skeleton className="h-40 w-full" />;

  return (
    <div className="space-y-2">
      <PaginationBar
        page={page}
        pageSize={limit}
        totalCount={total}
        totalPages={totalPages}
        onPage={onPage}
        onPageSize={onPageSize}
      />
      {jobs.length === 0 ? (
        <p className="text-muted-foreground text-center py-4">
          {t("jobs.noJobs")}
        </p>
      ) : (
        <div className="border rounded overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-muted">
              <tr>
                <th className="text-left p-2">{t("jobs.table.id")}</th>
                <th className="text-left p-2">{t("jobs.table.stepType")}</th>
                <th className="text-left p-2">{t("jobs.table.trigger")}</th>
                <th className="text-left p-2">{t("jobs.table.status")}</th>
                <th className="text-left p-2">{t("jobs.table.progress")}</th>
                <th className="text-left p-2">{t("jobs.table.created")}</th>
                <th className="text-left p-2">{t("jobs.table.ended")}</th>
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
                    <td
                      className="p-2 font-mono text-xs truncate max-w-[120px]"
                      title={job.id}
                    >
                      {job.id.length > 20
                        ? `${job.id.slice(0, 20)}...`
                        : job.id}
                    </td>
                    <td className="p-2">{stepTypeLabel(job.step_type)}</td>
                    <td className="p-2">{triggerTypeLabel(job.trigger_type)}</td>
                    <td className="p-2">
                      <StatusBadge status={ds} />
                    </td>
                    <td className="p-2 text-xs">
                      {job.completed_steps + job.failed_steps}/{job.total_steps}
                      {job.failed_steps > 0 && (
                        <span className="text-red-500 ml-1">
                          {t("jobs.failedSteps", { count: job.failed_steps })}
                        </span>
                      )}
                    </td>
                    <td className="p-2 text-xs text-muted-foreground">
                      {new Date(job.created_at).toLocaleString()}
                    </td>
                    <td className="p-2 text-xs text-muted-foreground">
                      {["completed", "failed", "partially_failed"].includes(
                        job.status,
                      )
                        ? new Date(job.updated_at).toLocaleString()
                        : "—"}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      <PaginationBar
        page={page}
        pageSize={limit}
        totalCount={total}
        totalPages={totalPages}
        onPage={onPage}
        onPageSize={onPageSize}
      />
    </div>
  );
}

// ── Archive Stats Card ──
function ArchiveStatsCard({ archiveStats }: { archiveStats: ArchiveStats }) {
  const { t } = useTranslation();
  const auto = archiveStats.auto_archive;
  const sc = archiveStats.storage_configured;
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("jobs.archiveOverview")}</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
          <div>
            <div className="text-muted-foreground">{t("jobs.autoArchive")}</div>
            <Badge variant={auto ? "default" : "secondary"}>
              <span
                className={`mr-1 inline-block w-2 h-2 rounded-full ${auto ? "bg-green-500" : "bg-gray-400"}`}
              />
              {auto ? t("jobs.on") : t("jobs.off")}
            </Badge>
          </div>
          <div>
            <div className="text-muted-foreground">{t("jobs.storage")}</div>
            <Badge variant={sc ? "default" : "secondary"}>
              <span
                className={`mr-1 inline-block w-2 h-2 rounded-full ${sc ? "bg-green-500" : "bg-yellow-500"}`}
              />
              {sc ? t("jobs.configured") : t("jobs.notConfigured")}
            </Badge>
          </div>
          <div>
            <div className="text-muted-foreground">
              {t("jobs.archivedRows")}
            </div>
            <div className="font-medium">
              {formatCount(archiveStats.total_archived_rows)}
            </div>
          </div>
          <div>
            <div className="text-muted-foreground">{t("jobs.pendingRows")}</div>
            <div className="font-medium">
              {formatCount(archiveStats.pending_rows)}
            </div>
          </div>
        </div>
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
  const { t } = useTranslation();
  if (!stepTypes.length) {
    return (
      <p className="text-muted-foreground col-span-3 text-center py-8">
        {t("jobs.noJobsRegistered")}
      </p>
    );
  }
  // stepTypes may include known types seeded before stats load (or when the DB has no jobs yet),
  // so stats[st] can be undefined. Fall back to a zero-valued queue so the cards still render.
  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
      {stepTypes.map((st) => {
        const s = stats?.[st]?.queue ?? {
          pending: 0,
          running: 0,
          completed: 0,
          failed: 0,
        };
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
                <div className="flex justify-between">
                  <span>{t("jobs.status.pending")}</span>
                  <Badge variant="secondary">{s.pending}</Badge>
                </div>
                <div className="flex justify-between">
                  <span>{t("jobs.status.running")}</span>
                  <Badge variant="secondary">{s.running}</Badge>
                </div>
                <div className="flex justify-between">
                  <span>{t("jobs.status.completed")}</span>
                  <Badge variant="secondary">{s.completed}</Badge>
                </div>
                <div className="flex justify-between">
                  <span>{t("jobs.status.failed")}</span>
                  <Badge variant="secondary">{s.failed}</Badge>
                </div>
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
  const { t } = useTranslation();
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
  const [budgetResetStats, setBudgetResetStats] =
    useState<BudgetResetStats | null>(null);
  const [loading, setLoading] = useState(false);
  const [triggerOpen, setTriggerOpen] = useState(false);
  const [budgetResetOpen, setBudgetResetOpen] = useState(false);

  // Budget-reset-specific: always fetch last 10 regardless of status filter
  const [budgetResetJobs, setBudgetResetJobs] = useState<JobItem[]>([]);

  // Tabs represent *registered* async-task types, not just types that have run. The backend
  // `GET /admin/jobs/stats` only reports step_types that already have rows in async_job_steps,
  // so on a fresh DB it returns `{}` and the implemented AsyncTask UI would be invisible. Seed
  // the known types first, then append any extra keys the backend reports (deduped, order-stable).
  const reportedTypes = stats ? Object.keys(stats) : [];
  const stepTypes = Array.from(
    new Set<string>([...KNOWN_STEP_TYPES, ...reportedTypes]),
  );
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
    [searchParams, setSearchParams],
  );

  // Navigate to detail
  const goToDetail = useCallback(
    (jobId: string) => {
      navigate(`/dash/jobs/${jobId}`);
    },
    [navigate],
  );

  // Fetch functions
  const loadStats = useCallback(async () => {
    try {
      const data = await fetchJobStats();
      setStats(data);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(`${t("jobs.toast.loadStatsFailed")}: ${msg}`);
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
      toast.error(`${t("jobs.toast.loadJobsFailed")}: ${msg}`);
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

  const loadBudgetResetJobs = useCallback(async () => {
    try {
      const data = await fetchJobs({
        step_type: "budget_reset",
        limit: 10,
        page: 1,
      });
      setBudgetResetJobs(data.jobs || []);
    } catch {
      // silently ignore
    }
  }, []);

  const loadBudgetResetStats = useCallback(async () => {
    try {
      const data = await fetchBudgetResetStats();
      setBudgetResetStats(data);
    } catch {
      // silently ignore
    }
  }, []);

  // Auto-refresh
  useEffect(() => {
    loadStats();
    loadJobs();
    loadArchiveStats();
    loadBudgetResetJobs();
    loadBudgetResetStats();
    const timer = setInterval(() => {
      loadStats();
      loadJobs();
      loadArchiveStats();
      loadBudgetResetJobs();
      loadBudgetResetStats();
    }, 30000);
    return () => clearInterval(timer);
  }, [
    loadStats,
    loadJobs,
    loadArchiveStats,
    loadBudgetResetJobs,
    loadBudgetResetStats,
  ]);

  // Re-fetch when tab/status/page changes
  useEffect(() => {
    loadJobs();
  }, [tab, statusFilter, page]);

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">{t("jobs.title")}</h1>

      <Tabs
        defaultValue="overview"
        value={tab}
        onValueChange={(v) => {
          setUrlParam({ tab: v, page: null, status: null });
        }}
      >
        <div className="flex items-center justify-between">
          <TabsList className="overflow-x-auto">
            <TabsTrigger value="overview">{t("jobs.overview")}</TabsTrigger>
            {stepTypes.map((st) => (
              <TabsTrigger key={st} value={st}>
                {stepTypeLabel(st)}
              </TabsTrigger>
            ))}
          </TabsList>
          {/* Trigger button in same row as tabs */}
          {tab === "body_archive" && (
            <Button
              onClick={() => setTriggerOpen(true)}
              disabled={
                archiveStats !== null && !archiveStats.storage_configured
              }
              title={
                archiveStats && !archiveStats.storage_configured
                  ? t("jobs.storageNotConfigured")
                  : t("jobs.triggerArchive")
              }
              size="sm"
            >
              {t("jobs.triggerArchive")}
            </Button>
          )}
          {tab === "budget_reset" && (
            <Button
              onClick={() => setBudgetResetOpen(true)}
              size="sm"
            >
              <RefreshCw className="h-3.5 w-3.5 mr-1" />
              {t("jobs.budgetReset.triggerReset")}
            </Button>
          )}
        </div>

        {/* Overview Tab */}
        <TabsContent value="overview" className="space-y-4 mt-4">
          <OverviewCards
            stepTypes={stepTypes}
            stats={stats}
            onSelectTab={(st) => setUrlParam({ tab: st })}
          />

          <Card>
            <CardHeader>
              <div className="flex items-center justify-between">
                <CardTitle className="text-base">{t("jobs.allJobs")}</CardTitle>
                <Select
                  value={statusFilter}
                  onValueChange={(v) =>
                    setUrlParam({ status: v === "all" ? null : v, page: null })
                  }
                >
                  <SelectTrigger className="w-32">
                    <SelectValue placeholder={t("common.status")} />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">
                      {t("spendLogs.filters.all")}
                    </SelectItem>
                    <SelectItem value="pending">
                      {t("jobs.status.pending")}
                    </SelectItem>
                    <SelectItem value="running">
                      {t("jobs.status.running")}
                    </SelectItem>
                    <SelectItem value="completed">
                      {t("jobs.status.completed")}
                    </SelectItem>
                    <SelectItem value="failed">
                      {t("jobs.status.failed")}
                    </SelectItem>
                    <SelectItem value="partially_failed">
                      {t("jobs.status.partiallyFailed")}
                    </SelectItem>
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
                onPage={(p) => setUrlParam({ page: p > 1 ? String(p) : null })}
                onPageSize={() => {}}
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

            {/* Budget Reset hero + upcoming preview */}
            {st === "budget_reset" && (
              <>
                <BudgetResetStatsCard
                  stats={budgetResetStats}
                  jobs={budgetResetJobs}
                />
                <BudgetResetPreview stats={budgetResetStats} />
              </>
            )}

            {/* Filtered Jobs */}
            <Card>
              <CardHeader>
                <div className="flex items-center justify-between">
                  <CardTitle className="text-base">
                    {t("jobs.stepJobs", { type: stepTypeLabel(st) })}
                  </CardTitle>
                  <Select
                    value={statusFilter}
                    onValueChange={(v) =>
                      setUrlParam({
                        status: v === "all" ? null : v,
                        page: null,
                      })
                    }
                  >
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
                  onPage={(p) =>
                    setUrlParam({ page: p > 1 ? String(p) : null })
                  }
                  onPageSize={() => {}}
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

      {/* Budget Reset Trigger Dialog */}
      <BudgetResetTriggerDialog
        open={budgetResetOpen}
        onOpenChange={setBudgetResetOpen}
        stats={budgetResetStats}
        onSuccess={() => {
          loadBudgetResetJobs();
          loadJobs();
          loadStats();
          loadBudgetResetStats();
        }}
      />
    </div>
  );
}
