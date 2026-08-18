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
import type { ProxyItem } from "./types";

interface ProxyDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  editing?: ProxyItem | null;
  onSaved: () => void;
}

export function ProxyDialog({
  open,
  onOpenChange,
  editing,
  onSaved,
}: ProxyDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState(editing?.name ?? "");
  const [proxyUrl, setProxyUrl] = useState(
    editing?.proxy_url && editing.proxy_url !== "[encrypted]" ? editing.proxy_url : "",
  );
  const [expiresAt, setExpiresAt] = useState(editing?.expires_at ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSave() {
    if (!name.trim()) {
      setError(t("proxies.dialog.nameRequired"));
      return;
    }
    if (!proxyUrl.trim()) {
      setError(t("proxies.dialog.urlRequired"));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const body: Record<string, unknown> = { name: name.trim(), proxy_url: proxyUrl.trim() };
      if (expiresAt.trim()) body.expires_at = new Date(expiresAt).toISOString();
      if (editing) {
        await apiPut(`/admin/proxies/${editing.id}`, body);
      } else {
        await apiPost("/admin/proxies", body);
      }
      onSaved();
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
            {editing ? t("proxies.dialog.titleEdit") : t("proxies.dialog.titleCreate")}
          </DialogTitle>
          <DialogDescription>
            {t("proxies.dialog.proxyUrlHint")}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="proxy-name">{t("proxies.dialog.nameLabel")}</Label>
            <Input
              id="proxy-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("proxies.dialog.namePlaceholder")}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="proxy-url">{t("proxies.dialog.proxyUrlLabel")}</Label>
            <Input
              id="proxy-url"
              value={proxyUrl}
              onChange={(e) => setProxyUrl(e.target.value)}
              placeholder={t("proxies.dialog.proxyUrlPlaceholder")}
              data-testid="proxy-url-input"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="proxy-expires">{t("proxies.dialog.expiresAtLabel")}</Label>
            <Input
              id="proxy-expires"
              type="datetime-local"
              value={expiresAt}
              onChange={(e) => setExpiresAt(e.target.value)}
            />
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
            {saving ? t("common.saving") : t("proxies.dialog.saveBtn")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
