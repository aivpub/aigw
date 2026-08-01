import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { apiGet, apiPost, apiPut, apiDelete } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { PaginationBar } from "@/components/ui/pagination";
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
  created_at: string | null;
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
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

interface DeletedTeamListResponse {
  data: DeletedTeamItem[];
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

export function TeamsPage() {
  const { t } = useTranslation();
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
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);
  const [deletedPage, setDeletedPage] = useState(1);
  const [deletedPageSize, setDeletedPageSize] = useState(30);

  const { data, isLoading, error } = useQuery<TeamListResponse>({
    queryKey: ["teams", page, pageSize],
    queryFn: () => apiGet(`/team/list?page=${page}&page_size=${pageSize}`),
  });

  const { data: deletedData, isLoading: deletedLoading } = useQuery<DeletedTeamListResponse>({
    queryKey: ["deleted-teams", deletedPage, deletedPageSize],
    queryFn: () => apiGet(`/team/deleted?page=${deletedPage}&page_size=${deletedPageSize}`),
    enabled: viewMode === "deleted",
  });

  const teams = data?.data ?? [];
  const totalCount = data?.total_count ?? teams.length;
  const totalPages = data?.total_pages ?? (teams.length === 0 ? 1 : Math.ceil(totalCount / pageSize));

  const deletedTeams = deletedData?.data ?? [];
  const deletedTotalCount = deletedData?.total_count ?? deletedTeams.length;
  const deletedTotalPages = deletedData?.total_pages ?? (deletedTeams.length === 0 ? 1 : Math.ceil(deletedTotalCount / deletedPageSize));

  const filtered = useMemo(() => {
    if (!search.trim()) return teams;
    const q = search.toLowerCase();
    return teams.filter((team) => team.team_alias.toLowerCase().includes(q));
  }, [teams, search]);

