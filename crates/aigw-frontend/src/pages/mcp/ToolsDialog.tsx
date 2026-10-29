import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { apiGet, apiPut } from "@/lib/api";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ShieldCheck } from "lucide-react";
import {
  ACL_SCOPE_TYPES,
  type AclScopeType,
  type McpAllowlistResponse,
  type McpAllowlistRule,
  type McpTool,
} from "./types";

interface ToolsResponse {
  tools: McpTool[];
  conflicts: string[];
  fetched_at: string | null;
}

interface ToolsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  serverId: number | null;
  alias: string;
}

export function ToolsDialog({
  open,
  onOpenChange,
  serverId,
  alias,
}: ToolsDialogProps) {
  const { t } = useTranslation();
  const [aclTool, setAclTool] = useState<string | null>(null);
  const [scopeType, setScopeType] = useState<AclScopeType>("team");
  const [scopeId, setScopeId] = useState("");
  const [allowed, setAllowed] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!open) {
      setAclTool(null);
      setScopeId("");
    }
  }, [open]);

  const { data, isLoading, error } = useQuery<ToolsResponse>({
    queryKey: ["mcp-server-tools", serverId],
    queryFn: () => apiGet(`/admin/mcp/servers/${serverId}/tools`),
    enabled: open && serverId !== null,
  });

  const { data: aclData, refetch: refetchAcl } = useQuery<McpAllowlistResponse>({
    queryKey: ["mcp-allowlist", serverId],
    queryFn: () => apiGet(`/admin/mcp/servers/${serverId}/allowlist`),
    enabled: open && serverId !== null,
  });

  const tools = data?.tools ?? [];
  const conflicts = new Set(data?.conflicts ?? []);
  const rules = aclData?.data ?? [];

  const rulesOf = (toolName: string) => rules.filter((r) => r.tool_name === toolName);

  function startAcl(toolName: string) {
    const existing = rulesOf(toolName)[0];
    setAclTool(toolName);
    setScopeType((existing?.scope_type as AclScopeType) ?? "team");
    setScopeId(existing?.scope_id ?? "");
    setAllowed(existing?.allowed ?? false);
  }

  async function saveAcl() {
    if (!serverId || !aclTool) return;
    if (!scopeId.trim()) return;
    setSaving(true);
    try {
      const target = scopeId.trim();
      const next: McpAllowlistRule[] = rules.filter(
        (r) =>
          !(
            r.tool_name === aclTool &&
            r.scope_type === scopeType &&
            r.scope_id === target
          ),
      );
      next.push({
        tool_name: aclTool,
        scope_type: scopeType,
        scope_id: target,
        allowed,
      });
      await apiPut(`/admin/mcp/servers/${serverId}/allowlist`, { rules: next });
      toast.success(t("mcp.toast.aclSaved"));
      setAclTool(null);
      setScopeId("");
      void refetchAcl();
    } catch (err) {
      toast.error((err as Error).message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] max-w-3xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("mcp.tools.title")}</DialogTitle>
          <DialogDescription>
            {alias} · {t("mcp.tools.count", { n: tools.length })}
            {data?.fetched_at && (
              <span className="block" data-testid="mcp-tools-fetched-at">
                {t("mcp.tools.probedAt")}: {data.fetched_at}
              </span>
            )}
          </DialogDescription>
        </DialogHeader>
        {isLoading ? (
          <div className="space-y-2">
            {Array.from({ length: 3 }).map((_, i) => (
              <Skeleton key={i} className="h-6 w-full" />
            ))}
          </div>
        ) : tools.length === 0 ? (
          <p className="py-6 text-center text-sm text-muted-foreground">
            {t("mcp.tools.empty")}
          </p>
        ) : (
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("mcp.tools.name")}</TableHead>
                  <TableHead>{t("mcp.tools.description")}</TableHead>
                  <TableHead className="w-24">{t("mcp.tools.conflict")}</TableHead>
                  <TableHead className="w-40">{t("mcp.tools.acl")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {tools.map((tool) => {
                  const isConflict = conflicts.has(tool.tool_name) || Boolean(tool.has_conflict);
                  const toolRules = rulesOf(tool.tool_name);
                  return (
                    <TableRow
                      key={tool.tool_name}
                      data-testid="mcp-tool-row"
                      className={isConflict ? "bg-destructive/5" : undefined}
                    >
                      <TableCell className="font-mono text-xs">{tool.tool_name}</TableCell>
                      <TableCell className="max-w-md">
                        <span className="line-clamp-2 text-xs text-muted-foreground">
                          {tool.description ?? "—"}
                        </span>
                      </TableCell>
                      <TableCell>
                        {isConflict ? (
                          <Badge variant="destructive">{t("mcp.tools.conflictBadge")}</Badge>
                        ) : (
                          <span className="text-xs text-muted-foreground">—</span>
                        )}
                      </TableCell>
                      <TableCell>
                        <div className="space-y-1">
                          {toolRules.length === 0 ? (
                            <span className="text-xs text-muted-foreground">
                              {t("mcp.tools.aclOpen")}
                            </span>
                          ) : (
                            toolRules.map((r) => (
                              <Badge
                                key={`${r.scope_type}:${r.scope_id}`}
                                variant={r.allowed ? "outline" : "destructive"}
                                className="block w-fit"
                                data-testid="mcp-tool-acl-rule"
                              >
                                {`${r.scope_type}:${r.scope_id} · ${r.allowed ? t("mcp.tools.aclAllowed") : t("mcp.tools.aclDenied")}`}
                              </Badge>
                            ))
                          )}
                          <Button
                            size="sm"
                            variant="ghost"
                            className="h-6 px-1 text-xs"
                            onClick={() => startAcl(tool.tool_name)}
                            data-testid="mcp-tool-acl-edit"
                          >
                            <ShieldCheck className="h-3 w-3 mr-1" />
                            {t("mcp.tools.aclEdit")}
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          </div>
        )}

        {aclTool && (
          <div className="space-y-3 rounded-md border p-3" data-testid="mcp-acl-form">
            <p className="text-sm font-medium">
              {t("mcp.tools.aclFormTitle", { tool: aclTool })}
            </p>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="space-y-1">
                <Label htmlFor="mcp-acl-scope-type">{t("mcp.tools.aclScopeType")}</Label>
                <select
                  id="mcp-acl-scope-type"
                  value={scopeType}
                  onChange={(e) => setScopeType(e.target.value as AclScopeType)}
                  className="h-9 w-full rounded-md border px-2 text-sm"
                  data-testid="mcp-acl-scope-type"
                >
                  {ACL_SCOPE_TYPES.map((s) => (
                    <option key={s} value={s}>
                      {t(`mcp.tools.aclScope.${s}`)}
                    </option>
                  ))}
                </select>
              </div>
              <div className="space-y-1">
                <Label htmlFor="mcp-acl-scope-id">{t("mcp.tools.aclScopeId")}</Label>
                <Input
                  id="mcp-acl-scope-id"
                  value={scopeId}
                  onChange={(e) => setScopeId(e.target.value)}
                  data-testid="mcp-acl-scope-id"
                />
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Switch
                id="mcp-acl-allowed"
                checked={allowed}
                onCheckedChange={setAllowed}
                data-testid="mcp-acl-allowed"
              />
              <Label htmlFor="mcp-acl-allowed">
                {allowed ? t("mcp.tools.aclAllowed") : t("mcp.tools.aclDenied")}
              </Label>
            </div>
            <p className="text-xs text-muted-foreground">{t("mcp.tools.aclHint")}</p>
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                onClick={saveAcl}
                disabled={saving || !scopeId.trim()}
                data-testid="mcp-acl-save"
              >
                {saving ? t("common.saving") : t("common.save")}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setAclTool(null)}
                disabled={saving}
              >
                {t("common.cancel")}
              </Button>
            </div>
          </div>
        )}

        {error && (
          <div className="rounded-md bg-destructive/10 border border-destructive/30 px-3 py-2 text-sm text-destructive">
            {(error as Error).message}
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
