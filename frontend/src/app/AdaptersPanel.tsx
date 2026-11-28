import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { ArrowDown, ArrowUp, Check, KeyRound, Play, RefreshCw, Terminal, Trash2, Wrench, Globe, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Textarea } from "@/components/ui/textarea";
import { Pill, PillIndicator } from "@/components/kibo-ui/pill";
import { call, enc } from "./ship/api";

export type AdapterCapability = "image_gen" | "reference_search" | "mesh_gen" | "audio_gen" | "custom";

export type AdapterSecretStatus = {
  has_key: boolean;
  masked?: string | null;
};

export type AdapterCliSpec = {
  argv: string[];
  env_secrets?: Record<string, string>;
  timeout_secs?: number;
  output_mode?: "file" | "stdout_json" | "stdout_path";
  result_json_path?: string;
};

export type AdapterHttpSpec = {
  method: string;
  url_template: string;
  headers_template?: Record<string, string>;
  body_template?: unknown;
  response_extract?: string;
  allow_loopback?: boolean;
};

export type AdapterView = {
  id: string;
  name: string;
  capability: AdapterCapability;
  description?: string;
  transport: "builtin" | "cli" | "http";
  enabled: boolean;
  priority: number;
  preset?: boolean | string | null;
  required_secrets?: string[];
  available?: boolean;
  missing_secrets?: string[];
  secrets_status?: Record<string, AdapterSecretStatus>;
  cli?: AdapterCliSpec | null;
  http?: AdapterHttpSpec | null;
  hardware_tier?: string | null;
  model_size?: string | null;
  install_hint?: string | null;
};

export type PresetView = {
  id: string;
  name: string;
  capability: AdapterCapability;
  description?: string;
  transport: "builtin" | "cli" | "http";
  required_secrets?: string[];
  installed?: boolean;
  hardware_tier?: string | null;
  model_size?: string | null;
  install_hint?: string | null;
};

type AdaptersListResponse = {
  capabilities?: string[];
  adapters?: AdapterView[];
  presets?: PresetView[];
};

const CAP_LABELS: Record<string, string> = {
  all: "All",
  image_gen: "Image",
  reference_search: "Search",
  mesh_gen: "3D Mesh",
  audio_gen: "Audio",
  custom: "Custom",
};

const BASE_FEATURED_ORDER = [
  "playwright-cdp-cli",
  "craft-anything-cli",
  "rea-cli",
  "recraft-v3-ui",
  "layer-ai-ui",
  "scenario-game-ui",
];

const AUDIO_FEATURED_ORDER = [
  "youtube-nocopyright-music-cli",
  "audioldm-s-local-cli",
  "stable-audio-small-cli",
  "tangoflux-sfx-cli",
  "audioldm2-sfx-cli",
  "stable-audio-open-cli",
  "ace-step-local-http",
  "google-gemini-lyria-http",
  "elevenlabs-sfx-http",
];

function AdapterTierBadge({
  modelSize,
  hardwareTier,
  installHint,
}: {
  modelSize?: string | null;
  hardwareTier?: string | null;
  installHint?: string | null;
}) {
  const label = modelSize?.trim() || hardwareTier?.trim();
  if (!label) return null;
  const tipParts = [
    hardwareTier?.trim() ? `Hardware tier: ${hardwareTier.trim()}` : "",
    installHint?.trim() || "",
  ].filter(Boolean);
  return (
    <Badge
      variant="outline"
      title={tipParts.length > 0 ? tipParts.join(", ") : undefined}
      data-hardware-tier={hardwareTier || undefined}
      data-model-size={modelSize || undefined}
      className="px-1.5 py-0.5 text-xs tabular-nums text-ink-2"
    >
      {label}
    </Badge>
  );
}

function slugifyId(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48) || `custom-${Date.now()}`;
}

