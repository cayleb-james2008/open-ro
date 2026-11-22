import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Check, Cpu, Globe, Image as ImageIcon, KeyRound, Laptop, Moon, RefreshCw, Server, SlidersHorizontal, Sparkles, Sun, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Pill } from "@/components/kibo-ui/pill";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogDescription,
  MorphingDialogTitle,
  MorphingDialogTrigger,
} from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { post, read, type AntigravityIssue, type AntigravityModel, type ConfigSnap, type WorkspaceMode } from "./data";
import { OpenCloudSettings } from "./ship/OpenCloudSettings";

export function SettingsDialog({
  theme,
  onThemeChange,
  open,
  onOpenChange,
  onApplied,
  compactLabel,
  workspaceMode,
  showTrigger = true,
  onOpenCloudChanged,
}: {
  theme: "light" | "dark";
  onThemeChange: (t: "light" | "dark") => void;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onApplied: (next: {
    studioConnected: boolean;
    modelConfigured: boolean;
    workspaceMode: WorkspaceMode;
    modelId?: string;
    antigravityIssue?: AntigravityIssue | null;
  }) => void;
  compactLabel?: string;
  workspaceMode: WorkspaceMode;
  showTrigger?: boolean;
  onOpenCloudChanged?: () => void;
}) {
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [refreshingModels, setRefreshingModels] = useState(false);
  const [loadErr, setLoadErr] = useState("");
  const [cfg, setCfg] = useState<ConfigSnap | null>(null);

  const [provider, setProvider] = useState<"antigravity" | "openai">("openai");
  const [agyModels, setAgyModels] = useState<AntigravityModel[]>([]);
  const [agyIssue, setAgyIssue] = useState<AntigravityIssue | null>(null);
  const [baseUrl, setBaseUrl] = useState("");
  const [modelId, setModelId] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [clearKey, setClearKey] = useState(false);
  const [mcpCommand, setMcpCommand] = useState("");
  const [wsMode, setWsMode] = useState("auto");
  const [webMode, setWebMode] = useState<"browser" | "byok" | "local_server">("browser");
  const [localServerUrl, setLocalServerUrl] = useState("http://127.0.0.1:8077");
  const [imageProvider, setImageProvider] = useState<"auto" | "gemini" | "openai">("auto");
  const [geminiApiKey, setGeminiApiKey] = useState("");
  const [clearGeminiKey, setClearGeminiKey] = useState(false);
  const [openaiImageKey, setOpenaiImageKey] = useState("");
  const [clearOpenaiImageKey, setClearOpenaiImageKey] = useState(false);

  const loadConfig = useCallback(async () => {
    setLoading(true);
    setLoadErr("");
    try {
      const data = await read<ConfigSnap>("/api/config");
      setCfg(data);
      const nextProv: "antigravity" | "openai" =
        data.provider === "antigravity" || (data.provider !== "openai" && data.antigravity_available)
          ? "antigravity"
          : "openai";
      setProvider(nextProv);
      setAgyModels(data.antigravity_models ?? []);
      setAgyIssue(data.antigravity_issue ?? null);
      setBaseUrl(data.base_url || "");
      setModelId(data.model_id || (nextProv === "antigravity" ? data.antigravity_default_model || "gemini-4-hydrogen-high" : ""));
      setApiKey("");
      setClearKey(false);
      setMcpCommand(data.mcp_command || "");
      setWsMode(data.workspace_mode || "auto");
      if (data.web_mode) setWebMode(data.web_mode);
      if (data.local_server_url) setLocalServerUrl(data.local_server_url);
      if (data.image_provider === "gemini" || data.image_provider === "openai" || data.image_provider === "auto") {
        setImageProvider(data.image_provider);
      }
      setGeminiApiKey("");
      setClearGeminiKey(false);
      setOpenaiImageKey("");
      setClearOpenaiImageKey(false);
    } catch (e) {
      setLoadErr(e instanceof Error ? e.message : "Could not load runtime settings.");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (open) void loadConfig();
  }, [open, loadConfig]);

  const refreshAgyModels = async () => {
    setRefreshingModels(true);
    try {
      const m = await read<{ models?: AntigravityModel[]; issue?: AntigravityIssue | null }>("/api/models?refresh=1");
      if (Array.isArray(m.models)) setAgyModels(m.models);
      if (m.issue !== undefined) setAgyIssue(m.issue);
    } catch {
      toast.error("Could not refresh Antigravity models.");
    } finally {
      setRefreshingModels(false);
    }
  };

  const saveConfig = async () => {
    setSaving(true);
    setLoadErr("");
    try {
      const imagePayload = {
        image_provider: imageProvider,
        gemini_api_key: geminiApiKey.trim() || undefined,
        clear_gemini_api_key: clearGeminiKey,
        openai_image_api_key: openaiImageKey.trim() || undefined,
        clear_openai_image_api_key: clearOpenaiImageKey,
      };
      const defaultAgy = cfg?.antigravity_default_model || "gemini-4-hydrogen-high";
      const payload: Record<string, unknown> = cfg?.web_engine
        ? {
            web_mode: webMode,
            base_url: baseUrl.trim(),
            model_id: modelId.trim(),
            api_key: apiKey.trim() || undefined,
            clear_api_key: clearKey,
            local_server_url: localServerUrl.trim() || "http://127.0.0.1:8077",
            ...imagePayload,
          }
        : {
            provider,
            base_url: provider === "antigravity" ? "agy://antigravity" : baseUrl.trim(),
            model_id: modelId.trim() || (provider === "antigravity" ? defaultAgy : ""),
            api_key: provider === "antigravity" ? undefined : apiKey.trim() || undefined,
            clear_api_key: provider === "antigravity" ? false : clearKey,
            mcp_command: mcpCommand.trim(),
            workspace_mode: wsMode,
            ...imagePayload,
          };
      const res = await post<ConfigSnap & { ok?: boolean; error?: string }>("/api/config", payload);
      if (res.ok === false) {
        setLoadErr(res.error || "Could not save settings.");
        return;
      }
      setCfg(res);
      if (res.antigravity_models) setAgyModels(res.antigravity_models);
      setAgyIssue(res.antigravity_issue ?? null);
      setApiKey("");
      setClearKey(false);
      setGeminiApiKey("");
      setClearGeminiKey(false);
      setOpenaiImageKey("");
      setClearOpenaiImageKey(false);
      onApplied({
        studioConnected: res.studio_connected,
        modelConfigured: res.model_configured,
        workspaceMode: res.active_backend,
        modelId: res.model_id,
        antigravityIssue: res.antigravity_issue ?? null,
      });
      toast.success(cfg?.web_engine ? "Workspace engine updated" : "Model & Studio settings applied");
      onOpenChange(false);
    } catch {
      setLoadErr("Could not reach the settings endpoint.");
    } finally {
      setSaving(false);
    }
  };

  const pickGateway = (gBase: string, gModel: string) => {
    setProvider("openai");
    setBaseUrl(gBase);
    setModelId(gModel);
    setApiKey("");
    setClearKey(true);
  };

  const backendLabel =
    workspaceMode === "studio_mcp"
      ? "Studio MCP"
      : workspaceMode === "local_place"
        ? "Local Place"
        : workspaceMode === "browser_place"
          ? "Browser Engine"
          : "Offline";

  return (
    <MorphingDialog transition={dialogSpring} open={open} onOpenChange={onOpenChange}>
      {showTrigger && (
        <MorphingDialogTrigger
          thing="settings"
          aria-label="Settings"
          className="flex min-h-11 w-full items-center justify-between gap-2 rounded-lg border border-hairline bg-board/60 px-3 text-sm font-medium text-ink outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board md:min-h-9"
        >
          <span className="inline-flex min-w-0 items-center gap-2">
            <SlidersHorizontal className="size-4 flex-none text-icon" strokeWidth={1.5} aria-hidden />
            <span className="truncate">Settings</span>
          </span>
          <span className="truncate font-mono text-xs text-ink-3">{compactLabel || backendLabel}</span>
        </MorphingDialogTrigger>
      )}

      <MorphingDialogContainer>
        <MorphingDialogContent className="relative mx-3 flex max-h-[88dvh] w-full max-w-[540px] flex-col rounded-dialog border border-hairline bg-surface text-ink shadow-lift">
          <div data-settings-dialog className="flex min-h-0 flex-1 flex-col">
            <div className="flex items-start justify-between gap-3 border-b border-hairline px-5 py-4">
              <div className="min-w-0">
                <MorphingDialogTitle className="text-title font-medium text-ink">
                  Model, Studio &amp; Appearance
                </MorphingDialogTitle>
                <MorphingDialogDescription className="mt-0.5 text-sm text-ink-2">
                  Switch Light/Dark appearance, configure a local or remote LLM, and choose your Roblox Studio backend live.
                </MorphingDialogDescription>
              </div>
              <MorphingDialogClose
                aria-label="Close settings"
                className="static inline-flex size-11 flex-none cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface text-ink-2 outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board hover:text-ink md:size-8"
              >
                <X className="size-4" strokeWidth={1.5} aria-hidden />
              </MorphingDialogClose>
            </div>

            <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-5 py-4">
              {/* 1. Appearance (Light / Dark moved inside Settings per owner ruling) */}
              <section aria-label="Appearance settings" className="flex flex-wrap items-center justify-between gap-3 rounded-card border border-hairline bg-board/50 p-3.5">
                <div className="min-w-0">
                  <h3 className="text-sm font-medium text-ink">Appearance</h3>
                  <p className="text-xs text-ink-2">Saved to this browser for Light and Dark modes.</p>
                </div>
                <SegmentedControl aria-label="Appearance" size="sm">
                  <SegmentedControlItem
                    data-checked={theme === "light" ? "" : undefined}
                    aria-pressed={theme === "light"}
                    onClick={() => onThemeChange("light")}
                    className="gap-1.5 max-md:!h-11 max-md:px-3"
                  >
                    <Sun className="size-3.5" strokeWidth={1.5} aria-hidden />
                    Light
                  </SegmentedControlItem>
                  <SegmentedControlItem
                    data-checked={theme === "dark" ? "" : undefined}
                    aria-pressed={theme === "dark"}
                    onClick={() => onThemeChange("dark")}
                    className="gap-1.5 max-md:!h-11 max-md:px-3"
                  >
                    <Moon className="size-3.5" strokeWidth={1.5} aria-hidden />
                    Dark
                  </SegmentedControlItem>
                </SegmentedControl>
              </section>

              {loadErr && (
                <div role="alert" className="rounded-card border border-hairline bg-pill-pro-bg px-3.5 py-2.5 text-sm text-pill-pro-fg">
                  {loadErr}
                </div>
              )}

              {loading && !cfg ? (
                <p className="text-sm text-ink-2">Checking local gateways and Studio backend…</p>
              ) : cfg?.web_engine ? (
                /* Standalone Browser Engine Settings */
                <section aria-label="Web Studio engine" className="flex flex-col gap-3 rounded-card border border-hairline bg-board/50 p-3.5">
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2">
                      <Globe className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                      <h3 className="text-sm font-medium text-ink">Execution Engine</h3>
                    </div>
                    <Pill className="bg-tag-mint-bg px-2 py-0.5 font-mono text-xs text-tag-mint-fg">IndexedDB persisted</Pill>
                  </div>

                  <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
                    {(
                      [
                        { id: "browser", label: "Browser Compiler", sub: "Zero-key local Luau" },
                        { id: "byok", label: "BYOK LLM", sub: "Session-only key" },
                        { id: "local_server", label: "Local OpenRO", sub: "127.0.0.1:8077" },
                      ] as const
                    ).map((opt) => (
                      <Button
                        key={opt.id}
                        variant={webMode === opt.id ? "default" : "outline"}
                        size="sm"
                        onClick={() => setWebMode(opt.id)}
                        className="h-auto flex-col items-start gap-0.5 whitespace-normal py-2 text-left sm:h-auto max-md:min-h-11"
                      >
                        <span className="text-xs font-medium">{opt.label}</span>
                        <span className={`text-xs ${webMode === opt.id ? "text-blue-fg" : "text-ink-2"}`}>{opt.sub}</span>
                      </Button>
                    ))}
                  </div>

                  {webMode === "byok" && (
                    <div className="flex flex-col gap-2.5 border-t border-hairline pt-3">
                      <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                        <span>OpenAI-compatible Base URL</span>
                        <Input
                          value={baseUrl}
                          onChange={(e) => setBaseUrl(e.target.value)}
                          placeholder="https://api.openai.com/v1 or http://127.0.0.1:8790/v1"
                          className="font-mono text-xs"
                        />
                      </label>
                      <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                        <span>Model ID</span>
                        <Input
                          value={modelId}
                          onChange={(e) => setModelId(e.target.value)}
                          placeholder="gpt-4o-mini or deepseek-v4.1-flash-free"
                          className="font-mono text-xs"
                        />
                      </label>
                      <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                        <span>API Key (stored in sessionStorage only — cleared when tab closes)</span>
                        <Input
                          type="password"
                          value={apiKey}
                          onChange={(e) => {
                            setApiKey(e.target.value);
                            setClearKey(false);
                          }}
                          placeholder={cfg.has_api_key ? `Saved in session (${cfg.api_key_masked})` : "Optional for 127.0.0.1 loopback"}
                          className="font-mono text-xs"
                        />
                      </label>
                    </div>
                  )}

                  {webMode === "local_server" && (
                    <div className="flex flex-col gap-2 border-t border-hairline pt-3">
                      <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                        <span>Local openro-rs URL</span>
                        <Input
                          value={localServerUrl}
                          onChange={(e) => setLocalServerUrl(e.target.value)}
                          placeholder="http://127.0.0.1:8077"
                          className="font-mono text-xs"
                        />
                      </label>
                      <p className="text-xs text-ink-2">
                        Run <code className="rounded-sm bg-surface px-1 font-mono">openro-rs</code> on this machine, then click Apply to bridge prompts and .rbxlx exports to your local Rust server.
                      </p>
                    </div>
                  )}
                </section>
              ) : (
                <>
                  {/* 2. Model Provider (Real openro-rs server) */}
                  <section aria-label="Model provider" className="flex flex-col gap-3 rounded-card border border-hairline bg-board/50 p-3.5">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <Cpu className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                        <h3 className="text-sm font-medium text-ink">Model Provider</h3>
                      </div>
                      <Button
                        variant="ghost"
                        size="xs"
                        disabled={refreshingModels}
                        onClick={() => void (provider === "antigravity" ? refreshAgyModels() : loadConfig())}
                        className="text-ink-2 hover:text-ink max-md:min-h-11"
                      >
                        <RefreshCw className={`size-3 ${refreshingModels ? "animate-spin" : ""}`} strokeWidth={1.5} aria-hidden />
                        {provider === "antigravity" ? "Refresh models" : "Probe gateways"}
                      </Button>
                    </div>

                    {(cfg?.antigravity_available || provider === "antigravity") && (
                      <SegmentedControl aria-label="Model provider" size="sm" className="w-full">
                        <SegmentedControlItem
                          data-checked={provider === "antigravity" ? "" : undefined}
                          aria-pressed={provider === "antigravity"}
                          onClick={() => {
                            setProvider("antigravity");
                            if (!modelId.trim() || baseUrl.startsWith("http")) {
                              setModelId(cfg?.antigravity_default_model || "gemini-4-hydrogen-high");
                            }
                          }}
                          className="flex-1 justify-center gap-1.5 max-md:!h-11"
                        >
                          <Sparkles className="size-3.5" strokeWidth={1.5} aria-hidden />
                          Antigravity
                        </SegmentedControlItem>
                        <SegmentedControlItem
                          data-checked={provider === "openai" ? "" : undefined}
                          aria-pressed={provider === "openai"}
                          onClick={() => {
                            setProvider("openai");
                            if (baseUrl === "agy://antigravity") setBaseUrl("");
                          }}
                          className="flex-1 justify-center gap-1.5 max-md:!h-11"
                        >
                          <Globe className="size-3.5" strokeWidth={1.5} aria-hidden />
                          OpenAI-compatible
                        </SegmentedControlItem>
                      </SegmentedControl>
                    )}

                    {agyIssue && (
                      <div
                        role="alert"
                        data-agy-issue
                        className={`flex flex-wrap items-center justify-between gap-2 rounded-card px-3 py-2 text-sm ${agyIssue.kind === "quota" ? "bg-pill-warn-bg text-pill-warn-fg" : "bg-pill-pro-bg text-pill-pro-fg"}`}
                      >
                        <span>{agyIssue.message}</span>
                        {agyIssue.resets_in && (
                          <span className="font-mono text-xs font-medium">resets in {agyIssue.resets_in}</span>
                        )}
                      </div>
                    )}

                    {provider === "antigravity" ? (
                      <div className="flex flex-col gap-2" data-agy-picker>
                        <div className="flex items-center justify-between gap-2 text-xs text-ink-2">
                          <span>Select an Antigravity model (Hydrogen default)</span>
                          <span className="font-mono text-ink">{modelId || cfg?.antigravity_default_model || "gemini-4-hydrogen-high"}</span>
                        </div>
                        {agyModels.length > 0 ? (
                          <div className="grid max-h-44 grid-cols-1 gap-1.5 overflow-y-auto sm:grid-cols-2">
                            {agyModels.map((m) => {
                              const active = (modelId.trim() || cfg?.antigravity_default_model || "gemini-4-hydrogen-high") === m.id;
                              return (
                                <Button
                                  key={m.id}
                                  variant={active ? "default" : "outline"}
                                  size="sm"
                                  onClick={() => setModelId(m.id)}
                                  className="h-auto min-h-11 flex-col items-start gap-0.5 py-2 text-left sm:h-auto"
                                >
                                  <span className="flex w-full items-center justify-between gap-1 text-xs font-medium">
                                    <span className="truncate">{m.label || m.id}</span>
                                    {active && <Check className="size-3 flex-none" strokeWidth={2} aria-hidden />}
                                  </span>
                                  <span className={`truncate font-mono text-xs ${active ? "text-blue-fg" : "text-ink-2"}`}>{m.id}</span>
                                </Button>
                              );
                            })}
                          </div>
                        ) : (
                          <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                            <span>Antigravity Model ID</span>
                            <Input
                              value={modelId}
                              onChange={(e) => setModelId(e.target.value)}
                              placeholder={cfg?.antigravity_default_model || "gemini-4-hydrogen-high"}
                              className="font-mono text-xs"
                            />
                          </label>
                        )}
                      </div>
                    ) : (
                      <>
                        {cfg && cfg.detected_gateways.length > 0 && (
                          <div className="flex flex-col gap-1.5">
                            <span className="text-xs text-ink-2">Running local gateways (no API key needed)</span>
                            <div className="flex flex-wrap gap-1.5">
                              {cfg.detected_gateways.map((gw) => {
                                const active = baseUrl.trim() === gw.base_url && modelId.trim() === gw.model_id;
                                return (
                                  <Button
                                    key={`${gw.base_url}-${gw.model_id}`}
                                    variant={active ? "default" : "outline"}
                                    size="xs"
                                    onClick={() => pickGateway(gw.base_url, gw.model_id)}
                                    className="max-md:min-h-11"
                                  >
                                    {active && <Check className="size-3" strokeWidth={2} aria-hidden />}
                                    <span className="font-mono text-xs">{gw.model_id}</span>
                                  </Button>
                                );
                              })}
                            </div>
                          </div>
                        )}

                        <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                          <span>OpenAI-compatible Base URL</span>
                          <Input
                            value={baseUrl}
                            onChange={(e) => setBaseUrl(e.target.value)}
                            placeholder="http://127.0.0.1:11434/v1"
                            className="font-mono text-xs"
                          />
                        </label>

                        <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                          <span>Model ID</span>
                          <Input
                            value={modelId}
                            onChange={(e) => setModelId(e.target.value)}
                            placeholder="qwen2.5:3b"
                            className="font-mono text-xs"
                          />
                        </label>

                        <div className="flex flex-col gap-1.5">
                          <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                            <span className="inline-flex items-center gap-1">
                              <KeyRound className="size-3 text-icon" strokeWidth={1.5} aria-hidden />
                              API Key (optional on 127.0.0.1 / localhost; required for remote hosts)
                            </span>
                            <Input
                              type="password"
                              value={apiKey}
                              onChange={(e) => {
                                setApiKey(e.target.value);
                                setClearKey(false);
                              }}
                              placeholder={
                                cfg?.has_api_key && !clearKey
                                  ? `Saved (${cfg.api_key_masked || "configured"}) — leave blank to keep`
                                  : "Leave blank for loopback local gateways"
                              }
                              className="font-mono text-xs"
                            />
                          </label>
                          {cfg?.has_api_key && !clearKey && (
                            <div>
                              <Button
                                variant="ghost"
                                size="xs"
                                onClick={() => setClearKey(true)}
                                className="text-ink-2 hover:text-ink max-md:min-h-11"
                              >
                                Clear saved API key
                              </Button>
                            </div>
                          )}
                        </div>
                      </>
                    )}
                  </section>

                  {/* 3. Studio & Local Place Backend */}
                  <section aria-label="Studio workspace backend" className="flex flex-col gap-3 rounded-card border border-hairline bg-board/50 p-3.5">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <Server className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                        <h3 className="text-sm font-medium text-ink">Roblox Studio &amp; Place Backend</h3>
                      </div>
                      <Pill className="bg-tag-sky-bg px-2 py-0.5 text-xs text-tag-sky-fg">
                        {cfg?.active_backend === "studio_mcp"
                          ? "Studio MCP"
                          : cfg?.active_backend === "local_place"
                            ? "Local Place"
                            : cfg?.active_backend === "browser_place"
                              ? "Browser Engine"
                              : backendLabel}
                      </Pill>
                    </div>

                    <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
                      {(
                        [
                          { id: "auto", label: "Auto", sub: "MCP -> Local Place" },
                          { id: "local_place", label: "Local Place", sub: "Persistent .rbxlx" },
                          { id: "studio_mcp", label: "Studio MCP", sub: "Live Studio bridge" },
                        ] as const
                      ).map((opt) => (
                        <Button
                          key={opt.id}
                          variant={wsMode === opt.id ? "default" : "outline"}
                          size="sm"
                          onClick={() => setWsMode(opt.id)}
                          className="h-auto flex-col items-start gap-0.5 whitespace-normal py-2 text-left sm:h-auto max-md:min-h-11"
                        >
                          <span className="text-xs font-medium">{opt.label}</span>
                          <span className={`text-xs ${wsMode === opt.id ? "text-blue-fg" : "text-ink-2"}`}>{opt.sub}</span>
                        </Button>
                      ))}
                    </div>

                    <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                      <span>Custom Studio MCP Command (optional)</span>
                      <Input
                        value={mcpCommand}
                        onChange={(e) => setMcpCommand(e.target.value)}
                        placeholder={cfg?.vinegar_mcp_path || "/home/cayleb/.local/bin/roblox-studio-mcp"}
                        className="font-mono text-xs"
                      />
                    </label>

                    <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-ink-2">
                      <span className="inline-flex items-center gap-1.5">
                        <Laptop className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                        {cfg?.vinegar_detected
                          ? "Vinegar Roblox Studio detected"
                          : "Roblox Studio not open — Local Place persists game.rbxlx"}
                      </span>
                      {cfg?.rbxlx_path && (
                        <code className="max-w-full truncate font-mono text-xs text-ink-3">{cfg.rbxlx_path}</code>
                      )}
                    </div>
                  </section>
                </>
              )}

              {/* Roblox Open Cloud: write-only key, always masked */}
              {!cfg?.web_engine && <OpenCloudSettings onChanged={onOpenCloudChanged} />}

              {/* Game Asset & Texture Image Generation (Gemini Nano Banana / OpenAI Images) */}
              {cfg && (
                <section aria-label="Game asset image generation" className="flex flex-col gap-3 rounded-card border border-hairline bg-board/50 p-3.5">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="flex items-center gap-2">
                      <ImageIcon className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                      <h3 className="text-sm font-medium text-ink">Game Asset &amp; Texture Generation</h3>
                    </div>
                    <Pill className="bg-tag-research-bg px-2 py-0.5 font-mono text-xs text-tag-research-fg">
                      {cfg.has_gemini_api_key || cfg.has_gemini_key || cfg.has_openai_image_api_key
                        ? "Key configured"
                        : "Optional BYOK"}
                    </Pill>
                  </div>

                  <p className="text-xs leading-relaxed text-ink-2">
                    Optional Google Gemini (Nano Banana) or OpenAI Image API key for generating custom decals, icons, badges, and stylized character textures for any Roblox game (install the Image Generation Studio pack in Tool Library to unlock).
                  </p>

                  <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
                    {(
                      [
                        { id: "auto", label: "Auto", sub: "Gemini -> OpenAI" },
                        { id: "gemini", label: "Gemini Nano Banana", sub: "gemini-2.5-flash-image" },
                        { id: "openai", label: "OpenAI Images", sub: "gpt-image-1" },
                      ] as const
                    ).map((opt) => (
                      <Button
                        key={opt.id}
                        variant={imageProvider === opt.id ? "default" : "outline"}
                        size="sm"
                        onClick={() => setImageProvider(opt.id)}
                        className="h-auto flex-col items-start gap-0.5 whitespace-normal py-2 text-left sm:h-auto max-md:min-h-11"
                      >
                        <span className="text-xs font-medium">{opt.label}</span>
                        <span className={`text-xs ${imageProvider === opt.id ? "text-blue-fg" : "text-ink-2"}`}>{opt.sub}</span>
                      </Button>
                    ))}
                  </div>

                  <div className="flex flex-col gap-2">
                    <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                      <span>Google Gemini API Key (Nano Banana){cfg.web_engine ? " — sessionStorage only" : ""}</span>
                      <Input
                        type="password"
                        value={geminiApiKey}
                        onChange={(e) => {
                          setGeminiApiKey(e.target.value);
                          setClearGeminiKey(false);
                        }}
                        placeholder={
                          (cfg.has_gemini_api_key || cfg.has_gemini_key) && !clearGeminiKey
                            ? `Saved (${cfg.gemini_api_key_masked || cfg.gemini_key_masked || "configured"}) — leave blank to keep`
                            : "AIza… (or set GEMINI_API_KEY)"
                        }
                        className="font-mono text-xs"
                      />
                    </label>
                    {(cfg.has_gemini_api_key || cfg.has_gemini_key) && !clearGeminiKey && (
                      <div>
                        <Button
                          variant="ghost"
                          size="xs"
                          onClick={() => setClearGeminiKey(true)}
                          className="text-ink-2 hover:text-ink max-md:min-h-11"
                        >
                          Clear saved Gemini key
                        </Button>
                      </div>
                    )}

                    <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                      <span>OpenAI Image API Key (optional){cfg.web_engine ? " — sessionStorage only" : ""}</span>
                      <Input
                        type="password"
                        value={openaiImageKey}
                        onChange={(e) => {
                          setOpenaiImageKey(e.target.value);
                          setClearOpenaiImageKey(false);
                        }}
                        placeholder={
                          cfg.has_openai_image_api_key && !clearOpenaiImageKey
                            ? `Saved (${cfg.openai_image_api_key_masked || "configured"}) — leave blank to keep`
                            : "sk-… (or set OPENAI_API_KEY)"
                        }
                        className="font-mono text-xs"
                      />
                    </label>
                    {cfg.has_openai_image_api_key && !clearOpenaiImageKey && (
                      <div>
                        <Button
                          variant="ghost"
                          size="xs"
                          onClick={() => setClearOpenaiImageKey(true)}
                          className="text-ink-2 hover:text-ink max-md:min-h-11"
                        >
                          Clear saved OpenAI Image key
                        </Button>
                      </div>
                    )}
                  </div>
                </section>
              )}
            </div>

            <div className="flex flex-wrap items-center justify-end gap-2 border-t border-hairline px-5 py-3.5">
              <MorphingDialogClose
                aria-label="Cancel settings"
                className="static inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board md:min-h-9"
              >
                Close
              </MorphingDialogClose>
              <Button
                variant="default"
                disabled={saving || loading}
                onClick={() => void saveConfig()}
                className="min-h-11 px-4 md:min-h-9"
              >
                {saving ? "Applying…" : "Save & Apply"}
              </Button>
            </div>
          </div>
        </MorphingDialogContent>
      </MorphingDialogContainer>
    </MorphingDialog>
  );
}
