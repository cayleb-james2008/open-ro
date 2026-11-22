import { useMemo, useState } from "react";
import { toast } from "sonner";
import {
  Archive,
  AtSign,
  Box,
  Check,
  ChevronDown,
  Copy,
  FileCode,
  FileDiff,
  FolderTree,
  History,
  Layers,
  Monitor,
  RefreshCw,
  Search,
  ShieldCheck,
  User,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { Pill, PillIndicator, PillStatus } from "@/components/kibo-ui/pill";
import type { Place, PlaceDiff, PlaceInstance, WorkspaceMode } from "./data";
import { analyzePlaceLuau, type LuauDiagnostic } from "./luauAudit";

type KindInfo = { label: string; service: string; Icon: typeof FileCode; cls: string };

const SERVICE_ORDER = [
  "Workspace",
  "ServerScriptService",
  "ReplicatedStorage",
  "StarterGui",
  "StarterPlayer",
  "ServerStorage",
  "Lighting",
  "SoundService",
] as const;

const KINDS: Record<string, KindInfo> = {
  Workspace: { label: "part", service: "Workspace", Icon: Box, cls: "bg-tag-mint-bg text-tag-mint-fg" },
  ServerScriptService: { label: "server", service: "ServerScriptService", Icon: FileCode, cls: "bg-tag-sky-bg text-tag-sky-fg" },
  ReplicatedStorage: { label: "shared", service: "ReplicatedStorage", Icon: Layers, cls: "bg-tag-ops-bg text-tag-ops-fg" },
  StarterGui: { label: "gui", service: "StarterGui", Icon: Monitor, cls: "bg-tag-design-bg text-tag-design-fg" },
  StarterPlayer: { label: "player", service: "StarterPlayer", Icon: User, cls: "bg-tag-research-bg text-tag-research-fg" },
  ServerStorage: { label: "storage", service: "ServerStorage", Icon: Archive, cls: "bg-tag-neutral-bg text-tag-neutral-fg" },
  Lighting: { label: "lighting", service: "Lighting", Icon: Layers, cls: "bg-tag-dev-bg text-tag-dev-fg" },
  SoundService: { label: "audio", service: "SoundService", Icon: Layers, cls: "bg-tag-research-bg text-tag-research-fg" },
};

const FALLBACK_KIND: KindInfo = { label: "item", service: "Workspace", Icon: Box, cls: "bg-board text-ink-2" };

export type ExplorerEntry = {
  path: string;
  name: string;
  service: string;
  kind: KindInfo;
  metaChip?: string;
  isScript: boolean;
  instance?: PlaceInstance;
  diagnostics: LuauDiagnostic[];
};

function parsePath(path: string, isScript = false): { name: string; service: string; kind: KindInfo } {
  const segs = path.split(".");
  const service = segs[0] || "Workspace";
  const rawLast = segs[segs.length - 1] || path;
  const kind = KINDS[service] ?? FALLBACK_KIND;
  if (isScript && service === "Workspace") {
    return { name: rawLast, service, kind: { ...kind, Icon: FileCode, label: "script" } };
  }
  return { name: rawLast, service, kind };
}

export function buildGameSummary(gameName: string, builtTitles: string[], place: Place | null | "error"): string {
  const scripts = place && place !== "error" ? Object.keys(place.scripts || {}) : [];
  const parts = place && place !== "error" ? Object.keys(place.instances || {}) : [];
  const total = scripts.length + parts.length;
  if (!builtTitles.length && total === 0) return "";
  const titlePart = builtTitles.length ? builtTitles.join(", ") : (gameName || "Custom Studio place");
  return `My Roblox game has ${titlePart}, ${total} ${total === 1 ? "piece" : "pieces"} in all (${parts.length} Workspace ${parts.length === 1 ? "part" : "parts"}, ${scripts.length} Luau ${scripts.length === 1 ? "script" : "scripts"}), built and playtested with OpenRO.`;
}

export function LeftExplorerTree({
  gameName,
  builtTitles,
  place,
  error,
  selectedPath,
  selectedScriptPath,
  contextPaths = [],
  studioSelection = [],
  buildDiff,
  serverDiagnostics,
  snapshotCount = 0,
  syncing = false,
  onSelectPart,
  onOpenScript,
  onAttachContext,
  onRefreshPlace,
  onOpenSnapshots,
}: {
  gameName: string;
  builtTitles: string[];
  place: Place | null | "error";
  error: boolean;
  selectedPath?: string | null;
  selectedScriptPath?: string | null;
  contextPaths?: string[];
  studioSelection?: string[];
  buildDiff?: PlaceDiff | null;
  serverDiagnostics?: LuauDiagnostic[];
  snapshotCount?: number;
  syncing?: boolean;
  onSelectPart: (path: string) => void;
  onOpenScript: (path: string) => void;
  onAttachContext: (path: string) => void;
  onRefreshPlace?: () => void;
  onOpenSnapshots?: () => void;
}) {
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();

  const diagnostics = useMemo(() => {
    if (!place || place === "error") return [] as LuauDiagnostic[];
    const local = analyzePlaceLuau(place);
    if (!serverDiagnostics || serverDiagnostics.length === 0) return local;
    const seen = new Set(local.map((d) => `${d.path}:${d.line}:${d.code}`));
    const merged = [...local];
    for (const sd of serverDiagnostics) {
      const k = `${sd.path}:${sd.line}:${sd.code}`;
      if (!seen.has(k)) merged.push(sd);
    }
    return merged;
  }, [place, serverDiagnostics]);

  const diagByPath = useMemo(() => {
    const m = new Map<string, LuauDiagnostic[]>();
    for (const d of diagnostics) {
      const list = m.get(d.path) || [];
      list.push(d);
      m.set(d.path, list);
    }
    return m;
  }, [diagnostics]);

  const { groups, total, scriptCount, errCount, warnCount } = useMemo(() => {
    if (!place || place === "error") {
      return {
        groups: [] as Array<{ service: string; entries: ExplorerEntry[] }>,
        total: 0,
        scriptCount: 0,
        errCount: 0,
        warnCount: 0,
      };
    }

    const all: ExplorerEntry[] = [];
    for (const [p, inst] of Object.entries(place.instances || {})) {
      const { name, service, kind } = parsePath(p, false);
      all.push({
        path: p,
        name,
        service,
        kind,
        metaChip: inst?.class_name || "Part",
        isScript: false,
        instance: inst,
        diagnostics: diagByPath.get(p) || [],
      });
    }
    for (const [p, src] of Object.entries(place.scripts || {})) {
      const { name, service, kind } = parsePath(p, true);
      const lineCount = src ? src.trim().split("\n").length : 0;
      all.push({
        path: p,
        name,
        service,
        kind,
        metaChip: `${Math.max(1, lineCount)}L`,
        isScript: true,
        diagnostics: diagByPath.get(p) || [],
      });
    }

    const filtered = q
      ? all.filter((e) => `${e.name} ${e.path} ${e.service} ${e.metaChip ?? ""}`.toLowerCase().includes(q))
      : all;

    const bucket = new Map<string, ExplorerEntry[]>();
    for (const entry of filtered) {
      const list = bucket.get(entry.service) || [];
      list.push(entry);
      bucket.set(entry.service, list);
    }

    const orderedServices = [
      ...SERVICE_ORDER.filter((s) => bucket.has(s)),
      ...[...bucket.keys()].filter((s) => !(SERVICE_ORDER as readonly string[]).includes(s)).sort(),
    ];

    const grouped = orderedServices.map((service) => ({
      service,
      entries: (bucket.get(service) || []).sort((a, b) => {
        if (a.isScript !== b.isScript) return a.isScript ? -1 : 1;
        return a.name.localeCompare(b.name);
      }),
    }));

    return {
      groups: grouped,
      total: all.length,
      scriptCount: Object.keys(place.scripts || {}).length,
      errCount: diagnostics.filter((d) => d.severity === "error").length,
      warnCount: diagnostics.filter((d) => d.severity === "warning").length,
    };
  }, [diagByPath, diagnostics, place, q]);

  const diffTotal = buildDiff ? buildDiff.added.length + buildDiff.modified.length + buildDiff.removed.length : 0;
  const summary = useMemo(() => buildGameSummary(gameName, builtTitles, place), [gameName, builtTitles, place]);

  const copySummary = () => {
    if (!summary) return;
    navigator.clipboard.writeText(summary).then(
      () => toast.success("Copied game summary"),
      () => toast.error("Could not copy automatically."),
    );
  };

  return (
    <section
      aria-label="Roblox DataModel Explorer"
      data-explorer-tree
      className="flex min-h-0 flex-1 flex-col gap-2"
    >
      {/* Count + Refresh + Audit Row */}
      <div className="flex items-center justify-between gap-1.5 px-0.5">
        <div className="inline-flex min-w-0 items-center gap-1.5 text-xs text-ink-2">
          <FolderTree className="size-3.5 shrink-0 text-icon" strokeWidth={1.5} aria-hidden />
          <span className="font-mono font-medium tabular-nums text-ink">
            <SlidingNumber value={total} />
          </span>
          <span className="truncate">{total === 1 ? "instance" : "instances"}</span>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          {scriptCount > 0 && (
            <Pill
              className={`px-1.5 py-0 font-mono text-xs ${
                errCount > 0
                  ? "bg-pill-pro-bg text-pill-pro-fg"
                  : warnCount > 0
                    ? "bg-pill-warn-bg text-pill-warn-fg"
                    : "bg-pill-active-bg text-pill-active-fg"
              }`}
            >
              {errCount === 0 && warnCount === 0 ? (
                <>
                  <ShieldCheck className="size-3" strokeWidth={1.5} aria-hidden />
                  <span>Clean</span>
                </>
              ) : errCount > 0 ? (
                <span>{errCount} err</span>
              ) : (
                <span>{warnCount} warn</span>
              )}
            </Pill>
          )}
          {onRefreshPlace && (
            <Button
              variant="ghost"
              size="icon-xs"
              disabled={syncing}
              onClick={onRefreshPlace}
              aria-label="Sync Explorer from Studio"
              className="size-7 text-ink-2 hover:text-ink max-md:size-11"
            >
              <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
            </Button>
          )}
        </div>
      </div>

      {/* Filter input */}
      <InputGroup className="bg-board/50">
        <InputGroupAddon align="inline-start" className="ps-2">
          <Search className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
        </InputGroupAddon>
        <InputGroupInput
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Filter Explorer"
          placeholder="Filter DataModel…"
          className="[&_input]:max-md:!min-h-11 [&_input]:md:!h-8 [&_input]:text-xs"
        />
        {query && (
          <InputGroupAddon align="inline-end" className="has-[>button]:me-0 pe-1">
            <Button
              variant="ghost"
              size="icon-xs"
              onClick={() => setQuery("")}
              aria-label="Clear Explorer filter"
              className="size-6 max-md:size-11"
            >
              <X className="size-3.5" strokeWidth={1.5} aria-hidden />
            </Button>
          </InputGroupAddon>
        )}
      </InputGroup>

      {/* Studio Live Selection Quick Attach Strip */}
      {studioSelection.length > 0 && (
        <div className="flex items-center justify-between gap-1.5 rounded-card border border-blue/35 bg-blue-soft/45 px-2 py-1.5 text-xs">
          <span className="min-w-0 truncate font-mono text-xs text-ink">
            Studio sel: {studioSelection[0].split(".").pop()}
            {studioSelection.length > 1 ? ` +${studioSelection.length - 1}` : ""}
          </span>
          <Button
            variant="ghost"
            size="xs"
            onClick={() => onAttachContext(studioSelection[0])}
            aria-label={`Attach live Studio selection ${studioSelection[0]} to Copilot`}
            className="h-6 shrink-0 px-1.5 font-mono text-xs max-md:min-h-11"
          >
            +@
          </Button>
        </div>
      )}

      {/* Expandable Latest Build Diff */}
      {buildDiff && diffTotal > 0 && (
        <div className="rounded-card border border-hairline bg-board/45 px-2 py-1.5">
          <Expandable expandDirection="vertical" expandBehavior="push">
            {({ isExpanded }) => (
              <>
                <div className="flex items-center justify-between gap-1">
                  <div className="flex min-w-0 items-center gap-1">
                    <FileDiff className="size-3.5 shrink-0 text-icon" strokeWidth={1.5} aria-hidden />
                    <span className="truncate text-xs font-medium text-ink">Diff</span>
                    {buildDiff.added.length > 0 && (
                      <span className="rounded-chip bg-pill-active-bg px-1 font-mono text-xs tabular-nums text-pill-active-fg">
                        +{buildDiff.added.length}
                      </span>
                    )}
                    {buildDiff.modified.length > 0 && (
                      <span className="rounded-chip bg-pill-warn-bg px-1 font-mono text-xs tabular-nums text-pill-warn-fg">
                        ~{buildDiff.modified.length}
                      </span>
                    )}
                    {buildDiff.removed.length > 0 && (
                      <span className="rounded-chip bg-pill-pro-bg px-1 font-mono text-xs tabular-nums text-pill-pro-fg">
                        -{buildDiff.removed.length}
                      </span>
                    )}
                  </div>
                  <ExpandableTrigger
                    aria-label={isExpanded ? "Hide build diff" : "Show build diff"}
                    className="inline-flex min-h-11 min-w-11 items-center justify-center rounded-sm text-ink-2 outline-none hover:bg-surface hover:text-ink md:min-h-6 md:min-w-6"
                  >
                    <ChevronDown
                      className={`size-3.5 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`}
                      strokeWidth={1.5}
                      aria-hidden
                    />
                  </ExpandableTrigger>
                </div>
                <ExpandableContent preset="fade">
                  <ul className="mt-1.5 flex max-h-28 flex-col gap-0.5 overflow-y-auto border-t border-hairline pt-1.5 font-mono text-xs">
                    {buildDiff.added.slice(0, 12).map((it) => (
                      <li key={`a:${it.path}`} className="truncate text-pill-active-fg">
                        + {it.path}
                      </li>
                    ))}
                    {buildDiff.modified.slice(0, 12).map((it) => (
                      <li key={`m:${it.path}`} className="truncate text-pill-warn-fg">
                        ~ {it.path}
                      </li>
                    ))}
                    {buildDiff.removed.slice(0, 12).map((it) => (
                      <li key={`r:${it.path}`} className="truncate text-pill-pro-fg">
                        - {it.path}
                      </li>
                    ))}
                  </ul>
                </ExpandableContent>
              </>
            )}
          </Expandable>
        </div>
      )}

      {/* DataModel Service Tree */}
      <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto pr-0.5">
        {error ? (
          <div className="flex flex-col items-start gap-2 rounded-card border border-hairline bg-board p-2.5 text-xs text-pill-pro-fg">
            <span>Could not read Studio DataModel.</span>
            {onRefreshPlace && (
              <Button variant="outline" size="xs" onClick={onRefreshPlace} className="bg-surface max-md:min-h-11">
                <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
                Retry sync
              </Button>
            )}
          </div>
        ) : !place ? (
          <div className="grid gap-1.5" aria-label="Loading Studio Explorer">
            <span className="h-8 rounded-sm bg-board" />
            <span className="h-8 rounded-sm bg-board" />
            <span className="h-8 rounded-sm bg-board" />
          </div>
        ) : total === 0 ? (
          <div className="flex flex-col gap-2 rounded-card border border-hairline bg-board/60 p-3 text-xs leading-relaxed text-ink-2">
            <span>No parts or scripts in this place yet.</span>
            <span>Use Starters, drop a .rbxlx file, or prompt the Copilot on the right.</span>
          </div>
        ) : groups.length === 0 ? (
          <p className="rounded-card border border-hairline bg-board/60 p-2.5 text-xs text-ink-2">
            No items match &ldquo;{query}&rdquo;.
          </p>
        ) : (
          groups.map((grp) => (
            <div key={grp.service} className="flex flex-col gap-0.5">
              <div className="flex items-center justify-between px-1 py-0.5">
                <span className="truncate font-mono text-xs font-medium text-ink-2">{grp.service}</span>
                <span className="font-mono text-xs tabular-nums text-ink-3">{grp.entries.length}</span>
              </div>

              <ul className="flex flex-col gap-0.5">
                {grp.entries.slice(0, 80).map((entry) => {
                  const isActive = entry.isScript
                    ? selectedScriptPath === entry.path || selectedPath === entry.path
                    : selectedPath === entry.path;
                  const attached = contextPaths.includes(entry.path);
                  const errN = entry.diagnostics.filter((d) => d.severity === "error").length;
                  const Icon = entry.kind.Icon;
                  return (
                    <li
                      key={entry.path}
                      data-explorer-path={entry.path}
                      className={`flex min-w-0 items-center gap-1 rounded-sm pr-0.5 transition-[background-color] duration-(--dur-press) ease-(--ease-house) ${
                        isActive ? "bg-blue-soft/75" : "hover:bg-board/65"
                      }`}
                    >
                      <Button
                        variant="ghost"
                        size="xs"
                        onClick={() => (entry.isScript ? onOpenScript(entry.path) : onSelectPart(entry.path))}
                        aria-label={`${entry.isScript ? "Open script" : "Select part"} ${entry.path}`}
                        className={`h-7 min-w-0 flex-1 justify-start gap-1.5 rounded-sm px-1.5 font-normal max-md:min-h-11 ${
                          isActive ? "font-medium text-ink" : "text-ink-2 hover:text-ink"
                        }`}
                      >
                        <Icon className="size-3.5 shrink-0 text-icon" strokeWidth={1.5} aria-hidden />
                        <span className="min-w-0 flex-1 truncate text-left text-xs">{entry.name}</span>
                        {errN > 0 ? (
                          <span className="shrink-0 rounded-chip bg-pill-pro-bg px-1 font-mono text-xs text-pill-pro-fg">
                            {errN}
                          </span>
                        ) : entry.metaChip ? (
                          <span className="shrink-0 font-mono text-xs text-ink-3">{entry.metaChip}</span>
                        ) : null}
                      </Button>

                      <Button
                        variant={attached ? "default" : "ghost"}
                        size="icon-xs"
                        aria-pressed={attached}
                        aria-label={`${attached ? "Remove" : "Attach"} ${entry.path} context`}
                        onClick={() => onAttachContext(entry.path)}
                        className={`size-6 shrink-0 max-md:size-11 ${
                          attached ? "" : "text-ink-3 hover:text-ink"
                        }`}
                      >
                        {attached ? (
                          <Check className="size-3" strokeWidth={2} aria-hidden />
                        ) : (
                          <AtSign className="size-3" strokeWidth={1.5} aria-hidden />
                        )}
                      </Button>
                    </li>
                  );
                })}
              </ul>
            </div>
          ))
        )}
      </div>

      {/* Compact Explorer Utilities */}
      <div className="flex flex-none items-center gap-1.5 border-t border-hairline pt-2">
        {onOpenSnapshots && (
          <Button
            variant="outline"
            size="xs"
            onClick={onOpenSnapshots}
            aria-label="Open version snapshots"
            className="flex-1 justify-center bg-board/50 max-md:min-h-11"
          >
            <History className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Snapshots{snapshotCount > 0 ? ` (${snapshotCount})` : ""}</span>
          </Button>
        )}
        <Button
          variant="outline"
          size="xs"
          disabled={!summary}
          onClick={copySummary}
          aria-label="Copy game summary"
          className="justify-center bg-board/50 max-md:min-h-11"
        >
          <Copy className="size-3.5" strokeWidth={1.5} aria-hidden />
          <span>Copy</span>
        </Button>
      </div>
    </section>
  );
}

export function StudioChip({
  connected,
  workspaceMode = "studio_mcp",
}: {
  connected: boolean;
  workspaceMode?: WorkspaceMode;
}) {
  const label = !connected
    ? "Offline"
    : workspaceMode === "plugin"
      ? "Plugin Live"
      : workspaceMode === "local_place"
        ? "Local Place"
        : workspaceMode === "browser_place"
          ? "Browser Place"
          : "Studio Live";
  return (
    <Pill
      className={`px-2 py-0.5 text-xs font-medium ${
        connected ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-pro-bg text-pill-pro-fg"
      }`}
    >
      <PillStatus className="border-transparent">
        <PillIndicator variant={connected ? "success" : "error"} />
        <span>{label}</span>
      </PillStatus>
    </Pill>
  );
}
