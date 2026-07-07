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
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
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

interface KeyListResponse {
  keys: KeyItem[];
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

  // Form state
  const [formAlias, setFormAlias] = useState("");
  const [formModels, setFormModels] = useState("");
  const [formBudget, setFormBudget] = useState("");
  const [formTPM, setFormTPM] = useState("");
  const [formRPM, setFormRPM] = useState("");

  const { data, isLoading, error } = useQuery<KeyListResponse>({
    queryKey: ["virtual-keys"],
    queryFn: () => apiGet("/key/list"),
  });

  const keys = data?.keys ?? [];

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
      queryClient.invalidateQueries({ queryKey: ["virtual-keys"] });
      setCreateOpen(false);
      setGeneratedToken(resp.key ?? null);
      toast.success("Key created successfully");
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

  async function copyToClipboard(text: string) {
    await navigator.clipboard.writeText(text);
    toast.success("Copied to clipboard");
  }

  function openCreate() {
    setFormAlias("");
    setFormModels("");
    setFormBudget("");
    setFormTPM("");
    setFormRPM("");
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
    setEditOpen(true);
  }

  function openDelete(key: KeyItem) {
    setSelectedKey(key);
    setDeleteOpen(true);
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
        <Button onClick={openCreate}>
          <Plus className="h-4 w-4" />
          New Key
        </Button>
      </div>

      <div className="relative">
        <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          placeholder="Search by alias, name, or user..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="pl-9 max-w-sm"
        />
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle>All Keys ({filteredKeys.length})</CardTitle>
        </CardHeader>
        <CardContent>
          {error ? (
            <p className="text-sm text-destructive">
              {(error as Error).message}
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Alias</TableHead>
                  <TableHead>Token</TableHead>
                  <TableHead>User</TableHead>
                  <TableHead>Models</TableHead>
                  <TableHead className="text-right">Spend</TableHead>
                  <TableHead className="text-right">Budget</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
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
                        <TableCell>
                          {key.blocked ? (
                            <Badge variant="destructive">blocked</Badge>
                          ) : (
                            <Badge variant="default">active</Badge>
                          )}
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
                {!isLoading && filteredKeys.length === 0 && (
                  <TableRow>
                    <TableCell
                      colSpan={8}
                      className="text-center text-muted-foreground py-8"
                    >
                      {search ? "No keys match your search" : "No keys yet"}
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
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
                }}
              >
                Done
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
              ? This action cannot be undone.
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
    </div>
  );
}
