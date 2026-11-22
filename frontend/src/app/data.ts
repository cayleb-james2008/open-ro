// Data contracts and runtime shape guards for OpenRO's HTTP API (/api/*).
export type Chat = { id: string; title: string };
export type Idea = { id: string; title: string; pitch: string; skills: string[] };
export type PlaceInstance = { class_name?: string; properties?: Record<string, unknown> };
export type Place = { scripts: Record<string, string>; instances: Record<string, PlaceInstance> };
export type ChatList = { chats: Chat[]; active: string; can_undo: boolean; can_redo?: boolean };
export type PartPreset = "block" | "sphere" | "cylinder" | "spawn" | "jump_pad" | "kill_brick" | "decal_board";
export type WorkspaceMode = "studio_mcp" | "plugin" | "local_place" | "browser_place" | "disconnected";

export type ComposerMode = "swarm" | "agent" | "plan" | "ask";
export type ReasoningLevel = "low" | "medium" | "high";

export type DiffLineKind = "context" | "add" | "remove";

export type DiffLine = {
  kind: DiffLineKind;
  text: string;
  old_line?: number;
  new_line?: number;
};

export type DiffHunk = {
  old_start: number;
  new_start: number;
  lines: DiffLine[];
};

export type ScriptFileDiff = {
  path: string;
  status: "added" | "modified" | "removed";
  additions: number;
  deletions: number;
  before_source: string;
  after_source: string;
  hunks: DiffHunk[];
};

export type ProposalDiagnostic = {
  path: string;
  line: number;
  severity: "error" | "warning";
  code: string;
  message: string;
};

export type PendingProposal = {
  id: string;
  chat_id: string;
  message: string;
  summary: string;
  mode?: ComposerMode;
  file_diffs: ScriptFileDiff[];
  diff?: PlaceDiff | null;
  diagnostics: ProposalDiagnostic[];
  status: "pending" | "applied" | "rejected";
  created_at_ms: number;
};

export type StudioPluginStatus = {
  connected: boolean;
  studio_name: string | null;
  place_name: string | null;
  selection: string[];
  last_heartbeat_ms: number | null;
  install_path: string | null;
  installed: boolean;
  token_configured: boolean;
  error?: string | null;
};

export type ProjectAssetRecord = {
  id: string;
  title: string;
  file?: string;
  image_url: string;
  source?: string;
};

export function normalizeAssetList(raw: unknown): ProjectAssetRecord[] {
  if (!raw || typeof raw !== "object") return [];
  const obj = raw as Record<string, unknown>;
  const list = Array.isArray(obj.assets) ? obj.assets : Array.isArray(raw) ? raw : [];
  const out: ProjectAssetRecord[] = [];
  for (const item of list) {
    if (!item || typeof item !== "object") continue;
    const r = item as Record<string, unknown>;
    const id = String(r.id || r.card_id || "").trim();
    const file = typeof r.file === "string" ? r.file : typeof r.filename === "string" ? r.filename : undefined;
    const imageUrl =
      typeof r.image_url === "string"
        ? r.image_url
        : typeof r.url === "string"
          ? r.url
          : typeof r.data_url === "string"
            ? r.data_url
            : file && file.length > 0 && !file.startsWith("data:")
              ? `/api/assets/${encodeURIComponent(file)}`
              : "";
    if (!id || !imageUrl) continue;
    out.push({
      id,
      title: typeof r.title === "string" && r.title.trim() ? r.title.trim() : id,
      file,
      image_url: imageUrl,
      source: typeof r.source === "string" ? r.source : undefined,
    });
  }
  return out;
}

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

export type AntigravityModel = {
  id: string;
  label?: string;
  family?: string;
  effort?: string;
};

export type AntigravityIssue = {
  kind: "quota" | "auth" | "error";
  message: string;
  resets_in?: string | null;
  at?: number;
};

