import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost } from "@/lib/api";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { CheckCircle, XCircle, Activity, RefreshCw } from "lucide-react";

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
}

interface CheckAllResponse {
  checked: number;
  healthy: number;
  unhealthy: number;
}

function fmtLatency(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return "—";
  return ms < 1000 ? `${ms.toFixed(0)}ms` : `${(ms / 1000).toFixed(1)}s`;
}

function fmtCheckedAt(iso: string | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  return d.toLocaleTimeString();
}

export function HealthTab() {
  const queryClient = useQueryClient();
  const [checking, setChecking] = useState(false);

  const { data, isLoading, error } = useQuery<HealthLatestResponse>({
    queryKey: ["health-latest"],
    queryFn: () => apiGet("/health/latest"),
  });

  const checks = data?.data ?? [];

  async function runCheckAll() {
    setChecking(true);
    try {
      const result = await apiPost("/model/health-check/all") as CheckAllResponse;
      // Refetch latest results
      await queryClient.invalidateQueries({ queryKey: ["health-latest"] });
      console.log(`Checked ${result.checked} models: ${result.healthy} healthy, ${result.unhealthy} unhealthy`);
    } catch (e) {
      console.error("Health check failed:", e);
    } finally {
      setChecking(false);
    }
  }

  const latestCheck = checks.length > 0
    ? checks.reduce((max, c) => (c.checked_at && c.checked_at > (max || "")) ? c.checked_at! : max, checks[0].checked_at)
    : null;

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Activity className="h-5 w-5" /> Model Health
          </CardTitle>
          <CardDescription>
            Ping each model's upstream endpoint to verify connectivity.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center gap-3">
            <Button onClick={runCheckAll} disabled={checking}>
              {checking ? <Spinner className="mr-2 h-4 w-4" /> : <RefreshCw className="mr-2 h-4 w-4" />}
              Check All Models
            </Button>
            {latestCheck && (
              <span className="text-xs text-muted-foreground">
                Last run: {fmtCheckedAt(latestCheck)}
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
              <p>No health checks run yet.</p>
              <p>Click "Check All Models" to run diagnostics.</p>
            </div>
          ) : (
            <>
              {/* Desktop table */}
              <div className="hidden md:block">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead className="w-8">Status</TableHead>
                      <TableHead>Model</TableHead>
                      <TableHead>Latency</TableHead>
                      <TableHead>Error</TableHead>
                      <TableHead>Checked</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {checks.map((c) => (
                      <TableRow key={c.model_name}>
                        <TableCell>
                          {c.status === "healthy" ? (
                            <CheckCircle className="h-4 w-4 text-green-500" />
                          ) : (
                            <XCircle className="h-4 w-4 text-destructive" />
                          )}
                        </TableCell>
                        <TableCell className="font-mono text-sm">{c.model_name}</TableCell>
                        <TableCell>{fmtLatency(c.response_time_ms)}</TableCell>
                        <TableCell className="max-w-[200px] truncate text-sm text-destructive">
                          {c.error_message || "—"}
                        </TableCell>
                        <TableCell className="text-xs text-muted-foreground">
                          {fmtCheckedAt(c.checked_at)}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>

              {/* Mobile card list */}
              <div className="md:hidden space-y-3">
                {checks.map((c) => (
                  <Card key={c.model_name}>
                    <CardContent className="p-3 flex items-center gap-3">
                      {c.status === "healthy" ? (
                        <CheckCircle className="h-5 w-5 shrink-0 text-green-500" />
                      ) : (
                        <XCircle className="h-5 w-5 shrink-0 text-destructive" />
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="font-mono text-sm truncate">{c.model_name}</div>
                        <div className="text-xs text-muted-foreground">
                          {fmtLatency(c.response_time_ms)}
                          {c.error_message && (
                            <span className="ml-2 text-destructive truncate block">{c.error_message}</span>
                          )}
                        </div>
                      </div>
                    </CardContent>
                  </Card>
                ))}
              </div>
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
