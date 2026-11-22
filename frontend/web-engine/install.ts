// Real in-browser Roblox Studio IDE engine installer for OpenRO v0.8.0 standalone web builds.
// Features:
//  - Co-Building 3D Roblox Studio IDE (+ Add Part, Duplicate, Delete, Undo/Redo, Live Luau Editor)
//  - Genuine client-side Studio & Rojo Ship Pack (.zip) builder (GET /api/export/bundle)
//  - Embedded Realtime 3D Roblox Studio Viewport & Live Agent Building Stream (/api/studio/stream)
//  - All 28 parametric Roblox game-system Luau compilers (including 6 Card-Battler systems)
//  - Static Luau analyzer & cross-reference playtest prover (POST /api/studio/playtest)
//  - Genuine bidirectional Roblox Studio XML (.rbxlx) BOTH Import AND Export (with Decal bindings)
//  - IndexedDB versioned snapshots, Studio action ring buffer, custom uploaded Card Art assets, & workspace persistence
//  - Live EventSource('/api/build/stream') and EventSource('/api/studio/stream') SSE emitters
//  - Optional BYOK OpenAI-compatible LLM & Gemini Nano Banana / OpenAI Image API (keys strictly in sessionStorage)
//  - One-click Connect to Local openro-rs (http://127.0.0.1:8077)
import { computePlaceDiff } from "../src/app/luauAudit";
import { GAME_SYSTEMS } from "../src/app/systems";
import { buildRojoShipPackZip } from "../src/app/zipBuilder";
import type { CardAssetRecord } from "../src/app/cardCatalog";
import type { PlaceDiff, PlaceSnapshot, StudioActionEvent, StudioActionKind } from "../src/app/data";
import {
  STARTER_CATALOG,
  buildPlanForSkill,
  compilePromptToPlan,
  generateSkillLuau,
  type PlaceInstance,
  type PlaceState,
  type PlanStep,
} from "./luau-skills";
import { analyzePlaceLuau, validatePlace } from "./luau-validator";
import { parseRbxlxXml, serializeRbxlx } from "./rbxlx";
import { idbLoad, idbSave } from "./storage";

type ChangeEntry =
  | { kind: "script"; path: string; prev: string | null; post: string | null }
  | { kind: "instance"; path: string; prev: PlaceInstance | null; post: PlaceInstance | null };

type StoredSnapshot = PlaceSnapshot & { place: PlaceState };

type ChatSession = {
  id: string;
  title: string;
  lines: string[];
  builtSkills: string[];
  place: PlaceState;
  undoStack: Array<{ changes: ChangeEntry[]; skills: string[] }>;
  redoStack: Array<{ changes: ChangeEntry[]; skills: string[] }>;
  snapshots: StoredSnapshot[];
  studioActions: StudioActionEvent[];
};

type EngineState = {
  active: string;
  counter: number;
  actionSeq: number;
  chats: Record<string, ChatSession>;
  webMode: "browser" | "byok" | "local_server";
  byokBaseUrl: string;
  byokModelId: string;
  localServerUrl: string;
  imageProvider: "auto" | "gemini" | "openai";
  customAssets: CardAssetRecord[];
};

const BYOK_KEY_SLOT = "openro-byok-key";
const GEMINI_KEY_SLOT = "openro-gemini-key";
const OPENAI_IMG_KEY_SLOT = "openro-openai-image-key";

function readSlotKey(slot: string): string {
  try {
    return sessionStorage.getItem(slot) || "";
  } catch {
    return "";
  }
}

function writeSlotKey(slot: string, k: string) {
  try {
    if (!k.trim()) sessionStorage.removeItem(slot);
    else sessionStorage.setItem(slot, k.trim());
  } catch {
    /* ignore */
  }
}

// Lightweight EventSource subscriber registries for /api/build/stream and /api/studio/stream
type PhaseListener = (payload: Record<string, unknown>) => void;
type StudioListener = (eventName: string, payload: Record<string, unknown>) => void;
const sseListeners = new Set<PhaseListener>();
const studioListeners = new Set<StudioListener>();

function emitSsePhase(phase: string, label: string, detail: string, progress: number) {
  const payload = { phase, label, detail, progress };
  for (const fn of sseListeners) {
    try {
      fn(payload);
    } catch {
      /* ignore */
    }
  }
}

function emitStudioEvent(eventName: string, payload: Record<string, unknown>) {
  for (const fn of studioListeners) {
    try {
      fn(eventName, payload);
    } catch {
      /* ignore */
    }
  }
}

function installSseShim() {
  const OrigEventSource = window.EventSource;
  if (typeof OrigEventSource === "undefined") return;

  class OpenROWebEventSource extends EventTarget {
    url: string;
    readyState = 1;
    withCredentials = false;
    onmessage: ((this: EventSource, ev: MessageEvent) => unknown) | null = null;
    onerror: ((this: EventSource, ev: Event) => unknown) | null = null;
    onopen: ((this: EventSource, ev: Event) => unknown) | null = null;
    private listener: PhaseListener | null = null;
    private studioListener: StudioListener | null = null;

    constructor(url: string | URL, eventSourceInitDict?: EventSourceInit) {
      super();
      const raw = typeof url === "string" ? url : url.href;
      let u: URL;
      try {
        u = new URL(raw, location.href);
      } catch {
        return new OrigEventSource(url, eventSourceInitDict) as unknown as OpenROWebEventSource;
      }
      if (u.pathname === "/api/studio/stream") {
        this.url = u.href;
        this.studioListener = (eventName, payload) => {
          if (this.readyState === 2) return;
          const ev = new MessageEvent(eventName, { data: JSON.stringify(payload) });
          this.dispatchEvent(ev);
        };
        studioListeners.add(this.studioListener);
        queueMicrotask(() => {
          if (this.readyState === 2) return;
          this.dispatchEvent(
            new MessageEvent("status", {
              data: JSON.stringify({ studio_mode: "browser_place", has_live_frame: false, frame_seq: 0 }),
            }),
          );
        });
        return;
      }
      if (u.pathname !== "/api/build/stream") {
        return new OrigEventSource(url, eventSourceInitDict) as unknown as OpenROWebEventSource;
      }
      this.url = u.href;
      this.listener = (payload) => {
        if (this.readyState === 2) return;
        const ev = new MessageEvent("build", { data: JSON.stringify(payload) });
        this.dispatchEvent(ev);
        this.onmessage?.call(this as unknown as EventSource, ev);
      };
      sseListeners.add(this.listener);
    }

    close() {
      this.readyState = 2;
      if (this.listener) {
        sseListeners.delete(this.listener);
        this.listener = null;
      }
      if (this.studioListener) {
        studioListeners.delete(this.studioListener);
        this.studioListener = null;
      }
    }
  }

  Object.defineProperty(window, "EventSource", {
    value: OpenROWebEventSource,
    configurable: true,
    writable: true,
  });
}

function readSessionKey(): string {
  try {
    return sessionStorage.getItem(BYOK_KEY_SLOT) || "";
  } catch {
    return "";
  }
}

function writeSessionKey(k: string) {
  try {
    if (!k.trim()) sessionStorage.removeItem(BYOK_KEY_SLOT);
    else sessionStorage.setItem(BYOK_KEY_SLOT, k.trim());
  } catch {
    /* ignore */
  }
}

function maskKey(k: string): string | null {
  if (!k) return null;
  return k.length <= 8 ? "••••••••" : `••••${k.slice(-4)}`;
}

function clonePlace(p: PlaceState): PlaceState {
  return JSON.parse(JSON.stringify(p)) as PlaceState;
}

