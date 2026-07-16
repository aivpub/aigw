import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost, apiPut, apiDelete } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter,
  DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Plus, Pencil, Trash2 } from "lucide-react";
import { toast } from "sonner";

interface CredentialItem {
  credential_id: string;
  credential_name: string;
  credential_values: Record<string, unknown>;
  credential_info: Record<string, unknown> | null;
}

interface CredentialForm {
  credential_name: string;
  credential_values: string; // JSON
  credential_info: string;   // JSON
}

function maskApiKey(raw: string): string {
  if (raw.length <= 8) return "***";
  return raw.slice(0, 4) + "***" + raw.slice(-4);
}

export function CredentialsTab() {
  const queryClient = useQueryClient();

  const { data, isLoading } = useQuery({
    queryKey: ["credentials-list"],
    queryFn: () => apiGet("/credential/list"),
  });

  const credentials: CredentialItem[] = (data as { data?: CredentialItem[] })?.data ?? [];

  // Dialog
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editing, setEditing] = useState<CredentialItem | null>(null);
  const [form, setForm] = useState<CredentialForm>({ credential_name: "", credential_values: "{}", credential_info: "{}" });
  const [saving, setSaving] = useState(false);

  // Delete
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleting, setDeleting] = useState<CredentialItem | null>(null);
  const [deleteLoading, setDeleteLoading] = useState(false);

  function openNew() {
    setEditing(null);
    setForm({ credential_name: "", credential_values: "{}", credential_info: "{}" });
    setDialogOpen(true);
  }

  function openEdit(c: CredentialItem) {
    setEditing(c);
    const values = c.credential_values ?? {};
    const info = c.credential_info ?? {};
    setForm({
      credential_name: c.credential_name,
      credential_values: JSON.stringify(values, null, 2),
      credential_info: JSON.stringify(info, null, 2),
    });
    setDialogOpen(true);
  }

  async function handleSave() {
    setSaving(true);
    try {
      let valuesJson: Record<string, unknown>;
      let infoJson: Record<string, unknown>;
      try { valuesJson = JSON.parse(form.credential_values); } catch { throw new Error("credential_values is not valid JSON"); }
      try { infoJson = JSON.parse(form.credential_info); } catch { throw new Error("credential_info is not valid JSON"); }

      const body = {
        credential_name: form.credential_name,
        credential_values: valuesJson,
        credential_info: infoJson,
      };

      if (editing) {
        await apiPut("/credential/update", { ...body, credential_name: editing.credential_name });
      } else {
        await apiPost("/credential/new", body);
      }

      queryClient.invalidateQueries({ queryKey: ["credentials-list"] });
      setDialogOpen(false);
      toast.success(editing ? "Credential updated" : "Credential created");
    } catch (e) {
      toast.error("Save failed", { description: (e as Error).message });
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete() {
    if (!deleting) return;
    setDeleteLoading(true);
    try {
      await apiDelete(`/credential/delete?credential_name=${encodeURIComponent(deleting.credential_name)}`);
      queryClient.invalidateQueries({ queryKey: ["credentials-list"] });
      setDeleteOpen(false);
      toast.success("Credential deleted");
    } catch (e) {
      toast.error("Delete failed", { description: (e as Error).message });
    } finally {
      setDeleteLoading(false);
    }
  }

  function getProvider(c: CredentialItem): string {
    const p = c.credential_values?.custom_llm_provider;
    return typeof p === "string" ? p : "—";
  }

  function getApiKeyHint(c: CredentialItem): string {
    const key = c.credential_values?.api_key;
    return typeof key === "string" ? maskApiKey(key as string) : "—";
  }

  function getApiBase(c: CredentialItem): string {
    const b = c.credential_values?.api_base;
    return typeof b === "string" ? b.replace("https://", "").slice(0, 30) : "—";
  }

  if (isLoading) return <Skeleton className="h-64 w-full" />;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          Stored credentials that models can reference via <code className="text-xs bg-muted px-1 rounded">litellm_credential_name</code>.
        </p>
        <Button size="sm" onClick={openNew}><Plus className="mr-1 h-4 w-4" /> New</Button>
      </div>

      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Provider</TableHead>
            <TableHead>API Base</TableHead>
            <TableHead>API Key</TableHead>
            <TableHead className="w-20">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {credentials.length === 0 ? (
            <TableRow><TableCell colSpan={5} className="text-center text-muted-foreground py-8">No credentials found</TableCell></TableRow>
          ) : credentials.map((c) => (
            <TableRow key={c.credential_id}>
              <TableCell className="font-medium">{c.credential_name}</TableCell>
              <TableCell><Badge variant="outline">{getProvider(c)}</Badge></TableCell>
              <TableCell className="text-muted-foreground text-sm font-mono">{getApiBase(c)}</TableCell>
              <TableCell className="text-muted-foreground text-sm font-mono">{getApiKeyHint(c)}</TableCell>
              <TableCell>
                <div className="flex gap-1">
                  <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => openEdit(c)}><Pencil className="h-3.5 w-3.5" /></Button>
                  <Button variant="ghost" size="icon" className="h-8 w-8 text-destructive" onClick={() => { setDeleting(c); setDeleteOpen(true); }}><Trash2 className="h-3.5 w-3.5" /></Button>
                </div>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>

      {/* Create/Edit Dialog */}
      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>{editing ? "Edit Credential" : "New Credential"}</DialogTitle>
            <DialogDescription>API keys stored in credential_values are encrypted at rest using AIGW_MASTER_KEY.</DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="cred-name">Credential Name</Label>
              <Input id="cred-name" value={form.credential_name} disabled={!!editing}
                onChange={(e) => setForm({ ...form, credential_name: e.target.value })} />
            </div>
            <div className="space-y-2">
              <Label htmlFor="cred-values">Credential Values (JSON)</Label>
              <Textarea id="cred-values" rows={5} className="font-mono text-xs" value={form.credential_values}
                onChange={(e) => setForm({ ...form, credential_values: e.target.value })} />
              <p className="text-xs text-muted-foreground">e.g. {`{"api_base":"https://api.openai.com/v1","api_key":"sk-...","custom_llm_provider":"openai"}`}</p>
            </div>
            <div className="space-y-2">
              <Label htmlFor="cred-info">Credential Info (JSON, optional)</Label>
              <Textarea id="cred-info" rows={3} className="font-mono text-xs" value={form.credential_info}
                onChange={(e) => setForm({ ...form, credential_info: e.target.value })} />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDialogOpen(false)}>Cancel</Button>
            <Button onClick={handleSave} disabled={saving || !form.credential_name}>{saving ? "Saving…" : "Save"}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation */}
      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Credential</DialogTitle>
            <DialogDescription>
              This will permanently remove <strong>{deleting?.credential_name}</strong>. Models referencing it will lose their API authentication.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteOpen(false)}>Cancel</Button>
            <Button variant="destructive" onClick={handleDelete} disabled={deleteLoading}>
              {deleteLoading ? "Deleting…" : "Delete"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
