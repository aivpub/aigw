import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { apiGet, apiPost, apiPut, apiDelete } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { PaginationBar } from "@/components/ui/pagination";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { toast } from "sonner";
import { Plus, Search, Pencil, Trash2, Building2 } from "lucide-react";
import { format } from "date-fns";

interface OrgItem {
  organization_id: string;
  organization_alias: string;
  budget_id: string | null;
  spend: number;
  budget_duration: string | null;
  soft_budget: number | null;
  created_at: string | null;
}

interface DeletedOrgItem {
  id: number;
  organization_id: string;
  organization_alias: string;
  spend: number;
  deleted_at: string;
}

interface OrgListResponse {
  data: OrgItem[];
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

interface DeletedOrgListResponse {
  data: DeletedOrgItem[];
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

export function OrgsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [viewMode, setViewMode] = useState<"active" | "deleted">("active");
  const [createOpen, setCreateOpen] = useState(false);
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [selected, setSelected] = useState<OrgItem | null>(null);
  const [formAlias, setFormAlias] = useState("");
  const [formBudgetDuration, setFormBudgetDuration] = useState("");
  const [formSoftBudget, setFormSoftBudget] = useState("");
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);
  const [deletedPage, setDeletedPage] = useState(1);
  const [deletedPageSize, setDeletedPageSize] = useState(30);

  const { data, isLoading, error } = useQuery<OrgListResponse>({
    queryKey: ["orgs", page, pageSize],
    queryFn: () => apiGet(`/org/list?page=${page}&page_size=${pageSize}`),
  });

  const { data: deletedData, isLoading: deletedLoading } =
    useQuery<DeletedOrgListResponse>({
      queryKey: ["deleted-orgs", deletedPage, deletedPageSize],
      queryFn: () =>
        apiGet(`/org/deleted?page=${deletedPage}&page_size=${deletedPageSize}`),
      enabled: viewMode === "deleted",
    });

  const orgs = data?.data ?? [];
  const totalCount = data?.total_count ?? orgs.length;
  const totalPages =
    data?.total_pages ??
    (orgs.length === 0 ? 1 : Math.ceil(totalCount / pageSize));

  const deletedOrgs = deletedData?.data ?? [];
  const deletedTotalCount = deletedData?.total_count ?? deletedOrgs.length;
  const deletedTotalPages =
    deletedData?.total_pages ??
    (deletedOrgs.length === 0
      ? 1
      : Math.ceil(deletedTotalCount / deletedPageSize));
  const canEdit = viewMode === "active";

  const filtered = useMemo(() => {
    if (!search.trim()) return orgs;
    const q = search.toLowerCase();
    return orgs.filter((o) => o.organization_alias.toLowerCase().includes(q));
  }, [orgs, search]);

