import { useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost, apiPut, apiDelete } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Plus, Pencil, Trash2, Code } from "lucide-react";
import { toast } from "sonner";
import { PaginationBar } from "@/components/ui/pagination";

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
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);

  const { data, isLoading } = useQuery({
    queryKey: ["credentials-list", page, pageSize],
    queryFn: () =>
      apiGet(`/credential/list?page=${page}&page_size=${pageSize}`),
  });

  const credentials: CredentialItem[] =
    (data as { data?: CredentialItem[] })?.data ?? [];
  const totalCount =
    (data as { total_count?: number })?.total_count ?? credentials.length;
  const totalPages =
    (data as { total_pages?: number })?.total_pages ??
    (credentials.length === 0 ? 1 : Math.ceil(totalCount / pageSize));

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
    setCredName("");
    setApiBase("");
    setApiKey("");
    setProvider("");
    setCredInfo("{}");
    setShowAdvanced(false);
    setAdvancedJson("{}");
    setAdvancedInfo("{}");
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
        try {
          valuesJson = JSON.parse(advancedJson);
        } catch {
          throw new Error(t("models.credentials.form.advancedJsonInvalid"));
        }
      } else {
        valuesJson = {
          api_base: apiBase || undefined,
          api_key: apiKey || undefined,
          custom_llm_provider: provider || undefined,
        };
        // Remove undefined keys
        Object.keys(valuesJson).forEach((k) => {
          if (valuesJson[k] === undefined) delete valuesJson[k];
        });
      }

      let infoJson: Record<string, unknown>;
      const infoStr = showAdvanced ? advancedInfo : credInfo;
      try {
        infoJson = JSON.parse(infoStr || "{}");
      } catch {
        throw new Error(t("models.credentials.form.credInfoJsonInvalid"));
      }

      const body = {
        credential_name: credName,
        credential_values: valuesJson,
        credential_info: infoJson,
      };

      if (editing) {
        await apiPut("/credential/update", {
          ...body,
          credential_name: editing.credential_name,
        });
      } else {
        await apiPost("/credential/new", body);
      }

      queryClient.invalidateQueries({ queryKey: ["credentials-list"] });
      setDialogOpen(false);
      toast.success(
        editing
          ? t("models.credentials.toast.updated")
          : t("models.credentials.toast.created"),
      );
    } catch (e) {
      toast.error(t("models.credentials.toast.saveFailed"), {
        description: (e as Error).message,
      });
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete() {
    if (!deleting) return;
    setDeleteLoading(true);
    try {
      await apiDelete(
        `/credential/delete?credential_name=${encodeURIComponent(deleting.credential_name)}`,
      );
      queryClient.invalidateQueries({ queryKey: ["credentials-list"] });
      setDeleteOpen(false);
      toast.success(t("models.credentials.toast.deleted"));
    } catch (e) {
      toast.error(t("models.credentials.toast.deleteFailed"), {
        description: (e as Error).message,
      });
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
          <Trans
            i18nKey="models.credentials.description"
            components={{
              code: <code className="text-xs bg-muted px-1 rounded" />,
            }}
          />
        </p>
        <Button size="sm" onClick={openNew}>
          <Plus className="mr-1 h-4 w-4" /> {t("models.credentials.new")}
        </Button>
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle>
            {t("models.credentials.allCredentials")} ({totalCount})
          </CardTitle>
        </CardHeader>
        <CardContent>
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
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t("models.credentials.name")}</TableHead>
                <TableHead>{t("models.credentials.provider")}</TableHead>
                <TableHead>{t("models.credentials.apiBase")}</TableHead>
                <TableHead>{t("models.credentials.apiKey")}</TableHead>
                <TableHead className="w-20">
                  {t("models.credentials.actions")}
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {credentials.length === 0 ? (
                <TableRow>
                  <TableCell
                    colSpan={5}
                    className="text-center text-muted-foreground py-8"
                  >
                    {t("models.credentials.empty")}
                  </TableCell>
                </TableRow>
              ) : (
                credentials.map((c) => (
                  <TableRow key={c.credential_id}>
                    <TableCell className="font-medium">
                      {c.credential_name}
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline">{getProvider(c)}</Badge>
                    </TableCell>
                    <TableCell className="text-muted-foreground text-sm font-mono">
                      {getApiBase(c)}
                    </TableCell>
                    <TableCell className="text-muted-foreground text-sm font-mono">
                      {getApiKeyHint(c)}
                    </TableCell>
                    <TableCell>
                      <div className="flex gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8"
                          onClick={() => openEdit(c)}
                        >
                          <Pencil className="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 text-destructive"
                          onClick={() => {
                            setDeleting(c);
                            setDeleteOpen(true);
                          }}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
          {credentials.length > 0 ? (
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
        </CardContent>
      </Card>

      {/* Create/Edit Dialog */}
      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>
              {editing
                ? t("models.credentials.editCredential")
                : t("models.credentials.newCredential")}
            </DialogTitle>
            <DialogDescription>
              {t("models.credentials.encryptDesc")}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 max-h-[60vh] overflow-y-auto">
            <div className="space-y-2">
              <Label htmlFor="cred-name">
                {t("models.credentials.form.nameLabel")}
              </Label>
              <Input
                id="cred-name"
                value={credName}
                disabled={!!editing}
                onChange={(e) => setCredName(e.target.value)}
                placeholder={t("models.credentials.form.namePlaceholder")}
              />
            </div>

            {/* Toggle */}
            <div className="flex items-center gap-2">
              <Switch
                checked={showAdvanced}
                onCheckedChange={setShowAdvanced}
              />
              <Label
                className="text-xs cursor-pointer flex items-center gap-1"
                onClick={() => setShowAdvanced(!showAdvanced)}
              >
                <Code className="h-3 w-3" />{" "}
                {t("models.credentials.form.advancedToggle")}
              </Label>
            </div>

            {showAdvanced ? (
              <>
                <div className="space-y-2">
                  <Label htmlFor="cred-values-adv">
                    {t("models.credentials.form.valuesJsonLabel")}
                  </Label>
                  <Textarea
                    id="cred-values-adv"
                    rows={6}
                    className="font-mono text-xs"
                    value={advancedJson}
                    onChange={(e) => setAdvancedJson(e.target.value)}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="cred-info-adv">
                    {t("models.credentials.form.infoJsonLabel")}
                  </Label>
                  <Textarea
                    id="cred-info-adv"
                    rows={4}
                    className="font-mono text-xs"
                    value={advancedInfo}
                    onChange={(e) => setAdvancedInfo(e.target.value)}
                  />
                </div>
              </>
            ) : (
              <>
                <div className="space-y-2">
                  <Label htmlFor="cred-provider">
                    {t("models.credentials.form.providerLabel")}
                  </Label>
                  <Input
                    id="cred-provider"
                    value={provider}
                    onChange={(e) => setProvider(e.target.value)}
                    placeholder={t(
                      "models.credentials.form.providerPlaceholder",
                    )}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="cred-apibase">
                    {t("models.credentials.form.apiBaseLabel")}
                  </Label>
                  <Input
                    id="cred-apibase"
                    value={apiBase}
                    onChange={(e) => setApiBase(e.target.value)}
                    placeholder={t(
                      "models.credentials.form.apiBasePlaceholder",
                    )}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="cred-apikey">
                    {t("models.credentials.form.apiKeyLabel")}
                  </Label>
                  <Input
                    id="cred-apikey"
                    value={apiKey}
                    type="password"
                    onChange={(e) => setApiKey(e.target.value)}
                    placeholder={t("models.credentials.form.apiKeyPlaceholder")}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="cred-info-vis">
                    {t("models.credentials.form.credInfoLabel")}
                  </Label>
                  <Textarea
                    id="cred-info-vis"
                    rows={3}
                    className="font-mono text-xs"
                    value={credInfo}
                    onChange={(e) => setCredInfo(e.target.value)}
                    placeholder={t(
                      "models.credentials.form.credInfoPlaceholder",
                    )}
                  />
                </div>
              </>
            )}
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setDialogOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={handleSave} disabled={saving || !credName}>
              {saving ? t("common.saving") : t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation */}
      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {t("models.credentials.deleteDialog.title")}
            </DialogTitle>
            <DialogDescription>
              {t("models.credentials.deleteDescription", {
                credentialName: deleting?.credential_name,
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={handleDelete}
              disabled={deleteLoading}
            >
              {deleteLoading ? t("common.deleting") : t("common.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
