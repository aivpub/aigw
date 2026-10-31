import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { ProbeRun, ProbeRunItem } from "./types";

/** Poll interval. A run's state changes once per server per few seconds at
 *  most, so a faster tick would add requests, not information. */
const POLL_MS = 500;

interface ProbeProgressDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  runId: string | null;
  /** Called once per run, when the polled snapshot stops being `running`. */
  onFinished: (run: ProbeRun) => void;
}

const STATE_BADGE: Record<ProbeRunItem["state"], string> = {
  pending: "bg-slate-100 text-slate-700",
  running: "bg-blue-100 text-blue-800",
  ok: "bg-emerald-100 text-emerald-800",
  failed: "bg-red-100 text-red-800",
};

const STATE_LABEL_KEY = {
  pending: "mcp.progress.statePending",
  running: "mcp.progress.stateRunning",
  ok: "mcp.progress.stateOk",
  failed: "mcp.progress.stateFailed",
} as const satisfies Record<ProbeRunItem["state"], string>;

export function ProbeProgressDialog({
  open,
  onOpenChange,
  runId,
  onFinished,
}: ProbeProgressDialogProps) {
  const { t } = useTranslation();
  const reported = useRef<string | null>(null);

  const { data, error } = useQuery<ProbeRun, Error>({
    queryKey: ["mcp-reload-run", runId],
    queryFn: () => apiGet<ProbeRun>(`/admin/mcp/reload/${runId}`),
    enabled: open && Boolean(runId),
    refetchInterval: (query) =>
      query.state.data?.status === "running" ? POLL_MS : false,
  });

  const run = data;
  const running = run?.status === "running";
  const failedCount = run?.items.filter((i) => i.state === "failed").length ?? 0;

  useEffect(() => {
    if (!run || running || reported.current === run.run_id) return;
    reported.current = run.run_id;
    onFinished(run);
  }, [run, running, onFinished]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] w-[calc(100vw-2rem)] max-w-2xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("mcp.progress.title")}</DialogTitle>
          <DialogDescription data-testid="mcp-progress-summary">
            {run
              ? t("mcp.progress.summary", { done: run.done, total: run.total })
              : "—"}
          </DialogDescription>
        </DialogHeader>

        {error ? (
          <p
            className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive break-all"
            data-testid="mcp-progress-error"
          >
            {t("mcp.progress.expired")}
          </p>
        ) : (
          <div className="space-y-3">
            <Progress
              value={run?.done ?? 0}
              max={run?.total ?? 1}
              data-testid="mcp-progress-bar"
            />

            {!run ? (
              <p className="py-4 text-center text-sm text-muted-foreground">
                {t("common.loading")}
              </p>
            ) : (
              <div className="-mx-1 overflow-x-auto px-1">
                <table className="w-full min-w-[28rem] text-xs">
                  <thead>
                    <tr className="text-left text-muted-foreground">
                      <th className="py-1 pr-2 font-medium">
                        {t("mcp.progress.server")}
                      </th>
                      <th className="py-1 pr-2 font-medium">
                        {t("mcp.progress.state")}
                      </th>
                      <th className="py-1 pr-2 font-medium">
                        {t("mcp.progress.toolCount")}
                      </th>
                      <th className="py-1 pr-2 font-medium">
                        {t("mcp.progress.protocolVersion")}
                      </th>
                      <th className="py-1 font-medium">{t("mcp.progress.error")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {run.items.map((item) => (
                      <tr
                        key={item.server_id}
                        data-testid="mcp-progress-row"
                        data-state={item.state}
                        className="border-t align-top"
                      >
                        <td className="max-w-[10rem] truncate py-1.5 pr-2 font-mono">
                          {item.alias}
                        </td>
                        <td className="py-1.5 pr-2">
                          <Badge
                            variant="outline"
                            className={STATE_BADGE[item.state]}
                            data-testid="mcp-progress-state"
                          >
                            {t(STATE_LABEL_KEY[item.state])}
                          </Badge>
                        </td>
                        <td className="py-1.5 pr-2 font-mono">
                          {item.tool_count ?? "—"}
                        </td>
                        <td className="py-1.5 pr-2 font-mono">
                          {item.protocol_version ?? "—"}
                        </td>
                        <td
                          className="max-w-[14rem] py-1.5 break-all text-destructive"
                          title={item.error ?? undefined}
                        >
                          {item.error ?? ""}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}

            {run && !running && (
              <p
                className={
                  failedCount > 0
                    ? "text-sm text-destructive"
                    : "text-sm text-muted-foreground"
                }
                data-testid="mcp-progress-result"
              >
                {failedCount > 0
                  ? t("mcp.progress.finishedPartial", {
                      failed: failedCount,
                      total: run.total,
                    })
                  : t("mcp.progress.finishedOk", { total: run.total })}
              </p>
            )}
            {running && (
              <p className="text-xs text-muted-foreground">
                {t("mcp.progress.inProgressHint")}
              </p>
            )}
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
