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

interface RestToolEntry {
  tool_name: string;
  description?: string;
}

interface RestToolsResponse {
  tools: RestToolEntry[];
}

interface AdminToolsResponse {
  tools: McpTool[];
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

  const toolNames = useMemo(
    () =>
      isAdmin
        ? (adminTools?.tools ?? []).map((tk) => tk.tool_name)
        : (restTools?.tools ?? []).map((tk) => tk.tool_name),
    [isAdmin, adminTools, restTools],
  );

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
    </div>
  );
}