export function AdaptersPanel({ initialCap }: { initialCap?: string } = {}) {
  const [cap, setCap] = useState<string>(() => initialCap || "all");
  const [adapters, setAdapters] = useState<AdapterView[]>([]);
  const [presets, setPresets] = useState<PresetView[]>([]);
  const [loading, setLoading] = useState(true);
  const [unavailable, setUnavailable] = useState(false);
  const [busyId, setBusyId] = useState<string>("");
  const [testResults, setTestResults] = useState<Record<string, { ok: boolean; status: string; details?: string; latency_ms?: number }>>({});
  const [secretDrafts, setSecretDrafts] = useState<Record<string, Record<string, string>>>({});
  const [addOpen, setAddOpen] = useState(false);

  // New Custom Adapter Form State
  const [formName, setFormName] = useState("");
  const [formId, setFormId] = useState("");
  const [formCap, setFormCap] = useState<AdapterCapability>("custom");
  const [formTransport, setFormTransport] = useState<"cli" | "http">("cli");
  const [formPriority, setFormPriority] = useState("50");
  const [cliArgvText, setCliArgvText] = useState("my-tool\n--prompt\n{{prompt}}\n--out\n{{out_path}}");
  const [cliOutputMode, setCliOutputMode] = useState<"file" | "stdout_json" | "stdout_path">("file");
  const [cliJsonPath, setCliJsonPath] = useState("");
  const [httpMethod, setHttpMethod] = useState("POST");
  const [httpUrl, setHttpUrl] = useState("https://api.example.com/v1/generate");
  const [httpHeaders, setHttpHeaders] = useState('{\n  "Authorization": "Bearer {{secret:CUSTOM_API_KEY}}"\n}');
  const [httpBody, setHttpBody] = useState('{\n  "prompt": "{{prompt}}"\n}');
  const [httpExtract, setHttpExtract] = useState("image_url_path:data.0.url");
  const [formSecretName, setFormSecretName] = useState("");
  const [formSecretVal, setFormSecretVal] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    const res = await call<AdaptersListResponse>("/api/adapters");
    setLoading(false);
    if (res.httpStatus === 404) {
      setUnavailable(true);
      return;
    }
    if (!res.ok) {
      toast.error(res.error || "Could not load tool adapters.");
      return;
    }
    setUnavailable(false);
    setAdapters(res.adapters ?? []);
    setPresets(res.presets ?? []);
  }, []);

  useEffect(() => {
    if (initialCap) setCap(initialCap);
  }, [initialCap]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const installPreset = async (preset: PresetView) => {
    setBusyId(`preset:${preset.id}`);
    const secrets = secretDrafts[preset.id] || {};
    const cleanSecrets: Record<string, string> = {};
    for (const [k, v] of Object.entries(secrets)) {
      if (v.trim()) cleanSecrets[k] = v.trim();
    }
    const res = await call<{ adapter?: AdapterView }>("/api/adapters", {
      preset_id: preset.id,
      ...(cap === "custom" && preset.id === "playwright-cdp-cli" ? { capability: "custom" } : {}),
      enabled: true,
      ...(Object.keys(cleanSecrets).length > 0 ? { secret_values: cleanSecrets } : {}),
    });
    setBusyId("");
    if (!res.ok) {
      toast.error(res.error || `Could not install ${preset.name}.`);
      return;
    }
    setSecretDrafts((prev) => {
      const next = { ...prev };
      delete next[preset.id];
      return next;
    });
    toast.success(`Installed ${preset.name}`);
    await refresh();
  };

  const toggleAdapter = async (a: AdapterView) => {
    setBusyId(a.id);
    const res = await call("/api/adapters", {
      id: a.id,
      name: a.name,
      capability: a.capability,
      transport: a.transport,
      enabled: !a.enabled,
      priority: a.priority,
      ...(a.preset ? { preset: a.preset } : {}),
    });
    setBusyId("");
    if (!res.ok) {
      toast.error(res.error || "Could not update adapter.");
      return;
    }
    await refresh();
  };

  const bumpPriority = async (a: AdapterView, delta: number) => {
    setBusyId(a.id);
    const nextPriority = Math.max(1, a.priority + delta);
    const res = await call("/api/adapters", {
      id: a.id,
      name: a.name,
      capability: a.capability,
      transport: a.transport,
      enabled: a.enabled,
      priority: nextPriority,
      ...(a.preset ? { preset: a.preset } : {}),
    });
    setBusyId("");
    if (!res.ok) {
      toast.error(res.error || "Could not update priority.");
      return;
    }
    await refresh();
  };

  const saveAdapterSecrets = async (a: AdapterView) => {
    const drafts = secretDrafts[a.id] || {};
    const clean: Record<string, string> = {};
    for (const [k, v] of Object.entries(drafts)) {
      if (v.trim()) clean[k] = v.trim();
    }
    if (Object.keys(clean).length === 0) return;
    setBusyId(`secret:${a.id}`);
    const res = await call("/api/adapters", {
      id: a.id,
      name: a.name,
      capability: a.capability,
      transport: a.transport,
      enabled: a.enabled,
      priority: a.priority,
      secret_values: clean,
    });
    setBusyId("");
    if (!res.ok) {
      toast.error(res.error || "Could not save secret.");
      return;
    }
    setSecretDrafts((prev) => {
      const next = { ...prev };
      delete next[a.id];
      return next;
    });
    toast.success(`Updated keys for ${a.name}`);
    await refresh();
  };

  const testAdapter = async (id: string) => {
    setBusyId(`test:${id}`);
    const res = await call<{ available?: boolean; status?: string; details?: string; latency_ms?: number }>(
      `/api/adapters/${enc(id)}/test`,
      {},
    );
    setBusyId("");
    setTestResults((prev) => ({
      ...prev,
      [id]: {
        ok: Boolean(res.ok && res.available !== false),
        status: res.status || (res.ok ? "ok" : "error"),
        details: res.details || res.error,
        latency_ms: res.latency_ms,
      },
    }));
    if (res.ok && res.available !== false) {
      toast.success(`Adapter ${id} ready`);
    } else {
      toast.error(res.details || res.error || `Adapter ${id} check failed`);
    }
  };

  const removeAdapter = async (a: AdapterView) => {
    setBusyId(`del:${a.id}`);
    try {
      const r = await fetch(`/api/adapters/${enc(a.id)}`, { method: "DELETE" });
      const data = (await r.json().catch(() => ({}))) as { ok?: boolean; error?: string };
      setBusyId("");
      if (!r.ok || data.ok === false) {
        toast.error(data.error || "Could not remove adapter.");
        return;
      }
      toast(`Removed ${a.name}`);
      await refresh();
    } catch {
      setBusyId("");
      toast.error("Could not reach the OpenRO server.");
    }
  };

  const createCustomAdapter = async (runTestAfter: boolean) => {
    const name = formName.trim();
    if (!name) {
      toast.error("Enter a tool name.");
      return;
    }
    const id = (formId.trim() || slugifyId(name)).toLowerCase();
    const priority = Number.parseInt(formPriority, 10);
    const payload: Record<string, unknown> = {
      id,
      name,
      capability: formCap,
      transport: formTransport,
      enabled: true,
      priority: Number.isFinite(priority) && priority > 0 ? priority : 50,
    };

    if (formTransport === "cli") {
      const argv = cliArgvText
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean);
      if (argv.length === 0) {
        toast.error("Provide at least one argv line for the CLI tool.");
        return;
      }
      payload.cli = {
        command: argv[0],
        argv: argv.slice(1),
        output_mode: cliOutputMode,
        ...(cliJsonPath.trim() ? { result_json_path: cliJsonPath.trim() } : {}),
      };
    } else {
      let headersObj: Record<string, string> | undefined;
      let bodyObj: unknown;
      if (httpHeaders.trim()) {
        try {
          headersObj = JSON.parse(httpHeaders) as Record<string, string>;
        } catch {
          toast.error("Headers must be valid JSON.");
          return;
        }
      }
      if (httpBody.trim()) {
        try {
          bodyObj = JSON.parse(httpBody);
        } catch {
          toast.error("Body template must be valid JSON.");
          return;
        }
      }
      payload.http = {
        method: httpMethod,
        url_template: httpUrl.trim(),
        ...(headersObj ? { headers_template: headersObj } : {}),
        ...(bodyObj !== undefined ? { body_template: bodyObj } : {}),
        ...(httpExtract.trim() ? { response_extract: httpExtract.trim() } : {}),
      };
    }

    const secName = formSecretName.trim();
    const secVal = formSecretVal.trim();
    if (secName) {
      payload.required_secrets = [secName];
      if (secVal) payload.secret_values = { [secName]: secVal };
    }

    setBusyId("create");
    const res = await call<{ adapter?: AdapterView }>("/api/adapters", payload);
    setBusyId("");
    if (!res.ok) {
      toast.error(res.error || "Could not save custom tool.");
      return;
    }
    toast.success(`Added ${name}`);
    setFormName("");
    setFormId("");
    setFormSecretVal("");
    setAddOpen(false);
    await refresh();
    if (runTestAfter) {
      await testAdapter(res.adapter?.id || id);
    }
  };

  if (unavailable) {
    return (
      <div className="rounded-card border border-hairline bg-board/45 p-4 text-sm text-ink-2">
        Tool adapter registry is available on the desktop server (`openro-rs`).
      </div>
    );
  }

  const installedIds = new Set(adapters.map((a) => (typeof a.preset === "string" ? a.preset : a.id)));
  const filteredAdapters = adapters
    .filter((a) => cap === "all" || a.capability === cap)
    .slice()
    .sort((a, b) => a.priority - b.priority);
  const featuredOrder =
    cap === "audio_gen"
      ? AUDIO_FEATURED_ORDER
      : cap === "all"
        ? [...BASE_FEATURED_ORDER, ...AUDIO_FEATURED_ORDER]
        : BASE_FEATURED_ORDER;
  const filteredPresets = presets
    .filter((p) => {
      if (p.installed || installedIds.has(p.id)) return false;
      if (cap === "all") return true;
      return p.capability === cap || (p.id === "playwright-cdp-cli" && cap === "custom");
    })
    .slice()
    .sort((a, b) => {
      const ia = featuredOrder.indexOf(a.id);
      const ib = featuredOrder.indexOf(b.id);
      if (ia !== -1 || ib !== -1) {
        return (ia === -1 ? 999 : ia) - (ib === -1 ? 999 : ib);
      }
      return a.name.localeCompare(b.name);
    });

  return (
    <div data-adapters-panel className="flex flex-col gap-3">
      {/* Header Bar: Capability Filter + "+ Add CLI / API Tool" */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap gap-1">
          {Object.entries(CAP_LABELS).map(([k, label]) => (
            <Button
              key={k}
              variant={cap === k ? "secondary" : "ghost"}
              size="xs"
              onClick={() => setCap(k)}
              className="active:opacity-80 max-md:min-h-11"
            >
              {label}
            </Button>
          ))}
        </div>

        <div className="flex items-center gap-1.5">
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Refresh adapters"
            onClick={() => void refresh()}
            className="text-ink-2 hover:text-ink max-md:size-11"
          >
            <RefreshCw className={`size-3.5 ${loading ? "motion-safe:animate-spin" : ""}`} strokeWidth={1.5} aria-hidden />
          </Button>
          <Button
            variant={addOpen ? "secondary" : "default"}
            size="sm"
            onClick={() => setAddOpen((v) => !v)}
            className="active:opacity-80 max-md:min-h-11"
          >
            {addOpen && <X className="size-3.5" strokeWidth={1.5} aria-hidden />}
            <span>{addOpen ? "Close form" : "+ Add CLI / API Tool"}</span>
          </Button>
        </div>
      </div>

      {/* Inline "+ Add CLI / API Tool" Form */}
      {addOpen && (
        <section
          aria-label="Add custom CLI or HTTP tool adapter"
          data-add-adapter-form
          className="flex flex-col gap-3 rounded-card border border-hairline bg-board/50 p-3.5"
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="text-sm font-medium text-ink">New CLI or HTTP Tool Adapter</span>
            <SegmentedControl aria-label="Adapter transport" size="sm">
              <SegmentedControlItem
                data-checked={formTransport === "cli" ? "" : undefined}
                aria-pressed={formTransport === "cli"}
                onClick={() => setFormTransport("cli")}
                className="gap-1 max-md:!h-11"
              >
                <Terminal className="size-3" strokeWidth={1.5} aria-hidden />CLI
              </SegmentedControlItem>
              <SegmentedControlItem
                data-checked={formTransport === "http" ? "" : undefined}
                aria-pressed={formTransport === "http"}
                onClick={() => setFormTransport("http")}
                className="gap-1 max-md:!h-11"
              >
                <Globe className="size-3" strokeWidth={1.5} aria-hidden />HTTP
              </SegmentedControlItem>
            </SegmentedControl>
          </div>

          <div className="grid gap-2 sm:grid-cols-4">
            <label className="flex flex-col gap-1 text-xs text-ink-2 sm:col-span-2">
              <span>Tool name</span>
              <Input value={formName} onChange={(e) => setFormName(e.target.value)} placeholder="e.g. ComfyUI Local" className="text-xs" />
            </label>
            <label className="flex flex-col gap-1 text-xs text-ink-2">
              <span>Capability</span>
              <select
                value={formCap}
                onChange={(e) => setFormCap(e.target.value as AdapterCapability)}
                aria-label="Adapter capability"
                className="h-9 rounded-lg border border-hairline bg-surface px-2 text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11"
              >
                <option value="image_gen">Image</option>
                <option value="reference_search">Search</option>
                <option value="mesh_gen">3D Mesh</option>
                <option value="audio_gen">Audio</option>
                <option value="custom">Custom</option>
              </select>
            </label>
            <label className="flex flex-col gap-1 text-xs text-ink-2">
              <span>Priority (low first)</span>
              <Input value={formPriority} onChange={(e) => setFormPriority(e.target.value)} className="font-mono text-xs" />
            </label>
          </div>

          {formTransport === "cli" ? (
            <div className="grid gap-2 sm:grid-cols-3">
              <label className="flex flex-col gap-1 text-xs text-ink-2 sm:col-span-2">
                <span>Argv lines (one argument per line)</span>
                <Textarea
                  value={cliArgvText}
                  onChange={(e) => setCliArgvText(e.target.value)}
                  rows={4}
                  className="font-mono text-xs"
                />
              </label>
              <div className="flex flex-col gap-2">
                <label className="flex flex-col gap-1 text-xs text-ink-2">
                  <span>Output mode</span>
                  <select
                    value={cliOutputMode}
                    onChange={(e) => setCliOutputMode(e.target.value as "file" | "stdout_json" | "stdout_path")}
                    aria-label="CLI output mode"
                    className="h-9 rounded-lg border border-hairline bg-surface px-2 text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11"
                  >
                    <option value="file">File</option>
                    <option value="stdout_json">Stdout JSON</option>
                    <option value="stdout_path">Stdout Path</option>
                  </select>
                </label>
                <label className="flex flex-col gap-1 text-xs text-ink-2">
                  <span>JSON path (optional)</span>
                  <Input value={cliJsonPath} onChange={(e) => setCliJsonPath(e.target.value)} placeholder="url or data.0.path" className="font-mono text-xs" />
                </label>
              </div>
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              <div className="grid gap-2 sm:grid-cols-4">
                <label className="flex flex-col gap-1 text-xs text-ink-2">
                  <span>Method</span>
                    <select
                    value={httpMethod}
                    onChange={(e) => setHttpMethod(e.target.value)}
                    aria-label="HTTP method"
                    className="h-9 rounded-lg border border-hairline bg-surface px-2 text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11"
                  >
                    <option value="POST">POST</option>
                    <option value="GET">GET</option>
                    <option value="PUT">PUT</option>
                  </select>
                </label>
                <label className="flex flex-col gap-1 text-xs text-ink-2 sm:col-span-3">
                  <span>URL template</span>
                  <Input value={httpUrl} onChange={(e) => setHttpUrl(e.target.value)} className="font-mono text-xs" />
                </label>
              </div>
              <div className="grid gap-2 sm:grid-cols-2">
                <label className="flex flex-col gap-1 text-xs text-ink-2">
                  <span>Headers JSON (supports secret refs)</span>
                  <Textarea value={httpHeaders} onChange={(e) => setHttpHeaders(e.target.value)} rows={3} className="font-mono text-xs" />
                </label>
                <label className="flex flex-col gap-1 text-xs text-ink-2">
                  <span>Body JSON</span>
                  <Textarea value={httpBody} onChange={(e) => setHttpBody(e.target.value)} rows={3} className="font-mono text-xs" />
                </label>
              </div>
              <label className="flex flex-col gap-1 text-xs text-ink-2">
                <span>Response extract path</span>
                <Input
                  value={httpExtract}
                  onChange={(e) => setHttpExtract(e.target.value)}
                  placeholder="image_url_path:data.0.url, audio_b64_path:outputs.0.data, or file_bytes"
                  className="font-mono text-xs max-md:min-h-11"
                />
                <span className="text-xs text-ink-3">
                  Supports <code className="font-mono">image_url_path:&lt;path&gt;</code>,{" "}
                  <code className="font-mono">image_b64_path:&lt;path&gt;</code>,{" "}
                  <code className="font-mono">audio_b64_path:&lt;path&gt;</code>,{" "}
                  <code className="font-mono">file_bytes</code>, or <code className="font-mono">json</code>.
                </span>
              </label>
            </div>
          )}

          <div className="grid gap-2 sm:grid-cols-3">
            <label className="flex flex-col gap-1 text-xs text-ink-2">
              <span>Adapter ID (optional)</span>
              <Input value={formId} onChange={(e) => setFormId(e.target.value)} placeholder="auto-slug" className="font-mono text-xs" />
            </label>
            <label className="flex flex-col gap-1 text-xs text-ink-2">
              <span>Secret name (optional)</span>
              <Input value={formSecretName} onChange={(e) => setFormSecretName(e.target.value)} placeholder="CUSTOM_API_KEY" className="font-mono text-xs" />
            </label>
            <label className="flex flex-col gap-1 text-xs text-ink-2">
              <span>Secret value (write-only)</span>
              <Input type="password" value={formSecretVal} onChange={(e) => setFormSecretVal(e.target.value)} placeholder="Saved securely" className="font-mono text-xs" />
            </label>
          </div>

          <div className="flex flex-wrap justify-end gap-2">
            <Button variant="outline" size="sm" disabled={busyId === "create"} onClick={() => void createCustomAdapter(false)} className="max-md:min-h-11">
              Save Tool
            </Button>
            <Button variant="default" size="sm" disabled={busyId === "create"} onClick={() => void createCustomAdapter(true)} className="max-md:min-h-11">
              <Play className="size-3.5" strokeWidth={1.5} aria-hidden />Save &amp; Test
            </Button>
          </div>
        </section>
      )}

      {/* Installed Adapters List */}
      <div className="flex flex-col gap-2">
        {filteredAdapters.map((a) => {
          const test = testResults[a.id];
          const neededSecrets = a.required_secrets ?? [];
          return (
            <article
              key={a.id}
              data-adapter-row={a.id}
              className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3"
            >
              <div className="flex flex-wrap items-start justify-between gap-2">
                <div className="flex min-w-0 flex-1 flex-col gap-1">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="text-sm font-medium text-ink">{a.name}</span>
                    <Badge className="border-transparent bg-surface px-1.5 py-0.5 text-xs text-ink-2">
                      {CAP_LABELS[a.capability] || a.capability}
                    </Badge>
                    <Badge className="border-transparent bg-surface px-1.5 py-0.5 text-xs text-ink-2">
                      {a.transport.toUpperCase()}
                    </Badge>
                    <AdapterTierBadge modelSize={a.model_size} hardwareTier={a.hardware_tier} installHint={a.install_hint} />
                    <span className="text-xs text-ink-3">P<span className="font-mono tabular-nums">{a.priority}</span></span>
                    <Pill className={a.available !== false && a.enabled ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg" : "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg"}>
                      <PillIndicator variant={a.available !== false && a.enabled ? "success" : "warning"} />
                      <span>{!a.enabled ? "Disabled" : a.missing_secrets?.length ? "Key needed" : "Ready"}</span>
                    </Pill>
                  </div>
                  {a.description && <p className="line-clamp-1 text-xs text-ink-2">{a.description}</p>}
                  {a.install_hint && <p className="line-clamp-1 text-xs text-ink-3">{a.install_hint}</p>}
                </div>

                <div className="flex shrink-0 flex-wrap items-center gap-1">
                  <Button variant="ghost" size="icon-sm" aria-label={`Raise ${a.name} priority`} disabled={busyId === a.id || a.priority <= 1} onClick={() => void bumpPriority(a, -10)} className="text-ink-2 hover:text-ink max-md:size-11">
                    <ArrowUp className="size-3.5" strokeWidth={1.5} aria-hidden />
                  </Button>
                  <Button variant="ghost" size="icon-sm" aria-label={`Lower ${a.name} priority`} disabled={busyId === a.id} onClick={() => void bumpPriority(a, 10)} className="text-ink-2 hover:text-ink max-md:size-11">
                    <ArrowDown className="size-3.5" strokeWidth={1.5} aria-hidden />
                  </Button>
                  <Button variant="outline" size="xs" disabled={busyId === a.id} onClick={() => void toggleAdapter(a)} className="bg-surface max-md:min-h-11">
                    {a.enabled ? "Disable" : "Enable"}
                  </Button>
                  <Button variant="outline" size="xs" disabled={busyId === `test:${a.id}`} onClick={() => void testAdapter(a.id)} className="bg-surface max-md:min-h-11">
                    <Wrench className="size-3" strokeWidth={1.5} aria-hidden />
                    <span>{busyId === `test:${a.id}` ? "Testing…" : "Test"}</span>
                  </Button>
                  {a.transport !== "builtin" && (
                    <Button variant="ghost" size="icon-sm" aria-label={`Remove ${a.name}`} disabled={busyId === `del:${a.id}`} onClick={() => void removeAdapter(a)} className="text-ink-2 hover:text-ink max-md:size-11">
                      <Trash2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                    </Button>
                  )}
                </div>
              </div>

              {neededSecrets.length > 0 && (
                <div className="flex flex-wrap items-center gap-2 rounded-sm border border-hairline bg-surface p-2">
                  <KeyRound className="size-3.5 shrink-0 text-icon" strokeWidth={1.5} aria-hidden />
                  {neededSecrets.map((sec) => {
                    const st = a.secrets_status?.[sec];
                    return (
                      <Input
                        key={sec}
                        type="password"
                        aria-label={`${a.name} ${sec}`}
                        value={secretDrafts[a.id]?.[sec] ?? ""}
                        onChange={(e) =>
                          setSecretDrafts((prev) => ({
                            ...prev,
                            [a.id]: { ...(prev[a.id] ?? {}), [sec]: e.target.value },
                          }))
                        }
                        placeholder={st?.has_key ? `${sec}: ${st.masked || "saved"}` : `Enter ${sec}`}
                        className="h-8 min-w-[180px] flex-1 font-mono text-xs"
                      />
                    );
                  })}
                  <Button variant="outline" size="xs" disabled={busyId === `secret:${a.id}`} onClick={() => void saveAdapterSecrets(a)} className="max-md:min-h-11">
                    Save key
                  </Button>
                </div>
              )}

              {test && (
                <div className={`rounded-sm border border-hairline px-2.5 py-1.5 text-xs ${test.ok ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-pro-bg text-pill-pro-fg"}`}>
                  <span>{test.status}</span>
                  {typeof test.latency_ms === "number" && <span className="ml-2"><span className="font-mono tabular-nums">{test.latency_ms}</span>ms</span>}
                  {test.details && <span className="ml-2">{test.details}</span>}
                </div>
              )}
            </article>
          );
        })}
      </div>

      {/* 1-Click Presets */}
      {filteredPresets.length > 0 && (
        <section className="flex flex-col gap-2 border-t border-hairline pt-3">
          <span className="text-xs font-medium text-ink-2">1-Click Presets</span>
          <div className="grid gap-2 sm:grid-cols-2">
            {filteredPresets.map((p) => (
              <div key={p.id} data-adapter-preset={p.id} className="flex flex-col justify-between gap-2 rounded-card border border-hairline bg-board/40 p-3">
                <div className="flex flex-col gap-1">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="text-sm font-medium text-ink">{p.name}</span>
                    <Badge className="border-transparent bg-surface px-1.5 py-0.5 text-xs text-ink-2">
                      {CAP_LABELS[p.capability] || p.capability}
                    </Badge>
                    <Badge className="border-transparent bg-surface px-1.5 py-0.5 text-xs text-ink-2">
                      {p.transport.toUpperCase()}
                    </Badge>
                    <AdapterTierBadge modelSize={p.model_size} hardwareTier={p.hardware_tier} installHint={p.install_hint} />
                  </div>
                  {p.description && <p className="line-clamp-1 text-xs text-ink-2">{p.description}</p>}
                  {p.install_hint && <p className="line-clamp-1 text-xs text-ink-3">{p.install_hint}</p>}
                </div>
                <Button
                  variant="outline"
                  size="xs"
                  disabled={busyId === `preset:${p.id}`}
                  onClick={() => void installPreset(p)}
                  className="self-start bg-surface max-md:min-h-11"
                >
                  <Check className="size-3" strokeWidth={1.5} aria-hidden />
                  <span>{busyId === `preset:${p.id}` ? "Installing…" : "Install"}</span>
                </Button>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
