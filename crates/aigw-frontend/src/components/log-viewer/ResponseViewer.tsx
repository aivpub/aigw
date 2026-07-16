import { ToolCallBlock } from "./ToolCallBlock";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ResponseViewer — parse and display LLM response
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface ParsedResponse {
  text: string;
  toolCalls: unknown[] | null;
  usage: Record<string, unknown> | null;
  finishReason: string | null;
}

function parseResponse(raw: unknown): ParsedResponse {
  const empty: ParsedResponse = { text: "", toolCalls: null, usage: null, finishReason: null };
  if (!raw) return empty;

  try {
    const r = typeof raw === "string" ? JSON.parse(raw) : (raw as Record<string, unknown>);
    if (!r || typeof r !== "object") return empty;

    // OpenAI format: choices[0].message.content + choices[0].message.tool_calls
    if (Array.isArray((r as Record<string, unknown>).choices)) {
      const choices = (r as Record<string, unknown>).choices as Array<Record<string, unknown>>;
      const first = choices[0];
      if (first) {
        const msg = (first.message ?? first.delta ?? {}) as Record<string, unknown>;
        const text = extractTextContent(msg.content);
        const tc = msg.tool_calls as unknown[] | null;
        return {
          text,
          toolCalls: tc,
          usage: (r as Record<string, unknown>).usage as Record<string, unknown> | null ?? null,
          finishReason: (first.finish_reason as string) ?? msg.finish_reason as string ?? null,
        };
      }
    }

    // Anthropic format: content[] blocks + stop_reason
    if (Array.isArray((r as Record<string, unknown>).content)) {
      const content = (r as Record<string, unknown>).content as Array<Record<string, unknown>>;
      const textParts: string[] = [];
      const toolCalls: unknown[] = [];
      for (const block of content) {
        if (block.type === "text" || block.type === "text_delta") {
          textParts.push(String(block.text ?? ""));
        } else if (block.type === "tool_use") {
          toolCalls.push({ name: block.name, input: block.input, id: block.id });
        } else {
          textParts.push(`[${block.type}]`);
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

function extractTextContent(content: unknown): string {
  if (content === null || content === undefined) return "";
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return (content as Array<Record<string, unknown>>)
      .map((part) => {
        if (part.type === "text") return String(part.text ?? "");
        return `[${part.type}]`;
      })
      .join("");
  }
  return String(content);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Sub-components
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface ResponseViewerProps {
  response: unknown;
}

export function ResponseViewer({ response }: ResponseViewerProps) {
  const parsed = parseResponse(response);

  if (!parsed.text && !parsed.toolCalls && !parsed.usage) {
    return (
      <p className="text-sm text-muted-foreground py-4 text-center">
        No response content to display
      </p>
    );
  }

  return (
    <div className="space-y-3">
      {parsed.text && (
        <div>
          <div className="text-[10px] text-muted-foreground uppercase tracking-wider mb-1">
            Text
          </div>
          <div className="bg-green-50 dark:bg-green-950/30 rounded-xl rounded-bl-md p-3 text-xs whitespace-pre-wrap leading-relaxed">
            {parsed.text}
          </div>
        </div>
      )}

      {parsed.toolCalls && parsed.toolCalls.length > 0 && (
        <div>
          <div className="text-[10px] text-muted-foreground uppercase tracking-wider mb-1">
            Tool Calls ({parsed.toolCalls.length})
          </div>
          <ToolCallBlock toolCalls={parsed.toolCalls} />
        </div>
      )}

      {parsed.usage && (
        <div>
          <div className="text-[10px] text-muted-foreground uppercase tracking-wider mb-1">
            Usage
          </div>
          <div className="rounded border p-2 text-xs font-mono space-y-0.5">
            {Object.entries(parsed.usage).map(([key, val]) => (
              <div key={key} className="flex justify-between">
                <span className="text-muted-foreground">{key}</span>
                <span>{String(val)}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {parsed.finishReason && (
        <div className="flex items-center gap-2 text-xs">
          <span className="text-muted-foreground">Finish reason:</span>
          <code className="font-mono bg-muted px-1 rounded">{parsed.finishReason}</code>
        </div>
      )}
    </div>
  );
}
