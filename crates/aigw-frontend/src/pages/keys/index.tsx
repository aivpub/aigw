import { useState, useMemo } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
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
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
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
import {
  Plus,
  Search,
  Pencil,
  Trash2,
  Copy,
  Eye,
  EyeOff,
} from "lucide-react";
import { format } from "date-fns";

interface KeyItem {
  token: string;
  key_name: string | null;
  key_alias: string | null;
  user_id: string | null;
  team_id: string | null;
  spend: number;
  max_budget: number | null;
  tpm_limit: number | null;
  rpm_limit: number | null;
  blocked: boolean | null;
  expires: string | null;
  models: string[];
  metadata: Record<string, unknown>;
  created_at: string | null;
  key?: string; // raw key, only present on generate response
}

interface DeletedKeyItem {
  token: string;
  key_alias: string | null;
  user_id: string | null;
  spend: number;
  key_name: string | null;
  blocked: boolean | null;
  updated_at: string | null;
}

interface KeyListResponse {
  keys?: KeyItem[];
  data?: KeyItem[];
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

interface DeletedKeyListResponse {
  keys?: DeletedKeyItem[];
  data?: DeletedKeyItem[];
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

function maskToken(token: string): string {
  if (token.length <= 10) return token;
  return `${token.slice(0, 5)}...${token.slice(-4)}`;
}

export function KeysPage() {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [visibleTokens, setVisibleTokens] = useState<Set<string>>(new Set());
  const [createOpen, setCreateOpen] = useState(false);
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [selectedKey, setSelectedKey] = useState<KeyItem | null>(null);
  const [generatedToken, setGeneratedToken] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<"active" | "deleted">("active");
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);
  const [deletedPage, setDeletedPage] = useState(1);
  const [deletedPageSize, setDeletedPageSize] = useState(30);

  // Form state
  const [formAlias, setFormAlias] = useState("");
  const [formModels, setFormModels] = useState("");
  const [formBudget, setFormBudget] = useState("");
  const [formTPM, setFormTPM] = useState("");
  const [formRPM, setFormRPM] = useState("");
  const [formExpires, setFormExpires] = useState("");

  const { data, isLoading, error } = useQuery<KeyListResponse>({
    queryKey: ["virtual-keys", page, pageSize],
    queryFn: () =>
      apiGet(`/key/list?page=${page}&page_size=${pageSize}`),
  });

  const {
    data: deletedData,
    isLoading: deletedLoading,
  } = useQuery<DeletedKeyListResponse>({
    queryKey: ["virtual-keys-deleted", deletedPage, deletedPageSize],
    queryFn: () => apiGet(`/key/deleted?page=${deletedPage}&page_size=${deletedPageSize}`),
    enabled: viewMode === "deleted",
  });

  const keys = data?.keys ?? data?.data ?? [];
  const totalCount = data?.total_count ?? keys.length;
  const totalPages = data?.total_pages ?? (keys.length === 0 ? 1 : Math.ceil(totalCount / pageSize));

  const deletedKeys = deletedData?.keys ?? deletedData?.data ?? [];
  const deletedTotalCount = deletedData?.total_count ?? deletedKeys.length;
  const deletedTotalPages = deletedData?.total_pages ?? (deletedKeys.length === 0 ? 1 : Math.ceil(deletedTotalCount / deletedPageSize));

  const filteredKeys = useMemo(() => {
    if (!search.trim()) return keys;
    const q = search.toLowerCase();
    return keys.filter(
      (k) =>
        (k.key_alias ?? "").toLowerCase().includes(q) ||
        (k.key_name ?? "").toLowerCase().includes(q) ||
        (k.user_id ?? "").toLowerCase().includes(q),
    );
  }, [keys, search]);

