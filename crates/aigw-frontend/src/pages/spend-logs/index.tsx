import { useState, useCallback, useRef, useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import {
  Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle,
} from "@/components/ui/sheet";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { PaginationBar } from "@/components/ui/pagination";
import {
  ScrollText, Calendar, RefreshCw, Search, Copy, Check,
  X, Clock, AlertCircle, Download,
} from "lucide-react";
import { format } from "date-fns";
import { useCopyToClipboard } from "@/hooks/useCopyToClipboard";
import { toast } from "sonner";
import { InputCard } from "@/components/log-viewer/InputCard";
import { OutputCard } from "@/components/log-viewer/OutputCard";
import { parseMessages } from "@/components/log-viewer/MessageViewer";

/* ─────────────────────────────────────────────── Types ── */

interface SpendLog {
  call_id: string; request_id?: string | null; call_type: string; api_key: string; key_name?: string | null;
  spend: number; total_tokens: number; prompt_tokens: number; completion_tokens: number;
  start_time: string; end_time: string; request_duration_ms: number | null; ttft_ms: number | null;
  model: string; model_id?: string | null; model_group?: string | null;
  custom_llm_provider?: string | null; api_base?: string | null;
  user: string | null; team_id?: string | null; organization_id?: string | null;
  end_user?: string | null; session_id?: string | null; requester_ip_address?: string | null;
  request_tags: unknown; metadata?: unknown; cache_hit?: unknown; cache_key?: string | null;
  mcp_namespaced_tool_name?: string | null; status: string | null;
}

interface SpendLogDetail {
  call_id: string; request_id?: string | null; call_type: string; api_key: string; key_name?: string | null;
  spend: number; total_tokens: number; prompt_tokens: number; completion_tokens: number;
  start_time: string; end_time: string; request_duration_ms: number | null; ttft_ms: number | null;
  model: string; model_id?: string | null; model_group?: string | null;
  custom_llm_provider?: string | null; api_base?: string | null;
  user: string | null; team_id?: string | null; organization_id?: string | null;
  end_user?: string | null; session_id?: string | null; requester_ip_address?: string | null;
  request_tags: unknown; metadata?: unknown; cache_hit?: unknown; cache_key?: string | null;
  mcp_namespaced_tool_name?: string | null; status: string | null;
  messages?: unknown; response?: unknown; proxy_server_request?: unknown;
}

interface SpendLogsResponse {
  data: SpendLog[]; count: number; total_count: number; page: number; page_size: number; total_pages: number;
}

type TimePreset = "15m" | "4h" | "24h" | "7d" | "custom";

/* ─────────────────────────────────────────── Helpers ── */

function presetRange(p: TimePreset) {
  const now = Date.now();
  switch (p) {
    case "15m": return { start: new Date(now - 15 * 60 * 1000).toISOString(), end: new Date(now).toISOString() };
    case "4h":  return { start: new Date(now - 4 * 3600 * 1000).toISOString(), end: new Date(now).toISOString() };
    case "24h": return { start: new Date(now - 24 * 3600 * 1000).toISOString(), end: new Date(now).toISOString() };
    case "7d":  return { start: new Date(now - 7 * 24 * 3600 * 1000).toISOString(), end: new Date(now).toISOString() };
    case "custom": return { start: new Date(now - 4 * 3600 * 1000).toISOString(), end: new Date(now).toISOString() };
  }
}

// Convert ISO string → datetime-local input format (YYYY-MM-DDTHH:MM in local time)
function toDatetimeLocalValue(iso: string): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  // datetime-local 输入框期望本地时间（YYYY-MM-DDTHH:mm）
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function fromDatetimeLocalValue(local: string): string {
  if (!local) return "";
  return new Date(local).toISOString();
}

function safeStringify(v: unknown): string {
  if (v === null || v === undefined) return "";
  if (typeof v === "string") return v;
  try { return JSON.stringify(v, null, 2); } catch { return String(v); }
}

function fmtSpend(v: number) { return `$${v.toFixed(4)}`; }
function fmtTokens(v: number) {
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`;
  return v.toString();
}
function fmtTtft(ms: number | null) {
  if (ms === null || ms === undefined) return "—";
  return ms < 1000 ? `${ms.toFixed(0)}ms` : `${(ms / 1000).toFixed(1)}s`;
}
function fmtDuration(ms: number | null) {
  if (ms === null || ms === undefined) return "—";
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
}
function truncate8(s: string) { return s ? (s.length > 8 ? s.slice(0, 8) + "…" : s) : "—"; }
function extractCacheTokens(metadata: unknown): { cache_read_tokens?: number; cache_creation_tokens?: number; cache_read_spend?: number; cache_create_spend?: number } | null {
  if (!metadata || typeof metadata !== "object") return null;
  const m = metadata as Record<string, unknown>;
  if (!m.cache_read_tokens && !m.cache_creation_tokens) return null;
  return {
    cache_read_tokens: typeof m.cache_read_tokens === "number" ? m.cache_read_tokens as number : undefined,
    cache_creation_tokens: typeof m.cache_creation_tokens === "number" ? m.cache_creation_tokens as number : undefined,
    cache_read_spend: typeof m.cache_read_spend === "number" ? m.cache_read_spend as number : undefined,
    cache_create_spend: typeof m.cache_create_spend === "number" ? m.cache_create_spend as number : undefined,
  };
}
function truncateEndUser(s: string): string {
  if (!s) return "—";
  return s.length > 30 ? s.slice(0, 30) + "…" : s;
}

function exportToCSV(logs: SpendLog[], startDate: string, endDate: string) {
  const headers = ["Call ID","Request ID","Time","Type","Model","Status","Prompt Tokens","Completion Tokens","Total Tokens","TTFT (ms)","Duration (ms)","Cost","User","End User","API Key"];
  const rows = logs.map(l => [l.call_id,l.request_id ?? "",l.start_time,l.call_type,l.model,l.status??"",l.prompt_tokens,l.completion_tokens,l.total_tokens,l.ttft_ms??"",l.request_duration_ms??"",l.spend,l.user??"",l.end_user??"",l.api_key.slice(0,12)+"…"]);
  const csv = [headers,...rows].map(r => r.map(v => `"${String(v).replace(/"/g,'""')}"`).join(",")).join("\n");
  const blob = new Blob(["﻿"+csv],{type:"text/csv;charset=utf-8"});
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a"); a.href=url; a.download=`spend-logs-${startDate.slice(0,10)}-${endDate.slice(0,10)}.csv`; a.click();
  URL.revokeObjectURL(url);
}

