import { useState, useCallback } from "react";
import { useCopyToClipboard } from "@/hooks/useCopyToClipboard";
import { SectionHeader } from "./SectionHeader";
import { ToolCallBlock } from "./ToolCallBlock";
import { safeStringify, extractText } from "./utils";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OutputCard — displays assistant response with tool calls
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface ParsedOutput {
  text: string;
  toolCalls: unknown[] | null;
  usage: Record<string, unknown> | null;
  finishReason: string | null;
}

function parseOutput(raw: unknown): ParsedOutput {
  const empty: ParsedOutput = { text: "", toolCalls: null, usage: null, finishReason: null };
  if (!raw) return empty;

  try {
    const r = typeof raw === "string" ? JSON.parse(raw) : (raw as Record<string, unknown>);
    if (!r || typeof r !== "object") return empty;

    // OpenAI: choices[0].message
    if (Array.isArray((r as Record<string, unknown>).choices)) {
      const choices = (r as Record<string, unknown>).choices as Array<Record<string, unknown>>;
      const first = choices[0];
      if (first) {
        const msg = (first.message ?? first.delta ?? {}) as Record<string, unknown>;
        return {
          text: extractText(msg.content),
          toolCalls: (msg.tool_calls as unknown[]) ?? null,
          usage: (r as Record<string, unknown>).usage as Record<string, unknown> | null ?? null,
          finishReason: (first.finish_reason as string) ?? (msg.finish_reason as string) ?? null,
        };
      }
    }

    // Anthropic: content[] blocks
    if (Array.isArray((r as Record<string, unknown>).content)) {
      const content = (r as Record<string, unknown>).content as Array<Record<string, unknown>>;
      const textParts: string[] = [];
      const toolCalls: unknown[] = [];
      for (const block of content) {
        if (block.type === "text" || block.type === "text_delta") {
          textParts.push(String(block.text ?? ""));
        } else if (block.type === "tool_use") {
          toolCalls.push({ name: block.name, input: block.input, id: block.id });
        }
      }
      return {
        text: textParts.join(""),
        toolCalls: toolCalls.length > 0 ? toolCalls : null,
        usage: (r as Record<string, unknown>).usage as Record<string, unknown> | null ?? null,
        finishReason: ((r as Record<string, unknown>).stop_reason as string) ?? null,
      };
    }

    return empty;
  } catch {
    return empty;
  }
}

interface OutputCardProps {
  response: unknown;
  completionTokens: number;
  spend: number;
}

export function OutputCard({ response, completionTokens, spend }: OutputCardProps) {
  const [collapsed, setCollapsed] = useState(false);
  const { copied, copy } = useCopyToClipboard();

  const parsed = parseOutput(response);

  const outputCost = spend > 0
    ? spend * (completionTokens / (completionTokens + 1))
    : undefined;

  const handleCopy = useCallback(() => {
    copy(safeStringify(response));
  }, [copy, response]);

  return (
    <div className="border rounded-lg overflow-hidden">
      <SectionHeader
        type="output"
        tokens={completionTokens > 0 ? completionTokens : undefined}
        cost={outputCost}
        onCopy={handleCopy}
        copied={copied}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed(!collapsed)}
      />

      {!collapsed && (
        <div className="p-3 space-y-2">
          {/* Plain text response */}
          {parsed.text && (
            <div className="bg-green-50/30 dark:bg-green-950/10 rounded-lg p-2.5 text-xs whitespace-pre-wrap leading-relaxed">
              {parsed.text}
            </div>
          )}

          {/* Tool calls */}
          {parsed.toolCalls && parsed.toolCalls.length > 0 && (
            <ToolCallBlock toolCalls={parsed.toolCalls} />
          )}

          {/* Usage stats */}
          {parsed.usage && (
            <div className="rounded border p-2 text-[11px] font-mono grid grid-cols-2 gap-x-4 gap-y-0.5">
              {Object.entries(parsed.usage).map(([key, val]) => (
                <div key={key} className="flex justify-between">
                  <span className="text-muted-foreground">{key}</span>
                  <span>{String(val)}</span>
                </div>
              ))}
            </div>
          )}

          {/* Finish reason */}
          {parsed.finishReason && (
            <div className="text-[11px] text-muted-foreground">
              Finish: <code className="font-mono bg-muted px-1 rounded">{parsed.finishReason}</code>
            </div>
          )}

          {/* Empty state */}
          {!parsed.text && !parsed.toolCalls && !parsed.usage && (
            <p className="text-xs text-muted-foreground italic py-2 text-center">
              No response content to display
            </p>
          )}
        </div>
      )}
    </div>
  );
}
