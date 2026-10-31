import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import {
  fromDatetimeLocalValue,
  presetRange,
  toDatetimeLocalValue,
  type TimePreset,
} from "@/lib/datetime";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { PaginationBar } from "@/components/ui/pagination";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ScrollText } from "lucide-react";
import {
  isCallFailure,
  type McpCallListResponse,
  type McpCallRow,
  type McpServerListResponse,
  type McpTokenOwner,
  type McpVisibleServerList,
} from "./types";

const PRESETS = ["1h", "24h", "7d", "30d", "custom"] as const satisfies readonly TimePreset[];

const PRESET_LABEL: Record<(typeof PRESETS)[number], string> = {
  "1h": "mcp.calls.presets.1h",
  "24h": "mcp.calls.presets.24h",
  "7d": "mcp.calls.presets.7d",
  "30d": "mcp.calls.presets.30d",
  custom: "mcp.calls.presets.custom",
};

const ownerKey: Record<
  McpTokenOwner,
  "mcp.calls.owner.server" | "mcp.calls.owner.user" | "mcp.calls.owner.anonymous"
> = {
  server: "mcp.calls.owner.server",
  user: "mcp.calls.owner.user",
  anonymous: "mcp.calls.owner.anonymous",
};

function statusClass(failed: boolean) {
  return failed ? "bg-red-100 text-red-800" : "bg-emerald-100 text-emerald-800";
}

/** `24h`, not the spend log's `4h`: MCP calls are low-frequency, and an empty
 * table would read as "the feature is broken". */
const DEFAULT_PRESET: TimePreset = "24h";

interface CallLogTabProps {
  isAdmin: boolean;
  /** Set by the row's log button; a server-scoped entry point. */
  alias: string;
  onAliasChange: (alias: string) => void;
}

