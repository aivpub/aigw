import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { RefreshCw } from "lucide-react";
import type { BudgetResetStats, JobItem } from "@/lib/api/jobs";

/** Format a server timestamp (RFC3339 or "%Y-%m-%d %H:%M:%S") for display. */
function formatTs(ts: string | null | undefined): string {
  if (!ts) return "—";
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return d.toLocaleString();
}

export function BudgetResetStatsCard({
  stats,
  jobs,
}: {
  stats: BudgetResetStats | null;
  jobs: JobItem[];
}) {
  const { t } = useTranslation();

  // Honest next-tick countdown: re-armed from the server's next_tick_at on each
  // poll (stats refresh every 30s), NOT a self-referential local interval.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);

  const nextTickAt = stats ? new Date(stats.next_tick_at).getTime() : null;
  const remaining =
    nextTickAt !== null && !Number.isNaN(nextTickAt)
      ? Math.max(0, Math.floor((nextTickAt - now) / 1000))
      : null;

  const formatCountdown = (s: number): string => {
    const m = Math.floor(s / 60);
    const sec = s % 60;
    if (m > 0) return `${m}m ${sec.toString().padStart(2, "0")}s`;
    return `${sec}s`;
  };

  // Auto reset is always on when the budget_reset AsyncTask is registered.
  const autoOn = stats != null;

  // Last reset: when the stats endpoint loaded, its `last_reset` is authoritative
  // (null = never ran). Only fall back to the history list when the endpoint is
  // entirely unavailable (stats === null), so an explicit null shows "Never".
  const lastResetAt =
    stats == null
      ? (jobs.find((j) => j.status === "completed")?.updated_at ?? null)
      : (stats.last_reset?.completed_at ?? stats.last_reset?.started_at ?? null);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">
          {t("jobs.budgetReset.overview")}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
          <div>
            <div className="text-muted-foreground">{t("jobs.autoArchive")}</div>
            <Badge variant={autoOn ? "default" : "secondary"}>
              <span
                className={`mr-1 inline-block w-2 h-2 rounded-full ${autoOn ? "bg-green-500" : "bg-gray-400"}`}
              />
              {autoOn ? t("jobs.on") : t("jobs.off")}
            </Badge>
          </div>
          <div>
            <div className="text-muted-foreground">
              {t("jobs.budgetReset.readyToReset")}
            </div>
            <div className="font-medium text-lg">
              {stats?.ready_total != null && stats.ready_total > 0
                ? stats.ready_total
                : "—"}
            </div>
          </div>
          <div>
            <div className="text-muted-foreground">
              {t("jobs.budgetReset.lastReset")}
            </div>
            <div className="font-medium text-sm">
              {lastResetAt ? formatTs(lastResetAt) : t("jobs.budgetReset.neverReset")}
            </div>
          </div>
          <div>
            <div className="text-muted-foreground">
              {t("jobs.budgetReset.nextCheck")}
            </div>
            <div className="font-medium flex items-center gap-1">
              <RefreshCw className="h-3 w-3" />
              <span>
                {remaining != null
                  ? t("jobs.budgetReset.nextTick", { count: formatCountdown(remaining) })
                  : t("jobs.budgetReset.resetSchedule")}
              </span>
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
