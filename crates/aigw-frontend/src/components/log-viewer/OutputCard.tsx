import { useState } from "react";
import { useTranslation } from "react-i18next";
import i18n from "@/i18n";
import { SectionHeader } from "./SectionHeader";
import { ToolCallBlock } from "./ToolCallBlock";
import { ImageThumbnails } from "./ImageThumbnails";
import { extractImages, extractText } from "./utils";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OutputCard — displays assistant response with tool calls
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface ParsedOutput {
  text: string;
  toolCalls: unknown[] | null;
  usage: Record<string, unknown> | null;
  finishReason: string | null;
  error: string | null;
  images: string[];
}

function parseOutput(raw: unknown): ParsedOutput {
  const empty: ParsedOutput = {
    text: "",
    toolCalls: null,
    usage: null,
    finishReason: null,
    error: null,
    images: [],
  };
  if (!raw) return empty;

  try {
    const r =
      typeof raw === "string"
        ? JSON.parse(raw)
        : (raw as Record<string, unknown>);
    if (!r || typeof r !== "object") return empty;

    // Detect error responses first
    if ((r as Record<string, unknown>).error) {
      const err = (r as Record<string, unknown>).error;
      let errMsg = "";
      if (typeof err === "string") {
        errMsg = err;
      } else if (typeof err === "object" && err !== null) {
        const e = err as Record<string, unknown>;
        errMsg =
          (e.message as string) || (e.code as string) || JSON.stringify(err);
      }
      return { ...empty, error: errMsg };
    }

    // OpenAI: choices[0].message
    if (Array.isArray((r as Record<string, unknown>).choices)) {
      const choices = (r as Record<string, unknown>).choices as Array<
        Record<string, unknown>
      >;
      const first = choices[0];
      if (first) {
        const msg = (first.message ?? first.delta ?? {}) as Record<
          string,
          unknown
        >;
        // Detect error in message (e.g. content filter)
        if (msg.refusal) {
          return { ...empty, error: `Refused: ${String(msg.refusal)}` };
        }
        return {
          text: extractText(msg.content),
          images: extractImages(msg.content),
          toolCalls: (msg.tool_calls as unknown[]) ?? null,
          usage:
            ((r as Record<string, unknown>).usage as Record<
              string,
              unknown
            > | null) ?? null,
          finishReason:
            (first.finish_reason as string) ??
            (msg.finish_reason as string) ??
            null,
          error: null,
        };
      }
    }

    // Anthropic: content[] blocks
    if (Array.isArray((r as Record<string, unknown>).content)) {
      const content = (r as Record<string, unknown>).content as Array<
        Record<string, unknown>
      >;
      const textParts: string[] = [];
      const toolCalls: unknown[] = [];
      const images = extractImages(content);
      for (const block of content) {
        if (block.type === "text" || block.type === "text_delta") {
          textParts.push(String(block.text ?? ""));
        } else if (block.type === "tool_use") {
          toolCalls.push({
            name: block.name,
            input: block.input,
            id: block.id,
          });
        }
      }
      return {
        text: textParts.join(""),
        images,
        toolCalls: toolCalls.length > 0 ? toolCalls : null,
        usage:
          ((r as Record<string, unknown>).usage as Record<
            string,
            unknown
          > | null) ?? null,
        finishReason:
          ((r as Record<string, unknown>).stop_reason as string) ?? null,
        error: null,
      };
    }

    // Responses API: output[].message.content[] (output_text / image_url blocks)
    if (Array.isArray((r as Record<string, unknown>).output)) {
      const output = (r as Record<string, unknown>).output as Array<
        Record<string, unknown>
      >;
      const textParts: string[] = [];
      const toolCalls: unknown[] = [];
      const images: string[] = [];
      for (const item of output) {
        if (item.type === "function_call") {
          toolCalls.push({
            name: item.name,
            arguments: item.arguments,
            call_id: item.call_id,
          });
        }
        if (item.type === "message") {
          const content = (item.content as Array<Record<string, unknown>>) ?? [];
          images.push(...extractImages(content));
          for (const block of content) {
            if (block.type === "output_text") {
              textParts.push(String(block.text ?? ""));
            } else if (block.type === "image_url") {
              images.push(...extractImages(block));
            }
          }
        }
      }
      return {
        text: textParts.join(""),
        images: [...new Set(images)],
        toolCalls: toolCalls.length > 0 ? toolCalls : null,
        usage:
          ((r as Record<string, unknown>).usage as Record<
            string,
            unknown
          > | null) ?? null,
        finishReason:
          ((r as Record<string, unknown>).status as string) ?? null,
        error: null,
      };
    }

    // Embeddings API: object=list with data[].embedding vectors.
    // Don't render the full vector array — just the dimension count + a short
    // truncated preview, so the detail drawer isn't flooded with a 1536-dim
    // JSON blob. Usage (prompt_tokens/total_tokens) is rendered by the caller.
    if (
      (r as Record<string, unknown>).object === "list" &&
      Array.isArray((r as Record<string, unknown>).data)
    ) {
      const data = (r as Record<string, unknown>).data as Array<
        Record<string, unknown>
      >;
      const first = data[0];
      const vector = (first?.embedding as unknown[] | undefined) ?? [];
      let text = "";
      if (vector.length > 0) {
        const preview = vector
          .slice(0, 8)
          .map((v) => (typeof v === "number" ? v.toFixed(4) : String(v)))
          .join(", ");
        text = `[${preview}${vector.length > 8 ? ", …" : ""}] (${
          vector.length
        } dims)`;
      } else {
        text = i18n.t("logViewer.embeddingsNoVectors");
      }
      return {
        text,
        images: [],
        toolCalls: null,
        usage:
          ((r as Record<string, unknown>).usage as Record<
            string,
            unknown
          > | null) ?? null,
        finishReason: `${data.length} vector${data.length !== 1 ? "s" : ""}`,
        error: null,
      };
    }

    return empty;
  } catch {
    return { ...empty, error: i18n.t("logViewer.parseError") };
  }
}

