import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { useReducedMotion } from "motion/react";
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Box,
  ChevronDown,
  Coins,
  Crosshair,
  Eye,
  Gamepad2,
  Layers,
  Monitor,
  RotateCcw,
  ShoppingBag,
  Sparkles,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Pill } from "@/components/kibo-ui/pill";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import type {
  InstanceEditPayload,
  PartPreset,
  Place,
  StudioActionEvent,
  StudioEmbedStatus,
  StudioKeyboardInputPayload,
  StudioMode,
  StudioMouseInputPayload,
  StudioToolName,
  StudioToolResult,
} from "./data";
import { CardBattlerStage } from "./CardBattlerStage";
import { APEX_CARDS, type CardAssetRecord } from "./cardCatalog";
import { StudioViewport3D } from "./StudioViewport3D";

export type ViewportTab = "studio-3d" | "embedded-studio" | "playable";

function isCardBattlerPlace(place: Place): boolean {
  const keys = [...Object.keys(place.scripts || {}), ...Object.keys(place.instances || {})].join(" ");
  return /cardgacha|cardbattle|towerprogression|deckbuilder|potionsweather|cardgallery|rollpedestal|cardbillboard|bossshrine|towergate/i.test(
    keys,
  );
}

type CameraMode = "top-down" | "side-view";

type StageObjKind =
  | "spawn"
  | "checkpoint"
  | "lava"
  | "ice"
  | "jump"
  | "moving"
  | "speed"
  | "portal"
  | "coin"
  | "dropper"
  | "npc"
  | "vehicle"
  | "part";

type StageObj = {
  id: string;
  explorerPath: string;
  label: string;
  kind: StageObjKind;
  // Logical world coordinates in a 620x240 top-down arena + elevation (yElev) for side-view
  x: number;
  y: number;
  w: number;
  h: number;
  elev: number; // height above floor for side-view
  color?: string;
  stage?: number;
  portalPair?: string;
  moveAmp?: number;
  moveSpeed?: number;
  moveAxis?: "x" | "y";
};

type ParsedWorld = {
  objects: StageObj[];
  currencyName: string;
  startAmount: number;
  shopItem: string | null;
  shopPrice: number;
  hasDayNight: boolean;
  hasWeather: boolean;
  hasCheckpoints: number;
  hasDropper: boolean;
  themeLabel: string;
};

type RenderHitbox = {
  objId: string;
  explorerPath: string;
  label: string;
  rx: number;
  ry: number;
  rw: number;
  rh: number;
};

type DropperOrb = {
  id: number;
  x: number;
  y: number;
  vx: number;
  collected: boolean;
};

type PhysicsState = {
  x: number;
  y: number;
  z: number; // altitude above ground/platform (>= 0)
  vx: number;
  vy: number;
  vz: number;
  grounded: boolean;
  coyote: number;
  hazardFlash: number; // 0..1 red overlay fade
  portalFlash: number; // 0..1 purple warp fade
  portalCooldown: number;
  speedBoost: number;
  onIce: boolean;
  time: number;
  lastMoveDx: number;
  lastMoveDy: number;
  dropperTimer: number;
  orbs: DropperOrb[];
  nextOrbId: number;
};

const CANVAS_W = 640;
const CANVAS_H = 248;
const FLOOR_Y_SIDE = 196;