/* ───────────────────────────────── JSON highlighter ── */

function JsonHighlight({ json }: { json: string }) {
  const html = json
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/("(?:\\.|[^"\\])*")\s*:/g, '<span class="text-blue-600 dark:text-blue-400">$1</span>:')
    .replace(/: (\d+(?:\.\d+)?)(?=[,\s\n\r}\]])/g, ': <span class="text-orange-500">$1</span>')
    .replace(/: (true|false|null)(?=[,\s\n\r}\]])/g, ': <span class="text-purple-500">$1</span>')
    .replace(/: ("(?:\\.|[^"\\])*")/g, ': <span class="text-green-600 dark:text-green-400">$1</span>');
  return <code className="text-[11px] font-mono whitespace-pre-wrap break-all leading-relaxed" dangerouslySetInnerHTML={{ __html: html }} />;
}

/* ────────────────────────────── Provider Logo ── */

const PROVIDER_LOGOS: Record<string, string> = {
  openai: "/assets/logos/openai.svg",
  anthropic: "/assets/logos/anthropic.svg",
  deepseek: "/assets/logos/deepseek.svg",
  vllm: "/assets/logos/vllm.png",
};

function ProviderLogo({ provider }: { provider?: string | null }) {
  const src = provider ? PROVIDER_LOGOS[provider.toLowerCase()] : null;
  if (!src) return null;
  return <img src={`${import.meta.env.BASE_URL}${src.replace(/^\//, "")}`} alt={provider ?? ""} className="h-4 w-4 object-contain shrink-0" />;
}

/* ───────────────────────────────── Row-level copy ── */

function RowCopyButton({ text }: { text: string }) {
  const { copied, copy } = useCopyToClipboard({ onError: () => toast.error("Copy failed — clipboard unavailable") });
  return (
    <Button variant="ghost" size="icon" className="h-4 w-4" onClick={e => { e.stopPropagation(); copy(text); }}>
      {copied ? <Check className="h-2.5 w-2.5 text-green-500" /> : <Copy className="h-2.5 w-2.5" />}
    </Button>
  );
}

function CopyIconButton({ text, className }: { text: string; className?: string }) {
  const { copied, copy } = useCopyToClipboard({ onError: () => toast.error("Copy failed — clipboard unavailable") });
  return (
    <button type="button" tabIndex={-1} className={className ?? "p-0.5 hover:text-foreground text-muted-foreground"} onClick={() => copy(text)} title="Copy">
      {copied ? <Check className="h-3.5 w-3.5 text-green-500" /> : <Copy className="h-3.5 w-3.5" />}
    </button>
  );
}

/* ───────────────── RawJsonBlock — with copy button ── */

function RawJsonBlock({ data }: { data: unknown }) {
  const json = safeStringify(data);
  return (
    <div className="relative group">
      <div className="absolute top-1 right-1 z-10 opacity-0 group-hover:opacity-100 transition-opacity">
        <CopyIconButton text={json} className="p-1 bg-muted rounded hover:bg-muted-foreground/20" />
      </div>
      <pre className="text-[11px] bg-muted/40 border rounded p-2 max-h-96 overflow-y-auto leading-relaxed whitespace-pre-wrap break-all font-mono">
        <JsonHighlight json={json} />
      </pre>
    </div>
  );
}

