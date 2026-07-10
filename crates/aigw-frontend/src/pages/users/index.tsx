import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost, apiPut, apiDelete } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter,
  DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { toast } from "sonner";
import { Plus, Search, Pencil, Trash2, Users, ChevronLeft, ChevronRight } from "lucide-react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

interface UserItem {
  user_id: string;
  user_alias: string | null;
  user_email: string | null;
  user_role: string | null;
  spend: number;
  max_budget: number | null;
  tpm_limit: number | null;
  rpm_limit: number | null;
  organization_id: string | null;
  team_id: string | null;
}

interface UserListResponse {
  data: UserItem[];
  total_count: number;
  page: number;
  page_size: number;
  total_pages: number;
}

export function UsersPage() {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(10);
  const [createOpen, setCreateOpen] = useState(false);
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [selected, setSelected] = useState<UserItem | null>(null);

  const [formEmail, setFormEmail] = useState("");
  const [formPassword, setFormPassword] = useState("");
  const [formAlias, setFormAlias] = useState("");
  const [formRole, setFormRole] = useState("internal_user");
  const [formBudget, setFormBudget] = useState("");
  const [formTPM, setFormTPM] = useState("");
  const [formRPM, setFormRPM] = useState("");

  const { data, isLoading, error } = useQuery<UserListResponse>({
    queryKey: ["users", page, pageSize],
    queryFn: () => apiGet(`/user/list?page=${page}&page_size=${pageSize}`),
  });

  const users = data?.data ?? [];
  const totalCount = data?.total_count ?? 0;
  const totalPages = data?.total_pages ?? 0;

  const filtered = useMemo(() => {
    if (!search.trim()) return users;
    const q = search.toLowerCase();
    return users.filter(
      (u) =>
        (u.user_alias ?? "").toLowerCase().includes(q) ||
        (u.user_email ?? "").toLowerCase().includes(q) ||
        (u.user_role ?? "").toLowerCase().includes(q),
    );
  }, [users, search]);

  const createMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPost("/user/new", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["users"] });
      setCreateOpen(false);
      toast.success("User created");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const editMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPut("/user/update", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["users"] });
      setEditOpen(false);
      setSelected(null);
      toast.success("User updated");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const deleteMutation = useMutation({
    mutationFn: (user_id: string) =>
      apiDelete(`/user/delete?user_id=${encodeURIComponent(user_id)}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["users"] });
      setDeleteOpen(false);
      setSelected(null);
      toast.success("User deleted");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  function openCreate() {
    setFormEmail(""); setFormPassword(""); setFormAlias("");
    setFormRole("internal_user"); setFormBudget(""); setFormTPM(""); setFormRPM("");
    setCreateOpen(true);
  }

  function openEdit(u: UserItem) {
    setSelected(u);
    setFormAlias(u.user_alias ?? "");
    setFormEmail(u.user_email ?? "");
    setFormRole(u.user_role ?? "internal_user");
    setFormBudget(u.max_budget?.toString() ?? "");
    setFormTPM(u.tpm_limit?.toString() ?? "");
    setFormRPM(u.rpm_limit?.toString() ?? "");
    setEditOpen(true);
  }

  function openDelete(u: UserItem) {
    setSelected(u);
    setDeleteOpen(true);
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Users</h1>
          <p className="text-sm text-muted-foreground">Manage user accounts and roles</p>
        </div>
        <Button onClick={openCreate}>
          <Plus className="h-4 w-4" /> New User
        </Button>
      </div>

      <div className="relative">
        <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          placeholder="Search by alias, email, or role..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="pl-9 max-w-sm"
        />
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle>All Users ({totalCount})</CardTitle>
        </CardHeader>
        <CardContent>
          {error ? (
            <p className="text-sm text-destructive">{(error as Error).message}</p>
          ) : (
            <>
              {/* Desktop table */}
              <div className="hidden md:block">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Alias</TableHead>
                      <TableHead>Email</TableHead>
                      <TableHead>Role</TableHead>
                      <TableHead className="text-right">Spend</TableHead>
                      <TableHead className="text-right">Budget</TableHead>
                      <TableHead className="text-right">Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {isLoading
                      ? Array.from({ length: 3 }).map((_, i) => (
                          <TableRow key={i}>
                            {Array.from({ length: 6 }).map((_, j) => (
                              <TableCell key={j}><Skeleton className="h-4 w-full" /></TableCell>
                            ))}
                          </TableRow>
                        ))
                      : filtered.map((u) => (
                          <TableRow key={u.user_id}>
                            <TableCell className="font-medium">{u.user_alias ?? "—"}</TableCell>
                            <TableCell className="text-sm">{u.user_email ?? "—"}</TableCell>
                            <TableCell>
                              <Badge variant={u.user_role === "proxy_admin" ? "default" : "secondary"}>
                                {u.user_role ?? "—"}
                              </Badge>
                            </TableCell>
                            <TableCell className="text-right text-sm">${u.spend.toFixed(4)}</TableCell>
                            <TableCell className="text-right text-sm">
                              {u.max_budget != null ? `$${u.max_budget.toFixed(2)}` : "∞"}
                            </TableCell>
                            <TableCell className="text-right">
                              <div className="flex justify-end gap-1">
                                <Button variant="ghost" size="icon" onClick={() => openEdit(u)}>
                                  <Pencil className="h-4 w-4" />
                                </Button>
                                <Button variant="ghost" size="icon" onClick={() => openDelete(u)}>
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
                    {search ? "No users match your search" : "No users yet"}
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
                          <Skeleton className="h-4 w-1/2" />
                        </CardContent>
                      </Card>
                    ))
                  : filtered.map((u) => (
                      <Card key={u.user_id}>
                        <CardContent className="p-4 space-y-2">
                          <div className="flex items-center justify-between">
                            <span className="font-medium text-sm truncate max-w-[60%]">
                              {u.user_alias ?? u.user_email ?? "—"}
                            </span>
                            <Badge variant={u.user_role === "proxy_admin" ? "default" : "secondary"} className="text-xs">
                              {u.user_role ?? "—"}
                            </Badge>
                          </div>
                          <div className="text-xs text-muted-foreground">{u.user_email ?? "—"}</div>
                          <div className="flex items-center justify-between text-xs text-muted-foreground">
                            <span>Spent ${u.spend.toFixed(4)}</span>
                            <span>{u.max_budget != null ? `Budget $${u.max_budget.toFixed(2)}` : "No budget"}</span>
                          </div>
                          <div className="flex justify-end gap-1 pt-1">
                            <Button variant="ghost" size="sm" onClick={() => openEdit(u)}>
                              <Pencil className="h-3.5 w-3.5 mr-1" /> Edit
                            </Button>
                            <Button variant="ghost" size="sm" onClick={() => openDelete(u)}>
                              <Trash2 className="h-3.5 w-3.5 mr-1 text-destructive" /> Delete
                            </Button>
                          </div>
                        </CardContent>
                      </Card>
                    ))}
                {!isLoading && filtered.length === 0 && (
                  <div className="text-center text-muted-foreground py-8">
                    {search ? "No users match your search" : "No users yet"}
                  </div>
                )}
              </div>
              {/* Pagination */}
              {totalCount > 0 && (
                <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-2 mt-4 pt-3 border-t">
                  <div className="flex items-center gap-3">
                    <span className="text-xs text-muted-foreground">
                      Showing {Math.min((page - 1) * pageSize + 1, totalCount)}–{Math.min(page * pageSize, totalCount)} of {totalCount}
                    </span>
                    <span className="text-xs text-muted-foreground">
                      Page {page} of {Math.max(totalPages, 1)}
                    </span>
                  </div>
                  <div className="flex items-center gap-2">
                    <Select value={String(pageSize)} onValueChange={(v) => { setPageSize(Number(v)); setPage(1); }}>
                      <SelectTrigger className="h-7 w-[70px] text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="10">10</SelectItem>
                        <SelectItem value="25">25</SelectItem>
                        <SelectItem value="50">50</SelectItem>
                      </SelectContent>
                    </Select>
                    <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => setPage(page - 1)} className="h-7 px-2">
                      <ChevronLeft className="h-3.5 w-3.5" />
                    </Button>
                    <Button variant="outline" size="sm" disabled={page >= totalPages || totalPages === 0} onClick={() => setPage(page + 1)} className="h-7 px-2">
                      <ChevronRight className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </div>
              )}
            </>
          )}
        </CardContent>
      </Card>

      {/* Create Dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create User</DialogTitle>
            <DialogDescription>Add a new user account.</DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="u-email">Email</Label>
              <Input id="u-email" value={formEmail} onChange={(e) => setFormEmail(e.target.value)} placeholder="user@example.com" />
            </div>
            <div>
              <Label htmlFor="u-password">Password</Label>
              <Input id="u-password" type="password" value={formPassword} onChange={(e) => setFormPassword(e.target.value)} placeholder="••••••" />
            </div>
            <div>
              <Label htmlFor="u-alias">Alias</Label>
              <Input id="u-alias" value={formAlias} onChange={(e) => setFormAlias(e.target.value)} placeholder="Display name" />
            </div>
            <div>
              <Label htmlFor="u-role">Role</Label>
              <select
                id="u-role"
                value={formRole}
                onChange={(e) => setFormRole(e.target.value)}
                className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background"
              >
                <option value="proxy_admin">proxy_admin</option>
                <option value="internal_user">internal_user</option>
                <option value="internal_user_viewer">internal_user_viewer</option>
              </select>
            </div>
            <div className="grid grid-cols-3 gap-4">
              <div>
                <Label htmlFor="u-budget">Max Budget ($)</Label>
                <Input id="u-budget" type="number" value={formBudget} onChange={(e) => setFormBudget(e.target.value)} placeholder="100" />
              </div>
              <div>
                <Label htmlFor="u-tpm">TPM Limit</Label>
                <Input id="u-tpm" type="number" value={formTPM} onChange={(e) => setFormTPM(e.target.value)} placeholder="100000" />
              </div>
              <div>
                <Label htmlFor="u-rpm">RPM Limit</Label>
                <Input id="u-rpm" type="number" value={formRPM} onChange={(e) => setFormRPM(e.target.value)} placeholder="100" />
              </div>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button onClick={() => createMutation.mutate({
              user_email: formEmail,
              password: formPassword,
              user_alias: formAlias || undefined,
              user_role: formRole,
              ...(formBudget && { max_budget: parseFloat(formBudget) }),
              ...(formTPM && { tpm_limit: parseInt(formTPM) }),
              ...(formRPM && { rpm_limit: parseInt(formRPM) }),
            })} disabled={createMutation.isPending || !formEmail.trim() || !formPassword.trim()}>
              {createMutation.isPending && <Spinner className="mr-2" />} Create
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Dialog */}
      <Dialog open={editOpen} onOpenChange={setEditOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Edit User</DialogTitle>
            <DialogDescription>Update {selected?.user_alias ?? selected?.user_id?.slice(0, 8)}</DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="ue-alias">Alias</Label>
              <Input id="ue-alias" value={formAlias} onChange={(e) => setFormAlias(e.target.value)} />
            </div>
            <div>
              <Label htmlFor="ue-email">Email</Label>
              <Input id="ue-email" value={formEmail} onChange={(e) => setFormEmail(e.target.value)} />
            </div>
            <div>
              <Label htmlFor="ue-role">Role</Label>
              <select
                id="ue-role"
                value={formRole}
                onChange={(e) => setFormRole(e.target.value)}
                className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background"
              >
                <option value="proxy_admin">proxy_admin</option>
                <option value="internal_user">internal_user</option>
                <option value="internal_user_viewer">internal_user_viewer</option>
              </select>
            </div>
            <div className="grid grid-cols-3 gap-4">
              <div>
                <Label htmlFor="ue-budget">Max Budget ($)</Label>
                <Input id="ue-budget" type="number" value={formBudget} onChange={(e) => setFormBudget(e.target.value)} />
              </div>
              <div>
                <Label htmlFor="ue-tpm">TPM Limit</Label>
                <Input id="ue-tpm" type="number" value={formTPM} onChange={(e) => setFormTPM(e.target.value)} />
              </div>
              <div>
                <Label htmlFor="ue-rpm">RPM Limit</Label>
                <Input id="ue-rpm" type="number" value={formRPM} onChange={(e) => setFormRPM(e.target.value)} />
              </div>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditOpen(false)}>Cancel</Button>
            <Button onClick={() => selected && editMutation.mutate({
              user_id: selected.user_id,
              ...(formAlias && { user_alias: formAlias }),
              ...(formEmail && { user_email: formEmail }),
              ...(formRole && { user_role: formRole }),
              ...(formBudget && { max_budget: parseFloat(formBudget) }),
              ...(formTPM && { tpm_limit: parseInt(formTPM) }),
              ...(formRPM && { rpm_limit: parseInt(formRPM) }),
            })} disabled={editMutation.isPending}>
              {editMutation.isPending && <Spinner className="mr-2" />} Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation */}
      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete User</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete <strong>{selected?.user_alias ?? selected?.user_id?.slice(0, 8)}</strong>? This cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteOpen(false)}>Cancel</Button>
            <Button variant="destructive" onClick={() => selected && deleteMutation.mutate(selected.user_id)} disabled={deleteMutation.isPending}>
              {deleteMutation.isPending && <Spinner className="mr-2" />} Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