function parseWorld(place: Place): ParsedWorld {
  const scripts = place.scripts || {};
  const instances = place.instances || {};
  const allSrc = Object.values(scripts).join("\n");
  const scriptKeys = Object.keys(scripts);
  const instKeys = Object.keys(instances);
  const leafNames = [...instKeys, ...scriptKeys].map((p) => p.split(".").pop() || p).join(" ");

  const curMatch = allSrc.match(/local CURRENCY\s*=\s*"([^"]+)"/) || allSrc.match(/stat\.Name\s*=\s*"([^"]+)"/);
  const currencyName = curMatch?.[1] || "Coins";
  const startMatch = allSrc.match(/local START_AMOUNT\s*=\s*(\d+)/) || allSrc.match(/stat\.Value\s*=\s*(\d+)/);
  const startAmount = startMatch ? Number(startMatch[1]) : 40;

  const itemMatch = allSrc.match(/local ITEM_NAME\s*=\s*"([^"]+)"/);
  const priceMatch = allSrc.match(/local PRICE\s*=\s*(\d+)/);
  const shopItem = itemMatch?.[1] || null;
  const shopPrice = priceMatch ? Number(priceMatch[1]) : 100;

  const stagesMatch = allSrc.match(/local STAGES\s*=\s*(\d+)/);
  const hasCheckpoints = stagesMatch ? Math.min(8, Math.max(2, Number(stagesMatch[1]))) : 0;
  const hasDayNight = /ClockTime/.test(allSrc);
  const hasWeather = /WeatherCycle|Atmosphere|Rain/i.test(leafNames + " " + allSrc);
  const hasDropper = /Dropper|Conveyor|TycoonDropper/i.test(leafNames + " " + allSrc);

  const isCardWorld = isCardBattlerPlace(place);
  const themeLabel = isCardWorld
    ? "Untitled Anime Card Game"
    : /\b(ice|frost)/i.test(leafNames)
      ? "Ice Parkour"
      : /\blava/i.test(leafNames)
        ? "Lava Obby"
        : hasDropper || shopItem
          ? "Tycoon"
          : /\b(round|arena|sword)/i.test(leafNames)
            ? "Combat Arena"
            : hasWeather || hasDayNight
              ? "Live World"
              : "Roblox Place";

  const objects: StageObj[] = [];
  let cursorX = 112;

  const pushObj = (o: Omit<StageObj, "x"> & { x?: number }) => {
    const x = o.x ?? Math.min(CANVAS_W - o.w - 20, cursorX);
    objects.push({ ...o, x });
    if (o.x === undefined) cursorX += o.w + 22;
  };

  const instEntries = Object.entries(instances);
  const spawnEntry = instEntries.find(([k, v]) => k.toLowerCase().includes("spawn") || v.class_name === "SpawnLocation");
  const spawnPath = spawnEntry?.[0] || scriptKeys.find((k) => /spawn/i.test(k)) || "Workspace.SpawnPad";

  if (spawnEntry || instEntries.length > 0 || scriptKeys.length > 0) {
    const col = typeof spawnEntry?.[1]?.properties?.Color === "string" ? spawnEntry[1].properties.Color : undefined;
    pushObj({
      id: "spawn",
      explorerPath: spawnPath,
      label: spawnEntry ? spawnEntry[0].split(".").pop() || "SpawnPad" : "SpawnPad",
      kind: "spawn",
      x: 34,
      y: 126,
      w: 58,
      h: 48,
      elev: 0,
      color: col,
    });
  }

  let portalCount = 0;
  for (const [path, rec] of instEntries) {
    const name = path.split(".").pop() || path;
    const low = name.toLowerCase();
    const col = typeof rec.properties?.Color === "string" ? rec.properties.Color : undefined;
    if (low.includes("spawn") || rec.class_name === "SpawnLocation") continue;
    if (low.includes("checkpoint") || low.includes("stage")) continue;

    if (low.includes("lava") || low.includes("kill") || low.includes("hazard") || low.includes("laser") || low.includes("acid")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "lava", y: 124, w: 62, h: 52, elev: 0, color: col });
    } else if (low.includes("moving") || low.includes("platform")) {
      pushObj({
        id: path,
        explorerPath: path,
        label: name,
        kind: "moving",
        y: 120,
        w: 76,
        h: 42,
        elev: 52,
        color: col,
        moveAmp: 34,
        moveSpeed: 1.7,
        moveAxis: "x",
      });
    } else if (low.includes("jump") || low.includes("bounce")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "jump", y: 126, w: 54, h: 44, elev: 0, color: col });
    } else if (low.includes("ice") || low.includes("frost")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "ice", y: 122, w: 72, h: 52, elev: 0, color: col });
    } else if (low.includes("speed") || low.includes("boost")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "speed", y: 126, w: 54, h: 44, elev: 0, color: col });
    } else if (low.includes("portal") || low.includes("teleport")) {
      portalCount++;
      pushObj({
        id: path,
        explorerPath: path,
        label: name,
        kind: "portal",
        y: portalCount % 2 === 1 ? 62 : 164,
        w: 44,
        h: 44,
        elev: portalCount % 2 === 1 ? 0 : 42,
        color: col,
      });
    } else if (low.includes("dropper") || low.includes("conveyor") || low.includes("collector")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "dropper", y: 54, w: 66, h: 38, elev: 28, color: col });
    } else if (low.includes("npc") || low.includes("guard")) {
      pushObj({
        id: path,
        explorerPath: path,
        label: name,
        kind: "npc",
        y: 168,
        w: 36,
        h: 36,
        elev: 0,
        color: col,
        moveAmp: 46,
        moveSpeed: 1.4,
        moveAxis: "x",
      });
    } else if (low.includes("vehicle")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "vehicle", y: 174, w: 62, h: 36, elev: 0, color: col });
    } else if (low.includes("rollpedestal") || low.includes("summon")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "speed", x: 146, y: 122, w: 68, h: 48, elev: 12, color: col });
    } else if (low.includes("towergate")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "portal", x: 256, y: 58, w: 62, h: 48, elev: 24, color: col });
    } else if (low.includes("bossshrine") || low.includes("arena")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "lava", x: 372, y: 120, w: 72, h: 52, elev: 16, color: col });
    } else if (low.includes("alchemist") || low.includes("potion")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "dropper", x: 256, y: 174, w: 68, h: 40, elev: 14, color: col });
    } else if (low.includes("billboard") || low.includes("gallery") || low.includes("showcase")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "part", x: 486, y: 66, w: 76, h: 46, elev: 36, color: col });
    } else if (low.includes("coin") || low.includes("gold") || low.includes("gem")) {
      pushObj({ id: path, explorerPath: path, label: name, kind: "coin", y: 74, w: 24, h: 24, elev: 36, color: col });
    } else {
      pushObj({ id: path, explorerPath: path, label: name, kind: "part", y: 124, w: 58, h: 46, elev: 20, color: col });
    }
  }

  // Pair any portals cleanly
  const portals = objects.filter((o) => o.kind === "portal");
  if (portals.length >= 2) {
    portals[0].portalPair = portals[1].id;
    portals[1].portalPair = portals[0].id;
  }

  // Script-driven virtual objects when not already placed as Workspace instances
  const hasMovingScript = scriptKeys.some((k) => /moving/i.test(k));
  if (hasMovingScript && !objects.some((o) => o.kind === "moving")) {
    const p = scriptKeys.find((k) => /moving/i.test(k)) || "ServerScriptService.MovingPlatform";
    objects.push({
      id: "virtual-moving",
      explorerPath: p,
      label: "MovingPlat",
      kind: "moving",
      x: 270,
      y: 120,
      w: 66,
      h: 40,
      elev: 36,
      moveAmp: 42,
      moveSpeed: 1.7,
      moveAxis: "x",
    });
  }

  const hasDropperScript = scriptKeys.some((k) => /dropper|tycoon/i.test(k));
  if (hasDropperScript && !objects.some((o) => o.kind === "dropper")) {
    const p = scriptKeys.find((k) => /dropper|tycoon/i.test(k)) || "ServerScriptService.TycoonDropper";
    objects.push({
      id: "virtual-dropper",
      explorerPath: p,
      label: "Dropper",
      kind: "dropper",
      x: 340,
      y: 44,
      w: 64,
      h: 34,
      elev: 26,
    });
  }

  // Stage checkpoints spawned by Checkpoints Luau script
  const cpPath = scriptKeys.find((k) => /checkpoint/i.test(k)) || "ServerScriptService.Checkpoints";
  const cpXs = [194, 308, 434, 548];
  for (let s = 1; s <= Math.min(4, hasCheckpoints); s++) {
    const cx = cpXs[s - 1] ?? Math.min(CANVAS_W - 64, 150 + s * 102);
    objects.push({
      id: `cp-${s}`,
      explorerPath: cpPath,
      label: `Stage ${s}`,
      kind: "checkpoint",
      x: cx,
      y: s % 2 === 0 ? 48 : 182,
      w: 54,
      h: 34,
      elev: s % 2 === 0 ? 64 : 28,
      stage: s,
    });
  }

  // Collectible coins if currency, coin spawner, or dropper exists
  const curPath =
    scriptKeys.find((k) => /coin|currency|dropper|quest/i.test(k)) ||
    scriptKeys[0] ||
    spawnPath;
  if (/leaderstats|Coin|Gold|Gem|Dropper|Currency/i.test(allSrc)) {
    for (let c = 0; c < 4; c++) {
      objects.push({
        id: `coin-${c}`,
        explorerPath: curPath,
        label: currencyName,
        kind: "coin",
        x: 158 + c * 116,
        y: c % 2 === 0 ? 64 : 186,
        w: 22,
        h: 22,
        elev: c % 2 === 0 ? 54 : 26,
      });
    }
  }

  return {
    objects,
    currencyName,
    startAmount,
    shopItem,
    shopPrice,
    hasDayNight,
    hasWeather,
    hasCheckpoints,
    hasDropper: hasDropper || hasDropperScript,
    themeLabel,
  };
}

