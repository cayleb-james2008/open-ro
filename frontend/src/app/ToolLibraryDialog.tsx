import { useState } from "react";
import { toast } from "sonner";
import {
  Box,
  Check,
  ChevronDown,
  Download,
  Music,
  Package,
  Plug,
  Settings2,
  Sparkles,
  Trash2,
  Volume2,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import { Pill, PillIndicator } from "@/components/kibo-ui/pill";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogDescription,
  MorphingDialogTitle,
} from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import {
  post,
  type AudioGenResult,
  type BlenderGenResult,
  type LibraryPack,
  type LibrarySnapshot,
} from "./data";

type LibraryTab = "installed" | "installable" | "generators";

const BLENDER_PRESETS = [
  { id: "torii_gate", label: "Torii Summon Gate", desc: "Dual-pillar Japanese shrine gate scaled to Roblox studs" },
  { id: "card_pedestal", label: "Card Pedestal", desc: "Illuminated Hall of Legends character display stand" },
  { id: "colosseum_arch", label: "Boss Colosseum Arch", desc: "Heroic arena archway for the Boss Raid plaza" },
  { id: "cauldron", label: "Alchemist Cauldron", desc: "Potion & trait forge cauldron pavilion prop" },
  { id: "crystal_obelisk", label: "Crystal Obelisk", desc: "Floating rarity spire monument for Infinite Tower" },
] as const;

const AUDIO_PRESETS = [
  { id: "summon_roll", label: "Summon Roll", desc: "Ascending crystal chime arpeggio for Torii pack rolls" },
  { id: "mythic_fanfare", label: "Mythic Fanfare", desc: "Bright heroic chord fanfare for Mythic/Secret pulls" },
  { id: "battle_crit", label: "Battle Crit", desc: "Punchy harmonic impact hit for 4v4 card clash crits" },
  { id: "potion_drink", label: "Potion Drink", desc: "Bubbling shimmer rise for Luck/Speed potion buffs" },
  { id: "ui_click", label: "UI Click", desc: "Crisp tactile interface click for Roblox ScreenGuis" },
  { id: "level_up", label: "Level Up", desc: "Tri-tone victory flourish for Tower floor clears" },
] as const;

