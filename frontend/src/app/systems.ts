// Canonical catalog of all 22 OpenRO v0.5.0 Roblox game systems across 6 categories.
// Exact 1:1 parity with Rust Rune skills (src/skills.rs) and standalone web-engine compilers.
import type { Place } from "./data";

export type SystemCategory =
  | "Movement"
  | "Economy"
  | "Combat"
  | "World"
  | "Persistence"
  | "UI";

export const SYSTEM_CATEGORIES: SystemCategory[] = [
  "Movement",
  "Economy",
  "Combat",
  "World",
  "Persistence",
  "UI",
];

export type GameSystemDef = {
  id: string;
  title: string;
  category: SystemCategory;
  service: string;
  summary: string;
  adds: string[];
  apiTags: string[];
  prompt: string;
  defaultIdeaId?: string;
  matchTokens: string[];
};

export const CATEGORY_TAG_CLS: Record<SystemCategory, string> = {
  Movement: "bg-tag-sky-bg text-tag-sky-fg",
  Economy: "bg-tag-mint-bg text-tag-mint-fg",
  Combat: "bg-tag-plan-bg text-tag-plan-fg",
  World: "bg-tag-dev-bg text-tag-dev-fg",
  Persistence: "bg-tag-research-bg text-tag-research-fg",
  UI: "bg-tag-design-bg text-tag-design-fg",
};

