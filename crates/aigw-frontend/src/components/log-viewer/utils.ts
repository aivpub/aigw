/**
 * Extract human-readable text from various content formats:
 * - string → returned as-is
 * - array of content parts (OpenAI / Anthropic multi-part) → joined
 * - object → JSON.stringify
 */
export function extractText(content: unknown): string {
  if (content === null || content === undefined) return "";
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((part: Record<string, unknown>) => {
        if (part.type === "text" || part.type === "input_text")
          return String(part.text ?? "");
        if (part.type === "image_url" || part.type === "image")
          return "[Image]";
        if (part.type === "tool_use")
          return `[Tool: ${part.name ?? "unknown"}]`;
        return JSON.stringify(part);
      })
      .join("\n");
  }
  if (typeof content === "object") return JSON.stringify(content, null, 2);
  return String(content);
}

/**
 * Safe JSON.stringify — handles null/undefined, strings, and objects.
 */
export function safeStringify(v: unknown): string {
  if (v === null || v === undefined) return "";
  if (typeof v === "string") return v;
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}
