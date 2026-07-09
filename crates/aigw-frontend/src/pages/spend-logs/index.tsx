import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
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
import { ScrollText, Calendar, RefreshCw } from "lucide-react";
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

function monthStartStr(): string {
  return format(new Date(), "yyyy-MM-01");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Component
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function SpendLogsPage() {
  const [startDate, setStartDate] = useState(monthStartStr());
  const [endDate, setEndDate] = useState(todayStr());
  const [modelFilter, setModelFilter] = useState("");

  const {
    data: logsData,
    isLoading,
    isError,
    refetch,
  } = useQuery<SpendLogsResponse>({
    queryKey: ["global-spend-logs", startDate, endDate, modelFilter],
    queryFn: () => {
      let url = `/global/spend/logs?start_date=${startDate}&end_date=${endDate}&limit=100`;
      if (modelFilter.trim()) url += `&model=${encodeURIComponent(modelFilter.trim())}`;
      return apiGet(url);
    },
    refetchInterval: 30_000,
  });

  const logs = logsData?.data ?? [];

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Spend Logs</h1>
        <p className="text-sm text-muted-foreground">
          Detailed request log with cost and token breakdown
        </p>
      </div>

      {/* Filters */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium flex items-center gap-2">
            <Calendar className="h-4 w-4" />
            Filters
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex flex-col sm:flex-row gap-3 items-start sm:items-end">
            <div className="flex flex-col gap-1.5">
              <Label className="text-xs" htmlFor="sl-start">Start Date</Label>
              <Input
                id="sl-start"
                type="date"
                value={startDate}
                onChange={(e) => setStartDate(e.target.value)}
                className="h-8 w-36 text-xs"
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label className="text-xs" htmlFor="sl-end">End Date</Label>
              <Input
                id="sl-end"
                type="date"
                value={endDate}
                onChange={(e) => setEndDate(e.target.value)}
                className="h-8 w-36 text-xs"
              />
            </div>
            <div className="flex flex-col gap-1.5 flex-1 sm:max-w-xs">
              <Label className="text-xs" htmlFor="sl-model">Model</Label>
              <Input
                id="sl-model"
                type="text"
                placeholder="gpt-4 (optional)"
                value={modelFilter}
                onChange={(e) => setModelFilter(e.target.value)}
                className="h-8 text-xs"
              />
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={() => refetch()}
              className="h-8"
            >
              <RefreshCw className="h-3.5 w-3.5 mr-1" />
              Refresh
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* Results */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between pb-2">
          <CardTitle className="text-sm font-medium flex items-center gap-2">
            <ScrollText className="h-4 w-4" />
            Requests ({logsData?.count ?? 0})
          </CardTitle>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-8 w-full" />
              ))}
            </div>
          ) : isError ? (
            <div className="flex flex-col items-center justify-center h-32 gap-2">
              <p className="text-sm text-muted-foreground">
                Failed to load spend logs
              </p>
              <Button variant="outline" size="sm" onClick={() => refetch()}>
                Retry
              </Button>
            </div>
          ) : logs.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-32 gap-1">
              <p className="text-sm text-muted-foreground">
                No spend logs found
              </p>
              <p className="text-xs text-muted-foreground">
                Try adjusting the date range or model filter
              </p>
            </div>
          ) : (
            <>
              {/* Desktop table */}
              <div className="hidden md:block">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Time</TableHead>
                      <TableHead>Model</TableHead>
                      <TableHead className="text-right">Tokens</TableHead>
                      <TableHead className="text-right">Cost</TableHead>
                      <TableHead>Status</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {logs.map((log) => (
                      <TableRow key={log.request_id}>
                        <TableCell className="text-xs">
                          {log.start_time
                            ? format(new Date(log.start_time), "MM-dd HH:mm")
                            : "—"}
                        </TableCell>
                        <TableCell className="text-sm">{log.model}</TableCell>
                        <TableCell className="text-right text-sm">
                          {fmtTokens(log.total_tokens)}
                        </TableCell>
                        <TableCell className="text-right text-sm font-mono">
                          {fmtSpend(log.spend)}
                        </TableCell>
                        <TableCell>
                          <span
                            className={`inline-flex items-center rounded-md px-2 py-0.5 text-xs font-medium ${
                              log.status === "success"
                                ? "bg-green-50 text-green-700 dark:bg-green-950 dark:text-green-400"
                                : "bg-red-50 text-red-700 dark:bg-red-950 dark:text-red-400"
                            }`}
                          >
                            {log.status || "—"}
                          </span>
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
                    className="flex items-center justify-between rounded-md border p-3"
                  >
                    <div className="space-y-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium truncate">
                          {log.model}
                        </span>
                        <span
                          className={`inline-flex items-center rounded-md px-1.5 py-0.5 text-xs font-medium ${
                            log.status === "success"
                              ? "bg-green-50 text-green-700 dark:bg-green-950 dark:text-green-400"
                              : "bg-red-50 text-red-700 dark:bg-red-950 dark:text-red-400"
                          }`}
                        >
                          {log.status || "—"}
                        </span>
                      </div>
                      <div className="text-xs text-muted-foreground">
                        {log.start_time
                          ? format(new Date(log.start_time), "MM-dd HH:mm")
                          : "—"}
                      </div>
                    </div>
                    <div className="text-right shrink-0 ml-3">
                      <div className="text-sm font-mono font-medium">
                        {fmtSpend(log.spend)}
                      </div>
                      <div className="text-xs text-muted-foreground">
                        {fmtTokens(log.total_tokens)} tokens
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
