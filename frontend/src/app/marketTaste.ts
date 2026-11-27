// Built-in Live Roblox Market Taste, Craft Bar & 8-Dimension Taste Dossier (OpenRO v0.9.0).
// Serves as OpenRO's opinionated Top-Trending Roblox Craft & Anti-Slop Quality Bar across
// 8 high-retention niches grounded in live Roblox Explore charts, with zero external requests
// in offline/standalone builds and live Explore + Game Research on OpenRO desktop.

export type MarketTasteGame = {
  name: string;
  universe_id?: number;
  root_place_id?: number;
  ccu: number;
  like_ratio: number;
  sort: string;
  sort_tags?: string[];
  hook?: string;
  source?: string;
};

export type MarketTasteNiche = {
  id: string;
  name: string;
  tagline: string;
  craft_bar: string;
  games: MarketTasteGame[];
  gameplay_loop: string[];
  ui_ux_style: string[];
  animations_and_juice: string[];
  model_and_world_3d_style: string[];
  audio_cadence: string[];
  monetization_loops: string[];
  retention_hooks: string[];
  anti_slop_rules: string[];
  automatic_twists: string[];
  quality_boosts: string[];
  pitch_prompt: string;
};

export type MarketTasteSnapshot = {
  active_niche: string;
  source: string;
  updated_at: string;
  live_supported: boolean;
  live_games_count?: number;
  note?: string;
  niches: MarketTasteNiche[];
};

export type MarketTasteResearchResult = {
  ok: boolean;
  name: string;
  universe_id: number;
  place_id?: number;
  creator?: string;
  ccu: number;
  visits: number;
  favorites: number;
  like_ratio: number;
  genre?: string;
  niche_id: string;
  niche_name: string;
  thumbnails: string[];
  craft_bar: string;
  gameplay_loop: string[];
  ui_ux_style: string[];
  animations_and_juice: string[];
  model_and_world_3d_style: string[];
  audio_cadence: string[];
  monetization_loops: string[];
  retention_hooks: string[];
  anti_slop_rules: string[];
  automatic_twists: string[];
  quality_boosts: string[];
  pitch_prompt: string;
  dossier_path?: string;
  offline_fallback?: boolean;
  note?: string;
  error?: string;
};

