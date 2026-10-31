import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { fromDatetimeLocalValue, presetRange, toDatetimeLocalValue, type TimePreset } from "@/lib/datetime";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { PaginationBar } from "@/components/ui/pagination";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ShieldAlert } from "lucide-react";
import type { McpRejectionListResponse, McpRejectionReason } from "./types";

const PRESETS = ["24h", "7d", "30d", "custom"] as const satisfies readonly TimePreset[];

const PRESET_LABEL: Record<(typeof PRESETS)[number], string> = {
  "24h": "mcp.calls.presets.24h",
  "7d": "mcp.calls.presets.7d",
  "30d": "mcp.calls.presets.30d",
  custom: "mcp.calls.presets.custom",
};

const REASONS: McpRejectionReason[] = ["missing_header", "invalid_format", "invalid_key"];

const REASON_LABEL: Record<
  McpRejectionReason,
  | "mcp.rejections.reason.missingHeader"
  | "mcp.rejections.reason.invalidFormat"
  | "mcp.rejections.reason.invalidKey"
> = {
  missing_header: "mcp.rejections.reason.missingHeader",
  invalid_format: "mcp.rejections.reason.invalidFormat",
  invalid_key: "mcp.rejections.reason.invalidKey",
};

/** Unknown reasons pass through from the API by design, so they render raw
 *  rather than disappearing behind a fallback label. */
function reasonLabel(t: (key: never) => string, reason: string) {
  const key = REASON_LABEL[reason as McpRejectionReason];
  return key ? t(key as never) : reason;
}

/** Amber rather than red: a rejection is a client that has not authenticated
 * yet, not a failure of this gateway. */
const REASON_CLASS = "bg-amber-100 text-amber-800";

