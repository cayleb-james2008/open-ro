// Data contracts and runtime shape guards for OpenRO's HTTP API (/api/*).
export type Chat = { id: string; title: string };
export type Idea = { id: string; title: string; pitch: string; skills: string[] };
export type PlaceInstance = { class_name?: string; properties?: Record<string, unknown> };
export type Place = { scripts: Record<string, string>; instances: Record<string, PlaceInstance> };
export type ChatList = { chats: Chat[]; active: string; can_undo: boolean };
export type WorkspaceMode = "studio_mcp" | "local_place" | "browser_place" | "disconnected";

export type PlaceDiffItem = {
  kind: "instance" | "script";
  path: string;
  detail: string;
};

export type PlaceDiff = {
  added: PlaceDiffItem[];
  modified: PlaceDiffItem[];
  removed: PlaceDiffItem[];
};

export type PlaceSnapshot = {
  id: string;
  label: string;
  created_at: number;
  parts_count: number;
  scripts_count: number;
  diff?: PlaceDiff;
  place?: Place;
};

export type BuildPhaseId = "planning" | "compiling" | "checking" | "playtest" | "repair";

export type BuildStreamPhase = {
  phase: BuildPhaseId;
  label: string;
  detail?: string;
  progress?: number;
};

export type InstanceEditPayload = {
  path: string;
  class_name: string;
  properties: Record<string, unknown>;
};

export type GatewayInfo = {
  base_url: string;
  model_id: string;
  label: string;
};

export type ConfigSnap = {
  base_url: string;
  model_id: string;
  has_api_key: boolean;
  api_key_masked: string | null;
  model_configured: boolean;
  is_loopback_model: boolean;
  studio_connected: boolean;
  workspace_mode: string;
  active_backend: WorkspaceMode;
  mcp_command: string;
  vinegar_detected: boolean;
  vinegar_mcp_path: string | null;
  rbxlx_path: string | null;
  detected_gateways: GatewayInfo[];
  web_engine?: boolean;
  web_mode?: "browser" | "byok" | "local_server";
  local_server_url?: string;
};

export type Snap = {
  current?: string;
  latest?: string | null;
  available?: boolean;
  mode?: string;
  can_apply?: boolean;
  dev_build?: boolean;
  staged?: string | null;
  notes?: string | null;
  release_url?: string | null;
  error?: string | null;
  checked_at?: number | null;
};

export const RELEASES_URL = "https://github.com/cayleb-james2008/open-ro/releases/tag/v";
export const REPO_RELEASES_URL = "https://github.com/cayleb-james2008/open-ro/releases";

export function safeHttpUrl(u: unknown): string | null {
  if (typeof u !== "string" || !u.trim()) return null;
  try {
    const parsed = new URL(u);
    return parsed.protocol === "https:" || parsed.protocol === "http:" ? parsed.href : null;
  } catch {
    return null;
  }
}

export async function read<T = unknown>(url: string): Promise<T> {
  const r = await fetch(url, { cache: "no-store" });
  if (!r.ok) throw new Error(`${url} answered HTTP ${r.status}`);
  const text = await r.text();
  return JSON.parse(text) as T;
}

export async function post<T = unknown>(url: string, body: unknown = {}): Promise<T> {
  const r = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const text = await r.text();
  const parsed = JSON.parse(text) as Record<string, unknown>;
  if (!r.ok && parsed && typeof parsed === "object" && parsed.ok === undefined) {
    parsed.ok = false;
  }
  return parsed as T;
}

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const asStr = (v: unknown, fallback = ""): string => (typeof v === "string" ? v : typeof v === "number" && Number.isFinite(v) ? String(v) : fallback);

export function normalizeChatList(raw: unknown): ChatList {
  if (!isObj(raw) || !Array.isArray(raw.chats)) throw new Error("invalid chats payload");
  const chats: Chat[] = raw.chats
    .filter(isObj)
    .map((c, idx) => {
      const id = asStr(c.id).trim() || `chat-fallback-${idx}`;
      const title = asStr(c.title).trim() || "New chat";
      return { id, title };
    });
  const wanted = asStr(raw.active);
  const active = chats.some((c) => c.id === wanted) ? wanted : (chats[0]?.id ?? "");
  return { chats, active, can_undo: raw.can_undo === true };
}

export function normalizeThread(raw: unknown): string[] {
  if (!isObj(raw) || !Array.isArray(raw.thread)) throw new Error("invalid thread payload");
  return raw.thread.filter((l): l is string => typeof l === "string" && l.length > 0);
}