export const BASELINE_MARKET_NICHES: MarketTasteNiche[] = [
  {
    id: "top-trending",
    name: "Top Trending (All)",
    tagline: "Cross-genre Roblox Explore leaders spanning tycoons, social RP, action RPG, co-op horror, PvP arenas, physics sandboxes, survival & RNG.",
    craft_bar:
      "Ship top-trending Roblox craft across ANY genre: instant under-10-second core verb right in front of SpawnLocation, sunlit stylized 3D worlds with clean silhouettes (never dark flat baseplates), non-overlapping HUDs, snappy TweenService juice, server-authoritative DataStore saving, and fresh twists.",
    games: [
      { name: "Steal An Egg", universe_id: 10563114921, ccu: 963000, like_ratio: 92.4, sort: "top-trending", hook: "Viral raid/steal tycoon loop + rare mutations + timed base locks" },
      { name: "Brookhaven RP", universe_id: 1686885941, ccu: 564000, like_ratio: 90.4, sort: "top-playing-now", hook: "Zero-friction town houses, vehicles, props & expressive roleplay" },
      { name: "Throw Food For Animals", universe_id: 10688120401, ccu: 361000, like_ratio: 93.1, sort: "up-and-coming", hook: "Tactile physical throw verb + escalating giant evolutions" },
      { name: "Blox Fruits", universe_id: 994732206, ccu: 312000, like_ratio: 93.4, sort: "top-playing-now", hook: "Multi-island progression + elemental ability mastery + sea boss raids" },
      { name: "Dandy's World", universe_id: 5721456859, ccu: 206000, like_ratio: 95.1, sort: "fun-with-friends", hook: "Co-op floor extraction + stylized mascot roster + skill checks" },
      { name: "RIVALS", universe_id: 6035872082, ccu: 175000, like_ratio: 94.9, sort: "top-playing-now", hook: "Sub-5-second 1v1/2v2 duel queues + crisp parry/hit feedback" },
    ],
    gameplay_loop: [
      "First-10-second rule: spawn directly facing an interactive verb (starter pedestal, duel pad, stunt ramp, quest giver, or shared vehicle)",
      "Tight 2-5 minute core session cycles that reward skill, exploration, or social coordination with immediate visible upgrades",
      "Compounding meta-progression (Rebirths, Ranks, Mastery, Unlockable Zones, or Collection Index) persisted safely via DataStoreService",
    ],
    ui_ux_style: [
      "Clean, non-overlapping Roblox ScreenGui: dock controls hide/dim whenever full-screen modals or cutscenes open",
      "Readable telemetry strip showing currency, active server event countdown, and next milestone without crowding the 3D viewport",
      "Touch-friendly 44px+ targets & ProximityPrompts so phones, tablets, and desktop play effortlessly",
    ],
    animations_and_juice: [
      "TweenService squash-and-stretch pops, floating damage/income numbers, and crisp 2-frame hitstop on impacts",
      "Translucent layered VFX (Transparency 0.65-0.8) & ParticleEmitters that highlight silhouettes instead of obscuring them",
      "Smooth camera framing punches on level-ups, big drops, or round finishes",
    ],
    model_and_world_3d_style: [
      "Vibrant sunlit stylized 3D worlds (emerald grass, warm marble/timber, turquoise water, crisp cerulean sky ClockTime 13.5), NEVER dark grey/black box slabs in daylight",
      "Real sculpted Blender 3D landmarks (<6k tris, clean UVs, 1m = 3.57 studs) framing clear player sightlines from SpawnLocation",
      "Reserve moody dark/crimson Atmosphere strictly for intentional night phases, horror corridors (with warm guide lights), or special storm events",
    ],
    audio_cadence: [
      "Layered tactile SFX feedback (chimes, crisp UI clicks, impactful hits, victory flourishes) synthesized cleanly as PCM16 .wav assets",
      "Escalating pitch audio on pickup streaks, combos, and milestone unlocks",
      "Clear audio telegraphs before boss attacks, nightfall, or server-wide events",
    ],
    monetization_loops: [
      "Optional convenience & progression gamepasses (2x Cash/XP, Auto-Collect, VIP Lounge, Private Server) anchored cleanly to the core loop",
      "Non-pay-to-win cosmetics (trails, finishers, skins, emotes, vehicle wraps) and transparent pity/milestone guarantees",
      "Server-wide shared boosts (Global Luck/Event triggers) that delight the whole lobby simultaneously",
    ],
    retention_hooks: [
      "Scheduled server-wide events (Golden Rain, Weather Storms, Boss Portals, Chaos Rounds) creating live countdown anticipation",
      "Visible social flexing in the 3D lobby (leaderboards, aura halos, upgraded bases, showcase pedestals, win-streak crowns)",
      "Daily quests and milestone badges that persist reliably across sessions via retry + BindToClose DataStore saving",
    ],
    anti_slop_rules: [
      "Never ship empty baseplates, unstyled default grey blocks, or 40-line stub scripts; build a cohesive sunlit 3D world with real systems",
      "Never stack overlapping HUD buttons over active modals or blind players in pitch-black unlit rooms",
      "Validate every currency grant, purchase, hit, and roll strictly on the server with rate-limited RemoteEvents",
    ],
    automatic_twists: [
      "DYNAMIC SERVER EVENTS: Every 10-15 minutes trigger a playable server-wide twist (Golden Rain, Storm Surge, Shifting Arena) that transforms the map live",
      "SKILL TIMING BONUS: Add a crisp timing mechanic (Overdrive burst, Perfect Parry, Bump-Lock counter) rewarding active players with +35% bonus impact",
      "SOCIAL SHOWCASE PRESENCE: Give every player a visible 3D world flex (upgraded plot landmark, floating companion aura, or streak crown) readable across the hub",
    ],
    quality_boosts: [
      "Sunlit architectural palette + sculpted Blender landmarks replacing flat neon boxes",
      "Modal-aware non-overlapping HUDs + ProximityPrompt mobile polish",
      "Server-authoritative Luau with retry/BindToClose DataStore persistence & clean 60Hz cleanup",
    ],
    pitch_prompt:
      "Build a high-craft top-trending Roblox experience with a vibrant sunlit 3D hub, an instant under-10-second core loop right in front of SpawnLocation, snappy TweenService juice and sound cues, a skill-timing bonus mechanic, periodic server-wide map events, clean non-overlapping ScreenGui HUD, and retry DataStore persistence.",
  },
  {
    id: "anime-rng-cards",
    name: "Anime RNG & Cards",
    tagline: "Weighted multi-tier gacha summons, foil variants, 4v4 synergy clash, weather storms & boss towers.",
    craft_bar:
      "Sunlit vibrant anime floating archipelago with sculpted Torii Summon Shrine, translucent light beams, crisp non-overlapping 3:4 cel-shaded cards, and punchy 4v4 Chain Overdrive combat.",
    games: [
      { name: "Anime Card Battles: Rebirth", universe_id: 10092619788, ccu: 84200, like_ratio: 94.6, sort: "top-revisited", hook: "Torii roll spectacle + 7-weather luck multipliers + 4v4 strategic card clashes" },
      { name: "Fisch", universe_id: 5750914919, ccu: 91000, like_ratio: 92.1, sort: "top-playing-now", hook: "Tactile RNG rarity hunts paired with weather totems, mutation index, and social flexing" },
      { name: "Sol's RNG", universe_id: 5361032378, ccu: 112400, like_ratio: 91.2, sort: "top-playing-now", hook: "High-drama aura reveal cutscenes, biome rarity boosts, and luck-stacking gauntlets" },
      { name: "Anime Vanguards", universe_id: 6299805723, ccu: 95800, like_ratio: 93.8, sort: "top-trending", hook: "Trait rerolls + pity banner + mythic raid bosses" },
    ],
    gameplay_loop: [
      "5-second core loop: Roll at the Torii Shrine -> reveal foil rarity + trait -> auto-equip strongest 4-card squad -> clash bosses for Coins & Luck Potions",
      "Escalating progression ladder: Starter Pack -> Trait Forge -> 50-Floor Infinite Tower -> Timed Server Raid Boss -> Rebirth multiplier",
      "Social lobby loop: Hall of Legends 3D card pedestals + visible floating Lead-Card Guardian Aura behind every player",
    ],
    ui_ux_style: [
      "Clean, non-overlapping Roblox ScreenGui: bottom dock auto-hides/dims during Summon Reveal & Battle overlays",
      "Crisp 3:4 cel-shaded portraits with bold rarity rims (Common -> Mythic -> Celestial -> Secret) and zero text clutter on decals",
      "Persistent top-edge telemetry strip: Coins, Luck multiplier, Active Weather timer, and 1/50 Pity progress bar",
    ],
    animations_and_juice: [
      "TweenService anticipation reel with rising chime arpeggio, rarity flash cut-in, and brief hitstop on Mythic/Secret pulls",
      "4v4 battle lunge tweens, floating Crit / Lifesteal / Guard Break skill popups, and Chain Overdrive dual-card cut-in",
      "Translucent Torii summon pillar beam (Transparency 0.72) pulsing softly with elemental ParticleEmitters",
    ],
    model_and_world_3d_style: [
      "Sunlit vibrant anime floating archipelago: lush stylized emerald grass terraces, warm cream marble plaza, turquoise koi moat, sculpted cliff undersides, cerulean sky (ClockTime 13.5)",
      "NEVER ship dark obsidian/black neon-box slabs in daylight; reserve dark crimson/indigo Atmosphere strictly for Blood Moon, Eclipse & Fusion Weather Storms",
      "Blender-sculpted Torii Summon Gate, Hall of Legends pedestals, Alchemist Cauldron & Boss Colosseum Arch (<6k tris, clean UVs, 1m = 3.57 studs)",
    ],
    audio_cadence: [
      "Crisp ascending crystal arpeggio on roll start (`summon_roll.wav`) -> heroic harmonic fanfare on Mythic+ (`mythic_fanfare.wav`)",
      "Punchy impact hit on battle crits (`battle_crit.wav`), bubbling rise on Luck/Speed potions (`potion_drink.wav`), tactile UI clicks (`ui_click.wav`)",
      "Subtle ambient courtyard chimes ducked cleanly during boss clash ultimates",
    ],
    monetization_loops: [
      "Luck I/II/III & Roll Speed potions (timed consumables + VIP 2x Luck & Fast Auto-Roll gamepass)",
      "Guaranteed Pity counter (1/50 Mythic+) paired with Server Weather Reroll developer products",
      "Trait reroll tokens, extra Deck/Inventory slots, and seasonal Secret Banner packs",
    ],
    retention_hooks: [
      "7 rotating Server Weather Biomes + rare Fusion Weather Storms with exclusive Secret pulls",
      "Visible 3D Hall of Legends pedestals & floating Lead-Card Guardian Aura behind every player",
      "50-floor Infinite Tower + Timed Server Raid Boss with damage leaderboards & Rebirth multipliers",
    ],
    anti_slop_rules: [
      "Never ship flat grey/black neon boxes or 40-line stub scripts: build a sunlit 3D archipelago with sculpted Blender landmarks",
      "Hide/dim bottom HUD buttons during summon/battle modals so UI elements never stack or overlap",
      "Validate every roll, equip, fusion, and raid claim server-side with rate-limited RemoteEvents & retry DataStore saving",
    ],
    automatic_twists: [
      "CHAIN OVERDRIVE: Timed [SPACE] / button burst window during 4v4 clashes triggering a +35% dual-card synergy finisher & cut-in",
      "FUSION WEATHER STORMS: Rare double-biome events (Crimson Eclipse 5x Luck & Celestial Tempest 4.5x Luck) shifting sky Lighting live",
      "LIVING 3D LEAD-CARD GUARDIAN AURA: Your active deck captain hovers beside your avatar in the 3D courtyard with elemental trail particles",
    ],
    quality_boosts: [
      "Sunlit stylized floating archipelago palette (emerald terraces + turquoise koi moat + cream stone) instead of dark generic baseplates",
      "Translucent Torii summon light column (Transparency 0.72) keeping sculpted crimson architecture 100% legible",
      "Modal-aware HUD layering + instant pity/weather telemetry strip with crisp TweenService transitions",
    ],
    pitch_prompt:
      "Build Untitled Anime Card Game: a sunlit floating-archipelago Anime RNG & Card Battler with a sculpted 3D Torii Summon Shrine, translucent summon beam, 6 weighted rarity tiers with 1/50 pity, Luck & Speed potions, 7 rotating weather biomes plus Fusion Weather Storms (Crimson Eclipse 5x Luck), 4v4 auto-battle with Chain Overdrive [SPACE] timing finisher, Living 3D Lead-Card Guardian Aura, and retry DataStore persistence.",
  },
  {
    id: "viral-sim-tycoon",
    name: "Viral Sim & Tycoon",
    tagline: "Ultra-tactile collect-steal-upgrade loops with instant first-10-second dopamine and rebirth scaling.",
    craft_bar:
      "Bright toy-like stylized world with unmistakable 10-second base-income feedback, active physical steal/throw verb, juicy coin arcs, and zero empty click-and-wait baseplates.",
    games: [
      { name: "Steal An Egg", universe_id: 10563114921, ccu: 963000, like_ratio: 92.4, sort: "top-trending", hook: "Viral raid/steal base loop + rare egg mutations + base locks" },
      { name: "Throw Food For Animals", universe_id: 10688120401, ccu: 361000, like_ratio: 93.1, sort: "up-and-coming", hook: "Instant physical throw feedback + giant pet evolution + feeding frenzy" },
      { name: "Pet Simulator 99", universe_id: 3317771874, ccu: 142000, like_ratio: 94.0, sort: "top-revisited", hook: "Huge pet hatches + zone rebirths + player trading plaza" },
    ],
    gameplay_loop: [
      "First-10-second hook: spawn directly facing glowing starter pedestal + live floating $/sec ticker",
      "Active risk/reward verb: raid rival bases, lock shields on timers, or physically launch upgrades into mutation machines",
      "Compounding Rebirth ladder unlocking new visual world biomes, giant mutations, and passive multipliers",
    ],
    ui_ux_style: [
      "High-contrast rounded economy HUD with sliding number counters and clean left/right icon rails",
      "World-space BillboardGuis above every pedestal/lock showing exact price, $/sec, and countdown shield timer",
      "Zero modal spam on spawn: starter pack prompt only surfaces after the player's first base milestone",
    ],
    animations_and_juice: [
      "Squash-and-stretch TweenService bounce on every drop pickup, upgrade button press, and mutation hatch",
      "Arcing coin particles flying from pedestals into the player's backpack with escalating pitch chimes",
      "Dramatic base-lock laser shutter + server-wide banner flash when a Rare Mutation spawns",
    ],
    model_and_world_3d_style: [
      "Vibrant sunlit pastel-meets-saturated stylized island hub with clear color-coded player plots and landmark conveyor arches",
      "Chunky readable silhouettes (oversized eggs, glowing mutation pods, clean low-poly trees) readable from 200 studs away",
      "Bright daytime Lighting (ClockTime 14, soft Bloom, warm SunRays) with clean grass/marble texture contrast",
    ],
    audio_cadence: [
      "Escalating pentatonic pop chimes as streak pickups chain together",
      "Heavy mechanical clunk + golden chime on base upgrade unlock and rebirth surge",
      "Warning klaxon pulse when a rival breaches your base perimeter",
    ],
    monetization_loops: [
      "2x Cash / 2x Luck / Auto-Collect & Magnet gamepasses anchored to the core collection loop",
      "Starter Pack bundle (< 99 Robux) triggered after the player's first base upgrade milestone",
      "Private Server VIP perks, Server-Wide Luck/Frenzy boosts, and limited-time Mutation Eggs",
    ],
    retention_hooks: [
      "Zero-tutorial first 10 seconds: spawn facing an interactive dropper/pedestal with floating income popups",
      "Compounding Rebirth ladder that unlocks new visual zones, rare mutation tiers, and persistent multipliers",
      "Hourly Global Event countdown (Golden Rain / Mutation Surge) driving repeat sessions & friend invites",
    ],
    anti_slop_rules: [
      "Avoid passive click-and-wait empty baseplates: give players active stealing, throwing, or conveyor interaction",
      "Every purchase button must trigger tactile sound + particle bursts + immediate visual base growth",
      "Persist all currency, upgrades, rebirths, and unlocked pets safely via pcall-wrapped DataStoreService",
    ],
    automatic_twists: [
      "BUMP-LOCK OVERCHARGE: Time your base shield snap right as a raider enters to bounce them out & steal 15% bonus bounty",
      "CHAIN CONVEYOR FUSION: Route two different elemental drops into one splitter pedestal to forge rare Hybrid Mutations",
      "GOLDEN RAIN AUCTION: Every 15 minutes a giant sky vault drops a Mythic relic in the center plaza for a 30-second dash",
    ],
    quality_boosts: [
      "Squash-and-stretch TweenService upgrade pops + arcing magnetic coin particles instead of static number ticks",
      "Color-coded plot landmarks and world-space BillboardGui income readouts readable from across the map",
      "Zero-friction first-10-second onboarding with starter pedestal placed directly in front of SpawnLocation",
    ],
    pitch_prompt:
      "Build a high-craft sunlit Viral Collector & Base Tycoon with instant 10-second starter pedestal income, active rival raid & timed Bump-Lock Overcharge defense, Chain Conveyor Fusion mutations, Golden Rain center-plaza drops, squash-and-stretch TweenService juice, compounding Rebirths, and DataStore saving.",
  },
  {
    id: "physics-crash-ragdoll",
    name: "Physics Crash & Ragdoll",
    tagline: "Destructible vehicles, exaggerated ragdoll stunts, and hilarious shareable multiplayer chaos.",
    craft_bar:
      "Crisp sunlit stunt arena with spawn-facing mega ramp, breakable physics props, snappy slow-mo impact camera, and rock-solid 60Hz debris cleanup.",
    games: [
      { name: "Hyper Realistic Ragdoll", universe_id: 10512883901, ccu: 149000, like_ratio: 90.8, sort: "top-trending", hook: "Tactile ragdoll physics playgrounds + stunt cannons + viral clips" },
      { name: "BloxNG Crash Physics", universe_id: 10644912300, ccu: 56000, like_ratio: 91.5, sort: "up-and-coming", hook: "Deformable vehicle crashes + mega ramps + slow-motion replay" },
      { name: "Natural Disaster Survival", universe_id: 65241, ccu: 48500, like_ratio: 93.2, sort: "top-revisited", hook: "60-second chaotic physics rounds + iconic destructible maps" },
    ],
    gameplay_loop: [
      "Under-5-second fun: spawn facing a giant launch ramp, cannon, and breakable brick tower",
      "Impact-force scoring converts every flip, wall smash, and bone-crunch launch into Coins for new vehicles/gadgets",
      "90-second shared chaos events (Low-Gravity Derby, Meteor Shower, Giant Crusher) keep lobbies laughing together",
    ],
    ui_ux_style: [
      "Minimal speedometer, live Impact Force / Airtime combo popups, and 1-tap Reset Vehicle / Ragdoll toggle",
      "Clean vehicle & gadget hotbar along the bottom edge with zero opaque clutter over the 3D action",
      "Live server Crash Distance & Top Stunt leaderboard tucked neatly in the corner",
    ],
    animations_and_juice: [
      "Brief 0.35s slow-motion impact pulse + camera shake on massive high-speed collisions",
      "Sparks, tire skid trails, and flying breakable part chunks with clean fade-out cleanup",
      "Oversized comic-style impact callouts ('MEGA FLIP x4!', 'WALL SHATTER!')",
    ],
    model_and_world_3d_style: [
      "Bright Mediterranean / coastal test-track sunlit terrain with checkered stunt ramps, neon boost gates, and layered crush arenas",
      "Color-coded hazard zones (amber boost rings, crimson destructible walls, cobalt jump pads) with crisp shadows",
      "Properly welded vehicle chassis & balanced center-of-mass so stunts feel weighty, never jittery",
    ],
    audio_cadence: [
      "Rising turbo whine on boost pads -> crunchy layered metallic crash + comic spring boing on ragdoll eject",
      "Tactile coin cascade tallying up stunt multiplier combos",
      "Countdown air-horn before every 90-second shared chaos round",
    ],
    monetization_loops: [
      "Exotic Vehicle & Stunt Gadget packs (Jet Thrusters, Gravity Coil, Monster Truck, Ragdoll Cannon)",
      "Private Server Chaos Sandbox controls (low gravity, meteor shower, super bounce modifiers)",
      "Cosmetic crash trails, horn SFX packs, and VIP instant vehicle respawn",
    ],
    retention_hooks: [
      "Immediate stunt ramp & launch cannon right in front of SpawnLocation (fun in under 5 seconds)",
      "Live Crash Score & Distance leaderboard awarding Coins for flips, impact force, and part destruction",
      "Short 90-second shared map chaos events (Tornado, Tsunami, Low-Gravity Derby) with friend spectating",
    ],
    anti_slop_rules: [
      "Keep all physics parts network-owned cleanly to avoid rubber-banding or anchored-part deadlocks",
      "Auto-clean debris and reset crashed vehicles cleanly so server FPS stays locked at 60Hz",
      "Include instant Reset/Respawn + Camera Reset buttons so players never get stuck upside-down",
    ],
    automatic_twists: [
      "MID-AIR EJECT & GRAPPLE: Launch out of your crashing car mid-flight to grapple onto passing wreckages for 3x Stunt Multiplier",
      "PINBALL BUMPER ARENA: Every 3 minutes giant glowing pinball bumpers rise across the valley turning crashes into chain-combo chaos",
      "GHOST REPLAY CHASER: Beat the translucent holographic ghost of the server's #1 longest launch for instant bonus Coins",
    ],
    quality_boosts: [
      "0.35s slow-mo impact camera punch + spark particles on high-velocity collisions",
      "1-tap instant Reset/Respawn & clean 60Hz debris pooling so physics never lag or deadlock",
      "Spawn-facing mega ramp with clear distance markers painted right onto the runway",
    ],
    pitch_prompt:
      "Build a sunlit Physics Crash & Ragdoll Stunt Sandbox with a spawn-facing mega ramp, vehicle spawner, jump boost pads, Mid-Air Eject & Grapple stunt combos, periodic Pinball Bumper Arena chaos events, impact-force coin scoring, slow-mo impact juice, and instant 60Hz-safe respawn.",
  },
  {
    id: "survival-expedition",
    name: "Survival & Expedition",
    tagline: "Co-op journey runs along a perilous route with fuel/loot scavenging, upgrades & extraction milestones.",
    craft_bar:
      "Atmospheric sunlit-to-dusk wilderness journey anchored to one shared upgradable vehicle/raft with clear milestone checkpoints and dramatic day/night threat contrast.",
    games: [
      { name: "100 Miles Down the River", universe_id: 10599221800, ccu: 72000, like_ratio: 92.7, sort: "up-and-coming", hook: "Co-op raft expedition + day/night river hazards + salvage upgrades" },
      { name: "Dead Rails", universe_id: 7018190066, ccu: 138000, like_ratio: 94.3, sort: "top-playing-now", hook: "Armored train survival + town looting + class perks" },
      { name: "99 Nights in the Forest", universe_id: 7326934954, ccu: 91000, like_ratio: 91.9, sort: "fun-with-friends", hook: "Campfire defense + escalating night threats + co-op revives" },
    ],
    gameplay_loop: [
      "Day phase: steer the shared raft/train, stop at scenic outposts to scavenge fuel, scrap & relics",
      "Night phase: defend the vehicle campfire/hull together against escalating ambushes until dawn",
      "Milestone checkpoints (10km, 25km, 50km, 100km) unlock permanent Class XP & lobby loadout upgrades",
    ],
    ui_ux_style: [
      "Top compass bar showing Distance (km), Next Outpost marker, and Day/Night sun-moon dial",
      "Clean Fuel / Hull HP / Squad Revive indicators anchored compactly without blocking forward visibility",
      "ProximityPrompt interactions on every fuel barrel, turret, and workbench for effortless mobile & PC play",
    ],
    animations_and_juice: [
      "Smooth Lighting transition from golden afternoon -> crimson sunset -> misty lantern-lit nightfall",
      "Water/track motion foam, bobbing raft physics, and glowing lantern sway as night approaches",
      "Tactile hammer-spark repair feedback and dramatic flare-gun revive beacons",
    ],
    model_and_world_3d_style: [
      "Crisp sunlit frontier / alpine river valley by day with turquoise rapids, pine cliffs, and warm wooden outpost docks",
      "Dynamic Atmosphere & ColorCorrection shifting mood cleanly between safe golden daylight and tense lantern-lit nights",
      "Modular upgradable vehicle deck (armor plates, searchlights, storage crates) that visibly evolves mid-run",
    ],
    audio_cadence: [
      "Gentle river/engine rhythm by day -> low resonant horn chime warning 20 seconds before nightfall",
      "Crackling campfire & distant creature calls at night -> uplifting acoustic dawn chord on survival",
      "Satisfying metal clank + fueling glug for every salvage interaction",
    ],
    monetization_loops: [
      "Starter Class unlocks (Medic, Engineer, Scout, Armored Captain) earned via wins or Robux shortcut",
      "One-time Squad Revive token & Cosmetic Raft/Train skin packs (non-pay-to-win)",
      "Private Server lobby pass for 4-player friend squads chasing distance milestones",
    ],
    retention_hooks: [
      "Clear distance milestone checkpoints (10km, 25km, 50km, 100km) with escalating biome visuals & loot",
      "Day/Night cycle tension: scavenge fuel & repairs by day, defend the vehicle together at night",
      "Persistent Class XP & lobby achievement badges that carry across every expedition run",
    ],
    anti_slop_rules: [
      "Never leave players wandering an empty map: anchor every player to one shared moving vehicle/camp",
      "Telegraph nightfall & storm waves clearly with Lighting transitions, warning chimes, and HUD timers",
      "Handle player disconnects gracefully so remaining squadmates can still complete the run",
    ],
    automatic_twists: [
      "MODULAR DECK SNAP-BUILD: Weld scavenged turrets, lanterns, and armor panels directly onto live sockets on your moving raft/train",
      "WEATHER FORK JUNCTIONS: At each 25km split, vote as a squad between Safe Foggy Channel or High-Loot Storm Rapids",
      "SIGNAL FLARE AIRDROPS: Fire a flare during night waves to summon a high-tier supply crate that also attracts an elite miniboss",
    ],
    quality_boosts: [
      "Golden-hour daytime wilderness readability paired with cozy warm lantern pools at night (zero blind pitch-black screens)",
      "Top distance compass + ProximityPrompt interactions tuned for both desktop and 390px mobile screens",
      "Disconnect-safe squad scaling and clean DataStore milestone/class persistence",
    ],
    pitch_prompt:
      "Build a Co-op Survival Expedition game with a sunlit alpine river/route, shared Modular Deck Snap-Build vehicle, Weather Fork Junctions, day/night cycle hazards with lantern pools, Signal Flare supply drops, salvage upgrade shop, and server-authoritative DataStore milestone saving.",
  },
  {
    id: "horror-coop-mascot",
    name: "Co-op Horror & Mascot",
    tagline: "Stylized mascot-horror co-op floors with distinct character abilities, generators & chase tension.",
    craft_bar:
      "Stylized high-contrast mascot horror with readable moody rim lighting (never pitch-black soup), fair audio-telegraphed chases, and snappy 2-minute elevator floor cycles.",
    games: [
      { name: "Dandy's World", universe_id: 5721456859, ccu: 206000, like_ratio: 95.1, sort: "fun-with-friends", hook: "Co-op machine extraction + collectible mascot roster + floor scaling" },
      { name: "DOORS", universe_id: 2440500124, ccu: 89000, like_ratio: 94.8, sort: "top-revisited", hook: "Audio-telegraphed entity encounters + room progression + revives" },
      { name: "Piggy", universe_id: 1720936166, ccu: 44000, like_ratio: 92.0, sort: "top-revisited", hook: "Chapter story maps + asymmetric chase + trap skins" },
    ],
    gameplay_loop: [
      "2-minute floor loop: exit elevator -> complete skill-check generators -> sprint back before elevator doors close",
      "Co-op role synergy: Extractors finish objectives fast while Distractors kite roaming mascots around looping corridors",
      "Lobby hub lets players unlock new mascot characters, trinkets, and mastery skins with run currency",
    ],
    ui_ux_style: [
      "Bold stylized comic/retro-studio HUD showing Floor #, Remaining Objectives, Stamina bar, and 1-button Role Ability",
      "Crisp circular skill-check timing ring on machines with clear audio/visual hit zones",
      "Co-op teammate status badges showing down/chase alerts so squads coordinate rescues instantly",
    ],
    animations_and_juice: [
      "Heartbeat vignette + subtle FOV widen when a mascot spots you, fading smoothly on line-of-sight break",
      "Elevator gate rattle, floor counter tick, and celebratory floor-clear currency tally between rounds",
      "Expressive mascot idle/chase animations with glowing eye trails in moody corridors",
    ],
    model_and_world_3d_style: [
      "Stylized vintage studio / toy-workshop halls with warm amber sconces, cyan rim lights, and crisp silhouettes",
      "NEVER ship unplayable pitch-black corridors: maintain moody Atmosphere with clear floor guides and lit landmarks",
      "Clean looping floor layouts with sightline pillars, lockers, and unmistakable glowing generator beacons",
    ],
    audio_cadence: [
      "Distinct directional audio cues per entity (music box, heavy footsteps, static hum) telegraphing danger fairly",
      "Rhythmic wrench/skill-check ping -> power-up surge chord when the final generator lights the exit",
      "Cheerful-yet-eerie retro elevator muzak providing relief between tense floors",
    ],
    monetization_loops: [
      "Collectible playable mascot/survivor roster unlocked via run currency or instant pack bundles",
      "Cosmetic skins, flashlight/lantern trails, and lobby emote packs",
      "Single-run Team Revive & Extra Trinket Slot gamepasses",
    ],
    retention_hooks: [
      "Escalating Floor elevator loop with 2-minute objectives (complete machines -> sprint to elevator)",
      "Distinct mascot roles (Extractor, Distractor, Healer) encouraging 4-player friend coordination",
      "Character Mastery quests tracking floors cleared, rescues, and items used for exclusive skins",
    ],
    anti_slop_rules: [
      "Avoid cheap pitch-black unplayable corridors: use moody Atmosphere, Rim PointLights, and clear silhouettes",
      "Every patrol entity must have fair audio/visual aggro cues and line-of-sight escape routes",
      "Ensure elevator/round timers never softlock if a player leaves mid-floor",
    ],
    automatic_twists: [
      "POWER ROUTER OVERLOAD: Reroute floor power to illuminate one wing & stun pursuing mascots for 4 seconds while dimming another",
      "DUO HIGH-FIVE BOOST: Tag a teammate mid-chase to share a 25% speed burst and refill 30% stamina",
      "CURSED FLOOR MODIFIER VOTE: Inside the elevator, squads pick a risky modifier (Double Mascots / Flicker Lights) for 2x Run Currency",
    ],
    quality_boosts: [
      "Moody stylized rim-lit studio halls with clear silhouettes instead of pitch-black corridors",
      "Fair directional audio telegraphs + looping escape geometry so chases reward skill",
      "Disconnect-proof elevator round manager & clean skill-check timing UI",
    ],
    pitch_prompt:
      "Build a Stylized Co-op Mascot Horror Floor-Extraction game with readable rim-lit studio halls, patrol entities with fair audio cues, circular generator skill checks, Power Router Overload stun switches, elevator Cursed Floor modifier votes, unlockable trinket shop, and DataStore saving.",
  },
  {
    id: "tactical-arena-pvp",
    name: "Tactical Arena PvP",
    tagline: "Snappy 1v1 to 4v4 skill-based rounds with crisp hit feedback, loadouts, killstreaks & ranked ladders.",
    craft_bar:
      "Razor-sharp symmetrical sunlit arena with unambiguous team silhouettes, sub-5-second rematch tempo, server-validated hit/parry timing, and zero visual noise.",
    games: [
      { name: "RIVALS", universe_id: 6035872082, ccu: 175000, like_ratio: 94.9, sort: "top-playing-now", hook: "Ultra-fast 1v1/2v2 duel queues + crisp weapon skins + ranked Elo" },
      { name: "STAND OUT", universe_id: 10540998112, ccu: 35000, like_ratio: 91.4, sort: "up-and-coming", hook: "Stylized anime ability arena + clean parries + ranked queues" },
      { name: "BedWars", universe_id: 2619619496, ccu: 62000, like_ratio: 91.8, sort: "top-revisited", hook: "Kit rotation + squad objective defense + battle pass" },
    ],
    gameplay_loop: [
      "Walk onto a 1v1/2v2 Duel Pad -> instant teleport into symmetrical arena -> best-of-5 crisp rounds",
      "Sub-5-second rematch flow: Victory banner -> 1-click Rematch or auto-queue next challenger",
      "Win Streaks & Daily Duel Quests unlock cosmetic wraps, finishers, and ranked Elo tier badges",
    ],
    ui_ux_style: [
      "Esports-grade clean top scoreboard (Round pips, Timer, Team HP) + crisp ability cooldown slots",
      "Unmistakable hitmarkers, parry spark flashes, and floating damage numbers",
      "Lobby Duel Pad overhead pads showing live queue count & opponent win streak",
    ],
    animations_and_juice: [
      "2-frame hitstop + directional spark burst + crisp camera punch on clean parries and crits",
      "Stylized finisher VFX on round-winning elimination with fast cut back to spawn",
      "Smooth weapon equip/dash trails keeping silhouettes clean at 60fps",
    ],
    model_and_world_3d_style: [
      "Bright sunlit architectural competitive arenas (marble rooftops, cherry-blossom dojo, neon skybridge) with clean sightlines",
      "High figure-ground contrast: calm low-noise arena floor textures so player avatars & projectiles pop instantly",
      "Symmetrical mirrored cover geometry guaranteeing 100% competitive fairness",
    ],
    audio_cadence: [
      "Crisp metallic clang on timed parry, tight bass thump on guard break, sharp tick on every hitmarker",
      "3-2-1 round horn stinger -> celebratory victory chord on streak extension",
      "Distinct ability wind-up SFX so skilled players can react by ear",
    ],
    monetization_loops: [
      "Weapon/Blade finishers, animated wraps, kill-effect VFX, and Victory Emote crates",
      "Seasonal Battle Pass & Kit/Ability trial passes (balanced so skill always decides matches)",
      "Private Duel Tournament Server pass with spectator camera controls",
    ],
    retention_hooks: [
      "Sub-5-second rematch loop: Win/Loss banner -> automatic next round or instant 1-click Rematch",
      "Visible Win Streak counter & Rank Tier badge above every player in the lobby",
      "Daily Duel Quests (land 5 crits, win 2 rounds, use ability combo) awarding loadout keys",
    ],
    anti_slop_rules: [
      "Validate all weapon hits, cooldowns, and damage distances strictly on the server (anti-exploit)",
      "Use clean symmetrical arena cover and unambiguous team highlights so visibility stays 100% readable",
      "Prevent spawn-camping with brief spawn shields and clean round teleport resets",
    ],
    automatic_twists: [
      "PERFECT CLASH PARRY: Pressing block within 0.22s of a melee/projectile strike reflects 100% damage + resets dash cooldown",
      "SHIFTING ARENA RING: On Round 3+, outer skybridge cover tiles drop cleanly to force electric center-stage duels",
      "BOUNTY STREAK CROWN: Reaching a 5-win streak ignites a golden crown aura; defeating the crowned player pays a 3x bounty",
    ],
    quality_boosts: [
      "Esports-readable sunlit marble arena with calm floor contrast so players & hitboxes pop effortlessly",
      "2-frame hitstop + directional parry sparks + sub-5-second automatic rematch loop",
      "Strict server-side hit validation, cooldown checks, and anti-spawn-camp shields",
    ],
    pitch_prompt:
      "Build a sunlit Tactical Arena PvP game with symmetrical marble rooftop cover, fast 1v1/2v2 duel pads, server-validated melee/ability combat with Perfect Clash Parry reflect, Shifting Arena Ring tie-breakers, Bounty Streak Crowns, win-streak leaderboard, spectator camera, and DataStore rank saving.",
  },
  {
    id: "anime-action-rpg-td",
    name: "Anime Action RPG & TD",
    tagline: "Progression islands, elemental fruit/ability mastery, wave defense & mythic boss raids.",
    craft_bar:
      "Vibrant multi-island anime adventure with crisp quest waypoints, responsive elemental combo abilities, hourly stock refresh anticipation, and co-op boss raid portals.",
    games: [
      { name: "Blox Fruits", universe_id: 994732206, ccu: 312000, like_ratio: 93.4, sort: "top-playing-now", hook: "Island progression + rare ability fruits + sea boss raids" },
      { name: "Anime Defenders", universe_id: 5836869368, ccu: 64000, like_ratio: 92.1, sort: "top-revisited", hook: "Unit banners + mythic evolutions + infinite wave mode" },
      { name: "Fruit Battlegrounds", universe_id: 3457700596, ccu: 47000, like_ratio: 93.0, sort: "top-revisited", hook: "Spin ability rarities + high-impact combo arenas" },
    ],
    gameplay_loop: [
      "Accept island NPC quest -> clear elemental mobs/bosses with Z/X/C/V ability combos -> earn Level & Mastery XP",
      "Roll new elemental abilities/units at the Shrine Banner with guaranteed Mythic pity & hourly stock rotation",
      "Enter Co-op Boss Raid Portals scaling rewards with party damage contribution",
    ],
    ui_ux_style: [
      "Clean Z/X/C/V elemental skill bar with radial cooldown fills & clear Mastery unlock levels",
      "Compact right-side Quest Tracker card with 3D Beam waypoint guiding directly to the target",
      "Server-wide Hourly Stock / Raid Portal countdown pill driving return visits",
    ],
    animations_and_juice: [
      "Elemental slash arcs, ground-crack shockwaves, and knockback trails on combo finishers",
      "Boss telegraph danger rings on the ground before AoE slams so dodging feels fair and skillful",
      "Level-up golden pillar aura + floating mastery milestone popups",
    ],
    model_and_world_3d_style: [
      "Bright sunlit stylized islands (Starter Harbor, Sakura Cliff, Storm Citadel) connected by turquoise ocean & glowing raid portals",
      "Distinct landmark silhouettes per island so players orient immediately without opening a map",
      "Original elemental VFX palettes and cel-shaded homage silhouettes (zero copyrighted IP)",
    ],
    audio_cadence: [
      "Crisp elemental whoosh + resonant impact boom per Z/X/C/V ability tier",
      "Triumphant brass/synth level-up flourish (`level_up.wav`) and quest-complete chime",
      "Epic pulsing raid horn when a server World Boss portal opens",
    ],
    monetization_loops: [
      "2x Mastery / 2x Drop Chance / Fast Boats & Inventory Storage gamepasses",
      "Ability/Unit Reroll spins paired with guaranteed Mythic pity thresholds",
      "VIP Private Raid Server & cosmetic aura packs",
    ],
    retention_hooks: [
      "Quest-giver NPC progression loop with clear level gates, boss milestones, and mastery unlocks",
      "Hourly Stock / Banner Refresh countdown creating social anticipation across the whole server",
      "Co-op Boss Raid portals scaling rewards with party damage contribution",
    ],
    anti_slop_rules: [
      "Use original ability names, elemental VFX palettes, and homage silhouettes (zero copyrighted IP)",
      "Keep combat hitboxes responsive and server-checked with clear cooldown HUD indicators",
      "Save player level, mastery XP, unlocked abilities, and quest progress atomically on leave & BindToClose",
    ],
    automatic_twists: [
      "ELEMENTAL RESONANCE COMBOS: Chain two squadmates' elements (Flame + Storm -> Plasma Nova) inside Boss Raids for 2x stagger",
      "DYNAMIC ISLAND BOUNTY EVENTS: Every 20 minutes a roaming Sky Leviathan circles an island, dropping raid tokens to all participants",
      "MASTERY AWAKENING TRIAL: Hit Mastery 50 on any ability to unlock a 60-second solo trial that upgrades its VFX & passive",
    ],
    quality_boosts: [
      "3D quest waypoint beams + boss ground telegraph rings for effortless spatial readability",
      "Vibrant sunlit island architecture with distinct silhouettes and turquoise water contrast",
      "Atomic DataStore persistence with retry/BindToClose across level, mastery, inventory & quests",
    ],
    pitch_prompt:
      "Build a sunlit Anime Action RPG & Raid Hub with vibrant stylized islands, Z/X/C/V elemental combat with Elemental Resonance Combos, quest-giver NPCs with 3D waypoints, boss raid portals with ground telegraph rings, weighted ability spins with pity, inventory backpack, and DataStore progression.",
  },
  {
    id: "social-rp-fashion",
    name: "Social RP & Fashion",
    tagline: "Expressive avatar customization, cozy town roleplay, timed runway voting & social hangouts.",
    craft_bar:
      "Warm golden-hour cozy town & photography-grade runway plaza with zero-grind avatar expression, silky mobile-friendly wardrobe menus, and 4-minute social voting rounds.",
    games: [
      { name: "Brookhaven RP", universe_id: 1686885941, ccu: 564000, like_ratio: 90.4, sort: "top-playing-now", hook: "Zero-friction town houses, vehicles, props & roleplay jobs" },
      { name: "Dress to Impress", universe_id: 5203828273, ccu: 184000, like_ratio: 93.7, sort: "fun-with-friends", hook: "5-minute themed fashion runway + live podium voting + rank tiers" },
      { name: "Berry Avenue", universe_id: 3179796787, ccu: 51000, like_ratio: 91.6, sort: "top-revisited", hook: "Modern house customization + school/cafe social props" },
    ],
    gameplay_loop: [
      "Zero-grind entry: spawn straight into a sunlit plaza with instant free outfit/prop/vehicle presets",
      "4-minute themed fashion showcase or roleplay town event culminating in a live 5-star podium reveal",
      "Votes & daily hangouts level up Star Rank & unlock exclusive photo poses, studio backdrops, and estates",
    ],
    ui_ux_style: [
      "Clean icon-categorized side wardrobe/prop drawer sized generously for touch tablets and phones",
      "1-tap Photo Mode hiding all HUD chrome for instant clean avatar screenshots",
      "Snappy 5-star voting bar during runway walkouts with immediate live podium celebration",
    ],
    animations_and_juice: [
      "Sparkle wardrobe-swap puff + smooth camera orbit framing the player's avatar when trying outfits",
      "Runway spotlight sweep, confetti fountain on top-3 podium reveal, and expressive emote chains",
      "Cozy golden-hour -> twinkling twilight toggle with warm cafe string lights",
    ],
    model_and_world_3d_style: [
      "Warm flattering daylight Lighting (soft Bloom, balanced fill light, zero harsh dark shadows on avatars)",
      "Cozy pastel-and-timber town square, glass boutique salon, interactive cafe patio, and raised marble runway stage",
      "Clean prop/vehicle auto-cleanup on player leave so streets stay tidy and 60fps",
    ],
    audio_cadence: [
      "Warm lo-fi acoustic plaza soundtrack smoothly crossfading into upbeat synth pop during runway walkouts",
      "Soft sparkle chime on outfit swap, camera shutter click in Photo Mode, and applause burst on podium reveal",
      "Gentle mute/track switcher right in the corner for social voice-chat comfort",
    ],
    monetization_loops: [
      "VIP Lounge / Custom Makeup / Extra Outfit Slots & Premium Estate gamepasses",
      "Themed seasonal clothing packs, vehicle colors, and custom boombox/music passes",
      "Runway Pose Packs & Private Hangout Server passes",
    ],
    retention_hooks: [
      "Expressive social identity: instant outfit/house swapping with zero grind wall on first join",
      "Short 4-minute themed rounds or town events with photo-worthy podium/plaza backdrops",
      "Star Rank & Style Title progression unlocked by votes, visits, and daily check-ins",
    ],
    anti_slop_rules: [
      "Keep lighting warm, clean, and flattering (Day/Night toggle + soft Bloom) for avatar photography",
      "Organize UI menus into crisp icon categories so mobile/tablet players never fight cramped modals",
      "Ensure all vehicles, props, and interactive furniture clean up cleanly when a player leaves",
    ],
    automatic_twists: [
      "DUO CO-OP RUNWAY: Pair up with a friend for synchronized dual-pose walkouts scored on theme harmony",
      "LIVE PHOTO BOOTH BILLBOARDS: Top-voted outfits automatically appear on sunlit town-square holographic posters for the next round",
      "COZY WEATHER MOOD DIAL: Players vote between Golden Sunset, Sakura Breeze, or Starry Cafe Night lighting between rounds",
    ],
    quality_boosts: [
      "Flattering portrait-grade Lighting + 1-tap HUD-free Photo Mode for shareable screenshots",
      "Touch-friendly wardrobe & prop icon menus with sparkle swap transitions",
      "Automatic player-leave cleanup for all vehicles, houses, and interactive props",
    ],
    pitch_prompt:
      "Build a Cozy Social Roleplay & Fashion Showcase Plaza with flattering golden-hour lighting, raised runway stage, Duo Co-op Runway walkouts, Live Photo Booth Billboards for winners, Cozy Weather Mood voting, teleport portals, vehicle spawner, 1-tap Photo Mode, and DataStore Star Rank saving.",
  },
];