function liveObjPos(obj: StageObj, t: number): { x: number; y: number } {
  if (!obj.moveAmp || !obj.moveSpeed) return { x: obj.x, y: obj.y };
  const offset = Math.sin(t * obj.moveSpeed) * obj.moveAmp;
  return obj.moveAxis === "y"
    ? { x: obj.x, y: Math.max(24, Math.min(CANVAS_H - obj.h - 24, obj.y + offset)) }
    : { x: Math.max(24, Math.min(CANVAS_W - obj.w - 24, obj.x + offset)), y: obj.y };
}

export function StagePreview({
  place,
  theme,
  selectedPath,
  onSelectPart,
  assets,
  onOpenArtStudio,
  studioMode = "local_place",
  studioActions = [],
  hasLiveFrame = false,
  frameSeq = 0,
  building = false,
  canRedo = false,
  embedStatus,
  requestedTab,
  onSaveInstance,
  onCreatePart,
  onDuplicatePart,
  onDeletePart,
  onTriggerRedo,
  onRunPlaytest,
  onCaptureFrame,
  onConnectMcp,
  onDisconnectMcp,
  onLaunchEmbed,
  onStopEmbed,
  onRefreshEmbedStatus,
  onSendMouseInput,
  onSendKeyboardInput,
  onInvokeStudioTool,
}: {
  place: Place;
  theme: "light" | "dark";
  selectedPath?: string | null;
  onSelectPart?: (path: string) => void;
  assets?: CardAssetRecord[];
  onOpenArtStudio?: () => void;
  studioMode?: StudioMode;
  studioActions?: StudioActionEvent[];
  hasLiveFrame?: boolean;
  frameSeq?: number;
  building?: boolean;
  canRedo?: boolean;
  embedStatus: StudioEmbedStatus;
  requestedTab?: ViewportTab | null;
  onSaveInstance?: (payload: InstanceEditPayload) => Promise<void> | void;
  onCreatePart?: (preset: PartPreset) => Promise<void> | void;
  onDuplicatePart?: (path: string) => Promise<void> | void;
  onDeletePart?: (path: string) => Promise<void> | void;
  onTriggerRedo?: () => Promise<void> | void;
  onRunPlaytest?: () => Promise<void> | void;
  onCaptureFrame?: () => Promise<void> | void;
  onConnectMcp?: (command?: string) => Promise<void> | void;
  onDisconnectMcp?: () => Promise<void> | void;
  onLaunchEmbed?: (command?: string) => Promise<void> | void;
  onStopEmbed?: () => Promise<void> | void;
  onRefreshEmbedStatus?: () => Promise<void> | void;
  onSendMouseInput?: (payload: StudioMouseInputPayload) => Promise<void> | void;
  onSendKeyboardInput?: (payload: StudioKeyboardInputPayload) => Promise<void> | void;
  onInvokeStudioTool?: (tool: StudioToolName, args: Record<string, unknown>) => Promise<StudioToolResult>;
}) {
  const reduce = useReducedMotion();
  const isCardWorld = useMemo(() => isCardBattlerPlace(place), [place]);
  const [viewportTab, setViewportTab] = useState<ViewportTab>(() => requestedTab || "studio-3d");
  useEffect(() => {
    if (requestedTab) setViewportTab(requestedTab);
  }, [requestedTab]);
  const world = useMemo(() => parseWorld(place), [place]);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const hitboxesRef = useRef<RenderHitbox[]>([]);
  const keysRef = useRef<Set<string>>(new Set());

  const [cameraMode, setCameraMode] = useState<CameraMode>("top-down");
  const [stage, setStage] = useState(1);
  const [balance, setBalance] = useState(world.startAmount);
  const [respawns, setRespawns] = useState(0);
  const [collected, setCollected] = useState<string[]>([]);
  const [ownedItem, setOwnedItem] = useState<string | null>(null);
  const [statusLine, setStatusLine] = useState("Click canvas + use WASD/Arrows & Space to playtest, or click any part to inspect it.");
  const [clockHour, setClockHour] = useState(11);
  const [stageOpen, setStageOpen] = useState(true);
  const [renderTick, setRenderTick] = useState(0);

  const stageRef = useRef(stage);
  stageRef.current = stage;
  const collectedRef = useRef<Set<string>>(new Set(collected));
  collectedRef.current = new Set(collected);
  const ownedRef = useRef(ownedItem);
  ownedRef.current = ownedItem;

  const physRef = useRef<PhysicsState>({
    x: 64,
    y: 150,
    z: 0,
    vx: 0,
    vy: 0,
    vz: 0,
    grounded: true,
    coyote: 0,
    hazardFlash: 0,
    portalFlash: 0,
    portalCooldown: 0,
    speedBoost: 0,
    onIce: false,
    time: 0,
    lastMoveDx: 0,
    lastMoveDy: 0,
    dropperTimer: 0,
    orbs: [],
    nextOrbId: 1,
  });

  useEffect(() => {
    setBalance(world.startAmount);
  }, [world.startAmount]);

  const respawnPoint = useCallback(() => {
    const cp = world.objects.find((o) => o.kind === "checkpoint" && o.stage === stageRef.current);
    if (cp) return { x: cp.x + cp.w / 2, y: cp.y + cp.h / 2, z: cp.elev };
    const sp = world.objects.find((o) => o.kind === "spawn");
    if (sp) return { x: sp.x + sp.w / 2, y: sp.y + sp.h / 2, z: 0 };
    return { x: 64, y: 150, z: 0 };
  }, [world.objects]);

  const resetPlay = useCallback(() => {
    const sp = world.objects.find((o) => o.kind === "spawn");
    physRef.current = {
      x: sp ? sp.x + sp.w / 2 : 64,
      y: sp ? sp.y + sp.h / 2 : 150,
      z: 0,
      vx: 0,
      vy: 0,
      vz: 0,
      grounded: true,
      coyote: 0,
      hazardFlash: 0,
      portalFlash: 0,
      portalCooldown: 0,
      speedBoost: 0,
      onIce: false,
      time: 0,
      lastMoveDx: 0,
      lastMoveDy: 0,
      dropperTimer: 0,
      orbs: [],
      nextOrbId: 1,
    };
    setStage(1);
    setBalance(world.startAmount);
    setRespawns(0);
    setCollected([]);
    setStatusLine("Playtest reset to SpawnPad.");
    setRenderTick((n) => n + 1);
  }, [world.objects, world.startAmount]);

  // Physics step (used both at 60fps via rAF and for single discrete button taps)
  const stepPhysics = useCallback(
    (dt: number, pulseDx = 0, pulseDy = 0, forceJump = false) => {
      const p = physRef.current;
      const prevTime = p.time;
      p.time += dt;

      if (p.hazardFlash > 0) p.hazardFlash = Math.max(0, p.hazardFlash - dt * 2.6);
      if (p.portalFlash > 0) p.portalFlash = Math.max(0, p.portalFlash - dt * 2.4);
      if (p.portalCooldown > 0) p.portalCooldown = Math.max(0, p.portalCooldown - dt);
      if (p.speedBoost > 0) p.speedBoost = Math.max(0, p.speedBoost - dt);

      const keys = keysRef.current;
      let ix = pulseDx;
      let iy = pulseDy;
      if (keys.has("ArrowLeft") || keys.has("a") || keys.has("A")) ix -= 1;
      if (keys.has("ArrowRight") || keys.has("d") || keys.has("D")) ix += 1;
      if (keys.has("ArrowUp") || keys.has("w") || keys.has("W")) iy -= 1;
      if (keys.has("ArrowDown") || keys.has("s") || keys.has("S")) iy += 1;

      const wantJump = forceJump || keys.has(" ") || (cameraMode === "side-view" && (keys.has("ArrowUp") || keys.has("w") || keys.has("W")));

      const baseSpeed = 185 * (ownedRef.current ? 1.45 : 1) * (p.speedBoost > 0 ? 1.5 : 1);
      const accel = p.onIce ? 4.2 : 18;
      const targetVx = ix * baseSpeed;
      const targetVy = cameraMode === "side-view" ? 0 : iy * baseSpeed;

      p.vx += (targetVx - p.vx) * Math.min(1, accel * dt);
      p.vy += (targetVy - p.vy) * Math.min(1, accel * dt);
      if (Math.abs(p.vx) < 1 && ix === 0) p.vx = 0;
      if (Math.abs(p.vy) < 1 && iy === 0) p.vy = 0;

      if (p.grounded) p.coyote = 0.1;
      else p.coyote = Math.max(0, p.coyote - dt);

      if (wantJump && p.coyote > 0) {
        p.vz = 250;
        p.grounded = false;
        p.coyote = 0;
        setStatusLine("Jumped (Spacebar).");
      }

      // Gravity
      p.vz -= 680 * dt;
      p.x = Math.max(18, Math.min(CANVAS_W - 18, p.x + p.vx * dt));
      p.y = Math.max(22, Math.min(CANVAS_H - 22, p.y + p.vy * dt));
      p.z += p.vz * dt;

      let supportElev = 0;
      p.onIce = false;

      // Spawn tycoon dropper orbs periodically
      if (world.hasDropper) {
        const dropper = world.objects.find((o) => o.kind === "dropper");
        if (dropper) {
          p.dropperTimer += dt;
          if (p.dropperTimer >= 2.2 && p.orbs.length < 5) {
            p.dropperTimer = 0;
            p.orbs.push({
              id: p.nextOrbId++,
              x: dropper.x + dropper.w,
              y: dropper.y + dropper.h / 2,
              vx: 46,
              collected: false,
            });
          }
          for (const orb of p.orbs) {
            orb.x += orb.vx * dt;
            if (!orb.collected && Math.hypot(p.x - orb.x, p.y - orb.y) < 22) {
              orb.collected = true;
              setBalance((b) => b + 15);
              setStatusLine(`Collected Tycoon Dropper block (+15 ${world.currencyName}).`);
            }
          }
          p.orbs = p.orbs.filter((o) => !o.collected && o.x < CANVAS_W - 24);
        }
      }

      // Check collisions & triggers against every placed object
      for (const obj of world.objects) {
        const cur = liveObjPos(obj, p.time);
        const prev = liveObjPos(obj, prevTime);
        const hitXY =
          cameraMode === "side-view"
            ? p.x >= cur.x - 10 && p.x <= cur.x + obj.w + 10
            : p.x >= cur.x - 10 && p.x <= cur.x + obj.w + 10 && p.y >= cur.y - 10 && p.y <= cur.y + obj.h + 10;

        if (!hitXY) continue;

        if (obj.kind === "moving") {
          const platTop = cameraMode === "side-view" ? obj.elev : 0;
          if (p.z >= platTop - 10 && p.vz <= 20) {
            supportElev = Math.max(supportElev, platTop);
            // Carry avatar smoothly with moving platform delta!
            p.x = Math.max(18, Math.min(CANVAS_W - 18, p.x + (cur.x - prev.x)));
            p.y = Math.max(22, Math.min(CANVAS_H - 22, p.y + (cur.y - prev.y)));
          }
          continue;
        }

        if (obj.kind === "part" || obj.kind === "checkpoint" || obj.kind === "dropper") {
          const top = cameraMode === "side-view" ? obj.elev : 0;
          if (p.z >= top - 12 && p.vz <= 20) {
            supportElev = Math.max(supportElev, top);
          }
        }

        if (obj.kind === "lava" && p.z < 14) {
          const rp = respawnPoint();
          p.x = rp.x;
          p.y = rp.y;
          p.z = rp.z;
          p.vx = 0;
          p.vy = 0;
          p.vz = 0;
          p.hazardFlash = 1;
          setRespawns((r) => r + 1);
          setStatusLine(`Touched ${obj.label} — respawned at ${stageRef.current > 1 ? `Stage ${stageRef.current}` : "SpawnPad"}.`);
          break;
        }

        if (obj.kind === "npc" && p.z < 18 && p.hazardFlash <= 0.1) {
          p.vx = p.x < cur.x + obj.w / 2 ? -180 : 180;
          p.vz = 140;
          p.hazardFlash = 0.65;
          setStatusLine(`${obj.label}: "Halt! Guard patrol zone."`);
        }

        if (obj.kind === "jump" && p.z < 18 && p.vz <= 40) {
          p.vz = 360;
          p.grounded = false;
          setStatusLine(`Launched on ${obj.label} (JumpPad impulse).`);
        }

        if (obj.kind === "ice" && p.z < 10) {
          p.onIce = true;
        }

        if (obj.kind === "speed" && p.z < 12) {
          p.speedBoost = 2.5;
          setStatusLine(`Boosted on ${obj.label} (+WalkSpeed).`);
        }

        if (obj.kind === "checkpoint" && obj.stage && obj.stage > stageRef.current) {
          setStage(obj.stage);
          setStatusLine(`Reached ${obj.label} — checkpoint saved.`);
        }

        if (obj.kind === "portal" && obj.portalPair && p.portalCooldown <= 0) {
          const target = world.objects.find((o) => o.id === obj.portalPair);
          if (target) {
            p.x = target.x + target.w / 2;
            p.y = target.y + target.h / 2;
            p.z = cameraMode === "side-view" ? target.elev + 8 : 0;
            p.portalCooldown = 1.2;
            p.portalFlash = 1;
            setStatusLine(`Warped ${obj.label} -> ${target.label}.`);
          }
        }

        if (obj.kind === "coin" && !collectedRef.current.has(obj.id)) {
          const zOk = cameraMode === "top-down" || Math.abs(p.z - obj.elev) < 34;
          if (zOk) {
            collectedRef.current.add(obj.id);
            setCollected((prevCol) => (prevCol.includes(obj.id) ? prevCol : [...prevCol, obj.id]));
            setBalance((b) => b + 25);
            setStatusLine(`Collected +25 ${world.currencyName}.`);
          }
        }
      }

      if (p.z <= supportElev) {
        p.z = supportElev;
        p.vz = 0;
        p.grounded = true;
      } else {
        p.grounded = false;
      }

      if (world.hasDayNight && (ix !== 0 || iy !== 0 || forceJump)) {
        setClockHour((h) => (h + dt * 1.4) % 24);
      }
    },
    [cameraMode, respawnPoint, world],
  );

  // Render crisp HTML5 Canvas frame (supports BOTH Top-Down & Side-View + click hitboxes)
  const drawFrame = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const p = physRef.current;
    const dark = theme === "dark";
    const isNight = world.hasDayNight && (clockHour < 6 || clockHour >= 19);
    const hitboxes: RenderHitbox[] = [];

    ctx.clearRect(0, 0, CANVAS_W, CANVAS_H);
    ctx.fillStyle = isNight ? "#151A24" : dark ? "#1A1917" : "#F4F2EA";
    ctx.fillRect(0, 0, CANVAS_W, CANVAS_H);

    // Subtle Studio grid
    ctx.strokeStyle = isNight ? "rgba(255,255,255,0.06)" : dark ? "rgba(255,255,255,0.05)" : "rgba(20,20,19,0.06)";
    ctx.lineWidth = 1;
    for (let gx = 20; gx < CANVAS_W; gx += 28) {
      ctx.beginPath();
      ctx.moveTo(gx, 0);
      ctx.lineTo(gx, CANVAS_H);
      ctx.stroke();
    }
    for (let gy = 20; gy < CANVAS_H; gy += 28) {
      ctx.beginPath();
      ctx.moveTo(0, gy);
      ctx.lineTo(CANVAS_W, gy);
      ctx.stroke();
    }

    // Side-View baseplate horizon floor
    if (cameraMode === "side-view") {
      ctx.fillStyle = dark ? "#252420" : "#E5E2D6";
      ctx.fillRect(0, FLOOR_Y_SIDE, CANVAS_W, CANVAS_H - FLOOR_Y_SIDE);
      ctx.strokeStyle = dark ? "#3F3D38" : "#C5C1B4";
      ctx.beginPath();
      ctx.moveTo(0, FLOOR_Y_SIDE + 0.5);
      ctx.lineTo(CANVAS_W, FLOOR_Y_SIDE + 0.5);
      ctx.stroke();
    }

    const palette: Record<StageObjKind, { fill: string; stroke: string; text: string }> = {
      spawn: { fill: "#2860D8", stroke: "#1C449E", text: "#FFFFFF" },
      checkpoint: { fill: "#267F4F", stroke: "#185A36", text: "#FFFFFF" },
      lava: { fill: "#D44824", stroke: "#922B12", text: "#FFFFFF" },
      ice: { fill: "#5CAEE8", stroke: "#2B77AD", text: "#0F1B26" },
      jump: { fill: "#1F8C6A", stroke: "#125E46", text: "#FFFFFF" },
      moving: { fill: "#3B6EA8", stroke: "#244A75", text: "#FFFFFF" },
      speed: { fill: "#C47D16", stroke: "#8A5409", text: "#FFFFFF" },
      portal: { fill: "#7046C8", stroke: "#48298E", text: "#FFFFFF" },
      coin: { fill: "#E5B42A", stroke: "#96700B", text: "#141413" },
      dropper: { fill: "#B36A2E", stroke: "#7A4519", text: "#FFFFFF" },
      npc: { fill: "#B94435", stroke: "#7D281D", text: "#FFFFFF" },
      vehicle: { fill: "#4663C9", stroke: "#2D4291", text: "#FFFFFF" },
      part: { fill: dark ? "#3A3936" : "#D6D3C9", stroke: dark ? "#5A5853" : "#A29E93", text: dark ? "#F4F3EE" : "#141413" },
    };

    // Draw portal link line if two portals exist
    const portals = world.objects.filter((o) => o.kind === "portal");
    if (portals.length >= 2) {
      const a = liveObjPos(portals[0], p.time);
      const b = liveObjPos(portals[1], p.time);
      ctx.save();
      ctx.setLineDash([4, 4]);
      ctx.strokeStyle = "rgba(124, 82, 212, 0.55)";
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      if (cameraMode === "top-down") {
        ctx.moveTo(a.x + portals[0].w / 2, a.y + portals[0].h / 2);
        ctx.lineTo(b.x + portals[1].w / 2, b.y + portals[1].h / 2);
      } else {
        ctx.moveTo(a.x + portals[0].w / 2, FLOOR_Y_SIDE - portals[0].elev - 22);
        ctx.lineTo(b.x + portals[1].w / 2, FLOOR_Y_SIDE - portals[1].elev - 22);
      }
      ctx.stroke();
      ctx.restore();
    }

    for (const obj of world.objects) {
      if (obj.kind === "coin" && collectedRef.current.has(obj.id)) continue;
      const cur = liveObjPos(obj, p.time);
      const basePal = palette[obj.kind];
      const fill = obj.color || basePal.fill;
      const stroke = basePal.stroke;

      let rx = cur.x;
      let ry = cur.y;
      let rw = obj.w;
      let rh = obj.h;

      if (cameraMode === "side-view") {
        if (obj.kind === "coin") {
          ry = FLOOR_Y_SIDE - obj.elev - 18 + Math.sin(p.time * 4 + obj.x) * 3;
          rw = 20;
          rh = 20;
        } else if (obj.kind === "portal") {
          rw = 34;
          rh = 44;
          ry = FLOOR_Y_SIDE - obj.elev - rh;
        } else if (obj.kind === "npc") {
          rw = 24;
          rh = 34;
          ry = FLOOR_Y_SIDE - rh;
        } else if (obj.kind === "checkpoint") {
          rw = obj.w;
          rh = 12;
          ry = FLOOR_Y_SIDE - obj.elev - rh;
        } else {
          rh = obj.kind === "lava" ? 10 : 14;
          ry = FLOOR_Y_SIDE - obj.elev - rh;
        }
      }

      // Track rail for moving platform
      if (obj.kind === "moving" && obj.moveAmp) {
        ctx.strokeStyle = dark ? "rgba(255,255,255,0.16)" : "rgba(20,20,19,0.16)";
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(obj.x - obj.moveAmp + rw / 2, ry + rh / 2);
        ctx.lineTo(obj.x + obj.moveAmp + rw / 2, ry + rh / 2);
        ctx.stroke();
      }

      hitboxes.push({
        objId: obj.id,
        explorerPath: obj.explorerPath,
        label: obj.label,
        rx,
        ry,
        rw,
        rh,
      });

      if (obj.kind === "coin") {
        ctx.beginPath();
        ctx.arc(rx + rw / 2, ry + rh / 2, rw / 2, 0, Math.PI * 2);
        ctx.fillStyle = fill;
        ctx.fill();
        ctx.strokeStyle = stroke;
        ctx.lineWidth = 1.5;
        ctx.stroke();
      } else {
        // 3D extrusion bevel
        ctx.fillStyle = stroke;
        ctx.fillRect(rx, ry + 4, rw, rh);
        ctx.fillStyle = fill;
        ctx.fillRect(rx, ry, rw, rh);
        ctx.strokeStyle = stroke;
        ctx.lineWidth = 1;
        ctx.strokeRect(rx + 0.5, ry + 0.5, rw - 1, rh - 1);

        // Checkpoint flagpole in side-view
        if (cameraMode === "side-view" && obj.kind === "checkpoint") {
          const reached = Boolean(obj.stage && obj.stage <= stageRef.current);
          ctx.fillStyle = dark ? "#D5D2C8" : "#3F3D38";
          ctx.fillRect(rx + rw - 8, ry - 26, 2, 26);
          ctx.fillStyle = reached ? "#31B88A" : "#D97757";
          ctx.fillRect(rx + rw - 6, ry - 26, 14, 9);
        }

        // Crisp non-overlapping label
        const shortLabel = obj.label.replace(/^MovingPlatform$/i, "MovePlat").slice(0, 10);
        ctx.font = "500 11px monospace";
        ctx.textAlign = "center";
        if (cameraMode === "side-view") {
          ctx.fillStyle = dark ? "#C8C5BC" : "#403E39";
          if (obj.elev >= 24) {
            ctx.textBaseline = "bottom";
            ctx.fillText(shortLabel, rx + rw / 2, obj.kind === "checkpoint" ? ry - 28 : ry - 5);
          } else {
            ctx.textBaseline = "top";
            ctx.fillText(shortLabel, rx + rw / 2, ry + rh + 6);
          }
        } else if (obj.kind === "spawn") {
          ctx.fillStyle = dark ? "#C8C5BC" : "#403E39";
          ctx.textBaseline = "top";
          ctx.fillText(shortLabel, rx + rw / 2, ry + rh + 6);
        } else {
          ctx.fillStyle = basePal.text;
          ctx.textBaseline = "middle";
          ctx.fillText(shortLabel, rx + rw / 2, ry + rh / 2);
        }
      }

      // Studio selection bounding box if selected in Explorer
      if (selectedPath && obj.explorerPath === selectedPath) {
        ctx.save();
        ctx.strokeStyle = "#1562FC";
        ctx.lineWidth = 2;
        ctx.setLineDash([4, 2]);
        ctx.strokeRect(rx - 3, ry - 3, rw + 6, rh + 6);
        ctx.restore();
      }
    }

    // Draw drifting Tycoon Dropper orbs
    for (const orb of p.orbs) {
      const oy = cameraMode === "side-view" ? FLOOR_Y_SIDE - 34 : orb.y;
      ctx.fillStyle = "#E5B42A";
      ctx.fillRect(orb.x - 5, oy - 5, 10, 10);
      ctx.strokeStyle = "#8A6208";
      ctx.lineWidth = 1;
      ctx.strokeRect(orb.x - 5, oy - 5, 10, 10);
    }

    // Draw Roblox blocky avatar
    const ax = p.x;
    const ayBase = cameraMode === "side-view" ? FLOOR_Y_SIDE - 12 : p.y;
    const jumpOffset = cameraMode === "side-view" ? p.z : p.z * 0.45;
    const ay = ayBase - jumpOffset;

    // Ground contact shadow
    const shadowScale = Math.max(0.45, 1 - p.z / 260);
    ctx.fillStyle = "rgba(0,0,0,0.22)";
    ctx.beginPath();
    ctx.ellipse(ax, ayBase + 10, 11 * shadowScale, 4 * shadowScale, 0, 0, Math.PI * 2);
    ctx.fill();

    // Torso + Head
    ctx.fillStyle = ownedRef.current ? "#D97757" : dark ? "#F2F0E8" : "#141413";
    ctx.fillRect(ax - 8, ay - 6, 16, 16);
    ctx.fillStyle = "#F5C451";
    ctx.fillRect(ax - 6, ay - 16, 12, 10);
    ctx.strokeStyle = dark ? "#141413" : "#FFFFFF";
    ctx.lineWidth = 1;
    ctx.strokeRect(ax - 8, ay - 6, 16, 16);

    // Subtle rain lines when WeatherCycle is active
    if (world.hasWeather) {
      ctx.strokeStyle = dark ? "rgba(146, 208, 255, 0.28)" : "rgba(38, 96, 180, 0.22)";
      ctx.lineWidth = 1;
      for (let i = 0; i < 18; i++) {
        const rx = ((i * 47 + p.time * 90) % (CANVAS_W + 40)) - 20;
        const ry = (i * 31 + p.time * 210) % CANVAS_H;
        ctx.beginPath();
        ctx.moveTo(rx, ry);
        ctx.lineTo(rx - 4, ry + 11);
        ctx.stroke();
      }
    }

    // Hazard flash / portal warp feedback overlays
    if (p.hazardFlash > 0.01) {
      ctx.fillStyle = `rgba(212, 72, 36, ${Math.min(0.42, p.hazardFlash * 0.42)})`;
      ctx.fillRect(0, 0, CANVAS_W, CANVAS_H);
    }
    if (p.portalFlash > 0.01) {
      ctx.fillStyle = `rgba(112, 70, 200, ${Math.min(0.35, p.portalFlash * 0.35)})`;
      ctx.fillRect(0, 0, CANVAS_W, CANVAS_H);
    }

    hitboxesRef.current = hitboxes;
  }, [cameraMode, clockHour, selectedPath, theme, world]);

  const showingPlayable2D = viewportTab === "playable" && !isCardWorld;

  // 60fps requestAnimationFrame loop (only runs when 2D Playable Obby/Platformer stage is visible)
  useEffect(() => {
    if (!showingPlayable2D) return;
    drawFrame();
    if (!stageOpen || reduce) return;
    let rafId = 0;
    let lastTs = performance.now();

    const loop = (now: number) => {
      const dt = Math.min(0.05, Math.max(0.001, (now - lastTs) / 1000));
      lastTs = now;
      const p = physRef.current;
      const hasAnimatedWorld =
        world.objects.some((o) => Boolean(o.moveAmp)) ||
        world.hasDropper ||
        world.hasWeather;
      const isMoving =
        keysRef.current.size > 0 ||
        !p.grounded ||
        Math.abs(p.vx) > 0.5 ||
        Math.abs(p.vy) > 0.5 ||
        p.hazardFlash > 0.01 ||
        p.portalFlash > 0.01 ||
        p.portalCooldown > 0.01;

      if (hasAnimatedWorld || isMoving) {
        stepPhysics(dt);
        drawFrame();
      }
      rafId = requestAnimationFrame(loop);
    };

    rafId = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(rafId);
  }, [drawFrame, reduce, showingPlayable2D, stageOpen, stepPhysics, world, renderTick]);

  const nudge = (dx: number, dy: number, jump = false) => {
    stepPhysics(0.14, dx, dy, jump);
    drawFrame();
    setRenderTick((n) => n + 1);
  };

  const handleCanvasClick = (e: ReactMouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const mx = ((e.clientX - rect.left) / rect.width) * CANVAS_W;
    const my = ((e.clientY - rect.top) / rect.height) * CANVAS_H;

    // Reverse scan so topmost drawn object wins
    for (let i = hitboxesRef.current.length - 1; i >= 0; i--) {
      const hb = hitboxesRef.current[i];
      if (mx >= hb.rx - 4 && mx <= hb.rx + hb.rw + 4 && my >= hb.ry - 4 && my <= hb.ry + hb.rh + 4) {
        onSelectPart?.(hb.explorerPath);
        setStatusLine(`Selected ${hb.label} (${hb.explorerPath}) in Studio Explorer.`);
        drawFrame();
        return;
      }
    }
  };

  const buyShopItem = () => {
    if (!world.shopItem) return;
    if (ownedItem === world.shopItem) {
      setStatusLine(`Already equipped ${world.shopItem}.`);
      return;
    }
    if (balance < world.shopPrice) {
      setBalance((b) => b + world.shopPrice);
      setStatusLine(`Earned ${world.shopPrice} ${world.currencyName} passive income — click Buy again.`);
      return;
    }
    setBalance((b) => b - world.shopPrice);
    setOwnedItem(world.shopItem);
    setStatusLine(`Bought ${world.shopItem} via OpenROShopBuy (-${world.shopPrice} ${world.currencyName}).`);
  };

  return (
    <section
      aria-label="Playable Roblox Stage Preview"
      className="rounded-card border border-hairline bg-surface"
    >
      <Expandable
        expandDirection="vertical"
        expandBehavior="push"
        expanded={stageOpen}
        onToggle={() => setStageOpen((v) => !v)}
      >
        {({ isExpanded }) => (
          <>
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-hairline bg-board/60 px-3.5 py-2.5">
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                {viewportTab === "studio-3d" ? (
                  <Box className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                ) : viewportTab === "embedded-studio" ? (
                  <Monitor className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                ) : (
                  <Gamepad2 className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                )}
                <span className="text-sm font-medium text-ink">
                  {viewportTab === "studio-3d"
                    ? "Roblox Studio 3D Viewport"
                    : viewportTab === "embedded-studio"
                      ? "Embedded Roblox Studio"
                      : isCardWorld
                        ? "Playable Card Battler Stage"
                        : "Playable Stage"}
                </span>
                <Pill className="bg-tag-sky-bg px-2 py-0.5 text-xs text-tag-sky-fg">{world.themeLabel}</Pill>
                {showingPlayable2D && world.hasCheckpoints > 0 && (
                  <Pill className="bg-board px-2 py-0.5 font-mono text-xs tabular-nums text-ink">
                    Stage {stage}/{world.hasCheckpoints}
                  </Pill>
                )}
                {showingPlayable2D && (
                  <Pill className="bg-tag-dev-bg px-2 py-0.5 font-mono text-xs tabular-nums text-tag-dev-fg">
                    <Coins className="size-3" strokeWidth={1.5} aria-hidden />
                    {balance} {world.currencyName}
                  </Pill>
                )}
                {showingPlayable2D && respawns > 0 && (
                  <Pill className="bg-pill-pro-bg px-2 py-0.5 font-mono text-xs tabular-nums text-pill-pro-fg">
                    {respawns} {respawns === 1 ? "hazard" : "hazards"}
                  </Pill>
                )}
                {showingPlayable2D && world.hasDayNight && (
                  <span className="font-mono text-xs tabular-nums text-ink-2">
                    {String(Math.floor(clockHour)).padStart(2, "0")}:00
                  </span>
                )}
              </div>

              <div className="flex min-w-0 max-w-full flex-wrap items-center gap-1.5">
                <SegmentedControl aria-label="Studio viewport mode" size="sm" className="max-w-full flex-wrap">
                  <SegmentedControlItem
                    data-checked={viewportTab === "studio-3d" ? "" : undefined}
                    aria-pressed={viewportTab === "studio-3d"}
                    onClick={() => setViewportTab("studio-3d")}
                    className="gap-1 max-md:!h-11 max-md:px-2"
                  >
                    <Box className="size-3.5" strokeWidth={1.5} aria-hidden />
                    Studio 3D
                  </SegmentedControlItem>
                  <SegmentedControlItem
                    data-checked={viewportTab === "embedded-studio" ? "" : undefined}
                    aria-pressed={viewportTab === "embedded-studio"}
                    onClick={() => setViewportTab("embedded-studio")}
                    className="gap-1 max-md:!h-11 max-md:px-2"
                  >
                    <Monitor className="size-3.5" strokeWidth={1.5} aria-hidden />
                    Embedded Studio
                  </SegmentedControlItem>
                  <SegmentedControlItem
                    data-checked={viewportTab === "playable" ? "" : undefined}
                    aria-pressed={viewportTab === "playable"}
                    onClick={() => setViewportTab("playable")}
                    className="gap-1 max-md:!h-11 max-md:px-2"
                  >
                    <Gamepad2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                    Playable Stage
                  </SegmentedControlItem>
                </SegmentedControl>

                {showingPlayable2D && (
                  <>
                    <SegmentedControl aria-label="Stage camera mode" size="sm">
                      <SegmentedControlItem
                        data-checked={cameraMode === "top-down" ? "" : undefined}
                        aria-pressed={cameraMode === "top-down"}
                        onClick={() => setCameraMode("top-down")}
                        className="gap-1 max-md:!h-11 max-md:px-2.5"
                      >
                        <Layers className="size-3.5" strokeWidth={1.5} aria-hidden />
                        Top-Down
                      </SegmentedControlItem>
                      <SegmentedControlItem
                        data-checked={cameraMode === "side-view" ? "" : undefined}
                        aria-pressed={cameraMode === "side-view"}
                        onClick={() => setCameraMode("side-view")}
                        className="gap-1 max-md:!h-11 max-md:px-2.5"
                      >
                        <Eye className="size-3.5" strokeWidth={1.5} aria-hidden />
                        Side-View
                      </SegmentedControlItem>
                    </SegmentedControl>

                    {world.shopItem && (
                      <Button variant="outline" size="xs" onClick={buyShopItem} className="max-md:min-h-11">
                        {ownedItem ? (
                          <Sparkles className="size-3 text-blue" strokeWidth={1.5} aria-hidden />
                        ) : (
                          <ShoppingBag className="size-3" strokeWidth={1.5} aria-hidden />
                        )}
                        {ownedItem ? `${ownedItem} equipped` : `Buy ${world.shopItem} (${world.shopPrice})`}
                      </Button>
                    )}
                    <Button
                      variant="outline"
                      size="xs"
                      onClick={resetPlay}
                      aria-label="Reset playtest"
                      className="max-md:min-h-11"
                    >
                      <RotateCcw className="size-3.5" strokeWidth={1.5} aria-hidden />
                      Reset Playtest
                    </Button>
                  </>
                )}
                <ExpandableTrigger
                  aria-label="Toggle playable stage preview"
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
              {viewportTab === "studio-3d" || viewportTab === "embedded-studio" ? (
                <div className="p-3">
                  <StudioViewport3D
                    mode={viewportTab}
                    place={place}
                    theme={theme}
                    assets={assets && assets.length > 0 ? assets : APEX_CARDS}
                    selectedPath={selectedPath}
                    studioMode={studioMode}
                    actions={studioActions}
                    hasLiveFrame={hasLiveFrame}
                    frameSeq={frameSeq}
                    building={building}
                    canRedo={canRedo}
                    embedStatus={embedStatus}
                    onSelectPart={onSelectPart}
                    onSaveInstance={onSaveInstance}
                    onCreatePart={onCreatePart}
                    onDuplicatePart={onDuplicatePart}
                    onDeletePart={onDeletePart}
                    onTriggerRedo={onTriggerRedo}
                    onRunPlaytest={onRunPlaytest}
                    onCaptureFrame={onCaptureFrame}
                    onConnectMcp={onConnectMcp}
                    onDisconnectMcp={onDisconnectMcp}
                    onLaunchEmbed={onLaunchEmbed}
                    onStopEmbed={onStopEmbed}
                    onRefreshEmbedStatus={onRefreshEmbedStatus}
                    onSendMouseInput={onSendMouseInput}
                    onSendKeyboardInput={onSendKeyboardInput}
                    onInvokeStudioTool={onInvokeStudioTool}
                  />
                </div>
              ) : isCardWorld ? (
                <div className="p-3">
                  <CardBattlerStage
                    assets={assets && assets.length > 0 ? assets : APEX_CARDS}
                    onOpenArtStudio={onOpenArtStudio}
                  />
                </div>
              ) : (
                <div className="flex flex-col gap-2.5 p-3">
                  <div
                    tabIndex={0}
                    aria-label="Interactive Roblox stage canvas (use WASD or Arrow keys and Space, or click any part to select)"
                    onKeyDown={(e) => {
                      if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", " ", "a", "A", "d", "D", "w", "W", "s", "S"].includes(e.key)) {
                        e.preventDefault();
                        keysRef.current.add(e.key);
                        if (reduce) {
                          const dx = e.key === "ArrowLeft" || e.key === "a" || e.key === "A" ? -1 : e.key === "ArrowRight" || e.key === "d" || e.key === "D" ? 1 : 0;
                          const dy = e.key === "ArrowUp" || e.key === "w" || e.key === "W" ? -1 : e.key === "ArrowDown" || e.key === "s" || e.key === "S" ? 1 : 0;
                          nudge(dx, dy, e.key === " ");
                        }
                      }
                    }}
                    onKeyUp={(e) => {
                      keysRef.current.delete(e.key);
                    }}
                    onBlur={() => {
                      keysRef.current.clear();
                    }}
                    className="overflow-hidden rounded-sm border border-hairline outline-none"
                  >
                    <canvas
                      ref={canvasRef}
                      width={CANVAS_W}
                      height={CANVAS_H}
                      onClick={handleCanvasClick}
                      className="block h-auto w-full cursor-crosshair"
                    />
                  </div>

                  <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                    <p className="inline-flex min-w-0 flex-1 items-center gap-1.5 text-xs text-ink-2">
                      <Crosshair className="size-3.5 shrink-0 text-icon" strokeWidth={1.5} aria-hidden />
                      <span className="truncate">{statusLine}</span>
                    </p>
                    <div className="flex shrink-0 items-center gap-1">
                      <Button variant="outline" size="icon-xs" aria-label="Move avatar left" onClick={() => nudge(-1.4, 0)} className="max-md:size-11">
                        <ArrowLeft className="size-3.5" strokeWidth={1.5} aria-hidden />
                      </Button>
                      <Button variant="outline" size="icon-xs" aria-label="Move avatar up" onClick={() => nudge(0, -1.4)} className="max-md:size-11">
                        <ArrowUp className="size-3.5" strokeWidth={1.5} aria-hidden />
                      </Button>
                      <Button variant="outline" size="icon-xs" aria-label="Move avatar down" onClick={() => nudge(0, 1.4)} className="max-md:size-11">
                        <ArrowDown className="size-3.5" strokeWidth={1.5} aria-hidden />
                      </Button>
                      <Button variant="outline" size="icon-xs" aria-label="Move avatar right" onClick={() => nudge(1.4, 0)} className="max-md:size-11">
                        <ArrowRight className="size-3.5" strokeWidth={1.5} aria-hidden />
                      </Button>
                      <Button variant="outline" size="xs" onClick={() => nudge(0.6, 0, true)} className="max-md:min-h-11">
                        Jump
                      </Button>
                    </div>
                  </div>
                </div>
              )}
            </ExpandableContent>
          </>
        )}
      </Expandable>
    </section>
  );
}
