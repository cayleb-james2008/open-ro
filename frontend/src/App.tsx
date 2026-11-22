import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type DragEvent,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent,
} from "react";
import { MotionConfig } from "motion/react";
import { Toaster, toast } from "sonner";
import { Drawer } from "vaul";
import {
  Blocks,
  Command,
  ExternalLink,
  FolderTree,
  Menu,
  Package,
  Palette,
  Plus,
  RefreshCw,
  Search,
  Send,
  SlidersHorizontal,
  Undo2,
  Upload,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupAddon, InputGroupInput, InputGroupTextarea } from "@/components/ui/input-group";
import { Kbd } from "@/components/ui/kbd";
import { AnimatedBackground } from "@/components/ui/animated-background";
import { SlidingNumber } from "@/components/ui/sliding-number";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogDescription,
  MorphingDialogTitle,
  MorphingDialogTrigger,
} from "@/components/ui/morphing-dialog";
import { Pill } from "@/components/kibo-ui/pill";
import { glide, open } from "@/motion";
import {
  RELEASES_URL,
  SEND_FAILED,
  isV050OrNewer,
  isV060OrNewer,
  isV070OrNewer,
  isV080OrNewer,
  isV090OrNewer,
  normalizeChatList,
  normalizeEmbedStatus,
  normalizeIdeas,
  normalizeLibrarySnapshot,
  normalizePlace,
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
  type Idea,
  type InstanceEditPayload,
  type LibrarySnapshot,
  type PartPreset,
  type Place,
  type PlaceDiff,
  type PlaceSnapshot,
  type StudioActionEvent,
  type StudioEmbedStatus,
  type StudioKeyboardInputPayload,
  type StudioMode,
  type StudioMouseInputPayload,
  type StudioToolName,
  type StudioToolResult,
  type WorkspaceMode,
} from "./app/data";
import { BlueprintWorkbench, FeaturedApexBanner } from "./app/Blueprints";
import { CardArtStudio } from "./app/CardArtStudio";
import {
  APEX_CARDS,
  ART_KNOWLEDGE_FALLBACK,
  normalizeArtKnowledge,
  normalizeAssetList,
  type ArtKnowledgeGuide,
  type CardAssetRecord,
} from "./app/cardCatalog";
import { CommandPalette } from "./app/CommandPalette";
import { analyzePlaceLuau, computePlaceDiff, type LuauDiagnostic } from "./app/luauAudit";
import { parseRbxlxXml, serializeRbxlx } from "./app/rbxlxCodec";
import { buildRojoShipPackZip } from "./app/zipBuilder";
import { SettingsDialog } from "./app/SettingsDialog";
import { PartsShelf, StudioChip } from "./app/Shelf";
import { SnapshotsDialog } from "./app/SnapshotsDialog";
import { StagePreview, type ViewportTab } from "./app/StagePreview";
import { SystemLibrary } from "./app/SystemLibrary";
import type { GameSystemDef } from "./app/systems";
import { ThreadFeed } from "./app/Thread";
import { ToolLibraryDialog } from "./app/ToolLibraryDialog";
import { UpdateStrip, useUpdate } from "./app/Update";

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
    rawMode === "studio_mcp" || rawMode === "local_place" || rawMode === "browser_place" || rawMode === "disconnected"
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