export function formatCcu(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "-";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n >= 100_000 ? 0 : 1)}k`;
  return String(Math.round(n));
}

export function formatCompactNumber(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "-";
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(2)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n >= 100_000 ? 0 : 1)}k`;
  return String(Math.round(n));
}

const EMPTY_NICHE: MarketTasteNiche = {
  id: "top-trending",
  name: "Top-Trending Roblox Craft",
  tagline: "High-retention Roblox experiences grounded in live Explore sorts.",
  craft_bar: "Immediate 3-second action hook, crisp diegetic feedback, 60 FPS input loop.",
  games: [],
  gameplay_loop: [],
  ui_ux_style: [],
  animations_and_juice: [],
  model_and_world_3d_style: [],
  audio_cadence: [],
  monetization_loops: [],
  retention_hooks: [],
  anti_slop_rules: [],
  automatic_twists: [],
  quality_boosts: [],
  pitch_prompt: "Build an original high-retention Roblox experience with crisp juice and clear core loop.",
};

export function defaultMarketTasteSnapshot(): MarketTasteSnapshot {
  return {
    active_niche: "top-trending",
    source: "baseline_nov_2026",
    updated_at: "2026-11-22",
    live_supported: false,
    live_games_count: 0,
    note: "Grounded in live Roblox Explore charts + 8-dimension Craft & Anti-Slop Dossier.",
    niches: [],
  };
}

