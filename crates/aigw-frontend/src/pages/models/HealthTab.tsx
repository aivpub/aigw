import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost } from "@/lib/api";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  CheckCircle,
  XCircle,
  Activity,
  RefreshCw,
  Loader2,
} from "lucide-react";

interface HealthCheckItem {
  model_name: string;
  model_id?: string | null;
  status: string;
  response_time_ms?: number | null;
  error_message?: string | null;
  checked_at?: string;
}

interface HealthLatestResponse {
  data: HealthCheckItem[];
  count: number;
  last_success: Record<string, string | null>;
}

function fmtLatency(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return "—";
  return ms < 1000 ? `${ms.toFixed(0)}ms` : `${(ms / 1000).toFixed(1)}s`;
}

function fmtRelative(iso: string | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  const diffMs = Date.now() - d.getTime();
  if (diffMs < 60_000) return `${Math.floor(diffMs / 1000)}s ago`;
  if (diffMs < 3600_000) return `${Math.floor(diffMs / 60000)}m ago`;
  if (diffMs < 86400_000) return `${Math.floor(diffMs / 3600000)}h ago`;
  return d.toLocaleDateString();
}

export function HealthTab() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [checking, setChecking] = useState(false);

  const { data, isLoading, error } = useQuery<HealthLatestResponse>({
    queryKey: ["health-latest"],
    queryFn: () => apiGet("/health/latest"),
    refetchInterval: 3000, // poll while checking
  });

  const checks = data?.data ?? [];
  const anyChecking = checks.some((c) => c.status === "checking");

  // Stop polling faster once all checks are done
  // (react-query handles this via refetchInterval — we keep 3s for simplicity)

  async function runCheckAll() {
    setChecking(true);
    try {
      await apiPost("/model/health-check/all");
      // Start polling; results will arrive asynchronously
      await queryClient.invalidateQueries({ queryKey: ["health-latest"] });
    } catch (e) {
      console.error(t("models.health.healthCheckFailed"), e);
    } finally {
      // Keep checking state true until all are done
      setTimeout(() => setChecking(false), 2000);
    }
  }

  async function checkOne(modelId: string) {
    try {
      await apiPost(
        `/model/health-check?model_id=${encodeURIComponent(modelId)}`,
      );
      await queryClient.invalidateQueries({ queryKey: ["health-latest"] });
      setChecking(true);
      setTimeout(() => setChecking(false), 2000);
    } catch (e) {
      console.error(t("models.health.singleHealthCheckFailed"), e);
    }
  }

  const isRunning = checking || anyChecking;
  const latestCheck =
    checks.length > 0
      ? checks.reduce(
          (max, c) =>
            c.checked_at && c.checked_at > (max || "") ? c.checked_at! : max,
          checks[0].checked_at,
        )
      : null;

  return (
    <TooltipProvider>
      <div className="space-y-6">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Activity className="h-5 w-5" /> {t("models.health.title")}
            </CardTitle>
            <CardDescription>{t("models.health.description")}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center gap-3">
              <Button onClick={runCheckAll} disabled={isRunning}>
                {isRunning ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <RefreshCw className="mr-2 h-4 w-4" />
                )}
                {isRunning
                  ? t("models.health.checking")
                  : t("models.health.checkAll")}
              </Button>
              {latestCheck && !isRunning && (
                <span className="text-xs text-muted-foreground">
                  {t("models.health.lastRun")} {fmtRelative(latestCheck)}
                </span>
              )}
            </div>

            {error && (
              <div className="rounded-md bg-destructive/10 border border-destructive/30 px-4 py-2 text-sm text-destructive">
                {(error as Error).message}
              </div>
            )}

            {isLoading ? (
              <Skeleton className="h-32 w-full" />
            ) : checks.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-12 text-sm text-muted-foreground gap-2">
                <Activity className="h-8 w-8" />
                <p>{t("models.health.noRuns")}</p>
                <p>{t("models.health.noRunsHint")}</p>
              </div>
            ) : (
              <>
                <div className="hidden md:block">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead className="w-8">
                          {t("models.health.status")}
                        </TableHead>
                        <TableHead>{t("models.health.model")}</TableHead>
                        <TableHead>{t("models.health.latency")}</TableHead>
                        <TableHead>{t("models.health.lastSuccess")}</TableHead>
                        <TableHead>{t("models.health.error")}</TableHead>
                        <TableHead className="w-20">
                          {t("models.health.action")}
                        </TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {checks.map((c) => {
                        const lastOk = data?.last_success?.[c.model_name];
                        return (
                          <TableRow key={c.model_name}>
                            <TableCell>
                              {c.status === "healthy" ? (
                                <CheckCircle className="h-4 w-4 text-green-500" />
                              ) : c.status === "checking" ? (
                                <Loader2 className="h-4 w-4 text-blue-500 animate-spin" />
                              ) : (
                                <XCircle className="h-4 w-4 text-destructive" />
                              )}
                            </TableCell>
                            <TableCell className="font-mono text-sm">
                              {c.model_name}
                            </TableCell>
                            <TableCell>
                              {fmtLatency(c.response_time_ms)}
                            </TableCell>
                            <TableCell className="text-xs text-muted-foreground">
                              {lastOk ? fmtRelative(lastOk) : "—"}
                            </TableCell>
                            <TableCell className="max-w-[220px] truncate text-xs">
                              {c.status === "healthy" ? (
                                <span className="text-green-600">
                                  {t("models.health.noErrors")}
                                </span>
                              ) : c.status === "checking" ? (
                                <span className="text-blue-500 italic">
                                  {t("models.health.checkingStatus")}
                                </span>
                              ) : c.error_message &&
                                c.error_message.length > 40 ? (
                                <Tooltip>
                                  <TooltipTrigger asChild>
                                    <span className="text-destructive cursor-help underline decoration-dotted">
                                      {c.error_message.slice(0, 40)}…
                                    </span>
                                  </TooltipTrigger>
                                  <TooltipContent
                                    side="bottom"
                                    className="max-w-[400px] break-all text-xs p-3"
                                  >
                                    {c.error_message}
                                  </TooltipContent>
                                </Tooltip>
                              ) : (
                                <span className="text-destructive">
                                  {c.error_message ||
                                    t("models.health.unknownError")}
                                </span>
                              )}
                            </TableCell>
                            <TableCell>
                              <Button
                                variant="ghost"
                                size="sm"
                                className="h-7 text-xs"
                                disabled={c.status === "checking"}
                                onClick={() =>
                                  c.model_id && checkOne(c.model_id)
                                }
                              >
                                <RefreshCw className="h-3 w-3 mr-1" />
                                {t("models.health.checkBtn")}
                              </Button>
                            </TableCell>
                          </TableRow>
                        );
                      })}
                    </TableBody>
                  </Table>
                </div>

                <div className="md:hidden space-y-3">
                  {checks.map((c) => {
                    const lastOk = data?.last_success?.[c.model_name];
                    return (
                      <Card key={c.model_name}>
                        <CardContent className="p-3 space-y-2">
                          <div className="flex items-center gap-3">
                            {c.status === "healthy" ? (
                              <CheckCircle className="h-5 w-5 shrink-0 text-green-500" />
                            ) : c.status === "checking" ? (
                              <Loader2 className="h-5 w-5 shrink-0 text-blue-500 animate-spin" />
                            ) : (
                              <XCircle className="h-5 w-5 shrink-0 text-destructive" />
                            )}
                            <div className="min-w-0 flex-1">
                              <div className="font-mono text-sm truncate">
                                {c.model_name}
                              </div>
                              <div className="text-xs text-muted-foreground flex gap-3">
                                <span>{fmtLatency(c.response_time_ms)}</span>
                                {lastOk && (
                                  <span>
                                    {t("models.health.lastOk")}{" "}
                                    {fmtRelative(lastOk)}
                                  </span>
                                )}
                              </div>
                            </div>
                          </div>
                          {c.status === "checking" ? (
                            <div className="text-xs text-blue-500 italic">
                              {t("models.health.checkingStatus")}
                            </div>
                          ) : (
                            c.error_message && (
                              <div className="text-xs text-destructive bg-destructive/5 rounded px-2 py-1 break-all">
                                {c.error_message}
                              </div>
                            )
                          )}
                          <Button
                            variant="outline"
                            size="sm"
                            className="h-7 text-xs w-full"
                            disabled={c.status === "checking"}
                            onClick={() => c.model_id && checkOne(c.model_id)}
                          >
                            <RefreshCw className="h-3 w-3 mr-1" />
                            {t("models.health.reCheck")}
                          </Button>
                        </CardContent>
                      </Card>
                    );
                  })}
                </div>
              </>
            )}
          </CardContent>
        </Card>
      </div>
    </TooltipProvider>
  );
}
