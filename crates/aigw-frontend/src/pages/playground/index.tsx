import { useState, useRef, useCallback } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Switch } from "@/components/ui/switch";
import {
  Gamepad2,
  Send,
  StopCircle,
  Copy,
  Plus,
  Trash2,
  Pencil,
  Check,
  X,
  Settings2,
  User,
  Bot,
  Zap,
  Sparkles,
  Eraser,
  Code2,
} from "lucide-react";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface ModelItem {
  id: string;
}

interface ChatMessage {
  id: string;
  role: "system" | "user" | "assistant";
  content: string;
  timestamp: number;
  tokens?: { prompt: number; completion: number };
  error?: string;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Markdown renderer
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function renderMarkdown(text: string): string {
  let html = text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

  html = html.replace(/```(\w*)\n([\s\S]*?)```/g, (_m, _lang, code) =>
    `<pre class="bg-muted rounded-md p-3 my-2 overflow-x-auto text-xs font-mono"><code>${code}</code></pre>`
  );
  html = html.replace(/`([^`]+)`/g, '<code class="bg-muted px-1 rounded text-xs font-mono">$1</code>');
  html = html.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  html = html.replace(/\n\n/g, '</p><p class="mb-2">');
  html = html.replace(/\n/g, "<br>");
  return `<p class="mb-2">${html}</p>`;
}

function quoteName(s: string, max = 8): string {
  return s.length > max ? s.slice(0, max - 1) + "…" : s;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Chat Message Bubble
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function MessageBubble({
  msg,
  onEdit,
  onDelete,
  onCopy,
}: {
  msg: ChatMessage;
  onEdit?: (id: string, newContent: string) => void;
  onDelete?: (id: string) => void;
  onCopy?: (content: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [editText, setEditText] = useState(msg.content);

  const isUser = msg.role === "user";
  const isSystem = msg.role === "system";
  const isAsst = msg.role === "assistant";

  return (
    <div className={`flex gap-3 ${isUser ? "flex-row-reverse" : ""}`}>
      {/* Avatar */}
      <div
        className={`shrink-0 w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold border-2 ${
          isSystem
            ? "bg-violet-100 text-violet-700 border-violet-300 dark:bg-violet-900 dark:text-violet-300 dark:border-violet-700"
            : isUser
            ? "bg-blue-100 text-blue-700 border-blue-300 dark:bg-blue-900 dark:text-blue-300 dark:border-blue-700"
            : "bg-emerald-100 text-emerald-700 border-emerald-300 dark:bg-emerald-900 dark:text-emerald-300 dark:border-emerald-700"
        }`}
      >
        {isSystem ? <Settings2 className="h-4 w-4" /> : isUser ? <User className="h-4 w-4" /> : <Bot className="h-4 w-4" />}
      </div>

      {/* Content */}
      <div className={`flex-1 min-w-0 ${isUser ? "flex flex-col items-end" : ""}`}>
        <div className="flex items-center gap-2 mb-1">
          <span className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider">
            {isSystem ? "System" : isUser ? "You" : "Assistant"}
          </span>
          {msg.tokens && (
            <Badge variant="outline" className="text-[9px] px-1 py-0 h-4">
              {msg.tokens.prompt + msg.tokens.completion} tokens
            </Badge>
          )}
        </div>

        {/* Editing mode */}
        {editing ? (
          <div className="w-full space-y-2">
            <Textarea
              value={editText}
              onChange={(e) => setEditText(e.target.value)}
              className="min-h-[60px] text-sm resize-y"
            />
            <div className="flex gap-1">
              <Button size="sm" variant="outline" className="h-7 text-xs" onClick={() => { onEdit?.(msg.id, editText); setEditing(false); }}>
                <Check className="h-3 w-3 mr-1" /> Save & Resend
              </Button>
              <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={() => { setEditText(msg.content); setEditing(false); }}>
                <X className="h-3 w-3 mr-1" /> Cancel
              </Button>
            </div>
          </div>
        ) : msg.error ? (
          <div className="bg-red-50 dark:bg-red-950 border border-red-200 dark:border-red-800 rounded-lg p-3 text-sm text-red-700 dark:text-red-300">
            <p className="font-medium">Error</p>
            <p className="text-xs mt-1">{msg.error}</p>
          </div>
        ) : (
          <div
            className={`rounded-lg px-3 py-2.5 text-sm border ${
              isSystem
                ? "bg-violet-50 dark:bg-violet-950 border-violet-300 dark:border-violet-700"
                : isUser
                ? "bg-blue-50 dark:bg-blue-950 border-blue-300 dark:border-blue-700"
                : "bg-white dark:bg-zinc-900 border-gray-200 dark:border-zinc-700"
            }`}
          >
            {isAsst ? (
              <div className="prose prose-sm dark:prose-invert max-w-none" dangerouslySetInnerHTML={{ __html: renderMarkdown(msg.content) }} />
            ) : (
              <div className="whitespace-pre-wrap">{msg.content}</div>
            )}
          </div>
        )}

        {/* Bottom stats bar for assistant messages */}
        {isAsst && !msg.error && msg.content && (
          <div className="flex items-center gap-2 mt-1 text-[10px] text-muted-foreground">
            {msg.tokens && (
              <>
                <span>{msg.tokens.prompt}+{msg.tokens.completion} tokens</span>
                <span className="text-muted-foreground/50">|</span>
              </>
            )}
            <Button
              variant="ghost"
              size="icon"
              className="h-5 w-5"
              onClick={() => onCopy?.(msg.content)}
              title="Copy"
            >
              <Copy className="h-2.5 w-2.5" />
            </Button>
            {onDelete && (
              <Button
                variant="ghost"
                size="icon"
                className="h-5 w-5"
                onClick={() => onDelete(msg.id)}
                title="Delete"
              >
                <Trash2 className="h-2.5 w-2.5" />
              </Button>
            )}
          </div>
        )}

        {/* Action buttons for user/system */}
        {!isAsst && !editing && !msg.error && (
          <div className="flex items-center gap-1 mt-1 opacity-0 hover:opacity-100 transition-opacity">
            {(isUser || isSystem) && onEdit && (
              <Button variant="ghost" size="icon" className="h-6 w-6" onClick={() => { setEditText(msg.content); setEditing(true); }}>
                <Pencil className="h-3 w-3" />
              </Button>
            )}
            {onDelete && (
              <Button variant="ghost" size="icon" className="h-6 w-6" onClick={() => onDelete(msg.id)}>
                <Trash2 className="h-3 w-3" />
              </Button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Settings Panel
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface SettingsData {
  model: string;
  temperature: number;
  maxTokens: number;
  streaming: boolean;
  topP: number;
  freqPenalty: number;
  presPenalty: number;
  virtualKey: "session" | "custom";
  customApiKey: string;
  endpointType: "chat" | "messages";
}

const DEFAULT_SETTINGS: SettingsData = {
  model: "",
  temperature: 0.7,
  maxTokens: 4096,
  streaming: true,
  topP: 1.0,
  freqPenalty: 0,
  presPenalty: 0,
  virtualKey: "session",
  customApiKey: "",
  endpointType: "chat",
};

function SettingsPanel({
  settings,
  onChange,
  models,
}: {
  settings: SettingsData;
  onChange: (s: SettingsData) => void;
  models: ModelItem[];
}) {
  return (
    <div className="space-y-4">
      <div className="flex flex-col gap-1.5">
        <Label className="text-xs">Model</Label>
        <Select value={settings.model} onValueChange={(v) => onChange({ ...settings, model: v })}>
          <SelectTrigger className="h-8 text-xs">
            <SelectValue placeholder="Select a model" />
          </SelectTrigger>
          <SelectContent>
            {models.map((m) => (
              <SelectItem key={m.id} value={m.id}>{m.id}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-xs">Temperature ({settings.temperature.toFixed(2)})</Label>
        <Input type="range" min="0" max="2" step="0.01" value={settings.temperature}
          onChange={(e) => onChange({ ...settings, temperature: parseFloat(e.target.value) })} className="h-8" />
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-xs">Max Tokens</Label>
        <Input type="number" min={1} max={200000} value={settings.maxTokens}
          onChange={(e) => onChange({ ...settings, maxTokens: parseInt(e.target.value) || 4096 })} className="h-8 text-xs" />
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-xs">Top P ({settings.topP.toFixed(2)})</Label>
        <Input type="range" min="0" max="1" step="0.01" value={settings.topP}
          onChange={(e) => onChange({ ...settings, topP: parseFloat(e.target.value) })} className="h-8" />
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-xs">Frequency Penalty ({settings.freqPenalty.toFixed(2)})</Label>
        <Input type="range" min="-2" max="2" step="0.01" value={settings.freqPenalty}
          onChange={(e) => onChange({ ...settings, freqPenalty: parseFloat(e.target.value) })} className="h-8" />
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-xs">Presence Penalty ({settings.presPenalty.toFixed(2)})</Label>
        <Input type="range" min="-2" max="2" step="0.01" value={settings.presPenalty}
          onChange={(e) => onChange({ ...settings, presPenalty: parseFloat(e.target.value) })} className="h-8" />
      </div>

      <div className="flex items-center justify-between">
        <Label className="text-xs">Streaming</Label>
        <Switch checked={settings.streaming} onCheckedChange={(v) => onChange({ ...settings, streaming: v })} />
      </div>

      <div className="border-t pt-4 mt-2">
        <Label className="text-xs font-semibold mb-2 block">Virtual Key</Label>
        <Select value={settings.virtualKey} onValueChange={(v) => onChange({ ...settings, virtualKey: v as "session" | "custom" })}>
          <SelectTrigger className="h-8 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="session">Current UI Session (Cookie)</SelectItem>
            <SelectItem value="custom">Custom Virtual Key (SK)</SelectItem>
          </SelectContent>
        </Select>
        {settings.virtualKey === "custom" && (
          <Input
            type="password"
            placeholder="sk-..."
            value={settings.customApiKey}
            onChange={(e) => onChange({ ...settings, customApiKey: e.target.value })}
            className="h-8 text-xs mt-2"
          />
        )}
      </div>

      <div className="border-t pt-4 mt-2">
        <Label className="text-xs font-semibold mb-2 block">Endpoint Type</Label>
        <Select value={settings.endpointType} onValueChange={(v) => onChange({ ...settings, endpointType: v as "chat" | "messages" })}>
          <SelectTrigger className="h-8 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="chat">Chat Completions (/v1/chat/completions)</SelectItem>
            <SelectItem value="messages">Claude Messages (/v1/messages)</SelectItem>
          </SelectContent>
        </Select>
        {settings.endpointType === "messages" && (
          <p className="text-[10px] text-muted-foreground mt-1">Uses Anthropic Messages API format</p>
        )}
      </div>
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Main Component
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function PlaygroundPage() {
  const [settings, setSettings] = useState<SettingsData>(DEFAULT_SETTINGS);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [getCodeOpen, setGetCodeOpen] = useState(false);
  const abortRef = useRef<AbortController | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const { data: modelsData, isLoading: modelsLoading } = useQuery<{ data: ModelItem[] }>({
    queryKey: ["models"],
    queryFn: () => apiGet("/v1/models"),
  });
  const models = modelsData?.data ?? [];

  const scrollToBottom = () => {
    setTimeout(() => messagesEndRef.current?.scrollIntoView({ behavior: "smooth" }), 100);
  };

  const addMessage = useCallback((msg: ChatMessage) => {
    setMessages((prev) => [...prev, msg]);
  }, []);

  const updateMessage = useCallback((id: string, update: Partial<ChatMessage>) => {
    setMessages((prev) => prev.map((m) => (m.id === id ? { ...m, ...update } : m)));
  }, []);

  const deleteMessage = useCallback((id: string) => {
    setMessages((prev) => prev.filter((m) => m.id !== id));
  }, []);

  const clearChat = () => {
    setMessages([]);
    setInput("");
    setSettings(DEFAULT_SETTINGS);
  };

  const clearSession = () => {
    setMessages([]);
    setInput("");
  };

  const handleEdit = useCallback((id: string, newContent: string) => {
    // Remove from edited message onward, then resend
    const idx = messages.findIndex((m) => m.id === id);
    if (idx === -1) return;
    const truncated = messages.slice(0, idx);
    setMessages(truncated);
    // Trigger resend with the new content in the input
    setInput(newContent);
  }, [messages]);

  const handleSend = useCallback(async () => {
    const content = input.trim();
    if (!content || !settings.model || sending) return;

    setInput("");
    setSending(true);
    scrollToBottom();

    const userMsg: ChatMessage = {
      id: crypto.randomUUID?.() || Math.random().toString(36).slice(2),
      role: "user",
      content,
      timestamp: Date.now(),
    };

    // Find system message if it exists
    const systemMsg = messages.length > 0 && messages[0].role === "system" ? messages[0] : null;
    const conversationMsgs = systemMsg ? messages.slice(1) : messages;
    const newMessages = systemMsg ? [systemMsg, ...conversationMsgs, userMsg] : [...messages, userMsg];
    setMessages(newMessages);

    const asstId = crypto.randomUUID?.() || Math.random().toString(36).slice(2);
    const asstMsg: ChatMessage = {
      id: asstId,
      role: "assistant",
      content: "",
      timestamp: Date.now(),
    };
    addMessage(asstMsg);
    scrollToBottom();

    const controller = new AbortController();
    abortRef.current = controller;

    // Build apiMessages from conversation history (used by both endpoint types)
    const apiMessages: { role: string; content: string }[] = [];
    for (const msg of newMessages) {
      if (msg.role === "system" || msg.role === "user" || (msg.role === "assistant" && msg.id !== asstId)) {
        apiMessages.push({ role: msg.role, content: msg.content });
      }
    }

    try {
      const isMessages = settings.endpointType === "messages";
      const endpoint = isMessages ? "/v1/messages" : "/v1/chat/completions";

      const headers: Record<string, string> = { "Content-Type": "application/json" };
      if (isMessages) {
        headers["anthropic-version"] = "2023-06-01";
      }
      if (settings.virtualKey === "custom" && settings.customApiKey) {
        headers["x-api-key"] = settings.customApiKey;
      }

      let body: Record<string, unknown>;
      if (isMessages) {
        const systemMsg = apiMessages.find((m) => m.role === "system");
        const convMsgs = apiMessages.filter((m) => m.role !== "system");
        body = {
          model: settings.model,
          messages: convMsgs.map((m) => ({ role: m.role, content: m.content })),
          max_tokens: settings.maxTokens,
          stream: settings.streaming,
          ...(systemMsg ? { system: systemMsg.content } : {}),
          ...(settings.temperature > 0 ? { temperature: settings.temperature } : {}),
          ...(settings.topP < 1.0 ? { top_p: settings.topP } : {}),
        };
      } else {
        body = {
          model: settings.model,
          messages: apiMessages,
          temperature: settings.temperature,
          max_tokens: settings.maxTokens,
          top_p: settings.topP,
          frequency_penalty: settings.freqPenalty,
          presence_penalty: settings.presPenalty,
          stream: settings.streaming,
        };
      }

      const res = await fetch(endpoint, {
        method: "POST",
        headers,
        credentials: settings.virtualKey === "session" ? "include" : undefined as unknown as RequestCredentials,
        body: JSON.stringify(body),
        signal: controller.signal,
      });

      if (!res.ok) {
        const err = await res.json().catch(() => ({}));
        throw new Error(err.error?.message || `Request failed (${res.status})`);
      }

      if (settings.streaming) {
        const reader = res.body?.getReader();
        if (!reader) throw new Error("No response body");
        const decoder = new TextDecoder();
        let buf = "";
        let fullContent = "";

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          buf += decoder.decode(value, { stream: true });
          const lines = buf.split("\n");
          buf = lines.pop() ?? "";
          for (const line of lines) {
            const trimmed = line.trim();
            if (!trimmed || !trimmed.startsWith("data: ")) continue;
            const data = trimmed.slice(6);
            if (data === "[DONE]") continue;
            // Skip Anthropic SSE event: lines
            if (trimmed.startsWith("event: ")) continue;
            try {
              const parsed = JSON.parse(data);
              // Extract content: OpenAI uses choices[0].delta.content,
              // Anthropic SSE uses content_block_delta.delta.text
              const chunk = isMessages
                ? (parsed.delta?.text ?? undefined)
                : parsed.choices?.[0]?.delta?.content;
              if (chunk) {
                fullContent += chunk;
                updateMessage(asstId, { content: fullContent });
                // Auto-scroll during streaming to keep latest content visible
                setTimeout(() => messagesEndRef.current?.scrollIntoView({ behavior: "instant" }), 0);
              }
            } catch { /* skip */ }
          }
        }
      } else {
        const json = await res.json();
        const respContent = isMessages
          ? (json.content?.filter((c: { type: string }) => c.type === "text").map((c: { text: string }) => c.text).join("") ?? "")
          : json.choices?.[0]?.message?.content ?? "";
        const usage = isMessages
          ? json.usage
          : json.usage;
        updateMessage(asstId, {
          content: respContent,
          tokens: usage
            ? isMessages
              ? { prompt: usage.input_tokens ?? 0, completion: usage.output_tokens ?? 0 }
              : { prompt: usage.prompt_tokens ?? 0, completion: usage.completion_tokens ?? 0 }
            : undefined,
        });
      }
    } catch (err: unknown) {
      if (err instanceof DOMException && err.name === "AbortError") {
        updateMessage(asstId, { content: updateMessage.toString(), error: undefined });
      } else {
        updateMessage(asstId, {
          error: err instanceof Error ? err.message : "Request failed",
        });
      }
    } finally {
      setSending(false);
      abortRef.current = null;
      scrollToBottom();
    }
  }, [input, settings, messages, sending, addMessage, updateMessage]);

  const handleCancel = () => {
    abortRef.current?.abort();
    setSending(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  if (modelsLoading) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-8 w-40" />
        <Skeleton className="h-96 w-full" />
      </div>
    );
  }

  return (
    <div className="h-[calc(100vh-12rem)] flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between mb-4 shrink-0">
        <div>
          <h1 className="text-xl font-bold tracking-tight flex items-center gap-2">
            <Gamepad2 className="h-5 w-5" />
            Playground
          </h1>
          <p className="text-xs text-muted-foreground">Test chat completions interactively</p>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={clearChat} className="h-8 text-xs">
            <Plus className="h-3.5 w-3.5 mr-1" /> New Chat
          </Button>
          <Button variant="ghost" size="sm" onClick={clearSession} className="h-8 text-xs" disabled={messages.length === 0}>
            <Eraser className="h-3.5 w-3.5 mr-1" /> Clear Session
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setGetCodeOpen(true)} className="h-8 text-xs">
            <Code2 className="h-3.5 w-3.5 mr-1" /> Get Code
          </Button>
          {/* Mobile settings toggle */}
          <Button variant="outline" size="sm" className="h-8 text-xs lg:hidden" onClick={() => setSettingsOpen(true)}>
            <Settings2 className="h-3.5 w-3.5 mr-1" /> Settings
          </Button>
        </div>
      </div>

      {/* Main area */}
      <div className="flex-1 flex gap-4 min-h-0">
        {/* Chat area */}
        <div className="flex-1 flex flex-col min-w-0 min-h-0">
          {/* Messages */}
          <div className="flex-1 overflow-y-auto space-y-4 pr-2 min-h-0">
            {messages.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-full text-center gap-3">
                <Sparkles className="h-12 w-12 text-muted-foreground/50" />
                <div>
                  <p className="text-sm font-medium text-muted-foreground">Start a conversation</p>
                  <p className="text-xs text-muted-foreground/70 mt-1">Type a message below to begin testing</p>
                </div>
              </div>
            ) : (
              messages.map((msg) => (
                <MessageBubble
                  key={msg.id}
                  msg={msg}
                  onEdit={handleEdit}
                  onDelete={deleteMessage}
                  onCopy={(c) => { navigator.clipboard.writeText(c).catch(() => {}); }}
                />
              ))
            )}
            <div ref={messagesEndRef} />
          </div>

          {/* Input area */}
          <div className="shrink-0 mt-4">
            <div className="flex gap-2">
              <Textarea
                placeholder="Type a message… (Shift+Enter for new line)"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                disabled={sending}
                className="min-h-[44px] max-h-[120px] text-sm resize-none"
                rows={1}
              />
              <div className="flex flex-col gap-1 shrink-0">
                {sending ? (
                  <Button onClick={handleCancel} variant="destructive" size="icon" className="h-11 w-11">
                    <StopCircle className="h-5 w-5" />
                  </Button>
                ) : (
                  <Button
                    onClick={handleSend}
                    disabled={!input.trim() || !settings.model}
                    size="icon"
                    className="h-11 w-11"
                  >
                    <Send className="h-5 w-5" />
                  </Button>
                )}
              </div>
            </div>
            <p className="text-[10px] text-muted-foreground mt-1">
              {settings.model ? (
                <span className="flex items-center gap-2">
                  <span>Model: <Badge variant="outline" className="text-[9px] px-1 py-0 h-4">{quoteName(settings.model, 20)}</Badge></span>
                  {settings.streaming && <span className="flex items-center gap-0.5"><Zap className="h-2.5 w-2.5" /> Stream</span>}
                </span>
              ) : (
                "Select a model to begin"
              )}
            </p>
          </div>
        </div>

        {/* Desktop settings sidebar */}
        <div className="hidden lg:block w-60 shrink-0 overflow-y-auto">
          <div className="border rounded-lg p-4">
            <h3 className="text-sm font-medium mb-3 flex items-center gap-2">
              <Settings2 className="h-4 w-4" /> Settings
            </h3>
            <SettingsPanel settings={settings} onChange={setSettings} models={models} />
          </div>
        </div>

        {/* Mobile settings sheet */}
        {/* Mobile settings sheet */}
        <Sheet open={settingsOpen} onOpenChange={setSettingsOpen}>
          <SheetContent side="right">
            <SheetHeader>
              <SheetTitle>Settings</SheetTitle>
            </SheetHeader>
            <div className="mt-4">
              <SettingsPanel settings={settings} onChange={setSettings} models={models} />
            </div>
          </SheetContent>
        </Sheet>

        {/* Get Code Dialog */}
        <Dialog open={getCodeOpen} onOpenChange={setGetCodeOpen}>
          <DialogContent className="max-w-2xl">
            <DialogHeader>
              <DialogTitle>Get Code</DialogTitle>
            </DialogHeader>
            <Tabs defaultValue="curl">
              <TabsList>
                <TabsTrigger value="curl">curl</TabsTrigger>
                <TabsTrigger value="openai">OpenAI SDK</TabsTrigger>
                <TabsTrigger value="enio">Enio</TabsTrigger>
              </TabsList>
              <TabsContent value="curl">
                <pre className="bg-muted p-4 rounded-md text-xs overflow-auto max-h-96">
                  <code>{(() => {
                    const isMsg = settings.endpointType === "messages";
                    const ep = isMsg ? "/v1/messages" : "/v1/chat/completions";
                    const hdrs = [
                      '  -H "Content-Type: application/json" \\',
                      isMsg ? '  -H "anthropic-version: 2023-06-01" \\' : null,
                      '  -H "x-api-key: sk-xxx" \\',
                    ].filter(Boolean).join("\n");
                    const bd = isMsg
                      ? JSON.stringify({
                          model: settings.model,
                          max_tokens: settings.maxTokens,
                          stream: settings.streaming,
                          messages: [{ role: "user", content: "Hello" }],
                        }, null, 2)
                      : JSON.stringify({
                          model: settings.model,
                          messages: [{ role: "user", content: "Hello" }],
                          temperature: settings.temperature,
                          max_tokens: settings.maxTokens,
                          stream: settings.streaming,
                        }, null, 2);
                    return `curl -X POST http://localhost:3000${ep} \\\n${hdrs}\n  -d '${bd}'`;
                  })()}</code>
                </pre>
              </TabsContent>
              <TabsContent value="openai">
                <pre className="bg-muted p-4 rounded-md text-xs overflow-auto max-h-96">
                  <code>{`from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:3000/v1",
    api_key="sk-xxx",
)

response = client.chat.completions.create(
    model="${settings.model}",
    messages=[{"role": "user", "content": "Hello"}],
    temperature=${settings.temperature},
    max_tokens=${settings.maxTokens},
    stream=${settings.streaming ? "True" : "False"},
)
print(response.choices[0].message.content)`}</code>
                </pre>
              </TabsContent>
              <TabsContent value="enio">
                <pre className="bg-muted p-4 rounded-md text-xs overflow-auto max-h-96">
                  <code>{`import { EnioAI } from "enio";

const enio = new EnioAI({
    baseURL: "http://localhost:3000/v1",
    apiKey: "sk-xxx",
});

const response = await enio.chat.completions.create({
    model: "${settings.model}",
    messages: [{ role: "user", content: "Hello" }],
    temperature: ${settings.temperature},
    maxTokens: ${settings.maxTokens},
});

console.log(response.content);`}</code>
                </pre>
              </TabsContent>
            </Tabs>
          </DialogContent>
        </Dialog>
      </div>
    </div>
  );
}