function formatTimestamp(v: unknown, fallback: string): string {
  if (typeof v === "string" && v.trim()) return v.trim();
  if (typeof v === "number" && Number.isFinite(v) && v > 0) {
    const ms = v > 1e12 ? v : v * 1000;
    try {
      return new Date(ms).toISOString().slice(0, 16).replace("T", " ") + " UTC";
    } catch {
      return fallback;
    }
  }
  return fallback;
}

function strList(v: unknown, fb: string[]): string[] {
  if (typeof v === "string" && v.trim()) {
    return v
      .split(/\n+/)
      .map((s) => s.replace(/^[-•*]\s*/, "").trim())
      .filter(Boolean);
  }
  if (Array.isArray(v) && v.length > 0) {
    const out = v
      .map((item) => {
        if (typeof item === "string") return item.trim();
        if (item && typeof item === "object") {
          const obj = item as Record<string, unknown>;
          const title = typeof obj.title === "string" ? obj.title : typeof obj.name === "string" ? obj.name : "";
          const desc = typeof obj.description === "string" ? obj.description : typeof obj.detail === "string" ? obj.detail : "";
          return title && desc ? `${title}: ${desc}` : title || desc;
        }
        return "";
      })
      .filter((s) => s.length > 0);
    if (out.length > 0) return out;
  }
  return fb;
}

