import { useState, useCallback, useRef, useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  ScrollText,
  Calendar,
  RefreshCw,
  Search,
  Copy,
  Check,
  ChevronLeft,
  ChevronRight,
  X,
  Clock,
  Zap,
  AlertCircle,
  Download,
} from "lucide-react";
import { format } from "date-fns";
import { useCopyToClipboard } from "@/hooks/useCopyToClipboard";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface SpendLog {
  request_id: string;
  call_type: string;
  api_key: string;
  key_name?: string | null;
  spend: number;
  total_tokens: number;
  prompt_tokens: number;
  completion_tokens: number;
  start_time: string;
  end_time: string;
  request_duration_ms: number | null;
  ttft_ms: number | null;
  model: string;
  model_id?: string | null;
  model_group?: string | null;
  custom_llm_provider?: string | null;
  api_base?: string | null;
  user: string | null;
  team_id?: string | null;
  organization_id?: string | null;
  end_user?: string | null;
  session_id?: string | null;
  request_tags: unknown;
  metadata?: unknown;
  cache_hit?: unknown;
  cache_key?: string | null;
  mcp_namespaced_tool_name?: string | null;
  status: string | null;
  messages?: unknown;
  response?: unknown;
}

interface SpendLogsResponse {
  data: SpendLog[];
  count: number;
  total_count: number;
  page: number;
  page_size: number;
  total_pages: number;
}

type TimePreset = "15m" | "4h" | "24h" | "7d" | "custom";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function todayStr(): string {
  return format(new Date(), "yyyy-MM-dd");
}

function presetRange(p: TimePreset): { start: string; end: string } {
  const now = Date.now();
  switch (p) {
    case "15m":
      return { start: new Date(now - 15 * 60 * 1000).toISOString(), end: new Date(now).toISOString() };
    case "4h":
      return { start: new Date(now - 4 * 3600 * 1000).toISOString(), end: new Date(now).toISOString() };
    case "24h":
      return { start: new Date(now - 24 * 3600 * 1000).toISOString(), end: new Date(now).toISOString() };
    case "7d":
      return { start: new Date(now - 7 * 24 * 3600 * 1000).toISOString(), end: new Date(now).toISOString() };
    case "custom":
      return { start: "", end: "" };
  }
}

function fmtSpend(v: number): string {
  return `$${v.toFixed(4)}`;
}

