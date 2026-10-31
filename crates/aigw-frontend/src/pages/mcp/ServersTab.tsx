import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ApiError, apiDelete, apiGet, apiPost } from "@/lib/api";
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
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { PaginationBar } from "@/components/ui/pagination";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { toast } from "sonner";
import {
  Activity,
  KeyRound,
  Pencil,
  Plus,
  Power,
  Radar,
  ScrollText,
  Search,
  Server,
  Trash2,
  Wrench,
} from "lucide-react";
import { useNavigate } from "react-router-dom";
import { ServerDialog } from "./ServerDialog";
import { ProbeDialog } from "./ProbeDialog";
import { ProbeProgressDialog } from "./ProbeProgressDialog";
import { TokenDialog } from "./TokenDialog";
import { ToolsDialog } from "./ToolsDialog";
import {
  scopeI18nKey,
  type McpServer,
  type McpServerListResponse,
  type ProbeOutcome,
  type ProbeRun,
  type ReloadStarted,
} from "./types";

const scopeBadgeClass: Record<string, string> = {
  shared: "bg-slate-100 text-slate-700",
  shared_or_user: "bg-blue-100 text-blue-800",
  user_required: "bg-orange-100 text-orange-800",
};

function ScopeBadge({ scope }: { scope: string }) {
  const { t } = useTranslation();
  return (
    <Badge
      variant="outline"
      className={scopeBadgeClass[scope] ?? "bg-slate-100 text-slate-700"}
      data-testid="mcp-scope-badge"
    >
      {t(`mcp.scope.${scopeI18nKey(scope)}`)}
    </Badge>
  );
}

interface ReloadConflict {
  type?: string;
  run_id?: string;
  message?: string;
}

