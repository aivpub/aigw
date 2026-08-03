import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPost, apiPut, apiDelete } from "@/lib/api";
import { useAuth } from "@/hooks/use-auth";
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
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { PaginationBar } from "@/components/ui/pagination";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { MultiModelSelect } from "@/components/ui/multi-model-select";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { toast } from "sonner";
import { Plus, Search, Pencil, Trash2, Copy, Eye, EyeOff } from "lucide-react";
import { format } from "date-fns";

interface KeyItem {
  token: string;
  key_name: string | null;
  key_alias: string | null;
  user_id: string | null;
  user_email: string | null;
  user_alias: string | null;
  team_id: string | null;
  spend: number;
  max_budget: number | null;
  budget_duration: string | null;
  soft_budget: number | null;
  tpm_limit: number | null;
  rpm_limit: number | null;
  blocked: boolean | null;
  expires: string | null;
  models: string[];
  metadata: Record<string, unknown>;
  created_at: string | null;
  key?: string; // raw key, only present on generate response
}

interface DeletedKeyItem {
  token: string;
  key_alias: string | null;
  user_id: string | null;
  spend: number;
  key_name: string | null;
  blocked: boolean | null;
  updated_at: string | null;
}

interface KeyListResponse {
  keys?: KeyItem[];
  data?: KeyItem[];
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

interface DeletedKeyListResponse {
  keys?: DeletedKeyItem[];
  data?: DeletedKeyItem[];
  total_count?: number;
  page?: number;
  page_size?: number;
  total_pages?: number;
}

function maskToken(token: string): string {
  if (token.length <= 10) return token;
  return `${token.slice(0, 5)}...${token.slice(-4)}`;
}

export function KeysPage() {
  const { t } = useTranslation();
  const { userRole } = useAuth();
  const isAdmin = userRole === "proxy_admin";
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [visibleTokens, setVisibleTokens] = useState<Set<string>>(new Set());
  const [createOpen, setCreateOpen] = useState(false);
  const [editOpen, setEditOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [selectedKey, setSelectedKey] = useState<KeyItem | null>(null);
  const [generatedToken, setGeneratedToken] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<"active" | "deleted">("active");
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);
  const [deletedPage, setDeletedPage] = useState(1);
  const [deletedPageSize, setDeletedPageSize] = useState(30);

  // Form state
  const [formName, setFormName] = useState("");
  const [formAlias, setFormAlias] = useState("");
  const [selectedModels, setSelectedModels] = useState<string[]>([]);
  const [formBudget, setFormBudget] = useState("");
  const [formBudgetDuration, setFormBudgetDuration] = useState("");
  const [formSoftBudget, setFormSoftBudget] = useState("");
  const [formTPM, setFormTPM] = useState("");
  const [formRPM, setFormRPM] = useState("");
  const [formExpires, setFormExpires] = useState("");
  const [formUserId, setFormUserId] = useState("");

  const [userList, setUserList] = useState<Array<{ user_id: string; user_alias: string | null; user_email: string | null }>>([]);

  // Fetch user list for admin user_id selection
  useQuery({
    queryKey: ["user-list-for-keys"],
    queryFn: async () => {
      const resp = await apiGet<{ data: Array<{ user_id: string; user_alias: string | null; user_email: string | null }> }>("/user/list?page=1&page_size=500");
      setUserList(resp.data ?? []);
      return resp;
    },
    enabled: isAdmin,
  });

  const { data, isLoading, error } = useQuery<KeyListResponse>({
    queryKey: ["virtual-keys", page, pageSize],
    queryFn: () => apiGet(`/key/list?page=${page}&page_size=${pageSize}`),
  });

  const { data: deletedData, isLoading: deletedLoading } =
    useQuery<DeletedKeyListResponse>({
      queryKey: ["virtual-keys-deleted", deletedPage, deletedPageSize],
      queryFn: () =>
        apiGet(`/key/deleted?page=${deletedPage}&page_size=${deletedPageSize}`),
      enabled: viewMode === "deleted",
    });

  const keys = data?.keys ?? data?.data ?? [];
  const totalCount = data?.total_count ?? keys.length;
  const totalPages =
    data?.total_pages ??
    (keys.length === 0 ? 1 : Math.ceil(totalCount / pageSize));

  const deletedKeys = deletedData?.keys ?? deletedData?.data ?? [];
  const deletedTotalCount = deletedData?.total_count ?? deletedKeys.length;
  const deletedTotalPages =
    deletedData?.total_pages ??
    (deletedKeys.length === 0
      ? 1
      : Math.ceil(deletedTotalCount / deletedPageSize));

  const filteredKeys = useMemo(() => {
    if (!search.trim()) return keys;
    const q = search.toLowerCase();
    return keys.filter(
      (k) =>
        (k.key_alias ?? "").toLowerCase().includes(q) ||
        (k.key_name ?? "").toLowerCase().includes(q) ||
        (k.user_id ?? "").toLowerCase().includes(q) ||
        (k.user_email ?? "").toLowerCase().includes(q) ||
        (k.user_alias ?? "").toLowerCase().includes(q),
    );
  }, [keys, search]);

  const createMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) =>
      apiPost<KeyItem>("/key/generate", body),
    onSuccess: (resp) => {
      setGeneratedToken(resp.key ?? null);
      toast.success(t("keys.toast.created"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const editMutation = useMutation({
    mutationFn: (body: Record<string, unknown>) => apiPut("/key/update", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["virtual-keys"] });
      setEditOpen(false);
      setSelectedKey(null);
      toast.success(t("keys.toast.updated"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  const deleteMutation = useMutation({
    mutationFn: (token: string) =>
      apiDelete(`/key/delete?key=${encodeURIComponent(token)}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["virtual-keys"] });
      setDeleteOpen(false);
      setSelectedKey(null);
      toast.success(t("keys.toast.deleted"));
    },
    onError: (err: Error) => toast.error(err.message),
  });

  function toggleTokenVisible(token: string) {
    setVisibleTokens((prev) => {
      const next = new Set(prev);
      if (next.has(token)) next.delete(token);
      else next.add(token);
      return next;
    });
  }

  function copyToClipboard(text: string) {
    if (!text) return;
    try {
      if (typeof navigator.clipboard?.writeText === "function") {
        navigator.clipboard.writeText(text).then(
          () => toast.success(t("keys.toast.copied")),
          () => fallbackCopyToClipboard(text),
        );
      } else {
        fallbackCopyToClipboard(text);
      }
    } catch {
      fallbackCopyToClipboard(text);
    }

    function fallbackCopyToClipboard(textToCopy: string) {
      const textarea = document.createElement("textarea");
      textarea.value = textToCopy;
      textarea.style.position = "fixed";
      textarea.style.opacity = "0";
      document.body.appendChild(textarea);
      textarea.select();
      try {
        document.execCommand("copy");
        toast.success(t("keys.toast.copied"));
      } catch {
        toast.error(t("keys.toast.copyFailed"));
      }
      document.body.removeChild(textarea);
    }
  }

  function openCreate() {
    setFormName("");
    setFormAlias("");
    setSelectedModels([]);
    setFormBudget("");
    setFormBudgetDuration("");
    setFormSoftBudget("");
    setFormTPM("");
    setFormRPM("");
    setFormExpires("");
    setFormUserId("");
    setGeneratedToken(null);
    setCreateOpen(true);
  }

  function openEdit(key: KeyItem) {
    setSelectedKey(key);
    setFormName(key.key_name ?? "");
    setFormAlias(key.key_alias ?? "");
    if (Array.isArray(key.models) && key.models.length > 0) {
      setSelectedModels(key.models);
    } else {
      setSelectedModels(["*"]);
    }
    setFormBudget(key.max_budget?.toString() ?? "");
    setFormBudgetDuration(key.budget_duration ?? "");
    setFormSoftBudget(key.soft_budget?.toString() ?? "");
    setFormTPM(key.tpm_limit?.toString() ?? "");
    setFormRPM(key.rpm_limit?.toString() ?? "");
    setFormExpires(key.expires ?? "");
    setEditOpen(true);
  }

  function openDelete(key: KeyItem) {
    setSelectedKey(key);
    setDeleteOpen(true);
  }

  function formatDate(d: string) {
    try {
      return format(new Date(d), "yyyy-MM-dd HH:mm");
    } catch {
      return d;
    }
  }

  function buildCreateBody(): Record<string, unknown> {
    const body: Record<string, unknown> = {};
    if (formName.trim()) body.key_name = formName.trim();
    if (formAlias.trim()) body.key_alias = formAlias.trim();
    if (selectedModels.includes("*")) {
      body.models = [];
    } else if (selectedModels.length > 0) {
      body.models = selectedModels;
    }
    if (formBudget.trim()) body.max_budget = parseFloat(formBudget);
    if (formBudgetDuration.trim())
      body.budget_duration = formBudgetDuration.trim();
    if (formSoftBudget.trim()) body.soft_budget = parseFloat(formSoftBudget);
    if (formTPM.trim()) body.tpm_limit = parseInt(formTPM);
    if (formRPM.trim()) body.rpm_limit = parseInt(formRPM);
    if (formExpires.trim()) body.expires = formExpires.trim();
    if (isAdmin && formUserId.trim()) body.user_id = formUserId.trim();
    return body;
  }

  function handleCreate() {
    createMutation.mutate(buildCreateBody());
  }

  function handleEdit() {
    if (!selectedKey) return;
    editMutation.mutate({
      key: selectedKey.token,
      ...(formName.trim() && { key_name: formName.trim() }),
      ...(formAlias.trim() && { key_alias: formAlias.trim() }),
      ...(selectedModels.includes("*")
        ? { models: [] }
        : selectedModels.length > 0 && { models: selectedModels }),
      ...(formBudget.trim() && { max_budget: parseFloat(formBudget) }),
      ...(formBudgetDuration !== selectedKey?.budget_duration ? { budget_duration: formBudgetDuration.trim() } : undefined),
      ...(formSoftBudget.trim() && { soft_budget: parseFloat(formSoftBudget) }),
      ...(formTPM.trim() && { tpm_limit: parseInt(formTPM) }),
      ...(formRPM.trim() && { rpm_limit: parseInt(formRPM) }),
      ...(formExpires.trim() && { expires: formExpires.trim() }),
    });
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">
            {t("keys.title")}
          </h1>
          <p className="text-sm text-muted-foreground">
            {t("keys.description")}
          </p>
        </div>
        {viewMode === "active" && (
          <Button onClick={openCreate}>
            <Plus className="h-4 w-4" />
            {t("keys.newKey")}
          </Button>
        )}
      </div>

      <div className="flex items-center gap-2">
        {viewMode === "active" && (
          <div className="relative flex-1 max-w-sm">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder={t("keys.searchPlaceholder")}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="pl-9"
            />
          </div>
        )}
        <div className="flex-1" />
        <div className="flex items-center rounded-md border p-0.5">
          <button
            type="button"
            onClick={() => setViewMode("active")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${
              viewMode === "active"
                ? "bg-primary text-primary-foreground"
                : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {t("keys.viewMode.active")}
          </button>
          <button
            type="button"
            onClick={() => setViewMode("deleted")}
            className={`px-3 py-1 text-sm rounded-sm font-medium transition-colors ${
              viewMode === "deleted"
                ? "bg-primary text-primary-foreground"
                : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {t("keys.viewMode.deleted")}
          </button>
        </div>
      </div>

      {/* Deleted Keys View */}
      {viewMode === "deleted" && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle>
              {t("keys.deletedKeys", { count: deletedTotalCount })}
            </CardTitle>
          </CardHeader>
          <CardContent>
            {deletedLoading ? (
              Array.from({ length: 3 }).map((_, i) => (
                <div key={i} className="py-2">
                  <Skeleton className="h-4 w-full" />
                </div>
              ))
            ) : deletedKeys.length === 0 ? (
              <div className="text-center text-muted-foreground py-8">
                {t("keys.noDeletedRecords")}
              </div>
            ) : (
              <>
                <PaginationBar
                  page={deletedPage}
                  pageSize={deletedPageSize}
                  totalCount={deletedTotalCount}
                  totalPages={deletedTotalPages}
                  onPage={setDeletedPage}
                  onPageSize={(s) => {
                    setDeletedPageSize(s);
                    setDeletedPage(1);
                  }}
                />
                <div className="hidden md:block">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>{t("keys.table.alias")}</TableHead>
                        <TableHead>{t("keys.table.token")}</TableHead>
                        <TableHead>{t("keys.table.user")}</TableHead>
                        <TableHead className="text-right">
                          {t("keys.table.spend")}
                        </TableHead>
                        <TableHead>{t("keys.table.status")}</TableHead>
                        <TableHead className="text-right">
                          {t("keys.table.deletedAt")}
                        </TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {deletedKeys.map((k) => (
                        <TableRow key={k.token}>
                          <TableCell className="font-medium">
                            {k.key_alias ?? k.key_name ?? "—"}
                          </TableCell>
                          <TableCell className="font-mono text-xs">
                            {maskToken(k.token)}
                          </TableCell>
                          <TableCell className="text-sm">
                            {k.user_id ?? "—"}
                          </TableCell>
                          <TableCell className="text-right text-sm">
                            ${k.spend.toFixed(4)}
                          </TableCell>
                          <TableCell>
                            {k.blocked ? (
                              <Badge variant="destructive">
                                {t("keys.blocked")}
                              </Badge>
                            ) : (
                              <Badge variant="secondary">
                                {t("keys.active")}
                              </Badge>
                            )}
                          </TableCell>
                          <TableCell className="text-right text-sm text-muted-foreground">
                            {k.updated_at ? formatDate(k.updated_at) : "—"}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div className="md:hidden space-y-3">
                  {deletedKeys.map((k) => (
                    <Card key={k.token}>
                      <CardContent className="p-4 space-y-2">
                        <div className="flex items-center justify-between">
                          <span className="font-medium text-sm">
                            {k.key_alias ?? k.key_name ?? "—"}
                          </span>
                          <span className="text-xs text-muted-foreground">
                            ${k.spend.toFixed(4)}
                          </span>
                        </div>
                        <div className="text-xs font-mono text-muted-foreground">
                          {maskToken(k.token)}
                        </div>
                        <div className="text-xs text-muted-foreground">
                          User: {k.user_id ?? "—"} | Deleted:{" "}
                          {k.updated_at ? formatDate(k.updated_at) : "—"}
                        </div>
                      </CardContent>
                    </Card>
                  ))}
                </div>
                <div className="mt-3">
                  <PaginationBar
                    page={deletedPage}
                    pageSize={deletedPageSize}
                    totalCount={deletedTotalCount}
                    totalPages={deletedTotalPages}
                    onPage={setDeletedPage}
                    onPageSize={(s) => {
                      setDeletedPageSize(s);
                      setDeletedPage(1);
                    }}
                  />
                </div>
              </>
            )}
          </CardContent>
        </Card>
      )}

      {/* Active Keys View */}
      {viewMode === "active" && (
        <>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle>{t("keys.allKeys", { count: totalCount })}</CardTitle>
            </CardHeader>
            <CardContent>
              {error ? (
                <p className="text-sm text-destructive">
                  {(error as Error).message}
                </p>
              ) : (
                <>
                  <PaginationBar
                    page={page}
                    pageSize={pageSize}
                    totalCount={totalCount}
                    totalPages={totalPages}
                    onPage={setPage}
                    onPageSize={(s) => {
                      setPageSize(s);
                      setPage(1);
                    }}
                  />
                  {/* Desktop table */}
                  <div className="hidden md:block">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>{t("keys.table.alias")}</TableHead>
                          <TableHead>{t("keys.table.name")}</TableHead>
                          <TableHead>{t("keys.table.token")}</TableHead>
                          <TableHead>{t("keys.table.user")}</TableHead>
                          <TableHead>{t("keys.table.models")}</TableHead>
                          <TableHead className="text-right">
                            {t("keys.table.spend")}
                          </TableHead>
                          <TableHead className="text-right">
                            {t("keys.table.budget")}
                          </TableHead>
                          <TableHead className="text-right text-xs">
                            {t("keys.table.resetPeriod")}
                          </TableHead>
                          <TableHead>{t("keys.table.expires")}</TableHead>
                          <TableHead>{t("keys.table.created")}</TableHead>
                          <TableHead>{t("keys.table.status")}</TableHead>
                          <TableHead className="text-right">
                            {t("keys.table.actions")}
                          </TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {isLoading
                          ? Array.from({ length: 3 }).map((_, i) => (
                              <TableRow key={i}>
                                {Array.from({ length: 12 }).map((_, j) => (
                                  <TableCell key={j}>
                                    <Skeleton className="h-4 w-full" />
                                  </TableCell>
                                ))}
                              </TableRow>
                            ))
                          : filteredKeys.map((key) => (
                              <TableRow key={key.token}>
                                <TableCell className="font-medium">
                                  {key.key_alias ?? "—"}
                                </TableCell>
                                <TableCell className="text-sm">
                                  {key.key_name ?? "—"}
                                </TableCell>
                                <TableCell className="font-mono text-xs">
                                  <span className="inline-flex items-center gap-1">
                                    {visibleTokens.has(key.token)
                                      ? key.token
                                      : maskToken(key.token)}
                                    <button
                                      type="button"
                                      onClick={() =>
                                        toggleTokenVisible(key.token)
                                      }
                                      className="text-muted-foreground hover:text-foreground"
                                    >
                                      {visibleTokens.has(key.token) ? (
                                        <EyeOff className="h-3.5 w-3.5" />
                                      ) : (
                                        <Eye className="h-3.5 w-3.5" />
                                      )}
                                    </button>
                                    <button
                                      type="button"
                                      onClick={() => copyToClipboard(key.token)}
                                      className="text-muted-foreground hover:text-foreground"
                                    >
                                      <Copy className="h-3.5 w-3.5" />
                                    </button>
                                  </span>
                                </TableCell>
                                <TableCell className="text-sm">
                                  {key.user_id ? (
                                    <TooltipProvider>
                                      <Tooltip delayDuration={300}>
                                        <TooltipTrigger asChild>
                                          <span className="cursor-default underline decoration-dotted underline-offset-2">
                                            {key.user_email ??
                                              key.user_alias ??
                                              key.user_id}
                                          </span>
                                        </TooltipTrigger>
                                        <TooltipContent
                                          side="bottom"
                                          className="max-w-xs space-y-1.5 p-2.5 text-xs"
                                        >
                                          <div className="flex items-center justify-between gap-3">
                                            <span className="text-muted-foreground whitespace-nowrap">
                                              {t("keys.userTooltip.userId")}
                                            </span>
                                            <span className="flex items-center gap-1.5">
                                              <code className="text-[11px]">
                                                {key.user_id}
                                              </code>
                                              <button
                                                type="button"
                                                onClick={(e) => {
                                                  e.stopPropagation();
                                                  copyToClipboard(key.user_id!);
                                                }}
                                                className="text-muted-foreground hover:text-foreground"
                                              >
                                                <Copy className="h-3 w-3" />
                                              </button>
                                            </span>
                                          </div>
                                          {key.user_alias && (
                                            <div className="flex items-center justify-between gap-3">
                                              <span className="text-muted-foreground whitespace-nowrap">
                                                {t("keys.userTooltip.alias")}
                                              </span>
                                              <span className="flex items-center gap-1.5">
                                                <code className="text-[11px]">
                                                  {key.user_alias}
                                                </code>
                                                <button
                                                  type="button"
                                                  onClick={(e) => {
                                                    e.stopPropagation();
                                                    copyToClipboard(
                                                      key.user_alias!,
                                                    );
                                                  }}
                                                  className="text-muted-foreground hover:text-foreground"
                                                >
                                                  <Copy className="h-3 w-3" />
                                                </button>
                                              </span>
                                            </div>
                                          )}
                                          {key.user_email && key.user_alias && (
                                            <div className="flex items-center justify-between gap-3">
                                              <span className="text-muted-foreground whitespace-nowrap">
                                                {t("keys.userTooltip.email")}
                                              </span>
                                              <span className="flex items-center gap-1.5">
                                                <code className="text-[11px]">
                                                  {key.user_email}
                                                </code>
                                                <button
                                                  type="button"
                                                  onClick={(e) => {
                                                    e.stopPropagation();
                                                    copyToClipboard(
                                                      key.user_email!,
                                                    );
                                                  }}
                                                  className="text-muted-foreground hover:text-foreground"
                                                >
                                                  <Copy className="h-3 w-3" />
                                                </button>
                                              </span>
                                            </div>
                                          )}
                                        </TooltipContent>
                                      </Tooltip>
                                    </TooltipProvider>
                                  ) : (
                                    "—"
                                  )}
                                </TableCell>
                                <TableCell className="text-sm max-w-[160px]">
                                  {!Array.isArray(key.models) ||
                                  key.models.length === 0 ? (
                                    t("keys.allModels")
                                  ) : key.models.length <= 3 ? (
                                    <span className="truncate block">
                                      {key.models.join(", ")}
                                    </span>
                                  ) : (
                                    <TooltipProvider>
                                      <Tooltip delayDuration={300}>
                                        <TooltipTrigger asChild>
                                          <span className="cursor-default truncate block">
                                            {key.models.slice(0, 2).join(", ")}{" "}
                                            <Badge
                                              variant="secondary"
                                              className="text-[10px] px-1 py-0 align-middle"
                                            >
                                              +{key.models.length - 2}
                                            </Badge>
                                          </span>
                                        </TooltipTrigger>
                                        <TooltipContent
                                          side="bottom"
                                          className="max-w-xs p-2"
                                        >
                                          <div className="flex flex-wrap gap-1">
                                            {key.models.map((m) => (
                                              <Badge
                                                key={m}
                                                variant="secondary"
                                                className="text-xs"
                                              >
                                                {m}
                                              </Badge>
                                            ))}
                                          </div>
                                        </TooltipContent>
                                      </Tooltip>
                                    </TooltipProvider>
                                  )}
                                </TableCell>
                                <TableCell className="text-right text-sm">
                                  ${key.spend.toFixed(4)}
                                </TableCell>
                                <TableCell className="text-right text-sm">
                                  {key.max_budget != null
                                    ? `$${key.max_budget.toFixed(2)}${key.budget_duration ? ` / ${key.budget_duration}` : ""}`
                                    : "∞"}
                                </TableCell>
                                <TableCell className="text-right text-xs text-muted-foreground">
                                  {key.budget_duration ?? "—"}
                                </TableCell>
                                <TableCell className="text-xs text-muted-foreground">
                                  {key.expires
                                    ? new Date(key.expires).toLocaleDateString()
                                    : "∞"}
                                </TableCell>
                                <TableCell className="text-xs text-muted-foreground whitespace-nowrap">
                                  {key.created_at
                                    ? formatDate(key.created_at)
                                    : "—"}
                                </TableCell>
                                <TableCell>
                                  <div className="flex items-center gap-2">
                                    <Switch
                                      checked={!key.blocked}
                                      onCheckedChange={async (checked) => {
                                        try {
                                          await apiPut("/key/update", {
                                            key: key.token,
                                            blocked: !checked,
                                          });
                                          queryClient.invalidateQueries({
                                            queryKey: ["virtual-keys"],
                                          });
                                          toast.success(
                                            checked
                                              ? t("keys.toast.unblocked")
                                              : t("keys.toast.blocked"),
                                          );
                                        } catch (err) {
                                          toast.error((err as Error).message);
                                        }
                                      }}
                                    />
                                    {key.blocked ? (
                                      <Badge variant="destructive">
                                        {t("keys.blocked")}
                                      </Badge>
                                    ) : (
                                      <Badge variant="default">
                                        {t("keys.active")}
                                      </Badge>
                                    )}
                                  </div>
                                </TableCell>
                                <TableCell className="text-right">
                                  <div className="flex justify-end gap-1">
                                    <Button
                                      variant="ghost"
                                      size="icon"
                                      onClick={() => openEdit(key)}
                                    >
                                      <Pencil className="h-4 w-4" />
                                    </Button>
                                    <Button
                                      variant="ghost"
                                      size="icon"
                                      onClick={() => openDelete(key)}
                                    >
                                      <Trash2 className="h-4 w-4 text-destructive" />
                                    </Button>
                                  </div>
                                </TableCell>
                              </TableRow>
                            ))}
                      </TableBody>
                    </Table>
                    {!isLoading && filteredKeys.length === 0 && (
                      <div className="text-center text-muted-foreground py-8">
                        {search ? t("keys.noMatch") : t("keys.noKeys")}
                      </div>
                    )}
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
                      : filteredKeys.map((key) => (
                          <Card key={key.token}>
                            <CardContent className="p-4 space-y-2">
                              <div className="flex items-center justify-between">
                                <span className="font-medium text-sm truncate max-w-[70%]">
                                  {key.key_alias ?? key.key_name ?? "—"}
                                </span>
                                {key.blocked ? (
                                  <Badge
                                    variant="destructive"
                                    className="text-xs"
                                  >
                                    {t("keys.blocked")}
                                  </Badge>
                                ) : (
                                  <Badge variant="default" className="text-xs">
                                    {t("keys.active")}
                                  </Badge>
                                )}
                              </div>
                              {key.key_name && key.key_alias && (
                                <div className="flex items-center justify-between text-xs text-muted-foreground">
                                  <span>{key.key_name}</span>
                                </div>
                              )}
                              <div className="flex items-center gap-2 text-xs font-mono">
                                <span className="text-muted-foreground truncate max-w-[60%]">
                                  {visibleTokens.has(key.token)
                                    ? key.token
                                    : maskToken(key.token)}
                                </span>
                                <button
                                  type="button"
                                  onClick={() => toggleTokenVisible(key.token)}
                                  className="text-muted-foreground hover:text-foreground shrink-0"
                                >
                                  {visibleTokens.has(key.token) ? (
                                    <EyeOff className="h-3 w-3" />
                                  ) : (
                                    <Eye className="h-3 w-3" />
                                  )}
                                </button>
                                <button
                                  type="button"
                                  onClick={() => copyToClipboard(key.token)}
                                  className="text-muted-foreground hover:text-foreground shrink-0"
                                >
                                  <Copy className="h-3 w-3" />
                                </button>
                              </div>
                              <div className="flex items-center justify-between text-xs text-muted-foreground">
                                <span>
                                  {key.user_email ??
                                    key.user_alias ??
                                    key.user_id ??
                                    "—"}
                                </span>
                                <span>
                                  Spent ${key.spend.toFixed(4)}
                                  {" / "}
                                  {key.max_budget != null
                                    ? `$${key.max_budget.toFixed(2)}${key.budget_duration ? ` / ${key.budget_duration}` : ""}`
                                    : "∞"}
                                </span>
                              </div>
                              <div className="text-xs text-muted-foreground">
                                {t("keys.table.resetPeriod")}: {key.budget_duration ?? "—"}
                              </div>
                              <div className="text-xs text-muted-foreground">
                                {t("keys.mobile.expires")}{" "}
                                {key.expires
                                  ? new Date(key.expires).toLocaleDateString()
                                  : "∞"}
                              </div>
                              <div className="text-xs text-muted-foreground whitespace-nowrap">
                                {t("keys.mobile.created")}{" "}
                                {key.created_at
                                  ? formatDate(key.created_at)
                                  : "—"}
                              </div>
                              <div className="flex justify-end gap-1 pt-1">
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => openEdit(key)}
                                >
                                  <Pencil className="h-3.5 w-3.5 mr-1" />
                                  {t("common.edit")}
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => openDelete(key)}
                                >
                                  <Trash2 className="h-3.5 w-3.5 mr-1 text-destructive" />
                                  {t("common.delete")}
                                </Button>
                              </div>
                            </CardContent>
                          </Card>
                        ))}
                    {!isLoading && filteredKeys.length === 0 && (
                      <div className="text-center text-muted-foreground py-8">
                        {search ? t("keys.noMatch") : t("keys.noKeys")}
                      </div>
                    )}
                  </div>
                  {filteredKeys.length > 0 ? (
                    <div className="mt-3">
                      <PaginationBar
                        page={page}
                        pageSize={pageSize}
                        totalCount={totalCount}
                        totalPages={totalPages}
                        onPage={setPage}
                        onPageSize={(s) => {
                          setPageSize(s);
                          setPage(1);
                        }}
                      />
                    </div>
                  ) : null}
                </>
              )}
            </CardContent>
          </Card>

          {/* Create Dialog */}
          <Dialog open={createOpen} onOpenChange={setCreateOpen}>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t("keys.createDialog.title")}</DialogTitle>
                <DialogDescription>
                  {t("keys.createDialog.description")}
                </DialogDescription>
              </DialogHeader>

              {generatedToken ? (
                <div className="space-y-4">
                  <p className="text-sm font-medium text-green-600">
                    {t("keys.createDialog.created")}
                  </p>
                  <div className="flex items-center gap-2 rounded-md border bg-muted p-3">
                    <code className="flex-1 break-all text-sm font-mono">
                      {generatedToken}
                    </code>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => copyToClipboard(generatedToken)}
                    >
                      <Copy className="h-4 w-4" />
                    </Button>
                  </div>
                  <Button
                    className="w-full"
                    onClick={() => {
                      setCreateOpen(false);
                      setGeneratedToken(null);
                      queryClient.invalidateQueries({
                        queryKey: ["virtual-keys"],
                      });
                    }}
                  >
                    {t("keys.createDialog.savedBtn")}
                  </Button>
                </div>
              ) : (
                <>
                  <div className="space-y-4">
                    <div>
                      <Label htmlFor="name">
                        {t("keys.createDialog.nameLabel")}
                      </Label>
                      <Input
                        id="name"
                        value={formName}
                        onChange={(e) => setFormName(e.target.value)}
                        placeholder={t("keys.createDialog.namePlaceholder")}
                      />
                    </div>
                    <div>
                      <Label htmlFor="alias">
                        {t("keys.createDialog.aliasLabel")}
                      </Label>
                      <Input
                        id="alias"
                        value={formAlias}
                        onChange={(e) => setFormAlias(e.target.value)}
                        placeholder={t("keys.createDialog.aliasPlaceholder")}
                      />
                    </div>
                    <div>
                      <Label>{t("keys.createDialog.modelsLabel")}</Label>
                      <MultiModelSelect
                        selected={selectedModels}
                        onChange={setSelectedModels}
                      />
                    </div>
                    {isAdmin && (
                      <div>
                        <Label htmlFor="user-id">
                          {t("keys.createDialog.userSelectorLabel")}
                        </Label>
                        <select
                          id="user-id"
                          value={formUserId}
                          onChange={(e) => setFormUserId(e.target.value)}
                          className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background"
                        >
                          <option value="">{t("keys.createDialog.userSelectorNone")}</option>
                          {userList.map((u) => (
                            <option key={u.user_id} value={u.user_id}>
                              {u.user_email ?? u.user_alias ?? u.user_id}
                            </option>
                          ))}
                        </select>
                      </div>
                    )}
                    <div className="grid grid-cols-3 gap-4">
                      <div>
                        <Label htmlFor="budget">
                          {t("keys.createDialog.budgetLabel")}
                        </Label>
                        <Input
                          id="budget"
                          type="number"
                          value={formBudget}
                          onChange={(e) => setFormBudget(e.target.value)}
                          placeholder="50"
                        />
                      </div>
                      <div>
                        <Label htmlFor="tpm">
                          {t("keys.createDialog.tpmLabel")}
                        </Label>
                        <Input
                          id="tpm"
                          type="number"
                          value={formTPM}
                          onChange={(e) => setFormTPM(e.target.value)}
                          placeholder="100000"
                        />
                      </div>
                      <div>
                        <Label htmlFor="rpm">
                          {t("keys.createDialog.rpmLabel")}
                        </Label>
                        <Input
                          id="rpm"
                          type="number"
                          value={formRPM}
                          onChange={(e) => setFormRPM(e.target.value)}
                          placeholder="100"
                        />
                      </div>
                    </div>
                    <div className="grid grid-cols-2 gap-4">
                      {/* Budget Duration */}
                      <div className="space-y-2">
                        <Label htmlFor="budget-duration">
                          {t("keys.createDialog.budgetDurationLabel")}
                        </Label>
                        <select
                          id="budget-duration"
                          className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
                          value={formBudgetDuration}
                          onChange={(e) =>
                            setFormBudgetDuration(e.target.value)
                          }
                        >
                          <option value="">
                            {t("keys.createDialog.budgetDurationOptions.none")}
                          </option>
                          <option value="24h">
                            {t("keys.createDialog.budgetDurationOptions.daily")}
                          </option>
                          <option value="7d">
                            {t(
                              "keys.createDialog.budgetDurationOptions.weekly",
                            )}
                          </option>
                          <option value="30d">
                            {t(
                              "keys.createDialog.budgetDurationOptions.monthly",
                            )}
                          </option>
                        </select>
                      </div>

                      {/* Soft Budget */}
                      <div className="space-y-2">
                        <Label htmlFor="soft-budget">
                          {t("keys.createDialog.softBudgetLabel")}
                        </Label>
                        <Input
                          id="soft-budget"
                          type="number"
                          step="0.0001"
                          placeholder="0.00 — warn but don't block"
                          value={formSoftBudget}
                          onChange={(e) => setFormSoftBudget(e.target.value)}
                        />
                      </div>
                    </div>
                    <div>
                      <Label htmlFor="expires">
                        {t("keys.createDialog.expiresLabel")}
                      </Label>
                      <Input
                        id="expires"
                        type="date"
                        value={formExpires}
                        onChange={(e) => setFormExpires(e.target.value)}
                      />
                    </div>
                  </div>
                  <DialogFooter>
                    <Button
                      variant="outline"
                      onClick={() => setCreateOpen(false)}
                    >
                      {t("common.cancel")}
                    </Button>
                    <Button
                      onClick={handleCreate}
                      disabled={createMutation.isPending}
                    >
                      {createMutation.isPending && <Spinner className="mr-2" />}
                      {t("keys.createDialog.generateBtn")}
                    </Button>
                  </DialogFooter>
                </>
              )}
            </DialogContent>
          </Dialog>

          {/* Edit Dialog */}
          <Dialog open={editOpen} onOpenChange={setEditOpen}>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t("keys.editDialog.title")}</DialogTitle>
                <DialogDescription>
                  {t("keys.editDialog.description", {
                    name:
                      selectedKey?.key_alias ?? selectedKey?.token.slice(0, 8),
                  })}
                </DialogDescription>
              </DialogHeader>
              <div className="space-y-4">
                <div>
                  <Label htmlFor="edit-name">
                    {t("keys.createDialog.nameLabel")}
                  </Label>
                  <Input
                    id="edit-name"
                    value={formName}
                    onChange={(e) => setFormName(e.target.value)}
                    placeholder={t("keys.createDialog.namePlaceholder")}
                  />
                </div>
                <div>
                  <Label htmlFor="edit-alias">
                    {t("keys.createDialog.aliasLabel")}
                  </Label>
                  <Input
                    id="edit-alias"
                    value={formAlias}
                    onChange={(e) => setFormAlias(e.target.value)}
                  />
                </div>
                <div>
                  <Label>{t("keys.createDialog.modelsLabel")}</Label>
                  <MultiModelSelect
                    selected={selectedModels}
                    onChange={setSelectedModels}
                  />
                </div>
                <div className="grid grid-cols-3 gap-4">
                  <div>
                    <Label htmlFor="edit-budget">
                      {t("keys.createDialog.budgetLabel")}
                    </Label>
                    <Input
                      id="edit-budget"
                      type="number"
                      value={formBudget}
                      onChange={(e) => setFormBudget(e.target.value)}
                    />
                  </div>
                  <div>
                    <Label htmlFor="edit-tpm">
                      {t("keys.createDialog.tpmLabel")}
                    </Label>
                    <Input
                      id="edit-tpm"
                      type="number"
                      value={formTPM}
                      onChange={(e) => setFormTPM(e.target.value)}
                    />
                  </div>
                  <div>
                    <Label htmlFor="edit-rpm">
                      {t("keys.createDialog.rpmLabel")}
                    </Label>
                    <Input
                      id="edit-rpm"
                      type="number"
                      value={formRPM}
                      onChange={(e) => setFormRPM(e.target.value)}
                    />
                  </div>
                </div>
                <div className="grid grid-cols-2 gap-4">
                  {/* Budget Duration */}
                  <div className="space-y-2">
                    <Label htmlFor="edit-budget-duration">
                      {t("keys.createDialog.budgetDurationLabel")}
                    </Label>
                    <select
                      id="edit-budget-duration"
                      className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
                      value={formBudgetDuration}
                      onChange={(e) => setFormBudgetDuration(e.target.value)}
                    >
                      <option value="">
                        {t("keys.createDialog.budgetDurationOptions.none")}
                      </option>
                      <option value="24h">
                        {t("keys.createDialog.budgetDurationOptions.daily")}
                      </option>
                      <option value="7d">
                        {t("keys.createDialog.budgetDurationOptions.weekly")}
                      </option>
                      <option value="30d">
                        {t("keys.createDialog.budgetDurationOptions.monthly")}
                      </option>
                    </select>
                  </div>

                  {/* Soft Budget */}
                  <div className="space-y-2">
                    <Label htmlFor="edit-soft-budget">
                      {t("keys.createDialog.softBudgetLabel")}
                    </Label>
                    <Input
                      id="edit-soft-budget"
                      type="number"
                      step="0.0001"
                      placeholder="0.00 — warn but don't block"
                      value={formSoftBudget}
                      onChange={(e) => setFormSoftBudget(e.target.value)}
                    />
                  </div>
                </div>
                <div>
                  <Label htmlFor="edit-expires">
                    {t("keys.createDialog.expiresLabel")}
                  </Label>
                  <Input
                    id="edit-expires"
                    type="date"
                    value={formExpires}
                    onChange={(e) => setFormExpires(e.target.value)}
                  />
                </div>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={() => setEditOpen(false)}>
                  {t("common.cancel")}
                </Button>
                <Button onClick={handleEdit} disabled={editMutation.isPending}>
                  {editMutation.isPending && <Spinner className="mr-2" />}
                  {t("keys.editDialog.saveBtn")}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          {/* Delete Confirmation */}
          <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t("keys.deleteDialog.title")}</DialogTitle>
                <DialogDescription>
                  {t("keys.deleteDialog.description", {
                    name:
                      selectedKey?.key_alias ?? selectedKey?.token.slice(0, 8),
                  })}
                </DialogDescription>
              </DialogHeader>
              <DialogFooter>
                <Button variant="outline" onClick={() => setDeleteOpen(false)}>
                  {t("common.cancel")}
                </Button>
                <Button
                  variant="destructive"
                  onClick={() =>
                    selectedKey && deleteMutation.mutate(selectedKey.token)
                  }
                  disabled={deleteMutation.isPending}
                >
                  {deleteMutation.isPending && <Spinner className="mr-2" />}
                  {t("common.delete")}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        </>
      )}
    </div>
  );
}
