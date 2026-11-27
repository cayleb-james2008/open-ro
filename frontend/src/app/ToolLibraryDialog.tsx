import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import {
  Box,
  Check,
  ChevronDown,
  Cpu,
  DollarSign,
  Download,
  Flame,
  Gamepad2,
  Layers,
  Music,
  Package,
  Plug,
  Plus,
  RefreshCw,
  Search,
  Send,
  Settings2,
  ShieldCheck,
  Sparkles,
  Trash2,
  TrendingUp,
  Volume2,
  Wand2,
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
import {
  formatCcu,
  formatCompactNumber,
  normalizeResearchResult,
  type MarketTasteResearchResult,
  type MarketTasteSnapshot,
} from "./marketTaste";
import { call, enc } from "./ship/api";
import { AdaptersPanel } from "./AdaptersPanel";

type LibraryTab = "installed" | "installable" | "sfx-packs" | "adapters" | "market" | "generators";

export type SfxPackEntry = {
  id: string;
  name: string;
  author: string;
  license: string;
  url?: string;
  description: string;
  tags?: string[];
  clip_count: number;
  installed: boolean;
  installed_clips?: number;
  installed_at_ms?: number | null;
  custom?: boolean;
};

export type SfxPacksResponse = {
  ok?: boolean;
  packs?: SfxPackEntry[];
  installed_packs_count?: number;
  installed_clips_total?: number;
  runner_installed?: boolean;
  runner_path?: string | null;
};

export type SfxSearchClip = {
  pack_id: string;
  pack_name: string;
  license?: string;
  rel_path: string;
  filename: string;
  ext?: string;
  bytes?: number;
  tags?: string[];
  preview_url?: string;
};

export type SfxSearchResponse = {
  ok?: boolean;
  query?: string;
  total?: number;
  clips?: SfxSearchClip[];
};

export type SfxUseResponse = {
  ok?: boolean;
  path?: string;
  project_path?: string;
  filename?: string;
  url?: string;
  wav_url?: string;
  bytes?: number;
  duration_ms?: number;
  sample_rate?: number;
  attribution?: string;
  error?: string;
};

export type AudioRunnerInstallResponse = {
  ok?: boolean;
  path?: string;
  command?: string;
  executable?: boolean;
  python_available?: boolean;
  backends?: string[];
  install_hint?: string;
  error?: string;
};

const AUDIO_MODEL_TIERS = [
  { tier: "Tiny CPU ~185M", detail: "AudioLDM-S local neural SFX (4GB RAM)" },
  { tier: "Small CPU ~341M", detail: "Stable Audio Small local SFX (8GB RAM)" },
  { tier: "Medium GPU ~515M", detail: "TangoFlux local SFX (8-12GB GPU)" },
  { tier: "Large GPU ~1.1B", detail: "AudioLDM 2 and Stable Audio Open (12-16GB+ GPU)" },
  { tier: "ACE-Step Local HTTP", detail: "Local ACE-Step 1.5 server on 127.0.0.1:8001" },
  { tier: "Google Gemini Lyria", detail: "Cloud Lyria audio generation (gemini_api_key)" },
  { tier: "ElevenLabs", detail: "Cloud Sound Generation API (elevenlabs_api_key)" },
] as const;

function buildPreviewUrl(clip: SfxSearchClip): string {
  if (clip.preview_url && clip.preview_url.trim()) return clip.preview_url;
  const encodedRel = clip.rel_path
    .split("/")
    .filter(Boolean)
    .map((seg) => encodeURIComponent(seg))
    .join("/");
  return `/api/library/sfx-packs/preview/${enc(clip.pack_id)}/${encodedRel}`;
}

function defaultClipOutName(filename: string): string {
  const stem = filename.replace(/\.[^.]+$/, "").trim();
  return stem ? `${stem}.wav` : "sfx_clip.wav";
}

const BLENDER_PRESETS = [
  { id: "torii_gate", label: "Shrine Gate", desc: "Dual-pillar stylized archway scaled to Roblox studs" },
  { id: "card_pedestal", label: "Display Pedestal", desc: "Illuminated showcase pedestal prop for game hubs" },
  { id: "colosseum_arch", label: "Arena Arch", desc: "Heroic entrance archway for PvP or boss arenas" },
  { id: "cauldron", label: "Prop Cauldron", desc: "Stylized crafting & alchemy hub prop" },
  { id: "crystal_obelisk", label: "Crystal Obelisk", desc: "Floating spire landmark for world hubs & checkpoints" },
] as const;

const AUDIO_PRESETS = [
  { id: "summon_roll", label: "Loot Chime", desc: "Ascending crystal chime arpeggio for unlocks & drops" },
  { id: "mythic_fanfare", label: "Victory Fanfare", desc: "Bright heroic chord fanfare for major milestones" },
  { id: "battle_crit", label: "Combat Impact", desc: "Punchy harmonic impact hit for melee & ability crits" },
  { id: "potion_drink", label: "Power-Up Drink", desc: "Bubbling shimmer rise for buffs & pickups" },
  { id: "ui_click", label: "UI Click", desc: "Crisp tactile interface click for Roblox ScreenGuis" },
  { id: "level_up", label: "Level Up", desc: "Tri-tone victory flourish for rank & stage clears" },
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
              <Pill className="bg-tag-sky-bg px-2 py-0.5 text-xs text-tag-sky-fg">
                Key <code className="font-mono">{pack.masked_key}</code>
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
            <span className="tabular-nums">{testResult.latency_ms}ms</span>
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

function SfxPacksTab({
  open,
  projectId,
  onOpenAudioAdapters,
  onSfxAdded,
}: {
  open: boolean;
  projectId?: string | null;
  onOpenAudioAdapters: () => void;
  onSfxAdded?: () => void;
}) {
  const [packs, setPacks] = useState<SfxPackEntry[]>([]);
  const [installedClipsTotal, setInstalledClipsTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [unavailable, setUnavailable] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busyPackId, setBusyPackId] = useState<string | null>(null);
  const [packActionError, setPackActionError] = useState<string | null>(null);

  const [searchQuery, setSearchQuery] = useState("");
  const searchQueryRef = useRef(searchQuery);
  searchQueryRef.current = searchQuery;
  const [searchBusy, setSearchBusy] = useState(false);
  const [clips, setClips] = useState<SfxSearchClip[]>([]);
  const [outNames, setOutNames] = useState<Record<string, string>>({});
  const [useBusyKey, setUseBusyKey] = useState<string | null>(null);
  const [useResult, setUseResult] = useState<SfxUseResponse | null>(null);

  const [runnerInstalled, setRunnerInstalled] = useState(false);
  const [runnerPath, setRunnerPath] = useState<string | null>(null);
  const [runnerBusy, setRunnerBusy] = useState(false);
  const [runnerResult, setRunnerResult] = useState<AudioRunnerInstallResponse | null>(null);
  const [runnerNote, setRunnerNote] = useState<string | null>(null);

  const runSearch = useCallback(async (queryText: string) => {
    setSearchBusy(true);
    const res = await call<SfxSearchResponse>(
      `/api/library/sfx-packs/search?q=${encodeURIComponent(queryText.trim())}&limit=40`,
    );
    setSearchBusy(false);
    if (res.ok) {
      setClips(Array.isArray(res.clips) ? res.clips : []);
    } else if (res.httpStatus === 404) {
      setClips([]);
    }
  }, []);

  const refreshSfxPacks = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    const res = await call<SfxPacksResponse>("/api/library/sfx-packs");
    setLoading(false);
    if (res.httpStatus === 404) {
      setUnavailable(true);
      setPacks([]);
      setInstalledClipsTotal(0);
      setClips([]);
      return;
    }
    if (!res.ok) {
      setUnavailable(false);
      setLoadError(res.error || "Could not load CC0 SFX packs.");
      return;
    }
    setUnavailable(false);
    const nextPacks = Array.isArray(res.packs) ? res.packs : [];
    const nextTotal =
      typeof res.installed_clips_total === "number" && Number.isFinite(res.installed_clips_total)
        ? res.installed_clips_total
        : 0;
    setPacks(nextPacks);
    setInstalledClipsTotal(nextTotal);
    setRunnerInstalled(Boolean(res.runner_installed));
    if (typeof res.runner_path === "string" && res.runner_path.trim()) {
      setRunnerPath(res.runner_path.trim());
    }
    if (nextTotal > 0) {
      await runSearch(searchQueryRef.current);
    } else {
      setClips([]);
    }
  }, [runSearch]);

  useEffect(() => {
    if (open) {
      void refreshSfxPacks();
    }
  }, [open, refreshSfxPacks]);

  const handleInstallPack = async (pack: SfxPackEntry) => {
    setBusyPackId(pack.id);
    setPackActionError(null);
    const res = await call<SfxPacksResponse & { error?: string }>("/api/library/sfx-packs/install", {
      pack_id: pack.id,
      id: pack.id,
    });
    setBusyPackId(null);
    if (!res.ok) {
      const msg =
        res.httpStatus === 404
          ? "CC0 SFX pack installation requires the desktop OpenRO server."
          : res.error || `Offline or download error while installing ${pack.name}.`;
      setPackActionError(msg);
      toast.error(msg);
      return;
    }
    if (Array.isArray(res.packs)) setPacks(res.packs);
    if (typeof res.installed_clips_total === "number") setInstalledClipsTotal(res.installed_clips_total);
    toast.success(`Downloaded ${pack.name}`);
    await refreshSfxPacks();
  };

  const handleRemovePack = async (pack: SfxPackEntry) => {
    setBusyPackId(pack.id);
    setPackActionError(null);
    const res = await call<SfxPacksResponse & { error?: string }>(
      `/api/library/sfx-packs/${enc(pack.id)}`,
      undefined,
      { method: "DELETE" },
    );
    setBusyPackId(null);
    if (!res.ok) {
      const msg = res.error || `Could not remove ${pack.name}.`;
      setPackActionError(msg);
      toast.error(msg);
      return;
    }
    if (Array.isArray(res.packs)) setPacks(res.packs);
    if (typeof res.installed_clips_total === "number") setInstalledClipsTotal(res.installed_clips_total);
    toast(`Removed ${pack.name}`);
    await refreshSfxPacks();
  };

  const handleUseClip = async (clip: SfxSearchClip) => {
    const clipKey = `${clip.pack_id}:${clip.rel_path}`;
    const customOut = (outNames[clipKey] ?? "").trim();
    setUseBusyKey(clipKey);
    const res = await call<SfxUseResponse>("/api/library/sfx-packs/use", {
      pack_id: clip.pack_id,
      clip: clip.rel_path,
      ...(customOut ? { out_name: customOut } : {}),
      ...(projectId ? { project: projectId } : {}),
    });
    setUseBusyKey(null);
    if (!res.ok) {
      toast.error(res.error || `Could not add ${clip.filename} to project.`);
      return;
    }
    setUseResult(res);
    toast.success(`Added ${res.filename || clip.filename} to project`);
    onSfxAdded?.();
  };

  const handleInstallAudioRunner = async () => {
    setRunnerBusy(true);
    setRunnerNote(null);
    const res = await call<AudioRunnerInstallResponse>("/api/library/audio-runner/install", {});
    setRunnerBusy(false);
    if (res.httpStatus === 404) {
      const msg = "Local Audio Runner installer is available on the desktop server (openro-rs v1.33.0+).";
      setRunnerNote(msg);
      toast.info(msg);
      return;
    }
    if (!res.ok) {
      const msg = res.error || "Could not install Local Audio Runner.";
      setRunnerNote(msg);
      toast.error(msg);
      return;
    }
    setRunnerInstalled(true);
    if (typeof res.path === "string" && res.path.trim()) setRunnerPath(res.path.trim());
    setRunnerResult(res);
    toast.success("Installed Local Audio Runner (openro-audio-gen)");
  };

  const installedPacksCount = packs.filter((p) => p.installed).length;

  return (
    <div data-sfx-packs-panel className="flex flex-col gap-3">
      {/* Header Summary + Refresh */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-1.5">
          <Music className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
          <h3 className="text-sm font-medium text-ink">Free CC0-1.0 SFX Packs</h3>
          <Pill className="bg-tag-mint-bg px-2 py-0.5 text-xs text-tag-mint-fg">CC0-1.0</Pill>
          {!unavailable && (
            <>
              <Pill className="bg-board px-2 py-0.5 text-xs tabular-nums text-ink-2">
                {installedPacksCount}/{packs.length} packs
              </Pill>
              <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs tabular-nums text-pill-active-fg">
                {installedClipsTotal} clips ready
              </Pill>
            </>
          )}
        </div>
        <Button
          variant="outline"
          size="xs"
          disabled={loading}
          aria-label="Refresh CC0 SFX packs"
          onClick={() => void refreshSfxPacks()}
          className="bg-surface max-md:min-h-11"
        >
          <RefreshCw className={`size-3 ${loading ? "motion-safe:animate-spin" : ""}`} strokeWidth={1.5} aria-hidden />
          <span>{loading ? "Refreshing…" : "Refresh"}</span>
        </Button>
      </div>

      {unavailable && (
        <div
          data-sfx-unavailable
          className="rounded-card border border-hairline bg-board/45 p-3.5 text-xs leading-relaxed text-ink-2"
        >
          CC0 SFX Pack library requires the desktop OpenRO server (<code className="font-mono">openro-rs</code> v1.33.0+). You can still review local and cloud audio tiers below or configure custom adapters.
        </div>
      )}

      {loadError && (
        <div
          data-sfx-error
          className="flex flex-wrap items-center justify-between gap-2 rounded-card border border-hairline bg-pill-warn-bg/50 p-3 text-xs text-pill-warn-fg"
        >
          <span>{loadError}</span>
          <Button variant="outline" size="xs" onClick={() => void refreshSfxPacks()} className="bg-surface max-md:min-h-11">
            Retry
          </Button>
        </div>
      )}

      {packActionError && (
        <div
          data-sfx-action-error
          className="rounded-sm border border-hairline bg-pill-warn-bg/40 px-3 py-2 text-xs text-pill-warn-fg"
        >
          {packActionError}
        </div>
      )}

      {/* Search Bar Across Installed CC0 Clips */}
      {!unavailable && (
        <section
          data-sfx-search-section
          className="flex flex-col gap-2.5 rounded-card border border-hairline bg-board/45 p-3.5"
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-1.5">
              <Search className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
              <h4 className="text-xs font-medium text-ink">Search Installed CC0 Clips</h4>
            </div>
            <Pill className="bg-surface px-2 py-0.5 text-xs tabular-nums text-ink-2">
              {clips.length} match{clips.length === 1 ? "" : "es"}
            </Pill>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <Input
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void runSearch(searchQuery);
                }
              }}
              aria-label="Search CC0 SFX clips"
              placeholder="Search installed clips (click, sword, fanfare, coin, magic...)"
              className="h-9 min-w-[200px] flex-1 bg-surface text-xs max-md:min-h-11"
            />
            <Button
              variant="default"
              size="sm"
              disabled={searchBusy}
              aria-label="Search CC0 clips"
              onClick={() => void runSearch(searchQuery)}
              className="max-md:min-h-11"
            >
              <Search className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>{searchBusy ? "Searching…" : "Search"}</span>
            </Button>
          </div>

          {installedClipsTotal === 0 && clips.length === 0 ? (
            <p className="text-xs text-ink-2">
              Download any CC0-1.0 pack below to preview audio clips and convert them to 44.1 kHz WAV in your project.
            </p>
          ) : clips.length === 0 ? (
            <p className="text-xs text-ink-2">No installed CC0 clips matched your query.</p>
          ) : (
            <div data-sfx-clip-list className="flex flex-col gap-2">
              {clips.slice(0, 20).map((clip) => {
                const clipKey = `${clip.pack_id}:${clip.rel_path}`;
                const previewSrc = buildPreviewUrl(clip);
                const busyUse = useBusyKey === clipKey;
                return (
                  <div
                    key={clipKey}
                    data-sfx-clip-row={clip.rel_path}
                    className="flex flex-col gap-2 rounded-sm border border-hairline bg-surface p-2.5"
                  >
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                        <span className="truncate font-mono text-xs font-medium text-ink">
                          {clip.filename}
                        </span>
                        <Badge variant="outline" className="px-1.5 py-0.5 text-xs">
                          {clip.pack_name}
                        </Badge>
                        {typeof clip.bytes === "number" && clip.bytes > 0 && (
                          <span className="text-xs tabular-nums text-ink-3">
                            {Math.max(1, Math.round(clip.bytes / 1024))} KB
                          </span>
                        )}
                      </div>
                    </div>

                    <audio controls preload="none" src={previewSrc} className="h-9 w-full" />

                    <div className="flex flex-wrap items-center gap-2">
                      <Input
                        value={outNames[clipKey] ?? ""}
                        onChange={(e) =>
                          setOutNames((prev) => ({ ...prev, [clipKey]: e.target.value }))
                        }
                        aria-label={`Output name for ${clip.filename}`}
                        placeholder={defaultClipOutName(clip.filename)}
                        className="h-8 min-w-[160px] flex-1 font-mono text-xs max-md:min-h-11"
                      />
                      <Button
                        variant="default"
                        size="xs"
                        disabled={busyUse}
                        data-sfx-use={clip.rel_path}
                        onClick={() => void handleUseClip(clip)}
                        className="max-md:min-h-11"
                      >
                        <Plus className="size-3" strokeWidth={1.5} aria-hidden />
                        <span>{busyUse ? "Adding…" : "Add to Project"}</span>
                      </Button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {useResult && (
            <div
              data-sfx-use-result
              className="flex flex-col gap-1 rounded-sm border border-hairline bg-surface p-2.5 text-xs"
            >
              <div className="flex flex-wrap items-center justify-between gap-1.5">
                <span className="font-mono font-medium text-ink">
                  {useResult.project_path || useResult.path || useResult.filename}
                </span>
                <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs tabular-nums text-pill-active-fg">
                  {useResult.sample_rate ?? 44100} Hz PCM16 WAV
                </Pill>
              </div>
              <span className="text-ink-2">
                Attribution: {useResult.attribution || "None required (CC0 1.0 Public Domain)"}
              </span>
            </div>
          )}
        </section>
      )}

      {/* 7 Free CC0-1.0 Packs Catalog */}
      {packs.length > 0 && (
        <div data-sfx-pack-list className="flex flex-col gap-2.5">
          {packs.map((pack) => {
            const busy = busyPackId === pack.id;
            const clipCount = pack.installed ? (pack.installed_clips ?? pack.clip_count) : pack.clip_count;
            return (
              <article
                key={pack.id}
                data-sfx-pack={pack.id}
                className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3.5"
              >
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div className="flex min-w-0 flex-1 flex-col gap-1">
                    <div className="flex flex-wrap items-center gap-1.5">
                      <h4 className="text-sm font-medium text-ink">{pack.name}</h4>
                      <Pill className="bg-tag-mint-bg px-2 py-0.5 text-xs text-tag-mint-fg">
                        {pack.license || "CC0-1.0"}
                      </Pill>
                      <span className="text-xs text-ink-2">by {pack.author}</span>
                      <Pill className="bg-surface px-2 py-0.5 text-xs tabular-nums text-ink-2">
                        {clipCount} clips
                      </Pill>
                      {pack.installed && (
                        <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg">
                          <PillIndicator variant="success" />
                          Installed
                        </Pill>
                      )}
                    </div>
                    <p className="text-xs leading-relaxed text-ink-2">{pack.description}</p>
                  </div>

                  <div className="flex shrink-0 items-center gap-1.5">
                    {pack.installed ? (
                      <Button
                        variant="outline"
                        size="xs"
                        disabled={busy}
                        data-sfx-remove={pack.id}
                        aria-label={`Remove ${pack.name}`}
                        onClick={() => void handleRemovePack(pack)}
                        className="bg-surface max-md:min-h-11"
                      >
                        <Trash2 className="size-3" strokeWidth={1.5} aria-hidden />
                        <span>{busy ? "Removing…" : "Remove"}</span>
                      </Button>
                    ) : (
                      <Button
                        variant="default"
                        size="xs"
                        disabled={busy}
                        data-sfx-install={pack.id}
                        aria-label={`Download Pack ${pack.name}`}
                        onClick={() => void handleInstallPack(pack)}
                        className="max-md:min-h-11"
                      >
                        <Download className="size-3" strokeWidth={1.5} aria-hidden />
                        <span>{busy ? "Downloading…" : "Download Pack"}</span>
                      </Button>
                    )}
                  </div>
                </div>
              </article>
            );
          })}
        </div>
      )}

      {/* Local & Cloud Audio Models Quick-Setup Card */}
      <section
        data-audio-models-card
        className="flex flex-col gap-2.5 rounded-card border border-hairline bg-board/45 p-3.5"
      >
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <div className="flex flex-wrap items-center gap-1.5">
              <Cpu className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
              <h4 className="text-sm font-medium text-ink">Local &amp; Cloud Audio Models</h4>
              {runnerInstalled && (
                <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg">
                  <PillIndicator variant="success" />
                  Runner Ready
                </Pill>
              )}
            </div>
            <p className="text-xs leading-relaxed text-ink-2">
              Install the unified local neural audio runner or pick a hardware-tiered local CLI, local HTTP, or cloud audio preset in Adapters.
            </p>
          </div>

          <div className="flex shrink-0 flex-wrap items-center gap-1.5">
            <Button
              variant="default"
              size="xs"
              disabled={runnerBusy}
              data-install-audio-runner
              onClick={() => void handleInstallAudioRunner()}
              className="max-md:min-h-11"
            >
              <Download className="size-3" strokeWidth={1.5} aria-hidden />
              <span>{runnerBusy ? "Installing…" : "Install Local Audio Runner"}</span>
            </Button>
            <Button
              variant="outline"
              size="xs"
              data-open-audio-adapters
              onClick={onOpenAudioAdapters}
              className="bg-surface max-md:min-h-11"
            >
              <Plug className="size-3" strokeWidth={1.5} aria-hidden />
              <span>Open Audio Adapters</span>
            </Button>
          </div>
        </div>

        <div className="grid gap-1.5 sm:grid-cols-2">
          {AUDIO_MODEL_TIERS.map((item) => (
            <div
              key={item.tier}
              className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-surface px-2.5 py-2 text-xs"
            >
              <Badge variant="outline" className="text-xs tabular-nums">
                {item.tier}
              </Badge>
              <span className="text-ink-2">{item.detail}</span>
            </div>
          ))}
        </div>

        {(runnerResult || runnerPath || runnerNote) && (
          <div
            data-audio-runner-result
            className="flex flex-col gap-1 rounded-sm border border-hairline bg-surface p-2.5 text-xs text-ink-2"
          >
            {runnerPath && (
              <span>
                Runner binary: <code className="font-mono text-ink">{runnerPath}</code>
              </span>
            )}
            {runnerResult?.install_hint && <span>{runnerResult.install_hint}</span>}
            {runnerNote && <span>{runnerNote}</span>}
          </div>
        )}
      </section>
    </div>
  );
}

export function ToolLibraryDialog({
  open,
  onOpenChange,
  snapshot,
  marketTaste,
  marketBusy,
  v090Ready,
  projectId,
  onRefresh,
  onSelectMarketNiche,
  onRefreshMarketTaste,
  onPitchMarketPrompt,
  onSfxAdded,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  snapshot: LibrarySnapshot;
  marketTaste: MarketTasteSnapshot;
  marketBusy: boolean;
  v090Ready: boolean;
  projectId?: string | null;
  onRefresh: () => Promise<void>;
  onSelectMarketNiche: (nicheId: string) => Promise<void>;
  onRefreshMarketTaste: () => Promise<void>;
  onPitchMarketPrompt: (prompt: string, nicheName: string) => void;
  onSfxAdded?: () => void;
}) {
  const [tab, setTab] = useState<LibraryTab>("installed");
  const [adaptersInitialCap, setAdaptersInitialCap] = useState<string | undefined>(undefined);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [blenderPreset, setBlenderPreset] = useState<string>("torii_gate");
  const [blenderBusy, setBlenderBusy] = useState(false);
  const [blenderRes, setBlenderRes] = useState<BlenderGenResult | null>(null);
  const [audioPreset, setAudioPreset] = useState<string>("mythic_fanfare");
  const [audioBusy, setAudioBusy] = useState(false);
  const [audioRes, setAudioRes] = useState<AudioGenResult | null>(null);
  const [researchQuery, setResearchQuery] = useState("");
  const [researchBusy, setResearchBusy] = useState(false);
  const [researchRes, setResearchRes] = useState<MarketTasteResearchResult | null>(null);

  const defaultPacks = snapshot.packs.filter((p) => p.default_installed);
  const installablePacks = snapshot.packs.filter((p) => !p.default_installed);
  const installedCount = snapshot.packs.filter((p) => p.installed).length;
  const activeNiche =
    marketTaste.niches.find((n) => n.id === marketTaste.active_niche) || marketTaste.niches[0];

  const handleResearchGame = async (overrideQuery?: string) => {
    const q = (overrideQuery ?? researchQuery).trim();
    if (!q) {
      toast.info("Enter a Roblox game URL, Place ID, or Universe ID first.");
      return;
    }
    if (overrideQuery !== undefined) setResearchQuery(overrideQuery);
    if (!v090Ready || researchBusy) return;
    setResearchBusy(true);
    try {
      const raw = await post<Record<string, unknown>>("/api/market-taste/research", {
        place_or_universe_id: q,
        url_or_id: q,
        query: q,
        niche_hint: activeNiche?.id || "top-trending",
      });
      const norm = normalizeResearchResult(raw, activeNiche?.id || "top-trending");
      setResearchRes(norm);
      if (norm.ok) {
        toast.success(`Synthesized 8-dimension Taste Dossier for ${norm.name}`);
      } else {
        toast.error(norm.error || "Could not research Roblox game.");
      }
    } catch {
      toast.error("Could not reach /api/market-taste/research.");
    } finally {
      setResearchBusy(false);
    }
  };

  const handleInstall = async (id: string) => {
    if (!v090Ready) {
      toast.error("Tool Library install requires OpenRO v0.9.0.");
      return;
    }
    setBusyId(id);
    try {
      const res = await post<{ ok?: boolean; error?: string }>("/api/library/install", { id });
      if (res.ok) {
        toast.success(`Installed ${id}: new agent tools unlocked`);
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
      if (res.ok) toast.success(`Synthesized ${res.filename || `${audioPreset}.wav`}`);
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
                <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs tabular-nums text-pill-active-fg">
                  {installedCount}/{snapshot.packs.length} active
                </Pill>
                <Pill className="bg-board px-2 py-0.5 text-xs tabular-nums text-ink-2">
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
              Manage built-in Roblox Studio, Blender 3D, Live Roblox Market Taste research, procedural audio SFX, and installable media packs for OpenRO agents.
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
                  data-checked={tab === "sfx-packs" ? "" : undefined}
                  aria-pressed={tab === "sfx-packs"}
                  onClick={() => setTab("sfx-packs")}
                  className="max-md:!h-11 max-md:px-2.5"
                >
                  Free CC0 SFX
                </SegmentedControlItem>
                <SegmentedControlItem
                  data-checked={tab === "adapters" ? "" : undefined}
                  aria-pressed={tab === "adapters"}
                  onClick={() => {
                    setAdaptersInitialCap(undefined);
                    setTab("adapters");
                  }}
                  className="max-md:!h-11 max-md:px-2.5"
                >
                  Adapters
                </SegmentedControlItem>
                <SegmentedControlItem
                  data-checked={tab === "market" ? "" : undefined}
                  aria-pressed={tab === "market"}
                  onClick={() => setTab("market")}
                  className="max-md:!h-11 max-md:px-2.5"
                >
                  Market Taste ({marketTaste.niches.length})
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

              {tab === "sfx-packs" && (
                <SfxPacksTab
                  open={open}
                  projectId={projectId}
                  onOpenAudioAdapters={() => {
                    setAdaptersInitialCap("audio_gen");
                    setTab("adapters");
                  }}
                  onSfxAdded={onSfxAdded}
                />
              )}

              {tab === "adapters" && <AdaptersPanel initialCap={adaptersInitialCap} />}

              {tab === "market" && activeNiche && (
                <div data-market-taste-panel className="flex flex-col gap-3">
                  {/* Niche Selector Pills + Live Refresh & Pitch Actions */}
                  <section className="flex flex-col gap-2.5 rounded-card border border-hairline bg-board/45 p-3.5">
                    <div className="flex flex-wrap items-start justify-between gap-2">
                      <div className="flex min-w-0 flex-1 flex-col gap-1">
                        <div className="flex flex-wrap items-center gap-1.5">
                          <TrendingUp className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                          <h3 className="text-sm font-medium text-ink">Top-Trending Roblox Craft &amp; Taste Bar</h3>
                          <Pill className="bg-pill-active-bg px-2 py-0.5 font-mono text-xs text-pill-active-fg">
                            {marketTaste.source.includes("live") ? "Live Roblox Charts" : "Nov 2026 Baseline"}
                          </Pill>
                          <Pill className="bg-surface px-2 py-0.5 text-xs tabular-nums text-ink-2">
                            Updated {marketTaste.updated_at}
                          </Pill>
                        </div>
                        <p className="text-xs leading-relaxed text-ink-2">
                          {activeNiche.craft_bar}
                        </p>
                      </div>
                      <div className="flex shrink-0 flex-wrap items-center gap-1.5">
                        <Button
                          variant="outline"
                          size="xs"
                          disabled={marketBusy}
                          aria-label="Refresh Live Roblox Charts"
                          onClick={() => void onRefreshMarketTaste()}
                          className="bg-surface max-md:min-h-11"
                        >
                          <RefreshCw className={`size-3 ${marketBusy ? "animate-spin" : ""}`} strokeWidth={1.5} aria-hidden />
                          {marketBusy ? "Refreshing…" : "Refresh Live Roblox Charts"}
                        </Button>
                        <Button
                          variant="default"
                          size="xs"
                          disabled={marketBusy}
                          aria-label="Pitch Profitable Game"
                          onClick={() => onPitchMarketPrompt(activeNiche.pitch_prompt, activeNiche.name)}
                          className="max-md:min-h-11"
                        >
                          <Send className="size-3" strokeWidth={1.5} aria-hidden />
                          Pitch Profitable Game
                        </Button>
                      </div>
                    </div>

                    <div data-market-niche-pills className="flex flex-wrap gap-1.5">
                      {marketTaste.niches.map((niche) => {
                        const selected = niche.id === activeNiche.id;
                        return (
                          <Button
                            key={niche.id}
                            data-market-niche={niche.id}
                            variant={selected ? "default" : "outline"}
                            size="xs"
                            disabled={marketBusy}
                            aria-pressed={selected}
                            onClick={() => void onSelectMarketNiche(niche.id)}
                            className={selected ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
                          >
                            {niche.name}
                          </Button>
                        );
                      })}
                    </div>

                    {marketTaste.note && (
                      <p className="text-xs text-ink-2">{marketTaste.note}</p>
                    )}
                  </section>

                  {/* Research Roblox Game URL / ID Bar */}
                  <section
                    data-market-research-bar
                    className="flex flex-col gap-2.5 rounded-card border border-hairline bg-board/45 p-3.5"
                  >
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div className="flex items-center gap-1.5">
                        <Search className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                        <h4 className="text-xs font-medium text-ink">Research Roblox Game URL / ID</h4>
                        <Badge className="border-transparent bg-tag-design-bg px-2 py-0.5 text-xs text-tag-design-fg">
                          8-Dimension Dossier
                        </Badge>
                      </div>
                      <Button
                        variant="ghost"
                        size="xs"
                        disabled={researchBusy}
                        aria-label="Try sample Roblox Universe 10092619788"
                        onClick={() => void handleResearchGame("10092619788")}
                        className="font-mono text-xs text-ink-2 hover:text-ink max-md:min-h-11"
                      >
                        Try 10092619788
                      </Button>
                    </div>
                    <div className="flex flex-wrap items-center gap-2">
                      <Input
                        value={researchQuery}
                        onChange={(e) => setResearchQuery(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") {
                            e.preventDefault();
                            void handleResearchGame();
                          }
                        }}
                        aria-label="Roblox Game URL or ID"
                        placeholder="Paste https://www.roblox.com/games/... or Universe / Place ID"
                        className="h-9 min-w-[220px] flex-1 bg-surface font-mono text-xs max-md:min-h-11"
                      />
                      <Button
                        variant="default"
                        size="sm"
                        disabled={researchBusy}
                        aria-label="Research Roblox Game"
                        onClick={() => void handleResearchGame()}
                        className="max-md:min-h-11"
                      >
                        <Wand2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                        {researchBusy ? "Researching…" : "Research Game"}
                      </Button>
                    </div>

                    {researchRes && researchRes.ok && (
                      <div
                        data-market-research-result
                        className="flex flex-col gap-2.5 rounded-sm border border-hairline bg-surface p-3"
                      >
                        <div className="flex flex-wrap items-start justify-between gap-2">
                          <div className="flex min-w-0 flex-1 flex-col gap-1">
                            <div className="flex flex-wrap items-center gap-1.5">
                              <span className="text-sm font-medium text-ink">{researchRes.name}</span>
                              <Badge className="border-transparent bg-board px-1.5 py-0.5 text-xs text-ink-2">
                                {researchRes.niche_name}
                              </Badge>
                              {researchRes.creator && (
                                <span className="text-xs text-ink-2">by {researchRes.creator}</span>
                              )}
                            </div>
                            <div className="flex flex-wrap items-center gap-1.5">
                              <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs tabular-nums text-pill-active-fg">
                                {formatCcu(researchRes.ccu)} CCU
                              </Pill>
                              {researchRes.visits > 0 && (
                                <Pill className="bg-board px-2 py-0.5 text-xs tabular-nums text-ink-2">
                                  {formatCompactNumber(researchRes.visits)} visits
                                </Pill>
                              )}
                              <Pill className="bg-tag-mint-bg px-2 py-0.5 text-xs tabular-nums text-tag-mint-fg">
                                {researchRes.like_ratio.toFixed(1)}% likes
                              </Pill>
                              {researchRes.universe_id > 0 && (
                                <Pill className="bg-board px-2 py-0.5 text-xs tabular-nums text-ink-2">
                                  Universe {researchRes.universe_id}
                                </Pill>
                              )}
                            </div>
                          </div>
                          <Button
                            variant="default"
                            size="xs"
                            aria-label="Pitch With These Twists"
                            onClick={() => onPitchMarketPrompt(researchRes.pitch_prompt, researchRes.name)}
                            className="max-md:min-h-11"
                          >
                            <Send className="size-3" strokeWidth={1.5} aria-hidden />
                            Pitch With These Twists
                          </Button>
                        </div>

                        {researchRes.thumbnails.length > 0 && (
                          <div data-research-thumbnails className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                            {researchRes.thumbnails.slice(0, 4).map((src, idx) => (
                              <img
                                key={`${src.slice(0, 48)}-${idx}`}
                                src={src}
                                alt={`${researchRes.name} reference ${idx + 1}`}
                                className="aspect-video w-full rounded-sm border border-hairline bg-board object-cover"
                              />
                            ))}
                          </div>
                        )}

                        <div className="grid gap-2 md:grid-cols-2">
                          <div className="flex flex-col gap-1 rounded-sm border border-hairline bg-board/40 p-2.5">
                            <span className="text-xs font-medium text-ink">Generated Twists (&ldquo;Similar But Different&rdquo;)</span>
                            <ul className="flex list-disc flex-col gap-1 pl-4 text-xs text-ink-2">
                              {researchRes.automatic_twists.map((t) => (
                                <li key={t}>{t}</li>
                              ))}
                            </ul>
                          </div>
                          <div className="flex flex-col gap-1 rounded-sm border border-hairline bg-board/40 p-2.5">
                            <span className="text-xs font-medium text-ink">Quality Boosts</span>
                            <ul className="flex list-disc flex-col gap-1 pl-4 text-xs text-ink-2">
                              {researchRes.quality_boosts.map((q) => (
                                <li key={q}>{q}</li>
                              ))}
                            </ul>
                          </div>
                        </div>

                        {researchRes.note && <p className="text-xs text-ink-2">{researchRes.note}</p>}
                        {researchRes.dossier_path && (
                          <p className="text-xs text-ink-2">
                            Saved dossier: <code className="font-mono">{researchRes.dossier_path}</code>
                          </p>
                        )}
                      </div>
                    )}
                  </section>

                  {/* Dimension 1: Live CCU & Like-Ratio Leaderboard for Selected Niche */}
                  <section
                    data-market-leaderboard
                    className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3.5"
                  >
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div className="flex flex-col gap-0.5">
                        <h4 className="text-sm font-medium text-ink">
                          1. Market Leaders: {activeNiche.name}
                        </h4>
                        <p className="text-xs text-ink-2">{activeNiche.tagline}</p>
                      </div>
                      <Badge className="border-transparent bg-tag-sky-bg px-2 py-0.5 text-xs tabular-nums text-tag-sky-fg">
                        {activeNiche.games.length} chart leaders
                      </Badge>
                    </div>

                    <div className="flex flex-col gap-1.5">
                      {activeNiche.games.map((game, idx) => (
                        <div
                          key={`${activeNiche.id}-${game.name}`}
                          data-market-game-row
                          className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-surface px-3 py-2"
                        >
                          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                            <div className="flex flex-wrap items-center gap-1.5">
                              <span className="font-mono text-xs tabular-nums text-ink-2">#{idx + 1}</span>
                              <span className="text-sm font-medium text-ink">{game.name}</span>
                              <Badge className="border-transparent bg-board px-1.5 py-0.5 font-mono text-xs text-ink-2">
                                {game.sort}
                              </Badge>
                            </div>
                            {game.hook && <p className="text-xs text-ink-2">{game.hook}</p>}
                          </div>
                          <div className="flex shrink-0 items-center gap-1.5">
                            <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs tabular-nums text-pill-active-fg">
                              {formatCcu(game.ccu)} CCU
                            </Pill>
                            <Pill className="bg-tag-mint-bg px-2 py-0.5 text-xs tabular-nums text-tag-mint-fg">
                              {game.like_ratio.toFixed(1)}% likes
                            </Pill>
                          </div>
                        </div>
                      ))}
                    </div>
                  </section>

                  {/* Dimensions 2-7: Production Craft Taste Dossier */}
                  <div data-market-dossier-dimensions className="grid gap-3 md:grid-cols-2">
                    <section className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3">
                      <div className="flex items-center gap-1.5">
                        <Gamepad2 className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                        <h4 className="text-xs font-medium text-ink">2. Gameplay Loop</h4>
                      </div>
                      <ul className="flex list-disc flex-col gap-1.5 pl-4 text-xs leading-relaxed text-ink-2">
                        {activeNiche.gameplay_loop.map((item) => (
                          <li key={item}>{item}</li>
                        ))}
                      </ul>
                    </section>

                    <section className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3">
                      <div className="flex items-center gap-1.5">
                        <Layers className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                        <h4 className="text-xs font-medium text-ink">3. UI / UX Style</h4>
                      </div>
                      <ul className="flex list-disc flex-col gap-1.5 pl-4 text-xs leading-relaxed text-ink-2">
                        {activeNiche.ui_ux_style.map((item) => (
                          <li key={item}>{item}</li>
                        ))}
                      </ul>
                    </section>

                    <section className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3">
                      <div className="flex items-center gap-1.5">
                        <Sparkles className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                        <h4 className="text-xs font-medium text-ink">4. Animations &amp; Juice</h4>
                      </div>
                      <ul className="flex list-disc flex-col gap-1.5 pl-4 text-xs leading-relaxed text-ink-2">
                        {activeNiche.animations_and_juice.map((item) => (
                          <li key={item}>{item}</li>
                        ))}
                      </ul>
                    </section>

                    <section className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3">
                      <div className="flex items-center gap-1.5">
                        <Box className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                        <h4 className="text-xs font-medium text-ink">5. 3D World &amp; Model Style</h4>
                      </div>
                      <ul className="flex list-disc flex-col gap-1.5 pl-4 text-xs leading-relaxed text-ink-2">
                        {activeNiche.model_and_world_3d_style.map((item) => (
                          <li key={item}>{item}</li>
                        ))}
                      </ul>
                    </section>

                    <section className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3">
                      <div className="flex items-center gap-1.5">
                        <Music className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                        <h4 className="text-xs font-medium text-ink">6. Audio Cadence</h4>
                      </div>
                      <ul className="flex list-disc flex-col gap-1.5 pl-4 text-xs leading-relaxed text-ink-2">
                        {activeNiche.audio_cadence.map((item) => (
                          <li key={item}>{item}</li>
                        ))}
                      </ul>
                    </section>

                    <section className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3">
                      <div className="flex items-center gap-1.5">
                        <ShieldCheck className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                        <h4 className="text-xs font-medium text-ink">Anti-Slop Craft Guardrails</h4>
                      </div>
                      <ul className="flex list-disc flex-col gap-1.5 pl-4 text-xs leading-relaxed text-ink-2">
                        {activeNiche.anti_slop_rules.map((item) => (
                          <li key={item}>{item}</li>
                        ))}
                      </ul>
                    </section>
                  </div>

                  {/* Dimensions 7 & 8: Monetization, Retention, Automatic Twists & Quality Boosts */}
                  <div data-market-twists-section className="grid gap-3 md:grid-cols-2">
                    <section className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3">
                      <div className="flex items-center gap-1.5">
                        <DollarSign className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                        <h4 className="text-xs font-medium text-ink">7. Monetization &amp; D1/D7 Retention</h4>
                      </div>
                      <ul className="flex list-disc flex-col gap-1.5 pl-4 text-xs leading-relaxed text-ink-2">
                        {activeNiche.monetization_loops.map((item) => (
                          <li key={item}>{item}</li>
                        ))}
                        {activeNiche.retention_hooks.map((item) => (
                          <li key={item}>{item}</li>
                        ))}
                      </ul>
                    </section>

                    <section className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3">
                      <div className="flex items-center justify-between gap-1.5">
                        <div className="flex items-center gap-1.5">
                          <Flame className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                          <h4 className="text-xs font-medium text-ink">8. Automatic Twists &amp; Quality Boosts</h4>
                        </div>
                        <Badge className="border-transparent bg-tag-plan-bg px-1.5 py-0.5 text-xs text-tag-plan-fg">
                          Similar But Different
                        </Badge>
                      </div>
                      <ul className="flex list-disc flex-col gap-1.5 pl-4 text-xs leading-relaxed text-ink-2">
                        {activeNiche.automatic_twists.map((item) => (
                          <li key={item}>{item}</li>
                        ))}
                      </ul>
                      <div className="border-t border-hairline pt-2">
                        <span className="text-xs font-medium text-ink">UX &amp; Craft Quality Boosts</span>
                        <ul className="mt-1 flex list-disc flex-col gap-1 pl-4 text-xs leading-relaxed text-ink-2">
                          {activeNiche.quality_boosts.map((item) => (
                            <li key={item}>{item}</li>
                          ))}
                        </ul>
                      </div>
                    </section>
                  </div>
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
                          <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs tabular-nums text-pill-active-fg">
                            {blenderRes.triangles ?? 0} tris, {Math.max(1, Math.round((blenderRes.bytes ?? 0) / 1024))} KB
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
                          <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs tabular-nums text-pill-active-fg">
                            {audioRes.duration_ms ?? 450}ms, {Math.max(1, Math.round((audioRes.bytes ?? 0) / 1024))} KB
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