  const createMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPost("/team/new", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["teams"] });
      setCreateOpen(false);
      toast.success(t("teams.toast.created"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const editMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPut("/team/update", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["teams"] });
      setEditOpen(false);
      setSelected(null);
      toast.success(t("teams.toast.updated"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const deleteMutation = useMutation({
    mutationFn: (team_id: string) => apiDelete(`/team/delete?team_id=${encodeURIComponent(team_id)}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["teams"] });
      setDeleteOpen(false);
      setSelected(null);
      toast.success(t("teams.toast.deleted"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  function openCreate() { setFormAlias(""); setFormOrgId(""); setFormBudget(""); setCreateOpen(true); }
  function openEdit(team: TeamItem) { setSelected(team); setFormAlias(team.team_alias); setFormOrgId(team.organization_id ?? ""); setFormBudget(team.max_budget?.toString() ?? ""); setEditOpen(true); }
  function openDelete(team: TeamItem) { setSelected(team); setDeleteOpen(true); }

  function formatDate(d: string) {
    try { return format(new Date(d), "yyyy-MM-dd HH:mm"); } catch { return d; }
  }

  const isActive = viewMode === "active" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground";
  const isDeleted = viewMode === "deleted" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground";

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">{t("teams.title")}</h1>
          <p className="text-sm text-muted-foreground">{t("teams.description")}</p>
        </div>
        {viewMode === "active" && (
          <Button onClick={openCreate}>
            <Plus className="h-4 w-4" /> {t("teams.newTeam")}
          </Button>
        )}
      </div>

      <div className="flex items-center gap-2">
        {viewMode === "active" && (
          <div className="relative flex-1 max-w-sm">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input placeholder={t("teams.searchPlaceholder")} value={search} onChange={(e) => setSearch(e.target.value)} className="pl-9" />
          </div>
        )}
        <div className="flex-1" />
        <div className="flex items-center rounded-md border p-0.5">
          <button type="button" onClick={() => setViewMode("active")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${isActive}`}>{t("keys.viewMode.active")}</button>
          <button type="button" onClick={() => setViewMode("deleted")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${isDeleted}`}>{t("keys.viewMode.deleted")}</button>
        </div>
      </div>

      {viewMode === "deleted" ? (
        <Card>
          <CardHeader className="pb-2"><CardTitle>{t("teams.deletedCardTitle", { count: deletedTotalCount })}</CardTitle></CardHeader>
          <CardContent>
            {deletedLoading ? (
              Array.from({ length: 3 }).map((_, i) => <div key={i} className="py-2"><Skeleton className="h-4 w-full" /></div>)
            ) : deletedTeams.length === 0 ? (
              <div className="text-center text-muted-foreground py-8">{t("teams.noDeletedRecords")}</div>
            ) : (
              <>
                <PaginationBar
                  page={deletedPage}
                  pageSize={deletedPageSize}
                  totalCount={deletedTotalCount}
                  totalPages={deletedTotalPages}
                  onPage={setDeletedPage}
                  onPageSize={(s) => { setDeletedPageSize(s); setDeletedPage(1); }}
                />
                <div className="hidden md:block">
                  <Table>
                    <TableHeader><TableRow>
                      <TableHead>{t("teams.table.name")}</TableHead>
                      <TableHead>{t("teams.table.teamId")}</TableHead>
                      <TableHead>{t("teams.table.org")}</TableHead>
                      <TableHead className="text-right">{t("teams.table.spend")}</TableHead>
                      <TableHead className="text-right">{t("teams.table.deletedAt")}</TableHead>
                    </TableRow></TableHeader>
                    <TableBody>
                      {deletedTeams.map((dt) => (
                        <TableRow key={dt.id}>
                          <TableCell className="font-medium">{dt.team_alias ?? "—"}</TableCell>
                          <TableCell className="text-sm font-mono">{dt.team_id}</TableCell>
                          <TableCell className="text-sm">{dt.organization_id ?? "—"}</TableCell>
                          <TableCell className="text-right text-sm">${dt.spend.toFixed(4)}</TableCell>
                          <TableCell className="text-right text-sm text-muted-foreground">{formatDate(dt.deleted_at)}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div className="md:hidden space-y-3">
                  {deletedTeams.map((dt) => (
                    <Card key={dt.id}><CardContent className="p-4 space-y-2">
                      <div className="flex items-center justify-between">
                        <span className="font-medium text-sm">{dt.team_alias ?? "—"}</span>
                        <span className="text-xs text-muted-foreground">${dt.spend.toFixed(4)}</span>
                      </div>
                      <div className="text-xs text-muted-foreground">{t("teams.mobile.id")}: {dt.team_id} | {t("teams.mobile.org")}: {dt.organization_id ?? "—"}</div>
                      <div className="text-xs text-muted-foreground">{t("teams.mobile.deleted")}: {formatDate(dt.deleted_at)}</div>
                    </CardContent></Card>
                  ))}
                </div>
                <div className="mt-3">
                  <PaginationBar
                    page={deletedPage}
                    pageSize={deletedPageSize}
                    totalCount={deletedTotalCount}
                    totalPages={deletedTotalPages}
                    onPage={setDeletedPage}
                    onPageSize={(s) => { setDeletedPageSize(s); setDeletedPage(1); }}
                  />
                </div>
              </>
            )}
          </CardContent>
        </Card>
      ) : (
        <>
          <Card>
            <CardHeader className="pb-2"><CardTitle>{t("teams.allCardTitle", { count: totalCount })}</CardTitle></CardHeader>
            <CardContent>
              {error ? (
                <p className="text-sm text-destructive">{(error as Error).message}</p>
              ) : (
                <>
                  <PaginationBar
                    page={page}
                    pageSize={pageSize}
                    totalCount={totalCount}
                    totalPages={totalPages}
                    onPage={setPage}
                    onPageSize={(s) => { setPageSize(s); setPage(1); }}
                  />
                  <div className="hidden md:block">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>{t("teams.table.name")}</TableHead>
                          <TableHead>{t("teams.table.org")}</TableHead>
                          <TableHead>{t("teams.table.members")}</TableHead>
                          <TableHead>{t("teams.table.status")}</TableHead>
                          <TableHead className="text-right">{t("teams.table.spend")}</TableHead>
                          <TableHead className="text-right">{t("teams.table.budget")}</TableHead>
                          <TableHead>{t("teams.table.created")}</TableHead>
                          <TableHead className="text-right">{t("teams.table.actions")}</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {isLoading
                          ? Array.from({ length: 3 }).map((_, i) => (
                              <TableRow key={i}>{Array.from({ length: 8 }).map((_, j) => <TableCell key={j}><Skeleton className="h-4 w-full" /></TableCell>)}</TableRow>
                            ))
                          : filtered.map((team) => (
                              <TableRow key={team.team_id}>
                                <TableCell className="font-medium">{team.team_alias}</TableCell>
                                <TableCell className="text-sm font-mono">{team.organization_id ?? "—"}</TableCell>
                                <TableCell className="text-sm">{t("teams.membersCount", { count: team.members.length })}</TableCell>
                                <TableCell>{team.blocked ? <Badge variant="destructive">{t("keys.blocked")}</Badge> : <Badge variant="default">{t("teams.active")}</Badge>}</TableCell>
                                <TableCell className="text-right text-sm">${team.spend.toFixed(4)}</TableCell>
                                <TableCell className="text-right text-sm">{team.max_budget != null ? `$${team.max_budget.toFixed(2)}` : "∞"}</TableCell>
                                <TableCell className="text-xs text-muted-foreground">{team.created_at ? formatDate(team.created_at) : "—"}</TableCell>
                                <TableCell className="text-right">
                                  <div className="flex justify-end gap-1">
                                    <Button variant="ghost" size="icon" onClick={() => openEdit(team)}><Pencil className="h-4 w-4" /></Button>
                                    <Button variant="ghost" size="icon" onClick={() => openDelete(team)}><Trash2 className="h-4 w-4 text-destructive" /></Button>
                                  </div>
                                </TableCell>
                              </TableRow>
                            ))}
                      </TableBody>
                    </Table>
                    {!isLoading && filtered.length === 0 && (
                      <div className="text-center text-muted-foreground py-8">{search ? t("teams.noMatch") : t("teams.noTeams")}</div>
                    )}
                  </div>

                  <div className="md:hidden space-y-3">
                    {isLoading
                      ? Array.from({ length: 2 }).map((_, i) => (
                          <Card key={i}><CardContent className="p-4 space-y-2"><Skeleton className="h-4 w-3/4" /><Skeleton className="h-4 w-full" /></CardContent></Card>
                        ))
                      : filtered.map((team) => (
                          <Card key={team.team_id}>
                            <CardContent className="p-4 space-y-2">
                              <div className="flex items-center justify-between">
                                <span className="font-medium text-sm">{team.team_alias}</span>
                                {team.blocked ? <Badge variant="destructive" className="text-xs">{t("keys.blocked")}</Badge> : <Badge variant="default" className="text-xs">{t("teams.active")}</Badge>}
                              </div>
                              <div className="text-xs text-muted-foreground">{t("teams.mobile.org")}: {team.organization_id ?? "—"} | {t("teams.membersCount", { count: team.members.length })}</div>
                              <div className="flex items-center justify-between text-xs text-muted-foreground">
                                <span>{t("teams.mobile.spent")} ${team.spend.toFixed(4)}</span>
                                <span>{team.max_budget != null ? `${t("teams.mobile.budget")} $${team.max_budget.toFixed(2)}` : t("teams.noBudget")}</span>
                              </div>
                              <div className="text-xs text-muted-foreground">{t("teams.mobile.created")}: {team.created_at ? formatDate(team.created_at) : "—"}</div>
                              <div className="flex justify-end gap-1 pt-1">
                                <Button variant="ghost" size="sm" onClick={() => openEdit(team)}><Pencil className="h-3.5 w-3.5 mr-1" /> {t("common.edit")}</Button>
                                <Button variant="ghost" size="sm" onClick={() => openDelete(team)}><Trash2 className="h-3.5 w-3.5 mr-1 text-destructive" /> {t("common.delete")}</Button>
                              </div>
                            </CardContent>
                          </Card>
                        ))}
                    {!isLoading && filtered.length === 0 && (
                      <div className="text-center text-muted-foreground py-8">{search ? t("teams.noMatch") : t("teams.noTeams")}</div>
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
                        onPageSize={(s) => { setPageSize(s); setPage(1); }}
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
              <DialogHeader><DialogTitle>{t("teams.dialog.titleCreate")}</DialogTitle><DialogDescription>{t("teams.dialog.descriptionCreate")}</DialogDescription></DialogHeader>
              <div className="space-y-4">
                <div><Label htmlFor="t-alias">{t("teams.dialog.nameLabel")}</Label><Input id="t-alias" value={formAlias} onChange={(e) => setFormAlias(e.target.value)} placeholder={t("teams.dialog.namePlaceholder")} /></div>
                <div><Label htmlFor="t-org">{t("teams.dialog.orgIdLabel")}</Label><Input id="t-org" value={formOrgId} onChange={(e) => setFormOrgId(e.target.value)} placeholder={t("teams.dialog.orgIdPlaceholder")} /></div>
                <div><Label htmlFor="t-budget">{t("teams.dialog.budgetLabel")}</Label><Input id="t-budget" type="number" value={formBudget} onChange={(e) => setFormBudget(e.target.value)} placeholder={t("teams.dialog.budgetPlaceholder")} /></div>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={() => setCreateOpen(false)}>{t("common.cancel")}</Button>
                <Button onClick={() => createMutation.mutate({ team_alias: formAlias, organization_id: formOrgId || undefined, ...(formBudget && { max_budget: parseFloat(formBudget) }) })} disabled={createMutation.isPending || !formAlias.trim()}>
                  {createMutation.isPending && <Spinner className="mr-2" />} {t("common.create")}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          {/* Edit Dialog */}
          <Dialog open={editOpen} onOpenChange={setEditOpen}>
            <DialogContent>
              <DialogHeader><DialogTitle>{t("teams.dialog.titleEdit")}</DialogTitle></DialogHeader>
              <div className="space-y-4">
                <div><Label htmlFor="te-alias">{t("teams.dialog.nameLabel")}</Label><Input id="te-alias" value={formAlias} onChange={(e) => setFormAlias(e.target.value)} /></div>
                <div><Label htmlFor="te-org">{t("teams.dialog.orgIdLabel")}</Label><Input id="te-org" value={formOrgId} onChange={(e) => setFormOrgId(e.target.value)} /></div>
                <div><Label htmlFor="te-budget">{t("teams.dialog.budgetLabel")}</Label><Input id="te-budget" type="number" value={formBudget} onChange={(e) => setFormBudget(e.target.value)} /></div>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={() => setEditOpen(false)}>{t("common.cancel")}</Button>
                <Button onClick={() => selected && editMutation.mutate({ team_id: selected.team_id, team_alias: formAlias, ...(formOrgId && { organization_id: formOrgId }), ...(formBudget && { max_budget: parseFloat(formBudget) }) })} disabled={editMutation.isPending}>
                  {editMutation.isPending && <Spinner className="mr-2" />} {t("common.save")}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          {/* Delete */}
          <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t("teams.dialog.titleDelete")}</DialogTitle>
                <DialogDescription>{t("teams.dialog.confirmDelete", { name: selected?.team_alias })}</DialogDescription>
              </DialogHeader>
              <DialogFooter>
                <Button variant="outline" onClick={() => setDeleteOpen(false)}>{t("common.cancel")}</Button>
                <Button variant="destructive" onClick={() => selected && deleteMutation.mutate(selected.team_id)} disabled={deleteMutation.isPending}>
                  {deleteMutation.isPending && <Spinner className="mr-2" />} {t("common.delete")}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        </>
      )}
    </div>
  );
}