function normalizeGames(raw: unknown, fb: MarketTasteGame[]): MarketTasteGame[] {
  if (!Array.isArray(raw) || raw.length === 0) return fb;
  const out = raw
    .filter((g): g is Record<string, unknown> => Boolean(g && typeof g === "object"))
    .map((g): MarketTasteGame => {
      const rawRatio =
        typeof g.like_ratio_pct === "number"
          ? g.like_ratio_pct
          : typeof g.like_ratio === "number"
            ? g.like_ratio
            : typeof g.like_pct === "number"
              ? g.like_pct
              : typeof g.rating_pct === "number"
                ? g.rating_pct
                : 92;
      const like_ratio = rawRatio <= 1 && rawRatio > 0 ? Number((rawRatio * 100).toFixed(1)) : Number(rawRatio.toFixed(1));
      const sortTags = Array.isArray(g.sort_tags)
        ? g.sort_tags.filter((s): s is string => typeof s === "string" && s.trim().length > 0)
        : undefined;
      return {
        name: String(g.name ?? g.title ?? "Roblox Game"),
        universe_id: typeof g.universe_id === "number" ? g.universe_id : undefined,
        root_place_id: typeof g.root_place_id === "number" ? g.root_place_id : undefined,
        ccu: Number(g.ccu ?? g.playing ?? g.player_count ?? 0) || 0,
        like_ratio,
        sort: String(g.sort ?? g.sort_id ?? g.chart ?? sortTags?.[0] ?? "top-trending"),
        sort_tags: sortTags,
        hook:
          typeof g.why_it_wins === "string"
            ? g.why_it_wins
            : typeof g.hook === "string"
              ? g.hook
              : undefined,
        source: typeof g.source === "string" ? g.source : undefined,
      };
    });
  return out.length > 0 ? out : fb;
}

