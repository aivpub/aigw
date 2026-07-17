import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  PieChart,
  Pie,
  Cell,
  Legend,
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
  "hsl(var(--primary))",
  "hsl(var(--secondary-foreground))",
  "hsl(var(--muted-foreground))",
  "hsl(var(--accent-foreground))",
  "hsl(var(--destructive))",
  "#3b82f6",
  "#22c55e",
  "#f59e0b",
  "#8b5cf6",
  "#ec4899",
];

const PRESETS: { key: DatePreset; label: string }[] = [
  { key: "3d", label: "3 days" },
  { key: "7d", label: "7 days" },
  { key: "30d", label: "30 days" },
  { key: "custom", label: "Custom" },
];

type ChartMode = "spend" | "tokens";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Component
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function UsagePage() {
  const [preset, setPreset] = useState<DatePreset>("30d");
  const [startDate, setStartDate] = useState(presetRange("30d").start);
  const [endDate, setEndDate] = useState(presetRange("30d").end);
  const [chartMode, setChartMode] = useState<ChartMode>("spend");

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

  const modelChartData = (modelData?.data ?? []) as ModelAgg[];
  const providerChartData = ((providerData?.data ?? []) as ProviderAgg[]).map((a) => ({
    name: a.provider,
    value: Math.round(a.total_spend * 10000) / 10000,
    tokens: a.total_tokens,
  }));

  const dailyChartData = (activity?.daily ?? []).map((d) => ({
    date: d.date,
    spend: Math.round(d.spend * 10000) / 10000,
    tokens: d.tokens,
    requests: d.requests,
  }));

  const isLoading = activityLoading || modelLoading || providerLoading;

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

      {/* Daily Chart with Spend/Tokens Tabs */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between pb-2 pt-4 px-4">
          <CardTitle className="text-sm font-medium">Daily Trend</CardTitle>
          <Tabs defaultValue="spend" value={chartMode} onValueChange={(v) => setChartMode(v as ChartMode)}>
            <TabsList className="h-7">
              <TabsTrigger value="spend" className="text-xs px-3 h-5">💰 Spend</TabsTrigger>
              <TabsTrigger value="tokens" className="text-xs px-3 h-5">📊 Tokens</TabsTrigger>
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
                  <YAxis tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: "hsl(var(--card))",
                      border: "1px solid hsl(var(--border))",
                      borderRadius: "6px",
                      fontSize: "12px",
                    }}
                    formatter={(value, name, props) => {
                      if (name === "spend") return [fmtSpend(value as number), "Spend"];
                      if (name === "tokens") return [fmtTokens(value as number), "Tokens"];
                      if (name === "requests") return [value, "Requests"];
                      return [value, name];
                    }}
                    labelFormatter={(label) => {
                      const item = dailyChartData.find((d) => d.date === label);
                      if (!item) return label;
                      // Enhanced tooltip: show spend + tokens + requests for the day
                      return `${label}  |  ${fmtSpend(item.spend)}  |  ${item.requests} req  |  ${fmtTokens(item.tokens)} tokens`;
                    }}
                  />
                  {chartMode === "spend" ? (
                    <Bar dataKey="spend" fill="hsl(var(--primary))" radius={[4, 4, 0, 0]} />
                  ) : (
                    <Bar dataKey="tokens" fill="#3b82f6" radius={[4, 4, 0, 0]} />
                  )}
                </BarChart>
              </ResponsiveContainer>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Model / Provider Charts with Spend/Tokens Tabs */}
      <div className="grid gap-4 lg:grid-cols-2">
        {/* Model Bar Chart */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2 pt-4 px-4">
            <CardTitle className="text-sm font-medium">Spend by Model</CardTitle>
            <Tabs defaultValue="spend" value={chartMode} onValueChange={(v) => setChartMode(v as ChartMode)}>
              <TabsList className="h-7">
                <TabsTrigger value="spend" className="text-xs px-3 h-5">💰</TabsTrigger>
                <TabsTrigger value="tokens" className="text-xs px-3 h-5">📊</TabsTrigger>
              </TabsList>
            </Tabs>
          </CardHeader>
          <CardContent>
            {modelLoading ? (
              <Skeleton className="h-64 w-full" />
            ) : modelChartData.length === 0 ? (
              <div className="flex items-center justify-center h-64 text-sm text-muted-foreground">
                No data available
              </div>
            ) : (
              <div className="h-[200px] md:h-[260px]">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={modelChartData} margin={{ top: 5, right: 20, left: 10, bottom: 5 }}>
                    <CartesianGrid strokeDasharray="3 3" className="stroke-muted" />
                    <XAxis dataKey="model" tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" />
                    <YAxis tick={{ fontSize: 11 }} stroke="hsl(var(--muted-foreground))" />
                    <Tooltip
                      contentStyle={{
                        backgroundColor: "hsl(var(--card))",
                        border: "1px solid hsl(var(--border))",
                        borderRadius: "6px",
                        fontSize: "12px",
                      }}
                      formatter={(value, name, props) => {
                        if (chartMode === "tokens") return [fmtTokens(value as number), "Tokens"];
                        return [fmtSpend(value as number), "Spend"];
                      }}
                    />
                    {chartMode === "spend" ? (
                      <Bar dataKey="total_spend" name="Spend" fill="hsl(var(--primary))" radius={[4, 4, 0, 0]} />
                    ) : (
                      <Bar dataKey="total_tokens" name="Tokens" fill="#3b82f6" radius={[4, 4, 0, 0]} />
                    )}
                  </BarChart>
                </ResponsiveContainer>
              </div>
            )}
          </CardContent>
        </Card>

        {/* Provider Donut Chart */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2 pt-4 px-4">
            <CardTitle className="text-sm font-medium">Spend by Provider</CardTitle>
            <PieChartIcon className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {providerLoading ? (
              <Skeleton className="h-64 w-full" />
            ) : providerChartData.length === 0 ? (
              <div className="flex items-center justify-center h-64 text-sm text-muted-foreground">
                No data available
              </div>
            ) : (
              <div className="h-[200px] md:h-[260px]">
                <ResponsiveContainer width="100%" height="100%">
                  <PieChart>
                    <Pie
                      data={providerChartData}
                      cx="50%"
                      cy="50%"
                      innerRadius={50}
                      outerRadius={80}
                      paddingAngle={2}
                      dataKey="value"
                      label={({ name, value }) => `${name}: ${fmtSpend(value)}`}
                      labelLine={false}
                    >
                      {providerChartData.map((_, i) => (
                        <Cell key={i} fill={COLORS[i % COLORS.length]} />
                      ))}
                    </Pie>
                    <Tooltip
                      contentStyle={{
                        backgroundColor: "hsl(var(--card))",
                        border: "1px solid hsl(var(--border))",
                        borderRadius: "6px",
                        fontSize: "12px",
                      }}
                      formatter={(value, name, props) => {
                        const item = providerChartData[props.payload?.payload ? providerChartData.indexOf(props.payload.payload) : -1];
                        if (item && chartMode === "tokens") return [fmtTokens(item.tokens), "Tokens"];
                        return [fmtSpend(value as number), "Spend"];
                      }}
                    />
                    <Legend />
                  </PieChart>
                </ResponsiveContainer>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
