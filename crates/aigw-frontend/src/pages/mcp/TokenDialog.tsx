import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiDelete, apiGet, apiPost } from "@/lib/api";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Plus, Trash2 } from "lucide-react";
import { scopeI18nKey, type McpServer, type McpToken } from "./types";

interface TokenDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  server: McpServer | null;
}

function TokenRow({
  token,
  onAction,
  pending,
}: {
  token: McpToken;
  onAction: (token: McpToken, action: "default" | "delete") => void;
  pending: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div
      className="flex flex-wrap items-center gap-2 rounded-md border px-3 py-2 text-sm"
      data-testid="mcp-token-row"
      data-token-id={token.id}
    >
      <span className="font-medium">{token.token_name}</span>
      <span className="font-mono text-xs text-muted-foreground" data-testid="mcp-token-value">
        {token.token_redacted || t("mcp.token.unset")}
      </span>
      {token.is_default && (
        <Badge variant="secondary" data-testid="mcp-token-default-badge">
          {t("mcp.token.default")}
        </Badge>
      )}
      <Badge variant={token.enabled ? "default" : "secondary"}>
        {token.enabled ? t("common.enabled") : t("common.disabled")}
      </Badge>
      <span className="text-xs text-muted-foreground">
        {new Date(token.updated_at).toLocaleDateString()}
      </span>
      <div className="ml-auto flex items-center gap-1">
        {!token.is_default && token.enabled && (
          <Button
            size="sm"
            variant="outline"
            disabled={pending}
            onClick={() => onAction(token, "default")}
            data-testid="mcp-token-set-default"
          >
            {t("mcp.token.setDefault")}
          </Button>
        )}
        <Button
          size="sm"
          variant="ghost"
          className="text-destructive hover:text-destructive"
          disabled={pending}
          onClick={() => onAction(token, "delete")}
          data-testid="mcp-token-delete"
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </div>
    </div>
  );
}

