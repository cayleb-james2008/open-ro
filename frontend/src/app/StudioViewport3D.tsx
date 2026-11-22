import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type WheelEvent as ReactWheelEvent,
} from "react";
import { useReducedMotion } from "motion/react";
import {
  ChevronDown,
  CopyPlus,
  Crosshair,
  Focus,
  Maximize2,
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
  StudioActionEvent,
  StudioMode,
  ProjectAssetRecord,
} from "./data";

export type StudioTool = "select" | "move" | "scale" | "rotate";
type ConsoleFilter = "all" | "parts" | "scripts" | "console";
type Vec3 = [number, number, number];
type Vec2 = [number, number];

const PART_PRESETS: Array<{ id: PartPreset; label: string; swatch: string }> = [
  { id: "block", label: "Block", swatch: "#8B939E" },
  { id: "sphere", label: "Sphere", swatch: "#3B82F6" },
  { id: "cylinder", label: "Cylinder", swatch: "#D49B26" },
  { id: "spawn", label: "SpawnLocation", swatch: "#2D68DF" },
  { id: "jump_pad", label: "JumpPad", swatch: "#24946E" },
  { id: "kill_brick", label: "KillBrick", swatch: "#DF522D" },
  { id: "decal_board", label: "DecalBoard", swatch: "#7B4ED4" },
];

type Part3D = {
  path: string;
  name: string;
  className: string;
  shape: "Block" | "Ball" | "Cylinder";
  pos: Vec3;
  size: Vec3;
  rotYDeg: number;
  color: string;
  material: string;
  transparency: number;
  decalUrl?: string;
  cardTitle?: string;
  rawProps: Record<string, unknown>;
};

type CameraState = {
  tx: number;
  ty: number;
  tz: number;
  yaw: number;
  pitch: number;
  dist: number;
};

type GizmoHandleHit = {
  axis: "x" | "y" | "z";
  x: number;
  y: number;
  dir2d: Vec2;
};

type ProjectedPoly = {
  path: string;
  points: Vec2[];
  depth: number;
};

const CANVAS_W = 920;
const CANVAS_H = 580;

const DEFAULT_CAMERA: CameraState = {
  tx: 0,
  ty: 3,
  tz: 14,
  yaw: 0.68,
  pitch: 0.44,
  dist: 56,
};

function parseVec3(v: unknown, fallback: Vec3): Vec3 {
  if (Array.isArray(v) && v.length >= 3 && v.every((n) => typeof n === "number" && Number.isFinite(n))) {
    return [v[0], v[1], v[2]];
  }
  return fallback;
}

function parseColor(name: string, className: string, props: Record<string, unknown>): string {
  if (typeof props.Color === "string" && /^#[0-9a-fA-F]{6}$/.test(props.Color.trim())) {
    return props.Color.trim();
  }
  const low = name.toLowerCase();
  if (className === "SpawnLocation" || low.includes("spawn")) return "#2D68DF";
  if (low.includes("lava") || low.includes("kill") || low.includes("shrine") || low.includes("boss")) return "#DF522D";
  if (low.includes("roll") || low.includes("pedestal") || low.includes("coin") || low.includes("gold")) return "#D49B26";
  if (low.includes("tower") || low.includes("portal") || low.includes("gate")) return "#7B4ED4";
  if (low.includes("alchemist") || low.includes("jump") || low.includes("checkpoint")) return "#24946E";
  if (low.includes("ice") || low.includes("frost")) return "#65B7E8";
  if (low.includes("moving") || low.includes("platform")) return "#3F78B5";
  if (low.includes("billboard") || low.includes("gallery")) return "#475569";
  return "#8B939E";
}

