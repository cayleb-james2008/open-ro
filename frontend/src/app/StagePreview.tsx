import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, ChevronDown, Coins, Gamepad2, RotateCcw, ShoppingBag, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import type { Place } from "./data";

type StageObj = {
  id: string;
  label: string;
  kind: "spawn" | "checkpoint" | "lava" | "ice" | "jump" | "speed" | "portal" | "coin" | "part";
  x: number;
  y: number;
  w: number;
  h: number;
  stage?: number;
};

type ParsedWorld = {
  objects: StageObj[];
  currencyName: string;
  startAmount: number;
  shopItem: string | null;
  shopPrice: number;
  hasDayNight: boolean;
  hasCheckpoints: number;
  themeLabel: string;
};

function parseWorld(place: Place): ParsedWorld {
  const scripts = place.scripts || {};
  const instances = place.instances || {};
  const allSrc = Object.values(scripts).join("\n");
  const leafNames = [...Object.keys(instances), ...Object.keys(scripts)]
    .map((p) => p.split(".").pop() || p)
    .join(" ");

  const curMatch = allSrc.match(/local CURRENCY\s*=\s*"([^"]+)"/) || allSrc.match(/stat\.Name\s*=\s*"([^"]+)"/);
  const currencyName = curMatch?.[1] || "Coins";
  const startMatch =
    allSrc.match(/local START_AMOUNT\s*=\s*(\d+)/) ||
    allSrc.match(/stat\.Value\s*=\s*(\d+)/);
  const startAmount = startMatch ? Number(startMatch[1]) : 40;

  const itemMatch = allSrc.match(/local ITEM_NAME\s*=\s*"([^"]+)"/);
  const priceMatch = allSrc.match(/local PRICE\s*=\s*(\d+)/);
  const shopItem = itemMatch?.[1] || null;
  const shopPrice = priceMatch ? Number(priceMatch[1]) : 100;

  const stagesMatch = allSrc.match(/local STAGES\s*=\s*(\d+)/);
  const hasCheckpoints = stagesMatch ? Math.min(8, Math.max(2, Number(stagesMatch[1]))) : 0;
  const hasDayNight = /ClockTime/.test(allSrc);
  const themeLabel = /\b(ice|frost)/i.test(leafNames)
    ? "Ice Parkour"
    : /\blava/i.test(leafNames)
      ? "Lava Obby"
      : /\bround/i.test(leafNames)
        ? "Arena"
        : shopItem
          ? "Tycoon"
          : hasDayNight
            ? "Live World"
            : "Roblox Place";

  const objects: StageObj[] = [];
  let cursorX = 72;

  const pushObj = (o: Omit<StageObj, "x"> & { x?: number }) => {
    const x = o.x ?? cursorX;
    objects.push({ ...o, x });
    if (o.x === undefined) cursorX += o.w + 26;
  };

  // Spawn pad first if present or implied
  const instEntries = Object.entries(instances);
  const hasSpawn = instEntries.some(([k, v]) => k.toLowerCase().includes("spawn") || v.class_name === "SpawnLocation");
  if (hasSpawn || instEntries.length > 0 || Object.keys(scripts).length > 0) {
    pushObj({ id: "spawn", label: "SpawnPad", kind: "spawn", x: 54, y: 122, w: 58, h: 46 });
    cursorX = 138;
  }

  // Real Workspace instances
  for (const [path, rec] of instEntries) {
    const name = path.split(".").pop() || path;
    const low = name.toLowerCase();
    if (low.includes("spawn") || rec.class_name === "SpawnLocation") continue;
    if (low.includes("checkpoint")) continue;
    if (low.includes("lava") || low.includes("kill") || low.includes("hazard")) {
      pushObj({ id: path, label: name, kind: "lava", y: 118, w: 58, h: 52 });
    } else if (low.includes("ice") || low.includes("frost")) {
      pushObj({ id: path, label: name, kind: "ice", y: 116, w: 74, h: 54 });
    } else if (low.includes("jump") || low.includes("bounce")) {
      pushObj({ id: path, label: name, kind: "jump", y: 124, w: 52, h: 42 });
    } else if (low.includes("speed") || low.includes("boost")) {
      pushObj({ id: path, label: name, kind: "speed", y: 124, w: 54, h: 42 });
    } else if (low.includes("portal") || low.includes("teleport")) {
      pushObj({ id: path, label: name, kind: "portal", y: 116, w: 46, h: 46 });
    } else if (low.includes("coin") || low.includes("gold") || low.includes("gem")) {
      pushObj({ id: path, label: name, kind: "coin", y: 82, w: 28, h: 28 });
    } else {
      pushObj({ id: path, label: name, kind: "part", y: 120, w: 58, h: 46 });
    }
  }

  // Stage checkpoints spawned by Checkpoints Luau script
  for (let s = 1; s <= Math.min(4, hasCheckpoints); s++) {
    const cx = Math.min(540, 150 + s * 96);
    objects.push({
      id: `cp-${s}`,
      label: `Stage ${s}`,
      kind: "checkpoint",
      x: cx,
      y: s % 2 === 0 ? 74 : 162,
      w: 50,
      h: 34,
      stage: s,
    });
  }

  // Collectible coins if currency or coins exist
  if (/leaderstats|Coin|Gold|Gem/i.test(allSrc)) {
    for (let c = 0; c < 3; c++) {
      objects.push({
        id: `coin-${c}`,
        label: currencyName,
        kind: "coin",
        x: 185 + c * 130,
        y: c % 2 === 0 ? 64 : 184,
        w: 22,
        h: 22,
      });
    }
  }

  return { objects, currencyName, startAmount, shopItem, shopPrice, hasDayNight, hasCheckpoints, themeLabel };
}

