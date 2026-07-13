import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
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
import { format, subDays } from "date-fns";

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

function todayStr(): string {
  return format(new Date(), "yyyy-MM-dd");
}

type DatePreset = "3d" | "7d" | "30d" | "custom";

function presetRange(p: DatePreset): { start: string; end: string } {
  const now = new Date();
  const end = format(now, "yyyy-MM-dd");
  switch (p) {
    case "3d":
      return { start: format(subDays(now, 3), "yyyy-MM-dd"), end };
    case "7d":
      return { start: format(subDays(now, 7), "yyyy-MM-dd"), end };
    case "30d":
    default:
      return { start: format(subDays(now, 30), "yyyy-MM-dd"), end };
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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Component
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function UsagePage() {
  const [preset, setPreset] = useState<DatePreset>("30d");
  const [startDate, setStartDate] = useState(presetRange("30d").start);
  const [endDate, setEndDate] = useState(presetRange("30d").end);

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
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Usage</h1>
        <p className="text-sm text-muted-foreground">
          Usage and spend overview
        </p>
      </div>

      {/* Date presets */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium flex items-center gap-2">
            <Calendar className="h-4 w-4" />
            Time Range
          </CardTitle>
        </CardHeader>
        <CardContent>
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
              <div className="flex items-center gap-2 ml-2">
                <Input
                  type="date"
                  value={startDate}
                  onChange={(e) => setStartDate(e.target.value)}
                  className="h-7 w-36 text-xs"
                />
                <span className="text-xs text-muted-foreground">–</span>
                <Input
                  type="date"
                  value={endDate}
                  onChange={(e) => setEndDate(e.target.value)}
                  className="h-7 w-36 text-xs"
                />
              </div>
            )}
          </div>
        </CardContent>
      </Card>

      {/* Metric Cards */}
      <div className="grid gap-4 grid-cols-2 md:grid-cols-3">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Total Spend</CardTitle>
            <DollarSign className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {activityLoading ? (
              <Skeleton className="h-8 w-24" />
            ) : (
              <div className="text-2xl font-bold">{fmtSpend(metadata?.total_spend ?? 0)}</div>
            )}
            <p className="text-xs text-muted-foreground mt-1">
              {startDate} — {endDate}
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Total Requests</CardTitle>
            <BarChart3 className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {activityLoading ? (
              <Skeleton className="h-8 w-16" />
            ) : (
              <div className="text-2xl font-bold">{metadata?.total_requests?.toLocaleString() ?? "—"}</div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Successful</CardTitle>
            <CheckCircle className="h-4 w-4 text-green-500" />
          </CardHeader>
          <CardContent>
            {activityLoading ? (
              <Skeleton className="h-8 w-16" />
            ) : (
              <div className="text-2xl font-bold text-green-600 dark:text-green-400">
                {metadata?.successful_requests?.toLocaleString() ?? "—"}
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Failed</CardTitle>
            <XCircle className="h-4 w-4 text-red-500" />
          </CardHeader>
          <CardContent>
            {activityLoading ? (
              <Skeleton className="h-8 w-16" />
            ) : (
              <div className="text-2xl font-bold text-red-600 dark:text-red-400">
                {metadata?.failed_requests?.toLocaleString() ?? "—"}
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Total Tokens</CardTitle>
            <Sparkles className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {activityLoading ? (
              <Skeleton className="h-8 w-20" />
            ) : (
              <div className="text-2xl font-bold">{fmtTokens(metadata?.total_tokens ?? 0)}</div>
            )}
            <p className="text-xs text-muted-foreground mt-1">
              prompt {fmtTokens(metadata?.prompt_tokens ?? 0)} / completion {fmtTokens(metadata?.completion_tokens ?? 0)}
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Success Rate</CardTitle>
            <TrendingUp className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {activityLoading ? (
              <Skeleton className="h-8 w-16" />
            ) : (
              <div className="text-2xl font-bold">
                {metadata && metadata.total_requests > 0
                  ? `${((metadata.successful_requests / metadata.total_requests) * 100).toFixed(1)}%`
                  : "—"}
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Daily Spend Bar Chart */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between pb-2">
          <CardTitle className="text-sm font-medium">Daily Spend</CardTitle>
          <BarChart3 className="h-4 w-4 text-muted-foreground" />
        </CardHeader>
        <CardContent>
          {activityLoading ? (
            <Skeleton className="h-64 w-full" />
          ) : dailyChartData.length === 0 ? (
            <div className="flex items-center justify-center h-64 text-sm text-muted-foreground">
              No data available
            </div>
          ) : (
            <div className="h-[200px] md:h-[280px]">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={dailyChartData} margin={{ top: 5, right: 20, left: 10, bottom: 5 }}>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-muted" />
                  <XAxis
                    dataKey="date"
                    tick={{ fontSize: 11 }}
                    stroke="hsl(var(--muted-foreground))"
                  />
                  <YAxis
                    tick={{ fontSize: 11 }}
                    stroke="hsl(var(--muted-foreground))"
                  />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: "hsl(var(--card))",
                      border: "1px solid hsl(var(--border))",
                      borderRadius: "6px",
                    }}
                    formatter={(value, name) => {
                      if (name === "spend") return [fmtSpend(value as number), "Spend"];
                      if (name === "tokens") return [fmtTokens(value as number), "Tokens"];
                      if (name === "requests") return [value, "Requests"];
                      return [value, name];
                    }}
                  />
                  <Bar dataKey="spend" fill="hsl(var(--primary))" radius={[4, 4, 0, 0]} />
                </BarChart>
              </ResponsiveContainer>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Model / Provider Charts */}
      <div className="grid gap-4 lg:grid-cols-2">
        {/* Model Bar Chart */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Spend by Model</CardTitle>
            <BarChart3 className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {modelLoading ? (
              <Skeleton className="h-64 w-full" />
            ) : modelChartData.length === 0 ? (
              <div className="flex items-center justify-center h-64 text-sm text-muted-foreground">
                No data available
              </div>
            ) : (
              <div className="h-[200px] md:h-[280px]">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={modelChartData} margin={{ top: 5, right: 20, left: 10, bottom: 5 }}>
                    <CartesianGrid strokeDasharray="3 3" className="stroke-muted" />
                    <XAxis
                      dataKey="model"
                      tick={{ fontSize: 11 }}
                      stroke="hsl(var(--muted-foreground))"
                    />
                    <YAxis
                      tick={{ fontSize: 11 }}
                      stroke="hsl(var(--muted-foreground))"
                    />
                    <Tooltip
                      contentStyle={{
                        backgroundColor: "hsl(var(--card))",
                        border: "1px solid hsl(var(--border))",
                        borderRadius: "6px",
                      }}
                      formatter={(value) => [fmtSpend(value as number), "Spend"]}
                    />
                    <Bar dataKey="total_spend" name="Spend" fill="hsl(var(--primary))" radius={[4, 4, 0, 0]} />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            )}
          </CardContent>
        </Card>

        {/* Provider Donut Chart */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
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
              <div className="h-[200px] md:h-[280px]">
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
                      }}
                      formatter={(value) => [fmtSpend(value as number), "Spend"]}
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