  const createMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPost("/org/new", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["orgs"] });
      setCreateOpen(false);
      toast.success(t("orgs.toast.created"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const editMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPut("/org/update", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["orgs"] });
      setEditOpen(false);
      setSelected(null);
      toast.success(t("orgs.toast.updated"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const deleteMutation = useMutation({
    mutationFn: (org_id: string) =>
      apiDelete(`/org/delete?organization_id=${encodeURIComponent(org_id)}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["orgs"] });
      setDeleteOpen(false);
      setSelected(null);
      toast.success(t("orgs.toast.deleted"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  function openCreate() {
    setFormAlias("");
    setFormBudgetDuration("");
    setFormSoftBudget("");
    setCreateOpen(true);
  }
  function openEdit(o: OrgItem) {
    setSelected(o);
    setFormAlias(o.organization_alias);
    setFormBudgetDuration(o.budget_duration ?? "");
    setFormSoftBudget(o.soft_budget?.toString() ?? "");
    setEditOpen(true);
  }
  function openDelete(o: OrgItem) {
    setSelected(o);
    setDeleteOpen(true);
  }

  function formatDate(d: string) {
    try {
      return format(new Date(d), "yyyy-MM-dd HH:mm");
    } catch {
      return d;
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">
            {t("orgs.title")}
          </h1>
          <p className="text-sm text-muted-foreground">
            {t("orgs.description")}
          </p>
        </div>
        {canEdit && (
          <Button onClick={openCreate}>
            <Plus className="h-4 w-4" /> {t("orgs.newOrg")}
          </Button>
        )}
      </div>

      <div className="flex items-center gap-2">
        {viewMode === "active" && (
          <div className="relative flex-1 max-w-sm">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder={t("orgs.searchPlaceholder")}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-9"
            />
          </div>
        )}
        <div className="flex-1" />
        <div className="flex items-center rounded-md border p-0.5">
          <button
            type="button"
            onClick={() => setViewMode("active")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${viewMode === "active" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground"}`}
          >
            {t("keys.viewMode.active")}
          </button>
          <button
            type="button"
            onClick={() => setViewMode("deleted")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${viewMode === "deleted" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground"}`}
          >
            {t("keys.viewMode.deleted")}
          </button>
        </div>
      </div>

      {viewMode === "deleted" && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle>
              {t("orgs.deletedCardTitle", { count: deletedTotalCount })}
            </CardTitle>
          </CardHeader>
          <CardContent>
            {deletedLoading ? (
              Array.from({ length: 3 }).map((_, i) => (
                <div key={i} className="py-2">
                  <Skeleton className="h-4 w-full" />
                </div>
              ))
            ) : deletedOrgs.length === 0 ? (
              <div className="text-center text-muted-foreground py-8">
                {t("orgs.noDeletedRecords")}
              </div>
            ) : (
              <>
                <PaginationBar
                  page={deletedPage}
                  pageSize={deletedPageSize}
                  totalCount={deletedTotalCount}
                  totalPages={deletedTotalPages}
                  onPage={setDeletedPage}
                  onPageSize={(s) => {
                    setDeletedPageSize(s);
                    setDeletedPage(1);
                  }}
                />
                <div className="hidden md:block">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>{t("orgs.table.name")}</TableHead>
                        <TableHead>{t("orgs.table.orgId")}</TableHead>
                        <TableHead className="text-right">
                          {t("orgs.table.spend")}
                        </TableHead>
                        <TableHead className="text-right">
                          {t("orgs.table.deletedAt")}
                        </TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {deletedOrgs.map((o) => (
                        <TableRow key={o.id}>
                          <TableCell className="font-medium">
                            {o.organization_alias}
                          </TableCell>
                          <TableCell className="text-sm font-mono">
                            {o.organization_id}
                          </TableCell>
                          <TableCell className="text-right text-sm">
                            ${o.spend.toFixed(4)}
                          </TableCell>
                          <TableCell className="text-right text-sm text-muted-foreground">
                            {formatDate(o.deleted_at)}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div className="md:hidden space-y-3">
                  {deletedOrgs.map((o) => (
                    <Card key={o.id}>
                      <CardContent className="p-4 space-y-2">
                        <div className="flex items-center justify-between">
                          <span className="font-medium text-sm">
                            {o.organization_alias}
                          </span>
                          <span className="text-xs text-muted-foreground">
                            ${o.spend.toFixed(4)}
                          </span>
                        </div>
                        <div className="text-xs text-muted-foreground">
                          {t("orgs.mobile.id")}: {o.organization_id}
                        </div>
                        <div className="text-xs text-muted-foreground">
                          {t("orgs.mobile.deleted")}: {formatDate(o.deleted_at)}
                        </div>
                      </CardContent>
                    </Card>
                  ))}
                </div>
                <div className="mt-3">
                  <PaginationBar
                    page={deletedPage}
                    pageSize={deletedPageSize}
                    totalCount={deletedTotalCount}
                    totalPages={deletedTotalPages}
                    onPage={setDeletedPage}
                    onPageSize={(s) => {
                      setDeletedPageSize(s);
                      setDeletedPage(1);
                    }}
                  />
                </div>
              </>
            )}
          </CardContent>
        </Card>
      )}

      {viewMode === "active" && (
        <>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle>
                {t("orgs.allCardTitle", { count: totalCount })}
              </CardTitle>
            </CardHeader>
            <CardContent>
              {error ? (
                <p className="text-sm text-destructive">
                  {(error as Error).message}
                </p>
              ) : (
                <>
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
                  <div className="hidden md:block">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>{t("orgs.table.name")}</TableHead>
                          <TableHead>{t("orgs.table.budgetId")}</TableHead>
                          <TableHead className="text-right">
                            {t("orgs.table.spend")}
                          </TableHead>
                          <TableHead>{t("orgs.table.created")}</TableHead>
                          <TableHead className="text-right">
                            {t("orgs.table.actions")}
                          </TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {isLoading
                          ? Array.from({ length: 3 }).map((_, i) => (
                              <TableRow key={i}>
                                {Array.from({ length: 5 }).map((_, j) => (
                                  <TableCell key={j}>
                                    <Skeleton className="h-4 w-full" />
                                  </TableCell>
                                ))}
                              </TableRow>
                            ))
                          : filtered.map((o) => (
                              <TableRow key={o.organization_id}>
                                <TableCell className="font-medium">
                                  {o.organization_alias}
                                </TableCell>
                                <TableCell className="text-sm font-mono">
                                  {o.budget_id ?? "—"}
                                </TableCell>
                                <TableCell className="text-right text-sm">
                                  ${o.spend.toFixed(4)}
                                </TableCell>
                                <TableCell className="text-xs text-muted-foreground">
                                  {o.created_at
                                    ? formatDate(o.created_at)
                                    : "—"}
                                </TableCell>
                                <TableCell className="text-right">
                                  <div className="flex justify-end gap-1">
                                    <Button
                                      variant="ghost"
                                      size="icon"
                                      onClick={() => openEdit(o)}
                                    >
                                      <Pencil className="h-4 w-4" />
                                    </Button>
                                    <Button
                                      variant="ghost"
                                      size="icon"
                                      onClick={() => openDelete(o)}
                                    >
                                      <Trash2 className="h-4 w-4 text-destructive" />
                                    </Button>
                                  </div>
                                </TableCell>
                              </TableRow>
                            ))}
                      </TableBody>
                    </Table>
                    {!isLoading && filtered.length === 0 && (
                      <div className="text-center text-muted-foreground py-8">
                        {search ? t("orgs.noMatch") : t("orgs.noOrgs")}
                      </div>
                    )}
                  </div>

                  <div className="md:hidden space-y-3">
                    {isLoading
                      ? Array.from({ length: 2 }).map((_, i) => (
                          <Card key={i}>
                            <CardContent className="p-4 space-y-2">
                              <Skeleton className="h-4 w-3/4" />
                              <Skeleton className="h-4 w-full" />
                            </CardContent>
                          </Card>
                        ))
                      : filtered.map((o) => (
                          <Card key={o.organization_id}>
                            <CardContent className="p-4 space-y-2">
                              <div className="flex items-center justify-between">
                                <span className="font-medium text-sm">
                                  {o.organization_alias}
                                </span>
                                <span className="text-xs text-muted-foreground">
                                  ${o.spend.toFixed(4)}
                                </span>
                              </div>
                              <div className="text-xs text-muted-foreground">
                                {t("orgs.mobile.budget")}: {o.budget_id ?? "—"}
                              </div>
                              <div className="text-xs text-muted-foreground">
                                {t("orgs.mobile.created")}:{" "}
                                {o.created_at ? formatDate(o.created_at) : "—"}
                              </div>
                              <div className="flex justify-end gap-1 pt-1">
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => openEdit(o)}
                                >
                                  <Pencil className="h-3.5 w-3.5 mr-1" />{" "}
                                  {t("common.edit")}
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => openDelete(o)}
                                >
                                  <Trash2 className="h-3.5 w-3.5 mr-1 text-destructive" />{" "}
                                  {t("common.delete")}
                                </Button>
                              </div>
                            </CardContent>
                          </Card>
                        ))}
                    {!isLoading && filtered.length === 0 && (
                      <div className="text-center text-muted-foreground py-8">
                        {search ? t("orgs.noMatch") : t("orgs.noOrgs")}
                      </div>
                    )}
                  </div>
                  {filtered.length > 0 ? (
                    <div className="mt-3">
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
                  ) : null}
                </>
              )}
            </CardContent>
          </Card>

          {/* Create Dialog */}
          <Dialog open={createOpen} onOpenChange={setCreateOpen}>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t("orgs.dialog.titleCreate")}</DialogTitle>
              </DialogHeader>
              <div>
                <Label htmlFor="o-alias">{t("orgs.dialog.nameLabel")}</Label>
                <Input
                  id="o-alias"
                  value={formAlias}
                  onChange={(e) => setFormAlias(e.target.value)}
                  placeholder={t("orgs.dialog.namePlaceholder")}
                />
              </div>
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <Label htmlFor="o-budget-duration">Reset Cycle</Label>
                  <select
                    id="o-budget-duration"
                    className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
                    value={formBudgetDuration}
                    onChange={(e) => setFormBudgetDuration(e.target.value)}
                  >
                    <option value="">None (no reset)</option>
                    <option value="24h">Daily (24h)</option>
                    <option value="7d">Weekly (7d)</option>
                    <option value="30d">Monthly (30d)</option>
                  </select>
                </div>
                <div>
                  <Label htmlFor="o-soft-budget">Soft Budget Alert ($)</Label>
                  <Input
                    id="o-soft-budget"
                    type="number"
                    step="0.0001"
                    value={formSoftBudget}
                    onChange={(e) => setFormSoftBudget(e.target.value)}
                    placeholder="0.00 — warn but don't block"
                  />
                </div>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={() => setCreateOpen(false)}>
                  {t("common.cancel")}
                </Button>
                <Button
                  onClick={() =>
                    createMutation.mutate({
                      organization_alias: formAlias,
                      ...(formBudgetDuration.trim() && {
                        budget_duration: formBudgetDuration.trim(),
                      }),
                      ...(formSoftBudget.trim() && {
                        soft_budget: parseFloat(formSoftBudget),
                      }),
                    })
                  }
                  disabled={createMutation.isPending || !formAlias.trim()}
                >
                  {createMutation.isPending && <Spinner className="mr-2" />}{" "}
                  {t("common.create")}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          {/* Edit Dialog */}
          <Dialog open={editOpen} onOpenChange={setEditOpen}>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t("orgs.dialog.titleEdit")}</DialogTitle>
              </DialogHeader>
              <div>
                <Label htmlFor="oe-alias">{t("orgs.dialog.nameLabel")}</Label>
                <Input
                  id="oe-alias"
                  value={formAlias}
                  onChange={(e) => setFormAlias(e.target.value)}
                />
              </div>
              {selected?.budget_id ? (
                <div className="grid grid-cols-2 gap-4">
                  <div>
                    <Label htmlFor="oe-budget-duration">Reset Cycle</Label>
                    <Input
                      id="oe-budget-duration"
                      value={formBudgetDuration || "—"}
                      disabled
                    />
                  </div>
                  <div>
                    <Label htmlFor="oe-soft-budget">
                      Soft Budget Alert ($)
                    </Label>
                    <Input
                      id="oe-soft-budget"
                      value={formSoftBudget || "—"}
                      disabled
                    />
                  </div>
                </div>
              ) : (
                <div className="grid grid-cols-2 gap-4">
                  <div>
                    <Label htmlFor="oe-budget-duration">Reset Cycle</Label>
                    <select
                      id="oe-budget-duration"
                      className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
                      value={formBudgetDuration}
                      onChange={(e) => setFormBudgetDuration(e.target.value)}
                    >
                      <option value="">None (no reset)</option>
                      <option value="24h">Daily (24h)</option>
                      <option value="7d">Weekly (7d)</option>
                      <option value="30d">Monthly (30d)</option>
                    </select>
                  </div>
                  <div>
                    <Label htmlFor="oe-soft-budget">
                      Soft Budget Alert ($)
                    </Label>
                    <Input
                      id="oe-soft-budget"
                      type="number"
                      step="0.0001"
                      value={formSoftBudget}
                      onChange={(e) => setFormSoftBudget(e.target.value)}
                      placeholder="0.00 — warn but don't block"
                    />
                  </div>
                </div>
              )}
              <DialogFooter>
                <Button variant="outline" onClick={() => setEditOpen(false)}>
                  {t("common.cancel")}
                </Button>
                <Button
                  onClick={() =>
                    selected &&
                    editMutation.mutate({
                      organization_id: selected.organization_id,
                      organization_alias: formAlias,
                      ...(!selected.budget_id && {
                        ...(formBudgetDuration.trim() && {
                          budget_duration: formBudgetDuration.trim(),
                        }),
                        ...(formSoftBudget.trim() && {
                          soft_budget: parseFloat(formSoftBudget),
                        }),
                      }),
                    })
                  }
                  disabled={editMutation.isPending}
                >
                  {editMutation.isPending && <Spinner className="mr-2" />}{" "}
                  {t("common.save")}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          {/* Delete */}
          <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t("orgs.dialog.titleDelete")}</DialogTitle>
                <DialogDescription>
                  {t("orgs.dialog.confirmDelete", {
                    name: selected?.organization_alias,
                  })}
                </DialogDescription>
              </DialogHeader>
              <DialogFooter>
                <Button variant="outline" onClick={() => setDeleteOpen(false)}>
                  {t("common.cancel")}
                </Button>
                <Button
                  variant="destructive"
                  onClick={() =>
                    selected && deleteMutation.mutate(selected.organization_id)
                  }
                  disabled={deleteMutation.isPending}
                >
                  {deleteMutation.isPending && <Spinner className="mr-2" />}{" "}
                  {t("common.delete")}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        </>
      )}
    </div>
  );
}
