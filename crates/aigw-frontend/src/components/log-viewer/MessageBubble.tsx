import { extractText } from "./utils";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// MessageBubble — role-specific message rendering
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface MessageBubbleProps {
  role: string;
  content: unknown;
}

const BUBBLE_COLORS: Record<string, string> = {
  user: "bg-blue-50 dark:bg-blue-950/30 ml-4 rounded-2xl rounded-br-md",
  assistant: "bg-green-50 dark:bg-green-950/30 mr-4 rounded-2xl rounded-bl-md",
};

export function MessageBubble({ role, content }: MessageBubbleProps) {
  const text = extractText(content);
  const bubbleClass =
    BUBBLE_COLORS[role] ?? "bg-muted/30 ml-4 rounded-2xl rounded-br-md";

  return (
    <div
      className={`p-2.5 text-xs whitespace-pre-wrap leading-relaxed ${bubbleClass}`}
    >
      {text || <span className="text-muted-foreground italic">(empty)</span>}
    </div>
  );
}
