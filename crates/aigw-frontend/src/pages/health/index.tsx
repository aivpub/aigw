import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { CheckCircle, XCircle } from "lucide-react";

interface HealthResponse {
  status: string;
  uptime_seconds: number;
  db: {
    size: number;
    connections: {
      pool_min: number;
      pool_max: number;
      pool_idle: number;
    };
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

export function HealthPage() {
  const { data, isLoading } = useQuery<HealthResponse>({
    queryKey: ["health"],
    queryFn: () => apiGet("/health"),
  });

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Health</h1>
        <p className="text-sm text-muted-foreground">System health status</p>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
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

        <Card>
          <CardHeader>
            <CardTitle>Version</CardTitle>
          </CardHeader>
          <CardContent>
            {isLoading ? <Skeleton className="h-4 w-24" /> : <p className="text-2xl font-bold">{data?.version}</p>}
          </CardContent>
        </Card>

        <Card className="md:col-span-2">
          <CardHeader>
            <CardTitle>Database</CardTitle>
            <CardDescription>Connection pool status</CardDescription>
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <div className="space-y-2">
                <Skeleton className="h-4 w-full" />
                <Skeleton className="h-4 w-3/4" />
              </div>
            ) : (
              <dl className="grid grid-cols-3 gap-4 text-sm">
                <div>
                  <dt className="text-muted-foreground">Min</dt>
                  <dd className="text-lg font-semibold">{data?.db.connections.pool_min}</dd>
                </div>
                <div>
                  <dt className="text-muted-foreground">Max</dt>
                  <dd className="text-lg font-semibold">{data?.db.connections.pool_max}</dd>
                </div>
                <div>
                  <dt className="text-muted-foreground">Idle</dt>
                  <dd className="text-lg font-semibold">{data?.db.connections.pool_idle}</dd>
                </div>
              </dl>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