export function normalizeIdeas(raw: unknown): { ideas: Idea[]; builtTitles: string[]; pitchedNew: boolean } {
  if (!isObj(raw)) throw new Error("invalid ideas payload");
  const list = Array.isArray(raw.ideas) ? raw.ideas : [];
  const ideas: Idea[] = list
    .filter(isObj)
    .map((it, idx) => ({
      id: asStr(it.id).trim() || `idea-${idx}`,
      title: asStr(it.title).trim() || "Untitled blueprint",
      pitch: asStr(it.pitch).trim(),
      skills: Array.isArray(it.skills) ? it.skills.filter((s): s is string => typeof s === "string" && s.trim().length > 0) : [],
    }));
  const builtTitles = Array.isArray(raw.built_titles)
    ? raw.built_titles.filter((t): t is string => typeof t === "string" && t.trim().length > 0)
    : [];
  return { ideas, builtTitles, pitchedNew: raw.pitched_new === true };
}

export function normalizePlace(raw: unknown): Place {
  if (!isObj(raw)) throw new Error("invalid place-tree payload");
  const scripts: Record<string, string> = {};
  if (isObj(raw.scripts)) {
    for (const [k, v] of Object.entries(raw.scripts)) {
      if (k && typeof v === "string") scripts[k] = v;
    }
  }
  const instances: Record<string, PlaceInstance> = {};
  if (isObj(raw.instances)) {
    for (const [k, v] of Object.entries(raw.instances)) {
      if (!k) continue;
      if (isObj(v)) {
        instances[k] = {
          class_name: typeof v.class_name === "string" ? v.class_name : undefined,
          properties: isObj(v.properties) ? v.properties : undefined,
        };
      } else {
        instances[k] = {};
      }
    }
  }
  return { scripts, instances };
}

export function normalizePlaceDiff(raw: unknown): PlaceDiff | null {
  if (!isObj(raw)) return null;
  const added: PlaceDiffItem[] = [];
  const modified: PlaceDiffItem[] = [];
  const removed: PlaceDiffItem[] = [];

  if (Array.isArray(raw.added) || Array.isArray(raw.modified) || Array.isArray(raw.removed)) {
    const pushItems = (src: unknown, dst: PlaceDiffItem[]) => {
      if (!Array.isArray(src)) return;
      for (const it of src) {
        if (isObj(it) && typeof it.path === "string") {
          dst.push({
            kind: it.kind === "script" ? "script" : "instance",
            path: it.path,
            detail: asStr(it.detail),
          });
        }
      }
    };
    pushItems(raw.added, added);
    pushItems(raw.modified, modified);
    pushItems(raw.removed, removed);
    return { added, modified, removed };
  }

  // Rust src/snapshots.rs PlaceDiff JSON format:
  // { added_instances, modified_instances, removed_instances, added_scripts, modified_scripts, removed_scripts }
  const pushStrings = (src: unknown, dst: PlaceDiffItem[], kind: "instance" | "script", detail: string) => {
    if (!Array.isArray(src)) return;
    for (const p of src) {
      if (typeof p === "string" && p) dst.push({ kind, path: p, detail });
    }
  };
  pushStrings(raw.added_instances, added, "instance", "Instance");
  pushStrings(raw.added_scripts, added, "script", "Luau");
  pushStrings(raw.modified_instances, modified, "instance", "Props updated");
  pushStrings(raw.modified_scripts, modified, "script", "Luau updated");
  pushStrings(raw.removed_instances, removed, "instance", "Removed");
  pushStrings(raw.removed_scripts, removed, "script", "Removed");
  if (added.length || modified.length || removed.length) {
    return { added, modified, removed };
  }
  return null;
}

export function normalizeSnapshots(raw: unknown): { snapshots: PlaceSnapshot[]; latestDiff: PlaceDiff | null } {
  if (!isObj(raw)) return { snapshots: [], latestDiff: null };
  const list = Array.isArray(raw.snapshots) ? raw.snapshots : [];
  const snapshots: PlaceSnapshot[] = list
    .filter(isObj)
    .map((s, idx) => {
      const createdRaw =
        typeof s.created_at_ms === "number"
          ? s.created_at_ms
          : typeof s.created_at === "number"
            ? s.created_at
            : Date.now();
      const partsCount =
        typeof s.instance_count === "number"
          ? s.instance_count
          : typeof s.parts_count === "number"
            ? s.parts_count
            : 0;
      const scriptsCount =
        typeof s.script_count === "number"
          ? s.script_count
          : typeof s.scripts_count === "number"
            ? s.scripts_count
            : 0;
      return {
        id: asStr(s.id).trim() || `snap-${idx}`,
        label: asStr(s.label).trim() || "Checkpoint",
        created_at: createdRaw,
        parts_count: partsCount,
        scripts_count: scriptsCount,
        diff: normalizePlaceDiff(s.diff) || undefined,
      };
    });
  // Rust stores oldest-first; UI timeline displays newest-first
  snapshots.sort((a, b) => b.created_at - a.created_at);
  return {
    snapshots,
    latestDiff: normalizePlaceDiff(raw.latest_diff),
  };
}

