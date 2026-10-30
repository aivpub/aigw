import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { apiGet, apiPost, apiPut } from "@/lib/api";
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
  CREDENTIAL_SCOPES,
  extraHeaderPayload,
  scopeI18nKey,
  validateAlias,
  validateHeaderName,
  type CredentialScope,
  type McpServer,
  type McpServerListResponse,
} from "./types";

interface HeaderRow {
  name: string;
  value: string;
}

interface ServerDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  editing: McpServer | null;
  onSaved: () => void;
}

export function ServerDialog({
  open,
  onOpenChange,
  editing,
  onSaved,
}: ServerDialogProps) {
  const { t } = useTranslation();
  const [alias, setAlias] = useState(editing?.alias ?? "");
  const [displayName, setDisplayName] = useState(editing?.display_name ?? "");
  const [url, setUrl] = useState(editing?.url ?? "");
  const [scope, setScope] = useState<CredentialScope>(
    editing?.credential_scope ?? "shared_or_user",
  );
  const [enabled, setEnabled] = useState(editing?.enabled ?? true);
  // A stored name is a custom header, full stop. The backend keeps `NULL` for
  // "no name", so there is nothing to disambiguate — and treating the literal
  // name `Authorization` as the Bearer default would silently clear it on save.
  const [customAuth, setCustomAuth] = useState(Boolean(editing?.auth_header_name));
  const [authHeaderName, setAuthHeaderName] = useState(editing?.auth_header_name ?? "");
  const [authHeaderValue, setAuthHeaderValue] = useState("");
  // Names come back in clear, values as `***` — so the inputs start empty and an
  // empty value on an existing row means "keep it" (the backend merges by name).
  const [extraHeaders, setExtraHeaders] = useState<HeaderRow[]>(
    (editing?.extra_headers ?? []).map((h) => ({ name: h.name, value: "" })),
  );
  const [aliasTouched, setAliasTouched] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { data: all } = useQuery<McpServerListResponse>({
    queryKey: ["mcp-servers-all"],
    queryFn: () => apiGet("/admin/mcp/servers/all"),
    enabled: open,
  });

  const aliasProblem = validateAlias(alias.trim());
  const duplicate =
    !editing &&
    aliasProblem === null &&
    (all?.data ?? []).some((s) => s.alias === alias.trim());

  function aliasError(): string | null {
    if (!aliasTouched) return null;
    if (duplicate) return t("mcp.dialog.aliasDuplicate");
    if (!aliasProblem) return null;
    return t(`mcp.dialog.aliasErrors.${aliasProblem}`);
  }

  function headerNameError(row: HeaderRow): string | null {
    if (!row.name.trim()) return null;
    const problem = validateHeaderName(row.name);
    if (!problem) return null;
    return t(`mcp.dialog.headerErrors.${problem}`);
  }

  function authNameError(): string | null {
    if (!customAuth || !authHeaderName.trim()) return null;
    const problem = validateHeaderName(authHeaderName);
    if (!problem) return null;
    return t(`mcp.dialog.headerErrors.${problem}`);
  }

  async function handleSave() {
    setAliasTouched(true);
    if (aliasProblem || duplicate) return;
    if (!displayName.trim()) {
      setError(t("mcp.dialog.displayNameRequired"));
      return;
    }
    if (!/^https?:\/\//.test(url.trim())) {
      setError(t("mcp.dialog.urlInvalid"));
      return;
    }
    if (authNameError()) return;
    if (extraHeaders.some((row) => headerNameError(row))) return;
    if (customAuth && !authHeaderName.trim()) {
      setError(t("mcp.dialog.authNameRequired"));
      return;
    }
    if (customAuth && !authHeaderValue && !editing?.auth_header_value) {
      setError(t("mcp.dialog.authValueRequired"));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const headerFields: Record<string, unknown> = {};
      if (customAuth) {
        headerFields.auth_header_name = authHeaderName.trim();
        // Blank means "unchanged" on an edit, so the field is omitted rather
        // than sent empty — an empty string is the backend's "clear" signal and
        // would wipe a value the operator never asked to remove.
        if (authHeaderValue) headerFields.auth_header_value = authHeaderValue;
      } else if (editing?.auth_header_name) {
        // Switching back to Bearer has to clear the name explicitly — an absent
        // field would leave the custom name in place.
        headerFields.auth_header_name = "";
      }
      // Always sent, always the complete list: the backend replaces the set and
      // re-uses the stored value for any row left blank.
      headerFields.extra_headers = extraHeaderPayload(extraHeaders);
      const editable = {
        display_name: displayName.trim(),
        url: url.trim(),
        credential_scope: scope,
        enabled,
        ...headerFields,
      };
      if (editing) {
        await apiPut(`/admin/mcp/servers/${editing.id}`, editable);
      } else {
        await apiPost("/admin/mcp/servers", { alias: alias.trim(), ...editable });
      }
      onSaved();
      onOpenChange(false);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setSaving(false);
    }
  }

  const shownAliasError = aliasError();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] max-w-lg overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {editing ? t("mcp.dialog.titleEdit") : t("mcp.dialog.titleCreate")}
          </DialogTitle>
          <DialogDescription>{t("mcp.dialog.description")}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="mcp-server-alias">{t("mcp.dialog.aliasLabel")}</Label>
            <Input
              id="mcp-server-alias"
              value={alias}
              disabled={Boolean(editing)}
              onChange={(e) => {
                setAlias(e.target.value);
                setAliasTouched(true);
              }}
              placeholder={t("mcp.dialog.aliasPlaceholder")}
              aria-invalid={shownAliasError ? true : undefined}
              data-testid="mcp-server-alias-input"
            />
            <p className="text-xs text-muted-foreground">{t("mcp.dialog.aliasHint")}</p>
            {shownAliasError && (
              <p className="text-xs text-destructive" data-testid="mcp-alias-error">
                {shownAliasError}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="mcp-server-display-name">{t("mcp.dialog.displayNameLabel")}</Label>
            <Input
              id="mcp-server-display-name"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder={t("mcp.dialog.displayNamePlaceholder")}
              data-testid="mcp-server-display-name-input"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="mcp-server-url">{t("mcp.dialog.urlLabel")}</Label>
            <Input
              id="mcp-server-url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder={t("mcp.dialog.urlPlaceholder")}
              data-testid="mcp-server-url-input"
            />
          </div>
          <div className="space-y-2">
            <Label>{t("mcp.dialog.scopeLabel")}</Label>
            <div className="space-y-1.5">
              {CREDENTIAL_SCOPES.map((value) => (
                <label
                  key={value}
                  className="flex cursor-pointer items-start gap-2 rounded-md border px-3 py-2"
                  data-testid={`mcp-scope-${value}`}
                >
                  <input
                    type="radio"
                    name="mcp-credential-scope"
                    value={value}
                    checked={scope === value}
                    onChange={() => setScope(value)}
                    className="mt-0.5"
                  />
                  <span>
                    <span className="block text-sm font-medium">
                      {t(`mcp.scope.${scopeI18nKey(value)}`)}
                    </span>
                    <span className="block text-xs text-muted-foreground">
                      {t(`mcp.scopeHint.${scopeI18nKey(value)}`)}
                    </span>
                  </span>
                </label>
              ))}
            </div>
          </div>
          <div className="space-y-2">
            <Label>{t("mcp.dialog.authLabel")}</Label>
            <div className="space-y-1.5">
              <label
                className="flex cursor-pointer items-center gap-2 rounded-md border px-3 py-2"
                data-testid="mcp-auth-bearer"
              >
                <input
                  type="radio"
                  name="mcp-auth-mode"
                  checked={!customAuth}
                  onChange={() => setCustomAuth(false)}
                />
                <span className="text-sm">{t("mcp.dialog.authBearer")}</span>
              </label>
              <label
                className="flex cursor-pointer items-center gap-2 rounded-md border px-3 py-2"
                data-testid="mcp-auth-custom"
              >
                <input
                  type="radio"
                  name="mcp-auth-mode"
                  checked={customAuth}
                  onChange={() => setCustomAuth(true)}
                />
                <span className="text-sm">{t("mcp.dialog.authCustom")}</span>
              </label>
            </div>
            {customAuth && (
              <Input
                value={authHeaderName}
                onChange={(e) => setAuthHeaderName(e.target.value)}
                placeholder={t("mcp.dialog.authNamePlaceholder")}
                aria-invalid={authNameError() ? true : undefined}
                data-testid="mcp-auth-name-input"
              />
            )}
            {authNameError() && (
              <p className="text-xs text-destructive" data-testid="mcp-auth-name-error">
                {authNameError()}
              </p>
            )}
            <div className="flex items-center gap-2">
              <Input
                type="password"
                value={authHeaderValue}
                onChange={(e) => setAuthHeaderValue(e.target.value)}
                placeholder={
                  editing?.auth_header_value
                    ? t("mcp.dialog.secretKeepPlaceholder")
                    : t("mcp.dialog.secretPlaceholder")
                }
                data-testid="mcp-auth-value-input"
              />
              {editing?.auth_header_value && (
                <span
                  className="text-xs text-muted-foreground"
                  data-testid="mcp-auth-value-masked"
                >
                  {editing.auth_header_value}
                </span>
              )}
            </div>
          </div>
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label>{t("mcp.dialog.extraHeadersLabel")}</Label>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => setExtraHeaders([...extraHeaders, { name: "", value: "" }])}
                data-testid="mcp-extra-header-add"
              >
                {t("mcp.dialog.extraHeaderAdd")}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              {t("mcp.dialog.extraHeadersHint")}
            </p>
            {extraHeaders.map((row, index) => (
              <div key={index} className="space-y-1">
                <div className="flex items-center gap-2">
                  <Input
                    value={row.name}
                    onChange={(e) =>
                      setExtraHeaders(
                        extraHeaders.map((r, i) =>
                          i === index ? { ...r, name: e.target.value } : r,
                        ),
                      )
                    }
                    placeholder={t("mcp.dialog.extraHeaderNamePlaceholder")}
                    aria-invalid={headerNameError(row) ? true : undefined}
                    data-testid={`mcp-extra-header-name-${index}`}
                  />
                  <Input
                    type="password"
                    value={row.value}
                    onChange={(e) =>
                      setExtraHeaders(
                        extraHeaders.map((r, i) =>
                          i === index ? { ...r, value: e.target.value } : r,
                        ),
                      )
                    }
                    placeholder={t("mcp.dialog.secretKeepPlaceholder")}
                    data-testid={`mcp-extra-header-value-${index}`}
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => setExtraHeaders(extraHeaders.filter((_, i) => i !== index))}
                    data-testid={`mcp-extra-header-remove-${index}`}
                  >
                    {t("common.delete")}
                  </Button>
                </div>
                {headerNameError(row) && (
                  <p className="text-xs text-destructive">{headerNameError(row)}</p>
                )}
              </div>
            ))}
          </div>
          <div className="flex items-center justify-between rounded-md border px-3 py-2">
            <Label htmlFor="mcp-server-enabled">{t("mcp.dialog.enabledLabel")}</Label>
            <Switch
              id="mcp-server-enabled"
              checked={enabled}
              onCheckedChange={setEnabled}
              data-testid="mcp-server-enabled"
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
          <Button
            onClick={handleSave}
            disabled={saving || Boolean(shownAliasError)}
            data-testid="mcp-server-save"
          >
            {saving ? t("common.saving") : t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
