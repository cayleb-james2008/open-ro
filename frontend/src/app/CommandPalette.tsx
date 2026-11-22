import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  Box,
  Command,
  Download,
  FileCode,
  History,
  Layers,
  Plus,
  Search,
  SlidersHorizontal,
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
  onAddSystem,
  onSelectPath,
  onNewChat,
  onExportRbxlx,
  onImportRbxlxClick,
  onOpenSnapshots,
  onOpenSettings,
  onTriggerUndo,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  place: Place | null | "error";
  canBuild: boolean;
  busy: boolean;
  canUndo: boolean;
  onAddSystem: (sys: GameSystemDef) => void;
  onSelectPath: (path: string) => void;
  onNewChat: () => void;
  onExportRbxlx: () => void;
  onImportRbxlxClick: () => void;
  onOpenSnapshots: () => void;
  onOpenSettings: () => void;
  onTriggerUndo: () => void;
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

    // 3. All 22 Roblox Game Systems
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
  }, [canUndo, onExportRbxlx, onImportRbxlxClick, onNewChat, onOpenSettings, onOpenSnapshots, onTriggerUndo, place, query]);

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
    if (item.id === "act-export") return Download;
    if (item.id === "act-import") return Upload;
    if (item.id === "act-snapshots") return History;
    if (item.id === "act-settings") return SlidersHorizontal;
    if (item.id === "act-undo") return Undo2;
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
                  22 systems
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
              Search 22 Roblox game systems, Explorer parts and scripts, and Studio workbench actions.
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
                  aria-label="Search commands, parts, scripts, and 22 game systems"
                  placeholder="Search 22 game systems, parts, scripts, or actions…"
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
