import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { fmtTokens, fmtExact } from "@/lib/format";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Tooltip as UiTooltip,
  TooltipTrigger,
  TooltipContent,
  TooltipProvider,
} from "@/components/ui/tooltip";
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
  cache_read_tokens: number;
  cache_creation_tokens: number;
  regular_input_tokens: number;
  cache_read_spend: number;
  cache_create_spend: number;
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
  cache_read_tokens: number;
  cache_creation_tokens: number;
  regular_input_tokens: number;
  cache_read_spend: number;
  cache_create_spend: number;
}

interface ActivityResponse {
  metadata: ActivityMetadata;
  daily: DailyRow[];
  granularity: "hourly" | "daily";
  /** Wall-clock timezone (minutes east of UTC) the daily[] buckets are in. 0 = UTC. */
  timezone_offset_minutes?: number;
  /** Optional IANA timezone name for the buckets (echoed from the request). */
  tz_name?: string;
}

interface ModelAgg {
  model: string;
  total_tokens: number;
  total_spend: number;
  requests: number;
}

interface ModelGroupAgg {
  model_group: string;
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

type DatePreset = "today" | "3d" | "7d" | "30d" | "custom";

function toLocalDateStr(d: Date): string {
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${mm}-${dd}`;
}

function presetRange(p: DatePreset): { start: string; end: string } {
  const now = new Date();
  switch (p) {
    case "today":
      return { start: toLocalDateStr(now), end: toLocalDateStr(now) };
    case "3d":
      return {
        start: toLocalDateStr(new Date(now.getTime() - 3 * 86400000)),
        end: toLocalDateStr(now),
      };
    case "7d":
      return {
        start: toLocalDateStr(new Date(now.getTime() - 7 * 86400000)),
        end: toLocalDateStr(now),
      };
    case "30d":
    default:
      return {
        start: toLocalDateStr(new Date(now.getTime() - 30 * 86400000)),
        end: toLocalDateStr(now),
      };
  }
}

const COLORS = [
  "#3b82f6",
  "#22c55e",
  "#f59e0b",
  "#8b5cf6",
  "#ec4899",
  "#14b8a6",
  "#f97316",
  "#06b6d4",
  "#84cc16",
  "#6366f1",
];

const PRESET_KEYS: DatePreset[] = ["today", "3d", "7d", "30d", "custom"];
const PRESET_LABELS: Record<DatePreset, string> = {
  today: "usage.datePresets.today",
  "3d": "usage.datePresets.3d",
  "7d": "usage.datePresets.7d",
  "30d": "usage.datePresets.30d",
  custom: "usage.datePresets.custom",
};

type ChartMode = "spend" | "tokens" | "requests";
type ModelViewMode = "chart" | "ranking";

/** Merge providers whose share is below 1% into a single "others" entry. */
function mergeSmallProviders(
  data: ProviderChartData[],
  mode: ChartMode,
): ProviderChartData[] {
  const dataKey =
    mode === "tokens" ? "tokens" : mode === "requests" ? "requests" : "value";
  const total = data.reduce((sum, d) => sum + (d as any)[dataKey], 0);
  if (total === 0) return data;

  const threshold = total * 0.01; // 1%
  const main: ProviderChartData[] = [];
  let othersValue = 0,
    othersTokens = 0,
    othersRequests = 0;

  for (const d of data) {
    if ((d as any)[dataKey] >= threshold) {
      main.push(d);
    } else {
      othersValue += d.value;
      othersTokens += d.tokens;
      othersRequests += d.requests;
    }
  }

  if (othersValue > 0 || othersTokens > 0 || othersRequests > 0) {
    main.push({
      name: "others",
      value: othersValue,
      tokens: othersTokens,
      requests: othersRequests,
    });
  }

  return main;
}

function truncateApiKey(apiKey: string): string {
  if (apiKey.length <= 12) return apiKey;
  return `${apiKey.slice(0, 6)}...${apiKey.slice(-4)}`;
}

function yAxisTick(v: number): string {
  return fmtTokens(v);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Component
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function UsagePage() {
  const { t } = useTranslation();
  const [preset, setPreset] = useState<DatePreset>("3d");
  const [startDate, setStartDate] = useState(presetRange("3d").start);
  const [endDate, setEndDate] = useState(presetRange("3d").end);
  const [globalChartMode, setGlobalChartMode] = useState<ChartMode>("spend");
  const [modelViewMode, setModelViewMode] = useState<ModelViewMode>("chart");
  const [providerViewMode, setProviderViewMode] =
    useState<ModelViewMode>("chart");
  const [groupViewMode, setGroupViewMode] = useState<ModelViewMode>("chart");

  // Minutes east of UTC for the browser's timezone (UTC+8 → 480). The backend
  // buckets daily/hourly spend by LOCAL wall-clock day when this is provided.
  const offsetMinutes = -new Date().getTimezoneOffset();
  // IANA name of the browser's timezone, echoed back in the activity response.
  const tzName = Intl.DateTimeFormat().resolvedOptions().timeZone ?? "";

  const handlePreset = (p: DatePreset) => {
    setPreset(p);
    if (p !== "custom") {
      const r = presetRange(p);
      setStartDate(r.start);
      setEndDate(r.end);
    }
  };

  // Activity overview (metadata + daily)
  const { data: activity, isLoading: activityLoading } =
    useQuery<ActivityResponse>({
      queryKey: ["global-spend-activity", startDate, endDate, offsetMinutes, tzName],
      queryFn: () =>
        apiGet(
          `/global/spend/activity?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(endDate)}&offset_minutes=${offsetMinutes}&tz_name=${encodeURIComponent(tzName)}`,
        ),
      refetchInterval: 30_000,
    });

  const metadata = activity?.metadata;

  // Model aggregation
  const { data: modelData, isLoading: modelLoading } = useQuery<AggResponse>({
    queryKey: ["spend-models", startDate, endDate, offsetMinutes],
    queryFn: () =>
      apiGet(
        `/global/spend/models?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(endDate)}&offset_minutes=${offsetMinutes}`,
      ),
    refetchInterval: 30_000,
  });

  // Provider aggregation
  const { data: providerData, isLoading: providerLoading } =
    useQuery<AggResponse>({
      queryKey: ["spend-providers", startDate, endDate, offsetMinutes],
      queryFn: () =>
        apiGet(
          `/spend/providers?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(endDate)}&offset_minutes=${offsetMinutes}`,
        ),
      refetchInterval: 30_000,
    });

  // Model Group aggregation
  const { data: groupData, isLoading: groupLoading } = useQuery<AggResponse>({
    queryKey: ["spend-model-groups", startDate, endDate, offsetMinutes],
    queryFn: () =>
      apiGet(
        `/global/spend/model-groups?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(endDate)}&offset_minutes=${offsetMinutes}`,
      ),
    refetchInterval: 30_000,
  });

  // Key rankings
  const { data: keyRankings, isLoading: keyRankingsLoading } =
    useQuery<KeyRankingResponse>({
      queryKey: ["key-rankings", startDate, endDate, offsetMinutes],
      queryFn: () =>
        apiGet(
          `/global/spend/keys/rankings?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(endDate)}&offset_minutes=${offsetMinutes}&limit=5`,
        ),
      refetchInterval: 30_000,
    });

  const modelChartData = (modelData?.data ?? []) as ModelAgg[];
  const groupChartData = (groupData?.data ?? []) as unknown as ModelGroupAgg[];
  const providerChartData: ProviderChartData[] = (
    (providerData?.data ?? []) as ProviderAgg[]
  ).map((a) => ({
    name: a.provider,
    value: Math.round(a.total_spend * 10000) / 10000,
    tokens: a.total_tokens,
    requests: a.requests,
  }));

  // Merge small providers (<1%) into "others" for the donut chart
  const providerChartDataMerged = useMemo(
    () => mergeSmallProviders(providerChartData, globalChartMode),
    [providerData, globalChartMode],
  );

  const dailyChartData = (activity?.daily ?? []).map((d) => ({
    date: d.date,
    spend: Math.round(d.spend * 10000) / 10000,
    tokens: d.tokens,
    requests: d.requests,
    prompt_tokens: d.prompt_tokens,
    completion_tokens: d.completion_tokens,
    successful_requests: d.successful_requests,
    failed_requests: d.failed_requests,
    cache_read_tokens: d.cache_read_tokens,
    cache_creation_tokens: d.cache_creation_tokens,
    regular_input_tokens: d.regular_input_tokens,
    cache_read_spend: d.cache_read_spend,
    cache_create_spend: d.cache_create_spend,
  }));

  const isLoading =
    activityLoading || modelLoading || providerLoading || groupLoading;

  // Compute max values for ranking progress bars
  const rankings = keyRankings ?? [];
  const rankingMaxSpend =
    rankings.length > 0 ? Math.max(...rankings.map((r) => r.total_spend)) : 1;
  const rankingMaxTokens =
    rankings.length > 0 ? Math.max(...rankings.map((r) => r.total_tokens)) : 1;
  const rankingMaxRequests =
    rankings.length > 0
      ? Math.max(...rankings.map((r) => r.total_requests))
      : 1;

  const modelRankingMaxSpend =
    modelChartData.length > 0
      ? Math.max(...modelChartData.map((m) => m.total_spend))
      : 1;
  const modelRankingMaxTokens =
    modelChartData.length > 0
      ? Math.max(...modelChartData.map((m) => m.total_tokens))
      : 1;
  const modelRankingMaxRequests =
    modelChartData.length > 0
      ? Math.max(...modelChartData.map((m) => m.requests))
      : 1;

  const groupRankingMaxSpend =
    groupChartData.length > 0
      ? Math.max(...groupChartData.map((m) => m.total_spend))
      : 1;
  const groupRankingMaxTokens =
    groupChartData.length > 0
      ? Math.max(...groupChartData.map((m) => m.total_tokens))
      : 1;
  const groupRankingMaxRequests =
    groupChartData.length > 0
      ? Math.max(...groupChartData.map((m) => m.requests))
      : 1;

  return (
    <div className="space-y-4">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">
            {t("usage.title")}
          </h1>
          <p className="text-sm text-muted-foreground">
            {t("usage.description")}
          </p>
        </div>
        {/* Toolbar: date presets + custom picker */}
        <div className="flex flex-wrap items-center gap-2">
          {PRESET_KEYS.map((k) => (
            <Button
              key={k}
              variant={preset === k ? "default" : "outline"}
              size="sm"
              onClick={() => handlePreset(k)}
              className="h-7 text-xs"
            >
              {t(PRESET_LABELS[k])}
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
            <CardTitle className="text-xs font-medium">
              {t("usage.cards.spend")}
            </CardTitle>
            <DollarSign className="h-3.5 w-3.5 text-muted-foreground" />
          </CardHeader>
          <CardContent className="p-4 pt-0">
            {activityLoading ? (
              <Skeleton className="h-6 w-16" />
            ) : (
              <div className="text-lg font-bold">
                {fmtSpend(metadata?.total_spend ?? 0)}
              </div>
            )}
            {(metadata?.cache_read_spend ?? 0) +
              (metadata?.cache_create_spend ?? 0) >
              0 && (
              <p className="text-[10px] text-amber-600 mt-0.5">
                cache $
                {(
                  (metadata?.cache_read_spend ?? 0) +
                  (metadata?.cache_create_spend ?? 0)
                ).toFixed(4)}
              </p>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-1 p-4">
            <CardTitle className="text-xs font-medium">
              {t("usage.cards.requests")}
            </CardTitle>
            <BarChart3 className="h-3.5 w-3.5 text-muted-foreground" />
          </CardHeader>
          <CardContent className="p-4 pt-0">
            {activityLoading ? (
              <Skeleton className="h-6 w-12" />
            ) : (
              <div className="text-lg font-bold">
                {metadata?.total_requests?.toLocaleString() ?? "—"}
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-1 p-4">
            <CardTitle className="text-xs font-medium">
              {t("usage.cards.ok")}
            </CardTitle>
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
            <CardTitle className="text-xs font-medium">
              {t("usage.cards.failed")}
            </CardTitle>
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
            <CardTitle className="text-xs font-medium">
              {t("usage.cards.tokens")}
            </CardTitle>
            <Sparkles className="h-3.5 w-3.5 text-muted-foreground" />
          </CardHeader>
          <CardContent className="p-4 pt-0">
            {activityLoading ? (
              <Skeleton className="h-6 w-16" />
            ) : (
              <TooltipProvider delayDuration={0}>
                <UiTooltip>
                  <TooltipTrigger asChild>
                    <div className="text-lg font-bold w-fit cursor-default">
                      {fmtTokens(metadata?.total_tokens ?? 0)}
                    </div>
                  </TooltipTrigger>
                  <TooltipContent side="bottom">
                    {fmtExact(metadata?.total_tokens ?? 0)}
                  </TooltipContent>
                </UiTooltip>
              </TooltipProvider>
            )}
            <p className="text-[10px] text-muted-foreground mt-0.5">
              p {fmtTokens(metadata?.prompt_tokens ?? 0)} / c{" "}
              {fmtTokens(metadata?.completion_tokens ?? 0)}
              {(metadata?.cache_read_tokens ?? 0) +
                (metadata?.cache_creation_tokens ?? 0) >
                0 &&
                `  ·  cache ${fmtTokens(metadata!.cache_read_tokens)}R / ${fmtTokens(metadata!.cache_creation_tokens)}W`}
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-1 p-4">
            <CardTitle className="text-xs font-medium">
              {t("usage.cards.rate")}
            </CardTitle>
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

      {/* Trend — independent tab state */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between pb-2 pt-4 px-4">
          <CardTitle className="text-sm font-medium">
            {t("usage.trend")}
          </CardTitle>
          <Tabs
            defaultValue="spend"
            value={globalChartMode}
            onValueChange={(v) => setGlobalChartMode(v as ChartMode)}
          >
            <TabsList className="h-7">
              <TabsTrigger value="spend" className="text-xs px-3 h-5">
                💰 Spend
              </TabsTrigger>
              <TabsTrigger value="tokens" className="text-xs px-3 h-5">
                📊 Tokens
              </TabsTrigger>
              <TabsTrigger value="requests" className="text-xs px-3 h-5">
                📋 Requests
              </TabsTrigger>
            </TabsList>
          </Tabs>
        </CardHeader>
        <CardContent>
          {activityLoading ? (
            <Skeleton className="h-64 w-full" />
          ) : dailyChartData.length === 0 ? (
            <div className="flex items-center justify-center h-64 text-sm text-muted-foreground">
              {t("usage.noData")}
            </div>
          ) : (
            <div className="h-[220px] md:h-[260px]">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart
                  data={dailyChartData}
                  margin={{ top: 5, right: 20, left: 10, bottom: 5 }}
                >
                  <CartesianGrid
                    strokeDasharray="3 3"
                    className="stroke-muted"
                  />
                  <XAxis
                    dataKey="date"
                    tick={{ fontSize: 11 }}
                    stroke="hsl(var(--muted-foreground))"
                    tickFormatter={(val) => {
                      if (activity?.granularity === "hourly") {
                        // Backend now returns LOCAL hour strings ("YYYY-MM-DDTHH:00:00");
                        // render directly — re-parsing as UTC would double-shift the label.
                        const s = String(val);
                        return `${s.slice(5, 7)}/${s.slice(8, 10)} ${s.slice(11, 16)}`;
                      }
                      return val;
                    }}
                  />
                  <YAxis
                    tick={{ fontSize: 11 }}
                    stroke="hsl(var(--muted-foreground))"
                    tickFormatter={yAxisTick}
                  />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: "hsl(var(--card))",
                      border: "1px solid hsl(var(--border))",
                      borderRadius: "6px",
                      fontSize: "12px",
                    }}
                    formatter={(value, name) => {
                      if (globalChartMode === "tokens")
                        return [fmtTokens(value as number), name];
                      if (globalChartMode === "requests") return [value, name];
                      return [fmtSpend(value as number), name];
                    }}
                    labelFormatter={(label) => {
                      const item = dailyChartData.find((d) => d.date === label);
                      if (!item) return label;
                      if (globalChartMode === "tokens") {
                        return `${label}\n  ${t("usage.chart.promptTokens")}: ${fmtTokens(item.prompt_tokens)}  |  ${t("usage.chart.completionTokens")}: ${fmtTokens(item.completion_tokens)}\n  ${t("usage.chart.totalTokens")}: ${fmtTokens(item.tokens)}`;
                      }
                      if (globalChartMode === "requests") {
                        return `${label}\n  ${t("usage.chart.successCount")}: ${item.successful_requests}  |  ${t("usage.chart.failedCount")}: ${item.failed_requests}\n  ${t("usage.chart.totalRequests")}: ${item.requests}`;
                      }
                      return `${label}  |  ${fmtSpend(item.spend)}`;
                    }}
                  />
                  {globalChartMode === "spend" && (
                    <Bar
                      dataKey="spend"
                      name={t("usage.chart.spend")}
                      fill="hsl(var(--primary))"
                      radius={[4, 4, 0, 0]}
                    />
                  )}
                  {globalChartMode === "tokens" && (
                    <>
                      <Bar
                        dataKey="completion_tokens"
                        name={t("usage.chart.output")}
                        fill="#3b82f6"
                        stackId="tokens"
                      />
                      <Bar
                        dataKey="cache_creation_tokens"
                        name={t("usage.chart.cacheWrite")}
                        fill="#f59e0b"
                        stackId="tokens"
                      />
                      <Bar
                        dataKey="cache_read_tokens"
                        name={t("usage.chart.cacheRead")}
                        fill="#22c55e"
                        stackId="tokens"
                      />
                      <Bar
                        dataKey="regular_input_tokens"
                        name={t("usage.chart.input")}
                        fill="#94a3b8"
                        stackId="tokens"
                        radius={[4, 4, 0, 0]}
                      />
                    </>
                  )}
                  {globalChartMode === "requests" && (
                    <>
                      <Bar
                        dataKey="failed_requests"
                        name={t("usage.cards.failed")}
                        fill="#ef4444"
                        stackId="requests"
                      />
                      <Bar
                        dataKey="successful_requests"
                        name={t("usage.chart.success")}
                        fill="#22c55e"
                        stackId="requests"
                        radius={[4, 4, 0, 0]}
                      />
                    </>
                  )}
                  <Legend />
                </BarChart>
              </ResponsiveContainer>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Row 1: {t('usage.topKeys')} + Spend by Provider */}
      <div className="grid gap-4 lg:grid-cols-2">
        {/* {t('usage.topKeys')} Ranking */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2 pt-4 px-4">
            <CardTitle className="text-sm font-medium">
              {t("usage.topKeys")}
            </CardTitle>
            <Tabs
              defaultValue="spend"
              value={globalChartMode}
              onValueChange={(v) => setGlobalChartMode(v as ChartMode)}
            >
              <TabsList className="h-7">
                <TabsTrigger value="spend" className="text-xs px-3 h-5">
                  💰 Spend
                </TabsTrigger>
                <TabsTrigger value="tokens" className="text-xs px-3 h-5">
                  📊 Tokens
                </TabsTrigger>
                <TabsTrigger value="requests" className="text-xs px-3 h-5">
                  📋 Requests
                </TabsTrigger>
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
                {t("usage.noData")}
              </div>
            ) : (
              <div className="space-y-2">
                {[...rankings]
                  .sort((a, b) => {
                    if (globalChartMode === "tokens")
                      return b.total_tokens - a.total_tokens;
                    if (globalChartMode === "requests")
                      return b.total_requests - a.total_requests;
                    return b.total_spend - a.total_spend;
                  })
                  .slice(0, 5)
                  .map((r, i) => {
                    const metricValue =
                      globalChartMode === "tokens"
                        ? r.total_tokens
                        : globalChartMode === "requests"
                          ? r.total_requests
                          : r.total_spend;
                    const maxValue =
                      globalChartMode === "tokens"
                        ? rankingMaxTokens
                        : globalChartMode === "requests"
                          ? rankingMaxRequests
                          : rankingMaxSpend;
                    const pct =
                      maxValue > 0 ? (metricValue / maxValue) * 100 : 0;

                    return (
                      <div key={r.api_key} className="flex items-center gap-3">
                        <span className="text-xs font-mono w-5 text-muted-foreground">
                          #{i + 1}
                        </span>
                        <span className="text-sm font-mono truncate w-[140px]">
                          {r.key_alias || "unknown"}
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
                          {globalChartMode === "tokens"
                            ? fmtTokens(metricValue)
                            : globalChartMode === "requests"
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
            <CardTitle className="text-sm font-medium">
              {t("usage.spendByProvider")}
            </CardTitle>
            <div className="flex items-center gap-2">
              <Tabs
                defaultValue="chart"
                value={providerViewMode}
                onValueChange={(v) => setProviderViewMode(v as ModelViewMode)}
              >
                <TabsList className="h-7">
                  <TabsTrigger value="chart" className="text-xs px-3 h-5">
                    📊 Chart
                  </TabsTrigger>
                  <TabsTrigger value="ranking" className="text-xs px-3 h-5">
                    <ListOrdered className="h-3 w-3" />
                  </TabsTrigger>
                </TabsList>
              </Tabs>
              <Tabs
                defaultValue="spend"
                value={globalChartMode}
                onValueChange={(v) => setGlobalChartMode(v as ChartMode)}
              >
                <TabsList className="h-7">
                  <TabsTrigger value="spend" className="text-xs px-3 h-5">
                    💰
                  </TabsTrigger>
                  <TabsTrigger value="tokens" className="text-xs px-3 h-5">
                    📊
                  </TabsTrigger>
                  <TabsTrigger value="requests" className="text-xs px-3 h-5">
                    📋
                  </TabsTrigger>
                </TabsList>
              </Tabs>
            </div>
          </CardHeader>
          <CardContent>
            {providerLoading ? (
              <Skeleton className="h-64 w-full" />
            ) : providerChartData.length === 0 ? (
              <div className="flex items-center justify-center h-64 text-sm text-muted-foreground">
                {t("usage.noData")}
              </div>
            ) : providerViewMode === "chart" ? (
              <div className="h-[200px] md:h-[260px]">
                <ResponsiveContainer width="100%" height="100%">
                  <PieChart>
                    <Pie
                      data={providerChartDataMerged}
                      dataKey={
                        globalChartMode === "tokens"
                          ? "tokens"
                          : globalChartMode === "requests"
                            ? "requests"
                            : "value"
                      }
                      nameKey="name"
                      cx="50%"
                      cy="50%"
                      outerRadius={80}
                      label={({ name, percent }) =>
                        `${name} ${((percent ?? 0) * 100).toFixed(2)}%`
                      }
                      labelLine={true}
                    >
                      {providerChartDataMerged.map((_, i) => (
                        <Cell key={i} fill={COLORS[i % COLORS.length]} />
                      ))}
                    </Pie>
                    <Tooltip
                      formatter={(value) => {
                        if (globalChartMode === "tokens")
                          return [fmtTokens(value as number), "Tokens"];
                        if (globalChartMode === "requests")
                          return [value, "Requests"];
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
                    if (globalChartMode === "tokens")
                      return b.tokens - a.tokens;
                    if (globalChartMode === "requests")
                      return b.requests - a.requests;
                    return b.value - a.value;
                  })
                  .slice(0, 5)
                  .map((p, i) => {
                    const metricValue =
                      globalChartMode === "tokens"
                        ? p.tokens
                        : globalChartMode === "requests"
                          ? p.requests
                          : p.value;
                    const provMaxValue = (() => {
                      if (globalChartMode === "tokens")
                        return Math.max(
                          ...providerChartData.map((x) => x.tokens),
                        );
                      if (globalChartMode === "requests")
                        return Math.max(
                          ...providerChartData.map((x) => x.requests),
                        );
                      return Math.max(...providerChartData.map((x) => x.value));
                    })();
                    const pct =
                      provMaxValue > 0 ? (metricValue / provMaxValue) * 100 : 0;

                    return (
                      <div key={p.name} className="flex items-center gap-3">
                        <span className="text-xs font-mono w-5 text-muted-foreground">
                          #{i + 1}
                        </span>
                        <span className="text-sm truncate w-[100px]">
                          {p.name}
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
                          {globalChartMode === "tokens"
                            ? fmtTokens(metricValue)
                            : globalChartMode === "requests"
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

      {/* Row 2: Spend by Model + Spend by Model Group */}
      <div className="grid gap-4 lg:grid-cols-2 mt-4">
        {/* Model Group card — left side */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2 pt-4 px-4">
            <CardTitle className="text-sm font-medium">
              {t("usage.spendByModelGroup")}
            </CardTitle>
            <div className="flex items-center gap-2">
              <Tabs
                defaultValue="chart"
                value={groupViewMode}
                onValueChange={(v) => setGroupViewMode(v as ModelViewMode)}
              >
                <TabsList className="h-7">
                  <TabsTrigger value="chart" className="text-xs px-3 h-5">
                    📊 Chart
                  </TabsTrigger>
                  <TabsTrigger value="ranking" className="text-xs px-3 h-5">
                    <ListOrdered className="h-3 w-3" />
                  </TabsTrigger>
                </TabsList>
              </Tabs>
              {groupViewMode === "chart" && (
                <Tabs
                  defaultValue="spend"
                  value={globalChartMode}
                  onValueChange={(v) => setGlobalChartMode(v as ChartMode)}
                >
                  <TabsList className="h-7">
                    <TabsTrigger value="spend" className="text-xs px-3 h-5">
                      💰
                    </TabsTrigger>
                    <TabsTrigger value="tokens" className="text-xs px-3 h-5">
                      📊
                    </TabsTrigger>
                    <TabsTrigger value="requests" className="text-xs px-3 h-5">
                      📋
                    </TabsTrigger>
                  </TabsList>
                </Tabs>
              )}
              {groupViewMode === "ranking" && (
                <Tabs
                  defaultValue="spend"
                  value={globalChartMode}
                  onValueChange={(v) => setGlobalChartMode(v as ChartMode)}
                >
                  <TabsList className="h-7">
                    <TabsTrigger value="spend" className="text-xs px-3 h-5">
                      💰
                    </TabsTrigger>
                    <TabsTrigger value="tokens" className="text-xs px-3 h-5">
                      📊
                    </TabsTrigger>
                    <TabsTrigger value="requests" className="text-xs px-3 h-5">
                      📋
                    </TabsTrigger>
                  </TabsList>
                </Tabs>
              )}
            </div>
          </CardHeader>
          <CardContent>
            {groupLoading ? (
              <Skeleton className="h-64 w-full" />
            ) : groupChartData.length === 0 ? (
              <div className="flex items-center justify-center h-64 text-sm text-muted-foreground">
                {t("usage.noData")}
              </div>
            ) : groupViewMode === "chart" ? (
              <div className="h-[200px] md:h-[260px]">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart
                    data={[...groupChartData]
                      .sort((a, b) => {
                        if (globalChartMode === "tokens")
                          return b.total_tokens - a.total_tokens;
                        if (globalChartMode === "requests")
                          return b.requests - a.requests;
                        return b.total_spend - a.total_spend;
                      })
                      .slice(0, 5)}
                    margin={{ top: 5, right: 20, left: 10, bottom: 5 }}
                  >
                    <CartesianGrid
                      strokeDasharray="3 3"
                      className="stroke-muted"
                    />
                    <XAxis
                      dataKey="model_group"
                      tick={{ fontSize: 11 }}
                      stroke="hsl(var(--muted-foreground))"
                    />
                    <YAxis
                      tick={{ fontSize: 11 }}
                      stroke="hsl(var(--muted-foreground))"
                      tickFormatter={yAxisTick}
                    />
                    <Tooltip
                      contentStyle={{
                        backgroundColor: "hsl(var(--card))",
                        border: "1px solid hsl(var(--border))",
                        borderRadius: "6px",
                        fontSize: "12px",
                      }}
                      formatter={(value) => {
                        if (globalChartMode === "tokens")
                          return [fmtTokens(value as number), "Tokens"];
                        if (globalChartMode === "requests")
                          return [value, "Requests"];
                        return [fmtSpend(value as number), "Spend"];
                      }}
                    />
                    {globalChartMode === "spend" && (
                      <Bar
                        dataKey="total_spend"
                        name="Spend"
                        fill="hsl(var(--primary))"
                        radius={[4, 4, 0, 0]}
                      />
                    )}
                    {globalChartMode === "tokens" && (
                      <Bar
                        dataKey="total_tokens"
                        name="Tokens"
                        fill="#f59e0b"
                        radius={[4, 4, 0, 0]}
                      />
                    )}
                    {globalChartMode === "requests" && (
                      <Bar
                        dataKey="requests"
                        name="Requests"
                        fill="#22c55e"
                        radius={[4, 4, 0, 0]}
                      />
                    )}
                  </BarChart>
                </ResponsiveContainer>
              </div>
            ) : (
              <div className="space-y-2">
                {[...groupChartData]
                  .sort((a, b) => {
                    if (globalChartMode === "tokens")
                      return b.total_tokens - a.total_tokens;
                    if (globalChartMode === "requests")
                      return b.requests - a.requests;
                    return b.total_spend - a.total_spend;
                  })
                  .slice(0, 5)
                  .map((g, i) => {
                    const metricValue =
                      globalChartMode === "tokens"
                        ? g.total_tokens
                        : globalChartMode === "requests"
                          ? g.requests
                          : g.total_spend;
                    const maxValue =
                      globalChartMode === "tokens"
                        ? groupRankingMaxTokens
                        : globalChartMode === "requests"
                          ? groupRankingMaxRequests
                          : groupRankingMaxSpend;
                    const pct =
                      maxValue > 0 ? (metricValue / maxValue) * 100 : 0;

                    return (
                      <div
                        key={g.model_group}
                        className="flex items-center gap-3"
                      >
                        <span className="text-xs font-mono w-5 text-muted-foreground">
                          #{i + 1}
                        </span>
                        <span className="text-sm truncate w-[120px]">
                          {g.model_group}
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
                          {globalChartMode === "tokens"
                            ? fmtTokens(metricValue)
                            : globalChartMode === "requests"
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

        {/* Model card — right side */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2 pt-4 px-4">
            <CardTitle className="text-sm font-medium">
              {t("usage.spendByModel")}
            </CardTitle>
            <div className="flex items-center gap-2">
              <Tabs
                defaultValue="chart"
                value={modelViewMode}
                onValueChange={(v) => setModelViewMode(v as ModelViewMode)}
              >
                <TabsList className="h-7">
                  <TabsTrigger value="chart" className="text-xs px-3 h-5">
                    📊 Chart
                  </TabsTrigger>
                  <TabsTrigger value="ranking" className="text-xs px-3 h-5">
                    <ListOrdered className="h-3 w-3" />
                  </TabsTrigger>
                </TabsList>
              </Tabs>
              {modelViewMode === "chart" && (
                <Tabs
                  defaultValue="spend"
                  value={globalChartMode}
                  onValueChange={(v) => setGlobalChartMode(v as ChartMode)}
                >
                  <TabsList className="h-7">
                    <TabsTrigger value="spend" className="text-xs px-3 h-5">
                      💰
                    </TabsTrigger>
                    <TabsTrigger value="tokens" className="text-xs px-3 h-5">
                      📊
                    </TabsTrigger>
                    <TabsTrigger value="requests" className="text-xs px-3 h-5">
                      📋
                    </TabsTrigger>
                  </TabsList>
                </Tabs>
              )}
              {modelViewMode === "ranking" && (
                <Tabs
                  defaultValue="spend"
                  value={globalChartMode}
                  onValueChange={(v) => setGlobalChartMode(v as ChartMode)}
                >
                  <TabsList className="h-7">
                    <TabsTrigger value="spend" className="text-xs px-3 h-5">
                      💰
                    </TabsTrigger>
                    <TabsTrigger value="tokens" className="text-xs px-3 h-5">
                      📊
                    </TabsTrigger>
                    <TabsTrigger value="requests" className="text-xs px-3 h-5">
                      📋
                    </TabsTrigger>
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
                {t("usage.noData")}
              </div>
            ) : modelViewMode === "chart" ? (
              <div className="h-[200px] md:h-[260px]">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart
                    data={[...modelChartData]
                      .sort((a, b) => {
                        if (globalChartMode === "tokens")
                          return b.total_tokens - a.total_tokens;
                        if (globalChartMode === "requests")
                          return b.requests - a.requests;
                        return b.total_spend - a.total_spend;
                      })
                      .slice(0, 5)}
                    margin={{ top: 5, right: 20, left: 10, bottom: 5 }}
                  >
                    <CartesianGrid
                      strokeDasharray="3 3"
                      className="stroke-muted"
                    />
                    <XAxis
                      dataKey="model"
                      tick={{ fontSize: 11 }}
                      stroke="hsl(var(--muted-foreground))"
                    />
                    <YAxis
                      tick={{ fontSize: 11 }}
                      stroke="hsl(var(--muted-foreground))"
                      tickFormatter={yAxisTick}
                    />
                    <Tooltip
                      contentStyle={{
                        backgroundColor: "hsl(var(--card))",
                        border: "1px solid hsl(var(--border))",
                        borderRadius: "6px",
                        fontSize: "12px",
                      }}
                      formatter={(value) => {
                        if (globalChartMode === "tokens")
                          return [fmtTokens(value as number), "Tokens"];
                        if (globalChartMode === "requests")
                          return [value, "Requests"];
                        return [fmtSpend(value as number), "Spend"];
                      }}
                    />
                    {globalChartMode === "spend" && (
                      <Bar
                        dataKey="total_spend"
                        name="Spend"
                        fill="hsl(var(--primary))"
                        radius={[4, 4, 0, 0]}
                      />
                    )}
                    {globalChartMode === "tokens" && (
                      <Bar
                        dataKey="total_tokens"
                        name="Tokens"
                        fill="#f59e0b"
                        radius={[4, 4, 0, 0]}
                      />
                    )}
                    {globalChartMode === "requests" && (
                      <Bar
                        dataKey="requests"
                        name="Requests"
                        fill="#22c55e"
                        radius={[4, 4, 0, 0]}
                      />
                    )}
                  </BarChart>
                </ResponsiveContainer>
              </div>
            ) : (
              <div className="space-y-2">
                {[...modelChartData]
                  .sort((a, b) => {
                    if (globalChartMode === "tokens")
                      return b.total_tokens - a.total_tokens;
                    if (globalChartMode === "requests")
                      return b.requests - a.requests;
                    return b.total_spend - a.total_spend;
                  })
                  .slice(0, 5)
                  .map((m, i) => {
                    const metricValue =
                      globalChartMode === "tokens"
                        ? m.total_tokens
                        : globalChartMode === "requests"
                          ? m.requests
                          : m.total_spend;
                    const maxValue =
                      globalChartMode === "tokens"
                        ? modelRankingMaxTokens
                        : globalChartMode === "requests"
                          ? modelRankingMaxRequests
                          : modelRankingMaxSpend;
                    const pct =
                      maxValue > 0 ? (metricValue / maxValue) * 100 : 0;

                    return (
                      <div key={m.model} className="flex items-center gap-3">
                        <span className="text-xs font-mono w-5 text-muted-foreground">
                          #{i + 1}
                        </span>
                        <span className="text-sm truncate w-[120px]">
                          {m.model}
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
                          {globalChartMode === "tokens"
                            ? fmtTokens(metricValue)
                            : globalChartMode === "requests"
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
