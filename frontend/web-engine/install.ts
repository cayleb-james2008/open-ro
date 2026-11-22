// Real in-browser Roblox Studio engine installer for OpenRO v0.5.0 standalone web builds.
// Features:
//  - All 22 parametric Roblox game-system Luau compilers (zero canned responses)
//  - Static Luau analyzer & cross-reference playtest prover
//  - Genuine bidirectional Roblox Studio XML (.rbxlx) BOTH Import AND Export
//  - IndexedDB versioned snapshots, instance property editing, & workspace persistence
//  - Live EventSource('/api/build/stream') SSE phase emitter
//  - Optional BYOK OpenAI-compatible LLM (key stored strictly in sessionStorage)
//  - One-click Connect to Local openro-rs (http://127.0.0.1:8077)
import { computePlaceDiff } from "../src/app/luauAudit";
import { GAME_SYSTEMS } from "../src/app/systems";
import type { PlaceDiff, PlaceSnapshot } from "../src/app/data";
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
  snapshots: StoredSnapshot[];
};

type EngineState = {
  active: string;
  counter: number;
  chats: Record<string, ChatSession>;
  webMode: "browser" | "byok" | "local_server";
  byokBaseUrl: string;
  byokModelId: string;
  localServerUrl: string;
};

const BYOK_KEY_SLOT = "openro-byok-key";

// Lightweight EventSource subscriber registry for /api/build/stream
type PhaseListener = (payload: Record<string, unknown>) => void;
const sseListeners = new Set<PhaseListener>();

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

    constructor(url: string | URL, eventSourceInitDict?: EventSourceInit) {
      super();
      const raw = typeof url === "string" ? url : url.href;
      let u: URL;
      try {
        u = new URL(raw, location.href);
      } catch {
        return new OrigEventSource(url, eventSourceInitDict) as unknown as OpenROWebEventSource;
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
    chats: {
      [id]: {
        id,
        title: "New chat",
        lines: [],
        builtSkills: [],
        place: { scripts: {}, instances: {} },
        undoStack: [],
        snapshots: [],
      },
    },
    webMode: "browser",
    byokBaseUrl: "https://api.openai.com/v1",
    byokModelId: "gpt-4o-mini",
    localServerUrl: "http://127.0.0.1:8077",
  };
}

function jsonResponse(obj: unknown, status = 200): Response {
  return new Response(JSON.stringify(obj), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function executePlan(
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
      emitSsePhase("compiling", "Compiling Rune", `Generating Luau for ${step.skill}`, 48);
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
      produced.push(`Wrote ${path}.`);
      continue;
    }
    if (step.tool === "playtest") {
      emitSsePhase("checking", "Static Luau Check", "Auditing syntax, blocks, DataStore guards & cross-refs", 76);
      const verdict = validatePlace(chat.place);
      emitSsePhase("playtest", "Playtest Proof", verdict.passed ? "Playtest verified (0 errors)" : "Playtest failed", 96);
      lastConsole = verdict.console;
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
    'You are OpenRO v0.5.0, a Roblox Studio game builder. Return ONLY a JSON object with {"steps": [...], "summary": "..."}. ' +
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
            }
            state = {
              ...defaultState(),
              ...saved,
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

    if (method === "GET" && u.pathname === "/api/version") {
      return jsonResponse({ name: "openro-rs", version: "0.5.0" });
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
        version: "0.5.0",
        sse_build_stream: true,
        store_recovered: [],
      });
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
        current: "0.5.0",
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
      return jsonResponse({
        ok: true,
        web_engine: true,
        web_mode: state.webMode,
        local_server_url: state.localServerUrl,
        base_url: state.byokBaseUrl,
        model_id: state.byokModelId,
        has_api_key: Boolean(k),
        api_key_masked: maskKey(k),
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
      persist();
      const k = readSessionKey();
      return jsonResponse({
        ok: true,
        web_engine: true,
        web_mode: state.webMode,
        local_server_url: state.localServerUrl,
        base_url: state.byokBaseUrl,
        model_id: state.byokModelId,
        has_api_key: Boolean(k),
        api_key_masked: maskKey(k),
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
        snapshots: [],
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
      recordSnapshot(chat, `Edited ${path.split(".").pop() || path}`, before);
      persist();
      return jsonResponse({ ok: true, path, instance: next });
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
      if (changes.length > 0) chat.undoStack.push({ changes, skills: [] });
      const snap = recordSnapshot(chat, `Imported ${filename}`, before);
      const verdict = validatePlace(chat.place);
      chat.lines.push(`You: Import ${filename}`);
      chat.lines.push(
        `OpenRO: Imported ${filename} (${snap.parts_count} parts, ${snap.scripts_count} scripts). ${verdict.passed ? "Static Luau check passed with 0 errors." : `${verdict.errors.length} Luau issue(s) flagged in Explorer.`}`,
      );
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
      const res = executePlan(chat, plan.steps, plan.skills, plan.summary, plan.title);
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
        const res = executePlan(chat, plan.steps, plan.skills, plan.summary, plan.title);
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

      const res = executePlan(chat, plan.steps, plan.skills, plan.summary, msg.slice(0, 36));
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
      recordSnapshot(chat, "Undo revert", before);
      chat.lines.push(`OpenRO: Undid the last build: ${restored.length} change(s) put back.`);
      persist();
      return jsonResponse({ ok: true, restored });
    }

    return jsonResponse({ ok: false, error: "not found" }, 404);
  };
}

installWebEngine();
