import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import type { BudgetResetPreviewItem, BudgetResetStats } from "@/lib/api/jobs";

/** Per-entity-type tile configuration; value matches the backend entity_type. */
const ENTITY_TYPES = [
  { value: "all", labelKey: "jobs.budgetReset.entityTypes.all" },
  { value: "key", labelKey: "jobs.budgetReset.entityTypes.keys" },
  { value: "team", labelKey: "jobs.budgetReset.entityTypes.teams" },
  { value: "user", labelKey: "jobs.budgetReset.entityTypes.users" },
  { value: "org", labelKey: "jobs.budgetReset.entityTypes.orgs" },
] as const;

const TYPE_COLORS: Record<string, string> = {
  key: "bg-blue-500/10 text-blue-500 border-blue-500/20",
  team: "bg-purple-500/10 text-purple-500 border-purple-500/20",
  user: "bg-green-500/10 text-green-500 border-green-500/20",
  org: "bg-orange-500/10 text-orange-500 border-orange-500/20",
};

function formatTs(ts: string | null | undefined): string {
  if (!ts) return "—";
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  return d.toLocaleString();
}

function spentLabel(p: BudgetResetPreviewItem): string {
  const max = p.max_budget != null ? ` / $${p.max_budget.toFixed(2)}` : "";
  return `$${p.spend.toFixed(4)}${max}`;
}

export function BudgetResetPreview({
  stats,
}: {
  stats: BudgetResetStats | null;
}) {
  const { t } = useTranslation();
  const [filter, setFilter] = useState<string>("all");

  const preview = stats?.preview ?? [];
  const filtered = useMemo(
    () => (filter === "all" ? preview : preview.filter((p) => p.entity_type === filter)),
    [preview, filter],
  );

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">
          {t("jobs.budgetReset.previewTitle")}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {/* Per-entity-type tiles — click to filter the preview below. */}
        <div className="flex flex-wrap gap-2">
          {ENTITY_TYPES.map((et) => {
            const count =
              et.value === "all"
                ? stats?.ready_total ?? 0
                : (stats?.counts?.[et.value]?.ready ?? 0);
            const total =
              et.value === "all"
                ? preview.length
                : (stats?.counts?.[et.value]?.total ?? 0);
            return (
              <button
                key={et.value}
                type="button"
                onClick={() => setFilter(et.value)}
                className={`px-3 py-1.5 text-xs rounded-md border font-medium transition-colors ${
                  filter === et.value
                    ? "bg-accent text-accent-foreground border-ring"
                    : "border-border text-muted-foreground hover:bg-accent/50"
                }`}
              >
                {t(et.labelKey)}{" "}
                <span className="font-mono">
                  {t("jobs.budgetReset.readyOf", {
                    ready: count,
                    total,
                  })}
                </span>
              </button>
            );
          })}
        </div>

        {filtered.length === 0 ? (
          <p className="text-sm text-muted-foreground text-center py-6">
            {t("jobs.budgetReset.previewEmpty")}
          </p>
        ) : (
          <div className="border rounded overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead className="bg-muted">
                  <tr>
                    <th className="text-left p-2">{t("jobs.table.stepType")}</th>
                    <th className="text-left p-2">
                      {t("jobs.table.jobId") === "Job ID" ? "Name" : t("jobs.table.stepType")}
                    </th>
                    <th className="text-left p-2">{t("jobs.budgetReset.entityType")}</th>
                    <th className="text-left p-2">{t("budgets.table.resetPeriod")}</th>
                    <th className="text-right p-2">{t("keys.table.budget")}</th>
                    <th className="text-left p-2">{t("jobs.budgetReset.lastReset")}</th>
                  </tr>
                </thead>
                <tbody>
                  {filtered.map((p) => (
                    <tr key={`${p.entity_type}:${p.entity_id}`} className="border-t">
                      <td className="p-2">
                        <Badge
                          variant="outline"
                          className={TYPE_COLORS[p.entity_type] ?? "bg-muted"}
                        >
                          {t(
                            `jobs.budgetReset.entityTypes.${
                              p.entity_type === "org" ? "orgs" : p.entity_type === "key" ? "keys" : `${p.entity_type}s`
                            }`,
                          )}
                        </Badge>
                      </td>
                      <td className="p-2 font-mono text-xs max-w-[200px] truncate" title={p.alias}>
                        {p.alias}
                      </td>
                      <td className="p-2 text-xs text-muted-foreground">
                        {p.entity_id.slice(0, 12)}
                      </td>
                      <td className="p-2 text-xs">{p.budget_duration}</td>
                      <td className="p-2 text-right text-xs">
                        {spentLabel(p)}
                      </td>
                      <td className="p-2 text-xs text-muted-foreground">
                        {formatTs(p.budget_reset_at)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
