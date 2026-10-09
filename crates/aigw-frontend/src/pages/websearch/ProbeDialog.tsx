import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

export interface ProbeOutcome {
  ok: boolean;
  http_status: number | null;
  latency_ms: number | null;
  result_count: number | null;
  parse_ok: boolean;
  error: string | null;
  probed_at: string;
}

interface ProbeDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  instanceName: string;
  baseUrl: string;
  outcome: ProbeOutcome | null;
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-4 border-b py-1.5 last:border-0">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="font-mono text-xs">{value}</span>
    </div>
  );
}

export function ProbeDialog({
  open,
  onOpenChange,
  instanceName,
  baseUrl,
  outcome,
}: ProbeDialogProps) {
  const { t } = useTranslation();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("webSearch.probe.title")}</DialogTitle>
          <DialogDescription>
            {instanceName} · {baseUrl}
          </DialogDescription>
        </DialogHeader>
        {outcome ? (
          <div className="space-y-2">
            <div className="flex items-center gap-2 pb-1">
              <Badge variant={outcome.ok ? "default" : "secondary"}>
                {outcome.ok
                  ? t("webSearch.probe.ok")
                  : t("webSearch.probe.failed")}
              </Badge>
              {!outcome.parse_ok && (
                <span
                  className="rounded bg-amber-100 px-1.5 py-0.5 text-xs font-medium text-amber-900"
                  data-testid="ws-probe-parse-warning"
                >
                  {t("webSearch.probe.parseFailed")}
                </span>
              )}
            </div>
            <Row label={t("webSearch.probe.httpStatus")} value={outcome.http_status ?? "—"} />
            <Row
              label={t("webSearch.probe.latency")}
              value={outcome.latency_ms != null ? `${outcome.latency_ms} ms` : "—"}
            />
            <Row
              label={t("webSearch.probe.resultCount")}
              value={outcome.result_count ?? "—"}
            />
            <Row
              label={t("webSearch.probe.parseOk")}
              value={outcome.parse_ok ? t("webSearch.probe.yes") : t("webSearch.probe.no")}
            />
            <Row label={t("webSearch.probe.probedAt")} value={outcome.probed_at} />
            {!outcome.parse_ok && (
              <p className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-900">
                {t("webSearch.probe.parseHint")}
              </p>
            )}
            {outcome.error && (
              <p className="rounded-md bg-destructive/10 border border-destructive/30 px-3 py-2 text-xs text-destructive break-all">
                {outcome.error}
              </p>
            )}
          </div>
        ) : (
          <div className="py-6 text-center text-muted-foreground">
            {t("webSearch.probe.noResult")}
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