function hexToRgb(hex: string): [number, number, number] {
  const c = hex.replace("#", "");
  if (c.length !== 6) return [139, 147, 158];
  const n = parseInt(c, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function shadeHex(hex: string, factor: number, alpha = 1): string {
  const [r, g, b] = hexToRgb(hex);
  const clamp = (x: number) => Math.max(0, Math.min(255, Math.round(x)));
  const rr = clamp(r * factor);
  const gg = clamp(g * factor);
  const bb = clamp(b * factor);
  return alpha < 0.99 ? `rgba(${rr},${gg},${bb},${alpha.toFixed(2)})` : `rgb(${rr},${gg},${bb})`;
}

function extractParts(place: Place, assets: ProjectAssetRecord[]): Part3D[] {
  const out: Part3D[] = [];
  const entries = Object.entries(place.instances || {});
  let fallbackIdx = 0;

  for (const [path, inst] of entries) {
    if (!path.startsWith("Workspace.")) continue;
    const cls = inst.class_name || "Part";
    if (cls === "Decal" || cls === "SurfaceGui" || cls === "Script" || cls === "Folder") continue;
    const name = path.split(".").slice(1).join(".") || path;
    const props = inst.properties || {};
    const size = parseVec3(props.Size, cls === "SpawnLocation" ? [8, 1, 8] : [8, 2, 8]);
    const hasPos = Array.isArray(props.Position) && props.Position.length >= 3;
    const pos: Vec3 = hasPos
      ? parseVec3(props.Position, [0, size[1] / 2, 0])
      : [(fallbackIdx % 4 - 1.5) * 14, Math.max(1, size[1] / 2), Math.floor(fallbackIdx / 4) * 14];
    if (!hasPos) fallbackIdx++;

    const rotArr = parseVec3(props.Rotation, [0, 0, 0]);
    const rotYDeg = rotArr[1];
    const transparency =
      typeof props.Transparency === "number" && Number.isFinite(props.Transparency)
        ? Math.max(0, Math.min(0.9, props.Transparency))
        : 0;
    const material = typeof props.Material === "string" ? props.Material : "SmoothPlastic";
    const color = parseColor(name, cls, props);

    // Resolve generic Decal / SurfaceGui texture asset for display parts
    let decalUrl: string | undefined;
    const cardTitle = typeof props.CardTitle === "string" ? props.CardTitle : typeof props.DisplayTitle === "string" ? props.DisplayTitle : undefined;
    const rawTex =
      typeof props.DecalTexture === "string"
        ? props.DecalTexture
        : typeof props.Texture === "string"
          ? props.Texture
          : undefined;
    if (rawTex || cardTitle || /billboard|decalboard|showcase/i.test(name)) {
      const matched = assets.find(
        (a) =>
          (cardTitle && a.title.toLowerCase() === cardTitle.toLowerCase()) ||
          (rawTex && a.file && rawTex.toLowerCase().includes(a.file.toLowerCase())) ||
          (rawTex && rawTex.toLowerCase().includes(a.id.toLowerCase())),
      );
      if (matched) {
        decalUrl = matched.image_url;
      } else if (rawTex && (rawTex.startsWith("data:image/") || rawTex.startsWith("/api/assets/"))) {
        decalUrl = rawTex;
      }
    }

    const rawShape = typeof props.Shape === "string" ? props.Shape : typeof props.shape === "string" ? props.shape : "";
    const shape: Part3D["shape"] =
      /ball|sphere/i.test(rawShape) || /sphere|ball/i.test(name)
        ? "Ball"
        : /cylinder/i.test(rawShape) || /cylinder|pillar/i.test(name)
          ? "Cylinder"
          : "Block";

    out.push({
      path,
      name,
      className: cls,
      shape,
      pos,
      size,
      rotYDeg,
      color,
      material,
      transparency,
      decalUrl,
      cardTitle,
      rawProps: props,
    });
  }

  return out;
}

function buildCameraBasis(cam: CameraState) {
  const cp = Math.cos(cam.pitch);
  const sp = Math.sin(cam.pitch);
  const cy = Math.cos(cam.yaw);
  const sy = Math.sin(cam.yaw);
  const ex = cam.tx + cam.dist * cp * sy;
  const ey = cam.ty + cam.dist * sp;
  const ez = cam.tz + cam.dist * cp * cy;

  // Forward unit vector (eye -> target)
  let fx = cam.tx - ex;
  let fy = cam.ty - ey;
  let fz = cam.tz - ez;
  const fl = Math.hypot(fx, fy, fz) || 1;
  fx /= fl;
  fy /= fl;
  fz /= fl;

  // Right = normalize(forward x up(0,1,0)) = (-fz, 0, fx)
  let rx = -fz;
  const ry = 0;
  let rz = fx;
  const rl = Math.hypot(rx, rz) || 1;
  rx /= rl;
  rz /= rl;

  // Up = right x forward
  const ux = ry * fz - rz * fy;
  const uy = rz * fx - rx * fz;
  const uz = rx * fy - ry * fx;

  const focal = (CANVAS_H * 0.5) / Math.tan((46 * Math.PI) / 360);

  const project = (x: number, y: number, z: number): { sx: number; sy: number; depth: number; visible: boolean } => {
    const dx = x - ex;
    const dy = y - ey;
    const dz = z - ez;
    const cz = dx * fx + dy * fy + dz * fz;
    if (cz <= 0.6) return { sx: 0, sy: 0, depth: cz, visible: false };
    const cx = dx * rx + dy * ry + dz * rz;
    const cyCam = dx * ux + dy * uy + dz * uz;
    const scale = focal / cz;
    return {
      sx: CANVAS_W * 0.5 + cx * scale,
      sy: CANVAS_H * 0.5 - cyCam * scale,
      depth: cz,
      visible: true,
    };
  };

  return { eye: [ex, ey, ez] as Vec3, forward: [fx, fy, fz] as Vec3, right: [rx, ry, rz] as Vec3, up: [ux, uy, uz] as Vec3, project };
}

function pointInPoly(px: number, py: number, poly: Vec2[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i];
    const [xj, yj] = poly[j];
    const intersect = yi > py !== yj > py && px < ((xj - xi) * (py - yi)) / (yj - yi + 1e-9) + xi;
    if (intersect) inside = !inside;
  }
  return inside;
}

// Affine texture map of a triangle from source image (u0,v0..u2,v2) to screen triangle (x0,y0..x2,y2)
function drawTexturedTriangle(
  ctx: CanvasRenderingContext2D,
  img: HTMLImageElement,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  u0: number,
  v0: number,
  u1: number,
  v1: number,
  u2: number,
  v2: number,
) {
  const denom = u0 * (v1 - v2) - v0 * (u1 - u2) + (u1 * v2 - u2 * v1);
  if (Math.abs(denom) < 1e-4) return;
  const a = (x0 * (v1 - v2) - v0 * (x1 - x2) + (x1 * v2 - x2 * v1)) / denom;
  const b = (y0 * (v1 - v2) - v0 * (y1 - y2) + (y1 * v2 - y2 * v1)) / denom;
  const c = (u0 * (x1 - x2) - x0 * (u1 - u2) + (u1 * x2 - u2 * x1)) / denom;
  const d = (u0 * (y1 - y2) - y0 * (u1 - u2) + (u1 * y2 - u2 * y1)) / denom;
  const e = (u0 * (v1 * x2 - v2 * x1) - v0 * (u1 * x2 - u2 * x1) + x0 * (u1 * v2 - u2 * v1)) / denom;
  const f = (u0 * (v1 * y2 - v2 * y1) - v0 * (u1 * y2 - u2 * y1) + y0 * (u1 * v2 - u2 * v1)) / denom;

  ctx.save();
  ctx.beginPath();
  ctx.moveTo(x0, y0);
  ctx.lineTo(x1, y1);
  ctx.lineTo(x2, y2);
  ctx.closePath();
  ctx.clip();
  ctx.transform(a, b, c, d, e, f);
  ctx.drawImage(img, 0, 0);
  ctx.restore();
}

function formatActionTime(tsMs: number): string {
  const d = new Date(tsMs);
  return d.toLocaleTimeString([], { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function actionPrefix(kind: StudioActionEvent["kind"]): { tag: string; cls: string } {
  switch (kind) {
    case "part_created":
      return { tag: "+ Part", cls: "bg-tag-mint-bg text-tag-mint-fg" };
    case "part_updated":
      return { tag: "✎ Edit", cls: "bg-tag-sky-bg text-tag-sky-fg" };
    case "script_written":
      return { tag: "✎ Luau", cls: "bg-tag-research-bg text-tag-research-fg" };
    case "decal_bound":
      return { tag: "◈ Art", cls: "bg-tag-design-bg text-tag-design-fg" };
    case "luau_executed":
      return { tag: "⚡ Run", cls: "bg-tag-dev-bg text-tag-dev-fg" };
    case "playtest_started":
      return { tag: "▶ Test", cls: "bg-tag-ops-bg text-tag-ops-fg" };
    case "playtest_log":
      return { tag: "Log", cls: "bg-board text-ink-2" };
    case "playtest_passed":
      return { tag: "✓ Pass", cls: "bg-pill-active-bg text-pill-active-fg" };
    case "playtest_failed":
      return { tag: "✕ Fail", cls: "bg-pill-pro-bg text-pill-pro-fg" };
    case "studio_frame":
      return { tag: "Frame", cls: "bg-tag-sky-bg text-tag-sky-fg" };
    case "reverted":
      return { tag: "↶ Undo", cls: "bg-pill-warn-bg text-pill-warn-fg" };
    case "imported":
      return { tag: "↑ XML", cls: "bg-tag-ops-bg text-tag-ops-fg" };
    case "restored":
      return { tag: "⟲ Snap", cls: "bg-tag-research-bg text-tag-research-fg" };
    default:
      return { tag: "Focus", cls: "bg-board text-ink-2" };
  }
}

export function StudioViewport3D({
  place,
  theme,
  assets,
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
  const imgCacheRef = useRef<Map<string, HTMLImageElement>>(new Map());
  const [imgTick, setImgTick] = useState(0);

  const [tool, setTool] = useState<StudioTool>("select");
  const [followAgent, setFollowAgent] = useState(true);
  const [consoleOpen, setConsoleOpen] = useState(true);
  const [consoleFilter, setConsoleFilter] = useState<ConsoleFilter>("all");
  const [replaying, setReplaying] = useState(false);
  const [replayIndex, setReplayIndex] = useState<number | null>(null);
  const [playtestBusy, setPlaytestBusy] = useState(false);
  const [addPartOpen, setAddPartOpen] = useState(false);
  const [partActionBusy, setPartActionBusy] = useState(false);
  const [localDraftEdit, setLocalDraftEdit] = useState<{ path: string; pos: Vec3; size: Vec3; rotY: number } | null>(null);

  const camRef = useRef<CameraState>({ ...DEFAULT_CAMERA });
  const targetCamRef = useRef<CameraState>({ ...DEFAULT_CAMERA });
  const hitPolysRef = useRef<ProjectedPoly[]>([]);
  const gizmoHitsRef = useRef<GizmoHandleHit[]>([]);
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

  // Live agent cursor animation state (3D drone + laser placement beam + part pop-in)
  const agentAnimRef = useRef<{
    x: number;
    y: number;
    z: number;
    tx: number;
    ty: number;
    tz: number;
    beam: number; // 0..1 glow
    popPath: string;
    popProgress: number; // 0..1
    callout: string;
    calloutUntil: number;
    time: number;
  }>({
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
    const raw = extractParts(place, assets);
    if (!localDraftEdit) return raw;
    return raw.map((p) =>
      p.path === localDraftEdit.path
        ? { ...p, pos: localDraftEdit.pos, size: localDraftEdit.size, rotYDeg: localDraftEdit.rotY }
        : p,
    );
  }, [assets, localDraftEdit, place]);

  const selectedPart = useMemo(
    () => parts.find((p) => p.path === selectedPath) || null,
    [parts, selectedPath],
  );

  // Build replay timeline: prefer real streamed actions; synthesize deterministic steps from place if cold-loaded
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

  // Preload Nano Banana card images for 3D billboard projection
  useEffect(() => {
    const urls = new Set<string>();
    for (const p of parts) {
      if (p.decalUrl) urls.add(p.decalUrl);
    }
    for (const url of urls) {
      if (imgCacheRef.current.has(url)) continue;
      const img = new Image();
      img.onload = () => setImgTick((t) => t + 1);
      img.src = url;
      imgCacheRef.current.set(url, img);
    }
  }, [parts]);

  // Focus camera helper
  const focusOnCoords = useCallback(
    (pos: Vec3, size?: Vec3, zoomDist?: number) => {
      const nextDist = zoomDist ?? Math.max(24, Math.min(64, (Math.max(size?.[0] || 8, size?.[1] || 4, size?.[2] || 8) * 3.4)));
      targetCamRef.current = {
        ...targetCamRef.current,
        tx: pos[0],
        ty: pos[1] + (size?.[1] || 2) * 0.35,
        tz: pos[2],
        dist: nextDist,
      };
      if (reduce) {
        camRef.current = { ...targetCamRef.current };
      }
    },
    [reduce],
  );

  // Trigger Agent Builder Cursor + Laser Beam on any action
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
            ? "✎"
            : ev.kind === "playtest_passed"
              ? "▶"
              : ev.kind === "decal_bound"
                ? "◈"
                : "•";
      anim.callout = `${shortPrefix} ${ev.path || ev.summary}`;
      anim.calloutUntil = performance.now() + 4200;

      if (followAgent) {
        focusOnCoords(pos, size, 42);
      }
    },
    [followAgent, focusOnCoords, parts, reduce],
  );

  // Track latest streamed action
  const lastSeqRef = useRef<number>(0);
  useEffect(() => {
    if (replaying || actions.length === 0) return;
    const latest = actions[actions.length - 1];
    if (latest.seq !== lastSeqRef.current) {
      lastSeqRef.current = latest.seq;
      focusAgentOnAction(latest);
    }
  }, [actions, focusAgentOnAction, replaying]);

  // Replay Build timer loop
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

  // Parts visible during Replay Build (hides parts not yet placed at current replay step)
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

  // Fit all parts cleanly on reset
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

  // Auto-frame when a fresh place loads for the first time
  const framedCountRef = useRef(0);
  useEffect(() => {
    if (parts.length > 0 && framedCountRef.current === 0) {
      framedCountRef.current = parts.length;
      resetCamera();
    } else if (parts.length === 0) {
      framedCountRef.current = 0;
    }
  }, [parts.length, resetCamera]);

  // Main 3D Canvas Render Function
  const drawScene = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const isDark = theme === "dark";
    const cam = camRef.current;
    const { project, right, up } = buildCameraBasis(cam);

    // 1. Sky gradient (crisp Roblox Studio daylight / dark studio sky)
    const sky = ctx.createLinearGradient(0, 0, 0, CANVAS_H);
    if (isDark) {
      sky.addColorStop(0, "#121722");
      sky.addColorStop(0.58, "#1A2233");
      sky.addColorStop(1, "#161B24");
    } else {
      sky.addColorStop(0, "#CFE3F7");
      sky.addColorStop(0.55, "#E8F0F7");
      sky.addColorStop(1, "#ECEAE2");
    }
    ctx.fillStyle = sky;
    ctx.fillRect(0, 0, CANVAS_W, CANVAS_H);

    // 2. Roblox Studio Baseplate Slab & Stud Grid
    const baseHalf = 72;
    const bCorners = [
      project(-baseHalf, 0, -baseHalf),
      project(baseHalf, 0, -baseHalf),
      project(baseHalf, 0, baseHalf),
      project(-baseHalf, 0, baseHalf),
    ];
    if (bCorners.every((c) => c.visible)) {
      ctx.beginPath();
      ctx.moveTo(bCorners[0].sx, bCorners[0].sy);
      for (let i = 1; i < 4; i++) ctx.lineTo(bCorners[i].sx, bCorners[i].sy);
      ctx.closePath();
      ctx.fillStyle = isDark ? "#232A36" : "#DFDDD4";
      ctx.fill();
      ctx.strokeStyle = isDark ? "rgba(148,163,184,0.22)" : "rgba(60,58,52,0.18)";
      ctx.lineWidth = 1;
      ctx.stroke();
    }

    // Perspective Grid Lines (every 8 studs; bold every 24 studs)
    for (let g = -baseHalf; g <= baseHalf; g += 8) {
      const major = g % 24 === 0;
      ctx.strokeStyle = major
        ? isDark
          ? "rgba(148,163,184,0.24)"
          : "rgba(55,52,46,0.18)"
        : isDark
          ? "rgba(148,163,184,0.10)"
          : "rgba(55,52,46,0.08)";
      ctx.lineWidth = major ? 1.1 : 0.7;

      const a1 = project(g, 0.02, -baseHalf);
      const a2 = project(g, 0.02, baseHalf);
      if (a1.visible && a2.visible) {
        ctx.beginPath();
        ctx.moveTo(a1.sx, a1.sy);
        ctx.lineTo(a2.sx, a2.sy);
        ctx.stroke();
      }
      const b1 = project(-baseHalf, 0.02, g);
      const b2 = project(baseHalf, 0.02, g);
      if (b1.visible && b2.visible) {
        ctx.beginPath();
        ctx.moveTo(b1.sx, b1.sy);
        ctx.lineTo(b2.sx, b2.sy);
        ctx.stroke();
      }
    }

    // Subtle Roblox Stud Dots near origin
    ctx.fillStyle = isDark ? "rgba(148,163,184,0.18)" : "rgba(70,66,58,0.15)";
    for (let gx = -32; gx <= 32; gx += 8) {
      for (let gz = -24; gz <= 40; gz += 8) {
        const st = project(gx, 0.04, gz);
        if (st.visible && st.sx >= 0 && st.sx <= CANVAS_W && st.sy >= 0 && st.sy <= CANVAS_H) {
          const rad = Math.max(0.8, Math.min(2.2, 55 / st.depth));
          ctx.beginPath();
          ctx.arc(st.sx, st.sy, rad, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    }

    // World Origin X (Red) / Z (Blue) axis rails
    const orig = project(0, 0.06, 0);
    const xEnd = project(18, 0.06, 0);
    const zEnd = project(0, 0.06, 18);
    if (orig.visible && xEnd.visible) {
      ctx.strokeStyle = "rgba(220,76,62,0.55)";
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      ctx.moveTo(orig.sx, orig.sy);
      ctx.lineTo(xEnd.sx, xEnd.sy);
      ctx.stroke();
    }
    if (orig.visible && zEnd.visible) {
      ctx.strokeStyle = "rgba(56,122,232,0.55)";
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      ctx.moveTo(orig.sx, orig.sy);
      ctx.lineTo(zEnd.sx, zEnd.sy);
      ctx.stroke();
    }

    // 3. Collect all 3D Part faces & sort via Painter's Algorithm (back-to-front)
    type DrawFace = {
      path: string;
      pts: Vec2[];
      depth: number;
      fill: string;
      stroke: string;
      lineWidth: number;
      isNeon: boolean;
      isTopSpawn?: boolean;
      spawnCorners?: Vec2[];
      decalImg?: HTMLImageElement;
      decalCorners?: Vec2[];
      sphere?: { cx: number; cy: number; r: number; baseHex: string; alpha: number };
    };

    const anim = agentAnimRef.current;
    const faces: DrawFace[] = [];
    const hitPolys: ProjectedPoly[] = [];
    const sunDir: Vec3 = [0.46, 0.81, 0.36];

    // Local cube face indices & outward normals (CW winding in Roblox Y-up)
    const FACE_DEFS: Array<{ id: string; idx: [number, number, number, number]; n: Vec3 }> = [
      { id: "top", idx: [4, 5, 6, 7], n: [0, 1, 0] },
      { id: "bottom", idx: [0, 3, 2, 1], n: [0, -1, 0] },
      { id: "front", idx: [3, 2, 6, 7], n: [0, 0, 1] }, // +Z
      { id: "back", idx: [1, 0, 4, 5], n: [0, 0, -1] }, // -Z
      { id: "right", idx: [2, 1, 5, 6], n: [1, 0, 0] }, // +X
      { id: "left", idx: [0, 3, 7, 4], n: [-1, 0, 0] }, // -X
    ];

    const partTopCenters = new Map<string, { sx: number; sy: number; depth: number; part: Part3D }>();

    for (const part of visibleParts) {
      const popScale = anim.popPath === part.path && anim.popProgress < 0.99 ? 0.25 + 0.75 * anim.popProgress : 1;
      const hx = (part.size[0] * popScale) / 2;
      const hy = (part.size[1] * popScale) / 2;
      const hz = (part.size[2] * popScale) / 2;
      const [px, py, pz] = part.pos;
      const radY = (part.rotYDeg * Math.PI) / 180;
      const cosY = Math.cos(radY);
      const sinY = Math.sin(radY);

      const localCorners: Vec3[] = [
        [-hx, -hy, -hz], // 0
        [hx, -hy, -hz],  // 1
        [hx, -hy, hz],   // 2
        [-hx, -hy, hz],  // 3
        [-hx, hy, -hz],  // 4
        [hx, hy, -hz],   // 5
        [hx, hy, hz],    // 6
        [-hx, hy, hz],   // 7
      ];

      const worldCorners: Vec3[] = localCorners.map(([lx, ly, lz]) => [
        px + lx * cosY + lz * sinY,
        py + ly,
        pz - lx * sinY + lz * cosY,
      ]);

      const projCorners = worldCorners.map(([wx, wy, wz]) => project(wx, wy, wz));
      if (projCorners.some((c) => !c.visible)) continue;

      const topC = project(px, py + hy + 1.2, pz);
      if (topC.visible) {
        partTopCenters.set(part.path, { sx: topC.sx, sy: topC.sy, depth: topC.depth, part });
      }

      // Ground contact shadow on baseplate Y = 0.05
      const shPts = [
        project(worldCorners[0][0], 0.05, worldCorners[0][2]),
        project(worldCorners[1][0], 0.05, worldCorners[1][2]),
        project(worldCorners[2][0], 0.05, worldCorners[2][2]),
        project(worldCorners[3][0], 0.05, worldCorners[3][2]),
      ];
      if (shPts.every((s) => s.visible)) {
        faces.push({
          path: "",
          pts: shPts.map((s) => [s.sx, s.sy]),
          depth: (shPts[0].depth + shPts[1].depth + shPts[2].depth + shPts[3].depth) / 4 + 14,
          fill: isDark ? "rgba(0,0,0,0.34)" : "rgba(25,28,36,0.14)",
          stroke: "transparent",
          lineWidth: 0,
          isNeon: false,
        });
      }

      const isSelected = selectedPath === part.path;
      const isNeon = part.material === "Neon";
      const alpha = 1 - part.transparency;
      const decalImg = part.decalUrl ? imgCacheRef.current.get(part.decalUrl) : undefined;
      const decalReady = Boolean(decalImg && decalImg.complete && decalImg.naturalWidth > 0);

      // Sphere ("Ball") 3D shading + octagonal hit polygon
      if (part.shape === "Ball") {
        const ctr = project(px, py, pz);
        const edge = project(px + right[0] * Math.max(hx, hy, hz), py + right[1] * Math.max(hx, hy, hz), pz + right[2] * Math.max(hx, hy, hz));
        if (ctr.visible && edge.visible) {
          const sr = Math.max(6, Math.hypot(edge.sx - ctr.sx, edge.sy - ctr.sy));
          const ring: Vec2[] = [];
          for (let k = 0; k < 10; k++) {
            const ang = (k / 10) * Math.PI * 2;
            ring.push([ctr.sx + Math.cos(ang) * sr, ctr.sy + Math.sin(ang) * sr]);
          }
          faces.push({
            path: part.path,
            pts: ring,
            depth: ctr.depth,
            fill: shadeHex(part.color, 1, alpha),
            stroke: isSelected ? "#2F80FF" : isDark ? "rgba(255,255,255,0.22)" : "rgba(20,24,32,0.22)",
            lineWidth: isSelected ? 2 : 1,
            isNeon,
            sphere: { cx: ctr.sx, cy: ctr.sy, r: sr, baseHex: part.color, alpha },
          });
          hitPolys.push({ path: part.path, points: ring, depth: ctr.depth });
        }
        continue;
      }

      // Cylinder 10-sided 3D prism
      if (part.shape === "Cylinder") {
        const segs = 10;
        const bot: Array<ReturnType<typeof project>> = [];
        const top: Array<ReturnType<typeof project>> = [];
        for (let k = 0; k < segs; k++) {
          const a = (k / segs) * Math.PI * 2 + radY;
          const ox = Math.cos(a) * hx;
          const oz = Math.sin(a) * hz;
          bot.push(project(px + ox, py - hy, pz + oz));
          top.push(project(px + ox, py + hy, pz + oz));
        }
        if (bot.every((c) => c.visible) && top.every((c) => c.visible)) {
          const topPts: Vec2[] = top.map((c) => [c.sx, c.sy]);
          const topDepth = top.reduce((acc, c) => acc + c.depth, 0) / segs;
          faces.push({
            path: part.path,
            pts: topPts,
            depth: topDepth,
            fill: shadeHex(part.color, isNeon ? 1.15 : 0.98, alpha),
            stroke: isSelected ? "#2F80FF" : isDark ? "rgba(255,255,255,0.18)" : "rgba(20,24,32,0.20)",
            lineWidth: isSelected ? 1.8 : 0.85,
            isNeon,
          });
          hitPolys.push({ path: part.path, points: topPts, depth: topDepth });
          for (let k = 0; k < segs; k++) {
            const nk = (k + 1) % segs;
            const midA = ((k + 0.5) / segs) * Math.PI * 2 + radY;
            const nx = Math.cos(midA);
            const nz = Math.sin(midA);
            const ndotl = Math.max(0, nx * sunDir[0] + nz * sunDir[2]);
            const shade = isNeon ? 0.94 + ndotl * 0.22 : 0.55 + ndotl * 0.45;
            const pts: Vec2[] = [
              [bot[k].sx, bot[k].sy],
              [bot[nk].sx, bot[nk].sy],
              [top[nk].sx, top[nk].sy],
              [top[k].sx, top[k].sy],
            ];
            const dAvg = (bot[k].depth + bot[nk].depth + top[nk].depth + top[k].depth) / 4;
            faces.push({
              path: part.path,
              pts,
              depth: dAvg,
              fill: shadeHex(part.color, shade, alpha),
              stroke: isSelected ? "#2F80FF" : isDark ? "rgba(255,255,255,0.12)" : "rgba(20,24,32,0.14)",
              lineWidth: isSelected ? 1.4 : 0.7,
              isNeon,
            });
            hitPolys.push({ path: part.path, points: pts, depth: dAvg });
          }
          continue;
        }
      }

      for (const fd of FACE_DEFS) {
        const [i0, i1, i2, i3] = fd.idx;
        const p0 = projCorners[i0];
        const p1 = projCorners[i1];
        const p2 = projCorners[i2];
        const p3 = projCorners[i3];

        // Screen-space signed area back-face culling (keep visible faces)
        const cross = (p1.sx - p0.sx) * (p2.sy - p0.sy) - (p1.sy - p0.sy) * (p2.sx - p0.sx);
        if (cross > 0 && part.transparency < 0.05) continue;

        // Rotate normal by rotY
        const nx = fd.n[0] * cosY + fd.n[2] * sinY;
        const ny = fd.n[1];
        const nz = -fd.n[0] * sinY + fd.n[2] * cosY;
        const ndotl = Math.max(0, nx * sunDir[0] + ny * sunDir[1] + nz * sunDir[2]);
        const shade = isNeon ? 0.96 + ndotl * 0.22 : 0.58 + ndotl * 0.46;

        const pts: Vec2[] = [
          [p0.sx, p0.sy],
          [p1.sx, p1.sy],
          [p2.sx, p2.sy],
          [p3.sx, p3.sy],
        ];
        const depth = (p0.depth + p1.depth + p2.depth + p3.depth) / 4;

        // Apply Nano Banana decal on Front (+Z) or Back (-Z) of Card Billboards so it is visible from both sides!
        const showDecal = decalReady && (fd.id === "front" || fd.id === "back");
        // Quad vertex order for upright portrait image: top-left, top-right, bottom-right, bottom-left
        const decalCorners: Vec2[] | undefined = showDecal
          ? fd.id === "front"
            ? [
                [projCorners[7].sx, projCorners[7].sy],
                [projCorners[6].sx, projCorners[6].sy],
                [projCorners[2].sx, projCorners[2].sy],
                [projCorners[3].sx, projCorners[3].sy],
              ]
            : [
                [projCorners[5].sx, projCorners[5].sy],
                [projCorners[4].sx, projCorners[4].sy],
                [projCorners[0].sx, projCorners[0].sy],
                [projCorners[1].sx, projCorners[1].sy],
              ]
          : undefined;

        faces.push({
          path: part.path,
          pts,
          depth,
          fill: shadeHex(part.color, shade, alpha),
          stroke: isSelected
            ? "#2F80FF"
            : isNeon
              ? shadeHex(part.color, 1.35, 0.92)
              : isDark
                ? "rgba(255,255,255,0.16)"
                : "rgba(20,24,32,0.20)",
          lineWidth: isSelected ? 1.8 : isNeon ? 1.4 : 0.85,
          isNeon,
          isTopSpawn: fd.id === "top" && (part.className === "SpawnLocation" || /spawn/i.test(part.name)),
          spawnCorners: fd.id === "top" ? pts : undefined,
          decalImg: showDecal ? decalImg : undefined,
          decalCorners,
        });

        hitPolys.push({ path: part.path, points: pts, depth });
      }
    }

    // Sort back-to-front (largest depth drawn first)
    faces.sort((a, b) => b.depth - a.depth);
    hitPolys.sort((a, b) => a.depth - b.depth); // frontmost first for hit-testing!
    hitPolysRef.current = hitPolys;

    for (const f of faces) {
      if (f.sphere) {
        const { cx, cy, r, baseHex, alpha } = f.sphere;
        const grad = ctx.createRadialGradient(cx - r * 0.32, cy - r * 0.34, Math.max(1, r * 0.12), cx, cy, r);
        grad.addColorStop(0, shadeHex(baseHex, 1.38, alpha));
        grad.addColorStop(0.55, shadeHex(baseHex, 0.96, alpha));
        grad.addColorStop(1, shadeHex(baseHex, 0.52, alpha));
        ctx.beginPath();
        ctx.arc(cx, cy, r, 0, Math.PI * 2);
        ctx.fillStyle = grad;
        ctx.fill();
        if (f.stroke !== "transparent") {
          ctx.strokeStyle = f.stroke;
          ctx.lineWidth = f.lineWidth;
          ctx.stroke();
        }
        continue;
      }

      ctx.beginPath();
      ctx.moveTo(f.pts[0][0], f.pts[0][1]);
      for (let i = 1; i < f.pts.length; i++) ctx.lineTo(f.pts[i][0], f.pts[i][1]);
      ctx.closePath();
      ctx.fillStyle = f.fill;
      ctx.fill();

      // Draw real Nano Banana card portrait perspective-mapped onto 3D Billboard face!
      if (f.decalImg && f.decalCorners) {
        const [tl, tr, br, bl] = f.decalCorners;
        const iw = f.decalImg.naturalWidth;
        const ih = f.decalImg.naturalHeight;
        drawTexturedTriangle(ctx, f.decalImg, tl[0], tl[1], tr[0], tr[1], br[0], br[1], 0, 0, iw, 0, iw, ih);
        drawTexturedTriangle(ctx, f.decalImg, tl[0], tl[1], br[0], br[1], bl[0], bl[1], 0, 0, iw, ih, 0, ih);
      }

      // Draw Roblox SpawnLocation decal emblem on top face
      if (f.isTopSpawn && f.spawnCorners) {
        const [c0, c1, c2, c3] = f.spawnCorners;
        const cx = (c0[0] + c1[0] + c2[0] + c3[0]) / 4;
        const cy = (c0[1] + c1[1] + c2[1] + c3[1]) / 4;
        const rx = Math.hypot(c1[0] - c0[0], c1[1] - c0[1]) * 0.24;
        const ry = Math.hypot(c3[0] - c0[0], c3[1] - c0[1]) * 0.24;
        ctx.save();
        ctx.strokeStyle = "rgba(255,255,255,0.9)";
        ctx.lineWidth = 1.6;
        ctx.beginPath();
        ctx.ellipse(cx, cy, Math.max(4, rx), Math.max(2.5, ry * 0.7), 0, 0, Math.PI * 2);
        ctx.stroke();
        ctx.fillStyle = "rgba(255,255,255,0.9)";
        ctx.beginPath();
        ctx.arc(cx, cy, Math.max(2, Math.min(4, rx * 0.3)), 0, Math.PI * 2);
        ctx.fill();
        ctx.restore();
      }

      if (f.stroke !== "transparent") {
        ctx.strokeStyle = f.stroke;
        ctx.lineWidth = f.lineWidth;
        ctx.stroke();
      }
    }

    // 4. Crisp 3D Floating Part Name Labels (for Spawn, Pedestal, Tower, Shrine, Billboards, or Selected Part)
    ctx.font = '500 10.5px "Geist", system-ui, sans-serif';
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    for (const [pPath, info] of partTopCenters.entries()) {
      const isSel = pPath === selectedPath;
      // Textured decal boards showcase their own image; only show label when selected to keep 0 overlap
      if (!isSel && /billboard|showcase/i.test(info.part.name)) continue;
      if (!isSel && partTopCenters.size > 10 && !/spawn|pedestal|tower|shrine|alchemist|lava/i.test(info.part.name)) {
        continue;
      }
      const lbl = info.part.cardTitle || info.part.name;
      const tw = ctx.measureText(lbl).width + 10;
      const bx = info.sx - tw / 2;
      const by = info.sy - 14;
      ctx.fillStyle = isSel
        ? "#2563EB"
        : isDark
          ? "rgba(20,24,34,0.82)"
          : "rgba(255,255,255,0.86)";
      ctx.strokeStyle = isSel ? "#60A5FA" : isDark ? "rgba(255,255,255,0.18)" : "rgba(30,34,42,0.16)";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.roundRect(bx, by, tw, 17, 5);
      ctx.fill();
      ctx.stroke();
      ctx.fillStyle = isSel ? "#FFFFFF" : isDark ? "#EAECEF" : "#1D2026";
      ctx.fillText(lbl, info.sx, by + 8.5);
    }

    // 5. Glowing Roblox Studio Selection Box + 3D Move / Scale / Rotate Gizmo Handles
    const gizmoHits: GizmoHandleHit[] = [];
    if (selectedPart) {
      const [px, py, pz] = selectedPart.pos;
      const origin = project(px, py, pz);
      if (origin.visible) {
        const handleLen = Math.max(7, Math.min(15, Math.max(...selectedPart.size) * 0.9));
        if (tool === "move" || tool === "scale") {
          const axes: Array<{ axis: "x" | "y" | "z"; end: Vec3; color: string; label: string }> = [
            { axis: "x", end: [px + handleLen, py, pz], color: "#E5484D", label: "X" },
            { axis: "y", end: [px, py + handleLen, pz], color: "#30A46C", label: "Y" },
            { axis: "z", end: [px, py, pz + handleLen], color: "#3B82F6", label: "Z" },
          ];
          for (const ax of axes) {
            const ep = project(ax.end[0], ax.end[1], ax.end[2]);
            if (!ep.visible) continue;
            ctx.strokeStyle = ax.color;
            ctx.lineWidth = 2.6;
            ctx.beginPath();
            ctx.moveTo(origin.sx, origin.sy);
            ctx.lineTo(ep.sx, ep.sy);
            ctx.stroke();

            const dx = ep.sx - origin.sx;
            const dy = ep.sy - origin.sy;
            const len = Math.hypot(dx, dy) || 1;
            const ux2 = dx / len;
            const uy2 = dy / len;

            ctx.fillStyle = ax.color;
            if (tool === "move") {
              ctx.beginPath();
              ctx.moveTo(ep.sx + ux2 * 6, ep.sy + uy2 * 6);
              ctx.lineTo(ep.sx - ux2 * 5 - uy2 * 4.5, ep.sy - uy2 * 5 + ux2 * 4.5);
              ctx.lineTo(ep.sx - ux2 * 5 + uy2 * 4.5, ep.sy - uy2 * 5 - ux2 * 4.5);
              ctx.closePath();
              ctx.fill();
            } else {
              ctx.fillRect(ep.sx - 4.5, ep.sy - 4.5, 9, 9);
            }
            gizmoHits.push({ axis: ax.axis, x: ep.sx, y: ep.sy, dir2d: [ux2, uy2] });
          }
        } else if (tool === "rotate") {
          const ringR = Math.max(6, Math.max(selectedPart.size[0], selectedPart.size[2]) * 0.72);
          ctx.strokeStyle = "#30A46C";
          ctx.lineWidth = 2.2;
          ctx.beginPath();
          let started = false;
          for (let a = 0; a <= 36; a++) {
            const ang = (a / 36) * Math.PI * 2;
            const pt = project(px + Math.cos(ang) * ringR, py, pz + Math.sin(ang) * ringR);
            if (!pt.visible) continue;
            if (!started) {
              ctx.moveTo(pt.sx, pt.sy);
              started = true;
            } else {
              ctx.lineTo(pt.sx, pt.sy);
            }
          }
          ctx.stroke();
          const handlePt = project(px + ringR, py, pz);
          if (handlePt.visible) {
            ctx.fillStyle = "#30A46C";
            ctx.beginPath();
            ctx.arc(handlePt.sx, handlePt.sy, 6, 0, Math.PI * 2);
            ctx.fill();
            gizmoHits.push({ axis: "y", x: handlePt.sx, y: handlePt.sy, dir2d: [1, 0] });
          }
        }
      }
    }
    gizmoHitsRef.current = gizmoHits;

    // 6. Live Agent Builder Cursor + Laser Placement Beam + Floating Callout
    const now = performance.now();
    const showAgent = building || replaying || anim.beam > 0.04 || now < anim.calloutUntil;
    if (showAgent) {
      const hoverY = anim.y + (reduce ? 0 : Math.sin(anim.time * 4.2) * 0.65);
      const drone = project(anim.x, hoverY, anim.z);
      const groundTarget = project(anim.tx, Math.max(0.5, anim.ty - 8.5), anim.tz);

      if (drone.visible && groundTarget.visible) {
        // Laser Placement Beam
        const beamAlpha = Math.max(0.18, anim.beam * 0.72);
        const beamGrad = ctx.createLinearGradient(drone.sx, drone.sy, groundTarget.sx, groundTarget.sy);
        beamGrad.addColorStop(0, `rgba(217, 107, 67, ${beamAlpha.toFixed(2)})`);
        beamGrad.addColorStop(1, `rgba(56, 139, 253, ${(beamAlpha * 0.35).toFixed(2)})`);
        ctx.fillStyle = beamGrad;
        ctx.beginPath();
        ctx.moveTo(drone.sx - 2, drone.sy + 5);
        ctx.lineTo(drone.sx + 2, drone.sy + 5);
        ctx.lineTo(groundTarget.sx + 16, groundTarget.sy);
        ctx.lineTo(groundTarget.sx - 16, groundTarget.sy);
        ctx.closePath();
        ctx.fill();

        // Target spawn pulse ring
        ctx.strokeStyle = `rgba(217, 107, 67, ${beamAlpha.toFixed(2)})`;
        ctx.lineWidth = 1.8;
        ctx.beginPath();
        ctx.ellipse(groundTarget.sx, groundTarget.sy, 16 + (1 - anim.beam) * 14, 7 + (1 - anim.beam) * 6, 0, 0, Math.PI * 2);
        ctx.stroke();

        // Floating 3D Agent Builder Drone Diamond
        ctx.fillStyle = "#D96B43";
        ctx.strokeStyle = "#FFFFFF";
        ctx.lineWidth = 1.6;
        ctx.beginPath();
        ctx.moveTo(drone.sx, drone.sy - 8);
        ctx.lineTo(drone.sx + 7, drone.sy);
        ctx.lineTo(drone.sx, drone.sy + 8);
        ctx.lineTo(drone.sx - 7, drone.sy);
        ctx.closePath();
        ctx.fill();
        ctx.stroke();

        // Floating Action Callout Pill
        const calloutText = anim.callout || (building ? "Agent building in Studio…" : "");
        if (calloutText) {
          ctx.font = '600 11px "Geist Mono", monospace';
          const tw = Math.min(340, ctx.measureText(calloutText).width + 18);
          const cx = Math.max(tw / 2 + 8, Math.min(CANVAS_W - tw / 2 - 8, drone.sx));
          const cy = Math.max(24, drone.sy - 26);
          ctx.fillStyle = isDark ? "#1B2230" : "#181C24";
          ctx.strokeStyle = "#D96B43";
          ctx.lineWidth = 1.2;
          ctx.beginPath();
          ctx.roundRect(cx - tw / 2, cy - 11, tw, 22, 6);
          ctx.fill();
          ctx.stroke();
          ctx.fillStyle = "#FFFFFF";
          ctx.textAlign = "center";
          ctx.textBaseline = "middle";
          ctx.fillText(calloutText.slice(0, 44), cx, cy);
        }
      }
    }

    // 7. Interactive 3D Axis ViewCube (X / Y / Z) in top-right corner
    const vcX = CANVAS_W - 44;
    const vcY = 44;
    const vcR = 24;
    ctx.fillStyle = isDark ? "rgba(22,27,36,0.84)" : "rgba(255,255,255,0.86)";
    ctx.strokeStyle = isDark ? "rgba(255,255,255,0.16)" : "rgba(30,34,42,0.16)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.arc(vcX, vcY, vcR + 6, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();

    const triad: Array<{ lbl: string; v: Vec3; color: string }> = [
      { lbl: "X", v: [1, 0, 0], color: "#E5484D" },
      { lbl: "Y", v: [0, 1, 0], color: "#30A46C" },
      { lbl: "Z", v: [0, 0, 1], color: "#3B82F6" },
    ];
    ctx.font = '600 9.5px "Geist Mono", monospace';
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    for (const t of triad) {
      const sx = t.v[0] * right[0] + t.v[1] * right[1] + t.v[2] * right[2];
      const sy = -(t.v[0] * up[0] + t.v[1] * up[1] + t.v[2] * up[2]);
      const ex = vcX + sx * vcR;
      const ey = vcY + sy * vcR;
      ctx.strokeStyle = t.color;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(vcX, vcY);
      ctx.lineTo(ex, ey);
      ctx.stroke();
      ctx.fillStyle = t.color;
      ctx.beginPath();
      ctx.arc(ex, ey, 6.5, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = "#FFFFFF";
      ctx.fillText(t.lbl, ex, ey + 0.5);
    }

    // 8. Empty baseplate helper hint when no parts yet
    if (visibleParts.length === 0 && !building) {
      ctx.font = '500 12px "Geist", system-ui, sans-serif';
      ctx.fillStyle = isDark ? "rgba(232,236,244,0.75)" : "rgba(30,34,42,0.72)";
      ctx.textAlign = "center";
      ctx.fillText("Roblox Studio 3D Baseplate ready — run a blueprint or ask OpenRO to watch parts build live.", CANVAS_W / 2, CANVAS_H - 28);
    }
  }, [building, replaying, reduce, selectedPart, selectedPath, theme, tool, visibleParts, imgTick]);

  // 60fps animation loop (stops cleanly under prefers-reduced-motion)
  useEffect(() => {
    drawScene();
    if (reduce) return;

    let rafId = 0;
    let lastTs = performance.now();

    const tick = (now: number) => {
      const dt = Math.min(0.05, Math.max(0.001, (now - lastTs) / 1000));
      lastTs = now;
      let dirty = false;

      // Smooth WASD/QE camera fly when keys held
      if (keysRef.current.size > 0) {
        const speed = 28 * dt;
        const { forward, right } = buildCameraBasis(targetCamRef.current);
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

      // Smooth camera interpolation toward target
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

      // Agent Builder Cursor & pop-in animation
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
      if (building || replaying || now < anim.calloutUntil) {
        dirty = true;
      }

      if (dirty) drawScene();
      rafId = requestAnimationFrame(tick);
    };

    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, [building, drawScene, reduce, replaying]);

  // Persist a transform change cleanly via POST /api/place/instance
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

  // Pointer handlers: Orbit, Shift+Drag/Right-Drag Pan, Gizmo Drag, and 3D Click Hit-Test
  const onPointerDown = (e: ReactPointerEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.setPointerCapture(e.pointerId);
    const rect = canvas.getBoundingClientRect();
    const mx = ((e.clientX - rect.left) / rect.width) * CANVAS_W;
    const my = ((e.clientY - rect.top) / rect.height) * CANVAS_H;

    // 1. Check Gizmo handle hit first when Move / Scale / Rotate active
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

    // 2. Orbit or Pan
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
        nextPos[axisIdx] = Math.round((d.origPart.pos[axisIdx] + projDelta * 0.14) * 2) / 2;
      } else if (tool === "scale") {
        nextSize[axisIdx] = Math.max(1, Math.round((d.origPart.size[axisIdx] + projDelta * 0.12) * 2) / 2);
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
      const { right } = buildCameraBasis(d.origCam);
      const scale = d.origCam.dist * 0.0022;
      targetCamRef.current = {
        ...targetCamRef.current,
        tx: d.origCam.tx - right[0] * dx * scale,
        ty: Math.max(0.5, d.origCam.ty + dy * scale),
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
        color: "#8B939E",
        material: "SmoothPlastic",
        transparency: 0,
        rawProps: d.origPart.rawProps,
      };
      const draft = localDraftEdit;
      setLocalDraftEdit(null);
      void commitTransform(partObj, draft.pos, draft.size, draft.rotY);
      return;
    }

    // Pure click -> 3D Part hit-testing (reverse depth order = frontmost polygon wins!)
    if (!d.moved) {
      const canvas = canvasRef.current;
      if (!canvas) return;
      const rect = canvas.getBoundingClientRect();
      const mx = ((e.clientX - rect.left) / rect.width) * CANVAS_W;
      const my = ((e.clientY - rect.top) / rect.height) * CANVAS_H;

      for (const poly of hitPolysRef.current) {
        if (pointInPoly(mx, my, poly.points)) {
          onSelectPart?.(poly.path);
          const p = parts.find((item) => item.path === poly.path);
          if (p) {
            agentAnimRef.current.callout = `Selected ${p.path}`;
            agentAnimRef.current.calloutUntil = performance.now() + 2400;
            agentAnimRef.current.tx = p.pos[0];
            agentAnimRef.current.ty = p.pos[1] + p.size[1] * 0.5 + 8;
            agentAnimRef.current.tz = p.pos[2];
          }
          drawScene();
          return;
        }
      }
    }
  };

  const onWheel = (e: ReactWheelEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    const nextDist = Math.max(14, Math.min(130, targetCamRef.current.dist * (e.deltaY > 0 ? 1.1 : 0.91)));
    targetCamRef.current = { ...targetCamRef.current, dist: nextDist };
    if (reduce) {
      camRef.current = { ...targetCamRef.current };
      drawScene();
    }
  };

  const onCanvasKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
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
    }
  };

  const zoomBy = (factor: number) => {
    targetCamRef.current = {
      ...targetCamRef.current,
      dist: Math.max(14, Math.min(130, targetCamRef.current.dist * factor)),
    };
    if (reduce) {
      camRef.current = { ...targetCamRef.current };
      drawScene();
    }
  };

  const setViewPreset = (preset: "iso" | "top" | "front") => {
    const base = targetCamRef.current;
    if (preset === "iso") targetCamRef.current = { ...base, yaw: 0.68, pitch: 0.44 };
    else if (preset === "top") targetCamRef.current = { ...base, yaw: 0, pitch: 1.38 };
    else targetCamRef.current = { ...base, yaw: 0, pitch: 0.16 };
    if (reduce) {
      camRef.current = { ...targetCamRef.current };
      drawScene();
    }
  };

  const nudgeSelected = (axis: "x" | "y" | "z", delta: number) => {
    if (!selectedPart) return;
    const idx = axis === "x" ? 0 : axis === "y" ? 1 : 2;
    const nextPos: Vec3 = [...selectedPart.pos];
    const nextSize: Vec3 = [...selectedPart.size];
    let nextRotY = selectedPart.rotYDeg;
    if (tool === "scale") nextSize[idx] = Math.max(1, nextSize[idx] + delta);
    else if (tool === "rotate") nextRotY = (nextRotY + delta * 15) % 360;
    else nextPos[idx] = nextPos[idx] + delta;
    void commitTransform(selectedPart, nextPos, nextSize, nextRotY);
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
                onClick={() => setAddPartOpen((v) => !v)}
                className={addPartOpen ? "max-md:min-h-11 max-md:min-w-11" : "bg-surface max-md:min-h-11 max-md:min-w-11"}
              >
                <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Add Part</span>
              </Button>
            )}
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
              <span>{playtestBusy ? "Testing…" : "Playtest"}</span>
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
            <Pill
              className={`px-2 py-0.5 font-mono text-xs ${
                studioMode === "live_mcp" || studioMode === "plugin_live"
                  ? "bg-pill-active-bg text-pill-active-fg"
                  : "bg-tag-sky-bg text-tag-sky-fg"
              }`}
            >
              <PillIndicator
                variant={studioMode === "live_mcp" || studioMode === "plugin_live" ? "success" : "info"}
                pulse={Boolean(building)}
              />
              {modeBadgeLabel}
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
                <span
                  className="size-2.5 flex-none rounded-full border border-hairline"
                  style={{ backgroundColor: p.swatch }}
                  aria-hidden
                />
                <span>{p.label}</span>
              </Button>
            ))}
          </div>
        )}
      </div>

      {/* Main 3D Viewport Canvas */}
      <div className="flex min-h-0 flex-1 flex-col gap-2">
        <div
          tabIndex={0}
          aria-label="Interactive 3D Roblox Studio Viewport — drag to orbit, Shift+drag to pan, wheel to zoom, click any 3D part to select"
          onKeyDown={onCanvasKeyDown}
          onKeyUp={(e) => keysRef.current.delete(e.key)}
          onBlur={() => keysRef.current.clear()}
          className="relative flex min-h-[260px] flex-1 items-center justify-center overflow-hidden rounded-sm border border-hairline bg-board/60 outline-none"
        >
          <canvas
            ref={canvasRef}
            width={CANVAS_W}
            height={CANVAS_H}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onContextMenu={(e) => e.preventDefault()}
            onWheel={onWheel}
            className="block h-full w-full touch-none cursor-grab active:cursor-grabbing"
          />
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
              <span className="inline-flex min-w-0 max-w-[14rem] items-center gap-1 truncate font-mono text-xs text-ink">
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
              Drag to orbit · Shift+drag pan · Wheel zoom · Click any 3D part to edit
            </span>
          )}
        </div>
      </div>

      {/* Collapsible Docked Studio Output & Live Agent Action Console (hidden when Bottom Drawer hosts it) */}
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
                  <Pill className="bg-surface px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2">
                    {filteredActions.length} events
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
                                <span className="hidden max-w-[14rem] shrink-0 truncate font-mono text-xs text-ink-3 sm:inline">
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
