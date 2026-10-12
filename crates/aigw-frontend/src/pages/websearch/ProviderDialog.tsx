import { useState } from "react";
import { useTranslation } from "react-i18next";
import { apiPost, apiPut } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  PLACEHOLDER_PRICE_THRESHOLD,
  SUPPORTED_KINDS,
  type WebSearchProvider,
} from "./types";

interface ProviderDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  editing?: WebSearchProvider | null;
  onSaved: (reloadOk: boolean) => void;
}

export function ProviderDialog({
  open,
  onOpenChange,
  editing,
  onSaved,
}: ProviderDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState(editing?.name ?? "");
  const [kind, setKind] = useState(editing?.kind ?? SUPPORTED_KINDS[0]);
  const [cost, setCost] = useState(
    editing ? String(editing.cost_per_query) : "0.01",
  );
  const [failoverOrder, setFailoverOrder] = useState(
    editing?.failover_order != null ? String(editing.failover_order) : "",
  );
  const [timeoutMs, setTimeoutMs] = useState(
    editing?.timeout_ms != null ? String(editing.timeout_ms) : "",
  );
  // Default to enabled on create: the common case is "add a working provider".
  // Being able to create one switched off matters because the only other way to
  // get a disabled provider is to create it enabled and immediately toggle it.
  const [enabled, setEnabled] = useState(editing?.enabled ?? true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const parsedCost = Number(cost);
  const costIsPlaceholder =
    kind === "searxng" &&
    Number.isFinite(parsedCost) &&
    parsedCost > PLACEHOLDER_PRICE_THRESHOLD;

  async function handleSave() {
    if (!name.trim()) {
      setError(t("webSearch.dialog.nameRequired"));
      return;
    }
    if (!Number.isFinite(parsedCost) || parsedCost < 0) {
      setError(t("webSearch.dialog.costInvalid"));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const body: Record<string, unknown> = {
        name: name.trim(),
        kind,
        cost_per_query: parsedCost,
        enabled,
      };
      // Blank omits the field entirely: the API reads an absent (or null)
      // ordering as "leave the stored value alone", so this cannot unset one.
      if (failoverOrder.trim() !== "") {
        body.failover_order = Number(failoverOrder);
      }
      if (timeoutMs.trim() !== "") {
        body.timeout_ms = Number(timeoutMs);
      }
      const res = editing
        ? await apiPut<{ reload?: { ok?: boolean } }>(
            `/admin/websearch/providers/${editing.id}`,
            body,
          )
        : await apiPost<{ reload?: { ok?: boolean } }>(
            "/admin/websearch/providers",
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
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {editing
              ? t("webSearch.dialog.titleEdit")
              : t("webSearch.dialog.titleCreate")}
          </DialogTitle>
          <DialogDescription>{t("webSearch.dialog.description")}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="ws-provider-name">{t("webSearch.dialog.nameLabel")}</Label>
            <Input
              id="ws-provider-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("webSearch.dialog.namePlaceholder")}
              data-testid="ws-provider-name"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="ws-provider-kind">{t("webSearch.dialog.kindLabel")}</Label>
            <select
              id="ws-provider-kind"
              value={kind}
              onChange={(e) => setKind(e.target.value)}
              className="h-9 w-full rounded-md border bg-transparent px-3 text-sm"
              data-testid="ws-provider-kind"
            >
              {SUPPORTED_KINDS.map((k) => (
                <option key={k} value={k}>
                  {k}
                </option>
              ))}
            </select>
          </div>
          <div className="flex items-center justify-between rounded-md border px-3 py-2">
            <div>
              <Label htmlFor="ws-provider-enabled">
                {t("webSearch.dialog.enabledLabel")}
              </Label>
              <p className="text-xs text-muted-foreground">
                {t("webSearch.dialog.enabledHint")}
              </p>
            </div>
            <Switch
              id="ws-provider-enabled"
              checked={enabled}
              onCheckedChange={setEnabled}
              data-testid="ws-provider-enabled"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="ws-provider-cost">{t("webSearch.dialog.costLabel")}</Label>
            <Input
              id="ws-provider-cost"
              type="number"
              step="0.000001"
              min="0"
              value={cost}
              onChange={(e) => setCost(e.target.value)}
              data-testid="ws-provider-cost"
            />
            <p className="text-xs text-muted-foreground">
              {t("webSearch.dialog.costHint")}
            </p>
            {costIsPlaceholder && (
              <p
                className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-900"
                data-testid="ws-cost-placeholder-warning"
              >
                {t("webSearch.dialog.costPlaceholderWarning")}
              </p>
            )}
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="ws-provider-order">
                {t("webSearch.dialog.failoverOrderLabel")}
              </Label>
              <Input
                id="ws-provider-order"
                type="number"
                value={failoverOrder}
                onChange={(e) => setFailoverOrder(e.target.value)}
                placeholder={t("webSearch.dialog.optionalPlaceholder")}
              />
              <p className="text-xs text-muted-foreground">
                {t("webSearch.dialog.failoverOrderHint")}
              </p>
            </div>
            <div className="space-y-2">
              <Label htmlFor="ws-provider-timeout">
                {t("webSearch.dialog.timeoutLabel")}
              </Label>
              <Input
                id="ws-provider-timeout"
                type="number"
                value={timeoutMs}
                onChange={(e) => setTimeoutMs(e.target.value)}
                placeholder={t("webSearch.dialog.optionalPlaceholder")}
              />
            </div>
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
