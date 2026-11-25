//! Rune-backed code-generation skills.
//!
//! The Luau the agent applies is genuinely produced by executing embedded
//! Rune scripts (`.rn`) on a Rune VM — there is no Rust-side string template
//! for game code. Every skill is one Rune script with one entry function;
//! the registry below is the single source the agent's system prompt and
//! the dispatch both read, so a skill that exists is a skill the model is
//! told about, with the parameters it actually takes.

use serde_json::{json, Value};
use std::sync::Arc;

const ROUND_MANAGER_RN: &str = include_str!("round_manager.rn");
const LEADERBOARD_RN: &str = include_str!("leaderboard.rn");
const DAY_NIGHT_RN: &str = include_str!("day_night.rn");
const KILL_BRICK_RN: &str = include_str!("kill_brick.rn");
const SPAWN_LINK_RN: &str = include_str!("spawn_link.rn");
const CURRENCY_RN: &str = include_str!("currency.rn");
const SHOP_RN: &str = include_str!("shop.rn");
const GAME_GUI_RN: &str = include_str!("game_gui.rn");
const CHECKPOINTS_RN: &str = include_str!("checkpoints.rn");
const SOUNDTRACK_RN: &str = include_str!("soundtrack.rn");

const JUMP_PAD_RN: &str = include_str!("jump_pad.rn");
const MOVING_PLATFORM_RN: &str = include_str!("moving_platform.rn");
const DATASTORE_SAVE_RN: &str = include_str!("datastore_save.rn");
const NPC_PATROL_RN: &str = include_str!("npc_patrol.rn");
const PORTAL_TELEPORTER_RN: &str = include_str!("portal_teleporter.rn");
const COMBAT_SWORD_RN: &str = include_str!("combat_sword.rn");
const INVENTORY_BACKPACK_RN: &str = include_str!("inventory_backpack.rn");
const QUEST_TRACKER_RN: &str = include_str!("quest_tracker.rn");
const VEHICLE_SPAWNER_RN: &str = include_str!("vehicle_spawner.rn");
const WEATHER_CYCLE_RN: &str = include_str!("weather_cycle.rn");
const TYCOON_DROPPER_RN: &str = include_str!("tycoon_dropper.rn");
const ADMIN_SPECTATE_RN: &str = include_str!("admin_spectate.rn");

const CARD_GACHA_ROLLER_RN: &str = include_str!("card_gacha_roller.rn");
const DECK_BUILDER_RN: &str = include_str!("deck_builder.rn");
const CARD_BATTLE_ARENA_RN: &str = include_str!("card_battle_arena.rn");
const TOWER_PROGRESSION_RN: &str = include_str!("tower_progression.rn");
const POTIONS_WEATHER_RN: &str = include_str!("potions_weather.rn");
const CARD_GALLERY_WORLD_RN: &str = include_str!("card_gallery_world.rn");

#[derive(Clone, Copy)]
pub struct SkillDef {
    pub name: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    /// Human-readable parameter spec, shown to the model in the prompt.
    pub params: &'static str,
    /// Recommended target script path in the Roblox DataModel.
    pub default_path: &'static str,
    /// Valid JSON default arguments for one-click Blueprint Library insertion.
    pub default_args: &'static str,
    source: &'static str,
    entry: &'static str,
}

/// Fast count of registered skills without allocating.
pub fn count() -> usize {
    REGISTRY.len()
}

/// The skill registry: all 28 verified, composable Roblox game-building skills.
pub fn registry() -> &'static [SkillDef] {
    REGISTRY
}

