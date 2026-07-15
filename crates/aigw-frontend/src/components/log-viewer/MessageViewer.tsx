import { MessageBubble } from "./MessageBubble";
import { ToolCallBlock } from "./ToolCallBlock";
import { extractText } from "./utils";
import { useState } from "react";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export interface ParsedRequest {
  messages: Array<{ role: string; content: unknown; tool_calls?: unknown[]; name?: string; tool_call_id?: string }>;
  tools: unknown[] | null;
}

/**
 * Parse the stored "messages" field from SpendLog.
 *
 * The field can be one of:
 *   1. An array of message objects (e.g. from Anthropic /v1/messages)
 *   2. A JSON string of the above
 *   3. An OpenAI request body object: `{"model":"gpt-4","messages":[...],"tools":[...],...}`
 *   4. A JSON string of the above
 *
 * Case 3 is the most common — the backend stores the full adapted request body
 * in the `messages` column.
 */
export function parseMessages(raw: unknown): ParsedRequest {
  const empty: ParsedRequest = { messages: [], tools: null };
  if (!raw) return empty;

  try {
    const obj = typeof raw === "string" ? JSON.parse(raw) : raw;
    if (!obj || typeof obj !== "object") return empty;

    // Case 1/2: bare array of messages
    if (Array.isArray(obj)) {
      return {
        messages: (obj as Array<Record<string, unknown>>).map(normalizeMsg),
        tools: null,
      };
    }

    const record = obj as Record<string, unknown>;

    // Case 3/4: request body wrapper — extract { messages, tools }
    if (Array.isArray(record.messages)) {
      return {
        messages: (record.messages as Array<Record<string, unknown>>).map(normalizeMsg),
        tools: Array.isArray(record.tools) ? (record.tools as unknown[]) : null,
      };
    }

    // Fallback: try to find any array field named "messages"
    return empty;
  } catch {
    return empty;
  }
}

function normalizeMsg(m: Record<string, unknown>) {
  return {
    role: String(m.role ?? "unknown"),
    content: m.content,
    tool_calls: m.tool_calls as unknown[] | undefined,
    name: m.name as string | undefined,
    tool_call_id: m.tool_call_id as string | undefined,
  };
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// MessageViewer
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const MAX_VISIBLE = 20;

interface MessageViewerProps {
  messages: unknown;
}

export function MessageViewer({ messages }: MessageViewerProps) {
  const [showAll, setShowAll] = useState(false);
  const parsed = parseMessages(messages);

  if (parsed.messages.length === 0 && !parsed.tools) {
    return (
      <p className="text-sm text-muted-foreground py-4 text-center">
        No messages in this request
      </p>
    );
  }

  const visible = showAll ? parsed.messages : parsed.messages.slice(0, MAX_VISIBLE);

  return (
    <div className="space-y-2">
      {/* Tools section — show what tools were available to the model */}
      {parsed.tools && parsed.tools.length > 0 && (
        <ToolsSection tools={parsed.tools} />
      )}

      {visible.map((msg, i) => {
        switch (msg.role) {
          case "system":
            return <SystemBlock key={i} content={msg.content} />;
          case "tool":
            return <ToolResultBlock key={i} content={msg.content} name={msg.name} />;
          case "assistant":
            return (
              <div key={i} className="space-y-1.5">
                <MessageBubble role="assistant" content={msg.content} />
                {msg.tool_calls && msg.tool_calls.length > 0 && (
                  <ToolCallBlock toolCalls={msg.tool_calls} />
                )}
              </div>
            );
          case "user":
          default:
            return <MessageBubble key={i} role={msg.role} content={msg.content} />;
        }
      })}
      {parsed.messages.length > MAX_VISIBLE && !showAll && (
        <button
          type="button"
          className="text-xs text-blue-600 hover:underline w-full text-center py-1"
          onClick={() => setShowAll(true)}
        >
          Show all {parsed.messages.length} messages
        </button>
      )}
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tools section
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function ToolsSection({ tools }: { tools: unknown[] }) {
  const [open, setOpen] = useState(false);

  return (
    <div className="bg-blue-50 dark:bg-blue-950/30 border border-blue-200 dark:border-blue-800 rounded p-2 text-xs">
      <button
        type="button"
        className="flex items-center gap-1 font-medium text-blue-700 dark:text-blue-300 w-full text-left"
        onClick={() => setOpen(!open)}
      >
        <span className="text-[10px]">{open ? "▾" : "▸"}</span>
        <span>{tools.length} tool{tools.length !== 1 ? "s" : ""} available</span>
      </button>
      {open && (
        <div className="mt-1.5 space-y-1">
          {tools.map((t, i) => {
            const tool = t as Record<string, unknown>;
            const func = (tool.function ?? {}) as Record<string, unknown>;
            return (
              <div key={i} className="bg-background/50 rounded p-1.5 text-[11px]">
                <div className="font-mono font-medium">{String(func.name ?? `tool_${i}`)}</div>
                {(func.description as string) && (
                  <div className="text-muted-foreground mt-0.5">{String(func.description)}</div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Sub-components
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function SystemBlock({ content }: { content: unknown }) {
  const text = extractText(content);
  return (
    <div className="bg-muted/40 border-l-2 border-muted-foreground/30 rounded p-2.5 text-xs whitespace-pre-wrap text-muted-foreground">
      {text}
    </div>
  );
}

function ToolResultBlock({ content, name }: { content: unknown; name?: string }) {
  const [open, setOpen] = useState(false);
  const text = extractText(content);
  const label = name ? `tool: ${name}` : "tool result";

  return (
    <div className="bg-orange-50 dark:bg-orange-950 border border-orange-200 dark:border-orange-800 rounded p-2 text-xs">
      <button
        type="button"
        className="flex items-center gap-1 font-medium text-orange-700 dark:text-orange-300 w-full text-left"
        onClick={() => setOpen(!open)}
      >
        <span className="text-[10px]">{open ? "▾" : "▸"}</span>
        <span>{label}</span>
      </button>
      {open && (
        <pre className="mt-1.5 whitespace-pre-wrap break-all text-[11px] leading-relaxed max-h-40 overflow-y-auto">
          {text}
        </pre>
      )}
    </div>
  );
}