function PackCard({
  pack,
  busyId,
  onInstall,
  onUninstall,
  onConfigure,
  onTest,
}: {
  pack: LibraryPack;
  busyId: string | null;
  onInstall: (id: string) => Promise<void>;
  onUninstall: (id: string) => Promise<void>;
  onConfigure: (id: string, payload: { api_key?: string; base_url?: string; provider?: string; binary_path?: string; clear_api_key?: boolean }) => Promise<void>;
  onTest: (id: string) => Promise<{ passed: boolean; detail: string; latency_ms?: number }>;
}) {
  const [configOpen, setConfigOpen] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [provider, setProvider] = useState(pack.provider || "");
  const [binaryPath, setBinaryPath] = useState(pack.detected_binary || "");
  const [testResult, setTestResult] = useState<{ passed: boolean; detail: string; latency_ms?: number } | null>(null);
  const busy = busyId === pack.id;

  const handleSaveConfig = async () => {
    await onConfigure(pack.id, {
      ...(apiKey.trim() ? { api_key: apiKey.trim() } : {}),
      ...(baseUrl.trim() ? { base_url: baseUrl.trim() } : {}),
      ...(provider.trim() ? { provider: provider.trim() } : {}),
      ...(binaryPath.trim() ? { binary_path: binaryPath.trim() } : {}),
    });
    setApiKey("");
  };

  const handleTestClick = async () => {
    const res = await onTest(pack.id);
    setTestResult(res);
  };

  return (
    <article
      data-library-pack={pack.id}
      className="flex flex-col gap-2.5 rounded-card border border-hairline bg-board/45 p-3.5"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <h3 className="text-sm font-medium text-ink">{pack.name}</h3>
            <Badge className="border-transparent bg-surface px-2 py-0.5 text-xs font-medium text-ink-2">
              {pack.category}
            </Badge>
            {pack.default_installed ? (
              <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg">
                <Check className="size-3" strokeWidth={1.5} aria-hidden />
                Built-in
              </Pill>
            ) : pack.installed ? (
              <Pill className="bg-tag-mint-bg px-2 py-0.5 text-xs text-tag-mint-fg">
                <PillIndicator variant="success" />
                Installed
              </Pill>
            ) : (
              <Pill className="bg-surface px-2 py-0.5 text-xs text-ink-2">Available</Pill>
            )}
            {pack.masked_key && (
              <Pill className="bg-tag-sky-bg px-2 py-0.5 font-mono text-xs text-tag-sky-fg">
                Key {pack.masked_key}
              </Pill>
            )}
          </div>
          <p className="text-xs leading-relaxed text-ink-2">{pack.description}</p>
        </div>

        <div className="flex shrink-0 flex-wrap items-center gap-1.5">
          {!pack.default_installed && (
            pack.installed ? (
              <Button
                variant="outline"
                size="xs"
                disabled={busy}
                aria-label={`Remove ${pack.name}`}
                onClick={() => void onUninstall(pack.id)}
                className="bg-surface max-md:min-h-11"
              >
                <Trash2 className="size-3" strokeWidth={1.5} aria-hidden />
                {busy ? "Removing…" : "Remove"}
              </Button>
            ) : (
              <Button
                variant="default"
                size="xs"
                disabled={busy}
                aria-label={`Install ${pack.name}`}
                onClick={() => void onInstall(pack.id)}
                className="max-md:min-h-11"
              >
                <Package className="size-3" strokeWidth={1.5} aria-hidden />
                {busy ? "Installing…" : "Install"}
              </Button>
            )
          )}
          <Button
            variant="outline"
            size="xs"
            disabled={busy}
            aria-label={`Test connection for ${pack.name}`}
            onClick={() => void handleTestClick()}
            className="bg-surface max-md:min-h-11"
          >
            <Plug className="size-3" strokeWidth={1.5} aria-hidden />
            Test Connection
          </Button>
        </div>
      </div>

      {/* Capabilities & Unlocked Agent Tools */}
      <div className="flex flex-wrap items-center gap-1.5">
        {pack.tools.map((t) => (
          <Pill key={t} className="bg-surface px-2 py-0.5 font-mono text-xs text-ink-2">
            {t}
          </Pill>
        ))}
        {pack.detected_binary && (
          <Pill className="bg-tag-ops-bg px-2 py-0.5 font-mono text-xs text-tag-ops-fg">
            {pack.detected_binary}
          </Pill>
        )}
      </div>

      {testResult && (
        <div
          data-pack-test-result={pack.id}
          className={`flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline px-2.5 py-1.5 text-xs ${
            testResult.passed ? "bg-pill-active-bg/40 text-pill-active-fg" : "bg-pill-warn-bg/40 text-pill-warn-fg"
          }`}
        >
          <span>{testResult.detail}</span>
          {typeof testResult.latency_ms === "number" && (
            <span className="font-mono tabular-nums">{testResult.latency_ms}ms</span>
          )}
        </div>
      )}

      {/* Expandable Provider Configuration */}
      <Expandable
        expandDirection="vertical"
        expandBehavior="push"
        expanded={configOpen}
        onToggle={() => setConfigOpen((v) => !v)}
      >
        {({ isExpanded }) => (
          <>
            <div className="flex items-center justify-between border-t border-hairline pt-2">
              <span className="text-xs text-ink-2">
                {pack.skill_summary || "Provider credentials & binary path"}
              </span>
              <ExpandableTrigger
                aria-label={`Configure ${pack.name}`}
                className="inline-flex min-h-11 items-center gap-1 rounded-sm px-2 text-xs font-medium text-ink-2 outline-none hover:bg-surface hover:text-ink md:min-h-7"
              >
                <Settings2 className="size-3" strokeWidth={1.5} aria-hidden />
                <span>Configure</span>
                <ChevronDown
                  className={`size-3 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`}
                  strokeWidth={1.5}
                  aria-hidden
                />
              </ExpandableTrigger>
            </div>
            <ExpandableContent preset="fade">
              <div className="mt-2 grid gap-2 rounded-sm border border-hairline bg-surface p-2.5 sm:grid-cols-2">
                <label className="flex flex-col gap-1 text-xs text-ink-2">
                  <span>Provider</span>
                  <Input
                    value={provider}
                    onChange={(e) => setProvider(e.target.value)}
                    aria-label={`${pack.name} provider`}
                    placeholder={pack.provider || "provider id"}
                    className="h-8 font-mono text-xs max-md:min-h-11"
                  />
                </label>
                <label className="flex flex-col gap-1 text-xs text-ink-2">
                  <span>API Key {pack.masked_key ? `(saved: ${pack.masked_key})` : "(stored 0600 locally)"}</span>
                  <Input
                    type="password"
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    aria-label={`${pack.name} API key`}
                    placeholder="Paste key to update (never echoed)"
                    className="h-8 font-mono text-xs max-md:min-h-11"
                  />
                </label>
                <label className="flex flex-col gap-1 text-xs text-ink-2">
                  <span>Base URL (optional)</span>
                  <Input
                    value={baseUrl}
                    onChange={(e) => setBaseUrl(e.target.value)}
                    aria-label={`${pack.name} base URL`}
                    placeholder="https://..."
                    className="h-8 font-mono text-xs max-md:min-h-11"
                  />
                </label>
                <label className="flex flex-col gap-1 text-xs text-ink-2">
                  <span>Binary Path (optional)</span>
                  <Input
                    value={binaryPath}
                    onChange={(e) => setBinaryPath(e.target.value)}
                    aria-label={`${pack.name} binary path`}
                    placeholder="/usr/bin/blender"
                    className="h-8 font-mono text-xs max-md:min-h-11"
                  />
                </label>
                <div className="flex flex-wrap items-center justify-end gap-1.5 sm:col-span-2">
                  {pack.masked_key && (
                    <Button
                      variant="ghost"
                      size="xs"
                      disabled={busy}
                      onClick={() => void onConfigure(pack.id, { clear_api_key: true })}
                      className="max-md:min-h-11"
                    >
                      Clear saved key
                    </Button>
                  )}
                  <Button
                    variant="default"
                    size="xs"
                    disabled={busy}
                    onClick={() => void handleSaveConfig()}
                    className="max-md:min-h-11"
                  >
                    Save configuration
                  </Button>
                </div>
              </div>
            </ExpandableContent>
          </>
        )}
      </Expandable>
    </article>
  );
}