const REGISTRY: &[SkillDef] = &[
        SkillDef {
            name: "round_manager",
            title: "Round Manager",
            category: "Combat",
            description: "a RoundManager script (ServerScriptService) that starts the round; with create_round it first creates the workspace.Round folder it starts",
            params: "{\"create_round\": bool}",
            default_path: "ServerScriptService.RoundManager",
            default_args: "{\"create_round\": true}",
            source: ROUND_MANAGER_RN,
            entry: "generate_round_manager",
        },
        SkillDef {
            name: "leaderboard",
            title: "Leaderboard",
            category: "Economy",
            description: "a Leaderboard script (ServerScriptService) that gives every joining player a leaderstats folder with one currency stat",
            params: "{\"currency\": string, \"start_amount\": int}",
            default_path: "ServerScriptService.Leaderboard",
            default_args: "{\"currency\": \"Coins\", \"start_amount\": 0}",
            source: LEADERBOARD_RN,
            entry: "generate_leaderboard",
        },
        SkillDef {
            name: "day_night",
            title: "Day / Night Cycle",
            category: "World",
            description: "a DayNight script (ServerScriptService) that cycles Lighting.ClockTime through a full day",
            params: "{\"cycle_minutes\": int}",
            default_path: "ServerScriptService.DayNight",
            default_args: "{\"cycle_minutes\": 10}",
            source: DAY_NIGHT_RN,
            entry: "generate_day_night",
        },
        SkillDef {
            name: "kill_brick",
            title: "Kill Brick",
            category: "Combat",
            description: "a KillBrick script placed inside a part; any humanoid touching the part takes damage",
            params: "{\"damage\": int}",
            default_path: "Workspace.LavaBrick.KillBrick",
            default_args: "{\"damage\": 100}",
            source: KILL_BRICK_RN,
            entry: "generate_kill_brick",
        },
        SkillDef {
            name: "spawn_link",
            title: "Spawn Link",
            category: "Movement",
            description: "a SpawnLink script (ServerScriptService) that links to a named Workspace instance at startup — write it, playtest, and if the instance is missing the playtest says so and you create it",
            params: "{\"instance_name\": string}",
            default_path: "ServerScriptService.SpawnLink",
            default_args: "{\"instance_name\": \"SpawnPad\"}",
            source: SPAWN_LINK_RN,
            entry: "generate_spawn_link",
        },
        SkillDef {
            name: "currency",
            title: "Saved Currency",
            category: "Economy",
            description: "a Currency script (ServerScriptService): leaderstats whose balance is saved in a DataStore between sessions — loaded on join, topped up passively over time, saved on leave and at shutdown (bind_close). Supersedes the plain leaderboard skill when the game needs money that persists",
            params: "{\"currency\": string, \"start_amount\": int, \"earn_amount\": int, \"earn_seconds\": int, \"bind_close\": bool}",
            default_path: "ServerScriptService.Currency",
            default_args: "{\"currency\": \"Coins\", \"start_amount\": 0, \"earn_amount\": 10, \"earn_seconds\": 5, \"bind_close\": true}",
            source: CURRENCY_RN,
            entry: "generate_currency",
        },
        SkillDef {
            name: "shop",
            title: "Item Shop",
            category: "Economy",
            description: "a Shop script (ServerScriptService) that creates the OpenROShopBuy RemoteEvent in ReplicatedStorage and sells one item for the currency: checks the buyer's balance, deducts the price, records the purchase on the player",
            params: "{\"item_name\": string, \"price\": int, \"currency\": string}",
            default_path: "ServerScriptService.Shop",
            default_args: "{\"item_name\": \"Speed Coil\", \"price\": 100, \"currency\": \"Coins\"}",
            source: SHOP_RN,
            entry: "generate_shop",
        },
        SkillDef {
            name: "game_gui",
            title: "Game HUD & Shop GUI",
            category: "UI",
            description: "a GameGui LocalScript — it belongs in StarterGui, NOT ServerScriptService: a title, a live balance label that follows the player's currency stat, and a shop button that fires the shop's OpenROShopBuy RemoteEvent. The game also needs the shop skill, or the playtest fails because the button has nothing to fire",
            params: "{\"title\": string, \"button_label\": string, \"currency\": string}",
            default_path: "StarterGui.GameGui",
            default_args: "{\"title\": \"My Game\", \"button_label\": \"Shop\", \"currency\": \"Coins\"}",
            source: GAME_GUI_RN,
            entry: "generate_game_gui",
        },
        SkillDef {
            name: "checkpoints",
            title: "Stage Checkpoints",
            category: "Movement",
            description: "a Checkpoints script (ServerScriptService): stage pads that record each player's progress and respawn them at their highest stage; with create_folder it first creates the checkpoints folder and pads it uses",
            params: "{\"stage_count\": int, \"folder_name\": string, \"create_folder\": bool}",
            default_path: "ServerScriptService.Checkpoints",
            default_args: "{\"stage_count\": 5, \"folder_name\": \"Checkpoints\", \"create_folder\": true}",
            source: CHECKPOINTS_RN,
            entry: "generate_checkpoints",
        },
        SkillDef {
            name: "soundtrack",
            title: "Soundtrack",
            category: "World",
            description: "a Soundtrack script (ServerScriptService) that plays a music track from SoundService for every player; sound_id 0 leaves the track silent until a real audio id is chosen",
            params: "{\"sound_id\": int, \"volume\": int (tenths, 0-10), \"looped\": bool}",
            default_path: "ServerScriptService.Soundtrack",
            default_args: "{\"sound_id\": 0, \"volume\": 5, \"looped\": true}",
            source: SOUNDTRACK_RN,
            entry: "generate_soundtrack",
        },
        SkillDef {
            name: "jump_pad",
            title: "Jump Pad",
            category: "Movement",
            description: "a JumpPad script (ServerScriptService): velocity bounce pads with per-player cooldown, particle burst FX, and bounce sound; with create_pad it creates the neon pad in Workspace",
            params: "{\"boost_power\": int, \"cooldown_ms\": int, \"pad_name\": string, \"create_pad\": bool}",
            default_path: "ServerScriptService.JumpPad",
            default_args: "{\"boost_power\": 120, \"cooldown_ms\": 600, \"pad_name\": \"JumpPad\", \"create_pad\": true}",
            source: JUMP_PAD_RN,
            entry: "generate_jump_pad",
        },
        SkillDef {
            name: "moving_platform",
            title: "Moving Platform",
            category: "Movement",
            description: "a MovingPlatform script (ServerScriptService): RunService.Heartbeat + TweenService moving platform that carries standing players smoothly along X, Y, or Z without slipping",
            params: "{\"platform_name\": string, \"travel_distance\": int, \"travel_seconds\": int, \"axis\": string (X|Y|Z), \"create_platform\": bool}",
            default_path: "ServerScriptService.MovingPlatform",
            default_args: "{\"platform_name\": \"MovingPlatform\", \"travel_distance\": 24, \"travel_seconds\": 4, \"axis\": \"X\", \"create_platform\": true}",
            source: MOVING_PLATFORM_RN,
            entry: "generate_moving_platform",
        },
        SkillDef {
            name: "datastore_save",
            title: "DataStore Save Engine",
            category: "Persistence",
            description: "a DataStoreSave script (ServerScriptService): pcall-guarded DataStoreService leaderstats load/save with retry backoff, UpdateAsync, bad-load protection, periodic autosave, and parallel BindToClose shutdown save",
            params: "{\"store_name\": string, \"stat_name\": string, \"default_value\": int, \"autosave_seconds\": int, \"bind_close\": bool}",
            default_path: "ServerScriptService.DataStoreSave",
            default_args: "{\"store_name\": \"OpenROPlayerData\", \"stat_name\": \"Coins\", \"default_value\": 0, \"autosave_seconds\": 60, \"bind_close\": true}",
            source: DATASTORE_SAVE_RN,
            entry: "generate_datastore_save",
        },
        SkillDef {
            name: "npc_patrol",
            title: "NPC Patrol Guard",
            category: "Combat",
            description: "an NpcPatrol script (ServerScriptService): PathfindingService waypoint patrolling NPC guard with overhead BillboardGui dialogue and debounced contact damage",
            params: "{\"npc_name\": string, \"waypoint_count\": int, \"walk_speed\": int, \"damage\": int, \"dialogue\": string}",
            default_path: "ServerScriptService.NpcPatrol",
            default_args: "{\"npc_name\": \"GuardNPC\", \"waypoint_count\": 4, \"walk_speed\": 12, \"damage\": 15, \"dialogue\": \"Halt! Restricted zone.\"}",
            source: NPC_PATROL_RN,
            entry: "generate_npc_patrol",
        },
        SkillDef {
            name: "portal_teleporter",
            title: "Portal Teleporter",
            category: "Movement",
            description: "a PortalTeleporter script (ServerScriptService): paired CFrame teleporters (PortalA <-> PortalB) with per-player cooldown debounce and warp particle FX",
            params: "{\"pair_name\": string, \"cooldown_seconds\": int, \"offset_distance\": int, \"create_portals\": bool}",
            default_path: "ServerScriptService.PortalTeleporter",
            default_args: "{\"pair_name\": \"WarpPortal\", \"cooldown_seconds\": 3, \"offset_distance\": 60, \"create_portals\": true}",
            source: PORTAL_TELEPORTER_RN,
            entry: "generate_portal_teleporter",
        },
        SkillDef {
            name: "combat_sword",
            title: "Combat Sword",
            category: "Combat",
            description: "a CombatSword script (ServerScriptService): StarterPack melee sword Tool with server-validated hit range, equipped check, cooldown, damage, and KOs leaderboard hook",
            params: "{\"sword_name\": string, \"damage\": int, \"cooldown_ms\": int, \"range_studs\": int}",
            default_path: "ServerScriptService.CombatSword",
            default_args: "{\"sword_name\": \"ClassicSword\", \"damage\": 25, \"cooldown_ms\": 450, \"range_studs\": 10}",
            source: COMBAT_SWORD_RN,
            entry: "generate_combat_sword",
        },
        SkillDef {
            name: "inventory_backpack",
            title: "Inventory Backpack",
            category: "Persistence",
            description: "an InventoryBackpack script (ServerScriptService): server-authoritative item inventory with max slots, replicated Inventory folder, and validated equip/unequip RemoteEvent",
            params: "{\"starter_item\": string, \"max_slots\": int, \"remote_name\": string}",
            default_path: "ServerScriptService.InventoryBackpack",
            default_args: "{\"starter_item\": \"Torch\", \"max_slots\": 12, \"remote_name\": \"OpenROInventoryEquip\"}",
            source: INVENTORY_BACKPACK_RN,
            entry: "generate_inventory_backpack",
        },
        SkillDef {
            name: "quest_tracker",
            title: "Quest Tracker",
            category: "Economy",
            description: "a QuestTracker script (ServerScriptService): server-authoritative objective milestone tracker with progress RemoteEvent, ServerStorage BindableEvent advance signal, and currency payout",
            params: "{\"quest_name\": string, \"target_count\": int, \"reward_amount\": int, \"currency\": string}",
            default_path: "ServerScriptService.QuestTracker",
            default_args: "{\"quest_name\": \"Crystal Hunt\", \"target_count\": 5, \"reward_amount\": 150, \"currency\": \"Coins\"}",
            source: QUEST_TRACKER_RN,
            entry: "generate_quest_tracker",
        },
        SkillDef {
            name: "vehicle_spawner",
            title: "Vehicle Spawner",
            category: "Movement",
            description: "a VehicleSpawner script (ServerScriptService): ProximityPrompt pad spawner that builds a welded VehicleSeat chassis with a strict 1-vehicle-per-player limit and automatic leave cleanup",
            params: "{\"vehicle_name\": string, \"max_speed\": int, \"cooldown_seconds\": int, \"create_pad\": bool}",
            default_path: "ServerScriptService.VehicleSpawner",
            default_args: "{\"vehicle_name\": \"Rover\", \"max_speed\": 65, \"cooldown_seconds\": 5, \"create_pad\": true}",
            source: VEHICLE_SPAWNER_RN,
            entry: "generate_vehicle_spawner",
        },
        SkillDef {
            name: "weather_cycle",
            title: "Weather Cycle",
            category: "World",
            description: "a WeatherCycle script (ServerScriptService): dynamic Lighting & Atmosphere transitions between Clear, Dusk, Fog, and Rain via TweenService with optional rain ParticleEmitter",
            params: "{\"cycle_seconds\": int, \"start_weather\": string (Clear|Rain|Fog|Dusk), \"enable_particles\": bool}",
            default_path: "ServerScriptService.WeatherCycle",
            default_args: "{\"cycle_seconds\": 45, \"start_weather\": \"Clear\", \"enable_particles\": true}",
            source: WEATHER_CYCLE_RN,
            entry: "generate_weather_cycle",
        },
        SkillDef {
            name: "tycoon_dropper",
            title: "Tycoon Dropper",
            category: "Economy",
            description: "a TycoonDropper script (ServerScriptService): complete dropper -> conveyor -> collector -> x2 purchase button upgrader loop that credits and charges leaderstats currency",
            params: "{\"tycoon_name\": string, \"drop_value\": int, \"drop_interval\": int, \"upgrade_cost\": int, \"currency\": string}",
            default_path: "ServerScriptService.TycoonDropper",
            default_args: "{\"tycoon_name\": \"FactoryTycoon\", \"drop_value\": 10, \"drop_interval\": 2, \"upgrade_cost\": 100, \"currency\": \"Coins\"}",
            source: TYCOON_DROPPER_RN,
            entry: "generate_tycoon_dropper",
        },
        SkillDef {
            name: "admin_spectate",
            title: "Admin Spectate & Status Feed",
            category: "UI",
            description: "an AdminSpectate script (ServerScriptService): server-validated spectator target cycling RemoteEvent (toggle/next/prev/status) and live OpenRORoundStatus feed in ReplicatedStorage",
            params: "{\"remote_name\": string, \"cycle_interval\": int, \"allow_all_players\": bool}",
            default_path: "ServerScriptService.AdminSpectate",
            default_args: "{\"remote_name\": \"OpenROSpectate\", \"cycle_interval\": 5, \"allow_all_players\": true}",
            source: ADMIN_SPECTATE_RN,
            entry: "generate_admin_spectate",
        },
        SkillDef {
            name: "card_gacha_roller",
            title: "Card Gacha Roller",
            category: "Economy",
            description: "a CardGachaRoller script (ServerScriptService): server-authoritative card roller across 7 rarity tiers (Common to Secret) and 5 foil border variants (Normal, Shiny, Gold, Rainbow, Void) with luck, weather, potion multipliers, pity counter, and fast/auto roll RemoteFunction",
            params: "{\"base_luck_pct\": int, \"pity_threshold\": int, \"fast_roll_ms\": int, \"enable_borders\": bool}",
            default_path: "ServerScriptService.CardGachaRoller",
            default_args: "{\"base_luck_pct\": 100, \"pity_threshold\": 50, \"fast_roll_ms\": 350, \"enable_borders\": true}",
            source: CARD_GACHA_ROLLER_RN,
            entry: "generate_card_gacha_roller",
        },
        SkillDef {
            name: "deck_builder",
            title: "Deck Builder & Rebirth",
            category: "Persistence",
            description: "a DeckBuilder script (ServerScriptService): 6-slot active card deck, Collection Index team bonuses, duplicate Star Fusion rank-ups, Rebirth Ascension multipliers, Auto-Equip Best, and pcall-guarded DataStore persistence with BindToClose",
            params: "{\"deck_slots\": int, \"fusion_cost\": int, \"rebirth_floor\": int, \"store_name\": string, \"bind_close\": bool}",
            default_path: "ServerScriptService.DeckBuilder",
            default_args: "{\"deck_slots\": 6, \"fusion_cost\": 3, \"rebirth_floor\": 10, \"store_name\": \"OpenROApexDecks_v1\", \"bind_close\": true}",
            source: DECK_BUILDER_RN,
            entry: "generate_deck_builder",
        },
        SkillDef {
            name: "card_battle_arena",
            title: "Tactical Card Battle Arena",
            category: "Combat",
            description: "a CardBattleArena script (ServerScriptService): tactical and auto turn-based 3v3/6-card combat arena with 5 Elemental Synergies (Flame, Void, Storm, Shadow, Arcane), Speed initiative, Ultimate Burst Gauge, Active Counter/Interrupt timing, Crit/Shield/Stun/Burn status effects, and multi-phase Impossible Boss Raids",
            params: "{\"team_size\": int, \"ultimate_charge_max\": int, \"counter_window_ms\": int, \"boss_hp_scale\": int}",
            default_path: "ServerScriptService.CardBattleArena",
            default_args: "{\"team_size\": 4, \"ultimate_charge_max\": 100, \"counter_window_ms\": 800, \"boss_hp_scale\": 4}",
            source: CARD_BATTLE_ARENA_RN,
            entry: "generate_card_battle_arena",
        },
        SkillDef {
            name: "tower_progression",
            title: "Endless Anime Tower",
            category: "Movement",
            description: "a TowerProgression script (ServerScriptService): 50-floor Endless Anime Tower with 5 themed World Portals in Workspace, boss milestone rewards, and a persistent OrderedDataStore global Tower Leaderboard",
            params: "{\"max_floors\": int, \"world_count\": int, \"base_reward\": int, \"store_name\": string, \"bind_close\": bool}",
            default_path: "ServerScriptService.TowerProgression",
            default_args: "{\"max_floors\": 50, \"world_count\": 5, \"base_reward\": 120, \"store_name\": \"OpenROTowerBoard_v1\", \"bind_close\": true}",
            source: TOWER_PROGRESSION_RN,
            entry: "generate_tower_progression",
        },
        SkillDef {
            name: "potions_weather",
            title: "Potions, Weather & Codes",
            category: "World",
            description: "a PotionsWeather script (ServerScriptService): dynamic server weather events (Solar Flare, Void Eclipse, Thunderstorm, Blood Moon) with Lighting/Atmosphere tweens, craftable Luck and Speed potions, and server-validated redeemable codes (REBIRTH, APEX2026, LUCKBOOST)",
            params: "{\"weather_cycle_seconds\": int, \"luck_potion_cost\": int, \"speed_potion_cost\": int, \"enable_codes\": bool}",
            default_path: "ServerScriptService.PotionsWeather",
            default_args: "{\"weather_cycle_seconds\": 90, \"luck_potion_cost\": 150, \"speed_potion_cost\": 120, \"enable_codes\": true}",
            source: POTIONS_WEATHER_RN,
            entry: "generate_potions_weather",
        },
        SkillDef {
            name: "card_gallery_world",
            title: "3D Card Gallery Lobby",
            category: "UI",
            description: "a CardGalleryWorld script (ServerScriptService): builds the 3D Studio Lobby with Spawn Courtyard, Holographic Roll Pedestal, 3D Card Showcase Billboards (Decal and ImageLabel art slots), Tower Portal Gate, Boss Raid Shrine, Potion Alchemist Stand, and live Leaderboard Wall",
            params: "{\"lobby_name\": string, \"showcase_count\": int, \"enable_holograms\": bool, \"create_structures\": bool}",
            default_path: "ServerScriptService.CardGalleryWorld",
            default_args: "{\"lobby_name\": \"ApexLobby\", \"showcase_count\": 8, \"enable_holograms\": true, \"create_structures\": true}",
            source: CARD_GALLERY_WORLD_RN,
            entry: "generate_card_gallery_world",
        },
];