/* ─────────────────────────────────────────── Tabs ── */

const PRESETS: { key: TimePreset; label: string }[] = [
  { key: "15m", label: "15 min" }, { key: "4h", label: "4 hours" },
  { key: "24h", label: "24 hours" }, { key: "7d", label: "7 days" }, { key: "custom", label: "Custom" },
];

function TimePresetBar({ preset, onPreset, startDate, endDate, onStartDate, onEndDate }: {
  preset: TimePreset; onPreset: (p: TimePreset) => void;
  startDate: string; endDate: string; onStartDate: (v: string) => void; onEndDate: (v: string) => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      {PRESETS.map(p => (
        <Button key={p.key} variant={preset===p.key?"default":"outline"} size="sm" onClick={()=>onPreset(p.key)} className="h-7 text-xs">{p.label}</Button>
      ))}
      {preset === "custom" && (
        <div className="flex items-center gap-2 ml-2">
          <Input type="datetime-local" value={toDatetimeLocalValue(startDate)} onChange={e => onStartDate(fromDatetimeLocalValue(e.target.value))} className="h-7 w-44 text-xs" />
          <span className="text-xs text-muted-foreground">–</span>
          <Input type="datetime-local" value={toDatetimeLocalValue(endDate)} onChange={e => onEndDate(fromDatetimeLocalValue(e.target.value))} className="h-7 w-44 text-xs" />
        </div>
      )}
    </div>
  );
}

/* ───────────────────────────────── Tools Card (top-level) ── */

function ToolsCard({ tools }: { tools: unknown[] }) {
  const [open, setOpen] = useState(false);
  if (!tools || tools.length === 0) return null;

  return (
    <div className="border rounded-lg overflow-hidden">
      <button type="button" tabIndex={-1}
        className="flex items-center gap-2 w-full text-left px-3 py-2 text-xs font-medium bg-blue-50/50 dark:bg-blue-950/20 hover:bg-blue-100/50 dark:hover:bg-blue-950/40 transition-colors"
        onClick={() => setOpen(!open)}
      >
        <span className="text-[10px]">{open ? "▾" : "▸"}</span>
        <span className="text-blue-600 dark:text-blue-400">🛠</span>
        <span>Tools ({tools.length})</span>
      </button>
      {open ? (
        <div className="p-2 space-y-1 border-t max-h-64 overflow-y-auto">
          {tools.map((t, i) => {
            const tool = t as Record<string, unknown>;
            const func = (tool.function ?? {}) as Record<string, unknown>;
            return <ToolItem key={i} name={String(func.name ?? `tool_${i}`)} description={func.description as string | undefined} params={func.parameters} />;
          })}
        </div>
      ) : null}
    </div>
  );
}

function ToolItem({ name, description, params }: { name: string; description?: string; params?: unknown }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="border rounded overflow-hidden text-[11px]">
      <button type="button" tabIndex={-1}
        className="flex items-center gap-1.5 w-full text-left px-2 py-1.5 hover:bg-muted/30 transition-colors"
        onClick={() => setOpen(!open)}
      >
        <span className="text-[10px]">{open ? "▾" : "▸"}</span>
        <code className="font-mono font-medium text-xs">{name}</code>
        {description && !open ? (
          <span className="text-muted-foreground truncate ml-2 hidden sm:inline">{description.slice(0, 60)}</span>
        ) : null}
      </button>
      {open ? (
        <div className="px-3 py-1.5 border-t bg-muted/20 space-y-1.5">
          {description && params ? (
            <Tabs defaultValue="desc">
              <TabsList className="h-6">
                <TabsTrigger value="desc" className="text-[10px] h-5 px-2">Description</TabsTrigger>
                <TabsTrigger value="params" className="text-[10px] h-5 px-2">Params</TabsTrigger>
              </TabsList>
              <TabsContent value="desc" className="mt-1">
                {description ? <p className="text-muted-foreground leading-relaxed text-[11px]">{description}</p> : null}
              </TabsContent>
              <TabsContent value="params" className="mt-1">
                <pre className="text-[10px] whitespace-pre-wrap break-all bg-background rounded p-1.5 max-h-32 overflow-y-auto">
                  {safeStringify(params)}
                </pre>
              </TabsContent>
            </Tabs>
          ) : (
            <>
              {description ? <p className="text-muted-foreground leading-relaxed text-[11px]">{description}</p> : null}
              {params ? (
                <pre className="text-[10px] whitespace-pre-wrap break-all bg-background rounded p-1.5 max-h-32 overflow-y-auto">
                  {safeStringify(params)}
                </pre>
              ) : null}
            </>
          )}
        </div>
      ) : null}
    </div>
  );
}

/* ─────────────────────────────────── Drawer ── */

