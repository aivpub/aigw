import { useState } from "react";
import { useTranslation } from "react-i18next";
import { apiPost, apiPut } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { WebSearchInstance } from "./types";

interface InstanceDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  providerId: number;
  providerName: string;
  editing?: WebSearchInstance | null;
  onSaved: (reloadOk: boolean) => void;
}

export function InstanceDialog({
  open,
  onOpenChange,
  providerId,
  providerName,
  editing,
  onSaved,
}: InstanceDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState(editing?.name ?? "");
  const [baseUrl, setBaseUrl] = useState(editing?.base_url ?? "");
  const [apiKey, setApiKey] = useState("");
  const [weight, setWeight] = useState(
    editing?.weight != null ? String(editing.weight) : "",
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSave() {
    if (!name.trim()) {
      setError(t("webSearch.instance.nameRequired"));
      return;
    }
    if (!/^https?:\/\//.test(baseUrl.trim())) {
      setError(t("webSearch.instance.baseUrlInvalid"));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const body: Record<string, unknown> = {
        name: name.trim(),
        base_url: baseUrl.trim(),
      };
      // An empty string would clear the stored key, and a blank field means
      // "not editing it" — so only a typed value is sent.
      if (apiKey.trim() !== "") {
        body.api_key = apiKey.trim();
      }
      if (weight.trim() !== "") {
        body.weight = Number(weight);
      }
      const res = editing
        ? await apiPut<{ reload?: { ok?: boolean } }>(
            `/admin/websearch/instances/${editing.id}`,
            body,
          )
        : await apiPost<{ reload?: { ok?: boolean } }>(
            `/admin/websearch/providers/${providerId}/instances`,
            body,
          );
      onSaved(res.reload?.ok !== false);
      onOpenChange(false);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>
            {editing
              ? t("webSearch.instance.titleEdit")
              : t("webSearch.instance.titleCreate")}
          </DialogTitle>
          <DialogDescription>
            {t("webSearch.instance.description", { provider: providerName })}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="ws-instance-name">{t("webSearch.instance.nameLabel")}</Label>
            <Input
              id="ws-instance-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("webSearch.instance.namePlaceholder")}
              data-testid="ws-instance-name"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="ws-instance-url">{t("webSearch.instance.baseUrlLabel")}</Label>
            <Input
              id="ws-instance-url"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder={t("webSearch.instance.baseUrlPlaceholder")}
              data-testid="ws-instance-url"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="ws-instance-key">{t("webSearch.instance.apiKeyLabel")}</Label>
            <Input
              id="ws-instance-key"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={
                editing?.has_api_key
                  ? t("webSearch.instance.apiKeyKeepPlaceholder")
                  : t("webSearch.instance.apiKeyPlaceholder")
              }
              data-testid="ws-instance-key"
            />
            <p className="text-xs text-muted-foreground">
              {t("webSearch.instance.apiKeyHint")}
            </p>
          </div>
          <div className="space-y-2">
            <Label htmlFor="ws-instance-weight">{t("webSearch.instance.weightLabel")}</Label>
            <Input
              id="ws-instance-weight"
              type="number"
              value={weight}
              onChange={(e) => setWeight(e.target.value)}
              placeholder={t("webSearch.dialog.optionalPlaceholder")}
            />
            <p className="text-xs text-muted-foreground">
              {t("webSearch.instance.weightHint")}
            </p>
          </div>
          {error && (
            <div className="rounded-md bg-destructive/10 border border-destructive/30 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={saving}>
            {t("common.cancel")}
          </Button>
          <Button onClick={handleSave} disabled={saving}>
            {saving ? t("common.saving") : t("webSearch.dialog.saveBtn")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