export function App() {
  const shell = useMemo(readShell, []);
  const [theme, setThemeState] = useState<"light" | "dark">(() =>
    document.documentElement.dataset.theme === "dark" ? "dark" : "light",
  );
  const setTheme = (t: "light" | "dark") => {
    setThemeState(t);
    document.documentElement.setAttribute("data-theme", t);
    (window as ShellWindow).openroSetTheme?.(t);
  };

  // <768px collapses Left Rail to Vaul drawer.
  const phone = useMedia("(max-width: 767px)");
  const compactInput = useMedia("(max-width: 960px)");
  // <1100px collapses Right Explorer Shelf to Vaul bottom drawer.
  const narrow = useMedia("(max-width: 1099px)");

  const [chats, setChats] = useState<ChatList | null>(null);
  const [chatsError, setChatsError] = useState(false);
  const [lines, setLines] = useState<string[]>([]);
  const [threadError, setThreadError] = useState(false);
  const [ideas, setIdeas] = useState<Idea[]>([]);
  const [builtTitles, setBuiltTitles] = useState<string[]>([]);
  const [place, setPlace] = useState<Place | null | "error">(null);
  const [inflight, setInflight] = useState<Record<string, Pending>>({});
  const [, bump] = useState(0);
  const [text, setText] = useState("");
  const [chatQuery, setChatQuery] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const [shelfOpen, setShelfOpen] = useState(false);
  const [shelfCollapsed, setShelfCollapsed] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [snapshotsOpen, setSnapshotsOpen] = useState(false);
  const [artStudioOpen, setArtStudioOpen] = useState(false);
  const [toolLibraryOpen, setToolLibraryOpen] = useState(false);
  const [librarySnap, setLibrarySnap] = useState<LibrarySnapshot>(() => normalizeLibrarySnapshot({}));
  const [settingsOpen, setSettingsOpen] = useState(() => {
    try {
      return new URLSearchParams(window.location.search).get("settings") === "1";
    } catch {
      return false;
    }
  });
  const [studioOk, setStudioOk] = useState(shell.studio);
  const [modelOk, setModelOk] = useState(shell.canBuild || !shell.studio);
  const [workspaceMode, setWorkspaceMode] = useState<WorkspaceMode>(shell.workspaceMode);
  const [activeModelLabel, setActiveModelLabel] = useState("");
  const [version, setVersion] = useState("");
  const [v050Ready, setV050Ready] = useState(() => Boolean((window as ShellWindow).__OPENRO_WEB_ENGINE__));
  const [v060Ready, setV060Ready] = useState(() => Boolean((window as ShellWindow).__OPENRO_WEB_ENGINE__));
  const [v070Ready, setV070Ready] = useState(() => Boolean((window as ShellWindow).__OPENRO_WEB_ENGINE__));
  const [v080Ready, setV080Ready] = useState(() => Boolean((window as ShellWindow).__OPENRO_WEB_ENGINE__));
  const [v090Ready, setV090Ready] = useState(() => Boolean((window as ShellWindow).__OPENRO_WEB_ENGINE__));
  const [embedStatus, setEmbedStatus] = useState<StudioEmbedStatus>(() =>
    normalizeEmbedStatus({
      running: false,
      state: (window as ShellWindow).__OPENRO_WEB_ENGINE__ ? "unsupported" : "idle",
      play_mode: "edit",
    }),
  );
  const [embedRevealed, setEmbedRevealed] = useState(false);
  const [requestedViewportTab, setRequestedViewportTab] = useState<ViewportTab | null>(null);
  const [studioMode, setStudioMode] = useState<StudioMode>(() =>
    (window as ShellWindow).__OPENRO_WEB_ENGINE__
      ? "browser_place"
      : shell.workspaceMode === "studio_mcp"
        ? "live_mcp"
        : "local_place",
  );
  const [studioActions, setStudioActions] = useState<StudioActionEvent[]>([]);
  const [hasLiveFrame, setHasLiveFrame] = useState(false);
  const [frameSeq, setFrameSeq] = useState(0);
  const [cardAssets, setCardAssets] = useState<CardAssetRecord[]>(APEX_CARDS);
  const [artKnowledge, setArtKnowledge] = useState<ArtKnowledgeGuide>(ART_KNOWLEDGE_FALLBACK);
  const [lastTriedIdea, setLastTriedIdea] = useState<Idea | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [buildPhase, setBuildPhase] = useState<BuildStreamPhase | null>(null);
  const [buildDiff, setBuildDiff] = useState<PlaceDiff | null>(null);
  const [snapshots, setSnapshots] = useState<PlaceSnapshot[]>([]);
  const [serverDiagnostics, setServerDiagnostics] = useState<LuauDiagnostic[]>([]);
  const [dragActive, setDragActive] = useState(false);

  const inputRef = useRef<HTMLTextAreaElement>(null);
  const chatSearchRef = useRef<HTMLInputElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const menuKeyRef = useRef<HTMLButtonElement>(null);
  const shelfKeyRef = useRef<HTMLButtonElement>(null);
  const wasMenuOpenRef = useRef(false);
  const wasShelfOpenRef = useRef(false);
  const showUpdateRef = useRef<HTMLButtonElement>(null);
  const threadRef = useRef<HTMLDivElement>(null);
  const placeRef = useRef<Place | null | "error">(place);
  placeRef.current = place;

  const update = useUpdate(showUpdateRef);

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

  useEffect(() => {
    if (wasShelfOpenRef.current && !shelfOpen) {
      const focusTrigger = () => {
        if (document.activeElement === inputRef.current) return;
        (shelfKeyRef.current ?? document.querySelector<HTMLElement>('[aria-label="In your game"]'))?.focus();
      };
      requestAnimationFrame(focusTrigger);
      const t1 = window.setTimeout(focusTrigger, 120);
      const t2 = window.setTimeout(focusTrigger, 420);
      wasShelfOpenRef.current = false;
      return () => {
        window.clearTimeout(t1);
        window.clearTimeout(t2);
      };
    }
    wasShelfOpenRef.current = shelfOpen;
  }, [shelfOpen]);

  const activeId = chats?.active ?? "";
  const notices: Notice[] = activeId ? slotRead<Notice[]>(noticeKey(activeId), []) : [];
  const busy = Object.keys(inflight).length > 0;
  const building = activeId ? inflight[activeId] : undefined;
  // Built-in Studio blueprints & 28 game systems run deterministically without a model call.
  const canBuild = studioOk;

  const totalPieces =
    place && place !== "error"
      ? Object.keys(place.scripts || {}).length + Object.keys(place.instances || {}).length
      : 0;
  const builtCount = place && place !== "error" ? totalPieces : null;

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
      : undoState === "nothing" || (builtCount === 0 && undoState !== "ok")
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

  const refreshPlace = useCallback(async (): Promise<Place | null> => {
    try {
      const raw = await read("/api/place-tree");
      const norm = normalizePlace(raw);
      setPlace(norm);
      return norm;
    } catch {
      setPlace("error");
      return null;
    }
  }, []);

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
      if (!v050Ready && !forceServer) return;
      try {
        const raw = await read<{ diagnostics?: Array<Partial<LuauDiagnostic>> }>("/api/luau/check");
        if (raw && Array.isArray(raw.diagnostics)) {
          setServerDiagnostics(
            raw.diagnostics
              .filter((d): d is Partial<LuauDiagnostic> & { path: string; message: string } => Boolean(d && typeof d.path === "string" && typeof d.message === "string"))
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

  const refreshCardStudio = useCallback(
    async (forceServer = false) => {
      if (!v060Ready && !forceServer) return;
      try {
        const [rawAssets, rawKnowledge] = await Promise.all([
          read("/api/assets").catch(() => null),
          read("/api/art-knowledge").catch(() => null),
        ]);
        if (rawAssets) setCardAssets(normalizeAssetList(rawAssets));
        if (rawKnowledge) setArtKnowledge(normalizeArtKnowledge(rawKnowledge));
      } catch {
        /* keep bundled Nano Banana manifest & fallback style guide */
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
          setHasLiveFrame(true);
          setFrameSeq(norm.frame_seq);
        }
      } catch {
        /* keep current embed status */
      }
    },
    [v090Ready],
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

  const refreshStudioActions = useCallback(
    async (forceServer = false) => {
      if (!v070Ready && !forceServer) return;
      try {
        const raw = await read("/api/studio/actions");
        const norm = normalizeStudioActionsResponse(raw);
        setStudioMode(norm.studio_mode);
        setHasLiveFrame(norm.has_live_frame);
        setFrameSeq(norm.frame_seq);
        setStudioActions(norm.actions.slice(-500));
      } catch {
        /* keep current stream buffer */
      }
    },
    [v070Ready],
  );

  const refreshAll = useCallback(async () => {
    const [, , nextPlace] = await Promise.all([refreshChats(), refreshThread(), refreshPlace()]);
    await refreshIdeas();
    if (v050Ready) {
      void refreshSnapshots(true);
      if (nextPlace) void refreshDiagnostics(true);
    }
    if (v060Ready) {
      void refreshCardStudio(true);
    }
    if (v070Ready) {
      void refreshStudioActions(true);
    }
    return nextPlace;
  }, [
    refreshChats,
    refreshThread,
    refreshPlace,
    refreshIdeas,
    refreshSnapshots,
    refreshDiagnostics,
    refreshCardStudio,
    refreshStudioActions,
    v050Ready,
    v060Ready,
    v070Ready,
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
            void refreshCardStudio(true);
          }
          if (isV070OrNewer(v.version)) {
            setV070Ready(true);
            void refreshStudioActions(true);
          }
          if (isV080OrNewer(v.version)) {
            setV080Ready(true);
          }
          if (isV090OrNewer(v.version)) {
            setV090Ready(true);
            void refreshEmbedStatus(true);
            void refreshLibrary(true);
          }
        }
      })
      .catch(() => {});
    read<{
      studio_connected?: unknown;
      model_configured?: unknown;
      workspace_mode?: unknown;
      model_id?: unknown;
      version?: unknown;
      sse_build_stream?: unknown;
      studio_stream?: unknown;
      assets_api?: unknown;
      ide_api?: unknown;
      embed_api?: unknown;
      studio_embed?: unknown;
      skills_count?: unknown;
    }>("/api/status")
      .then((s) => {
        if (!s || typeof s !== "object") return;
        if (typeof s.studio_connected === "boolean") setStudioOk(s.studio_connected);
        if (typeof s.model_configured === "boolean") setModelOk(s.model_configured);
        if (
          s.workspace_mode === "studio_mcp" ||
          s.workspace_mode === "local_place" ||
          s.workspace_mode === "browser_place" ||
          s.workspace_mode === "disconnected"
        ) {
          setWorkspaceMode(s.workspace_mode);
          setStudioMode(
            s.workspace_mode === "studio_mcp"
              ? "live_mcp"
              : s.workspace_mode === "browser_place"
                ? "browser_place"
                : "local_place",
          );
        }
        if (typeof s.model_id === "string" && s.model_id.trim()) {
          setActiveModelLabel(s.model_id.trim().split("/").pop() || s.model_id.trim());
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
          void refreshCardStudio(true);
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
        }
      })
      .catch(() => {});
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Live Studio SSE stream (/api/studio/stream) for realtime 3D viewport updates & MCP frames
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
        setStudioMode(ev.studio_mode);
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
        setHasLiveFrame(true);
        setFrameSeq(info.seq);
      },
      onStatus: (st) => {
        if (st.studio_mode) setStudioMode(st.studio_mode);
        if (typeof st.has_live_frame === "boolean") setHasLiveFrame(st.has_live_frame);
        if (typeof st.frame_seq === "number") setFrameSeq(st.frame_seq);
      },
    });
    return () => {
      unsub();
      if (placeTimer !== null) window.clearTimeout(placeTimer);
    };
  }, [refreshPlace, v070Ready]);

  const prevChatRef = useRef<string | null>(null);
  const prevLenRef = useRef(0);

  useEffect(() => {
    const el = threadRef.current;
    if (!el) return;
    if (!hasConversation || prevChatRef.current !== activeId) {
      el.scrollTop = 0;
    } else if (lines.length > prevLenRef.current || Boolean(building)) {
      el.scrollTop = el.scrollHeight;
    }
    prevChatRef.current = activeId;
    prevLenRef.current = lines.length;
  }, [activeId, lines.length, notices.length, Boolean(building), hasConversation]);

  const ideShortcutRef = useRef<{
    selectedPath: string | null;
    canRedo: boolean;
    duplicate: (path: string) => Promise<void>;
    redo: () => Promise<void>;
  } | null>(null);

  // Global keyboard shortcuts: Cmd+K opens Command Palette; Ctrl+D duplicates selected part; Ctrl+Y / Cmd+Shift+Z redoes; `/` jumps to chat search
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((v) => !v);
        return;
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
      if (!phone && chatSearchRef.current) chatSearchRef.current.focus();
      else inputRef.current?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [phone]);

  const cantBuildWhy = !studioOk
    ? "Building needs the Local Place Engine or Roblox Studio MCP connected. Open Settings or use Check again."
    : !modelOk
      ? "Chat prompts need a local gateway or API key configured in Settings. Built-in Studio blueprints still run right away."
      : "";

  const checkStudio = async () => {
    try {
      const s = await read<{
        studio_connected?: unknown;
        model_configured?: unknown;
        workspace_mode?: unknown;
        model_id?: unknown;
      }>("/api/status");
      const nextStudio = s?.studio_connected === true;
      const nextModel = s?.model_configured === true;
      if (nextStudio && !studioOk) toast.success("Studio reconnected");
      else if (!nextStudio) toast.error("Studio is still disconnected");
      setStudioOk(nextStudio);
      setModelOk(nextModel);
      if (
        s?.workspace_mode === "studio_mcp" ||
        s?.workspace_mode === "local_place" ||
        s?.workspace_mode === "browser_place" ||
        s?.workspace_mode === "disconnected"
      ) {
        setWorkspaceMode(s.workspace_mode);
      }
      if (typeof s?.model_id === "string" && s.model_id.trim()) {
        setActiveModelLabel(s.model_id.trim().split("/").pop() || s.model_id.trim());
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
    setBuildDiff(null);
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
    setBuildDiff(null);
    setMenuOpen(false);
    await refreshAll();
    inputRef.current?.focus();
  };

  const buildIdea = async (idea: Idea) => {
    const chatId = activeId || "default";
    const beforePlace = clonePlace(placeRef.current);
    setLastTriedIdea(idea);
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
      if (!v050Ready) pushLocalSnapshot(idea.title, afterPlace, diff);
    }
    await new Promise((r) => setTimeout(r, 60));
    const active = document.activeElement;
    if (!active || active === document.body) {
      const key = document.querySelector<HTMLButtonElement>(`[data-focus-key="idea:${CSS.escape(idea.title)}"]`);
      if (key && !key.disabled) key.focus();
      else inputRef.current?.focus();
    }
  };

  const submitPromptText = async (rawMessage: string, restoreOnFail = true) => {
    const message = rawMessage.trim();
    if (!message || busy) return;
    const chatId = activeId || "default";
    const beforePlace = clonePlace(placeRef.current);
    if (restoreOnFail) setText("");
    begin(chatId, { message });
    const stopStream = subscribeBuildStream(setBuildPhase, v050Ready);
    let failure: string | null = null;
    try {
      const r = await fetch("/api/chat", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ message }),
      });
      const raw = await r.text();
      let data: { ok?: boolean; error?: string } = {};
      try {
        data = JSON.parse(raw) as { ok?: boolean; error?: string };
      } catch {
        /* non-JSON proxy or crash reply */
      }
      if (r.status === 403 || r.status === 413 || r.status === 415) {
        failure = data.error || "OpenRO did not accept that message.";
      } else if (!r.ok && !((r.status === 502 || r.status === 503) && data.ok === false)) {
        failure = SEND_FAILED;
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
    if (!failure && afterPlace) {
      const diff = computePlaceDiff(beforePlace, afterPlace);
      setBuildDiff(diff);
      if (!v050Ready) pushLocalSnapshot(message.slice(0, 36), afterPlace, diff);
    }
    inputRef.current?.focus();
  };

  const submitMessage = () => submitPromptText(text, true);

  const addSystem = async (sys: GameSystemDef) => {
    if (!canBuild || busy) return;
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
          if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
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

  const serverApexIdea = useMemo(
    () => ideas.find((i) => i.id === "anime-card-battles-apex" || /anime card battles/i.test(i.title)) || null,
    [ideas],
  );
  const workbenchIdeas = useMemo(
    () => ideas.filter((i) => i.id !== "anime-card-battles-apex" && !/anime card battles/i.test(i.title)),
    [ideas],
  );

  const buildApexCardBattles = async () => {
    if (!canBuild || busy) return;
    const apexIdea: Idea = serverApexIdea || {
      id: "anime-card-battles-apex",
      title: "Anime Card Battles: Apex",
      pitch:
        "Complete Roblox anime card battler: weighted 7-tier gacha packs with pity, foils & promo codes, 4-card deck synergies & star fusion, tactical boss arena, 50-floor tower & rebirth, live server weather & luck potions, and a 3D card gallery lobby.",
      skills: [
        "card_gacha_roller",
        "deck_builder",
        "card_battle_arena",
        "tower_progression",
        "potions_weather",
        "card_gallery_world",
      ],
    };
    if (v060Ready || serverApexIdea) {
      await buildIdea(apexIdea);
      return;
    }
    await submitPromptText(
      "Build Anime Card Battles: Rebirth — Apex Edition with weighted 7-tier gacha pack roller, 4-card deck with star fusion and rebirth, tactical boss battle arena, 50-floor tower progression, server weather and luck potions, and a 3D card gallery lobby.",
      false,
    );
  };

  const handleCardAssetAdded = (asset: CardAssetRecord) => {
    setCardAssets((prev) => {
      const rest = prev.filter((a) => a.card_id !== asset.card_id);
      return [asset, ...rest];
    });
  };

  const apexBuilt =
    builtTitles.some((t) => /anime card battles/i.test(t)) ||
    Boolean(
      place &&
        place !== "error" &&
        Object.keys(place.scripts || {}).some((k) => /CardGacha|CardBattle|TowerProgression/i.test(k)),
    );

  const send = (e: FormEvent) => {
    e.preventDefault();
    void submitMessage();
  };

  const onComposerKeyDown = (e: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void submitMessage();
    }
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
    }
    if (msg) {
      addNotice(chatId, { text: msg, bad });
      if (bad) toast.error(msg);
      else toast.success(msg);
    }
  };

  const handleSaveInstance = async (payload: InstanceEditPayload) => {
    const beforePlace = clonePlace(placeRef.current);
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
        /* fall back to local parsed place */
      }
    }

    setPlace(parsed.place);
    const diff = computePlaceDiff(beforePlace, parsed.place);
    setBuildDiff(diff);
    pushLocalSnapshot(`Imported ${filename}`, parsed.place, diff);
    toast.success(`Imported ${filename} (${partN} parts, ${scrN} scripts)`);
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
    const file = [...e.dataTransfer.files].find((f) => /\.(rbxlx|xml)$/i.test(f.name)) || e.dataTransfer.files[0];
    if (!file) return;
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
      toast.success("Static Luau check complete — upgrade server to v0.7.0 for live Studio playtest streaming.");
      return;
    }
    try {
      const res = await post<{ ok?: boolean; passed?: boolean; error?: string; console?: string }>(
        "/api/studio/playtest",
        {},
      );
      if (res.ok === false) {
        toast.error(res.error || "Studio playtest failed.");
      } else if (res.passed === false) {
        toast.error("Playtest flagged issues — check Studio Output console.");
      } else {
        toast.success("Studio Playtest passed — 0 Luau errors");
      }
      await refreshStudioActions(true);
      await refreshDiagnostics(true);
    } catch {
      toast.error("Could not reach /api/studio/playtest.");
    }
  };

  const handleCaptureStudioFrame = async () => {
    if (!v070Ready) {
      toast.error("Live Studio Frame capture requires OpenRO v0.7.0 with Roblox Studio MCP attached.");
      return;
    }
    try {
      const res = await post<{ ok?: boolean; frame_seq?: number; error?: string }>(
        "/api/studio/capture",
        {},
      );
      if (res.ok === false) {
        toast.error(res.error || "Live Studio MCP is not attached — switch to Studio 3D or connect Studio MCP.");
        return;
      }
      setHasLiveFrame(true);
      if (typeof res.frame_seq === "number") setFrameSeq(res.frame_seq);
      else setFrameSeq((n) => n + 1);
      toast.success("Captured fresh frame from Roblox Studio MCP");
    } catch {
      toast.error("Could not capture frame from Studio MCP.");
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
    const offsetX = (Object.keys(existing).length % 5 - 2) * 8;
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
      case "card_billboard": {
        const card = cardAssets[Object.keys(existing).length % Math.max(1, cardAssets.length)] || APEX_CARDS[0];
        return {
          name: pickName("CardBillboard"),
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
            CardTitle: card?.title || "Apex Card",
            DecalTexture: card?.image_url || "",
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

    const applyLocal = () => {
      const src = beforePlace.instances[path];
      const base = path.replace(/^Workspace\./, "");
      let dupName = `${base}Copy`;
      for (let i = 2; i < 500 && `Workspace.${dupName}` in beforePlace.instances; i++) {
        dupName = `${base}Copy${i}`;
      }
      const dupPath = `Workspace.${dupName}`;
      const pos = Array.isArray(src.properties?.Position) && src.properties.Position.length >= 3
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
        toast.success(`Duplicated ${path.split(".").pop() || path} → ${dupPath.split(".").pop() || dupPath}`);
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

    const applyLocal = () => {
      const nextInstances = { ...beforePlace.instances };
      delete nextInstances[path];
      const updated: Place = { scripts: { ...beforePlace.scripts }, instances: nextInstances };
      setPlace(updated);
      if (selectedPath === path) setSelectedPath(null);
      setBuildDiff(computePlaceDiff(beforePlace, updated));
    };

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
    const localDiags = analyzePlaceLuau({ scripts: { [path]: source }, instances: beforePlace?.instances || {} });
    const applyLocal = () => {
      if (!beforePlace) return;
      const updated: Place = {
        instances: { ...beforePlace.instances },
        scripts: { ...beforePlace.scripts, [path]: source },
      };
      setPlace(updated);
      setBuildDiff(computePlaceDiff(beforePlace, updated));
    };

    if (!v080Ready) {
      applyLocal();
      const errs = localDiags.filter((d) => d.severity === "error").length;
      if (errs > 0) toast.error(`Saved ${path} (${errs} Luau error${errs === 1 ? "" : "s"} flagged)`);
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
        diagnostics?: LuauDiagnostic[];
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
        if (errs > 0) {
          toast.error(`Saved ${path.split(".").pop() || path} — ${errs} Luau error${errs === 1 ? "" : "s"} detected`);
        } else if (warns > 0) {
          toast.success(`Saved ${path.split(".").pop() || path} — 0 errors, ${warns} warning${warns === 1 ? "" : "s"}`);
        } else {
          toast.success(`Saved & checked ${path.split(".").pop() || path} — 0 Luau errors`);
        }
        const afterPlace = await refreshPlace();
        void refreshChats();
        void refreshDiagnostics(true);
        void refreshStudioActions(true);
        void refreshSnapshots(true);
        if (afterPlace) setBuildDiff(computePlaceDiff(beforePlace, afterPlace));
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
    if (!v080Ready) {
      toast.error("Redo requires OpenRO v0.8.0.");
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

  const handleDownloadShipPack = async () => {
    const cur = placeRef.current && placeRef.current !== "error" ? placeRef.current : null;
    if (!cur || (Object.keys(cur.instances || {}).length === 0 && Object.keys(cur.scripts || {}).length === 0)) {
      toast.error("Build or import a Roblox place before exporting a Ship Pack.");
      return;
    }
    const label = activeTitle && activeTitle !== "New chat" ? activeTitle : shell.gameName;
    const slug = (label || "openro-ship-pack").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "openro-ship-pack";
    const isBrowserEngine = Boolean((window as ShellWindow).__OPENRO_WEB_ENGINE__);

    try {
      let zipBytes: Uint8Array | null = null;
      if (v080Ready && !isBrowserEngine) {
        const r = await fetch("/api/export/bundle", { cache: "no-store" });
        if (r.ok) {
          const buf = new Uint8Array(await r.arrayBuffer());
          if (buf.length >= 4 && buf[0] === 0x50 && buf[1] === 0x4b) {
            zipBytes = buf;
          }
        }
      }
      if (!zipBytes) {
        zipBytes = buildRojoShipPackZip({
          gameName: label,
          place: cur,
          assets: cardAssets,
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
      toast.error("Live Studio MCP Connect requires OpenRO v0.8.0.");
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
        if (typeof res.frame_seq === "number") setFrameSeq(res.frame_seq);
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
      const res = await post<{
        ok?: boolean;
        workspace_mode?: WorkspaceMode;
        studio_mode?: StudioMode;
        error?: string;
      }>("/api/studio/mcp/disconnect", {});
      if (res.ok) {
        await refreshPlace();
        setWorkspaceMode(res.workspace_mode || "local_place");
        setStudioMode(res.studio_mode || "local_place");
        setHasLiveFrame(false);
        toast.success("Disconnected Studio MCP bridge — returned to Local Studio 3D");
        void refreshStudioActions(true);
      } else {
        toast.error(res.error || "Could not disconnect Studio MCP bridge.");
      }
    } catch {
      toast.error("Could not reach /api/studio/mcp/disconnect.");
    }
  };

  const handleLaunchEmbed = async (command?: string) => {
    if (!v090Ready) {
      toast.error("Embedded Roblox Studio requires OpenRO v0.9.0 desktop server.");
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
          setHasLiveFrame(true);
          setFrameSeq(res.frame_seq);
        }
        setEmbedRevealed(true);
        setRequestedViewportTab("embedded-studio");
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
        setEmbedStatus(normalizeEmbedStatus({ ...(res.status && typeof res.status === "object" ? (res.status as Record<string, unknown>) : res), running: false, state: "stopped", play_mode: "edit" }));
        setWorkspaceMode("local_place");
        setStudioMode("local_place");
        setHasLiveFrame(false);
        await refreshPlace();
        void refreshStudioActions(true);
        toast.success("Stopped Embedded Roblox Studio — returned to Local Studio 3D");
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
        setHasLiveFrame(true);
        setFrameSeq(res.frame_seq);
      }
    } catch {
      /* non-fatal input drop */
    }
  };

  const handleSendKeyboardInput = async (payload: StudioKeyboardInputPayload) => {
    if (!v090Ready) return;
    try {
      const res = await post<{ ok?: boolean; frame_seq?: number }>("/api/studio/input/keyboard", payload);
      if (res.ok && typeof res.frame_seq === "number" && res.frame_seq > 0) {
        setHasLiveFrame(true);
        setFrameSeq(res.frame_seq);
      }
    } catch {
      /* non-fatal input drop */
    }
  };

  const handleInvokeStudioTool = async (
    tool: StudioToolName,
    args: Record<string, unknown>,
  ): Promise<StudioToolResult> => {
    const started = performance.now();
    if (!v090Ready) {
      return {
        ok: false,
        tool,
        error: "All-12 Studio MCP Tools require OpenRO v0.9.0.",
        duration_ms: 0,
      };
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
        setHasLiveFrame(true);
        setFrameSeq(res.frame_seq);
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

  const handleOpenEmbeddedStudio = () => {
    setEmbedRevealed(true);
    setRequestedViewportTab("embedded-studio");
    if (threadRef.current) threadRef.current.scrollTop = 0;
  };

  ideShortcutRef.current = {
    selectedPath,
    canRedo,
    duplicate: handleDuplicatePart,
    redo: handleRedo,
  };

  const handleSelectPartFromStage = (path: string) => {
    setSelectedPath(path);
    if (narrow) {
      setShelfOpen(true);
    } else if (shelfCollapsed) {
      setShelfCollapsed(false);
    }
  };

  const triggerExportFromPalette = async () => {
    try {
      const r = await fetch("/api/export/rbxlx", { cache: "no-store" });
      if (!r.ok) throw new Error("export failed");
      const xml = await r.text();
      const blob = new Blob([xml], { type: "application/xml;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "openro-place.rbxlx";
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      toast.success("Exported openro-place.rbxlx");
    } catch {
      toast.error("Could not export .rbxlx right now.");
    }
  };

  const onUndoClick = (e: MouseEvent<HTMLDivElement>) => {
    if ((e.target as HTMLElement).closest("[data-undo-yes]")) void undoNow();
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

  const rail = (
    <nav aria-label="Chats and Studio controls" className="flex h-full min-h-0 flex-col gap-3 bg-surface p-3.5 text-ink">
      <div className="flex min-w-0 items-center gap-2.5 px-0.5 pt-0.5">
        <span className="grid size-9 flex-none place-items-center rounded-card border border-hairline bg-board text-ink">
          <Blocks className="size-4.5" strokeWidth={1.5} aria-hidden />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <h1 className="text-base font-medium leading-tight text-ink">OpenRO</h1>
          </div>
          <div className="truncate text-xs text-ink-2" title={shell.gameName}>
            {shell.gameName}
          </div>
        </div>
      </div>

      {/* Live Studio / Local Place connection block inside Left Rail on desktop */}
      {!phone && (
        <div
          data-answers="2"
          className="flex flex-wrap items-center justify-between gap-2 rounded-card border border-hairline bg-board/60 px-2.5 py-2"
        >
          <StudioChip connected={studioOk} workspaceMode={workspaceMode} />
          {!studioOk && (
            <Button variant="outline" size="xs" onClick={() => void checkStudio()} className="bg-surface">
              <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
              Check again
            </Button>
          )}
        </div>
      )}

      <div className="flex flex-col gap-1.5">
        <Button
          variant="secondary"
          onClick={() => void newChat()}
          className="w-full justify-start border-ink-button bg-ink-button text-ink-button-fg hover:bg-ink-button/90 max-md:min-h-11"
        >
          <Plus className="size-4" strokeWidth={1.5} aria-hidden />
          New chat
        </Button>

        <Button
          variant="outline"
          onClick={() => {
            setMenuOpen(false);
            setPaletteOpen(true);
          }}
          aria-label="Open command palette"
          className="w-full justify-between bg-board/50 font-normal text-ink-2 hover:bg-board hover:text-ink max-md:min-h-11"
        >
          <span className="inline-flex min-w-0 items-center gap-2">
            <Command className="size-3.5 flex-none text-icon" strokeWidth={1.5} aria-hidden />
            <span className="truncate text-sm">Command palette</span>
          </span>
          <Kbd className="px-1.5 font-mono">⌘K</Kbd>
        </Button>

        <Button
          variant="outline"
          onClick={() => {
            setMenuOpen(false);
            setArtStudioOpen(true);
          }}
          aria-label="Open Card Art Studio"
          className="w-full justify-between bg-board/50 font-normal text-ink-2 hover:bg-board hover:text-ink max-md:min-h-11"
        >
          <span className="inline-flex min-w-0 items-center gap-2">
            <Palette className="size-3.5 flex-none text-icon" strokeWidth={1.5} aria-hidden />
            <span className="truncate text-sm">Card Art Studio</span>
          </span>
          <span className="rounded-chip bg-surface px-1.5 py-0.5 font-mono text-xs tabular-nums text-ink-2">
            {cardAssets.length}
          </span>
        </Button>

        <Button
          variant="outline"
          onClick={() => {
            setMenuOpen(false);
            setToolLibraryOpen(true);
            void refreshLibrary();
          }}
          aria-label="Open Tool and Skill Library"
          className="w-full justify-between bg-board/50 font-normal text-ink-2 hover:bg-board hover:text-ink max-md:min-h-11"
        >
          <span className="inline-flex min-w-0 items-center gap-2">
            <Package className="size-3.5 flex-none text-icon" strokeWidth={1.5} aria-hidden />
            <span className="truncate text-sm">Tool Library</span>
          </span>
          <span className="rounded-chip bg-surface px-1.5 py-0.5 font-mono text-xs tabular-nums text-ink-2">
            {librarySnap.packs.filter((p) => p.installed).length}/{librarySnap.packs.length}
          </span>
        </Button>
      </div>

      <InputGroup className="bg-board/45">
        <InputGroupAddon align="inline-start" className="ps-2.5">
          <Search className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
        </InputGroupAddon>
        <InputGroupInput
          ref={chatSearchRef}
          type="search"
          value={chatQuery}
          onChange={(e) => setChatQuery(e.target.value)}
          aria-label="Search chats"
          placeholder="Search chats"
          className="[&_input]:max-md:!min-h-11 [&_input]:md:!h-8"
        />
        {chatQuery ? (
          <InputGroupAddon align="inline-end" className="has-[>button]:me-0 pe-1">
            <Button
              variant="ghost"
              size="icon-xs"
              onClick={() => setChatQuery("")}
              aria-label="Clear chat search"
              className="max-md:size-11"
            >
              <X className="size-3.5" strokeWidth={1.5} aria-hidden />
            </Button>
          </InputGroupAddon>
        ) : (
          <InputGroupAddon align="inline-end" className="has-[>kbd:last-child]:me-0 pe-2">
            <Kbd className="px-1.5 font-mono">/</Kbd>
          </InputGroupAddon>
        )}
      </InputGroup>

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {chatsError && !chats ? (
          <div className="flex flex-col items-start gap-2 rounded-card border border-hairline bg-board p-3 text-xs text-ink-2">
            <span>Could not load chats from server.</span>
            <Button variant="outline" size="xs" onClick={() => void refreshAll()} className="max-md:min-h-11">
              Retry chats
            </Button>
          </div>
        ) : filteredChats.length === 0 ? (
          <div className="flex flex-col items-start gap-2 rounded-card border border-hairline bg-board/50 p-3 text-xs text-ink-2">
            <span>No chats match &ldquo;{chatQuery}&rdquo;.</span>
            {chatQuery && (
              <Button variant="outline" size="xs" onClick={() => setChatQuery("")} className="max-md:min-h-11">
                Show all chats
              </Button>
            )}
          </div>
        ) : (
          <AnimatedBackground
            defaultValue={activeId}
            onValueChange={(id) => id && id !== activeId && void switchChat(id)}
            className="rounded-card bg-blue-soft"
            transition={glide}
            enableHover={false}
          >
            <ul id="chat-list" className="flex flex-col gap-0.5" aria-label="Your chats">
              {filteredChats.map((c: Chat) => (
                <li key={c.id} className="relative">
                  <Button
                    variant="ghost"
                    data-id={c.id}
                    aria-current={c.id === activeId ? "true" : undefined}
                    onClick={() => void switchChat(c.id)}
                    className="chat-btn h-auto min-h-9 w-full justify-between rounded-card border-transparent px-2.5 py-1.5 text-left font-normal text-ink-2 hover:bg-board/70 hover:text-ink data-[checked=true]:font-medium data-[checked=true]:text-ink max-md:min-h-11"
                  >
                    <span className="min-w-0 flex-1 truncate text-sm">{c.title || "New chat"}</span>
                    {inflight[c.id] && (
                      <Pill className="bg-surface px-1.5 py-0 font-mono text-xs text-ink">working</Pill>
                    )}
                  </Button>
                </li>
              ))}
            </ul>
          </AnimatedBackground>
        )}
      </div>

      <div className="flex flex-col gap-2.5 border-t border-hairline pt-3">
        <Button
          variant="outline"
          aria-label="Settings"
          onClick={() => {
            setMenuOpen(false);
            setSettingsOpen(true);
          }}
          className="w-full justify-between bg-board/60 font-medium text-ink hover:bg-board max-md:min-h-11"
        >
          <span className="inline-flex min-w-0 items-center gap-2">
            <SlidersHorizontal className="size-4 flex-none text-icon" strokeWidth={1.5} aria-hidden />
            <span className="truncate">Settings</span>
          </span>
          <span className="truncate font-mono text-xs text-ink-3">
            {activeModelLabel ||
              (workspaceMode === "studio_mcp"
                ? "Studio MCP"
                : workspaceMode === "local_place"
                  ? "Local Place"
                  : workspaceMode === "browser_place"
                    ? "Browser"
                    : "Offline")}
          </span>
        </Button>

        <div className="flex items-center justify-between gap-2 text-xs text-ink-2">
          <a
            href={RELEASES_URL + (version || "0.9.0")}
            target="_blank"
            rel="noreferrer"
            className="inline-flex min-h-11 items-center gap-1 rounded-sm px-1 font-mono text-xs text-ink-2 outline-none hover:text-ink hover:underline md:min-h-7"
          >
            <span>OpenRO v{version || "0.9.0"}</span>
            <ExternalLink className="size-3 text-icon" strokeWidth={1.5} aria-hidden />
          </a>
          <span className="truncate text-right text-xs text-ink-3">
            {update.busy === "check"
              ? "Checking…"
              : update.snap?.staged
                ? `${update.snap.staged} staged`
                : update.snap?.available
                  ? `${update.snap.latest} available`
                  : update.snap?.checked_at
                    ? "Up to date"
                    : "Studio Workbench"}
          </span>
        </div>
      </div>
    </nav>
  );

  return (
    <MotionConfig reducedMotion="user">
      <div
        className="relative flex h-dvh min-h-0 w-full overflow-hidden bg-canvas text-ink"
        data-app="openro"
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

        {dragActive && (
          <div
            role="status"
            className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center bg-canvas/85 p-6"
          >
            <div className="flex flex-col items-center gap-2 rounded-panel border-2 border-dashed border-blue bg-surface px-8 py-6 text-center shadow-lift">
              <Upload className="size-6 text-blue" strokeWidth={1.5} aria-hidden />
              <p className="text-base font-medium text-ink">Drop .rbxlx XML file to import</p>
              <p className="text-sm text-ink-2">Parses Workspace parts, CFrame/Size/Color, and Luau scripts directly into Studio.</p>
            </div>
          </div>
        )}

        <a className="skip" href="#composer-input">
          Skip to message box
        </a>

        {!phone && <div className="w-[256px] flex-none border-r border-hairline">{rail}</div>}

        <main className="flex min-w-0 flex-1 flex-col">
          <header className="flex min-h-14 flex-none flex-col justify-center gap-2 border-b border-hairline bg-surface px-3 py-2 md:flex-row md:items-center md:justify-between md:px-6">
            <div className="flex min-w-0 flex-1 items-center justify-between gap-2">
              <div className="flex min-w-0 flex-1 items-center gap-2">
                {phone && (
                  <Drawer.Root open={menuOpen} onOpenChange={setMenuOpen} direction="left" autoFocus>
                    <Drawer.Trigger asChild>
                      <Button
                        ref={menuKeyRef}
                        variant="ghost"
                        size="icon"
                        aria-label="Open the menu"
                        className="size-11"
                      >
                        <Menu className="size-5" strokeWidth={1.5} aria-hidden />
                      </Button>
                    </Drawer.Trigger>
                    <Drawer.Portal>
                      <Drawer.Overlay className="fixed inset-0 z-40 bg-ink/30" />
                      <Drawer.Content
                        onEscapeKeyDown={(e) => {
                          if (settingsOpen || paletteOpen || snapshotsOpen || artStudioOpen || toolLibraryOpen) {
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
                        <Drawer.Title className="sr-only">Chats and Studio controls</Drawer.Title>
                        <Drawer.Description className="sr-only">
                          Switch Studio chats, start a new chat, search commands, and open Settings.
                        </Drawer.Description>
                        {rail}
                      </Drawer.Content>
                    </Drawer.Portal>
                  </Drawer.Root>
                )}
                <h2 className="min-w-0 flex-1 truncate text-base font-medium text-ink md:text-title">
                  {activeTitle}
                </h2>
              </div>

              {narrow && (
                <Drawer.Root open={shelfOpen} onOpenChange={setShelfOpen} autoFocus>
                  <Drawer.Trigger asChild>
                    <Button
                      ref={shelfKeyRef}
                      variant="outline"
                      size="sm"
                      aria-label="In your game"
                      data-answers-via="3"
                      className="shrink-0 gap-1.5 max-md:min-h-11 max-md:px-3"
                    >
                      <FolderTree className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                      <span className="hidden sm:inline">In your game</span>
                      <span className="rounded-chip bg-board px-1.5 py-0.5 font-mono text-xs tabular-nums text-ink">
                        <SlidingNumber value={totalPieces} />
                      </span>
                    </Button>
                  </Drawer.Trigger>
                  <Drawer.Portal>
                    <Drawer.Overlay className="fixed inset-0 z-40 bg-ink/30" />
                    <Drawer.Content
                      onEscapeKeyDown={() => setShelfOpen(false)}
                      onCloseAutoFocus={(e) => {
                        e.preventDefault();
                        (shelfKeyRef.current ?? document.querySelector<HTMLElement>('[aria-label="In your game"]'))?.focus();
                      }}
                      className="fixed inset-x-0 bottom-0 z-50 flex max-h-[82dvh] flex-col rounded-t-panel border-t border-hairline bg-surface outline-none"
                    >
                      <Drawer.Handle className="mx-auto mt-2.5 mb-1 h-1.5 w-10 rounded-full bg-track" />
                      <Drawer.Title className="sr-only">In your game</Drawer.Title>
                      <Drawer.Description className="sr-only">
                        Roblox Studio DataModel Explorer listing Workspace parts and Luau scripts in your place.
                      </Drawer.Description>
                      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
                        <PartsShelf
                          gameName={shell.gameName}
                          builtTitles={builtTitles}
                          place={place}
                          error={place === "error"}
                          selectedPath={selectedPath}
                          onSelectPath={setSelectedPath}
                          onSaveInstance={handleSaveInstance}
                          onSaveScript={handleSaveScript}
                          onDuplicatePart={handleDuplicatePart}
                          onDeletePart={handleDeletePart}
                          onRunPlaytest={handleRunStudioPlaytest}
                          onDownloadShipPack={handleDownloadShipPack}
                          onImportClick={() => fileInputRef.current?.click()}
                          onOpenSnapshots={() => {
                            setShelfOpen(false);
                            setSnapshotsOpen(true);
                          }}
                          snapshotCount={snapshots.length}
                          buildDiff={buildDiff}
                          serverDiagnostics={serverDiagnostics}
                        />
                      </div>
                    </Drawer.Content>
                  </Drawer.Portal>
                </Drawer.Root>
              )}
            </div>

            {/* Mobile Studio / Local Place status + Check again in first fold */}
            {phone && (
              <div className="flex w-full flex-wrap items-center justify-between gap-2 border-t border-hairline pt-2">
                <div data-answers="2" className="flex items-center gap-1.5">
                  <StudioChip connected={studioOk} workspaceMode={workspaceMode} />
                  {!studioOk && (
                    <Button variant="outline" size="sm" onClick={() => void checkStudio()} className="min-h-11">
                      <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
                      Check again
                    </Button>
                  )}
                </div>
              </div>
            )}
          </header>

          <div
            className="server-banners"
            onClick={onBannerClick}
            dangerouslySetInnerHTML={{ __html: shell.bannerHtml }}
          />
          {update.show && <UpdateStrip u={update} />}

          <div
            id="thread"
            ref={threadRef}
            aria-live="polite"
            aria-busy={!chats && !chatsError}
            className="min-h-0 flex-1 overflow-y-auto"
          >
            <div className="mx-auto flex w-full max-w-[840px] flex-col gap-3.5 p-3.5 md:px-6 md:py-4">
              {threadError ? (
                <div role="alert" className="flex flex-col items-start gap-3 rounded-card border border-hairline bg-surface p-4">
                  <strong className="text-ui font-medium text-pill-pro-fg">Could not load the conversation.</strong>
                  <p className="text-sm text-ink-2">The OpenRO server did not answer. Try again, or reload the page.</p>
                  <Button variant="default" onClick={() => void refreshAll()} className="max-md:min-h-11">
                    Try again
                  </Button>
                </div>
              ) : !hasConversation ? (
                <>
                  {(embedRevealed || embedStatus.running) && (
                    <StagePreview
                      place={place && place !== "error" ? place : { scripts: {}, instances: {} }}
                      theme={theme}
                      selectedPath={selectedPath}
                      onSelectPart={handleSelectPartFromStage}
                      assets={cardAssets}
                      onOpenArtStudio={() => setArtStudioOpen(true)}
                      studioMode={studioMode}
                      studioActions={studioActions}
                      hasLiveFrame={hasLiveFrame}
                      frameSeq={frameSeq}
                      building={Boolean(building)}
                      canRedo={canRedo}
                      embedStatus={embedStatus}
                      requestedTab={requestedViewportTab}
                      onSaveInstance={handleSaveInstance}
                      onCreatePart={handleCreatePart}
                      onDuplicatePart={handleDuplicatePart}
                      onDeletePart={handleDeletePart}
                      onTriggerRedo={handleRedo}
                      onRunPlaytest={handleRunStudioPlaytest}
                      onCaptureFrame={handleCaptureStudioFrame}
                      onConnectMcp={handleConnectMcp}
                      onDisconnectMcp={handleDisconnectMcp}
                      onLaunchEmbed={handleLaunchEmbed}
                      onStopEmbed={handleStopEmbed}
                      onRefreshEmbedStatus={() => void refreshEmbedStatus(true)}
                      onSendMouseInput={handleSendMouseInput}
                      onSendKeyboardInput={handleSendKeyboardInput}
                      onInvokeStudioTool={handleInvokeStudioTool}
                    />
                  )}
                  <BlueprintWorkbench
                    ideas={workbenchIdeas}
                    builtTitles={builtTitles}
                    canBuild={canBuild}
                    cantBuildWhy={cantBuildWhy}
                    busy={busy}
                    buildingTitle={building?.build}
                    onBuild={(idea) => void buildIdea(idea)}
                  />
                  <FeaturedApexBanner
                    assets={cardAssets}
                    built={apexBuilt}
                    canBuild={canBuild}
                    busy={busy}
                    building={Boolean(building?.build && /anime card battles/i.test(building.build))}
                    onBuildApex={() => void buildApexCardBattles()}
                    onOpenArtStudio={() => setArtStudioOpen(true)}
                  />
                  <SystemLibrary
                    place={place}
                    canBuild={canBuild}
                    busy={busy}
                    buildingTitle={building?.build}
                    onAddSystem={(sys) => void addSystem(sys)}
                  />
                </>
              ) : (
                <>
                  <div className="sr-only" id="stage-title">
                    Studio build thread
                  </div>
                  {(totalPieces > 0 || Boolean(building) || studioMode === "live_mcp" || hasLiveFrame || embedStatus.running || embedRevealed) && (
                    <StagePreview
                      place={place && place !== "error" ? place : { scripts: {}, instances: {} }}
                      theme={theme}
                      selectedPath={selectedPath}
                      onSelectPart={handleSelectPartFromStage}
                      assets={cardAssets}
                      onOpenArtStudio={() => setArtStudioOpen(true)}
                      studioMode={studioMode}
                      studioActions={studioActions}
                      hasLiveFrame={hasLiveFrame}
                      frameSeq={frameSeq}
                      building={Boolean(building)}
                      canRedo={canRedo}
                      embedStatus={embedStatus}
                      requestedTab={requestedViewportTab}
                      onSaveInstance={handleSaveInstance}
                      onCreatePart={handleCreatePart}
                      onDuplicatePart={handleDuplicatePart}
                      onDeletePart={handleDeletePart}
                      onTriggerRedo={handleRedo}
                      onRunPlaytest={handleRunStudioPlaytest}
                      onCaptureFrame={handleCaptureStudioFrame}
                      onConnectMcp={handleConnectMcp}
                      onDisconnectMcp={handleDisconnectMcp}
                      onLaunchEmbed={handleLaunchEmbed}
                      onStopEmbed={handleStopEmbed}
                      onRefreshEmbedStatus={() => void refreshEmbedStatus(true)}
                      onSendMouseInput={handleSendMouseInput}
                      onSendKeyboardInput={handleSendKeyboardInput}
                      onInvokeStudioTool={handleInvokeStudioTool}
                    />
                  )}
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
                    onBuild={(idea) => void buildIdea(idea)}
                  />
                  {!building && workbenchIdeas.some((i) => !builtTitles.includes(i.title)) && !lines.some((l) => l.startsWith("OpenRO: That landed.")) && (
                    <BlueprintWorkbench
                      ideas={workbenchIdeas}
                      builtTitles={builtTitles}
                      canBuild={canBuild}
                      cantBuildWhy={cantBuildWhy}
                      busy={busy}
                      onBuild={(idea) => void buildIdea(idea)}
                    />
                  )}
                  <SystemLibrary
                    place={place}
                    canBuild={canBuild}
                    busy={busy}
                    buildingTitle={building?.build}
                    onAddSystem={(sys) => void addSystem(sys)}
                  />
                </>
              )}
            </div>
          </div>

          {/* Hairline Bottom Composer Dock */}
          <div data-answers="4" className="flex-none border-t border-hairline bg-surface">
            <div className="mx-auto flex w-full max-w-[840px] flex-col gap-2 px-3 py-2.5 md:px-6 md:py-3.5">
              <form id="composer" onSubmit={send} className="grid gap-2 sm:flex sm:items-end">
                <InputGroup className="min-w-0 bg-canvas/55 sm:flex-1">
                  <InputGroupTextarea
                    ref={inputRef}
                    id="composer-input"
                    rows={1}
                    aria-label="Describe your game"
                    placeholder={
                      compactInput
                        ? "Describe your game…"
                        : "Describe a game or change, e.g. lava obby with coins…"
                    }
                    value={text}
                    onChange={(e) => setText(e.target.value)}
                    onKeyDown={onComposerKeyDown}
                    className="[&_textarea]:!min-h-11 [&_textarea]:max-h-28 [&_textarea]:!py-2.5 [&_textarea]:leading-5"
                  />
                  <InputGroupAddon align="inline-end" className="has-[>button]:me-0 self-center pe-1.5">
                    <MorphingDialog transition={open}>
                      <MorphingDialogTrigger
                        thing="undo"
                        blocked={!canUndo}
                        describedBy={undoReason ? "undo-reason" : undefined}
                        aria-label="Undo"
                        className="inline-flex min-h-11 items-center gap-1.5 rounded-md px-2.5 text-sm font-medium text-ink-2 outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-track hover:text-ink disabled:text-ink-3 md:min-h-8"
                      >
                        <Undo2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                        <span>Undo</span>
                      </MorphingDialogTrigger>
                      <MorphingDialogContainer>
                        <MorphingDialogContent className="relative mx-4 w-full max-w-[420px] rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
                          <div onClick={onUndoClick} data-undo-dialog className="flex flex-col gap-4">
                            <div>
                              <MorphingDialogTitle className="text-title font-medium text-ink">
                                Undo the last build?
                              </MorphingDialogTitle>
                              <MorphingDialogDescription className="mt-1 text-sm text-ink-2">
                                OpenRO restores the scripts and Workspace instances modified by the most recent build in this chat.
                              </MorphingDialogDescription>
                            </div>
                            <div className="flex flex-wrap justify-end gap-2">
                              <MorphingDialogClose
                                aria-label="Keep it"
                                className="static inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board md:min-h-9"
                              >
                                Keep it
                              </MorphingDialogClose>
                              <MorphingDialogClose
                                data-undo-yes
                                aria-label="Yes, undo it"
                                className="static inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-blue bg-blue px-4 text-sm font-medium text-blue-fg outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-blue-hover md:min-h-9"
                              >
                                Yes, undo it
                              </MorphingDialogClose>
                            </div>
                          </div>
                        </MorphingDialogContent>
                      </MorphingDialogContainer>
                    </MorphingDialog>
                  </InputGroupAddon>
                </InputGroup>

                <Button
                  type="submit"
                  variant="default"
                  disabled={busy || !text.trim()}
                  className="h-11 w-full px-4 sm:w-auto"
                >
                  <Send className="size-4" strokeWidth={1.5} aria-hidden />
                  Send
                </Button>
              </form>

              <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 text-xs text-ink-2">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="hidden items-center gap-1 sm:inline-flex">
                    <Kbd className="px-1.5 font-mono">Enter</Kbd>
                    <span>send</span>
                    <span aria-hidden>·</span>
                    <Kbd className="px-1.5 font-mono">Shift+Enter</Kbd>
                    <span>new line</span>
                    <span aria-hidden>·</span>
                    <Kbd className="px-1.5 font-mono">⌘K</Kbd>
                    <span>commands</span>
                  </span>
                  {undoReason && <span id="undo-reason">{undoReason}</span>}
                </div>

                {/* Always-reachable update controls at every viewport width */}
                <div className="flex items-center gap-1.5">
                  {update.isDismissedAvailable && (
                    <Button
                      ref={showUpdateRef}
                      variant="ghost"
                      size="xs"
                      onClick={() => update.showAgain()}
                      className="text-ink underline-offset-2 hover:underline max-md:min-h-11 max-md:px-2.5"
                    >
                      Show update
                    </Button>
                  )}
                  {update.canCheck && (
                    <Button
                      variant="ghost"
                      size="xs"
                      aria-label="Check for updates"
                      disabled={update.busy !== ""}
                      onClick={() => void update.check()}
                      className="text-ink-2 hover:text-ink max-md:min-h-11 max-md:px-2.5"
                    >
                      <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
                      <span>Check updates</span>
                    </Button>
                  )}
                </div>
              </div>
            </div>
          </div>
        </main>

        {!narrow && (
          <div className="flex-none border-l border-hairline" style={{ width: shelfCollapsed ? "auto" : 316 }}>
            <PartsShelf
              gameName={shell.gameName}
              builtTitles={builtTitles}
              place={place}
              error={place === "error"}
              collapsed={shelfCollapsed}
              onToggle={() => setShelfCollapsed((c) => !c)}
              answerAttr
              selectedPath={selectedPath}
              onSelectPath={setSelectedPath}
              onSaveInstance={handleSaveInstance}
              onSaveScript={handleSaveScript}
              onDuplicatePart={handleDuplicatePart}
              onDeletePart={handleDeletePart}
              onRunPlaytest={handleRunStudioPlaytest}
              onDownloadShipPack={handleDownloadShipPack}
              onImportClick={() => fileInputRef.current?.click()}
              onOpenSnapshots={() => setSnapshotsOpen(true)}
              snapshotCount={snapshots.length}
              buildDiff={buildDiff}
              serverDiagnostics={serverDiagnostics}
            />
          </div>
        )}

        <CommandPalette
          open={paletteOpen}
          onOpenChange={setPaletteOpen}
          place={place}
          canBuild={canBuild}
          busy={busy}
          canUndo={canUndo}
          canRedo={canRedo}
          onAddSystem={(sys) => void addSystem(sys)}
          onSelectPath={handleSelectPartFromStage}
          onNewChat={() => void newChat()}
          onExportRbxlx={() => void triggerExportFromPalette()}
          onDownloadShipPack={() => void handleDownloadShipPack()}
          onAddBlockPart={() => void handleCreatePart("block")}
          onImportRbxlxClick={() => fileInputRef.current?.click()}
          onOpenSnapshots={() => setSnapshotsOpen(true)}
          onOpenSettings={() => setSettingsOpen(true)}
          onOpenArtStudio={() => setArtStudioOpen(true)}
          onOpenToolLibrary={() => {
            setToolLibraryOpen(true);
            void refreshLibrary();
          }}
          onOpenEmbeddedStudio={handleOpenEmbeddedStudio}
          onBuildApex={() => void buildApexCardBattles()}
          onRunPlaytest={() => void handleRunStudioPlaytest()}
          onTriggerUndo={() => void undoNow()}
          onTriggerRedo={() => void handleRedo()}
        />

        <ToolLibraryDialog
          open={toolLibraryOpen}
          onOpenChange={setToolLibraryOpen}
          snapshot={librarySnap}
          v090Ready={v090Ready}
          onRefresh={() => refreshLibrary(true)}
        />

        <CardArtStudio
          open={artStudioOpen}
          onOpenChange={setArtStudioOpen}
          assets={cardAssets}
          knowledge={artKnowledge}
          v060Ready={v060Ready}
          onAssetAdded={handleCardAssetAdded}
          onOpenSettings={() => setSettingsOpen(true)}
        />

        <SnapshotsDialog
          open={snapshotsOpen}
          onOpenChange={setSnapshotsOpen}
          snapshots={snapshots}
          busy={busy}
          onCreateSnapshot={(lbl) => void handleCreateSnapshot(lbl)}
          onRestoreSnapshot={(snap) => void handleRestoreSnapshot(snap)}
          onExportSnapshot={(snap) => void handleExportSnapshot(snap)}
        />

        <SettingsDialog
          theme={theme}
          onThemeChange={setTheme}
          open={settingsOpen}
          onOpenChange={setSettingsOpen}
          workspaceMode={workspaceMode}
          compactLabel={activeModelLabel}
          showTrigger={false}
          onApplied={({ studioConnected, modelConfigured, workspaceMode: nextMode }) => {
            setStudioOk(studioConnected);
            setModelOk(modelConfigured);
            setWorkspaceMode(nextMode);
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
        <Toaster position="bottom-right" theme={theme} />
      </div>
    </MotionConfig>
  );
}
