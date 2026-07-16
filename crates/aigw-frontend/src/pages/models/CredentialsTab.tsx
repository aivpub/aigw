import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost, apiPut, apiDelete } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter,
  DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Plus, Pencil, Trash2, Code } from "lucide-react";
import { toast } from "sonner";

interface CredentialItem {
  credential_id: string;
  credential_name: string;
  credential_values: Record<string, unknown>;
  credential_info: Record<string, unknown> | null;
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

  // Visual form fields
  const [credName, setCredName] = useState("");
  const [apiBase, setApiBase] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [provider, setProvider] = useState("");
  const [credInfo, setCredInfo] = useState("{}");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [advancedJson, setAdvancedJson] = useState("{}");
  const [advancedInfo, setAdvancedInfo] = useState("{}");
  const [saving, setSaving] = useState(false);

  // Delete
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleting, setDeleting] = useState<CredentialItem | null>(null);
  const [deleteLoading, setDeleteLoading] = useState(false);

  function openNew() {
    setEditing(null);
    setCredName(""); setApiBase(""); setApiKey(""); setProvider("");
    setCredInfo("{}"); setShowAdvanced(false); setAdvancedJson("{}"); setAdvancedInfo("{}");
    setDialogOpen(true);
  }

  function openEdit(c: CredentialItem) {
    setEditing(c);
    const v = c.credential_values ?? {};
    setCredName(c.credential_name);
    setApiBase((v.api_base as string) ?? "");
    setApiKey((v.api_key as string) ?? "");
    setProvider((v.custom_llm_provider as string) ?? "");
    setCredInfo(JSON.stringify(c.credential_info ?? {}, null, 2));
    setAdvancedJson(JSON.stringify(v, null, 2));
    setAdvancedInfo(JSON.stringify(c.credential_info ?? {}, null, 2));
    setShowAdvanced(false);
    setDialogOpen(true);
  }

  async function handleSave() {
    setSaving(true);
    try {
      let valuesJson: Record<string, unknown>;
      if (showAdvanced) {
        try { valuesJson = JSON.parse(advancedJson); } catch { throw new Error("Advanced JSON is not valid"); }
      } else {
        valuesJson = {
          api_base: apiBase || undefined,
          api_key: apiKey || undefined,
          custom_llm_provider: provider || undefined,
        };
        // Remove undefined keys
        Object.keys(valuesJson).forEach((k) => { if (valuesJson[k] === undefined) delete valuesJson[k]; });
      }

      let infoJson: Record<string, unknown>;
      const infoStr = showAdvanced ? advancedInfo : credInfo;
      try { infoJson = JSON.parse(infoStr || "{}"); } catch { throw new Error("Credential Info is not valid JSON"); }

      const body = {
        credential_name: credName,
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
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{editing ? "Edit Credential" : "New Credential"}</DialogTitle>
            <DialogDescription>
              API keys are encrypted at rest using AIGW_MASTER_KEY. Toggle advanced mode to edit raw JSON.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 max-h-[60vh] overflow-y-auto">
            <div className="space-y-2">
              <Label htmlFor="cred-name">Credential Name *</Label>
              <Input id="cred-name" value={credName} disabled={!!editing}
                onChange={(e) => setCredName(e.target.value)} placeholder="e.g. openai-prod" />
            </div>

            {/* Toggle */}
            <div className="flex items-center gap-2">
              <Switch checked={showAdvanced} onCheckedChange={setShowAdvanced} />
              <Label className="text-xs cursor-pointer flex items-center gap-1" onClick={() => setShowAdvanced(!showAdvanced)}>
                <Code className="h-3 w-3" /> Advanced (raw JSON)
              </Label>
            </div>

            {showAdvanced ? (
              <>
                <div className="space-y-2">
                  <Label htmlFor="cred-values-adv">Credential Values (JSON)</Label>
                  <Textarea id="cred-values-adv" rows={6} className="font-mono text-xs"
                    value={advancedJson} onChange={(e) => setAdvancedJson(e.target.value)} />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="cred-info-adv">Credential Info (JSON)</Label>
                  <Textarea id="cred-info-adv" rows={4} className="font-mono text-xs"
                    value={advancedInfo} onChange={(e) => setAdvancedInfo(e.target.value)} />
                </div>
              </>
            ) : (
              <>
                <div className="space-y-2">
                  <Label htmlFor="cred-provider">Provider</Label>
                  <Input id="cred-provider" value={provider}
                    onChange={(e) => setProvider(e.target.value)} placeholder="e.g. openai, anthropic, deepseek" />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="cred-apibase">API Base URL</Label>
                  <Input id="cred-apibase" value={apiBase}
                    onChange={(e) => setApiBase(e.target.value)} placeholder="https://api.openai.com/v1" />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="cred-apikey">API Key</Label>
                  <Input id="cred-apikey" value={apiKey} type="password"
                    onChange={(e) => setApiKey(e.target.value)} placeholder="sk-..." />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="cred-info-vis">Credential Info (JSON, optional)</Label>
                  <Textarea id="cred-info-vis" rows={3} className="font-mono text-xs"
                    value={credInfo} onChange={(e) => setCredInfo(e.target.value)} placeholder='{"description":"..."}' />
                </div>
              </>
            )}
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setDialogOpen(false)}>Cancel</Button>
            <Button onClick={handleSave} disabled={saving || !credName}>{saving ? "Saving…" : "Save"}</Button>
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
