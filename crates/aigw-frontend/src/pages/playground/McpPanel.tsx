import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { apiGet, apiPost } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Play } from "lucide-react";
import type { McpServerListResponse, McpVisibleServerList, McpTool } from "@/pages/mcp/types";

interface RestToolsResponse {
  tools: McpTool[];
}

interface AdminToolsResponse {
  tools: McpTool[];
}

interface UsageParam {
  name: string;
  type: string;
  description?: string;
  required: boolean;
}

function usageParams(schema?: Record<string, unknown>): UsageParam[] {
  const properties = (schema?.properties ?? {}) as Record<
    string,
    { type?: string; description?: string } | undefined
  >;
  const required = new Set((schema?.required as string[] | undefined) ?? []);
  return Object.entries(properties).map(([name, spec]) => ({
    name,
    type: spec?.type ?? "any",
    description: spec?.description,
    required: required.has(name),
  }));
}

export function McpPanel({ isAdmin }: { isAdmin: boolean }) {
  const { t } = useTranslation();
  const [alias, setAlias] = useState("");
  const [tool, setTool] = useState("");
  const [args, setArgs] = useState("{}");
  const [sending, setSending] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  const { data: visible } = useQuery<McpVisibleServerList>({
    queryKey: ["mcp-my-servers"],
    queryFn: () => apiGet("/v1/mcp/servers"),
    enabled: !isAdmin,
  });

  const { data: adminServers } = useQuery<McpServerListResponse>({
    queryKey: ["mcp-servers-all"],
    queryFn: () => apiGet("/admin/mcp/servers?page=1&page_size=100"),
    enabled: isAdmin,
  });

  const servers = useMemo(
    () =>
      isAdmin
        ? (adminServers?.data ?? []).filter((s) => s.enabled)
        : (visible?.data ?? []).filter((s) => s.enabled),
    [isAdmin, adminServers, visible],
  );

  const selectedId = servers.find((s) => s.alias === alias)?.id;

  const { data: adminTools } = useQuery<AdminToolsResponse>({
    queryKey: ["mcp-playground-tools", selectedId],
    queryFn: () => apiGet(`/admin/mcp/servers/${selectedId}/tools`),
    enabled: isAdmin && selectedId !== undefined,
  });

  const { data: restTools } = useQuery<RestToolsResponse>({
    queryKey: ["mcp-rest-tools", alias],
    queryFn: () => apiPost("/mcp-rest/tools/list", { alias }),
    enabled: !isAdmin && alias !== "",
  });

  const tools = useMemo(
    () => (isAdmin ? (adminTools?.tools ?? []) : (restTools?.tools ?? [])),
    [isAdmin, adminTools, restTools],
  );

  const toolNames = useMemo(() => tools.map((tk) => tk.tool_name), [tools]);

  const selectedTool = useMemo(
    () => tools.find((tk) => tk.tool_name === tool),
    [tools, tool],
  );

  const params = useMemo(() => usageParams(selectedTool?.input_schema), [selectedTool]);

  const fillSchema = useCallback(() => {
    const skeleton: Record<string, unknown> = {};
    for (const p of params) skeleton[p.name] = p.type;
    setArgs(JSON.stringify(skeleton, null, 2));
  }, [params]);

  const send = useCallback(async () => {
    if (!alias || !tool) return;
    let parsedArgs: unknown;
    try {
      parsedArgs = JSON.parse(args.trim() || "{}");
    } catch {
      setResult({ ok: false, text: t("playground.mcpInvalidArgs") });
      return;
    }
    setSending(true);
    setResult(null);
    try {
      const body = await apiPost<unknown>("/mcp-rest/tools/call", {
        alias,
        tool,
        arguments: parsedArgs,
      });
      setResult({ ok: true, text: JSON.stringify(body, null, 2) });
    } catch (err) {
      setResult({ ok: false, text: (err as Error).message });
    } finally {
      setSending(false);
    }
  }, [alias, tool, args, t]);

  return (
    <div className="flex flex-1 flex-col gap-3 min-w-0 min-h-0">
      <div className="flex flex-col gap-3 sm:flex-row">
        <div className="flex-1 space-y-1">
          <Label className="text-xs">{t("playground.mcpServer")}</Label>
          <Select
            value={alias}
            onValueChange={(v) => {
              setAlias(v);
              setTool("");
            }}
          >
            <SelectTrigger className="h-8 text-xs" data-testid="playground-mcp-server">
              <SelectValue placeholder={t("playground.mcpSelectServer")} />
            </SelectTrigger>
            <SelectContent>
              {servers.map((s) => (
                <SelectItem key={s.id} value={s.alias}>
                  {s.alias}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex-1 space-y-1">
          <Label className="text-xs">{t("playground.mcpTool")}</Label>
          <Select value={tool} onValueChange={setTool} disabled={!alias}>
            <SelectTrigger className="h-8 text-xs" data-testid="playground-mcp-tool">
              <SelectValue placeholder={t("playground.mcpSelectTool")} />
            </SelectTrigger>
            <SelectContent>
              {toolNames.map((name) => (
                <SelectItem key={name} value={name}>
                  {name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      <div className="space-y-1">
        <Label className="text-xs">{t("playground.mcpArguments")}</Label>
        <Textarea
          value={args}
          onChange={(e) => setArgs(e.target.value)}
          className="min-h-[80px] font-mono text-xs"
          data-testid="playground-mcp-args"
        />
      </div>

      <div className="flex justify-end">
        <Button
          size="sm"
          onClick={send}
          disabled={sending || !alias || !tool}
          data-testid="playground-mcp-send"
        >
          {sending ? <Spinner className="mr-1" /> : <Play className="h-3.5 w-3.5 mr-1" />}
          {t("playground.send")}
        </Button>
      </div>

      <div className="flex-1 min-h-0 overflow-auto">
        {result ? (
          <div className="space-y-2">
            <Badge
              variant={result.ok ? "default" : "destructive"}
              data-testid="playground-mcp-status"
            >
              {result.ok ? t("playground.mcpOk") : t("playground.mcpFailed")}
            </Badge>
            <pre
              className="whitespace-pre-wrap break-all rounded-md bg-muted p-3 font-mono text-xs"
              data-testid="playground-mcp-response"
            >
              <code>{result.text}</code>
            </pre>
          </div>
        ) : (
          <p
            className="py-8 text-center text-xs text-muted-foreground"
            data-testid="playground-mcp-idle"
          >
            {t("playground.mcpIdle")}
          </p>
        )}
      </div>

      {selectedTool && (
        <div
          className="shrink-0 space-y-2 rounded-md border p-3"
          data-testid="playground-mcp-usage"
        >
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <p className="font-mono text-xs font-medium">{selectedTool.tool_name}</p>
            {params.length > 0 && (
              <Button
                size="sm"
                variant="ghost"
                className="h-6 px-1 text-xs"
                onClick={fillSchema}
                data-testid="playground-mcp-fill-schema"
              >
                {t("playground.mcpFillSchema")}
              </Button>
            )}
          </div>
          <p
            className="text-xs text-muted-foreground"
            data-testid="playground-mcp-usage-description"
          >
            {selectedTool.description || t("playground.mcpUsageNoDescription")}
          </p>
          {params.length > 0 ? (
            <ul className="space-y-1" data-testid="playground-mcp-usage-params">
              {params.map((p) => (
                <li
                  key={p.name}
                  className="flex flex-wrap items-baseline gap-x-2 text-xs"
                  data-testid="playground-mcp-usage-param"
                >
                  <code className="font-mono">{p.name}</code>
                  <Badge variant="outline" className="px-1 py-0 text-[10px]">
                    {p.type}
                  </Badge>
                  <span className="text-muted-foreground">
                    {p.required
                      ? t("playground.mcpUsageRequired")
                      : t("playground.mcpUsageOptional")}
                  </span>
                  {p.description && (
                    <span className="text-muted-foreground">{p.description}</span>
                  )}
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-xs text-muted-foreground" data-testid="playground-mcp-usage-noschema">
              {t("playground.mcpUsageNoSchema")}
            </p>
          )}
          {params.length > 0 && (
            <p className="text-xs text-muted-foreground">{t("playground.mcpFillSchemaHint")}</p>
          )}
        </div>
      )}
    </div>
  );
}