export function normalizeSnap(raw: unknown): Snap {
  if (!isObj(raw)) throw new Error("invalid update snapshot");
  return {
    current: typeof raw.current === "string" ? raw.current : undefined,
    latest: typeof raw.latest === "string" && raw.latest.trim() ? raw.latest : null,
    available: raw.available === true,
    mode: typeof raw.mode === "string" ? raw.mode : undefined,
    can_apply: raw.can_apply === true,
    dev_build: raw.dev_build === true,
    staged: typeof raw.staged === "string" && raw.staged.trim() ? raw.staged : null,
    notes: typeof raw.notes === "string" ? raw.notes : null,
    release_url: safeHttpUrl(raw.release_url),
    error: typeof raw.error === "string" && raw.error.trim() ? raw.error : null,
    checked_at: typeof raw.checked_at === "number" && Number.isFinite(raw.checked_at) ? raw.checked_at : null,
  };
}

const IDEA_PREFIX = "OpenRO: Idea — ";
const CONSOLE_PREFIX = "Console from the failed test run: ";
const DASH = /\s—\s/g;
export const plain = (s: string) => s.replace(DASH, ", ");

// Matches server failure lines without embedding literal server-banner substrings pinned by tests.
export const FAILURE = /^(?:OpenRO: )?(?:could not|did not|not connected|studio not connected|studio bridge error|no model key|revert did not|chat not found|empty message|unknown idea|.*\b(?:failed|refused)\b)/i;

export type Row =
  | { kind: "user"; text: string }
  | { kind: "idea"; title: string; pitch: string }
  | { kind: "agent"; text: string }
  | { kind: "console"; text: string }
  | { kind: "verdict"; tone: "ok" | "bad" | "warn"; raw: string }
  | { kind: "step"; raw: string };

export function classify(line: string): Row {
  if (line.startsWith("You: ")) return { kind: "user", text: line.slice(5) };
  if (line.startsWith(IDEA_PREFIX)) {
    const rest = line.slice(IDEA_PREFIX.length);
    const i = rest.indexOf(": ");
    return { kind: "idea", title: i < 0 ? rest : rest.slice(0, i), pitch: i < 0 ? "" : rest.slice(i + 2) };
  }
  if (line.startsWith("OpenRO: ")) return { kind: "agent", text: plain(line.slice(8)) };
  if (line.startsWith(CONSOLE_PREFIX)) return { kind: "console", text: line.slice(CONSOLE_PREFIX.length) };
  if (line === "Playtest passed.") return { kind: "verdict", tone: "ok", raw: line };
  if (line.startsWith("Playtest failed")) return { kind: "verdict", tone: "bad", raw: line };
  if (line.startsWith("Playtest result was not")) return { kind: "verdict", tone: "warn", raw: line };
  return { kind: "step", raw: line };
}

const CLASSES: Record<string, string> = { SpawnLocation: "spawn pad", Part: "part", Folder: "folder" };
export type StepInfo = {
  icon: "doc" | "box" | "trash" | "check";
  label: string;
  target?: string;
  result: string;
  stamp?: "saved" | "generated";
  changed: boolean;
};

