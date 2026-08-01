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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { triggerJob } from "@/lib/api/jobs";
import { toast } from "sonner";

export function TriggerDialog({
  open,
  onOpenChange,
  onSuccess,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSuccess: () => void;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const now = new Date();
  const end = now.toISOString().slice(0, 16);
  const start = new Date(now.getTime() - 24 * 60 * 60 * 1000).toISOString().slice(0, 16);

  const [form, setForm] = useState({
    start_date: start,
    end_date: end,
    batch_size: "5000",
  });

  const handleTrigger = async () => {
    try {
      const startDate = new Date(form.start_date).toISOString();
      const endDate = new Date(form.end_date).toISOString();
      const batchSize = parseInt(form.batch_size, 10) || 5000;

      const data = await triggerJob({
        step_type: "body_archive",
        payload: {
          start_date: startDate,
          end_date: endDate,
          batch_size: batchSize,
        },
      });

      toast.success(t("jobs.triggerToast", { jobId: data.job_id.slice(0, 12), totalSteps: data.total_steps }));
      onOpenChange(false);
      onSuccess();
      navigate(`/dash/jobs/${data.job_id}`);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(`${t("jobs.toast.triggerFailed")}: ${msg}`);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("jobs.triggerArchive")}</DialogTitle>
          <DialogDescription>
            {t("jobs.triggerArchiveDesc")}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="start-date">{t("jobs.startDate")}</Label>
            <Input
              id="start-date"
              type="datetime-local"
              value={form.start_date}
              onChange={(e) => setForm({ ...form, start_date: e.target.value })}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="end-date">{t("jobs.endDate")}</Label>
            <Input
              id="end-date"
              type="datetime-local"
              value={form.end_date}
              onChange={(e) => setForm({ ...form, end_date: e.target.value })}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="batch-size">{t("jobs.batchSize")}</Label>
            <Input
              id="batch-size"
              type="number"
              min={100}
              max={50000}
              value={form.batch_size}
              onChange={(e) => setForm({ ...form, batch_size: e.target.value })}
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button onClick={handleTrigger}>{t("jobs.triggerJob")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
