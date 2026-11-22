import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  Box,
  Command,
  Download,
  FileCode,
  FolderArchive,
  History,
  Layers,
  Monitor,
  Package,
  Palette,
  Plus,
  Redo2,
  Search,
  SlidersHorizontal,
  Sparkles,
  Undo2,
  Upload,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Kbd } from "@/components/ui/kbd";
import { Pill } from "@/components/kibo-ui/pill";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogDescription,
  MorphingDialogTitle,
} from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import type { Place } from "./data";
import { CATEGORY_TAG_CLS, GAME_SYSTEMS, isSystemInPlace, type GameSystemDef } from "./systems";

type PaletteItem =
  | { kind: "action"; id: string; title: string; subtitle: string; badge: string; badgeCls: string; onRun: () => void }
  | { kind: "system"; id: string; title: string; subtitle: string; badge: string; badgeCls: string; inPlace: boolean; sys: GameSystemDef }
  | { kind: "part"; id: string; title: string; subtitle: string; badge: string; badgeCls: string; path: string }
  | { kind: "script"; id: string; title: string; subtitle: string; badge: string; badgeCls: string; path: string };

export function CommandPalette({
  open,
  onOpenChange,
  place,
  canBuild,
  busy,
  canUndo,
  canRedo = false,
  onAddSystem,
  onSelectPath,
  onNewChat,
  onExportRbxlx,
  onDownloadShipPack,
  onAddBlockPart,
  onImportRbxlxClick,
  onOpenSnapshots,
  onOpenSettings,
  onOpenArtStudio,
  onOpenToolLibrary,
  onOpenEmbeddedStudio,
  onBuildApex,
  onRunPlaytest,
  onTriggerUndo,
  onTriggerRedo,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  place: Place | null | "error";
  canBuild: boolean;
  busy: boolean;
  canUndo: boolean;
  canRedo?: boolean;
  onAddSystem: (sys: GameSystemDef) => void;
  onSelectPath: (path: string) => void;
  onNewChat: () => void;
  onExportRbxlx: () => void;
  onDownloadShipPack?: () => void;
  onAddBlockPart?: () => void;
  onImportRbxlxClick: () => void;
  onOpenSnapshots: () => void;
  onOpenSettings: () => void;
  onOpenArtStudio: () => void;
  onOpenToolLibrary?: () => void;
  onOpenEmbeddedStudio?: () => void;
  onBuildApex: () => void;
  onRunPlaytest?: () => void;
  onTriggerUndo: () => void;
  onTriggerRedo?: () => void;
}) {
  const [query, setQuery] = useState("");
  const [activeIdx, setActiveIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIdx(0);
      const t = window.setTimeout(() => inputRef.current?.focus(), 40);
      return () => window.clearTimeout(t);
    }
  }, [open]);

  const items = useMemo<PaletteItem[]>(() => {
    const out: PaletteItem[] = [];
    const pieceCount =
      place && place !== "error"
        ? Object.keys(place.instances || {}).length + Object.keys(place.scripts || {}).length
        : 0;

    // 1. Workbench Actions
    out.push(
      {
        kind: "action",
        id: "act-build-apex",
        title: "Build Anime Card Battles: Rebirth — Apex Edition",
        subtitle: "One-click 6-system Roblox card battler with gacha packs, deck fusion & 50-floor boss tower",
        badge: "Flagship",
        badgeCls: "bg-tag-plan-bg text-tag-plan-fg",
        onRun: onBuildApex,
      },
      {
        kind: "action",
        id: "act-art-studio",
        title: "Open Card Art & Asset Studio",
        subtitle: "Browse Nano Banana card art, upload custom PNG/WebP artwork, or open the Anime Style Guide",
        badge: "Art Studio",
        badgeCls: "bg-tag-research-bg text-tag-research-fg",
        onRun: onOpenArtStudio,
      },
      {
        kind: "action",
        id: "act-new-chat",
        title: "New Studio chat",
        subtitle: "Start a clean game session with starter blueprints",
        badge: "Action",
        badgeCls: "bg-tag-neutral-bg text-tag-neutral-fg",
        onRun: onNewChat,
      },
      {
        kind: "action",
        id: "act-import",
        title: "Import .rbxlx XML",
        subtitle: "Load an existing Roblox Studio place into OpenRO",
        badge: "Action",
        badgeCls: "bg-tag-ops-bg text-tag-ops-fg",
        onRun: onImportRbxlxClick,
      },
      {
        kind: "action",
        id: "act-snapshots",
        title: "Version Snapshots Timeline",
        subtitle: "Browse, restore, or export prior build checkpoints",
        badge: "Action",
        badgeCls: "bg-tag-research-bg text-tag-research-fg",
        onRun: onOpenSnapshots,
      },
      {
        kind: "action",
        id: "act-settings",
        title: "Model, Studio & Appearance Settings",
        subtitle: "Configure local gateway, Studio MCP, or Light/Dark mode",
        badge: "Settings",
        badgeCls: "bg-tag-neutral-bg text-tag-neutral-fg",
        onRun: onOpenSettings,
      },
    );
    if (pieceCount > 0 && onRunPlaytest) {
      out.push({
        kind: "action",
        id: "act-playtest",
        title: "Run Live Studio Playtest",
        subtitle: "Execute Luau playtest and stream console output into the 3D Studio Viewport",
        badge: "Studio 3D",
        badgeCls: "bg-tag-sky-bg text-tag-sky-fg",
        onRun: onRunPlaytest,
      });
    }
    if (onOpenToolLibrary) {
      out.push({
        kind: "action",
        id: "act-tool-library",
        title: "Open Tool & Skill Library (Blender 3D, Audio SFX, Media Packs)",
        subtitle: "Install/configure agent packs, generate Blender .obj meshes, and synthesize .wav SFX",
        badge: "Library",
        badgeCls: "bg-tag-mint-bg text-tag-mint-fg",
        onRun: onOpenToolLibrary,
      });
    }
    if (onOpenEmbeddedStudio) {
      out.push({
        kind: "action",
        id: "act-embed-studio",
        title: "Open Embedded Roblox Studio & All-12 MCP Tools",
        subtitle: "Stream live MJPEG Roblox Studio viewport, pass mouse/keyboard input, Play/Server/Stop, and run all 12 MCP tools",
        badge: "Embedded",
        badgeCls: "bg-tag-sky-bg text-tag-sky-fg",
        onRun: onOpenEmbeddedStudio,
      });
    }
    if (onAddBlockPart) {
      out.push({
        kind: "action",
        id: "act-add-part",
        title: "Add 3D Part (Block) into Workspace",
        subtitle: "Insert a new Roblox Part directly into the 3D Studio Viewport & Explorer",
        badge: "Studio 3D",
        badgeCls: "bg-tag-mint-bg text-tag-mint-fg",
        onRun: onAddBlockPart,
      });
    }
    if (pieceCount > 0 && onDownloadShipPack) {
      out.push({
        kind: "action",
        id: "act-ship-pack",
        title: "Download Studio & Rojo Ship Pack (.zip)",
        subtitle: "Bundle game.rbxlx, default.project.json, src/*.luau, card assets, audit report & checklist",
        badge: "Rojo ZIP",
        badgeCls: "bg-tag-ops-bg text-tag-ops-fg",
        onRun: onDownloadShipPack,
      });
    }
    if (pieceCount > 0) {
      out.push({
        kind: "action",
        id: "act-export",
        title: "Export game.rbxlx",
        subtitle: `Download Roblox Studio XML (${pieceCount} pieces)`,
        badge: "Export",
        badgeCls: "bg-tag-mint-bg text-tag-mint-fg",
        onRun: onExportRbxlx,
      });
    }
    if (canUndo) {
      out.push({
        kind: "action",
        id: "act-undo",
        title: "Undo last Studio build",
        subtitle: "Revert instances and Luau scripts from the last build",
        badge: "Action",
        badgeCls: "bg-pill-warn-bg text-pill-warn-fg",
        onRun: onTriggerUndo,
      });
    }
    if (canRedo && onTriggerRedo) {
      out.push({
        kind: "action",
        id: "act-redo",
        title: "Redo last Studio action",
        subtitle: "Re-apply the most recently undone Studio part or script change (Ctrl+Y)",
        badge: "Action",
        badgeCls: "bg-tag-sky-bg text-tag-sky-fg",
        onRun: onTriggerRedo,
      });
    }

    // 2. Place Instances & Scripts
    if (place && place !== "error") {
      for (const [path, inst] of Object.entries(place.instances || {})) {
        const name = path.split(".").pop() || path;
        out.push({
          kind: "part",
          id: `inst:${path}`,
          title: name,
          subtitle: `${path} (${inst.class_name || "Instance"})`,
          badge: "Part",
          badgeCls: "bg-tag-mint-bg text-tag-mint-fg",
          path,
        });
      }
      for (const path of Object.keys(place.scripts || {})) {
        const name = path.split(".").pop() || path;
        out.push({
          kind: "script",
          id: `scr:${path}`,
          title: name,
          subtitle: path,
          badge: "Luau",
          badgeCls: "bg-tag-sky-bg text-tag-sky-fg",
          path,
        });
      }
    }

    // 3. All 28 Roblox Game Systems
    for (const sys of GAME_SYSTEMS) {
      out.push({
        kind: "system",
        id: `sys:${sys.id}`,
        title: sys.title,
        subtitle: `${sys.id} — ${sys.summary}`,
        badge: sys.category,
        badgeCls: CATEGORY_TAG_CLS[sys.category],
        inPlace: isSystemInPlace(sys, place),
        sys,
      });
    }

    const q = query.trim().toLowerCase();
    if (!q) return out;
    return out.filter((it) => `${it.title} ${it.subtitle} ${it.badge}`.toLowerCase().includes(q));
  }, [canRedo, canUndo, onAddBlockPart, onBuildApex, onDownloadShipPack, onExportRbxlx, onImportRbxlxClick, onNewChat, onOpenArtStudio, onOpenEmbeddedStudio, onOpenSettings, onOpenSnapshots, onOpenToolLibrary, onRunPlaytest, onTriggerRedo, onTriggerUndo, place, query]);

  const visible = items.slice(0, 40);
  const clampedIdx = visible.length ? Math.min(activeIdx, visible.length - 1) : 0;

  const runItem = (item: PaletteItem) => {
    onOpenChange(false);
    if (item.kind === "action") {
      item.onRun();
    } else if (item.kind === "part" || item.kind === "script") {
      onSelectPath(item.path);
    } else if (item.kind === "system") {
      if (canBuild && !busy) onAddSystem(item.sys);
    }
  };

  const onInputKeyDown = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (visible.length) setActiveIdx((i) => (i + 1) % visible.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (visible.length) setActiveIdx((i) => (i - 1 + visible.length) % visible.length);
    } else if (e.key === "Enter" && visible[clampedIdx]) {
      e.preventDefault();
      runItem(visible[clampedIdx]);
    }
  };

  const iconFor = (item: PaletteItem) => {
    if (item.kind === "part") return Box;
    if (item.kind === "script") return FileCode;
    if (item.kind === "system") return Layers;
    if (item.id === "act-build-apex") return Sparkles;
    if (item.id === "act-art-studio") return Palette;
    if (item.id === "act-tool-library") return Package;
    if (item.id === "act-embed-studio") return Monitor;
    if (item.id === "act-export") return Download;
    if (item.id === "act-ship-pack") return FolderArchive;
    if (item.id === "act-import") return Upload;
    if (item.id === "act-snapshots") return History;
    if (item.id === "act-settings") return SlidersHorizontal;
    if (item.id === "act-undo") return Undo2;
    if (item.id === "act-redo") return Redo2;
    return Plus;
  };

  return (
    <MorphingDialog transition={dialogSpring} open={open} onOpenChange={onOpenChange}>
      <MorphingDialogContainer>
        <MorphingDialogContent className="relative mx-3 flex max-h-[82dvh] w-full max-w-[600px] flex-col rounded-dialog border border-hairline bg-surface text-ink shadow-lift">
          <div data-command-palette className="flex min-h-0 flex-1 flex-col">
            <div className="flex items-center justify-between gap-2 border-b border-hairline px-4 py-3">
              <div className="flex min-w-0 items-center gap-2">
                <Command className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                <MorphingDialogTitle className="text-ui font-medium text-ink">
                  Studio Command Palette
                </MorphingDialogTitle>
                <Pill className="bg-board px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2">
                  28 systems
                </Pill>
              </div>
              <MorphingDialogClose
                aria-label="Close command palette"
                className="static inline-flex size-11 flex-none cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface text-ink-2 outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board hover:text-ink md:size-8"
              >
                <X className="size-4" strokeWidth={1.5} aria-hidden />
              </MorphingDialogClose>
            </div>
            <MorphingDialogDescription className="sr-only">
              Search 28 Roblox game systems, Explorer parts and scripts, Card Art Studio, and Studio workbench actions.
            </MorphingDialogDescription>

            <div className="border-b border-hairline px-4 py-3">
              <InputGroup className="bg-board/50">
                <InputGroupAddon align="inline-start" className="ps-2.5">
                  <Search className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                </InputGroupAddon>
                <InputGroupInput
                  ref={inputRef}
                  type="search"
                  value={query}
                  onChange={(e) => {
                    setQuery(e.target.value);
                    setActiveIdx(0);
                  }}
                  onKeyDown={onInputKeyDown}
                  aria-label="Search commands, parts, scripts, and 28 game systems"
                  placeholder="Search 28 game systems, card art, parts, scripts, or actions…"
                  className="[&_input]:max-md:!min-h-11 [&_input]:md:!h-9"
                />
                {query ? (
                  <InputGroupAddon align="inline-end" className="has-[>button]:me-0 pe-1">
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      onClick={() => setQuery("")}
                      aria-label="Clear command search"
                      className="max-md:size-11"
                    >
                      <X className="size-3.5" strokeWidth={1.5} aria-hidden />
                    </Button>
                  </InputGroupAddon>
                ) : (
                  <InputGroupAddon align="inline-end" className="has-[>kbd:last-child]:me-0 pe-2">
                    <Kbd className="px-1.5 font-mono">Esc</Kbd>
                  </InputGroupAddon>
                )}
              </InputGroup>
            </div>

            <div className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto p-2">
              {visible.length === 0 ? (
                <p className="rounded-card bg-board p-4 text-sm text-ink-2">
                  No commands, parts, or game systems match &ldquo;{query}&rdquo;.
                </p>
              ) : (
                visible.map((item, idx) => {
                  const Icon = iconFor(item);
                  const active = idx === clampedIdx;
                  const disabled = item.kind === "system" && (!canBuild || busy);
                  return (
                    <Button
                      key={item.id}
                      variant="ghost"
                      disabled={disabled}
                      onMouseEnter={() => setActiveIdx(idx)}
                      onClick={() => runItem(item)}
                      className={`h-auto w-full justify-between gap-3 rounded-card px-3 py-2 text-left font-normal max-md:min-h-11 ${
                        active ? "bg-blue-soft text-ink" : "text-ink hover:bg-board/70"
                      }`}
                    >
                      <span className="flex min-w-0 flex-1 items-center gap-2.5">
                        <Icon className="size-4 flex-none text-icon" strokeWidth={1.5} aria-hidden />
                        <span className="flex min-w-0 flex-1 flex-col">
                          <span className="truncate text-sm font-medium text-ink">{item.title}</span>
                          <span className="truncate text-xs text-ink-2">{item.subtitle}</span>
                        </span>
                      </span>
                      <span className="flex shrink-0 items-center gap-1.5">
                        {item.kind === "system" && item.inPlace && (
                          <Pill className="bg-pill-active-bg px-1.5 py-0 text-xs text-pill-active-fg">In place</Pill>
                        )}
                        <Badge className={`border-transparent px-2 py-0.5 text-xs font-medium ${item.badgeCls}`}>
                          {item.badge}
                        </Badge>
                      </span>
                    </Button>
                  );
                })
              )}
            </div>

            <div className="flex flex-wrap items-center justify-between gap-2 border-t border-hairline bg-board/40 px-4 py-2 text-xs text-ink-2">
              <span className="inline-flex items-center gap-1.5">
                <Kbd className="px-1 font-mono">↑</Kbd>
                <Kbd className="px-1 font-mono">↓</Kbd>
                <span>navigate</span>
                <span aria-hidden>·</span>
                <Kbd className="px-1.5 font-mono">Enter</Kbd>
                <span>run</span>
              </span>
              <span className="font-mono tabular-nums">{items.length} matches</span>
            </div>
          </div>
        </MorphingDialogContent>
      </MorphingDialogContainer>
    </MorphingDialog>
  );
}