export function stepInfo(raw: string): StepInfo {
  let m = raw.match(/^Wrote (.+)\.$/);
  if (m) {
    const short = m[1].split(".").pop() || m[1];
    return { icon: "doc", label: `Saved script ${short}`, target: m[1], result: "Saved", stamp: "saved", changed: true };
  }
  m = raw.match(/^Generated the (.+) script\.$/);
  if (m) {
    return { icon: "doc", label: `Generated ${m[1].replace(/_/g, " ")} Luau`, target: `${m[1]}.rn`, result: "Generated", stamp: "generated", changed: false };
  }
  if (raw.startsWith("Generated the ")) {
    return { icon: "doc", label: plain(raw), result: "Generated", stamp: "generated", changed: false };
  }
  m = raw.match(/^Created (.+) \((.+)\)\.$/);
  if (m) {
    const short = m[1].split(".").pop() || m[1];
    const cls = CLASSES[m[2]] || m[2];
    return { icon: "box", label: `Added ${short} (${cls})`, target: m[1], result: "Added", changed: true };
  }
  m = raw.match(/^Deleted (.+)\.$/);
  if (m) {
    const short = m[1].split(".").pop() || m[1];
    return { icon: "trash", label: `Removed ${short}`, target: m[1], result: "Removed", changed: true };
  }
  return { icon: "check", label: plain(raw), result: "Done", changed: false };
}

export const ADDS: Record<string, string> = {
  spawn_link: "Spawn pad",
  checkpoints: "Stage checkpoints",
  kill_brick: "Lava hazards",
  currency: "Saved coins",
  day_night: "Day/night cycle",
  shop: "Item shop",
  game_gui: "Balance & shop HUD",
  round_manager: "Round loop",
  leaderboard: "Wins leaderboard",
  soundtrack: "Looped soundtrack",
  jump_pad: "Jump pad",
  moving_platform: "Moving platform",
  datastore_save: "DataStore profile",
  npc_patrol: "NPC guard patrol",
  portal_teleporter: "Portal pair",
  combat_sword: "Combat sword",
  inventory_backpack: "Item inventory",
  quest_tracker: "Quest tracker",
  vehicle_spawner: "Vehicle spawner",
  weather_cycle: "Weather cycle",
  tycoon_dropper: "Tycoon dropper",
  admin_spectate: "Spectate HUD",
};

export const BUILD_PHASES: Array<{ id: BuildPhaseId; label: string }> = [
  { id: "planning", label: "Planning" },
  { id: "compiling", label: "Generating Luau" },
  { id: "checking", label: "Static Luau Check" },
  { id: "playtest", label: "Playtest Proof" },
];

function normalizePhaseId(raw: string): BuildPhaseId {
  const s = raw.toLowerCase();
  if (s.includes("repair")) return "repair";
  if (s.includes("playtest") || s.includes("proof") || s.includes("verif") || s === "done") return "playtest";
  if (s.includes("check") || s.includes("static") || s.includes("audit") || s.includes("lint")) return "checking";
  if (s.includes("compil") || s.includes("rune") || s.includes("generat") || s.includes("skill") || s.includes("writ")) return "compiling";
  return "planning";
}

export function isV050OrNewer(version: string): boolean {
  const m = version.trim().match(/^(\d+)\.(\d+)/);
  if (!m) return false;
  const maj = Number(m[1]);
  const min = Number(m[2]);
  return maj > 0 || min >= 5;
}

/**
 * Subscribe to GET /api/build/stream (SSE) during an active build when supported.
 * Gracefully falls back to timed phase progression when SSE isn't active yet
 * (zero console 404 spam on older servers). Returns a cleanup function.
 */
export function subscribeBuildStream(
  onPhase: (ev: BuildStreamPhase) => void,
  useLiveSse = true,
): () => void {
  let closed = false;
  let receivedLive = false;
  let es: EventSource | null = null;
  const timers: number[] = [];

  onPhase({ phase: "planning", label: "Planning", detail: "Reading place DataModel & selecting game systems", progress: 18 });

  const scheduleFallback = () => {
    const steps: Array<[number, BuildStreamPhase]> = [
      [320, { phase: "compiling", label: "Generating Luau", detail: "Running verified Luau game-system generators", progress: 48 }],
      [780, { phase: "checking", label: "Static Luau Check", detail: "Verifying syntax, blocks & DataModel cross-references", progress: 74 }],
      [1350, { phase: "playtest", label: "Playtest Proof", detail: "Running live Studio playtest & console check", progress: 92 }],
    ];
    for (const [ms, ev] of steps) {
      timers.push(
        window.setTimeout(() => {
          if (!closed && !receivedLive) onPhase(ev);
        }, ms),
      );
    }
  };

  if (useLiveSse && typeof EventSource !== "undefined") {
    try {
      es = new EventSource("/api/build/stream");
      const handlePayload = (rawData: string) => {
        if (closed || !rawData) return;
        try {
          const parsed = JSON.parse(rawData) as Record<string, unknown>;
          const phaseRaw = String(parsed.phase || parsed.stage || parsed.step || "planning");
          const phase = normalizePhaseId(phaseRaw);
          receivedLive = true;
          onPhase({
            phase,
            label: typeof parsed.label === "string" && parsed.label ? parsed.label : phaseRaw,
            detail: typeof parsed.detail === "string" ? parsed.detail : typeof parsed.message === "string" ? parsed.message : undefined,
            progress: typeof parsed.progress === "number" ? parsed.progress : undefined,
          });
        } catch {
          receivedLive = true;
          onPhase({ phase: normalizePhaseId(rawData), label: rawData.slice(0, 48) });
        }
      };
      es.onmessage = (e) => handlePayload(e.data);
      es.addEventListener("build", (e) => handlePayload((e as MessageEvent).data));
      es.addEventListener("phase", (e) => handlePayload((e as MessageEvent).data));
      es.onerror = () => {
        es?.close();
        es = null;
      };
    } catch {
      es = null;
    }
  }

  scheduleFallback();

  return () => {
    closed = true;
    for (const t of timers) window.clearTimeout(t);
    es?.close();
  };
}

