import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import type { QualityItem } from "./types";

interface QualityDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  result?: {
    score: number;
    grade: string;
    overall_status: string;
    items: QualityItem[];
    last_check_at: string;
    latency_ms?: number | null;
  } | null;
  name?: string;
}

function statusBadgeClass(status: string): string {
  switch (status) {
    case "pass":
      return "bg-green-100 text-green-800";
    case "warn":
      return "bg-yellow-100 text-yellow-800";
    case "challenge":
      return "bg-purple-100 text-purple-800";
    default:
      return "bg-red-100 text-red-800";
  }
}

const ITEM_STATUS_KEY: Record<string, string> = {
  pass: "proxies.quality.itemStatus.pass",
  warn: "proxies.quality.itemStatus.warn",
  challenge: "proxies.quality.itemStatus.challenge",
  fail: "proxies.quality.itemStatus.fail",
};

export function QualityDialog({
  open,
  onOpenChange,
  result,
  name,
}: QualityDialogProps) {
  const { t } = useTranslation();
  const overallLabel =
    result?.overall_status === "healthy"
      ? t("proxies.quality.overallHealthy")
      : result?.overall_status === "warn"
        ? t("proxies.quality.overallWarn")
        : result?.overall_status === "failed"
          ? t("proxies.quality.overallFailed")
          : t("proxies.quality.overallChallenge");

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg max-h-[80vh] overflow-auto">
        <DialogHeader>
          <DialogTitle>{t("proxies.quality.title")}</DialogTitle>
          <DialogDescription>{name ?? ""}</DialogDescription>
        </DialogHeader>
        {result ? (
          <div className="space-y-4">
            <div className="flex flex-wrap gap-4">
              <div className="rounded-md border px-4 py-2">
                <div className="text-xs text-muted-foreground">{t("proxies.quality.score")}</div>
                <div className="text-2xl font-bold">{result.score}</div>
              </div>
              <div className="rounded-md border px-4 py-2">
                <div className="text-xs text-muted-foreground">{t("proxies.quality.grade")}</div>
                <div className="text-2xl font-bold">{result.grade}</div>
              </div>
              <div className="rounded-md border px-4 py-2">
                <div className="text-xs text-muted-foreground">{t("proxies.quality.overall")}</div>
                <div className="text-xl font-semibold">{overallLabel}</div>
              </div>
            </div>
            <div className="text-xs text-muted-foreground">
              {t("proxies.quality.lastCheckAt")} {new Date(result.last_check_at).toLocaleString()}
            </div>
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b text-left text-xs text-muted-foreground">
                  <th className="py-1 pr-2">{t("proxies.quality.target")}</th>
                  <th className="py-1 pr-2">{t("proxies.quality.status")}</th>
                  <th className="py-1 pr-2">{t("proxies.quality.latency")}</th>
                  <th className="py-1">{t("proxies.quality.message")}</th>
                </tr>
              </thead>
              <tbody>
                {result.items.map((item) => (
                  <tr key={item.target} className="border-b last:border-0">
                    <td className="py-1.5 pr-2 font-mono text-xs">{item.target}</td>
                    <td className="py-1.5 pr-2">
                      <span
                        className={`inline-block rounded px-1.5 py-0.5 text-xs font-medium ${statusBadgeClass(item.status)}`}
                      >
                        {t(ITEM_STATUS_KEY[item.status] as never)}
                      </span>
                    </td>
                    <td className="py-1.5 pr-2 text-xs">{item.latency_ms ?? "—"} ms</td>
                    <td className="py-1.5 text-xs text-muted-foreground">{item.message}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="py-6 text-center text-muted-foreground">
            {t("proxies.quality.noItems")}
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
