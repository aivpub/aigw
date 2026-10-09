import { Fragment, useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiDelete, apiGet, apiPost } from "@/lib/api";
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
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import { toast } from "sonner";
import {
  Activity,
  CheckSquare,
  ChevronDown,
  ChevronRight,
  Globe,
  Pencil,
  Plus,
  Search,
  Trash2,
} from "lucide-react";
import { ProviderDialog } from "./ProviderDialog";
import { InstanceDialog } from "./InstanceDialog";
import { ProbeDialog, type ProbeOutcome } from "./ProbeDialog";
import type { WebSearchInstance, WebSearchProvider } from "./types";

/** `$0.00001200` — enough digits for an amortized self-host price to survive. */
function formatCost(cost: number): string {
  return `$${cost.toFixed(8).replace(/0+$/, "").replace(/\.$/, "")}`;
}

export function WebSearchPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [selected, setSelected] = useState<Set<number>>(new Set());

  const [providerDialogOpen, setProviderDialogOpen] = useState(false);
  const [editingProvider, setEditingProvider] = useState<WebSearchProvider | null>(null);

  const [instanceDialogOpen, setInstanceDialogOpen] = useState(false);
  const [instanceProvider, setInstanceProvider] = useState<WebSearchProvider | null>(null);
  const [editingInstance, setEditingInstance] = useState<WebSearchInstance | null>(null);

  const [probeTarget, setProbeTarget] = useState<{
    instance: WebSearchInstance;
    outcome: ProbeOutcome | null;
  } | null>(null);

  const { data, isLoading, error } = useQuery<{ data: WebSearchProvider[] }>({
    queryKey: ["websearch-providers"],
    queryFn: () => apiGet("/admin/websearch/providers"),
  });

  const providers = useMemo(() => data?.data ?? [], [data]);
  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return providers;
    return providers.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        p.kind.toLowerCase().includes(q) ||
        p.instances.some((i) => i.name.toLowerCase().includes(q)),
    );
  }, [providers, search]);

  const invalidate = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: ["websearch-providers"] });
  }, [queryClient]);

  const afterWrite = useCallback(
    (label: string) => (reloadOk: boolean) => {
      if (reloadOk) toast.success(label);
      else toast.warning(t("webSearch.toast.savedNotLive"));
      invalidate();
    },
    [invalidate, t],
  );

  const toggleExpand = useCallback((id: number) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const toggleSelect = useCallback((id: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const handleToggleProvider = useCallback(
    async (p: WebSearchProvider) => {
      try {
        const res = await apiPost<{ reload?: { ok?: boolean } }>(
          `/admin/websearch/providers/${p.id}/toggle`,
        );
        afterWrite(t("webSearch.toast.toggleDone"))(res.reload?.ok !== false);
      } catch (err) {
        toast.error((err as Error).message);
      }
    },
    [afterWrite, t],
  );

  const handleToggleInstance = useCallback(
    async (i: WebSearchInstance) => {
      try {
        const res = await apiPost<{ reload?: { ok?: boolean } }>(
          `/admin/websearch/instances/${i.id}/toggle`,
        );
        afterWrite(t("webSearch.toast.toggleDone"))(res.reload?.ok !== false);
      } catch (err) {
        toast.error((err as Error).message);
      }
    },
    [afterWrite, t],
  );

  const handleDeleteProvider = useCallback(
    async (p: WebSearchProvider) => {
      try {
        await apiDelete(`/admin/websearch/providers/${p.id}`);
        toast.success(t("webSearch.toast.deleted"));
        invalidate();
      } catch (err) {
        toast.error((err as Error).message);
      }
    },
    [invalidate, t],
  );

  const handleDeleteInstance = useCallback(
    async (i: WebSearchInstance) => {
      try {
        await apiDelete(`/admin/websearch/instances/${i.id}`);
        toast.success(t("webSearch.toast.deleted"));
        invalidate();
      } catch (err) {
        toast.error((err as Error).message);
      }
    },
    [invalidate, t],
  );

  const handleProbe = useCallback(
    async (i: WebSearchInstance) => {
      setProbeTarget({ instance: i, outcome: null });
      try {
        const res = await apiPost<ProbeOutcome>(
          `/admin/websearch/instances/${i.id}/test`,
        );
        setProbeTarget({ instance: i, outcome: res });
        invalidate();
      } catch (err) {
        toast.error((err as Error).message);
        setProbeTarget(null);
      }
    },
    [invalidate],
  );

  const handleBatchDelete = useCallback(async () => {
    const ids = Array.from(selected);
    if (ids.length === 0) return;
    try {
      const res = await apiPost<{
        deleted_ids: number[];
        skipped: { id: number; reason: string }[];
      }>("/admin/websearch/providers/batch-delete", { ids });
      const skipped = res.skipped?.length ?? 0;
      if (skipped > 0) toast.warning(t("webSearch.toast.skipped", { n: skipped }));
      toast.success(t("webSearch.toast.batchDone"));
      setSelected(new Set());
      invalidate();
    } catch (err) {
      toast.error((err as Error).message);
    }
  }, [selected, invalidate, t]);

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">{t("webSearch.title")}</h1>
          <p className="text-sm text-muted-foreground">
            {t("webSearch.description")}
          </p>
        </div>
        <div className="flex items-center gap-2">
          {selected.size > 0 && (
            <Button
              size="sm"
              variant="destructive"
              onClick={handleBatchDelete}
              data-testid="ws-batch-delete"
            >
              <CheckSquare className="h-4 w-4" /> {selected.size}
            </Button>
          )}
          <Button
            size="sm"
            onClick={() => {
              setEditingProvider(null);
              setProviderDialogOpen(true);
            }}
            data-testid="ws-new-provider"
          >
            <Plus className="h-4 w-4" /> {t("webSearch.newProvider")}
          </Button>
        </div>
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2">
            <Globe className="h-4 w-4" />
            {t("webSearch.allProviders")} ({filtered.length})
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="relative mb-3 max-w-sm">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder={t("webSearch.searchPlaceholder")}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-9"
              data-testid="ws-search"
            />
          </div>

          <div className="hidden md:block">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-8" />
                  <TableHead className="w-8">
                    <input
                      type="checkbox"
                      checked={selected.size > 0 && selected.size === filtered.length}
                      onChange={(e) => {
                        if (e.target.checked) {
                          setSelected(new Set(filtered.map((p) => p.id)));
                        } else {
                          setSelected(new Set());
                        }
                      }}
                    />
                  </TableHead>
                  <TableHead>{t("webSearch.table.name")}</TableHead>
                  <TableHead>{t("webSearch.table.kind")}</TableHead>
                  <TableHead>{t("webSearch.table.cost")}</TableHead>
                  <TableHead>{t("webSearch.table.instances")}</TableHead>
                  <TableHead>{t("webSearch.table.status")}</TableHead>
                  <TableHead className="w-32">{t("webSearch.table.actions")}</TableHead>
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
                  : filtered.map((p) => {
                      const isExpanded = expanded.has(p.id);
                      return (
                        <Fragment key={p.id}>
                          <TableRow
                            className="cursor-pointer hover:bg-muted/50"
                            onClick={() => toggleExpand(p.id)}
                            data-provider-id={p.id}
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
                            <TableCell className="font-medium">
                              <div className="flex items-center gap-2">
                                {p.name}
                                {p.is_default && (
                                  <Badge variant="secondary" data-testid="ws-default-badge">
                                    {t("webSearch.badge.default")}
                                  </Badge>
                                )}
                              </div>
                            </TableCell>
                            <TableCell className="font-mono text-xs">{p.kind}</TableCell>
                            <TableCell>
                              <span className="font-mono text-xs" data-testid="ws-cost-value">
                                {formatCost(p.cost_per_query)}
                              </span>
                              <span className="text-xs text-muted-foreground">
                                {" "}
                                / {t("webSearch.table.perQuery")}
                              </span>
                            </TableCell>
                            <TableCell className="text-sm">{p.instances.length}</TableCell>
                            <TableCell>
                              <div className="flex items-center gap-2">
                                <Switch
                                  checked={p.enabled}
                                  onCheckedChange={() => handleToggleProvider(p)}
                                  data-testid="ws-provider-toggle"
                                />
                                <Badge variant={p.enabled ? "default" : "secondary"}>
                                  {p.enabled
                                    ? t("common.active")
                                    : t("webSearch.status.disabled")}
                                </Badge>
                              </div>
                            </TableCell>
                            <TableCell>
                              <div
                                className="flex items-center gap-1"
                                onClick={(e) => e.stopPropagation()}
                              >
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-7 w-7"
                                  title={t("webSearch.instance.add")}
                                  onClick={() => {
                                    setInstanceProvider(p);
                                    setEditingInstance(null);
                                    setInstanceDialogOpen(true);
                                  }}
                                  data-testid="ws-add-instance"
                                >
                                  <Plus className="h-3.5 w-3.5" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-7 w-7"
                                  onClick={() => {
                                    setEditingProvider(p);
                                    setProviderDialogOpen(true);
                                  }}
                                  data-testid="ws-provider-edit"
                                >
                                  <Pencil className="h-3.5 w-3.5" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-7 w-7 text-destructive hover:text-destructive"
                                  onClick={() => handleDeleteProvider(p)}
                                  data-testid="ws-provider-delete"
                                >
                                  <Trash2 className="h-3.5 w-3.5" />
                                </Button>
                              </div>
                            </TableCell>
                          </TableRow>
                          {isExpanded && (
                            <TableRow>
                              <TableCell colSpan={8} className="bg-muted/30 p-4">
                                {p.instances.length === 0 ? (
                                  <p className="text-sm text-muted-foreground">
                                    {t("webSearch.instance.none")}
                                  </p>
                                ) : (
                                  <table className="w-full text-sm">
                                    <thead>
                                      <tr className="border-b text-left text-xs text-muted-foreground">
                                        <th className="py-1 pr-3">
                                          {t("webSearch.instance.name")}
                                        </th>
                                        <th className="py-1 pr-3">
                                          {t("webSearch.instance.baseUrl")}
                                        </th>
                                        <th className="py-1 pr-3">
                                          {t("webSearch.instance.apiKey")}
                                        </th>
                                        <th className="py-1 pr-3">
                                          {t("webSearch.instance.weight")}
                                        </th>
                                        <th className="py-1 pr-3">
                                          {t("webSearch.table.status")}
                                        </th>
                                        <th className="py-1">
                                          {t("webSearch.table.actions")}
                                        </th>
                                      </tr>
                                    </thead>
                                    <tbody>
                                      {p.instances.map((i) => (
                                        <tr
                                          key={i.id}
                                          className="border-b last:border-0"
                                          data-testid="ws-instance-row"
                                          data-instance-id={i.id}
                                        >
                                          <td className="py-1.5 pr-3 font-medium">{i.name}</td>
                                          <td className="py-1.5 pr-3 font-mono text-xs">
                                            {i.base_url}
                                          </td>
                                          <td className="py-1.5 pr-3 font-mono text-xs">
                                            {i.redacted_api_key ?? "—"}
                                          </td>
                                          <td className="py-1.5 pr-3 text-xs">
                                            {i.weight ?? "—"}
                                          </td>
                                          <td className="py-1.5 pr-3">
                                            <Switch
                                              checked={i.enabled}
                                              onCheckedChange={() => handleToggleInstance(i)}
                                            />
                                          </td>
                                          <td className="py-1.5">
                                            <div className="flex items-center gap-1">
                                              <Button
                                                variant="ghost"
                                                size="icon"
                                                className="h-7 w-7"
                                                title={t("webSearch.probe.title")}
                                                onClick={() => handleProbe(i)}
                                                data-testid="ws-probe-instance"
                                              >
                                                <Activity className="h-3.5 w-3.5" />
                                              </Button>
                                              <Button
                                                variant="ghost"
                                                size="icon"
                                                className="h-7 w-7"
                                                onClick={() => {
                                                  setInstanceProvider(p);
                                                  setEditingInstance(i);
                                                  setInstanceDialogOpen(true);
                                                }}
                                              >
                                                <Pencil className="h-3.5 w-3.5" />
                                              </Button>
                                              <Button
                                                variant="ghost"
                                                size="icon"
                                                className="h-7 w-7 text-destructive hover:text-destructive"
                                                onClick={() => handleDeleteInstance(i)}
                                              >
                                                <Trash2 className="h-3.5 w-3.5" />
                                              </Button>
                                            </div>
                                          </td>
                                        </tr>
                                      ))}
                                    </tbody>
                                  </table>
                                )}
                              </TableCell>
                            </TableRow>
                          )}
                        </Fragment>
                      );
                    })}
              </TableBody>
            </Table>
          </div>

          <div className="space-y-3 md:hidden">
            {filtered.map((p) => (
              <Card key={p.id} data-testid="ws-provider-card">
                <CardContent className="space-y-2 p-4">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2 font-medium text-sm">
                      {p.name}
                      {p.is_default && (
                        <Badge variant="secondary" data-testid="ws-default-badge">
                          {t("webSearch.badge.default")}
                        </Badge>
                      )}
                    </div>
                    <Badge variant={p.enabled ? "default" : "secondary"}>
                      {p.enabled ? t("common.active") : t("webSearch.status.disabled")}
                    </Badge>
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {p.kind} · {formatCost(p.cost_per_query)} / {t("webSearch.table.perQuery")} ·{" "}
                    {p.instances.length} {t("webSearch.table.instances")}
                  </div>
                  <div className="space-y-1">
                    {p.instances.map((i) => (
                      <div
                        key={i.id}
                        data-testid="ws-instance-row"
                        data-instance-id={i.id}
                        className="flex items-center justify-between rounded border px-2 py-1.5 text-xs"
                      >
                        <div className="min-w-0">
                          <div className="truncate font-medium">{i.name}</div>
                          <div className="truncate font-mono text-muted-foreground">
                            {i.base_url}
                          </div>
                        </div>
                        <div className="flex shrink-0 items-center gap-0.5">
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7"
                            title={t("webSearch.probe.title")}
                            onClick={() => handleProbe(i)}
                            data-testid="ws-probe-instance"
                          >
                            <Activity className="h-3.5 w-3.5" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7"
                            onClick={() => {
                              setInstanceProvider(p);
                              setEditingInstance(i);
                              setInstanceDialogOpen(true);
                            }}
                          >
                            <Pencil className="h-3.5 w-3.5" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7 text-destructive hover:text-destructive"
                            onClick={() => handleDeleteInstance(i)}
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                          </Button>
                        </div>
                      </div>
                    ))}
                  </div>
                  <div className="flex items-center gap-1 pt-1">
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => {
                        setInstanceProvider(p);
                        setEditingInstance(null);
                        setInstanceDialogOpen(true);
                      }}
                    >
                      <Plus className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => {
                        setEditingProvider(p);
                        setProviderDialogOpen(true);
                      }}
                      data-testid="ws-provider-edit"
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      className="text-destructive"
                      onClick={() => handleDeleteProvider(p)}
                      data-testid="ws-provider-delete"
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>

          {!isLoading && filtered.length === 0 && (
            <p className="py-6 text-center text-sm text-muted-foreground">
              {search ? t("webSearch.noMatch") : t("webSearch.noProviders")}
            </p>
          )}

          {error && (
            <div className="mt-3 rounded-md bg-destructive/10 border border-destructive/30 px-3 py-2 text-sm text-destructive">
              {(error as Error).message}
            </div>
          )}
        </CardContent>
      </Card>

      <ProviderDialog
        open={providerDialogOpen}
        onOpenChange={setProviderDialogOpen}
        editing={editingProvider}
        onSaved={afterWrite(
          editingProvider
            ? t("webSearch.toast.updated")
            : t("webSearch.toast.created"),
        )}
      />

      {instanceProvider && (
        <InstanceDialog
          open={instanceDialogOpen}
          onOpenChange={setInstanceDialogOpen}
          providerId={instanceProvider.id}
          providerName={instanceProvider.name}
          editing={editingInstance}
          onSaved={afterWrite(
            editingInstance
              ? t("webSearch.toast.updated")
              : t("webSearch.toast.created"),
          )}
        />
      )}

      <ProbeDialog
        open={probeTarget !== null}
        onOpenChange={(o) => {
          if (!o) setProbeTarget(null);
        }}
        instanceName={probeTarget?.instance.name ?? ""}
        baseUrl={probeTarget?.instance.base_url ?? ""}
        outcome={probeTarget?.outcome ?? null}
      />
    </div>
  );
}
