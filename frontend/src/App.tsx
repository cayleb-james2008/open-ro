import {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type DragEvent,
  type FormEvent,
  type MouseEvent,
} from "react";
import * as Popover from "@radix-ui/react-popover";
import { AnimatePresence, LayoutGroup, LazyMotion, MotionConfig, domMax, m, useReducedMotion } from "motion/react";
import { Toaster, toast } from "sonner";
import { Drawer } from "vaul";
import {
  ArrowLeftRight,
  Check,
  Coins,
  Command,
  Download,
  ExternalLink,
  FileDiff,
  FolderArchive,
  FolderTree,
  FolderUp,
  FolderKanban,
  Hammer,
  Images,
  Layers,
  LayoutTemplate,
  Menu,
  MessageSquare,
  Monitor,
  MoreHorizontal,
  Package,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Play,
  Plus,
  RefreshCw,
  Search,
  Ship as ShipIcon,
  SlidersHorizontal,
  Sparkles,
  Upload,
  Users,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Kbd } from "@/components/ui/kbd";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogDescription,
  MorphingDialogTitle,
} from "@/components/ui/morphing-dialog";
import { Pill } from "@/components/kibo-ui/pill";
import { glide, instant, open as dialogSpring } from "@/motion";
import { OpenRoLogo } from "./app/OpenRoLogo";
import {
  RELEASES_URL,
  SEND_FAILED,
  computeLocalScriptDiffs,
  defaultPluginStatus,
  isV0100OrNewer,
  isV050OrNewer,
  isV060OrNewer,
  isV070OrNewer,
  isV080OrNewer,
  isV090OrNewer,
  normalizeAssetList,
  normalizeChatList,
  normalizeEmbedStatus,
  normalizeFileDiffs,
  normalizeIdeas,
  normalizeLibrarySnapshot,
  normalizePlace,
  normalizePluginStatus,
  normalizeProposal,
  normalizeProposalList,
  normalizeSnapshots,
  normalizeStudioActionsResponse,
  normalizeThread,
  post,
  read,
  subscribeBuildStream,
  subscribeStudioStream,
  type BuildStreamPhase,
  type Chat,
  type ChatList,
  type ComposerMode,
  type Idea,
  type InstanceEditPayload,
  type LibrarySnapshot,
  type PartPreset,
  type PendingProposal,
  type Place,
  type PlaceDiff,
  type PlaceSnapshot,
  type ProjectAssetRecord,
  type ReasoningLevel,
  type ScriptFileDiff,
  type StudioActionEvent,
  type StudioEmbedStatus,
  type StudioKeyboardInputPayload,
  type StudioMode,
  type StudioMouseInputPayload,
  type StudioPluginStatus,
  type StudioToolName,
  type StudioToolResult,
  type WorkspaceMode,
} from "./app/data";
import { CopilotComposer } from "./app/CopilotComposer";
import { analyzePlaceLuau, computePlaceDiff, type LuauDiagnostic } from "./app/luauAudit";
import { useShip } from "./app/ship/useShip";
import { ProjectsRail } from "./app/ship/ProjectsRail";
import {
  bytesToBase64,
  call as apiCall,
  fetchProjectWorkspace,
  importProjectModel,
  inferPrefabFormat,
  mutateProjectScene,
  projUrl,
  writeProjectFile,
  type CreatorHubSetupResult,
  type WorkspaceBundle,
  type WorkspaceDiagnostic,
  type WorkspaceLighting,
  type WorkspaceMeshEntry,
} from "./app/ship/api";
import { SwarmSidebarFeed } from "./app/SwarmSidebarFeed";
import {
  createRun as createSwarmRun,
  getRun as getSwarmRun,
  postToBoard as postSwarmBoard,
  stopRun as stopSwarmRun,
  type TeamworkSnapshot,
} from "./app/teamwork/api";
import { AgentRunsPanel } from "./app/AgentRunsPanel";
import { LeftExplorerTree, StudioChip } from "./app/Shelf";
import { StudioBottomDrawer, type BottomDrawerTab } from "./app/StudioBottomDrawer";

type ViewportTab = "embedded-studio" | "studio-3d" | "code-diff" | "gui-designer" | "assets" | "ship" | "teamwork";
import { ThreadFeed } from "./app/Thread";
import { BlueprintWorkbench } from "./app/Blueprints";
import { PanelErrorBoundary } from "./components/PanelErrorBoundary";

const SettingsDialog = lazy(() =>
  import("./app/SettingsDialog").then((m) => ({ default: m.SettingsDialog })),
);

const CodeDiffWorkbench = lazy(() =>
  import("./app/CodeDiffWorkbench").then((m) => ({ default: m.CodeDiffWorkbench })),
);
const GuiDesignerWorkbench = lazy(() =>
  import("./app/GuiDesignerWorkbench").then((m) => ({ default: m.GuiDesignerWorkbench })),
);
const CommandPalette = lazy(() =>
  import("./app/CommandPalette").then((m) => ({ default: m.CommandPalette })),
);
const EmbeddedStudioPanel = lazy(() =>
  import("./app/EmbeddedStudioPanel").then((m) => ({ default: m.EmbeddedStudioPanel })),
);
const ShipPanel = lazy(() =>
  import("./app/ship/ShipPanel").then((m) => ({ default: m.ShipPanel })),
);
const StartupWizard = lazy(() =>
  import("./app/StartupWizard").then((m) => ({ default: m.StartupWizard })),
);
const TeamworkPanel = lazy(() =>
  import("./app/teamwork/TeamworkPanel").then((m) => ({ default: m.TeamworkPanel })),
);
const SnapshotsDialog = lazy(() =>
  import("./app/SnapshotsDialog").then((m) => ({ default: m.SnapshotsDialog })),
);
const StudioPluginDialog = lazy(() =>
  import("./app/StudioPluginDialog").then((m) => ({ default: m.StudioPluginDialog })),
);
const StudioViewport3D = lazy(() =>
  import("./app/StudioViewport3D").then((m) => ({ default: m.StudioViewport3D })),
);
const SystemLibrary = lazy(() =>
  import("./app/SystemLibrary").then((m) => ({ default: m.SystemLibrary })),
);
const ToolLibraryDialog = lazy(() =>
  import("./app/ToolLibraryDialog").then((m) => ({ default: m.ToolLibraryDialog })),
);
const SyncConflictsCard = lazy(() =>
  import("./app/ship/SyncConflictsCard").then((m) => ({ default: m.SyncConflictsCard })),
);
import {
  defaultMarketTasteSnapshot,
  normalizeMarketTasteSnapshot,
  type MarketTasteSnapshot,
} from "./app/marketTaste";
import type { GameSystemDef } from "./app/systems";
import { UpdateStrip, useUpdate } from "./app/Update";
import type { AntigravityIssue } from "./app/data";

type Pending = { message?: string; build?: string; undo?: boolean };
type Notice = { text: string; bad?: boolean; user?: boolean };
type ShellWindow = Window & { openroSetTheme?: (t: string) => void; __OPENRO_WEB_ENGINE__?: boolean };

const noticeKey = (id: string) => "openro-notices:" + id;
const draftKey = (id: string) => "openro-draft:" + id;

