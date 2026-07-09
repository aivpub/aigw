import { useState, useRef, useCallback } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiGet } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Gamepad2, Send, RotateCcw, Loader2 } from "lucide-react";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface ModelItem {
  id: string;
  object?: string;
  created?: number;
  owned_by?: string;
}

interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Simple Markdown-like renderer (no dependency)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function renderResponse(text: string): string {
  // Escape HTML, then convert code blocks and inline code
  let html = text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

  // Code blocks ```
  html = html.replace(/```(\w*)\n([\s\S]*?)```/g, (_m, lang, code) => {
    return `<pre class="bg-muted rounded-md p-3 my-2 overflow-x-auto text-xs font-mono"><code>${code}</code></pre>`;
  });

  // Inline code `
  html = html.replace(/`([^`]+)`/g, '<code class="bg-muted px-1 rounded text-xs font-mono">$1</code>');

  // Bold **
  html = html.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');

  // Line breaks
  html = html.replace(/\n\n/g, '</p><p class="mb-2">');
  html = html.replace(/\n/g, "<br>");

  return `<p class="mb-2">${html}</p>`;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Component
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function PlaygroundPage() {
  const [systemPrompt, setSystemPrompt] = useState("");
  const [userMessage, setUserMessage] = useState("");
  const [selectedModel, setSelectedModel] = useState("");
  const [temperature, setTemperature] = useState(0.7);
  const [maxTokens, setMaxTokens] = useState(1024);
  const [streaming, setStreaming] = useState(false);

  const [response, setResponse] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState("");
  const abortRef = useRef<AbortController | null>(null);

  const { data: modelsData, isLoading: modelsLoading } = useQuery<{ data: ModelItem[] }>({
    queryKey: ["models"],
    queryFn: () => apiGet("/v1/models"),
  });

  const models = modelsData?.data ?? [];

  const handleSend = useCallback(async () => {
    if (!userMessage.trim() || !selectedModel) return;

    setError("");
    setResponse("");
    setSending(true);

    const controller = new AbortController();
    abortRef.current = controller;

    const messages: ChatMessage[] = [];
    if (systemPrompt.trim()) {
      messages.push({ role: "system", content: systemPrompt.trim() });
    }
    messages.push({ role: "user", content: userMessage.trim() });

    try {
      const res = await fetch(`/v1/chat/completions`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer sk-dummy`,
        },
        credentials: "include",
        body: JSON.stringify({
          model: selectedModel,
          messages,
          temperature,
          max_tokens: maxTokens,
          stream: streaming,
        }),
        signal: controller.signal,
      });

      if (!res.ok) {
        const err = await res.json().catch(() => ({}));
        throw new Error(err.error?.message || `Request failed (${res.status})`);
      }

      if (streaming) {
        const reader = res.body?.getReader();
        if (!reader) throw new Error("No response body");
        const decoder = new TextDecoder();
        let buf = "";

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
            try {
              const parsed = JSON.parse(data);
              const content = parsed.choices?.[0]?.delta?.content;
              if (content) {
                setResponse((prev) => prev + content);
              }
            } catch {
              // skip unparseable chunks
            }
          }
        }
      } else {
        const json = await res.json();
        const content = json.choices?.[0]?.message?.content ?? "";
        setResponse(content);
      }
    } catch (err: unknown) {
      if (err instanceof DOMException && err.name === "AbortError") return;
      setError(err instanceof Error ? err.message : "Request failed");
    } finally {
      setSending(false);
      abortRef.current = null;
    }
  }, [systemPrompt, userMessage, selectedModel, temperature, maxTokens, streaming]);

  const handleCancel = () => {
    abortRef.current?.abort();
    setSending(false);
  };

  const handleReset = () => {
    setResponse("");
    setError("");
  };

  if (modelsLoading) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-8 w-40" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight flex items-center gap-2">
          <Gamepad2 className="h-6 w-6" />
          Playground
        </h1>
        <p className="text-sm text-muted-foreground">
          Test chat completions with your models
        </p>
      </div>

      <div className="grid gap-4 lg:grid-cols-[1fr_240px]">
        {/* Left column: messages */}
        <div className="space-y-4">
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium">System Prompt</CardTitle>
            </CardHeader>
            <CardContent>
              <Textarea
                placeholder="You are a helpful assistant. (optional)"
                value={systemPrompt}
                onChange={(e) => setSystemPrompt(e.target.value)}
                className="min-h-[80px] text-sm resize-y"
              />
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium">User Message</CardTitle>
            </CardHeader>
            <CardContent>
              <Textarea
                placeholder="Enter your message..."
                value={userMessage}
                onChange={(e) => setUserMessage(e.target.value)}
                className="min-h-[100px] text-sm resize-y"
              />
            </CardContent>
          </Card>

          <div className="flex items-center gap-2">
            {sending ? (
              <Button onClick={handleCancel} variant="destructive" size="sm">
                <Loader2 className="h-4 w-4 mr-1 animate-spin" />
                Cancel
              </Button>
            ) : (
              <Button
                onClick={handleSend}
                disabled={!userMessage.trim() || !selectedModel}
                size="sm"
              >
                <Send className="h-4 w-4 mr-1" />
                Send
              </Button>
            )}
            <Button onClick={handleReset} variant="outline" size="sm" disabled={sending}>
              <RotateCcw className="h-4 w-4 mr-1" />
              Clear
            </Button>
          </div>
        </div>

        {/* Right column: settings */}
        <div className="space-y-4">
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium">Settings</CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex flex-col gap-1.5">
                <Label className="text-xs">Model</Label>
                <Select value={selectedModel} onValueChange={setSelectedModel}>
                  <SelectTrigger className="h-9 text-sm">
                    <SelectValue placeholder="Select a model" />
                  </SelectTrigger>
                  <SelectContent>
                    {models.map((m) => (
                      <SelectItem key={m.id} value={m.id}>
                        {m.id}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <div className="flex flex-col gap-1.5">
                <Label className="text-xs">Temperature ({temperature})</Label>
                <Input
                  type="range"
                  min="0"
                  max="2"
                  step="0.1"
                  value={temperature}
                  onChange={(e) => setTemperature(parseFloat(e.target.value))}
                  className="h-8"
                />
              </div>

              <div className="flex flex-col gap-1.5">
                <Label className="text-xs">Max Tokens</Label>
                <Input
                  type="number"
                  min={1}
                  max={200000}
                  value={maxTokens}
                  onChange={(e) => setMaxTokens(parseInt(e.target.value) || 1024)}
                  className="h-8 text-xs"
                />
              </div>

              <div className="flex items-center justify-between">
                <Label className="text-xs">Streaming</Label>
                <Switch
                  checked={streaming}
                  onCheckedChange={setStreaming}
                />
              </div>
            </CardContent>
          </Card>
        </div>
      </div>

      {/* Response area */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium">Response</CardTitle>
        </CardHeader>
        <CardContent>
          {sending && !response ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground py-8 justify-center">
              <Loader2 className="h-4 w-4 animate-spin" />
              Sending...
            </div>
          ) : error ? (
            <div className="flex flex-col items-center justify-center py-8 gap-2">
              <p className="text-sm text-destructive">Request failed: {error}</p>
              <Button variant="outline" size="sm" onClick={handleSend}>
                Retry
              </Button>
            </div>
          ) : response ? (
            <div
              className="prose prose-sm dark:prose-invert max-w-none whitespace-pre-wrap"
              dangerouslySetInnerHTML={{ __html: renderResponse(response) }}
            />
          ) : (
            <div className="flex items-center justify-center py-8 text-sm text-muted-foreground">
              Enter a message and click Send to test
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
