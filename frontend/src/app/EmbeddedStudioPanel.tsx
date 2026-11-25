import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type WheelEvent as ReactWheelEvent,
} from "react";
import * as Popover from "@radix-ui/react-popover";
import {
  Box,
  Camera,
  Keyboard,
  Monitor,
  MoreHorizontal,
  Play,
  Plug,
  RefreshCw,
  Rocket,
  Square,
  Unplug,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill, PillIndicator } from "@/components/kibo-ui/pill";
import {
  type StudioActionEvent,
  type StudioEmbedStatus,
  type StudioKeyboardInputPayload,
  type StudioMode,
  type StudioMouseInputPayload,
  type StudioPlayMode,
  type StudioToolName,
  type StudioToolResult,
} from "./data";

export function EmbeddedStudioPanel({
  studioMode,
  embedStatus,
  hasLiveFrame,
  frameSeq,
  actions,
  onLaunchEmbed,
  onAttachEmbed,
  onTransferEmbed,
  onStopEmbed,
  onRefreshEmbedStatus,
  onCaptureFrame,
  onConnectMcp,
  onDisconnectMcp,
  onSendMouseInput,
  onSendKeyboardInput,
  onInvokeStudioTool,
  onOpen3DPreview,
}: {
  studioMode: StudioMode;
  embedStatus: StudioEmbedStatus;
  hasLiveFrame: boolean;
  frameSeq: number;
  actions: StudioActionEvent[];
  onLaunchEmbed?: (command?: string) => Promise<void> | void;
  onAttachEmbed?: () => Promise<void> | void;
  onTransferEmbed?: () => Promise<void> | void;
  onStopEmbed?: () => Promise<void> | void;
  onRefreshEmbedStatus?: () => Promise<void> | void;
  onCaptureFrame?: () => Promise<void> | void;
  onConnectMcp?: (command?: string) => Promise<void> | void;
  onDisconnectMcp?: () => Promise<void> | void;
  onSendMouseInput?: (payload: StudioMouseInputPayload) => Promise<void> | void;
  onSendKeyboardInput?: (payload: StudioKeyboardInputPayload) => Promise<void> | void;
  onInvokeStudioTool?: (tool: StudioToolName, args: Record<string, unknown>) => Promise<StudioToolResult>;
  onOpen3DPreview?: () => void;
}) {
  const viewportBoxRef = useRef<HTMLDivElement>(null);
  const draggingRef = useRef(false);
  const lastMoveSentRef = useRef(0);

  const [embedBusy, setEmbedBusy] = useState(false);
  const [playBusy, setPlayBusy] = useState(false);
  const [captureBusy, setCaptureBusy] = useState(false);
  const [mcpBusy, setMcpBusy] = useState(false);
  const [mcpOpen, setMcpOpen] = useState(false);
  const [mcpCmdDraft, setMcpCmdDraft] = useState("");
  const [streamKind, setStreamKind] = useState<"mjpeg" | "snapshot">("mjpeg");
  const [mjpegLoaded, setMjpegLoaded] = useState(false);
  const [mjpegFailed, setMjpegFailed] = useState(false);
  const [frameFailed, setFrameFailed] = useState(false);
  const [keyboardCapture, setKeyboardCapture] = useState(false);
  const [lastPointer, setLastPointer] = useState<{ x: number; y: number; action: string } | null>(null);
  const [lastKey, setLastKey] = useState<string | null>(null);

  useEffect(() => {
    setFrameFailed(false);
  }, [frameSeq]);

  useEffect(() => {
    if (embedStatus.running) {
      setMjpegLoaded(false);
      setMjpegFailed(false);
      setStreamKind("mjpeg");
    }
  }, [embedStatus.running]);

  useEffect(() => {
    if (!embedStatus.running || streamKind !== "mjpeg" || mjpegLoaded || mjpegFailed) return;
    const t = window.setTimeout(() => {
      setMjpegFailed(true);
    }, 1500);
    return () => window.clearTimeout(t);
  }, [embedStatus.running, mjpegFailed, mjpegLoaded, streamKind]);

  const computeCoords = (clientX: number, clientY: number) => {
    const el = viewportBoxRef.current;
    if (!el) return { x: 0, y: 0, norm_x: 0, norm_y: 0, width: embedStatus.width, height: embedStatus.height };
    const rect = el.getBoundingClientRect();
    const normX = Math.max(0, Math.min(1, (clientX - rect.left) / Math.max(1, rect.width)));
    const normY = Math.max(0, Math.min(1, (clientY - rect.top) / Math.max(1, rect.height)));
    const x = Math.round(normX * (embedStatus.width || 1440));
    const y = Math.round(normY * (embedStatus.height || 900));
    return { x, y, norm_x: Number(normX.toFixed(4)), norm_y: Number(normY.toFixed(4)), width: embedStatus.width || 1440, height: embedStatus.height || 900 };
  };

  const buttonName = (b: number): "left" | "middle" | "right" =>
    b === 2 ? "right" : b === 1 ? "middle" : "left";

  const sendMouse = (action: StudioMouseInputPayload["action"], clientX: number, clientY: number, btn: number, deltaY?: number) => {
    if (!onSendMouseInput) return;
    const coords = computeCoords(clientX, clientY);
    setLastPointer({ x: coords.x, y: coords.y, action });
    void onSendMouseInput({
      action,
      ...coords,
      normalized: true,
      button: buttonName(btn),
      ...(typeof deltaY === "number" ? { delta_y: deltaY } : {}),
    });
  };

  const handleViewportPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    viewportBoxRef.current?.focus();
    draggingRef.current = true;
    const act = e.button === 2 ? "right_click" : "click";
    sendMouse(act, e.clientX, e.clientY, e.button);
  };

  const handleViewportPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!draggingRef.current) return;
    const now = performance.now();
    if (now - lastMoveSentRef.current < 90) return;
    lastMoveSentRef.current = now;
    sendMouse("drag", e.clientX, e.clientY, e.buttons === 2 ? 2 : 0);
  };

  const handleViewportPointerUp = () => {
    draggingRef.current = false;
  };

  const handleViewportDoubleClick = (e: ReactMouseEvent<HTMLDivElement>) => {
    sendMouse("double_click", e.clientX, e.clientY, e.button);
  };

  const handleViewportWheel = (e: ReactWheelEvent<HTMLDivElement>) => {
    if (!embedStatus.running && !hasLiveFrame) return;
    const now = performance.now();
    if (now - lastMoveSentRef.current < 80) return;
    lastMoveSentRef.current = now;
    sendMouse("wheel", e.clientX, e.clientY, 0, Math.round(e.deltaY));
  };

  const handleViewportKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (!keyboardCapture) return;
    if (e.key === "Escape") {
      e.preventDefault();
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
    if (embedBusy) return;
    setEmbedBusy(true);
    try {
      if (onTransferEmbed) {
        await onTransferEmbed();
      } else if ((embedStatus.already_logged_in || embedStatus.studio_running) && onAttachEmbed) {
        await onAttachEmbed();
      } else if (onLaunchEmbed) {
        await onLaunchEmbed(mcpCmdDraft.trim() || undefined);
      }
    } finally {
      setEmbedBusy(false);
    }
  };

  const handleGrab = async () => {
    if (embedBusy || !onAttachEmbed) return;
    setEmbedBusy(true);
    try {
      await onAttachEmbed();
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

  const handlePlayMode = async (mode: "start_play" | "run_server" | "stop") => {
    if (!onInvokeStudioTool || playBusy) return;
    setPlayBusy(true);
    try {
      await onInvokeStudioTool("start_stop_play", { mode });
      await onRefreshEmbedStatus?.();
    } finally {
      setPlayBusy(false);
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
      setMcpOpen(false);
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

  const liveStreamReady = embedStatus.running || (studioMode === "live_mcp" && hasLiveFrame);
  const showMjpeg = embedStatus.running && streamKind === "mjpeg" && !mjpegFailed;
  const showSnapshot = !showMjpeg && (hasLiveFrame || embedStatus.running) && !frameFailed;
  const playLabel: Record<StudioPlayMode, string> = { edit: "Edit", play: "Playing", server: "Server" };
  const awaitingSignIn = embedStatus.phase === "awaiting_signin";

  return (
    <div data-embedded-studio-panel className="flex h-full min-h-0 flex-col bg-surface">
      {/* Slim Studio Bar */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-hairline bg-board/55 px-3 py-1.5">
        <div className="flex flex-wrap items-center gap-1.5">
          <Pill
            className={`px-2 py-0.5 text-xs font-medium ${
              embedStatus.running
                ? "bg-pill-active-bg text-pill-active-fg"
                : studioMode === "live_mcp"
                  ? "bg-tag-sky-bg text-tag-sky-fg"
                  : "bg-surface text-ink-2"
            }`}
          >
            <PillIndicator variant={embedStatus.running || studioMode === "live_mcp" ? "success" : "info"} pulse={embedStatus.running} />
            <span>
              {awaitingSignIn
                ? "Sign in"
                : embedStatus.running
                  ? "Live"
                  : studioMode === "live_mcp"
                    ? "Connected"
                    : studioMode === "browser_place"
                      ? "Browser"
                      : "Standby"}
            </span>
          </Pill>

          <Badge className="border-transparent bg-surface px-1.5 py-0.5 font-mono text-xs text-ink-2">
            {embedStatus.place_open === false ? "Start Page" : embedStatus.datamodel_type || playLabel[embedStatus.play_mode]}
          </Badge>

          {lastPointer && (
            <span className="hidden font-mono text-xs text-ink-3 xl:inline">
              {lastPointer.action} ({lastPointer.x},{lastPointer.y})
            </span>
          )}
          {lastKey && (
            <span className="hidden text-xs text-ink-3 xl:inline">
              Key <span className="font-mono">{lastKey}</span>
            </span>
          )}
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          {onInvokeStudioTool && (
            embedStatus.play_mode === "play" || embedStatus.play_mode === "server" ? (
              <Button
                variant="default"
                size="xs"
                disabled={playBusy}
                aria-label="Stop playtest"
                onClick={() => void handlePlayMode("stop")}
                className="max-md:min-h-11"
              >
                <Square className="size-3" strokeWidth={1.5} aria-hidden />
                <span>Stop</span>
              </Button>
            ) : (
              <Button
                variant="outline"
                size="xs"
                disabled={playBusy}
                aria-label="Play in Studio"
                onClick={() => void handlePlayMode("start_play")}
                className="bg-surface max-md:min-h-11"
              >
                <Play className="size-3" strokeWidth={1.5} aria-hidden />
                <span>Play</span>
              </Button>
            )
          )}

          {studioMode !== "browser_place" && embedStatus.running && (
            <Button
              variant="outline"
              size="xs"
              disabled={embedBusy}
              aria-label="Detach Studio"
              onClick={() => void handleStop()}
              className="bg-surface max-md:min-h-11"
            >
              <Square className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>{embedBusy ? "Stopping…" : "Stop"}</span>
            </Button>
          )}

          {onOpen3DPreview && (
            <Button
              variant="ghost"
              size="xs"
              onClick={onOpen3DPreview}
              aria-label="Open 3D Preview"
              className="text-ink-2 hover:text-ink max-md:min-h-11"
            >
              <Box className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>3D Preview</span>
            </Button>
          )}

          <Popover.Root>
            <Popover.Trigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Studio controls"
                className="shrink-0 text-ink-2 hover:text-ink max-md:size-11"
              >
                <MoreHorizontal className="size-4" strokeWidth={1.5} aria-hidden />
              </Button>
            </Popover.Trigger>
            <Popover.Portal>
              <Popover.Content
                align="end"
                sideOffset={6}
                className="z-50 flex w-48 flex-col gap-1 rounded-card border border-hairline bg-surface p-1.5 shadow-lift"
              >
                {onCaptureFrame && (
                  <Button
                    variant="ghost"
                    size="xs"
                    disabled={captureBusy}
                    onClick={() => void handleCapture()}
                    className="w-full justify-start gap-2"
                  >
                    <Camera className="size-3.5" strokeWidth={1.5} aria-hidden />
                    <span>{captureBusy ? "Capturing…" : "Capture frame"}</span>
                  </Button>
                )}
                {liveStreamReady && (
                  <Button
                    variant={keyboardCapture ? "secondary" : "ghost"}
                    size="xs"
                    onClick={() => {
                      setKeyboardCapture((v) => !v);
                      viewportBoxRef.current?.focus();
                    }}
                    className="w-full justify-start gap-2"
                  >
                    <Keyboard className="size-3.5" strokeWidth={1.5} aria-hidden />
                    <span>{keyboardCapture ? "Keys On (Esc)" : "Keyboard Focus"}</span>
                  </Button>
                )}
                {(onConnectMcp || onDisconnectMcp) && (
                  <Button
                    variant={mcpOpen ? "secondary" : "ghost"}
                    size="xs"
                    onClick={() => setMcpOpen((v) => !v)}
                    className="w-full justify-start gap-2"
                  >
                    <Plug className="size-3.5" strokeWidth={1.5} aria-hidden />
                    <span>Studio Bridge</span>
                  </Button>
                )}
                {onRefreshEmbedStatus && (
                  <Button
                    variant="ghost"
                    size="xs"
                    onClick={() => void onRefreshEmbedStatus()}
                    className="w-full justify-start gap-2"
                  >
                    <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
                    <span>Refresh status</span>
                  </Button>
                )}
              </Popover.Content>
            </Popover.Portal>
          </Popover.Root>
        </div>
      </div>

      {/* Compact Collapsible MCP Bridge Popover Strip */}
      {mcpOpen && (onConnectMcp || onDisconnectMcp) && (
        <div
          data-studio-mcp-controls
          className="flex flex-col gap-2 border-b border-hairline bg-board/40 px-3 py-2 sm:flex-row sm:items-center"
        >
          <Input
            value={mcpCmdDraft}
            onChange={(e) => setMcpCmdDraft(e.target.value)}
            aria-label="Studio MCP bridge command"
            placeholder="MCP command (optional, e.g. ~/.local/bin/roblox-studio-mcp)"
            disabled={mcpBusy || studioMode === "live_mcp"}
            className="h-8 min-w-0 flex-1 bg-surface font-mono text-xs max-md:min-h-11"
          />
          <div className="flex shrink-0 items-center gap-1.5">
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
                  variant="default"
                  size="xs"
                  disabled={mcpBusy}
                  aria-label="Connect Studio MCP bridge"
                  onClick={() => void handleMcpConnect()}
                  className="max-md:min-h-11"
                >
                  <Plug className="size-3.5" strokeWidth={1.5} aria-hidden />
                  <span>{mcpBusy ? "Connecting…" : "Connect MCP"}</span>
                </Button>
              )
            )}
          </div>
        </div>
      )}

      {/* Full-Height Unobstructed Embedded Studio Viewport */}
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center overflow-hidden bg-board/35 p-3">
        {liveStreamReady ? (
          <div
            ref={viewportBoxRef}
            tabIndex={0}
            data-embedded-viewport-surface
            aria-label="Embedded Roblox Studio interactive viewport — click, drag, or scroll to send mouse input; enable Keyboard Focus to forward keystrokes"
            onPointerDown={handleViewportPointerDown}
            onPointerMove={handleViewportPointerMove}
            onPointerUp={handleViewportPointerUp}
            onDoubleClick={handleViewportDoubleClick}
            onContextMenu={(e) => e.preventDefault()}
            onWheel={handleViewportWheel}
            onKeyDown={handleViewportKeyDown}
            className={`relative flex h-full w-full min-h-[240px] flex-col overflow-hidden rounded-card border bg-surface outline-none motion-safe:transition-[border-color] duration-(--dur-press) ease-(--ease-house) ${
              keyboardCapture ? "border-blue ring-2 ring-blue/30" : "border-hairline"
            }`}
          >
            {awaitingSignIn && (
              <div className="flex items-center justify-center border-b border-hairline bg-pill-warn-bg px-3 py-1.5 text-xs font-medium text-pill-warn-fg">
                Sign in above: OpenRO connects automatically
              </div>
            )}
            <div className="flex min-h-0 flex-1 items-center justify-center overflow-hidden bg-board/50">
              {showMjpeg ? (
                <img
                  src={embedStatus.stream_url || "/api/studio/stream.mjpg"}
                  alt="Live Roblox Studio viewport"
                  draggable={false}
                  onLoad={() => setMjpegLoaded(true)}
                  onError={() => setMjpegFailed(true)}
                  className="h-full w-full select-none object-contain"
                />
              ) : showSnapshot ? (
                <img
                  src={`/api/studio/frame?seq=${frameSeq}`}
                  alt={`Live Roblox Studio viewport frame ${frameSeq}`}
                  draggable={false}
                  onError={() => setFrameFailed(true)}
                  className="h-full w-full select-none object-contain"
                />
              ) : (
                <div className="flex flex-col items-center justify-center gap-2 px-6 py-12 text-center text-sm text-ink-2">
                  <p className="font-medium text-ink">Waiting for Studio frame…</p>
                </div>
              )}
            </div>

            {actions.length > 0 && (
              <div className="flex flex-none flex-wrap items-center justify-between gap-2 border-t border-hairline bg-surface px-3 py-1 text-xs">
                <span className="truncate font-mono text-ink">
                  {actions[actions.length - 1].summary}
                </span>
                <span className="font-mono tabular-nums text-ink-2">#{actions[actions.length - 1].seq}</span>
              </div>
            )}
          </div>
        ) : (
          <div
            data-embedded-studio-notice
            className="flex max-w-md flex-col items-center gap-3 rounded-card border border-hairline bg-surface p-6 text-center"
          >
            <span className="grid size-11 place-items-center rounded-sm border border-hairline bg-board text-icon">
              <Monitor className="size-5" strokeWidth={1.5} aria-hidden />
            </span>
            <div className="flex flex-col gap-1">
              <h3 className="text-sm font-medium text-ink">
                {studioMode === "browser_place"
                  ? "Browser mode"
                  : embedStatus.place_open === false
                    ? "Studio is on the Start Page"
                    : embedStatus.already_logged_in
                      ? "Found your logged-in Roblox Studio"
                      : "Connect Roblox Studio"}
              </h3>
              <p className="text-xs leading-relaxed text-ink-2">
                {studioMode === "browser_place"
                  ? "Use 3D Preview, Code, or Ship in browser mode."
                  : embedStatus.place_open === false
                    ? embedStatus.guidance || "Open your project directly into Edit mode."
                    : embedStatus.already_logged_in
                      ? `${embedStatus.running_studios?.[0]?.place_name || "Your open Studio session"} is ready.`
                      : "Opens your project directly in embedded Roblox Studio."}
              </p>
            </div>
            <div className="flex flex-wrap items-center justify-center gap-2">
              {studioMode !== "browser_place" && (onTransferEmbed || onAttachEmbed || onLaunchEmbed) && (
                <Button variant="default" size="sm" disabled={embedBusy} onClick={() => void handleLaunch()} className="max-md:min-h-11">
                  <Rocket className="size-3.5" strokeWidth={1.5} aria-hidden />
                  {embedBusy
                    ? "Opening…"
                    : onTransferEmbed
                      ? "Embed & Open Project in Studio"
                      : embedStatus.already_logged_in || embedStatus.studio_running
                        ? "Grab & Connect"
                        : "Open My Roblox Studio"}
                </Button>
              )}
              {studioMode !== "browser_place" && onTransferEmbed && onAttachEmbed && (embedStatus.already_logged_in || embedStatus.studio_running) && embedStatus.place_open !== false && (
                <Button variant="outline" size="sm" disabled={embedBusy} onClick={() => void handleGrab()} className="max-md:min-h-11">
                  Grab &amp; Connect
                </Button>
              )}
              {onOpen3DPreview && (
                <Button variant="outline" size="sm" onClick={onOpen3DPreview} className="max-md:min-h-11">
                  <Box className="size-3.5" strokeWidth={1.5} aria-hidden />
                  3D Preview
                </Button>
              )}
            </div>
            {embedStatus.error && (
              <p className="w-full rounded-sm border border-hairline bg-board px-2.5 py-1.5 font-mono text-xs text-pill-pro-fg">
                {embedStatus.error}
              </p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