export const GAME_SYSTEMS: GameSystemDef[] = [
  // --- Movement (6) ---
  {
    id: "spawn_link",
    title: "Spawn Pad & Link",
    category: "Movement",
    service: "Workspace · ServerScriptService",
    summary: "Anchored SpawnLocation pad verified at boot so players always enter cleanly.",
    adds: ["SpawnLocation", "SpawnLink script"],
    apiTags: ["SpawnLocation", "Workspace"],
    prompt: "Add a SpawnPad SpawnLocation and SpawnLink verification script in ServerScriptService.",
    matchTokens: ["spawnlink", "spawnpad"],
  },
  {
    id: "checkpoints",
    title: "Stage Checkpoints",
    category: "Movement",
    service: "Workspace · ServerScriptService",
    summary: "Multi-stage checkpoint pads that track player progress and respawn at the highest reached stage.",
    adds: ["Checkpoints folder", "Respawn script"],
    apiTags: ["Attributes", "CharacterAdded"],
    prompt: "Add 6 stage checkpoints that save player stage progress and respawn players at their latest checkpoint.",
    defaultIdeaId: "add-checkpoints",
    matchTokens: ["checkpoints", "stage"],
  },
  {
    id: "jump_pad",
    title: "Velocity Jump Pad",
    category: "Movement",
    service: "Workspace",
    summary: "Neon launch pad applying upward AssemblyLinearVelocity impulse with debounce cooldown and FX.",
    adds: ["JumpPad part", "Bounce script"],
    apiTags: ["AssemblyLinearVelocity", "Touched"],
    prompt: "Add a velocity JumpPad in Workspace that launches players upward with a bounce cooldown.",
    matchTokens: ["jumppad", "jump_pad", "jumpboost"],
  },
  {
    id: "moving_platform",
    title: "Moving Platform",
    category: "Movement",
    service: "Workspace · ServerScriptService",
    summary: "Smoothly oscillating platform driven by RunService/TweenService that carries standing players.",
    adds: ["MovingPlatform part", "Carry loop"],
    apiTags: ["TweenService", "RunService"],
    prompt: "Add a MovingPlatform in Workspace that glides smoothly between waypoints and carries standing players.",
    matchTokens: ["movingplatform", "moving_platform"],
  },
  {
    id: "portal_teleporter",
    title: "Paired Portal Teleporter",
    category: "Movement",
    service: "Workspace · ServerScriptService",
    summary: "Linked PortalA and PortalB CFrame teleporters with per-player debounce protection.",
    adds: ["PortalA & PortalB", "Warp script"],
    apiTags: ["CFrame", "Debounce"],
    prompt: "Add paired PortalA and PortalB teleporters in Workspace that warp players cleanly with debounce.",
    matchTokens: ["portal", "teleporter"],
  },
  {
    id: "vehicle_spawner",
    title: "Vehicle Spawner Pad",
    category: "Movement",
    service: "Workspace · ServerScriptService",
    summary: "Interactive spawner pad that builds a drivable chassis with per-player vehicle cap and cleanup.",
    adds: ["VehiclePad part", "Spawner script"],
    apiTags: ["VehicleSeat", "Per-player cap"],
    prompt: "Add a VehicleSpawner pad in Workspace with per-player vehicle limits and automatic cleanup on leave.",
    matchTokens: ["vehiclespawner", "vehicle_spawner", "vehiclepad"],
  },

  // --- Economy (4) ---
  {
    id: "currency",
    title: "Saved Coins Currency",
    category: "Economy",
    service: "ServerScriptService",
    summary: "Passive-earning leaderstats currency persisted across sessions via pcall DataStore and BindToClose.",
    adds: ["leaderstats", "DataStore save"],
    apiTags: ["DataStoreService", "BindToClose"],
    prompt: "Add a Coins currency system with passive income and DataStore save/load.",
    defaultIdeaId: "add-currency",
    matchTokens: ["currency"],
  },
  {
    id: "shop",
    title: "Item Shop (RemoteEvent)",
    category: "Economy",
    service: "ServerScriptService · ReplicatedStorage",
    summary: "Server-authoritative shop RemoteEvent validating balance before deducting currency and granting items.",
    adds: ["OpenROShopBuy", "Server shop"],
    apiTags: ["RemoteEvent", "Server-auth"],
    prompt: "Add a server-validated Item Shop selling a Speed Coil for 100 Coins over OpenROShopBuy.",
    defaultIdeaId: "add-shop",
    matchTokens: ["shop", "openroshopbuy"],
  },
  {
    id: "tycoon_dropper",
    title: "Tycoon Dropper & Conveyor",
    category: "Economy",
    service: "Workspace · ServerScriptService",
    summary: "Dropper spawning parts onto a moving conveyor into a collector with an upgrade purchase pad.",
    adds: ["Dropper & Collector", "Upgrader pad"],
    apiTags: ["Conveyor", "Tycoon loop"],
    prompt: "Add a TycoonDropper with conveyor, collector payout, and purchase upgrader button in Workspace.",
    matchTokens: ["tycoondropper", "tycoon_dropper", "dropper"],
  },
  {
    id: "quest_tracker",
    title: "Quest & Milestone Tracker",
    category: "Economy",
    service: "ServerScriptService · ReplicatedStorage",
    summary: "Objective milestone engine tracking progress over RemoteEvent and paying currency rewards.",
    adds: ["QuestProgress Remote", "Milestone rewards"],
    apiTags: ["RemoteEvent", "Milestones"],
    prompt: "Add a QuestTracker system with objective milestones, progress RemoteEvent, and coin payout.",
    matchTokens: ["questtracker", "quest_tracker", "quest"],
  },

  // --- Combat (4) ---
  {
    id: "kill_brick",
    title: "Lava Kill Brick",
    category: "Combat",
    service: "Workspace",
    summary: "Hazard brick dealing debounced Humanoid damage on touch for obby pits and arena traps.",
    adds: ["LavaBrick part", "KillBrick script"],
    apiTags: ["Humanoid:TakeDamage", "Touched"],
    prompt: "Add a LavaBrick kill hazard in Workspace with debounced damage on touch.",
    defaultIdeaId: "add-lava",
    matchTokens: ["killbrick", "kill_brick", "lavabrick"],
  },
  {
    id: "round_manager",
    title: "Round Arena Loop",
    category: "Combat",
    service: "ServerScriptService · Workspace",
    summary: "Automated intermission and active match round loop coordinating arena spawns and victory state.",
    adds: ["Round folder", "RoundManager"],
    apiTags: ["Match loop", "Workspace.Round"],
    prompt: "Add a RoundManager arena match loop in ServerScriptService.",
    matchTokens: ["roundmanager", "round_manager"],
  },
  {
    id: "combat_sword",
    title: "Server-Validated Combat Sword",
    category: "Combat",
    service: "ServerScriptService · ReplicatedStorage",
    summary: "Melee sword tool with server-side range/cooldown validation and KO leaderboard increments.",
    adds: ["CombatSword Remote", "KO hook"],
    apiTags: ["Server validation", "Cooldown"],
    prompt: "Add a server-validated CombatSword melee system with swing cooldown and KO tracking.",
    matchTokens: ["combatsword", "combat_sword", "sword"],
  },
  {
    id: "npc_patrol",
    title: "Patrolling NPC Guard",
    category: "Combat",
    service: "Workspace · ServerScriptService",
    summary: "Waypoint NPC guard powered by PathfindingService with patrol dialogue and proximity damage.",
    adds: ["NpcGuard rig", "Pathfinding loop"],
    apiTags: ["PathfindingService", "Waypoints"],
    prompt: "Add a patrolling NpcGuard in Workspace using PathfindingService waypoints and guard dialogue.",
    matchTokens: ["npcpatrol", "npc_patrol", "npcguard"],
  },

  // --- World (3) ---
  {
    id: "day_night",
    title: "Day/Night Sky Cycle",
    category: "World",
    service: "ServerScriptService · Lighting",
    summary: "Smooth Lighting.ClockTime progression cycling dawn, noon, dusk, and starlight.",
    adds: ["DayNight script", "Lighting cycle"],
    apiTags: ["Lighting.ClockTime", "task.wait"],
    prompt: "Add a DayNight sky cycle script advancing Lighting.ClockTime smoothly.",
    defaultIdeaId: "add-day-night",
    matchTokens: ["daynight", "day_night"],
  },
  {
    id: "soundtrack",
    title: "Looped Soundtrack",
    category: "World",
    service: "ServerScriptService · SoundService",
    summary: "Ambient SoundService track with configurable volume and looping ready for any asset ID.",
    adds: ["Soundtrack script", "SoundService"],
    apiTags: ["SoundService", "Sound"],
    prompt: "Add a looped Soundtrack script in SoundService with balanced volume.",
    defaultIdeaId: "add-soundtrack",
    matchTokens: ["soundtrack"],
  },
  {
    id: "weather_cycle",
    title: "Dynamic Weather Cycle",
    category: "World",
    service: "ServerScriptService · Lighting",
    summary: "Cycles clear skies, fog density, rain particles, and dusk Atmosphere transitions dynamically.",
    adds: ["WeatherCycle script", "Atmosphere FX"],
    apiTags: ["Atmosphere", "Lighting"],
    prompt: "Add a dynamic WeatherCycle system rotating clear, fog, rain, and dusk Lighting Atmosphere.",
    matchTokens: ["weathercycle", "weather_cycle", "weather"],
  },

  // --- Persistence (2) ---
  {
    id: "datastore_save",
    title: "DataStore Save & Load",
    category: "Persistence",
    service: "ServerScriptService",
    summary: "Dedicated pcall-guarded DataStoreService profile loader/saver with retries and BindToClose.",
    adds: ["DataStoreSave script", "BindToClose"],
    apiTags: ["DataStoreService", "pcall"],
    prompt: "Add a pcall-guarded DataStoreSave persistence module with player join load, leave save, and BindToClose.",
    matchTokens: ["datastoresave", "datastore_save"],
  },
  {
    id: "inventory_backpack",
    title: "Server Inventory & Backpack",
    category: "Persistence",
    service: "ServerScriptService · ReplicatedStorage",
    summary: "Server-authoritative item inventory table with validated equip and grant RemoteEvents.",
    adds: ["InventoryEquip Remote", "Server table"],
    apiTags: ["RemoteEvent", "Inventory"],
    prompt: "Add a server-authoritative InventoryBackpack system with an equip RemoteEvent.",
    matchTokens: ["inventorybackpack", "inventory_backpack", "inventory"],
  },

  // --- UI (3) ---
  {
    id: "game_gui",
    title: "Balance & Shop HUD",
    category: "UI",
    service: "StarterGui",
    summary: "Client ScreenGui displaying title banner, live leaderstats balance, and shop trigger button.",
    adds: ["ScreenGui", "Balance HUD"],
    apiTags: ["StarterGui", "LocalScript"],
    prompt: "Add a GameGui ScreenGui HUD showing live balance and shop purchase button.",
    matchTokens: ["gamegui", "game_gui"],
  },
  {
    id: "leaderboard",
    title: "Wins Leaderboard",
    category: "UI",
    service: "ServerScriptService",
    summary: "Native Roblox top-right leaderstats scoreboard tracking player wins or score automatically.",
    adds: ["leaderstats", "Wins counter"],
    apiTags: ["Players.PlayerAdded", "IntValue"],
    prompt: "Add a Wins Leaderboard creating leaderstats for every joining player.",
    matchTokens: ["leaderboard"],
  },
  {
    id: "admin_spectate",
    title: "Spectator Camera & Feed",
    category: "UI",
    service: "StarterGui · ReplicatedStorage",
    summary: "Spectator HUD cycling CurrentCamera.CameraSubject across active players with round status feed.",
    adds: ["SpectateGui", "Camera cycler"],
    apiTags: ["CurrentCamera", "StarterGui"],
    prompt: "Add an AdminSpectate spectator camera cycler and round status feed in StarterGui.",
    matchTokens: ["adminspectate", "admin_spectate", "spectate"],
  },
];

export function isSystemInPlace(sys: GameSystemDef, place: Place | null | "error"): boolean {
  if (!place || place === "error") return false;
  const hay = [
    ...Object.keys(place.scripts || {}),
    ...Object.keys(place.instances || {}),
  ]
    .join(" ")
    .toLowerCase()
    .replace(/[^a-z0-9_]/g, "");
  return sys.matchTokens.some((tok) => hay.includes(tok.replace(/[^a-z0-9_]/g, "")));
}