export function CallLogTab({ isAdmin, alias, onAliasChange }: CallLogTabProps) {
  const { t } = useTranslation();
  const [tool, setTool] = useState("");
  const [toolFilter, setToolFilter] = useState("");
  const [status, setStatus] = useState("");
  const [preset, setPreset] = useState<TimePreset>(DEFAULT_PRESET);
  const [range, setRange] = useState(() => presetRange(DEFAULT_PRESET));
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);
  const [selected, setSelected] = useState<McpCallRow | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, []);

  const onToolInput = useCallback((value: string) => {
    setTool(value);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      setToolFilter(value.trim());
      setPage(1);
    }, 300);
  }, []);

  const onPreset = useCallback((next: TimePreset) => {
    setPreset(next);
    setRange(presetRange(next));
    setPage(1);
  }, []);

  const endpoint = isAdmin ? "/admin/mcp/calls" : "/v1/mcp/calls";

  const queryStr = useMemo(() => {
    const params = new URLSearchParams();
    params.set("page", String(page));
    params.set("page_size", String(pageSize));
    params.set("from", range.start);
    params.set("to", range.end);
    if (alias) params.set("server_alias", alias);
    if (toolFilter) params.set("tool_name", toolFilter);
    if (status) params.set("status", status);
    return params.toString();
  }, [page, pageSize, range, alias, toolFilter, status]);

  const { data, isLoading, error } = useQuery<McpCallListResponse>({
    queryKey: ["mcp-calls", endpoint, queryStr],
    queryFn: () => apiGet(`${endpoint}?${queryStr}`),
  });

  const { data: aliasSource } = useQuery<{ data: { alias: string }[] }>({
    queryKey: ["mcp-call-aliases", isAdmin],
    queryFn: () =>
      isAdmin
        ? apiGet<McpServerListResponse>("/admin/mcp/servers/all")
        : apiGet<McpVisibleServerList>("/v1/mcp/servers"),
    staleTime: 60_000,
  });

  const rows = data?.data ?? [];
  const totalCount = data?.total_count ?? rows.length;
  const totalPages = data?.total_pages ?? 0;

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2">
            <ScrollText className="h-4 w-4" />
            {t("mcp.calls.title")}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <div
            className="flex flex-col gap-2 lg:flex-row lg:flex-wrap lg:items-center"
            data-testid="mcp-calls-filters"
          >
            <datalist id="mcp-call-alias-options">
              {(aliasSource?.data ?? []).map((s) => (
                <option key={s.alias} value={s.alias} />
              ))}
            </datalist>
            <Input
              list="mcp-call-alias-options"
              value={alias}
              onChange={(e) => {
                onAliasChange(e.target.value);
                setPage(1);
              }}
              placeholder={t("mcp.calls.aliasPlaceholder")}
              className="h-8 lg:max-w-[14rem]"
              data-testid="mcp-calls-alias-filter"
            />
            <Input
              value={tool}
              onChange={(e) => onToolInput(e.target.value)}
              placeholder={t("mcp.calls.toolPlaceholder")}
              className="h-8 lg:max-w-[14rem]"
              data-testid="mcp-calls-tool-filter"
            />
            <select
              value={status}
              onChange={(e) => {
                setStatus(e.target.value);
                setPage(1);
              }}
              className="h-8 rounded-md border px-2 text-sm"
              data-testid="mcp-calls-status-filter"
            >
              <option value="">{t("common.all")}</option>
              <option value="success">{t("mcp.calls.statusSuccess")}</option>
              <option value="failed">{t("mcp.calls.statusError")}</option>
            </select>
            <div className="flex flex-wrap items-center gap-1">
              {PRESETS.map((p) => (
                <Button
                  key={p}
                  variant={preset === p ? "default" : "outline"}
                  size="sm"
                  className="h-7 text-xs"
                  onClick={() => onPreset(p)}
                  data-testid={`mcp-calls-preset-${p}`}
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
                data-testid="mcp-calls-from"
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
                data-testid="mcp-calls-to"
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
                  <TableHead>{t("mcp.calls.time")}</TableHead>
                  <TableHead>{t("mcp.calls.server")}</TableHead>
                  <TableHead>{t("mcp.calls.tool")}</TableHead>
                  <TableHead>{t("mcp.calls.status")}</TableHead>
                  <TableHead>{t("mcp.calls.ownerLabel")}</TableHead>
                  {isAdmin && <TableHead>{t("mcp.calls.user")}</TableHead>}
                  <TableHead>{t("mcp.calls.latency")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoading
                  ? Array.from({ length: 3 }).map((_, i) => (
                      <TableRow key={i}>
                        {Array.from({ length: isAdmin ? 7 : 6 }).map((_, j) => (
                          <TableCell key={j}>
                            <Skeleton className="h-4 w-full" />
                          </TableCell>
                        ))}
                      </TableRow>
                    ))
                  : rows.map((row) => {
                      const failed = isCallFailure(row.status);
                      return (
                        <TableRow
                          key={row.call_id}
                          data-testid="mcp-call-row"
                          data-failed={failed ? "true" : "false"}
                          className={`cursor-pointer ${failed ? "bg-red-50 dark:bg-red-950/40" : ""}`}
                          onClick={() => setSelected(row)}
                        >
                          <TableCell className="whitespace-nowrap text-xs text-muted-foreground">
                            {new Date(row.created_at).toLocaleString()}
                          </TableCell>
                          <TableCell className="font-mono text-xs">{row.server_alias}</TableCell>
                          <TableCell className="max-w-[14rem] truncate font-mono text-xs">
                            {row.tool_name ?? row.method}
                          </TableCell>
                          <TableCell>
                            <Badge
                              variant="outline"
                              className={statusClass(failed)}
                              data-testid="mcp-call-status"
                            >
                              {failed
                                ? `${t("mcp.calls.statusError")}${
                                    row.error_code ? ` ${row.error_code}` : ""
                                  }`
                                : t("mcp.calls.statusSuccess")}
                            </Badge>
                          </TableCell>
                          <TableCell>
                            <Badge
                              variant="outline"
                              className={
                                row.token_owner === "anonymous"
                                  ? "bg-amber-100 text-amber-800"
                                  : undefined
                              }
                              data-testid="mcp-call-owner"
                            >
                              {t(ownerKey[row.token_owner])}
                            </Badge>
                          </TableCell>
                          {isAdmin && (
                            <TableCell
                              className="font-mono text-xs text-muted-foreground"
                              data-testid="mcp-call-user"
                            >
                              {row.user_id ?? t("mcp.calls.unboundUser")}
                            </TableCell>
                          )}
                          <TableCell className="text-xs">{row.latency_ms} ms</TableCell>
                        </TableRow>
                      );
                    })}
              </TableBody>
            </Table>
          </div>

          <div className="space-y-2 md:hidden">
            {rows.map((row) => {
              const failed = isCallFailure(row.status);
              return (
                <Card
                  key={row.call_id}
                  data-testid="mcp-call-card"
                  data-failed={failed ? "true" : "false"}
                  className={`cursor-pointer ${failed ? "bg-red-50 dark:bg-red-950/40" : ""}`}
                  onClick={() => setSelected(row)}
                >
                  <CardContent className="space-y-1 p-3 text-xs">
                    <div className="flex items-center justify-between gap-2">
                      <span className="truncate font-mono font-medium">{row.server_alias}</span>
                      <Badge variant="outline" className={statusClass(failed)}>
                        {failed ? t("mcp.calls.statusError") : t("mcp.calls.statusSuccess")}
                      </Badge>
                    </div>
                    <div className="truncate font-mono text-muted-foreground">
                      {row.tool_name ?? row.method}
                    </div>
                    <div className="flex flex-wrap gap-2 text-muted-foreground">
                      <Badge variant="outline">{t(ownerKey[row.token_owner])}</Badge>
                      {isAdmin && (
                        <span className="font-mono" data-testid="mcp-call-user">
                          {row.user_id ?? t("mcp.calls.unboundUser")}
                        </span>
                      )}
                      <span>{row.latency_ms} ms</span>
                      <span>{new Date(row.created_at).toLocaleString()}</span>
                    </div>
                  </CardContent>
                </Card>
              );
            })}
          </div>

          {!isLoading && rows.length === 0 && !error && (
            <p
              className="py-6 text-center text-sm text-muted-foreground"
              data-testid="mcp-calls-empty"
            >
              {t("mcp.calls.empty")}
            </p>
          )}

          {error && (
            <div
              className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive"
              data-testid="mcp-calls-error"
            >
              {(error as Error).message}
            </div>
          )}
        </CardContent>
      </Card>

      <Sheet open={selected !== null} onOpenChange={(o) => !o && setSelected(null)}>
        <SheetContent className="w-full sm:max-w-md" data-testid="mcp-call-detail">
          <SheetHeader>
            <SheetTitle className="text-sm">{t("mcp.calls.detailTitle")}</SheetTitle>
            <SheetDescription className="font-mono text-xs">
              {selected?.call_id}
            </SheetDescription>
          </SheetHeader>
          {selected && (
            <dl className="mt-4 space-y-3 text-xs">
              {(
                [
                  ["mcp.calls.time", new Date(selected.created_at).toLocaleString()],
                  ["mcp.calls.server", selected.server_alias],
                  ["mcp.calls.tool", selected.tool_name ?? "—"],
                  ["mcp.calls.method", selected.method],
                  [
                    "mcp.calls.status",
                    isCallFailure(selected.status)
                      ? t("mcp.calls.statusError")
                      : t("mcp.calls.statusSuccess"),
                  ],
                  ["mcp.calls.errorCode", selected.error_code ?? "—"],
                  ["mcp.calls.ownerLabel", t(ownerKey[selected.token_owner])],
                  ["mcp.calls.keyId", selected.key_id ?? "—"],
                  ["mcp.calls.user", selected.user_id ?? t("mcp.calls.unboundUser")],
                  ["mcp.calls.latency", `${selected.latency_ms} ms`],
                  ["mcp.calls.requestBytes", String(selected.request_bytes ?? "—")],
                  ["mcp.calls.responseBytes", String(selected.response_bytes ?? "—")],
                ] as const
              ).map(([label, value]) => (
                <div key={label} className="flex items-start justify-between gap-3">
                  <dt className="shrink-0 text-muted-foreground">
                    <Label className="text-xs font-normal">{t(label as never)}</Label>
                  </dt>
                  <dd className="break-all text-right font-mono">{value}</dd>
                </div>
              ))}
            </dl>
          )}
        </SheetContent>
      </Sheet>
    </div>
  );
}
