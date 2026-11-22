import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type WheelEvent as ReactWheelEvent,
} from "react";
import {
  Camera,
  FolderTree,
  Keyboard,
  Monitor,
  Play,
  Plug,
  RefreshCw,
  Rocket,
  Search,
  Server,
  Square,
  Terminal,
  Unplug,
  Wrench,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Textarea } from "@/components/ui/textarea";
import { Pill, PillIndicator } from "@/components/kibo-ui/pill";
import {
  STUDIO_TOOL_SPECS,
  type Place,
  type StudioActionEvent,
  type StudioEmbedStatus,
  type StudioKeyboardInputPayload,
  type StudioMode,
  type StudioMouseInputPayload,
  type StudioPlayMode,
  type StudioToolName,
  type StudioToolResult,
} from "./data";

type SubPanelTab = "tools" | "explorer" | "console";

type ExplorerSyncNode = {
  path: string;
  name: string;
  class_name: string;
};

type ConsoleSyncLine = {
  level: "info" | "warn" | "error";
  text: string;
  timestamp?: string;
};

function extractTreeNodes(res: StudioToolResult | null, fallbackPlace: Place): ExplorerSyncNode[] {
  const out: ExplorerSyncNode[] = [];
  const raw = res?.result;
  const arr = Array.isArray(raw)
    ? raw
    : raw && typeof raw === "object"
      ? (Array.isArray((raw as Record<string, unknown>).matches)
          ? ((raw as Record<string, unknown>).matches as unknown[])
          : Array.isArray((raw as Record<string, unknown>).nodes)
            ? ((raw as Record<string, unknown>).nodes as unknown[])
            : Array.isArray((raw as Record<string, unknown>).instances)
              ? ((raw as Record<string, unknown>).instances as unknown[])
              : null)
      : null;

  if (arr) {
    for (const item of arr) {
      if (typeof item === "string") {
        out.push({ path: item, name: item.split(".").pop() || item, class_name: "Instance" });
      } else if (item && typeof item === "object") {
        const o = item as Record<string, unknown>;
        const p = typeof o.path === "string" ? o.path : typeof o.Path === "string" ? o.Path : "";
        if (!p) continue;
        const name = typeof o.name === "string" ? o.name : p.split(".").pop() || p;
        const cls =
          typeof o.class_name === "string"
            ? o.class_name
            : typeof o.className === "string"
              ? o.className
              : typeof o.ClassName === "string"
                ? o.ClassName
                : "Instance";
        out.push({ path: p, name, class_name: cls });
      }
    }
    return out;
  }

  // Deterministic fallback from current place tree before first manual sync
  for (const [p, inst] of Object.entries(fallbackPlace.instances || {})) {
    out.push({ path: p, name: p.split(".").pop() || p, class_name: inst.class_name || "Part" });
  }
  for (const p of Object.keys(fallbackPlace.scripts || {})) {
    out.push({
      path: p,
      name: p.split(".").pop() || p,
      class_name: p.startsWith("StarterGui.") ? "LocalScript" : "Script",
    });
  }
  return out;
}

function extractConsoleLines(res: StudioToolResult | null): ConsoleSyncLine[] {
  if (!res) return [];
  const raw = res.result;
  const list = Array.isArray(raw)
    ? raw
    : raw && typeof raw === "object"
      ? (Array.isArray((raw as Record<string, unknown>).lines)
          ? ((raw as Record<string, unknown>).lines as unknown[])
          : Array.isArray((raw as Record<string, unknown>).logs)
            ? ((raw as Record<string, unknown>).logs as unknown[])
            : Array.isArray((raw as Record<string, unknown>).output)
              ? ((raw as Record<string, unknown>).output as unknown[])
              : null)
      : null;

  if (list) {
    return list
      .map((item): ConsoleSyncLine | null => {
        if (typeof item === "string") {
          const low = item.toLowerCase();
          return {
            level: low.includes("error") || low.includes("fail") ? "error" : low.includes("warn") ? "warn" : "info",
            text: item,
          };
        }
        if (item && typeof item === "object") {
          const o = item as Record<string, unknown>;
          const text = String(o.message ?? o.text ?? o.line ?? JSON.stringify(o));
          const rawLvl = String(o.level ?? o.type ?? "info").toLowerCase();
          const level: ConsoleSyncLine["level"] =
            rawLvl.includes("err") ? "error" : rawLvl.includes("warn") ? "warn" : "info";
          return {
            level,
            text,
            timestamp: typeof o.timestamp === "string" ? o.timestamp : undefined,
          };
        }
        return null;
      })
      .filter((x): x is ConsoleSyncLine => Boolean(x));
  }
  if (typeof res.text === "string" && res.text.trim()) {
    return res.text
      .split("\n")
      .filter((l) => l.trim().length > 0)
      .map((l) => ({
        level: /error|fail/i.test(l) ? "error" : /warn/i.test(l) ? "warn" : "info",
        text: l,
      }));
  }
  return [];
}