function fmtTokens(v: number): string {
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`;
  return v.toString();
}

function fmtTtft(ms: number | null): string {
  if (ms === null || ms === undefined) return "—";
  if (ms < 1000) return `${ms.toFixed(0)}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function fmtDuration(ms: number | null): string {
  if (ms === null || ms === undefined) return "—";
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function truncate8(s: string): string {
  if (!s) return "—";
  return s.length > 8 ? s.slice(0, 8) + "…" : s;
}

function exportToCSV(logs: SpendLog[], startDate: string, endDate: string) {
  const headers = [
    "Request ID", "Time", "Type", "Model", "Status",
    "Prompt Tokens", "Completion Tokens", "Total Tokens",
    "TTFT (ms)", "Duration (ms)", "Cost", "User", "End User", "API Key",
  ];
  const rows = logs.map((log) => [
    log.request_id,
    log.start_time,
    log.call_type,
    log.model,
    log.status ?? "",
    log.prompt_tokens,
    log.completion_tokens,
    log.total_tokens,
    log.ttft_ms ?? "",
    log.request_duration_ms ?? "",
    log.spend,
    log.user ?? "",
    log.end_user ?? "",
    log.api_key.slice(0, 12) + "…",
  ]);
  const csv = [headers, ...rows]
    .map((r) => r.map((v) => `"${String(v).replace(/"/g, '""')}"`).join(","))
    .join("\n");
  const blob = new Blob(["﻿" + csv], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `spend-logs-${startDate.slice(0, 10)}-${endDate.slice(0, 10)}.csv`;
  a.click();
  URL.revokeObjectURL(url);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Time Preset Bar
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const PRESETS: { key: TimePreset; label: string }[] = [
  { key: "15m", label: "15 min" },
  { key: "4h", label: "4 hours" },
  { key: "24h", label: "24 hours" },
  { key: "7d", label: "7 days" },
  { key: "custom", label: "Custom" },
];

interface TimePresetBarProps {
  preset: TimePreset;
  onPreset: (p: TimePreset) => void;
  startDate: string;
  endDate: string;
  onStartDate: (v: string) => void;
  onEndDate: (v: string) => void;
}

function TimePresetBar({ preset, onPreset, startDate, endDate, onStartDate, onEndDate }: TimePresetBarProps) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      {PRESETS.map((p) => (
        <Button
          key={p.key}
          variant={preset === p.key ? "default" : "outline"}
          size="sm"
          onClick={() => onPreset(p.key)}
          className="h-7 text-xs"
        >
          {p.label}
        </Button>
      ))}
      {preset === "custom" && (
        <div className="flex items-center gap-2 ml-2">
          <Input
            type="datetime-local"
            value={startDate}
            onChange={(e) => onStartDate(e.target.value)}
            className="h-7 w-44 text-xs"
          />
          <span className="text-xs text-muted-foreground">–</span>
          <Input
            type="datetime-local"
            value={endDate}
            onChange={(e) => onEndDate(e.target.value)}
            className="h-7 w-44 text-xs"
          />
        </div>
      )}
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Pagination Bar
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface PaginationBarProps {
  page: number;
  pageSize: number;
  totalCount: number;
  totalPages: number;
  onPage: (p: number) => void;
  onPageSize: (s: number) => void;
}

function PaginationBar({ page, pageSize, totalCount, totalPages, onPage, onPageSize }: PaginationBarProps) {
  const from = totalCount === 0 ? 0 : (page - 1) * pageSize + 1;
  const to = Math.min(page * pageSize, totalCount);

  return (
    <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-2">
      <div className="flex items-center gap-3">
        <span className="text-xs text-muted-foreground">
          Showing {from}–{to} of {totalCount}
        </span>
        <span className="text-xs text-muted-foreground">
          Page {page} of {Math.max(totalPages, 1)}
        </span>
      </div>
      <div className="flex items-center gap-2">
        <Select value={String(pageSize)} onValueChange={(v) => onPageSize(Number(v))}>
          <SelectTrigger className="h-7 w-[70px] text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="30">30</SelectItem>
            <SelectItem value="50">50</SelectItem>
            <SelectItem value="100">100</SelectItem>
          </SelectContent>
        </Select>
        <Button
          variant="outline"
          size="sm"
          disabled={page <= 1}
          onClick={() => onPage(page - 1)}
          className="h-7 px-2"
        >
          <ChevronLeft className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={page >= totalPages || totalPages === 0}
          onClick={() => onPage(page + 1)}
          className="h-7 px-2"
        >
          <ChevronRight className="h-3.5 w-3.5" />
        </Button>
      </div>
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Detail Drawer
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface DetailDrawerProps {
  log: SpendLog | null;
  open: boolean;
  onClose: () => void;
}

function DetailDrawer({ log, open, onClose }: DetailDrawerProps) {
  if (!log) return null;
  const { copied, copy } = useCopyToClipboard();
  const isFailure = (log.status ?? "").startsWith("failure");
  const ttftText = fmtTtft(log.ttft_ms);
  const durText = fmtDuration(log.request_duration_ms);
  return (
    <Sheet open={open} onOpenChange={(o) => !o && onClose()}>
      <SheetContent side="right" className="overflow-y-auto">
        <SheetHeader>
          <SheetTitle className="text-sm font-mono">Request Details</SheetTitle>
          <SheetDescription className="text-[10px] font-mono break-all">{log.request_id}</SheetDescription>
        </SheetHeader>
        <div className="space-y-4 mt-4">
          <div className="grid grid-cols-2 gap-3 text-sm">
            <div><Label className="text-xs text-muted-foreground">Status</Label>
              <div><Badge variant={isFailure ? "destructive" : "default"} className="mt-0.5">{log.status || "—"}</Badge></div>
            </div>
            <div><Label className="text-xs text-muted-foreground">Type</Label>
              <div className="text-sm mt-0.5"><Badge variant="outline">{log.call_type || "—"}</Badge></div>
            </div>
            <div><Label className="text-xs text-muted-foreground">Model</Label>
              <div className="text-sm font-medium">{log.model}</div>
            </div>
            <div><Label className="text-xs text-muted-foreground">Cost</Label>
              <div className="text-sm font-mono">{fmtSpend(log.spend)}</div>
            </div>
          </div>
          {(log.model_id || log.model_group || log.custom_llm_provider || log.api_base) && (
            <div>
              <Label className="text-xs text-muted-foreground">Model Info</Label>
              <div className="text-xs space-y-0.5 mt-1 bg-muted/30 rounded p-2">
                {log.model_id && <div><span className="text-muted-foreground">ID:</span> {log.model_id}</div>}
                {log.model_group && <div><span className="text-muted-foreground">Group:</span> {log.model_group}</div>}
                {log.custom_llm_provider && <div><span className="text-muted-foreground">Provider:</span> {log.custom_llm_provider}</div>}
                {log.api_base && <div><span className="text-muted-foreground">Base:</span> <code className="text-[10px]">{log.api_base}</code></div>}
              </div>
            </div>
          )}
          <div>
            <Label className="text-xs text-muted-foreground">Tokens</Label>
            <div className="grid grid-cols-3 gap-2 mt-1 text-xs">
              <div className="rounded border p-2"><div className="text-muted-foreground">Prompt</div><div className="font-medium">{fmtTokens(log.prompt_tokens)}</div></div>
              <div className="rounded border p-2"><div className="text-muted-foreground">Completion</div><div className="font-medium">{fmtTokens(log.completion_tokens)}</div></div>
              <div className="rounded border p-2"><div className="text-muted-foreground">Total</div><div className="font-medium">{fmtTokens(log.total_tokens)}</div></div>
            </div>
          </div>
          <div>
            <Label className="text-xs text-muted-foreground">Timing</Label>
            <div className="grid grid-cols-2 gap-2 mt-1 text-xs">
              <div className="rounded border p-2"><div className="text-muted-foreground">TTFT</div><div className="font-mono font-medium">{ttftText}</div></div>
              <div className="rounded border p-2"><div className="text-muted-foreground">Duration</div><div className="font-mono font-medium">{durText}</div></div>
            </div>
          </div>
          <div>
            <Label className="text-xs text-muted-foreground">Timestamps</Label>
            <div className="space-y-1 mt-1 text-xs">
              <div className="flex justify-between"><span className="text-muted-foreground">Start</span><span className="font-mono">{log.start_time ? format(new Date(log.start_time), "yyyy-MM-dd HH:mm:ss") : "—"}</span></div>
              <div className="flex justify-between"><span className="text-muted-foreground">End</span><span className="font-mono">{log.end_time ? format(new Date(log.end_time), "yyyy-MM-dd HH:mm:ss") : "—"}</span></div>
            </div>
          </div>
          <div>
            <Label className="text-xs text-muted-foreground">Metadata</Label>
            <div className="text-xs space-y-0.5 mt-1 bg-muted/30 rounded p-2">
              {log.user && <div><span className="text-muted-foreground">User:</span> {log.user}</div>}
              {log.team_id && <div><span className="text-muted-foreground">Team:</span> {log.team_id}</div>}
              {log.organization_id && <div><span className="text-muted-foreground">Org:</span> {log.organization_id}</div>}
              {log.end_user && <div><span className="text-muted-foreground">End User:</span> {log.end_user}</div>}
              {log.session_id && <div><span className="text-muted-foreground">Session:</span> {log.session_id}</div>}
              {log.cache_hit != null && <div><span className="text-muted-foreground">Cache Hit:</span> {String(log.cache_hit)}</div>}
              {log.cache_key && <div><span className="text-muted-foreground">Cache Key:</span> {log.cache_key}</div>}
              {log.mcp_namespaced_tool_name && <div><span className="text-muted-foreground">MCP Tool:</span> {log.mcp_namespaced_tool_name}</div>}
            </div>
          </div>
          <div>
            <Label className="text-xs text-muted-foreground">API Key</Label>
            <div className="flex items-center gap-1 mt-0.5">
              <code className="text-xs font-mono bg-muted rounded px-1.5 py-0.5">{truncate8(log.api_key)}</code>
              <Button variant="ghost" size="icon" className="h-5 w-5" onClick={() => copy(log.api_key)}>{copied ? <Check className="h-3 w-3 text-green-500" /> : <Copy className="h-3 w-3" />}</Button>
            </div>
          </div>
          {log.messages != null && (
            <div>
              <Label className="text-xs text-muted-foreground">Messages (Prompt)</Label>
              <div className="mt-1"><pre className="text-xs bg-muted/30 rounded p-2 max-h-48 overflow-y-auto whitespace-pre-wrap break-words">{typeof log.messages === "string" ? log.messages : JSON.stringify(log.messages, null, 2)}</pre></div>
            </div>
          )}
          {log.response != null && (
            <div>
              <Label className="text-xs text-muted-foreground">Response</Label>
              <div className="mt-1"><pre className="text-xs bg-muted/30 rounded p-2 max-h-48 overflow-y-auto whitespace-pre-wrap break-words">{typeof log.response === "string" ? log.response : JSON.stringify(log.response, null, 2)}</pre></div>
            </div>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Main Page Component
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function loadLiveTailPref(): boolean {
  try {
    return sessionStorage.getItem("spend-logs-live-tail") === "true";
  } catch {
    return false;
  }
}

function saveLiveTailPref(v: boolean) {
  try {
    sessionStorage.setItem("spend-logs-live-tail", String(v));
  } catch { /* noop */ }
}

export function SpendLogsPage() {
  const queryClient = useQueryClient();
  const [preset, setPreset] = useState<TimePreset>("24h");
  const [startDate, setStartDate] = useState(presetRange("24h").start);
  const [endDate, setEndDate] = useState(presetRange("24h").end);
  const [modelFilter, setModelFilter] = useState("");
  const [requestIdFilter, setRequestIdFilter] = useState("");
  const [requestIdInput, setRequestIdInput] = useState("");
  const [liveTail, setLiveTail] = useState(loadLiveTailPref);
  const [page, setPage] = useState(1);
  const { copied, copy } = useCopyToClipboard();
  const [pageSize, setPageSize] = useState(30);
  const [selectedLog, setSelectedLog] = useState<SpendLog | null>(null);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Derive start/end from preset
  const handlePreset = useCallback((p: TimePreset) => {
    setPreset(p);
    if (p !== "custom") {
      const r = presetRange(p);
      setStartDate(r.start);
      setEndDate(r.end);
    }
    setPage(1);
  }, []);

  // Debounced request ID search
  const handleRequestIdInput = useCallback((val: string) => {
    setRequestIdInput(val);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      setRequestIdFilter(val.trim());
      setPage(1);
    }, 300);
  }, []);

  // Cleanup debounce
  useEffect(() => {
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, []);

  // Live Tail: disable on page > 1
  const effectiveLiveTail = liveTail && page === 1;

  // Live Tail pref persistence
  const handleLiveTailToggle = useCallback((v: boolean) => {
    setLiveTail(v);
    saveLiveTailPref(v);
    if (v) setPage(1);
  }, []);

  const {
    data: logsData,
    isLoading,
    isError,
    refetch,
  } = useQuery<SpendLogsResponse>({
    queryKey: [
      "global-spend-logs",
      startDate,
      endDate,
      modelFilter,
      requestIdFilter,
      page,
      pageSize,
    ],
    queryFn: () => {
      let url = `/global/spend/logs?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(endDate)}&page=${page}&page_size=${pageSize}`;
      if (modelFilter.trim()) url += `&model=${encodeURIComponent(modelFilter.trim())}`;
      if (requestIdFilter) url += `&request_id=${encodeURIComponent(requestIdFilter)}`;
      return apiGet(url);
    },
    refetchInterval: effectiveLiveTail ? 15_000 : false,
  });

  const logs = logsData?.data ?? [];
  const totalCount = logsData?.total_count ?? 0;
  const totalPages = logsData?.total_pages ?? 0;

  const handleRowClick = (log: SpendLog) => {
    setSelectedLog(log);
    setDrawerOpen(true);
  };

  const handleFetch = () => {
    queryClient.invalidateQueries({ queryKey: ["global-spend-logs"] });
    refetch();
  };

  return (
    <div className="space-y-4 max-w-full">
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Spend Logs</h1>
          <p className="text-sm text-muted-foreground">
            Detailed request log with cost and token breakdown
          </p>
        </div>
        {/* Live Tail toggle */}
        <div className="flex items-center gap-3">
          {effectiveLiveTail && (
            <div className="flex items-center gap-2 text-xs text-green-600 dark:text-green-400 bg-green-50 dark:bg-green-950 rounded-md px-2 py-1 animate-pulse">
              <span className="relative flex h-2 w-2">
                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"></span>
                <span className="relative inline-flex rounded-full h-2 w-2 bg-green-500"></span>
              </span>
              ● LIVE
              <Button
                variant="ghost"
                size="icon"
                className="h-4 w-4 ml-1"
                onClick={() => handleLiveTailToggle(false)}
              >
                <X className="h-3 w-3" />
              </Button>
            </div>
          )}
          <div className="flex items-center gap-2">
            <Label htmlFor="live-tail" className="text-xs cursor-pointer">Live Tail</Label>
            <Switch
              id="live-tail"
              checked={liveTail}
              onCheckedChange={handleLiveTailToggle}
            />
          </div>
        </div>
      </div>

      {/* Time presets + controls */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium flex items-center gap-2">
            <Calendar className="h-4 w-4" />
            Time Range
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <TimePresetBar
            preset={preset}
            onPreset={handlePreset}
            startDate={startDate}
            endDate={endDate}
            onStartDate={setStartDate}
            onEndDate={setEndDate}
          />
          <div className="flex flex-col sm:flex-row gap-2">
            <div className="flex gap-2">
              <div className="w-44 relative">
                <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
                <Input
                  placeholder="Request ID…"
                  value={requestIdInput}
                  onChange={(e) => handleRequestIdInput(e.target.value)}
                  className="h-8 pl-7 text-xs"
                />
              </div>
              <div className="w-40">
                <Input
                  placeholder="Model filter…"
                  value={modelFilter}
                  onChange={(e) => { setModelFilter(e.target.value); setPage(1); }}
                  className="h-8 text-xs"
                />
              </div>
            </div>
            <Button variant="outline" size="sm" onClick={handleFetch} className="h-8 shrink-0">
              <RefreshCw className="h-3.5 w-3.5 mr-1" />
              Fetch
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => exportToCSV(logs, startDate, endDate)}
              disabled={logs.length === 0}
              className="h-8 shrink-0"
            >
              <Download className="h-3.5 w-3.5 mr-1" />
              CSV
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* Results */}
      <Card>
        <CardHeader className="pb-2">
          <div className="flex flex-row items-center justify-between">
            <CardTitle className="text-sm font-medium flex items-center gap-2">
              <ScrollText className="h-4 w-4" />
              Requests ({totalCount})
            </CardTitle>
          </div>
        </CardHeader>
        <CardContent>
          {/* Pagination top */}
          <div className="mb-3">
            <PaginationBar
              page={page}
              pageSize={pageSize}
              totalCount={totalCount}
              totalPages={totalPages}
              onPage={setPage}
              onPageSize={(s) => { setPageSize(s); setPage(1); }}
            />
          </div>

          {isLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 6 }).map((_, i) => (
                <Skeleton key={i} className="h-10 w-full" />
              ))}
            </div>
          ) : isError ? (
            <div className="flex flex-col items-center justify-center h-32 gap-2">
              <AlertCircle className="h-8 w-8 text-muted-foreground" />
              <p className="text-sm text-muted-foreground">Failed to load spend logs</p>
              <Button variant="outline" size="sm" onClick={handleFetch}>Retry</Button>
            </div>
          ) : logs.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-32 gap-1">
              <p className="text-sm text-muted-foreground">No spend logs found</p>
              <p className="text-xs text-muted-foreground">
                Try adjusting the date range or filters
              </p>
            </div>
          ) : (
            <>
              {/* Desktop table */}
              <div className="hidden md:block overflow-x-auto">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead className="text-xs whitespace-nowrap">
                        <Clock className="h-3 w-3 inline mr-1" />Time
                      </TableHead>
                      <TableHead className="text-xs whitespace-nowrap">Type</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">Model</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">Key</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">End User</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">Status</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">Request ID</TableHead>
                      <TableHead className="text-xs whitespace-nowrap text-right">TTFT</TableHead>
                      <TableHead className="text-xs whitespace-nowrap text-right">Duration</TableHead>
                      <TableHead className="text-xs whitespace-nowrap text-right">Tokens</TableHead>
                      <TableHead className="text-xs whitespace-nowrap text-right">Cost</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {logs.map((log) => (
                      <TableRow
                        key={log.request_id}
                        className="cursor-pointer hover:bg-muted/50"
                        onClick={() => handleRowClick(log)}
                      >
                        <TableCell className="text-xs whitespace-nowrap">
                          {log.start_time
                            ? format(new Date(log.start_time), "MM-dd HH:mm:ss")
                            : "—"}
                        </TableCell>
                        <TableCell>
                          <Badge variant="outline" className="text-[10px] px-1 py-0">
                            {log.call_type || "—"}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-xs whitespace-nowrap font-medium">
                          {log.model}
                        </TableCell>
                        <TableCell className="text-xs whitespace-nowrap text-muted-foreground max-w-[100px] truncate">
                          {log.key_name || log.user || "—"}
                        </TableCell>
                        <TableCell className="text-xs whitespace-nowrap text-muted-foreground max-w-[100px] truncate">
                          {log.end_user || "—"}
                        </TableCell>
                        <TableCell>
                          <Badge
                            variant={log.status === "success" ? "default" : "destructive"}
                            className="text-[10px] px-1.5 py-0"
                          >
                            {log.status || "—"}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-xs font-mono">
                          <div className="flex items-center gap-1">
                            {truncate8(log.request_id)}
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-4 w-4"
                              onClick={(e) => {
                                e.stopPropagation();
                                copy(log.request_id);
                              }}
                            >
                              {copied ? <Check className="h-2.5 w-2.5 text-green-500" /> : <Copy className="h-2.5 w-2.5" />}
                            </Button>
                          </div>
                        </TableCell>
                        <TableCell className="text-xs font-mono text-right">
                          {fmtTtft(log.ttft_ms)}
                        </TableCell>
                        <TableCell className="text-xs font-mono text-right">
                          {fmtDuration(log.request_duration_ms)}
                        </TableCell>
                        <TableCell className="text-xs text-right">
                          <span className="text-muted-foreground">{fmtTokens(log.prompt_tokens)}</span>
                          {" / "}
                          <span>{fmtTokens(log.completion_tokens)}</span>
                        </TableCell>
                        <TableCell className="text-xs font-mono text-right font-medium">
                          {fmtSpend(log.spend)}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>

              {/* Mobile card list */}
              <div className="md:hidden space-y-2">
                {logs.map((log) => (
                  <div
                    key={log.request_id}
                    className="rounded-md border p-3 cursor-pointer hover:bg-muted/50"
                    onClick={() => handleRowClick(log)}
                  >
                    <div className="flex items-center justify-between mb-2">
                      <div className="flex items-center gap-2">
                        <Badge variant="outline" className="text-[10px] px-1 py-0">
                          {log.call_type || "—"}
                        </Badge>
                        <Badge
                          variant={log.status === "success" ? "default" : "destructive"}
                          className="text-[10px] px-1.5 py-0"
                        >
                          {log.status || "—"}
                        </Badge>
                      </div>
                      <span className="text-xs font-mono font-medium">{fmtSpend(log.spend)}</span>
                    </div>
                    <div className="text-sm font-medium truncate mb-1">{log.model}</div>
                    <div className="flex items-center gap-1 text-xs text-muted-foreground mb-1">
                      {log.end_user ? (
                        <span>End User: <span className="font-mono">{log.end_user}</span></span>
                      ) : null}
                    </div>
                    <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-xs text-muted-foreground">
                      <div>
                        <span>TTFT: </span>
                        <span className="font-mono">{fmtTtft(log.ttft_ms)}</span>
                      </div>
                      <div>
                        <span>Dur: </span>
                        <span className="font-mono">{fmtDuration(log.request_duration_ms)}</span>
                      </div>
                      <div>
                        <span>Tokens: </span>
                        <span>{fmtTokens(log.total_tokens)}</span>
                      </div>
                      <div>
                        <span>Time: </span>
                        <span>{log.start_time ? format(new Date(log.start_time), "HH:mm:ss") : "—"}</span>
                      </div>
                    </div>
                    <div className="flex items-center gap-1 mt-1 text-xs text-muted-foreground">
                      <span className="font-mono">{truncate8(log.request_id)}</span>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-4 w-4"
                        onClick={(e) => {
                          e.stopPropagation();
                          copy(log.request_id);
                        }}
                      >
                        {copied ? <Check className="h-2.5 w-2.5 text-green-500" /> : <Copy className="h-2.5 w-2.5" />}
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
            </>
          )}

          {/* Pagination bottom */}
          {logs.length > 0 && (
            <div className="mt-3">
              <PaginationBar
                page={page}
                pageSize={pageSize}
                totalCount={totalCount}
                totalPages={totalPages}
                onPage={setPage}
                onPageSize={(s) => { setPageSize(s); setPage(1); }}
              />
            </div>
          )}
        </CardContent>
      </Card>

      {/* Detail Drawer */}
      <DetailDrawer
        log={selectedLog}
        open={drawerOpen}
        onClose={() => { setDrawerOpen(false); setSelectedLog(null); }}
      />
    </div>
  );
}
