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
import { Plus, Search, Pencil, Trash2, Users2 } from "lucide-react";
import { format } from "date-fns";

interface TeamItem {
  team_id: string;
  team_alias: string;
  organization_id: string | null;
  members: string[];
  admins: string[];
  spend: number;
  max_budget: number | null;
  blocked: boolean;
}

interface DeletedTeamItem {
  id: number;
  team_id: string;
  team_alias: string | null;
  organization_id: string | null;
  spend: number;
  deleted_at: string;
}

interface TeamListResponse {
  data: TeamItem[];
}

export function TeamsPage() {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [viewMode, setViewMode] = useState<string>("active");
  const [createOpen, setCreateOpen] = useState(false);
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [selected, setSelected] = useState<TeamItem | null>(null);

  const [formAlias, setFormAlias] = useState("");
  const [formOrgId, setFormOrgId] = useState("");
  const [formBudget, setFormBudget] = useState("");

  const { data, isLoading, error } = useQuery<TeamListResponse>({
    queryKey: ["teams"],
    queryFn: () => apiGet("/team/list"),
  });

  const { data: deletedTeams = [], isLoading: deletedLoading } = useQuery<DeletedTeamItem[]>({
    queryKey: ["deleted-teams"],
    queryFn: () => apiGet("/team/deleted"),
    enabled: viewMode === "deleted",
  });

  const teams = data?.data ?? [];
  const canEdit = viewMode === "active";

  const filtered = useMemo(() => {
    if (!search.trim()) return teams;
    const q = search.toLowerCase();
    return teams.filter((t) => t.team_alias.toLowerCase().includes(q));
  }, [teams, search]);

  const createMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPost("/team/new", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["teams"] });
      setCreateOpen(false);
      toast.success("Team created");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const editMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPut("/team/update", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["teams"] });
      setEditOpen(false);
      setSelected(null);
      toast.success("Team updated");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const deleteMutation = useMutation({
    mutationFn: (team_id: string) => apiDelete(`/team/delete?team_id=${encodeURIComponent(team_id)}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["teams"] });
      setDeleteOpen(false);
      setSelected(null);
      toast.success("Team deleted");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  function openCreate() { setFormAlias(""); setFormOrgId(""); setFormBudget(""); setCreateOpen(true); }
  function openEdit(t: TeamItem) { setSelected(t); setFormAlias(t.team_alias); setFormOrgId(t.organization_id ?? ""); setFormBudget(t.max_budget?.toString() ?? ""); setEditOpen(true); }
  function openDelete(t: TeamItem) { setSelected(t); setDeleteOpen(true); }

  function formatDate(d: string) {
    try { return format(new Date(d), "yyyy-MM-dd HH:mm"); } catch { return d; }
  }

  if (viewMode === "deleted") {

    return (
      <Card>
        <CardHeader className="pb-2"><CardTitle>Deleted Teams ({deletedTeams.length})</CardTitle></CardHeader>
        <CardContent>
          {deletedLoading ? (
            Array.from({ length: 3 }).map((_, i) => (
              <div key={i} className="py-2"><Skeleton className="h-4 w-full" /></div>
            ))
          ) : deletedTeams.length === 0 ? (
            <div className="text-center text-muted-foreground py-8">No deleted records</div>
          ) : (
            <>
              <div className="hidden md:block">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Name</TableHead>
                      <TableHead>Team ID</TableHead>
                      <TableHead>Org</TableHead>
                      <TableHead className="text-right">Spend</TableHead>
                      <TableHead className="text-right">Deleted At</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {deletedTeams.map((t) => (
                      <TableRow key={t.id}>
                        <TableCell className="font-medium">{t.team_alias ?? "—"}</TableCell>
                        <TableCell className="text-sm font-mono">{t.team_id}</TableCell>
                        <TableCell className="text-sm">{t.organization_id ?? "—"}</TableCell>
                        <TableCell className="text-right text-sm">${t.spend.toFixed(4)}</TableCell>
                        <TableCell className="text-right text-sm text-muted-foreground">{formatDate(t.deleted_at)}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
              <div className="md:hidden space-y-3">
                {deletedTeams.map((t) => (
                  <Card key={t.id}>
                    <CardContent className="p-4 space-y-2">
                      <div className="flex items-center justify-between">
                        <span className="font-medium text-sm">{t.team_alias ?? "—"}</span>
                        <span className="text-xs text-muted-foreground">${t.spend.toFixed(4)}</span>
                      </div>
                      <div className="text-xs text-muted-foreground">ID: {t.team_id} | Org: {t.organization_id ?? "—"}</div>
                      <div className="text-xs text-muted-foreground">Deleted: {formatDate(t.deleted_at)}</div>
                    </CardContent>
                  </Card>
                ))}
              </div>
            </>
          )}
        </CardContent>
      </Card>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Teams</h1>
          <p className="text-sm text-muted-foreground">Manage teams and members</p>
        </div>
        {canEdit && (
          <Button onClick={openCreate}>
            <Plus className="h-4 w-4" /> New Team
          </Button>
        )}
      </div>

      <div className="flex items-center gap-2">
        {viewMode === "active" && (
          <div className="relative flex-1 max-w-sm">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input placeholder="Search by name..." value={search} onChange={(e) => setSearch(e.target.value)} className="pl-9" />
          </div>
        )}
        <div className="flex-1" />
        <div className="flex items-center rounded-md border p-0.5">
          <button
            type="button"
            onClick={() => setViewMode("active")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${
              viewMode === "active" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground"
            }`}
          >
            Active
          </button>
          <button
            type="button"
            onClick={() => setViewMode("deleted")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${
              viewMode === "deleted" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground"
            }`}
          >
            Deleted
          </button>
        </div>
      </div>

      <Card>
        <CardHeader className="pb-2"><CardTitle>All Teams ({filtered.length})</CardTitle></CardHeader>
        <CardContent>
          {error ? (
            <p className="text-sm text-destructive">{(error as Error).message}</p>
          ) : (
            <>
              <div className="hidden md:block">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Name</TableHead>
                      <TableHead>Org</TableHead>
                      <TableHead>Members</TableHead>
                      <TableHead>Status</TableHead>
                      <TableHead className="text-right">Spend</TableHead>
                      <TableHead className="text-right">Budget</TableHead>
                      <TableHead className="text-right">Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {isLoading
                      ? Array.from({ length: 3 }).map((_, i) => (
                          <TableRow key={i}>{Array.from({ length: 7 }).map((_, j) => <TableCell key={j}><Skeleton className="h-4 w-full" /></TableCell>)}</TableRow>
                        ))
                      : filtered.map((t) => (
                          <TableRow key={t.team_id}>
                            <TableCell className="font-medium">{t.team_alias}</TableCell>
                            <TableCell className="text-sm font-mono">{t.organization_id ?? "—"}</TableCell>
                            <TableCell className="text-sm">{t.members.length} members</TableCell>
                            <TableCell>{t.blocked ? <Badge variant="destructive">blocked</Badge> : <Badge variant="default">active</Badge>}</TableCell>
                            <TableCell className="text-right text-sm">${t.spend.toFixed(4)}</TableCell>
                            <TableCell className="text-right text-sm">{t.max_budget != null ? `$${t.max_budget.toFixed(2)}` : "∞"}</TableCell>
                            <TableCell className="text-right">
                              <div className="flex justify-end gap-1">
                                <Button variant="ghost" size="icon" onClick={() => openEdit(t)}><Pencil className="h-4 w-4" /></Button>
                                <Button variant="ghost" size="icon" onClick={() => openDelete(t)}><Trash2 className="h-4 w-4 text-destructive" /></Button>
                              </div>
                            </TableCell>
                          </TableRow>
                        ))}
                  </TableBody>
                </Table>
                {!isLoading && filtered.length === 0 && (
                  <div className="text-center text-muted-foreground py-8">{search ? "No teams match" : "No teams yet"}</div>
                )}
              </div>

              <div className="md:hidden space-y-3">
                {isLoading
                  ? Array.from({ length: 2 }).map((_, i) => (
                      <Card key={i}><CardContent className="p-4 space-y-2"><Skeleton className="h-4 w-3/4" /><Skeleton className="h-4 w-full" /></CardContent></Card>
                    ))
                  : filtered.map((t) => (
                      <Card key={t.team_id}>
                        <CardContent className="p-4 space-y-2">
                          <div className="flex items-center justify-between">
                            <span className="font-medium text-sm">{t.team_alias}</span>
                            {t.blocked ? <Badge variant="destructive" className="text-xs">blocked</Badge> : <Badge variant="default" className="text-xs">active</Badge>}
                          </div>
                          <div className="text-xs text-muted-foreground">Org: {t.organization_id ?? "—"} | {t.members.length} members</div>
                          <div className="flex items-center justify-between text-xs text-muted-foreground">
                            <span>Spent ${t.spend.toFixed(4)}</span>
                            <span>{t.max_budget != null ? `Budget $${t.max_budget.toFixed(2)}` : "No budget"}</span>
                          </div>
                          <div className="flex justify-end gap-1 pt-1">
                            <Button variant="ghost" size="sm" onClick={() => openEdit(t)}><Pencil className="h-3.5 w-3.5 mr-1" /> Edit</Button>
                            <Button variant="ghost" size="sm" onClick={() => openDelete(t)}><Trash2 className="h-3.5 w-3.5 mr-1 text-destructive" /> Delete</Button>
                          </div>
                        </CardContent>
                      </Card>
                    ))}
                {!isLoading && filtered.length === 0 && (
                  <div className="text-center text-muted-foreground py-8">{search ? "No teams match" : "No teams yet"}</div>
                )}
              </div>
            </>
          )}
        </CardContent>
      </Card>

      {/* Create Dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader><DialogTitle>Create Team</DialogTitle><DialogDescription>Add a new team.</DialogDescription></DialogHeader>
          <div className="space-y-4">
            <div><Label htmlFor="t-alias">Name</Label><Input id="t-alias" value={formAlias} onChange={(e) => setFormAlias(e.target.value)} placeholder="AI Team" /></div>
            <div><Label htmlFor="t-org">Organization ID</Label><Input id="t-org" value={formOrgId} onChange={(e) => setFormOrgId(e.target.value)} placeholder="org-1" /></div>
            <div><Label htmlFor="t-budget">Max Budget ($)</Label><Input id="t-budget" type="number" value={formBudget} onChange={(e) => setFormBudget(e.target.value)} placeholder="500" /></div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button onClick={() => createMutation.mutate({ team_alias: formAlias, organization_id: formOrgId || undefined, ...(formBudget && { max_budget: parseFloat(formBudget) }) })} disabled={createMutation.isPending || !formAlias.trim()}>
              {createMutation.isPending && <Spinner className="mr-2" />} Create
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Dialog */}
      <Dialog open={editOpen} onOpenChange={setEditOpen}>
        <DialogContent>
          <DialogHeader><DialogTitle>Edit Team</DialogTitle></DialogHeader>
          <div className="space-y-4">
            <div><Label htmlFor="te-alias">Name</Label><Input id="te-alias" value={formAlias} onChange={(e) => setFormAlias(e.target.value)} /></div>
            <div><Label htmlFor="te-org">Organization ID</Label><Input id="te-org" value={formOrgId} onChange={(e) => setFormOrgId(e.target.value)} /></div>
            <div><Label htmlFor="te-budget">Max Budget ($)</Label><Input id="te-budget" type="number" value={formBudget} onChange={(e) => setFormBudget(e.target.value)} /></div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditOpen(false)}>Cancel</Button>
            <Button onClick={() => selected && editMutation.mutate({ team_id: selected.team_id, team_alias: formAlias, ...(formOrgId && { organization_id: formOrgId }), ...(formBudget && { max_budget: parseFloat(formBudget) }) })} disabled={editMutation.isPending}>
              {editMutation.isPending && <Spinner className="mr-2" />} Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete */}
      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Team</DialogTitle>
            <DialogDescription>Delete <strong>{selected?.team_alias}</strong>? It will be archived and viewable in the Deleted tab.</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteOpen(false)}>Cancel</Button>
            <Button variant="destructive" onClick={() => selected && deleteMutation.mutate(selected.team_id)} disabled={deleteMutation.isPending}>
              {deleteMutation.isPending && <Spinner className="mr-2" />} Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