function mergeNiche(raw: Record<string, unknown>, base: MarketTasteNiche): MarketTasteNiche {
  const dossier =
    raw.taste_dossier && typeof raw.taste_dossier === "object"
      ? (raw.taste_dossier as Record<string, unknown>)
      : raw.dossier && typeof raw.dossier === "object"
        ? (raw.dossier as Record<string, unknown>)
        : raw;
  const rawGames = raw.top_games ?? raw.games ?? raw.market_leaders ?? dossier.market_leaders ?? raw.leaderboard;
  return {
    id: String(raw.id ?? base.id).trim() || base.id,
    name: String(raw.name ?? raw.label ?? raw.title ?? base.name),
    tagline: String(raw.tagline ?? raw.summary ?? raw.description ?? base.tagline),
    craft_bar: String(raw.craft_bar ?? dossier.craft_bar ?? base.craft_bar),
    games: normalizeGames(rawGames, base.games),
    gameplay_loop: strList(raw.gameplay_loop ?? dossier.gameplay_loop, base.gameplay_loop),
    ui_ux_style: strList(raw.ui_ux_style ?? dossier.ui_ux_style, base.ui_ux_style),
    animations_and_juice: strList(raw.animations_and_juice ?? dossier.animations_and_juice, base.animations_and_juice),
    model_and_world_3d_style: strList(
      raw.model_and_world_3d_style ?? dossier.model_and_world_3d_style ?? raw.world_3d_style,
      base.model_and_world_3d_style,
    ),
    audio_cadence: strList(raw.audio_cadence ?? dossier.audio_cadence, base.audio_cadence),
    monetization_loops: strList(
      raw.monetization_loops ?? dossier.monetization_loops ?? raw.monetization_and_retention ?? raw.monetization,
      base.monetization_loops,
    ),
    retention_hooks: strList(raw.retention_hooks ?? dossier.retention_hooks ?? raw.retention, base.retention_hooks),
    anti_slop_rules: strList(
      raw.anti_slop_guardrails ?? raw.anti_slop_rules ?? dossier.anti_slop_guardrails ?? dossier.anti_slop_rules ?? raw.anti_slop,
      base.anti_slop_rules,
    ),
    automatic_twists: strList(raw.automatic_twists ?? dossier.automatic_twists ?? raw.twists, base.automatic_twists),
    quality_boosts: strList(raw.quality_boosts ?? dossier.quality_boosts, base.quality_boosts),
    pitch_prompt: String(raw.pitch_prompt ?? dossier.pitch_prompt ?? raw.pitch_hook ?? base.pitch_prompt),
  };
}

