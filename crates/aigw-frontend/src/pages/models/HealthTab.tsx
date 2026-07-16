import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Spinner } from "@/components/ui/spinner";
import { CheckCircle, XCircle, Activity, Database, Key, Box, Users, Users2, Building2 } from "lucide-react";

interface CheckResult {
  label: string;
  ok: boolean;
  detail: string;
}

export function HealthTab() {
  const [running, setRunning] = useState(false);
  const [checks, setChecks] = useState<CheckResult[]>([]);
  const [lastRun, setLastRun] = useState<string | null>(null);

  const { data: metrics, isLoading } = useQuery({
    queryKey: ["health-metrics"],
    queryFn: () => apiGet("/health/metrics"),
    retry: false,
  });

  async function runHealthCheck() {
    setRunning(true);
    const results: CheckResult[] = [];
    const now = new Date().toLocaleTimeString();

    // Check 1: /health
    try {
      const h = await apiGet("/health");
      results.push({ label: "API Health", ok: (h as { status: string }).status === "ok", detail: JSON.stringify(h) });
    } catch (e) {
      results.push({ label: "API Health", ok: false, detail: String(e) });
    }

    // Check 2: /health/readiness
    try {
      const r = await apiGet("/health/readiness");
      results.push({ label: "Readiness", ok: (r as { ready: boolean }).ready === true, detail: JSON.stringify(r) });
    } catch (e) {
      results.push({ label: "Readiness", ok: false, detail: String(e) });
    }

    // Check 3: /health/liveliness
    try {
      const l = await apiGet("/health/liveliness");
      results.push({ label: "Liveliness", ok: (l as { alive: boolean }).alive === true, detail: JSON.stringify(l) });
    } catch (e) {
      results.push({ label: "Liveliness", ok: false, detail: String(e) });
    }

    // Check 4: /system/info
    try {
      const si = await apiGet("/system/info");
      results.push({ label: "System Info", ok: true, detail: JSON.stringify(si) });
    } catch (e) {
      results.push({ label: "System Info", ok: false, detail: String(e) });
    }

    setChecks(results);
    setLastRun(now);
    setRunning(false);
  }

  const m = metrics as Record<string, unknown> | undefined;
  const allOk = checks.length > 0 && checks.every((c) => c.ok);

  return (
    <div className="space-y-6">
      {/* Health Check Button & Results */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2"><Activity className="h-5 w-5" /> Health Check</CardTitle>
          <CardDescription>Run diagnostics against the aigw API.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center gap-3">
            <Button onClick={runHealthCheck} disabled={running}>
              {running ? <Spinner className="mr-2 h-4 w-4" /> : <Activity className="mr-2 h-4 w-4" />}
              Run Checks
            </Button>
            {lastRun && <span className="text-xs text-muted-foreground">Last run: {lastRun}</span>}
            {checks.length > 0 && (
              <Badge variant={allOk ? "default" : "destructive"} className={allOk ? "bg-green-600" : ""}>
                {allOk ? <CheckCircle className="mr-1 h-3 w-3" /> : <XCircle className="mr-1 h-3 w-3" />}
                {checks.filter((c) => c.ok).length}/{checks.length} passed
              </Badge>
            )}
          </div>

          {checks.length > 0 && (
            <div className="space-y-2">
              {checks.map((c) => (
                <div key={c.label} className="flex items-start gap-2 rounded-md border p-3 text-sm">
                  {c.ok ? (
                    <CheckCircle className="mt-0.5 h-4 w-4 shrink-0 text-green-500" />
                  ) : (
                    <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
                  )}
                  <div className="min-w-0 flex-1">
                    <span className="font-medium">{c.label}</span>
                    <p className="text-muted-foreground truncate font-mono text-xs">{c.detail}</p>
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Metrics */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2"><Database className="h-5 w-5" /> Overview</CardTitle>
          <CardDescription>Runtime metrics &amp; counts.</CardDescription>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <Skeleton className="h-32 w-full" />
          ) : (
            <div className="space-y-6">
              {/* Status row */}
              <div className="grid grid-cols-3 gap-4">
                <div>
                  <p className="text-xs text-muted-foreground">Status</p>
                  <p className="text-xl font-bold text-green-600">
                    {(m?.status as string) ?? "—"}
                  </p>
                </div>
                <div>
                  <p className="text-xs text-muted-foreground">Version</p>
                  <p className="text-xl font-bold">{m?.version as string ?? "—"}</p>
                </div>
                <div>
                  <p className="text-xs text-muted-foreground">Uptime</p>
                  <p className="text-xl font-bold">
                    {m?.uptime_seconds != null
                      ? `${Math.floor((m.uptime_seconds as number) / 3600)}h ${Math.floor(((m.uptime_seconds as number) % 3600) / 60)}m`
                      : "—"}
                  </p>
                </div>
              </div>

              {/* DB pool */}
              {m?.db != null && (
                <div>
                  <p className="text-xs text-muted-foreground mb-2">Database Pool</p>
                  <div className="grid grid-cols-3 gap-4 text-sm">
                    <div className="flex items-center gap-2">
                      <Database className="h-4 w-4 text-muted-foreground" />
                      <span>Pool Size: <strong>{(m.db as Record<string, unknown>).pool_size as number ?? "—"}</strong></span>
                    </div>
                    <div className="flex items-center gap-2">
                      <span>Idle: <strong>{(m.db as Record<string, unknown>).idle as number ?? "—"}</strong></span>
                    </div>
                    <div className="flex items-center gap-2">
                      <span>Connected: <strong>{(m.db as Record<string, unknown>).connected ? "yes" : "no"}</strong></span>
                    </div>
                  </div>
                </div>
              )}

              {/* Counts */}
              {m?.counts != null && (
                <div>
                  <p className="text-xs text-muted-foreground mb-2">Resource Counts</p>
                  <div className="grid grid-cols-5 gap-4 text-center">
                    {[
                      { icon: Key, label: "Keys", key: "virtual_keys" },
                      { icon: Box, label: "Models", key: "proxy_models" },
                      { icon: Building2, label: "Orgs", key: "organizations" },
                      { icon: Users2, label: "Teams", key: "teams" },
                      { icon: Users, label: "Users", key: "users" },
                    ].map(({ icon: Icon, label, key }) => (
                      <div key={key}>
                        <Icon className="mx-auto mb-1 h-4 w-4 text-muted-foreground" />
                        <dt className="text-[10px] text-muted-foreground">{label}</dt>
                        <dd className="text-lg font-semibold">{(m.counts as Record<string, unknown>)[key] as number ?? "—"}</dd>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
