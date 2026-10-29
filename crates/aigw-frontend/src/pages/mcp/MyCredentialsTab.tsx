import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { KeyRound } from "lucide-react";
import { MyTokenDialog } from "./MyTokenDialog";
import {
  scopeI18nKey,
  type McpVisibleServer,
  type McpVisibleServerList,
} from "./types";

const scopeBadgeClass: Record<string, string> = {
  shared: "bg-slate-100 text-slate-700",
  shared_or_user: "bg-blue-100 text-blue-800",
  user_required: "bg-orange-100 text-orange-800",
};

function ScopeBadge({ scope }: { scope: string }) {
  const { t } = useTranslation();
  return (
    <Badge
      variant="outline"
      className={scopeBadgeClass[scope] ?? "bg-slate-100 text-slate-700"}
      data-testid="mcp-my-scope-badge"
    >
      {t(`mcp.scope.${scopeI18nKey(scope)}`)}
    </Badge>
  );
}

function TokenCell({ server }: { server: McpVisibleServer }) {
  const { t } = useTranslation();
  if (server.credential_scope === "shared") {
    return (
      <span className="text-xs text-muted-foreground" data-testid="mcp-my-token-not-needed">
        {t("mcp.myCredentials.notNeeded")}
      </span>
    );
  }
  if (server.has_my_token) {
    return (
      <Badge variant="secondary" data-testid="mcp-my-token-configured">
        {t("mcp.myCredentials.configured")}
      </Badge>
    );
  }
  if (server.credential_scope === "user_required") {
    return (
      <div className="space-y-1" data-testid="mcp-my-token-missing">
        <Badge variant="destructive">{t("mcp.myCredentials.missing")}</Badge>
        <p className="text-[11px] text-amber-800 dark:text-amber-300">
          {t("mcp.myCredentials.missingHint")}
        </p>
      </div>
    );
  }
  return (
    <span className="text-xs text-muted-foreground" data-testid="mcp-my-token-optional">
      {t("mcp.myCredentials.optionalHint")}
    </span>
  );
}

function ActionCell({
  server,
  onConfigure,
}: {
  server: McpVisibleServer;
  onConfigure: () => void;
}) {
  const { t } = useTranslation();
  if (server.credential_scope === "shared") return null;
  const urgent = server.credential_scope === "user_required" && !server.has_my_token;
  return (
    <Button
      size="sm"
      variant={urgent ? "default" : "outline"}
      className="h-7 px-2 text-xs"
      onClick={onConfigure}
      data-testid="mcp-my-token-configure"
    >
      <KeyRound className="h-3.5 w-3.5" />
      <span className="ml-1">
        {server.has_my_token ? t("mcp.myCredentials.replace") : t("mcp.myCredentials.set")}
      </span>
    </Button>
  );
}

export function MyCredentialsTab() {
  const { t } = useTranslation();
  const [target, setTarget] = useState<McpVisibleServer | null>(null);

  const { data, isLoading, error, refetch } = useQuery<McpVisibleServerList>({
    queryKey: ["mcp-my-servers"],
    queryFn: () => apiGet("/v1/mcp/servers"),
  });

  const servers = data?.data ?? [];
  const refresh = useCallback(() => {
    void refetch();
  }, [refetch]);

  return (
    <div className="space-y-4">
      <p className="text-xs text-muted-foreground" data-testid="mcp-my-creds-explainer">
        {t("mcp.myCredentials.explainer")}
      </p>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2">
            <KeyRound className="h-4 w-4" />
            {t("mcp.myCredentials.title")} ({servers.length})
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="hidden overflow-x-auto md:block">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("mcp.myCredentials.server")}</TableHead>
                  <TableHead>{t("mcp.myCredentials.scope")}</TableHead>
                  <TableHead>{t("mcp.myCredentials.myToken")}</TableHead>
                  <TableHead className="w-32">{t("common.actions")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoading
                  ? Array.from({ length: 3 }).map((_, i) => (
                      <TableRow key={i}>
                        {Array.from({ length: 4 }).map((_, j) => (
                          <TableCell key={j}>
                            <Skeleton className="h-4 w-full" />
                          </TableCell>
                        ))}
                      </TableRow>
                    ))
                  : servers.map((s) => {
                      const blocked =
                        s.credential_scope === "user_required" && !s.has_my_token;
                      return (
                        <TableRow
                          key={s.id}
                          data-server-id={s.id}
                          data-testid="mcp-my-cred-row"
                          data-blocked={blocked ? "true" : "false"}
                          className={blocked ? "bg-amber-50 dark:bg-amber-950/40" : undefined}
                        >
                          <TableCell className="font-medium" data-testid="mcp-my-cred-alias">
                            {s.alias}
                            <span className="block text-xs text-muted-foreground">
                              {s.display_name}
                            </span>
                          </TableCell>
                          <TableCell>
                            <ScopeBadge scope={s.credential_scope} />
                          </TableCell>
                          <TableCell><TokenCell server={s} /></TableCell>
                          <TableCell><ActionCell server={s} onConfigure={() => setTarget(s)} /></TableCell>
                        </TableRow>
                      );
                    })}
              </TableBody>
            </Table>
          </div>

          <div className="space-y-3 md:hidden">
            {servers.map((s) => {
              const blocked =
                s.credential_scope === "user_required" && !s.has_my_token;
              return (
                <Card
                  key={s.id}
                  data-server-id={s.id}
                  data-testid="mcp-my-cred-card"
                  data-blocked={blocked ? "true" : "false"}
                  className={blocked ? "bg-amber-50 dark:bg-amber-950/40" : undefined}
                >
                  <CardContent className="space-y-2 p-4">
                    <div className="min-w-0">
                      <div className="truncate font-medium text-sm" data-testid="mcp-my-cred-alias">
                        {s.alias}
                      </div>
                      <div className="truncate text-xs text-muted-foreground">
                        {s.display_name}
                      </div>
                    </div>
                    <div className="flex flex-wrap items-center gap-2 text-xs">
                      <ScopeBadge scope={s.credential_scope} />
                      <TokenCell server={s} />
                    </div>
                    <div className="pt-1"><ActionCell server={s} onConfigure={() => setTarget(s)} /></div>
                  </CardContent>
                </Card>
              );
            })}
          </div>

          {!isLoading && servers.length === 0 && (
            <div className="py-8 text-center" data-testid="mcp-my-creds-empty">
              <p className="text-sm text-muted-foreground">
                {t("mcp.myCredentials.empty")}
              </p>
            </div>
          )}

          {error && (
            <div className="mt-3 rounded-md bg-destructive/10 border border-destructive/30 px-3 py-2 text-sm text-destructive">
              {(error as Error).message}
            </div>
          )}
        </CardContent>
      </Card>

      <MyTokenDialog
        key={target?.id ?? "closed"}
        open={target !== null}
        onOpenChange={(o) => {
          if (!o) setTarget(null);
        }}
        server={target}
        onChanged={refresh}
      />
    </div>
  );
}