export function normalizeMarketTasteSnapshot(raw: unknown): MarketTasteSnapshot {
  const fallback = defaultMarketTasteSnapshot();
  if (!raw || typeof raw !== "object") return fallback;
  const o = raw as Record<string, unknown>;
  const inner = o.snapshot && typeof o.snapshot === "object" ? (o.snapshot as Record<string, unknown>) : o;

  const detailedNiche =
    inner.niche && typeof inner.niche === "object" ? (inner.niche as Record<string, unknown>) : null;
  const detailedId = detailedNiche ? String(detailedNiche.id ?? "").trim() : "";

  const rawNiches = Array.isArray(inner.niches)
    ? inner.niches
    : Array.isArray(inner.niche_profiles)
      ? inner.niche_profiles
      : [];

  const byId = new Map<string, MarketTasteNiche>();

  for (const item of rawNiches) {
    if (!item || typeof item !== "object") continue;
    const rec = item as Record<string, unknown>;
    const id = String(rec.id ?? rec.slug ?? "").trim();
    if (!id) continue;
    const base = byId.get(id) ?? { ...EMPTY_NICHE, id, name: String(rec.name ?? rec.label ?? id) };
    byId.set(id, mergeNiche(rec, base));
  }

  if (detailedNiche && detailedId) {
    const base = byId.get(detailedId) ?? { ...EMPTY_NICHE, id: detailedId };
    byId.set(detailedId, mergeNiche(detailedNiche, base));
  }

  const niches = [...byId.values()];
  const rawActive = String(inner.selected_niche ?? inner.active_niche ?? detailedId ?? "").trim();
  const active = niches.some((n) => n.id === rawActive) ? rawActive : niches[0]?.id || "top-trending";
  const source = typeof inner.source === "string" && inner.source ? inner.source : fallback.source;
  const liveCount = typeof inner.live_games_count === "number" ? inner.live_games_count : 0;

  return {
    active_niche: active,
    source,
    updated_at: formatTimestamp(
      inner.last_refreshed_ms || inner.updated_at || inner.refreshed_at || inner.baseline_date,
      fallback.updated_at,
    ),
    live_supported: inner.live_supported === true || source.includes("live") || liveCount > 0,
    live_games_count: liveCount,
    note: typeof inner.note === "string" ? inner.note : typeof inner.message === "string" ? inner.message : fallback.note,
    niches,
  };
}