export function ToolLibraryDialog({
  open,
  onOpenChange,
  snapshot,
  v090Ready,
  onRefresh,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  snapshot: LibrarySnapshot;
  v090Ready: boolean;
  onRefresh: () => Promise<void>;
}) {
  const [tab, setTab] = useState<LibraryTab>("installed");
  const [busyId, setBusyId] = useState<string | null>(null);
  const [blenderPreset, setBlenderPreset] = useState<string>("torii_gate");
  const [blenderBusy, setBlenderBusy] = useState(false);
  const [blenderRes, setBlenderRes] = useState<BlenderGenResult | null>(null);
  const [audioPreset, setAudioPreset] = useState<string>("mythic_fanfare");
  const [audioBusy, setAudioBusy] = useState(false);
  const [audioRes, setAudioRes] = useState<AudioGenResult | null>(null);

  const defaultPacks = snapshot.packs.filter((p) => p.default_installed);
  const installablePacks = snapshot.packs.filter((p) => !p.default_installed);
  const installedCount = snapshot.packs.filter((p) => p.installed).length;

  const handleInstall = async (id: string) => {
    if (!v090Ready) {
      toast.error("Tool Library install requires OpenRO v0.9.0.");
      return;
    }
    setBusyId(id);
    try {
      const res = await post<{ ok?: boolean; error?: string }>("/api/library/install", { id });
      if (res.ok) {
        toast.success(`Installed ${id} — new agent tools unlocked`);
        await onRefresh();
      } else {
        toast.error(res.error || `Could not install ${id}.`);
      }
    } catch {
      toast.error("Could not reach /api/library/install.");
    } finally {
      setBusyId(null);
    }
  };

  const handleUninstall = async (id: string) => {
    if (!v090Ready) return;
    setBusyId(id);
    try {
      const res = await post<{ ok?: boolean; error?: string }>("/api/library/uninstall", { id });
      if (res.ok) {
        toast.success(`Removed ${id} from active agent tools`);
        await onRefresh();
      } else {
        toast.error(res.error || `Could not remove ${id}.`);
      }
    } catch {
      toast.error("Could not reach /api/library/uninstall.");
    } finally {
      setBusyId(null);
    }
  };

  const handleConfigure = async (
    id: string,
    payload: { api_key?: string; base_url?: string; provider?: string; binary_path?: string; clear_api_key?: boolean },
  ) => {
    if (!v090Ready) return;
    setBusyId(id);
    try {
      const res = await post<{ ok?: boolean; error?: string }>("/api/library/configure", { id, ...payload });
      if (res.ok) {
        toast.success(`Saved configuration for ${id}`);
        await onRefresh();
      } else {
        toast.error(res.error || `Could not save configuration for ${id}.`);
      }
    } catch {
      toast.error("Could not reach /api/library/configure.");
    } finally {
      setBusyId(null);
    }
  };

  const handleTest = async (id: string) => {
    if (!v090Ready) return { passed: false, detail: "Requires OpenRO v0.9.0." };
    setBusyId(id);
    try {
      const res = await post<{ ok?: boolean; passed?: boolean; detail?: string; latency_ms?: number; error?: string }>(
        "/api/library/test",
        { id },
      );
      return {
        passed: res.passed === true,
        detail: res.detail || res.error || (res.passed ? "Connection healthy." : "Connection check failed."),
        latency_ms: res.latency_ms,
      };
    } catch {
      return { passed: false, detail: "Could not reach /api/library/test." };
    } finally {
      setBusyId(null);
    }
  };

  const handleGenerateBlender = async () => {
    if (!v090Ready || blenderBusy) return;
    setBlenderBusy(true);
    try {
      const res = await post<BlenderGenResult>("/api/tools/blender/generate", {
        preset: blenderPreset,
        render_preview: true,
      });
      setBlenderRes(res);
      if (res.ok) toast.success(`Generated 3D mesh ${res.filename || blenderPreset}`);
      else toast.error(res.error || "Blender mesh generation failed.");
    } catch {
      toast.error("Could not reach /api/tools/blender/generate.");
    } finally {
      setBlenderBusy(false);
    }
  };

  const handleGenerateAudio = async () => {
    if (!v090Ready || audioBusy) return;
    setAudioBusy(true);
    try {
      const res = await post<AudioGenResult>("/api/tools/audio/generate", { preset: audioPreset });
      setAudioRes(res);
      if (res.ok) toast.success(`Synthesized ${res.filename || audioPreset}.wav`);
      else toast.error(res.error || "Audio SFX synthesis failed.");
    } catch {
      toast.error("Could not reach /api/tools/audio/generate.");
    } finally {
      setAudioBusy(false);
    }
  };

  return (
    <MorphingDialog transition={dialogSpring} open={open} onOpenChange={onOpenChange}>
      <MorphingDialogContainer>
        <MorphingDialogContent className="relative mx-3 flex max-h-[86dvh] w-full max-w-[820px] flex-col rounded-dialog border border-hairline bg-surface text-ink shadow-lift">
          <div data-tool-library-dialog className="flex min-h-0 flex-1 flex-col">
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-hairline px-4 py-3">
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                <Package className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                <MorphingDialogTitle className="text-ui font-medium text-ink">
                  Tool &amp; Skill Library
                </MorphingDialogTitle>
                <Pill className="bg-pill-active-bg px-2 py-0.5 font-mono text-xs tabular-nums text-pill-active-fg">
                  {installedCount}/{snapshot.packs.length} active
                </Pill>
                <Pill className="bg-board px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2">
                  {snapshot.active_tools.length} agent tools
                </Pill>
              </div>
              <MorphingDialogClose
                aria-label="Close Tool Library"
                className="static inline-flex size-11 flex-none cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface text-ink-2 outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board hover:text-ink md:size-8"
              >
                <X className="size-4" strokeWidth={1.5} aria-hidden />
              </MorphingDialogClose>
            </div>

            <MorphingDialogDescription className="sr-only">
              Manage built-in Roblox Studio, Blender 3D, Anime Fan-Art, procedural audio SFX, and installable media packs for OpenRO agents.
            </MorphingDialogDescription>

            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-hairline bg-board/40 px-4 py-2.5">
              <SegmentedControl aria-label="Tool library views" size="sm" className="max-w-full flex-wrap">
                <SegmentedControlItem
                  data-checked={tab === "installed" ? "" : undefined}
                  aria-pressed={tab === "installed"}
                  onClick={() => setTab("installed")}
                  className="max-md:!h-11 max-md:px-2.5"
                >
                  Installed Default ({defaultPacks.length})
                </SegmentedControlItem>
                <SegmentedControlItem
                  data-checked={tab === "installable" ? "" : undefined}
                  aria-pressed={tab === "installable"}
                  onClick={() => setTab("installable")}
                  className="max-md:!h-11 max-md:px-2.5"
                >
                  Installable Packs ({installablePacks.length})
                </SegmentedControlItem>
                <SegmentedControlItem
                  data-checked={tab === "generators" ? "" : undefined}
                  aria-pressed={tab === "generators"}
                  onClick={() => setTab("generators")}
                  className="max-md:!h-11 max-md:px-2.5"
                >
                  3D &amp; Audio Generators
                </SegmentedControlItem>
              </SegmentedControl>

              <Pill className="bg-surface px-2 py-0.5 font-mono text-xs text-ink-2">
                {snapshot.blender_detected
                  ? `Blender Ready (${snapshot.blender_path || "CLI"})`
                  : "Procedural 3D + .wav Ready"}
              </Pill>
            </div>

            <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4">
              {tab === "installed" && (
                <div className="flex flex-col gap-2.5">
                  {defaultPacks.map((pack) => (
                    <PackCard
                      key={pack.id}
                      pack={pack}
                      busyId={busyId}
                      onInstall={handleInstall}
                      onUninstall={handleUninstall}
                      onConfigure={handleConfigure}
                      onTest={handleTest}
                    />
                  ))}
                </div>
              )}

              {tab === "installable" && (
                <div className="flex flex-col gap-2.5">
                  {installablePacks.map((pack) => (
                    <PackCard
                      key={pack.id}
                      pack={pack}
                      busyId={busyId}
                      onInstall={handleInstall}
                      onUninstall={handleUninstall}
                      onConfigure={handleConfigure}
                      onTest={handleTest}
                    />
                  ))}
                </div>
              )}

              {tab === "generators" && (
                <div data-library-generators className="grid gap-3 md:grid-cols-2">
                  {/* Generate 3D Mesh (Blender) */}
                  <section className="flex flex-col gap-2.5 rounded-card border border-hairline bg-board/45 p-3.5">
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <Box className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                        <h3 className="text-sm font-medium text-ink">Generate 3D Mesh (Blender)</h3>
                      </div>
                      <Badge className="border-transparent bg-tag-sky-bg px-2 py-0.5 text-xs text-tag-sky-fg">
                        .OBJ
                      </Badge>
                    </div>
                    <p className="text-xs text-ink-2">
                      Build UV-unwrapped Roblox-scaled hero props (1m = 3.57 studs) via headless Blender CLI or procedural mesh fallback.
                    </p>
                    <div className="flex flex-wrap gap-1.5">
                      {BLENDER_PRESETS.map((p) => (
                        <Button
                          key={p.id}
                          variant={blenderPreset === p.id ? "default" : "outline"}
                          size="xs"
                          aria-pressed={blenderPreset === p.id}
                          onClick={() => setBlenderPreset(p.id)}
                          className={blenderPreset === p.id ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
                        >
                          {p.label}
                        </Button>
                      ))}
                    </div>
                    <Button
                      variant="default"
                      size="sm"
                      disabled={blenderBusy}
                      aria-label="Generate 3D Mesh with Blender"
                      onClick={() => void handleGenerateBlender()}
                      className="w-full justify-center max-md:min-h-11"
                    >
                      <Sparkles className="size-3.5" strokeWidth={1.5} aria-hidden />
                      {blenderBusy ? "Generating 3D Mesh…" : "Generate 3D Mesh (Blender)"}
                    </Button>
                    {blenderRes && (
                      <div
                        data-blender-gen-result
                        className="flex flex-col gap-1.5 rounded-sm border border-hairline bg-surface p-2.5 text-xs"
                      >
                        <div className="flex flex-wrap items-center justify-between gap-1.5">
                          <span className="font-mono font-medium text-ink">
                            {blenderRes.filename || blenderRes.preset}
                          </span>
                          <Pill className="bg-pill-active-bg px-2 py-0.5 font-mono text-xs text-pill-active-fg">
                            {blenderRes.triangles ?? 0} tris · {Math.max(1, Math.round((blenderRes.bytes ?? 0) / 1024))} KB
                          </Pill>
                        </div>
                        {blenderRes.detail && <p className="text-ink-2">{blenderRes.detail}</p>}
                        {blenderRes.preview_url && (
                          <img
                            src={blenderRes.preview_url}
                            alt={`Blender preview of ${blenderRes.preset}`}
                            className="max-h-36 w-full rounded-sm border border-hairline object-contain"
                          />
                        )}
                        {blenderRes.obj_url && (
                          <a
                            href={blenderRes.obj_url}
                            download={blenderRes.filename || `${blenderRes.preset}.obj`}
                            className="inline-flex items-center gap-1.5 font-medium text-blue hover:underline"
                          >
                            <Download className="size-3.5" strokeWidth={1.5} aria-hidden />
                            Download {blenderRes.filename || `${blenderRes.preset}.obj`}
                          </a>
                        )}
                      </div>
                    )}
                  </section>

                  {/* Generate SFX (.wav) */}
                  <section className="flex flex-col gap-2.5 rounded-card border border-hairline bg-board/45 p-3.5">
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <Music className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                        <h3 className="text-sm font-medium text-ink">Generate SFX (.wav)</h3>
                      </div>
                      <Badge className="border-transparent bg-tag-mint-bg px-2 py-0.5 text-xs text-tag-mint-fg">
                        PCM16 WAV
                      </Badge>
                    </div>
                    <p className="text-xs text-ink-2">
                      Synthesize royalty-free Roblox game sound effects locally with zero external dependencies and preview immediately.
                    </p>
                    <div className="flex flex-wrap gap-1.5">
                      {AUDIO_PRESETS.map((p) => (
                        <Button
                          key={p.id}
                          variant={audioPreset === p.id ? "default" : "outline"}
                          size="xs"
                          aria-pressed={audioPreset === p.id}
                          onClick={() => setAudioPreset(p.id)}
                          className={audioPreset === p.id ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
                        >
                          {p.label}
                        </Button>
                      ))}
                    </div>
                    <Button
                      variant="default"
                      size="sm"
                      disabled={audioBusy}
                      aria-label="Generate SFX wav audio"
                      onClick={() => void handleGenerateAudio()}
                      className="w-full justify-center max-md:min-h-11"
                    >
                      <Volume2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                      {audioBusy ? "Synthesizing .wav…" : "Generate SFX (.wav)"}
                    </Button>
                    {audioRes && (
                      <div
                        data-audio-gen-result
                        className="flex flex-col gap-2 rounded-sm border border-hairline bg-surface p-2.5 text-xs"
                      >
                        <div className="flex flex-wrap items-center justify-between gap-1.5">
                          <span className="font-mono font-medium text-ink">
                            {audioRes.filename || `${audioRes.preset}.wav`}
                          </span>
                          <Pill className="bg-pill-active-bg px-2 py-0.5 font-mono text-xs text-pill-active-fg">
                            {audioRes.duration_ms ?? 450}ms · {Math.max(1, Math.round((audioRes.bytes ?? 0) / 1024))} KB
                          </Pill>
                        </div>
                        {audioRes.wav_url && (
                          <>
                            <audio controls src={audioRes.wav_url} className="h-9 w-full" />
                            <a
                              href={audioRes.wav_url}
                              download={audioRes.filename || `${audioRes.preset}.wav`}
                              className="inline-flex items-center gap-1.5 font-medium text-blue hover:underline"
                            >
                              <Download className="size-3.5" strokeWidth={1.5} aria-hidden />
                              Download {audioRes.filename || `${audioRes.preset}.wav`}
                            </a>
                          </>
                        )}
                      </div>
                    )}
                  </section>
                </div>
              )}
            </div>
          </div>
        </MorphingDialogContent>
      </MorphingDialogContainer>
    </MorphingDialog>
  );
}
