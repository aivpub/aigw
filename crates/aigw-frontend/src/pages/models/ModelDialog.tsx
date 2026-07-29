import { useState, useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiGet, apiPost, apiPut } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Tabs,
  TabsList,
  TabsTrigger,
} from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { Plus } from "lucide-react";
import type { ModelItem } from "./types";

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

type AuthMode = "api_key" | "credential";

interface ModelFormData {
  model_name: string;
  upstream_model: string;
  custom_llm_provider: string;
  auth_mode: AuthMode;
  api_base: string;
  api_key: string;
  credential_name: string;
  rpm: string;
  tpm: string;
  input_price_per_million: string;
  output_price_per_million: string;
  cache_read_price_per_million: string;
  cache_create_price_per_million: string;
  chat_template_compat: string;
}

const PROVIDERS = [
  { value: "openai", label: "OpenAI" },
  { value: "anthropic", label: "Anthropic" },
  { value: "deepseek", label: "DeepSeek" },
  { value: "cohere", label: "Cohere" },
  { value: "google", label: "Google" },
  { value: "mistral", label: "Mistral" },
  { value: "openrouter", label: "OpenRouter" },
  { value: "azure", label: "Azure" },
  { value: "bedrock", label: "AWS Bedrock" },
  { value: "vertex_ai", label: "Vertex AI" },
];

const emptyForm = (): ModelFormData => ({
  model_name: "",
  upstream_model: "",
  custom_llm_provider: "",
  auth_mode: "api_key",
  api_base: "",
  api_key: "",
  credential_name: "",
  rpm: "",
  tpm: "",
  input_price_per_million: "",
  output_price_per_million: "",
  cache_read_price_per_million: "",
  cache_create_price_per_million: "",
  chat_template_compat: "",
});

function populateForm(model: ModelItem): ModelFormData {
  const p = (model.litellm_params ?? {}) as Record<string, unknown>;
  const info = (model.model_info ?? {}) as Record<string, unknown>;
  const rawInput = (info.input_cost_per_token as number) ?? (p.input_cost_per_token as number);
  const rawOutput = (info.output_cost_per_token as number) ?? (p.output_cost_per_token as number);
  const rawCacheRead = (info.cache_read_input_token_cost as number) ?? (p.cache_read_input_token_cost as number);
  const rawCacheCreate = (info.cache_creation_input_token_cost as number) ?? (p.cache_creation_input_token_cost as number);
  const hasCredential = !!(p.litellm_credential_name as string);

  return {
    model_name: model.model_name,
    upstream_model: (p.model as string) || model.model_name,
    custom_llm_provider: (p.custom_llm_provider as string) || "",
    auth_mode: hasCredential ? "credential" : "api_key",
    api_base: (p.api_base as string) || "",
    api_key: (p.api_key as string) ? "***" : "",
    credential_name: (p.litellm_credential_name as string) || "",
    rpm: p.rpm != null ? String(p.rpm) : "",
    tpm: p.tpm != null ? String(p.tpm) : "",
    input_price_per_million: rawInput != null ? String(rawInput * 1_000_000) : "",
    output_price_per_million: rawOutput != null ? String(rawOutput * 1_000_000) : "",
    cache_read_price_per_million: rawCacheRead != null ? String(rawCacheRead * 1_000_000) : "",
    cache_create_price_per_million: rawCacheCreate != null ? String(rawCacheCreate * 1_000_000) : "",
    chat_template_compat: (info.chat_template_compat as string) || "",
  };
}

