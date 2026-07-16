import { useState } from "react";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ToolCallBlock — collapsible tool call display
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface ToolCallInfo {
  id?: string;
  function?: { name?: string; arguments?: string };
  name?: string;
  input?: unknown;
  type?: string;
}

interface ToolCallBlockProps {
  toolCalls: unknown[];
}

export function ToolCallBlock({ toolCalls }: ToolCallBlockProps) {
  return (
    <div className="space-y-1 ml-4">
      {toolCalls.map((tc, i) => {
        const t = tc as ToolCallInfo;
        const funcName = t.function?.name ?? t.name ?? `tool_${i}`;
        const args = t.function?.arguments ?? t.input;
        return <ToolCallItem key={i} name={funcName} args={args} />;
      })}
    </div>
  );
}

function ToolCallItem({ name, args }: { name: string; args: unknown }) {
  const [open, setOpen] = useState(false);

  let parsedArgs: unknown = args;
  if (typeof args === "string") {
    try {
      parsedArgs = JSON.parse(args);
    } catch {
      // keep as string
    }
  }

  const argsStr =
    parsedArgs && typeof parsedArgs === "object"
      ? JSON.stringify(parsedArgs, null, 2)
      : String(args ?? "");

  return (
    <div className="bg-orange-50 dark:bg-orange-950/30 border border-orange-200 dark:border-orange-800 rounded p-1.5 text-xs">
      <button
        type="button"
        className="flex items-center gap-1 w-full text-left font-medium text-orange-700 dark:text-orange-300"
        onClick={() => setOpen(!open)}
      >
        <span className="text-[10px]">{open ? "▾" : "▸"}</span>
        <code className="text-[11px]">{name}</code>
      </button>
      {open && argsStr && (
        <pre className="mt-1 whitespace-pre-wrap break-all text-[10px] leading-relaxed max-h-48 overflow-y-auto text-muted-foreground pl-4">
          {argsStr}
        </pre>
      )}
    </div>
  );
}
