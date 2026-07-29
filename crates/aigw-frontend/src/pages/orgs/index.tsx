import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost, apiPut, apiDelete } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
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
import { Plus, Search, Pencil, Trash2, Building2 } from "lucide-react";
import { format } from "date-fns";

interface OrgItem {
  organization_id: string;
  organization_alias: string;
  budget_id: string | null;
  spend: number;
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
}

export function OrgsPage() {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [viewMode, setViewMode] = useState<"active" | "deleted">("active");
  const [createOpen, setCreateOpen] = useState(false);
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [selected, setSelected] = useState<OrgItem | null>(null);
  const [formAlias, setFormAlias] = useState("");

  const { data, isLoading, error } = useQuery<OrgListResponse>({
    queryKey: ["orgs"],
    queryFn: () => apiGet("/org/list"),
  });

  const { data: deletedOrgs = [], isLoading: deletedLoading } = useQuery<DeletedOrgItem[]>({
    queryKey: ["deleted-orgs"],
    queryFn: () => apiGet("/org/deleted"),
    enabled: viewMode === "deleted",
  });

  const orgs = data?.data ?? [];
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
      toast.success("Organization created");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const editMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPut("/org/update", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["orgs"] });
      setEditOpen(false);
      setSelected(null);
      toast.success("Organization updated");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const deleteMutation = useMutation({
    mutationFn: (org_id: string) => apiDelete(`/org/delete?organization_id=${encodeURIComponent(org_id)}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["orgs"] });
      setDeleteOpen(false);
      setSelected(null);
      toast.success("Organization deleted");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  function openCreate() { setFormAlias(""); setCreateOpen(true); }
  function openEdit(o: OrgItem) { setSelected(o); setFormAlias(o.organization_alias); setEditOpen(true); }
  function openDelete(o: OrgItem) { setSelected(o); setDeleteOpen(true); }

  function formatDate(d: string) {
    try { return format(new Date(d), "yyyy-MM-dd HH:mm"); } catch { return d; }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Organizations</h1>
          <p className="text-sm text-muted-foreground">Manage organizations</p>
        </div>
        {canEdit && (
          <Button onClick={openCreate}>
            <Plus className="h-4 w-4" /> New Org
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
          <button type="button" onClick={() => setViewMode("active")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${viewMode === "active" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground"}`}>Active</button>
          <button type="button" onClick={() => setViewMode("deleted")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${viewMode === "deleted" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground"}`}>Deleted</button>
        </div>
      </div>

      {viewMode === "deleted" && (
        <Card>
          <CardHeader className="pb-2"><CardTitle>Deleted Organizations ({deletedOrgs.length})</CardTitle></CardHeader>
          <CardContent>
            {deletedLoading ? (
              Array.from({ length: 3 }).map((_, i) => <div key={i} className="py-2"><Skeleton className="h-4 w-full" /></div>)
            ) : deletedOrgs.length === 0 ? (
              <div className="text-center text-muted-foreground py-8">No deleted records</div>
            ) : (
              <>
                <div className="hidden md:block">
                  <Table>
                    <TableHeader><TableRow>
                      <TableHead>Name</TableHead>
                      <TableHead>Organization ID</TableHead>
                      <TableHead className="text-right">Spend</TableHead>
                      <TableHead className="text-right">Deleted At</TableHead>
                    </TableRow></TableHeader>
                    <TableBody>
                      {deletedOrgs.map((o) => (
                        <TableRow key={o.id}>
                          <TableCell className="font-medium">{o.organization_alias}</TableCell>
                          <TableCell className="text-sm font-mono">{o.organization_id}</TableCell>
                          <TableCell className="text-right text-sm">${o.spend.toFixed(4)}</TableCell>
                          <TableCell className="text-right text-sm text-muted-foreground">{formatDate(o.deleted_at)}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div className="md:hidden space-y-3">
                  {deletedOrgs.map((o) => (
                    <Card key={o.id}><CardContent className="p-4 space-y-2">
                      <div className="flex items-center justify-between">
                        <span className="font-medium text-sm">{o.organization_alias}</span>
                        <span className="text-xs text-muted-foreground">${o.spend.toFixed(4)}</span>
                      </div>
                      <div className="text-xs text-muted-foreground">ID: {o.organization_id}</div>
                      <div className="text-xs text-muted-foreground">Deleted: {formatDate(o.deleted_at)}</div>
                    </CardContent></Card>
                  ))}
                </div>
              </>
            )}
          </CardContent>
        </Card>
      )}

      {viewMode === "active" && (<>

      <Card>
        <CardHeader className="pb-2"><CardTitle>All Organizations ({filtered.length})</CardTitle></CardHeader>
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
                      <TableHead>Budget ID</TableHead>
                      <TableHead className="text-right">Spend</TableHead>
                      <TableHead className="text-right">Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {isLoading
                      ? Array.from({ length: 3 }).map((_, i) => (
                          <TableRow key={i}>{Array.from({ length: 4 }).map((_, j) => <TableCell key={j}><Skeleton className="h-4 w-full" /></TableCell>)}</TableRow>
                        ))
                      : filtered.map((o) => (
                          <TableRow key={o.organization_id}>
                            <TableCell className="font-medium">{o.organization_alias}</TableCell>
                            <TableCell className="text-sm font-mono">{o.budget_id ?? "—"}</TableCell>
                            <TableCell className="text-right text-sm">${o.spend.toFixed(4)}</TableCell>
                            <TableCell className="text-right">
                              <div className="flex justify-end gap-1">
                                <Button variant="ghost" size="icon" onClick={() => openEdit(o)}><Pencil className="h-4 w-4" /></Button>
                                <Button variant="ghost" size="icon" onClick={() => openDelete(o)}><Trash2 className="h-4 w-4 text-destructive" /></Button>
                              </div>
                            </TableCell>
                          </TableRow>
                        ))}
                  </TableBody>
                </Table>
                {!isLoading && filtered.length === 0 && (
                  <div className="text-center text-muted-foreground py-8">{search ? "No orgs match" : "No orgs yet"}</div>
                )}
              </div>

              <div className="md:hidden space-y-3">
                {isLoading
                  ? Array.from({ length: 2 }).map((_, i) => (
                      <Card key={i}><CardContent className="p-4 space-y-2"><Skeleton className="h-4 w-3/4" /><Skeleton className="h-4 w-full" /></CardContent></Card>
                    ))
                  : filtered.map((o) => (
                      <Card key={o.organization_id}>
                        <CardContent className="p-4 space-y-2">
                          <div className="flex items-center justify-between">
                            <span className="font-medium text-sm">{o.organization_alias}</span>
                            <span className="text-xs text-muted-foreground">${o.spend.toFixed(4)}</span>
                          </div>
                          <div className="text-xs text-muted-foreground">Budget: {o.budget_id ?? "—"}</div>
                          <div className="flex justify-end gap-1 pt-1">
                            <Button variant="ghost" size="sm" onClick={() => openEdit(o)}><Pencil className="h-3.5 w-3.5 mr-1" /> Edit</Button>
                            <Button variant="ghost" size="sm" onClick={() => openDelete(o)}><Trash2 className="h-3.5 w-3.5 mr-1 text-destructive" /> Delete</Button>
                          </div>
                        </CardContent>
                      </Card>
                    ))}
                {!isLoading && filtered.length === 0 && (
                  <div className="text-center text-muted-foreground py-8">{search ? "No orgs match" : "No orgs yet"}</div>
                )}
              </div>
            </>
          )}
        </CardContent>
      </Card>

      {/* Create Dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader><DialogTitle>Create Organization</DialogTitle></DialogHeader>
          <div>
            <Label htmlFor="o-alias">Name</Label>
            <Input id="o-alias" value={formAlias} onChange={(e) => setFormAlias(e.target.value)} placeholder="Engineering" />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button onClick={() => createMutation.mutate({ organization_alias: formAlias })} disabled={createMutation.isPending || !formAlias.trim()}>
              {createMutation.isPending && <Spinner className="mr-2" />} Create
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Dialog */}
      <Dialog open={editOpen} onOpenChange={setEditOpen}>
        <DialogContent>
          <DialogHeader><DialogTitle>Edit Organization</DialogTitle></DialogHeader>
          <div>
            <Label htmlFor="oe-alias">Name</Label>
            <Input id="oe-alias" value={formAlias} onChange={(e) => setFormAlias(e.target.value)} />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditOpen(false)}>Cancel</Button>
            <Button onClick={() => selected && editMutation.mutate({ organization_id: selected.organization_id, organization_alias: formAlias })} disabled={editMutation.isPending}>
              {editMutation.isPending && <Spinner className="mr-2" />} Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete */}
      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Organization</DialogTitle>
            <DialogDescription>Delete <strong>{selected?.organization_alias}</strong>? It will be archived and viewable in the Deleted tab.</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteOpen(false)}>Cancel</Button>
            <Button variant="destructive" onClick={() => selected && deleteMutation.mutate(selected.organization_id)} disabled={deleteMutation.isPending}>
              {deleteMutation.isPending && <Spinner className="mr-2" />} Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
    </>
    )}
  );
}