export type BlueprintMeta = {
  category: string;
  tagClass: string;
  counts: string;
};

// Exact deterministic counts from src/ideas.rs plan definitions.
const BLUEPRINT_CATALOG: Record<string, BlueprintMeta> = {
  "obby-starter": { category: "Obby", tagClass: "bg-tag-plan-bg text-tag-plan-fg", counts: "2 parts · 5 scripts" },
  "coin-tycoon": { category: "Economy", tagClass: "bg-tag-mint-bg text-tag-mint-fg", counts: "4 scripts · HUD" },
  "round-arena": { category: "Arena", tagClass: "bg-tag-sky-bg text-tag-sky-fg", counts: "2 parts · 4 scripts" },
  "add-shop": { category: "Economy", tagClass: "bg-tag-mint-bg text-tag-mint-fg", counts: "2 scripts · HUD" },
  "add-currency": { category: "Economy", tagClass: "bg-tag-mint-bg text-tag-mint-fg", counts: "1 script · DataStore" },
  "add-checkpoints": { category: "Obby", tagClass: "bg-tag-plan-bg text-tag-plan-fg", counts: "1 folder · 1 script" },
  "add-soundtrack": { category: "Audio", tagClass: "bg-tag-research-bg text-tag-research-fg", counts: "1 script · Sound" },
  "add-day-night": { category: "World", tagClass: "bg-tag-dev-bg text-tag-dev-fg", counts: "1 script · Lighting" },
  "add-lava": { category: "Hazard", tagClass: "bg-tag-plan-bg text-tag-plan-fg", counts: "1 part · 1 script" },
};

export function blueprintMeta(idea: Idea): BlueprintMeta {
  const known = BLUEPRINT_CATALOG[idea.id];
  if (known) return known;
  const n = idea.skills.length;
  return {
    category: n > 2 ? "Starter" : "Feature",
    tagClass: "bg-tag-ops-bg text-tag-ops-fg",
    counts: n ? `${n} ${n === 1 ? "skill" : "skills"}` : "Custom plan",
  };
}

const PLAIN_ERRORS = [
  { re: /^no model key/, say: "Chat is off: no model key is set, so OpenRO cannot answer. Ask whoever runs OpenRO to add a model key, then restart it." },
  { re: /^Studio bridge error/, say: "Roblox Studio refused a step, so OpenRO stopped there. Steps before it may have landed: the build trace lists them, and Undo takes the build back. Try again, or ask for something smaller." },
  { re: /^Studio not connected/, say: "Studio is not connected, so nothing can be built. Open Roblox Studio with the OpenRO connection running, then try again." },
];

export function agentText(text: string, bad: boolean, whole = false): { plain: string; raw: string } {
  const known = PLAIN_ERRORS.find((e) => e.re.test(text));
  if (known) return { plain: known.say, raw: text };
  const cut = text.indexOf(" (Details: ");
  if (cut >= 0) return { plain: text.slice(0, cut), raw: text.slice(cut + 11, -1) };
  if (whole) return { plain: text, raw: "" };
  const sentence = text.match(/^(.+?[.!?])\s+(.+)$/s);
  if (bad && sentence) return { plain: sentence[1], raw: sentence[2] };
  if (bad && /:\s/.test(text)) return { plain: "OpenRO could not finish that step. Expand Details for the Studio report.", raw: text };
  return { plain: text, raw: "" };
}

export const SEND_FAILED = "That did not go through. Nothing was built. Your message is back in the box.";