/// Find a registered game system definition by name.
pub fn find(name: &str) -> Option<SkillDef> {
    REGISTRY.iter().copied().find(|d| d.name == name)
}

/// JSON catalog of all 28 registered game systems for HTTP endpoints and UI Blueprint Library browsing.
pub fn catalog_json() -> Value {
    let items: Vec<Value> = REGISTRY
        .iter()
        .map(|d| {
            let parsed_defaults: Value =
                serde_json::from_str(d.default_args).unwrap_or_else(|_| json!({}));
            json!({
                "name": d.name,
                "title": d.title,
                "category": d.category,
                "description": d.description,
                "params": d.params,
                "default_path": d.default_path,
                "default_args": parsed_defaults,
            })
        })
        .collect();
    Value::Array(items)
}

/// A present-but-wrong-typed argument is an error, never a silent
/// default: a model typo must change nothing quietly. (Absent args
/// still take their documented defaults.)
/// An integer arg with a range: type validation alone let
/// `cycle_minutes: 0` generate a division by zero in the Luau and a
/// negative `damage` generate a healing brick — and the unbounded top
/// end let i64::MAX generate literals Luau doubles / IntValues
/// cannot represent (a permanently frozen day cycle).
fn arg_i64_range(
    args: &Value,
    name: &str,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64, String> {
    let v = match args.get(name) {
        None => default,
        Some(v) => v
            .as_i64()
            .ok_or_else(|| format!("skill arg '{name}' must be an integer, got: {v}"))?,
    };
    if v < min || v > max {
        return Err(format!(
            "skill arg '{name}' must be between {min} and {max}, got: {v}"
        ));
    }
    Ok(v)
}

fn arg_bool(args: &Value, name: &str, default: bool) -> Result<bool, String> {
    match args.get(name) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("skill arg '{name}' must be true or false, got: {v}")),
    }
}

