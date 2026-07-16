import { useState, useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPut } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { toast } from "sonner";
import { Shuffle, RotateCcw, Save } from "lucide-react";

interface RouterSettingsValue {
  routing_strategy?: string;
  num_retries?: number;
  allowed_fails?: number;
  cooldown_time?: number;
}

const DEFAULTS: RouterSettingsValue = {
  routing_strategy: "simple-shuffle",
  num_retries: 0,
  allowed_fails: 3,
  cooldown_time: 5,
};

export function RouterSettingsPage() {
  const queryClient = useQueryClient();

  const { data, isLoading } = useQuery({
    queryKey: ["router-settings"],
    queryFn: () => apiGet("/router/settings"),
  });

  const [form, setForm] = useState<RouterSettingsValue>({ ...DEFAULTS });

  useEffect(() => {
    if (data) {
      const d = data as Record<string, unknown>;
      setForm({
        routing_strategy: (d.routing_strategy as string) ?? DEFAULTS.routing_strategy,
        num_retries: (d.num_retries as number) ?? DEFAULTS.num_retries,
        allowed_fails: (d.allowed_fails as number) ?? DEFAULTS.allowed_fails,
        cooldown_time: (d.cooldown_time as number) ?? DEFAULTS.cooldown_time,
      });
    }
  }, [data]);

  const saveMutation = useMutation({
    mutationFn: (body: RouterSettingsValue) => apiPut("/router/settings", body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["router-settings"] });
      toast.success("Router settings updated", {
        description: "Changes take effect immediately.",
      });
    },
    onError: (err: Error) => {
      toast.error("Failed to save", { description: err.message });
    },
  });

  const handleSave = () => {
    saveMutation.mutate(form);
  };

  const handleReset = () => {
    setForm({ ...DEFAULTS });
  };

  if (isLoading) {
    return (
      <div className="space-y-4 p-6">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }

  const isSaving = saveMutation.isPending;

  return (
    <div className="p-6 space-y-6 max-w-2xl">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Router Settings</h1>
          <p className="text-sm text-muted-foreground mt-1">
            Configure how aigw selects among multiple upstream deployments for the same model.
          </p>
        </div>
        <Button onClick={handleSave} disabled={isSaving}>
          {isSaving ? (
            <Spinner className="mr-2 h-4 w-4" />
          ) : (
            <Save className="mr-2 h-4 w-4" />
          )}
          Save
        </Button>
      </div>

      {/* Routing Strategy */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-lg">
            <Shuffle className="h-5 w-5" />
            Routing Strategy
          </CardTitle>
          <CardDescription>
            Determines how aigw picks an upstream deployment when multiple instances share the same model name.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="routing_strategy">Strategy</Label>
            <Select
              value={form.routing_strategy ?? DEFAULTS.routing_strategy}
              onValueChange={(v) => setForm((prev) => ({ ...prev, routing_strategy: v }))}
            >
              <SelectTrigger id="routing_strategy">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="simple-shuffle">Simple Shuffle (random)</SelectItem>
                <SelectItem value="least-busy" disabled>
                  Least Busy (coming soon)
                </SelectItem>
                <SelectItem value="usage-based-routing" disabled>
                  Usage-Based Routing (coming soon)
                </SelectItem>
                <SelectItem value="latency-based-routing" disabled>
                  Latency-Based Routing (coming soon)
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </CardContent>
      </Card>

      {/* Reliability Settings */}
      <Card>
        <CardHeader>
          <CardTitle className="text-lg">Reliability &amp; Retries</CardTitle>
          <CardDescription>
            Control failure tolerance and automatic cooldown behavior.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-3 gap-4">
            <div className="space-y-2">
              <Label htmlFor="num_retries">Retry Count</Label>
              <Input
                id="num_retries"
                type="number"
                min={0}
                max={10}
                value={form.num_retries ?? 0}
                onChange={(e) =>
                  setForm((prev) => ({
                    ...prev,
                    num_retries: Math.max(0, parseInt(e.target.value) || 0),
                  }))
                }
              />
              <p className="text-xs text-muted-foreground">
                Number of retries on failure (0–10).
              </p>
            </div>
            <div className="space-y-2">
              <Label htmlFor="allowed_fails">Allowed Failures</Label>
              <Input
                id="allowed_fails"
                type="number"
                min={1}
                max={100}
                value={form.allowed_fails ?? 3}
                onChange={(e) =>
                  setForm((prev) => ({
                    ...prev,
                    allowed_fails: Math.max(1, parseInt(e.target.value) || 1),
                  }))
                }
              />
              <p className="text-xs text-muted-foreground">
                Consecutive failures before cooldown (1–100).
              </p>
            </div>
            <div className="space-y-2">
              <Label htmlFor="cooldown_time">Cooldown (seconds)</Label>
              <Input
                id="cooldown_time"
                type="number"
                min={1}
                max={3600}
                value={form.cooldown_time ?? 5}
                onChange={(e) =>
                  setForm((prev) => ({
                    ...prev,
                    cooldown_time: Math.max(1, parseInt(e.target.value) || 1),
                  }))
                }
              />
              <p className="text-xs text-muted-foreground">
                Time a deployment stays on cooldown (1–3600s).
              </p>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Reset */}
      <div className="flex justify-end">
        <Button variant="outline" onClick={handleReset}>
          <RotateCcw className="mr-2 h-4 w-4" />
          Reset to Defaults
        </Button>
      </div>
    </div>
  );
}