function DetailDrawer({ log, open, onClose, isDetailLoading, detailError, onRetry }: {
  log: SpendLogDetail | null; open: boolean; onClose: () => void;
  isDetailLoading: boolean; detailError: boolean; onRetry: () => void;
}) {
  if (!log) return null;

  const isFailure = (log.status ?? "").startsWith("failure");
  const hasPrompt = log.messages != null;
  const hasResponse = log.response != null;

  const parsed = parseMessages(log.messages);
  const tools = parsed.tools;

  const side = typeof window !== "undefined" && window.innerWidth < 640 ? "bottom" : "right";

  return (
    <Sheet open={open} onOpenChange={o => !o && onClose()}>
      <SheetContent side={side} className={`overflow-y-auto ${side === "bottom" ? "h-[90dvh] rounded-t-xl max-h-[90dvh]" : "w-[90vw] max-w-3xl sm:max-w-3xl"}`}>
        <SheetHeader>
          <SheetTitle className="text-sm flex items-center gap-2">
            <ProviderLogo provider={log.custom_llm_provider} />
            {log.model}
            {log.custom_llm_provider ? (
              <span className="text-[10px] font-normal text-muted-foreground">{log.custom_llm_provider}</span>
            ) : null}
          </SheetTitle>
          <SheetDescription className="text-[10px] font-mono break-all space-y-1" asChild>
            <div>
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant="default" className="text-[10px]">Call ID</Badge>
              <code>{log.call_id}</code>
              <CopyIconButton text={log.call_id} />
              <Badge variant="secondary" className="text-[10px]">Request ID</Badge>
              {log.request_id ? (
                <><code>{log.request_id}</code><CopyIconButton text={log.request_id} /></>
              ) : (
                <span className="text-muted-foreground italic">—</span>
              )}
            </div>
            </div>
          </SheetDescription>
        </SheetHeader>

        {/* ── Summary pills row ── */}
        <div className="flex flex-wrap items-center gap-2 mt-3 mb-3">
          <Badge variant={isFailure ? "destructive" : "default"} className="text-[10px]">{log.status || "—"}</Badge>
          <Badge variant="outline" className="text-[10px]">{log.call_type || "—"}</Badge>
          <span className="text-xs font-medium">{log.model}</span>
          <span className="text-xs font-mono text-muted-foreground">{fmtSpend(log.spend)}</span>
          <span className="text-[11px] text-muted-foreground">
            {fmtTokens(log.prompt_tokens)}↑ / {fmtTokens(log.completion_tokens)}↓ · {fmtTtft(log.ttft_ms)} / {fmtDuration(log.request_duration_ms)}
          </span>
          <span className="flex items-center gap-1 ml-auto">
            <code className="text-[10px] font-mono bg-muted rounded px-1 py-0.5">{log.key_name || truncate8(log.api_key)}</code>
            <CopyIconButton text={log.key_name || log.api_key} />
          </span>
        </div>

        {/* ── Top info row: model info + timestamps + metadata ── */}
        <div className="text-[11px] text-muted-foreground bg-muted/20 rounded p-2 mb-3 space-y-1">
          <div className="flex flex-wrap gap-x-4 gap-y-0.5">
            <span>Start: <span className="font-mono text-foreground">{log.start_time ? format(new Date(log.start_time), "yyyy-MM-dd HH:mm:ss") : "—"}</span></span>
            <span>End: <span className="font-mono text-foreground">{log.end_time ? format(new Date(log.end_time), "yyyy-MM-dd HH:mm:ss") : "—"}</span></span>
            {log.model_group ? <span>Group: <span className="font-mono text-foreground">{log.model_group}</span></span> : null}
            {log.custom_llm_provider ? <span>Provider: <span className="font-mono text-foreground">{log.custom_llm_provider}</span></span> : null}
            {log.model_id ? <span>ID: <code className="text-[10px] text-foreground">{log.model_id}</code></span> : null}
            {log.api_base ? <span>Base: <code className="text-[10px]">{log.api_base}</code></span> : null}
          </div>
          <div className="flex flex-wrap gap-x-4 gap-y-0.5">
            {log.user ? <span>User: <span className="font-mono text-foreground">{log.user}</span></span> : null}
            {log.end_user ? (
              <span className="flex items-center gap-1">
                <span>End User:</span>
                <code className="text-[10px] bg-muted/40 rounded px-1 py-0.5 font-mono max-w-[200px] truncate">{log.end_user}</code>
                <CopyIconButton text={log.end_user} />
              </span>
            ) : null}
            {log.session_id ? <span>Session: <code className="text-[10px] text-foreground">{log.session_id}</code></span> : null}
            {(() => { const c = extractCacheTokens(log.metadata); return c ? (
              <span className="text-amber-600">Cache: {fmtTokens(c.cache_read_tokens ?? 0)} read / {fmtTokens(c.cache_creation_tokens ?? 0)} write{c.cache_read_spend ? ` ($${c.cache_read_spend.toFixed(4)} + $${(c.cache_create_spend ?? 0).toFixed(4)})` : ""}</span>
            ) : null; })()}
            {log.team_id ? <span>Team: <span className="text-foreground">{log.team_id}</span></span> : null}
            {log.organization_id ? <span>Org: <span className="text-foreground">{log.organization_id}</span></span> : null}
            {log.cache_hit != null ? <span>Cache: <span className="text-foreground">{String(log.cache_hit)}</span></span> : null}
            {log.cache_key ? <span>Cache Key: <code className="text-[10px] text-foreground">{log.cache_key}</code></span> : null}
            {log.mcp_namespaced_tool_name ? <span>MCP Tool: <span className="text-foreground">{log.mcp_namespaced_tool_name}</span></span> : null}
          </div>
        </div>

        {/* ── Body area: loading / error / content ── */}
        <div className="space-y-3">
          {tools && tools.length > 0 ? <ToolsCard tools={tools} /> : null}

          {isDetailLoading ? (
            <div className="space-y-3 py-4">
              <Skeleton className="h-4 w-1/3" />
              <Skeleton className="h-32 w-full rounded-md" />
              <Skeleton className="h-4 w-1/4" />
              <Skeleton className="h-24 w-full rounded-md" />
            </div>
          ) : detailError ? (
            <div className="flex flex-col items-center gap-3 py-8 text-center">
              <AlertCircle className="h-8 w-8 text-red-500" />
              <p className="text-sm text-muted-foreground">Failed to load request detail</p>
              <Button variant="outline" size="sm" onClick={onRetry}>
                <RefreshCw className="h-3.5 w-3.5 mr-1.5" /> Retry
              </Button>
            </div>
          ) : (
            <>
              {hasPrompt ? (
                <div>
                  <Tabs defaultValue="visual">
                    <div className="flex items-center justify-between mb-1.5">
                      <TabsList className="h-7">
                        <TabsTrigger value="visual" className="text-xs h-6">Visual</TabsTrigger>
                        <TabsTrigger value="raw" className="text-xs h-6">Raw</TabsTrigger>
                      </TabsList>
                    </div>
                    <TabsContent value="visual" className="mt-0">
                      <InputCard messages={log.messages} promptTokens={log.prompt_tokens} spend={log.spend} />
                    </TabsContent>
                    <TabsContent value="raw" className="mt-0">
                      <RawJsonBlock data={log.messages} />
                    </TabsContent>
                  </Tabs>
                </div>
              ) : (
                <p className="text-xs text-muted-foreground italic py-2">No prompt data</p>
              )}

              {hasResponse ? (
                <div>
                  <Tabs defaultValue="visual">
                    <div className="flex items-center justify-between mb-1.5">
                      <TabsList className="h-7">
                        <TabsTrigger value="visual" className="text-xs h-6">Visual</TabsTrigger>
                        <TabsTrigger value="raw" className="text-xs h-6">Raw</TabsTrigger>
                      </TabsList>
                    </div>
                    <TabsContent value="visual" className="mt-0">
                      <OutputCard response={log.response} completionTokens={log.completion_tokens} spend={log.spend} />
                    </TabsContent>
                    <TabsContent value="raw" className="mt-0">
                      <RawJsonBlock data={log.response} />
                    </TabsContent>
                  </Tabs>
                </div>
              ) : (
                <p className="text-xs text-muted-foreground italic py-2">No response data</p>
              )}
            </>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}

/* ─────────────────────────────────── Main Page ── */

function loadLiveTailPref() { try { return sessionStorage.getItem("spend-logs-live-tail") === "true"; } catch { return false; } }
function saveLiveTailPref(v: boolean) { try { sessionStorage.setItem("spend-logs-live-tail", String(v)); } catch { /* */ } }

const LIVE_TAIL_INTERVAL = 15_000; // 15 seconds

export function SpendLogsPage() {
  const queryClient = useQueryClient();
  const [preset, setPreset] = useState<TimePreset>("4h");
  const [startDate, setStartDate] = useState(presetRange("4h").start);
  const [endDate, setEndDate] = useState(presetRange("4h").end);
  const [modelFilter, setModelFilter] = useState("");
  const [requestIdFilter, setRequestIdFilter] = useState("");
  const [requestIdInput, setRequestIdInput] = useState("");
  const [statusFilter, setStatusFilter] = useState("all");
  const [minTokens, setMinTokens] = useState<number | undefined>();
  const [maxTokens, setMaxTokens] = useState<number | undefined>();
  const [liveTail, setLiveTail] = useState(loadLiveTailPref);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);
  const [selectedLog, setSelectedLog] = useState<SpendLog | null>(null);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [detailRequestId, setDetailRequestId] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [lastRefreshTime, setLastRefreshTime] = useState<number | null>(null);
  const [countdownSec, setCountdownSec] = useState(LIVE_TAIL_INTERVAL / 1000);

  const handlePreset = useCallback((p: TimePreset) => {
    setPreset(p);
    const r = presetRange(p);
    setStartDate(r.start);
    setEndDate(r.end);
    setPage(1);
  }, []);

  const handleRequestIdInput = useCallback((val: string) => {
    setRequestIdInput(val);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => { setRequestIdFilter(val.trim()); setPage(1); }, 300);
  }, []);

  useEffect(() => { return () => { if (debounceRef.current) clearTimeout(debounceRef.current); }; }, []);

  const effectiveLiveTail = liveTail && page === 1;
  const handleLiveTailToggle = useCallback((v: boolean) => { setLiveTail(v); saveLiveTailPref(v); if (v) setPage(1); }, []);

  // Countdown ticker for Live Tail refresh indicator
  useEffect(() => {
    if (!effectiveLiveTail) {
      setLastRefreshTime(null);
      return;
    }
    const tick = setInterval(() => {
      if (lastRefreshTime !== null) {
        const elapsed = Date.now() - lastRefreshTime;
        const remaining = Math.max(0, LIVE_TAIL_INTERVAL - elapsed);
        setCountdownSec(Math.ceil(remaining / 1000));
      }
    }, 250);
    return () => clearInterval(tick);
  }, [effectiveLiveTail, lastRefreshTime]);

  const { data: logsData, isLoading, isError, refetch, dataUpdatedAt } = useQuery<SpendLogsResponse>({
    queryKey: ["global-spend-logs", startDate, endDate, modelFilter, requestIdFilter, statusFilter, minTokens, maxTokens, page, pageSize, effectiveLiveTail],
    queryFn: () => {
      // When Live Tail is on, slide the end_date to "now" so each poll sees fresh data
      const effectiveEnd = effectiveLiveTail ? new Date().toISOString() : endDate;
      let url = `/global/spend/logs?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(effectiveEnd)}&page=${page}&page_size=${pageSize}`;
      if (modelFilter.trim()) url += `&model=${encodeURIComponent(modelFilter.trim())}`;
      if (requestIdFilter) url += `&request_id=${encodeURIComponent(requestIdFilter)}`;
      if (statusFilter && statusFilter !== "all") url += `&status=${encodeURIComponent(statusFilter)}`;
      if (minTokens !== undefined) url += `&min_tokens=${minTokens}`;
      if (maxTokens !== undefined) url += `&max_tokens=${maxTokens}`;
      return apiGet(url);
    },
    refetchInterval: effectiveLiveTail ? LIVE_TAIL_INTERVAL : false,
  });

  // Track last successful fetch via dataUpdatedAt for countdown display
  useEffect(() => {
    if (dataUpdatedAt > 0) setLastRefreshTime(dataUpdatedAt);
  }, [dataUpdatedAt]);

  const logs = logsData?.data ?? [];
  const totalCount = logsData?.total_count ?? 0;
  const totalPages = logsData?.total_pages ?? 0;

  // Fetch detail for the selected log on-demand
  const { data: detailData, isLoading: isDetailLoading, isError: isDetailError, refetch: refetchDetail } = useQuery<SpendLogDetail>({
    queryKey: ["spend-log-detail", detailRequestId],
    queryFn: () => apiGet(`/global/spend/logs/${encodeURIComponent(detailRequestId!)}`),
    enabled: detailRequestId !== null,
    staleTime: Infinity,
  });

  // Merge detail data into the selected log to enrich it with body blobs
  const enrichedLog = selectedLog && detailData ? { ...selectedLog, messages: detailData.messages, response: detailData.response } : selectedLog;

  return (
    <div className="space-y-4 max-w-full">
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Spend Logs</h1>
          <p className="text-sm text-muted-foreground">Detailed request log with cost and token breakdown</p>
        </div>
        <div className="flex items-center gap-3">
          {effectiveLiveTail ? (
            <div className="flex items-center gap-2 text-xs text-green-600 dark:text-green-400 bg-green-50 dark:bg-green-950 rounded-md pl-2 pr-1 py-1">
              <span className="relative flex h-2 w-2">
                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"/>
                <span className="relative inline-flex rounded-full h-2 w-2 bg-green-500"/>
              </span>
              <span className="font-medium">LIVE</span>
              <span className="text-muted-foreground tabular-nums">
                · {lastRefreshTime ? `${countdownSec}s` : "…"}
              </span>
              <Button variant="ghost" size="icon" className="h-4 w-4 ml-0.5" onClick={() => handleLiveTailToggle(false)}><X className="h-3 w-3"/></Button>
            </div>
          ) : null}
          <div className="flex items-center gap-2">
            <Label htmlFor="live-tail" className="text-xs cursor-pointer">Live Tail</Label>
            <Switch id="live-tail" checked={liveTail} onCheckedChange={handleLiveTailToggle}/>
          </div>
        </div>
      </div>

      <Card>
        <CardHeader className="pb-2"><CardTitle className="text-sm font-medium flex items-center gap-2"><Calendar className="h-4 w-4"/>Spend Logs ({totalCount})</CardTitle></CardHeader>
        <CardContent className="space-y-2">
          {/* Time presets + all filters + actions in one compact row */}
          <div className="flex flex-wrap items-center gap-2">
            {PRESETS.map(p => (
              <Button key={p.key} variant={preset===p.key?"default":"outline"} size="sm" onClick={()=>handlePreset(p.key)} className="h-7 text-xs">{p.label}</Button>
            ))}
            {preset === "custom" && (
              <>
                <Input type="datetime-local" value={toDatetimeLocalValue(startDate)} onChange={e => setStartDate(fromDatetimeLocalValue(e.target.value))} className="h-7 w-36 text-xs" />
                <span className="text-xs text-muted-foreground">–</span>
                <Input type="datetime-local" value={toDatetimeLocalValue(endDate)} onChange={e => setEndDate(fromDatetimeLocalValue(e.target.value))} className="h-7 w-36 text-xs" />
              </>
            )}
            <div className="h-5 w-px bg-border mx-1" />
            <div className="w-36 relative"><Search className="absolute left-2 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground"/><Input placeholder="Call / Request ID…" value={requestIdInput} onChange={e => handleRequestIdInput(e.target.value)} className="h-7 pl-7 text-xs"/></div>
            <div className="w-32"><Input placeholder="Model…" value={modelFilter} onChange={e => { setModelFilter(e.target.value); setPage(1); }} className="h-7 text-xs"/></div>
            <Select value={statusFilter} onValueChange={(v) => { setStatusFilter(v); setPage(1); }}>
              <SelectTrigger className="h-7 w-[100px] text-xs"><SelectValue placeholder="Status"/></SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All</SelectItem>
                <SelectItem value="success">Success</SelectItem>
                <SelectItem value="failure">Failure</SelectItem>
                <SelectItem value="streaming">Streaming</SelectItem>
              </SelectContent>
            </Select>
            <Input type="number" placeholder="Min tok" value={minTokens !== undefined ? String(minTokens) : ""} onChange={e => { const v = e.target.value; setMinTokens(v ? Number(v) : undefined); setPage(1); }} className="h-7 w-[70px] text-xs"/>
            <Input type="number" placeholder="Max tok" value={maxTokens !== undefined ? String(maxTokens) : ""} onChange={e => { const v = e.target.value; setMaxTokens(v ? Number(v) : undefined); setPage(1); }} className="h-7 w-[70px] text-xs"/>
            <Button variant="outline" size="sm" onClick={() => { queryClient.invalidateQueries({ queryKey: ["global-spend-logs"] }); refetch(); }} className="h-7 shrink-0 text-xs"><RefreshCw className="h-3 w-3 mr-1"/>Fetch</Button>
            <Button variant="outline" size="sm" onClick={() => exportToCSV(logs, startDate, endDate)} disabled={logs.length===0} className="h-7 shrink-0 text-xs"><Download className="h-3 w-3 mr-1"/>CSV</Button>
          </div>
          <PaginationBar page={page} pageSize={pageSize} totalCount={totalCount} totalPages={totalPages} onPage={setPage} onPageSize={s => { setPageSize(s); setPage(1); }}/>
          <div className="hidden md:block overflow-x-auto">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead className="text-xs whitespace-nowrap">Call ID</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">Request ID</TableHead>
                      <TableHead className="text-xs whitespace-nowrap"><Clock className="h-3 w-3 inline mr-1"/>Time</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">Type</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">Model</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">Key</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">End User</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">IP</TableHead>
                      <TableHead className="text-xs whitespace-nowrap">Status</TableHead>
                      <TableHead className="text-xs whitespace-nowrap text-right">TTFT</TableHead>
                      <TableHead className="text-xs whitespace-nowrap text-right">Duration</TableHead>
                      <TableHead className="text-xs whitespace-nowrap text-right">Tokens</TableHead>
                      <TableHead className="text-xs whitespace-nowrap text-right">Cost</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {logs.map(log => (
                      <TableRow key={log.call_id} data-testid="spend-log-row" className="cursor-pointer hover:bg-muted/50" onClick={() => { setSelectedLog(log); setDrawerOpen(true); setDetailRequestId(log.call_id); }}>
                        <TableCell className="text-xs font-mono"><div className="flex items-center gap-1">{truncate8(log.call_id)}<RowCopyButton text={log.call_id}/></div></TableCell>
                        <TableCell className="text-xs font-mono text-muted-foreground">{log.request_id ? <><span>{truncate8(log.request_id)}</span><RowCopyButton text={log.request_id} /></> : "—"}</TableCell>
                        <TableCell className="text-xs whitespace-nowrap">{log.start_time ? format(new Date(log.start_time), "MM-dd HH:mm:ss") : "—"}</TableCell>
                        <TableCell><Badge variant="outline" className="text-[10px] px-1 py-0">{log.call_type || "—"}</Badge></TableCell>
                        <TableCell className="text-xs whitespace-nowrap">
                          <div className="flex items-center gap-1">
                            {log.model_group ? <Badge variant="secondary" className="text-[10px] px-1 py-0 font-normal">{log.model_group}</Badge> : null}
                            <span className="font-medium">{log.model}</span>
                          </div>
                        </TableCell>
                        <TableCell className="text-xs whitespace-nowrap text-muted-foreground max-w-[100px] truncate">{log.key_name || log.user || "—"}</TableCell>
                        <TableCell className="text-xs whitespace-nowrap text-muted-foreground max-w-[120px]">
                          <code className="text-[10px] truncate block">{truncateEndUser(log.end_user || "")}</code>
                        </TableCell>
                        <TableCell className="text-xs font-mono whitespace-nowrap">
                          <span className="inline-flex items-center gap-1">
                            <span>{log.requester_ip_address ?? "—"}</span>
                            {log.requester_ip_address && <RowCopyButton text={log.requester_ip_address} />}
                          </span>
                        </TableCell>
                        <TableCell><Badge variant={log.status==="success"?"default":"destructive"} className="text-[10px] px-1.5 py-0">{log.status || "—"}</Badge></TableCell>
                        <TableCell className="text-xs font-mono text-right">{fmtTtft(log.ttft_ms)}</TableCell>
                        <TableCell className="text-xs font-mono text-right">{fmtDuration(log.request_duration_ms)}</TableCell>
                        <TableCell className="text-xs text-right"><span className="text-muted-foreground">{fmtTokens(log.prompt_tokens)}</span>{" / "}<span>{fmtTokens(log.completion_tokens)}</span>
                          {(() => { const c = extractCacheTokens(log.metadata); return c ? (
                            <span className="text-[10px] block text-muted-foreground/70">cache: {fmtTokens(c.cache_read_tokens ?? 0)}R / {fmtTokens(c.cache_creation_tokens ?? 0)}W</span>
                          ) : null; })()}
                        </TableCell>
                        <TableCell className="text-xs font-mono text-right font-medium">{fmtSpend(log.spend)}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
              <div className="md:hidden space-y-2">
                {logs.map(log => (
                  <div key={log.call_id} data-testid="spend-log-row" className="rounded-md border p-3 cursor-pointer hover:bg-muted/50" onClick={() => { setSelectedLog(log); setDrawerOpen(true); setDetailRequestId(log.call_id); }}>
                    <div className="flex items-center justify-between mb-2">
                      <div className="flex items-center gap-2">
                        <Badge variant="outline" className="text-[10px] px-1 py-0">{log.call_type||"—"}</Badge>
                        <Badge variant={log.status==="success"?"default":"destructive"} className="text-[10px] px-1.5 py-0">{log.status||"—"}</Badge>
                      </div>
                      <span className="text-xs font-mono font-medium">{fmtSpend(log.spend)}</span>
                    </div>
                    <div className="text-sm font-medium mb-1 flex items-center gap-1">
                      {log.model_group ? <Badge variant="secondary" className="text-[10px] px-1 py-0 font-normal">{log.model_group}</Badge> : null}
                      <span className="truncate">{log.model}</span>
                    </div>
                    {log.end_user ? <div className="text-xs text-muted-foreground mb-1">End User: <code className="text-[10px]">{truncateEndUser(log.end_user)}</code></div> : null}
                    <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-xs text-muted-foreground">
                      <div>TTFT: <span className="font-mono">{fmtTtft(log.ttft_ms)}</span></div>
                      <div>Dur: <span className="font-mono">{fmtDuration(log.request_duration_ms)}</span></div>
                      <div>Tokens: <span>{fmtTokens(log.total_tokens)}</span></div>
                      <div>Time: <span>{log.start_time ? format(new Date(log.start_time), "HH:mm:ss") : "—"}</span></div>
                    </div>
                    <div className="flex items-center gap-1 mt-1 text-xs text-muted-foreground">
                      <span className="font-mono">{truncate8(log.call_id)}</span>
                      <RowCopyButton text={log.call_id}/>
                    </div>
                  </div>
                ))}
              </div>
          {logs.length > 0 ? <div className="mt-3"><PaginationBar page={page} pageSize={pageSize} totalCount={totalCount} totalPages={totalPages} onPage={setPage} onPageSize={s => { setPageSize(s); setPage(1); }}/></div> : null}
        </CardContent>
      </Card>

      <DetailDrawer
        log={enrichedLog}
        open={drawerOpen}
        onClose={() => { setDrawerOpen(false); setSelectedLog(null); setDetailRequestId(null); }}
        isDetailLoading={isDetailLoading}
        detailError={isDetailError}
        onRetry={() => refetchDetail()}
      />
    </div>
  );
}
