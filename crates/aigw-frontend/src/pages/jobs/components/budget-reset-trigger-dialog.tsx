import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { triggerJob } from "@/lib/api/jobs";
import type {
  BudgetResetStats,
  BudgetResetEntityCount,
} from "@/lib/api/jobs";
import { toast } from "sonner";

/** Entity-type scope options shown in the trigger dialog. Values match the
 *  backend `EntityType::from_str` ("key"/"team"/"user"/"org") plus "all". */
const ENTITY_TYPE_OPTIONS = [
  { value: "all", labelKey: "jobs.budgetReset.entityTypes.all" },
  { value: "key", labelKey: "jobs.budgetReset.entityTypes.keys" },
  { value: "user", labelKey: "jobs.budgetReset.entityTypes.users" },
  { value: "team", labelKey: "jobs.budgetReset.entityTypes.teams" },
  { value: "org", labelKey: "jobs.budgetReset.entityTypes.orgs" },
] as const;

const EMPTY_COUNT: BudgetResetEntityCount = { ready: 0, total: 0 };

export function BudgetResetTriggerDialog({
  open,
  onOpenChange,
  stats,
  onSuccess,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  stats: BudgetResetStats | null;
  onSuccess: () => void;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [entityType, setEntityType] = useState<string>("all");
  const [loading, setLoading] = useState(false);

  const counts = stats?.counts ?? {};
  const readyTotal = stats?.ready_total ?? 0;

  // For a scoped trigger, estimate from the per-type count.
  const scopeReady =
    entityType === "all"
      ? readyTotal
      : (counts[entityType] ?? EMPTY_COUNT).ready;

  const handleTrigger = async () => {
    setLoading(true);
    try {
      const result = await triggerJob({
        step_type: "budget_reset",
        payload: entityType === "all" ? {} : { entity_type: entityType },
      });
      if (result.total_steps === 0) {
        toast.info(t("jobs.budgetReset.nothingToReset"));
        return; // stay open
      }
      toast.success(
        t("jobs.triggerToast", {
          jobId: result.job_id.slice(0, 12),
          totalSteps: result.total_steps,
        }),
      );
      onOpenChange(false);
      onSuccess();
      navigate(`/dash/jobs/${result.job_id}`);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(`${t("jobs.toast.triggerFailed")}: ${msg}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        onOpenChange(o);
        if (!o) setEntityType("all");
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("jobs.budgetReset.triggerReset")}</DialogTitle>
          <DialogDescription>
            {t("jobs.budgetReset.triggerDesc")}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          {/* Scope segmented control */}
          <div className="flex flex-wrap items-center gap-1.5">
            {ENTITY_TYPE_OPTIONS.map((opt) => (
              <button
                key={opt.value}
                type="button"
                onClick={() => setEntityType(opt.value)}
                className={`px-3 py-1.5 text-sm rounded-md font-medium transition-colors ${
                  entityType === opt.value
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:text-foreground hover:bg-accent"
                }`}
              >
                {t(opt.labelKey)}
              </button>
            ))}
          </div>

          {/* Estimate block */}
          {readyTotal > 0 ? (
            <div className="text-sm rounded-md bg-muted p-3 space-y-1">
              <p className="font-medium">
                {t("jobs.budgetReset.willReset", { count: scopeReady })}
              </p>
              <p className="text-xs text-muted-foreground">
                {t("jobs.budgetReset.entitiesReset", {
                  count: readyTotal,
                })}
              </p>
              <p className="text-xs text-muted-foreground">
                {t("jobs.budgetReset.resetSchedule")}
              </p>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground rounded-md bg-muted p-3">
              {t("jobs.budgetReset.nothingToReset")}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button
            onClick={handleTrigger}
            disabled={loading || readyTotal === 0}
          >
            {loading
              ? t("jobs.status.running")
              : t("jobs.budgetReset.confirmReset")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
