import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
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

function durationLabel(d: string): string {
  const m: Record<string, string> = {
    "24h": "Daily (24h)",
    "7d": "Weekly (7d)",
    "30d": "Monthly (30d)",
  };
  return m[d] ?? d;
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
      toast.success("Budget created");
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
      toast.success("Budget updated");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const deleteMutation = useMutation({
    mutationFn: (budget_id: string) => apiPost("/budget/delete", { budget_id }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["budgets"] });
      setDeleteOpen(false);
      setSelected(null);
      toast.success("Budget deleted");
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
          <h1 className="text-2xl font-bold tracking-tight">Budgets</h1>
          <p className="text-sm text-muted-foreground">
            Manage budget limits and reset cycles
          </p>
        </div>
        <Button onClick={openCreate}>
          <Plus className="h-4 w-4" /> New Budget
        </Button>
      </div>

      <div className="flex items-center gap-2">
        <div className="relative flex-1 max-w-sm">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            placeholder="Search by name or ID..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="pl-9"
          />
        </div>
        <div className="flex-1" />
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle>All Budgets ({totalCount})</CardTitle>
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
                      <TableHead>Name</TableHead>
                      <TableHead>Budget ID</TableHead>
                      <TableHead>Budget Limit</TableHead>
                      <TableHead>Reset Cycle</TableHead>
                      <TableHead>Soft Alert</TableHead>
                      <TableHead>Created</TableHead>
                      <TableHead className="text-right">Actions</TableHead>
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
                                ? durationLabel(b.budget_duration)
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
                    {search ? "No budgets match your search" : "No budgets yet"}
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
                                ? durationLabel(b.budget_duration)
                                : "No cycle"}
                            </span>
                            <span>
                              {b.soft_budget
                                ? `Alert: $${parseFloat(b.soft_budget).toFixed(2)}`
                                : ""}
                            </span>
                          </div>
                          <div className="text-xs text-muted-foreground">
                            Created:{" "}
                            {b.created_at ? formatDate(b.created_at) : "—"}
                          </div>
                          <div className="flex justify-end gap-1 pt-1">
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => openEdit(b)}
                            >
                              <Pencil className="h-3.5 w-3.5 mr-1" /> Edit
                            </Button>
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => openDelete(b)}
                            >
                              <Trash2 className="h-3.5 w-3.5 mr-1 text-destructive" />{" "}
                              Delete
                            </Button>
                          </div>
                        </CardContent>
                      </Card>
                    ))}
                {!isLoading && filtered.length === 0 && (
                  <div className="text-center text-muted-foreground py-8">
                    {search ? "No budgets match your search" : "No budgets yet"}
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
            <DialogTitle>Create Budget</DialogTitle>
            <DialogDescription>
              Define a budget with optional limits and reset cycle.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="b-name">Budget Name</Label>
              <Input
                id="b-name"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
                placeholder="e.g. Monthly GPT-4 Budget"
              />
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div>
                <Label htmlFor="b-max">Max Budget ($)</Label>
                <Input
                  id="b-max"
                  type="number"
                  value={formMaxBudget}
                  onChange={(e) => setFormMaxBudget(e.target.value)}
                  placeholder="50"
                />
              </div>
              <div>
                <Label htmlFor="b-soft">Soft Budget Alert ($)</Label>
                <Input
                  id="b-soft"
                  type="number"
                  step="0.0001"
                  value={formSoftBudget}
                  onChange={(e) => setFormSoftBudget(e.target.value)}
                  placeholder="0.00 — warn but don't block"
                />
              </div>
            </div>
            <div>
              <Label htmlFor="b-duration">Reset Cycle</Label>
              <select
                id="b-duration"
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
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>
              Cancel
            </Button>
            <Button onClick={handleCreate} disabled={createMutation.isPending}>
              {createMutation.isPending && <Spinner className="mr-2" />}
              Create
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Dialog */}
      <Dialog open={editOpen} onOpenChange={setEditOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Edit Budget</DialogTitle>
            <DialogDescription>
              Update budget {selected ? displayName(selected) : ""}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="be-name">Budget Name</Label>
              <Input
                id="be-name"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
                placeholder="e.g. Monthly GPT-4 Budget"
              />
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div>
                <Label htmlFor="be-max">Max Budget ($)</Label>
                <Input
                  id="be-max"
                  type="number"
                  value={formMaxBudget}
                  onChange={(e) => setFormMaxBudget(e.target.value)}
                />
              </div>
              <div>
                <Label htmlFor="be-soft">Soft Budget Alert ($)</Label>
                <Input
                  id="be-soft"
                  type="number"
                  step="0.0001"
                  value={formSoftBudget}
                  onChange={(e) => setFormSoftBudget(e.target.value)}
                  placeholder="0.00 — warn but don't block"
                />
              </div>
            </div>
            <div>
              <Label htmlFor="be-duration">Reset Cycle</Label>
              <select
                id="be-duration"
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
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditOpen(false)}>
              Cancel
            </Button>
            <Button onClick={handleEdit} disabled={editMutation.isPending}>
              {editMutation.isPending && <Spinner className="mr-2" />}
              Save Changes
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation */}
      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Budget</DialogTitle>
            <DialogDescription>
              Delete budget "{selected ? displayName(selected) : ""}"? This
              action cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() =>
                selected && deleteMutation.mutate(selected.budget_id)
              }
              disabled={deleteMutation.isPending}
            >
              {deleteMutation.isPending && <Spinner className="mr-2" />}
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