fn arg_string(args: &Value, name: &str, default: &str) -> Result<String, String> {
    match args.get(name) {
        None => Ok(default.to_string()),
        Some(v) => {
            let s = v
                .as_str()
                .ok_or_else(|| format!("skill arg '{name}' must be a string, got: {v}"))?;
            // String args land inside generated Luau literals; control
            // characters (a raw NUL sailed through once) are refused
            // here rather than escaped char-by-char downstream.
            if s.chars().any(|c| c.is_control()) {
                return Err(format!(
                    "skill arg '{name}' must not contain control characters"
                ));
            }
            Ok(s.to_string())
        }
    }
}

fn arg_identifier(args: &Value, name: &str, default: &str) -> Result<String, String> {
    let val = arg_string(args, name, default)?;
    if !valid_identifier(&val) {
        return Err(format!(
            "skill arg '{name}' must be a plain name (letters, digits, underscores), got: {val}"
        ));
    }
    Ok(val)
}

/// A plain Luau identifier — the only thing allowed in member position
/// (`workspace.<name>`) in generated code.
fn valid_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Run the named skill's Rune script with the model's JSON args and
/// return the generated Luau source.
pub fn generate(skill: &str, args: &Value) -> Result<String, String> {
    let def = find(skill).ok_or_else(|| format!("unknown skill: {skill}"))?;
    match def.name {
        "round_manager" => {
            let create_round = arg_bool(args, "create_round", false)?;
            run_skill(def.source, def.entry, (create_round,))
        }
        "leaderboard" => {
            let currency = arg_string(args, "currency", "Coins")?;
            let start_amount = arg_i64_range(args, "start_amount", 0, 0, 1_000_000_000)?;
            run_skill(def.source, def.entry, (currency, start_amount))
        }
        "day_night" => {
            // A full cycle longer than a week of real time is not a
            // day/night cycle.
            let cycle_minutes = arg_i64_range(args, "cycle_minutes", 10, 1, 10_080)?;
            run_skill(def.source, def.entry, (cycle_minutes,))
        }
        "kill_brick" => {
            let damage = arg_i64_range(args, "damage", 100, 1, 100_000)?;
            run_skill(def.source, def.entry, (damage,))
        }
        "spawn_link" => {
            let instance_name = arg_identifier(args, "instance_name", "SpawnPad")?;
            run_skill(def.source, def.entry, (instance_name,))
        }
        "currency" => {
            let currency = arg_string(args, "currency", "Coins")?;
            let start_amount = arg_i64_range(args, "start_amount", 0, 0, 1_000_000_000)?;
            let earn_amount = arg_i64_range(args, "earn_amount", 10, 1, 1_000_000)?;
            // A passive-income period longer than an hour is not a period.
            let earn_seconds = arg_i64_range(args, "earn_seconds", 5, 1, 3_600)?;
            let bind_close = arg_bool(args, "bind_close", true)?;
            run_skill(
                def.source,
                def.entry,
                (currency, start_amount, earn_amount, earn_seconds, bind_close),
            )
        }
        "shop" => {
            let item_name = arg_string(args, "item_name", "Speed Coil")?;
            let price = arg_i64_range(args, "price", 100, 1, 1_000_000_000)?;
            let currency = arg_string(args, "currency", "Coins")?;
            run_skill(def.source, def.entry, (item_name, price, currency))
        }
        "game_gui" => {
            let title = arg_string(args, "title", "My Game")?;
            let button_label = arg_string(args, "button_label", "Shop")?;
            let currency = arg_string(args, "currency", "Coins")?;
            run_skill(def.source, def.entry, (title, button_label, currency))
        }
        "checkpoints" => {
            let stage_count = arg_i64_range(args, "stage_count", 5, 2, 50)?;
            let folder_name = arg_identifier(args, "folder_name", "Checkpoints")?;
            let create_folder = arg_bool(args, "create_folder", true)?;
            run_skill(def.source, def.entry, (stage_count, folder_name, create_folder))
        }
        "soundtrack" => {
            let sound_id = arg_i64_range(args, "sound_id", 0, 0, 9_999_999_999)?;
            let volume = arg_i64_range(args, "volume", 5, 0, 10)?;
            let looped = arg_bool(args, "looped", true)?;
            run_skill(def.source, def.entry, (sound_id, volume, looped))
        }
        "jump_pad" => {
            let boost_power = arg_i64_range(args, "boost_power", 120, 20, 500)?;
            let cooldown_ms = arg_i64_range(args, "cooldown_ms", 600, 100, 10_000)?;
            let pad_name = arg_identifier(args, "pad_name", "JumpPad")?;
            let create_pad = arg_bool(args, "create_pad", true)?;
            run_skill(
                def.source,
                def.entry,
                (boost_power, cooldown_ms, pad_name, create_pad),
            )
        }
        "moving_platform" => {
            let platform_name = arg_identifier(args, "platform_name", "MovingPlatform")?;
            let travel_distance = arg_i64_range(args, "travel_distance", 24, 4, 200)?;
            let travel_seconds = arg_i64_range(args, "travel_seconds", 4, 1, 60)?;
            let axis_raw = arg_string(args, "axis", "X")?;
            let axis = match axis_raw.to_ascii_uppercase().as_str() {
                "X" => "X".to_string(),
                "Y" => "Y".to_string(),
                "Z" => "Z".to_string(),
                _ => {
                    return Err(format!(
                        "skill arg 'axis' must be X, Y, or Z, got: {axis_raw}"
                    ))
                }
            };
            let create_platform = arg_bool(args, "create_platform", true)?;
            run_skill(
                def.source,
                def.entry,
                (
                    platform_name,
                    travel_distance,
                    travel_seconds,
                    axis,
                    create_platform,
                ),
            )
        }
        "datastore_save" => {
            let store_name = arg_string(args, "store_name", "OpenROPlayerData")?;
            let stat_name = arg_string(args, "stat_name", "Coins")?;
            let default_value = arg_i64_range(args, "default_value", 0, 0, 1_000_000_000)?;
            let autosave_seconds = arg_i64_range(args, "autosave_seconds", 60, 15, 3_600)?;
            let bind_close = arg_bool(args, "bind_close", true)?;
            run_skill(
                def.source,
                def.entry,
                (
                    store_name,
                    stat_name,
                    default_value,
                    autosave_seconds,
                    bind_close,
                ),
            )
        }
        "npc_patrol" => {
            let npc_name = arg_identifier(args, "npc_name", "GuardNPC")?;
            let waypoint_count = arg_i64_range(args, "waypoint_count", 4, 2, 20)?;
            let walk_speed = arg_i64_range(args, "walk_speed", 12, 4, 40)?;
            let damage = arg_i64_range(args, "damage", 15, 0, 100)?;
            let dialogue = arg_string(args, "dialogue", "Halt! Restricted zone.")?;
            run_skill(
                def.source,
                def.entry,
                (npc_name, waypoint_count, walk_speed, damage, dialogue),
            )
        }
        "portal_teleporter" => {
            let pair_name = arg_identifier(args, "pair_name", "WarpPortal")?;
            let cooldown_seconds = arg_i64_range(args, "cooldown_seconds", 3, 1, 30)?;
            let offset_distance = arg_i64_range(args, "offset_distance", 60, 12, 500)?;
            let create_portals = arg_bool(args, "create_portals", true)?;
            run_skill(
                def.source,
                def.entry,
                (pair_name, cooldown_seconds, offset_distance, create_portals),
            )
        }
        "combat_sword" => {
            let sword_name = arg_string(args, "sword_name", "ClassicSword")?;
            let damage = arg_i64_range(args, "damage", 25, 1, 500)?;
            let cooldown_ms = arg_i64_range(args, "cooldown_ms", 450, 100, 5_000)?;
            let range_studs = arg_i64_range(args, "range_studs", 10, 4, 24)?;
            run_skill(
                def.source,
                def.entry,
                (sword_name, damage, cooldown_ms, range_studs),
            )
        }
        "inventory_backpack" => {
            let starter_item = arg_string(args, "starter_item", "Torch")?;
            let max_slots = arg_i64_range(args, "max_slots", 12, 1, 50)?;
            let remote_name = arg_identifier(args, "remote_name", "OpenROInventoryEquip")?;
            run_skill(def.source, def.entry, (starter_item, max_slots, remote_name))
        }
        "quest_tracker" => {
            let quest_name = arg_string(args, "quest_name", "Crystal Hunt")?;
            let target_count = arg_i64_range(args, "target_count", 5, 1, 1_000)?;
            let reward_amount = arg_i64_range(args, "reward_amount", 150, 1, 1_000_000)?;
            let currency = arg_string(args, "currency", "Coins")?;
            run_skill(
                def.source,
                def.entry,
                (quest_name, target_count, reward_amount, currency),
            )
        }
        "vehicle_spawner" => {
            let vehicle_name = arg_identifier(args, "vehicle_name", "Rover")?;
            let max_speed = arg_i64_range(args, "max_speed", 65, 10, 200)?;
            let cooldown_seconds = arg_i64_range(args, "cooldown_seconds", 5, 1, 60)?;
            let create_pad = arg_bool(args, "create_pad", true)?;
            run_skill(
                def.source,
                def.entry,
                (vehicle_name, max_speed, cooldown_seconds, create_pad),
            )
        }
        "weather_cycle" => {
            let cycle_seconds = arg_i64_range(args, "cycle_seconds", 45, 10, 3_600)?;
            let weather_raw = arg_string(args, "start_weather", "Clear")?;
            let start_weather = match weather_raw.to_ascii_lowercase().as_str() {
                "clear" => "Clear".to_string(),
                "rain" => "Rain".to_string(),
                "fog" => "Fog".to_string(),
                "dusk" => "Dusk".to_string(),
                _ => {
                    return Err(format!(
                        "skill arg 'start_weather' must be Clear, Rain, Fog, or Dusk, got: {weather_raw}"
                    ))
                }
            };
            let enable_particles = arg_bool(args, "enable_particles", true)?;
            run_skill(
                def.source,
                def.entry,
                (cycle_seconds, start_weather, enable_particles),
            )
        }
        "tycoon_dropper" => {
            let tycoon_name = arg_identifier(args, "tycoon_name", "FactoryTycoon")?;
            let drop_value = arg_i64_range(args, "drop_value", 10, 1, 100_000)?;
            let drop_interval = arg_i64_range(args, "drop_interval", 2, 1, 30)?;
            let upgrade_cost = arg_i64_range(args, "upgrade_cost", 100, 10, 10_000_000)?;
            let currency = arg_string(args, "currency", "Coins")?;
            run_skill(
                def.source,
                def.entry,
                (
                    tycoon_name,
                    drop_value,
                    drop_interval,
                    upgrade_cost,
                    currency,
                ),
            )
        }
        "admin_spectate" => {
            let remote_name = arg_identifier(args, "remote_name", "OpenROSpectate")?;
            let cycle_interval = arg_i64_range(args, "cycle_interval", 5, 1, 60)?;
            let allow_all_players = arg_bool(args, "allow_all_players", true)?;
            run_skill(
                def.source,
                def.entry,
                (remote_name, cycle_interval, allow_all_players),
            )
        }
        "card_gacha_roller" => {
            let base_luck_pct = arg_i64_range(args, "base_luck_pct", 100, 100, 1_000)?;
            let pity_threshold = arg_i64_range(args, "pity_threshold", 50, 10, 500)?;
            let fast_roll_ms = arg_i64_range(args, "fast_roll_ms", 350, 100, 3_000)?;
            let enable_borders = arg_bool(args, "enable_borders", true)?;
            run_skill(
                def.source,
                def.entry,
                (base_luck_pct, pity_threshold, fast_roll_ms, enable_borders),
            )
        }
        "deck_builder" => {
            let deck_slots = arg_i64_range(args, "deck_slots", 6, 3, 6)?;
            let fusion_cost = arg_i64_range(args, "fusion_cost", 3, 2, 10)?;
            let rebirth_floor = arg_i64_range(args, "rebirth_floor", 10, 5, 100)?;
            let store_name = arg_string(args, "store_name", "OpenROApexDecks_v1")?;
            let bind_close = arg_bool(args, "bind_close", true)?;
            run_skill(
                def.source,
                def.entry,
                (deck_slots, fusion_cost, rebirth_floor, store_name, bind_close),
            )
        }
        "card_battle_arena" => {
            let team_size = arg_i64_range(args, "team_size", 4, 3, 6)?;
            let ultimate_charge_max = arg_i64_range(args, "ultimate_charge_max", 100, 50, 200)?;
            let counter_window_ms = arg_i64_range(args, "counter_window_ms", 800, 250, 2_000)?;
            let boss_hp_scale = arg_i64_range(args, "boss_hp_scale", 4, 1, 20)?;
            run_skill(
                def.source,
                def.entry,
                (
                    team_size,
                    ultimate_charge_max,
                    counter_window_ms,
                    boss_hp_scale,
                ),
            )
        }
        "tower_progression" => {
            let max_floors = arg_i64_range(args, "max_floors", 50, 10, 200)?;
            let world_count = arg_i64_range(args, "world_count", 5, 3, 10)?;
            let base_reward = arg_i64_range(args, "base_reward", 120, 50, 10_000)?;
            let store_name = arg_string(args, "store_name", "OpenROTowerBoard_v1")?;
            let bind_close = arg_bool(args, "bind_close", true)?;
            run_skill(
                def.source,
                def.entry,
                (max_floors, world_count, base_reward, store_name, bind_close),
            )
        }
        "potions_weather" => {
            let weather_cycle_seconds =
                arg_i64_range(args, "weather_cycle_seconds", 90, 15, 600)?;
            let luck_potion_cost = arg_i64_range(args, "luck_potion_cost", 150, 10, 100_000)?;
            let speed_potion_cost = arg_i64_range(args, "speed_potion_cost", 120, 10, 100_000)?;
            let enable_codes = arg_bool(args, "enable_codes", true)?;
            run_skill(
                def.source,
                def.entry,
                (
                    weather_cycle_seconds,
                    luck_potion_cost,
                    speed_potion_cost,
                    enable_codes,
                ),
            )
        }
        "card_gallery_world" => {
            let lobby_name = arg_identifier(args, "lobby_name", "ApexLobby")?;
            let showcase_count = arg_i64_range(args, "showcase_count", 8, 4, 12)?;
            let enable_holograms = arg_bool(args, "enable_holograms", true)?;
            let create_structures = arg_bool(args, "create_structures", true)?;
            run_skill(
                def.source,
                def.entry,
                (
                    lobby_name,
                    showcase_count,
                    enable_holograms,
                    create_structures,
                ),
            )
        }
        _ => unreachable!("registry and dispatch are kept in step"),
    }
}

