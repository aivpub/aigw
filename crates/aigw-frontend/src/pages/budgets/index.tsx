import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import i18n from "@/i18n";
import { apiGet, apiPost } from "@/lib/api";
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
import { Plus, Search, Pencil, Trash2 } from "lucide-react";
import { format } from "date-fns";

interface BudgetItem {
  budget_id: string;
  budget_name: string | null;
  max_budget: string | null;
  soft_budget: string | null;
  budget_duration: string | null;
  budget_reset_at: string | null;
  created_at: string | null;
  updated_at: string | null;
  created_by: string;
  updated_by: string;
}

interface BudgetListResponse {
  data: BudgetItem[];
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

function durationLabel(d: string, _t: ReturnType<typeof useTranslation>["t"]): string {
  const key =
    d === "24h"
      ? "budgets.resetCycleOptions.daily"
      : d === "7d"
        ? "budgets.resetCycleOptions.weekly"
        : d === "30d"
          ? "budgets.resetCycleOptions.monthly"
          : null;
  return key ? (i18n.t(key) as string) : d;
}

export function BudgetsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [createOpen, setCreateOpen] = useState(false);
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [selected, setSelected] = useState<BudgetItem | null>(null);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);

  // Form state
  const [formName, setFormName] = useState("");
  const [formMaxBudget, setFormMaxBudget] = useState("");
  const [formBudgetDuration, setFormBudgetDuration] = useState("");
  const [formSoftBudget, setFormSoftBudget] = useState("");

  const { data, isLoading, error } = useQuery<BudgetListResponse>({
    queryKey: ["budgets", page, pageSize],
    queryFn: () => apiGet(`/budget/list?page=${page}&page_size=${pageSize}`),
  });

  const budgets = data?.data ?? [];
  const totalCount = data?.total_count ?? budgets.length;
  const totalPages =
    data?.total_pages ??
    (budgets.length === 0 ? 1 : Math.ceil(totalCount / pageSize));

  const filtered = useMemo(() => {
    if (!search.trim()) return budgets;
    const q = search.toLowerCase();
    return budgets.filter(
      (b) =>
        (b.budget_id ?? "").toLowerCase().includes(q) ||
        (b.budget_name ?? "").toLowerCase().includes(q),
    );
  }, [budgets, search]);

  const createMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPost("/budget/new", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["budgets"] });
      setCreateOpen(false);
      toast.success(t("budgets.toast.created"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const editMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) =>
      apiPost("/budget/update", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["budgets"] });
      setEditOpen(false);
      setSelected(null);
      toast.success(t("budgets.toast.updated"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const deleteMutation = useMutation({
    mutationFn: (budget_id: string) => apiPost("/budget/delete", { budget_id }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["budgets"] });
      setDeleteOpen(false);
      setSelected(null);
      toast.success(t("budgets.toast.deleted"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  function openCreate() {
    setFormName("");
    setFormMaxBudget("");
    setFormBudgetDuration("");
    setFormSoftBudget("");
    setCreateOpen(true);
  }

  function openEdit(b: BudgetItem) {
    setSelected(b);
    setFormName(b.budget_name ?? "");
    setFormMaxBudget(b.max_budget ?? "");
    setFormBudgetDuration(b.budget_duration ?? "");
    setFormSoftBudget(b.soft_budget ?? "");
    setEditOpen(true);
  }

  function openDelete(b: BudgetItem) {
    setSelected(b);
    setDeleteOpen(true);
  }

  function formatDate(d: string) {
    try {
      return format(new Date(d), "yyyy-MM-dd HH:mm");
    } catch {
      return d;
    }
  }

  function displayName(b: BudgetItem): string {
    return b.budget_name ?? b.budget_id;
  }

  function buildBody(): Record<string, unknown> {
    const body: Record<string, unknown> = {};
    if (formName.trim()) body.budget_name = formName.trim();
    if (formMaxBudget.trim()) body.max_budget = parseFloat(formMaxBudget);
    if (formBudgetDuration.trim())
      body.budget_duration = formBudgetDuration.trim();
    if (formSoftBudget.trim()) body.soft_budget = parseFloat(formSoftBudget);
    return body;
  }

  function handleCreate() {
    createMutation.mutate(buildBody());
  }

  function handleEdit() {
    if (!selected) return;
    editMutation.mutate({
      budget_id: selected.budget_id,
      ...buildBody(),
    });
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">
            {t("budgets.title")}
          </h1>
          <p className="text-sm text-muted-foreground">
            {t("budgets.description")}
          </p>
        </div>
        <Button onClick={openCreate}>
          <Plus className="h-4 w-4" /> {t("budgets.newBudget")}
        </Button>
      </div>

      <div className="flex items-center gap-2">
        <div className="relative flex-1 max-w-sm">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            placeholder={t("budgets.searchPlaceholder")}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="pl-9"
          />
        </div>
        <div className="flex-1" />
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle>{t("budgets.allBudgets")} ({totalCount})</CardTitle>
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
              {/* Desktop table */}
              <div className="hidden md:block">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t("budgets.table.name")}</TableHead>
                      <TableHead>{t("budgets.table.budgetId")}</TableHead>
                      <TableHead>{t("budgets.table.limit")}</TableHead>
                      <TableHead>{t("budgets.table.resetPeriod")}</TableHead>
                      <TableHead>{t("budgets.table.softAlert")}</TableHead>
                      <TableHead>{t("budgets.table.created")}</TableHead>
                      <TableHead className="text-right">
                        {t("budgets.table.actions")}
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {isLoading
                      ? Array.from({ length: 3 }).map((_, i) => (
                          <TableRow key={i}>
                            {Array.from({ length: 7 }).map((_, j) => (
                              <TableCell key={j}>
                                <Skeleton className="h-4 w-full" />
                              </TableCell>
                            ))}
                          </TableRow>
                        ))
                      : filtered.map((b) => (
                          <TableRow key={b.budget_id}>
                            <TableCell className="font-medium">
                              {displayName(b)}
                            </TableCell>
                            <TableCell className="text-sm font-mono text-xs">
                              {b.budget_id}
                            </TableCell>
                            <TableCell className="text-sm">
                              {b.max_budget
                                ? `$${parseFloat(b.max_budget).toFixed(2)}`
                                : "∞"}
                            </TableCell>
                            <TableCell className="text-sm">
                              {b.budget_duration
                                ? durationLabel(b.budget_duration, t)
                                : "—"}
                            </TableCell>
                            <TableCell className="text-sm">
                              {b.soft_budget
                                ? `$${parseFloat(b.soft_budget).toFixed(2)}`
                                : "—"}
                            </TableCell>
                            <TableCell className="text-xs text-muted-foreground">
                              {b.created_at ? formatDate(b.created_at) : "—"}
                            </TableCell>
                            <TableCell className="text-right">
                              <div className="flex justify-end gap-1">
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  onClick={() => openEdit(b)}
                                >
                                  <Pencil className="h-4 w-4" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  onClick={() => openDelete(b)}
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
                    {search ? t("budgets.noMatch") : t("budgets.noBudgets")}
                  </div>
                )}
              </div>

              {/* Mobile card list */}
              <div className="md:hidden space-y-3">
                {isLoading
                  ? Array.from({ length: 3 }).map((_, i) => (
                      <Card key={i}>
                        <CardContent className="p-4 space-y-2">
                          <Skeleton className="h-4 w-3/4" />
                          <Skeleton className="h-4 w-full" />
                        </CardContent>
                      </Card>
                    ))
                  : filtered.map((b) => (
                      <Card key={b.budget_id}>
                        <CardContent className="p-4 space-y-2">
                          <div className="flex items-center justify-between">
                            <span className="font-medium text-sm">
                              {displayName(b)}
                            </span>
                            <span className="text-xs text-muted-foreground">
                              {b.max_budget
                                ? `$${parseFloat(b.max_budget).toFixed(2)}`
                                : "∞"}
                            </span>
                          </div>
                          <div className="text-xs font-mono text-muted-foreground truncate">
                            {b.budget_id}
                          </div>
                          <div className="flex items-center justify-between text-xs text-muted-foreground">
                            <span>
                              {b.budget_duration
                                ? durationLabel(b.budget_duration, t)
                                : t("budgets.mobile.noCycle")}
                            </span>
                            <span>
                              {b.soft_budget
                                ? `${t("budgets.mobile.alert")}: $${parseFloat(b.soft_budget).toFixed(2)}`
                                : ""}
                            </span>
                          </div>
                          <div className="text-xs text-muted-foreground">
                            {t("budgets.mobile.created")}:{" "}
                            {b.created_at ? formatDate(b.created_at) : "—"}
                          </div>
                          <div className="flex justify-end gap-1 pt-1">
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => openEdit(b)}
                            >
                              <Pencil className="h-3.5 w-3.5 mr-1" />{" "}
                              {t("common.edit")}
                            </Button>
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => openDelete(b)}
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
                    {search ? t("budgets.noMatch") : t("budgets.noBudgets")}
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
            <DialogTitle>{t("budgets.dialog.titleCreate")}</DialogTitle>
            <DialogDescription>
              {t("budgets.dialog.descriptionCreate")}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="b-name">{t("budgets.dialog.nameLabel")}</Label>
              <Input
                id="b-name"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
                placeholder={t("budgets.dialog.namePlaceholder")}
              />
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div>
                <Label htmlFor="b-max">{t("budgets.dialog.maxBudgetLabel")}</Label>
                <Input
                  id="b-max"
                  type="number"
                  value={formMaxBudget}
                  onChange={(e) => setFormMaxBudget(e.target.value)}
                  placeholder={t("budgets.dialog.maxBudgetPlaceholder")}
                />
              </div>
              <div>
                <Label htmlFor="b-soft">{t("budgets.dialog.softBudgetLabel")}</Label>
                <Input
                  id="b-soft"
                  type="number"
                  step="0.0001"
                  value={formSoftBudget}
                  onChange={(e) => setFormSoftBudget(e.target.value)}
                  placeholder={t("budgets.dialog.softBudgetPlaceholder")}
                />
              </div>
            </div>
            <div>
              <Label htmlFor="b-duration">{t("budgets.dialog.resetCycleLabel")}</Label>
              <select
                id="b-duration"
                className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
                value={formBudgetDuration}
                onChange={(e) => setFormBudgetDuration(e.target.value)}
              >
                <option value="">{t("budgets.resetCycleOptions.none")}</option>
                <option value="24h">{t("budgets.resetCycleOptions.daily")}</option>
                <option value="7d">{t("budgets.resetCycleOptions.weekly")}</option>
                <option value="30d">{t("budgets.resetCycleOptions.monthly")}</option>
              </select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={handleCreate} disabled={createMutation.isPending}>
              {createMutation.isPending && <Spinner className="mr-2" />}
              {t("common.create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Dialog */}
      <Dialog open={editOpen} onOpenChange={setEditOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("budgets.dialog.titleEdit")}</DialogTitle>
            <DialogDescription>
              {t("budgets.dialog.descriptionEdit", {
                name: selected ? displayName(selected) : "",
              })}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="be-name">{t("budgets.dialog.nameLabel")}</Label>
              <Input
                id="be-name"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
                placeholder={t("budgets.dialog.namePlaceholder")}
              />
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div>
                <Label htmlFor="be-max">{t("budgets.dialog.maxBudgetLabel")}</Label>
                <Input
                  id="be-max"
                  type="number"
                  value={formMaxBudget}
                  onChange={(e) => setFormMaxBudget(e.target.value)}
                />
              </div>
              <div>
                <Label htmlFor="be-soft">{t("budgets.dialog.softBudgetLabel")}</Label>
                <Input
                  id="be-soft"
                  type="number"
                  step="0.0001"
                  value={formSoftBudget}
                  onChange={(e) => setFormSoftBudget(e.target.value)}
                  placeholder={t("budgets.dialog.softBudgetPlaceholder")}
                />
              </div>
            </div>
            <div>
              <Label htmlFor="be-duration">{t("budgets.dialog.resetCycleLabel")}</Label>
              <select
                id="be-duration"
                className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
                value={formBudgetDuration}
                onChange={(e) => setFormBudgetDuration(e.target.value)}
              >
                <option value="">{t("budgets.resetCycleOptions.none")}</option>
                <option value="24h">{t("budgets.resetCycleOptions.daily")}</option>
                <option value="7d">{t("budgets.resetCycleOptions.weekly")}</option>
                <option value="30d">{t("budgets.resetCycleOptions.monthly")}</option>
              </select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={handleEdit} disabled={editMutation.isPending}>
              {editMutation.isPending && <Spinner className="mr-2" />}
              {t("budgets.dialog.saveBtn")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation */}
      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("budgets.dialog.titleDelete")}</DialogTitle>
            <DialogDescription>
              {t("budgets.dialog.descriptionDelete", {
                name: selected ? displayName(selected) : "",
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
                selected && deleteMutation.mutate(selected.budget_id)
              }
              disabled={deleteMutation.isPending}
            >
              {deleteMutation.isPending && <Spinner className="mr-2" />}
              {t("common.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
