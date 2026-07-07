import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Key, Box, Users, Activity } from "lucide-react";

interface HealthMetrics {
  status: string;
  uptime_seconds: number;
  db: {
    size: number;
    connections: { pool_min: number; pool_max: number; pool_idle: number };
  };
  counts: {
    virtual_keys: number;
    proxy_models: number;
    organizations: number;
    teams: number;
    users: number;
  };
  version: string;
}

function formatUptime(seconds: number): string {
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

export function DashboardPage() {
  const { data, isLoading, error } = useQuery<HealthMetrics>({
    queryKey: ["health-metrics"],
    queryFn: () => apiGet("/health/metrics"),
    refetchInterval: 30_000,
  });

  if (error) {
    return (
      <div className="flex items-center justify-center h-64">
        <Card className="w-96">
          <CardHeader>
            <CardTitle className="text-destructive">Error loading metrics</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-sm text-muted-foreground">
              {(error as Error).message}
            </p>
          </CardContent>
        </Card>
      </div>
    );
  }

  const metrics = data;

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Dashboard</h1>
        <p className="text-sm text-muted-foreground">
          Overview of your AI Gateway
        </p>
      </div>

      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">API Keys</CardTitle>
            <Key className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <Skeleton className="h-8 w-16" />
            ) : (
              <div className="text-2xl font-bold">{metrics?.counts.virtual_keys ?? "-"}</div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Models</CardTitle>
            <Box className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <Skeleton className="h-8 w-16" />
            ) : (
              <div className="text-2xl font-bold">{metrics?.counts.proxy_models ?? "-"}</div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Users</CardTitle>
            <Users className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <Skeleton className="h-8 w-16" />
            ) : (
              <div className="text-2xl font-bold">{metrics?.counts.users ?? "-"}</div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Uptime</CardTitle>
            <Activity className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <Skeleton className="h-8 w-20" />
            ) : (
              <div className="text-2xl font-bold">
                {metrics ? formatUptime(metrics.uptime_seconds) : "-"}
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {metrics && (
        <Card>
          <CardHeader>
            <CardTitle>System Info</CardTitle>
          </CardHeader>
          <CardContent>
            <dl className="grid gap-2 text-sm">
              <div className="flex gap-2">
                <dt className="font-medium text-muted-foreground">Status:</dt>
                <dd className="text-green-600 font-medium">{metrics.status}</dd>
              </div>
              <div className="flex gap-2">
                <dt className="font-medium text-muted-foreground">Version:</dt>
                <dd>{metrics.version}</dd>
              </div>
              <div className="flex gap-2">
                <dt className="font-medium text-muted-foreground">DB Connections:</dt>
                <dd>{metrics.db.connections.pool_idle} idle / {metrics.db.connections.pool_max} max</dd>
              </div>
            </dl>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
