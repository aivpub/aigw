import { useState, useMemo } from "react";
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
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Search,
  ChevronDown,
  ChevronRight,
  Box,
} from "lucide-react";
import type { ReactNode } from "react";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface ModelItem {
  model_id: string;
  model_name: string;
  litellm_params: Record<string, unknown>;
  model_info: Record<string, unknown>;
  created_at: string;
  created_by: string | null;
  updated_at: string;
  updated_by: string | null;
}

interface ModelListResponse {
  object: string;
  data: ModelItem[];
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function extractProvider(params: Record<string, unknown>): string {
  if (typeof params.model === "string") {
    const parts = params.model.split("/");
    return parts.length > 1 ? parts[0] : params.model;
  }
  return "—";
}

function extractModelType(params: Record<string, unknown>): string {
  if (typeof params.model === "string") {
    const parts = params.model.split("/");
    return parts.length > 1 ? parts[1] : params.model;
  }
  return "—";
}

function isActive(info: Record<string, unknown>): boolean {
  const mode = info.mode;
  return mode !== "inactive" && mode !== "disabled";
}

function renderJsonValue(value: unknown): ReactNode {
  if (value === null || value === undefined) {
    return <span className="text-muted-foreground italic">null</span>;
  }
  if (typeof value === "boolean") {
    return (
      <span className={value ? "text-green-600" : "text-red-600"}>
        {String(value)}
      </span>
    );
  }
  if (typeof value === "number") {
    return <span className="text-blue-600">{value}</span>;
  }
  return String(value);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Component
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function ModelsPage() {
  const [search, setSearch] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const { data, isLoading, error } = useQuery<ModelListResponse>({
    queryKey: ["proxy-models"],
    queryFn: () => apiGet("/model/list"),
  });

  const models = data?.data ?? [];

  const filteredModels = useMemo(() => {
    if (!search.trim()) return models;
    const q = search.toLowerCase();
    return models.filter(
      (m) =>
        m.model_name.toLowerCase().includes(q) ||
        m.model_id.toLowerCase().includes(q) ||
        extractProvider(m.litellm_params).toLowerCase().includes(q),
    );
  }, [models, search]);

  function toggleExpand(id: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Models</h1>
        <p className="text-sm text-muted-foreground">
          Proxy model configurations and upstream mappings
        </p>
      </div>

      <div className="relative">
        <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          placeholder="Search by name, provider..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="pl-9 max-w-sm"
        />
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2">
            <Box className="h-4 w-4" />
            All Models ({filteredModels.length})
          </CardTitle>
        </CardHeader>
        <CardContent>
          {error ? (
            <p className="text-sm text-destructive">
              {(error as Error).message}
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-8" />
                  <TableHead>Model Name</TableHead>
                  <TableHead>Provider</TableHead>
                  <TableHead>Upstream Model</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Created</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoading
                  ? Array.from({ length: 3 }).map((_, i) => (
                      <TableRow key={i}>
                        {Array.from({ length: 6 }).map((_, j) => (
                          <TableCell key={j}>
                            <Skeleton className="h-4 w-full" />
                          </TableCell>
                        ))}
                      </TableRow>
                    ))
                  : filteredModels.map((model) => {
                      const active = isActive(model.model_info);
                      const provider = extractProvider(model.litellm_params);
                      const upstream = extractModelType(model.litellm_params);
                      const isExpanded = expanded.has(model.model_id);

                      return (
                        <>
                          <TableRow
                            key={model.model_id}
                            className="cursor-pointer hover:bg-muted/50"
                            onClick={() => toggleExpand(model.model_id)}
                          >
                            <TableCell>
                              {isExpanded ? (
                                <ChevronDown className="h-4 w-4" />
                              ) : (
                                <ChevronRight className="h-4 w-4" />
                              )}
                            </TableCell>
                            <TableCell className="font-mono text-sm font-medium">
                              {model.model_name}
                            </TableCell>
                            <TableCell className="text-sm">{provider}</TableCell>
                            <TableCell className="text-sm text-muted-foreground">
                              {upstream}
                            </TableCell>
                            <TableCell>
                              <Badge variant={active ? "default" : "secondary"}>
                                {active ? "active" : "inactive"}
                              </Badge>
                            </TableCell>
                            <TableCell className="text-xs text-muted-foreground">
                              {model.created_at
                                ? new Date(model.created_at).toLocaleDateString()
                                : "—"}
                            </TableCell>
                          </TableRow>
                          {isExpanded && (
                            <TableRow key={`${model.model_id}-detail`}>
                              <TableCell colSpan={6} className="bg-muted/30 p-4">
                                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                                  {/* litellm_params */}
                                  <div>
                                    <h4 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
                                      litellm_params
                                    </h4>
                                    <div className="rounded-md border bg-card p-3 font-mono text-xs leading-relaxed overflow-auto max-h-64">
                                      {Object.entries(model.litellm_params).length === 0
                                        ? "(empty)"
                                        : Object.entries(model.litellm_params).map(
                                            ([key, value]) => (
                                              <div key={key} className="flex gap-2">
                                                <span className="text-muted-foreground shrink-0">
                                                  {key}:
                                                </span>
                                                <span>{renderJsonValue(value)}</span>
                                              </div>
                                            ),
                                          )}
                                    </div>
                                  </div>

                                  {/* model_info */}
                                  <div>
                                    <h4 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
                                      model_info
                                    </h4>
                                    <div className="rounded-md border bg-card p-3 font-mono text-xs leading-relaxed overflow-auto max-h-64">
                                      {Object.entries(model.model_info).length === 0
                                        ? "(empty)"
                                        : Object.entries(model.model_info).map(
                                            ([key, value]) => (
                                              <div key={key} className="flex gap-2">
                                                <span className="text-muted-foreground shrink-0">
                                                  {key}:
                                                </span>
                                                <span>{renderJsonValue(value)}</span>
                                              </div>
                                            ),
                                          )}
                                    </div>
                                  </div>
                                </div>

                                {/* Metadata row */}
                                <div className="mt-3 flex gap-4 text-xs text-muted-foreground">
                                  <span>
                                    ID:{" "}
                                    <code className="text-foreground">
                                      {model.model_id}
                                    </code>
                                  </span>
                                  {model.created_by && (
                                    <span>
                                      Created by:{" "}
                                      <span className="text-foreground">
                                        {model.created_by}
                                      </span>
                                    </span>
                                  )}
                                  {model.updated_by && (
                                    <span>
                                      Updated by:{" "}
                                      <span className="text-foreground">
                                        {model.updated_by}
                                      </span>
                                    </span>
                                  )}
                                </div>
                              </TableCell>
                            </TableRow>
                          )}
                        </>
                      );
                    })}
                {!isLoading && filteredModels.length === 0 && (
                  <TableRow>
                    <TableCell
                      colSpan={6}
                      className="text-center text-muted-foreground py-8"
                    >
                      {search ? "No models match your search" : "No models configured"}
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