  const createMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) =>
      apiPost<KeyItem>("/key/generate", body),
    onSuccess: (resp) => {
      // Set token FIRST so the dialog shows it; keep dialog open
      setGeneratedToken(resp.key ?? null);
      toast.success("Key created. Please save your API key.");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const editMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPut("/key/update", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["virtual-keys"] });
      setEditOpen(false);
      setSelectedKey(null);
      toast.success("Key updated");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const deleteMutation = useMutation({
    mutationFn: (token: string) =>
      apiDelete(`/key/delete?key=${encodeURIComponent(token)}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["virtual-keys"] });
      setDeleteOpen(false);
      setSelectedKey(null);
      toast.success("Key deleted");
    },
    onError: (err: Error) => toast.error(err.message),
  });

  function toggleTokenVisible(token: string) {
    setVisibleTokens((prev) => {
      const next = new Set(prev);
      if (next.has(token)) next.delete(token);
      else next.add(token);
      return next;
    });
  }

  function copyToClipboard(text: string) {
    if (!text) return;
    try {
      if (
        typeof navigator.clipboard?.writeText === "function"
      ) {
        navigator.clipboard.writeText(text).then(
          () => toast.success("Copied to clipboard"),
          () => fallbackCopyToClipboard(text),
        );
      } else {
        fallbackCopyToClipboard(text);
      }
    } catch {
      fallbackCopyToClipboard(text);
    }

    function fallbackCopyToClipboard(t: string) {
      const textarea = document.createElement("textarea");
      textarea.value = t;
      textarea.style.position = "fixed";
      textarea.style.opacity = "0";
      document.body.appendChild(textarea);
      textarea.select();
      try {
        document.execCommand("copy");
        toast.success("Copied to clipboard");
      } catch {
        toast.error("Copy failed — clipboard unavailable");
      }
      document.body.removeChild(textarea);
    }
  }

  function openCreate() {
    setFormAlias("");
    setFormModels("");
    setFormBudget("");
    setFormTPM("");
    setFormRPM("");
    setFormExpires("");
    setGeneratedToken(null);
    setCreateOpen(true);
  }

  function openEdit(key: KeyItem) {
    setSelectedKey(key);
    setFormAlias(key.key_alias ?? "");
    setFormModels(Array.isArray(key.models) ? key.models.join(", ") : "");
    setFormBudget(key.max_budget?.toString() ?? "");
    setFormTPM(key.tpm_limit?.toString() ?? "");
    setFormRPM(key.rpm_limit?.toString() ?? "");
    setFormExpires(key.expires ?? "");
    setEditOpen(true);
  }

  function openDelete(key: KeyItem) {
    setSelectedKey(key);
    setDeleteOpen(true);
  }

  function formatDate(d: string) {
    try { return format(new Date(d), "yyyy-MM-dd HH:mm"); } catch { return d; }
  }

  function buildCreateBody(): Record<string, unknown> {
    const body: Record<string, unknown> = {};
    if (formAlias.trim()) body.key_alias = formAlias.trim();
    if (formModels.trim()) {
      body.models = formModels.split(",").map((s) => s.trim()).filter(Boolean);
    }
    if (formBudget.trim()) body.max_budget = parseFloat(formBudget);
    if (formTPM.trim()) body.tpm_limit = parseInt(formTPM);
    if (formRPM.trim()) body.rpm_limit = parseInt(formRPM);
    if (formExpires.trim()) body.expires = formExpires.trim();
    return body;
  }

  function handleCreate() {
    createMutation.mutate(buildCreateBody());
  }

  function handleEdit() {
    if (!selectedKey) return;
    editMutation.mutate({
      key: selectedKey.token,
      ...(formAlias.trim() && { key_alias: formAlias.trim() }),
      ...(formModels.trim() && {
        models: formModels.split(",").map((s) => s.trim()).filter(Boolean),
      }),
      ...(formBudget.trim() && { max_budget: parseFloat(formBudget) }),
      ...(formTPM.trim() && { tpm_limit: parseInt(formTPM) }),
      ...(formRPM.trim() && { rpm_limit: parseInt(formRPM) }),
      ...(formExpires.trim() && { expires: formExpires.trim() }),
    });
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">API Keys</h1>
          <p className="text-sm text-muted-foreground">
            Manage virtual keys for LLM access
          </p>
        </div>
        {viewMode === "active" && (
          <Button onClick={openCreate}>
            <Plus className="h-4 w-4" />
            New Key
          </Button>
        )}
      </div>

      <div className="flex items-center gap-2">
        {viewMode === "active" && (
          <div className="relative flex-1 max-w-sm">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder="Search by alias, name, or user..."
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

      {/* Deleted Keys View */}
      {viewMode === "deleted" && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle>Deleted Keys ({deletedTotalCount})</CardTitle>
          </CardHeader>
          <CardContent>
            {deletedLoading ? (
              Array.from({ length: 3 }).map((_, i) => (
                <div key={i} className="py-2"><Skeleton className="h-4 w-full" /></div>
              ))
            ) : deletedKeys.length === 0 ? (
              <div className="text-center text-muted-foreground py-8">No deleted records</div>
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
                    <TableHeader>
                      <TableRow>
                        <TableHead>Alias</TableHead>
                        <TableHead>Token</TableHead>
                        <TableHead>User</TableHead>
                        <TableHead className="text-right">Spend</TableHead>
                        <TableHead>Status</TableHead>
                        <TableHead className="text-right">Deleted At</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {deletedKeys.map((k) => (
                        <TableRow key={k.token}>
                          <TableCell className="font-medium">{k.key_alias ?? k.key_name ?? "—"}</TableCell>
                          <TableCell className="font-mono text-xs">{maskToken(k.token)}</TableCell>
                          <TableCell className="text-sm">{k.user_id ?? "—"}</TableCell>
                          <TableCell className="text-right text-sm">${k.spend.toFixed(4)}</TableCell>
                          <TableCell>{k.blocked ? <Badge variant="destructive">blocked</Badge> : <Badge variant="secondary">active</Badge>}</TableCell>
                          <TableCell className="text-right text-sm text-muted-foreground">{k.updated_at ? formatDate(k.updated_at) : "—"}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div className="md:hidden space-y-3">
                  {deletedKeys.map((k) => (
                    <Card key={k.token}>
                      <CardContent className="p-4 space-y-2">
                        <div className="flex items-center justify-between">
                          <span className="font-medium text-sm">{k.key_alias ?? k.key_name ?? "—"}</span>
                          <span className="text-xs text-muted-foreground">${k.spend.toFixed(4)}</span>
                        </div>
                        <div className="text-xs font-mono text-muted-foreground">{maskToken(k.token)}</div>
                        <div className="text-xs text-muted-foreground">User: {k.user_id ?? "—"} | Deleted: {k.updated_at ? formatDate(k.updated_at) : "—"}</div>
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
                    onPageSize={(s) => { setDeletedPageSize(s); setDeletedPage(1); }}
                  />
                </div>
              </>
            )}
          </CardContent>
        </Card>
      )}

      {/* Active Keys View */}
      {viewMode === "active" && (
        <>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle>All Keys ({totalCount})</CardTitle>
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
                onPageSize={(s) => { setPageSize(s); setPage(1); }}
              />
              {/* Desktop table */}
              <div className="hidden md:block">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Alias</TableHead>
                      <TableHead>Token</TableHead>
                      <TableHead>User</TableHead>
                      <TableHead>Models</TableHead>
                      <TableHead className="text-right">Spend</TableHead>
                      <TableHead className="text-right">Budget</TableHead>
                      <TableHead>Expires</TableHead>
                      <TableHead>Created</TableHead>
                      <TableHead>Status</TableHead>
                      <TableHead className="text-right">Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {isLoading
                      ? Array.from({ length: 3 }).map((_, i) => (
                          <TableRow key={i}>
                            {Array.from({ length: 10 }).map((_, j) => (
                              <TableCell key={j}>
                                <Skeleton className="h-4 w-full" />
                              </TableCell>
                            ))}
                          </TableRow>
                        ))
                      : filteredKeys.map((key) => (
                          <TableRow key={key.token}>
                            <TableCell className="font-medium">
                              {key.key_alias ?? key.key_name ?? "—"}
                            </TableCell>
                            <TableCell className="font-mono text-xs">
                              <span className="inline-flex items-center gap-1">
                                {visibleTokens.has(key.token)
                                  ? key.token
                                  : maskToken(key.token)}
                                <button
                                  type="button"
                                  onClick={() => toggleTokenVisible(key.token)}
                                  className="text-muted-foreground hover:text-foreground"
                                >
                                  {visibleTokens.has(key.token) ? (
                                    <EyeOff className="h-3.5 w-3.5" />
                                  ) : (
                                    <Eye className="h-3.5 w-3.5" />
                                  )}
                                </button>
                                <button
                                  type="button"
                                  onClick={() => copyToClipboard(key.token)}
                                  className="text-muted-foreground hover:text-foreground"
                                >
                                  <Copy className="h-3.5 w-3.5" />
                                </button>
                              </span>
                            </TableCell>
                            <TableCell className="text-sm">
                              {key.user_id ?? "—"}
                            </TableCell>
                            <TableCell className="text-sm">
                              {Array.isArray(key.models) && key.models.length > 0
                                ? key.models.slice(0, 3).join(", ") +
                                  (key.models.length > 3 ? " ..." : "")
                                : "—"}
                            </TableCell>
                            <TableCell className="text-right text-sm">
                              ${key.spend.toFixed(4)}
                            </TableCell>
                            <TableCell className="text-right text-sm">
                              {key.max_budget != null
                                ? `$${key.max_budget.toFixed(2)}`
                                : "∞"}
                            </TableCell>
                            <TableCell className="text-xs text-muted-foreground">
                              {key.expires
                                ? new Date(key.expires).toLocaleDateString()
                                : "∞"}
                            </TableCell>
                            <TableCell className="text-xs text-muted-foreground">
                              {key.created_at ? formatDate(key.created_at) : "—"}
                            </TableCell>
                            <TableCell>
                              <div className="flex items-center gap-2">
                                <Switch
                                  checked={!key.blocked}
                                  onCheckedChange={async (checked) => {
                                    try {
                                      if (checked) {
                                        await apiPost("/key/unblock", { key: key.token });
                                      } else {
                                        await apiPost("/key/block", { key: key.token });
                                      }
                                      queryClient.invalidateQueries({ queryKey: ["virtual-keys"] });
                                      toast.success(checked ? "Key unblocked" : "Key blocked");
                                    } catch (err) {
                                      toast.error((err as Error).message);
                                    }
                                  }}
                                />
                                {key.blocked ? (
                                  <Badge variant="destructive">blocked</Badge>
                                ) : (
                                  <Badge variant="default">active</Badge>
                                )}
                              </div>
                            </TableCell>
                            <TableCell className="text-right">
                              <div className="flex justify-end gap-1">
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  onClick={() => openEdit(key)}
                                >
                                  <Pencil className="h-4 w-4" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  onClick={() => openDelete(key)}
                                >
                                  <Trash2 className="h-4 w-4 text-destructive" />
                                </Button>
                              </div>
                            </TableCell>
                          </TableRow>
                        ))}
                  </TableBody>
                </Table>
                {!isLoading && filteredKeys.length === 0 && (
                  <div className="text-center text-muted-foreground py-8">
                    {search ? "No keys match your search" : "No keys yet"}
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
                  : filteredKeys.map((key) => (
                      <Card key={key.token}>
                        <CardContent className="p-4 space-y-2">
                          <div className="flex items-center justify-between">
                            <span className="font-medium text-sm truncate max-w-[70%]">
                              {key.key_alias ?? key.key_name ?? "—"}
                            </span>
                            {key.blocked ? (
                              <Badge variant="destructive" className="text-xs">blocked</Badge>
                            ) : (
                              <Badge variant="default" className="text-xs">active</Badge>
                            )}
                          </div>
                          <div className="flex items-center gap-2 text-xs font-mono">
                            <span className="text-muted-foreground truncate max-w-[60%]">
                              {visibleTokens.has(key.token)
                                ? key.token
                                : maskToken(key.token)}
                            </span>
                            <button
                              type="button"
                              onClick={() => toggleTokenVisible(key.token)}
                              className="text-muted-foreground hover:text-foreground shrink-0"
                            >
                              {visibleTokens.has(key.token) ? (
                                <EyeOff className="h-3 w-3" />
                              ) : (
                                <Eye className="h-3 w-3" />
                              )}
                            </button>
                            <button
                              type="button"
                              onClick={() => copyToClipboard(key.token)}
                              className="text-muted-foreground hover:text-foreground shrink-0"
                            >
                              <Copy className="h-3 w-3" />
                            </button>
                          </div>
                          <div className="flex items-center justify-between text-xs text-muted-foreground">
                            <span>{key.user_id ?? "—"}</span>
                            <span>
                              Spent ${key.spend.toFixed(4)}
                              {" / "}
                              {key.max_budget != null
                                ? `$${key.max_budget.toFixed(2)}`
                                : "∞"}
                            </span>
                          </div>
                          <div className="text-xs text-muted-foreground">
                            Expires:{" "}
                            {key.expires
                              ? new Date(key.expires).toLocaleDateString()
                              : "∞"}
                          </div>
                          <div className="text-xs text-muted-foreground">
                            Created: {key.created_at ? formatDate(key.created_at) : "—"}
                          </div>
                          <div className="flex justify-end gap-1 pt-1">
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => openEdit(key)}
                            >
                              <Pencil className="h-3.5 w-3.5 mr-1" />
                              Edit
                            </Button>
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => openDelete(key)}
                            >
                              <Trash2 className="h-3.5 w-3.5 mr-1 text-destructive" />
                              Delete
                            </Button>
                          </div>
                        </CardContent>
                      </Card>
                    ))}
                {!isLoading && filteredKeys.length === 0 && (
                  <div className="text-center text-muted-foreground py-8">
                    {search ? "No keys match your search" : "No keys yet"}
                  </div>
                )}
              </div>
              {filteredKeys.length > 0 ? (
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
          <DialogHeader>
            <DialogTitle>Create API Key</DialogTitle>
            <DialogDescription>
              Generate a new virtual key for LLM access.
            </DialogDescription>
          </DialogHeader>

          {generatedToken ? (
            <div className="space-y-4">
              <p className="text-sm font-medium text-green-600">
                Key created! Copy it now — it won't be shown again.
              </p>
              <div className="flex items-center gap-2 rounded-md border bg-muted p-3">
                <code className="flex-1 break-all text-sm font-mono">
                  {generatedToken}
                </code>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => copyToClipboard(generatedToken)}
                >
                  <Copy className="h-4 w-4" />
                </Button>
              </div>
              <Button
                className="w-full"
                onClick={() => {
                  setCreateOpen(false);
                  setGeneratedToken(null);
                  queryClient.invalidateQueries({ queryKey: ["virtual-keys"] });
                }}
              >
                I've saved my key
              </Button>
            </div>
          ) : (
            <>
              <div className="space-y-4">
                <div>
                  <Label htmlFor="alias">Alias</Label>
                  <Input
                    id="alias"
                    value={formAlias}
                    onChange={(e) => setFormAlias(e.target.value)}
                    placeholder="my-app-key"
                  />
                </div>
                <div>
                  <Label htmlFor="models">Models (comma-separated)</Label>
                  <Input
                    id="models"
                    value={formModels}
                    onChange={(e) => setFormModels(e.target.value)}
                    placeholder="gpt-4, gpt-3.5-turbo"
                  />
                </div>
                <div className="grid grid-cols-3 gap-4">
                  <div>
                    <Label htmlFor="budget">Max Budget ($)</Label>
                    <Input
                      id="budget"
                      type="number"
                      value={formBudget}
                      onChange={(e) => setFormBudget(e.target.value)}
                      placeholder="50"
                    />
                  </div>
                  <div>
                    <Label htmlFor="tpm">TPM Limit</Label>
                    <Input
                      id="tpm"
                      type="number"
                      value={formTPM}
                      onChange={(e) => setFormTPM(e.target.value)}
                      placeholder="100000"
                    />
                  </div>
                  <div>
                    <Label htmlFor="rpm">RPM Limit</Label>
                    <Input
                      id="rpm"
                      type="number"
                      value={formRPM}
                      onChange={(e) => setFormRPM(e.target.value)}
                      placeholder="100"
                    />
                  </div>
                </div>
                <div>
                  <Label htmlFor="expires">Expires</Label>
                  <Input
                    id="expires"
                    type="date"
                    value={formExpires}
                    onChange={(e) => setFormExpires(e.target.value)}
                  />
                </div>
              </div>
              <DialogFooter>
                <Button
                  variant="outline"
                  onClick={() => setCreateOpen(false)}
                >
                  Cancel
                </Button>
                <Button
                  onClick={handleCreate}
                  disabled={createMutation.isPending}
                >
                  {createMutation.isPending && (
                    <Spinner className="mr-2" />
                  )}
                  Generate Key
                </Button>
              </DialogFooter>
            </>
          )}
        </DialogContent>
      </Dialog>

      {/* Edit Dialog */}
      <Dialog open={editOpen} onOpenChange={setEditOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Edit Key</DialogTitle>
            <DialogDescription>
              Update {selectedKey?.key_alias ?? selectedKey?.token.slice(0, 8)}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="edit-alias">Alias</Label>
              <Input
                id="edit-alias"
                value={formAlias}
                onChange={(e) => setFormAlias(e.target.value)}
              />
            </div>
            <div>
              <Label htmlFor="edit-models">Models (comma-separated)</Label>
              <Input
                id="edit-models"
                value={formModels}
                onChange={(e) => setFormModels(e.target.value)}
              />
            </div>
            <div className="grid grid-cols-3 gap-4">
              <div>
                <Label htmlFor="edit-budget">Max Budget ($)</Label>
                <Input
                  id="edit-budget"
                  type="number"
                  value={formBudget}
                  onChange={(e) => setFormBudget(e.target.value)}
                />
              </div>
              <div>
                <Label htmlFor="edit-tpm">TPM Limit</Label>
                <Input
                  id="edit-tpm"
                  type="number"
                  value={formTPM}
                  onChange={(e) => setFormTPM(e.target.value)}
                />
              </div>
              <div>
                <Label htmlFor="edit-rpm">RPM Limit</Label>
                <Input
                  id="edit-rpm"
                  type="number"
                  value={formRPM}
                  onChange={(e) => setFormRPM(e.target.value)}
                />
              </div>
            </div>
            <div>
              <Label htmlFor="edit-expires">Expires</Label>
              <Input
                id="edit-expires"
                type="date"
                value={formExpires}
                onChange={(e) => setFormExpires(e.target.value)}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditOpen(false)}>
              Cancel
            </Button>
            <Button
              onClick={handleEdit}
              disabled={editMutation.isPending}
            >
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
            <DialogTitle>Delete Key</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete{" "}
              <strong>
                {selectedKey?.key_alias ?? selectedKey?.token.slice(0, 8)}
              </strong>
              ? This key will be archived and viewable in the Deleted tab.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() =>
                selectedKey && deleteMutation.mutate(selectedKey.token)
              }
              disabled={deleteMutation.isPending}
            >
              {deleteMutation.isPending && <Spinner className="mr-2" />}
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

        </>
      )}
    </div>
  );
}
