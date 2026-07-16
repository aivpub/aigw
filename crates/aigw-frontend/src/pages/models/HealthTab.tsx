import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { CheckCircle, XCircle } from "lucide-react";

interface HealthData {
  status: string;
  uptime_seconds: number;
  db: { size: number; connections: { pool_min: number; pool_max: number; pool_idle: number } };
  counts: { virtual_keys: number; proxy_models: number; organizations: number; teams: number; users: number };
  version: string;
}

interface MetricsData {
  pool_size: number;
  idle: number;
  key_count: number;
  model_count: number;
  uptime_seconds: number;
}

export function HealthTab() {
  const { data, isLoading } = useQuery<HealthData>({
    queryKey: ["health"],
    queryFn: () => apiGet("/health"),
  });

  const { data: metrics } = useQuery<MetricsData>({
    queryKey: ["health-metrics"],
    queryFn: () => apiGet("/health/metrics"),
  });

  const uptimeMin = Math.floor((data?.uptime_seconds ?? 0) / 60);
  const uptimeStr = uptimeMin > 60
    ? `${Math.floor(uptimeMin / 60)}h ${uptimeMin % 60}m`
    : `${uptimeMin}m`;

  return (
    <div className="space-y-6">
      <div className="grid gap-4 md:grid-cols-2">
        {/* API Status */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              {isLoading ? (
                <Skeleton className="h-5 w-5 rounded-full" />
              ) : data?.status === "ok" ? (
                <CheckCircle className="h-5 w-5 text-green-500" />
              ) : (
                <XCircle className="h-5 w-5 text-destructive" />
              )}
              API Status
            </CardTitle>
          </CardHeader>
          <CardContent>
            {isLoading ? <Skeleton className="h-4 w-16" /> : <p className="text-2xl font-bold">{data?.status}</p>}
          </CardContent>
        </Card>

        {/* Version */}
        <Card>
          <CardHeader><CardTitle>Version</CardTitle></CardHeader>
          <CardContent>
            {isLoading ? <Skeleton className="h-4 w-24" /> : <p className="text-2xl font-bold">{data?.version}</p>}
          </CardContent>
        </Card>

        {/* Uptime */}
        <Card>
          <CardHeader><CardTitle>Uptime</CardTitle></CardHeader>
          <CardContent>
            {isLoading ? <Skeleton className="h-4 w-16" /> : <p className="text-2xl font-bold">{uptimeStr}</p>}
          </CardContent>
        </Card>

        {/* DB Pool */}
        <Card>
          <CardHeader>
            <CardTitle>Database Connections</CardTitle>
            <CardDescription>Connection pool status</CardDescription>
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <Skeleton className="h-4 w-3/4" />
            ) : (
              <dl className="grid grid-cols-3 gap-4 text-sm">
                <div>
                  <dt className="text-muted-foreground">Min</dt>
                  <dd className="text-lg font-semibold">{data?.db.connections.pool_min ?? "—"}</dd>
                </div>
                <div>
                  <dt className="text-muted-foreground">Max</dt>
                  <dd className="text-lg font-semibold">{data?.db.connections.pool_max ?? "—"}</dd>
                </div>
                <div>
                  <dt className="text-muted-foreground">Idle</dt>
                  <dd className="text-lg font-semibold">{data?.db.connections.pool_idle ?? "—"}</dd>
                </div>
              </dl>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Counts */}
      <Card>
        <CardHeader>
          <CardTitle>Resource Counts</CardTitle>
          <CardDescription>Entities tracked in the database</CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <Skeleton className="h-16 w-full" />
          ) : (
            <dl className="grid grid-cols-5 gap-4 text-center">
              <div>
                <dt className="text-xs text-muted-foreground">Keys</dt>
                <dd className="text-xl font-bold">{data?.counts.virtual_keys ?? "—"}</dd>
              </div>
              <div>
                <dt className="text-xs text-muted-foreground">Models</dt>
                <dd className="text-xl font-bold">{data?.counts.proxy_models ?? "—"}</dd>
              </div>
              <div>
                <dt className="text-xs text-muted-foreground">Orgs</dt>
                <dd className="text-xl font-bold">{data?.counts.organizations ?? "—"}</dd>
              </div>
              <div>
                <dt className="text-xs text-muted-foreground">Teams</dt>
                <dd className="text-xl font-bold">{data?.counts.teams ?? "—"}</dd>
              </div>
              <div>
                <dt className="text-xs text-muted-foreground">Users</dt>
                <dd className="text-xl font-bold">{data?.counts.users ?? "—"}</dd>
              </div>
            </dl>
          )}
        </CardContent>
      </Card>

      {/* Metrics card — compact, admin-only */}
      {metrics ? (
        <Card>
          <CardHeader>
            <CardTitle>Metrics</CardTitle>
            <CardDescription>Runtime operational metrics</CardDescription>
          </CardHeader>
          <CardContent>
            <dl className="grid grid-cols-4 gap-4 text-center text-sm">
              <div>
                <dt className="text-muted-foreground">Pool Size</dt>
                <dd className="text-lg font-semibold">{metrics.pool_size}</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Idle</dt>
                <dd className="text-lg font-semibold">{metrics.idle}</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Key Count</dt>
                <dd className="text-lg font-semibold">{metrics.key_count}</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Model Count</dt>
                <dd className="text-lg font-semibold">{metrics.model_count}</dd>
              </div>
            </dl>
          </CardContent>
        </Card>
      ) : null}
    </div>
  );
}
