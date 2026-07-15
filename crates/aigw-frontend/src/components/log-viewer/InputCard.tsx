import { useState, useCallback } from "react";
import { useCopyToClipboard } from "@/hooks/useCopyToClipboard";
import { SectionHeader, CollapsibleMessage, HistoryTree } from "./SectionHeader";
import { parseMessages, type ParsedRequest } from "./MessageViewer";
import { safeStringify, extractText } from "./utils";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tools section
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function ToolsBlock({ tools }: { tools: unknown[] }) {
  const [open, setOpen] = useState(false);

  return (
    <div className="border rounded overflow-hidden">
      <button
        type="button"
        className="flex items-center gap-1.5 w-full text-left px-2 py-1 text-[11px] font-medium text-muted-foreground hover:bg-muted/30 transition-colors"
        onClick={() => setOpen(!open)}
      >
        <span className="text-[10px]">{open ? "▾" : "▸"}</span>
        <span>TOOLS ({tools.length} available)</span>
      </button>
      {open && (
        <div className="px-2 py-1.5 border-t bg-background/50 space-y-1 max-h-48 overflow-y-auto">
          {tools.map((t, i) => {
            const tool = t as Record<string, unknown>;
            const func = (tool.function ?? {}) as Record<string, unknown>;
            return (
              <div key={i} className="bg-muted/30 rounded p-1.5 text-[11px]">
                <div className="font-mono font-medium">{String(func.name ?? `tool_${i}`)}</div>
                {func.description ? (
                  <div className="text-muted-foreground mt-0.5">{String(func.description)}</div>
                ) : null}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// InputCard
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface InputCardProps {
  messages: unknown;  // raw SpendLog.messages value
  promptTokens: number;
  spend: number;
}

export function InputCard({ messages, promptTokens, spend }: InputCardProps) {
  const [collapsed, setCollapsed] = useState(false);
  const { copied, copy } = useCopyToClipboard();

  const parsed: ParsedRequest = parseMessages(messages);
  const allMsgs = parsed.messages;
  const tools = parsed.tools;

  // Separate system, history, and last message (litellm-style)
  const systemMsg = allMsgs.find((m) => m.role === "system");
  const nonSystem = allMsgs.filter((m) => m.role !== "system" && m.role !== "tool");
  const toolResults = allMsgs.filter((m) => m.role === "tool");
  const lastMsg = nonSystem.length > 0 ? nonSystem[nonSystem.length - 1] : null;
  const history = nonSystem.length > 1 ? nonSystem.slice(0, -1) : [];

  // Estimate input cost from total spend ratio (rough: prompt/total ratio)
  const inputCost = spend > 0 ? spend * (promptTokens / (promptTokens + 1)) : undefined;

  const handleCopy = useCallback(() => {
    copy(safeStringify(messages));
  }, [copy, messages]);

  if (allMsgs.length === 0 && (!tools || tools.length === 0)) {
    return null;
  }

  return (
    <div className="border rounded-lg mb-2 overflow-hidden">
      <SectionHeader
        type="input"
        tokens={promptTokens > 0 ? promptTokens : undefined}
        cost={inputCost}
        onCopy={handleCopy}
        copied={copied}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed(!collapsed)}
      />

      {!collapsed && (
        <div className="p-3 space-y-1.5">
          {/* System message */}
          {systemMsg && (
            <CollapsibleMessage
              label="SYSTEM"
              content={systemMsg.content}
              defaultExpanded={!!(extractText(systemMsg.content).length < 200)}
            />
          )}

          {/* History */}
          {history.length > 0 && (
            <HistoryTree messages={history} defaultExpanded={false} />
          )}

          {/* Last User Message — always visible */}
          {lastMsg && (
            <div className="border rounded p-2.5 text-xs whitespace-pre-wrap leading-relaxed bg-blue-50/30 dark:bg-blue-950/10">
              <div className="text-[10px] uppercase tracking-wider text-muted-foreground mb-1">
                {lastMsg.role}
              </div>
              {extractText(lastMsg.content) || (
                <span className="text-muted-foreground italic">(empty)</span>
              )}
            </div>
          )}

          {/* Tool results */}
          {toolResults.length > 0 && (
            <CollapsibleMessage
              label={`TOOL RESULTS (${toolResults.length})`}
              content={toolResults.map((t) => `[${t.name ?? t.tool_call_id ?? "tool"}]: ${extractText(t.content)}`).join("\n\n")}
              defaultExpanded={false}
            />
          )}

          {/* Tools definitions */}
          {tools && tools.length > 0 && <ToolsBlock tools={tools} />}
        </div>
      )}
    </div>
  );
}