export function RejectionsTab() {
  const { t } = useTranslation();
  const [reason, setReason] = useState("");
  const [preset, setPreset] = useState<TimePreset>("24h");
  const [range, setRange] = useState(() => presetRange("24h"));
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);

  const onPreset = (next: TimePreset) => {
    setPreset(next);
    setRange(presetRange(next));
    setPage(1);
  };

  const queryStr = useMemo(() => {
    const params = new URLSearchParams();
    params.set("page", String(page));
    params.set("page_size", String(pageSize));
    params.set("from", range.start);
    params.set("to", range.end);
    if (reason) params.set("reason", reason);
    return params.toString();
  }, [page, pageSize, range, reason]);

  const { data, isLoading, error } = useQuery<McpRejectionListResponse>({
    queryKey: ["mcp-rejections", queryStr],
    queryFn: () => apiGet(`/admin/mcp/rejections?${queryStr}`),
  });

  const rows = data?.data ?? [];
  const totalCount = data?.total_count ?? rows.length;
  const totalPages = data?.total_pages ?? 0;

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2">
          <ShieldAlert className="h-4 w-4" />
          {t("mcp.rejections.title")}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <p className="text-xs text-muted-foreground">{t("mcp.rejections.subtitle")}</p>

        <div
          className="flex flex-col gap-2 lg:flex-row lg:flex-wrap lg:items-center"
          data-testid="mcp-rejections-filters"
        >
          <select
            value={reason}
            onChange={(e) => {
              setReason(e.target.value);
              setPage(1);
            }}
            className="h-8 rounded-md border px-2 text-sm"
            data-testid="mcp-rejections-reason-filter"
          >
            <option value="">{t("common.all")}</option>
            {REASONS.map((r) => (
              <option key={r} value={r}>
                {t(REASON_LABEL[r])}
              </option>
            ))}
          </select>
          <div className="flex flex-wrap items-center gap-1">
            {PRESETS.map((p) => (
              <Button
                key={p}
                variant={preset === p ? "default" : "outline"}
                size="sm"
                className="h-7 text-xs"
                onClick={() => onPreset(p)}
                data-testid={`mcp-rejections-preset-${p}`}
              >
                {t(PRESET_LABEL[p] as never)}
              </Button>
            ))}
          </div>
        </div>

        {preset === "custom" && (
          <div className="flex flex-wrap items-center gap-2">
            <Input
              type="datetime-local"
              value={toDatetimeLocalValue(range.start)}
              onChange={(e) => {
                setRange((r) => ({ ...r, start: fromDatetimeLocalValue(e.target.value) }));
                setPage(1);
              }}
              className="h-8 w-48 text-xs"
              data-testid="mcp-rejections-from"
            />
            <span className="text-xs text-muted-foreground">–</span>
            <Input
              type="datetime-local"
              value={toDatetimeLocalValue(range.end)}
              onChange={(e) => {
                setRange((r) => ({ ...r, end: fromDatetimeLocalValue(e.target.value) }));
                setPage(1);
              }}
              className="h-8 w-48 text-xs"
              data-testid="mcp-rejections-to"
            />
          </div>
        )}

        <PaginationBar
          page={page}
          pageSize={pageSize}
          totalCount={totalCount}
          totalPages={totalPages}
          onPage={setPage}
          onPageSize={(s) => {
            setPageSize(s);
            setPage(1);
          }}
        />

        <div className="hidden overflow-x-auto md:block">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t("mcp.rejections.time")}</TableHead>
                <TableHead>{t("mcp.rejections.request")}</TableHead>
                <TableHead>{t("mcp.rejections.reasonLabel")}</TableHead>
                <TableHead>{t("mcp.rejections.clientIp")}</TableHead>
                <TableHead>{t("mcp.rejections.tokenHash")}</TableHead>
                <TableHead>{t("mcp.rejections.userAgent")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading
                ? Array.from({ length: 3 }).map((_, i) => (
                    <TableRow key={i}>
                      {Array.from({ length: 6 }).map((_, j) => (
                        <TableCell key={j}>
                          <Skeleton className="h-4 w-full" />
                        </TableCell>
                      ))}
                    </TableRow>
                  ))
                : rows.map((row) => (
                    <TableRow key={row.id} data-testid="mcp-rejection-row">
                      <TableCell className="whitespace-nowrap text-xs text-muted-foreground">
                        {new Date(row.created_at).toLocaleString()}
                      </TableCell>
                      <TableCell className="font-mono text-xs">
                        <span className="mr-1 text-muted-foreground">{row.http_method}</span>
                        {row.path}
                      </TableCell>
                      <TableCell>
                        <Badge
                          variant="outline"
                          className={REASON_CLASS}
                          data-testid="mcp-rejection-reason"
                        >
                          {reasonLabel(t, row.reason)}
                        </Badge>
                      </TableCell>
                      <TableCell className="font-mono text-xs">{row.client_ip ?? "—"}</TableCell>
                      <TableCell className="font-mono text-xs" title={row.token_hash ?? undefined}>
                        {row.token_hash ? `${row.token_hash.slice(0, 12)}…` : "—"}
                      </TableCell>
                      <TableCell
                        className="max-w-[16rem] truncate text-xs text-muted-foreground"
                        title={row.user_agent ?? undefined}
                      >
                        {row.user_agent ?? "—"}
                      </TableCell>
                    </TableRow>
                  ))}
            </TableBody>
          </Table>
        </div>

        <div className="space-y-2 md:hidden">
          {rows.map((row) => (
            <Card key={row.id} data-testid="mcp-rejection-card">
              <CardContent className="space-y-1 p-3 text-xs">
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate font-mono">
                    {row.http_method} {row.path}
                  </span>
                  <Badge
                    variant="outline"
                    className={REASON_CLASS}
                    data-testid="mcp-rejection-reason"
                  >
                    {reasonLabel(t, row.reason)}
                  </Badge>
                </div>
                <div className="flex flex-wrap gap-2 text-muted-foreground">
                  <span>{row.client_ip ?? "—"}</span>
                  <span className="font-mono" title={row.token_hash ?? undefined}>
                    {row.token_hash ? `${row.token_hash.slice(0, 12)}…` : "—"}
                  </span>
                  <span>{new Date(row.created_at).toLocaleString()}</span>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>

        {!isLoading && rows.length === 0 && !error && (
          <p
            className="py-6 text-center text-sm text-muted-foreground"
            data-testid="mcp-rejections-empty"
          >
            {t("mcp.rejections.empty")}
          </p>
        )}

        {error && (
          <div
            className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive"
            data-testid="mcp-rejections-error"
          >
            {(error as Error).message}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
