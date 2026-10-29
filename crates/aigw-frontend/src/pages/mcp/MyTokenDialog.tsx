import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiDelete, apiGet, apiPut } from "@/lib/api";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
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
import { Trash2 } from "lucide-react";
import type { McpMyToken, McpVisibleServer } from "./types";

interface MyTokenDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  server: McpVisibleServer | null;
  onChanged: () => void;
}

function expired(expiresAt: string | null): boolean {
  if (!expiresAt) return false;
  const ts = Date.parse(expiresAt);
  return Number.isFinite(ts) && ts < Date.now();
}

export function MyTokenDialog({
  open,
  onOpenChange,
  server,
  onChanged,
}: MyTokenDialogProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [formOpen, setFormOpen] = useState(false);
  const [tokenName, setTokenName] = useState("personal");
  const [tokenValue, setTokenValue] = useState("");
  const [expiresAt, setExpiresAt] = useState("");
  const [pending, setPending] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const resetForm = useCallback(() => {
    setFormOpen(false);
    setTokenName("personal");
    setTokenValue("");
    setExpiresAt("");
    setConfirmDelete(false);
    setError(null);
  }, []);

  useEffect(() => {
    if (!open) resetForm();
  }, [open, resetForm]);

  const { data, isLoading } = useQuery<McpMyToken | null>({
    queryKey: ["mcp-my-token", server?.id],
    queryFn: async () => {
      try {
        return await apiGet<McpMyToken>(`/v1/mcp/servers/${server?.id}/my-token`);
      } catch {
        return null;
      }
    },
    enabled: open && server !== null,
  });

  const invalidate = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: ["mcp-my-token", server?.id] });
    queryClient.invalidateQueries({ queryKey: ["mcp-my-servers"] });
    onChanged();
  }, [queryClient, server?.id, onChanged]);

  async function handleSave() {
    if (!server) return;
    if (!tokenName.trim() || !tokenValue.trim()) {
      setError(t("mcp.myToken.nameValueRequired"));
      return;
    }
    setPending(true);
    setError(null);
    try {
      await apiPut(`/v1/mcp/servers/${server.id}/my-token`, {
        token_name: tokenName.trim(),
        token_value: tokenValue,
        expires_at: expiresAt.trim() ? new Date(`${expiresAt}T00:00:00Z`).toISOString() : undefined,
      });
      toast.success(t("mcp.toast.myTokenSaved"));
      resetForm();
      invalidate();
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setPending(false);
    }
  }

  async function handleDelete() {
    if (!server) return;
    setPending(true);
    setError(null);
    try {
      await apiDelete(`/v1/mcp/servers/${server.id}/my-token`);
      toast.success(t("mcp.toast.myTokenDeleted"));
      setConfirmDelete(false);
      resetForm();
      invalidate();
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setPending(false);
    }
  }

  const configured = Boolean(data);

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) resetForm();
        onOpenChange(o);
      }}
    >
      <DialogContent className="max-h-[90vh] max-w-lg overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {t("mcp.myToken.title")} — {server?.alias ?? ""}
          </DialogTitle>
          <DialogDescription>
            {t("mcp.myToken.description")}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {isLoading ? (
            <p className="text-xs text-muted-foreground">{t("common.loading")}</p>
          ) : configured && data ? (
            <div
              className="space-y-2 rounded-md border px-3 py-2"
              data-testid="mcp-my-token-current"
            >
              <div className="flex flex-wrap items-center gap-2 text-sm">
                <span className="font-medium">{data.token_name}</span>
                <span className="font-mono text-xs text-muted-foreground">***</span>
                <Badge variant="secondary">{t("mcp.myToken.configured")}</Badge>
                {data.is_default && (
                  <Badge variant="outline">{t("mcp.token.default")}</Badge>
                )}
                {expired(data.expires_at) && (
                  <Badge variant="destructive">{t("mcp.myToken.expired")}</Badge>
                )}
              </div>
              <p className="text-xs text-muted-foreground">
                {data.expires_at
                  ? t("mcp.myToken.expiresAt", {
                      date: new Date(data.expires_at).toLocaleString(),
                    })
                  : t("mcp.myToken.noExpiry")}
              </p>
            </div>
          ) : (
            <p className="text-xs text-muted-foreground" data-testid="mcp-my-token-unset">
              {t("mcp.myToken.unset")}
            </p>
          )}

          {formOpen ? (
            <div className="space-y-3 rounded-md border p-3" data-testid="mcp-my-token-form">
              <div className="space-y-1">
                <Label htmlFor="mcp-my-token-name">{t("mcp.myToken.nameLabel")}</Label>
                <Input
                  id="mcp-my-token-name"
                  value={tokenName}
                  onChange={(e) => setTokenName(e.target.value)}
                  placeholder="personal"
                  data-testid="mcp-my-token-name"
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="mcp-my-token-value">{t("mcp.myToken.valueLabel")}</Label>
                <Input
                  id="mcp-my-token-value"
                  type="password"
                  autoComplete="new-password"
                  value={tokenValue}
                  onChange={(e) => setTokenValue(e.target.value)}
                  data-testid="mcp-my-token-value-input"
                />
                <p className="text-xs text-muted-foreground">
                  {t("mcp.myToken.valueHint")}
                </p>
              </div>
              <div className="space-y-1">
                <Label htmlFor="mcp-my-token-expires">{t("mcp.myToken.expiresLabel")}</Label>
                <Input
                  id="mcp-my-token-expires"
                  type="date"
                  value={expiresAt}
                  onChange={(e) => setExpiresAt(e.target.value)}
                  data-testid="mcp-my-token-expires"
                />
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <Button
                  size="sm"
                  onClick={handleSave}
                  disabled={pending}
                  data-testid="mcp-my-token-save"
                >
                  {pending ? t("common.saving") : t("mcp.myToken.save")}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={resetForm}
                  disabled={pending}
                >
                  {t("common.cancel")}
                </Button>
              </div>
            </div>
          ) : (
            <Button
              size="sm"
              variant="outline"
              onClick={() => setFormOpen(true)}
              data-testid="mcp-my-token-set"
            >
              {configured ? t("mcp.myToken.replace") : t("mcp.myToken.set")}
            </Button>
          )}

          {confirmDelete ? (
            <div
              className="space-y-2 rounded-md border border-destructive/40 bg-destructive/5 p-3"
              data-testid="mcp-my-token-delete-confirm"
            >
              <p className="text-sm text-destructive">{t("mcp.myToken.deleteWarning")}</p>
              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  variant="destructive"
                  onClick={handleDelete}
                  disabled={pending}
                  data-testid="mcp-my-token-delete-confirm-button"
                >
                  {pending ? t("common.deleting") : t("common.delete")}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setConfirmDelete(false)}
                  disabled={pending}
                >
                  {t("common.cancel")}
                </Button>
              </div>
            </div>
          ) : (
            configured && (
              <Button
                size="sm"
                variant="ghost"
                className="text-destructive hover:text-destructive"
                onClick={() => setConfirmDelete(true)}
                data-testid="mcp-my-token-delete"
              >
                <Trash2 className="h-3.5 w-3.5" /> {t("mcp.myToken.delete")}
              </Button>
            )
          )}

          {error && (
            <p className="text-xs text-destructive" data-testid="mcp-my-token-error">
              {error}
            </p>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
