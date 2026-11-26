import { memo, useEffect, useState } from "react";
import { AnimatePresence, m, useReducedMotion } from "motion/react";
import {
  Activity,
  AtSign,
  ChevronDown,
  ChevronUp,
  CopyPlus,
  Play,
  RefreshCw,
  Save,
  Sliders,
  Terminal,
  Trash2,
  Wrench,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Textarea } from "@/components/ui/textarea";
import { Pill } from "@/components/kibo-ui/pill";
import { instant, open as openAnim } from "@/motion";
import {
  STUDIO_TOOL_SPECS,
  type InstanceEditPayload,
  type Place,
  type StudioActionEvent,
  type StudioToolName,
  type StudioToolResult,
} from "./data";
import type { WorkspaceLighting } from "./ship/api";

export type BottomDrawerTab = "console" | "trace" | "tools" | "properties";

function readVec3(val: unknown, fallback: [number, number, number]): [number, number, number] {
  if (Array.isArray(val) && val.length >= 3 && val.every((n) => typeof n === "number" && Number.isFinite(n))) {
    return [val[0], val[1], val[2]];
  }
  return fallback;
}

export function StudioBottomDrawer({
  open,
  onToggle,
  activeTab,
  onTabChange,
  place,
  selectedPath,
  studioActions,
  lighting,
  projectMode = false,
  onSelectPath,
  onOpenScript,
  onAttachContext,
  onSaveInstance,
  onSaveLighting,
  onDuplicatePart,
  onDeletePart,
  onInvokeStudioTool,
}: {
  open: boolean;
  onToggle: () => void;
  activeTab: BottomDrawerTab;
  onTabChange: (tab: BottomDrawerTab) => void;
  place: Place;
  selectedPath: string | null;
  studioActions: StudioActionEvent[];
  lighting?: WorkspaceLighting | null;
  projectMode?: boolean;
  onSelectPath: (path: string) => void;
  onOpenScript: (path: string) => void;
  onAttachContext: (path: string) => void;
  onSaveInstance: (payload: InstanceEditPayload) => Promise<void> | void;
  onSaveLighting?: (lighting: WorkspaceLighting) => Promise<void> | void;
  onDuplicatePart: (path: string) => Promise<void> | void;
  onDeletePart: (path: string) => Promise<void> | void;
  onInvokeStudioTool: (tool: StudioToolName, args: Record<string, unknown>) => Promise<StudioToolResult>;
}) {
  const [selectedTool, setSelectedTool] = useState<StudioToolName>("get_studio_state");
  const [toolArgs, setToolArgs] = useState("{}");
  const [toolBusy, setToolBusy] = useState(false);
  const [toolResult, setToolResult] = useState<StudioToolResult | null>(null);
  const [consoleLines, setConsoleLines] = useState<string[]>([]);
  const [consoleBusy, setConsoleBusy] = useState(false);
  const reduced = useReducedMotion();

  const selectedInstance = selectedPath && selectedPath !== "Lighting" ? place.instances?.[selectedPath] ?? null : null;
  const [posX, setPosX] = useState("0");
  const [posY, setPosY] = useState("2");
  const [posZ, setPosZ] = useState("0");
  const [sizeX, setSizeX] = useState("8");
  const [sizeY, setSizeY] = useState("4");
  const [sizeZ, setSizeZ] = useState("8");
  const [color, setColor] = useState("#8B939E");
  const [material, setMaterial] = useState("SmoothPlastic");
  const [anchored, setAnchored] = useState(true);
  const [meshId, setMeshId] = useState("");
  const [textureId, setTextureId] = useState("");
  const [propBusy, setPropBusy] = useState(false);

  const [clockTime, setClockTime] = useState("14");
  const [brightness, setBrightness] = useState("2");
  const [ambient, setAmbient] = useState("#707A8A");
  const [fogEnd, setFogEnd] = useState("10000");
  const [technology, setTechnology] = useState("ShadowMap");
  const [lightingMode, setLightingMode] = useState(false);

  useEffect(() => {
    if (selectedPath === "Lighting") setLightingMode(true);
    else if (selectedInstance) setLightingMode(false);
  }, [selectedInstance, selectedPath]);

  useEffect(() => {
    if (!lighting) return;
    setClockTime(String(typeof lighting.clock_time === "number" ? lighting.clock_time : 14));
    setBrightness(String(typeof lighting.brightness === "number" ? lighting.brightness : 2));
    setAmbient(typeof lighting.ambient === "string" && lighting.ambient ? lighting.ambient : "#707A8A");
    setFogEnd(String(typeof lighting.fog_end === "number" ? lighting.fog_end : 10000));
    setTechnology(typeof lighting.technology === "string" && lighting.technology ? lighting.technology : "ShadowMap");
  }, [lighting]);

  useEffect(() => {
    if (!selectedInstance) return;
    const props = selectedInstance.properties || {};
    const pos = readVec3(props.Position, [0, 2, 0]);
    const size = readVec3(props.Size, [8, 4, 8]);
    setPosX(String(pos[0]));
    setPosY(String(pos[1]));
    setPosZ(String(pos[2]));
    setSizeX(String(size[0]));
    setSizeY(String(size[1]));
    setSizeZ(String(size[2]));
    setColor(typeof props.Color === "string" && props.Color ? props.Color : "#8B939E");
    setMaterial(typeof props.Material === "string" && props.Material ? props.Material : "SmoothPlastic");
    setAnchored(props.Anchored !== false);
    setMeshId(typeof props.MeshId === "string" ? props.MeshId : "");
    setTextureId(typeof props.TextureID === "string" ? props.TextureID : "");
  }, [selectedInstance, selectedPath]);

  const selectToolSpec = (name: StudioToolName) => {
    setSelectedTool(name);
    const spec = STUDIO_TOOL_SPECS.find((s) => s.name === name);
    setToolArgs(JSON.stringify(spec?.defaultArgs ?? {}, null, 2));
  };

  const handleRunTool = async () => {
    if (toolBusy) return;
    let parsed: Record<string, unknown> = {};
    try {
      const val = toolArgs.trim() ? JSON.parse(toolArgs) : {};
      if (val && typeof val === "object" && !Array.isArray(val)) parsed = val as Record<string, unknown>;
    } catch {
      setToolResult({ ok: false, tool: selectedTool, error: "Invalid JSON arguments." });
      return;
    }
    setToolBusy(true);
    try {
      const res = await onInvokeStudioTool(selectedTool, parsed);
      setToolResult(res);
    } finally {
      setToolBusy(false);
    }
  };

  const handleRefreshConsole = async () => {
    if (consoleBusy) return;
    setConsoleBusy(true);
    try {
      const res = await onInvokeStudioTool("get_console_output", { limit: 40 });
      const raw = res.result;
      const arr = Array.isArray(raw)
        ? raw
        : raw && typeof raw === "object" && Array.isArray((raw as Record<string, unknown>).lines)
          ? ((raw as Record<string, unknown>).lines as unknown[])
          : raw && typeof raw === "object" && Array.isArray((raw as Record<string, unknown>).logs)
            ? ((raw as Record<string, unknown>).logs as unknown[])
            : [];
      const extracted = arr.map((item) =>
        typeof item === "string" ? item : JSON.stringify(item),
      );
      if (extracted.length > 0) setConsoleLines(extracted);
      else if (res.text) setConsoleLines(res.text.split("\n").filter(Boolean));
      else if (res.error) setConsoleLines([`Error: ${res.error}`]);
    } finally {
      setConsoleBusy(false);
    }
  };

  const handleSaveProps = async () => {
    if (!selectedPath || !selectedInstance || propBusy) return;
    setPropBusy(true);
    try {
      const nextProps: Record<string, unknown> = {
        ...(selectedInstance.properties || {}),
        Position: [Number(posX) || 0, Number(posY) || 0, Number(posZ) || 0],
        Size: [Math.max(0.5, Number(sizeX) || 4), Math.max(0.5, Number(sizeY) || 2), Math.max(0.5, Number(sizeZ) || 4)],
        Color: color.trim() || "#8B939E",
        Material: material.trim() || "SmoothPlastic",
        Anchored: anchored,
      };
      if (selectedInstance.class_name === "MeshPart" || meshId.trim()) {
        nextProps.MeshId = meshId.trim();
      }
      if (selectedInstance.class_name === "MeshPart" || textureId.trim()) {
        nextProps.TextureID = textureId.trim();
      }
      await onSaveInstance({
        path: selectedPath,
        class_name: selectedInstance.class_name || "Part",
        properties: nextProps,
      });
    } finally {
      setPropBusy(false);
    }
  };

  const handleSaveLightingProps = async () => {
    if (!onSaveLighting || propBusy) return;
    setPropBusy(true);
    try {
      const ct = Number(clockTime);
      const br = Number(brightness);
      const fe = Number(fogEnd);
      await onSaveLighting({
        clock_time: Number.isFinite(ct) ? Math.max(0, Math.min(24, ct)) : 14,
        brightness: Number.isFinite(br) ? Math.max(0, Math.min(10, br)) : 2,
        ambient: /^#[0-9a-f]{6}$/i.test(ambient.trim()) ? ambient.trim() : "#707A8A",
        fog_end: Number.isFinite(fe) ? Math.max(20, fe) : 10000,
        technology: technology || "ShadowMap",
      });
    } finally {
      setPropBusy(false);
    }
  };

  const mergedConsole = [
    ...studioActions
      .filter((a) => a.console_line || a.kind.startsWith("playtest"))
      .map((a) => a.console_line || `[${a.kind}] ${a.summary}`),
    ...consoleLines,
  ].slice(-60);

  const errorCount = mergedConsole.filter((line) => /\b(error|failed|exception|nil)\b/i.test(line)).length;

  const openTab = (t: BottomDrawerTab) => {
    onTabChange(t);
    if (!open) onToggle();
  };

  return (
    <section
      data-studio-bottom-drawer
      aria-label="Studio bottom drawer"
      className="flex flex-none flex-col border-t border-hairline bg-surface"
    >
      {!open ? (
        <div className="flex items-center justify-end bg-board/40 px-2.5 py-1">
          <Button
            variant="ghost"
            size="xs"
            aria-label="Expand Console and Inspector"
            onClick={() => openTab(errorCount > 0 ? "console" : activeTab)}
            className="gap-1.5 text-ink-2 hover:text-ink max-md:min-h-11"
          >
            <Terminal className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Console / Inspector</span>
            {errorCount > 0 && (
              <Badge className="border-transparent bg-pill-pro-bg px-1.5 py-0 font-mono text-xs tabular-nums text-pill-pro-fg">
                {errorCount}
              </Badge>
            )}
            <ChevronUp className="size-3.5" strokeWidth={1.5} aria-hidden />
          </Button>
        </div>
      ) : (
        <div className="flex min-h-9 flex-wrap items-center justify-between gap-2 bg-board/60 px-2.5 py-1">
          <SegmentedControl aria-label="Studio bottom drawer views" size="sm" className="max-w-full flex-wrap">
            <SegmentedControlItem
              data-checked={activeTab === "console" ? "" : undefined}
              aria-pressed={activeTab === "console"}
              onClick={() => openTab("console")}
              className="gap-1 text-xs max-md:!h-11"
            >
              <Terminal className="size-3" strokeWidth={1.5} aria-hidden />
              Console
              {errorCount > 0 && (
                <span className="rounded-chip bg-pill-pro-bg px-1 font-mono text-xs tabular-nums text-pill-pro-fg">
                  {errorCount}
                </span>
              )}
            </SegmentedControlItem>
            <SegmentedControlItem
              data-checked={activeTab === "trace" ? "" : undefined}
              aria-pressed={activeTab === "trace"}
              onClick={() => openTab("trace")}
              className="gap-1 text-xs max-md:!h-11"
            >
              <Activity className="size-3" strokeWidth={1.5} aria-hidden />
              Trace{studioActions.length > 0 ? ` (${studioActions.length})` : ""}
            </SegmentedControlItem>
            <SegmentedControlItem
              data-checked={activeTab === "tools" ? "" : undefined}
              aria-pressed={activeTab === "tools"}
              onClick={() => openTab("tools")}
              className="gap-1 text-xs max-md:!h-11"
            >
              <Wrench className="size-3" strokeWidth={1.5} aria-hidden />
              Studio Tools
            </SegmentedControlItem>
            <SegmentedControlItem
              data-checked={activeTab === "properties" ? "" : undefined}
              aria-pressed={activeTab === "properties"}
              onClick={() => openTab("properties")}
              className="gap-1 text-xs max-md:!h-11"
            >
              <Sliders className="size-3" strokeWidth={1.5} aria-hidden />
              Properties{selectedPath ? `: ${selectedPath.split(".").pop()}` : ""}
            </SegmentedControlItem>
          </SegmentedControl>

          <Button
            variant="ghost"
            size="xs"
            aria-label="Collapse Studio bottom drawer"
            onClick={onToggle}
            className="text-ink-2 hover:text-ink max-md:min-h-11"
          >
            <ChevronDown className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Hide</span>
          </Button>
        </div>
      )}

      {/* 236px Expanded Drawer Body */}
      <AnimatePresence initial={false}>
      {open && (
        <m.div
          key="studio-bottom-drawer-body"
          initial={reduced ? { opacity: 1 } : { opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={reduced ? { opacity: 0 } : { opacity: 0, y: 4 }}
          transition={reduced ? instant : openAnim}
          className="h-[236px] overflow-y-auto border-t border-hairline bg-surface p-3"
        >
          {activeTab === "console" && (
            <div data-studio-console-sync className="flex flex-col gap-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-xs font-medium text-ink-2">Roblox Studio Output &amp; Playtest Console</span>
                <div className="flex items-center gap-1.5">
                  <Button
                    variant="outline"
                    size="xs"
                    disabled={consoleBusy}
                    aria-label="Fetch Studio console output"
                    onClick={() => void handleRefreshConsole()}
                    className="bg-surface max-md:min-h-11"
                  >
                    <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
                    {consoleBusy ? "Fetching…" : "Refresh Output"}
                  </Button>
                  {consoleLines.length > 0 && (
                    <Button variant="ghost" size="xs" onClick={() => setConsoleLines([])} className="max-md:min-h-11">
                      Clear
                    </Button>
                  )}
                </div>
              </div>
              {mergedConsole.length === 0 ? (
                <p className="rounded-sm border border-hairline bg-board/35 p-3 text-xs text-ink-2">
                  No Studio console lines yet. Run Playtest or click Refresh Output to inspect live logs.
                </p>
              ) : (
                <div className="flex flex-col gap-1 rounded-sm border border-hairline bg-board/35 p-2.5 font-mono text-xs">
                  {mergedConsole.map((line, idx) => (
                    <div
                      key={idx}
                      className={
                        /error|fail/i.test(line)
                          ? "text-pill-pro-fg"
                          : /warn/i.test(line)
                            ? "text-pill-warn-fg"
                            : "text-ink"
                      }
                    >
                      {line}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {activeTab === "trace" && (
            <div data-agent-trace-panel className="flex flex-col gap-1.5">
              {studioActions.length === 0 ? (
                <p className="rounded-sm border border-hairline bg-board/35 p-3 text-xs text-ink-2">
                  Live Studio build mutations, script writes, and playtest actions stream here in real time.
                </p>
              ) : (
                studioActions
                  .slice(-40)
                  .reverse()
                  .map((ev) => (
                    <div
                      key={ev.seq}
                      className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-board/30 px-2.5 py-1.5 text-xs"
                    >
                      <div className="flex min-w-0 flex-1 items-center gap-2">
                        <Badge className="border-transparent bg-surface px-1.5 py-0.5 font-mono text-xs text-ink-2">
                          {ev.kind}
                        </Badge>
                        <span className="truncate text-ink">{ev.summary}</span>
                      </div>
                      {ev.path && (
                        <Button
                          variant="ghost"
                          size="xs"
                          onClick={() => {
                            if (ev.kind === "script_written") onOpenScript(ev.path);
                            else onSelectPath(ev.path);
                          }}
                          className="font-mono text-xs text-blue max-md:min-h-11"
                        >
                          {ev.path}
                        </Button>
                      )}
                    </div>
                  ))
              )}
            </div>
          )}

          {activeTab === "tools" && (
            <div data-studio-tools-drawer className="flex flex-col gap-2.5">
              <div className="flex flex-wrap gap-1">
                {STUDIO_TOOL_SPECS.map((spec) => {
                  const active = selectedTool === spec.name;
                  return (
                    <Button
                      key={spec.name}
                      variant={active ? "default" : "outline"}
                      size="xs"
                      aria-pressed={active}
                      aria-label={`Select Studio tool ${spec.name}`}
                      onClick={() => selectToolSpec(spec.name)}
                      className={active ? "font-mono text-xs max-md:min-h-11" : "bg-board/40 font-mono text-xs max-md:min-h-11"}
                    >
                      {spec.name}
                    </Button>
                  );
                })}
              </div>
              <div className="grid gap-2 md:grid-cols-2">
                <div className="flex flex-col gap-1.5">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-xs text-ink-2">
                      {STUDIO_TOOL_SPECS.find((s) => s.name === selectedTool)?.summary}
                    </span>
                    <Button
                      variant="default"
                      size="xs"
                      disabled={toolBusy}
                      aria-label={`Run Studio tool ${selectedTool}`}
                      onClick={() => void handleRunTool()}
                      className="max-md:min-h-11"
                    >
                      <Play className="size-3" strokeWidth={1.5} aria-hidden />
                      {toolBusy ? "Running…" : "Run Tool"}
                    </Button>
                  </div>
                  <Textarea
                    value={toolArgs}
                    onChange={(e) => setToolArgs(e.target.value)}
                    aria-label="Studio MCP tool JSON arguments"
                    spellCheck={false}
                    className="h-24 resize-none bg-board/35 font-mono text-xs"
                  />
                </div>
                <div data-studio-tool-result className="flex flex-col gap-1">
                  <div className="flex items-center justify-between text-xs">
                    <span className="font-medium text-ink-2">Tool Result</span>
                    {toolResult && (
                      <Pill
                        className={
                          toolResult.ok
                            ? "bg-pill-active-bg px-2 py-0.5 font-mono text-xs text-pill-active-fg"
                            : "bg-pill-pro-bg px-2 py-0.5 font-mono text-xs text-pill-pro-fg"
                        }
                      >
                        {toolResult.ok ? "OK" : "Error"}
                      </Pill>
                    )}
                  </div>
                  <pre className="h-24 overflow-auto whitespace-pre-wrap break-words rounded-sm border border-hairline bg-board/35 p-2 font-mono text-xs text-ink">
                    {toolResult
                      ? toolResult.error
                        ? toolResult.error
                        : toolResult.text || JSON.stringify(toolResult.result ?? toolResult, null, 2)
                      : "Select any of the 13 official Roblox Studio MCP tools and click Run Tool."}
                  </pre>
                </div>
              </div>
            </div>
          )}

          {activeTab === "properties" && (
            <div data-properties-panel className="flex flex-col gap-2.5">
              {(lightingMode || (!selectedInstance && (projectMode || Boolean(lighting)) && Boolean(onSaveLighting))) && onSaveLighting ? (
                <>
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="flex items-center gap-2">
                      <code className="font-mono text-xs font-medium text-ink">Lighting</code>
                      <Badge className="border-transparent bg-board px-1.5 py-0.5 text-xs text-ink-2">
                        Scene Lighting
                      </Badge>
                    </div>
                    <div className="flex flex-wrap items-center gap-1.5">
                      {selectedInstance && (
                        <Button
                          variant="outline"
                          size="xs"
                          onClick={() => setLightingMode(false)}
                          className="bg-surface max-md:min-h-11"
                        >
                          Back to Part
                        </Button>
                      )}
                      <Button
                        variant="default"
                        size="xs"
                        disabled={propBusy}
                        onClick={() => void handleSaveLightingProps()}
                        className="max-md:min-h-11"
                      >
                        <Save className="size-3" strokeWidth={1.5} aria-hidden />
                        Save lighting
                      </Button>
                    </div>
                  </div>
                  <div className="grid gap-2 sm:grid-cols-4">
                    <div className="flex flex-col gap-1">
                      <span className="text-xs text-ink-2">ClockTime (0-24)</span>
                      <Input value={clockTime} onChange={(e) => setClockTime(e.target.value)} aria-label="Lighting ClockTime" className="h-8 font-mono text-xs max-md:min-h-11" />
                    </div>
                    <div className="flex flex-col gap-1">
                      <span className="text-xs text-ink-2">Brightness</span>
                      <Input value={brightness} onChange={(e) => setBrightness(e.target.value)} aria-label="Lighting Brightness" className="h-8 font-mono text-xs max-md:min-h-11" />
                    </div>
                    <div className="flex flex-col gap-1">
                      <span className="text-xs text-ink-2">FogEnd</span>
                      <Input value={fogEnd} onChange={(e) => setFogEnd(e.target.value)} aria-label="Lighting FogEnd" className="h-8 font-mono text-xs max-md:min-h-11" />
                    </div>
                    <div className="flex flex-col gap-1">
                      <span className="text-xs text-ink-2">Ambient &amp; Tech</span>
                      <div className="flex items-center gap-1.5">
                        <input
                          type="color"
                          aria-label="Lighting Ambient picker"
                          value={/^#[0-9a-f]{6}$/i.test(ambient.trim()) ? ambient.trim() : "#707A8A"}
                          onChange={(e) => setAmbient(e.target.value)}
                          className="h-8 w-9 shrink-0 cursor-pointer rounded-sm border border-hairline bg-surface p-0.5 max-md:min-h-11"
                        />
                        <Input value={ambient} onChange={(e) => setAmbient(e.target.value)} aria-label="Lighting Ambient hex" className="h-8 w-20 font-mono text-xs max-md:min-h-11" />
                        <select
                          value={technology}
                          onChange={(e) => setTechnology(e.target.value)}
                          aria-label="Lighting Technology"
                          className="h-8 min-w-0 flex-1 rounded-lg border border-hairline bg-surface px-2 font-mono text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11"
                        >
                          {["ShadowMap", "Future", "Voxel", "Compatibility"].map((t) => (
                            <option key={t} value={t}>{t}</option>
                          ))}
                        </select>
                      </div>
                    </div>
                  </div>
                </>
              ) : !selectedPath || !selectedInstance ? (
                <p className="rounded-sm border border-hairline bg-board/35 p-3 text-xs text-ink-2">
                  Select a Workspace part in the Left Explorer or click any part in Studio 3D to edit Position, Size, Color, Material, and Anchored state.
                </p>
              ) : (
                <>
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="flex items-center gap-2">
                      <code className="font-mono text-xs font-medium text-ink">{selectedPath}</code>
                      <Badge className="border-transparent bg-board px-1.5 py-0.5 text-xs text-ink-2">
                        {selectedInstance.class_name || "Part"}
                      </Badge>
                    </div>
                    <div className="flex flex-wrap items-center gap-1.5">
                      {onSaveLighting && (
                        <Button
                          variant="outline"
                          size="xs"
                          onClick={() => setLightingMode(true)}
                          className="bg-surface max-md:min-h-11"
                        >
                          Scene Lighting
                        </Button>
                      )}
                      <Button
                        variant="outline"
                        size="xs"
                        onClick={() => onAttachContext(selectedPath)}
                        className="bg-surface max-md:min-h-11"
                      >
                        <AtSign className="size-3" strokeWidth={1.5} aria-hidden />
                        Context
                      </Button>
                      <Button
                        variant="outline"
                        size="xs"
                        disabled={propBusy}
                        onClick={() => void onDuplicatePart(selectedPath)}
                        className="bg-surface max-md:min-h-11"
                      >
                        <CopyPlus className="size-3" strokeWidth={1.5} aria-hidden />
                        Duplicate
                      </Button>
                      <Button
                        variant="outline"
                        size="xs"
                        disabled={propBusy}
                        onClick={() => void onDeletePart(selectedPath)}
                        className="bg-surface text-pill-pro-fg max-md:min-h-11"
                      >
                        <Trash2 className="size-3" strokeWidth={1.5} aria-hidden />
                        Delete
                      </Button>
                      <Button
                        variant="default"
                        size="xs"
                        disabled={propBusy}
                        onClick={() => void handleSaveProps()}
                        className="max-md:min-h-11"
                      >
                        <Save className="size-3" strokeWidth={1.5} aria-hidden />
                        Save properties
                      </Button>
                    </div>
                  </div>
                  <div className="grid gap-2 sm:grid-cols-3">
                    <div className="flex flex-col gap-1">
                      <span className="text-xs text-ink-2">Position (X, Y, Z)</span>
                      <div className="grid grid-cols-3 gap-1">
                        <Input value={posX} onChange={(e) => setPosX(e.target.value)} aria-label="Position X" className="h-8 font-mono text-xs max-md:min-h-11" />
                        <Input value={posY} onChange={(e) => setPosY(e.target.value)} aria-label="Position Y" className="h-8 font-mono text-xs max-md:min-h-11" />
                        <Input value={posZ} onChange={(e) => setPosZ(e.target.value)} aria-label="Position Z" className="h-8 font-mono text-xs max-md:min-h-11" />
                      </div>
                    </div>
                    <div className="flex flex-col gap-1">
                      <span className="text-xs text-ink-2">Size (X, Y, Z)</span>
                      <div className="grid grid-cols-3 gap-1">
                        <Input value={sizeX} onChange={(e) => setSizeX(e.target.value)} aria-label="Size X" className="h-8 font-mono text-xs max-md:min-h-11" />
                        <Input value={sizeY} onChange={(e) => setSizeY(e.target.value)} aria-label="Size Y" className="h-8 font-mono text-xs max-md:min-h-11" />
                        <Input value={sizeZ} onChange={(e) => setSizeZ(e.target.value)} aria-label="Size Z" className="h-8 font-mono text-xs max-md:min-h-11" />
                      </div>
                    </div>
                    <div className="flex flex-col gap-1">
                      <span className="text-xs text-ink-2">Color, Material, Anchored</span>
                      <div className="flex items-center gap-1.5">
                        <input
                          type="color"
                          aria-label="Color picker"
                          value={/^#[0-9a-f]{6}$/i.test(color.trim()) ? color.trim() : "#9aa0a6"}
                          onChange={(e) => setColor(e.target.value)}
                          className="h-8 w-9 shrink-0 cursor-pointer rounded-sm border border-hairline bg-surface p-0.5 max-md:min-h-11"
                        />
                        <Input value={color} onChange={(e) => setColor(e.target.value)} aria-label="Color hex" className="h-8 w-24 font-mono text-xs max-md:min-h-11" />
                        <select
                          value={material}
                          onChange={(e) => setMaterial(e.target.value)}
                          aria-label="Material"
                          className="h-8 min-w-0 flex-1 rounded-lg border border-hairline bg-surface px-2 font-mono text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11"
                        >
                          {["SmoothPlastic", "Plastic", "Neon", "Metal", "Wood", "WoodPlanks", "Slate", "Concrete", "Glass", "ForceField", "Granite", "Marble", "Ice", "Fabric", "Foil"].map((m) => (
                            <option key={m} value={m}>{m}</option>
                          ))}
                          {material && !["SmoothPlastic", "Plastic", "Neon", "Metal", "Wood", "WoodPlanks", "Slate", "Concrete", "Glass", "ForceField", "Granite", "Marble", "Ice", "Fabric", "Foil"].includes(material) && (
                            <option value={material}>{material}</option>
                          )}
                        </select>
                        <Button
                          variant={anchored ? "default" : "outline"}
                          size="xs"
                          aria-pressed={anchored}
                          onClick={() => setAnchored((v) => !v)}
                          className="max-md:min-h-11"
                        >
                          {anchored ? "Anchored" : "Free"}
                        </Button>
                      </div>
                    </div>
                  </div>
                  {(selectedInstance.class_name === "MeshPart" || meshId || textureId) && (
                    <div className="grid gap-2 sm:grid-cols-2">
                      <div className="flex flex-col gap-1">
                        <span className="text-xs text-ink-2">MeshId / OBJ path</span>
                        <Input value={meshId} onChange={(e) => setMeshId(e.target.value)} aria-label="MeshId" placeholder="assets/meshes/prop.obj" className="h-8 font-mono text-xs max-md:min-h-11" />
                      </div>
                      <div className="flex flex-col gap-1">
                        <span className="text-xs text-ink-2">TextureID</span>
                        <Input value={textureId} onChange={(e) => setTextureId(e.target.value)} aria-label="TextureID" placeholder="assets/meshes/prop.png" className="h-8 font-mono text-xs max-md:min-h-11" />
                      </div>
                    </div>
                  )}
                </>
              )}
            </div>
          )}
        </m.div>
      )}
      </AnimatePresence>
    </section>
  );
}