function DeleteConfirmDialog({
  alias,
  onCancel,
  onConfirm,
}: {
  alias: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog open onOpenChange={(o) => !o && onCancel()}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>{t("mcp.deleteDialog.title")}</DialogTitle>
          <DialogDescription>
            {t("mcp.deleteDialog.description", { alias })}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="destructive"
            onClick={onConfirm}
            data-testid="mcp-delete-confirm"
          >
            {t("common.delete")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function ServersTab() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState("");
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);

  const [serverDialogOpen, setServerDialogOpen] = useState(false);
  const [editing, setEditing] = useState<McpServer | null>(null);

  const [probeTarget, setProbeTarget] = useState<{
    server: McpServer;
    outcome: ProbeOutcome | null;
    error: string | null;
  } | null>(null);

  const [tokenServer, setTokenServer] = useState<McpServer | null>(null);
  const [toolsServer, setToolsServer] = useState<McpServer | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<McpServer | null>(null);
  const [probingId, setProbingId] = useState<number | null>(null);
  const [reloadRunId, setReloadRunId] = useState<string | null>(null);
  const [progressOpen, setProgressOpen] = useState(false);

  const queryStr = useMemo(() => {
    const params = new URLSearchParams();
    params.set("page", String(page));
    params.set("page_size", String(pageSize));
    if (statusFilter) params.set("status", statusFilter);
    if (search.trim()) params.set("search", search.trim());
    return params.toString();
  }, [page, pageSize, statusFilter, search]);

  const { data, isLoading, error } = useQuery<McpServerListResponse>({
    queryKey: ["mcp-servers", queryStr],
    queryFn: () => apiGet(`/admin/mcp/servers?${queryStr}`),
  });

  const servers = data?.data ?? [];
  const totalCount = data?.total_count ?? servers.length;
  const totalPages =
    data?.total_pages ?? (servers.length === 0 ? 1 : Math.ceil(totalCount / pageSize));

  const invalidate = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: ["mcp-servers"] });
    queryClient.invalidateQueries({ queryKey: ["mcp-servers-all"] });
  }, [queryClient]);

  const handleProbe = useCallback(
    async (s: McpServer) => {
      setProbingId(s.id);
      try {
        const res = await apiPost<ProbeOutcome>(
          `/admin/mcp/servers/${s.id}/probe`,
        );
        setProbeTarget({ server: s, outcome: res, error: null });
        toast.success(t("mcp.toast.probed"));
        invalidate();
        queryClient.invalidateQueries({ queryKey: ["mcp-server-tools", s.id] });
      } catch (err) {
        setProbeTarget({ server: s, outcome: null, error: (err as Error).message });
        toast.error(t("mcp.toast.probeFailed"));
      } finally {
        setProbingId(null);
      }
    },
    [invalidate, queryClient, t],
  );

  const handleToggle = useCallback(
    async (s: McpServer) => {
      try {
        await apiPost(`/admin/mcp/servers/${s.id}/toggle`);
        toast.success(t("mcp.toast.toggleDone"));
        invalidate();
      } catch (err) {
        toast.error((err as Error).message);
      }
    },
    [invalidate, t],
  );

  const handleDelete = useCallback(
    async (s: McpServer) => {
      setDeleteTarget(null);
      try {
        await apiDelete(`/admin/mcp/servers/${s.id}`);
        toast.success(t("mcp.toast.deleted"));
        invalidate();
      } catch (err) {
        toast.error((err as Error).message);
      }
    },
    [invalidate, t],
  );

  const handleReloadAll = useCallback(async () => {
    // A click while a sweep is still running attaches to it instead of
    // erroring: the task lives on the server and the dialog only observes it.
    // `latest` is read here rather than mirrored into state, so a page refresh
    // — which wipes local state — still finds the live run.
    //
    // `404 no_reload_run` is the normal "nothing has run yet" answer and means
    // "go ahead and start one". Any other failure is not that: swallowing it
    // would turn a 403 or a 500 into a click that silently does nothing.
    const latest = await apiGet<ProbeRun>("/admin/mcp/reload/latest").catch(
      (err: unknown) => {
        if (err instanceof ApiError && err.status === 404) return null;
        toast.error((err as Error).message);
        return undefined;
      },
    );
    if (latest === undefined) return;
    if (latest?.status === "running") {
      setReloadRunId(latest.run_id);
      setProgressOpen(true);
      toast.info(t("mcp.toast.reloadInProgress"));
      return;
    }
    try {
      const res = await apiPost<ReloadStarted>("/admin/mcp/reload");
      setReloadRunId(res.run_id);
      setProgressOpen(true);
    } catch (err) {
      // The check above can lose a race with another operator's start; the
      // `409` names the run to attach to, so that is not an error either.
      if (err instanceof ApiError && err.status === 409) {
        const conflict = (err.body as { error?: ReloadConflict })?.error;
        if (conflict?.run_id) {
          setReloadRunId(conflict.run_id);
          setProgressOpen(true);
          toast.info(t("mcp.toast.reloadInProgress"));
          return;
        }
      }
      toast.error((err as Error).message);
    }
  }, [t]);

  const handleReloadFinished = useCallback(
    (run: ProbeRun) => {
      const failed = run.items.filter((i) => i.state === "failed").length;
      if (failed > 0) {
        toast.warning(t("mcp.toast.reloadPartial", { failed }));
      } else {
        toast.success(t("mcp.toast.reloaded"));
      }
      invalidate();
    },
    [invalidate, t],
  );

  const openCreate = useCallback(() => {
    setEditing(null);
    setServerDialogOpen(true);
  }, []);

  const serverLogs = useCallback(
    (s: McpServer) => {
      navigate(`/dash/mcp/calls?alias=${encodeURIComponent(s.alias)}`);
    },
    [navigate],
  );

  return (
    <div className="space-y-4">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-1 flex-col gap-2 sm:flex-row sm:items-center">
          <div className="relative w-full sm:max-w-sm">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder={t("mcp.searchPlaceholder")}
              value={search}
              onChange={(e) => {
                setSearch(e.target.value);
                setPage(1);
              }}
              className="pl-9"
              data-testid="mcp-search"
            />
          </div>
          <select
            value={statusFilter}
            onChange={(e) => {
              setStatusFilter(e.target.value);
              setPage(1);
            }}
            className="h-9 rounded-md border px-2 text-sm"
            data-testid="mcp-status-filter"
          >
            <option value="">{t("common.all")}</option>
            <option value="enabled">{t("common.enabled")}</option>
            <option value="disabled">{t("common.disabled")}</option>
          </select>
        </div>
        <div className="flex items-center gap-2">
          <Button size="sm" onClick={openCreate} data-testid="mcp-new-server">
            <Plus className="h-4 w-4" /> {t("mcp.newServer")}
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={handleReloadAll}
            data-testid="mcp-reload-all"
          >
            <Radar className="h-4 w-4" /> {t("mcp.reloadAll")}
          </Button>
        </div>
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2">
            <Server className="h-4 w-4" />
            {t("mcp.allServers")} ({totalCount})
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="mb-3">
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
          </div>

          <div className="hidden overflow-x-auto md:block">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("mcp.table.alias")}</TableHead>
                  <TableHead>{t("mcp.table.displayName")}</TableHead>
                  <TableHead className="hidden xl:table-cell">{t("mcp.table.url")}</TableHead>
                  <TableHead>{t("mcp.table.protocol")}</TableHead>
                  <TableHead className="w-16">{t("mcp.table.toolCount")}</TableHead>
                  <TableHead>{t("mcp.table.credentialScope")}</TableHead>
                  <TableHead>{t("mcp.table.status")}</TableHead>
                  <TableHead className="w-40">{t("mcp.table.actions")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoading
                  ? Array.from({ length: 3 }).map((_, i) => (
                      <TableRow key={i}>
                        {Array.from({ length: 8 }).map((_, j) => (
                          <TableCell key={j}>
                            <Skeleton className="h-4 w-full" />
                          </TableCell>
                        ))}
                      </TableRow>
                    ))
                  : servers.map((s) => (
                      <TableRow key={s.id} data-server-id={s.id}>
                        <TableCell className="font-medium" data-testid="mcp-server-alias">
                          {s.alias}
                        </TableCell>
                        <TableCell className="text-sm">{s.display_name}</TableCell>
                        <TableCell className="hidden max-w-[16rem] truncate font-mono text-xs xl:table-cell">
                          {s.url}
                        </TableCell>
                        <TableCell className="font-mono text-xs">
                          {s.protocol_version ? (
                            s.protocol_version
                          ) : (
                            <span className="text-muted-foreground" data-testid="mcp-not-probed">
                              {t("mcp.notProbed")}
                            </span>
                          )}
                        </TableCell>
                        <TableCell className="text-sm" data-testid="mcp-tool-count">
                          {s.tool_count ?? "—"}
                        </TableCell>
                        <TableCell>
                          <ScopeBadge scope={s.credential_scope} />
                        </TableCell>
                        <TableCell>
                          <Badge
                            variant={s.enabled ? "default" : "secondary"}
                            data-testid="mcp-status-badge"
                          >
                            {s.enabled ? t("common.enabled") : t("common.disabled")}
                          </Badge>
                        </TableCell>
                        <TableCell>
                          <div className="flex items-center gap-1">
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7"
                              title={t("common.edit")}
                              onClick={() => {
                                setEditing(s);
                                setServerDialogOpen(true);
                              }}
                              data-testid="mcp-server-edit"
                            >
                              <Pencil className="h-3.5 w-3.5" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7"
                              title={t("mcp.actions.probe")}
                              onClick={() => handleProbe(s)}
                              disabled={probingId === s.id}
                              data-testid="mcp-server-probe"
                            >
                              {probingId === s.id ? (
                                <Spinner className="h-3.5 w-3.5" />
                              ) : (
                                <Activity className="h-3.5 w-3.5" />
                              )}
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7"
                              title={t("mcp.actions.tools")}
                              onClick={() => setToolsServer(s)}
                              data-testid="mcp-server-tools"
                            >
                              <Wrench className="h-3.5 w-3.5" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7"
                              title={t("mcp.actions.credentials")}
                              onClick={() => setTokenServer(s)}
                              data-testid="mcp-server-credentials"
                            >
                              <KeyRound className="h-3.5 w-3.5" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7"
                              title={
                                s.enabled ? t("mcp.actions.disable") : t("mcp.actions.enable")
                              }
                              onClick={() => handleToggle(s)}
                              data-testid="mcp-server-toggle"
                            >
                              <Power className="h-3.5 w-3.5" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7"
                              title={t("mcp.actions.logs")}
                              onClick={() => serverLogs(s)}
                              data-testid="mcp-server-logs"
                            >
                              <ScrollText className="h-3.5 w-3.5" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7 text-destructive hover:text-destructive"
                              title={t("common.delete")}
                              onClick={() => setDeleteTarget(s)}
                              data-testid="mcp-server-delete"
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                          </div>
                        </TableCell>
                      </TableRow>
                    ))}
              </TableBody>
            </Table>
          </div>

          <div className="space-y-3 md:hidden">
            {servers.map((s) => (
              <Card key={s.id} data-testid="mcp-server-card" data-server-id={s.id}>
                <CardContent className="space-y-2 p-4">
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <div className="truncate font-medium text-sm" data-testid="mcp-server-alias">
                        {s.alias}
                      </div>
                      <div className="truncate text-xs text-muted-foreground">
                        {s.display_name}
                      </div>
                    </div>
                    <Badge
                      variant={s.enabled ? "default" : "secondary"}
                      data-testid="mcp-status-badge"
                    >
                      {s.enabled ? t("common.enabled") : t("common.disabled")}
                    </Badge>
                  </div>
                  <div className="truncate font-mono text-xs text-muted-foreground">
                    {s.url}
                  </div>
                  <div className="flex flex-wrap items-center gap-2 text-xs">
                    <span className="font-mono">
                      {s.protocol_version ?? t("mcp.notProbed")}
                    </span>
                    <ScopeBadge scope={s.credential_scope} />
                    <span className="text-muted-foreground">
                      {t("mcp.table.toolCount")}: {s.tool_count ?? "—"}
                    </span>
                  </div>
                  <div className="flex flex-wrap items-center gap-1 pt-1">
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 px-2"
                      title={t("common.edit")}
                      onClick={() => {
                        setEditing(s);
                        setServerDialogOpen(true);
                      }}
                      data-testid="mcp-server-edit"
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 px-2"
                      title={t("mcp.actions.probe")}
                      onClick={() => handleProbe(s)}
                      disabled={probingId === s.id}
                      data-testid="mcp-server-probe"
                    >
                      {probingId === s.id ? (
                        <Spinner className="h-3.5 w-3.5" />
                      ) : (
                        <Activity className="h-3.5 w-3.5" />
                      )}
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 px-2"
                      title={t("mcp.actions.tools")}
                      onClick={() => setToolsServer(s)}
                      data-testid="mcp-server-tools"
                    >
                      <Wrench className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 px-2"
                      title={t("mcp.actions.credentials")}
                      onClick={() => setTokenServer(s)}
                      data-testid="mcp-server-credentials"
                    >
                      <KeyRound className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 px-2"
                      title={
                        s.enabled ? t("mcp.actions.disable") : t("mcp.actions.enable")
                      }
                      onClick={() => handleToggle(s)}
                      data-testid="mcp-server-toggle"
                    >
                      <Power className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 px-2"
                      title={t("mcp.actions.logs")}
                      onClick={() => serverLogs(s)}
                      data-testid="mcp-server-logs"
                    >
                      <ScrollText className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 px-2 text-destructive"
                      title={t("common.delete")}
                      onClick={() => setDeleteTarget(s)}
                      data-testid="mcp-server-delete"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>

          {!isLoading && servers.length === 0 && (
            <div className="py-8 text-center" data-testid="mcp-empty-state">
              {search || statusFilter ? (
                <p className="text-sm text-muted-foreground">{t("mcp.empty.noMatch")}</p>
              ) : (
                <>
                  <p className="text-sm font-medium">{t("mcp.empty.title")}</p>
                  <p className="mx-auto mt-1 max-w-md text-xs text-muted-foreground">
                    {t("mcp.empty.body")}
                  </p>
                  <Button size="sm" className="mt-4" onClick={openCreate}>
                    <Plus className="h-4 w-4" /> {t("mcp.newServer")}
                  </Button>
                </>
              )}
            </div>
          )}

          {error && (
            <div className="mt-3 rounded-md bg-destructive/10 border border-destructive/30 px-3 py-2 text-sm text-destructive">
              {(error as Error).message}
            </div>
          )}
        </CardContent>
      </Card>

      <ServerDialog
        key={`${editing?.id ?? "new"}-${serverDialogOpen ? 1 : 0}`}
        open={serverDialogOpen}
        onOpenChange={setServerDialogOpen}
        editing={editing}
        onSaved={() => {
          toast.success(editing ? t("mcp.toast.updated") : t("mcp.toast.created"));
          invalidate();
        }}
      />

      <ProbeDialog
        open={probeTarget !== null}
        onOpenChange={(o) => {
          if (!o) setProbeTarget(null);
        }}
        alias={probeTarget?.server.alias ?? ""}
        outcome={probeTarget?.outcome ?? null}
        error={probeTarget?.error ?? null}
      />

      <ProbeProgressDialog
        open={progressOpen}
        onOpenChange={setProgressOpen}
        runId={reloadRunId}
        onFinished={handleReloadFinished}
      />

      <TokenDialog
        key={tokenServer?.id ?? "token-closed"}
        open={tokenServer !== null}
        onOpenChange={(o) => {
          if (!o) setTokenServer(null);
        }}
        server={tokenServer}
      />

      <ToolsDialog
        key={toolsServer?.id ?? "tools-closed"}
        open={toolsServer !== null}
        onOpenChange={(o) => {
          if (!o) setToolsServer(null);
        }}
        serverId={toolsServer?.id ?? null}
        alias={toolsServer?.alias ?? ""}
      />

      {deleteTarget && (
        <DeleteConfirmDialog
          alias={deleteTarget.alias}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => handleDelete(deleteTarget)}
        />
      )}
    </div>
  );
}