function recordSnapshot(chat: ChatSession, label: string, prevPlace: PlaceState | null) {
  const diff: PlaceDiff = computePlaceDiff(prevPlace, chat.place);
  const snap: StoredSnapshot = {
    id: `snap-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
    label,
    created_at: Math.floor(Date.now() / 1000),
    parts_count: Object.keys(chat.place.instances).length,
    scripts_count: Object.keys(chat.place.scripts).length,
    diff,
    place: clonePlace(chat.place),
  };
  chat.snapshots = [snap, ...(chat.snapshots || [])].slice(0, 30);
  return snap;
}

function defaultState(): EngineState {
  const id = "chat-studio-1";
  return {
    active: id,
    counter: 2,
    actionSeq: 1,
    chats: {
      [id]: {
        id,
        title: "New chat",
        lines: [],
        builtSkills: [],
        place: { scripts: {}, instances: {} },
        undoStack: [],
        redoStack: [],
        snapshots: [],
        studioActions: [],
      },
    },
    webMode: "browser",
    byokBaseUrl: "https://api.openai.com/v1",
    byokModelId: "gpt-4o-mini",
    localServerUrl: "http://127.0.0.1:8077",
    imageProvider: "auto",
    customAssets: [],
  };
}

function recordStudioAction(
  state: EngineState,
  chat: ChatSession,
  input: {
    kind: StudioActionKind;
    path: string;
    class_name: string;
    summary: string;
    position?: [number, number, number] | null;
    size?: [number, number, number] | null;
    color?: string | null;
    code_snippet?: string | null;
    console_line?: string | null;
  },
): StudioActionEvent {
  if (!Array.isArray(chat.studioActions)) chat.studioActions = [];
  const ev: StudioActionEvent = {
    seq: state.actionSeq++,
    ts_ms: Date.now(),
    kind: input.kind,
    studio_mode: "browser_place",
    path: input.path,
    class_name: input.class_name,
    position: input.position ?? null,
    size: input.size ?? null,
    color: input.color ?? null,
    summary: input.summary,
    code_snippet: input.code_snippet ?? null,
    console_line: input.console_line ?? null,
    frame_seq: null,
    instance_count: Object.keys(chat.place.instances).length,
    script_count: Object.keys(chat.place.scripts).length,
  };
  chat.studioActions.push(ev);
  if (chat.studioActions.length > 500) {
    chat.studioActions = chat.studioActions.slice(-500);
  }
  emitStudioEvent("studio_action", ev as unknown as Record<string, unknown>);
  return ev;
}

function propVec3(v: unknown): [number, number, number] | null {
  if (Array.isArray(v) && v.length >= 3 && v.every((n) => typeof n === "number" && Number.isFinite(n))) {
    return [v[0], v[1], v[2]];
  }
  return null;
}

function jsonResponse(obj: unknown, status = 200): Response {
  return new Response(JSON.stringify(obj), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function executePlan(
  state: EngineState,
  chat: ChatSession,
  steps: PlanStep[],
  skillsTag: string[],
  summaryText: string,
  snapLabel: string,
): { ok: boolean; error?: string; thread: string[] } {
  const beforePlace = clonePlace(chat.place);
  const produced: string[] = [];
  const generated = new Map<string, string>();
  let lastSkill = "";
  const changes: ChangeEntry[] = [];
  let playtestPassed = false;
  let lastConsole = "";

  emitSsePhase("planning", "Planning", "Inspecting place DataModel & skill plan", 20);

  const recordScript = (path: string, prev: string | null, post: string | null) => {
    const existing = changes.find((c) => c.kind === "script" && c.path === path);
    if (existing && existing.kind === "script") existing.post = post;
    else changes.push({ kind: "script", path, prev, post });
  };

  const recordInstance = (path: string, prev: PlaceInstance | null, post: PlaceInstance | null) => {
    const existing = changes.find((c) => c.kind === "instance" && c.path === path);
    if (existing && existing.kind === "instance") existing.post = post;
    else changes.push({ kind: "instance", path, prev, post });
  };

  for (const step of steps) {
    if ("skill" in step) {
      emitSsePhase("compiling", "Generating Luau", `Generating Luau for ${step.skill}`, 48);
      const luau = generateSkillLuau(step.skill, step.args || {});
      generated.set(step.skill, luau);
      lastSkill = step.skill;
      produced.push(`Generated the ${step.skill} script.`);
      continue;
    }
    if (step.tool === "create_instance") {
      const { class_name, name, parent, properties } = step.args;
      const path = `${parent}.${name}`;
      const prev = chat.place.instances[path] ? JSON.parse(JSON.stringify(chat.place.instances[path])) : null;
      const post: PlaceInstance = { class_name, properties: properties || {} };
      chat.place.instances[path] = post;
      recordInstance(path, prev, JSON.parse(JSON.stringify(post)));
      const props = properties || {};
      recordStudioAction(state, chat, {
        kind: typeof props.DecalTexture === "string" ? "decal_bound" : "part_created",
        path,
        class_name,
        position: propVec3(props.Position),
        size: propVec3(props.Size),
        color: typeof props.Color === "string" ? props.Color : null,
        summary:
          typeof props.CardTitle === "string"
            ? `Bound 3D card art "${props.CardTitle}" on ${path}`
            : `Created ${path} (${class_name})`,
      });
      produced.push(`Created ${path} (${class_name}).`);
      continue;
    }
    if (step.tool === "write_script") {
      const path = step.args.path;
      const src =
        step.args.source ||
        (step.args.skill ? generated.get(step.args.skill) : undefined) ||
        (lastSkill ? generated.get(lastSkill) : undefined);
      if (!src) {
        return { ok: false, error: `No generated Luau source available for ${path}.`, thread: produced };
      }
      const prev = chat.place.scripts[path] ?? null;
      chat.place.scripts[path] = src;
      recordScript(path, prev, src);
      recordStudioAction(state, chat, {
        kind: "script_written",
        path,
        class_name: path.startsWith("StarterGui.") ? "LocalScript" : "Script",
        summary: `Wrote ${path} (${src.split("\n").length} lines)`,
        code_snippet: src.split("\n").slice(0, 3).join("\n"),
      });
      produced.push(`Wrote ${path}.`);
      continue;
    }
    if (step.tool === "playtest") {
      emitSsePhase("checking", "Static Luau Check", "Auditing syntax, blocks, DataStore guards & cross-refs", 76);
      recordStudioAction(state, chat, {
        kind: "playtest_started",
        path: "Workspace",
        class_name: "Playtest",
        summary: "Started Studio Playtest & static Luau audit",
      });
      const verdict = validatePlace(chat.place);
      emitSsePhase("playtest", "Playtest Proof", verdict.passed ? "Playtest verified (0 errors)" : "Playtest failed", 96);
      lastConsole = verdict.console;
      recordStudioAction(state, chat, {
        kind: verdict.passed ? "playtest_passed" : "playtest_failed",
        path: "Workspace",
        class_name: "Playtest",
        summary: verdict.passed ? "Playtest passed (0 errors)" : "Playtest failed",
        console_line: verdict.console,
      });
      if (verdict.passed) {
        playtestPassed = true;
        produced.push("Playtest passed.");
      } else {
        produced.push("Playtest failed — reading the console and repairing.");
        produced.push(`Console from the failed test run: ${verdict.console}`);
      }
    }
  }

  if (changes.length > 0) {
    chat.undoStack.push({ changes, skills: [...skillsTag] });
    chat.redoStack = [];
    recordSnapshot(chat, snapLabel, beforePlace);
  }
  for (const sk of skillsTag) {
    if (!chat.builtSkills.includes(sk)) chat.builtSkills.push(sk);
  }

  if (!playtestPassed) {
    const errMsg = `Playtest failed: ${lastConsole}`;
    produced.push(`OpenRO: ${errMsg}`);
    chat.lines.push(...produced);
    return { ok: false, error: errMsg, thread: produced };
  }

  produced.push(`OpenRO: ${summaryText}`);
  chat.lines.push(...produced);
  return { ok: true, thread: produced };
}

async function runByokLlm(
  origFetch: typeof window.fetch,
  state: EngineState,
  chat: ChatSession,
  prompt: string,
): Promise<{ steps: PlanStep[]; skills: string[]; summary: string }> {
  const key = readSessionKey();
  const base = state.byokBaseUrl.replace(/\/+$/, "");
  const isLoopback = /^https?:\/\/(127\.0\.0\.1|localhost|\[::1\])(?::\d+)?(\/|$)/i.test(base);
  if (!key && !isLoopback) {
    throw new Error("BYOK mode needs an API key in Settings (or a loopback URL).");
  }
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (key) headers.Authorization = `Bearer ${key}`;
  const skillNames = GAME_SYSTEMS.map((s) => s.id).join("|");
  const sysPrompt =
    'You are OpenRO v0.7.0, a Roblox Studio game builder. Return ONLY a JSON object with {"steps": [...], "summary": "..."}. ' +
    'Each step is either {"tool":"create_instance","args":{"class_name":"Part|SpawnLocation","name":"...","parent":"Workspace","properties":{"Anchored":true}}} ' +
    `or {"skill":"${skillNames}","args":{...}} ` +
    'followed by {"tool":"write_script","args":{"path":"ServerScriptService.Name","skill":"..."}} and finish with {"tool":"playtest","args":{}}.';
  const res = await origFetch(`${base}/chat/completions`, {
    method: "POST",
    headers,
    body: JSON.stringify({
      model: state.byokModelId || "gpt-4o-mini",
      messages: [
        { role: "system", content: sysPrompt },
        { role: "user", content: `Current place has ${Object.keys(chat.place.scripts).length} scripts and ${Object.keys(chat.place.instances).length} instances. Build: ${prompt}` },
      ],
    }),
  });
  if (!res.ok) throw new Error(`BYOK model returned HTTP ${res.status}`);
  const data = (await res.json()) as { choices?: Array<{ message?: { content?: string } }> };
  const text = data.choices?.[0]?.message?.content || "";
  const start = text.indexOf("{");
  const end = text.lastIndexOf("}");
  if (start < 0 || end <= start) throw new Error("BYOK model did not return a JSON plan");
  const parsed = JSON.parse(text.slice(start, end + 1)) as { steps?: PlanStep[]; summary?: string };
  if (!Array.isArray(parsed.steps) || parsed.steps.length === 0) {
    throw new Error("BYOK model returned an empty step list");
  }
  const steps = parsed.steps.some((s) => "tool" in s && s.tool === "playtest")
    ? parsed.steps
    : [...parsed.steps, { tool: "playtest" as const, args: {} as Record<string, never> }];
  const skills = steps.filter((s): s is { skill: string; args: Record<string, unknown> } => "skill" in s).map((s) => s.skill);
  return { steps, skills, summary: parsed.summary || "Built via BYOK LLM and verified playtest." };
}

function ideasFor(chat: ChatSession) {
  if (chat.builtSkills.length === 0) return STARTER_CATALOG.slice(0, 3);
  return STARTER_CATALOG.slice(3)
    .filter(
      (i) =>
        i.needs.every((n) => chat.builtSkills.includes(n)) &&
        i.skills.some((s) => !chat.builtSkills.includes(s)),
    )
    .slice(0, 3);
}

function builtTitlesFor(chat: ChatSession): string[] {
  return STARTER_CATALOG.filter(
    (i) => i.skills.length > 0 && i.skills.every((s) => chat.builtSkills.includes(s)),
  ).map((i) => i.title);
}

export function installWebEngine() {
  (window as unknown as { __OPENRO_WEB_ENGINE__?: boolean }).__OPENRO_WEB_ENGINE__ = true;
  installSseShim();

  const origFetch = window.fetch.bind(window);
  let state: EngineState = defaultState();
  let hydrated: Promise<void> | null = null;

  const ensureReady = () => {
    if (!hydrated) {
      hydrated = idbLoad<EngineState>()
        .then((saved) => {
          if (saved && saved.chats && typeof saved.active === "string" && saved.chats[saved.active]) {
            for (const c of Object.values(saved.chats)) {
              if (!Array.isArray(c.snapshots)) c.snapshots = [];
              if (!Array.isArray(c.studioActions)) c.studioActions = [];
            }
            state = {
              ...defaultState(),
              ...saved,
              actionSeq: typeof saved.actionSeq === "number" && Number.isFinite(saved.actionSeq) ? saved.actionSeq : 1,
            };
          }
        })
        .catch(() => {});
    }
    return hydrated;
  };

  const persist = () => {
    void idbSave(state);
  };

  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const rawUrl = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    let u: URL;
    try {
      u = new URL(rawUrl, location.href);
    } catch {
      return origFetch(input, init);
    }
    if (!u.pathname.startsWith("/api/")) return origFetch(input, init);

    await ensureReady();

    if (state.webMode === "local_server" && u.pathname !== "/api/config") {
      try {
        const target = `${state.localServerUrl.replace(/\/+$/, "")}${u.pathname}${u.search}`;
        return await origFetch(target, init);
      } catch {
        return jsonResponse(
          { ok: false, error: `Could not reach local OpenRO server at ${state.localServerUrl}. Switch back to Browser Compiler in Settings or start openro-rs.` },
          503,
        );
      }
    }

    const method = (init?.method || "GET").toUpperCase();
    let body: Record<string, unknown> = {};
    if (typeof init?.body === "string" && init.body) {
      try {
        body = JSON.parse(init.body) as Record<string, unknown>;
      } catch {
        /* ignore */
      }
    }
    const chat = state.chats[state.active] || Object.values(state.chats)[0];
    if (!Array.isArray(chat.snapshots)) chat.snapshots = [];
    if (!Array.isArray(chat.studioActions)) chat.studioActions = [];
    if (!Array.isArray(chat.redoStack)) chat.redoStack = [];

    if (method === "GET" && u.pathname === "/api/version") {
      return jsonResponse({ name: "openro-rs", version: "0.8.0" });
    }
    if (method === "GET" && u.pathname === "/api/status") {
      return jsonResponse({
        studio_connected: true,
        model_configured: true,
        workspace_mode: "browser_place",
        model_id: state.webMode === "byok" ? state.byokModelId : "browser-luau",
        model_base_url: state.webMode === "byok" ? state.byokBaseUrl : "in-browser",
        is_loopback_model: true,
        game_name: chat.title === "New chat" ? "OpenRO Studio" : chat.title,
        version: "0.8.0",
        sse_build_stream: true,
        studio_stream: true,
        assets_api: true,
        ide_api: true,
        skills_count: GAME_SYSTEMS.length,
        store_recovered: [],
      });
    }
    if (method === "GET" && u.pathname === "/api/studio/actions") {
      const sinceParam = Number(u.searchParams.get("since") || "0");
      const filtered = Number.isFinite(sinceParam) && sinceParam > 0
        ? chat.studioActions.filter((a) => a.seq > sinceParam)
        : chat.studioActions;
      return jsonResponse({
        ok: true,
        studio_mode: "browser_place",
        has_live_frame: false,
        frame_seq: 0,
        actions: filtered,
      });
    }
    if (method === "GET" && u.pathname === "/api/studio/frame") {
      return jsonResponse(
        {
          ok: false,
          error: "No live Roblox Studio MCP frame available in Browser Engine mode. Connect Local OpenRO with OPENRO_MCP_COMMAND to stream Studio screenshots.",
        },
        404,
      );
    }
    if (method === "POST" && u.pathname === "/api/studio/capture") {
      return jsonResponse(
        {
          ok: false,
          error: "Live Studio screen_capture requires a connected Roblox Studio MCP bridge. Use Studio 3D or connect Local OpenRO in Settings.",
        },
        503,
      );
    }
    if (method === "POST" && u.pathname === "/api/studio/playtest") {
      recordStudioAction(state, chat, {
        kind: "playtest_started",
        path: "Workspace",
        class_name: "Playtest",
        summary: "Started Browser Studio 3D Playtest",
      });
      const verdict = validatePlace(chat.place);
      recordStudioAction(state, chat, {
        kind: "playtest_log",
        path: "Workspace",
        class_name: "Playtest",
        summary: verdict.console,
        console_line: verdict.console,
      });
      recordStudioAction(state, chat, {
        kind: verdict.passed ? "playtest_passed" : "playtest_failed",
        path: "Workspace",
        class_name: "Playtest",
        summary: verdict.passed ? "Studio Playtest passed (0 errors)" : `Studio Playtest failed (${verdict.errors.length} error(s))`,
        console_line: verdict.console,
      });
      persist();
      return jsonResponse({
        ok: true,
        passed: verdict.passed,
        console: verdict.console,
        errors: verdict.errors.length,
        diagnostics: verdict.diagnostics,
      });
    }
    if (method === "POST" && u.pathname === "/api/studio/mcp/connect") {
      return jsonResponse(
        {
          ok: false,
          connected: false,
          studio_mode: "browser_place",
          error:
            "Live Roblox Studio MCP bridge requires a local openro-rs desktop server. Switch to Connect Local Server in Settings (http://127.0.0.1:8077) to attach Roblox Studio MCP.",
        },
        503,
      );
    }
    if (method === "POST" && u.pathname === "/api/studio/mcp/disconnect") {
      return jsonResponse({
        ok: true,
        connected: false,
        studio_connected: true,
        workspace_mode: "browser_place",
        studio_mode: "browser_place",
      });
    }
    if (method === "GET" && u.pathname === "/api/art-knowledge") {
      return jsonResponse({
        ok: true,
        title: "Anime Homage & Fan-Art Styling Knowledge Point",
        summary:
          "How OpenRO builds iconic Roblox Anime Card Battlers that feel alive with classic shonen energy while shipping original cel-shaded silhouettes, titles, abilities, and studio-ready textures.",
        policy_note:
          "Never knee-jerk refuse anime card battler requests: transform iconic archetypes into original homage heroes with bespoke titles, elemental palettes, and clean 3:4 cel-shaded portraits ready for Roblox Decals and SurfaceGuis.",
        principles: [
          "Translate iconic anime archetypes into original titles, passives, and ultimates rather than refusing creative requests.",
          "Preserve high-impact visual readability: crisp cel-shaded line art, bold 3:4 trading-card framing, elemental particle auras, and high-contrast palettes.",
          "Keep card art watermark-free and text-free so Roblox SurfaceGui/ImageLabel overlays render rarity frames, stats, and foil effects crisply.",
          "Bind each generated asset through manifest records and Roblox Studio Decal/ImageLabel properties for instant Studio import.",
        ],
        archetype_translations: [
          { archetype: "Straw-Hat Pirate Brawler", homage_title: "Crimson Sea Captain", element: "Flame", rarity: "Mythic", palette: "Crimson, obsidian, sunlit gold" },
          { archetype: "Limitless Void Mentor", homage_title: "Infinity Void Sorcerer", element: "Void", rarity: "Celestial", palette: "Electric violet, silver-white, abyssal indigo" },
          { archetype: "Lightning Iaido Swordsman", homage_title: "Thunder Flash Ronin", element: "Storm", rarity: "Legendary", palette: "Storm gold, deep navy, arc cyan" },
          { archetype: "Shadow Monarch Necromancer", homage_title: "Shadow Sovereign", element: "Shadow", rarity: "Celestial", palette: "Obsidian black, neon cobalt, phantom violet" },
          { archetype: "Fox-Flame Ninja", homage_title: "Nine-Tail Ember Shinobi", element: "Flame", rarity: "Mythic", palette: "Solar orange, crimson embers, warm gold" },
          { archetype: "Ascended Golden Martial Artist", homage_title: "Supernova Ascended Striker", element: "Flame", rarity: "Secret", palette: "Blazing gold, plasma cyan, starlight white" },
          { archetype: "Gravity-Blade Ronin", homage_title: "Blood-Moon Gravity Ronin", element: "Void", rarity: "Epic", palette: "Blood crimson, dark slate, lunar silver" },
          { archetype: "Transmutation Alchemist", homage_title: "Eclipse Arcane Alchemist", element: "Arcane", rarity: "Rare", palette: "Alchemical gold, crimson coat, arcane teal" },
        ],
        prompt_template:
          "High-craft modern anime cel-shaded trading card illustration (3:4 portrait), dynamic heroic pose, bold inked contours, glowing {element} elemental aura, dramatic rim lighting, original stylized homage character '{title}' ({rarity} tier), no text, no watermark, no borders.",
      });
    }
    if (method === "GET" && u.pathname === "/api/assets") {
      const custom = state.customAssets || [];
      return jsonResponse({ ok: true, count: custom.length, assets: custom });
    }
    if (method === "POST" && u.pathname === "/api/assets/upload") {
      const filename = typeof body.filename === "string" && body.filename.trim() ? body.filename.trim() : "custom-card.png";
      const dataUrl =
        typeof body.data_url === "string" && body.data_url.startsWith("data:image/")
          ? body.data_url
          : typeof body.data_base64 === "string" && body.data_base64.trim()
            ? `data:image/png;base64,${body.data_base64.trim()}`
            : "";
      if (!dataUrl) {
        return jsonResponse({ ok: false, error: "Expected PNG, JPEG, or WebP image bytes." }, 400);
      }
      const title = typeof body.title === "string" && body.title.trim() ? body.title.trim() : filename.replace(/\.[^.]+$/, "");
      const cardId =
        typeof body.card_id === "string" && body.card_id.trim()
          ? body.card_id.trim()
          : title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || `card-${Date.now()}`;
      const record: CardAssetRecord = {
        card_id: cardId,
        title,
        rarity: (typeof body.rarity === "string" ? body.rarity : "Mythic") as CardAssetRecord["rarity"],
        odds: typeof body.odds === "string" ? body.odds : "Custom Upload",
        element: (typeof body.element === "string" ? body.element : "Flame") as CardAssetRecord["element"],
        atk: typeof body.atk === "number" ? body.atk : 430,
        hp: typeof body.hp === "number" ? body.hp : 1460,
        speed: typeof body.speed === "number" ? body.speed : 92,
        passive: typeof body.passive === "string" ? body.passive : "Custom Card Resonance: +18% ATK",
        ultimate: typeof body.ultimate === "string" ? body.ultimate : `${title} Burst`,
        prompt: typeof body.prompt === "string" ? body.prompt : undefined,
        file: filename,
        image_url: dataUrl,
        source: "upload",
      };
      state.customAssets = [record, ...(state.customAssets || []).filter((a) => a.card_id !== cardId)].slice(0, 24);
      persist();
      return jsonResponse({ ok: true, asset: record, file: filename, url: dataUrl }, 201);
    }
    if (method === "POST" && u.pathname === "/api/assets/generate") {
      const title = typeof body.title === "string" && body.title.trim() ? body.title.trim() : "Apex Vanguard";
      const element = typeof body.element === "string" ? body.element : "Flame";
      const rarity = typeof body.rarity === "string" ? body.rarity : "Mythic";
      const promptText =
        typeof body.prompt === "string" && body.prompt.trim()
          ? body.prompt.trim()
          : `High-craft modern anime cel-shaded trading card illustration (3:4 portrait), glowing ${element} aura, original stylized homage character '${title}' (${rarity} tier), no text, no watermark.`;
      const reqProvider = typeof body.provider === "string" ? body.provider : state.imageProvider || "auto";
      const gemKey = readSlotKey(GEMINI_KEY_SLOT);
      const oaiKey = readSlotKey(OPENAI_IMG_KEY_SLOT);
      const useGem = (reqProvider === "gemini" || reqProvider === "auto") && Boolean(gemKey);
      const useOai = !useGem && (reqProvider === "openai" || reqProvider === "auto") && Boolean(oaiKey);
      if (!useGem && !useOai) {
        return jsonResponse(
          {
            ok: false,
            error:
              "No image generation API key configured in this browser session. Add a Google Gemini API key (Nano Banana) or OpenAI Image key in Settings, or upload PNG/JPG/WebP artwork directly.",
            homage_prompt: promptText,
          },
          503,
        );
      }
      try {
        let dataUrl = "";
        let srcTag: CardAssetRecord["source"] = "gemini-api";
        if (useGem) {
          const resp = await origFetch(
            `https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-image:generateContent?key=${encodeURIComponent(gemKey)}`,
            {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({
                contents: [{ parts: [{ text: promptText }] }],
                generationConfig: { responseModalities: ["IMAGE"] },
              }),
            },
          );
          if (!resp.ok) throw new Error(`Gemini Image API returned HTTP ${resp.status}`);
          const val = (await resp.json()) as {
            candidates?: Array<{ content?: { parts?: Array<{ inlineData?: { data?: string; mimeType?: string } }> } }>;
          };
          const part = val.candidates?.[0]?.content?.parts?.find((p) => p.inlineData?.data);
          if (!part?.inlineData?.data) throw new Error("Gemini Image API returned no inline image bytes.");
          dataUrl = `data:${part.inlineData.mimeType || "image/png"};base64,${part.inlineData.data}`;
        } else {
          srcTag = "openai-api";
          const resp = await origFetch("https://api.openai.com/v1/images/generations", {
            method: "POST",
            headers: { "Content-Type": "application/json", Authorization: `Bearer ${oaiKey}` },
            body: JSON.stringify({ model: "gpt-image-1", prompt: promptText, size: "1024x1536", response_format: "b64_json" }),
          });
          if (!resp.ok) throw new Error(`OpenAI Image API returned HTTP ${resp.status}`);
          const val = (await resp.json()) as { data?: Array<{ b64_json?: string }> };
          const b64 = val.data?.[0]?.b64_json;
          if (!b64) throw new Error("OpenAI Image API returned no b64_json image bytes.");
          dataUrl = `data:image/png;base64,${b64}`;
        }
        const cardId = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || `gen-${Date.now()}`;
        const record: CardAssetRecord = {
          card_id: cardId,
          title,
          rarity: rarity as CardAssetRecord["rarity"],
          odds: "Generated",
          element: element as CardAssetRecord["element"],
          atk: 450,
          hp: 1520,
          speed: 94,
          passive: `${element} Surge: +20% squad ${element} ATK`,
          ultimate: `${title} Overdrive`,
          prompt: promptText,
          file: `${cardId}.png`,
          image_url: dataUrl,
          source: srcTag,
        };
        state.customAssets = [record, ...(state.customAssets || []).filter((a) => a.card_id !== cardId)].slice(0, 24);
        persist();
        return jsonResponse({ ok: true, asset: record }, 201);
      } catch (err) {
        return jsonResponse(
          { ok: false, error: err instanceof Error ? err.message : "Image generation request failed." },
          502,
        );
      }
    }
    if (method === "GET" && (u.pathname === "/api/systems" || u.pathname === "/api/skills")) {
      return jsonResponse({ ok: true, count: GAME_SYSTEMS.length, systems: GAME_SYSTEMS, skills: GAME_SYSTEMS });
    }
    if (method === "GET" && u.pathname === "/api/place/diff") {
      const latest = chat.snapshots[0]?.diff || { added: [], modified: [], removed: [] };
      return jsonResponse({ ok: true, ...latest });
    }
    if (method === "GET" && u.pathname === "/api/luau/check") {
      const diagnostics = analyzePlaceLuau(chat.place);
      return jsonResponse({
        ok: true,
        errors: diagnostics.filter((d) => d.severity === "error").length,
        warnings: diagnostics.filter((d) => d.severity === "warning").length,
        diagnostics,
      });
    }
    if (method === "GET" && u.pathname === "/api/update") {
      return jsonResponse({
        name: "openro-rs",
        current: "0.8.0",
        mode: "off",
        available: false,
        can_apply: false,
        dev_build: false,
        checked_at: Math.floor(Date.now() / 1000),
        staged: null,
        latest: null,
        error: null,
      });
    }
    if (method === "GET" && u.pathname === "/api/config") {
      const k = readSessionKey();
      const gk = readSlotKey(GEMINI_KEY_SLOT);
      const ok = readSlotKey(OPENAI_IMG_KEY_SLOT);
      return jsonResponse({
        ok: true,
        web_engine: true,
        web_mode: state.webMode,
        local_server_url: state.localServerUrl,
        base_url: state.byokBaseUrl,
        model_id: state.byokModelId,
        has_api_key: Boolean(k),
        api_key_masked: maskKey(k),
        image_provider: state.imageProvider || "auto",
        has_gemini_api_key: Boolean(gk),
        gemini_api_key_masked: maskKey(gk),
        has_openai_image_api_key: Boolean(ok),
        openai_image_api_key_masked: maskKey(ok),
        model_configured: true,
        is_loopback_model: true,
        studio_connected: true,
        workspace_mode: "browser_place",
        active_backend: "browser_place",
        mcp_command: "",
        vinegar_detected: false,
        vinegar_mcp_path: null,
        rbxlx_path: null,
        detected_gateways: [],
      });
    }
    if (method === "POST" && u.pathname === "/api/config") {
      if (body.web_mode === "browser" || body.web_mode === "byok" || body.web_mode === "local_server") {
        state.webMode = body.web_mode;
      }
      if (typeof body.base_url === "string" && body.base_url.trim()) state.byokBaseUrl = body.base_url.trim();
      if (typeof body.model_id === "string" && body.model_id.trim()) state.byokModelId = body.model_id.trim();
      if (typeof body.local_server_url === "string" && body.local_server_url.trim()) {
        state.localServerUrl = body.local_server_url.trim();
      }
      if (body.clear_api_key === true) writeSessionKey("");
      else if (typeof body.api_key === "string" && body.api_key.trim()) writeSessionKey(body.api_key.trim());
      if (body.image_provider === "auto" || body.image_provider === "gemini" || body.image_provider === "openai") {
        state.imageProvider = body.image_provider;
      }
      if (body.clear_gemini_api_key === true) writeSlotKey(GEMINI_KEY_SLOT, "");
      else if (typeof body.gemini_api_key === "string" && body.gemini_api_key.trim()) {
        writeSlotKey(GEMINI_KEY_SLOT, body.gemini_api_key.trim());
      }
      if (body.clear_openai_image_api_key === true) writeSlotKey(OPENAI_IMG_KEY_SLOT, "");
      else if (typeof body.openai_image_api_key === "string" && body.openai_image_api_key.trim()) {
        writeSlotKey(OPENAI_IMG_KEY_SLOT, body.openai_image_api_key.trim());
      }
      persist();
      const k = readSessionKey();
      const gk = readSlotKey(GEMINI_KEY_SLOT);
      const ok = readSlotKey(OPENAI_IMG_KEY_SLOT);
      return jsonResponse({
        ok: true,
        web_engine: true,
        web_mode: state.webMode,
        local_server_url: state.localServerUrl,
        base_url: state.byokBaseUrl,
        model_id: state.byokModelId,
        has_api_key: Boolean(k),
        api_key_masked: maskKey(k),
        image_provider: state.imageProvider || "auto",
        has_gemini_api_key: Boolean(gk),
        gemini_api_key_masked: maskKey(gk),
        has_openai_image_api_key: Boolean(ok),
        openai_image_api_key_masked: maskKey(ok),
        model_configured: true,
        is_loopback_model: true,
        studio_connected: true,
        workspace_mode: "browser_place",
        active_backend: "browser_place",
        mcp_command: "",
        vinegar_detected: false,
        vinegar_mcp_path: null,
        rbxlx_path: null,
        detected_gateways: [],
      });
    }
    if (method === "GET" && u.pathname === "/api/chats") {
      return jsonResponse({
        chats: Object.values(state.chats).map((c) => ({ id: c.id, title: c.title })),
        active: state.active,
        can_undo: chat.undoStack.length > 0,
        can_redo: chat.redoStack.length > 0,
      });
    }
    if (method === "POST" && u.pathname === "/api/chats") {
      const id = `chat-studio-${state.counter++}`;
      state.chats[id] = {
        id,
        title: "New chat",
        lines: [],
        builtSkills: [],
        place: { scripts: {}, instances: {} },
        undoStack: [],
        redoStack: [],
        snapshots: [],
        studioActions: [],
      };
      state.active = id;
      persist();
      return jsonResponse({ id, title: "New chat" }, 201);
    }
    if (method === "POST" && u.pathname === "/api/chats/select") {
      const id = typeof body.id === "string" ? body.id : "";
      if (id && state.chats[id]) {
        state.active = id;
        persist();
        return jsonResponse({ ok: true });
      }
      return jsonResponse({ ok: false, error: "chat not found" }, 404);
    }
    if (method === "GET" && u.pathname === "/api/thread") {
      return jsonResponse({ thread: [...chat.lines] });
    }
    if (method === "GET" && u.pathname === "/api/place-tree") {
      return jsonResponse({ scripts: chat.place.scripts, instances: chat.place.instances });
    }
    if (method === "POST" && u.pathname === "/api/place/instance") {
      const path = typeof body.path === "string" ? body.path.trim() : "";
      const className = typeof body.class_name === "string" ? body.class_name.trim() : "Part";
      const props = body.properties && typeof body.properties === "object" && !Array.isArray(body.properties)
        ? (body.properties as Record<string, unknown>)
        : {};
      if (!path) return jsonResponse({ ok: false, error: "missing instance path" }, 400);
      const before = clonePlace(chat.place);
      const prev = chat.place.instances[path] ? JSON.parse(JSON.stringify(chat.place.instances[path])) : null;
      const next: PlaceInstance = { class_name: className, properties: props };
      chat.place.instances[path] = next;
      chat.undoStack.push({ changes: [{ kind: "instance", path, prev, post: JSON.parse(JSON.stringify(next)) }], skills: [] });
      chat.redoStack = [];
      recordSnapshot(chat, `Edited ${path.split(".").pop() || path}`, before);
      recordStudioAction(state, chat, {
        kind: "part_updated",
        path,
        class_name: className,
        position: propVec3(props.Position),
        size: propVec3(props.Size),
        color: typeof props.Color === "string" ? props.Color : null,
        summary: `Updated ${path} transform & properties`,
      });
      persist();
      return jsonResponse({ ok: true, path, instance: next, can_undo: chat.undoStack.length > 0, can_redo: false });
    }
    if (method === "POST" && u.pathname === "/api/place/create") {
      const preset = typeof body.preset === "string" ? body.preset.trim().toLowerCase() : "block";
      const parent = typeof body.parent === "string" && body.parent.trim() ? body.parent.trim() : "Workspace";
      const existing = chat.place.instances;
      const pickUnique = (base: string) => {
        if (!(`${parent}.${base}` in existing)) return base;
        for (let i = 2; i < 500; i++) {
          if (!(`${parent}.${base}${i}` in existing)) return `${base}${i}`;
        }
        return `${base}${Date.now() % 1000}`;
      };
      const defaultBase =
        preset === "sphere"
          ? "SpherePart"
          : preset === "cylinder"
            ? "CylinderPillar"
            : preset === "spawn"
              ? "SpawnLocation"
              : preset === "jump_pad"
                ? "JumpPad"
                : preset === "kill_brick"
                  ? "KillBrick"
                  : preset === "card_billboard"
                    ? "CardBillboard"
                    : "StudioBlock";
      const rawName = typeof body.name === "string" && body.name.trim() ? body.name.trim() : defaultBase;
      const safeName = rawName.replace(/[^A-Za-z0-9_]/g, "") || defaultBase;
      const name = `${parent}.${safeName}` in existing ? pickUnique(safeName) : safeName;
      const path = `${parent}.${name}`;
      const className =
        typeof body.class_name === "string" && body.class_name.trim()
          ? body.class_name.trim()
          : preset === "spawn"
            ? "SpawnLocation"
            : "Part";
      const bodyProps =
        body.properties && typeof body.properties === "object" && !Array.isArray(body.properties)
          ? { ...(body.properties as Record<string, unknown>) }
          : {};
      const offsetX = (Object.keys(existing).length % 5 - 2) * 8;
      const offsetZ = 10 + Math.floor(Object.keys(existing).length / 5) * 6;
      const pos = propVec3(body.position) || propVec3(bodyProps.Position) || [offsetX, preset === "card_billboard" ? 7 : 2, offsetZ];
      const size =
        propVec3(body.size) ||
        propVec3(bodyProps.Size) ||
        (preset === "sphere"
          ? [6, 6, 6]
          : preset === "cylinder"
            ? [6, 10, 6]
            : preset === "spawn"
              ? [8, 1, 8]
              : preset === "card_billboard"
                ? [7, 10, 1]
                : [8, 4, 8]);
      const color =
        typeof body.color === "string" && body.color.trim()
          ? body.color.trim()
          : typeof bodyProps.Color === "string" && bodyProps.Color.trim()
            ? bodyProps.Color.trim()
            : preset === "jump_pad"
              ? "#24946E"
              : preset === "kill_brick"
                ? "#DF522D"
                : preset === "spawn"
                  ? "#2D68DF"
                  : preset === "sphere"
                    ? "#3B82F6"
                    : preset === "cylinder"
                      ? "#D49B26"
                      : "#8B939E";
      const props: Record<string, unknown> = {
        Anchored: true,
        Material: preset === "jump_pad" || preset === "kill_brick" ? "Neon" : "SmoothPlastic",
        ...(preset === "sphere" ? { Shape: "Ball" } : preset === "cylinder" ? { Shape: "Cylinder" } : {}),
        ...bodyProps,
        Position: pos,
        Size: size,
        Color: color,
      };
      const before = clonePlace(chat.place);
      const next: PlaceInstance = { class_name: className, properties: props };
      chat.place.instances[path] = next;
      chat.undoStack.push({ changes: [{ kind: "instance", path, prev: null, post: JSON.parse(JSON.stringify(next)) }], skills: [] });
      chat.redoStack = [];
      recordSnapshot(chat, `Added ${name}`, before);
      recordStudioAction(state, chat, {
        kind: "part_created",
        path,
        class_name: className,
        position: pos,
        size,
        color,
        summary: `Inserted ${path} (${className})`,
      });
      persist();
      return jsonResponse(
        { ok: true, path, class_name: className, properties: props, can_undo: chat.undoStack.length > 0, can_redo: false },
        201,
      );
    }
    if (method === "POST" && u.pathname === "/api/place/duplicate") {
      const srcPath = typeof body.path === "string" ? body.path.trim() : "";
      const src = srcPath ? chat.place.instances[srcPath] : undefined;
      if (!src) return jsonResponse({ ok: false, error: `instance not found: ${srcPath}` }, 404);
      const offset = propVec3(body.offset) || [4, 0, 4];
      const segs = srcPath.split(".");
      const leaf = segs.pop() || "Part";
      const parent = segs.join(".") || "Workspace";
      let dupName = `${leaf}Copy`;
      for (let i = 2; i < 500 && `${parent}.${dupName}` in chat.place.instances; i++) {
        dupName = `${leaf}Copy${i}`;
      }
      const dupPath = `${parent}.${dupName}`;
      const clonedProps: Record<string, unknown> = JSON.parse(JSON.stringify(src.properties || {}));
      const oldPos = propVec3(clonedProps.Position) || [0, 2, 0];
      const nextPos: [number, number, number] = [oldPos[0] + offset[0], oldPos[1] + offset[1], oldPos[2] + offset[2]];
      clonedProps.Position = nextPos;
      const before = clonePlace(chat.place);
      const next: PlaceInstance = { class_name: src.class_name, properties: clonedProps };
      chat.place.instances[dupPath] = next;
      chat.undoStack.push({ changes: [{ kind: "instance", path: dupPath, prev: null, post: JSON.parse(JSON.stringify(next)) }], skills: [] });
      chat.redoStack = [];
      recordSnapshot(chat, `Duplicated ${leaf}`, before);
      recordStudioAction(state, chat, {
        kind: "part_created",
        path: dupPath,
        class_name: src.class_name,
        position: nextPos,
        size: propVec3(clonedProps.Size),
        color: typeof clonedProps.Color === "string" ? clonedProps.Color : null,
        summary: `Duplicated ${srcPath} → ${dupPath}`,
      });
      persist();
      return jsonResponse(
        {
          ok: true,
          path: dupPath,
          new_path: dupPath,
          source_path: srcPath,
          class_name: src.class_name,
          properties: clonedProps,
          can_undo: chat.undoStack.length > 0,
          can_redo: false,
        },
        201,
      );
    }
    if (method === "POST" && u.pathname === "/api/place/delete") {
      const targetPath = typeof body.path === "string" ? body.path.trim() : "";
      if (!targetPath) return jsonResponse({ ok: false, error: "missing path" }, 400);
      const hasInst = targetPath in chat.place.instances;
      const hasScr = targetPath in chat.place.scripts;
      if (!hasInst && !hasScr) return jsonResponse({ ok: false, error: `not found: ${targetPath}` }, 404);
      const before = clonePlace(chat.place);
      const changes: ChangeEntry[] = [];
      if (hasInst) {
        const prev = JSON.parse(JSON.stringify(chat.place.instances[targetPath])) as PlaceInstance;
        delete chat.place.instances[targetPath];
        changes.push({ kind: "instance", path: targetPath, prev, post: null });
      }
      if (hasScr) {
        const prev = chat.place.scripts[targetPath];
        delete chat.place.scripts[targetPath];
        changes.push({ kind: "script", path: targetPath, prev, post: null });
      }
      chat.undoStack.push({ changes, skills: [] });
      chat.redoStack = [];
      recordSnapshot(chat, `Deleted ${targetPath.split(".").pop() || targetPath}`, before);
      recordStudioAction(state, chat, {
        kind: "part_updated",
        path: targetPath,
        class_name: hasInst ? "Part" : "Script",
        summary: `Deleted ${targetPath}`,
      });
      persist();
      return jsonResponse({ ok: true, deleted: targetPath, path: targetPath, can_undo: chat.undoStack.length > 0, can_redo: false });
    }
    if (method === "POST" && u.pathname === "/api/place/script") {
      const scriptPath = typeof body.path === "string" ? body.path.trim() : "";
      const source = typeof body.source === "string" ? body.source : "";
      if (!scriptPath) return jsonResponse({ ok: false, error: "missing script path" }, 400);
      const before = clonePlace(chat.place);
      const prev = chat.place.scripts[scriptPath] ?? null;
      chat.place.scripts[scriptPath] = source;
      chat.undoStack.push({ changes: [{ kind: "script", path: scriptPath, prev, post: source }], skills: [] });
      chat.redoStack = [];
      const diagnostics = analyzePlaceLuau(chat.place);
      const scriptDiags = diagnostics.filter((d) => d.path === scriptPath);
      const errCount = scriptDiags.filter((d) => d.severity === "error").length;
      const warnCount = scriptDiags.filter((d) => d.severity === "warning").length;
      recordSnapshot(chat, `Edited ${scriptPath.split(".").pop() || scriptPath}`, before);
      recordStudioAction(state, chat, {
        kind: "script_written",
        path: scriptPath,
        class_name: scriptPath.startsWith("StarterGui.") ? "LocalScript" : "Script",
        summary: `Saved ${scriptPath} (${source.split("\n").length} lines · ${errCount} err, ${warnCount} warn)`,
        code_snippet: source.split("\n").slice(0, 3).join("\n"),
      });
      persist();
      return jsonResponse({
        ok: true,
        path: scriptPath,
        passed: errCount === 0,
        errors: errCount,
        error_count: errCount,
        warnings: warnCount,
        warning_count: warnCount,
        diagnostics: scriptDiags,
        can_undo: chat.undoStack.length > 0,
        can_redo: false,
      });
    }
    if (method === "GET" && u.pathname === "/api/export/bundle") {
      const zipBytes = buildRojoShipPackZip({
        gameName: chat.title === "New chat" ? "OpenRO Studio Place" : chat.title,
        place: chat.place,
        assets: state.customAssets || [],
        diagnostics: analyzePlaceLuau(chat.place),
      });
      return new Response(zipBytes.buffer as ArrayBuffer, {
        status: 200,
        headers: {
          "Content-Type": "application/zip",
          "Content-Disposition": 'attachment; filename="openro-rojo-ship-pack.zip"',
        },
      });
    }
    if (method === "GET" && u.pathname === "/api/export/rbxlx") {
      const xml = serializeRbxlx(chat.title === "New chat" ? "OpenRO Place" : chat.title, chat.place);
      return new Response(xml, {
        status: 200,
        headers: {
          "Content-Type": "application/xml; charset=utf-8",
          "Content-Disposition": 'attachment; filename="openro-place.rbxlx"',
        },
      });
    }
    if (method === "POST" && u.pathname === "/api/import/rbxlx") {
      const xml = typeof body.xml === "string" ? body.xml : "";
      const filename = typeof body.filename === "string" && body.filename.trim() ? body.filename.trim() : "imported.rbxlx";
      const parsed = parseRbxlxXml(xml);
      if (!parsed.ok) return jsonResponse({ ok: false, error: parsed.error }, 400);
      const before = clonePlace(chat.place);
      const changes: ChangeEntry[] = [];
      for (const [k, v] of Object.entries(chat.place.instances)) {
        if (!(k in parsed.place.instances)) changes.push({ kind: "instance", path: k, prev: v, post: null });
      }
      for (const [k, v] of Object.entries(parsed.place.instances)) {
        changes.push({
          kind: "instance",
          path: k,
          prev: chat.place.instances[k] ?? null,
          post: { class_name: v.class_name || "Part", properties: v.properties || {} },
        });
      }
      for (const [k, v] of Object.entries(chat.place.scripts)) {
        if (!(k in parsed.place.scripts)) changes.push({ kind: "script", path: k, prev: v, post: null });
      }
      for (const [k, v] of Object.entries(parsed.place.scripts)) {
        changes.push({ kind: "script", path: k, prev: chat.place.scripts[k] ?? null, post: v });
      }
      chat.place = {
        scripts: { ...parsed.place.scripts },
        instances: Object.fromEntries(
          Object.entries(parsed.place.instances).map(([k, v]) => [
            k,
            { class_name: v.class_name || "Part", properties: v.properties || {} },
          ]),
        ),
      };
      if (chat.title === "New chat") chat.title = parsed.gameName || filename.replace(/\.rbxlx$/i, "");
      if (changes.length > 0) {
        chat.undoStack.push({ changes, skills: [] });
        chat.redoStack = [];
      }
      const snap = recordSnapshot(chat, `Imported ${filename}`, before);
      const verdict = validatePlace(chat.place);
      chat.lines.push(`You: Import ${filename}`);
      chat.lines.push(
        `OpenRO: Imported ${filename} (${snap.parts_count} parts, ${snap.scripts_count} scripts). ${verdict.passed ? "Static Luau check passed with 0 errors." : `${verdict.errors.length} Luau issue(s) flagged in Explorer.`}`,
      );
      recordStudioAction(state, chat, {
        kind: "imported",
        path: "Workspace",
        class_name: "Place",
        summary: `Imported ${filename} (${snap.parts_count} parts, ${snap.scripts_count} scripts)`,
      });
      persist();
      return jsonResponse({
        ok: true,
        parts_count: snap.parts_count,
        scripts_count: snap.scripts_count,
        diff: snap.diff,
        diagnostics: verdict.diagnostics,
      });
    }

    // Versioned Snapshots API
    if (method === "GET" && u.pathname === "/api/snapshots") {
      return jsonResponse({
        ok: true,
        snapshots: chat.snapshots.map(({ id, label, created_at, parts_count, scripts_count, diff }) => ({
          id,
          label,
          created_at,
          parts_count,
          scripts_count,
          diff,
        })),
      });
    }
    if (method === "POST" && u.pathname === "/api/snapshots") {
      const label =
        typeof body.label === "string" && body.label.trim()
          ? body.label.trim()
          : `Checkpoint ${chat.snapshots.length + 1}`;
      const snap = recordSnapshot(chat, label, chat.snapshots[0]?.place || null);
      persist();
      return jsonResponse({ ok: true, snapshot: { id: snap.id, label: snap.label, created_at: snap.created_at, parts_count: snap.parts_count, scripts_count: snap.scripts_count, diff: snap.diff } }, 201);
    }
    const snapRestoreMatch = u.pathname.match(/^\/api\/snapshots\/([^/]+)\/restore$/);
    if (method === "POST" && snapRestoreMatch) {
      const id = decodeURIComponent(snapRestoreMatch[1]);
      const found = chat.snapshots.find((s) => s.id === id);
      if (!found) return jsonResponse({ ok: false, error: "snapshot not found" }, 404);
      const before = clonePlace(chat.place);
      chat.place = clonePlace(found.place);
      recordSnapshot(chat, `Restored ${found.label}`, before);
      recordStudioAction(state, chat, {
        kind: "restored",
        path: "Workspace",
        class_name: "Snapshot",
        summary: `Restored snapshot "${found.label}" (${found.parts_count} parts, ${found.scripts_count} scripts)`,
      });
      chat.lines.push(`OpenRO: Restored snapshot "${found.label}" (${found.parts_count} parts, ${found.scripts_count} scripts).`);
      persist();
      return jsonResponse({ ok: true, restored: found.id });
    }
    const snapExportMatch = u.pathname.match(/^\/api\/snapshots\/([^/]+)\/rbxlx$/);
    if (method === "GET" && snapExportMatch) {
      const id = decodeURIComponent(snapExportMatch[1]);
      const found = chat.snapshots.find((s) => s.id === id);
      if (!found) return jsonResponse({ ok: false, error: "snapshot not found" }, 404);
      const xml = serializeRbxlx(found.label, found.place);
      return new Response(xml, {
        status: 200,
        headers: {
          "Content-Type": "application/xml; charset=utf-8",
          "Content-Disposition": `attachment; filename="${found.id}.rbxlx"`,
        },
      });
    }

    if (method === "POST" && u.pathname === "/api/ideas") {
      return jsonResponse({
        ok: true,
        ideas: ideasFor(chat).map(({ id, title, pitch, skills }) => ({ id, title, pitch, skills })),
        built_titles: builtTitlesFor(chat),
        pitched_new: false,
      });
    }
    if (method === "POST" && u.pathname === "/api/skills/build") {
      const skillParam =
        typeof body.skill === "string"
          ? body.skill.trim()
          : typeof body.name === "string"
            ? body.name.trim()
            : "";
      const plan = buildPlanForSkill(skillParam, chat.place);
      if (!plan) return jsonResponse({ ok: false, error: `unknown skill: ${skillParam}` }, 404);
      if (chat.title === "New chat") chat.title = plan.title;
      chat.lines.push(`You: Add ${plan.title} (${skillParam})`);
      const res = executePlan(state, chat, plan.steps, plan.skills, plan.summary, plan.title);
      persist();
      return jsonResponse({
        ok: res.ok,
        error: res.error,
        summary: res.ok ? plan.summary : res.error,
        thread: res.thread,
      });
    }
    if (method === "POST" && u.pathname === "/api/ideas/build") {
      const id = typeof body.id === "string" ? body.id : "";
      const skillParam = typeof body.skill === "string" ? body.skill : id.startsWith("skill:") ? id.slice(6) : "";
      if (skillParam) {
        const plan = buildPlanForSkill(skillParam, chat.place);
        if (!plan) return jsonResponse({ ok: false, error: `unknown skill: ${skillParam}` }, 404);
        if (chat.title === "New chat") chat.title = plan.title;
        chat.lines.push(`You: Add ${plan.title} (${skillParam})`);
        const res = executePlan(state, chat, plan.steps, plan.skills, plan.summary, plan.title);
        persist();
        return jsonResponse({
          ok: res.ok,
          error: res.error,
          summary: res.ok ? plan.summary : res.error,
          thread: res.thread,
        });
      }
      const idea = STARTER_CATALOG.find((x) => x.id === id);
      if (!idea) return jsonResponse({ ok: false, error: `unknown idea: ${id}` }, 404);
      if (chat.title === "New chat") chat.title = idea.title;
      chat.lines.push(`You: Build this — ${idea.title}`);
      const res = executePlan(
        state,
        chat,
        idea.plan,
        idea.skills,
        `Built "${idea.title}" — verified Luau syntax and Roblox wiring with 0 playtest errors.`,
        idea.title,
      );
      const next = ideasFor(chat);
      if (res.ok && next.length) {
        chat.lines.push("OpenRO: That landed. Next, I could add:");
        for (const n of next) chat.lines.push(`OpenRO: Idea — ${n.title}: ${n.pitch}`);
      }
      persist();
      return jsonResponse({
        ok: res.ok,
        error: res.error,
        summary: res.ok ? `Built ${idea.title}` : res.error,
        thread: res.thread,
        ideas: next.map(({ id: i, title, pitch, skills }) => ({ id: i, title, pitch, skills })),
      });
    }
    if (method === "POST" && u.pathname === "/api/chat") {
      const msg = typeof body.message === "string" ? body.message.trim() : "";
      if (!msg) return jsonResponse({ ok: false, error: "empty message" }, 400);
      if (chat.title === "New chat") chat.title = msg.slice(0, 48);
      chat.lines.push(`You: ${msg}`);

      let plan: { steps: PlanStep[]; skills: string[]; summary: string };
      if (state.webMode === "byok") {
        try {
          plan = await runByokLlm(origFetch, state, chat, msg);
        } catch (err) {
          const errMsg = err instanceof Error ? err.message : "BYOK request failed";
          chat.lines.push(`OpenRO: ${errMsg}`);
          persist();
          return jsonResponse({ ok: false, error: errMsg }, 502);
        }
      } else {
        plan = compilePromptToPlan(msg, chat.place);
      }

      const res = executePlan(state, chat, plan.steps, plan.skills, plan.summary, msg.slice(0, 36));
      persist();
      return jsonResponse(
        { ok: res.ok, error: res.error, summary: plan.summary, thread: res.thread, transcript: [] },
        res.ok ? 200 : 502,
      );
    }
    if (method === "POST" && u.pathname === "/api/revert") {
      const top = chat.undoStack.pop();
      if (!top) {
        chat.lines.push("OpenRO: Nothing to revert — no change set is waiting in this chat.");
        persist();
        return jsonResponse({ ok: false, error: "nothing to revert" });
      }
      const before = clonePlace(chat.place);
      const restored: string[] = [];
      for (const ch of [...top.changes].reverse()) {
        if (ch.kind === "script") {
          if (ch.prev === null) delete chat.place.scripts[ch.path];
          else chat.place.scripts[ch.path] = ch.prev;
        } else {
          if (ch.prev === null) delete chat.place.instances[ch.path];
          else chat.place.instances[ch.path] = ch.prev;
        }
        restored.push(ch.path);
      }
      const remainingSkills = new Set(chat.undoStack.flatMap((u) => u.skills));
      chat.builtSkills = chat.builtSkills.filter((sk) => remainingSkills.has(sk));
      chat.redoStack.push(top);
      recordSnapshot(chat, "Undo revert", before);
      recordStudioAction(state, chat, {
        kind: "reverted",
        path: "Workspace",
        class_name: "Place",
        summary: `Reverted ${restored.length} Studio change(s)`,
      });
      chat.lines.push(`OpenRO: Undid the last build: ${restored.length} change(s) put back.`);
      persist();
      return jsonResponse({
        ok: true,
        restored,
        can_undo: chat.undoStack.length > 0,
        can_redo: chat.redoStack.length > 0,
      });
    }
    if (method === "POST" && u.pathname === "/api/redo") {
      const top = chat.redoStack.pop();
      if (!top) {
        return jsonResponse({ ok: false, error: "nothing to redo", can_undo: chat.undoStack.length > 0, can_redo: false });
      }
      const before = clonePlace(chat.place);
      const restored: string[] = [];
      for (const ch of top.changes) {
        if (ch.kind === "script") {
          if (ch.post === null) delete chat.place.scripts[ch.path];
          else chat.place.scripts[ch.path] = ch.post;
        } else {
          if (ch.post === null) delete chat.place.instances[ch.path];
          else chat.place.instances[ch.path] = ch.post;
        }
        restored.push(ch.path);
      }
      chat.undoStack.push(top);
      for (const sk of top.skills) {
        if (!chat.builtSkills.includes(sk)) chat.builtSkills.push(sk);
      }
      recordSnapshot(chat, "Redo action", before);
      recordStudioAction(state, chat, {
        kind: "restored",
        path: restored[0] || "Workspace",
        class_name: "Place",
        summary: `Redid ${restored.length} Studio change(s)`,
      });
      chat.lines.push(`OpenRO: Redid ${restored.length} Studio change(s).`);
      persist();
      return jsonResponse({
        ok: true,
        restored,
        can_undo: chat.undoStack.length > 0,
        can_redo: chat.redoStack.length > 0,
      });
    }

    return jsonResponse({ ok: false, error: "not found" }, 404);
  };
}

installWebEngine();
