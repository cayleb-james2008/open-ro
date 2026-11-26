import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { useReducedMotion } from "@/motion";
import {
  ChevronDown,
  CopyPlus,
  Crosshair,
  Focus,
  Lightbulb,
  Maximize2,
  Mountain,
  MousePointer2,
  Move,
  Play,
  Plus,
  Radio,
  Redo2,
  RotateCcw,
  RotateCw,
  Square,
  Terminal,
  Trash2,
  Video,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import { Pill, PillIndicator } from "@/components/kibo-ui/pill";
import type {
  InstanceEditPayload,
  PartPreset,
  Place,
  ProjectAssetRecord,
  StudioActionEvent,
  StudioMode,
} from "./data";
import type { WorkspaceLighting, WorkspaceMeshEntry } from "./ship/api";
import {
  VIEWPORT_CANVAS_H,
  VIEWPORT_CANVAS_W,
  buildCameraBasis,
  drawCanvas2DFallbackScene,
  drawViewportHud,
  rayPickScene,
} from "./viewport/gizmos";
import { RendererStatusBadge, TerrainLightingToolbar } from "./viewport/TerrainLightingToolbar";
import { buildTerrainMeshData } from "./viewport/terrainMesh";
import {
  extractLightsFromPlace,
  extractParts,
  parseObjGeometry,
  type AgentAnimState,
  type CameraState,
  type GizmoHandleHit,
  type ParsedObjGeometry,
  type Part3D,
  type RendererMode,
  type StudioTool,
  type TerrainGeneratorKind,
  type Vec2,
  type Vec3,
  type ViewCubeHit,
  type WorkspaceLight,
  type WorkspaceLightKind,
  type WorkspaceTerrain,
} from "./viewport/types";
import { WebGL2ViewportRenderer } from "./viewport/webgl2Renderer";

export type { StudioTool };
type ConsoleFilter = "all" | "parts" | "scripts" | "console";

const PART_PRESETS: Array<{ id: PartPreset; label: string; dotClass: string }> = [
  { id: "block", label: "Block", dotClass: "bg-slate-400" },
  { id: "sphere", label: "Sphere", dotClass: "bg-blue-500" },
  { id: "cylinder", label: "Cylinder", dotClass: "bg-amber-500" },
  { id: "spawn", label: "SpawnLocation", dotClass: "bg-blue-600" },
  { id: "jump_pad", label: "JumpPad", dotClass: "bg-emerald-600" },
  { id: "kill_brick", label: "KillBrick", dotClass: "bg-red-500" },
  { id: "decal_board", label: "DecalBoard", dotClass: "bg-violet-600" },
];

const DEFAULT_CAMERA: CameraState = {
  tx: 0,
  ty: 3,
  tz: 14,
  yaw: 0.68,
  pitch: 0.44,
  dist: 56,
};

function formatActionTime(tsMs: number): string {
  const d = new Date(tsMs);
  return d.toLocaleTimeString([], { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function actionPrefix(kind: StudioActionEvent["kind"]): { tag: string; cls: string } {
  switch (kind) {
    case "part_created":
      return { tag: "+ Part", cls: "bg-tag-mint-bg text-tag-mint-fg" };
    case "part_updated":
      return { tag: "Edit", cls: "bg-tag-sky-bg text-tag-sky-fg" };
    case "script_written":
      return { tag: "Luau", cls: "bg-tag-research-bg text-tag-research-fg" };
    case "decal_bound":
      return { tag: "Art", cls: "bg-tag-design-bg text-tag-design-fg" };
    case "luau_executed":
      return { tag: "Run", cls: "bg-tag-dev-bg text-tag-dev-fg" };
    case "playtest_started":
      return { tag: "Test", cls: "bg-tag-ops-bg text-tag-ops-fg" };
    case "playtest_log":
      return { tag: "Log", cls: "bg-board text-ink-2" };
    case "playtest_passed":
      return { tag: "Pass", cls: "bg-pill-active-bg text-pill-active-fg" };
    case "playtest_failed":
      return { tag: "Fail", cls: "bg-pill-pro-bg text-pill-pro-fg" };
    case "studio_frame":
      return { tag: "Frame", cls: "bg-tag-sky-bg text-tag-sky-fg" };
    case "reverted":
      return { tag: "Undo", cls: "bg-pill-warn-bg text-pill-warn-fg" };
    case "imported":
      return { tag: "XML", cls: "bg-tag-ops-bg text-tag-ops-fg" };
    case "restored":
      return { tag: "Snap", cls: "bg-tag-research-bg text-tag-research-fg" };
    default:
      return { tag: "Focus", cls: "bg-board text-ink-2" };
  }
}

export function StudioViewport3D({
  place,
  theme,
  assets,
  meshes = [],
  lighting,
  terrain: terrainProp,
  lights: lightsProp,
  projectId: projectIdProp,
  selectedPath,
  studioMode,
  actions,
  building,
  canRedo = false,
  hideConsole = false,
  onSelectPart,
  onSaveInstance,
  onCreatePart,
  onDuplicatePart,
  onDeletePart,
  onTriggerRedo,
  onRunPlaytest,
}: {
  place: Place;
  theme: "light" | "dark";
  assets: ProjectAssetRecord[];
  meshes?: WorkspaceMeshEntry[];
  lighting?: WorkspaceLighting | null;
  terrain?: WorkspaceTerrain | null;
  lights?: WorkspaceLight[] | null;
  projectId?: string | null;
  selectedPath?: string | null;
  studioMode: StudioMode;
  actions: StudioActionEvent[];
  building?: boolean;
  canRedo?: boolean;
  hideConsole?: boolean;
  onSelectPart?: (path: string) => void;
  onSaveInstance?: (payload: InstanceEditPayload) => Promise<void> | void;
  onCreatePart?: (preset: PartPreset) => Promise<void> | void;
  onDuplicatePart?: (path: string) => Promise<void> | void;
  onDeletePart?: (path: string) => Promise<void> | void;
  onTriggerRedo?: () => Promise<void> | void;
  onRunPlaytest?: () => Promise<void> | void;
}) {
  const reduce = useReducedMotion();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const overlayRef = useRef<HTMLCanvasElement>(null);
  const glRendererRef = useRef<WebGL2ViewportRenderer | null>(null);

  const [rendererMode, setRendererMode] = useState<RendererMode>("canvas2d-fallback");
  const [glContextLost, setGlContextLost] = useState(false);

  const mountedRef = useRef(true);
  const imgCacheRef = useRef<Map<string, HTMLImageElement>>(new Map());
  const failedImgUrlsRef = useRef<Set<string>>(new Set());
  const [imgTick, setImgTick] = useState(0);
  const meshCacheRef = useRef<Map<string, ParsedObjGeometry | "loading" | "error">>(new Map());
  const meshAbortControllersRef = useRef<Set<AbortController>>(new Set());
  const [meshTick, setMeshTick] = useState(0);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      for (const ctrl of meshAbortControllersRef.current) {
        ctrl.abort();
      }
      meshAbortControllersRef.current.clear();
      meshCacheRef.current.clear();
      for (const img of imgCacheRef.current.values()) {
        img.onload = null;
        img.onerror = null;
      }
      imgCacheRef.current.clear();
      failedImgUrlsRef.current.clear();
    };
  }, []);

  const [tool, setTool] = useState<StudioTool>("select");
  const [followAgent, setFollowAgent] = useState(true);
  const [consoleOpen, setConsoleOpen] = useState(true);
  const [consoleFilter, setConsoleFilter] = useState<ConsoleFilter>("all");
  const [replaying, setReplaying] = useState(false);
  const [replayIndex, setReplayIndex] = useState<number | null>(null);
  const [playtestBusy, setPlaytestBusy] = useState(false);
  const [addPartOpen, setAddPartOpen] = useState(false);
  const [toolbarPanel, setToolbarPanel] = useState<"terrain" | "lights" | null>(null);
  const [partActionBusy, setPartActionBusy] = useState(false);
  const [sceneMutateBusy, setSceneMutateBusy] = useState(false);
  const [localDraftEdit, setLocalDraftEdit] = useState<{ path: string; pos: Vec3; size: Vec3; rotY: number } | null>(null);

  // Project workspace sync for Terrain, Lights, and textured Mesh entries
  const [resolvedProjectId, setResolvedProjectId] = useState<string | null>(projectIdProp ?? null);
  const [wsTerrain, setWsTerrain] = useState<WorkspaceTerrain | null>(null);
  const [wsLights, setWsLights] = useState<WorkspaceLight[] | null>(null);
  const [wsMeshes, setWsMeshes] = useState<WorkspaceMeshEntry[]>([]);
  const [wsLighting, setWsLighting] = useState<WorkspaceLighting | null>(null);

  const inferredProjectId = useMemo(() => {
    if (projectIdProp) return projectIdProp;
    for (const m of meshes) {
      const match = (m.obj_url || "").match(/\/api\/projects\/([^/]+)\//);
      if (match) return decodeURIComponent(match[1]);
    }
    return resolvedProjectId;
  }, [meshes, projectIdProp, resolvedProjectId]);

  // Discover active project ID if not explicitly passed
  useEffect(() => {
    if (projectIdProp) {
      setResolvedProjectId(projectIdProp);
      return;
    }
    let cancelled = false;
    void fetch("/api/projects", { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : null))
      .then((data: { active?: string | null } | null) => {
        if (!cancelled && data && typeof data.active === "string" && data.active) {
          setResolvedProjectId(data.active);
        }
      })
      .catch(() => {
        /* local place mode without projects */
      });
    return () => {
      cancelled = true;
    };
  }, [projectIdProp, place.instances]);

  const placeInstanceKeySig = useMemo(() => Object.keys(place.instances || {}).length, [place.instances]);

  const refreshWorkspaceExtras = useCallback(async (pid: string) => {
    try {
      const r = await fetch(`/api/projects/${encodeURIComponent(pid)}/workspace`, { cache: "no-store" });
      if (!r.ok) return;
      const data = (await r.json()) as {
        terrain?: WorkspaceTerrain | null;
        lights?: WorkspaceLight[];
        meshes?: WorkspaceMeshEntry[];
        lighting?: WorkspaceLighting;
      };
      if (!mountedRef.current) return;
      if (data.terrain !== undefined) setWsTerrain(data.terrain);
      if (Array.isArray(data.lights)) setWsLights(data.lights);
      if (Array.isArray(data.meshes)) setWsMeshes(data.meshes);
      if (data.lighting) setWsLighting(data.lighting);
    } catch {
      /* ignore transient fetch failure */
    }
  }, []);

  useEffect(() => {
    if (!inferredProjectId) return;
    void refreshWorkspaceExtras(inferredProjectId);
  }, [inferredProjectId, placeInstanceKeySig, refreshWorkspaceExtras]);

  useEffect(() => {
    if (lighting) setWsLighting(lighting);
  }, [lighting]);

  const effectiveTerrain = terrainProp !== undefined ? terrainProp : wsTerrain;
  const effectiveLighting = wsLighting ?? lighting ?? null;
  const effectiveMeshes = useMemo(() => {
    const byName = new Map<string, WorkspaceMeshEntry>();
    for (const m of wsMeshes) byName.set(m.obj_path || m.name, m);
    for (const m of meshes) {
      const key = m.obj_path || m.name;
      const prev = byName.get(key);
      byName.set(key, prev ? { ...prev, ...m, texture_url: m.texture_url ?? prev.texture_url } : m);
    }
    return Array.from(byName.values());
  }, [meshes, wsMeshes]);

  const terrainMesh = useMemo(() => buildTerrainMeshData(effectiveTerrain), [effectiveTerrain]);

  const camRef = useRef<CameraState>({ ...DEFAULT_CAMERA });
  const targetCamRef = useRef<CameraState>({ ...DEFAULT_CAMERA });
  const gizmoHitsRef = useRef<GizmoHandleHit[]>([]);
  const viewCubeHitsRef = useRef<ViewCubeHit[]>([]);
  const keysRef = useRef<Set<string>>(new Set());

  const dragRef = useRef<{
    kind: "orbit" | "pan" | "gizmo";
    axis?: "x" | "y" | "z";
    dir2d?: Vec2;
    startX: number;
    startY: number;
    moved: boolean;
    origCam: CameraState;
    origPart?: { path: string; pos: Vec3; size: Vec3; rotY: number; className: string; rawProps: Record<string, unknown> };
  } | null>(null);

  const agentAnimRef = useRef<AgentAnimState>({
    x: 0,
    y: 12,
    z: 8,
    tx: 0,
    ty: 12,
    tz: 8,
    beam: 0,
    popPath: "",
    popProgress: 1,
    callout: "",
    calloutUntil: 0,
    time: 0,
  });

  const parts = useMemo(() => {
    const raw = extractParts(place, assets, effectiveMeshes);
    if (!localDraftEdit) return raw;
    return raw.map((p) =>
      p.path === localDraftEdit.path
        ? { ...p, pos: localDraftEdit.pos, size: localDraftEdit.size, rotYDeg: localDraftEdit.rotY }
        : p,
    );
  }, [assets, effectiveMeshes, localDraftEdit, place]);

  const lights = useMemo(
    () => extractLightsFromPlace(place, lightsProp !== undefined ? lightsProp : wsLights),
    [lightsProp, place, wsLights],
  );

  const selectedPart = useMemo(
    () => parts.find((p) => p.path === selectedPath) || null,
    [parts, selectedPath],
  );

  const replaySteps = useMemo<StudioActionEvent[]>(() => {
    const buildActions = actions.filter((a) =>
      ["part_created", "part_updated", "decal_bound", "script_written", "playtest_passed", "playtest_failed"].includes(
        a.kind,
      ),
    );
    if (buildActions.length > 0) return buildActions.slice(-40);
    const synthetic: StudioActionEvent[] = [];
    let seq = 1;
    for (const p of parts) {
      synthetic.push({
        seq: seq++,
        ts_ms: Date.now() - (parts.length - seq) * 500,
        kind: p.decalUrl ? "decal_bound" : "part_created",
        studio_mode: studioMode,
        path: p.path,
        class_name: p.className,
        position: p.pos,
        size: p.size,
        color: p.color,
        summary: p.decalUrl ? `Bound card art on ${p.path}` : `Placed ${p.path} (${p.className})`,
      });
    }
    for (const scriptPath of Object.keys(place.scripts || {})) {
      synthetic.push({
        seq: seq++,
        ts_ms: Date.now(),
        kind: "script_written",
        studio_mode: studioMode,
        path: scriptPath,
        class_name: scriptPath.startsWith("StarterGui.") ? "LocalScript" : "Script",
        summary: `Wrote ${scriptPath}`,
      });
    }
    if (synthetic.length > 0) {
      synthetic.push({
        seq: seq++,
        ts_ms: Date.now(),
        kind: "playtest_passed",
        studio_mode: studioMode,
        path: "Workspace",
        class_name: "Playtest",
        summary: `Playtest passed (${parts.length} parts, ${Object.keys(place.scripts || {}).length} scripts)`,
      });
    }
    return synthetic;
  }, [actions, parts, place.scripts, studioMode]);

  // Preload project .obj mesh geometry + normals + UVs
  useEffect(() => {
    const urls = new Set<string>();
    for (const p of parts) {
      if (p.meshUrl) urls.add(p.meshUrl);
    }
    for (const m of effectiveMeshes) {
      if (m.obj_url) urls.add(m.obj_url);
    }
    for (const url of urls) {
      if (meshCacheRef.current.has(url)) continue;
      meshCacheRef.current.set(url, "loading");
      const ctrl = new AbortController();
      meshAbortControllersRef.current.add(ctrl);
      void fetch(url, { cache: "no-store", signal: ctrl.signal })
        .then((r) => (r.ok ? r.text() : Promise.reject(new Error(`mesh fetch ${r.status}`))))
        .then((txt) => {
          if (!mountedRef.current) return;
          const trimmed = txt.trimStart();
          const parsed = trimmed && !trimmed.startsWith("<") ? parseObjGeometry(txt) : null;
          meshCacheRef.current.set(url, parsed ?? "error");
          setMeshTick((t) => t + 1);
        })
        .catch(() => {
          if (!mountedRef.current) return;
          meshCacheRef.current.set(url, "error");
        })
        .finally(() => {
          meshAbortControllersRef.current.delete(ctrl);
        });
    }
  }, [effectiveMeshes, parts]);

  // Preload textures & decals
  useEffect(() => {
    const urls = new Set<string>();
    for (const p of parts) {
      if (p.decalUrl) urls.add(p.decalUrl);
    }
    for (const m of effectiveMeshes) {
      if (m.texture_url) urls.add(m.texture_url);
    }
    for (const url of urls) {
      if (imgCacheRef.current.has(url) || failedImgUrlsRef.current.has(url)) continue;
      const img = new Image();
      img.onload = () => {
        if (!mountedRef.current) return;
        if (img.naturalWidth <= 0 || img.naturalHeight <= 0) {
          imgCacheRef.current.delete(url);
          failedImgUrlsRef.current.add(url);
          return;
        }
        setImgTick((t) => t + 1);
      };
      img.onerror = () => {
        imgCacheRef.current.delete(url);
        failedImgUrlsRef.current.add(url);
      };
      img.src = url;
      imgCacheRef.current.set(url, img);
    }
  }, [effectiveMeshes, parts]);

  // Initialize native WebGL2 renderer (falls back to Canvas2D cleanly if null)
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const renderer = WebGL2ViewportRenderer.tryCreate(canvas, {
      onContextLost: () => {
        if (mountedRef.current) setGlContextLost(true);
      },
      onContextRestored: () => {
        if (mountedRef.current) setGlContextLost(false);
      },
    });
    if (renderer) {
      glRendererRef.current = renderer;
      setRendererMode("webgl2");
    } else {
      glRendererRef.current = null;
      setRendererMode("canvas2d-fallback");
    }
    const handleWindowBlur = () => {
      keysRef.current.clear();
    };
    window.addEventListener("blur", handleWindowBlur);
    return () => {
      window.removeEventListener("blur", handleWindowBlur);
      keysRef.current.clear();
      renderer?.dispose();
      glRendererRef.current = null;
    };
  }, []);

  const focusOnCoords = useCallback(
    (pos: Vec3, size?: Vec3, zoomDist?: number) => {
      const nextDist =
        zoomDist ??
        Math.max(24, Math.min(64, Math.max(size?.[0] || 8, size?.[1] || 4, size?.[2] || 8) * 3.4));
      targetCamRef.current = {
        ...targetCamRef.current,
        tx: pos[0],
        ty: pos[1] + (size?.[1] || 2) * 0.35,
        tz: pos[2],
        dist: nextDist,
      };
      if (reduce) camRef.current = { ...targetCamRef.current };
    },
    [reduce],
  );

  const focusAgentOnAction = useCallback(
    (ev: StudioActionEvent) => {
      const matchedPart = parts.find((p) => p.path === ev.path);
      const pos: Vec3 = ev.position || matchedPart?.pos || [0, 3, 8];
      const size: Vec3 = ev.size || matchedPart?.size || [6, 3, 6];
      const anim = agentAnimRef.current;
      anim.tx = pos[0];
      anim.ty = pos[1] + size[1] * 0.5 + 8.5;
      anim.tz = pos[2];
      if (reduce) {
        anim.x = anim.tx;
        anim.y = anim.ty;
        anim.z = anim.tz;
      }
      anim.beam = 1;
      if (ev.kind === "part_created" || ev.kind === "part_updated" || ev.kind === "decal_bound") {
        anim.popPath = ev.path;
        anim.popProgress = reduce ? 1 : 0.15;
      }
      const shortPrefix =
        ev.kind === "part_created"
          ? "+"
          : ev.kind === "script_written"
            ? "*"
            : ev.kind === "playtest_passed"
              ? ">"
              : ev.kind === "decal_bound"
                ? "#"
                : "-";
      anim.callout = `${shortPrefix} ${ev.path || ev.summary}`;
      anim.calloutUntil = performance.now() + 4200;

      if (followAgent) focusOnCoords(pos, size, 42);
    },
    [followAgent, focusOnCoords, parts, reduce],
  );

  const lastSeqRef = useRef<number>(0);
  useEffect(() => {
    if (replaying || actions.length === 0) return;
    const latest = actions[actions.length - 1];
    if (latest.seq !== lastSeqRef.current) {
      lastSeqRef.current = latest.seq;
      focusAgentOnAction(latest);
    }
  }, [actions, focusAgentOnAction, replaying]);

  useEffect(() => {
    if (!replaying) {
      setReplayIndex(null);
      return;
    }
    if (replaySteps.length === 0) {
      setReplaying(false);
      return;
    }
    let idx = 0;
    setReplayIndex(0);
    focusAgentOnAction(replaySteps[0]);
    const id = window.setInterval(() => {
      idx++;
      if (idx >= replaySteps.length) {
        window.clearInterval(id);
        setReplaying(false);
        setReplayIndex(null);
        toast.success(`Replayed ${replaySteps.length} Studio build actions`);
        return;
      }
      setReplayIndex(idx);
      focusAgentOnAction(replaySteps[idx]);
    }, reduce ? 350 : 680);
    return () => window.clearInterval(id);
  }, [replaying, replaySteps, focusAgentOnAction, reduce]);

  const visibleParts = useMemo(() => {
    if (!replaying || replayIndex === null) return parts;
    const placedSoFar = new Set<string>();
    const mentionedAnyPart = replaySteps.some((s) => s.path.startsWith("Workspace."));
    if (!mentionedAnyPart) return parts;
    for (let i = 0; i <= Math.min(replayIndex, replaySteps.length - 1); i++) {
      const s = replaySteps[i];
      if (s.path.startsWith("Workspace.")) placedSoFar.add(s.path);
    }
    return parts.filter((p) => placedSoFar.has(p.path));
  }, [parts, replayIndex, replaySteps, replaying]);

  const resetCamera = useCallback(() => {
    if (parts.length === 0) {
      targetCamRef.current = { ...DEFAULT_CAMERA };
    } else {
      let sx = 0;
      let sy = 0;
      let sz = 0;
      let maxR = 14;
      for (const p of parts) {
        sx += p.pos[0];
        sy += p.pos[1];
        sz += p.pos[2];
      }
      const cx = sx / parts.length;
      const cy = sy / parts.length;
      const cz = sz / parts.length;
      for (const p of parts) {
        const d = Math.hypot(p.pos[0] - cx, p.pos[2] - cz) + Math.max(p.size[0], p.size[2]) * 0.5;
        if (d > maxR) maxR = d;
      }
      targetCamRef.current = {
        tx: cx,
        ty: Math.max(2, cy),
        tz: cz,
        yaw: 0.64,
        pitch: 0.44,
        dist: Math.max(36, Math.min(92, maxR * 1.95)),
      };
    }
    if (reduce) camRef.current = { ...targetCamRef.current };
  }, [parts, reduce]);

  const framedCountRef = useRef(0);
  useEffect(() => {
    if (parts.length > 0 && framedCountRef.current === 0) {
      framedCountRef.current = parts.length;
      resetCamera();
    } else if (parts.length === 0) {
      framedCountRef.current = 0;
    }
  }, [parts.length, resetCamera]);

  // Unified Draw Function (WebGL2 + Overlay HUD, OR Canvas2D Fallback)
  const drawScene = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const anim = agentAnimRef.current;
    const glRenderer = glRendererRef.current;

    if (glRenderer && rendererMode === "webgl2") {
      if (!glRenderer.isContextLost()) {
        glRenderer.render({
          width: VIEWPORT_CANVAS_W,
          height: VIEWPORT_CANVAS_H,
          cam: camRef.current,
          theme,
          lighting: effectiveLighting,
          parts: visibleParts,
          lights,
          terrainMesh,
          selectedPath,
          popPath: anim.popPath,
          popProgress: anim.popProgress,
          timeSec: anim.time,
          imgCache: imgCacheRef.current,
          meshCache: meshCacheRef.current,
        });
      }
      const overlay = overlayRef.current;
      const octx = overlay?.getContext("2d");
      if (octx) {
        const hits = drawViewportHud(octx, {
          clear: true,
          width: VIEWPORT_CANVAS_W,
          height: VIEWPORT_CANVAS_H,
          cam: camRef.current,
          theme,
          parts: visibleParts,
          lights,
          selectedPath,
          selectedPart,
          tool,
          building,
          replaying,
          reduce,
          hasTerrain: Boolean(terrainMesh),
          anim,
        });
        gizmoHitsRef.current = hits.gizmoHits;
        viewCubeHitsRef.current = hits.viewCubeHits;
      }
      return;
    }

    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    drawCanvas2DFallbackScene(ctx, {
      width: VIEWPORT_CANVAS_W,
      height: VIEWPORT_CANVAS_H,
      cam: camRef.current,
      theme,
      lighting: effectiveLighting,
      parts: visibleParts,
      lights,
      terrainMesh,
      selectedPath,
      anim,
      imgCache: imgCacheRef.current,
      meshCache: meshCacheRef.current,
    });
    const hits = drawViewportHud(ctx, {
      clear: false,
      width: VIEWPORT_CANVAS_W,
      height: VIEWPORT_CANVAS_H,
      cam: camRef.current,
      theme,
      parts: visibleParts,
      lights,
      selectedPath,
      selectedPart,
      tool,
      building,
      replaying,
      reduce,
      hasTerrain: Boolean(terrainMesh),
      anim,
    });
    gizmoHitsRef.current = hits.gizmoHits;
    viewCubeHitsRef.current = hits.viewCubeHits;
  }, [
    building,
    effectiveLighting,
    glContextLost,
    imgTick,
    lights,
    meshTick,
    reduce,
    rendererMode,
    replaying,
    selectedPart,
    selectedPath,
    terrainMesh,
    theme,
    tool,
    visibleParts,
  ]);

  // 60fps animation loop
  useEffect(() => {
    drawScene();
    if (reduce) return;

    let rafId = 0;
    let lastTs = performance.now();
    const hasWater = Boolean(terrainMesh && terrainMesh.waterQuads.length > 0 && rendererMode === "webgl2");

    const tick = (now: number) => {
      const dt = Math.min(0.05, Math.max(0.001, (now - lastTs) / 1000));
      lastTs = now;
      let dirty = hasWater;

      if (keysRef.current.size > 0) {
        const speed = 28 * dt;
        const { forward, right } = buildCameraBasis(targetCamRef.current, VIEWPORT_CANVAS_W, VIEWPORT_CANVAS_H);
        const fHoriz = Math.hypot(forward[0], forward[2]) || 1;
        const fx = forward[0] / fHoriz;
        const fz = forward[2] / fHoriz;
        const k = keysRef.current;
        if (k.has("w") || k.has("W")) {
          targetCamRef.current.tx += fx * speed;
          targetCamRef.current.tz += fz * speed;
          dirty = true;
        }
        if (k.has("s") || k.has("S")) {
          targetCamRef.current.tx -= fx * speed;
          targetCamRef.current.tz -= fz * speed;
          dirty = true;
        }
        if (k.has("d") || k.has("D")) {
          targetCamRef.current.tx += right[0] * speed;
          targetCamRef.current.tz += right[2] * speed;
          dirty = true;
        }
        if (k.has("a") || k.has("A")) {
          targetCamRef.current.tx -= right[0] * speed;
          targetCamRef.current.tz -= right[2] * speed;
          dirty = true;
        }
        if (k.has("e") || k.has("E")) {
          targetCamRef.current.ty += speed * 0.7;
          dirty = true;
        }
        if (k.has("q") || k.has("Q")) {
          targetCamRef.current.ty = Math.max(0.5, targetCamRef.current.ty - speed * 0.7);
          dirty = true;
        }
      }

      const cam = camRef.current;
      const tc = targetCamRef.current;
      const lerp = 1 - Math.exp(-8 * dt);
      const camDelta =
        Math.abs(tc.tx - cam.tx) +
        Math.abs(tc.ty - cam.ty) +
        Math.abs(tc.tz - cam.tz) +
        Math.abs(tc.yaw - cam.yaw) +
        Math.abs(tc.pitch - cam.pitch) +
        Math.abs(tc.dist - cam.dist);
      if (camDelta > 0.004) {
        cam.tx += (tc.tx - cam.tx) * lerp;
        cam.ty += (tc.ty - cam.ty) * lerp;
        cam.tz += (tc.tz - cam.tz) * lerp;
        cam.yaw += (tc.yaw - cam.yaw) * lerp;
        cam.pitch += (tc.pitch - cam.pitch) * lerp;
        cam.dist += (tc.dist - cam.dist) * lerp;
        dirty = true;
      }

      const anim = agentAnimRef.current;
      anim.time += dt;
      const aDelta = Math.abs(anim.tx - anim.x) + Math.abs(anim.ty - anim.y) + Math.abs(anim.tz - anim.z);
      if (aDelta > 0.02) {
        anim.x += (anim.tx - anim.x) * lerp;
        anim.y += (anim.ty - anim.y) * lerp;
        anim.z += (anim.tz - anim.z) * lerp;
        dirty = true;
      }
      if (anim.beam > 0.01 && !building && !replaying) {
        anim.beam = Math.max(0, anim.beam - dt * 0.55);
        dirty = true;
      }
      if (anim.popProgress < 1) {
        anim.popProgress = Math.min(1, anim.popProgress + dt * 2.6);
        dirty = true;
      }
      if (building || replaying || now < anim.calloutUntil) dirty = true;

      if (dirty) drawScene();
      rafId = requestAnimationFrame(tick);
    };

    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, [building, drawScene, reduce, rendererMode, replaying, terrainMesh]);

  const commitTransform = useCallback(
    async (part: Part3D, nextPos: Vec3, nextSize: Vec3, nextRotY: number) => {
      if (!onSaveInstance) return;
      const round1 = (v: number) => Math.round(v * 10) / 10;
      const props: Record<string, unknown> = {
        ...part.rawProps,
        Position: nextPos.map(round1),
        Size: nextSize.map((n) => Math.max(1, round1(n))),
      };
      if (Math.abs(nextRotY) > 0.1 || part.rawProps.Rotation) {
        props.Rotation = [0, Math.round(nextRotY) % 360, 0];
      }
      await onSaveInstance({
        path: part.path,
        class_name: part.className,
        properties: props,
      });
    },
    [onSaveInstance],
  );

  const setViewPreset = useCallback(
    (preset: "iso" | "top" | "front" | "side") => {
      const base = targetCamRef.current;
      if (preset === "iso") targetCamRef.current = { ...base, yaw: 0.68, pitch: 0.44 };
      else if (preset === "top") targetCamRef.current = { ...base, yaw: 0, pitch: 1.38 };
      else if (preset === "side") targetCamRef.current = { ...base, yaw: 1.57, pitch: 0.18 };
      else targetCamRef.current = { ...base, yaw: 0, pitch: 0.16 };
      if (reduce) {
        camRef.current = { ...targetCamRef.current };
        drawScene();
      }
    },
    [drawScene, reduce],
  );

  // Pointer handlers
  const onPointerDown = (e: ReactPointerEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.setPointerCapture(e.pointerId);
    const rect = canvas.getBoundingClientRect();
    const mx = ((e.clientX - rect.left) / rect.width) * VIEWPORT_CANVAS_W;
    const my = ((e.clientY - rect.top) / rect.height) * VIEWPORT_CANVAS_H;

    if (selectedPart && tool !== "select") {
      for (const gh of gizmoHitsRef.current) {
        if (Math.hypot(mx - gh.x, my - gh.y) <= 14) {
          dragRef.current = {
            kind: "gizmo",
            axis: gh.axis,
            dir2d: gh.dir2d,
            startX: e.clientX,
            startY: e.clientY,
            moved: false,
            origCam: { ...targetCamRef.current },
            origPart: {
              path: selectedPart.path,
              pos: [...selectedPart.pos] as Vec3,
              size: [...selectedPart.size] as Vec3,
              rotY: selectedPart.rotYDeg,
              className: selectedPart.className,
              rawProps: selectedPart.rawProps,
            },
          };
          return;
        }
      }
    }

    dragRef.current = {
      kind: e.shiftKey || e.button === 2 ? "pan" : "orbit",
      startX: e.clientX,
      startY: e.clientY,
      moved: false,
      origCam: { ...targetCamRef.current },
    };
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLCanvasElement>) => {
    const d = dragRef.current;
    if (!d) return;
    const dx = e.clientX - d.startX;
    const dy = e.clientY - d.startY;
    if (Math.hypot(dx, dy) > 4) d.moved = true;

    if (d.kind === "gizmo" && d.origPart && d.axis) {
      const axisIdx = d.axis === "x" ? 0 : d.axis === "y" ? 1 : 2;
      const projDelta = d.dir2d ? dx * d.dir2d[0] + dy * d.dir2d[1] : dx;
      const nextPos: Vec3 = [...d.origPart.pos];
      const nextSize: Vec3 = [...d.origPart.size];
      let nextRotY = d.origPart.rotY;

      if (tool === "move") {
        const rawVal = Math.round((d.origPart.pos[axisIdx] + projDelta * 0.14) * 2) / 2;
        const minP = axisIdx === 1 ? -64 : -256;
        nextPos[axisIdx] = Math.max(minP, Math.min(256, rawVal));
      } else if (tool === "scale") {
        nextSize[axisIdx] = Math.max(1, Math.min(256, Math.round((d.origPart.size[axisIdx] + projDelta * 0.12) * 2) / 2));
      } else if (tool === "rotate") {
        nextRotY = (d.origPart.rotY + Math.round(dx * 0.8)) % 360;
      }
      setLocalDraftEdit({ path: d.origPart.path, pos: nextPos, size: nextSize, rotY: nextRotY });
      return;
    }

    if (d.kind === "orbit") {
      targetCamRef.current = {
        ...targetCamRef.current,
        yaw: d.origCam.yaw - dx * 0.008,
        pitch: Math.max(0.08, Math.min(1.42, d.origCam.pitch + dy * 0.007)),
      };
      if (reduce) {
        camRef.current = { ...targetCamRef.current };
        drawScene();
      }
    } else if (d.kind === "pan") {
      const { right } = buildCameraBasis(d.origCam, VIEWPORT_CANVAS_W, VIEWPORT_CANVAS_H);
      const scale = d.origCam.dist * 0.0022;
      targetCamRef.current = {
        ...targetCamRef.current,
        tx: d.origCam.tx - right[0] * dx * scale,
        ty: Math.max(-16, d.origCam.ty + dy * scale),
        tz: d.origCam.tz - right[2] * dx * scale,
      };
      if (reduce) {
        camRef.current = { ...targetCamRef.current };
        drawScene();
      }
    }
  };

  const onPointerUp = (e: ReactPointerEvent<HTMLCanvasElement>) => {
    const d = dragRef.current;
    dragRef.current = null;
    if (!d) return;

    if (d.kind === "gizmo" && d.origPart && localDraftEdit && localDraftEdit.path === d.origPart.path) {
      const partObj: Part3D = {
        path: d.origPart.path,
        name: d.origPart.path.split(".").pop() || d.origPart.path,
        className: d.origPart.className,
        shape: "Block",
        pos: d.origPart.pos,
        size: d.origPart.size,
        rotYDeg: d.origPart.rotY,
        rotDeg: [0, d.origPart.rotY, 0],
        color: "#8B939E",
        material: "SmoothPlastic",
        roughness: 0.42,
        metalness: 0.04,
        emissive: 0,
        transparency: 0,
        rawProps: d.origPart.rawProps,
      };
      const draft = localDraftEdit;
      setLocalDraftEdit(null);
      void commitTransform(partObj, draft.pos, draft.size, draft.rotY);
      return;
    }

    if (!d.moved) {
      const canvas = canvasRef.current;
      if (!canvas) return;
      const rect = canvas.getBoundingClientRect();
      const mx = ((e.clientX - rect.left) / rect.width) * VIEWPORT_CANVAS_W;
      const my = ((e.clientY - rect.top) / rect.height) * VIEWPORT_CANVAS_H;

      // ViewCube quick snap
      for (const vc of viewCubeHitsRef.current) {
        if (Math.hypot(mx - vc.x, my - vc.y) <= 10) {
          if (vc.axis === "y") setViewPreset("top");
          else if (vc.axis === "x") setViewPreset("side");
          else setViewPreset("front");
          return;
        }
      }

      // 3D Ray-pick parts & lights
      const hitPath = rayPickScene(mx, my, camRef.current, VIEWPORT_CANVAS_W, VIEWPORT_CANVAS_H, visibleParts, lights);
      if (hitPath) {
        onSelectPart?.(hitPath);
        const p = parts.find((item) => item.path === hitPath);
        const l = lights.find((item) => item.path === hitPath);
        const pos = p?.pos || l?.position;
        if (pos) {
          agentAnimRef.current.callout = `Selected ${hitPath}`;
          agentAnimRef.current.calloutUntil = performance.now() + 2400;
          agentAnimRef.current.tx = pos[0];
          agentAnimRef.current.ty = pos[1] + (p?.size[1] ?? 2) * 0.5 + 8;
          agentAnimRef.current.tz = pos[2];
        }
        drawScene();
      }
    }
  };

  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const handleWheel = (e: globalThis.WheelEvent) => {
      e.preventDefault();
      const nextDist = Math.max(14, Math.min(180, targetCamRef.current.dist * (e.deltaY > 0 ? 1.1 : 0.91)));
      targetCamRef.current = { ...targetCamRef.current, dist: nextDist };
      if (reduce) {
        camRef.current = { ...targetCamRef.current };
        drawScene();
      }
    };
    el.addEventListener("wheel", handleWheel, { passive: false });
    return () => el.removeEventListener("wheel", handleWheel);
  }, [drawScene, reduce]);

  const onPointerCancel = () => {
    dragRef.current = null;
    setLocalDraftEdit(null);
  };

  const nudgeSelected = (axis: "x" | "y" | "z", delta: number) => {
    if (!selectedPart) return;
    const idx = axis === "x" ? 0 : axis === "y" ? 1 : 2;
    const nextPos: Vec3 = [...selectedPart.pos];
    const nextSize: Vec3 = [...selectedPart.size];
    let nextRotY = selectedPart.rotYDeg;
    if (tool === "scale") nextSize[idx] = Math.max(1, Math.min(256, nextSize[idx] + delta));
    else if (tool === "rotate") nextRotY = (nextRotY + delta * 15) % 360;
    else nextPos[idx] = Math.max(idx === 1 ? -64 : -256, Math.min(256, nextPos[idx] + delta));
    void commitTransform(selectedPart, nextPos, nextSize, nextRotY);
  };

  const onCanvasKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (["w", "a", "s", "d", "q", "e", "W", "A", "S", "D", "Q", "E"].includes(e.key)) {
      e.preventDefault();
      keysRef.current.add(e.key);
    } else if (e.key === "f" || e.key === "F") {
      e.preventDefault();
      if (selectedPart) focusOnCoords(selectedPart.pos, selectedPart.size);
      else resetCamera();
    } else if (e.key === "r" || e.key === "R") {
      e.preventDefault();
      resetCamera();
    } else if (e.key === "1") {
      setTool("select");
    } else if (e.key === "2") {
      setTool("move");
    } else if (e.key === "3") {
      setTool("scale");
    } else if (e.key === "4") {
      setTool("rotate");
    } else if (e.key === "Escape") {
      dragRef.current = null;
      setLocalDraftEdit(null);
      setAddPartOpen(false);
    } else if (selectedPart && ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key)) {
      e.preventDefault();
      const step = e.shiftKey ? 4 : 1;
      if (e.key === "ArrowLeft") nudgeSelected("x", -step);
      else if (e.key === "ArrowRight") nudgeSelected("x", step);
      else if (e.key === "ArrowUp") nudgeSelected("z", -step);
      else if (e.key === "ArrowDown") nudgeSelected("z", step);
    }
  };

  const zoomBy = (factor: number) => {
    targetCamRef.current = {
      ...targetCamRef.current,
      dist: Math.max(14, Math.min(180, targetCamRef.current.dist * factor)),
    };
    if (reduce) {
      camRef.current = { ...targetCamRef.current };
      drawScene();
    }
  };

  // Terrain & Lighting Mutators
  const mutateProjectOps = useCallback(
    async (ops: Array<Record<string, unknown>>): Promise<boolean> => {
      if (!inferredProjectId) return false;
      setSceneMutateBusy(true);
      try {
        const r = await fetch(`/api/projects/${encodeURIComponent(inferredProjectId)}/scene/mutate`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ ops }),
        });
        const data = (await r.json()) as {
          ok?: boolean;
          error?: string;
          terrain?: WorkspaceTerrain | null;
          lights?: WorkspaceLight[];
          lighting?: WorkspaceLighting;
        };
        if (!r.ok || data.ok === false) {
          toast.error(data.error || "Scene mutation failed.");
          return false;
        }
        if (data.terrain !== undefined) setWsTerrain(data.terrain);
        if (Array.isArray(data.lights)) setWsLights(data.lights);
        if (data.lighting) setWsLighting(data.lighting);
        return true;
      } catch {
        toast.error("Could not reach scene mutation endpoint.");
        return false;
      } finally {
        setSceneMutateBusy(false);
      }
    },
    [inferredProjectId],
  );

  const handleGenerateTerrain = async (cfg: {
    kind: TerrainGeneratorKind;
    seed: number;
    amplitude: number;
    water_level: number;
    material: string;
  }) => {
    const nextRegion = {
      id: "terrain-main",
      bounds_min: [-96, -16, -96] as Vec3,
      bounds_max: [96, 48, 96] as Vec3,
      material: cfg.material,
      generator: {
        kind: cfg.kind,
        seed: cfg.seed,
        amplitude: cfg.amplitude,
        water_level: cfg.water_level,
      },
    };
    if (inferredProjectId) {
      const ok = await mutateProjectOps([
        {
          op: "set_terrain",
          enabled: true,
          clear: true,
          regions: [nextRegion],
          generator: nextRegion.generator,
        },
      ]);
      if (ok) toast.success(`Generated ${cfg.kind} terrain`);
    } else {
      setWsTerrain({ enabled: true, cell_size: 4, regions: [nextRegion] });
      toast.success(`Previewing ${cfg.kind} terrain`);
    }
  };

  const handlePaintBox = async (material: string) => {
    const paintBox = {
      id: `paint-${Date.now().toString(36)}`,
      min: [-24, -4, -24] as Vec3,
      max: [24, 4, 24] as Vec3,
      material,
    };
    if (inferredProjectId) {
      const ok = await mutateProjectOps([
        {
          op: "set_terrain",
          enabled: true,
          paint_box: paintBox,
        },
      ]);
      if (ok) toast.success(`Painted ${material} terrain box`);
    } else {
      setWsTerrain((prev) => ({
        enabled: true,
        cell_size: prev?.cell_size ?? 4,
        regions: [
          ...(prev?.regions ?? []),
          { id: paintBox.id, bounds_min: paintBox.min, bounds_max: paintBox.max, material },
        ],
      }));
      toast.success(`Painted ${material} terrain box`);
    }
  };

  const handleClearTerrain = async () => {
    if (inferredProjectId) {
      const ok = await mutateProjectOps([{ op: "set_terrain", enabled: false, clear: true }]);
      if (ok) toast.success("Cleared terrain to baseplate");
    } else {
      setWsTerrain({ enabled: false, cell_size: 4, regions: [] });
      toast.success("Cleared terrain");
    }
  };

  const handleInsertLight = async (kind: WorkspaceLightKind) => {
    const idx = lights.length + 1;
    const baseName = `${kind}${idx}`;
    const targetParent = selectedPart ? selectedPart.path : "Workspace";
    const path = `${targetParent}.${baseName}`;
    const pos: Vec3 = selectedPart
      ? [selectedPart.pos[0], selectedPart.pos[1] + selectedPart.size[1] * 0.5 + 3, selectedPart.pos[2]]
      : [((idx - 1) % 3 - 1) * 12, 10, 6];
    const props: Record<string, unknown> = {
      Position: pos,
      Color: kind === "SpotLight" ? "#FFD59A" : kind === "SurfaceLight" ? "#A8D8FF" : "#FFE4B5",
      Brightness: 2.4,
      Range: 32,
      Enabled: true,
      Shadows: true,
    };
    if (kind === "SpotLight") {
      props.Angle = 60;
      props.Direction = [0, -1, 0.25];
    } else if (kind === "SurfaceLight") {
      props.Face = "Bottom";
      props.Direction = [0, -1, 0];
    }

    if (onSaveInstance) {
      await onSaveInstance({ path, class_name: kind, properties: props });
      onSelectPart?.(path);
      if (inferredProjectId) void refreshWorkspaceExtras(inferredProjectId);
      return;
    }

    if (inferredProjectId) {
      const ok = await mutateProjectOps([{ op: "upsert", path, class_name: kind, properties: props }]);
      if (ok) {
        onSelectPart?.(path);
        toast.success(`Inserted ${kind}`);
      }
    }
  };

  const handleToggleLight = async (light: WorkspaceLight) => {
    const nextEnabled = light.enabled === false;
    const existing = place.instances?.[light.path]?.properties || {};
    const props = {
      ...existing,
      Position: light.position,
      Color: light.color,
      Brightness: light.brightness,
      Range: light.range,
      Enabled: nextEnabled,
    };
    if (onSaveInstance) {
      await onSaveInstance({ path: light.path, class_name: light.kind, properties: props });
      if (inferredProjectId) void refreshWorkspaceExtras(inferredProjectId);
    } else if (inferredProjectId) {
      await mutateProjectOps([{ op: "upsert", path: light.path, class_name: light.kind, properties: props }]);
    }
  };

  const handleDeleteLight = async (path: string) => {
    if (onDeletePart) {
      await onDeletePart(path);
      if (inferredProjectId) void refreshWorkspaceExtras(inferredProjectId);
    } else if (inferredProjectId) {
      await mutateProjectOps([{ op: "delete", path }]);
    }
  };

  const handleSetLightingPreset = async (preset: "day" | "golden" | "night") => {
    const nextProps =
      preset === "night"
        ? { clock_time: 22, brightness: 1.1, ambient: "#2A3654", fog_color: "#141D30" }
        : preset === "golden"
          ? { clock_time: 18, brightness: 1.7, ambient: "#8C6F63", fog_color: "#D9AD8B" }
          : { clock_time: 14, brightness: 2.0, ambient: "#707A8A", fog_color: "#C8D8EA" };

    if (inferredProjectId) {
      const ok = await mutateProjectOps([{ op: "set_lighting", path: "Lighting", properties: nextProps }]);
      if (ok) toast.success(`Applied ${preset} sky lighting`);
    } else {
      setWsLighting((prev) => ({ ...(prev || {}), ...nextProps }));
      toast.success(`Applied ${preset} sky lighting`);
    }
  };

  const handlePlaytestClick = async () => {
    if (!onRunPlaytest || playtestBusy) return;
    setPlaytestBusy(true);
    try {
      await onRunPlaytest();
    } finally {
      setPlaytestBusy(false);
    }
  };

  const handleCreatePreset = async (preset: PartPreset) => {
    if (!onCreatePart || partActionBusy) return;
    setPartActionBusy(true);
    try {
      await onCreatePart(preset);
      setAddPartOpen(false);
    } finally {
      setPartActionBusy(false);
    }
  };

  const handleDuplicateSelected = async () => {
    if (!onDuplicatePart || !selectedPart || partActionBusy) return;
    setPartActionBusy(true);
    try {
      await onDuplicatePart(selectedPart.path);
    } finally {
      setPartActionBusy(false);
    }
  };

  const handleDeleteSelected = async () => {
    if (!onDeletePart || !selectedPart || partActionBusy) return;
    setPartActionBusy(true);
    try {
      await onDeletePart(selectedPart.path);
    } finally {
      setPartActionBusy(false);
    }
  };

  const handleRedoClick = async () => {
    if (!onTriggerRedo || !canRedo || partActionBusy) return;
    setPartActionBusy(true);
    try {
      await onTriggerRedo();
    } finally {
      setPartActionBusy(false);
    }
  };

  const filteredActions = useMemo(() => {
    const list = actions.length > 0 ? actions : replaySteps;
    if (consoleFilter === "all") return [...list].reverse().slice(0, 60);
    if (consoleFilter === "parts") {
      return [...list]
        .filter((a) => ["part_created", "part_updated", "decal_bound"].includes(a.kind))
        .reverse()
        .slice(0, 60);
    }
    if (consoleFilter === "scripts") {
      return [...list]
        .filter((a) => ["script_written", "luau_executed"].includes(a.kind))
        .reverse()
        .slice(0, 60);
    }
    return [...list]
      .filter((a) => ["playtest_started", "playtest_log", "playtest_passed", "playtest_failed", "studio_frame"].includes(a.kind))
      .reverse()
      .slice(0, 60);
  }, [actions, consoleFilter, replaySteps]);

  const modeBadgeLabel =
    studioMode === "live_mcp"
      ? "Live Studio MCP"
      : studioMode === "plugin_live"
        ? "Studio Plugin Live"
        : studioMode === "browser_place"
          ? "Browser Studio 3D"
          : "Local Studio 3D";

  return (
    <div data-studio-viewport-3d className="flex h-full min-h-0 min-w-0 flex-col gap-2 overflow-hidden">
      {/* Studio Ribbon Toolbar */}
      <div className="flex flex-col gap-2 rounded-card border border-hairline bg-board/55 px-2.5 py-2">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-1">
            {(
              [
                { id: "select", label: "Select", Icon: MousePointer2 },
                { id: "move", label: "Move", Icon: Move },
                { id: "scale", label: "Scale", Icon: Maximize2 },
                { id: "rotate", label: "Rotate", Icon: RotateCw },
              ] as const
            ).map(({ id, label, Icon }) => (
              <Button
                key={id}
                variant={tool === id ? "default" : "outline"}
                size="xs"
                aria-pressed={tool === id}
                aria-label={`${label} tool`}
                onClick={() => setTool(id)}
                className={tool === id ? "max-md:min-h-11 max-md:min-w-11" : "bg-surface max-md:min-h-11 max-md:min-w-11"}
              >
                <Icon className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>{label}</span>
              </Button>
            ))}

            {onCreatePart && (
              <Button
                variant={addPartOpen ? "default" : "outline"}
                size="xs"
                aria-pressed={addPartOpen}
                aria-label="Add 3D part"
                disabled={partActionBusy}
                onClick={() => {
                  setAddPartOpen((v) => !v);
                  setToolbarPanel(null);
                }}
                className={addPartOpen ? "max-md:min-h-11 max-md:min-w-11" : "bg-surface max-md:min-h-11 max-md:min-w-11"}
              >
                <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Add Part</span>
              </Button>
            )}
            <Button
              data-toggle-terrain-panel
              variant={toolbarPanel === "terrain" ? "default" : "outline"}
              size="xs"
              aria-pressed={toolbarPanel === "terrain"}
              aria-label="Toggle Terrain controls"
              onClick={() => {
                setToolbarPanel((cur) => (cur === "terrain" ? null : "terrain"));
                setAddPartOpen(false);
              }}
              className={toolbarPanel === "terrain" ? "max-md:min-h-11 max-md:min-w-11" : "bg-surface max-md:min-h-11 max-md:min-w-11"}
            >
              <Mountain className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>Terrain</span>
            </Button>
            <Button
              data-toggle-lights-panel
              variant={toolbarPanel === "lights" ? "default" : "outline"}
              size="xs"
              aria-pressed={toolbarPanel === "lights"}
              aria-label="Toggle Lighting controls"
              onClick={() => {
                setToolbarPanel((cur) => (cur === "lights" ? null : "lights"));
                setAddPartOpen(false);
              }}
              className={toolbarPanel === "lights" ? "max-md:min-h-11 max-md:min-w-11" : "bg-surface max-md:min-h-11 max-md:min-w-11"}
            >
              <Lightbulb className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>Lights</span>
            </Button>
            {onDuplicatePart && (
              <Button
                variant="outline"
                size="xs"
                disabled={!selectedPart || partActionBusy}
                aria-label="Duplicate selected part"
                onClick={() => void handleDuplicateSelected()}
                className="bg-surface max-md:min-h-11 max-md:min-w-11"
              >
                <CopyPlus className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Duplicate</span>
              </Button>
            )}
            {onDeletePart && (
              <Button
                variant="outline"
                size="xs"
                disabled={!selectedPart || partActionBusy}
                aria-label="Delete selected part"
                onClick={() => void handleDeleteSelected()}
                className="bg-surface max-md:min-h-11 max-md:min-w-11"
              >
                <Trash2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Delete</span>
              </Button>
            )}
            {onTriggerRedo && (
              <Button
                variant="outline"
                size="xs"
                disabled={!canRedo || partActionBusy}
                aria-label="Redo last Studio action"
                onClick={() => void handleRedoClick()}
                className="bg-surface max-md:min-h-11 max-md:min-w-11"
              >
                <Redo2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Redo</span>
              </Button>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            <Button
              variant="outline"
              size="xs"
              disabled={playtestBusy}
              onClick={() => void handlePlaytestClick()}
              aria-label="Run Studio playtest"
              className="bg-surface max-md:min-h-11"
            >
              <Play className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>{playtestBusy ? "Testing..." : "Playtest"}</span>
            </Button>
            <Button
              variant={replaying ? "secondary" : "outline"}
              size="xs"
              disabled={replaySteps.length === 0}
              aria-pressed={replaying}
              onClick={() => setReplaying((v) => !v)}
              aria-label="Replay agent build in 3D"
              className={replaying ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
            >
              {replaying ? (
                <Square className="size-3.5" strokeWidth={1.5} aria-hidden />
              ) : (
                <Video className="size-3.5" strokeWidth={1.5} aria-hidden />
              )}
              <span>
                {replaying && replayIndex !== null
                  ? `Stop (${replayIndex + 1}/${replaySteps.length})`
                  : "Replay Build"}
              </span>
            </Button>
            <Button
              variant={followAgent ? "default" : "outline"}
              size="xs"
              aria-pressed={followAgent}
              onClick={() => setFollowAgent((v) => !v)}
              aria-label="Follow agent camera"
              className={followAgent ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
            >
              <Radio className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>Follow Agent</span>
            </Button>
            <RendererStatusBadge mode={rendererMode} contextLost={glContextLost} />
            <Pill
              className={`px-2 py-0.5 text-xs ${
                studioMode === "live_mcp" || studioMode === "plugin_live"
                  ? "bg-pill-active-bg text-pill-active-fg"
                  : "bg-tag-sky-bg text-tag-sky-fg"
              }`}
            >
              <PillIndicator
                variant={studioMode === "live_mcp" || studioMode === "plugin_live" ? "success" : "info"}
                pulse={Boolean(building)}
              />
              <span>{modeBadgeLabel}</span>
            </Pill>
          </div>
        </div>

        {/* Inline + Add Part Preset Picker Strip */}
        {addPartOpen && onCreatePart && (
          <div
            data-add-part-picker
            className="flex flex-wrap items-center gap-1.5 border-t border-hairline pt-2"
          >
            <span className="text-xs font-medium text-ink-2">Insert into Workspace:</span>
            {PART_PRESETS.map((p) => (
              <Button
                key={p.id}
                variant="outline"
                size="xs"
                disabled={partActionBusy}
                aria-label={`Insert ${p.label}`}
                onClick={() => void handleCreatePreset(p.id)}
                className="gap-1.5 bg-surface max-md:min-h-11 max-md:min-w-11"
              >
                <span className={`size-2.5 flex-none rounded-full border border-hairline ${p.dotClass}`} aria-hidden />
                <span>{p.label}</span>
              </Button>
            ))}
          </div>
        )}

        {/* Inline Terrain Generator / Paint Box / Dynamic Lights Strip */}
        <TerrainLightingToolbar
          panel={toolbarPanel}
          terrain={effectiveTerrain ?? null}
          lights={lights}
          selectedPath={selectedPath}
          clockTime={effectiveLighting?.clock_time}
          busy={sceneMutateBusy || partActionBusy}
          onGenerateTerrain={handleGenerateTerrain}
          onPaintBox={handlePaintBox}
          onClearTerrain={handleClearTerrain}
          onInsertLight={handleInsertLight}
          onSelectLight={(p) => onSelectPart?.(p)}
          onToggleLight={handleToggleLight}
          onDeleteLight={handleDeleteLight}
          onSetLightingPreset={handleSetLightingPreset}
        />
      </div>

      {/* Main 3D Viewport Canvas */}
      <div className="flex min-h-0 flex-1 flex-col gap-2">
        <div
          tabIndex={0}
          aria-label="Interactive 3D Roblox Studio Viewport: drag to orbit, Shift+drag to pan, wheel to zoom, click any 3D part to select"
          onKeyDown={onCanvasKeyDown}
          onKeyUp={(e) => keysRef.current.delete(e.key)}
          onBlur={() => keysRef.current.clear()}
          className="relative flex min-h-[260px] flex-1 items-center justify-center overflow-hidden rounded-sm border border-hairline bg-board/60 outline-none"
        >
          <canvas
            ref={canvasRef}
            data-testid="viewport-canvas"
            data-renderer={rendererMode}
            width={VIEWPORT_CANVAS_W}
            height={VIEWPORT_CANVAS_H}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerCancel}
            onContextMenu={(e) => e.preventDefault()}
            className="block h-full w-full touch-none cursor-grab active:cursor-grabbing"
          />
          {rendererMode === "webgl2" && (
            <canvas
              ref={overlayRef}
              width={VIEWPORT_CANVAS_W}
              height={VIEWPORT_CANVAS_H}
              aria-hidden
              className="pointer-events-none absolute inset-0 block h-full w-full"
            />
          )}
        </div>

        {/* Camera & Precision Gizmo Bar */}
        <div className="flex flex-none flex-wrap items-center justify-between gap-2 text-xs text-ink-2">
          <div className="flex flex-wrap items-center gap-1">
            <Button variant="outline" size="xs" onClick={() => setViewPreset("iso")} aria-label="Isometric camera view" className="bg-surface max-md:min-h-11 max-md:min-w-11">
              Iso
            </Button>
            <Button variant="outline" size="xs" onClick={() => setViewPreset("top")} aria-label="Top camera view" className="bg-surface max-md:min-h-11 max-md:min-w-11">
              Top
            </Button>
            <Button variant="outline" size="xs" onClick={() => setViewPreset("front")} aria-label="Front camera view" className="bg-surface max-md:min-h-11 max-md:min-w-11">
              Front
            </Button>
            <Button variant="outline" size="icon-xs" onClick={() => zoomBy(0.84)} aria-label="Zoom in 3D camera" className="bg-surface max-md:size-11">
              <ZoomIn className="size-3.5" strokeWidth={1.5} aria-hidden />
            </Button>
            <Button variant="outline" size="icon-xs" onClick={() => zoomBy(1.18)} aria-label="Zoom out 3D camera" className="bg-surface max-md:size-11">
              <ZoomOut className="size-3.5" strokeWidth={1.5} aria-hidden />
            </Button>
            <Button
              variant="outline"
              size="xs"
              onClick={() => (selectedPart ? focusOnCoords(selectedPart.pos, selectedPart.size) : resetCamera())}
              aria-label="Focus selected part"
              className="bg-surface max-md:min-h-11 max-md:min-w-11"
            >
              <Focus className="size-3.5" strokeWidth={1.5} aria-hidden />
              Focus
            </Button>
            <Button variant="outline" size="xs" onClick={resetCamera} aria-label="Reset 3D camera" className="bg-surface max-md:min-h-11 max-md:min-w-11">
              <RotateCcw className="size-3.5" strokeWidth={1.5} aria-hidden />
              Reset Cam
            </Button>
          </div>

          {selectedPart ? (
            <div className="flex min-w-0 max-w-full flex-wrap items-center gap-1">
              <span className="inline-flex min-w-0 max-w-[14rem] items-center gap-1 truncate text-xs font-medium text-ink">
                <Crosshair className="size-3 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
                <span className="truncate">{selectedPart.name}</span>
              </span>
              {tool === "rotate" ? (
                <>
                  <Button variant="outline" size="xs" onClick={() => nudgeSelected("y", -1)} aria-label="Rotate part -15 degrees" className="bg-surface max-md:min-h-11 max-md:min-w-11">
                    -15°
                  </Button>
                  <Button variant="outline" size="xs" onClick={() => nudgeSelected("y", 1)} aria-label="Rotate part +15 degrees" className="bg-surface max-md:min-h-11 max-md:min-w-11">
                    +15°
                  </Button>
                </>
              ) : (
                <>
                  <Button variant="outline" size="xs" onClick={() => nudgeSelected("x", -2)} aria-label={`${tool === "scale" ? "Shrink" : "Move"} X minus 2`} className="bg-surface max-md:min-h-11 max-md:min-w-11">
                    -X
                  </Button>
                  <Button variant="outline" size="xs" onClick={() => nudgeSelected("x", 2)} aria-label={`${tool === "scale" ? "Grow" : "Move"} X plus 2`} className="bg-surface max-md:min-h-11 max-md:min-w-11">
                    +X
                  </Button>
                  <Button variant="outline" size="xs" onClick={() => nudgeSelected("y", 2)} aria-label={`${tool === "scale" ? "Grow" : "Move"} Y plus 2`} className="bg-surface max-md:min-h-11 max-md:min-w-11">
                    +Y
                  </Button>
                  <Button variant="outline" size="xs" onClick={() => nudgeSelected("z", 2)} aria-label={`${tool === "scale" ? "Grow" : "Move"} Z plus 2`} className="bg-surface max-md:min-h-11 max-md:min-w-11">
                    +Z
                  </Button>
                </>
              )}
            </div>
          ) : (
            <span className="min-w-0 max-w-full truncate text-xs text-ink-2">
              Drag to orbit, Shift+drag to pan, wheel to zoom, click any 3D part to edit
            </span>
          )}
        </div>
      </div>

      {/* Collapsible Docked Studio Output & Live Agent Action Console */}
      {!hideConsole && (
        <div className="flex-none rounded-card border border-hairline bg-board/40">
          <Expandable
            expandDirection="vertical"
            expandBehavior="push"
            expanded={consoleOpen}
            onToggle={() => setConsoleOpen((v) => !v)}
          >
            {({ isExpanded }) => (
              <>
                <div className="flex flex-wrap items-center justify-between gap-2 px-3 py-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <Terminal className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                    <span className="text-xs font-medium text-ink">Studio Output &amp; Live Agent Stream</span>
                    <Pill className="bg-surface px-2 py-0.5 text-xs text-ink-2">
                      <span className="font-mono tabular-nums">{filteredActions.length}</span>
                      <span>events</span>
                    </Pill>
                  </div>

                  <div className="flex flex-wrap items-center gap-1">
                    {(["all", "parts", "scripts", "console"] as const).map((f) => (
                      <Button
                        key={f}
                        variant={consoleFilter === f ? "default" : "ghost"}
                        size="xs"
                        aria-pressed={consoleFilter === f}
                        onClick={() => setConsoleFilter(f)}
                        className="capitalize max-md:min-h-11 max-md:min-w-11"
                      >
                        {f}
                      </Button>
                    ))}
                    <ExpandableTrigger
                      aria-label="Toggle Studio output console"
                      className="inline-flex min-h-11 min-w-11 items-center justify-center rounded-sm text-ink-2 outline-none hover:bg-surface hover:text-ink md:min-h-7 md:min-w-7"
                    >
                      <ChevronDown
                        className={`size-3.5 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`}
                        strokeWidth={1.5}
                        aria-hidden
                      />
                    </ExpandableTrigger>
                  </div>
                </div>

                <ExpandableContent preset="fade">
                  <div
                    data-studio-console
                    className="max-h-40 overflow-x-hidden overflow-y-auto border-t border-hairline bg-surface p-2"
                  >
                    {filteredActions.length === 0 ? (
                      <p className="px-2 py-2 text-xs text-ink-2">
                        No Studio events in this filter yet. Build a blueprint or click Playtest to stream live actions.
                      </p>
                    ) : (
                      <ul className="flex min-w-0 flex-col gap-1" aria-label="Live agent Studio actions">
                        {filteredActions.map((ev) => {
                          const badge = actionPrefix(ev.kind);
                          const clickable = Boolean(ev.path && (ev.path.startsWith("Workspace.") || ev.path.includes(".")));
                          return (
                            <li key={`${ev.seq}-${ev.kind}-${ev.path}`} className="min-w-0">
                              <Button
                                variant="ghost"
                                size="xs"
                                onClick={() => {
                                  focusAgentOnAction(ev);
                                  if (clickable) onSelectPart?.(ev.path);
                                }}
                                className="h-auto w-full min-w-0 justify-between gap-2 overflow-hidden rounded-sm px-2 py-1 text-left font-normal hover:bg-board/60 max-md:min-h-11"
                              >
                                <span className="flex min-w-0 flex-1 items-center gap-2">
                                  <span className="shrink-0 font-mono text-xs tabular-nums text-ink-3">
                                    {formatActionTime(ev.ts_ms)}
                                  </span>
                                  <Badge className={`shrink-0 border-transparent px-1.5 py-0 text-xs font-medium ${badge.cls}`}>
                                    {badge.tag}
                                  </Badge>
                                  <span className="min-w-0 truncate text-xs text-ink">
                                    {ev.kind === "playtest_log" && ev.console_line
                                      ? ev.console_line
                                      : ev.summary}
                                  </span>
                                </span>
                                {ev.path && (
                                  <span className="hidden max-w-[14rem] shrink-0 truncate text-xs text-ink-3 sm:inline">
                                    {ev.path}
                                  </span>
                                )}
                              </Button>
                            </li>
                          );
                        })}
                      </ul>
                    )}
                  </div>
                </ExpandableContent>
              </>
            )}
          </Expandable>
        </div>
      )}
    </div>
  );
}
