import { useState, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
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
} from "lucide-react";
import { format } from "date-fns";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface SpendLog {
  request_id: string;
  call_type: string;
  api_key: string;
  spend: number;
  total_tokens: number;
  prompt_tokens: number;
  completion_tokens: number;
  start_time: string;
  end_time: string;
  model: string;
  user: string;
  request_tags: unknown;
  status: string;
}

interface SpendLogsResponse {
  data: SpendLog[];
  count: number;
  total_count: number;
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

interface SpendResponse {
  spend: number;
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

function todayStr(): string {
  return format(new Date(), "yyyy-MM-dd");
}

function monthStartStr(): string {
  return format(new Date(), "yyyy-MM-01");
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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Component
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function UsagePage() {
  const [startDate, setStartDate] = useState(monthStartStr());
  const [endDate, setEndDate] = useState(todayStr());

  // Total global spend
  const { data: totalSpend, isLoading: totalLoading } = useQuery<SpendResponse>({
    queryKey: ["global-spend"],
    queryFn: () => apiGet("/global/spend"),
    refetchInterval: 30_000,
  });

  // Spend logs (for period spend calculation)
  const { data: logsData, isLoading: logsLoading } = useQuery<SpendLogsResponse>({
    queryKey: ["global-spend-logs", startDate, endDate],
    queryFn: () =>
      apiGet(
        `/global/spend/logs?start_date=${startDate}&end_date=${endDate}&limit=100`,
      ),
    refetchInterval: 30_000,
  });

  // Model aggregation
  const { data: modelData, isLoading: modelLoading } = useQuery<AggResponse>({
    queryKey: ["spend-models"],
    queryFn: () => apiGet("/global/spend/models"),
    refetchInterval: 30_000,
  });

  // Provider aggregation
  const { data: providerData, isLoading: providerLoading } = useQuery<AggResponse>({
    queryKey: ["spend-providers"],
    queryFn: () => apiGet("/spend/providers"),
    refetchInterval: 30_000,
  });

  // Compute period spend from logs
  const periodSpend = useMemo(() => {
    if (!logsData?.data) return 0;
    return logsData.data.reduce((sum, l) => sum + l.spend, 0);
  }, [logsData]);

  const modelChartData = useMemo(() => {
    if (!modelData?.data) return [];
    return (modelData.data as ModelAgg[]).map((a) => ({
      name: a.model,
      spend: Math.round(a.total_spend * 10000) / 10000,
      tokens: a.total_tokens,
      requests: a.requests,
    }));
  }, [modelData]);

  const providerChartData = useMemo(() => {
    if (!providerData?.data) return [];
    return (providerData.data as ProviderAgg[]).map((a) => ({
      name: a.provider,
      value: Math.round(a.total_spend * 10000) / 10000,
      tokens: a.total_tokens,
    }));
  }, [providerData]);

  const isLoading = totalLoading || logsLoading || modelLoading || providerLoading;

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Usage</h1>
        <p className="text-sm text-muted-foreground">
          Usage and spend overview
        </p>
      </div>

      {/* Spend Cards */}
      <div className="grid gap-4 grid-cols-2 md:grid-cols-3">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Total Spend</CardTitle>
            <DollarSign className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {totalLoading ? (
              <Skeleton className="h-8 w-24" />
            ) : (
              <div className="text-2xl font-bold">
                {fmtSpend(totalSpend?.spend ?? 0)}
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium">Period Spend</CardTitle>
            <TrendingUp className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <Skeleton className="h-8 w-24" />
            ) : (
              <div className="text-2xl font-bold">{fmtSpend(periodSpend)}</div>
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
            {isLoading ? (
              <Skeleton className="h-8 w-16" />
            ) : (
              <div className="text-2xl font-bold">{logsData?.total_count ?? 0}</div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Charts Row */}
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
                      dataKey="name"
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
                    <Bar dataKey="spend" fill="hsl(var(--primary))" radius={[4, 4, 0, 0]} />
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
