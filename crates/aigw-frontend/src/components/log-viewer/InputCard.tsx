import { useState } from "react";
import {
  SectionHeader,
  CollapsibleMessage,
  HistoryTree,
} from "./SectionHeader";
import { parseMessages, type ParsedRequest } from "./MessageViewer";
import { ImageThumbnails } from "./ImageThumbnails";
import { extractImages, extractText } from "./utils";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// InputCard
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface InputCardProps {
  messages: unknown; // raw SpendLog.messages value
  promptTokens: number;
  spend: number;
}

export function InputCard({ messages, promptTokens, spend }: InputCardProps) {
  const [collapsed, setCollapsed] = useState(false);

  const parsed: ParsedRequest = parseMessages(messages);
  const allMsgs = parsed.messages;

  // Separate system, history, and last message (litellm-style)
  const systemMsg = allMsgs.find((m) => m.role === "system");
  const nonSystem = allMsgs.filter(
    (m) => m.role !== "system" && m.role !== "tool",
  );
  const toolResults = allMsgs.filter((m) => m.role === "tool");
  const lastMsg = nonSystem.length > 0 ? nonSystem[nonSystem.length - 1] : null;
  const history = nonSystem.length > 1 ? nonSystem.slice(0, -1) : [];

  // Estimate input cost from total spend ratio (rough: prompt/total ratio)
  const inputCost =
    spend > 0 ? spend * (promptTokens / (promptTokens + 1)) : undefined;

  if (allMsgs.length === 0) {
    return null;
  }

  return (
    <div className="border rounded-lg mb-2 overflow-hidden">
      <SectionHeader
        type="input"
        tokens={promptTokens > 0 ? promptTokens : undefined}
        cost={inputCost}
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
              {(() => {
                const images = extractImages(lastMsg.content);
                return images.length > 0 ? (
                  <ImageThumbnails images={images} maxH="h-24" maxW="max-w-36" />
                ) : null;
              })()}
            </div>
          )}

          {/* Tool results — tabular, each row collapsible, collapsed by default */}
          {toolResults.length > 0 && (
            <div className="border rounded mb-1.5 overflow-hidden">
              <CollapsibleMessage
                label={`Tool Results (${toolResults.length})`}
                content=""
                defaultExpanded={false}
                customContent={
                  <table className="w-full text-xs">
                    <thead>
                      <tr className="border-b bg-muted/30 text-muted-foreground">
                        <th className="text-left px-2 py-1 text-[10px] uppercase tracking-wider w-1/4">
                          Tool
                        </th>
                        <th className="text-left px-2 py-1 text-[10px] uppercase tracking-wider w-3/4">
                          Result
                        </th>
                      </tr>
                    </thead>
                    <tbody>
                      {toolResults.map((t, i) => (
                        <ToolResultRow
                          key={i}
                          name={t.name ?? t.tool_call_id ?? `tool_${i}`}
                          content={t.content}
                        />
                      ))}
                    </tbody>
                  </table>
                }
              />
            </div>
          )}

          {/* Tools definitions are rendered at drawer level as ToolsCard */}
        </div>
      )}
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ToolResultRow — single tool result row in the table
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function ToolResultRow({ name, content }: { name: string; content: unknown }) {
  const [open, setOpen] = useState(false);
  const text = extractText(content);
  const preview = text.length > 80 ? text.slice(0, 80) + "…" : text;

  return (
    <>
      <tr
        className="border-t hover:bg-muted/20 cursor-pointer"
        onClick={() => setOpen(!open)}
      >
        <td className="px-2 py-1 font-mono text-[11px] align-top">{name}</td>
        <td className="px-2 py-1 text-muted-foreground align-top">
          {open ? (
            <pre className="whitespace-pre-wrap break-all leading-relaxed text-foreground">
              {text}
            </pre>
          ) : (
            <span>{preview || <span className="italic">(empty)</span>}</span>
          )}
        </td>
      </tr>
    </>
  );
}
