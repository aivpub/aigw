import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { useSearchParams } from "react-router-dom";
import { apiGet } from "@/lib/api";
import { useAuth } from "@/hooks/use-auth";
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
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { prettifyBody } from "@/components/log-viewer/utils";
import { ScrollText } from "lucide-react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  isCallFailure,
  type McpCallBody,
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
  | "mcp.calls.owner.server"
  | "mcp.calls.owner.user"
  | "mcp.calls.owner.anonymous"
  | "mcp.calls.owner.mixed"
> = {
  server: "mcp.calls.owner.server",
  user: "mcp.calls.owner.user",
  anonymous: "mcp.calls.owner.anonymous",
  mixed: "mcp.calls.owner.mixed",
};

/** Amber for `anonymous` and violet for `mixed`: both say "not every upstream
 * got a proper credential", but they are different facts and an operator
 * scanning the column must be able to tell them apart at a glance. */
function ownerClass(owner: McpTokenOwner) {
  if (owner === "anonymous") return "bg-amber-100 text-amber-800";
  if (owner === "mixed") return "bg-violet-100 text-violet-800";
  return undefined;
}

/** Show the readable name, keep the raw id one hover away. */
function DisplayName({
  name,
  raw,
  fallback,
}: {
  name: string | null | undefined;
  raw: string | null | undefined;
  fallback: string;
}) {
  const label = name ?? raw ?? fallback;
  if (!name) return <>{label}</>;
  return (
    <TooltipProvider delayDuration={0}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="cursor-help border-b border-dotted border-muted-foreground/40">
            {label}
          </span>
        </TooltipTrigger>
        <TooltipContent className="font-mono">{raw}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

function statusClass(failed: boolean) {
  return failed ? "bg-red-100 text-red-800" : "bg-emerald-100 text-emerald-800";
}

/** `24h`, not the spend log's `4h`: MCP calls are low-frequency, and an empty
 * table would read as "the feature is broken". */
const DEFAULT_PRESET: TimePreset = "24h";

const PRESET_VALUES = new Set<string>(PRESETS);

/** A hand-edited URL can carry anything, and a bad `from` reaching the API is a
 * 400 the user cannot read. Anything unparseable falls back to the default
 * window rather than being passed through. */
function parseIso(raw: string | null): string | null {
  if (!raw) return null;
  const d = new Date(raw);
  return isNaN(d.getTime()) ? null : d.toISOString();
}

function readFilters(params: URLSearchParams) {
  const presetParam = params.get("preset");
  const preset: TimePreset =
    presetParam && PRESET_VALUES.has(presetParam) ? (presetParam as TimePreset) : DEFAULT_PRESET;
  const page = Number(params.get("page") ?? 1);
  return {
    alias: params.get("alias") ?? "",
    tool: params.get("tool") ?? "",
    status: params.get("status") ?? "",
    preset,
    from: parseIso(params.get("from")),
    to: parseIso(params.get("to")),
    page: Number.isFinite(page) && page >= 1 ? Math.floor(page) : 1,
  };
}

/** A body the detail endpoint refused to return is not the same as one that was
 *  never recorded, and neither is an error worth hiding — the viewer says which
 *  of the three it is instead of rendering an empty pane. */
function BodyViewer({ callId }: { callId: string }) {
  const { t } = useTranslation();
  const endpoint = useAuth().userRole === "proxy_admin" ? "/admin" : "/v1";
  const { data, isLoading, error } = useQuery<McpCallBody>({
    queryKey: ["mcp-call-body", callId],
    queryFn: () => apiGet(`${endpoint}/mcp/calls/${encodeURIComponent(callId)}/body`),
    retry: false,
  });

  if (isLoading) return <Skeleton className="h-24 w-full" />;
  if (error) {
    const notRecorded = (error as { status?: number }).status === 404;
    return (
      <p
        className="rounded-md border bg-muted/40 px-3 py-2 text-xs text-muted-foreground"
        data-testid="mcp-call-body-missing"
      >
        {notRecorded ? t("mcp.calls.body.notRecorded") : t("mcp.calls.body.failed")}
      </p>
    );
  }

  const pane = (text: string | null | undefined, testId: string) => (
    <pre
      className="max-h-64 overflow-auto rounded-md border bg-muted/40 p-2 font-mono text-xs whitespace-pre-wrap break-all"
      data-testid={testId}
    >
      {text === null || text === undefined || text === ""
        ? t("mcp.calls.body.empty")
        : prettifyBody(text)}
    </pre>
  );

  return (
    <div className="space-y-2">
      {data?.truncated && (
        <p
          className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-800"
          data-testid="mcp-call-body-truncated"
        >
          {t("mcp.calls.body.truncated")}
        </p>
      )}
      <Tabs defaultValue="request">
        <TabsList>
          <TabsTrigger value="request">{t("mcp.calls.body.request")}</TabsTrigger>
          <TabsTrigger value="response">{t("mcp.calls.body.response")}</TabsTrigger>
        </TabsList>
        <TabsContent value="request">{pane(data?.request, "mcp-call-body-request")}</TabsContent>
        <TabsContent value="response">{pane(data?.response, "mcp-call-body-response")}</TabsContent>
      </Tabs>
    </div>
  );
}

export function CallLogTab() {
  const { t } = useTranslation();
  const { userRole } = useAuth();
  const isAdmin = userRole === "proxy_admin";
  const [params, setParams] = useSearchParams();
  const filters = readFilters(params);

  /** `presetRange` reads the clock, so it must not run on every render: a
   *  shifting `from`/`to` would make the query string differ each time and
   *  refetch in a loop. It is recomputed only when the preset changes. */
  const presetWindow = useMemo(() => presetRange(filters.preset), [filters.preset]);
  const range = {
    start: filters.from ?? presetWindow.start,
    end: filters.to ?? presetWindow.end,
  };

  const [tool, setTool] = useState(filters.tool);
  const [pageSize, setPageSize] = useState(30);
  const [selected, setSelected] = useState<McpCallRow | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    setTool(filters.tool);
  }, [filters.tool]);

  useEffect(() => {
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, []);

  /** Filter changes are `replace`: every keystroke in the tool box would
   * otherwise be a back-button stop. Tab switches use `push` (the layout). */
  const patch = useCallback(
    (next: Record<string, string | null>) => {
      setParams(
        (prev) => {
          const merged = new URLSearchParams(prev);
          for (const [key, value] of Object.entries(next)) {
            if (value === null || value === "") merged.delete(key);
            else merged.set(key, value);
          }
          return merged;
        },
        { replace: true },
      );
    },
    [setParams],
  );

  const setPage = useCallback(
    (page: number) => patch({ page: page === 1 ? null : String(page) }),
    [patch],
  );

  const onToolInput = useCallback(
    (value: string) => {
      setTool(value);
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(() => {
        patch({ tool: value.trim(), page: null });
      }, 300);
    },
    [patch],
  );

  const onPreset = useCallback(
    (next: TimePreset) => {
      const range = presetRange(next);
      patch({
        preset: next === DEFAULT_PRESET ? null : next,
        ...range,
        page: null,
      });
    },
    [patch],
  );

  const onCustomBound = useCallback(
    (bound: "from" | "to", local: string) => {
      const iso = fromDatetimeLocalValue(local);
      if (iso) patch({ [bound]: iso, page: null });
    },
    [patch],
  );

  const endpoint = isAdmin ? "/admin/mcp/calls" : "/v1/mcp/calls";

  const queryStr = useMemo(() => {
    const q = new URLSearchParams();
    q.set("page", String(filters.page));
    q.set("page_size", String(pageSize));
    q.set("from", range.start);
    q.set("to", range.end);
    if (filters.alias) q.set("server_alias", filters.alias);
    if (filters.tool) q.set("tool_name", filters.tool);
    if (filters.status) q.set("status", filters.status);
    return q.toString();
  }, [filters.alias, filters.tool, filters.status, filters.page, range.start, range.end, pageSize]);

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
              value={filters.alias}
              onChange={(e) => patch({ alias: e.target.value, page: null })}
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
              value={filters.status}
              onChange={(e) => patch({ status: e.target.value, page: null })}
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
                  variant={filters.preset === p ? "default" : "outline"}
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

          {filters.preset === "custom" && (
            <div className="flex flex-wrap items-center gap-2">
              <Input
                type="datetime-local"
                value={toDatetimeLocalValue(range.start)}
                onChange={(e) => onCustomBound("from", e.target.value)}
                className="h-8 w-48 text-xs"
                data-testid="mcp-calls-from"
              />
              <span className="text-xs text-muted-foreground">–</span>
              <Input
                type="datetime-local"
                value={toDatetimeLocalValue(range.end)}
                onChange={(e) => onCustomBound("to", e.target.value)}
                className="h-8 w-48 text-xs"
                data-testid="mcp-calls-to"
              />
            </div>
          )}

          <PaginationBar
            page={filters.page}
            pageSize={pageSize}
            totalCount={totalCount}
            totalPages={totalPages}
            onPage={setPage}
            onPageSize={(s) => {
              setPageSize(s);
              patch({ page: null });
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
                              className={ownerClass(row.token_owner)}
                              data-testid="mcp-call-owner"
                            >
                              {t(ownerKey[row.token_owner])}
                            </Badge>
                          </TableCell>
                          {isAdmin && (
                            <TableCell
                              className="text-xs text-muted-foreground"
                              data-testid="mcp-call-user"
                            >
                              <DisplayName
                                name={row.user_name}
                                raw={row.user_id}
                                fallback={t("mcp.calls.unboundUser")}
                              />
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
                      <Badge
                        variant="outline"
                        className={ownerClass(row.token_owner)}
                        data-testid="mcp-call-owner"
                      >
                        {t(ownerKey[row.token_owner])}
                      </Badge>
                      {isAdmin && (
                        <span data-testid="mcp-call-user">
                          <DisplayName
                            name={row.user_name}
                            raw={row.user_id}
                            fallback={t("mcp.calls.unboundUser")}
                          />
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
        <SheetContent className="w-full overflow-y-auto sm:max-w-2xl" data-testid="mcp-call-detail">
          <SheetHeader>
            <SheetTitle className="text-sm">{t("mcp.calls.detailTitle")}</SheetTitle>
            <SheetDescription className="font-mono text-xs">{selected?.call_id}</SheetDescription>
          </SheetHeader>
          {selected && (
            <>
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
                    [
                      "mcp.calls.key",
                      <DisplayName name={selected.key_name} raw={selected.key_id} fallback="—" />,
                    ],
                    [
                      "mcp.calls.user",
                      <DisplayName
                        name={selected.user_name}
                        raw={selected.user_id}
                        fallback={t("mcp.calls.unboundUser")}
                      />,
                    ],
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
              <div className="mt-6">
                <BodyViewer callId={selected.call_id} />
              </div>
            </>
          )}
        </SheetContent>
      </Sheet>
    </div>
  );
}