function buildBody(form: ModelFormData, _original?: ModelItem): Record<string, unknown> {
  const inputCostPerToken = form.input_price_per_million !== ""
    ? parseFloat((parseFloat(form.input_price_per_million) / 1_000_000).toFixed(10))
    : undefined;
  const outputCostPerToken = form.output_price_per_million !== ""
    ? parseFloat((parseFloat(form.output_price_per_million) / 1_000_000).toFixed(10))
    : undefined;
  const cacheReadCost = form.cache_read_price_per_million !== ""
    ? parseFloat((parseFloat(form.cache_read_price_per_million) / 1_000_000).toFixed(10))
    : undefined;
  const cacheCreateCost = form.cache_create_price_per_million !== ""
    ? parseFloat((parseFloat(form.cache_create_price_per_million) / 1_000_000).toFixed(10))
    : undefined;

  const litellm_params: Record<string, unknown> = {
    model: form.upstream_model || form.model_name,
    custom_llm_provider: form.custom_llm_provider || undefined,
    rpm: form.rpm !== "" ? parseInt(form.rpm, 10) : undefined,
    tpm: form.tpm !== "" ? parseInt(form.tpm, 10) : undefined,
    input_cost_per_token: inputCostPerToken,
    output_cost_per_token: outputCostPerToken,
  };
  if (cacheReadCost !== undefined) {
    litellm_params.cache_read_input_token_cost = cacheReadCost;
  }
  if (cacheCreateCost !== undefined) {
    litellm_params.cache_creation_input_token_cost = cacheCreateCost;
  }

  if (form.auth_mode === "api_key") {
    if (form.api_base) litellm_params.api_base = form.api_base;
    // only send api_key if user actually changed it
    if (form.api_key && form.api_key !== "***") {
      litellm_params.api_key = form.api_key;
    }
  } else {
    if (form.credential_name) {
      litellm_params.litellm_credential_name = form.credential_name;
    }
  }

  const model_info: Record<string, unknown> = {
    input_cost_per_token: inputCostPerToken,
    output_cost_per_token: outputCostPerToken,
    cache_read_input_token_cost: cacheReadCost,
    cache_creation_input_token_cost: cacheCreateCost,
  };
  // Only write chat_template_compat if explicitly set (non-empty)
  if (form.chat_template_compat) {
    model_info.chat_template_compat = form.chat_template_compat;
  }

  const body: Record<string, unknown> = {
    model_name: form.model_name,
    litellm_params,
    model_info,
  };

  return body;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Sub-components
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface FormFieldProps {
  label: string;
  required?: boolean;
  description?: string;
  children: React.ReactNode;
  htmlFor?: string;
}

function FormField({ label, required, description, children, htmlFor }: FormFieldProps) {
  return (
    <div className="space-y-1">
      <Label className="text-xs font-medium" htmlFor={htmlFor}>
        {label}
        {required && <span className="text-destructive ml-0.5">*</span>}
      </Label>
      {children}
      {description && (
        <p className="text-[10px] text-muted-foreground">{description}</p>
      )}
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ModelDialog
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface ModelDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  model?: ModelItem | null; // null = create, object = edit
  onSaved: () => void;
  onError: (msg: string) => void;
}

export function ModelDialog({ open, onOpenChange, model, onSaved, onError }: ModelDialogProps) {
  const isEdit = model != null;
  const [form, setForm] = useState<ModelFormData>(emptyForm());
  const [upstreamManuallyEdited, setUpstreamManuallyEdited] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const { data: credData } = useQuery<{ data?: Array<{ credential_name: string }> }>({
    queryKey: ["credentials-list"],
    queryFn: () => apiGet("/credential/list"),
    enabled: open,
  });
  const credentials = credData?.data ?? [];

  // Reset form on open
  useEffect(() => {
    if (open) {
      if (model) {
        setForm(populateForm(model));
        setUpstreamManuallyEdited(true); // editing: already has a value
      } else {
        setForm(emptyForm());
        setUpstreamManuallyEdited(false);
      }
    }
  }, [open, model]);

  function update(field: keyof ModelFormData, value: string) {
    setForm((prev) => ({ ...prev, [field]: value }));
  }

  function handleModelNameChange(v: string) {
    update("model_name", v);
    if (!upstreamManuallyEdited) {
      setForm((prev) => ({ ...prev, upstream_model: v }));
    }
  }

  function handleUpstreamModelChange(v: string) {
    update("upstream_model", v);
    setUpstreamManuallyEdited(true);
  }

  function handleAuthModeChange(mode: AuthMode) {
    setForm((prev) => ({
      ...prev,
      auth_mode: mode,
      ...(mode === "credential"
        ? { api_base: "", api_key: "" }
        : { credential_name: "" }),
    }));
  }

  async function handleSubmit() {
    if (!form.model_name.trim()) {
      onError("Model name is required");
      return;
    }
    setSubmitting(true);
    try {
      const body = buildBody(form, model ?? undefined);
      if (isEdit) {
        await apiPut(`/model/update?model_id=${model!.model_id}`, body);
      } else {
        await apiPost("/model/new", body);
      }
      onSaved();
      onOpenChange(false);
    } catch (err) {
      onError((err as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit Model" : "Add Model"}</DialogTitle>
          <DialogDescription>
            {isEdit
              ? `Update proxy model configuration for "${model!.model_name}"`
              : "Create a new proxy model configuration"}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* Model Name */}
          <FormField label="Model Name" required htmlFor="model_name" description="Proxy model group name exposed via /v1/models">
            <Input
              id="model_name"
              value={form.model_name}
              onChange={(e) => handleModelNameChange(e.target.value)}
              placeholder="my-gpt-4"
              disabled={isEdit}
            />
          </FormField>

          {/* Upstream Model */}
          <FormField label="Upstream Model" required htmlFor="upstream_model" description="Auto-fills from Model Name. Edit to override (e.g. openai/gpt-4)">
            <Input
              id="upstream_model"
              value={form.upstream_model}
              onChange={(e) => handleUpstreamModelChange(e.target.value)}
            />
          </FormField>

          {/* Provider */}
          <FormField label="Provider" description="Upstream LLM provider">
            <Select
              value={form.custom_llm_provider || "none"}
              onValueChange={(v) => update("custom_llm_provider", v === "none" ? "" : v)}
            >
              <SelectTrigger className="h-9">
                <SelectValue placeholder="Select provider…" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="none">Auto-detect</SelectItem>
                {PROVIDERS.map((p) => (
                  <SelectItem key={p.value} value={p.value}>
                    {p.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>

          {/* Authentication Mode */}
          <div className="space-y-1">
            <Label className="text-xs font-medium">Authentication</Label>
            <Tabs
              defaultValue={form.auth_mode}
              value={form.auth_mode}
              onValueChange={(v) => handleAuthModeChange(v as AuthMode)}
            >
              <TabsList className="h-7">
                <TabsTrigger value="api_key" className="text-xs h-6">
                  API Key
                </TabsTrigger>
                <TabsTrigger value="credential" className="text-xs h-6">
                  Credential
                </TabsTrigger>
              </TabsList>
            </Tabs>
          </div>

          {/* API Key fields */}
          {form.auth_mode === "api_key" && (
            <div className="space-y-3 pl-1 border-l-2 border-muted">
              <FormField label="API Base">
                <Input
                  value={form.api_base}
                  onChange={(e) => update("api_base", e.target.value)}
                  placeholder="https://api.openai.com/v1"
                />
              </FormField>
              <FormField label="API Key">
                <Input
                  type="password"
                  value={form.api_key}
                  onChange={(e) => update("api_key", e.target.value)}
                  placeholder="sk-..."
                />
              </FormField>
            </div>
          )}

          {/* Credential fields */}
          {form.auth_mode === "credential" && (
            <div className="pl-1 border-l-2 border-muted">
              <FormField label="Credential" description="Select a stored credential">
                <div className="flex items-end gap-2">
                  <div className="flex-1">
                    <Select
                      value={form.credential_name || "none"}
                      onValueChange={(v) => update("credential_name", v === "none" ? "" : v)}
                    >
                      <SelectTrigger className="h-9">
                        <SelectValue placeholder="Select credential…" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="none">None</SelectItem>
                        {credentials.map((c) => (
                          <SelectItem key={c.credential_name} value={c.credential_name}>
                            {c.credential_name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <Button
                    variant="outline"
                    size="icon"
                    className="h-9 w-9 shrink-0"
                    title="New credential"
                    onClick={() => window.open("/#/credentials", "_blank")}
                  >
                    <Plus className="h-4 w-4" />
                  </Button>
                </div>
              </FormField>
            </div>
          )}

          {/* Pricing */}
          <div className="space-y-3 pt-1 border-t">
            <Label className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
              Pricing ($/1M tokens)
            </Label>
            <div className="grid grid-cols-2 gap-3">
              <FormField label="Input Price ($/1M)" description="e.g. 30 for GPT-4">
                <Input
                  type="number"
                  step="0.0001"
                  min="0"
                  value={form.input_price_per_million}
                  onChange={(e) => update("input_price_per_million", e.target.value)}
                  placeholder="30.00"
                />
              </FormField>
              <FormField label="Output Price ($/1M)" description="e.g. 60 for GPT-4">
                <Input
                  type="number"
                  step="0.0001"
                  min="0"
                  value={form.output_price_per_million}
                  onChange={(e) => update("output_price_per_million", e.target.value)}
                  placeholder="60.00"
                />
              </FormField>
              <FormField label="Cache Read Price ($/1M)" description="Prompt caching read price; typically 10%-50% of Input">
                <Input
                  type="number"
                  step="0.0001"
                  min="0"
                  value={form.cache_read_price_per_million}
                  onChange={(e) => update("cache_read_price_per_million", e.target.value)}
                  placeholder="3.00"
                />
              </FormField>
              <FormField label="Cache Write Price ($/1M)" description="Prompt caching write price; typically ~125% of Input">
                <Input
                  type="number"
                  step="0.0001"
                  min="0"
                  value={form.cache_create_price_per_million}
                  onChange={(e) => update("cache_create_price_per_million", e.target.value)}
                  placeholder="37.50"
                />
              </FormField>
            </div>
          </div>

          {/* Rate Limits */}
          <div className="space-y-3 pt-1 border-t">
            <Label className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
              Rate Limits
            </Label>
            <div className="grid grid-cols-2 gap-3">
              <FormField label="RPM" description="Requests per minute">
                <Input
                  type="number"
                  step="1"
                  min="0"
                  value={form.rpm}
                  onChange={(e) => update("rpm", e.target.value)}
                  placeholder="Unlimited"
                />
              </FormField>
              <FormField label="TPM" description="Tokens per minute">
                <Input
                  type="number"
                  step="1"
                  min="0"
                  value={form.tpm}
                  onChange={(e) => update("tpm", e.target.value)}
                  placeholder="Unlimited"
                />
              </FormField>
            </div>
          </div>

          {/* Chat Template Compatibility */}
          <div className="space-y-3 pt-1 border-t">
            <Label className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
              Chat Template Compatibility
            </Label>
            <FormField
              label="System Message Handling"
              description="Some models (e.g. Qwen) require system messages only at position 0, otherwise 400. Auto-detect by model name; override if misdetected."
            >
              <Select
                value={form.chat_template_compat || "auto"}
                onValueChange={(v) => update("chat_template_compat", v === "auto" ? "" : v)}
              >
                <SelectTrigger className="h-9">
                  <SelectValue placeholder="Auto-detect (recommended)" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="auto">Auto-detect (recommended)</SelectItem>
                  <SelectItem value="strict">Strict (Qwen-like templates)</SelectItem>
                  <SelectItem value="loose">Loose (passthrough)</SelectItem>
                </SelectContent>
              </Select>
            </FormField>
          </div>

          {/* Advanced: litellm_params extra */}
          <details className="text-xs">
            <summary className="cursor-pointer text-muted-foreground hover:text-foreground select-none">
              Advanced: litellm_params (raw JSON override)
            </summary>
            <Textarea
              className="mt-2 font-mono text-xs h-24"
              placeholder='{"key": "value"}'
            />
            <p className="text-[10px] text-muted-foreground mt-1">
              Use only fields not covered by the form above. Merged with form fields.
            </p>
          </details>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={submitting}>
            Cancel
          </Button>
          <Button onClick={handleSubmit} disabled={submitting}>
            {submitting ? "Saving…" : isEdit ? "Save Changes" : "Create Model"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
