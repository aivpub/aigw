import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { isCallFailure, type McpCallListResponse, type McpTokenOwner } from "./types";

interface CallLogDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  serverAlias: string | null;
}

const statusClass: Record<string, string> = {
  success: "bg-emerald-100 text-emerald-800",
  error: "bg-red-100 text-red-800",
};

const ownerKey: Record<
  McpTokenOwner,
  "mcp.calls.owner.server" | "mcp.calls.owner.user" | "mcp.calls.owner.anonymous"
> = {
  server: "mcp.calls.owner.server",
  user: "mcp.calls.owner.user",
  anonymous: "mcp.calls.owner.anonymous",
};

export function CallLogDialog({ open, onOpenChange, serverAlias }: CallLogDialogProps) {
  const { t } = useTranslation();
  const [statusFilter, setStatusFilter] = useState("");
  const [toolFilter, setToolFilter] = useState("");

  const queryStr = useMemo(() => {
    const params = new URLSearchParams();
    params.set("page", "1");
    params.set("page_size", "30");
    if (serverAlias) params.set("server_alias", serverAlias);
    if (statusFilter) params.set("status", statusFilter);
    if (toolFilter.trim()) params.set("tool_name", toolFilter.trim());
    return params.toString();
  }, [serverAlias, statusFilter, toolFilter]);

  const { data, isLoading, error } = useQuery<McpCallListResponse>({
    queryKey: ["mcp-calls", queryStr],
    queryFn: () => apiGet(`/admin/mcp/calls?${queryStr}`),
    enabled: open,
  });

  const rows = data?.data ?? [];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] max-w-4xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("mcp.calls.title")}</DialogTitle>
          <DialogDescription>
            {serverAlias
              ? t("mcp.calls.descriptionFiltered", { alias: serverAlias })
              : t("mcp.calls.description")}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
          <Input
            value={toolFilter}
            onChange={(e) => setToolFilter(e.target.value)}
            placeholder={t("mcp.calls.toolPlaceholder")}
            className="h-8 sm:max-w-xs"
            data-testid="mcp-calls-tool-filter"
          />
          <select
            value={statusFilter}
            onChange={(e) => setStatusFilter(e.target.value)}
            className="h-8 rounded-md border px-2 text-sm"
            data-testid="mcp-calls-status-filter"
          >
            <option value="">{t("common.all")}</option>
            <option value="success">{t("mcp.calls.statusSuccess")}</option>
            <option value="error">{t("mcp.calls.statusError")}</option>
          </select>
        </div>

        {isLoading ? (
          <div className="space-y-2">
            {Array.from({ length: 3 }).map((_, i) => (
              <Skeleton key={i} className="h-6 w-full" />
            ))}
          </div>
        ) : rows.length === 0 ? (
          <p className="py-6 text-center text-sm text-muted-foreground" data-testid="mcp-calls-empty">
            {t("mcp.calls.empty")}
          </p>
        ) : (
          <>
            <div className="hidden overflow-x-auto md:block">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t("mcp.calls.server")}</TableHead>
                    <TableHead>{t("mcp.calls.tool")}</TableHead>
                    <TableHead>{t("mcp.calls.ownerLabel")}</TableHead>
                    <TableHead>{t("mcp.calls.status")}</TableHead>
                    <TableHead>{t("mcp.calls.latency")}</TableHead>
                    <TableHead>{t("mcp.calls.time")}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {rows.map((row) => {
                    const failed = isCallFailure(row.status);
                    return (
                      <TableRow
                        key={row.call_id}
                        data-testid="mcp-call-row"
                        data-failed={failed ? "true" : "false"}
                        className={failed ? "bg-red-50 dark:bg-red-950/40" : undefined}
                      >
                        <TableCell className="font-mono text-xs">{row.server_alias}</TableCell>
                        <TableCell className="max-w-[14rem] truncate font-mono text-xs">
                          {row.tool_name}
                        </TableCell>
                        <TableCell>
                          <Badge
                            variant="outline"
                            className={row.token_owner === "anonymous" ? "bg-amber-100 text-amber-800" : undefined}
                            data-testid="mcp-call-owner"
                          >
                            {t(ownerKey[row.token_owner])}
                          </Badge>
                        </TableCell>
                        <TableCell>
                          <Badge
                            variant="outline"
                            className={statusClass[failed ? "error" : "success"]}
                            data-testid="mcp-call-status"
                          >
                            {failed
                              ? `${t("mcp.calls.statusError")}${row.error_code ? ` ${row.error_code}` : ""}`
                              : t("mcp.calls.statusSuccess")}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-xs">{row.latency_ms} ms</TableCell>
                        <TableCell className="text-xs text-muted-foreground">
                          {new Date(row.created_at).toLocaleString()}
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </div>

            <div className="space-y-2 md:hidden">
              {rows.map((row) => {
                const failed = isCallFailure(row.status);
                return (
                  <Card
                    key={row.call_id}
                    data-testid="mcp-call-card"
                    data-failed={failed ? "true" : "false"}
                    className={failed ? "bg-red-50 dark:bg-red-950/40" : undefined}
                  >
                    <CardContent className="space-y-1 p-3 text-xs">
                      <div className="flex items-center justify-between gap-2">
                        <span className="font-mono font-medium">{row.server_alias}</span>
                        <Badge
                          variant="outline"
                          className={statusClass[failed ? "error" : "success"]}
                          data-testid="mcp-call-status"
                        >
                          {failed ? t("mcp.calls.statusError") : t("mcp.calls.statusSuccess")}
                        </Badge>
                      </div>
                      <div className="truncate font-mono text-muted-foreground">
                        {row.tool_name}
                      </div>
                      <div className="flex flex-wrap gap-2 text-muted-foreground">
                        <Badge variant="outline" data-testid="mcp-call-owner">
                          {t(ownerKey[row.token_owner])}
                        </Badge>
                        <span>{row.latency_ms} ms</span>
                        <span>{new Date(row.created_at).toLocaleString()}</span>
                      </div>
                    </CardContent>
                  </Card>
                );
              })}
            </div>
          </>
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
