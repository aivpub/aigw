import { Fragment, useMemo, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiDelete, apiPost } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { PaginationBar } from "@/components/ui/pagination";
import { toast } from "sonner";
import {
  Search,
  Plus,
  Pencil,
  Trash2,
  Activity,
  Gauge,
  Network,
  ChevronDown,
  ChevronRight,
  CheckSquare,
} from "lucide-react";
import type { ProxyItem, ProxyListResponse } from "./types";
import { ProxyDialog } from "./ProxyDialog";
import { QualityDialog } from "./QualityDialog";

function gradeClass(grade: string | null | undefined): string {
  switch (grade) {
    case "A":
      return "bg-green-100 text-green-800";
    case "B":
      return "bg-blue-100 text-blue-800";
    case "C":
      return "bg-yellow-100 text-yellow-800";
    case "D":
      return "bg-orange-100 text-orange-800";
    default:
      return "bg-red-100 text-red-800";
  }
}

export function ProxiesPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState<string>("");
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [selected, setSelected] = useState<Set<number>>(new Set());

  const [dialogOpen, setDialogOpen] = useState(false);
  const [editing, setEditing] = useState<ProxyItem | null>(null);

  const [deleteTarget, setDeleteTarget] = useState<ProxyItem | null>(null);

  const [qualityTarget, setQualityTarget] = useState<{
    result?: {
      score: number;
      grade: string;
      overall_status: string;
      items: {
        target: string;
        status: string;
        latency_ms?: number | null;
        cf_ray?: string | null;
        message: string;
      }[];
      last_check_at: string;
      latency_ms?: number | null;
    } | null;
    name?: string;
  } | null>(null);

  const queryStr = useMemo(() => {
    const params = new URLSearchParams();
    params.set("page", String(page));
    params.set("page_size", String(pageSize));
    if (statusFilter) params.set("status", statusFilter);
    if (search.trim()) params.set("search", search.trim());
    return params.toString();
  }, [page, pageSize, statusFilter, search]);

  const { data, isLoading, error } = useQuery<ProxyListResponse>({
    queryKey: ["proxies", queryStr],
    queryFn: () => apiGet(`/admin/proxies?${queryStr}`),
  });

  const proxies = data?.data ?? [];
  const totalCount = data?.total_count ?? proxies.length;
  const totalPages =
    data?.total_pages ?? (proxies.length === 0 ? 1 : Math.ceil(totalCount / pageSize));

  const invalidate = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: ["proxies"] });
  }, [queryClient]);

  const handleAdd = useCallback(() => {
    setEditing(null);
    setDialogOpen(true);
  }, []);

  const handleEdit = useCallback((p: ProxyItem, e: React.MouseEvent) => {
    e.stopPropagation();
    setEditing(p);
    setDialogOpen(true);
  }, []);

  const handleDeleteClick = useCallback((p: ProxyItem, e: React.MouseEvent) => {
    e.stopPropagation();
    setDeleteTarget(p);
  }, []);

  const handleDeleteConfirm = useCallback(async () => {
    if (!deleteTarget) return;
    try {
      await apiDelete(`/admin/proxies/${deleteTarget.id}`);
      toast.success(t("proxies.toast.deleted"));
      invalidate();
    } catch (err) {
      toast.error((err as Error).message);
    }
    setDeleteTarget(null);
  }, [deleteTarget, invalidate, t]);

  const handleToggle = useCallback(
    async (p: ProxyItem) => {
      try {
        await apiPost(`/admin/proxies/${p.id}/toggle`);
        toast.success(t("proxies.toast.toggleDone"));
        invalidate();
      } catch (err) {
        toast.error((err as Error).message);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [invalidate],
  );

  const handleTest = useCallback(
    async (p: ProxyItem) => {
      try {
        await apiPost(`/admin/proxies/${p.id}/test`);
        toast.success(t("proxies.toast.testDone"));
        invalidate();
      } catch (err) {
        toast.error((err as Error).message);
      }
    },
    [invalidate, t],
  );

  const handleQuality = useCallback(
    async (p: ProxyItem) => {
      setQualityTarget({ name: p.name });
      try {
        const res = await apiPost<{ id: number; probe_result: Record<string, unknown> }>(
          `/admin/proxies/${p.id}/quality`,
        );
        const pr = res.probe_result;
        setQualityTarget({
          name: p.name,
          result: {
            score: (pr.score as number) ?? 0,
            grade: (pr.grade as string) ?? "F",
            overall_status: (pr.overall_status as string) ?? "unknown",
            items: (pr.items as QualityItemLoose[]) ?? [],
            last_check_at: (pr.last_check_at as string) ?? new Date().toISOString(),
            latency_ms: (pr.latency_ms as number | null) ?? null,
          },
        });
        invalidate();
      } catch (err) {
        toast.error((err as Error).message);
        setQualityTarget(null);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [invalidate],
  );

  const handleBatchDelete = useCallback(async () => {
    const ids = Array.from(selected);
    if (ids.length === 0) return;
    try {
      const res = await apiPost<{ deleted_ids: number[]; skipped: { id: number; reason: string }[] }>(
        "/admin/proxies/batch-delete",
        { ids },
      );
      const skipped = res.skipped?.length ?? 0;
      if (skipped > 0) {
        toast.warning(t("proxies.toast.inUseSkipped", { n: skipped }));
      }
      toast.success(t("proxies.toast.batchDone"));
      setSelected(new Set());
      invalidate();
    } catch (err) {
      toast.error((err as Error).message);
    }
  }, [selected, invalidate, t]);

  const toggleSelect = useCallback((id: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const toggleExpand = useCallback((id: number) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">{t("proxies.title")}</h1>
          <p className="text-sm text-muted-foreground">{t("proxies.description")}</p>
        </div>
        <div className="flex items-center gap-2">
          {selected.size > 0 && (
            <Button size="sm" variant="destructive" onClick={handleBatchDelete}>
              <CheckSquare className="h-4 w-4" /> {selected.size}
            </Button>
          )}
          <Button size="sm" onClick={handleAdd}>
            <Plus className="h-4 w-4" /> {t("proxies.newProxy")}
          </Button>
        </div>
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2">
            <Network className="h-4 w-4" />
            {t("proxies.allProxies")} ({totalCount})
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="mb-3 flex items-center gap-2">
            <div className="relative flex-1 max-w-sm">
              <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                placeholder={t("proxies.searchPlaceholder")}
                value={search}
                onChange={(e) => {
                  setSearch(e.target.value);
                  setPage(1);
                }}
                className="pl-9"
                data-testid="proxy-search"
              />
            </div>
            <select
              value={statusFilter}
              onChange={(e) => {
                setStatusFilter(e.target.value);
                setPage(1);
              }}
              className="h-9 rounded-md border px-2 text-sm"
              data-testid="proxy-status-filter"
            >
              <option value="">{t("common.all")}</option>
              <option value="active">{t("common.active")}</option>
              <option value="inactive">{t("proxies.status.inactive")}</option>
            </select>
          </div>

          <PaginationBar
            page={page}
            pageSize={pageSize}
            totalCount={totalCount}
            totalPages={totalPages}
            onPage={setPage}
            onPageSize={(s) => {
              setPageSize(s);
              setPage(1);
            }}
          />

          {/* Desktop table */}
          <div className="hidden md:block">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-8" />
                  <TableHead className="w-8">
                    <input
                      type="checkbox"
                      checked={selected.size > 0 && selected.size === proxies.length}
                      onChange={(e) => {
                        if (e.target.checked) {
                          setSelected(new Set(proxies.map((p) => p.id)));
                        } else {
                          setSelected(new Set());
                        }
                      }}
                    />
                  </TableHead>
                  <TableHead>{t("proxies.table.name")}</TableHead>
                  <TableHead>{t("proxies.table.exitIp")}</TableHead>
                  <TableHead>{t("proxies.table.country")}</TableHead>
                  <TableHead>{t("proxies.table.latency")}</TableHead>
                  <TableHead>{t("proxies.table.score")}</TableHead>
                  <TableHead>{t("proxies.table.grade")}</TableHead>
                  <TableHead>{t("proxies.table.status")}</TableHead>
                  <TableHead>{t("proxies.table.expiresAt")}</TableHead>
                  <TableHead className="w-32">{t("proxies.table.actions")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoading
                  ? Array.from({ length: 3 }).map((_, i) => (
                      <TableRow key={i}>
                        {Array.from({ length: 11 }).map((_, j) => (
                          <TableCell key={j}>
                            <Skeleton className="h-4 w-full" />
                          </TableCell>
                        ))}
                      </TableRow>
                    ))
                  : proxies.map((p) => {
                      const isExpanded = expanded.has(p.id);
                      return (
                        <Fragment key={p.id}>
                          <TableRow
                            className="cursor-pointer hover:bg-muted/50"
                            onClick={() => toggleExpand(p.id)}
                            data-proxy-id={p.id}
                          >
                            <TableCell>
                              {isExpanded ? (
                                <ChevronDown className="h-4 w-4" />
                              ) : (
                                <ChevronRight className="h-4 w-4" />
                              )}
                            </TableCell>
                            <TableCell>
                              <input
                                type="checkbox"
                                checked={selected.has(p.id)}
                                onChange={(e) => {
                                  e.stopPropagation();
                                  toggleSelect(p.id);
                                }}
                              />
                            </TableCell>
                            <TableCell className="font-medium">{p.name}</TableCell>
                            <TableCell className="font-mono text-sm">{p.exit_ip ?? "—"}</TableCell>
                            <TableCell className="text-sm">
                              {p.country ?? ""}{" "}
                              {p.country_code ? (
                                <span className="text-muted-foreground">({p.country_code})</span>
                              ) : null}
                            </TableCell>
                            <TableCell className="text-sm">
                              {p.latency_ms != null ? `${p.latency_ms} ms` : "—"}
                            </TableCell>
                            <TableCell className="text-sm">{p.score ?? "—"}</TableCell>
                            <TableCell>
                              {p.grade ? (
                                <Badge className={gradeClass(p.grade)}>{p.grade}</Badge>
                              ) : (
                                "—"
                              )}
                            </TableCell>
                            <TableCell>
                              <div className="flex items-center gap-2">
                                <Switch
                                  checked={p.status === "active"}
                                  onCheckedChange={() => {
                                    // toggle endpoint flips; re-fetch handles truth
                                    handleToggle(p);
                                  }}
                                />
                                <Badge variant={p.status === "active" ? "default" : "secondary"}>
                                  {p.status === "active" ? t("common.active") : t("proxies.status.inactive")}
                                </Badge>
                              </div>
                            </TableCell>
                            <TableCell className="text-xs text-muted-foreground">
                              {p.expires_at ? new Date(p.expires_at).toLocaleDateString() : "—"}
                            </TableCell>
                            <TableCell>
                              <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-7 w-7"
                                  title={t("proxies.table.latency")}
                                  onClick={() => handleTest(p)}
                                >
                                  <Activity className="h-3.5 w-3.5" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-7 w-7"
                                  title={t("proxies.quality.title")}
                                  onClick={() => handleQuality(p)}
                                >
                                  <Gauge className="h-3.5 w-3.5" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-7 w-7"
                                  onClick={(e) => handleEdit(p, e)}
                                >
                                  <Pencil className="h-3.5 w-3.5" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-7 w-7 text-destructive hover:text-destructive"
                                  onClick={(e) => handleDeleteClick(p, e)}
                                >
                                  <Trash2 className="h-3.5 w-3.5" />
                                </Button>
                              </div>
                            </TableCell>
                          </TableRow>
                          {isExpanded && (
                            <TableRow key={`${p.id}-detail`}>
                              <TableCell colSpan={11} className="bg-muted/30 p-4">
                                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                                  <div>
                                    <h4 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-1">
                                      {t("proxies.table.proxyUrl")}
                                    </h4>
                                    <div className="rounded-md border bg-card p-2 font-mono text-xs">
                                      {p.proxy_url}
                                    </div>
                                  </div>
                                  <div>
                                    <h4 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-1">
                                      {t("proxies.quality.title")}
                                    </h4>
                                    <div className="rounded-md border bg-card p-2 font-mono text-xs overflow-auto max-h-48">
                                      {JSON.stringify(p.probe_result ?? {}, null, 2)}
                                    </div>
                                  </div>
                                </div>
                              </TableCell>
                            </TableRow>
                          )}
                        </Fragment>
                      );
                    })}
              </TableBody>
            </Table>
          </div>

          {/* Mobile cards */}
          <div className="md:hidden space-y-3">
            {proxies.map((p) => (
              <Card key={p.id}>
                <CardContent className="p-4 space-y-2">
                  <div className="flex items-center justify-between">
                    <div className="font-medium text-sm">{p.name}</div>
                    <div className="flex items-center gap-1">
                      <Badge variant={p.status === "active" ? "default" : "secondary"}>
                        {p.status === "active" ? t("common.active") : t("proxies.status.inactive")}
                      </Badge>
                    </div>
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {t("proxies.table.exitIp")}: {p.exit_ip ?? "—"} ·{" "}
                    {t("proxies.table.country")}: {p.country ?? "—"}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {t("proxies.table.latency")}: {p.latency_ms != null ? `${p.latency_ms} ms` : "—"} ·{" "}
                    {t("proxies.table.score")}: {p.score ?? "—"} ·{" "}
                    {t("proxies.table.grade")}: {p.grade ?? "—"}
                  </div>
                  <div className="flex items-center gap-1 pt-1">
                    <Button size="sm" variant="outline" onClick={() => handleTest(p)}>
                      <Activity className="h-3.5 w-3.5" />
                    </Button>
                    <Button size="sm" variant="outline" onClick={() => handleQuality(p)}>
                      <Gauge className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => {
                        setEditing(p);
                        setDialogOpen(true);
                      }}
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      className="text-destructive"
                      onClick={() => setDeleteTarget(p)}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>

          {error && (
            <div className="mt-3 rounded-md bg-destructive/10 border border-destructive/30 px-3 py-2 text-sm text-destructive">
              {(error as Error).message}
            </div>
          )}
        </CardContent>
      </Card>

      <ProxyDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        editing={editing}
        onSaved={() => {
          toast.success(
            editing ? t("proxies.toast.updated") : t("proxies.toast.created"),
          );
          invalidate();
        }}
      />

      {/* Delete confirmation */}
      {deleteTarget && (
        <DialogWrapper
          title={t("proxies.deleteDialog.title")}
          description={t("proxies.deleteDialog.description", { name: deleteTarget.name })}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={handleDeleteConfirm}
        />
      )}

      <QualityDialog
        open={qualityTarget !== null}
        onOpenChange={(o) => {
          if (!o) setQualityTarget(null);
        }}
        result={qualityTarget?.result ?? null}
        name={qualityTarget?.name}
      />
    </div>
  );
}

// ─━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Loose types + a minimal dialog wrapper to keep this file self-contained
// ─━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

type QualityItemLoose = {
  target: string;
  status: string;
  latency_ms?: number | null;
  cf_ray?: string | null;
  message: string;
};

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

function DialogWrapper({
  title,
  description,
  onCancel,
  onConfirm,
}: {
  title: string;
  description: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog open onOpenChange={(o) => !o && onCancel()}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button variant="destructive" onClick={onConfirm}>
            {t("common.delete")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