function slotRead<T>(key: string, fallback: T): T {
  try {
    const v = sessionStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

function slotWrite(key: string, value: unknown) {
  try {
    if (value === "" || (Array.isArray(value) && !value.length)) sessionStorage.removeItem(key);
    else sessionStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage unavailable: in-memory render continues cleanly */
  }
}

const useMedia = (q: string) => {
  const [m, setM] = useState(() => window.matchMedia(q).matches);
  useEffect(() => {
    const mq = window.matchMedia(q);
    const on = () => setM(mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, [q]);
  return m;
};

function readShell() {
  const s = document.getElementById("server-state");
  const rawMode = s?.dataset.workspaceMode;
  const workspaceMode: WorkspaceMode =
    rawMode === "studio_mcp" ||
    rawMode === "plugin" ||
    rawMode === "local_place" ||
    rawMode === "browser_place" ||
    rawMode === "disconnected"
      ? rawMode
      : s?.dataset.studio === "true"
        ? "studio_mcp"
        : "disconnected";
  return {
    gameName: s?.dataset.gameName || "Studio Place",
    canBuild: s?.dataset.canBuild === "true",
    studio: s?.dataset.studio === "true",
    workspaceMode,
    bannerHtml: document.getElementById("server-banners")?.innerHTML || "",
  };
}

function clonePlace(p: Place | null | "error"): Place | null {
  if (!p || p === "error") return null;
  try {
    return JSON.parse(JSON.stringify(p)) as Place;
  } catch {
    return null;
  }
}

function workspaceBundleToPlace(ws: WorkspaceBundle): Place {
  const scripts: Record<string, string> = {};
  if (ws.scripts && typeof ws.scripts === "object") {
    for (const [relPath, entry] of Object.entries(ws.scripts)) {
      if (!entry || typeof entry !== "object") continue;
      scripts[relPath] = typeof entry.source === "string" ? entry.source : "";
    }
  }
  const instances: Place["instances"] = {};
  if (ws.instances && typeof ws.instances === "object") {
    for (const [dmPath, inst] of Object.entries(ws.instances)) {
      if (!inst || typeof inst !== "object") continue;
      instances[dmPath] = {
        class_name: typeof inst.class_name === "string" && inst.class_name ? inst.class_name : "Part",
        properties: inst.properties && typeof inst.properties === "object" && !Array.isArray(inst.properties) ? inst.properties : {},
        ...(typeof inst.source_file === "string" && inst.source_file ? { source_file: inst.source_file } : {}),
      };
    }
  }
  return { scripts, instances };
}

function mapWorkspaceDiagnostics(diags: WorkspaceDiagnostic[]): LuauDiagnostic[] {
  return diags
    .filter((d) => Boolean(d && (d.file || d.service_path) && d.message))
    .map((d) => ({
      path: String(d.file || d.service_path || ""),
      line: typeof d.line === "number" && Number.isFinite(d.line) ? d.line : 1,
      severity: d.severity === "error" ? "error" : "warning",
      code: typeof d.code === "string" ? d.code : "LUAU_CHECK",
      message: d.message,
    }));
}

function resolveScriptPathInWorkspace(
  inputPath: string,
  curPlace: Place | null | "error",
  ws: WorkspaceBundle | null,
): string {
  const clean = inputPath.trim().replace(/\\/g, "/").replace(/^\.\//, "").replace(/^\/+/, "");
  if (!clean) return inputPath;
  const scripts = curPlace && curPlace !== "error" ? curPlace.scripts || {} : {};
  if (clean in scripts) return clean;
  if (inputPath in scripts) return inputPath;

  const lower = clean.toLowerCase();
  for (const k of Object.keys(scripts)) {
    const kl = k.toLowerCase();
    if (kl === lower || kl.endsWith(`/${lower}`)) return k;
  }

  if (ws?.scripts) {
    for (const [relPath, entry] of Object.entries(ws.scripts)) {
      if (entry?.service_path && (entry.service_path === inputPath || entry.service_path === clean)) {
        return relPath;
      }
    }
    const segs = inputPath.split(".");
    const leaf = segs[segs.length - 1]?.toLowerCase() || "";
    const svc = segs[0]?.toLowerCase() || "";
    if (leaf && segs.length >= 2) {
      for (const [relPath, entry] of Object.entries(ws.scripts)) {
        const sp = (entry?.service_path || "").toLowerCase();
        const fileBase = (relPath.split("/").pop() || "").replace(/\.(server|client)?\.luau?$/i, "").toLowerCase();
        if (sp === `${svc}.${leaf}` || (sp.startsWith(`${svc}.`) && (sp.endsWith(`.${leaf}`) || fileBase === leaf))) {
          return relPath;
        }
      }
      for (const relPath of Object.keys(scripts)) {
        const fileBase = (relPath.split("/").pop() || "").replace(/\.(server|client)?\.luau?$/i, "").toLowerCase();
        if (fileBase === leaf) return relPath;
      }
    }
  }
  return clean;
}

export function App() {
  const shell = useMemo(readShell, []);
  const isBrowserEngine = Boolean((window as ShellWindow).__OPENRO_WEB_ENGINE__);
  const reduceMotion = useReducedMotion();

  const [theme, setThemeState] = useState<"light" | "dark">(() =>
    document.documentElement.dataset.theme === "dark" ? "dark" : "light",
  );
  const setTheme = (t: "light" | "dark") => {
    setThemeState(t);
    document.documentElement.setAttribute("data-theme", t);
    (window as ShellWindow).openroSetTheme?.(t);
  };

  // < 1100px uses compact Vaul drawer + Studio | Copilot tab switcher so 390x844 has 0px overflow.
  const narrow = useMedia("(max-width: 1099px)");

  const [chats, setChats] = useState<ChatList | null>(null);
  const [chatsError, setChatsError] = useState(false);
  const [lines, setLines] = useState<string[]>([]);
  const [threadError, setThreadError] = useState(false);
  const [ideas, setIdeas] = useState<Idea[]>([]);
  const [builtTitles, setBuiltTitles] = useState<string[]>([]);
  const [place, setPlace] = useState<Place | null | "error">(null);
  const [syncingPlace, setSyncingPlace] = useState(false);
  const [inflight, setInflight] = useState<Record<string, Pending>>({});
  const [, bump] = useState(0);
  const [text, setText] = useState("");
  const [chatQuery, setChatQuery] = useState("");

  // RoPilot 3-Pane IDE Layout State
  const [leftDockTab, setLeftDockTab] = useState<"explorer" | "chats" | "projects">("explorer");
  const [leftDockCollapsed, setLeftDockCollapsed] = useState(false);
  const [rightDockCollapsed, setRightDockCollapsed] = useState(false);
  const [centerTab, setCenterTab] = useState<ViewportTab>(() => {
    try {
      const h = window.location.hash;
      const p = window.location.pathname;
      if (p === "/play" || h === "#/lab" || h === "#/play" || h === "#/ship") return "ship";
    } catch {}
    return isBrowserEngine ? "studio-3d" : "embedded-studio";
  });
  const [shipSection, setShipSection] = useState<"publish" | "assets" | "art" | "playtests">(() => {
    try {
      const h = window.location.hash;
      const p = window.location.pathname;
      if (p === "/play" || h === "#/lab" || h === "#/play") return "playtests";
    } catch {}
    return "publish";
  });
  const [bottomDrawerOpen, setBottomDrawerOpen] = useState(false);
  const [bottomDrawerTab, setBottomDrawerTab] = useState<BottomDrawerTab>("console");
  const [mobilePane, setMobilePane] = useState<"studio" | "copilot">("copilot");
  const [menuOpen, setMenuOpen] = useState(false);

  // Dialogs
  const [startersOpen, setStartersOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [snapshotsOpen, setSnapshotsOpen] = useState(false);
  const [toolLibraryOpen, setToolLibraryOpen] = useState(false);
  const [pluginDialogOpen, setPluginDialogOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(() => {
    try {
      return new URLSearchParams(window.location.search).get("settings") === "1";
    } catch {
      return false;
    }
  });
  const [wizardOpen, setWizardOpen] = useState(false);
  const [twoWaySyncOpen, setTwoWaySyncOpen] = useState(false);
  const [openedDialogs, setOpenedDialogs] = useState<{
    starters?: boolean;
    palette?: boolean;
    snapshots?: boolean;
    toolLibrary?: boolean;
    plugin?: boolean;
    settings?: boolean;
    wizard?: boolean;
    twoWaySync?: boolean;
  }>(() => ({ settings: settingsOpen }));

  useEffect(() => {
    setOpenedDialogs((prev) => {
      if (
        (startersOpen && !prev.starters) ||
        (paletteOpen && !prev.palette) ||
        (snapshotsOpen && !prev.snapshots) ||
        (toolLibraryOpen && !prev.toolLibrary) ||
        (pluginDialogOpen && !prev.plugin) ||
        (settingsOpen && !prev.settings) ||
        (wizardOpen && !prev.wizard) ||
        (twoWaySyncOpen && !prev.twoWaySync)
      ) {
        return {
          starters: prev.starters || startersOpen,
          palette: prev.palette || paletteOpen,
          snapshots: prev.snapshots || snapshotsOpen,
          toolLibrary: prev.toolLibrary || toolLibraryOpen,
          plugin: prev.plugin || pluginDialogOpen,
          settings: prev.settings || settingsOpen,
          wizard: prev.wizard || wizardOpen,
          twoWaySync: prev.twoWaySync || twoWaySyncOpen,
        };
      }
      return prev;
    });
  }, [startersOpen, paletteOpen, snapshotsOpen, toolLibraryOpen, pluginDialogOpen, settingsOpen, wizardOpen, twoWaySyncOpen]);

  // Studio Copilot Modes, Proposals & Diffs
  const [composerMode, setComposerMode] = useState<ComposerMode>("agent");
  const [activeSwarmId, setActiveSwarmId] = useState<string | null>(null);
  const [activeSwarmSnap, setActiveSwarmSnap] = useState<TeamworkSnapshot | null>(null);
  const [runsRefreshSignal, setRunsRefreshSignal] = useState(0);
  const [reviewDiffs, setReviewDiffs] = useState(false);
  const [reasoning, setReasoning] = useState<ReasoningLevel>("medium");
  const [contextPaths, setContextPaths] = useState<string[]>([]);
  const [proposals, setProposals] = useState<PendingProposal[]>([]);
  const [activeProposalId, setActiveProposalId] = useState<string | null>(null);
  const [latestFileDiffs, setLatestFileDiffs] = useState<ScriptFileDiff[]>([]);
  const [selectedScriptPath, setSelectedScriptPath] = useState<string | null>(null);
  const [pluginStatus, setPluginStatus] = useState<StudioPluginStatus>(() => defaultPluginStatus());

  // Library, Market Taste, Studio & Feature Flags
  const [librarySnap, setLibrarySnap] = useState<LibrarySnapshot>(() => normalizeLibrarySnapshot({}));
  const [marketTaste, setMarketTaste] = useState<MarketTasteSnapshot>(() => defaultMarketTasteSnapshot());
  const [marketBusy, setMarketBusy] = useState(false);
  const [studioOk, setStudioOk] = useState(shell.studio);
  const [modelOk, setModelOk] = useState(shell.canBuild || !shell.studio);
  const [workspaceMode, setWorkspaceMode] = useState<WorkspaceMode>(shell.workspaceMode);
  const [activeModelLabel, setActiveModelLabel] = useState("");
  const [antigravityIssue, setAntigravityIssue] = useState<AntigravityIssue | null>(null);
  const [version, setVersion] = useState("");
  const [v050Ready, setV050Ready] = useState(() => isBrowserEngine);
  const [v060Ready, setV060Ready] = useState(() => isBrowserEngine);
  const [v070Ready, setV070Ready] = useState(() => isBrowserEngine);
  const [v080Ready, setV080Ready] = useState(() => isBrowserEngine);
  const [v090Ready, setV090Ready] = useState(() => isBrowserEngine);
  const [v0100Ready, setV0100Ready] = useState(() => isBrowserEngine);
  const [embedStatus, setEmbedStatus] = useState<StudioEmbedStatus>(() =>
    normalizeEmbedStatus({
      running: false,
      state: isBrowserEngine ? "unsupported" : "idle",
      play_mode: "edit",
    }),
  );
  const [studioMode, setStudioMode] = useState<StudioMode>(() =>
    isBrowserEngine
      ? "browser_place"
      : shell.workspaceMode === "studio_mcp"
        ? "live_mcp"
        : "local_place",
  );
  const [studioActions, setStudioActions] = useState<StudioActionEvent[]>([]);
  const [hasLiveFrame, setHasLiveFrame] = useState(false);
  const [threadFrameSeq, setThreadFrameSeq] = useState(0);
  const frameSeqRef = useRef(0);
  const lastThreadFrameAtRef = useRef(0);
  const frameListenersRef = useRef<Set<(seq: number) => void>>(new Set());
  const subscribeFrameSeq = useCallback((cb: (seq: number) => void) => {
    frameListenersRef.current.add(cb);
    return () => {
      frameListenersRef.current.delete(cb);
    };
  }, []);
  const publishLiveFrame = useCallback((seq: number) => {
    frameSeqRef.current = seq;
    setHasLiveFrame((prev) => (prev ? prev : true));
    frameListenersRef.current.forEach((cb) => cb(seq));
    const now = performance.now();
    if (lastThreadFrameAtRef.current === 0 || now - lastThreadFrameAtRef.current >= 2500) {
      lastThreadFrameAtRef.current = now;
      setThreadFrameSeq(seq);
    }
  }, []);
  const [projectAssets, setProjectAssets] = useState<ProjectAssetRecord[]>([]);
  const [lastTriedIdea, setLastTriedIdea] = useState<Idea | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [buildPhase, setBuildPhase] = useState<BuildStreamPhase | null>(null);
  const [buildDiff, setBuildDiff] = useState<PlaceDiff | null>(null);
  const [snapshots, setSnapshots] = useState<PlaceSnapshot[]>([]);
  const [serverDiagnostics, setServerDiagnostics] = useState<LuauDiagnostic[]>([]);
  const [projectWorkspace, setProjectWorkspace] = useState<WorkspaceBundle | null>(null);
  const [dragActive, setDragActive] = useState(false);

  const inputRef = useRef<HTMLTextAreaElement>(null);
  const chatSearchRef = useRef<HTMLInputElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const modelFileInputRef = useRef<HTMLInputElement>(null);
  const menuKeyRef = useRef<HTMLButtonElement>(null);
  const wasMenuOpenRef = useRef(false);
  const showUpdateRef = useRef<HTMLButtonElement>(null);
  const threadRef = useRef<HTMLDivElement>(null);
  const placeRef = useRef<Place | null | "error">(place);
  placeRef.current = place;
  const projectWorkspaceRef = useRef<WorkspaceBundle | null>(projectWorkspace);
  projectWorkspaceRef.current = projectWorkspace;

  const update = useUpdate(showUpdateRef);
  const ship = useShip(true);
  const shipActiveIdRef = useRef<string | null>(ship.activeId);
  shipActiveIdRef.current = ship.activeId;

  useEffect(() => {
    if (wasMenuOpenRef.current && !menuOpen) {
      const focusTrigger = () => {
        if (document.activeElement === inputRef.current) return;
        (menuKeyRef.current ?? document.querySelector<HTMLElement>('[aria-label="Open the menu"]'))?.focus();
      };
      requestAnimationFrame(focusTrigger);
      const t1 = window.setTimeout(focusTrigger, 120);
      const t2 = window.setTimeout(focusTrigger, 420);
      wasMenuOpenRef.current = false;
      return () => {
        window.clearTimeout(t1);
        window.clearTimeout(t2);
      };
    }
    wasMenuOpenRef.current = menuOpen;
  }, [menuOpen]);

  const activeId = chats?.active ?? "";
  const notices: Notice[] = activeId ? slotRead<Notice[]>(noticeKey(activeId), []) : [];
  const busy = Object.keys(inflight).length > 0;
  const building = activeId ? inflight[activeId] : undefined;
  const canBuild = studioOk;

  const safePlace: Place = useMemo(
    () => (place && place !== "error" ? place : { scripts: {}, instances: {} }),
    [place],
  );
  const totalPieces = Object.keys(safePlace.scripts || {}).length + Object.keys(safePlace.instances || {}).length;
  const pendingProposalCount = useMemo(
    () => proposals.filter((p) => p.status === "pending").length,
    [proposals],
  );

  const undoState = !chats
    ? "loading"
    : busy || place === null
      ? "busy"
      : place === "error"
        ? "unknown"
        : !studioOk
          ? "nostudio"
          : !chats.can_undo
            ? "nothing"
            : "ok";
  const undoReason =
    undoState === "nostudio"
      ? "Connect Studio to inspect or undo builds"
      : undoState === "nothing" || (totalPieces === 0 && undoState !== "ok")
        ? "Nothing to undo yet"
        : "";
  const canUndo = undoState === "ok";
  const canRedo = Boolean(chats?.can_redo) && !busy && studioOk;
  const activeTitle = chats?.chats.find((c) => c.id === activeId)?.title || "New chat";
  const hasConversation = useMemo(
    () => lines.some((l) => l.startsWith("You: ")) || notices.length > 0 || Boolean(building),
    [lines, notices.length, building],
  );

  const addNotice = useCallback((chatId: string, n: Notice) => {
    const key = noticeKey(chatId || "default");
    slotWrite(key, [...slotRead<Notice[]>(key, []), n]);
    bump((x) => x + 1);
  }, []);

  const begin = (chatId: string, p: Pending) => {
    slotWrite(noticeKey(chatId || "default"), "");
    setInflight((s) => ({ ...s, [chatId || "default"]: p }));
  };
  const end = (chatId: string) =>
    setInflight((s) => {
      const next = { ...s };
      delete next[chatId || "default"];
      return next;
    });

  const refreshChats = useCallback(async () => {
    try {
      const raw = await read("/api/chats");
      setChats(normalizeChatList(raw));
      setChatsError(false);
    } catch {
      setChatsError(true);
    }
  }, []);

  const refreshThread = useCallback(async () => {
    try {
      const raw = await read("/api/thread");
      setLines(normalizeThread(raw));
      setThreadError(false);
    } catch {
      setThreadError(true);
    }
  }, []);

  const refreshIdeas = useCallback(async () => {
    try {
      const raw = await post("/api/ideas", {});
      const norm = normalizeIdeas(raw);
      setIdeas(norm.ideas);
      setBuiltTitles(norm.builtTitles);
      if (norm.pitchedNew) await refreshThread();
    } catch {
      /* retain last good ideas list */
    }
  }, [refreshThread]);

  const refreshPlace = useCallback(async (overrideProjectId?: string | null): Promise<Place | null> => {
    const projId = overrideProjectId !== undefined ? overrideProjectId : shipActiveIdRef.current;
    setSyncingPlace(true);
    try {
      if (projId) {
        const ws = await fetchProjectWorkspace(projId);
        if (ws.ok && ws.scripts && ws.instances) {
          const bundle: WorkspaceBundle = {
            project_id: ws.project_id || projId,
            project_name: ws.project_name,
            mode: ws.mode || "rojo",
            scene_revision: ws.scene_revision,
            can_undo: ws.can_undo,
            can_redo: ws.can_redo,
            scripts: ws.scripts,
            instances: ws.instances,
            meshes: ws.meshes || [],
            lighting: ws.lighting || {},
            diagnostics: ws.diagnostics || [],
            skipped: ws.skipped,
            tree_error: ws.tree_error,
          };
          setProjectWorkspace(bundle);
          const unified = workspaceBundleToPlace(bundle);
          setPlace(unified);
          if (Array.isArray(bundle.diagnostics)) {
            setServerDiagnostics(mapWorkspaceDiagnostics(bundle.diagnostics));
          }
          return unified;
        }
        if (ws.httpStatus !== 404) {
          setPlace("error");
          return null;
        }
      } else {
        setProjectWorkspace(null);
      }
      const raw = await read("/api/place-tree");
      const norm = normalizePlace(raw);
      setPlace(norm);
      return norm;
    } catch {
      setPlace("error");
      return null;
    } finally {
      setSyncingPlace(false);
    }
  }, []);

  const prevShipProjectRef = useRef<string | null>(null);
  useEffect(() => {
    if (!ship.loaded) return;
    if (prevShipProjectRef.current === ship.activeId && projectWorkspaceRef.current?.project_id === ship.activeId) {
      return;
    }
    const switching = prevShipProjectRef.current !== null && prevShipProjectRef.current !== ship.activeId;
    prevShipProjectRef.current = ship.activeId;
    if (switching) {
      setSelectedPath(null);
      setSelectedScriptPath(null);
      setBuildDiff(null);
      setLatestFileDiffs([]);
    }
    void refreshPlace(ship.activeId);
  }, [refreshPlace, ship.activeId, ship.loaded]);

  const refreshSnapshots = useCallback(
    async (forceServer = false) => {
      if (!v050Ready && !forceServer) return;
      try {
        const raw = await read("/api/snapshots");
        const norm = normalizeSnapshots(raw);
        setSnapshots(norm.snapshots);
        if (norm.latestDiff) {
          setBuildDiff((prev) => prev ?? norm.latestDiff);
        }
      } catch {
        /* keep local snapshots */
      }
    },
    [v050Ready],
  );

  const refreshDiagnostics = useCallback(
    async (forceServer = false) => {
      if (shipActiveIdRef.current) return;
      if (!v050Ready && !forceServer) return;
      try {
        const raw = await read<{ diagnostics?: Array<Partial<LuauDiagnostic>> }>("/api/luau/check");
        if (raw && Array.isArray(raw.diagnostics)) {
          setServerDiagnostics(
            raw.diagnostics
              .filter((d): d is Partial<LuauDiagnostic> & { path: string; message: string } =>
                Boolean(d && typeof d.path === "string" && typeof d.message === "string"),
              )
              .map((d) => ({
                path: d.path,
                line: typeof d.line === "number" && Number.isFinite(d.line) ? d.line : 1,
                severity: d.severity === "warning" ? "warning" : "error",
                code: typeof d.code === "string" ? d.code : "LUAU_CHECK",
                message: d.message,
              })),
          );
        }
      } catch {
        /* client-side analyzer remains active */
      }
    },
    [v050Ready],
  );

  const refreshProjectAssets = useCallback(
    async (forceServer = false) => {
      if (!v060Ready && !forceServer) return;
      try {
        const rawAssets = await read("/api/assets").catch(() => null);
        if (rawAssets) setProjectAssets(normalizeAssetList(rawAssets));
      } catch {
        /* keep current project asset list */
      }
    },
    [v060Ready],
  );

  const refreshEmbedStatus = useCallback(
    async (forceServer = false) => {
      if (!v090Ready && !forceServer) return;
      try {
        const raw = await read("/api/studio/embed/status");
        const norm = normalizeEmbedStatus(raw);
        setEmbedStatus(norm);
        if (norm.running && norm.mcp_connected) {
          setStudioMode("live_mcp");
          setWorkspaceMode("studio_mcp");
        }
        if (norm.has_live_frame && typeof norm.frame_seq === "number" && norm.frame_seq > 0) {
          publishLiveFrame(norm.frame_seq);
        }
      } catch {
        /* keep current embed status */
      }
    },
    [publishLiveFrame, v090Ready],
  );

  const refreshPluginStatus = useCallback(
    async (forceServer = false) => {
      if (!v0100Ready && !forceServer) return;
      try {
        const raw = await read("/api/studio/plugin/status");
        const norm = normalizePluginStatus(raw);
        setPluginStatus(norm);
        if (norm.connected) {
          setStudioOk(true);
          setWorkspaceMode((prev) => (prev === "studio_mcp" ? prev : "plugin"));
        }
      } catch {
        /* keep current plugin status */
      }
    },
    [v0100Ready],
  );

  const refreshProposals = useCallback(
    async (forceServer = false) => {
      if (!v0100Ready && !forceServer) return;
      try {
        const raw = await read("/api/proposals");
        setProposals(normalizeProposalList(raw));
      } catch {
        /* keep current proposal list */
      }
    },
    [v0100Ready],
  );

  const refreshLibrary = useCallback(
    async (forceServer = false) => {
      if (!v090Ready && !forceServer) return;
      try {
        const raw = await read("/api/library");
        setLibrarySnap(normalizeLibrarySnapshot(raw));
      } catch {
        /* keep bundled fallback pack list */
      }
    },
    [v090Ready],
  );

  const refreshMarketTaste = useCallback(
    async (forceServer = false) => {
      if (!v090Ready && !forceServer) return;
      try {
        const raw = await read("/api/market-taste");
        setMarketTaste(normalizeMarketTasteSnapshot(raw));
      } catch {
        /* keep bundled baseline */
      }
    },
    [v090Ready],
  );

  const refreshStudioActions = useCallback(
    async (forceServer = false) => {
      if (!v070Ready && !forceServer) return;
      try {
        const raw = await read("/api/studio/actions");
        const norm = normalizeStudioActionsResponse(raw);
        setStudioMode(norm.studio_mode);
        setHasLiveFrame(norm.has_live_frame);
        if (norm.frame_seq > 0) publishLiveFrame(norm.frame_seq);
        setStudioActions(norm.actions.slice(-500));
      } catch {
        /* keep current stream buffer */
      }
    },
    [publishLiveFrame, v070Ready],
  );

  const refreshAll = useCallback(async () => {
    const [, , nextPlace] = await Promise.all([refreshChats(), refreshThread(), refreshPlace()]);
    await refreshIdeas();
    if (v050Ready) {
      void refreshSnapshots(true);
      if (nextPlace) void refreshDiagnostics(true);
    }
    if (v060Ready) void refreshProjectAssets(true);
    if (v070Ready) void refreshStudioActions(true);
    if (v0100Ready) void refreshProposals(true);
    return nextPlace;
  }, [
    refreshChats,
    refreshThread,
    refreshPlace,
    refreshIdeas,
    refreshSnapshots,
    refreshDiagnostics,
    refreshProjectAssets,
    refreshStudioActions,
    refreshProposals,
    v050Ready,
    v060Ready,
    v070Ready,
    v0100Ready,
  ]);

  const pushLocalSnapshot = useCallback((label: string, snapPlace: Place | null, diff?: PlaceDiff | null) => {
    if (!snapPlace) return;
    const entry: PlaceSnapshot = {
      id: `local-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
      label,
      created_at: Math.floor(Date.now() / 1000),
      parts_count: Object.keys(snapPlace.instances || {}).length,
      scripts_count: Object.keys(snapPlace.scripts || {}).length,
      diff: diff || undefined,
      place: clonePlace(snapPlace) || undefined,
    };
    setSnapshots((prev) => [entry, ...prev].slice(0, 30));
  }, []);

  // Initial boot discovery
  useEffect(() => {
    void refreshAll();
    read<{ version?: unknown }>("/api/version")
      .then((v) => {
        if (v && typeof v.version === "string") {
          setVersion(v.version);
          if (isV050OrNewer(v.version)) {
            setV050Ready(true);
            void refreshSnapshots(true);
            void refreshDiagnostics(true);
          }
          if (isV060OrNewer(v.version)) {
            setV060Ready(true);
            void refreshProjectAssets(true);
          }
          if (isV070OrNewer(v.version)) {
            setV070Ready(true);
            void refreshStudioActions(true);
          }
          if (isV080OrNewer(v.version)) setV080Ready(true);
          if (isV090OrNewer(v.version)) {
            setV090Ready(true);
            void refreshEmbedStatus(true);
            void refreshLibrary(true);
            void refreshMarketTaste(true);
          }
          if (isV0100OrNewer(v.version)) {
            setV0100Ready(true);
            void refreshProposals(true);
            void refreshPluginStatus(true);
          }
        }
      })
      .catch(() => {});

    read<{
      studio_connected?: unknown;
      model_configured?: unknown;
      workspace_mode?: unknown;
      model_id?: unknown;
      antigravity_issue?: AntigravityIssue | null;
      version?: unknown;
      sse_build_stream?: unknown;
      studio_stream?: unknown;
      assets_api?: unknown;
      ide_api?: unknown;
      embed_api?: unknown;
      studio_embed?: unknown;
      ropilot_v0100?: unknown;
      proposals_api?: unknown;
      plugin_bridge?: unknown;
      skills_count?: unknown;
    }>("/api/status")
      .then((s) => {
        if (!s || typeof s !== "object") return;
        if (typeof s.studio_connected === "boolean") setStudioOk(s.studio_connected);
        if (typeof s.model_configured === "boolean") setModelOk(s.model_configured);
        if (
          s.workspace_mode === "studio_mcp" ||
          s.workspace_mode === "plugin" ||
          s.workspace_mode === "local_place" ||
          s.workspace_mode === "browser_place" ||
          s.workspace_mode === "disconnected"
        ) {
          setWorkspaceMode(s.workspace_mode);
          setStudioMode(
            s.workspace_mode === "studio_mcp"
              ? "live_mcp"
              : s.workspace_mode === "plugin"
                ? "plugin_live"
                : s.workspace_mode === "browser_place"
                  ? "browser_place"
                  : "local_place",
          );
        }
        if (typeof s.model_id === "string" && s.model_id.trim()) {
          setActiveModelLabel(s.model_id.trim().split("/").pop() || s.model_id.trim());
        }
        if (s.antigravity_issue !== undefined) {
          setAntigravityIssue(s.antigravity_issue ?? null);
        }
        if (s.sse_build_stream === true || (typeof s.version === "string" && isV050OrNewer(s.version))) {
          setV050Ready(true);
        }
        if (
          s.assets_api === true ||
          (typeof s.skills_count === "number" && s.skills_count >= 28) ||
          (typeof s.version === "string" && isV060OrNewer(s.version))
        ) {
          setV060Ready(true);
          void refreshProjectAssets(true);
        }
        if (s.studio_stream === true || (typeof s.version === "string" && isV070OrNewer(s.version))) {
          setV070Ready(true);
          void refreshStudioActions(true);
        }
        if (s.ide_api === true || (typeof s.version === "string" && isV080OrNewer(s.version))) {
          setV080Ready(true);
        }
        if (s.studio_embed === true || s.embed_api === true || (typeof s.version === "string" && isV090OrNewer(s.version))) {
          setV090Ready(true);
          void refreshEmbedStatus(true);
          void refreshLibrary(true);
          void refreshMarketTaste(true);
        }
        if (
          s.ropilot_v0100 === true ||
          s.proposals_api === true ||
          s.plugin_bridge === true ||
          (typeof s.version === "string" && isV0100OrNewer(s.version))
        ) {
          setV0100Ready(true);
          void refreshProposals(true);
          void refreshPluginStatus(true);
        }
      })
      .catch(() => {});
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Explorer Auto-Sync Fix (#397): whenever live MCP, embedded Studio, or companion plugin turns active,
  // automatically trigger debounced refreshPlace() + one +1.5s retry so Explorer never stays stuck at "0 instances".
  // Keep callbacks in a ref so unrelated version-flag updates never cancel pending sync timers.
  const syncFuncsRef = useRef({ refreshPlace, refreshDiagnostics });
  syncFuncsRef.current = { refreshPlace, refreshDiagnostics };
  const liveSyncKey = [
    studioMode === "live_mcp" ? "mcp" : studioMode === "plugin_live" ? "plugmode" : "",
    embedStatus.mcp_connected ? "emcp" : "",
    embedStatus.running ? "erun" : "",
    pluginStatus.connected ? "plug" : "",
  ]
    .filter(Boolean)
    .join("|");
  const prevLiveSyncKeyRef = useRef("");

  useEffect(() => {
    const prev = prevLiveSyncKeyRef.current;
    prevLiveSyncKeyRef.current = liveSyncKey;
    if (!liveSyncKey || liveSyncKey === prev) return;
    const t1 = window.setTimeout(() => {
      void syncFuncsRef.current.refreshPlace();
      void syncFuncsRef.current.refreshDiagnostics(true);
    }, 150);
    const t2 = window.setTimeout(() => {
      void syncFuncsRef.current.refreshPlace();
    }, 1500);
    return () => {
      window.clearTimeout(t1);
      window.clearTimeout(t2);
    };
  }, [liveSyncKey]);

  // Gentle plugin status polling ONLY while plugin dialog is open or plugin mode is active
  useEffect(() => {
    if (!v0100Ready) return;
    if (!pluginDialogOpen && workspaceMode !== "plugin") return;
    const iv = window.setInterval(() => {
      void refreshPluginStatus(true);
    }, 5000);
    return () => window.clearInterval(iv);
  }, [pluginDialogOpen, refreshPluginStatus, v0100Ready, workspaceMode]);

  // Live Studio SSE stream (/api/studio/stream) for realtime viewport updates & MCP frames
  useEffect(() => {
    if (!v070Ready) return;
    let placeTimer: number | null = null;
    const schedulePlaceSync = () => {
      if (placeTimer !== null) return;
      placeTimer = window.setTimeout(() => {
        placeTimer = null;
        void refreshPlace();
      }, 160);
    };
    const unsub = subscribeStudioStream({
      enabled: v070Ready,
      onAction: (ev) => {
        setStudioMode((prev) => (prev === ev.studio_mode ? prev : ev.studio_mode));
        setStudioActions((prev) => {
          if (prev.some((x) => x.seq === ev.seq)) return prev;
          return [...prev, ev].slice(-500);
        });
        if (
          ["part_created", "part_updated", "script_written", "decal_bound", "reverted", "imported", "restored"].includes(
            ev.kind,
          )
        ) {
          schedulePlaceSync();
        }
      },
      onFrame: (info) => {
        publishLiveFrame(info.seq);
      },
      onStatus: (st) => {
        if (st.studio_mode) setStudioMode((prev) => (prev === st.studio_mode ? prev : st.studio_mode!));
        if (typeof st.has_live_frame === "boolean") setHasLiveFrame(st.has_live_frame);
        if (typeof st.frame_seq === "number" && st.frame_seq > 0) publishLiveFrame(st.frame_seq);
      },
    });
    return () => {
      unsub();
      if (placeTimer !== null) window.clearTimeout(placeTimer);
    };
  }, [publishLiveFrame, refreshPlace, v070Ready]);

  // Auto-scroll right Copilot thread on new messages
  const prevChatRef = useRef<string | null>(null);
  const prevLenRef = useRef(0);
  useEffect(() => {
    const el = threadRef.current;
    if (!el) return;
    if (!hasConversation || prevChatRef.current !== activeId) {
      el.scrollTop = 0;
    } else if (lines.length > prevLenRef.current || Boolean(building) || proposals.length > 0) {
      el.scrollTop = el.scrollHeight;
    }
    prevChatRef.current = activeId;
    prevLenRef.current = lines.length;
  }, [activeId, lines.length, notices.length, Boolean(building), hasConversation, proposals.length]);

  const ideShortcutRef = useRef<{
    selectedPath: string | null;
    canRedo: boolean;
    duplicate: (path: string) => Promise<void>;
    redo: () => Promise<void>;
  } | null>(null);

  // Global keyboard shortcuts: Cmd+K opens Command Palette; Ctrl+D duplicates; Ctrl+Y redoes; `/` focuses search
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((v) => !v);
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "b" && !narrow) {
        e.preventDefault();
        setLeftDockCollapsed((v) => !v);
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "j") {
        e.preventDefault();
        setBottomDrawerOpen((v) => !v);
        return;
      }
      if (e.altKey && !e.metaKey && !e.ctrlKey) {
        if (e.key === "1") { e.preventDefault(); setCenterTab(isBrowserEngine ? "studio-3d" : "embedded-studio"); return; }
        if (e.key === "2") { e.preventDefault(); setCenterTab("code-diff"); return; }
        if (e.key === "3" && !isBrowserEngine) { e.preventDefault(); setCenterTab("assets"); return; }
        if (e.key === "4") { e.preventDefault(); setCenterTab("ship"); return; }
        if (e.key === "5" && !isBrowserEngine) { e.preventDefault(); setCenterTab("teamwork"); return; }
        if (e.key === "6") { e.preventDefault(); setCenterTab("gui-designer"); return; }
      }
      const el = e.target as HTMLElement | null;
      const inEditable = Boolean(el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable));
      if (!inEditable && (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "d") {
        const cur = ideShortcutRef.current;
        if (cur?.selectedPath && cur.selectedPath.startsWith("Workspace.")) {
          e.preventDefault();
          void cur.duplicate(cur.selectedPath);
          return;
        }
      }
      if (
        !inEditable &&
        (((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "y") ||
          ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "z"))
      ) {
        const cur = ideShortcutRef.current;
        if (cur?.canRedo) {
          e.preventDefault();
          void cur.redo();
          return;
        }
      }
      if (e.key !== "/" || e.metaKey || e.ctrlKey || e.altKey || inEditable) return;
      e.preventDefault();
      if (!narrow && leftDockTab === "chats" && chatSearchRef.current) chatSearchRef.current.focus();
      else inputRef.current?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [isBrowserEngine, leftDockTab, narrow]);

  const cantBuildWhy = !studioOk
    ? "Building needs the Local Place Engine, Studio Plugin, or Roblox Studio MCP connected."
    : !modelOk
      ? "Chat prompts need a local gateway or API key configured in Settings. Built-in Studio starters still run right away."
      : "";

  const handleToggleContextPath = useCallback((path: string) => {
    setContextPaths((prev) =>
      prev.includes(path) ? prev.filter((p) => p !== path) : [...prev, path].slice(-8),
    );
  }, []);

  const handleAddContextPath = useCallback((path: string) => {
    setContextPaths((prev) => (prev.includes(path) ? prev : [...prev, path].slice(-8)));
  }, []);

  const handleRemoveContextPath = useCallback((path: string) => {
    setContextPaths((prev) => prev.filter((p) => p !== path));
  }, []);

  const handleOpenScriptInCodeDiff = useCallback((path: string, proposalId?: string) => {
    const resolved = resolveScriptPathInWorkspace(path, placeRef.current, projectWorkspaceRef.current);
    setSelectedScriptPath(resolved);
    setSelectedPath(resolved);
    setContextPaths((prev) => (prev.includes(resolved) ? prev : [...prev, resolved].slice(-8)));
    if (proposalId) setActiveProposalId(proposalId);
    setCenterTab("code-diff");
    setRightDockCollapsed(false);
    if (window.matchMedia("(max-width: 1099px)").matches) {
      setMobilePane("studio");
    }
    setMenuOpen(false);
  }, []);

  const handleSelectPartAndInspect = useCallback((path: string) => {
    setSelectedPath(path);
    setContextPaths((prev) => (prev.includes(path) ? prev : [...prev, path].slice(-8)));
    if (placeRef.current && placeRef.current !== "error" && path in (placeRef.current.scripts || {})) {
      setSelectedScriptPath(path);
      setCenterTab("code-diff");
    } else {
      setBottomDrawerTab("properties");
      setBottomDrawerOpen(true);
    }
    if (window.matchMedia("(max-width: 1099px)").matches) {
      setMobilePane("studio");
    }
    setMenuOpen(false);
  }, []);

  const checkStudio = async () => {
    try {
      const s = await read<{
        studio_connected?: unknown;
        model_configured?: unknown;
        workspace_mode?: unknown;
        model_id?: unknown;
        antigravity_issue?: AntigravityIssue | null;
      }>("/api/status");
      const nextStudio = s?.studio_connected === true;
      const nextModel = s?.model_configured === true;
      if (nextStudio && !studioOk) toast.success("Studio reconnected");
      else if (!nextStudio) toast.error("Studio is still disconnected");
      setStudioOk(nextStudio);
      setModelOk(nextModel);
      if (
        s?.workspace_mode === "studio_mcp" ||
        s?.workspace_mode === "plugin" ||
        s?.workspace_mode === "local_place" ||
        s?.workspace_mode === "browser_place" ||
        s?.workspace_mode === "disconnected"
      ) {
        setWorkspaceMode(s.workspace_mode);
      }
      if (typeof s?.model_id === "string" && s.model_id.trim()) {
        setActiveModelLabel(s.model_id.trim().split("/").pop() || s.model_id.trim());
      }
      if (s?.antigravity_issue !== undefined) {
        setAntigravityIssue(s.antigravity_issue ?? null);
      }
      if (nextStudio) await refreshPlace();
    } catch {
      toast.error("Could not reach the OpenRO server.");
    }
  };

  const switchChat = async (id: string) => {
    if (!id || id === activeId) return;
    try {
      const r = await fetch("/api/chats/select", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ id }),
      });
      if (!r.ok) throw new Error("select");
    } catch {
      addNotice(activeId || id, { text: "Could not switch chats. The server did not answer.", bad: true });
      return;
    }
    if (activeId && text.trim()) slotWrite(draftKey(activeId), text);
    setText(slotRead<string>(draftKey(id), ""));
    slotWrite(draftKey(id), "");
    setSelectedPath(null);
    setSelectedScriptPath(null);
    setBuildDiff(null);
    setLatestFileDiffs([]);
    setActiveProposalId(null);
    setMenuOpen(false);
    await refreshAll();
  };

  const newChat = async () => {
    try {
      const r = await fetch("/api/chats", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: "{}",
      });
      if (!r.ok) throw new Error("new chat");
    } catch {
      toast.error("Could not reach the OpenRO server.");
      return;
    }
    setChatQuery("");
    setSelectedPath(null);
    setSelectedScriptPath(null);
    setBuildDiff(null);
    setLatestFileDiffs([]);
    setActiveProposalId(null);
    setContextPaths([]);
    setMenuOpen(false);
    await refreshAll();
    inputRef.current?.focus();
  };

  const buildIdea = async (idea: Idea) => {
    const chatId = activeId || "default";
    const beforePlace = clonePlace(placeRef.current);
    setLastTriedIdea(idea);
    setStartersOpen(false);
    begin(chatId, { build: idea.title });
    const stopStream = subscribeBuildStream(setBuildPhase, v050Ready);
    let failure: string | null = null;
    try {
      const data = await post<{ ok?: boolean; error?: string }>("/api/ideas/build", { id: idea.id });
      if (!data.ok) failure = typeof data.error === "string" && data.error ? data.error : "The build did not finish.";
      else toast.success(`Built ${idea.title} in Studio`);
    } catch {
      failure = "Could not reach the OpenRO server.";
    }
    stopStream();
    setBuildPhase(null);
    end(chatId);
    if (failure) addNotice(chatId, { text: failure, bad: true });
    const afterPlace = await refreshAll();
    if (!failure && afterPlace) {
      const diff = computePlaceDiff(beforePlace, afterPlace);
      setBuildDiff(diff);
      const scriptDiffs = computeLocalScriptDiffs(beforePlace, afterPlace);
      if (scriptDiffs.length > 0) setLatestFileDiffs(scriptDiffs);
      if (!v050Ready) pushLocalSnapshot(idea.title, afterPlace, diff);
      if (centerTab === "embedded-studio" && !embedStatus.running) {
        setCenterTab("studio-3d");
      }
    }
  };

  const submitPromptText = async (rawMessage: string, restoreOnFail = true) => {
    const message = rawMessage.trim();
    if (!message || busy) return;
    const chatId = activeId || "default";
    const beforePlace = clonePlace(placeRef.current);
    const effectiveReview = composerMode === "plan" || reviewDiffs;
    if (restoreOnFail) setText("");
    begin(chatId, { message });
    const stopStream = subscribeBuildStream(setBuildPhase, v050Ready);
    let failure: string | null = null;
    let returnedProposal: PendingProposal | null = null;
    let returnedFileDiffs: ScriptFileDiff[] = [];

    try {
      const bodyPayload: Record<string, unknown> = { message };
      if (v0100Ready) {
        bodyPayload.mode = composerMode;
        bodyPayload.reasoning = reasoning;
        bodyPayload.review_diffs = effectiveReview;
        if (contextPaths.length > 0) bodyPayload.context_paths = contextPaths;
      }
      const r = await fetch("/api/chat", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(bodyPayload),
      });
      const raw = await r.text();
      let data: {
        ok?: boolean;
        error?: string;
        proposal?: unknown;
        file_diffs?: unknown;
      } = {};
      try {
        data = JSON.parse(raw) as typeof data;
      } catch {
        /* non-JSON proxy or crash reply */
      }
      if (r.status === 403 || r.status === 413 || r.status === 415) {
        failure = data.error || "OpenRO did not accept that message.";
      } else if (!r.ok && !((r.status === 502 || r.status === 503) && data.ok === false)) {
        failure = SEND_FAILED;
      } else {
        if (data.proposal) {
          returnedProposal = normalizeProposal(data.proposal);
        }
        if (data.file_diffs) {
          returnedFileDiffs = normalizeFileDiffs(data.file_diffs);
        }
      }
    } catch {
      failure = SEND_FAILED;
    }

    stopStream();
    setBuildPhase(null);
    end(chatId);

    if (failure) {
      addNotice(chatId, { text: message, user: true });
      addNotice(chatId, { text: failure, bad: true });
      if (restoreOnFail) setText(message);
    }

    const afterPlace = await refreshAll();

    if (!failure) {
      if (returnedProposal) {
        setProposals((prev) => [returnedProposal!, ...prev.filter((p) => p.id !== returnedProposal!.id)]);
        setActiveProposalId(returnedProposal.id);
        if (returnedProposal.file_diffs[0]?.path) {
          setSelectedScriptPath(returnedProposal.file_diffs[0].path);
        }
        setCenterTab("code-diff");
        toast.success(`Staged proposal ready (${returnedProposal.file_diffs.length} script diffs)`);
      } else if (afterPlace) {
        const diff = computePlaceDiff(beforePlace, afterPlace);
        if (diff.added.length + diff.modified.length + diff.removed.length > 0) {
          setBuildDiff(diff);
        }
        const scriptDiffs =
          returnedFileDiffs.length > 0 ? returnedFileDiffs : computeLocalScriptDiffs(beforePlace, afterPlace);
        if (scriptDiffs.length > 0) {
          setLatestFileDiffs(scriptDiffs);
        }
        if (!v050Ready) pushLocalSnapshot(message.slice(0, 36), afterPlace, diff);
      }
    }
    inputRef.current?.focus();
  };

  const handleApplyProposal = async (proposalId: string, selectedPaths?: string[]) => {
    const target = proposals.find((p) => p.id === proposalId);
    const paths = selectedPaths ?? target?.file_diffs.map((d) => d.path) ?? [];
    const beforePlace = clonePlace(placeRef.current);
    try {
      const res = await post<{ ok?: boolean; error?: string; file_diffs?: unknown }>(
        `/api/proposals/${encodeURIComponent(proposalId)}/apply`,
        { selected_paths: paths },
      );
      if (res.ok) {
        toast.success(`Applied proposal (${paths.length || "all"} files) to Studio`);
        setProposals((prev) =>
          prev.map((p) => (p.id === proposalId ? { ...p, status: "applied" } : p)),
        );
        const afterPlace = await refreshAll();
        if (afterPlace) {
          setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
          const diffs = res.file_diffs
            ? normalizeFileDiffs(res.file_diffs)
            : computeLocalScriptDiffs(beforePlace, afterPlace);
          if (diffs.length > 0) setLatestFileDiffs(diffs);
        }
      } else {
        toast.error(res.error || "Could not apply proposal.");
      }
    } catch {
      toast.error("Could not reach /api/proposals/:id/apply.");
    }
  };

  const handleRejectProposal = async (proposalId: string) => {
    try {
      const res = await post<{ ok?: boolean; error?: string }>(
        `/api/proposals/${encodeURIComponent(proposalId)}/reject`,
        {},
      );
      if (res.ok) {
        toast.info("Proposal rejected: Studio place untouched");
        setProposals((prev) =>
          prev.map((p) => (p.id === proposalId ? { ...p, status: "rejected" } : p)),
        );
        if (activeProposalId === proposalId) setActiveProposalId(null);
        void refreshProposals(true);
      } else {
        toast.error(res.error || "Could not reject proposal.");
      }
    } catch {
      toast.error("Could not reach /api/proposals/:id/reject.");
    }
  };

  const addSystem = async (sys: GameSystemDef) => {
    if (!canBuild || busy) return;
    setStartersOpen(false);
    const matchedIdea = ideas.find((i) => i.id === sys.defaultIdeaId || (i.skills.length === 1 && i.skills[0] === sys.id));
    if (matchedIdea) {
      await buildIdea(matchedIdea);
      return;
    }
    if (v050Ready) {
      const chatId = activeId || "default";
      const beforePlace = clonePlace(placeRef.current);
      begin(chatId, { build: sys.title });
      const stopStream = subscribeBuildStream(setBuildPhase, true);
      try {
        const res = await post<{ ok?: boolean; error?: string }>("/api/skills/build", {
          skill: sys.id,
          name: sys.id,
        });
        stopStream();
        setBuildPhase(null);
        end(chatId);
        if (res.ok) {
          toast.success(`Added ${sys.title} (${sys.id}) to Studio`);
          const afterPlace = await refreshAll();
          if (afterPlace) {
            setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
            setLatestFileDiffs(computeLocalScriptDiffs(beforePlace, afterPlace));
          }
          return;
        }
      } catch {
        stopStream();
        setBuildPhase(null);
        end(chatId);
      }
    }
    await submitPromptText(sys.prompt, false);
  };

  const workbenchIdeas = useMemo(
    () =>
      ideas.filter(
        (i) =>
          i.id !== "untitled-anime-card-game" &&
          i.id !== "anime-card-battles-apex" &&
          !/untitled anime card game|anime card battles/i.test(i.title),
      ),
    [ideas],
  );

  const send = (e: FormEvent) => {
    e.preventDefault();
    const trimmed = text.trim();
    if (!trimmed) return;
    if (composerMode === "swarm" && !isBrowserEngine) {
      void sendSwarmPrompt(trimmed);
      return;
    }
    void submitPromptText(text, true);
  };

  const sendSwarmPrompt = async (goal: string) => {
    setText("");
    if (activeSwarmId && activeSwarmSnap?.run.status === "running") {
      const posted = await postSwarmBoard(activeSwarmId, `@coordinator ${goal}`);
      if (!posted.ok) {
        toast.error(posted.error || "Could not send message to Swarm.");
        setText(goal);
        return;
      }
      const detail = await getSwarmRun(activeSwarmId);
      if (detail.ok && detail.run) {
        setActiveSwarmSnap({
          run: detail.run,
          roster: detail.roster ?? [],
          tasks: detail.tasks ?? [],
          claims: detail.claims ?? [],
          board: detail.board ?? [],
        });
      }
      inputRef.current?.focus();
      return;
    }
    const created = await createSwarmRun({
      goal,
      ...(ship.activeId ? { project_id: ship.activeId } : {}),
    });
    if (!created.ok || !created.run?.id) {
      toast.error(created.error || "Could not start Swarm.");
      setText(goal);
      return;
    }
    const runId = created.run.id;
    setActiveSwarmId(runId);
    const detail = await getSwarmRun(runId);
    if (detail.ok && detail.run) {
      setActiveSwarmSnap({
        run: detail.run,
        roster: detail.roster ?? [],
        tasks: detail.tasks ?? [],
        claims: detail.claims ?? [],
        board: detail.board ?? [],
      });
    }
    inputRef.current?.focus();
  };

  const handleStopSwarm = async () => {
    if (!activeSwarmId) return;
    const stopped = await stopSwarmRun(activeSwarmId);
    if (!stopped.ok) {
      toast.error(stopped.error || "Could not stop Swarm.");
      return;
    }
    const detail = await getSwarmRun(activeSwarmId);
    if (detail.ok && detail.run) {
      setActiveSwarmSnap({
        run: detail.run,
        roster: detail.roster ?? [],
        tasks: detail.tasks ?? [],
        claims: detail.claims ?? [],
        board: detail.board ?? [],
      });
    }
  };

  const handleQueuePlan = async (goal: string) => {
    const msg = goal.trim();
    if (!msg) return;
    const res = await apiCall<{ run_id?: string }>("/api/runs", {
      message: msg,
      chat_id: activeId || undefined,
      mode: "plan_execute",
      project_id: ship.activeId || undefined,
    });
    if (!res.ok) {
      toast.error(res.error || "Could not queue background plan.");
      return;
    }
    setText("");
    setRunsRefreshSignal((n) => n + 1);
    toast.success("Queued background plan");
  };

  const undoNow = async () => {
    const chatId = activeId || "default";
    const beforePlace = clonePlace(placeRef.current);
    begin(chatId, { undo: true });
    let msg: string | null = null;
    let bad = false;
    try {
      const data = await post<{ ok?: boolean; restored?: unknown[]; error?: string }>("/api/revert", {});
      if (data.ok) {
        const count = Array.isArray(data.restored) ? data.restored.length : 0;
        msg = `Undid the last build. ${count} change(s) were put back.`;
      } else if (data.error === "nothing to revert") {
        msg = "Nothing to undo. No build is waiting to be undone in this chat.";
      } else {
        msg = typeof data.error === "string" && data.error ? data.error : "The undo did not finish.";
        bad = true;
      }
    } catch {
      msg = "Could not reach the OpenRO server.";
      bad = true;
    }
    end(chatId);
    const afterPlace = await refreshAll();
    if (!bad && afterPlace) {
      setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
      setLatestFileDiffs(computeLocalScriptDiffs(beforePlace, afterPlace));
    }
    if (msg) {
      addNotice(chatId, { text: msg, bad });
      if (bad) toast.error(msg);
      else toast.success(msg);
    }
  };

  const applyProjectSceneResult = (
    res: {
      instances?: WorkspaceBundle["instances"];
      lighting?: WorkspaceLighting;
      scene_revision?: number;
      can_undo?: boolean;
      can_redo?: boolean;
    },
    beforePlace: Place | null,
  ): Place | null => {
    const curWs = projectWorkspaceRef.current;
    if (curWs && res.instances) {
      const nextWs: WorkspaceBundle = {
        ...curWs,
        instances: res.instances,
        lighting: res.lighting ?? curWs.lighting,
        scene_revision: res.scene_revision ?? curWs.scene_revision,
        can_undo: res.can_undo ?? curWs.can_undo,
        can_redo: res.can_redo ?? curWs.can_redo,
      };
      setProjectWorkspace(nextWs);
      const updated = workspaceBundleToPlace(nextWs);
      setPlace(updated);
      setBuildDiff(computePlaceDiff(beforePlace, updated));
      return updated;
    }
    return null;
  };

  const handleSaveLighting = async (nextLighting: WorkspaceLighting) => {
    const projId = shipActiveIdRef.current;
    if (!projId) return;
    const beforePlace = clonePlace(placeRef.current);
    const res = await mutateProjectScene(projId, [
      { op: "set_lighting", path: "Lighting", properties: nextLighting as Record<string, unknown> },
    ]);
    if (res.ok) {
      if (!applyProjectSceneResult(res, beforePlace)) {
        await refreshPlace(projId);
      }
      toast.success("Saved scene lighting");
    } else {
      toast.error(res.error || "Could not update scene lighting.");
    }
  };

  const handleSelectMesh = async (mesh: WorkspaceMeshEntry) => {
    const projId = shipActiveIdRef.current;
    const curPlace = placeRef.current && placeRef.current !== "error" ? placeRef.current : null;
    if (curPlace) {
      const meshPathLow = (mesh.obj_path || mesh.obj_url || "").toLowerCase();
      const meshBase = mesh.name.replace(/\.obj$/i, "").toLowerCase();
      const existing = Object.entries(curPlace.instances || {}).find(([dmPath, inst]) => {
        const mid = typeof inst.properties?.MeshId === "string" ? inst.properties.MeshId.toLowerCase() : "";
        return (
          dmPath.startsWith("Workspace.") &&
          ((mid && (mid.includes(meshPathLow) || mid.includes(meshBase))) ||
            (inst.class_name === "MeshPart" && dmPath.toLowerCase().includes(meshBase)))
        );
      });
      if (existing) {
        setSelectedPath(existing[0]);
        setCenterTab("studio-3d");
        setBottomDrawerTab("properties");
        setBottomDrawerOpen(true);
        return;
      }
    }
    if (!projId) return;
    const beforePlace = clonePlace(curPlace) || { scripts: {}, instances: {} };
    const cleanBase = mesh.name.replace(/\.obj$/i, "").replace(/[^A-Za-z0-9_]/g, "") || "MeshProp";
    let name = cleanBase;
    for (let i = 2; i < 200 && `Workspace.${name}` in beforePlace.instances; i++) name = `${cleanBase}${i}`;
    const targetPath = `Workspace.${name}`;
    const size = Array.isArray(mesh.size) && mesh.size.length >= 3 ? mesh.size : [6, 6, 6];
    const res = await mutateProjectScene(projId, [
      {
        op: "upsert",
        path: targetPath,
        class_name: "MeshPart",
        properties: {
          Position: [0, Math.max(2, Number(size[1]) / 2), 8],
          Size: size,
          Color: "#8B939E",
          Material: "SmoothPlastic",
          Anchored: true,
          MeshId: mesh.obj_path || mesh.obj_url,
          ...(mesh.texture_path || mesh.texture_url ? { TextureID: mesh.texture_path || mesh.texture_url } : {}),
        },
      },
    ]);
    if (res.ok) {
      applyProjectSceneResult(res, beforePlace);
      setSelectedPath(targetPath);
      setCenterTab("studio-3d");
      setBottomDrawerTab("properties");
      setBottomDrawerOpen(true);
      toast.success(`Placed ${name} mesh in Workspace`);
    } else {
      toast.error(res.error || "Could not place mesh in scene.");
    }
  };

  const handleSaveInstance = async (payload: InstanceEditPayload) => {
    const beforePlace = clonePlace(placeRef.current);
    const projId = shipActiveIdRef.current;
    const applyOptimistic = () => {
      if (!beforePlace) return;
      const updated: Place = {
        scripts: { ...beforePlace.scripts },
        instances: {
          ...beforePlace.instances,
          [payload.path]: { class_name: payload.class_name, properties: payload.properties },
        },
      };
      setPlace(updated);
      setBuildDiff(computePlaceDiff(beforePlace, updated));
    };

    if (projId) {
      const res = await mutateProjectScene(projId, [
        {
          op: "upsert",
          path: payload.path,
          class_name: payload.class_name,
          properties: payload.properties,
        },
      ]);
      if (res.ok) {
        toast.success(`Saved ${payload.path} properties`);
        if (!applyProjectSceneResult(res, beforePlace)) {
          const afterPlace = await refreshPlace(projId);
          if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
        }
      } else {
        applyOptimistic();
        toast.error(res.error || "Could not persist scene instance on server.");
      }
      return;
    }

    if (!v050Ready) {
      applyOptimistic();
      toast.success(`Updated ${payload.path} properties in workbench`);
      return;
    }

    try {
      const res = await post<{ ok?: boolean; error?: string }>("/api/place/instance", payload);
      if (res.ok) {
        toast.success(`Saved ${payload.path} properties`);
        const afterPlace = await refreshPlace();
        if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
        void refreshSnapshots(true);
      } else {
        applyOptimistic();
        toast.error(res.error || "Could not persist instance properties on server.");
      }
    } catch {
      applyOptimistic();
      toast.success(`Applied ${payload.path} locally in workbench`);
    }
  };

  const handleImportRbxlx = async (filename: string, xml: string) => {
    const { parseRbxlxXml } = await import("./app/rbxlxCodec");
    const parsed = parseRbxlxXml(xml);
    if (!parsed.ok) {
      toast.error(parsed.error);
      return;
    }
    const beforePlace = clonePlace(placeRef.current);
    const partN = Object.keys(parsed.place.instances).length;
    const scrN = Object.keys(parsed.place.scripts).length;

    if (v050Ready) {
      try {
        const res = await post<{ ok?: boolean; error?: string }>("/api/import/rbxlx", {
          filename,
          label: filename,
          xml,
        });
        if (res.ok) {
          toast.success(`Imported ${filename} (${partN} parts, ${scrN} scripts)`);
          const afterPlace = await refreshAll();
          if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
          return;
        }
        toast.error(res.error || "Server rejected .rbxlx import.");
        return;
      } catch {
        /* fallback */
      }
    }

    setPlace(parsed.place);
    const diff = computePlaceDiff(beforePlace, parsed.place);
    setBuildDiff(diff);
    pushLocalSnapshot(`Imported ${filename}`, parsed.place, diff);
    toast.success(`Imported ${filename} (${partN} parts, ${scrN} scripts)`);
  };

  const handleImportPrefabModel = async (file: File) => {
    const projectId = shipActiveIdRef.current;
    if (!projectId) {
      if (/\.rbxlx$/i.test(file.name)) {
        const xml = await file.text();
        await handleImportRbxlx(file.name, xml);
        return;
      }
      toast.error("Open or create a project first to import .rbxmx or .rbxm prefab models.");
      setLeftDockCollapsed(false);
      setLeftDockTab("projects");
      return;
    }
    const format = inferPrefabFormat(file.name);
    if (!format) {
      toast.error("Select a .rbxmx, .rbxm, or .rbxlx file.");
      return;
    }
    if (file.size === 0 || file.size > 16 * 1024 * 1024) {
      toast.error("Model file must be between 1 byte and 16 MB.");
      return;
    }
    try {
      const buf = await file.arrayBuffer();
      const bytes_base64 = bytesToBase64(new Uint8Array(buf));
      const res = await importProjectModel(projectId, {
        file_name: file.name,
        format,
        bytes_base64,
        target_service: "Workspace",
      });
      if (!res.ok) {
        toast.error(res.error || "Could not import prefab model.");
        return;
      }
      toast.success(`Imported ${res.model_path || file.name} into ${res.wired_service || "Workspace"}`);
      await refreshPlace(projectId);
      await ship.refresh();
      setCenterTab("studio-3d");
    } catch {
      toast.error("Could not read selected model file.");
    }
  };

  const onFileInputChange = (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    file.text().then(
      (xml) => void handleImportRbxlx(file.name, xml),
      () => toast.error("Could not read selected file."),
    );
  };

  const onModelInputChange = (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    void handleImportPrefabModel(file);
  };

  const onAppDragOver = (e: DragEvent<HTMLDivElement>) => {
    if (!e.dataTransfer.types.includes("Files")) return;
    e.preventDefault();
    if (!dragActive) setDragActive(true);
  };

  const onAppDragLeave = (e: DragEvent<HTMLDivElement>) => {
    if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
    setDragActive(false);
  };

  const onAppDrop = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setDragActive(false);
    const file = [...e.dataTransfer.files].find((f) => /\.(rbxmx|rbxm|rbxlx|xml)$/i.test(f.name)) || e.dataTransfer.files[0];
    if (!file) return;
    if (/\.(rbxmx|rbxm)$/i.test(file.name)) {
      void handleImportPrefabModel(file);
      return;
    }
    file.text().then(
      (xml) => void handleImportRbxlx(file.name, xml),
      () => toast.error("Could not read dropped file."),
    );
  };

  const handleCreateSnapshot = async (label: string) => {
    const cur = clonePlace(placeRef.current);
    if (!cur) {
      toast.error("Nothing in your place to snapshot yet.");
      return;
    }
    if (v050Ready) {
      try {
        const res = await post<{ ok?: boolean; error?: string }>("/api/snapshots", { label });
        if (res.ok) {
          toast.success(`Saved snapshot "${label}"`);
          await refreshSnapshots(true);
          return;
        }
      } catch {
        /* fallback */
      }
    }
    pushLocalSnapshot(label, cur, buildDiff);
    toast.success(`Saved snapshot "${label}"`);
  };

  const handleRestoreSnapshot = async (snap: PlaceSnapshot) => {
    const beforePlace = clonePlace(placeRef.current);
    if (v050Ready && !snap.id.startsWith("local-")) {
      try {
        const res = await post<{ ok?: boolean; error?: string }>(`/api/snapshots/${encodeURIComponent(snap.id)}/restore`, {});
        if (res.ok) {
          toast.success(`Restored snapshot "${snap.label}"`);
          setSnapshotsOpen(false);
          const afterPlace = await refreshAll();
          if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
          return;
        }
        toast.error(res.error || "Could not restore snapshot.");
        return;
      } catch {
        /* fallback */
      }
    }
    if (snap.place) {
      setPlace(clonePlace(snap.place));
      setBuildDiff(computePlaceDiff(beforePlace, snap.place));
      setSnapshotsOpen(false);
      toast.success(`Restored snapshot "${snap.label}"`);
    }
  };

  const handleExportSnapshot = async (snap: PlaceSnapshot) => {
    try {
      let xml = "";
      if (v050Ready && !snap.id.startsWith("local-")) {
        const r = await fetch(`/api/snapshots/${encodeURIComponent(snap.id)}/rbxlx`, { cache: "no-store" });
        if (r.ok) xml = await r.text();
      }
      if (!xml && snap.place) {
        const { serializeRbxlx } = await import("./app/rbxlxCodec");
        xml = serializeRbxlx(snap.label, snap.place);
      }
      if (!xml.includes("<roblox")) throw new Error("no xml");
      const slug = snap.label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "snapshot";
      const blob = new Blob([xml], { type: "application/xml;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${slug}.rbxlx`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      toast.success(`Exported ${slug}.rbxlx`);
    } catch {
      toast.error("Could not export snapshot .rbxlx.");
    }
  };

  const handleRunStudioPlaytest = async () => {
    if (!v070Ready) {
      toast.success("Static Luau check complete.");
      return;
    }
    try {
      const res = await post<{ ok?: boolean; passed?: boolean; error?: string }>("/api/studio/playtest", {});
      if (res.ok === false) {
        toast.error(res.error || "Studio playtest failed.");
      } else if (res.passed === false) {
        toast.error("Playtest flagged issues: inspect Output Console.");
        setBottomDrawerTab("console");
        setBottomDrawerOpen(true);
      } else {
        toast.success("Studio Playtest passed (0 Luau errors)");
      }
      await refreshStudioActions(true);
      await refreshDiagnostics(true);
    } catch {
      toast.error("Could not reach /api/studio/playtest.");
    }
  };

  const handleCaptureStudioFrame = async () => {
    if (!v070Ready) {
      toast.error("Live Studio Frame capture requires Roblox Studio attached.");
      return;
    }
    try {
      const res = await post<{ ok?: boolean; frame_seq?: number; error?: string }>("/api/studio/capture", {});
      if (res.ok === false) {
        toast.error(res.error || "Live Studio MCP is not attached. Switch to Studio 3D or connect Studio MCP.");
        return;
      }
      if (typeof res.frame_seq === "number") publishLiveFrame(res.frame_seq);
      else publishLiveFrame(frameSeqRef.current + 1);
      toast.success("Captured fresh frame from Roblox Studio");
    } catch {
      toast.error("Could not capture frame from Studio.");
    }
  };

  const buildPartPresetSpec = (preset: PartPreset, existing: Record<string, unknown>) => {
    const pickName = (base: string) => {
      if (!(`Workspace.${base}` in existing)) return base;
      for (let i = 2; i < 500; i++) {
        if (!(`Workspace.${base}${i}` in existing)) return `${base}${i}`;
      }
      return `${base}${Date.now() % 1000}`;
    };
    const offsetX = ((Object.keys(existing).length % 5) - 2) * 8;
    const offsetZ = 10 + Math.floor(Object.keys(existing).length / 5) * 6;
    switch (preset) {
      case "sphere":
        return {
          name: pickName("SpherePart"),
          class_name: "Part",
          position: [offsetX, 4, offsetZ] as [number, number, number],
          size: [6, 6, 6] as [number, number, number],
          color: "#3B82F6",
          properties: { Position: [offsetX, 4, offsetZ], Size: [6, 6, 6], Color: "#3B82F6", Shape: "Ball", Material: "SmoothPlastic", Anchored: true },
        };
      case "cylinder":
        return {
          name: pickName("CylinderPillar"),
          class_name: "Part",
          position: [offsetX, 5, offsetZ] as [number, number, number],
          size: [6, 10, 6] as [number, number, number],
          color: "#D49B26",
          properties: { Position: [offsetX, 5, offsetZ], Size: [6, 10, 6], Color: "#D49B26", Shape: "Cylinder", Material: "Metal", Anchored: true },
        };
      case "spawn":
        return {
          name: pickName("SpawnLocation"),
          class_name: "SpawnLocation",
          position: [offsetX, 0.5, offsetZ] as [number, number, number],
          size: [8, 1, 8] as [number, number, number],
          color: "#2D68DF",
          properties: { Position: [offsetX, 0.5, offsetZ], Size: [8, 1, 8], Color: "#2D68DF", Material: "SmoothPlastic", Anchored: true },
        };
      case "jump_pad":
        return {
          name: pickName("JumpPad"),
          class_name: "Part",
          position: [offsetX, 1, offsetZ] as [number, number, number],
          size: [8, 1.5, 8] as [number, number, number],
          color: "#24946E",
          properties: { Position: [offsetX, 1, offsetZ], Size: [8, 1.5, 8], Color: "#24946E", Material: "Neon", Anchored: true },
        };
      case "kill_brick":
        return {
          name: pickName("KillBrick"),
          class_name: "Part",
          position: [offsetX, 1, offsetZ] as [number, number, number],
          size: [10, 1.5, 6] as [number, number, number],
          color: "#DF522D",
          properties: { Position: [offsetX, 1, offsetZ], Size: [10, 1.5, 6], Color: "#DF522D", Material: "Neon", Anchored: true },
        };
      case "decal_board": {
        const asset = projectAssets[Object.keys(existing).length % Math.max(1, projectAssets.length)];
        return {
          name: pickName("DecalBoard"),
          class_name: "Part",
          position: [offsetX, 7, offsetZ] as [number, number, number],
          size: [7, 10, 1] as [number, number, number],
          color: "#475569",
          properties: {
            Position: [offsetX, 7, offsetZ],
            Size: [7, 10, 1],
            Color: "#475569",
            Material: "SmoothPlastic",
            Anchored: true,
            DisplayTitle: asset?.title || "Decal Board",
            DecalTexture: asset?.image_url || "",
          },
        };
      }
      default:
        return {
          name: pickName("StudioBlock"),
          class_name: "Part",
          position: [offsetX, 2, offsetZ] as [number, number, number],
          size: [8, 4, 8] as [number, number, number],
          color: "#8B939E",
          properties: { Position: [offsetX, 2, offsetZ], Size: [8, 4, 8], Color: "#8B939E", Shape: "Block", Material: "SmoothPlastic", Anchored: true },
        };
    }
  };

  const handleCreatePart = async (preset: PartPreset) => {
    const beforePlace = clonePlace(placeRef.current) || { scripts: {}, instances: {} };
    const spec = buildPartPresetSpec(preset, beforePlace.instances || {});
    const fallbackPath = `Workspace.${spec.name}`;
    const projId = shipActiveIdRef.current;
    const applyLocal = () => {
      const updated: Place = {
        scripts: { ...beforePlace.scripts },
        instances: {
          ...beforePlace.instances,
          [fallbackPath]: { class_name: spec.class_name, properties: spec.properties },
        },
      };
      setPlace(updated);
      setSelectedPath(fallbackPath);
      setBuildDiff(computePlaceDiff(beforePlace, updated));
    };

    if (projId) {
      const res = await mutateProjectScene(projId, [
        {
          op: "upsert",
          path: fallbackPath,
          class_name: spec.class_name,
          properties: spec.properties,
        },
      ]);
      if (res.ok) {
        setSelectedPath(fallbackPath);
        toast.success(`Added ${fallbackPath} to Workspace`);
        if (!applyProjectSceneResult(res, beforePlace)) {
          const afterPlace = await refreshPlace(projId);
          if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
        }
      } else {
        toast.error(res.error || "Could not create part in project scene.");
      }
      return;
    }

    if (!v080Ready) {
      applyLocal();
      toast.success(`Inserted ${fallbackPath} in Studio 3D`);
      return;
    }

    try {
      const res = await post<{ ok?: boolean; path?: string; error?: string }>("/api/place/create", {
        preset,
        name: spec.name,
        parent: "Workspace",
        class_name: spec.class_name,
        position: spec.position,
        size: spec.size,
        color: spec.color,
        properties: spec.properties,
      });
      if (res.ok) {
        const createdPath = typeof res.path === "string" && res.path ? res.path : fallbackPath;
        setSelectedPath(createdPath);
        toast.success(`Added ${createdPath} to Workspace`);
        const afterPlace = await refreshPlace();
        void refreshChats();
        void refreshStudioActions(true);
        void refreshSnapshots(true);
        if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
      } else {
        toast.error(res.error || "Could not create part.");
      }
    } catch {
      applyLocal();
      toast.success(`Added ${fallbackPath} locally`);
    }
  };

  const handleDuplicatePart = async (path: string) => {
    const beforePlace = clonePlace(placeRef.current);
    if (!beforePlace || !beforePlace.instances[path]) return;
    const projId = shipActiveIdRef.current;

    const applyLocal = () => {
      const src = beforePlace.instances[path];
      const base = path.replace(/^Workspace\./, "");
      let dupName = `${base}Copy`;
      for (let i = 2; i < 500 && `Workspace.${dupName}` in beforePlace.instances; i++) dupName = `${base}Copy${i}`;
      const dupPath = `Workspace.${dupName}`;
      const pos =
        Array.isArray(src.properties?.Position) && src.properties.Position.length >= 3
          ? [Number(src.properties.Position[0]) + 4, Number(src.properties.Position[1]), Number(src.properties.Position[2]) + 4]
          : [4, 2, 4];
      const updated: Place = {
        scripts: { ...beforePlace.scripts },
        instances: {
          ...beforePlace.instances,
          [dupPath]: { class_name: src.class_name, properties: { ...(src.properties || {}), Position: pos } },
        },
      };
      setPlace(updated);
      setSelectedPath(dupPath);
      setBuildDiff(computePlaceDiff(beforePlace, updated));
      return dupPath;
    };

    if (projId) {
      const res = await mutateProjectScene(projId, [
        { op: "duplicate", path, offset: [4, 0, 4] },
      ]);
      if (res.ok) {
        const addedPath =
          (res.instances &&
            Object.keys(res.instances).find((k) => !(k in beforePlace.instances))) ||
          path;
        setSelectedPath(addedPath);
        toast.success(`Duplicated ${path.split(".").pop() || path} as ${addedPath.split(".").pop() || addedPath}`);
        if (!applyProjectSceneResult(res, beforePlace)) {
          const afterPlace = await refreshPlace(projId);
          if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
        }
      } else {
        toast.error(res.error || "Could not duplicate part in project scene.");
      }
      return;
    }

    if (!v080Ready) {
      const dupPath = applyLocal();
      toast.success(`Duplicated to ${dupPath}`);
      return;
    }

    try {
      const res = await post<{ ok?: boolean; path?: string; error?: string }>("/api/place/duplicate", {
        path,
        offset: [4, 0, 4],
      });
      if (res.ok) {
        const dupPath = typeof res.path === "string" && res.path ? res.path : path;
        setSelectedPath(dupPath);
        toast.success(`Duplicated ${path.split(".").pop() || path} as ${dupPath.split(".").pop() || dupPath}`);
        const afterPlace = await refreshPlace();
        void refreshChats();
        void refreshStudioActions(true);
        void refreshSnapshots(true);
        if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
      } else {
        toast.error(res.error || "Could not duplicate part.");
      }
    } catch {
      const dupPath = applyLocal();
      toast.success(`Duplicated to ${dupPath}`);
    }
  };

  const handleDeletePart = async (path: string) => {
    const beforePlace = clonePlace(placeRef.current);
    if (!beforePlace || !beforePlace.instances[path]) return;
    const projId = shipActiveIdRef.current;

    const applyLocal = () => {
      const nextInstances = { ...beforePlace.instances };
      delete nextInstances[path];
      const updated: Place = { scripts: { ...beforePlace.scripts }, instances: nextInstances };
      setPlace(updated);
      if (selectedPath === path) setSelectedPath(null);
      setBuildDiff(computePlaceDiff(beforePlace, updated));
    };

    if (projId) {
      const res = await mutateProjectScene(projId, [{ op: "delete", path }]);
      if (res.ok) {
        if (selectedPath === path) setSelectedPath(null);
        toast.success(`Deleted ${path}`);
        if (!applyProjectSceneResult(res, beforePlace)) {
          const afterPlace = await refreshPlace(projId);
          if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
        }
      } else {
        toast.error(res.error || "Could not delete part from project scene.");
      }
      return;
    }

    if (!v080Ready) {
      applyLocal();
      toast.success(`Deleted ${path}`);
      return;
    }

    try {
      const res = await post<{ ok?: boolean; error?: string }>("/api/place/delete", { path });
      if (res.ok) {
        if (selectedPath === path) setSelectedPath(null);
        toast.success(`Deleted ${path}`);
        const afterPlace = await refreshPlace();
        void refreshChats();
        void refreshStudioActions(true);
        void refreshSnapshots(true);
        if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
      } else {
        toast.error(res.error || "Could not delete part.");
      }
    } catch {
      applyLocal();
      toast.success(`Deleted ${path} locally`);
    }
  };

  const handleSaveScript = async (path: string, source: string) => {
    const beforePlace = clonePlace(placeRef.current);
    const projId = shipActiveIdRef.current;
    const localDiags = analyzePlaceLuau({ scripts: { [path]: source }, instances: beforePlace?.instances || {} });
    const applyLocal = () => {
      const base = beforePlace || { scripts: {}, instances: {} };
      const updated: Place = {
        instances: { ...base.instances },
        scripts: { ...base.scripts, [path]: source },
      };
      setPlace(updated);
      setBuildDiff(computePlaceDiff(beforePlace, updated));
      setLatestFileDiffs(computeLocalScriptDiffs(beforePlace, updated));
    };

    if (projId) {
      const targetPath = resolveScriptPathInWorkspace(path, beforePlace, projectWorkspaceRef.current);
      const res = await writeProjectFile(projId, targetPath, source);
      if (res.ok) {
        const short = targetPath.includes("/") ? targetPath.split("/").pop() || targetPath : targetPath.split(".").pop() || targetPath;
        const errs = localDiags.filter((d) => d.severity === "error").length;
        if (errs > 0) toast.error(`Saved ${short} (${errs} Luau error${errs === 1 ? "" : "s"})`);
        else toast.success(`Saved ${short} to project`);
        const afterPlace = await refreshPlace(projId);
        void ship.refresh();
        if (afterPlace) {
          setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
          const diffs = computeLocalScriptDiffs(beforePlace, afterPlace);
          if (diffs.length > 0) setLatestFileDiffs(diffs);
        }
      } else {
        toast.error(res.error || "Could not save project Luau file.");
      }
      return;
    }

    if (!v080Ready) {
      applyLocal();
      const errs = localDiags.filter((d) => d.severity === "error").length;
      if (errs > 0) toast.error(`Saved ${path} (${errs} Luau error${errs === 1 ? "" : "s"})`);
      else toast.success(`Saved & checked ${path} (0 errors)`);
      return;
    }

    try {
      const res = await post<{
        ok?: boolean;
        errors?: number;
        error_count?: number;
        warnings?: number;
        warning_count?: number;
        error?: string;
      }>("/api/place/script", { path, source });
      if (res.ok) {
        const errs =
          typeof res.error_count === "number"
            ? res.error_count
            : typeof res.errors === "number"
              ? res.errors
              : localDiags.filter((d) => d.severity === "error").length;
        const warns =
          typeof res.warning_count === "number"
            ? res.warning_count
            : typeof res.warnings === "number"
              ? res.warnings
              : localDiags.filter((d) => d.severity === "warning").length;
        if (errs > 0) toast.error(`Saved ${path.split(".").pop() || path}: ${errs} Luau error${errs === 1 ? "" : "s"}`);
        else if (warns > 0) toast.success(`Saved ${path.split(".").pop() || path}: 0 errors, ${warns} warning${warns === 1 ? "" : "s"}`);
        else toast.success(`Saved & checked ${path.split(".").pop() || path} (0 Luau errors)`);
        const afterPlace = await refreshPlace();
        void refreshChats();
        void refreshDiagnostics(true);
        void refreshStudioActions(true);
        void refreshSnapshots(true);
        if (afterPlace) {
          setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
          const diffs = computeLocalScriptDiffs(beforePlace, afterPlace);
          if (diffs.length > 0) setLatestFileDiffs(diffs);
        }
      } else {
        toast.error(res.error || "Could not save Luau script.");
      }
    } catch {
      applyLocal();
      toast.success(`Saved ${path} locally`);
    }
  };

  const handleRedo = async () => {
    const chatId = activeId || "default";
    const beforePlace = clonePlace(placeRef.current);
    const projId = shipActiveIdRef.current;
    if (projId && projectWorkspaceRef.current?.can_redo && !chats?.can_redo) {
      const res = await mutateProjectScene(projId, [{ op: "redo" }]);
      if (res.ok) {
        applyProjectSceneResult(res, beforePlace);
        toast.success("Redid scene mutation");
      } else {
        toast.error(res.error || "Nothing to redo in scene.");
      }
      return;
    }
    if (!v080Ready) {
      toast.error("Redo requires OpenRO v0.8.0 or newer.");
      return;
    }
    try {
      const data = await post<{ ok?: boolean; label?: string; restored?: unknown[]; error?: string }>("/api/redo", {});
      if (data.ok) {
        const count = Array.isArray(data.restored) ? data.restored.length : 0;
        const msg =
          typeof data.label === "string" && data.label.trim()
            ? `Redid: ${data.label.trim()}`
            : count > 0
              ? `Redid ${count} Studio change(s).`
              : "Redid the last undone Studio action.";
        toast.success(msg);
        addNotice(chatId, { text: msg });
        const afterPlace = await refreshAll();
        if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
      } else {
        toast.error(data.error || "Nothing to redo.");
      }
    } catch {
      toast.error("Could not reach /api/redo.");
    }
  };

  const handleExportRbxlx = async () => {
    try {
      const r = await fetch("/api/export/rbxlx", { cache: "no-store" });
      if (!r.ok) throw new Error("export failed");
      const xml = await r.text();
      if (!xml.includes("<roblox")) throw new Error("unexpected xml");
      const label = activeTitle && activeTitle !== "New chat" ? activeTitle : shell.gameName;
      const slug = (label || "openro-place").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "openro-place";
      const blob = new Blob([xml], { type: "application/xml;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${slug}.rbxlx`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      toast.success(`Exported ${slug}.rbxlx`);
    } catch {
      toast.error("Could not export .rbxlx right now.");
    }
  };

  const handleDownloadShipPack = async () => {
    const cur = placeRef.current && placeRef.current !== "error" ? placeRef.current : null;
    if (!cur || (Object.keys(cur.instances || {}).length === 0 && Object.keys(cur.scripts || {}).length === 0)) {
      toast.error("Build or import a Roblox place before exporting a Ship Pack.");
      return;
    }
    const label = activeTitle && activeTitle !== "New chat" ? activeTitle : shell.gameName;
    const slug = (label || "openro-ship-pack").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "openro-ship-pack";

    try {
      let zipBytes: Uint8Array | null = null;
      if (v080Ready && !isBrowserEngine) {
        const r = await fetch("/api/export/bundle", { cache: "no-store" });
        if (r.ok) {
          const buf = new Uint8Array(await r.arrayBuffer());
          if (buf.length >= 4 && buf[0] === 0x50 && buf[1] === 0x4b) zipBytes = buf;
        }
      }
      if (!zipBytes) {
        const { buildRojoShipPackZip } = await import("./app/zipBuilder");
        zipBytes = buildRojoShipPackZip({
          gameName: label,
          place: cur,
          assets: projectAssets,
          diagnostics: analyzePlaceLuau(cur),
        });
      }
      const blob = new Blob([zipBytes.buffer as ArrayBuffer], { type: "application/zip" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${slug}-rojo-ship-pack.zip`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      toast.success(`Downloaded ${slug}-rojo-ship-pack.zip (${Math.max(1, Math.round(zipBytes.length / 1024))} KB)`);
    } catch {
      toast.error("Could not build Studio & Rojo Ship Pack (.zip).");
    }
  };

  const handleConnectMcp = async (command?: string) => {
    if (!v080Ready) {
      toast.error("Live Studio MCP Connect requires OpenRO v0.8.0+.");
      return;
    }
    try {
      const res = await post<{
        ok?: boolean;
        connected?: boolean;
        workspace_mode?: WorkspaceMode;
        studio_mode?: StudioMode;
        has_live_frame?: boolean;
        frame_seq?: number;
        error?: string;
      }>("/api/studio/mcp/connect", command ? { command } : {});
      if (res.ok && res.connected) {
        setStudioOk(true);
        setWorkspaceMode(res.workspace_mode || "studio_mcp");
        setStudioMode(res.studio_mode || "live_mcp");
        if (typeof res.has_live_frame === "boolean") setHasLiveFrame(res.has_live_frame);
        if (typeof res.frame_seq === "number" && res.frame_seq > 0) publishLiveFrame(res.frame_seq);
        toast.success("Connected live Roblox Studio MCP bridge");
        void refreshStudioActions(true);
        void refreshPlace();
      } else {
        toast.error(res.error || "Could not connect to a running Roblox Studio MCP bridge.");
      }
    } catch {
      toast.error("Could not reach /api/studio/mcp/connect.");
    }
  };

  const handleDisconnectMcp = async () => {
    if (!v080Ready) return;
    try {
      const res = await post<{ ok?: boolean; workspace_mode?: WorkspaceMode; studio_mode?: StudioMode; error?: string }>(
        "/api/studio/mcp/disconnect",
        {},
      );
      if (res.ok) {
        await refreshPlace();
        setWorkspaceMode(res.workspace_mode || "local_place");
        setStudioMode(res.studio_mode || "local_place");
        setHasLiveFrame(false);
        toast.success("Disconnected Studio MCP bridge");
        void refreshStudioActions(true);
      } else {
        toast.error(res.error || "Could not disconnect Studio MCP bridge.");
      }
    } catch {
      toast.error("Could not reach /api/studio/mcp/disconnect.");
    }
  };

  const handleTransferEmbed = async () => {
    if (!v090Ready) {
      toast.error("Connecting live Studio requires OpenRO desktop server.");
      return;
    }
    setEmbedStatus((prev) => ({ ...prev, state: "launching", error: null }));
    try {
      const url = ship.activeId
        ? `/api/projects/${encodeURIComponent(ship.activeId)}/open-in-studio`
        : "/api/studio/embed/transfer";
      const res = await apiCall<Record<string, unknown>>(url, { transfer: true, rebuild: true });
      if (res.httpStatus === 404) {
        await handleAttachEmbed();
        return;
      }
      if (res.ok) {
        const norm = normalizeEmbedStatus(res.status ?? res);
        setEmbedStatus(norm);
        if (norm.mcp_connected || norm.phase === "connected" || res.connected === true) {
          setStudioOk(true);
          setStudioMode("live_mcp");
          setWorkspaceMode("studio_mcp");
          toast.success(ship.active?.name ? `Opened ${ship.active.name} in Studio` : "Studio connected");
        } else if (norm.phase === "awaiting_signin") {
          toast("Sign in inside Studio to connect");
        }
        if (typeof res.frame_seq === "number" && res.frame_seq > 0) {
          publishLiveFrame(res.frame_seq);
        }
        setCenterTab("embedded-studio");
        void refreshStudioActions(true);
        void refreshPlace();
        return;
      }
      const errText = typeof res.error === "string" && res.error ? res.error : "Could not open project in Studio.";
      setEmbedStatus((prev) => ({ ...prev, error: errText }));
      toast.error(errText);
    } catch {
      await handleAttachEmbed();
    }
  };

  const handleAttachEmbed = async () => {
    if (!v090Ready) {
      toast.error("Connecting live Studio requires OpenRO desktop server.");
      return;
    }
    setEmbedStatus((prev) => ({ ...prev, state: "launching", error: null }));
    try {
      const res = await apiCall<Record<string, unknown>>("/api/studio/embed/attach", {});
      if (res.httpStatus === 404) {
        await handleLaunchEmbed();
        return;
      }
      if (res.ok) {
        const norm = normalizeEmbedStatus(res.status ?? res);
        setEmbedStatus(norm);
        if (norm.mcp_connected || norm.phase === "connected") {
          setStudioOk(true);
          setStudioMode("live_mcp");
          setWorkspaceMode("studio_mcp");
          toast.success("Connected to Roblox Studio");
        } else if (norm.phase === "awaiting_signin") {
          toast("Sign in inside Studio to connect");
        }
        void refreshStudioActions(true);
        void refreshPlace();
        return;
      }
      await handleLaunchEmbed();
    } catch {
      await handleLaunchEmbed();
    }
  };

  const handleLaunchEmbed = async (command?: string) => {
    if (!v090Ready) {
      toast.error("Embedded Roblox Studio requires OpenRO desktop server.");
      return;
    }
    setEmbedStatus((prev) => ({ ...prev, state: "launching", error: null }));
    try {
      const res = await post<Record<string, unknown>>("/api/studio/embed/launch", command ? { command } : {});
      if (res.ok === true) {
        const norm = normalizeEmbedStatus(res.status ?? res);
        setEmbedStatus({ ...norm, running: true });
        if (norm.mcp_connected || res.studio_mode === "live_mcp") {
          setStudioOk(true);
          setStudioMode("live_mcp");
          setWorkspaceMode("studio_mcp");
        }
        if (typeof res.frame_seq === "number" && res.frame_seq > 0) {
          publishLiveFrame(res.frame_seq);
        }
        setCenterTab("embedded-studio");
        toast.success("Launched & embedded Roblox Studio");
        void refreshStudioActions(true);
        void refreshPlace();
      } else {
        const errText =
          typeof res.error === "string" && res.error
            ? res.error
            : "Could not launch Embedded Roblox Studio on this host.";
        setEmbedStatus((prev) => ({
          ...prev,
          running: false,
          state: studioMode === "browser_place" ? "unsupported" : "error",
          error: errText,
        }));
        toast.error(errText);
      }
    } catch {
      const errText = "Could not reach /api/studio/embed/launch.";
      setEmbedStatus((prev) => ({ ...prev, running: false, state: "error", error: errText }));
      toast.error(errText);
    }
  };

  const handleStopEmbed = async () => {
    if (!v090Ready) return;
    try {
      const res = await post<Record<string, unknown>>("/api/studio/embed/stop", {});
      if (res.ok === true) {
        setEmbedStatus(
          normalizeEmbedStatus({
            ...(res.status && typeof res.status === "object" ? (res.status as Record<string, unknown>) : res),
            running: false,
            state: "stopped",
            play_mode: "edit",
          }),
        );
        setWorkspaceMode("local_place");
        setStudioMode("local_place");
        setHasLiveFrame(false);
        await refreshPlace();
        void refreshStudioActions(true);
        toast.success("Stopped Embedded Roblox Studio");
      } else {
        toast.error(typeof res.error === "string" ? res.error : "Could not stop Embedded Roblox Studio.");
      }
    } catch {
      toast.error("Could not reach /api/studio/embed/stop.");
    }
  };

  const handleSendMouseInput = async (payload: StudioMouseInputPayload) => {
    if (!v090Ready) return;
    try {
      const res = await post<{ ok?: boolean; frame_seq?: number }>("/api/studio/input/mouse", payload);
      if (res.ok && typeof res.frame_seq === "number" && res.frame_seq > 0) {
        publishLiveFrame(res.frame_seq);
      }
    } catch {
      /* non-fatal */
    }
  };

  const handleSendKeyboardInput = async (payload: StudioKeyboardInputPayload) => {
    if (!v090Ready) return;
    try {
      const res = await post<{ ok?: boolean; frame_seq?: number }>("/api/studio/input/keyboard", payload);
      if (res.ok && typeof res.frame_seq === "number" && res.frame_seq > 0) {
        publishLiveFrame(res.frame_seq);
      }
    } catch {
      /* non-fatal */
    }
  };

  const handleInvokeStudioTool = async (
    tool: StudioToolName,
    args: Record<string, unknown>,
  ): Promise<StudioToolResult> => {
    const started = performance.now();
    if (!v090Ready) {
      return { ok: false, tool, error: "13 Studio MCP Tools require OpenRO v0.9.0+.", duration_ms: 0 };
    }
    try {
      const res = await post<Record<string, unknown>>(`/api/studio/tools/${encodeURIComponent(tool)}`, args);
      const duration_ms = Math.max(1, Math.round(performance.now() - started));
      const ok = res.ok !== false;
      const playModeRaw = typeof res.play_mode === "string" ? res.play_mode : undefined;
      if (playModeRaw === "edit" || playModeRaw === "play" || playModeRaw === "server") {
        setEmbedStatus((prev) => ({ ...prev, play_mode: playModeRaw }));
      } else if (tool === "start_stop_play" && ok) {
        const reqMode = String(args.mode || "stop");
        const nextPm = reqMode === "start_play" ? "play" : reqMode === "run_server" ? "server" : "edit";
        setEmbedStatus((prev) => ({ ...prev, play_mode: nextPm }));
      }
      if (typeof res.frame_seq === "number" && res.frame_seq > 0) {
        publishLiveFrame(res.frame_seq);
      }
      if (ok && (tool === "multi_edit" || tool === "execute_luau")) {
        void refreshPlace();
        void refreshDiagnostics(true);
      }
      if (ok) void refreshStudioActions(true);
      return {
        ok,
        tool,
        result: res.result !== undefined ? res.result : ok ? res : undefined,
        text: typeof res.text === "string" ? res.text : undefined,
        error: !ok && typeof res.error === "string" ? res.error : undefined,
        frame_seq: typeof res.frame_seq === "number" ? res.frame_seq : undefined,
        duration_ms,
      };
    } catch {
      return {
        ok: false,
        tool,
        error: `Could not reach /api/studio/tools/${tool}.`,
        duration_ms: Math.max(1, Math.round(performance.now() - started)),
      };
    }
  };

  const handleSelectMarketNiche = useCallback(
    async (nicheId: string) => {
      setMarketTaste((prev) => ({ ...prev, active_niche: nicheId }));
      if (!v090Ready) return;
      setMarketBusy(true);
      try {
        const raw = await post<Record<string, unknown>>("/api/market-taste/niche", { niche: nicheId });
        if (raw && raw.ok !== false) setMarketTaste(normalizeMarketTasteSnapshot(raw));
      } catch {
        /* keep optimistic local selection */
      } finally {
        setMarketBusy(false);
      }
    },
    [v090Ready],
  );

  const handleRefreshMarketTaste = useCallback(async () => {
    if (!v090Ready) {
      toast.info("Live Roblox chart refresh requires OpenRO v0.9.0+.");
      return;
    }
    setMarketBusy(true);
    try {
      const raw = await post<Record<string, unknown>>("/api/market-taste/refresh", {});
      const norm = normalizeMarketTasteSnapshot(raw);
      setMarketTaste(norm);
      if (raw?.offline_fallback === true) {
        toast.info("Browser Studio uses the Nov 2026 Roblox Explore baseline offline.");
      } else if (raw?.ok === false) {
        toast.error(typeof raw.error === "string" ? raw.error : "Could not refresh Roblox Explore charts.");
      } else {
        toast.success(`Refreshed Roblox Explore charts (${norm.niches.length} niches)`);
      }
    } catch {
      toast.error("Could not reach /api/market-taste/refresh.");
    } finally {
      setMarketBusy(false);
    }
  }, [v090Ready]);

  const handlePitchMarketPrompt = useCallback(
    (prompt: string, nicheName: string) => {
      const clean = prompt.trim();
      if (!clean) return;
      setText(clean);
      if (activeId) slotWrite(draftKey(activeId), clean);
      setToolLibraryOpen(false);
      setMenuOpen(false);
      setRightDockCollapsed(false);
      setMobilePane("copilot");
      requestAnimationFrame(() => inputRef.current?.focus());
      toast.success(`Loaded ${nicheName} pitch into Copilot composer`);
    },
    [activeId],
  );

  ideShortcutRef.current = {
    selectedPath,
    canRedo,
    duplicate: handleDuplicatePart,
    redo: handleRedo,
  };

  const onBannerClick = (e: MouseEvent<HTMLDivElement>) => {
    const banner = (e.target as HTMLElement).closest("[data-dismiss]")?.closest<HTMLElement>(".banner");
    if (banner) banner.hidden = true;
  };

  const filteredChats = useMemo(() => {
    const all = chats?.chats || [];
    const q = chatQuery.trim().toLowerCase();
    return q ? all.filter((c) => (c.title || "New chat").toLowerCase().includes(q)) : all;
  }, [chats, chatQuery]);

  // Left Rail (244px collapsible desktop, Vaul drawer <1100px)
  const leftDockBody = (
    <nav
      aria-label="Explorer, Chats, and Projects"
      className="flex h-full min-h-0 flex-col gap-2.5 bg-surface p-2.5 text-ink"
    >
      {/* Explorer | Chats | Projects Segmented Toggle */}
      <SegmentedControl aria-label="Left dock section" size="sm" className="w-full flex-none">
        <SegmentedControlItem
          data-checked={leftDockTab === "explorer" ? "" : undefined}
          aria-pressed={leftDockTab === "explorer"}
          onClick={() => setLeftDockTab("explorer")}
          className="flex-1 justify-center gap-1 !px-1.5 sm:text-xs max-md:!h-11"
        >
          <FolderTree className="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden />
          <span>Explorer</span>
          <span className="font-mono text-xs tabular-nums text-ink-2">{totalPieces}</span>
        </SegmentedControlItem>
        <SegmentedControlItem
          data-checked={leftDockTab === "chats" ? "" : undefined}
          aria-pressed={leftDockTab === "chats"}
          onClick={() => setLeftDockTab("chats")}
          className="flex-1 justify-center gap-1 !px-1.5 sm:text-xs max-md:!h-11"
        >
          <MessageSquare className="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden />
          <span>Chats</span>
        </SegmentedControlItem>
        <SegmentedControlItem
          data-checked={leftDockTab === "projects" ? "" : undefined}
          aria-pressed={leftDockTab === "projects"}
          onClick={() => setLeftDockTab("projects")}
          className="flex-1 justify-center gap-1 !px-1.5 sm:text-xs max-md:!h-11"
        >
          <FolderKanban className="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden />
          <span>Projects</span>
        </SegmentedControlItem>
      </SegmentedControl>

      {/* Main Dock Scroll Region */}
      {leftDockTab === "projects" ? (
        <ProjectsRail ship={ship} studioOk={studioOk} onOpenTwoWaySync={() => setTwoWaySyncOpen(true)} />
      ) : leftDockTab === "explorer" ? (
        <LeftExplorerTree
          place={place}
          error={place === "error"}
          selectedPath={selectedPath}
          selectedScriptPath={selectedScriptPath}
          contextPaths={contextPaths}
          studioSelection={pluginStatus.selection}
          buildDiff={buildDiff}
          serverDiagnostics={serverDiagnostics}
          syncing={syncingPlace}
          projectMode={Boolean(ship.activeId)}
          projectName={projectWorkspace?.project_name || ship.active?.name || null}
          scriptMeta={projectWorkspace?.scripts}
          projectMeshes={projectWorkspace?.meshes}
          hasLighting={Boolean(projectWorkspace?.lighting)}
          onSelectPart={handleSelectPartAndInspect}
          onOpenScript={(p) => handleOpenScriptInCodeDiff(p)}
          onAttachContext={handleToggleContextPath}
          onRefreshPlace={() => void refreshPlace()}
          onImportModel={() => modelFileInputRef.current?.click()}
          onSelectLighting={() => {
            setSelectedPath("Lighting");
            setBottomDrawerTab("properties");
            setBottomDrawerOpen(true);
            setCenterTab("studio-3d");
            setMobilePane("studio");
            setMenuOpen(false);
          }}
          onSelectMesh={(mesh) => void handleSelectMesh(mesh)}
        />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-2">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => void newChat()}
            className="w-full justify-start border-ink-button bg-ink-button text-ink-button-fg hover:bg-ink-button/90 max-md:min-h-11"
          >
            <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
            New chat
          </Button>

          <InputGroup className="bg-board/45">
            <InputGroupAddon align="inline-start" className="ps-2">
              <Search className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
            </InputGroupAddon>
            <InputGroupInput
              ref={chatSearchRef}
              type="search"
              value={chatQuery}
              onChange={(e) => setChatQuery(e.target.value)}
              aria-label="Search chats"
              placeholder="Search chats…"
              className="[&_input]:max-md:!min-h-11 [&_input]:md:!h-8 [&_input]:text-xs"
            />
            {chatQuery ? (
              <InputGroupAddon align="inline-end" className="has-[>button]:me-0 pe-1">
                <Button
                  variant="ghost"
                  size="icon-xs"
                  onClick={() => setChatQuery("")}
                  aria-label="Clear chat search"
                  className="size-6 max-md:size-11"
                >
                  <X className="size-3.5" strokeWidth={1.5} aria-hidden />
                </Button>
              </InputGroupAddon>
            ) : (
              <InputGroupAddon align="inline-end" className="has-[>kbd:last-child]:me-0 pe-1.5">
                <Kbd className="px-1 font-mono text-xs">/</Kbd>
              </InputGroupAddon>
            )}
          </InputGroup>

          <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
            {chatsError && !chats ? (
              <div className="flex flex-col items-start gap-2 rounded-card border border-hairline bg-board p-2.5 text-xs text-ink-2">
                <span>Could not load chats from server.</span>
                <Button variant="outline" size="xs" onClick={() => void refreshAll()} className="max-md:min-h-11">
                  Retry chats
                </Button>
              </div>
            ) : filteredChats.length === 0 ? (
              <div className="flex flex-col items-start gap-2 rounded-card border border-hairline bg-board/50 p-2.5 text-xs text-ink-2">
                <span>No chats match &ldquo;{chatQuery}&rdquo;.</span>
                {chatQuery && (
                  <Button variant="outline" size="xs" onClick={() => setChatQuery("")} className="max-md:min-h-11">
                    Show all chats
                  </Button>
                )}
              </div>
            ) : (
              <LayoutGroup id="chat-rail">
                <ul id="chat-list" className="flex flex-col gap-0.5" aria-label="Your chats">
                  {filteredChats.map((c: Chat) => {
                    const isCurrent = c.id === activeId;
                    return (
                      <li key={c.id} className="relative">
                        {isCurrent && (
                          <m.span
                            layoutId="chat-rail-active"
                            transition={reduceMotion ? instant : glide}
                            aria-hidden
                            className="pointer-events-none absolute inset-0 rounded-card bg-blue-soft"
                          />
                        )}
                        <Button
                          variant="ghost"
                          data-id={c.id}
                          aria-current={isCurrent ? "true" : undefined}
                          onClick={() => void switchChat(c.id)}
                          className="chat-btn relative z-10 h-auto min-h-8 w-full justify-between rounded-card border-transparent bg-transparent px-2 py-1 text-left font-normal text-ink-2 hover:bg-board/50 hover:text-ink aria-[current=true]:font-medium aria-[current=true]:text-ink max-md:min-h-11"
                        >
                          <span className="min-w-0 flex-1 truncate text-xs">{c.title || "New chat"}</span>
                          {inflight[c.id] && (
                            <Pill className="bg-surface px-1.5 py-0 text-xs text-ink">working</Pill>
                          )}
                        </Button>
                      </li>
                    );
                  })}
                </ul>
              </LayoutGroup>
            )}
          </div>
        </div>
      )}

      {/* Compact Left Rail Footer */}
      <div className="flex flex-none flex-col gap-1.5 border-t border-hairline pt-2">
        <div className="grid grid-cols-2 gap-1.5">
          <Button
            variant="outline"
            size="xs"
            onClick={() => {
              setMenuOpen(false);
              setToolLibraryOpen(true);
              void refreshLibrary();
              void refreshMarketTaste();
            }}
            aria-label="Open Tool Library"
            className="justify-start gap-1.5 bg-board/50 px-2 font-normal text-ink hover:bg-board max-md:min-h-11"
          >
            <Package className="size-3.5 shrink-0 text-icon" strokeWidth={1.5} aria-hidden />
            <span className="truncate">Tools</span>
          </Button>
          <Button
            variant="outline"
            size="xs"
            onClick={() => {
              setMenuOpen(false);
              setSnapshotsOpen(true);
            }}
            aria-label="Open Snapshots"
            className="justify-start gap-1.5 bg-board/50 px-2 font-normal text-ink hover:bg-board max-md:min-h-11"
          >
            <Layers className="size-3.5 shrink-0 text-icon" strokeWidth={1.5} aria-hidden />
            <span className="truncate">Snapshots</span>
          </Button>
        </div>

        <div className="flex items-center justify-between gap-2 px-0.5 text-xs text-ink-3">
          <Button
            variant="ghost"
            size="xs"
            onClick={() => {
              setMenuOpen(false);
              setWizardOpen(true);
            }}
            aria-label="Studio connection setup"
            className="h-auto p-0 hover:bg-transparent"
          >
            <StudioChip connected={studioOk} workspaceMode={workspaceMode} />
          </Button>
          <a
            href={RELEASES_URL + (version || "1.26.0")}
            target="_blank"
            rel="noreferrer"
            className="inline-flex min-h-7 items-center gap-1 rounded-sm text-xs text-ink-2 outline-none hover:text-ink hover:underline active:opacity-80 focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11"
          >
            <span>v<span className="font-mono">{version || "1.26.0"}</span></span>
            <ExternalLink className="size-3 text-icon" strokeWidth={1.5} aria-hidden />
          </a>
        </div>
      </div>
    </nav>
  );

  // Tabbed Right Workbench (Viewport, Code & Diff, Assets, Ship & Monetize, Swarm + Bottom Drawer)
  const rightWorkbench = (
    <section
      aria-label="Studio Workbench"
      data-studio-workspace
      className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-surface"
    >
      {/* Workbench Tab Header */}
      <div className="flex min-h-10 flex-none flex-wrap items-center justify-between gap-1.5 border-b border-hairline bg-board/40 px-2.5 py-1">
        <SegmentedControl aria-label="Studio workbench tab" size="sm" className="max-w-full flex-wrap">
          <SegmentedControlItem
            data-checked={centerTab === "embedded-studio" || centerTab === "studio-3d" ? "" : undefined}
            aria-pressed={centerTab === "embedded-studio" || centerTab === "studio-3d"}
            onClick={() => setCenterTab(isBrowserEngine ? "studio-3d" : "embedded-studio")}
            className="gap-1 max-md:!h-11 max-md:px-2"
          >
            <Monitor className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Viewport</span>
          </SegmentedControlItem>
          <SegmentedControlItem
            data-checked={centerTab === "code-diff" ? "" : undefined}
            aria-pressed={centerTab === "code-diff"}
            onClick={() => setCenterTab("code-diff")}
            className="gap-1 max-md:!h-11 max-md:px-2"
          >
            <FileDiff className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Code</span>
            {pendingProposalCount > 0 && (
              <span className="rounded-chip bg-blue px-1.5 py-0 font-mono text-xs tabular-nums text-blue-fg">
                {pendingProposalCount}
              </span>
            )}
          </SegmentedControlItem>
          <SegmentedControlItem
            data-checked={centerTab === "gui-designer" ? "" : undefined}
            aria-pressed={centerTab === "gui-designer"}
            onClick={() => setCenterTab("gui-designer")}
            className="gap-1 max-md:!h-11 max-md:px-2"
          >
            <LayoutTemplate className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>UI</span>
          </SegmentedControlItem>
          {!isBrowserEngine && (
            <SegmentedControlItem
              data-checked={centerTab === "assets" ? "" : undefined}
              aria-pressed={centerTab === "assets"}
              onClick={() => setCenterTab("assets")}
              className="gap-1 max-md:!h-11 max-md:px-2"
            >
              <Images className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>Assets</span>
            </SegmentedControlItem>
          )}
          <SegmentedControlItem
            data-checked={centerTab === "ship" ? "" : undefined}
            aria-pressed={centerTab === "ship"}
            onClick={() => setCenterTab("ship")}
            className="gap-1 max-md:!h-11 max-md:px-2"
          >
            <ShipIcon className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Ship</span>
          </SegmentedControlItem>
          {!isBrowserEngine && (
            <SegmentedControlItem
              data-checked={centerTab === "teamwork" ? "" : undefined}
              aria-pressed={centerTab === "teamwork"}
              onClick={() => setCenterTab("teamwork")}
              className="gap-1 max-md:!h-11 max-md:px-2"
            >
              <Users className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>Swarm</span>
            </SegmentedControlItem>
          )}
        </SegmentedControl>

        <div className="flex items-center gap-1.5">
          <Button
            variant="outline"
            size="xs"
            data-studio-sync-toolbar-btn
            onClick={() => setTwoWaySyncOpen(true)}
            aria-label="Two-Way Studio Sync"
            className="gap-1 bg-surface max-md:min-h-11"
          >
            <ArrowLeftRight className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Two-Way Sync</span>
          </Button>
          {!narrow && (
            <Button
              variant="ghost"
              size="icon-xs"
              onClick={() => setRightDockCollapsed(true)}
              aria-label="Collapse Workbench"
              className="size-8 text-ink-2 hover:text-ink"
            >
              <PanelRightClose className="size-3.5" strokeWidth={1.5} aria-hidden />
            </Button>
          )}
        </div>
      </div>

      {/* Active Workbench Surface */}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden p-2.5">
        <PanelErrorBoundary
          key={centerTab}
          panelName={
            centerTab === "gui-designer"
              ? "ScreenGui Designer"
              : centerTab === "studio-3d"
                ? "Studio 3D Viewport"
                : centerTab === "code-diff"
                  ? "Code Workbench"
                  : centerTab === "ship" || centerTab === "assets"
                    ? "Ship & Monetize"
                    : "Workbench"
          }
          onReset={() => void refreshPlace()}
        >
        <Suspense fallback={<div className="flex flex-1 items-center justify-center text-sm text-ink-2">Loading panel…</div>}>
          {centerTab === "embedded-studio" ? (
            <EmbeddedStudioPanel
              studioMode={studioMode}
              embedStatus={embedStatus}
              hasLiveFrame={hasLiveFrame}
              frameSeq={frameSeqRef.current}
              subscribeFrameSeq={subscribeFrameSeq}
              actions={studioActions}
              onLaunchEmbed={handleLaunchEmbed}
              onAttachEmbed={handleAttachEmbed}
              onTransferEmbed={handleTransferEmbed}
              onStopEmbed={handleStopEmbed}
              onRefreshEmbedStatus={() => void refreshEmbedStatus(true)}
              onCaptureFrame={handleCaptureStudioFrame}
              onConnectMcp={handleConnectMcp}
              onDisconnectMcp={handleDisconnectMcp}
              onSendMouseInput={handleSendMouseInput}
              onSendKeyboardInput={handleSendKeyboardInput}
              onInvokeStudioTool={handleInvokeStudioTool}
              onOpen3DPreview={() => setCenterTab("studio-3d")}
            />
          ) : centerTab === "studio-3d" ? (
            <StudioViewport3D
              place={safePlace}
              theme={theme}
              assets={projectAssets}
              meshes={projectWorkspace?.meshes}
              lighting={projectWorkspace?.lighting}
              projectId={ship.activeId}
              selectedPath={selectedPath}
              studioMode={studioMode}
              actions={studioActions}
              building={Boolean(building)}
              canRedo={canRedo || Boolean(projectWorkspace?.can_redo)}
              hideConsole
              onSelectPart={handleSelectPartAndInspect}
              onSaveInstance={handleSaveInstance}
              onCreatePart={handleCreatePart}
              onDuplicatePart={handleDuplicatePart}
              onDeletePart={handleDeletePart}
              onTriggerRedo={handleRedo}
              onRunPlaytest={handleRunStudioPlaytest}
            />
          ) : centerTab === "teamwork" ? (
            <TeamworkPanel projectId={ship.activeId} />
          ) : centerTab === "gui-designer" ? (
            <GuiDesignerWorkbench
              projectId={ship.activeId}
              place={safePlace}
              assets={projectAssets}
              products={ship.detail?.products}
              onSavedScreenOrLuau={() => { void refreshPlace(); }}
            />
          ) : centerTab === "assets" || centerTab === "ship" ? (
            <ShipPanel
              ship={ship}
              studioOk={studioOk}
              place={place}
              initialSection={centerTab === "assets" ? "assets" : shipSection}
              onOpenProjects={() => {
                setLeftDockCollapsed(false);
                setLeftDockTab("projects");
                if (narrow) setMenuOpen(true);
              }}
              onOpenSettings={() => setSettingsOpen(true)}
              onInstalledMonetization={() => { void refreshPlace(); }}
              onRunPlaytest={() => handleRunStudioPlaytest()}
              onOpenCodeDiff={(p) => handleOpenScriptInCodeDiff(p)}
              onOpenViewport={() => setCenterTab("studio-3d")}
            />
          ) : (
            <CodeDiffWorkbench
              place={safePlace}
              selectedScriptPath={selectedScriptPath}
              onSelectScriptPath={setSelectedScriptPath}
              proposals={proposals}
              activeProposalId={activeProposalId}
              onSelectProposalId={setActiveProposalId}
              latestFileDiffs={latestFileDiffs}
              serverDiagnostics={serverDiagnostics}
              busy={busy}
              projectMode={Boolean(ship.activeId)}
              scriptMeta={projectWorkspace?.scripts}
              onSaveScript={handleSaveScript}
              onRunPlaytest={handleRunStudioPlaytest}
              onApplyProposal={handleApplyProposal}
              onRejectProposal={handleRejectProposal}
              onAttachContext={handleAddContextPath}
            />
          )}
        </Suspense>
        </PanelErrorBoundary>
      </div>

      {/* Collapsible Bottom Console / MCP Tools / Properties Drawer */}
      <PanelErrorBoundary panelName="Studio Drawer">
      <StudioBottomDrawer
        open={bottomDrawerOpen}
        onToggle={() => setBottomDrawerOpen((v) => !v)}
        activeTab={bottomDrawerTab}
        onTabChange={setBottomDrawerTab}
        place={safePlace}
        selectedPath={selectedPath}
        studioActions={studioActions}
        lighting={projectWorkspace?.lighting}
        projectMode={Boolean(ship.activeId)}
        onSelectPath={(p) => {
          setSelectedPath(p);
          setCenterTab("studio-3d");
        }}
        onOpenScript={(p) => handleOpenScriptInCodeDiff(p)}
        onAttachContext={handleAddContextPath}
        onSaveInstance={handleSaveInstance}
        onSaveLighting={ship.activeId ? handleSaveLighting : undefined}
        onDuplicatePart={handleDuplicatePart}
        onDeletePart={handleDeletePart}
        onInvokeStudioTool={handleInvokeStudioTool}
      />
      </PanelErrorBoundary>
    </section>
  );

  const handleQuickCreatorHubSetup = async (): Promise<CreatorHubSetupResult | void> => {
    const endpoint = ship.activeId
      ? projUrl(ship.activeId, "/creator-hub/setup")
      : "/api/creator-hub/setup";
    const r = await apiCall<CreatorHubSetupResult>(endpoint, {});
    if (r.ok && r.status === "ready") {
      toast.success("Creator Hub & Open Cloud linked");
      void ship.refresh();
      void ship.refreshCloud();
    } else if (r.status === "action_required") {
      toast.info(r.message || "Finish the prompt in the open Creator Hub browser tab, then retry.");
    } else {
      toast.error(r.error || r.message || "Could not reach open Creator Hub browser.");
    }
    return r;
  };

  const handleInstallMonetizationPack = async () => {
    try {
      const endpoint = ship.activeId
        ? `/api/projects/${encodeURIComponent(ship.activeId)}/monetization/pack`
        : "/api/monetization/pack";
      const res = await post<{ ok?: boolean; error?: string }>(endpoint, {
        tier_preset: "starter_obby",
        currency: "Coins",
        daily_base_reward: 100,
      });
      if (res.ok === false) {
        toast.error(res.error || "Could not install Monetization Pack.");
        return;
      }
      await refreshPlace();
      setCenterTab("ship");
      toast.success("Installed Monetization & Retention Luau Pack");
    } catch {
      toast.error("Could not install Monetization Pack.");
    }
  };

  // Central Focused Agent / Plan / Ask Thread
  const centerCopilot = (
    <main
      aria-label="OpenRO Studio Copilot"
      data-ropilot-copilot
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-canvas text-ink"
    >
      {/* Copilot Header */}
      <div className="flex h-10 flex-none items-center justify-between gap-2 border-b border-hairline bg-surface px-3.5">
        <div className="flex min-w-0 items-center gap-2">
          <Sparkles className="size-4 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
          <h2 className="truncate text-sm font-medium text-ink" title={activeTitle}>
            {activeTitle}
          </h2>
          {pendingProposalCount > 0 && (
            <Pill className="bg-pill-warn-bg px-1.5 py-0 text-xs text-pill-warn-fg">
              <span className="font-mono tabular-nums">{pendingProposalCount}</span> pending diff
            </Pill>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-1.5">
          <Button
            variant="ghost"
            size="xs"
            onClick={() => setStartersOpen(true)}
            className="gap-1 text-xs text-ink-2 hover:text-ink max-md:min-h-11"
          >
            <Layers className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Starters</span>
          </Button>
          <Button
            variant="outline"
            size="xs"
            onClick={() => void newChat()}
            aria-label="New chat"
            className="gap-1 bg-surface text-xs max-md:min-h-11"
          >
            <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>New chat</span>
          </Button>
        </div>
      </div>

      {/* Scrollable Thread */}
      <div
        id="thread"
        ref={threadRef}
        aria-live="polite"
        aria-busy={!chats && !chatsError}
        className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4"
      >
        {activeSwarmId && (
          <SwarmSidebarFeed
            runId={activeSwarmId}
            initial={activeSwarmSnap}
            projectId={ship.activeId}
            onOpenFullSwarm={() => {
              setCenterTab("teamwork");
              setRightDockCollapsed(false);
              setMobilePane("studio");
            }}
            onPrefillPrompt={(p) => {
              setText(p);
              inputRef.current?.focus();
            }}
            onQuickCreatorHubSetup={handleQuickCreatorHubSetup}
            onOpenShip={() => {
              setCenterTab("ship");
              setRightDockCollapsed(false);
              setMobilePane("studio");
            }}
          />
        )}

        {threadError ? (
          <div role="alert" className="flex flex-col items-start gap-2.5 rounded-card border border-hairline bg-surface p-4">
            <strong className="text-sm font-medium text-pill-pro-fg">Could not load the conversation.</strong>
            <p className="text-sm text-ink-2">The OpenRO server did not answer. Try again or reload.</p>
            <Button variant="default" size="sm" onClick={() => void refreshAll()} className="max-md:min-h-11">
              Try again
            </Button>
          </div>
        ) : !hasConversation && proposals.length === 0 && !activeSwarmId ? (
          <div className="mx-auto my-auto flex w-full max-w-[680px] flex-col items-center justify-center gap-5 py-6 text-center" data-copilot-welcome>
            <div className="flex flex-col items-center gap-2">
              <span className="grid size-10 place-items-center rounded-xl border border-hairline bg-surface">
                <OpenRoLogo className="size-7" />
              </span>
              <h3 className="text-lg font-medium tracking-tight text-ink">What do you want to build in Studio?</h3>
              <p className="max-w-[520px] text-sm text-ink-2">
                Describe any game mechanic, inspect Luau diffs side-by-side, or launch a ready starter into Roblox Studio.
              </p>
            </div>

            {/* Single compact horizontal row of 1-click starter pills */}
            <div className="flex flex-wrap items-center justify-center gap-2" data-starter-row>
              {workbenchIdeas.slice(0, 3).map((idea, idx) => (
                <Button
                  key={idea.id}
                  variant={idx === 0 ? "default" : "outline"}
                  size="sm"
                  disabled={!canBuild || busy}
                  onClick={() => void buildIdea(idea)}
                  aria-label={`Build ${idea.title} in Studio`}
                  className="max-md:min-h-11"
                >
                  <Hammer className="size-3.5" strokeWidth={1.5} aria-hidden />
                  <span>{idea.title}</span>
                </Button>
              ))}
              <Button
                variant="outline"
                size="sm"
                disabled={!studioOk && !ship.activeId}
                onClick={() => void handleInstallMonetizationPack()}
                aria-label="Install Monetization Pack"
                className="max-md:min-h-11"
              >
                <Coins className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                <span>Monetization Pack</span>
              </Button>
            </div>

            {/* Subtle prompt fill suggestions */}
            <div className="flex flex-wrap items-center justify-center gap-1.5" data-prompt-chips>
              {[
                { label: "Anime card battler", prompt: "Build a polished anime card battle game with packs, equip deck, and NPC fights" },
                { label: "Obby with coins", prompt: "Build an obby with 4 checkpoints, jump pads, and coin leaderstats" },
                { label: "Boss fight arena", prompt: "Build a boss fight arena with melee sword, health bar, and wave phases" },
                { label: "VIP GamePass & daily streak", prompt: "Add a VIP GamePass, ProcessReceipt developer product, and 7-day daily reward streak" },
              ].map((chip) => (
                <Button
                  key={chip.label}
                  variant="ghost"
                  size="xs"
                  onClick={() => {
                    setText(chip.prompt);
                    inputRef.current?.focus();
                  }}
                  className="border border-hairline bg-surface/75 text-xs font-normal text-ink-2 hover:bg-surface hover:text-ink max-md:min-h-11"
                >
                  {chip.label}
                </Button>
              ))}
            </div>
          </div>
        ) : (
          (hasConversation || proposals.length > 0) && (
            <ThreadFeed
              lines={lines}
              notices={notices}
              pending={building}
              ideas={ideas}
              builtTitles={builtTitles}
              canBuild={canBuild}
              busy={busy}
              buildingIdea={building?.build}
              freshFrom={lines.length}
              lastTriedIdea={lastTriedIdea}
              buildPhase={buildPhase}
              latestFileDiffs={latestFileDiffs}
              proposals={proposals}
              capturedFrameUrl={hasLiveFrame && threadFrameSeq > 0 ? `/api/studio/frame?seq=${threadFrameSeq}` : null}
              projectId={ship.activeId}
              onPrefillPrompt={(p) => {
                setText(p);
                inputRef.current?.focus();
              }}
              onQuickCreatorHubSetup={handleQuickCreatorHubSetup}
              onOpenShip={() => {
                setCenterTab("ship");
                setRightDockCollapsed(false);
                setMobilePane("studio");
              }}
              onBuild={(idea) => void buildIdea(idea)}
              onOpenScriptDiff={(p) => handleOpenScriptInCodeDiff(p)}
              onInspectProposal={(pid, scriptPath) => {
                setActiveProposalId(pid);
                if (scriptPath) setSelectedScriptPath(scriptPath);
                setCenterTab("code-diff");
                setRightDockCollapsed(false);
                setMobilePane("studio");
              }}
              onApplyProposal={(pid) => void handleApplyProposal(pid)}
              onRejectProposal={(pid) => void handleRejectProposal(pid)}
            />
          )
        )}
      </div>

      {/* Background Plan-Execute Run Queue & Checkpoints */}
      {!isBrowserEngine && (
        <AgentRunsPanel
          chatId={activeId}
          projectId={ship.activeId}
          composerText={text}
          onClearComposer={() => setText("")}
          onRunFinished={() => void refreshAll()}
          refreshSignal={runsRefreshSignal}
        />
      )}

      {/* Docked Copilot Composer */}
      <CopilotComposer
        text={text}
        onTextChange={setText}
        inputRef={inputRef}
        busy={busy}
        swarmRunning={activeSwarmSnap?.run.status === "running"}
        onStopSwarm={() => void handleStopSwarm()}
        canUndo={canUndo}
        place={place}
        contextPaths={contextPaths}
        pluginSelection={pluginStatus.selection}
        onAddContextPath={handleAddContextPath}
        onRemoveContextPath={handleRemoveContextPath}
        mode={composerMode}
        onModeChange={setComposerMode}
        reviewDiffs={reviewDiffs}
        onReviewDiffsChange={setReviewDiffs}
        reasoning={reasoning}
        onReasoningChange={setReasoning}
        modelLabel={activeModelLabel}
        onModelSelected={(label) => {
          setActiveModelLabel(label);
          setModelOk(true);
        }}
        onOpenSettings={() => setSettingsOpen(true)}
        onSubmit={send}
        onConfirmUndo={() => void undoNow()}
        onQueuePlan={isBrowserEngine ? undefined : () => void handleQueuePlan(text)}
      />
    </main>
  );

  return (
    <LazyMotion features={domMax}>
    <MotionConfig reducedMotion="user">
    <div
      className="relative flex h-dvh min-h-0 w-full flex-col overflow-hidden bg-canvas text-ink"
      data-app="openro"
      data-ide-layout="studio-copilot-3pane"
      onDragOver={onAppDragOver}
      onDragLeave={onAppDragLeave}
      onDrop={onAppDrop}
    >
      <input
        ref={fileInputRef}
        type="file"
        accept=".rbxlx,.xml"
        hidden
        onChange={onFileInputChange}
      />
      <input
        ref={modelFileInputRef}
        type="file"
        accept=".rbxmx,.rbxm,.rbxlx"
        hidden
        data-app-import-model-input
        onChange={onModelInputChange}
      />

      {dragActive && (
        <div
          role="status"
          className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center bg-canvas/85 p-6"
        >
          <div className="flex flex-col items-center gap-2 rounded-panel border-2 border-dashed border-blue bg-surface px-8 py-6 text-center shadow-lift">
            <Upload className="size-6 text-blue" strokeWidth={1.5} aria-hidden />
            <p className="text-base font-medium text-ink">Drop .rbxlx, .rbxmx, or .rbxm file to import</p>
            <p className="text-sm text-ink-2">Parses Workspace parts, CFrame/Size/Color, prefab models, and Luau scripts directly into Studio.</p>
          </div>
        </div>
      )}

      <a className="skip" href="#composer-input">
        Skip to message box
      </a>

      {/* Slim 44px Top Chrome */}
      <header
        data-top-chrome
        className="flex h-11 flex-none items-center justify-between gap-2 border-b border-hairline bg-surface px-3"
      >
        <div className="flex min-w-0 items-center gap-2">
          {narrow ? (
            <Drawer.Root open={menuOpen} onOpenChange={setMenuOpen} direction="left" autoFocus>
              <Drawer.Trigger asChild>
                <Button
                  ref={menuKeyRef}
                  variant="ghost"
                  size="icon"
                  aria-label="Open the menu"
                  className="size-11 shrink-0"
                >
                  <Menu className="size-5" strokeWidth={1.5} aria-hidden />
                </Button>
              </Drawer.Trigger>
              <Drawer.Portal>
                <Drawer.Overlay className="fixed inset-0 z-40 bg-ink/30" />
                <Drawer.Content
                  onEscapeKeyDown={(e) => {
                    if (settingsOpen || paletteOpen || snapshotsOpen || toolLibraryOpen || pluginDialogOpen || startersOpen) {
                      e.preventDefault();
                      return;
                    }
                    setMenuOpen(false);
                  }}
                  onCloseAutoFocus={(e) => {
                    e.preventDefault();
                    (menuKeyRef.current ?? document.querySelector<HTMLElement>('[aria-label="Open the menu"]'))?.focus();
                  }}
                  className="fixed inset-y-0 left-0 z-50 w-[min(86vw,288px)] border-r border-hairline bg-surface outline-none"
                >
                  <Drawer.Title className="sr-only">Explorer, Chats, and Projects</Drawer.Title>
                  <Drawer.Description className="sr-only">
                    Browse Studio Explorer, switch chats, open Tool Library, or inspect snapshots.
                  </Drawer.Description>
                  {leftDockBody}
                </Drawer.Content>
              </Drawer.Portal>
            </Drawer.Root>
          ) : (
            <Button
              variant="ghost"
              size="icon-xs"
              onClick={() => setLeftDockCollapsed((v) => !v)}
              aria-label={leftDockCollapsed ? "Expand left dock" : "Collapse left dock"}
              className="size-8 text-ink-2 hover:text-ink"
            >
              {leftDockCollapsed ? (
                <PanelLeftOpen className="size-4" strokeWidth={1.5} aria-hidden />
              ) : (
                <PanelLeftClose className="size-4" strokeWidth={1.5} aria-hidden />
              )}
            </Button>
          )}

          <div className="flex min-w-0 items-center gap-2">
            <span className="grid size-7 flex-none place-items-center rounded-lg border border-hairline bg-board">
              <OpenRoLogo className="size-5" />
            </span>
            <h1 className="truncate text-sm font-medium text-ink">OpenRO</h1>
            <span className="hidden truncate rounded-chip bg-board px-2 py-0.5 text-xs text-ink-2 sm:inline-block" title={ship.active?.name || shell.gameName}>
              {ship.active?.name || shell.gameName}
            </span>
          </div>

          <div className="hidden items-center gap-1.5 md:flex" data-answers="2">
            <StudioChip connected={studioOk} workspaceMode={workspaceMode} />
            {!studioOk && (
              <Button
                variant="outline"
                size="xs"
                onClick={() => void checkStudio()}
                className="gap-1 bg-surface text-xs"
              >
                <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
                <span>Check again</span>
              </Button>
            )}
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-1.5">
          {narrow && (
            <SegmentedControl aria-label="Switch Copilot or Workbench view" size="sm">
              <SegmentedControlItem
                data-checked={mobilePane === "copilot" ? "" : undefined}
                aria-pressed={mobilePane === "copilot"}
                onClick={() => setMobilePane("copilot")}
                className="gap-1 max-md:!h-11 max-md:px-2.5"
              >
                <Sparkles className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Copilot</span>
              </SegmentedControlItem>
              <SegmentedControlItem
                data-checked={mobilePane === "studio" ? "" : undefined}
                aria-pressed={mobilePane === "studio"}
                onClick={() => setMobilePane("studio")}
                className="gap-1 max-md:!h-11 max-md:px-2.5"
              >
                <Monitor className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Workbench</span>
                {pendingProposalCount > 0 && (
                  <span className="rounded-chip bg-blue px-1 font-mono text-xs text-blue-fg">
                    {pendingProposalCount}
                  </span>
                )}
              </SegmentedControlItem>
            </SegmentedControl>
          )}

          {!narrow && antigravityIssue && (
            <Button
              variant="outline"
              size="xs"
              onClick={() => setSettingsOpen(true)}
              data-top-quota-issue
              className="border-hairline bg-pill-warn-bg text-pill-warn-fg hover:bg-pill-warn-bg"
            >
              <span>{antigravityIssue.resets_in ? `Quota (${antigravityIssue.resets_in})` : antigravityIssue.message}</span>
            </Button>
          )}

          {!narrow && (
            <Button
              variant="outline"
              size="xs"
              disabled={totalPieces === 0}
              onClick={() => void handleRunStudioPlaytest()}
              aria-label="Run Studio playtest"
              className="bg-surface"
            >
              <Play className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>Play</span>
            </Button>
          )}

          {!narrow && (
            <Popover.Root>
              <Popover.Trigger asChild>
                <Button
                  variant="outline"
                  size="icon-sm"
                  aria-label="Project export and import menu"
                  className="shrink-0 bg-surface"
                >
                  <MoreHorizontal className="size-4" strokeWidth={1.5} aria-hidden />
                </Button>
              </Popover.Trigger>
              <Popover.Portal>
                <Popover.Content
                  align="end"
                  sideOffset={6}
                  className="z-50 flex w-44 flex-col gap-1 rounded-card border border-hairline bg-surface p-1.5 shadow-lift"
                >
                  <Button
                    variant="ghost"
                    size="xs"
                    disabled={totalPieces === 0}
                    onClick={() => void handleExportRbxlx()}
                    className="w-full justify-start gap-2"
                  >
                    <Download className="size-3.5" strokeWidth={1.5} aria-hidden />
                    <span>Export .rbxlx</span>
                  </Button>
                  <Button
                    variant="ghost"
                    size="xs"
                    disabled={totalPieces === 0}
                    onClick={() => void handleDownloadShipPack()}
                    className="w-full justify-start gap-2"
                  >
                    <FolderArchive className="size-3.5" strokeWidth={1.5} aria-hidden />
                    <span>Rojo .zip</span>
                  </Button>
                  <Button
                    variant="ghost"
                    size="xs"
                    onClick={() => fileInputRef.current?.click()}
                    className="w-full justify-start gap-2"
                  >
                    <Upload className="size-3.5" strokeWidth={1.5} aria-hidden />
                    <span>Import .rbxlx</span>
                  </Button>
                  <Button
                    variant="ghost"
                    size="xs"
                    data-menu-import-model
                    onClick={() => modelFileInputRef.current?.click()}
                    className="w-full justify-start gap-2"
                  >
                    <FolderUp className="size-3.5" strokeWidth={1.5} aria-hidden />
                    <span>Import Model</span>
                  </Button>
                </Popover.Content>
              </Popover.Portal>
            </Popover.Root>
          )}

          {!narrow && (
            <Button
              variant="ghost"
              size="icon-xs"
              onClick={() => setPaletteOpen(true)}
              aria-label="Open command palette"
              className="size-8 text-ink-2 hover:text-ink"
            >
              <Command className="size-3.5" strokeWidth={1.5} aria-hidden />
            </Button>
          )}

          <Button
            variant="outline"
            size="xs"
            aria-label="Settings"
            onClick={() => setSettingsOpen(true)}
            className="gap-1.5 bg-surface px-2 font-medium text-ink max-md:min-h-11"
          >
            <SlidersHorizontal className="size-3.5 shrink-0 text-icon" strokeWidth={1.5} aria-hidden />
            <span className="hidden sm:inline">Settings</span>
          </Button>

          {!narrow && (
            <Button
              variant="ghost"
              size="icon-xs"
              onClick={() => setRightDockCollapsed((v) => !v)}
              aria-label={rightDockCollapsed ? "Expand Workbench" : "Collapse Workbench"}
              className="size-8 text-ink-2 hover:text-ink"
            >
              {rightDockCollapsed ? (
                <PanelRightOpen className="size-4" strokeWidth={1.5} aria-hidden />
              ) : (
                <PanelRightClose className="size-4" strokeWidth={1.5} aria-hidden />
              )}
            </Button>
          )}
        </div>
      </header>

      <div
        className="server-banners"
        onClick={onBannerClick}
        dangerouslySetInnerHTML={{ __html: shell.bannerHtml }}
      />
      {update.show && <UpdateStrip u={update} />}

      {/* Main 3-Pane Studio Copilot IDE */}
      <div className="flex min-h-0 flex-1 overflow-hidden">
        {/* Pane 1: Collapsible Left Project / Explorer / Chats Rail */}
        <AnimatePresence initial={false}>
          {!narrow && !leftDockCollapsed && (
            <m.div
              key="left-dock-pane"
              initial={reduceMotion ? { opacity: 1 } : { opacity: 0, x: -6 }}
              animate={{ opacity: 1, x: 0 }}
              exit={reduceMotion ? { opacity: 0 } : { opacity: 0, x: -6 }}
              transition={reduceMotion ? instant : dialogSpring}
              className="w-[260px] flex-none border-r border-hairline"
            >
              {leftDockBody}
            </m.div>
          )}
        </AnimatePresence>

        {/* Pane 2: Focused Central Copilot Thread */}
        {(!narrow || mobilePane === "copilot") && centerCopilot}

        {/* Pane 3: Tabbed Right Studio Workbench */}
        {!narrow ? (
          <AnimatePresence initial={false}>
            {!rightDockCollapsed && (
              <m.div
                key="right-workbench-pane"
                initial={reduceMotion ? { opacity: 1 } : { opacity: 0, x: 6 }}
                animate={{ opacity: 1, x: 0 }}
                exit={reduceMotion ? { opacity: 0 } : { opacity: 0, x: 6 }}
                transition={reduceMotion ? instant : dialogSpring}
                className="flex w-[450px] flex-none border-l border-hairline xl:w-[500px]"
              >
                {rightWorkbench}
              </m.div>
            )}
          </AnimatePresence>
        ) : (
          mobilePane === "studio" && rightWorkbench
        )}
      </div>

      {/* Starters & 28 Game Systems Dialog */}
      <MorphingDialog open={startersOpen} onOpenChange={setStartersOpen} transition={dialogSpring}>
          <MorphingDialogContainer>
            <MorphingDialogContent className="relative mx-4 flex max-h-[86dvh] w-full max-w-[920px] flex-col overflow-hidden rounded-dialog border border-hairline bg-surface text-ink shadow-lift">
              <div className="flex flex-none items-center justify-between gap-2 border-b border-hairline bg-board/60 px-5 py-3.5">
                <div className="min-w-0">
                  <MorphingDialogTitle className="text-base font-medium text-ink">
                    Starters &amp; 28 Roblox Game Systems
                  </MorphingDialogTitle>
                  <MorphingDialogDescription className="text-xs text-ink-2">
                    Launch a full playable starter place or compose individual Luau systems directly into Studio.
                  </MorphingDialogDescription>
                </div>
                <MorphingDialogClose
                  aria-label="Close Starters dialog"
                  className="static inline-flex size-8 items-center justify-center rounded-md text-ink-2 outline-none hover:bg-board hover:text-ink max-md:size-11"
                >
                  <X className="size-4" strokeWidth={1.5} aria-hidden />
                </MorphingDialogClose>
              </div>
              <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-5">
                {(startersOpen || openedDialogs.starters) && (
                  <Suspense fallback={<div className="p-2 text-sm text-ink-2">Loading starters…</div>}>
                    <BlueprintWorkbench
                      ideas={workbenchIdeas}
                      builtTitles={builtTitles}
                      canBuild={canBuild}
                      cantBuildWhy={cantBuildWhy}
                      busy={busy}
                      buildingTitle={building?.build}
                      onBuild={(idea) => void buildIdea(idea)}
                    />
                    <SystemLibrary
                      place={place}
                      canBuild={canBuild}
                      busy={busy}
                      buildingTitle={building?.build}
                      onAddSystem={(sys) => void addSystem(sys)}
                    />
                  </Suspense>
                )}
              </div>
            </MorphingDialogContent>
          </MorphingDialogContainer>
        </MorphingDialog>

        {(paletteOpen || openedDialogs.palette) && (
          <Suspense fallback={null}>
            <CommandPalette
              open={paletteOpen}
              onOpenChange={setPaletteOpen}
              place={place}
              canBuild={canBuild}
              busy={busy}
              canUndo={canUndo}
              canRedo={canRedo}
              onAddSystem={(sys) => void addSystem(sys)}
              onSelectPath={handleSelectPartAndInspect}
              onNewChat={() => void newChat()}
              onExportRbxlx={() => void handleExportRbxlx()}
              onDownloadShipPack={() => void handleDownloadShipPack()}
              onAddBlockPart={() => void handleCreatePart("block")}
              onImportRbxlxClick={() => fileInputRef.current?.click()}
              onOpenSnapshots={() => setSnapshotsOpen(true)}
              onOpenSettings={() => setSettingsOpen(true)}
              onOpenToolLibrary={() => {
                setToolLibraryOpen(true);
                void refreshLibrary();
                void refreshMarketTaste();
              }}
              onOpenEmbeddedStudio={() => {
                setCenterTab("embedded-studio");
                setMobilePane("studio");
              }}
              onOpenShip={() => {
                setCenterTab("ship");
                setRightDockCollapsed(false);
                setMobilePane("studio");
              }}
              onOpenCodeDiff={() => {
                setCenterTab("code-diff");
                setMobilePane("studio");
              }}
              onOpenGuiDesigner={() => {
                setCenterTab("gui-designer");
                setRightDockCollapsed(false);
                setMobilePane("studio");
              }}
              onOpenStudioPlugin={() => {
                setPluginDialogOpen(true);
                void refreshPluginStatus(true);
              }}
              onToggleBottomDrawer={() => setBottomDrawerOpen((v) => !v)}
              onOpenStarters={() => setStartersOpen(true)}
              onRunPlaytest={() => void handleRunStudioPlaytest()}
              onTriggerUndo={() => void undoNow()}
              onTriggerRedo={() => void handleRedo()}
            />
          </Suspense>
        )}

        {(pluginDialogOpen || openedDialogs.plugin) && (
          <Suspense fallback={null}>
            <StudioPluginDialog
              open={pluginDialogOpen}
              onOpenChange={setPluginDialogOpen}
              status={pluginStatus}
              v0100Ready={v0100Ready}
              onRefreshStatus={() => refreshPluginStatus(true)}
              onAttachSelection={(paths) => {
                for (const p of paths) handleAddContextPath(p);
                toast.success(`Attached ${paths.length} selected Studio item(s) to Copilot`);
                setPluginDialogOpen(false);
              }}
            />
          </Suspense>
        )}

        {(toolLibraryOpen || openedDialogs.toolLibrary) && (
          <Suspense fallback={null}>
            <ToolLibraryDialog
              open={toolLibraryOpen}
              onOpenChange={setToolLibraryOpen}
              snapshot={librarySnap}
              marketTaste={marketTaste}
              marketBusy={marketBusy}
              v090Ready={v090Ready}
              onRefresh={() => refreshLibrary(true)}
              onSelectMarketNiche={handleSelectMarketNiche}
              onRefreshMarketTaste={handleRefreshMarketTaste}
              onPitchMarketPrompt={handlePitchMarketPrompt}
            />
          </Suspense>
        )}

        {(snapshotsOpen || openedDialogs.snapshots) && (
          <Suspense fallback={null}>
            <SnapshotsDialog
              open={snapshotsOpen}
              onOpenChange={setSnapshotsOpen}
              snapshots={snapshots}
              busy={busy}
              onCreateSnapshot={(lbl) => void handleCreateSnapshot(lbl)}
              onRestoreSnapshot={(snap) => void handleRestoreSnapshot(snap)}
              onExportSnapshot={(snap) => void handleExportSnapshot(snap)}
            />
          </Suspense>
        )}

        {(settingsOpen || openedDialogs.settings) && (
          <Suspense fallback={null}>
            <SettingsDialog
              theme={theme}
              onThemeChange={setTheme}
              open={settingsOpen}
              onOpenChange={setSettingsOpen}
              workspaceMode={workspaceMode}
              compactLabel={activeModelLabel}
              showTrigger={false}
              onOpenCloudChanged={() => void ship.refreshCloud()}
              onOpenSnapshots={() => setSnapshotsOpen(true)}
              onOpenPluginSetup={() => {
                setPluginDialogOpen(true);
                void refreshPluginStatus(true);
              }}
              onOpenWizard={() => setWizardOpen(true)}
              canCheckUpdate={update.canCheck}
              onCheckUpdate={() => void update.check()}
              onApplied={({ studioConnected, modelConfigured, workspaceMode: nextMode, modelId: nextModelId, antigravityIssue: nextIssue }) => {
                setStudioOk(studioConnected);
                setModelOk(modelConfigured);
                setWorkspaceMode(nextMode);
                if (nextModelId) {
                  setActiveModelLabel(nextModelId.trim().split("/").pop() || nextModelId.trim());
                }
                if (nextIssue !== undefined) {
                  setAntigravityIssue(nextIssue);
                }
                if (studioConnected) {
                  const noStudio = document.getElementById("no-studio-banner");
                  if (noStudio) noStudio.hidden = true;
                  void refreshPlace();
                }
                if (modelConfigured) {
                  const noKey = document.getElementById("no-key-banner");
                  if (noKey) noKey.hidden = true;
                }
              }}
            />
          </Suspense>
        )}
        {(wizardOpen || openedDialogs.wizard) && (
          <Suspense fallback={null}>
            <StartupWizard
              open={wizardOpen}
              onOpenChange={setWizardOpen}
              studioOk={studioOk}
              activeProjectId={ship.activeId}
              activeProjectName={ship.active?.name}
              onApplied={({ studioConnected, modelConfigured, workspaceMode: nextMode, modelId: nextModelId }) => {
                setStudioOk(studioConnected);
                setModelOk(modelConfigured);
                setWorkspaceMode(nextMode);
                if (nextModelId) {
                  setActiveModelLabel(nextModelId.trim().split("/").pop() || nextModelId.trim());
                }
                if (studioConnected) {
                  void refreshPlace();
                  void refreshEmbedStatus(true);
                }
              }}
              onDoneFocusComposer={() => inputRef.current?.focus()}
            />
          </Suspense>
        )}
        {(twoWaySyncOpen || openedDialogs.twoWaySync) && (
          <MorphingDialog transition={dialogSpring} open={twoWaySyncOpen} onOpenChange={setTwoWaySyncOpen}>
            <MorphingDialogContainer>
              <MorphingDialogContent
                data-two-way-sync-dialog
                className="relative mx-3 flex max-h-[86dvh] w-full max-w-[780px] flex-col gap-4 overflow-y-auto rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift"
              >
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <MorphingDialogTitle className="font-heading text-title font-semibold">
                      Two-Way Studio Sync
                    </MorphingDialogTitle>
                    <MorphingDialogDescription className="mt-1 text-sm text-ink-2">
                      Pull edits from live Roblox Studio into your project, review 3-way diffs, and resolve conflicts.
                    </MorphingDialogDescription>
                  </div>
                  <MorphingDialogClose
                    aria-label="Close"
                    className="static inline-flex min-h-11 cursor-pointer items-center rounded-lg border border-hairline bg-surface px-3 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus"
                  >
                    Close
                  </MorphingDialogClose>
                </div>
                <Suspense fallback={<div className="p-4 text-sm text-ink-2">Loading Studio sync…</div>}>
                  <SyncConflictsCard
                    projectId={ship.activeId}
                    studioOk={studioOk}
                    autoPullOnMount
                    onResolved={async () => {
                      await refreshPlace(ship.activeId);
                      await ship.refresh();
                    }}
                  />
                </Suspense>
              </MorphingDialogContent>
            </MorphingDialogContainer>
          </MorphingDialog>
        )}
        <Toaster
          position="bottom-right"
          theme={theme}
          toastOptions={{
            className: "border border-hairline bg-surface text-ink shadow-lift",
          }}
        />
    </div>
    </MotionConfig>
    </LazyMotion>
  );
}
