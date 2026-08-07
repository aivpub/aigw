/**
 * Extract human-readable text from various content formats:
 * - string → returned as-is
 * - array of content parts (OpenAI / Anthropic multi-part / Responses API) → joined
 * - object → JSON.stringify
 */
export function extractText(content: unknown): string {
  if (content === null || content === undefined) return "";
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((part: Record<string, unknown>) => {
        if (
          part.type === "text" ||
          part.type === "input_text" ||
          part.type === "output_text" ||
          part.type === "text_delta"
        )
          return String(part.text ?? "");
        if (part.type === "image_url" || part.type === "image")
          return "[Image]";
        if (part.type === "file")
          return `[File: ${String(part.filename ?? "unknown")}]`;
        if (part.type === "tool_use")
          return `[Tool: ${part.name ?? "unknown"}]`;
        if (part.type === "function_call")
          return `[Function call: ${String(part.name ?? "unknown")}]`;
        return JSON.stringify(part);
      })
      .join("\n");
  }
  if (typeof content === "object") return JSON.stringify(content, null, 2);
  return String(content);
}

/**
 * Recursively extract image data URLs from content.
 *
 * Handles OpenAI `image_url` parts (`{type:"image_url",image_url:{url}}`),
 * Anthropic `image` blocks (`{type:"image",source:{type:"base64",media_type,data}}`),
 * and nested containers (Responses API `output[].message.content[]`).
 *
 * Only `data:image/` URLs are returned — remote `https://` image URLs are NOT
 * rendered (admin detail view still avoids an arbitrary-URL fetch surface;
 * registered as TD-009e).
 */
export function extractImages(content: unknown): string[] {
  if (!content) return [];
  if (typeof content === "string") {
    return content.startsWith("data:image/") ? [content] : [];
  }
  if (Array.isArray(content)) {
    return content.flatMap((part: Record<string, unknown>) => {
      if (part.type === "image_url") {
        const url = (part.image_url as Record<string, unknown>)?.url;
        return typeof url === "string" && url.startsWith("data:image/")
          ? [url]
          : [];
      }
      if (part.type === "image") {
        const src = (part.source as Record<string, unknown>) ?? {};
        const data = String(src.data ?? "");
        const mt = String(src.media_type ?? "image/png");
        return data ? [`data:${mt};base64,${data}`] : [];
      }
      return extractImages(part as unknown);
    });
  }
  if (typeof content === "object" && content !== null) {
    return extractImages((content as Record<string, unknown>).content);
  }
  return [];
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
