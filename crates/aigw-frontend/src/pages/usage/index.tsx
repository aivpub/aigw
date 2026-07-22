import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
  PieChart,
  Pie,
  Cell,
} from "recharts";
import {
  DollarSign,
  TrendingUp,
  BarChart3,
  PieChart as PieChartIcon,
  Calendar,
  CheckCircle,
  XCircle,
  Sparkles,
  ListOrdered,
} from "lucide-react";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface ActivityMetadata {
  total_spend: number;
  total_requests: number;
  successful_requests: number;
  failed_requests: number;
  total_tokens: number;
  prompt_tokens: number;
  completion_tokens: number;
}

interface DailyRow {
  date: string;
  spend: number;
  tokens: number;
  requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  successful_requests: number;
  failed_requests: number;
}

interface ActivityResponse {
  metadata: ActivityMetadata;
  daily: DailyRow[];
}

interface ModelAgg {
  model: string;
  total_tokens: number;
  total_spend: number;
  requests: number;
}

interface ProviderAgg {
  provider: string;
  total_tokens: number;
  total_spend: number;
  requests: number;
}

interface AggResponse {
  data: (ModelAgg | ProviderAgg)[];
  count: number;
}

interface KeyRanking {
  api_key: string;
  key_alias?: string | null;
  total_spend: number;
  total_requests: number;
  total_tokens: number;
}

type KeyRankingResponse = KeyRanking[];