export function StagePreview({ place, theme }: { place: Place; theme: "light" | "dark" }) {
  const world = useMemo(() => parseWorld(place), [place]);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const [avatar, setAvatar] = useState({ x: 82, y: 144, jumpZ: 0 });
  const [stage, setStage] = useState(1);
  const [balance, setBalance] = useState(world.startAmount);
  useEffect(() => {
    setBalance(world.startAmount);
  }, [world.startAmount]);
  const [collected, setCollected] = useState<string[]>([]);
  const [ownedItem, setOwnedItem] = useState<string | null>(null);
  const [statusLine, setStatusLine] = useState("Use arrow buttons or WASD to test your Workspace parts.");
  const [clockHour, setClockHour] = useState(11);
  const [stageOpen, setStageOpen] = useState(true);

  const resetPlay = useCallback(() => {
    setAvatar({ x: 82, y: 144, jumpZ: 0 });
    setStage(1);
    setCollected([]);
    setStatusLine("Avatar reset to SpawnPad.");
  }, []);

  const moveAvatar = useCallback(
    (dx: number, dy: number, jump = false) => {
      const speedMult = ownedItem ? 1.55 : 1;
      setAvatar((prev) => {
        const nx = Math.max(24, Math.min(596, prev.x + dx * speedMult));
        const ny = Math.max(28, Math.min(208, prev.y + dy * speedMult));
        let jumpZ = jump ? 18 : 0;

        // Check interactions against real parsed objects
        for (const obj of world.objects) {
          const hit = nx >= obj.x - 10 && nx <= obj.x + obj.w + 10 && ny >= obj.y - 10 && ny <= obj.y + obj.h + 10;
          if (!hit) continue;
          if (obj.kind === "lava" && jumpZ === 0) {
            const cp = world.objects.find((o) => o.kind === "checkpoint" && o.stage === stage);
            const rx = cp ? cp.x + cp.w / 2 : 82;
            const ry = cp ? cp.y + cp.h / 2 : 144;
            setStatusLine(`Touched ${obj.label} — respawned at ${cp ? cp.label : "SpawnPad"}.`);
            return { x: rx, y: ry, jumpZ: 0 };
          }
          if (obj.kind === "checkpoint" && obj.stage && obj.stage > stage) {
            setStage(obj.stage);
            setStatusLine(`Reached ${obj.label} — checkpoint saved.`);
          }
          if (obj.kind === "jump") {
            jumpZ = 24;
            setStatusLine(`Bounced on ${obj.label} (+JumpPower boost).`);
          }
          if (obj.kind === "ice") {
            setStatusLine(`Gliding on ${obj.label} (low friction).`);
          }
          if (obj.kind === "speed") {
            setStatusLine(`Boosted by ${obj.label} (+WalkSpeed).`);
          }
          if (obj.kind === "coin") {
            setCollected((prevCol) => {
              if (prevCol.includes(obj.id)) return prevCol;
              setBalance((b) => b + 25);
              setStatusLine(`Collected +25 ${world.currencyName}.`);
              return [...prevCol, obj.id];
            });
          }
        }
        if (world.hasDayNight) setClockHour((h) => (h + 1) % 24);
        return { x: nx, y: ny, jumpZ };
      });
    },
    [ownedItem, stage, world],
  );

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

  // Draw crisp 2D/isometric stage canvas whenever state or theme changes (zero idle rAF loop!)
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const W = 620;
    const H = 230;
    const dark = theme === "dark";
    const isNight = world.hasDayNight && (clockHour < 6 || clockHour >= 19);

    ctx.clearRect(0, 0, W, H);
    ctx.fillStyle = isNight ? "#161B26" : dark ? "#1C1B19" : "#F3F1E8";
    ctx.fillRect(0, 0, W, H);

    // Subtle Studio baseplate grid
    ctx.strokeStyle = isNight ? "rgba(255,255,255,0.06)" : dark ? "rgba(255,255,255,0.05)" : "rgba(20,20,19,0.06)";
    ctx.lineWidth = 1;
    for (let gx = 20; gx < W; gx += 28) {
      ctx.beginPath();
      ctx.moveTo(gx, 0);
      ctx.lineTo(gx, H);
      ctx.stroke();
    }
    for (let gy = 20; gy < H; gy += 28) {
      ctx.beginPath();
      ctx.moveTo(0, gy);
      ctx.lineTo(W, gy);
      ctx.stroke();
    }

    const palette: Record<StageObj["kind"], { fill: string; stroke: string; text: string }> = {
      spawn: { fill: "#3B6CE2", stroke: "#284FB0", text: "#FFFFFF" },
      checkpoint: { fill: "#2F8F5B", stroke: "#1F6840", text: "#FFFFFF" },
      lava: { fill: "#D9532F", stroke: "#9B3318", text: "#FFFFFF" },
      ice: { fill: "#5CAEE8", stroke: "#2E7DB5", text: "#0F1B26" },
      jump: { fill: "#2D9D78", stroke: "#1C6E53", text: "#FFFFFF" },
      speed: { fill: "#C9851E", stroke: "#8F5A0E", text: "#FFFFFF" },
      portal: { fill: "#7C52D4", stroke: "#52329A", text: "#FFFFFF" },
      coin: { fill: "#E5B42A", stroke: "#9E780E", text: "#141413" },
      part: { fill: dark ? "#3A3936" : "#D6D3C9", stroke: dark ? "#5A5853" : "#A6A297", text: dark ? "#F4F3EE" : "#141413" },
    };

    for (const obj of world.objects) {
      if (obj.kind === "coin" && collected.includes(obj.id)) continue;
      const p = palette[obj.kind];
      if (obj.kind === "coin") {
        ctx.beginPath();
        ctx.arc(obj.x + obj.w / 2, obj.y + obj.h / 2, obj.w / 2, 0, Math.PI * 2);
        ctx.fillStyle = p.fill;
        ctx.fill();
        ctx.strokeStyle = p.stroke;
        ctx.lineWidth = 1.5;
        ctx.stroke();
        continue;
      }
      // Subtle 3D extrusion bevel
      ctx.fillStyle = p.stroke;
      ctx.fillRect(obj.x, obj.y + 4, obj.w, obj.h);
      ctx.fillStyle = p.fill;
      ctx.fillRect(obj.x, obj.y, obj.w, obj.h);
      ctx.strokeStyle = p.stroke;
      ctx.lineWidth = 1;
      ctx.strokeRect(obj.x + 0.5, obj.y + 0.5, obj.w - 1, obj.h - 1);

      // Label below the SpawnPad so the standing avatar never obscures the text
      if (obj.kind === "spawn") {
        ctx.fillStyle = dark ? "#C8C5BC" : "#4A4843";
        ctx.font = "500 11px monospace";
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        ctx.fillText(obj.label.slice(0, 10), obj.x + obj.w / 2, obj.y + obj.h + 8);
      } else {
        ctx.fillStyle = p.text;
        ctx.font = "500 11px monospace";
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(obj.label.slice(0, 10), obj.x + obj.w / 2, obj.y + obj.h / 2);
      }
    }

    // Draw Roblox blocky avatar
    const ay = avatar.y - avatar.jumpZ;
    ctx.fillStyle = "rgba(0,0,0,0.22)";
    ctx.beginPath();
    ctx.ellipse(avatar.x, avatar.y + 12, 10, 4, 0, 0, Math.PI * 2);
    ctx.fill();

    // Torso + Head
    ctx.fillStyle = ownedItem ? "#D97757" : dark ? "#F2F0E8" : "#141413";
    ctx.fillRect(avatar.x - 8, ay - 6, 16, 16);
    ctx.fillStyle = "#F5C451";
    ctx.fillRect(avatar.x - 6, ay - 16, 12, 10);
    ctx.strokeStyle = dark ? "#141413" : "#FFFFFF";
    ctx.lineWidth = 1;
    ctx.strokeRect(avatar.x - 8, ay - 6, 16, 16);
  }, [avatar, clockHour, collected, ownedItem, theme, world]);

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
            <div className="flex items-start justify-between gap-2 border-b border-hairline bg-board/60 px-3.5 py-2.5 sm:items-center">
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                <Gamepad2 className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                <span className="text-sm font-medium text-ink">Playable Stage</span>
                <Pill className="bg-tag-sky-bg px-2 py-0.5 text-xs text-tag-sky-fg">{world.themeLabel}</Pill>
                {world.hasCheckpoints > 0 && (
                  <Pill className="bg-board px-2 py-0.5 font-mono text-xs tabular-nums text-ink">
                    Stage {stage}/{world.hasCheckpoints}
                  </Pill>
                )}
                <Pill className="bg-tag-dev-bg px-2 py-0.5 font-mono text-xs tabular-nums text-tag-dev-fg">
                  <Coins className="size-3" strokeWidth={1.5} aria-hidden />
                  {balance} {world.currencyName}
                </Pill>
                {world.hasDayNight && (
                  <span className="font-mono text-xs text-ink-2">
                    {String(clockHour).padStart(2, "0")}:00
                  </span>
                )}
              </div>

              <div className="flex items-center gap-1.5">
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
                <Button variant="ghost" size="icon-xs" onClick={resetPlay} aria-label="Reset avatar" className="max-md:size-11">
                  <RotateCcw className="size-3.5" strokeWidth={1.5} aria-hidden />
                </Button>
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
              <div className="flex flex-col gap-2.5 p-3">
                <div
                  tabIndex={0}
                  aria-label="Interactive Roblox stage canvas (use WASD or Arrow keys and Space)"
                  onKeyDown={(e) => {
                    if (e.key === "ArrowLeft" || e.key === "a" || e.key === "A") {
                      e.preventDefault();
                      moveAvatar(-24, 0);
                    } else if (e.key === "ArrowRight" || e.key === "d" || e.key === "D") {
                      e.preventDefault();
                      moveAvatar(24, 0);
                    } else if (e.key === "ArrowUp" || e.key === "w" || e.key === "W") {
                      e.preventDefault();
                      moveAvatar(0, -22);
                    } else if (e.key === "ArrowDown" || e.key === "s" || e.key === "S") {
                      e.preventDefault();
                      moveAvatar(0, 22);
                    } else if (e.key === " ") {
                      e.preventDefault();
                      moveAvatar(14, 0, true);
                    }
                  }}
                  className="overflow-hidden rounded-sm border border-hairline outline-none"
                >
                  <canvas
                    ref={canvasRef}
                    width={620}
                    height={230}
                    className="block h-auto w-full"
                  />
                </div>

                <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                  <p className="min-w-0 flex-1 text-xs text-ink-2">{statusLine}</p>
                  <div className="flex shrink-0 items-center gap-1">
                    <Button variant="outline" size="icon-xs" aria-label="Move avatar left" onClick={() => moveAvatar(-28, 0)} className="max-md:size-11">
                      <ArrowLeft className="size-3.5" strokeWidth={1.5} aria-hidden />
                    </Button>
                    <Button variant="outline" size="icon-xs" aria-label="Move avatar up" onClick={() => moveAvatar(0, -24)} className="max-md:size-11">
                      <ArrowUp className="size-3.5" strokeWidth={1.5} aria-hidden />
                    </Button>
                    <Button variant="outline" size="icon-xs" aria-label="Move avatar down" onClick={() => moveAvatar(0, 24)} className="max-md:size-11">
                      <ArrowDown className="size-3.5" strokeWidth={1.5} aria-hidden />
                    </Button>
                    <Button variant="outline" size="icon-xs" aria-label="Move avatar right" onClick={() => moveAvatar(28, 0)} className="max-md:size-11">
                      <ArrowRight className="size-3.5" strokeWidth={1.5} aria-hidden />
                    </Button>
                    <Button variant="outline" size="xs" onClick={() => moveAvatar(18, 0, true)} className="max-md:min-h-11">
                      Jump
                    </Button>
                  </div>
                </div>
              </div>
            </ExpandableContent>
          </>
        )}
      </Expandable>
    </section>
  );
}