export function normalizeResearchResult(raw: unknown, fallbackNicheId = "top-trending"): MarketTasteResearchResult {
  const baseNiche = { ...EMPTY_NICHE, id: fallbackNicheId };
  if (!raw || typeof raw !== "object") {
    return {
      ok: false,
      name: "Unknown Roblox Game",
      universe_id: 0,
      ccu: 0,
      visits: 0,
      favorites: 0,
      like_ratio: 0,
      niche_id: baseNiche.id,
      niche_name: baseNiche.name,
      thumbnails: [],
      craft_bar: baseNiche.craft_bar,
      gameplay_loop: baseNiche.gameplay_loop,
      ui_ux_style: baseNiche.ui_ux_style,
      animations_and_juice: baseNiche.animations_and_juice,
      model_and_world_3d_style: baseNiche.model_and_world_3d_style,
      audio_cadence: baseNiche.audio_cadence,
      monetization_loops: baseNiche.monetization_loops,
      retention_hooks: baseNiche.retention_hooks,
      anti_slop_rules: baseNiche.anti_slop_rules,
      automatic_twists: baseNiche.automatic_twists,
      quality_boosts: baseNiche.quality_boosts,
      pitch_prompt: baseNiche.pitch_prompt,
      error: "Empty research response.",
    };
  }
  const o = raw as Record<string, unknown>;
  const dossier =
    o.dossier && typeof o.dossier === "object"
      ? (o.dossier as Record<string, unknown>)
      : o.taste_dossier && typeof o.taste_dossier === "object"
        ? (o.taste_dossier as Record<string, unknown>)
        : o;
  const game =
    o.game && typeof o.game === "object"
      ? (o.game as Record<string, unknown>)
      : dossier.game && typeof dossier.game === "object"
        ? (dossier.game as Record<string, unknown>)
        : o;

  const rawNicheId = String(o.niche_id ?? dossier.niche_id ?? dossier.niche ?? fallbackNicheId).trim();
  const niche = { ...EMPTY_NICHE, id: rawNicheId || fallbackNicheId, name: String(o.niche_name ?? dossier.niche_name ?? rawNicheId) };

  const rawThumbs = Array.isArray(o.thumbnails)
    ? o.thumbnails
    : Array.isArray(dossier.thumbnails)
      ? dossier.thumbnails
      : Array.isArray(game.thumbnails)
        ? game.thumbnails
        : [];
  const thumbnails = rawThumbs
    .map((t) => {
      if (typeof t === "string") return t.trim();
      if (t && typeof t === "object") {
        const rec = t as Record<string, unknown>;
        return String(rec.imageUrl ?? rec.image_url ?? rec.url ?? "").trim();
      }
      return "";
    })
    .filter((u) => u.length > 0)
    .slice(0, 6);

  const rawRatio =
    typeof game.like_ratio_pct === "number"
      ? game.like_ratio_pct
      : typeof o.like_ratio_pct === "number"
        ? o.like_ratio_pct
        : typeof game.like_ratio === "number"
          ? game.like_ratio
          : typeof o.like_ratio === "number"
            ? o.like_ratio
            : 93.4;
  const like_ratio = rawRatio <= 1 && rawRatio > 0 ? Number((rawRatio * 100).toFixed(1)) : Number(rawRatio.toFixed(1));

  const name = String(game.name ?? o.name ?? dossier.game_name ?? "Researched Roblox Game").trim();
  const twists = strList(o.automatic_twists ?? dossier.automatic_twists ?? dossier.twists, niche.automatic_twists);
  const boosts = strList(o.quality_boosts ?? dossier.quality_boosts, niche.quality_boosts);
  const defaultPitch = `Build an original ${niche.name} Roblox experience inspired by the proven craft bar of ${name} (similar feel, higher craft & fresh twists): apply ${twists.slice(0, 3).join("; ")}.`;

  return {
    ok: o.ok !== false,
    name,
    universe_id: Number(game.universe_id ?? o.universe_id ?? dossier.universe_id ?? 0) || 0,
    place_id: Number(game.root_place_id ?? game.place_id ?? o.place_id ?? 0) || undefined,
    creator: typeof game.creator === "string" ? game.creator : typeof o.creator === "string" ? o.creator : undefined,
    ccu: Number(game.playing ?? game.ccu ?? o.playing ?? o.ccu ?? 0) || 0,
    visits: Number(game.visits ?? o.visits ?? 0) || 0,
    favorites: Number(game.favorited_count ?? game.favorites ?? o.favorites ?? 0) || 0,
    like_ratio,
    genre: typeof game.genre === "string" ? game.genre : typeof o.genre === "string" ? o.genre : undefined,
    niche_id: niche.id,
    niche_name: niche.name,
    thumbnails,
    craft_bar: String(dossier.craft_bar ?? o.craft_bar ?? niche.craft_bar),
    gameplay_loop: strList(dossier.gameplay_loop ?? o.gameplay_loop, niche.gameplay_loop),
    ui_ux_style: strList(dossier.ui_ux_style ?? o.ui_ux_style, niche.ui_ux_style),
    animations_and_juice: strList(dossier.animations_and_juice ?? o.animations_and_juice, niche.animations_and_juice),
    model_and_world_3d_style: strList(
      dossier.model_and_world_3d_style ?? o.model_and_world_3d_style,
      niche.model_and_world_3d_style,
    ),
    audio_cadence: strList(dossier.audio_cadence ?? o.audio_cadence, niche.audio_cadence),
    monetization_loops: strList(
      dossier.monetization_loops ?? dossier.monetization_and_retention ?? o.monetization_loops,
      niche.monetization_loops,
    ),
    retention_hooks: strList(dossier.retention_hooks ?? o.retention_hooks, niche.retention_hooks),
    anti_slop_rules: strList(
      dossier.anti_slop_guardrails ?? dossier.anti_slop_rules ?? o.anti_slop_guardrails ?? o.anti_slop_rules,
      niche.anti_slop_rules,
    ),
    automatic_twists: twists,
    quality_boosts: boosts,
    pitch_prompt: String(o.pitch_prompt ?? dossier.pitch_prompt ?? defaultPitch),
    dossier_path:
      typeof o.markdown_path === "string"
        ? o.markdown_path
        : typeof o.dossier_md_path === "string"
          ? o.dossier_md_path
          : typeof o.dossier_path === "string"
            ? o.dossier_path
            : typeof o.json_path === "string"
              ? o.json_path
              : undefined,
    offline_fallback: o.offline_fallback === true,
    note: typeof o.note === "string" ? o.note : undefined,
    error: typeof o.error === "string" ? o.error : undefined,
  };
}
