import { useState, useMemo, useCallback } from "react";
import { useSearchParams } from "react-router-dom";
import { format } from "date-fns";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPut, apiDelete } from "@/lib/api";
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
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Search,
  ChevronDown,
  ChevronRight,
  Box,
  Plus,
  Pencil,
  Trash2,
} from "lucide-react";
import type { ReactNode } from "react";
import type { ModelItem, ModelListResponse } from "./types";
import { ModelDialog } from "./ModelDialog";
import { DeleteConfirm } from "./DeleteConfirm";
import { CredentialsTab } from "./CredentialsTab";
import { HealthTab } from "./HealthTab";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function extractProvider(params: Record<string, unknown>): string {
  if (typeof params.custom_llm_provider === "string") {
    return params.custom_llm_provider;
  }
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

function extractCost(info: Record<string, unknown>): { input: number | null; output: number | null } {
  const input = typeof info.input_cost_per_token === "number"
    ? info.input_cost_per_token * 1_000_000
    : null;
  const output = typeof info.output_cost_per_token === "number"
    ? info.output_cost_per_token * 1_000_000
    : null;
  return { input, output };
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Component
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function ModelsPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const tab = searchParams.get("tab") || "model-groups";
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  // Dialog state
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingModel, setEditingModel] = useState<ModelItem | null>(null);

  // Delete state
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deletingModel, setDeletingModel] = useState<ModelItem | null>(null);
  const [deleteLoading, setDeleteLoading] = useState(false);
  const [viewMode, setViewMode] = useState<"active" | "deleted">("active");

  // Error toast
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

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

  interface DeletedModelItem {
    id: number;
    model_id: string;
    model_name: string;
    litellm_params: Record<string, unknown>;
    deleted_at: string;
  }

  const { data: deletedModels = [], isLoading: deletedLoading } = useQuery<DeletedModelItem[]>({
    queryKey: ["deleted-models"],
    queryFn: () => apiGet("/model/deleted"),
    enabled: viewMode === "deleted",
  });

  function formatDate(d: string) {
    try { return format(new Date(d), "yyyy-MM-dd HH:mm"); } catch { return d; }
  }

  function toggleExpand(id: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  const handleAdd = useCallback(() => {
    setEditingModel(null);
    setDialogOpen(true);
  }, []);

  const handleEdit = useCallback((model: ModelItem, e: React.MouseEvent) => {
    e.stopPropagation();
    setEditingModel(model);
    setDialogOpen(true);
  }, []);

  const handleDeleteClick = useCallback((model: ModelItem, e: React.MouseEvent) => {
    e.stopPropagation();
    setDeletingModel(model);
    setDeleteOpen(true);
  }, []);

  const handleDeleteConfirm = useCallback(async () => {
    if (!deletingModel) return;
    setDeleteLoading(true);
    try {
      await apiDelete(`/model/delete?model_id=${encodeURIComponent(deletingModel.model_id)}`);
      queryClient.invalidateQueries({ queryKey: ["proxy-models"] });
      queryClient.invalidateQueries({ queryKey: ["deleted-models"] });
      setDeleteOpen(false);
      setDeletingModel(null);
    } catch (err) {
      setErrorMsg((err as Error).message);
    } finally {
      setDeleteLoading(false);
    }
  }, [deletingModel, queryClient]);

  const handleSaved = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: ["proxy-models"] });
  }, [queryClient]);

  const handleError = useCallback((msg: string) => {
    setErrorMsg(msg);
    setTimeout(() => setErrorMsg(null), 5000);
  }, []);

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Models</h1>
          <p className="text-sm text-muted-foreground">
            Proxy model configurations, credentials, and system health
          </p>
        </div>
        <Tabs
          defaultValue={tab}
          value={tab}
          onValueChange={(v) => setSearchParams({ tab: v }, { replace: true })}
        >
          <TabsList>
            <TabsTrigger value="model-groups">Model Groups</TabsTrigger>
            <TabsTrigger value="credentials">Credentials</TabsTrigger>
            <TabsTrigger value="health">Health</TabsTrigger>
          </TabsList>
        </Tabs>
      </div>

      <Tabs
        defaultValue={tab}
        value={tab}
        onValueChange={(v) => setSearchParams({ tab: v }, { replace: true })}
      >
        <TabsContent value="model-groups" className="mt-0 space-y-4">

      {/* Error toast */}
      {errorMsg && (
        <div className="rounded-md bg-destructive/10 border border-destructive/30 px-4 py-2 text-sm text-destructive flex items-center justify-between">
          <span>{errorMsg}</span>
          <Button variant="ghost" size="icon" className="h-5 w-5 text-destructive" onClick={() => setErrorMsg(null)}>
            ×
          </Button>
        </div>
      )}

      {error && (
        <div className="rounded-md bg-destructive/10 border border-destructive/30 px-4 py-2 text-sm text-destructive">
          {(error as Error).message}
        </div>
      )}

      <div className="flex items-center gap-2">
        {viewMode === "active" && (
          <div className="relative flex-1 max-w-sm">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder="Search by name, provider..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-9"
            />
          </div>
        )}
        <div className="flex-1" />
        <div className="flex items-center rounded-md border p-0.5">
          <button type="button" onClick={() => setViewMode("active")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${viewMode === "active" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground"}`}>Active</button>
          <button type="button" onClick={() => setViewMode("deleted")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${viewMode === "deleted" ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground"}`}>Deleted</button>
        </div>
        {viewMode === "active" && (
          <div className="flex items-center gap-1 ml-2">
            <Button size="sm" onClick={handleAdd}>
              <Plus className="h-4 w-4" /> Add Model
            </Button>
          </div>
        )}
      </div>

      {/* Deleted view */}
      {viewMode === "deleted" && (
        <Card>
          <CardHeader className="pb-2"><CardTitle>Deleted Models ({deletedModels.length})</CardTitle></CardHeader>
          <CardContent>
            {deletedLoading ? (
              Array.from({ length: 3 }).map((_, i) => <div key={i} className="py-2"><Skeleton className="h-4 w-full" /></div>)
            ) : deletedModels.length === 0 ? (
              <div className="text-center text-muted-foreground py-8">No deleted records</div>
            ) : (
              <>
                <div className="hidden md:block">
                  <Table>
                    <TableHeader><TableRow>
                      <TableHead>Model Name</TableHead>
                      <TableHead>Model ID</TableHead>
                      <TableHead>Provider</TableHead>
                      <TableHead className="text-right">Deleted At</TableHead>
                    </TableRow></TableHeader>
                    <TableBody>
                      {deletedModels.map((m) => {
                        const provider = typeof m.litellm_params?.custom_llm_provider === "string"
                          ? m.litellm_params.custom_llm_provider : typeof m.litellm_params?.model === "string"
                          ? m.litellm_params.model.split("/")[0] || "—" : "—";
                        return (
                          <TableRow key={m.id}>
                            <TableCell className="font-medium">{m.model_name}</TableCell>
                            <TableCell className="text-sm font-mono">{m.model_id}</TableCell>
                            <TableCell className="text-sm">{provider}</TableCell>
                            <TableCell className="text-right text-sm text-muted-foreground">{formatDate(m.deleted_at)}</TableCell>
                          </TableRow>
                        );
                      })}
                    </TableBody>
                  </Table>
                </div>
                <div className="md:hidden space-y-3">
                  {deletedModels.map((m) => {
                    const provider = typeof m.litellm_params?.custom_llm_provider === "string"
                      ? m.litellm_params.custom_llm_provider : typeof m.litellm_params?.model === "string"
                      ? m.litellm_params.model.split("/")[0] || "—" : "—";
                    return (
                      <Card key={m.id}><CardContent className="p-4 space-y-2">
                        <div className="font-medium text-sm">{m.model_name}</div>
                        <div className="text-xs text-muted-foreground">ID: {m.model_id} | Provider: {provider}</div>
                        <div className="text-xs text-muted-foreground">Deleted: {formatDate(m.deleted_at)}</div>
                      </CardContent></Card>
                    );
                  })}
                </div>
              </>
            )}
          </CardContent>
        </Card>
      )}

      {viewMode === "active" && (
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="flex items-center gap-2">
            <Box className="h-4 w-4" />
            All Models ({filteredModels.length})
          </CardTitle>
        </CardHeader>
        <CardContent>
          {/* Desktop table */}
          <div className="hidden md:block">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-8" />
                  <TableHead>Model Name</TableHead>
                  <TableHead>Provider</TableHead>
                  <TableHead>Upstream Model</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Cost (Per 1M)</TableHead>
                  <TableHead>Created</TableHead>
                  <TableHead className="w-20">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoading
                  ? Array.from({ length: 3 }).map((_, i) => (
                      <TableRow key={i}>
                        {Array.from({ length: 9 }).map((_, j) => (
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
                      const cost = extractCost(model.model_info);
                      const isExpanded = expanded.has(model.model_id);

                      return (
                        <>
                          <TableRow
                            key={model.model_id}
                            className="cursor-pointer hover:bg-muted/50"
                            onClick={() => toggleExpand(model.model_id)}
                            data-model-name={model.model_name}
                          >
                            <TableCell>
                              {isExpanded ? (
                                <ChevronDown className="h-4 w-4" />
                              ) : (
                                <ChevronRight className="h-4 w-4" />
                              )}
                            </TableCell>
                            <TableCell className="font-mono text-sm font-medium max-w-[180px] truncate">
                              {model.model_name}
                            </TableCell>
                            <TableCell className="text-sm">{provider}</TableCell>
                            <TableCell className="text-sm text-muted-foreground max-w-[160px] truncate" title={upstream}>
                              {upstream}
                            </TableCell>
                            <TableCell>
                              <div className="flex items-center gap-2">
                                <Switch
                                  checked={active}
                                  onCheckedChange={async (checked) => {
                                    try {
                                      await apiPut("/model/update", {
                                        model_id: model.model_id,
                                        model_info: {
                                          ...model.model_info,
                                          mode: checked ? undefined : "inactive",
                                        },
                                      });
                                      queryClient.invalidateQueries({ queryKey: ["proxy-models"] });
                                    } catch (err) {
                                      setErrorMsg((err as Error).message);
                                    }
                                  }}
                                />
                                <Badge variant={active ? "default" : "secondary"}>
                                  {active ? "active" : "inactive"}
                                </Badge>
                              </div>
                            </TableCell>
                            <TableCell>
                              {cost.input !== null ? (
                                <div className="text-xs leading-snug">
                                  <div>
                                    <span className="text-muted-foreground">$</span>
                                    {cost.input.toFixed(4)}{" "}
                                    <span className="text-muted-foreground">Input</span>
                                  </div>
                                  {cost.output !== null && (
                                    <div>
                                      <span className="text-muted-foreground">$</span>
                                      {cost.output.toFixed(4)}{" "}
                                      <span className="text-muted-foreground">Output</span>
                                    </div>
                                  )}
                                </div>
                              ) : (
                                <span className="text-sm text-muted-foreground">—</span>
                              )}
                            </TableCell>
                            <TableCell className="text-xs text-muted-foreground">
                              {model.created_at
                                ? new Date(model.created_at).toLocaleDateString()
                                : "—"}
                            </TableCell>
                            <TableCell>
                              <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-7 w-7"
                                  onClick={(e) => handleEdit(model, e)}
                                >
                                  <Pencil className="h-3.5 w-3.5" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-7 w-7 text-destructive hover:text-destructive"
                                  onClick={(e) => handleDeleteClick(model, e)}
                                >
                                  <Trash2 className="h-3.5 w-3.5" />
                                </Button>
                              </div>
                            </TableCell>
                          </TableRow>
                          {isExpanded && (
                            <TableRow key={`${model.model_id}-detail`}>
                              <TableCell colSpan={8} className="bg-muted/30 p-4">
                                <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
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
                      colSpan={9}
                      className="text-center text-muted-foreground py-8"
                    >
                      {search ? "No models match your search" : "No models configured"}
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>

          {/* Mobile card list */}
          <div className="md:hidden space-y-3">
            {isLoading
              ? Array.from({ length: 3 }).map((_, i) => (
                  <Card key={i}>
                    <CardContent className="p-4 space-y-2">
                      <Skeleton className="h-4 w-3/4" />
                      <Skeleton className="h-4 w-full" />
                      <Skeleton className="h-4 w-1/2" />
                    </CardContent>
                  </Card>
                ))
              : filteredModels.map((model) => {
                  const active = isActive(model.model_info);
                  const provider = extractProvider(model.litellm_params);
                  const upstream = extractModelType(model.litellm_params);
                  const cost = extractCost(model.model_info);
                  const isExpanded = expanded.has(model.model_id);

                  return (
                    <Card key={model.model_id}>
                      <CardContent className="p-4 space-y-2">
                        <div
                          className="flex items-center justify-between cursor-pointer"
                          onClick={() => toggleExpand(model.model_id)}
                        >
                          <div className="flex items-center gap-2 min-w-0">
                            {isExpanded ? (
                              <ChevronDown className="h-4 w-4 shrink-0" />
                            ) : (
                              <ChevronRight className="h-4 w-4 shrink-0" />
                            )}
                            <span className="font-mono text-sm font-medium truncate">
                              {model.model_name}
                            </span>
                          </div>
                          <Badge variant={active ? "default" : "secondary"} className="text-xs shrink-0 ml-2">
                            {active ? "active" : "inactive"}
                          </Badge>
                        </div>
                        <div className="flex items-center justify-between text-xs text-muted-foreground">
                          <span>Provider: {provider}</span>
                          <span>Upstream: {upstream}</span>
                        </div>
                        {cost.input !== null && (
                          <div className="text-xs text-muted-foreground">
                            Cost: ${cost.input.toFixed(4)} Input
                            {cost.output !== null && ` / $${cost.output.toFixed(4)} Output`}
                          </div>
                        )}
                        <div className="flex items-center justify-between">
                          <span className="text-xs text-muted-foreground">
                            {model.created_at
                              ? `Created ${new Date(model.created_at).toLocaleDateString()}`
                              : "—"}
                          </span>
                          <div className="flex items-center gap-1">
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7"
                              onClick={(e) => handleEdit(model, e)}
                            >
                              <Pencil className="h-3.5 w-3.5" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7 text-destructive hover:text-destructive"
                              onClick={(e) => handleDeleteClick(model, e)}
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                          </div>
                        </div>
                        {isExpanded && (
                          <div className="space-y-3 pt-2 border-t">
                            <div>
                              <h4 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-1">
                                litellm_params
                              </h4>
                              <div className="rounded-md border bg-muted/30 p-2 font-mono text-xs leading-relaxed max-h-40 overflow-auto">
                                {Object.entries(model.litellm_params).length === 0
                                  ? "(empty)"
                                  : Object.entries(model.litellm_params).map(
                                      ([key, value]) => (
                                        <div key={key} className="flex gap-2">
                                          <span className="text-muted-foreground shrink-0">{key}:</span>
                                          <span className="break-all">{renderJsonValue(value)}</span>
                                        </div>
                                      ),
                                    )}
                              </div>
                            </div>
                            <div>
                              <h4 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-1">
                                model_info
                              </h4>
                              <div className="rounded-md border bg-muted/30 p-2 font-mono text-xs leading-relaxed max-h-40 overflow-auto">
                                {Object.entries(model.model_info).length === 0
                                  ? "(empty)"
                                  : Object.entries(model.model_info).map(
                                      ([key, value]) => (
                                        <div key={key} className="flex gap-2">
                                          <span className="text-muted-foreground shrink-0">{key}:</span>
                                          <span className="break-all">{renderJsonValue(value)}</span>
                                        </div>
                                      ),
                                    )}
                              </div>
                            </div>
                            <div className="text-xs text-muted-foreground">
                              ID: <code className="text-foreground break-all">{model.model_id}</code>
                            </div>
                          </div>
                        )}
                      </CardContent>
                    </Card>
                  );
                })}
            {!isLoading && filteredModels.length === 0 && (
              <div className="text-center text-muted-foreground py-8">
                {search ? "No models match your search" : "No models configured"}
              </div>
            )}
          </div>
        </CardContent>
      </Card>
      )}

      {/* Model Dialog (Add/Edit) */}
      <ModelDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        model={editingModel}
        onSaved={handleSaved}
        onError={handleError}
      />

      {/* Delete Confirmation */}
      <DeleteConfirm
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        modelName={deletingModel?.model_name ?? ""}
        onConfirm={handleDeleteConfirm}
        loading={deleteLoading}
      />

        </TabsContent>

        <TabsContent value="credentials" className="mt-0">
          <CredentialsTab />
        </TabsContent>

        <TabsContent value="health" className="mt-0">
          <HealthTab />
        </TabsContent>
      </Tabs>
    </div>
  );
}
