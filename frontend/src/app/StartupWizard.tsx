import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { Check, Cpu, Hammer, Laptop, Rocket, Sparkles, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { MorphingDialog, MorphingDialogClose, MorphingDialogContainer, MorphingDialogContent, MorphingDialogDescription, MorphingDialogTitle } from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { normalizeEmbedStatus, post, read, type AntigravityModel, type ConfigSnap, type StudioEmbedStatus, type WorkspaceMode } from "./data";
import { call } from "./ship/api";

export const ONBOARDING_KEY = "openro-onboarding-done";

export function StartupWizard({
  open,
  onOpenChange,
  studioOk,
  onApplied,
  onDoneFocusComposer,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  studioOk: boolean;
  onApplied: (next: { studioConnected: boolean; modelConfigured: boolean; workspaceMode: WorkspaceMode; modelId?: string }) => void;
  onDoneFocusComposer: () => void;
}) {
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [cfg, setCfg] = useState<ConfigSnap | null>(null);
  const [provider, setProvider] = useState<"antigravity" | "openai">("antigravity");
  const [modelId, setModelId] = useState("gemini-4-hydrogen-high");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [blenderOk, setBlenderOk] = useState<boolean | null>(null);
  const [embed, setEmbed] = useState<StudioEmbedStatus | null>(null);
  const [busy, setBusy] = useState("");
  const [liveStudio, setLiveStudio] = useState(false);
  const onAppliedRef = useRef(onApplied);
  onAppliedRef.current = onApplied;

  useEffect(() => {
    if (!open) setLiveStudio(false);
  }, [open, studioOk]);

  const refreshEmbed = useCallback(async () => {
    try {
      const raw = await read<Record<string, unknown>>("/api/studio/embed/status");
      const norm = normalizeEmbedStatus(raw?.status ?? raw);
      setEmbed(norm);
      if (norm.mcp_connected || norm.phase === "connected") {
        setLiveStudio(true);
        onAppliedRef.current({ studioConnected: true, modelConfigured: true, workspaceMode: "studio_mcp" });
      }
    } catch { /* ignore */ }
  }, []);

  const load = useCallback(async () => {
    try {
      const c = await read<ConfigSnap>("/api/config");
      setCfg(c);
      const useAgy = c.provider === "antigravity" || (c.provider !== "openai" && c.antigravity_available);
      setProvider(useAgy ? "antigravity" : "openai");
      setModelId(c.model_id || (useAgy ? c.antigravity_default_model || "gemini-4-hydrogen-high" : ""));
      setBaseUrl(c.base_url || "");
    } catch { /* ignore */ }
    const b = await call<{ available?: boolean; blender_available?: boolean }>("/api/tools/blender/status");
    setBlenderOk(Boolean(b.ok && (b.available ?? b.blender_available ?? true)));
    await refreshEmbed();
  }, [refreshEmbed]);

  useEffect(() => {
    if (open) { setStep(1); void load(); }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => {
    if (!open || step !== 3 || liveStudio) return;
    void refreshEmbed();
    const t = window.setInterval(() => { void refreshEmbed(); }, 2500);
    return () => window.clearInterval(t);
  }, [open, step, liveStudio, refreshEmbed]);

  const saveModelAndNext = async () => {
    setBusy("model");
    try {
      const res = await post<ConfigSnap & { ok?: boolean; error?: string }>("/api/config", {
        provider,
        base_url: provider === "antigravity" ? "agy://antigravity" : baseUrl.trim(),
        model_id: modelId.trim() || "gemini-4-hydrogen-high",
        api_key: provider === "openai" && apiKey.trim() ? apiKey.trim() : undefined,
      });
      if (res.ok === false) { toast.error(res.error || "Could not save model."); return; }
      onApplied({ studioConnected: res.studio_connected, modelConfigured: res.model_configured, workspaceMode: res.active_backend, modelId: res.model_id });
      setStep(2);
    } catch {
      toast.error("Could not save model.");
    } finally {
      setBusy("");
    }
  };

  const launchOrGrabStudio = async () => {
    setBusy("studio");
    if (embed?.studio_running || embed?.already_logged_in) {
      const a = await call<Record<string, unknown>>("/api/studio/embed/attach", {});
      if (a.ok) {
        const norm = normalizeEmbedStatus(a.status ?? a);
        setEmbed(norm);
        if (norm.mcp_connected || norm.phase === "connected") {
          setLiveStudio(true);
          onAppliedRef.current({ studioConnected: true, modelConfigured: true, workspaceMode: "studio_mcp" });
          toast.success("Connected to Roblox Studio");
        } else if (norm.phase === "awaiting_signin") {
          toast("Sign in inside Studio to connect");
        } else {
          setLiveStudio(true);
        }
        setBusy("");
        return;
      }
    }
    const r = await call<{ studio_connected?: boolean; error?: string }>("/api/studio/launch-and-continue", {});
    if (r.ok && r.studio_connected) {
      setLiveStudio(true);
      toast.success("Studio connected");
    } else {
      const e = await call<Record<string, unknown>>("/api/studio/embed/launch", {});
      if (e.ok) {
        const norm = normalizeEmbedStatus(e.status ?? e);
        setEmbed(norm);
        if (norm.mcp_connected || norm.phase === "connected") {
          setLiveStudio(true);
          toast.success("Studio connected");
        } else {
          toast.success("Studio opened");
        }
      } else {
        toast.error(r.error || "Open Roblox Studio to connect.");
      }
    }
    setBusy("");
    await refreshEmbed();
  };

  const finish = () => {
    try { localStorage.setItem(ONBOARDING_KEY, "1"); } catch { /* ignore */ }
    onOpenChange(false);
    setTimeout(onDoneFocusComposer, 80);
  };

  const agyModels: AntigravityModel[] = cfg?.antigravity_models?.length
    ? cfg.antigravity_models.slice(0, 4)
    : [{ id: "gemini-4-hydrogen-high", label: "Gemini 4 Hydrogen" }];

  return (
    <MorphingDialog transition={dialogSpring} open={open} onOpenChange={(next) => { if (!next) try { localStorage.setItem(ONBOARDING_KEY, "1"); } catch {} ; onOpenChange(next); }}>
      <MorphingDialogContainer>
        <MorphingDialogContent className="relative mx-3 flex w-full max-w-[520px] flex-col overflow-hidden rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
          <div data-startup-wizard className="flex flex-col gap-4">
          <div className="flex items-start justify-between gap-3">
            <div className="flex items-center gap-3">
              <ProgressCircle value={Math.round((step / 3) * 100)} radius={15} strokeWidth={3.5} className="size-9 flex-none">
                <span className="font-mono text-xs font-medium tabular-nums text-ink">{step}/3</span>
              </ProgressCircle>
              <div>
                <MorphingDialogTitle className="font-heading text-title font-semibold">Set up OpenRO</MorphingDialogTitle>
                <MorphingDialogDescription className="text-sm text-ink-2">
                  {step === 1 ? "Choose AI model" : step === 2 ? "Ready tools" : "Connect Roblox Studio"}
                </MorphingDialogDescription>
              </div>
            </div>
            <MorphingDialogClose aria-label="Skip setup" className="static inline-flex size-11 cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface text-ink-2 hover:bg-board hover:text-ink md:size-8">
              <X className="size-4" strokeWidth={1.5} aria-hidden />
            </MorphingDialogClose>
          </div>

          {step === 1 && (
            <div className="flex flex-col gap-3">
              <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                {cfg?.antigravity_available !== false && (
                  <Button
                    variant={provider === "antigravity" ? "default" : "outline"}
                    onClick={() => { setProvider("antigravity"); setModelId(cfg?.antigravity_default_model || "gemini-4-hydrogen-high"); }}
                    className="h-auto flex-col items-start gap-1 whitespace-normal py-3 text-left sm:h-auto"
                  >
                    <span className="inline-flex items-center gap-1.5 text-sm font-medium"><Sparkles className="size-4" strokeWidth={1.5} aria-hidden />Gemini 4 Hydrogen</span>
                    <span className={`text-xs ${provider === "antigravity" ? "text-blue-fg" : "text-ink-2"}`}>Recommended default</span>
                  </Button>
                )}
                <Button
                  variant={provider === "openai" ? "default" : "outline"}
                  onClick={() => { setProvider("openai"); if (baseUrl === "agy://antigravity") setBaseUrl("http://127.0.0.1:11434/v1"); }}
                  className="h-auto flex-col items-start gap-1 whitespace-normal py-3 text-left sm:h-auto"
                >
                  <span className="inline-flex items-center gap-1.5 text-sm font-medium"><Cpu className="size-4" strokeWidth={1.5} aria-hidden />OpenAI / Local Gateway</span>
                  <span className={`text-xs ${provider === "openai" ? "text-blue-fg" : "text-ink-2"}`}>Custom key or loopback</span>
                </Button>
              </div>

              {provider === "antigravity" ? (
                <div className="flex flex-wrap gap-1.5">
                  {agyModels.map((m) => (
                    <Button key={m.id} variant={modelId === m.id ? "secondary" : "outline"} size="sm" onClick={() => setModelId(m.id)} className="max-md:min-h-11">
                      {modelId === m.id && <Check className="size-3.5" strokeWidth={2} aria-hidden />}
                      <span>{m.label || m.id}</span>
                    </Button>
                  ))}
                </div>
              ) : (
                <div className="flex flex-col gap-2">
                  <Input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="http://127.0.0.1:11434/v1" className="font-mono text-xs" aria-label="Base URL" />
                  <Input value={modelId} onChange={(e) => setModelId(e.target.value)} placeholder="Model ID" className="font-mono text-xs" aria-label="Model ID" />
                  <Input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder="API Key (optional for local)" className="font-mono text-xs" aria-label="API Key" />
                </div>
              )}

              <div className="flex justify-end gap-2 pt-1">
                <Button variant="ghost" onClick={finish} className="max-md:min-h-11">Skip</Button>
                <Button variant="default" disabled={busy === "model"} onClick={() => void saveModelAndNext()} className="active:scale-[0.99] disabled:opacity-50 max-md:min-h-11">
                  {busy === "model" ? "Saving…" : "Next"}
                </Button>
              </div>
            </div>
          )}

          {step === 2 && (
            <div className="flex flex-col gap-3">
              <ul className="flex flex-col gap-2">
                {[
                  { name: "Studio Bridge", detail: "Sync and playtest in Studio", ready: true },
                  { name: "Blender 3D", detail: "Textured props and meshes", ready: blenderOk ?? true },
                  { name: "Reference & Art Gen", detail: "Shared browser ChatGPT default", ready: true },
                  { name: "Audio Synth", detail: "SFX and game cues", ready: true },
                ].map((t) => (
                  <li key={t.name} className="flex items-center justify-between gap-2 rounded-card border border-hairline bg-board/50 px-3 py-2.5">
                    <div className="min-w-0">
                      <div className="text-sm font-medium text-ink">{t.name}</div>
                      <div className="text-xs text-ink-2">{t.detail}</div>
                    </div>
                    <Pill className={t.ready ? "bg-pill-active-bg px-2.5 py-0.5 text-xs text-pill-active-fg" : "bg-pill-warn-bg px-2.5 py-0.5 text-xs text-pill-warn-fg"}>
                      {t.ready ? "Ready" : "Unavailable"}
                    </Pill>
                  </li>
                ))}
              </ul>
              <div className="flex justify-between gap-2 pt-1">
                <Button variant="outline" onClick={() => setStep(1)} className="max-md:min-h-11">Back</Button>
                <Button variant="default" onClick={() => setStep(3)} className="max-md:min-h-11">Next</Button>
              </div>
            </div>
          )}

          {step === 3 && (
            <div className="flex flex-col gap-3">
              <div className="flex flex-col gap-2.5 rounded-card border border-hairline bg-board/50 p-3.5">
                <div className="flex items-center justify-between gap-2">
                  <span className="inline-flex items-center gap-2 text-sm font-medium text-ink">
                    {liveStudio ? <Check className="size-4 text-dot-green" strokeWidth={2} aria-hidden /> : <Laptop className="size-4 text-icon" strokeWidth={1.5} aria-hidden />}
                    {liveStudio
                      ? "Roblox Studio connected"
                      : embed?.already_logged_in
                        ? "Found your logged-in Roblox Studio"
                        : embed?.phase === "awaiting_signin"
                          ? "Sign in above: OpenRO connects automatically"
                          : "Connect Roblox Studio"}
                  </span>
                  <Pill className={liveStudio ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg" : "bg-pill-inactive-bg px-2 py-0.5 text-xs text-pill-inactive-fg"}>
                    {liveStudio ? "Connected" : embed?.phase === "awaiting_signin" ? "Waiting" : "Optional"}
                  </Pill>
                </div>

                {!liveStudio && embed?.phase === "awaiting_signin" && embed.running && (
                  <div className="overflow-hidden rounded-sm border border-hairline bg-surface">
                    <img
                      src={embed.stream_url || "/api/studio/stream.mjpg"}
                      alt="Live Roblox Studio sign-in stream"
                      className="max-h-52 w-full object-contain"
                    />
                  </div>
                )}

                {!liveStudio && embed?.phase !== "awaiting_signin" && (
                  <p className="text-xs text-ink-2">
                    {embed?.already_logged_in
                      ? `${embed.running_studios?.[0]?.place_name || "Your open Studio session"} is ready.`
                      : "Already have Studio open? OpenRO grabs it automatically."}
                  </p>
                )}

                {!liveStudio && embed?.phase !== "awaiting_signin" && (
                  <Button variant="outline" size="sm" disabled={busy === "studio"} onClick={() => void launchOrGrabStudio()} className="self-start max-md:min-h-11">
                    <Rocket className="size-3.5" strokeWidth={1.5} aria-hidden />
                    {busy === "studio"
                      ? "Connecting…"
                      : embed?.already_logged_in || embed?.studio_running
                        ? "Grab & Connect"
                        : "Open My Roblox Studio"}
                  </Button>
                )}
              </div>
              <div className="flex justify-between gap-2 pt-1">
                <Button variant="outline" onClick={() => setStep(2)} className="max-md:min-h-11">Back</Button>
                <Button variant="default" onClick={finish} className="max-md:min-h-11">
                  <Hammer className="size-4" strokeWidth={1.5} aria-hidden />Start Building
                </Button>
              </div>
            </div>
          )}
          </div>
        </MorphingDialogContent>
      </MorphingDialogContainer>
    </MorphingDialog>
  );
}
