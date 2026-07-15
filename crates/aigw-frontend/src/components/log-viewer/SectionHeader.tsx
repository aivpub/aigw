import { useState } from "react";
import { extractText } from "./utils";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SectionHeader — Datadog-style collapsible section bar
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface SectionHeaderProps {
  type: "input" | "output";
  tokens?: number;
  cost?: number;
  onCopy: () => void;
  copied?: boolean;
  collapsed?: boolean;
  onToggleCollapse?: () => void;
  extra?: React.ReactNode;
}

export function SectionHeader({
  type,
  tokens,
  cost,
  onCopy,
  copied,
  collapsed,
  onToggleCollapse,
  extra,
}: SectionHeaderProps) {
  return (
    <div
      className={`flex items-center justify-between px-3 py-2 bg-muted/40 border-b cursor-pointer transition-colors hover:bg-muted/60 ${
        collapsed ? "border-b-0" : ""
      }`}
      onClick={onToggleCollapse}
    >
      <div className="flex items-center gap-3 text-xs">
        {onToggleCollapse && (
          <span className="text-[10px] text-muted-foreground w-3">
            {collapsed ? "▸" : "▾"}
          </span>
        )}
        <span className="flex items-center gap-1.5 font-medium">
          {type === "input" ? (
            <span className="text-blue-500">↑</span>
          ) : (
            <span className="text-green-500">↓</span>
          )}
          {type === "input" ? "Input" : "Output"}
        </span>
        {tokens != null && (
          <span className="text-muted-foreground">
            Tokens: {tokens.toLocaleString()}
          </span>
        )}
        {cost != null && (
          <span className="text-muted-foreground">
            Cost: ${cost.toFixed(6)}
          </span>
        )}
      </div>
      <div className="flex items-center gap-2">
        {extra}
        <button
          type="button"
          className="text-muted-foreground hover:text-foreground transition-colors p-0.5"
          onClick={(e) => {
            e.stopPropagation();
            onCopy();
          }}
          title="Copy"
        >
          {copied ? (
            <span className="text-green-500 text-xs">✓</span>
          ) : (
            <span className="text-xs">📋</span>
          )}
        </button>
      </div>
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// CollapsibleMessage — single collapsed/expanded message block
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface CollapsibleMessageProps {
  label: string;
  content: unknown;
  defaultExpanded?: boolean;
}

export function CollapsibleMessage({ label, content, defaultExpanded }: CollapsibleMessageProps) {
  const text = extractText(content);
  const short = text.length < 200;
  const [open, setOpen] = useState(defaultExpanded ?? short);

  if (!text) return null;

  return (
    <div className="border rounded mb-1.5 overflow-hidden">
      <button
        type="button"
        className="flex items-center gap-1.5 w-full text-left px-2 py-1 text-[11px] font-medium text-muted-foreground hover:bg-muted/30 transition-colors"
        onClick={() => setOpen(!open)}
      >
        <span className="text-[10px]">{open ? "▾" : "▸"}</span>
        <span className="uppercase tracking-wider text-[10px]">{label}</span>
        {!open && (
          <span className="text-[10px] text-muted-foreground truncate flex-1 ml-2">
            {text.slice(0, 60)}…
          </span>
        )}
      </button>
      {open && (
        <div className="px-3 py-2 text-xs whitespace-pre-wrap leading-relaxed border-t bg-background/50">
          {text}
        </div>
      )}
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// HistoryTree — tree-style history of N previous turns
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface HistoryTreeProps {
  messages: Array<{ role: string; content: unknown }>;
  defaultExpanded?: boolean;
}

export function HistoryTree({ messages, defaultExpanded }: HistoryTreeProps) {
  const [open, setOpen] = useState(defaultExpanded ?? false);

  if (messages.length === 0) return null;

  return (
    <div className="border rounded mb-1.5 overflow-hidden">
      <button
        type="button"
        className="flex items-center gap-1.5 w-full text-left px-2 py-1 text-[11px] font-medium text-muted-foreground hover:bg-muted/30 transition-colors"
        onClick={() => setOpen(!open)}
      >
        <span className="text-[10px]">{open ? "▾" : "▸"}</span>
        <span>HISTORY ({messages.length} turn{messages.length !== 1 ? "s" : ""})</span>
      </button>
      {open && (
        <div className="px-2 py-1.5 border-t bg-background/50 space-y-1 max-h-48 overflow-y-auto">
          {messages.map((msg, i) => (
            <div key={i} className="text-[11px] leading-relaxed flex items-start gap-1.5">
              <span className="uppercase text-[10px] text-muted-foreground shrink-0 w-10 text-right">
                {msg.role}:
              </span>
              <span className="text-muted-foreground truncate">
                {extractText(msg.content).slice(0, 120)}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
