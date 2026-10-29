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
import type { ProbeOutcome } from "./types";

interface ProbeDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  alias: string;
  outcome: ProbeOutcome | null;
  error: string | null;
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
  alias,
  outcome,
  error,
}: ProbeDialogProps) {
  const { t } = useTranslation();
  const infoName = outcome?.server_info?.name as string | undefined;
  const infoVersion = outcome?.server_info?.version as string | undefined;
  const serverName = infoName
    ? infoVersion
      ? `${infoName} v${infoVersion}`
      : infoName
    : (infoVersion ?? null);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] max-w-md overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("mcp.probe.title")}</DialogTitle>
          <DialogDescription>{alias}</DialogDescription>
        </DialogHeader>
        {outcome ? (
          <div className="space-y-2" data-testid="mcp-probe-outcome">
            <div className="flex items-center gap-2 pb-1">
              <Badge variant="default">{t("mcp.probe.ok")}</Badge>
            </div>
            <Row
              label={t("mcp.probe.protocolVersion")}
              value={outcome.protocol_version}
            />
            <Row label={t("mcp.probe.server")} value={serverName ?? "—"} />
            <Row
              label={t("mcp.probe.ttl")}
              value={outcome.ttl_ms != null ? `${outcome.ttl_ms} ms` : "—"}
            />
            <Row
              label={t("mcp.probe.toolCount")}
              value={
                <span data-testid="mcp-probe-tool-count">{outcome.tools.length}</span>
              }
            />
            {outcome.conflicts.length > 0 && (
              <div className="space-y-1 pt-2" data-testid="mcp-probe-conflicts">
                <p className="text-xs font-medium text-destructive">
                  {t("mcp.probe.conflictTitle", { n: outcome.conflicts.length })}
                </p>
                <ul className="space-y-0.5">
                  {outcome.conflicts.map((name) => (
                    <li
                      key={name}
                      className="rounded bg-destructive/10 px-2 py-1 font-mono text-xs text-destructive"
                      data-testid="mcp-probe-conflict-row"
                    >
                      {name}
                    </li>
                  ))}
                </ul>
                <p className="text-xs text-muted-foreground">
                  {t("mcp.probe.conflictHint")}
                </p>
              </div>
            )}
          </div>
        ) : error ? (
          <p
            className="rounded-md bg-destructive/10 border border-destructive/30 px-3 py-2 text-xs text-destructive break-all"
            data-testid="mcp-probe-error"
          >
            {error}
          </p>
        ) : (
          <div className="py-6 text-center text-muted-foreground">
            {t("mcp.probe.inProgress")}
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