interface ProviderChartData {
  name: string;
  value: number;
  tokens: number;
  requests: number;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function fmtSpend(v: number): string {
  return `$${v.toFixed(4)}`;
}

function fmtTokens(v: number): string {
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`;
  return v.toString();
}

type DatePreset = "3d" | "7d" | "30d" | "custom";

function presetRange(p: DatePreset): { start: string; end: string } {
  const now = Date.now();
  const end = new Date(now).toISOString().split('T')[0];
  switch (p) {
    case "3d":
      return { start: new Date(now - 3 * 86400000).toISOString().split('T')[0], end };
    case "7d":
      return { start: new Date(now - 7 * 86400000).toISOString().split('T')[0], end };
    case "30d":
    default:
      return { start: new Date(now - 30 * 86400000).toISOString().split('T')[0], end };
  }
}

const COLORS = [
  "#3b82f6", "#22c55e", "#f59e0b", "#8b5cf6", "#ec4899",
  "#14b8a6", "#f97316", "#06b6d4", "#84cc16", "#6366f1",
];

const PRESETS: { key: DatePreset; label: string }[] = [
  { key: "3d", label: "3 days" },
  { key: "7d", label: "7 days" },
  { key: "30d", label: "30 days" },
  { key: "custom", label: "Custom" },
];

type ChartMode = "spend" | "tokens" | "requests";
type ModelViewMode = "chart" | "ranking";

function truncateApiKey(apiKey: string): string {
  if (apiKey.length <= 12) return apiKey;
  return `${apiKey.slice(0, 6)}...${apiKey.slice(-4)}`;
}

function yAxisTick(v: number): string {
  if (v >= 1_000_000_000) return `${(v / 1_000_000_000).toFixed(1)}B`;
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`;
  if (Number.isInteger(v)) return v.toString();
  return v.toFixed(2);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Component
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function UsagePage() {
  const [preset, setPreset] = useState<DatePreset>("3d");
  const [startDate, setStartDate] = useState(presetRange("3d").start);
  const [endDate, setEndDate] = useState(presetRange("3d").end);
  const [dailyChartMode, setDailyChartMode] = useState<ChartMode>("spend");
  const [modelChartMode, setModelChartMode] = useState<ChartMode>("spend");
  const [providerChartMode, setProviderChartMode] = useState<ChartMode>("spend");
  const [rankingChartMode, setRankingChartMode] = useState<ChartMode>("spend");
  const [modelViewMode, setModelViewMode] = useState<ModelViewMode>("chart");
  const [providerViewMode, setProviderViewMode] = useState<ModelViewMode>("chart");

  const handlePreset = (p: DatePreset) => {
    setPreset(p);
    if (p !== "custom") {
      const r = presetRange(p);
      setStartDate(r.start);
      setEndDate(r.end);
    }
  };

  // Activity overview (metadata + daily)
  const { data: activity, isLoading: activityLoading } = useQuery<ActivityResponse>({
    queryKey: ["global-spend-activity", startDate, endDate],
    queryFn: () =>
      apiGet(`/global/spend/activity?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(endDate)}`),
    refetchInterval: 30_000,
  });

  const metadata = activity?.metadata;

  // Model aggregation
  const { data: modelData, isLoading: modelLoading } = useQuery<AggResponse>({
    queryKey: ["spend-models", startDate, endDate],
    queryFn: () => apiGet(`/global/spend/models?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(endDate)}`),
    refetchInterval: 30_000,
  });

  // Provider aggregation
  const { data: providerData, isLoading: providerLoading } = useQuery<AggResponse>({
    queryKey: ["spend-providers", startDate, endDate],
    queryFn: () => apiGet(`/spend/providers?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(endDate)}`),
    refetchInterval: 30_000,
  });

  // Key rankings
  const { data: keyRankings, isLoading: keyRankingsLoading } = useQuery<KeyRankingResponse>({
    queryKey: ["key-rankings", startDate, endDate],
    queryFn: () =>
      apiGet(`/global/spend/keys/rankings?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(endDate)}&limit=5`),
    refetchInterval: 30_000,
  });

  const modelChartData = (modelData?.data ?? []) as ModelAgg[];
  const providerChartData: ProviderChartData[] = ((providerData?.data ?? []) as ProviderAgg[]).map((a) => ({
    name: a.provider,
    value: Math.round(a.total_spend * 10000) / 10000,
    tokens: a.total_tokens,
    requests: a.requests,
  }));

  const dailyChartData = (activity?.daily ?? []).map((d) => ({
    date: d.date,
    spend: Math.round(d.spend * 10000) / 10000,
    tokens: d.tokens,
    requests: d.requests,
    prompt_tokens: d.prompt_tokens,
    completion_tokens: d.completion_tokens,
    successful_requests: d.successful_requests,
    failed_requests: d.failed_requests,
  }));

  const isLoading = activityLoading || modelLoading || providerLoading;

  // Compute max values for ranking progress bars
  const rankings = keyRankings ?? [];
  const rankingMaxSpend = rankings.length > 0 ? Math.max(...rankings.map(r => r.total_spend)) : 1;
  const rankingMaxTokens = rankings.length > 0 ? Math.max(...rankings.map(r => r.total_tokens)) : 1;
  const rankingMaxRequests = rankings.length > 0 ? Math.max(...rankings.map(r => r.total_requests)) : 1;

  const modelRankingMaxSpend = modelChartData.length > 0 ? Math.max(...modelChartData.map(m => m.total_spend)) : 1;
  const modelRankingMaxTokens = modelChartData.length > 0 ? Math.max(...modelChartData.map(m => m.total_tokens)) : 1;
  const modelRankingMaxRequests = modelChartData.length > 0 ? Math.max(...modelChartData.map(m => m.requests)) : 1;

  return (
    <div className="space-y-4">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Usage</h1>
          <p className="text-sm text-muted-foreground">Usage and spend overview</p>
        </div>
        {/* Toolbar: date presets + custom picker */}
        <div className="flex flex-wrap items-center gap-2">
          {PRESETS.map((p) => (
            <Button
              key={p.key}
              variant={preset === p.key ? "default" : "outline"}
              size="sm"
              onClick={() => handlePreset(p.key)}
              className="h-7 text-xs"
            >
              {p.label}
            </Button>
          ))}
          {preset === "custom" && (
            <div className="flex items-center gap-1">
              <Input
                type="date"
                value={startDate}
                onChange={(e) => setStartDate(e.target.value)}
                className="h-7 w-32 text-xs"
              />
              <span className="text-xs text-muted-foreground">–</span>
              <Input
                type="date"
                value={endDate}
                onChange={(e) => setEndDate(e.target.value)}
                className="h-7 w-32 text-xs"
              />
            </div>
          )}
        </div>
      </div>

      {/* Metric Cards — compact */}
      <div className="grid gap-3 grid-cols-3 md:grid-cols-6">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-1 p-4">
            <CardTitle className="text-xs font-medium">Spend</CardTitle>
            <DollarSign className="h-3.5 w-3.5 text-muted-foreground" />
          </CardHeader>
          <CardContent className="p-4 pt-0">
            {activityLoading ? (
              <Skeleton className="h-6 w-16" />
            ) : (
              <div className="text-lg font-bold">{fmtSpend(metadata?.total_spend ?? 0)}</div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-1 p-4">
            <CardTitle className="text-xs font-medium">Requests</CardTitle>
            <BarChart3 className="h-3.5 w-3.5 text-muted-foreground" />
          </CardHeader>
          <CardContent className="p-4 pt-0">
            {activityLoading ? (
              <Skeleton className="h-6 w-12" />
            ) : (
              <div className="text-lg font-bold">{metadata?.total_requests?.toLocaleString() ?? "—"}</div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-1 p-4">
            <CardTitle className="text-xs font-medium">OK</CardTitle>
            <CheckCircle className="h-3.5 w-3.5 text-green-500" />
          </CardHeader>
          <CardContent className="p-4 pt-0">
            {activityLoading ? (
              <Skeleton className="h-6 w-12" />
            ) : (
              <div className="text-lg font-bold text-green-600 dark:text-green-400">
                {metadata?.successful_requests?.toLocaleString() ?? "—"}
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-1 p-4">
            <CardTitle className="text-xs font-medium">Failed</CardTitle>
            <XCircle className="h-3.5 w-3.5 text-red-500" />
          </CardHeader>
          <CardContent className="p-4 pt-0">
            {activityLoading ? (
              <Skeleton className="h-6 w-12" />
            ) : (
              <div className="text-lg font-bold text-red-600 dark:text-red-400">
                {metadata?.failed_requests?.toLocaleString() ?? "—"}
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-1 p-4">
            <CardTitle className="text-xs font-medium">Tokens</CardTitle>
            <Sparkles className="h-3.5 w-3.5 text-muted-foreground" />
          </CardHeader>
          <CardContent className="p-4 pt-0">
            {activityLoading ? (
              <Skeleton className="h-6 w-16" />
            ) : (
              <div className="text-lg font-bold">{fmtTokens(metadata?.total_tokens ?? 0)}</div>
            )}
            <p className="text-[10px] text-muted-foreground mt-0.5">
              p {fmtTokens(metadata?.prompt_tokens ?? 0)} / c {fmtTokens(metadata?.completion_tokens ?? 0)}
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-1 p-4">
            <CardTitle className="text-xs font-medium">Rate</CardTitle>
            <TrendingUp className="h-3.5 w-3.5 text-muted-foreground" />
          </CardHeader>
          <CardContent className="p-4 pt-0">
            {activityLoading ? (
              <Skeleton className="h-6 w-12" />
            ) : (
              <div className="text-lg font-bold">
                {metadata && metadata.total_requests > 0
                  ? `${((metadata.successful_requests / metadata.total_requests) * 100).toFixed(1)}%`
                  : "—"}
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Daily Trend — independent tab state */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between pb-2 pt-4 px-4">
          <CardTitle className="text-sm font-medium">Daily Trend</CardTitle>
          <Tabs defaultValue="spend" value={dailyChartMode} onValueChange={(v) => setDailyChartMode(v as ChartMode)}>
            <TabsList className="h-7">
              <TabsTrigger value="spend" className="text-xs px-3 h-5">💰 Spend</TabsTrigger>
              <TabsTrigger value="tokens" className="text-xs px-3 h-5">📊 Tokens</TabsTrigger>
              <TabsTrigger value="requests" className="text-xs px-3 h-5">📋 Requests</TabsTrigger>
            </TabsList>
          </Tabs>
        </CardHeader>
        <CardContent>
          {activityLoading ? (
            <Skeleton className="h-64 w-full" />
          ) : dailyChartData.length === 0 ? (
            <div className="flex items-center justify-center h-64 text-sm text-muted-foreground">
              No data available
            </div>
          ) : (
            <div className="h-[220px] md:h-[260px]">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={dailyChartData} margin={{ top: 5, right: 20, left: 10, bottom: 5 }}>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-muted" />
                  <XAxis dataKey="date" tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" />
                  <YAxis tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" tickFormatter={yAxisTick} />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: "hsl(var(--card))",
                      border: "1px solid hsl(var(--border))",
                      borderRadius: "6px",
                      fontSize: "12px",
                    }}
                    formatter={(value, name) => {
                      if (dailyChartMode === "tokens") return [fmtTokens(value as number), name];
                      if (dailyChartMode === "requests") return [value, name];
                      return [fmtSpend(value as number), name];
                    }}
                    labelFormatter={(label) => {
                      const item = dailyChartData.find((d) => d.date === label);
                      if (!item) return label;
                      if (dailyChartMode === "tokens") {
                        return `${label}\n  Prompt: ${fmtTokens(item.prompt_tokens)}  |  Completion: ${fmtTokens(item.completion_tokens)}\n  Total: ${fmtTokens(item.tokens)}`;
                      }
                      if (dailyChartMode === "requests") {
                        return `${label}\n  Success: ${item.successful_requests}  |  Failed: ${item.failed_requests}\n  Total: ${item.requests}`;
                      }
                      return `${label}  |  ${fmtSpend(item.spend)}`;
                    }}
                  />
                  {dailyChartMode === "spend" && (
                    <Bar dataKey="spend" name="Spend" fill="hsl(var(--primary))" radius={[4, 4, 0, 0]} />
                  )}
                  {dailyChartMode === "tokens" && (
                    <>
                      <Bar dataKey="prompt_tokens" name="Prompt" fill="#94a3b8" stackId="tokens" radius={[0, 0, 0, 0]} />
                      <Bar dataKey="completion_tokens" name="Completion" fill="#3b82f6" stackId="tokens" radius={[4, 4, 0, 0]} />
                    </>
                  )}
                  {dailyChartMode === "requests" && (
                    <>
                      <Bar dataKey="successful_requests" name="Success" fill="#22c55e" stackId="requests" radius={[0, 0, 0, 0]} />
                      <Bar dataKey="failed_requests" name="Failed" fill="#ef4444" stackId="requests" radius={[4, 4, 0, 0]} />
                    </>
                  )}
                  <Legend />
                </BarChart>
              </ResponsiveContainer>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Top Virtual Keys Ranking */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between pb-2 pt-4 px-4">
          <CardTitle className="text-sm font-medium">Top Virtual Keys</CardTitle>
          <Tabs defaultValue="spend" value={rankingChartMode} onValueChange={(v) => setRankingChartMode(v as ChartMode)}>
            <TabsList className="h-7">
              <TabsTrigger value="spend" className="text-xs px-3 h-5">💰 Spend</TabsTrigger>
              <TabsTrigger value="tokens" className="text-xs px-3 h-5">📊 Tokens</TabsTrigger>
              <TabsTrigger value="requests" className="text-xs px-3 h-5">📋 Requests</TabsTrigger>
            </TabsList>
          </Tabs>
        </CardHeader>
        <CardContent>
          {keyRankingsLoading ? (
            <div className="space-y-3">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-6 w-full" />
              ))}
            </div>
          ) : rankings.length === 0 ? (
            <div className="flex items-center justify-center py-8 text-sm text-muted-foreground">
              No data available
            </div>
          ) : (
            <div className="space-y-2">
              {rankings.slice(0, 5).map((r, i) => {
                const metricValue =
                  rankingChartMode === "tokens" ? r.total_tokens :
                  rankingChartMode === "requests" ? r.total_requests :
                  r.total_spend;
                const maxValue =
                  rankingChartMode === "tokens" ? rankingMaxTokens :
                  rankingChartMode === "requests" ? rankingMaxRequests :
                  rankingMaxSpend;
                const pct = maxValue > 0 ? (metricValue / maxValue) * 100 : 0;

                return (
                  <div key={r.api_key} className="flex items-center gap-3">
                    <span className="text-xs font-mono w-5 text-muted-foreground">#{i + 1}</span>
                    <span className="text-sm font-mono truncate w-[140px]">
                      {r.key_alias ?? truncateApiKey(r.api_key)}
                    </span>
                    <div className="flex-1 h-3 bg-muted rounded overflow-hidden">
                      <div
                        className="h-full rounded transition-all"
                        style={{
                          width: `${pct}%`,
                          backgroundColor: COLORS[i % COLORS.length],
                        }}
                      />
                    </div>
                    <span className="text-xs font-mono w-[80px] text-right">
                      {rankingChartMode === "tokens"
                        ? fmtTokens(metricValue)
                        : rankingChartMode === "requests"
                          ? metricValue.toLocaleString()
                          : fmtSpend(metricValue)}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Model / Provider section — 2 columns, independent tab states */}
      <div className="grid gap-4 lg:grid-cols-2">
        {/* Model card with Chart/Rank toggle + independent metric tabs */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2 pt-4 px-4">
            <CardTitle className="text-sm font-medium">Spend by Model</CardTitle>
            <div className="flex items-center gap-2">
              <Tabs defaultValue="chart" value={modelViewMode} onValueChange={(v) => setModelViewMode(v as ModelViewMode)}>
                <TabsList className="h-7">
                  <TabsTrigger value="chart" className="text-xs px-3 h-5">📊 Chart</TabsTrigger>
                  <TabsTrigger value="ranking" className="text-xs px-3 h-5"><ListOrdered className="h-3 w-3" /></TabsTrigger>
                </TabsList>
              </Tabs>
              {modelViewMode === "chart" && (
                <Tabs defaultValue="spend" value={modelChartMode} onValueChange={(v) => setModelChartMode(v as ChartMode)}>
                  <TabsList className="h-7">
                    <TabsTrigger value="spend" className="text-xs px-3 h-5">💰</TabsTrigger>
                    <TabsTrigger value="tokens" className="text-xs px-3 h-5">📊</TabsTrigger>
                    <TabsTrigger value="requests" className="text-xs px-3 h-5">📋</TabsTrigger>
                  </TabsList>
                </Tabs>
              )}
              {modelViewMode === "ranking" && (
                <Tabs defaultValue="spend" value={modelChartMode} onValueChange={(v) => setModelChartMode(v as ChartMode)}>
                  <TabsList className="h-7">
                    <TabsTrigger value="spend" className="text-xs px-3 h-5">💰</TabsTrigger>
                    <TabsTrigger value="tokens" className="text-xs px-3 h-5">📊</TabsTrigger>
                    <TabsTrigger value="requests" className="text-xs px-3 h-5">📋</TabsTrigger>
                  </TabsList>
                </Tabs>
              )}
            </div>
          </CardHeader>
          <CardContent>
            {modelLoading ? (
              <Skeleton className="h-64 w-full" />
            ) : modelChartData.length === 0 ? (
              <div className="flex items-center justify-center h-64 text-sm text-muted-foreground">
                No data available
              </div>
            ) : modelViewMode === "chart" ? (
              <div className="h-[200px] md:h-[260px]">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={[...modelChartData]
                    .sort((a, b) => {
                      if (modelChartMode === "tokens") return b.total_tokens - a.total_tokens;
                      if (modelChartMode === "requests") return b.requests - a.requests;
                      return b.total_spend - a.total_spend;
                    })
                    .slice(0, 5)} margin={{ top: 5, right: 20, left: 10, bottom: 5 }}>
                    <CartesianGrid strokeDasharray="3 3" className="stroke-muted" />
                    <XAxis dataKey="model" tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" />
                    <YAxis tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" tickFormatter={yAxisTick} />
                    <Tooltip
                      contentStyle={{
                        backgroundColor: "hsl(var(--card))",
                        border: "1px solid hsl(var(--border))",
                        borderRadius: "6px",
                        fontSize: "12px",
                      }}
                      formatter={(value) => {
                        if (modelChartMode === "tokens") return [fmtTokens(value as number), "Tokens"];
                        if (modelChartMode === "requests") return [value, "Requests"];
                        return [fmtSpend(value as number), "Spend"];
                      }}
                    />
                    {modelChartMode === "spend" && <Bar dataKey="total_spend" name="Spend" fill="hsl(var(--primary))" radius={[4, 4, 0, 0]} />}
                    {modelChartMode === "tokens" && <Bar dataKey="total_tokens" name="Tokens" fill="#f59e0b" radius={[4, 4, 0, 0]} />}
                    {modelChartMode === "requests" && <Bar dataKey="requests" name="Requests" fill="#22c55e" radius={[4, 4, 0, 0]} />}
                  </BarChart>
                </ResponsiveContainer>
              </div>
            ) : (
              <div className="space-y-2">
                {[...modelChartData]
                  .sort((a, b) => {
                    if (modelChartMode === "tokens") return b.total_tokens - a.total_tokens;
                    if (modelChartMode === "requests") return b.requests - a.requests;
                    return b.total_spend - a.total_spend;
                  })
                  .slice(0, 5)
                  .map((m, i) => {
                    const metricValue =
                      modelChartMode === "tokens" ? m.total_tokens :
                      modelChartMode === "requests" ? m.requests :
                      m.total_spend;
                    const maxValue =
                      modelChartMode === "tokens" ? modelRankingMaxTokens :
                      modelChartMode === "requests" ? modelRankingMaxRequests :
                      modelRankingMaxSpend;
                    const pct = maxValue > 0 ? (metricValue / maxValue) * 100 : 0;

                    return (
                      <div key={m.model} className="flex items-center gap-3">
                        <span className="text-xs font-mono w-5 text-muted-foreground">#{i + 1}</span>
                        <span className="text-sm truncate w-[120px]">{m.model}</span>
                        <div className="flex-1 h-3 bg-muted rounded overflow-hidden">
                          <div
                            className="h-full rounded transition-all"
                            style={{
                              width: `${pct}%`,
                              backgroundColor: COLORS[i % COLORS.length],
                            }}
                          />
                        </div>
                        <span className="text-xs font-mono w-[80px] text-right">
                          {modelChartMode === "tokens"
                            ? fmtTokens(metricValue)
                            : modelChartMode === "requests"
                              ? metricValue.toLocaleString()
                              : fmtSpend(metricValue)}
                        </span>
                      </div>
                    );
                  })}
              </div>
            )}
          </CardContent>
        </Card>

        {/* Provider card — donut chart default, toggle to ranking */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2 pt-4 px-4">
            <CardTitle className="text-sm font-medium">Spend by Provider</CardTitle>
            <div className="flex items-center gap-2">
              <Tabs defaultValue="chart" value={providerViewMode} onValueChange={(v) => setProviderViewMode(v as ModelViewMode)}>
                <TabsList className="h-7">
                  <TabsTrigger value="chart" className="text-xs px-3 h-5">📊 Chart</TabsTrigger>
                  <TabsTrigger value="ranking" className="text-xs px-3 h-5"><ListOrdered className="h-3 w-3" /></TabsTrigger>
                </TabsList>
              </Tabs>
              <Tabs defaultValue="spend" value={providerChartMode} onValueChange={(v) => setProviderChartMode(v as ChartMode)}>
                <TabsList className="h-7">
                  <TabsTrigger value="spend" className="text-xs px-3 h-5">💰</TabsTrigger>
                  <TabsTrigger value="tokens" className="text-xs px-3 h-5">📊</TabsTrigger>
                  <TabsTrigger value="requests" className="text-xs px-3 h-5">📋</TabsTrigger>
                </TabsList>
              </Tabs>
            </div>
          </CardHeader>
          <CardContent>
            {providerLoading ? (
              <Skeleton className="h-64 w-full" />
            ) : providerChartData.length === 0 ? (
              <div className="flex items-center justify-center h-64 text-sm text-muted-foreground">
                No data available
              </div>
            ) : providerViewMode === "chart" ? (
              <div className="h-[200px] md:h-[260px]">
                <ResponsiveContainer width="100%" height="100%">
                  <PieChart>
                    <Pie
                      data={providerChartData}
                      dataKey={providerChartMode === "tokens" ? "tokens" : providerChartMode === "requests" ? "requests" : "value"}
                      nameKey="name"
                      cx="50%"
                      cy="50%"
                      outerRadius={80}
                      label={({ name, percent }) => `${name} ${((percent ?? 0) * 100).toFixed(0)}%`}
                      labelLine={true}
                    >
                      {providerChartData.map((_, i) => (
                        <Cell key={i} fill={COLORS[i % COLORS.length]} />
                      ))}
                    </Pie>
                    <Tooltip
                      formatter={(value) => {
                        if (providerChartMode === "tokens") return [fmtTokens(value as number), "Tokens"];
                        if (providerChartMode === "requests") return [value, "Requests"];
                        return [fmtSpend(value as number), "Spend"];
                      }}
                    />
                  </PieChart>
                </ResponsiveContainer>
              </div>
            ) : (
              <div className="space-y-2">
                {[...providerChartData]
                  .sort((a, b) => {
                    if (providerChartMode === "tokens") return b.tokens - a.tokens;
                    if (providerChartMode === "requests") return b.requests - a.requests;
                    return b.value - a.value;
                  })
                  .slice(0, 5)
                  .map((p, i) => {
                    const metricValue =
                      providerChartMode === "tokens" ? p.tokens :
                      providerChartMode === "requests" ? p.requests :
                      p.value;
                    const provMaxValue = (() => {
                      if (providerChartMode === "tokens") return Math.max(...providerChartData.map(x => x.tokens));
                      if (providerChartMode === "requests") return Math.max(...providerChartData.map(x => x.requests));
                      return Math.max(...providerChartData.map(x => x.value));
                    })();
                    const pct = provMaxValue > 0 ? (metricValue / provMaxValue) * 100 : 0;

                    return (
                      <div key={p.name} className="flex items-center gap-3">
                        <span className="text-xs font-mono w-5 text-muted-foreground">#{i + 1}</span>
                        <span className="text-sm truncate w-[100px]">{p.name}</span>
                        <div className="flex-1 h-3 bg-muted rounded overflow-hidden">
                          <div
                            className="h-full rounded transition-all"
                            style={{
                              width: `${pct}%`,
                              backgroundColor: COLORS[i % COLORS.length],
                            }}
                          />
                        </div>
                        <span className="text-xs font-mono w-[80px] text-right">
                          {providerChartMode === "tokens"
                            ? fmtTokens(metricValue)
                            : providerChartMode === "requests"
                              ? metricValue.toLocaleString()
                              : fmtSpend(metricValue)}
                        </span>
                      </div>
                    );
                  })}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