export type ConfigSnap = {
  base_url: string;
  model_id: string;
  provider?: "antigravity" | "openai";
  antigravity_available?: boolean;
  antigravity_default_model?: string;
  antigravity_models?: AntigravityModel[];
  antigravity_issue?: AntigravityIssue | null;
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
  image_provider?: "chatgpt" | "auto" | "gemini" | "openai";
  image_model?: string;
  secrets_dir_shared?: boolean;
  auto_install_plugin?: boolean;
  has_opencloud_key?: boolean;
  opencloud_key_masked?: string | null;
  opencloud_creator?: { userId?: string | number; groupId?: string | number };
  has_gemini_api_key?: boolean;
  gemini_api_key_masked?: string | null;
  has_openai_image_api_key?: boolean;
  openai_image_api_key_masked?: string | null;
  has_gemini_key?: boolean;
  gemini_key_masked?: string | null;
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
  return { chats, active, can_undo: raw.can_undo === true, can_redo: raw.can_redo === true };
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
  card_gacha_roller: "Gacha roller",
  deck_builder: "Deck & fusion",
  card_battle_arena: "Tactical boss arena",
  tower_progression: "50-floor tower",
  potions_weather: "Weather & potions",
  card_gallery_world: "3D card lobby",
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

export function isV060OrNewer(version: string): boolean {
  const m = version.trim().match(/^(\d+)\.(\d+)/);
  if (!m) return false;
  const maj = Number(m[1]);
  const min = Number(m[2]);
  return maj > 0 || min >= 6;
}

export function isV070OrNewer(version: string): boolean {
  const m = version.trim().match(/^(\d+)\.(\d+)/);
  if (!m) return false;
  const maj = Number(m[1]);
  const min = Number(m[2]);
  return maj > 0 || min >= 7;
}

export function isV080OrNewer(version: string): boolean {
  const m = version.trim().match(/^(\d+)\.(\d+)/);
  if (!m) return false;
  const maj = Number(m[1]);
  const min = Number(m[2]);
  return maj > 0 || min >= 8;
}

export function isV090OrNewer(version: string | null | undefined): boolean {
  if (!version) return false;
  const m = version.trim().replace(/^v/i, "").match(/^(\d+)\.(\d+)/);
  if (!m) return false;
  const maj = Number(m[1]);
  const min = Number(m[2]);
  return maj > 0 || min >= 9;
}

export function isV0100OrNewer(version: string | null | undefined): boolean {
  if (!version) return false;
  const m = version.trim().replace(/^v/i, "").match(/^(\d+)\.(\d+)/);
  if (!m) return false;
  const maj = Number(m[1]);
  const min = Number(m[2]);
  return maj > 0 || min >= 10;
}

export function defaultPluginStatus(): StudioPluginStatus {
  return {
    connected: false,
    studio_name: null,
    place_name: null,
    selection: [],
    last_heartbeat_ms: null,
    install_path: null,
    installed: false,
    token_configured: false,
    error: null,
  };
}

export function normalizePluginStatus(raw: unknown): StudioPluginStatus {
  const fb = defaultPluginStatus();
  if (!raw || typeof raw !== "object") return fb;
  const o = raw as Record<string, unknown>;
  return {
    connected: o.connected === true,
    studio_name: typeof o.studio_name === "string" && o.studio_name.trim() ? o.studio_name.trim() : null,
    place_name: typeof o.place_name === "string" && o.place_name.trim() ? o.place_name.trim() : null,
    selection: Array.isArray(o.selection)
      ? o.selection.filter((s): s is string => typeof s === "string" && s.trim().length > 0)
      : [],
    last_heartbeat_ms:
      typeof o.last_heartbeat_ms === "number" && Number.isFinite(o.last_heartbeat_ms) ? o.last_heartbeat_ms : null,
    install_path: typeof o.install_path === "string" && o.install_path.trim() ? o.install_path.trim() : null,
    installed: o.installed === true,
    token_configured: o.token_configured === true,
    error: typeof o.error === "string" && o.error.trim() ? o.error.trim() : null,
  };
}

function splitLines(src: string): string[] {
  if (!src) return [];
  const norm = src.replace(/\r\n/g, "\n");
  const lines = norm.split("\n");
  if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

export function buildUnifiedHunks(beforeSrc: string, afterSrc: string, contextLines = 3): {
  hunks: DiffHunk[];
  additions: number;
  deletions: number;
} {
  const a = splitLines(beforeSrc);
  const b = splitLines(afterSrc);
  const m = a.length;
  const n = b.length;

  // Guard huge files cleanly while still returning an accurate single hunk.
  const ops: DiffLine[] = [];
  if (m * n > 250_000) {
    let ai = 1;
    let bi = 1;
    for (const line of a) ops.push({ kind: "remove", text: line, old_line: ai++ });
    for (const line of b) ops.push({ kind: "add", text: line, new_line: bi++ });
  } else {
    const dp: Uint32Array[] = Array.from({ length: m + 1 }, () => new Uint32Array(n + 1));
    for (let i = m - 1; i >= 0; i--) {
      for (let j = n - 1; j >= 0; j--) {
        dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < m && j < n) {
      if (a[i] === b[j]) {
        ops.push({ kind: "context", text: a[i], old_line: i + 1, new_line: j + 1 });
        i++;
        j++;
      } else if (dp[i + 1][j] >= dp[i][j + 1]) {
        ops.push({ kind: "remove", text: a[i], old_line: i + 1 });
        i++;
      } else {
        ops.push({ kind: "add", text: b[j], new_line: j + 1 });
        j++;
      }
    }
    while (i < m) {
      ops.push({ kind: "remove", text: a[i], old_line: i + 1 });
      i++;
    }
    while (j < n) {
      ops.push({ kind: "add", text: b[j], new_line: j + 1 });
      j++;
    }
  }

  let additions = 0;
  let deletions = 0;
  const changedIndices: number[] = [];
  ops.forEach((op, idx) => {
    if (op.kind === "add") {
      additions++;
      changedIndices.push(idx);
    } else if (op.kind === "remove") {
      deletions++;
      changedIndices.push(idx);
    }
  });
  if (changedIndices.length === 0) return { hunks: [], additions: 0, deletions: 0 };

  const ranges: Array<[number, number]> = [];
  for (const idx of changedIndices) {
    const start = Math.max(0, idx - contextLines);
    const end = Math.min(ops.length - 1, idx + contextLines);
    const last = ranges[ranges.length - 1];
    if (last && start <= last[1] + 1) {
      last[1] = Math.max(last[1], end);
    } else {
      ranges.push([start, end]);
    }
  }

  const hunks: DiffHunk[] = ranges.map(([s, e]) => {
    const slice = ops.slice(s, e + 1);
    const firstOld = slice.find((l) => typeof l.old_line === "number")?.old_line ?? 1;
    const firstNew = slice.find((l) => typeof l.new_line === "number")?.new_line ?? 1;
    return { old_start: firstOld, new_start: firstNew, lines: slice };
  });

  return { hunks, additions, deletions };
}

export function computeLocalScriptDiffs(before: Place | null, after: Place | null): ScriptFileDiff[] {
  const bScripts = before?.scripts ?? {};
  const aScripts = after?.scripts ?? {};
  const allPaths = [...new Set([...Object.keys(bScripts), ...Object.keys(aScripts)])].sort();
  const out: ScriptFileDiff[] = [];
  for (const path of allPaths) {
    const prev = bScripts[path];
    const next = aScripts[path];
    if (prev === next) continue;
    const status: ScriptFileDiff["status"] =
      prev === undefined ? "added" : next === undefined ? "removed" : "modified";
    const before_source = prev ?? "";
    const after_source = next ?? "";
    const { hunks, additions, deletions } = buildUnifiedHunks(before_source, after_source);
    out.push({ path, status, additions, deletions, before_source, after_source, hunks });
  }
  return out;
}

export function normalizeFileDiffs(raw: unknown): ScriptFileDiff[] {
  if (!Array.isArray(raw)) return [];
  const out: ScriptFileDiff[] = [];
  for (const item of raw) {
    if (!item || typeof item !== "object") continue;
    const o = item as Record<string, unknown>;
    const path = typeof o.path === "string" ? o.path.trim() : "";
    if (!path) continue;
    const before_source = typeof o.before_source === "string" ? o.before_source : "";
    const after_source = typeof o.after_source === "string" ? o.after_source : "";
    const rawStatus = String(o.status || "").toLowerCase();
    const status: ScriptFileDiff["status"] =
      rawStatus === "added" || rawStatus === "removed" || rawStatus === "modified"
        ? rawStatus
        : !before_source
          ? "added"
          : !after_source
            ? "removed"
            : "modified";
    const rawHunks = Array.isArray(o.hunks) ? o.hunks : [];
    const hunks: DiffHunk[] = rawHunks
      .filter((h): h is Record<string, unknown> => Boolean(h && typeof h === "object"))
      .map((h) => {
        const old_start = typeof h.old_start === "number" && Number.isFinite(h.old_start) ? h.old_start : 1;
        const new_start = typeof h.new_start === "number" && Number.isFinite(h.new_start) ? h.new_start : 1;
        const rawLines = Array.isArray(h.lines) ? h.lines : [];
        let oldCursor = old_start;
        let newCursor = new_start;
        const lines: DiffLine[] = rawLines
          .filter((l): l is Record<string, unknown> => Boolean(l && typeof l === "object"))
          .map((l) => {
            const k = String(l.kind || "context").toLowerCase();
            const kind: DiffLineKind = k === "add" || k === "added" || k === "+" ? "add" : k === "remove" || k === "removed" || k === "-" ? "remove" : "context";
            const text = typeof l.text === "string" ? l.text : "";
            const old_line =
              kind === "add"
                ? undefined
                : typeof l.old_line === "number"
                  ? l.old_line
                  : oldCursor++;
            const new_line =
              kind === "remove"
                ? undefined
                : typeof l.new_line === "number"
                  ? l.new_line
                  : newCursor++;
            return { kind, text, old_line, new_line };
          });
        return { old_start, new_start, lines };
      });
    const computed = hunks.length === 0 && (before_source || after_source) ? buildUnifiedHunks(before_source, after_source) : null;
    const additions =
      typeof o.additions === "number" && Number.isFinite(o.additions)
        ? o.additions
        : computed?.additions ?? hunks.reduce((acc, h) => acc + h.lines.filter((l) => l.kind === "add").length, 0);
    const deletions =
      typeof o.deletions === "number" && Number.isFinite(o.deletions)
        ? o.deletions
        : computed?.deletions ?? hunks.reduce((acc, h) => acc + h.lines.filter((l) => l.kind === "remove").length, 0);
    out.push({
      path,
      status,
      additions,
      deletions,
      before_source,
      after_source,
      hunks: hunks.length > 0 ? hunks : computed?.hunks ?? [],
    });
  }
  return out;
}

export function normalizeProposal(raw: unknown): PendingProposal | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  const id = typeof o.id === "string" && o.id.trim() ? o.id.trim() : "";
  if (!id) return null;
  const rawStatus = String(o.status || "pending").toLowerCase();
  const status: PendingProposal["status"] =
    rawStatus === "applied" || rawStatus === "rejected" ? rawStatus : "pending";
  const rawMode = String(o.mode || "plan").toLowerCase();
  const mode: ComposerMode = rawMode === "agent" || rawMode === "ask" ? rawMode : "plan";
  const rawDiag = Array.isArray(o.diagnostics)
    ? o.diagnostics
    : o.diagnostics && typeof o.diagnostics === "object" && Array.isArray((o.diagnostics as Record<string, unknown>).diagnostics)
      ? ((o.diagnostics as Record<string, unknown>).diagnostics as unknown[])
      : [];
  const diagnostics: ProposalDiagnostic[] = rawDiag
    .filter((d): d is Record<string, unknown> => Boolean(d && typeof d === "object" && typeof (d as Record<string, unknown>).message === "string"))
    .map((d) => ({
      path: typeof d.path === "string" ? d.path : "Script",
      line: typeof d.line === "number" && Number.isFinite(d.line) ? d.line : 1,
      severity: String(d.severity || "").toLowerCase() === "warning" ? "warning" : "error",
      code: typeof d.code === "string" ? d.code : "LUAU_CHECK",
      message: String(d.message),
    }));
  return {
    id,
    chat_id: typeof o.chat_id === "string" ? o.chat_id : "",
    message: typeof o.message === "string" ? o.message : "",
    summary: typeof o.summary === "string" ? o.summary : "Staged Studio change proposal",
    mode,
    file_diffs: normalizeFileDiffs(o.file_diffs),
    diff: normalizePlaceDiff(o.diff),
    diagnostics,
    status,
    created_at_ms: typeof o.created_at_ms === "number" && Number.isFinite(o.created_at_ms) ? o.created_at_ms : Date.now(),
  };
}

export function normalizeProposalList(raw: unknown): PendingProposal[] {
  const list = Array.isArray(raw)
    ? raw
    : raw && typeof raw === "object" && Array.isArray((raw as Record<string, unknown>).proposals)
      ? ((raw as Record<string, unknown>).proposals as unknown[])
      : [];
  return list
    .map(normalizeProposal)
    .filter((p): p is PendingProposal => p !== null)
    .sort((a, b) => b.created_at_ms - a.created_at_ms);
}

export type StudioPlayMode = "edit" | "play" | "server";

export type RunningStudioInfo = {
  studio_id: string;
  place_name?: string;
  datamodel_type?: string;
};

export type StudioEmbedStatus = {
  running: boolean;
  state: "idle" | "launching" | "running" | "connected" | "stopped" | "error" | "unsupported";
  phase?: "idle" | "launching" | "awaiting_signin" | "connected" | "error";
  studio_running?: boolean;
  studio_display?: string | null;
  already_logged_in?: boolean;
  running_studios?: RunningStudioInfo[];
  platform?: string;
  display?: string | null;
  studio_id?: string | null;
  datamodel_type?: string | null;
  play_mode: StudioPlayMode;
  mcp_connected: boolean;
  has_live_frame?: boolean;
  frame_seq?: number;
  vinegar_installed?: boolean;
  xvfb_installed?: boolean;
  windows_studio_path?: string | null;
  stream_url: string;
  width: number;
  height: number;
  fps?: number;
  tools_available: string[];
  message?: string;
  error?: string | null;
};

export type StudioMouseInputPayload = {
  action: "click" | "double_click" | "right_click" | "down" | "move" | "up" | "drag" | "wheel";
  x: number;
  y: number;
  norm_x: number;
  norm_y: number;
  normalized?: boolean;
  width?: number;
  height?: number;
  button?: "left" | "middle" | "right";
  delta_y?: number;
};

export type StudioKeyboardInputPayload = {
  action: "press" | "down" | "up" | "type";
  key: string;
  code?: string;
  text?: string;
  modifiers?: { ctrl?: boolean; shift?: boolean; alt?: boolean; meta?: boolean };
};

export type StudioToolName =
  | "list_roblox_studios"
  | "get_studio_state"
  | "search_game_tree"
  | "inspect_instance"
  | "script_read"
  | "script_grep"
  | "multi_edit"
  | "execute_luau"
  | "get_console_output"
  | "screen_capture"
  | "start_stop_play"
  | "user_mouse_input"
  | "user_keyboard_input";

export type StudioToolSpec = {
  name: StudioToolName;
  label: string;
  category: "Session" | "Explorer" | "Scripts" | "Runtime" | "Input";
  summary: string;
  defaultArgs: Record<string, unknown>;
};

export const STUDIO_TOOL_SPECS: StudioToolSpec[] = [
  {
    name: "list_roblox_studios",
    label: "List Studios",
    category: "Session",
    summary: "List connected Roblox Studio instances and active studio_id values.",
    defaultArgs: {},
  },
  {
    name: "get_studio_state",
    label: "Studio State",
    category: "Session",
    summary: "Inspect current Studio datamodel_type (Edit/Play/Server), selection, and place metadata.",
    defaultArgs: {},
  },
  {
    name: "search_game_tree",
    label: "Search Tree",
    category: "Explorer",
    summary: "Search live Roblox Studio DataModel instances and scripts by name or class.",
    defaultArgs: { query: "", max_results: 40 },
  },
  {
    name: "inspect_instance",
    label: "Inspect Instance",
    category: "Explorer",
    summary: "Read live Roblox properties and children for a DataModel path.",
    defaultArgs: { path: "Workspace.SpawnPad" },
  },
  {
    name: "script_read",
    label: "Read Script",
    category: "Scripts",
    summary: "Read Luau source lines directly from a Script, LocalScript, or ModuleScript.",
    defaultArgs: { path: "ServerScriptService.Currency", start_line: 1, end_line: 80 },
  },
  {
    name: "script_grep",
    label: "Grep Scripts",
    category: "Scripts",
    summary: "Search across every Luau script in the place for symbols or patterns.",
    defaultArgs: { pattern: "Players", max_matches: 25 },
  },
  {
    name: "multi_edit",
    label: "Multi Edit",
    category: "Scripts",
    summary: "Apply targeted find-and-replace edits inside a Luau script atomically.",
    defaultArgs: {
      path: "ServerScriptService.Currency",
      edits: [{ old_str: "local START_AMOUNT = 40", new_str: "local START_AMOUNT = 50" }],
    },
  },
  {
    name: "execute_luau",
    label: "Execute Luau",
    category: "Runtime",
    summary: "Execute a Luau command directly inside the connected Roblox Studio command bar.",
    defaultArgs: { code: 'print("[OpenRO] Workspace children:", #workspace:GetChildren())' },
  },
  {
    name: "get_console_output",
    label: "Console Output",
    category: "Runtime",
    summary: "Fetch recent Roblox Studio Output console logs, warnings, and errors.",
    defaultArgs: { limit: 40 },
  },
  {
    name: "screen_capture",
    label: "Screen Capture",
    category: "Runtime",
    summary: "Capture a fresh PNG frame of the live Roblox Studio viewport.",
    defaultArgs: {},
  },
  {
    name: "start_stop_play",
    label: "Play / Stop",
    category: "Runtime",
    summary: "Start Solo Play, Run Server, or Stop the live Roblox Studio simulation.",
    defaultArgs: { mode: "start_play" },
  },
  {
    name: "user_mouse_input",
    label: "Mouse Input",
    category: "Input",
    summary: "Forward mouse click, drag, or wheel coordinates directly into Roblox Studio.",
    defaultArgs: { action: "click", x: 720, y: 450, button: "left" },
  },
  {
    name: "user_keyboard_input",
    label: "Keyboard Input",
    category: "Input",
    summary: "Forward keyboard press or text input directly into Roblox Studio.",
    defaultArgs: { action: "press", key: "f" },
  },
];

export type StudioToolResult = {
  ok: boolean;
  tool: string;
  result?: unknown;
  text?: string;
  error?: string;
  frame_seq?: number;
  play_mode?: StudioPlayMode;
  duration_ms?: number;
};

export function normalizeEmbedStatus(raw: unknown): StudioEmbedStatus {
  const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const rawState = typeof o.state === "string" ? o.state.toLowerCase() : "";
  const state: StudioEmbedStatus["state"] =
    rawState === "starting"
      ? "launching"
      : rawState === "idle" ||
          rawState === "launching" ||
          rawState === "running" ||
          rawState === "connected" ||
          rawState === "stopped" ||
          rawState === "error" ||
          rawState === "unsupported"
        ? rawState
        : o.running === true
          ? "running"
          : "idle";
  const rawPlay = typeof o.play_mode === "string" ? o.play_mode.toLowerCase() : "";
  const play_mode: StudioPlayMode =
    rawPlay === "play" || rawPlay === "start_play"
      ? "play"
      : rawPlay === "server" || rawPlay === "run_server"
        ? "server"
        : "edit";
  const rawTools = Array.isArray(o.available_tools)
    ? o.available_tools
    : Array.isArray(o.tools_available)
      ? o.tools_available
      : null;
  const tools = rawTools
    ? rawTools.filter((t): t is string => typeof t === "string" && t.trim().length > 0)
    : STUDIO_TOOL_SPECS.map((s) => s.name);
  const rawErr =
    typeof o.last_error === "string" && o.last_error
      ? o.last_error
      : typeof o.error === "string" && o.error
        ? o.error
        : null;
  const rawPhase = typeof o.phase === "string" ? o.phase.toLowerCase() : "";
  const phase: StudioEmbedStatus["phase"] =
    rawPhase === "launching" ||
    rawPhase === "awaiting_signin" ||
    rawPhase === "connected" ||
    rawPhase === "error" ||
    rawPhase === "idle"
      ? rawPhase
      : undefined;
  const runningStudios: RunningStudioInfo[] = Array.isArray(o.running_studios)
    ? o.running_studios
        .filter((r): r is Record<string, unknown> => Boolean(r && typeof r === "object"))
        .map((r) => ({
          studio_id: typeof r.studio_id === "string" ? r.studio_id : "",
          place_name: typeof r.place_name === "string" ? r.place_name : undefined,
          datamodel_type: typeof r.datamodel_type === "string" ? r.datamodel_type : undefined,
        }))
        .filter((r) => r.studio_id.length > 0)
    : [];
  return {
    running: o.running === true || state === "running" || state === "connected" || phase === "awaiting_signin",
    state,
    phase,
    studio_running: o.studio_running === true,
    studio_display: typeof o.studio_display === "string" ? o.studio_display : null,
    already_logged_in: o.already_logged_in === true,
    running_studios: runningStudios,
    platform: typeof o.platform === "string" ? o.platform : undefined,
    display: typeof o.display === "string" ? o.display : null,
    studio_id: typeof o.studio_id === "string" ? o.studio_id : null,
    datamodel_type: typeof o.datamodel_type === "string" ? o.datamodel_type : null,
    play_mode,
    mcp_connected: o.mcp_connected === true || o.connected === true || o.studio_connected === true,
    has_live_frame: o.has_live_frame === true,
    frame_seq: typeof o.frame_seq === "number" && Number.isFinite(o.frame_seq) ? o.frame_seq : undefined,
    vinegar_installed: o.vinegar_installed === true,
    xvfb_installed: o.xvfb_installed === true,
    windows_studio_path: typeof o.windows_studio_path === "string" ? o.windows_studio_path : null,
    stream_url: typeof o.stream_url === "string" && o.stream_url ? o.stream_url : "/api/studio/stream.mjpg",
    width: typeof o.width === "number" && Number.isFinite(o.width) && o.width > 0 ? o.width : 1440,
    height: typeof o.height === "number" && Number.isFinite(o.height) && o.height > 0 ? o.height : 900,
    fps: typeof o.fps === "number" && Number.isFinite(o.fps) ? o.fps : undefined,
    tools_available: tools,
    message: typeof o.message === "string" ? o.message : undefined,
    error: rawErr,
  };
}

export type LibraryPack = {
  id: string;
  name: string;
  category: string;
  description: string;
  default_installed: boolean;
  installed: boolean;
  configured: boolean;
  version: string;
  tools: string[];
  capabilities: string[];
  env_vars: string[];
  provider?: string | null;
  masked_key?: string | null;
  detected_binary?: string | null;
  skill_summary: string;
};

export type LibrarySnapshot = {
  packs: LibraryPack[];
  active_tools: string[];
  blender_detected: boolean;
  blender_path: string | null;
  prompt_skills_count: number;
};

export type BlenderGenResult = {
  ok: boolean;
  preset: string;
  filename?: string;
  obj_url?: string;
  preview_url?: string | null;
  triangles?: number;
  bytes?: number;
  blender_used?: boolean;
  detail?: string;
  error?: string;
};

export type AudioGenResult = {
  ok: boolean;
  preset: string;
  filename?: string;
  wav_url?: string;
  bytes?: number;
  duration_ms?: number;
  sample_rate?: number;
  error?: string;
};

export const DEFAULT_LIBRARY_PACKS: LibraryPack[] = [
  {
    id: "roblox-studio-core",
    name: "Roblox Studio Core",
    category: "Core Studio",
    description: "Server-authoritative Luau scripting, RemoteEvent validation, DataStore persistence, and all 13 official Roblox Studio MCP tools.",
    default_installed: true,
    installed: true,
    configured: true,
    version: "0.9.0",
    tools: ["execute_luau", "search_game_tree", "inspect_instance", "script_read", "script_grep", "multi_edit", "start_stop_play"],
    capabilities: ["Luau Static Prover", "Server-Authoritative Remotes", "DataStore Safe Save", "13 Studio MCP Tools"],
    env_vars: ["OPENRO_MCP_COMMAND"],
    provider: "roblox-studio-mcp",
    skill_summary: "Guides agents to build safe server-authoritative Roblox scripts, rate-limited RemoteEvents, and pcall-wrapped DataStores.",
  },
  {
    id: "blender-3d-pipeline",
    name: "Blender 3D Pipeline",
    category: "3D & UV",
    description: "Headless Blender 5.2.2 LTS mesh modeling, UV unwrapping, Roblox stud scaling (1m = 3.57 studs), and preview rendering into assets/meshes/.",
    default_installed: true,
    installed: true,
    configured: true,
    version: "0.9.0",
    tools: ["blender_generate_mesh"],
    capabilities: ["Roblox Stud Scaling", "UV Unwrap & Baking", "OBJ Export (<6k tris)", "PNG Preview Renders"],
    env_vars: ["OPENRO_BLENDER_PATH"],
    provider: "blender-cli",
    detected_binary: "/usr/bin/blender",
    skill_summary: "Generates low-poly Roblox-ready .obj hero props (Torii gates, pedestals, arches, cauldrons) with clean UV maps.",
  },
  {
    id: "roblox-market-taste",
    name: "Roblox Market Taste & Craft Bar",
    category: "Market Taste",
    description: "Opinionated Top-Trending Roblox Craft & Anti-Slop Quality Bar with live Explore chart sync across all genres, 8-dimension Taste Dossiers, Roblox Game URL research, and automatic twists.",
    default_installed: true,
    installed: true,
    configured: true,
    version: "0.9.0",
    tools: ["market_taste_snapshot", "market_taste_set_niche", "market_taste_refresh", "research_roblox_game"],
    capabilities: ["Top-Trending Craft Bar", "8-Dimension Taste Dossier", "Auto Twists & Quality Boosts", "Live Roblox Game Research"],
    env_vars: ["OPENRO_MARKET_API_URL"],
    provider: "roblox-explore-api",
    skill_summary: "Enforces top-trending Roblox craft, sunlit stylized worlds, clean HUD layering, automatic similar-but-different twists, and live Explore chart research across all genres.",
  },
  {
    id: "image-gen-studio",
    name: "Image Generation Studio",
    category: "Media Gen",
    description: "Generate gamepass icons, UI badges, decals, and stylized character portraits using Google Nano Banana (Gemini Image) or OpenAI Image API — includes the Stylized Fan-Art & Title-Epithet Creative Knowledge point when installed.",
    default_installed: false,
    installed: false,
    configured: false,
    version: "0.9.0",
    tools: ["generate_game_art", "upload_game_asset", "art_knowledge_guide"],
    capabilities: ["Google Gemini Nano Banana", "OpenAI gpt-image-1", "Stylized Fan-Art & Title-Epithet Guide", "PNG/WebP Asset Manifest"],
    env_vars: ["GEMINI_API_KEY", "OPENAI_API_KEY"],
    provider: "gemini",
    skill_summary: "Unlocks on-demand texture, icon, decal, and stylized character generation (with iconic title-epithet naming guidance) plus automatic Roblox asset-manifest binding.",
  },
  {
    id: "model-3d-gen",
    name: "3D Model & Mesh Studio",
    category: "3D & UV",
    description: "Text-to-3D hero prop generation via Tripo / Meshy APIs with automatic headless Blender procedural fallback when offline.",
    default_installed: false,
    installed: false,
    configured: false,
    version: "0.9.0",
    tools: ["generate_3d_prop", "blender_generate_mesh"],
    capabilities: ["Procedural Blender Fallback", "Tripo / Meshy API Ready", "Roblox MeshPart Scale Check"],
    env_vars: ["TRIPO_API_KEY", "MESHY_API_KEY"],
    provider: "blender-procedural",
    skill_summary: "Produces optimized .obj/.glb hero props for Roblox MeshParts with guaranteed offline procedural generation.",
  },
  {
    id: "video-gen-studio",
    name: "OpenMotion & Video Studio",
    category: "Video & Promos",
    description: "Create game launch trailers, cutscene motion graphics, and Storefront showcase clips with OpenMotion, Veo, or Flow.",
    default_installed: false,
    installed: false,
    configured: false,
    version: "0.9.0",
    tools: ["render_promo_storyboard"],
    capabilities: ["OpenMotion Cutscenes", "Google Veo / Flow Ready", "1280x720 H.264 Launch Clips"],
    env_vars: ["GEMINI_API_KEY"],
    provider: "openmotion",
    skill_summary: "Plans and packages Roblox game trailers and animated UI reveals using OpenMotion and Veo/Flow pipelines.",
  },
  {
    id: "audio-sfx-gen",
    name: "Procedural Audio & SFX Studio",
    category: "Audio & SFX",
    description: "Synthesize instant PCM16 .wav game SFX (summon chimes, mythic fanfares, crits, potion drinks, UI clicks) plus optional ElevenLabs / Lyria APIs.",
    default_installed: false,
    installed: false,
    configured: true,
    version: "0.9.0",
    tools: ["generate_sfx_wav"],
    capabilities: ["Pure Procedural PCM16 .wav", "Summon & Crit Presets", "ElevenLabs / Lyria Ready"],
    env_vars: ["ELEVENLABS_API_KEY"],
    provider: "procedural-wav",
    skill_summary: "Generates crisp royalty-free .wav sound effects and wires SoundService Sound instances automatically.",
  },
  {
    id: "rojo-open-cloud",
    name: "Rojo & Roblox Open Cloud",
    category: "Publishing",
    description: "Rojo 7 live filesystem sync, Studio & Rojo Ship Pack (.zip) bundling, and Roblox Open Cloud Universe/Place publish workflow.",
    default_installed: false,
    installed: false,
    configured: false,
    version: "0.9.0",
    tools: ["export_rojo_bundle", "open_cloud_publish_check"],
    capabilities: ["Rojo 7 default.project.json", "Ship Pack .zip Archiver", "Roblox Open Cloud API Key"],
    env_vars: ["ROBLOX_OPEN_CLOUD_KEY"],
    provider: "rojo-7",
    skill_summary: "Packages versioned Roblox places into Rojo 7 projects and validates assets before Open Cloud deployment.",
  },
];

export function normalizeLibrarySnapshot(raw: unknown): LibrarySnapshot {
  const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const rawPacks = Array.isArray(o.packs) ? o.packs : [];
  const packs: LibraryPack[] =
    rawPacks.length > 0
      ? rawPacks
          .filter((p): p is Record<string, unknown> => Boolean(p && typeof p === "object"))
          .map((p) => ({
            id: String(p.id || ""),
            name: String(p.name || p.id || "Pack"),
            category: String(p.category || "Tools"),
            description: String(p.description || ""),
            default_installed: p.default_installed === true,
            installed: p.installed === true || p.default_installed === true,
            configured: p.configured === true,
            version: String(p.version || "0.9.0"),
            tools: Array.isArray(p.tools) ? p.tools.filter((t): t is string => typeof t === "string") : [],
            capabilities: Array.isArray(p.capabilities) ? p.capabilities.filter((c): c is string => typeof c === "string") : [],
            env_vars: Array.isArray(p.env_vars) ? p.env_vars.filter((e): e is string => typeof e === "string") : [],
            provider: typeof p.provider === "string" ? p.provider : null,
            masked_key: typeof p.masked_key === "string" ? p.masked_key : null,
            detected_binary: typeof p.detected_binary === "string" ? p.detected_binary : null,
            skill_summary: String(p.skill_summary || ""),
          }))
          .filter((p) => p.id.length > 0)
      : DEFAULT_LIBRARY_PACKS;
  const active_tools = Array.isArray(o.active_tools)
    ? o.active_tools.filter((t): t is string => typeof t === "string")
    : [...new Set(packs.filter((p) => p.installed).flatMap((p) => p.tools))];
  return {
    packs,
    active_tools,
    blender_detected: o.blender_detected === true,
    blender_path: typeof o.blender_path === "string" ? o.blender_path : null,
    prompt_skills_count:
      typeof o.prompt_skills_count === "number" && Number.isFinite(o.prompt_skills_count)
        ? o.prompt_skills_count
        : packs.filter((p) => p.installed).length,
  };
}

export type StudioActionKind =
  | "part_created"
  | "part_updated"
  | "script_written"
  | "decal_bound"
  | "luau_executed"
  | "playtest_started"
  | "playtest_log"
  | "playtest_passed"
  | "playtest_failed"
  | "studio_frame"
  | "camera_focus"
  | "reverted"
  | "imported"
  | "restored";

export type StudioMode = "live_mcp" | "plugin_live" | "local_place" | "browser_place";

export type StudioActionEvent = {
  seq: number;
  ts_ms: number;
  kind: StudioActionKind;
  studio_mode: StudioMode;
  path: string;
  class_name: string;
  position?: [number, number, number] | null;
  size?: [number, number, number] | null;
  color?: string | null;
  summary: string;
  code_snippet?: string | null;
  console_line?: string | null;
  frame_seq?: number | null;
  instance_count?: number;
  script_count?: number;
};

export type StudioActionsSnapshot = {
  studio_mode: StudioMode;
  has_live_frame: boolean;
  frame_seq: number;
  actions: StudioActionEvent[];
};

const STUDIO_KIND_MAP: Record<string, StudioActionKind> = {
  part_created: "part_created",
  partcreated: "part_created",
  part_updated: "part_updated",
  partupdated: "part_updated",
  script_written: "script_written",
  scriptwritten: "script_written",
  decal_bound: "decal_bound",
  decalbound: "decal_bound",
  luau_executed: "luau_executed",
  luauexecuted: "luau_executed",
  playtest_started: "playtest_started",
  playteststarted: "playtest_started",
  playtest_log: "playtest_log",
  playtestlog: "playtest_log",
  playtest_passed: "playtest_passed",
  playtestpassed: "playtest_passed",
  playtest_failed: "playtest_failed",
  playtestfailed: "playtest_failed",
  studio_frame: "studio_frame",
  studioframe: "studio_frame",
  camera_focus: "camera_focus",
  camerafocus: "camera_focus",
  reverted: "reverted",
  imported: "imported",
  restored: "restored",
};

function normalizeVec3(v: unknown): [number, number, number] | null {
  if (Array.isArray(v) && v.length >= 3 && v.every((n) => typeof n === "number" && Number.isFinite(n))) {
    return [v[0], v[1], v[2]];
  }
  return null;
}

export function normalizeStudioAction(raw: unknown): StudioActionEvent | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  const rawKind = String(o.kind || "part_created").trim();
  const kindKey = rawKind.toLowerCase().replace(/[^a-z0-9_]/g, "");
  const kind = STUDIO_KIND_MAP[kindKey] || STUDIO_KIND_MAP[kindKey.replace(/_/g, "")] || "part_created";
  const rawMode = String(o.studio_mode || "local_place");
  const studio_mode: StudioMode =
    rawMode === "live_mcp" || rawMode === "plugin_live" || rawMode === "local_place" || rawMode === "browser_place"
      ? rawMode
      : "local_place";
  return {
    seq: typeof o.seq === "number" && Number.isFinite(o.seq) ? o.seq : Date.now(),
    ts_ms: typeof o.ts_ms === "number" && Number.isFinite(o.ts_ms) ? o.ts_ms : Date.now(),
    kind,
    studio_mode,
    path: typeof o.path === "string" ? o.path : "",
    class_name: typeof o.class_name === "string" ? o.class_name : "Instance",
    position: normalizeVec3(o.position),
    size: normalizeVec3(o.size),
    color: typeof o.color === "string" && o.color.trim() ? o.color.trim() : null,
    summary: typeof o.summary === "string" && o.summary.trim() ? o.summary.trim() : `${kind} ${typeof o.path === "string" ? o.path : ""}`.trim(),
    code_snippet: typeof o.code_snippet === "string" ? o.code_snippet : null,
    console_line: typeof o.console_line === "string" ? o.console_line : null,
    frame_seq: typeof o.frame_seq === "number" && Number.isFinite(o.frame_seq) ? o.frame_seq : null,
    instance_count: typeof o.instance_count === "number" && Number.isFinite(o.instance_count) ? o.instance_count : undefined,
    script_count: typeof o.script_count === "number" && Number.isFinite(o.script_count) ? o.script_count : undefined,
  };
}

export function normalizeStudioActionsResponse(raw: unknown): StudioActionsSnapshot {
  const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const rawMode = String(o.studio_mode || "local_place");
  const studio_mode: StudioMode =
    rawMode === "live_mcp" || rawMode === "plugin_live" || rawMode === "local_place" || rawMode === "browser_place"
      ? rawMode
      : "local_place";
  const list = Array.isArray(o.actions) ? o.actions : [];
  const actions: StudioActionEvent[] = [];
  for (const item of list) {
    const norm = normalizeStudioAction(item);
    if (norm) actions.push(norm);
  }
  return {
    studio_mode,
    has_live_frame: o.has_live_frame === true,
    frame_seq: typeof o.frame_seq === "number" && Number.isFinite(o.frame_seq) ? o.frame_seq : 0,
    actions,
  };
}

/**
 * Subscribe to GET /api/studio/stream (SSE) for real-time agent Studio mutations & MCP frames.
 * Only opens EventSource when enabled (v0.7.0+ or standalone web engine) to keep zero 404 errors on older builds.
 */
export function subscribeStudioStream(opts: {
  enabled: boolean;
  since?: number;
  onAction: (ev: StudioActionEvent) => void;
  onFrame: (info: { seq: number; mime?: string; width?: number; height?: number; source?: string }) => void;
  onStatus?: (info: { studio_mode?: StudioMode; has_live_frame?: boolean; frame_seq?: number }) => void;
}): () => void {
  if (!opts.enabled || typeof EventSource === "undefined") {
    return () => {};
  }
  let closed = false;
  let es: EventSource | null = null;
  try {
    const url = opts.since && opts.since > 0 ? `/api/studio/stream?since=${opts.since}` : "/api/studio/stream";
    es = new EventSource(url);
    es.addEventListener("studio_action", (e) => {
      if (closed) return;
      try {
        const parsed = JSON.parse((e as MessageEvent).data);
        const norm = normalizeStudioAction(parsed);
        if (norm) opts.onAction(norm);
      } catch {
        /* ignore malformed event */
      }
    });
    es.addEventListener("studio_frame", (e) => {
      if (closed) return;
      try {
        const parsed = JSON.parse((e as MessageEvent).data) as Record<string, unknown>;
        const seq = typeof parsed.seq === "number" ? parsed.seq : typeof parsed.frame_seq === "number" ? parsed.frame_seq : Date.now();
        opts.onFrame({
          seq,
          mime: typeof parsed.mime === "string" ? parsed.mime : undefined,
          width: typeof parsed.width === "number" ? parsed.width : undefined,
          height: typeof parsed.height === "number" ? parsed.height : undefined,
          source: typeof parsed.source === "string" ? parsed.source : undefined,
        });
      } catch {
        /* ignore */
      }
    });
    es.addEventListener("status", (e) => {
      if (closed || !opts.onStatus) return;
      try {
        const parsed = JSON.parse((e as MessageEvent).data) as Record<string, unknown>;
        const rawMode = typeof parsed.studio_mode === "string" ? parsed.studio_mode : undefined;
        opts.onStatus({
          studio_mode:
            rawMode === "live_mcp" || rawMode === "plugin_live" || rawMode === "local_place" || rawMode === "browser_place"
              ? rawMode
              : undefined,
          has_live_frame: typeof parsed.has_live_frame === "boolean" ? parsed.has_live_frame : undefined,
          frame_seq: typeof parsed.frame_seq === "number" ? parsed.frame_seq : undefined,
        });
      } catch {
        /* ignore */
      }
    });
  } catch {
    es = null;
  }
  return () => {
    closed = true;
    es?.close();
  };
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
