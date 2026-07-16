import { useState, useEffect } from "react";
import { useSearchParams } from "react-router-dom";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { apiGet, apiPut, apiPatch } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Shared form component — used by all three tabs
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function RouterSettingsForm({
  form,
  onChange,
  saving,
  onSave,
}: {
  form: RouterSettingsValue;
  onChange: (f: RouterSettingsValue) => void;
  saving: boolean;
  onSave: () => void;
}) {
  return (
    <div className="space-y-6">
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
        <CardContent>
          <div className="space-y-2">
            <Label htmlFor="routing_strategy">Strategy</Label>
            <Select
              value={form.routing_strategy ?? DEFAULTS.routing_strategy}
              onValueChange={(v) => onChange({ ...form, routing_strategy: v })}
            >
              <SelectTrigger id="routing_strategy">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="simple-shuffle">Simple Shuffle (random)</SelectItem>
                <SelectItem value="least-busy" disabled>Least Busy (coming soon)</SelectItem>
                <SelectItem value="usage-based-routing" disabled>Usage-Based (coming soon)</SelectItem>
                <SelectItem value="latency-based-routing" disabled>Latency-Based (coming soon)</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-lg">Reliability &amp; Retries</CardTitle>
          <CardDescription>Control failure tolerance and automatic cooldown behavior.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-3 gap-4">
            <div className="space-y-2">
              <Label htmlFor="num_retries">Retry Count</Label>
              <Input
                id="num_retries"
                type="number" min={0} max={10}
                value={form.num_retries ?? 0}
                onChange={(e) => onChange({ ...form, num_retries: Math.max(0, parseInt(e.target.value) || 0) })}
              />
              <p className="text-xs text-muted-foreground">Number of retries on failure (0–10).</p>
            </div>
            <div className="space-y-2">
              <Label htmlFor="allowed_fails">Allowed Failures</Label>
              <Input
                id="allowed_fails"
                type="number" min={1} max={100}
                value={form.allowed_fails ?? 3}
                onChange={(e) => onChange({ ...form, allowed_fails: Math.max(1, parseInt(e.target.value) || 1) })}
              />
              <p className="text-xs text-muted-foreground">Consecutive failures before cooldown (1–100).</p>
            </div>
            <div className="space-y-2">
              <Label htmlFor="cooldown_time">Cooldown (seconds)</Label>
              <Input
                id="cooldown_time"
                type="number" min={1} max={3600}
                value={form.cooldown_time ?? 5}
                onChange={(e) => onChange({ ...form, cooldown_time: Math.max(1, parseInt(e.target.value) || 1) })}
              />
              <p className="text-xs text-muted-foreground">Time a deployment stays on cooldown (1–3600s).</p>
            </div>
          </div>
        </CardContent>
      </Card>

      <div className="flex justify-between">
        <Button variant="outline" onClick={() => onChange({ ...DEFAULTS })}>
          <RotateCcw className="mr-2 h-4 w-4" /> Reset to Defaults
        </Button>
        <Button onClick={onSave} disabled={saving}>
          {saving ? <Spinner className="mr-2 h-4 w-4" /> : <Save className="mr-2 h-4 w-4" />}
          Save
        </Button>
      </div>
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Global Tab
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function GlobalTab() {
  const queryClient = useQueryClient();

  const { data, isLoading } = useQuery({
    queryKey: ["router-settings-global"],
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
      queryClient.invalidateQueries({ queryKey: ["router-settings-global"] });
      toast.success("Global router settings updated");
    },
    onError: (err: Error) => toast.error("Failed to save", { description: err.message }),
  });

  if (isLoading) return <Skeleton className="h-64 w-full" />;

  return (
    <RouterSettingsForm
      form={form}
      onChange={setForm}
      saving={saveMutation.isPending}
      onSave={() => saveMutation.mutate(form)}
    />
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Keys Tab
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface KeyItem {
  token: string;
  key_alias?: string;
  key_name?: string;
  router_settings?: RouterSettingsValue;
}

function KeysTab() {
  const queryClient = useQueryClient();

  const { data: keysData, isLoading } = useQuery({
    queryKey: ["keys-list"],
    queryFn: () => apiGet("/key/list"),
  });

  const keys: KeyItem[] = (keysData as { data?: KeyItem[] })?.data ?? [];
  const [selectedToken, setSelectedToken] = useState<string>("");
  const [form, setForm] = useState<RouterSettingsValue>({ ...DEFAULTS });

  const selectedKey = keys.find((k) => k.token === selectedToken);

  useEffect(() => {
    if (selectedKey?.router_settings) {
      const rs = selectedKey.router_settings;
      setForm({
        routing_strategy: rs.routing_strategy ?? DEFAULTS.routing_strategy,
        num_retries: rs.num_retries ?? DEFAULTS.num_retries,
        allowed_fails: rs.allowed_fails ?? DEFAULTS.allowed_fails,
        cooldown_time: rs.cooldown_time ?? DEFAULTS.cooldown_time,
      });
    } else {
      setForm({ ...DEFAULTS });
    }
  }, [selectedToken, keys]);

  const saveMutation = useMutation({
    mutationFn: (body: RouterSettingsValue) => apiPatch(`/key/${selectedToken}/router/settings`, body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["keys-list"] });
      toast.success("Key router settings updated");
    },
    onError: (err: Error) => toast.error("Failed to save", { description: err.message }),
  });

  if (isLoading) return <Skeleton className="h-64 w-full" />;

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <Label htmlFor="key-select">Select Key</Label>
        <Select value={selectedToken || "none"} onValueChange={(v) => setSelectedToken(v === "none" ? "" : v)}>
          <SelectTrigger id="key-select">
            <SelectValue placeholder="Select a key to configure…" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="none">— Select a key —</SelectItem>
            {keys.map((k) => (
              <SelectItem key={k.token} value={k.token}>
                {k.key_alias || k.key_name || k.token.slice(0, 16) + "…"}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {selectedToken ? (
        <RouterSettingsForm
          form={form}
          onChange={setForm}
          saving={saveMutation.isPending}
          onSave={() => saveMutation.mutate(form)}
        />
      ) : (
        <p className="text-sm text-muted-foreground py-8 text-center">
          Select a key above to configure its router settings. Settings here override the global defaults.
        </p>
      )}
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Teams Tab
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

interface TeamItem {
  team_id: string;
  team_alias?: string;
  team_name?: string;
  router_settings?: RouterSettingsValue;
}

function TeamsTab() {
  const queryClient = useQueryClient();

  const { data: teamsData, isLoading } = useQuery({
    queryKey: ["teams-list"],
    queryFn: () => apiGet("/team/list"),
  });

  const teams: TeamItem[] = (teamsData as { data?: TeamItem[] })?.data ?? [];
  const [selectedId, setSelectedId] = useState<string>("");
  const [form, setForm] = useState<RouterSettingsValue>({ ...DEFAULTS });

  const selectedTeam = teams.find((t) => t.team_id === selectedId);

  useEffect(() => {
    if (selectedTeam?.router_settings) {
      const rs = selectedTeam.router_settings;
      setForm({
        routing_strategy: rs.routing_strategy ?? DEFAULTS.routing_strategy,
        num_retries: rs.num_retries ?? DEFAULTS.num_retries,
        allowed_fails: rs.allowed_fails ?? DEFAULTS.allowed_fails,
        cooldown_time: rs.cooldown_time ?? DEFAULTS.cooldown_time,
      });
    } else {
      setForm({ ...DEFAULTS });
    }
  }, [selectedId, teams]);

  const saveMutation = useMutation({
    mutationFn: (body: RouterSettingsValue) => apiPatch(`/team/${selectedId}/router/settings`, body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["teams-list"] });
      toast.success("Team router settings updated");
    },
    onError: (err: Error) => toast.error("Failed to save", { description: err.message }),
  });

  if (isLoading) return <Skeleton className="h-64 w-full" />;

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <Label htmlFor="team-select">Select Team</Label>
        <Select value={selectedId || "none"} onValueChange={(v) => setSelectedId(v === "none" ? "" : v)}>
          <SelectTrigger id="team-select">
            <SelectValue placeholder="Select a team to configure…" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="none">— Select a team —</SelectItem>
            {teams.map((t) => (
              <SelectItem key={t.team_id} value={t.team_id}>
                {t.team_alias || t.team_name || t.team_id}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {selectedId ? (
        <RouterSettingsForm
          form={form}
          onChange={setForm}
          saving={saveMutation.isPending}
          onSave={() => saveMutation.mutate(form)}
        />
      ) : (
        <p className="text-sm text-muted-foreground py-8 text-center">
          Select a team above to configure its router settings. Settings here override the global defaults.
        </p>
      )}
    </div>
  );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Page
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function RouterSettingsPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const tab = searchParams.get("tab") || "global";
  return (
    <div className="p-6 space-y-6 max-w-2xl">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Router Settings</h1>
        <p className="text-sm text-muted-foreground mt-1">
          Configure routing strategy, retries, and cooldown at Global or Team level.
          Team settings override Global defaults.
        </p>
      </div>

      <Tabs
        defaultValue={tab}
        value={tab}
        onValueChange={(v) => setSearchParams({ tab: v }, { replace: true })}
      >
        <TabsList>
          <TabsTrigger value="global">Global</TabsTrigger>
          <TabsTrigger value="teams">Teams</TabsTrigger>
        </TabsList>
        <TabsContent value="global" className="pt-4">
          <GlobalTab />
        </TabsContent>
        <TabsContent value="teams" className="pt-4">
          <TeamsTab />
        </TabsContent>
      </Tabs>
    </div>
  );
}
