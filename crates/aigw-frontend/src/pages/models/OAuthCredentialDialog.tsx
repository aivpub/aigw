import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { apiGet, apiPost } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { toast } from "sonner";

interface ProxyOption {
  id: number;
  name: string;
  status: string;
}

interface OAuthCredentialDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onExchanged: () => void;
}

export function OAuthCredentialDialog({
  open,
  onOpenChange,
  onExchanged,
}: OAuthCredentialDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [sessionKey, setSessionKey] = useState("");
  const [proxyId, setProxyId] = useState<string>("");
  const [injectPrompt, setInjectPrompt] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [exchanging, setExchanging] = useState(false);

  // Proxy dropdown (Stage 122 `/admin/proxies/all` — active proxies only).
  const { data: proxyData } = useQuery({
    queryKey: ["proxies-all"],
    queryFn: () => apiGet<{ data: ProxyOption[] }>("/admin/proxies/all"),
  });
  const activeProxies =
    (proxyData as { data?: ProxyOption[] })?.data?.filter(
      (p) => p.status === "active",
    ) ?? [];

  function reset() {
    setName("");
    setSessionKey("");
    setProxyId("");
    setInjectPrompt("");
    setError(null);
  }

  function handleOpenChange(next: boolean) {
    if (!next) reset();
    onOpenChange(next);
  }

  async function handleExchange() {
    setError(null);
    if (!name.trim()) {
      setError(t("claudeOAuth.nameRequired"));
      return;
    }
    if (!sessionKey.trim()) {
      setError(t("claudeOAuth.sessionKeyRequired"));
      return;
    }
    setExchanging(true);
    try {
      await apiPost("/credential/oauth/exchange", {
        session_key: sessionKey.trim(),
        proxy_id: proxyId ? Number(proxyId) : null,
        inject_prompt: injectPrompt.trim() || undefined,
        name: name.trim(),
      });
      onExchanged();
      reset();
      onOpenChange(false);
      toast.success(t("claudeOAuth.toast.exchanged"));
    } catch (e) {
      setError((e as Error).message);
      toast.error(t("claudeOAuth.toast.exchangeFailed"), {
        description: (e as Error).message,
      });
    } finally {
      setExchanging(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("claudeOAuth.new")}</DialogTitle>
          <DialogDescription>{t("claudeOAuth.sessionKeyHint")}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="oauth-name">{t("claudeOAuth.nameLabel")}</Label>
            <Input
              id="oauth-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("claudeOAuth.namePlaceholder")}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="oauth-session">
              {t("claudeOAuth.sessionKeyLabel")}
            </Label>
            <Textarea
              id="oauth-session"
              rows={3}
              className="font-mono text-xs"
              value={sessionKey}
              onChange={(e) => setSessionKey(e.target.value)}
              placeholder={t("claudeOAuth.sessionKeyPlaceholder")}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="oauth-proxy">{t("claudeOAuth.proxyLabel")}</Label>
            <Select value={proxyId} onValueChange={setProxyId}>
              <SelectTrigger id="oauth-proxy" className="text-xs">
                <SelectValue placeholder={t("claudeOAuth.direct")} />
              </SelectTrigger>
              <SelectContent>
                {activeProxies.length === 0 ? (
                  <SelectItem value="__none" disabled>
                    {t("claudeOAuth.noProxies")}
                  </SelectItem>
                ) : (
                  activeProxies.map((p) => (
                    <SelectItem key={p.id} value={String(p.id)}>
                      {p.name}
                    </SelectItem>
                  ))
                )}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">
              {t("claudeOAuth.proxyHint")}
            </p>
          </div>
          <div className="space-y-2">
            <Label htmlFor="oauth-inject">
              {t("claudeOAuth.injectPromptLabel")}
            </Label>
            <Textarea
              id="oauth-inject"
              rows={2}
              className="text-xs"
              value={injectPrompt}
              onChange={(e) => setInjectPrompt(e.target.value)}
              placeholder={t("claudeOAuth.injectPromptPlaceholder")}
            />
          </div>
          {error && (
            <div
              className="rounded-md bg-destructive/10 border border-destructive/30 px-3 py-2 text-sm text-destructive"
              data-testid="oauth-dialog-error"
            >
              {error}
            </div>
          )}
        </div>
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => handleOpenChange(false)}
            disabled={exchanging}
          >
            {t("common.cancel")}
          </Button>
          <Button onClick={handleExchange} disabled={exchanging}>
            {exchanging ? t("claudeOAuth.exchanging") : t("claudeOAuth.exchange")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