interface OutputCardProps {
  response: unknown;
  completionTokens: number;
  spend: number;
}

export function OutputCard({
  response,
  completionTokens,
  spend,
}: OutputCardProps) {
  const { t } = useTranslation();
  const [collapsed, setCollapsed] = useState(false);

  const parsed = parseOutput(response);

  const outputCost =
    spend > 0 ? spend * (completionTokens / (completionTokens + 1)) : undefined;

  return (
    <div className="border rounded-lg overflow-hidden">
      <SectionHeader
        type="output"
        tokens={completionTokens > 0 ? completionTokens : undefined}
        cost={outputCost}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed(!collapsed)}
      />

      {!collapsed && (
        <div className="p-3 space-y-2">
          {/* Error highlight */}
          {parsed.error ? (
            <div className="bg-red-50 dark:bg-red-950/20 border border-red-200 dark:border-red-800 rounded-lg p-3">
              <div className="flex items-center gap-1.5 mb-1">
                <span className="text-[10px] uppercase tracking-wider text-red-600 dark:text-red-400 font-medium">
                  {t("playground.error")}
                </span>
              </div>
              <pre className="text-xs text-red-700 dark:text-red-300 whitespace-pre-wrap break-all leading-relaxed font-mono">
                {parsed.error}
              </pre>
            </div>
          ) : null}

          {/* Plain text response */}
          {parsed.text ? (
            <div className="bg-green-50/30 dark:bg-green-950/10 rounded-lg p-2.5 text-xs whitespace-pre-wrap leading-relaxed">
              {parsed.text}
            </div>
          ) : null}

          {/* Stage 105: image thumbnails */}
          {parsed.images.length > 0 ? (
            <ImageThumbnails images={parsed.images} />
          ) : null}

          {/* Tool calls */}
          {parsed.toolCalls && parsed.toolCalls.length > 0 ? (
            <ToolCallBlock toolCalls={parsed.toolCalls} />
          ) : null}

          {/* Usage stats */}
          {parsed.usage ? (
            <div className="rounded border p-2 text-[11px] font-mono grid grid-cols-2 gap-x-4 gap-y-0.5">
              {Object.entries(parsed.usage).map(([key, val]) => (
                <div key={key} className="flex justify-between">
                  <span className="text-muted-foreground">{key}</span>
                  <span>{String(val)}</span>
                </div>
              ))}
            </div>
          ) : null}

          {/* Finish reason */}
          {parsed.finishReason ? (
            <div className="text-[11px] text-muted-foreground">
              {t("logViewer.finishReason")}:{" "}
              <code className="font-mono bg-muted px-1 rounded">
                {parsed.finishReason}
              </code>
            </div>
          ) : null}

          {/* Empty state */}
          {!parsed.error &&
          !parsed.text &&
          !parsed.toolCalls &&
          !parsed.usage ? (
            <p className="text-xs text-muted-foreground italic py-2 text-center">
              {t("logViewer.noContent")}
            </p>
          ) : null}
        </div>
      )}
    </div>
  );
}