export function EmbeddedStudioPanel({
  place,
  studioMode,
  actions,
  hasLiveFrame,
  frameSeq,
  embedStatus,
  onLaunchEmbed,
  onStopEmbed,
  onRefreshEmbedStatus,
  onCaptureFrame,
  onConnectMcp,
  onDisconnectMcp,
  onSendMouseInput,
  onSendKeyboardInput,
  onInvokeStudioTool,
  onSelectPart,
}: {
  place: Place;
  studioMode: StudioMode;
  actions: StudioActionEvent[];
  hasLiveFrame: boolean;
  frameSeq: number;
  embedStatus: StudioEmbedStatus;
  onLaunchEmbed?: (command?: string) => Promise<void> | void;
  onStopEmbed?: () => Promise<void> | void;
  onRefreshEmbedStatus?: () => Promise<void> | void;
  onCaptureFrame?: () => Promise<void> | void;
  onConnectMcp?: (command?: string) => Promise<void> | void;
  onDisconnectMcp?: () => Promise<void> | void;
  onSendMouseInput?: (payload: StudioMouseInputPayload) => Promise<void> | void;
  onSendKeyboardInput?: (payload: StudioKeyboardInputPayload) => Promise<void> | void;
  onInvokeStudioTool?: (tool: StudioToolName, args: Record<string, unknown>) => Promise<StudioToolResult>;
  onSelectPart?: (path: string) => void;
}) {
  const [embedBusy, setEmbedBusy] = useState(false);
  const [captureBusy, setCaptureBusy] = useState(false);
  const [mcpBusy, setMcpBusy] = useState(false);
  const [playBusy, setPlayBusy] = useState(false);
  const [mcpCmdDraft, setMcpCmdDraft] = useState("");
  const [streamKind, setStreamKind] = useState<"mjpeg" | "snapshot">("mjpeg");
  const [mjpegLoaded, setMjpegLoaded] = useState(false);
  const [mjpegFailed, setMjpegFailed] = useState(false);
  const [frameFailed, setFrameFailed] = useState(false);
  const [keyboardCapture, setKeyboardCapture] = useState(false);
  const [lastPointer, setLastPointer] = useState<{ x: number; y: number; action: string } | null>(null);
  const [lastKey, setLastKey] = useState<string | null>(null);

  // Sub-panel state (All-12 Studio Tools / Explorer Sync / Studio Console)
  const [subTab, setSubTab] = useState<SubPanelTab>("tools");
  const [selectedTool, setSelectedTool] = useState<StudioToolName>("get_studio_state");
  const [toolArgsDraft, setToolArgsDraft] = useState<string>("{}");
  const [toolRunning, setToolRunning] = useState(false);
  const [toolResult, setToolResult] = useState<StudioToolResult | null>(null);
  const [toolParseError, setToolParseError] = useState<string | null>(null);

  const [treeQuery, setTreeQuery] = useState("");
  const [treeResult, setTreeResult] = useState<StudioToolResult | null>(null);
  const [treeSyncing, setTreeSyncing] = useState(false);
  const [inspectedPath, setInspectedPath] = useState<string | null>(null);
  const [inspectedResult, setInspectedResult] = useState<StudioToolResult | null>(null);

  const [consoleResult, setConsoleResult] = useState<StudioToolResult | null>(null);
  const [consoleSyncing, setConsoleSyncing] = useState(false);

  const viewportBoxRef = useRef<HTMLDivElement>(null);
  const pointerDownRef = useRef(false);
  const lastMoveSentRef = useRef(0);

  useEffect(() => {
    setFrameFailed(false);
  }, [frameSeq]);

  useEffect(() => {
    if (embedStatus.running) {
      setMjpegLoaded(false);
      setMjpegFailed(false);
      setFrameFailed(false);
    }
  }, [embedStatus.running]);

  const liveStreamReady =
    studioMode !== "browser_place" && (embedStatus.running || studioMode === "live_mcp" || hasLiveFrame);
  const showMjpeg = liveStreamReady && streamKind === "mjpeg" && !mjpegFailed && (embedStatus.running || studioMode === "live_mcp");
  const showSnapshot = liveStreamReady && !showMjpeg && hasLiveFrame && !frameFailed;

  // If Chromium does not decode a multipart stream within 1.5s while a live PNG frame exists, fall back seamlessly to live /api/studio/frame snapshots!
  useEffect(() => {
    if (!showMjpeg || mjpegLoaded || !hasLiveFrame) return;
    const t = window.setTimeout(() => {
      setMjpegFailed(true);
    }, 1500);
    return () => window.clearTimeout(t);
  }, [hasLiveFrame, mjpegLoaded, showMjpeg]);

  const activeToolSpec = useMemo(
    () => STUDIO_TOOL_SPECS.find((t) => t.name === selectedTool) || STUDIO_TOOL_SPECS[0],
    [selectedTool],
  );

  const selectTool = (name: StudioToolName) => {
    setSelectedTool(name);
    setToolParseError(null);
    const spec = STUDIO_TOOL_SPECS.find((t) => t.name === name);
    setToolArgsDraft(JSON.stringify(spec?.defaultArgs ?? {}, null, 2));
  };

  const mapCoords = (clientX: number, clientY: number) => {
    const el = viewportBoxRef.current;
    if (!el) return { x: 0, y: 0, norm_x: 0, norm_y: 0 };
    const rect = el.getBoundingClientRect();
    const nx = Math.max(0, Math.min(1, (clientX - rect.left) / Math.max(1, rect.width)));
    const ny = Math.max(0, Math.min(1, (clientY - rect.top) / Math.max(1, rect.height)));
    return {
      x: Math.round(nx * (embedStatus.width || 1440)),
      y: Math.round(ny * (embedStatus.height || 900)),
      norm_x: Number(nx.toFixed(4)),
      norm_y: Number(ny.toFixed(4)),
      normalized: false,
      width: embedStatus.width || 1440,
      height: embedStatus.height || 900,
    };
  };

  const mouseButtonName = (b: number): "left" | "middle" | "right" =>
    b === 2 ? "right" : b === 1 ? "middle" : "left";

  const handleViewportPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    e.currentTarget.focus();
    pointerDownRef.current = true;
    const coords = mapCoords(e.clientX, e.clientY);
    const btn = mouseButtonName(e.button);
    setLastPointer({ x: coords.x, y: coords.y, action: `down (${btn})` });
    void onSendMouseInput?.({ action: "down", ...coords, button: btn });
  };

  const handleViewportPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!pointerDownRef.current) return;
    const now = performance.now();
    if (now - lastMoveSentRef.current < 65) return;
    lastMoveSentRef.current = now;
    const coords = mapCoords(e.clientX, e.clientY);
    setLastPointer({ x: coords.x, y: coords.y, action: "drag" });
    void onSendMouseInput?.({ action: "move", ...coords, button: "left" });
  };

  const handleViewportPointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!pointerDownRef.current) return;
    pointerDownRef.current = false;
    const coords = mapCoords(e.clientX, e.clientY);
    const btn = mouseButtonName(e.button);
    setLastPointer({ x: coords.x, y: coords.y, action: `click (${btn})` });
    void onSendMouseInput?.({ action: "click", ...coords, button: btn });
  };

  const handleViewportDoubleClick = (e: ReactMouseEvent<HTMLDivElement>) => {
    const coords = mapCoords(e.clientX, e.clientY);
    setLastPointer({ x: coords.x, y: coords.y, action: "double_click" });
    void onSendMouseInput?.({ action: "double_click", ...coords, button: "left" });
  };

  const handleViewportWheel = (e: ReactWheelEvent<HTMLDivElement>) => {
    if (!liveStreamReady) return;
    const coords = mapCoords(e.clientX, e.clientY);
    setLastPointer({ x: coords.x, y: coords.y, action: e.deltaY > 0 ? "wheel-down" : "wheel-up" });
    void onSendMouseInput?.({
      action: "wheel",
      ...coords,
      delta_y: Math.round(e.deltaY),
    });
  };

  const handleViewportKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (!keyboardCapture) return;
    if (e.key === "Escape") {
      setKeyboardCapture(false);
      return;
    }
    e.preventDefault();
    setLastKey(e.key);
    void onSendKeyboardInput?.({
      action: "press",
      key: e.key,
      code: e.code,
      text: e.key.length === 1 ? e.key : undefined,
      modifiers: { ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey, meta: e.metaKey },
    });
  };

  const handleLaunch = async () => {
    if (!onLaunchEmbed || embedBusy) return;
    setEmbedBusy(true);
    try {
      await onLaunchEmbed(mcpCmdDraft.trim() || undefined);
    } finally {
      setEmbedBusy(false);
    }
  };

  const handleStop = async () => {
    if (!onStopEmbed || embedBusy) return;
    setEmbedBusy(true);
    try {
      await onStopEmbed();
    } finally {
      setEmbedBusy(false);
    }
  };

  const handleCapture = async () => {
    if (!onCaptureFrame || captureBusy) return;
    setCaptureBusy(true);
    try {
      await onCaptureFrame();
    } finally {
      setCaptureBusy(false);
    }
  };

  const handleMcpConnect = async () => {
    if (!onConnectMcp || mcpBusy) return;
    setMcpBusy(true);
    try {
      await onConnectMcp(mcpCmdDraft.trim() || undefined);
    } finally {
      setMcpBusy(false);
    }
  };

  const handleMcpDisconnect = async () => {
    if (!onDisconnectMcp || mcpBusy) return;
    setMcpBusy(true);
    try {
      await onDisconnectMcp();
    } finally {
      setMcpBusy(false);
    }
  };

  const handlePlayMode = async (mode: "start_play" | "run_server" | "stop") => {
    if (!onInvokeStudioTool || playBusy) return;
    setPlayBusy(true);
    try {
      const res = await onInvokeStudioTool("start_stop_play", { mode });
      setToolResult(res);
    } finally {
      setPlayBusy(false);
    }
  };

  const handleRunSelectedTool = async () => {
    if (!onInvokeStudioTool || toolRunning) return;
    let parsed: Record<string, unknown> = {};
    try {
      const trimmed = toolArgsDraft.trim();
      if (trimmed) {
        const val = JSON.parse(trimmed);
        if (!val || typeof val !== "object" || Array.isArray(val)) {
          setToolParseError("Tool arguments must be a JSON object (e.g. {}).");
          return;
        }
        parsed = val as Record<string, unknown>;
      }
      setToolParseError(null);
    } catch {
      setToolParseError("Invalid JSON in tool arguments.");
      return;
    }
    setToolRunning(true);
    try {
      const res = await onInvokeStudioTool(selectedTool, parsed);
      setToolResult(res);
    } finally {
      setToolRunning(false);
    }
  };

  const handleSyncTree = async () => {
    if (!onInvokeStudioTool || treeSyncing) return;
    setTreeSyncing(true);
    try {
      const res = await onInvokeStudioTool("search_game_tree", {
        query: treeQuery.trim(),
        max_results: 60,
      });
      setTreeResult(res);
    } finally {
      setTreeSyncing(false);
    }
  };

  const handleInspectNode = async (nodePath: string) => {
    setInspectedPath(nodePath);
    onSelectPart?.(nodePath);
    if (!onInvokeStudioTool) return;
    const res = await onInvokeStudioTool("inspect_instance", { path: nodePath });
    setInspectedResult(res);
  };

  const handleRefreshConsole = async () => {
    if (!onInvokeStudioTool || consoleSyncing) return;
    setConsoleSyncing(true);
    try {
      const res = await onInvokeStudioTool("get_console_output", { limit: 60 });
      setConsoleResult(res);
    } finally {
      setConsoleSyncing(false);
    }
  };

  const syncedNodes = useMemo(() => {
    const all = extractTreeNodes(treeResult, place);
    const q = treeQuery.trim().toLowerCase();
    return q ? all.filter((n) => `${n.path} ${n.class_name}`.toLowerCase().includes(q)) : all;
  }, [place, treeQuery, treeResult]);

  const syncedConsoleLines = useMemo(() => extractConsoleLines(consoleResult), [consoleResult]);

  const statusPillText = embedStatus.running
    ? "Embedded Studio Live"
    : studioMode === "live_mcp"
      ? "Studio MCP Connected"
      : studioMode === "browser_place"
        ? "Browser Preview"
        : "Desktop Embed Idle";

  const playModeLabel: Record<StudioPlayMode, string> = {
    edit: "Edit Mode",
    play: "Solo Play",
    server: "Server Run",
  };

  return (
    <div
      data-embedded-studio-panel
      data-studio-feed-panel
      className="flex min-w-0 flex-col gap-3 rounded-card border border-hairline bg-board/40 p-3"
    >
      {/* Top Lifecycle & Play/Server/Stop Control Bar */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <Monitor className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
          <span className="text-sm font-medium text-ink">Embedded Roblox Studio</span>
          <Pill
            className={`px-2 py-0.5 font-mono text-xs ${
              embedStatus.running || studioMode === "live_mcp"
                ? "bg-pill-active-bg text-pill-active-fg"
                : "bg-surface text-ink-2"
            }`}
          >
            <PillIndicator
              variant={embedStatus.running || studioMode === "live_mcp" ? "success" : "info"}
              pulse={embedStatus.running || embedStatus.state === "launching"}
            />
            {statusPillText}
          </Pill>
          <Pill className="bg-surface px-2 py-0.5 font-mono text-xs text-ink-2">
            {playModeLabel[embedStatus.play_mode]}
          </Pill>
          {embedStatus.display && (
            <Pill className="bg-surface px-2 py-0.5 font-mono text-xs text-ink-3">
              {embedStatus.display}
            </Pill>
          )}
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          {!embedStatus.running ? (
            onLaunchEmbed && (
              <Button
                variant="default"
                size="xs"
                disabled={embedBusy}
                aria-label="Launch and embed Roblox Studio"
                onClick={() => void handleLaunch()}
                className="max-md:min-h-11"
              >
                <Rocket className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>{embedBusy ? "Launching…" : "Launch & Embed Studio"}</span>
              </Button>
            )
          ) : (
            onStopEmbed && (
              <Button
                variant="outline"
                size="xs"
                disabled={embedBusy}
                aria-label="Stop embedded Roblox Studio"
                onClick={() => void handleStop()}
                className="bg-surface max-md:min-h-11"
              >
                <Square className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>{embedBusy ? "Stopping…" : "Stop Embed"}</span>
              </Button>
            )
          )}

          {onInvokeStudioTool && (
            <>
              <Button
                variant={embedStatus.play_mode === "play" ? "default" : "outline"}
                size="xs"
                disabled={playBusy}
                aria-label="Start Solo Play in Studio"
                onClick={() => void handlePlayMode("start_play")}
                className={embedStatus.play_mode === "play" ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
              >
                <Play className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Play</span>
              </Button>
              <Button
                variant={embedStatus.play_mode === "server" ? "default" : "outline"}
                size="xs"
                disabled={playBusy}
                aria-label="Run Server in Studio"
                onClick={() => void handlePlayMode("run_server")}
                className={embedStatus.play_mode === "server" ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
              >
                <Server className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Run Server</span>
              </Button>
              <Button
                variant="outline"
                size="xs"
                disabled={playBusy}
                aria-label="Stop Studio simulation"
                onClick={() => void handlePlayMode("stop")}
                className="bg-surface max-md:min-h-11 max-md:min-w-11"
              >
                <Square className="size-3" strokeWidth={1.5} aria-hidden />
                <span>Stop</span>
              </Button>
            </>
          )}

          <Button
            variant="outline"
            size="xs"
            disabled={captureBusy}
            onClick={() => void handleCapture()}
            aria-label="Capture live Studio frame"
            className="bg-surface max-md:min-h-11"
          >
            <Camera className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>{captureBusy ? "Capturing…" : "Capture Frame"}</span>
          </Button>
        </div>
      </div>

      {/* Live Embedded Viewport Surface (MJPEG + Snapshot + Interactive Mouse/Keyboard Pass-Through) */}
      {liveStreamReady ? (
        <div className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-ink-2">
            <div className="flex flex-wrap items-center gap-1.5">
              <SegmentedControl aria-label="Studio stream transport" size="sm">
                <SegmentedControlItem
                  data-checked={streamKind === "mjpeg" ? "" : undefined}
                  aria-pressed={streamKind === "mjpeg"}
                  onClick={() => {
                    setMjpegFailed(false);
                    setStreamKind("mjpeg");
                  }}
                  className="max-md:!h-11 max-md:px-2.5"
                >
                  Live MJPEG
                </SegmentedControlItem>
                <SegmentedControlItem
                  data-checked={streamKind === "snapshot" ? "" : undefined}
                  aria-pressed={streamKind === "snapshot"}
                  onClick={() => {
                    setFrameFailed(false);
                    setStreamKind("snapshot");
                  }}
                  className="max-md:!h-11 max-md:px-2.5"
                >
                  Frame #{frameSeq}
                </SegmentedControlItem>
              </SegmentedControl>

              <Button
                variant={keyboardCapture ? "default" : "outline"}
                size="xs"
                aria-pressed={keyboardCapture}
                aria-label="Toggle keyboard capture for Embedded Studio"
                onClick={() => {
                  setKeyboardCapture((v) => !v);
                  viewportBoxRef.current?.focus();
                }}
                className={keyboardCapture ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
              >
                <Keyboard className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>{keyboardCapture ? "Keyboard On (Esc exits)" : "Keyboard Capture"}</span>
              </Button>
            </div>

            <div className="flex flex-wrap items-center gap-2 font-mono text-xs text-ink-3">
              {lastPointer && (
                <span>
                  Mouse: {lastPointer.action} ({lastPointer.x}, {lastPointer.y})
                </span>
              )}
              {lastKey && <span>Key: {lastKey}</span>}
              {onRefreshEmbedStatus && (
                <Button
                  variant="ghost"
                  size="xs"
                  aria-label="Refresh embedded Studio status"
                  onClick={() => void onRefreshEmbedStatus()}
                  className="max-md:min-h-11 max-md:min-w-11"
                >
                  <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
                </Button>
              )}
            </div>
          </div>

          <div
            ref={viewportBoxRef}
            tabIndex={0}
            data-embedded-viewport-surface
            aria-label="Embedded Roblox Studio interactive viewport — click, drag, or scroll to send mouse input; enable Keyboard Capture to forward keystrokes"
            onPointerDown={handleViewportPointerDown}
            onPointerMove={handleViewportPointerMove}
            onPointerUp={handleViewportPointerUp}
            onDoubleClick={handleViewportDoubleClick}
            onContextMenu={(e) => e.preventDefault()}
            onWheel={handleViewportWheel}
            onKeyDown={handleViewportKeyDown}
            className={`relative min-h-[220px] overflow-hidden rounded-sm border bg-surface outline-none transition-[border-color] duration-(--dur-press) ease-(--ease-house) ${
              keyboardCapture ? "border-blue ring-2 ring-blue/30" : "border-hairline"
            }`}
          >
            {showMjpeg ? (
              <img
                src={embedStatus.stream_url || "/api/studio/stream.mjpg"}
                alt="Live embedded Roblox Studio MJPEG viewport"
                draggable={false}
                onLoad={() => setMjpegLoaded(true)}
                onError={() => setMjpegFailed(true)}
                className="block max-h-[420px] w-full select-none object-contain"
              />
            ) : showSnapshot ? (
              <img
                src={`/api/studio/frame?seq=${frameSeq}`}
                alt={`Live Roblox Studio viewport frame ${frameSeq}`}
                draggable={false}
                onError={() => setFrameFailed(true)}
                className="block max-h-[420px] w-full select-none object-contain"
              />
            ) : (
              <div className="flex flex-col items-center justify-center gap-2 px-6 py-12 text-center text-sm text-ink-2">
                <p className="font-medium text-ink">Waiting for first live frame from Roblox Studio…</p>
                <p className="max-w-md text-xs text-ink-2">
                  Click Capture Frame above or run any build/playtest action to stream live PNG frames from Studio MCP.
                </p>
              </div>
            )}

            {actions.length > 0 && (
              <div className="flex flex-wrap items-center justify-between gap-2 border-t border-hairline bg-board/80 px-3 py-1.5 text-xs">
                <span className="truncate font-mono text-ink">
                  Latest action: {actions[actions.length - 1].summary}
                </span>
                <span className="font-mono tabular-nums text-ink-2">seq #{actions[actions.length - 1].seq}</span>
              </div>
            )}
          </div>
        </div>
      ) : (
        <div
          data-embedded-studio-notice
          className="flex flex-col gap-2 rounded-sm border border-hairline bg-surface p-4 text-sm text-ink-2"
        >
          <p className="font-medium text-ink">
            {studioMode === "browser_place"
              ? "Live local Roblox Studio desktop embedding requires the OpenRO desktop app."
              : "Embedded Roblox Studio is ready to launch on demand."}
          </p>
          <p className="text-xs leading-relaxed text-ink-2">
            {studioMode === "browser_place"
              ? "In this standalone browser build, you have the full interactive Studio 3D Co-Building IDE, Playable Roblox World sandbox, Market Taste Craft Bar, Tool & Skill Library, and all 13 Studio Tools running live against your in-browser Place DataModel below. Download the OpenRO v0.9.0 desktop app (Linux or Windows) to launch and embed live Roblox Studio directly inside OpenRO."
              : "Click Launch & Embed Studio above to start an isolated Roblox Studio session inside OpenRO with live MJPEG streaming and mouse/keyboard pass-through, or attach an existing Studio MCP bridge below. OpenRO never launches Roblox Studio automatically on startup."}
          </p>
          {embedStatus.error && (
            <p className="rounded-sm border border-hairline bg-board px-2.5 py-1.5 font-mono text-xs text-pill-pro-fg">
              {embedStatus.error}
            </p>
          )}
        </div>
      )}

      {/* Dynamic Studio MCP Bridge Connect / Disconnect Bar */}
      {(onConnectMcp || onDisconnectMcp) && (
        <div
          data-studio-mcp-controls
          className="flex flex-col gap-2 rounded-sm border border-hairline bg-surface p-3 sm:flex-row sm:items-center"
        >
          <Input
            value={mcpCmdDraft}
            onChange={(e) => setMcpCmdDraft(e.target.value)}
            aria-label="Studio MCP bridge command"
            placeholder="MCP command (optional, e.g. ~/.local/bin/roblox-studio-mcp)"
            disabled={mcpBusy || studioMode === "live_mcp"}
            className="min-w-0 flex-1 font-mono text-xs max-md:min-h-11"
          />
          <div className="flex shrink-0 flex-wrap items-center gap-1.5">
            {studioMode === "live_mcp" && onDisconnectMcp ? (
              <Button
                variant="outline"
                size="xs"
                disabled={mcpBusy}
                aria-label="Disconnect Studio MCP bridge"
                onClick={() => void handleMcpDisconnect()}
                className="bg-surface max-md:min-h-11"
              >
                <Unplug className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>{mcpBusy ? "Disconnecting…" : "Disconnect MCP"}</span>
              </Button>
            ) : (
              onConnectMcp && (
                <Button
                  variant="outline"
                  size="xs"
                  disabled={mcpBusy}
                  aria-label="Connect Studio MCP bridge"
                  onClick={() => void handleMcpConnect()}
                  className="bg-surface max-md:min-h-11"
                >
                  <Plug className="size-3.5" strokeWidth={1.5} aria-hidden />
                  <span>{mcpBusy ? "Connecting…" : "Connect MCP"}</span>
                </Button>
              )
            )}
          </div>
        </div>
      )}

      {/* All-12 Studio Tools Drawer + Live Studio Explorer Sync + Studio Output Console */}
      {onInvokeStudioTool && (
        <div className="flex flex-col gap-2.5 rounded-sm border border-hairline bg-surface p-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <SegmentedControl aria-label="Embedded Studio workbench tools" size="sm" className="max-w-full flex-wrap">
              <SegmentedControlItem
                data-checked={subTab === "tools" ? "" : undefined}
                aria-pressed={subTab === "tools"}
                onClick={() => setSubTab("tools")}
                className="gap-1.5 max-md:!h-11 max-md:px-2.5"
              >
                <Wrench className="size-3.5" strokeWidth={1.5} aria-hidden />
                All-12 Studio Tools
              </SegmentedControlItem>
              <SegmentedControlItem
                data-checked={subTab === "explorer" ? "" : undefined}
                aria-pressed={subTab === "explorer"}
                onClick={() => setSubTab("explorer")}
                className="gap-1.5 max-md:!h-11 max-md:px-2.5"
              >
                <FolderTree className="size-3.5" strokeWidth={1.5} aria-hidden />
                Explorer Sync
              </SegmentedControlItem>
              <SegmentedControlItem
                data-checked={subTab === "console" ? "" : undefined}
                aria-pressed={subTab === "console"}
                onClick={() => setSubTab("console")}
                className="gap-1.5 max-md:!h-11 max-md:px-2.5"
              >
                <Terminal className="size-3.5" strokeWidth={1.5} aria-hidden />
                Studio Output
              </SegmentedControlItem>
            </SegmentedControl>

            <Pill className="bg-board px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2">
              {STUDIO_TOOL_SPECS.length} MCP tools
            </Pill>
          </div>

          {subTab === "tools" && (
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
                      onClick={() => selectTool(spec.name)}
                      className={active ? "font-mono text-xs max-md:min-h-11" : "bg-board/50 font-mono text-xs max-md:min-h-11"}
                    >
                      {spec.name}
                    </Button>
                  );
                })}
              </div>

              <div className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-board/40 px-2.5 py-2 text-xs">
                <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
                  <Badge className="border-transparent bg-tag-sky-bg px-2 py-0.5 text-xs font-medium text-tag-sky-fg">
                    {activeToolSpec.category}
                  </Badge>
                  <span className="font-mono font-medium text-ink">{activeToolSpec.name}</span>
                  <span className="text-ink-2">{activeToolSpec.summary}</span>
                </div>
                <Button
                  variant="default"
                  size="xs"
                  disabled={toolRunning}
                  aria-label={`Run Studio tool ${activeToolSpec.name}`}
                  onClick={() => void handleRunSelectedTool()}
                  className="shrink-0 max-md:min-h-11"
                >
                  <Play className="size-3" strokeWidth={1.5} aria-hidden />
                  {toolRunning ? "Running…" : "Run Tool"}
                </Button>
              </div>

              <Textarea
                value={toolArgsDraft}
                onChange={(e) => setToolArgsDraft(e.target.value)}
                aria-label={`JSON arguments for ${activeToolSpec.name}`}
                spellCheck={false}
                rows={3}
                className="bg-board/35 font-mono text-xs [&_textarea]:min-h-16 [&_textarea]:font-mono [&_textarea]:text-xs"
              />

              {toolParseError && (
                <p className="text-xs font-medium text-pill-pro-fg">{toolParseError}</p>
              )}

              {toolResult && (
                <div
                  data-studio-tool-result
                  className="flex flex-col gap-1.5 rounded-sm border border-hairline bg-board/45 p-2.5"
                >
                  <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
                    <div className="flex items-center gap-2">
                      <Pill
                        className={`px-2 py-0.5 font-mono text-xs ${
                          toolResult.ok
                            ? "bg-pill-active-bg text-pill-active-fg"
                            : "bg-pill-pro-bg text-pill-pro-fg"
                        }`}
                      >
                        {toolResult.ok ? "OK" : "Error"}
                      </Pill>
                      <span className="font-mono font-medium text-ink">{toolResult.tool}</span>
                    </div>
                    {typeof toolResult.duration_ms === "number" && (
                      <span className="font-mono tabular-nums text-ink-3">{toolResult.duration_ms}ms</span>
                    )}
                  </div>
                  <pre className="max-h-44 overflow-auto whitespace-pre-wrap break-words rounded-sm border border-hairline bg-surface p-2 font-mono text-xs text-ink">
                    {toolResult.error
                      ? toolResult.error
                      : toolResult.result !== undefined
                        ? JSON.stringify(toolResult.result, null, 2)
                        : toolResult.text || "Tool completed cleanly."}
                  </pre>
                </div>
              )}
            </div>
          )}

          {subTab === "explorer" && (
            <div data-studio-explorer-sync className="flex flex-col gap-2.5">
              <div className="flex flex-wrap items-center gap-2">
                <Input
                  value={treeQuery}
                  onChange={(e) => setTreeQuery(e.target.value)}
                  aria-label="Search live Studio DataModel tree"
                  placeholder="Filter Studio DataModel (e.g. Spawn, Card, Workspace)…"
                  className="min-w-0 flex-1 text-xs max-md:min-h-11"
                />
                <Button
                  variant="outline"
                  size="xs"
                  disabled={treeSyncing}
                  aria-label="Sync live Studio Explorer tree"
                  onClick={() => void handleSyncTree()}
                  className="bg-board/50 max-md:min-h-11"
                >
                  <Search className="size-3.5" strokeWidth={1.5} aria-hidden />
                  <span>{treeSyncing ? "Syncing…" : "Sync Tree"}</span>
                </Button>
              </div>

              <div className="grid gap-2 md:grid-cols-2">
                <div className="max-h-48 overflow-y-auto rounded-sm border border-hairline bg-board/35 p-1.5">
                  {syncedNodes.length === 0 ? (
                    <p className="p-2 text-xs text-ink-2">No matching instances found. Click Sync Tree to query Studio.</p>
                  ) : (
                    <ul className="flex flex-col gap-1">
                      {syncedNodes.slice(0, 40).map((node) => (
                        <li key={node.path}>
                          <Button
                            variant={inspectedPath === node.path ? "default" : "ghost"}
                            size="xs"
                            onClick={() => void handleInspectNode(node.path)}
                            className="h-auto w-full justify-between gap-2 rounded-sm px-2 py-1 text-left font-normal max-md:min-h-11"
                          >
                            <span className="min-w-0 truncate font-mono text-xs">{node.path}</span>
                            <span className="shrink-0 font-mono text-xs opacity-75">{node.class_name}</span>
                          </Button>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>

                <div className="max-h-48 overflow-auto rounded-sm border border-hairline bg-board/35 p-2">
                  <div className="mb-1 text-xs font-medium text-ink">
                    {inspectedPath ? `Inspect: ${inspectedPath}` : "Select any instance to run inspect_instance"}
                  </div>
                  <pre className="whitespace-pre-wrap break-words font-mono text-xs text-ink-2">
                    {inspectedResult
                      ? JSON.stringify(inspectedResult.result ?? inspectedResult.text ?? inspectedResult, null, 2)
                      : "Click a DataModel node on the left to inspect its live Roblox properties."}
                  </pre>
                </div>
              </div>
            </div>
          )}

          {subTab === "console" && (
            <div data-studio-console-sync className="flex flex-col gap-2">
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs text-ink-2">
                  Live Roblox Studio Output retrieved via <code className="font-mono text-ink">get_console_output</code>
                </span>
                <Button
                  variant="outline"
                  size="xs"
                  disabled={consoleSyncing}
                  aria-label="Fetch Studio console output"
                  onClick={() => void handleRefreshConsole()}
                  className="bg-board/50 max-md:min-h-11"
                >
                  <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
                  <span>{consoleSyncing ? "Fetching…" : "Refresh Output"}</span>
                </Button>
              </div>
              <div className="max-h-48 overflow-y-auto rounded-sm border border-hairline bg-board/35 p-2 font-mono text-xs">
                {syncedConsoleLines.length === 0 ? (
                  <p className="text-ink-2">
                    Click Refresh Output (or run Play / Execute Luau) to pull live Roblox Studio console logs.
                  </p>
                ) : (
                  <ul className="flex flex-col gap-1">
                    {syncedConsoleLines.map((line, i) => (
                      <li
                        key={i}
                        className={
                          line.level === "error"
                            ? "text-pill-pro-fg"
                            : line.level === "warn"
                              ? "text-pill-warn-fg"
                              : "text-ink"
                        }
                      >
                        {line.timestamp ? `[${line.timestamp}] ` : ""}
                        {line.text}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