/// Run the `round_manager` Rune skill and return the generated Luau source.
pub fn generate_round_manager(create_round: bool) -> Result<String, String> {
    generate(
        "round_manager",
        &serde_json::json!({ "create_round": create_round }),
    )
}

fn run_skill<A>(source: &str, function: &str, args: A) -> Result<String, String>
where
    A: rune::runtime::Args + rune::runtime::GuardedArgs,
{
    let context = rune::Context::with_default_modules()
        .map_err(|e| format!("rune context: {e}"))?;
    let runtime = Arc::new(
        context
            .runtime()
            .map_err(|e| format!("rune runtime: {e}"))?,
    );

    let mut sources = rune::Sources::new();
    sources
        .insert(
            rune::Source::memory(source).map_err(|e| format!("rune source: {e}"))?,
        )
        .map_err(|e| format!("rune sources: {e}"))?;

    let mut diagnostics = rune::Diagnostics::new();
    let result = rune::prepare(&mut sources)
        .with_context(&context)
        .with_diagnostics(&mut diagnostics)
        .build();

    if !diagnostics.is_empty() {
        return Err(format!("rune diagnostics: {diagnostics:?}"));
    }
    let unit = result.map_err(|e| format!("rune build: {e}"))?;

    let mut vm = rune::Vm::new(runtime, Arc::new(unit));
    let output = vm
        .call([function], args)
        .map_err(|e| format!("rune call {function}: {e}"))?;
    let text: String =
        rune::from_value(output).map_err(|e| format!("rune return value: {e}"))?;
    Ok(text)
}
