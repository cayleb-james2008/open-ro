// build-demo.mjs: packages ../assets/index.html into a standalone, zero-network static fixture demo
// (../demo-dist/index.html and optional DEMO_EXPORT_DIR/index.html) with a client-side /api/* mock
// seeded with a realistic Roblox Obby + Tycoon Shop build and visible `fixture demo` pill (House Rule 7).
import fs from "node:fs";
import path from "node:path";

const feRoot = path.resolve(import.meta.dirname, "..");
const repoRoot = path.resolve(feRoot, "..");
const srcHtml = path.join(repoRoot, "assets", "index.html");

if (!fs.existsSync(srcHtml)) {
  throw new Error(`Missing ${srcHtml} — run npm run build first.`);
}

const DEMO_MOCK_SCRIPT = `<script>
(function(){
  window.__OPENRO_DEMO__ = true;
  const CATALOG = [
    { id: "obby-starter", title: "Lava Obby", pitch: "A lava obby that plays fair: a spawn pad, checkpoints that remember your stage so a fall doesn't restart you, lava bricks that burn, and coins that keep paying while you climb — and save between sessions.", skills: ["spawn_link", "checkpoints", "kill_brick", "currency", "day_night"], needs: [] },
    { id: "coin-tycoon", title: "Coin Tycoon", pitch: "A tycoon starter with a real economy: money that saves between sessions and keeps paying while you play, a shop selling a Speed Coil, and a proper game screen showing your balance with the shop button on it.", skills: ["currency", "shop", "game_gui", "day_night"], needs: [] },
    { id: "round-arena", title: "Round Arena", pitch: "A round-based arena: rounds that start themselves, a leaderboard counting wins, a spawn pad, and a lava pit in the middle for sudden death.", skills: ["round_manager", "leaderboard", "spawn_link", "kill_brick"], needs: [] },
    { id: "add-day-night", title: "A living sky", pitch: "Add a day/night cycle so the place breathes — the light turns through a full day every few minutes.", skills: ["day_night"], needs: [] },
    { id: "add-soundtrack", title: "A soundtrack", pitch: "Wire up the music: a soundtrack slot in SoundService, looping, volume set — silent until you pick a real audio id, then it's just there.", skills: ["soundtrack"], needs: [] },
    { id: "add-shop", title: "A shop for the money", pitch: "Your game has money now — give players something to spend it on: a shop selling a Speed Coil, plus the game screen with the balance and the shop button on it.", skills: ["shop", "game_gui"], needs: ["currency"] }
  ];

  const SEED_SCRIPTS = {
    "ServerScriptService.SpawnLink": "local Players = game:GetService(\\"Players\\")\\nlocal pad = workspace:WaitForChild(\\"SpawnPad\\")\\nPlayers.PlayerAdded:Connect(function(p) p.RespawnLocation = pad end)",
    "ServerScriptService.Checkpoints": "local Players = game:GetService(\\"Players\\")\\nlocal folder = workspace:WaitForChild(\\"Checkpoints\\")\\nfor stage = 1, 6 do\\n  local pad = folder:FindFirstChild(\\"Stage\\" .. stage)\\nend",
    "Workspace.LavaBrick.KillBrick": "script.Parent.Touched:Connect(function(hit)\\n  local hum = hit.Parent and hit.Parent:FindFirstChildOfClass(\\"Humanoid\\")\\n  if hum then hum:TakeDamage(25) end\\nend)",
    "ServerScriptService.Currency": "local DataStoreService = game:GetService(\\"DataStoreService\\")\\nlocal store = DataStoreService:GetDataStore(\\"OpenRO_Coins_v1\\")\\ngame:BindToClose(function() end)",
    "ServerScriptService.Shop": "local ReplicatedStorage = game:GetService(\\"ReplicatedStorage\\")\\nlocal buy = Instance.new(\\"RemoteEvent\\", ReplicatedStorage)\\nbuy.Name = \\"BuySpeedCoil\\"",
    "StarterGui.GameGui": "local gui = Instance.new(\\"ScreenGui\\")\\ngui.Name = \\"TycoonHud\\"\\ngui.ResetOnSpawn = false"
  };

  const SEED_INSTANCES = {
    "Workspace.SpawnPad": { class_name: "SpawnLocation", properties: { Anchored: true, Neutral: true } },
    "Workspace.Checkpoints": { class_name: "Folder", properties: { Stages: 6 } },
    "Workspace.LavaBrick": { class_name: "Part", properties: { Anchored: true, BrickColor: "Bright orange" } }
  };

  const state = {
    active: "chat-obby-1",
    counter: 3,
    chats: {
      "chat-obby-1": {
        id: "chat-obby-1",
        title: "Skyline Obby + Speed Shop",
        built: ["spawn_link", "checkpoints", "kill_brick", "currency", "shop", "game_gui"],
        builtTitles: ["Lava Obby", "A shop for the money"],
        undoStack: [
          {
            scripts: ["ServerScriptService.Shop", "StarterGui.GameGui"],
            instances: [],
            skills: ["shop", "game_gui"],
            title: "A shop for the money"
          }
        ],
        scripts: Object.assign({}, SEED_SCRIPTS),
        instances: JSON.parse(JSON.stringify(SEED_INSTANCES)),
        lines: [
          "You: Build a 6-stage Skyline Lava Obby with saved Coins and a Speed Coil shop HUD",
          "Generated the spawn_link script.",
          "Wrote ServerScriptService.SpawnLink.",
          "Created Workspace.SpawnPad (SpawnLocation).",
          "Generated the checkpoints script.",
          "Wrote ServerScriptService.Checkpoints.",
          "Created Workspace.Checkpoints (Folder).",
          "Created Workspace.LavaBrick (Part).",
          "Generated the kill_brick script.",
          "Wrote Workspace.LavaBrick.KillBrick.",
          "Generated the currency script.",
          "Wrote ServerScriptService.Currency.",
          "Generated the shop script.",
          "Wrote ServerScriptService.Shop.",
          "Generated the game_gui script.",
          "Wrote StarterGui.GameGui.",
          "Playtest passed.",
          "OpenRO: Built your Skyline Lava Obby and Speed Coil Shop — 6 checkpoint pads save stage progress, Coins persist via DataStore with BindToClose, and the shop HUD passed Studio playtest with zero errors."
        ]
      },
      "chat-starter-2": {
        id: "chat-starter-2",
        title: "Starter Blueprints",
        built: [],
        builtTitles: [],
        undoStack: [],
        scripts: {},
        instances: {},
        lines: []
      }
    }
  };

  function ideasFor(c) {
    if (!c.built.length) return CATALOG.slice(0, 3);
    return CATALOG.slice(3).filter(function(i) {
      return i.needs.every(function(n){ return c.built.indexOf(n) >= 0; }) &&
             i.skills.some(function(s){ return c.built.indexOf(s) < 0; });
    }).slice(0, 3);
  }

  function jsonRes(obj, status) {
    return Promise.resolve(new Response(JSON.stringify(obj), {
      status: status || 200,
      headers: { "Content-Type": "application/json" }
    }));
  }

  const origFetch = window.fetch.bind(window);
  window.fetch = function(input, init) {
    const rawUrl = typeof input === "string" ? input : (input && input.url) || "";
    let u;
    try { u = new URL(rawUrl, location.href); } catch (e) { return origFetch(input, init); }
    if (!u.pathname.startsWith("/api/")) return origFetch(input, init);
    const method = ((init && init.method) || "GET").toUpperCase();
    let body = {};
    if (init && typeof init.body === "string" && init.body) {
      try { body = JSON.parse(init.body); } catch (e) {}
    }
    const chat = state.chats[state.active];

    if (method === "GET" && u.pathname === "/api/version") return jsonRes({ name: "openro-rs", version: "0.3.0" });
    if (method === "GET" && u.pathname === "/api/status") return jsonRes({ studio_connected: true, model_configured: true, game_name: "Skyline Obby Tycoon", version: "0.3.0", store_recovered: [] });
    if (method === "GET" && u.pathname === "/api/update") return jsonRes({ name: "openro-rs", current: "0.3.0", mode: "off", available: false, can_apply: false, dev_build: false, checked_at: Math.floor(Date.now() / 1000), staged: null, latest: null, error: null });
    if (method === "GET" && u.pathname === "/api/chats") {
      return jsonRes({
        chats: Object.values(state.chats).map(function(c){ return { id: c.id, title: c.title }; }),
        active: state.active,
        can_undo: chat.undoStack.length > 0
      });
    }
    if (method === "POST" && u.pathname === "/api/chats") {
      const id = "chat-demo-" + (state.counter++);
      state.chats[id] = { id: id, title: "New chat", built: [], builtTitles: [], undoStack: [], scripts: {}, instances: {}, lines: [] };
      state.active = id;
      return jsonRes({ id: id, title: "New chat" }, 201);
    }
    if (method === "POST" && u.pathname === "/api/chats/select") {
      if (body.id && state.chats[body.id]) { state.active = body.id; return jsonRes({ ok: true }); }
      return jsonRes({ ok: false, error: "chat not found" }, 404);
    }
    if (method === "GET" && u.pathname === "/api/thread") return jsonRes({ thread: chat.lines.slice() });
    if (method === "GET" && u.pathname === "/api/place-tree") return jsonRes({ scripts: chat.scripts, instances: chat.instances });
    if (method === "POST" && u.pathname === "/api/ideas") {
      return jsonRes({ ok: true, ideas: ideasFor(chat), built_titles: chat.builtTitles.slice(), pitched_new: false });
    }
    if (method === "POST" && u.pathname === "/api/ideas/build") {
      const idea = CATALOG.find(function(x){ return x.id === body.id; }) || CATALOG[0];
      if (chat.title === "New chat" || chat.title === "Starter Blueprints") chat.title = idea.title;
      const addedScripts = [];
      const addedInstances = [];
      chat.lines.push("You: Build this — " + idea.title);
      idea.skills.forEach(function(sk) {
        const pascal = sk.split("_").map(function(w){ return w.charAt(0).toUpperCase() + w.slice(1); }).join("");
        const target = sk === "game_gui" ? "StarterGui.GameGui" : "ServerScriptService." + pascal;
        chat.lines.push("Generated the " + sk + " script.");
        chat.lines.push("Wrote " + target + ".");
        if (!chat.scripts[target]) {
          chat.scripts[target] = "-- Generated by OpenRO Rune skill: " + sk + "\\nreturn true";
          addedScripts.push(target);
        }
        if (chat.built.indexOf(sk) < 0) chat.built.push(sk);
      });
      if (idea.id === "obby-starter" || idea.id === "round-arena") {
        if (!chat.instances["Workspace.SpawnPad"]) {
          chat.instances["Workspace.SpawnPad"] = { class_name: "SpawnLocation", properties: { Anchored: true } };
          chat.lines.push("Created Workspace.SpawnPad (SpawnLocation).");
          addedInstances.push("Workspace.SpawnPad");
        }
      }
      chat.lines.push("Playtest passed.");
      chat.lines.push("OpenRO: Built “" + idea.title + "” in Studio and verified playtest with 0 errors.");
      if (chat.builtTitles.indexOf(idea.title) < 0) chat.builtTitles.push(idea.title);
      chat.undoStack.push({ scripts: addedScripts, instances: addedInstances, skills: idea.skills.slice(), title: idea.title });
      const next = ideasFor(chat);
      if (next.length) {
        chat.lines.push("OpenRO: That landed. Next, I could add:");
        next.forEach(function(n){ chat.lines.push("OpenRO: Idea — " + n.title + ": " + n.pitch); });
      }
      return jsonRes({ ok: true, summary: "Built " + idea.title, thread: chat.lines, ideas: next });
    }
    if (method === "POST" && u.pathname === "/api/chat") {
      const msg = String(body.message || "").trim();
      if (!msg) return jsonRes({ ok: false, error: "empty message" }, 400);
      if (chat.title === "New chat") chat.title = msg.slice(0, 44);
      const slug = "CustomMechanic" + (Object.keys(chat.scripts).length + 1);
      const target = "ServerScriptService." + slug;
      chat.lines.push("You: " + msg);
      chat.lines.push("Generated the custom_mechanic script.");
      chat.lines.push("Wrote " + target + ".");
      chat.lines.push("Playtest passed.");
      chat.lines.push("OpenRO: Added " + slug + " to ServerScriptService and verified the Studio playtest.");
      chat.scripts[target] = "-- Generated from prompt: " + msg.replace(/[\\r\\n]+/g, " ") + "\\nlocal RunService = game:GetService(\\"RunService\\")";
      chat.undoStack.push({ scripts: [target], instances: [], skills: [], title: slug });
      return jsonRes({ ok: true, summary: "Added " + slug, thread: chat.lines, transcript: [] });
    }
    if (method === "POST" && u.pathname === "/api/revert") {
      const top = chat.undoStack.pop();
      if (!top) return jsonRes({ ok: false, error: "nothing to revert" });
      top.scripts.forEach(function(k){ delete chat.scripts[k]; });
      top.instances.forEach(function(k){ delete chat.instances[k]; });
      if (top.title) chat.builtTitles = chat.builtTitles.filter(function(t){ return t !== top.title; });
      const restored = top.scripts.concat(top.instances);
      chat.lines.push("OpenRO: Undid the last build: " + restored.length + " change(s) put back.");
      return jsonRes({ ok: true, restored: restored });
    }
    return jsonRes({ ok: true });
  };
})();
</script>`;

let html = fs.readFileSync(srcHtml, "utf8")
  .replace(/id="server-state" hidden/, 'id="server-state" hidden data-demo="true"')
  .replace(/__KEY_BANNER__/g, "")
  .replace(/__CHIP_CLASS__/g, "on")
  .replace(/__CHIP__/g, "Studio connected")
  .replace(/__CAN_BUILD__/g, "true")
  .replace(/__STUDIO_CONNECTED__/g, "true")
  .replace(/__GAME_NAME__/g, "Skyline Obby Tycoon")
  .replace(/<script type="module">/, `${DEMO_MOCK_SCRIPT}\n<script type="module">`);

const targets = [
  path.join(repoRoot, "demo-dist", "index.html"),
  ...(process.env.DEMO_EXPORT_DIR ? [path.join(process.env.DEMO_EXPORT_DIR, "index.html")] : []),
];

for (const out of targets) {
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, html);
  console.log(`demo bundle: ${out} (${(html.length / 1024).toFixed(0)} kB)`);
}