export function TokenDialog({ open, onOpenChange, server }: TokenDialogProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [creating, setCreating] = useState(false);
  const [tokenName, setTokenName] = useState("");
  const [tokenValue, setTokenValue] = useState("");
  const [expiresAt, setExpiresAt] = useState("");
  const [makeDefault, setMakeDefault] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const resetForm = useCallback(() => {
    setCreating(false);
    setTokenName("");
    setTokenValue("");
    setExpiresAt("");
    setMakeDefault(false);
    setError(null);
  }, []);

  useEffect(() => {
    if (!open) resetForm();
  }, [open, resetForm]);

  const { data, isLoading } = useQuery<{ data?: McpToken[] } | McpToken[]>({
    queryKey: ["mcp-tokens", server?.id],
    queryFn: () => apiGet(`/admin/mcp/servers/${server?.id}/tokens`),
    enabled: open && server !== null,
  });

  const tokens: McpToken[] = Array.isArray(data) ? data : (data?.data ?? []);
  const shared = tokens.filter((tk) => tk.owner_type === "server");
  const userTokens = tokens.filter((tk) => tk.owner_type === "user");

  function invalidate() {
    queryClient.invalidateQueries({ queryKey: ["mcp-tokens", server?.id] });
  }

  async function handleCreate() {
    if (!server) return;
    if (!tokenName.trim() || !tokenValue.trim()) {
      setError(t("mcp.token.nameValueRequired"));
      return;
    }
    setPending(true);
    setError(null);
    try {
      await apiPost(`/admin/mcp/servers/${server.id}/tokens`, {
        token_name: tokenName.trim(),
        token_value: tokenValue,
        owner_type: "server",
        is_default: makeDefault,
        expires_at: expiresAt.trim() ? new Date(`${expiresAt}T00:00:00Z`).toISOString() : undefined,
      });
      toast.success(t("mcp.toast.tokenCreated"));
      resetForm();
      invalidate();
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setPending(false);
    }
  }

  async function handleAction(token: McpToken, action: "default" | "delete") {
    if (!server) return;
    setPending(true);
    try {
      if (action === "default") {
        await apiPost(`/admin/mcp/servers/${server.id}/tokens/${token.id}/default`);
        toast.success(t("mcp.toast.defaultSet"));
      } else {
        await apiDelete(`/admin/mcp/servers/${server.id}/tokens/${token.id}`);
        toast.success(t("mcp.toast.deleted"));
      }
      invalidate();
    } catch (err) {
      toast.error((err as Error).message);
    } finally {
      setPending(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] max-w-2xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {t("mcp.token.title")} — {server?.alias ?? ""}
          </DialogTitle>
          <DialogDescription>
            {t("mcp.token.scopeLabel")}:{" "}
            {server ? t(`mcp.scope.${scopeI18nKey(server.credential_scope)}`) : "—"}
            {server && (
              <span className="block text-xs">
                {t(`mcp.scopeHint.${scopeI18nKey(server.credential_scope)}`)}
              </span>
            )}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <section className="space-y-2" data-testid="mcp-token-shared-section">
            <h3 className="text-sm font-medium">{t("mcp.token.sharedSection")}</h3>
            {isLoading ? (
              <Skeleton className="h-10 w-full" />
            ) : shared.length === 0 ? (
              <p className="text-xs text-muted-foreground" data-testid="mcp-token-shared-empty">
                {t("mcp.token.sharedEmpty")}
              </p>
            ) : (
              shared.map((tk) => (
                <TokenRow key={tk.id} token={tk} onAction={handleAction} pending={pending} />
              ))
            )}
            {!creating && (
              <Button
                size="sm"
                variant="outline"
                onClick={() => setCreating(true)}
                data-testid="mcp-token-new"
              >
                <Plus className="h-3.5 w-3.5" /> {t("mcp.token.newShared")}
              </Button>
            )}
            {creating && (
              <div className="space-y-3 rounded-md border p-3" data-testid="mcp-token-form">
                <div className="space-y-1">
                  <Label htmlFor="mcp-token-name">{t("mcp.token.nameLabel")}</Label>
                  <Input
                    id="mcp-token-name"
                    value={tokenName}
                    onChange={(e) => setTokenName(e.target.value)}
                    placeholder={t("mcp.token.namePlaceholder")}
                    data-testid="mcp-token-name"
                  />
                </div>
                <div className="space-y-1">
                  <Label htmlFor="mcp-token-value">{t("mcp.token.valueLabel")}</Label>
                  <Input
                    id="mcp-token-value"
                    type="password"
                    autoComplete="new-password"
                    value={tokenValue}
                    onChange={(e) => setTokenValue(e.target.value)}
                    data-testid="mcp-token-value-input"
                  />
                  <p className="text-xs text-muted-foreground">{t("mcp.token.valueHint")}</p>
                </div>
                <div className="space-y-1">
                  <Label htmlFor="mcp-token-expires">{t("mcp.token.expiresLabel")}</Label>
                  <Input
                    id="mcp-token-expires"
                    type="date"
                    value={expiresAt}
                    onChange={(e) => setExpiresAt(e.target.value)}
                    data-testid="mcp-token-expires"
                  />
                </div>
                <div className="flex items-center gap-2">
                  <Switch
                    id="mcp-token-default"
                    checked={makeDefault}
                    onCheckedChange={setMakeDefault}
                    data-testid="mcp-token-set-default-checkbox"
                  />
                  <Label htmlFor="mcp-token-default">{t("mcp.token.makeDefault")}</Label>
                </div>
                <div className="flex items-center gap-2">
                  <Button size="sm" onClick={handleCreate} disabled={pending} data-testid="mcp-token-create">
                    {pending ? t("common.saving") : t("mcp.token.create")}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => setCreating(false)}
                    disabled={pending}
                  >
                    {t("common.cancel")}
                  </Button>
                </div>
                {error && <p className="text-xs text-destructive">{error}</p>}
              </div>
            )}
          </section>

          <section className="space-y-2">
            <h3 className="text-sm font-medium">
              {t("mcp.token.userSection", { n: userTokens.length })}
            </h3>
            {userTokens.length === 0 ? (
              <p className="text-xs text-muted-foreground">{t("mcp.token.userEmpty")}</p>
            ) : (
              userTokens.map((tk) => (
                <div key={tk.id} className="space-y-1">
                  <p className="text-xs text-muted-foreground">{tk.owner_id}</p>
                  <TokenRow token={tk} onAction={handleAction} pending={pending} />
                </div>
              ))
            )}
            <p className="text-xs text-muted-foreground">{t("mcp.token.userHint")}</p>
          </section>
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
