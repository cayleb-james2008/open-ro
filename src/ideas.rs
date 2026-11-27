//! The proactive creativity engine.
//!
//! OpenRO's staple is creativity, not gates: the agent pitches games
//! and features UNPROMPTED — on an empty chat it offers complete games
//! it can build in one go, and after a build lands it offers the
//! features that make sense next, based on what the game already has.
//!
//! Every idea carries a built-in PLAN: the same JSON steps the agent
//! loop executes for a model ask (skills + bridge tools + a playtest).
//! Building an idea runs the plan through the loop's own machinery —
//! checkpointed, playtested, revertable — with no model call involved,
//! and the thread says exactly that. A pitch is an offer; a build is
//! only called done when its playtest passes, like any other ask.
//!
//! The catalogue is deterministic on purpose: the pitches are the
//! product's own creative repertoire, and every plan is testable end
//! to end against the mock bridge.

use serde_json::{json, Value};
#[derive(Clone)]
pub struct Idea {
    pub id: &'static str,
    pub title: &'static str,
    pub pitch: &'static str,
    /// The plan's steps, in the agent loop's step format.
    pub plan: Vec<Value>,
    /// Skills the plan uses — recorded as built once it lands.
    pub skills: Vec<&'static str>,
    /// Skills that must already be built for this idea to be offered
    /// as a follow-up (empty = offerable any time).
    pub needs: Vec<&'static str>,
}

fn skill(name: &str, args: Value) -> Value {
    json!({ "skill": name, "args": args })
}

fn write(path: &str, skill_name: &str) -> Value {
    json!({ "tool": "write_script", "args": { "path": path, "skill": skill_name } })
}

fn instance(class_name: &str, name: &str, parent: &str, properties: Value) -> Value {
    json!({
        "tool": "create_instance",
        "args": {
            "class_name": class_name,
            "name": name,
            "parent": parent,
            "properties": properties,
        }
    })
}

fn playtest() -> Value {
    json!({ "tool": "playtest", "args": {} })
}

fn part_at(
    name: &str,
    parent: &str,
    pos: [f64; 3],
    size: [f64; 3],
    color: &str,
    material: &str,
) -> Value {
    instance(
        "Part",
        name,
        parent,
        json!({
            "Anchored": true,
            "CanCollide": true,
            "Position": pos,
            "Size": size,
            "Color": color,
            "Material": material,
        }),
    )
}

fn apex_hub_world_instances() -> Vec<Value> {
    let mut out = vec![
        instance(
            "SpawnLocation",
            "SpawnPad",
            "Workspace",
            json!({
                "Anchored": true,
                "CanCollide": true,
                "Position": [0.0, 2.2, 0.0],
                "Size": [12.0, 1.0, 12.0],
                "Color": "#F59E0B",
                "Material": "SmoothPlastic",
            }),
        ),
        part_at("RollPedestal", "Workspace", [0.0, 3.8, -61.0], [12.0, 1.0, 12.0], "#38BDF8", "Neon"),
        part_at("TowerPortal", "Workspace", [-80.0, 21.0, -18.0], [7.5, 38.0, 7.5], "#38BDF8", "Neon"),
        part_at("BossRaidShrine", "Workspace", [80.0, 7.5, -36.0], [18.0, 10.0, 6.0], "#C2410C", "Marble"),
        part_at("PotionStand", "Workspace", [-74.0, 1.4, 30.0], [34.0, 2.0, 34.0], "#9A5B25", "WoodPlanks"),
        instance("Folder", "ApexLobby", "Workspace", json!({})),
        instance("Sky", "ApexSky", "Lighting", json!({ "StarCount": 3500 })),
        instance("Atmosphere", "ApexAtmosphere", "Lighting", json!({ "Density": 0.16, "Haze": 0.45 })),
        instance("BloomEffect", "ApexBloom", "Lighting", json!({ "Enabled": true, "Intensity": 0.45, "Size": 18 })),
        instance("SunRaysEffect", "ApexSunRays", "Lighting", json!({ "Enabled": true, "Intensity": 0.14 })),
        instance("ColorCorrectionEffect", "ApexColorCorrection", "Lighting", json!({ "Enabled": true, "Contrast": 0.08, "Saturation": 0.22 })),
        // Sunlit Anime Floating Archipelago Courtyard, Cliff Undersides, Koi Moat & Stone Bridges
        part_at("IslandBaseTier1", "Workspace.ApexLobby", [0.0, -2.0, 0.0], [176.0, 4.2, 176.0], "#4ADE80", "Grass"),
        part_at("CliffTierUpper", "Workspace.ApexLobby", [0.0, -7.2, 0.0], [144.0, 6.4, 144.0], "#A68A64", "Rock"),
        part_at("CliffTierMid", "Workspace.ApexLobby", [0.0, -14.0, 0.0], [102.0, 8.0, 102.0], "#7F6A4F", "Rock"),
        part_at("CourtyardMain", "Workspace.ApexLobby", [0.0, 0.8, 0.0], [96.0, 1.6, 96.0], "#F5EFE6", "Marble"),
        part_at("CenterEmblemFloor", "Workspace.ApexLobby", [0.0, 1.8, 0.0], [34.0, 0.4, 34.0], "#E7DEC8", "Marble"),
        part_at("EmblemTrimN", "Workspace.ApexLobby", [0.0, 2.02, -17.0], [35.0, 0.2, 0.9], "#0EA5E9", "Neon"),
        part_at("EmblemTrimS", "Workspace.ApexLobby", [0.0, 2.02, 17.0], [35.0, 0.2, 0.9], "#0EA5E9", "Neon"),
        part_at("EmblemTrimE", "Workspace.ApexLobby", [17.0, 2.02, 0.0], [0.9, 0.2, 35.0], "#0EA5E9", "Neon"),
        part_at("EmblemTrimW", "Workspace.ApexLobby", [-17.0, 2.02, 0.0], [0.9, 0.2, 35.0], "#0EA5E9", "Neon"),
        part_at("KoiMoatNorth", "Workspace.ApexLobby", [0.0, 0.4, -55.0], [124.0, 0.6, 14.0], "#22D3EE", "Glass"),
        part_at("KoiMoatSouth", "Workspace.ApexLobby", [0.0, 0.4, 52.0], [124.0, 0.6, 10.0], "#22D3EE", "Glass"),
        part_at("KoiMoatEast", "Workspace.ApexLobby", [55.0, 0.4, 0.0], [14.0, 0.6, 124.0], "#22D3EE", "Glass"),
        part_at("KoiMoatWest", "Workspace.ApexLobby", [-55.0, 0.4, 0.0], [14.0, 0.6, 124.0], "#22D3EE", "Glass"),
        part_at("NorthBridgeDeck", "Workspace.ApexLobby", [0.0, 2.0, -44.0], [12.0, 1.0, 36.0], "#EDE4D3", "Marble"),
        part_at("NorthBridgeRailL", "Workspace.ApexLobby", [-6.2, 2.9, -44.0], [0.8, 1.2, 36.0], "#F59E0B", "Neon"),
        part_at("NorthBridgeRailR", "Workspace.ApexLobby", [6.2, 2.9, -44.0], [0.8, 1.2, 36.0], "#F59E0B", "Neon"),
        part_at("SouthBridgeDeck", "Workspace.ApexLobby", [0.0, 2.0, 44.0], [16.0, 1.0, 30.0], "#EDE4D3", "Marble"),
        part_at("EastBridgeDeck", "Workspace.ApexLobby", [44.0, 2.0, -14.0], [36.0, 1.0, 12.0], "#EDE4D3", "Marble"),
        part_at("WestBridgeDeck", "Workspace.ApexLobby", [-44.0, 2.0, -14.0], [36.0, 1.0, 12.0], "#EDE4D3", "Marble"),
        part_at("PathSEBridge", "Workspace.ApexLobby", [44.0, 2.0, 28.0], [36.0, 1.0, 10.0], "#EDE4D3", "Marble"),
        part_at("PathSWBridge", "Workspace.ApexLobby", [-44.0, 2.0, 28.0], [36.0, 1.0, 10.0], "#EDE4D3", "Marble"),
        // Japanese Torii Summon Shrine
        part_at("ShrineDais1", "Workspace.ApexLobby", [0.0, 1.6, -70.0], [42.0, 1.8, 30.0], "#D6C5A9", "Marble"),
        part_at("ShrineDais2", "Workspace.ApexLobby", [0.0, 2.8, -70.0], [32.0, 1.2, 22.0], "#F5EFE6", "Marble"),
        part_at("ToriiBaseL", "Workspace.ApexLobby", [-9.5, 4.4, -71.0], [3.4, 2.2, 3.4], "#475569", "Marble"),
        part_at("ToriiBaseR", "Workspace.ApexLobby", [9.5, 4.4, -71.0], [3.4, 2.2, 3.4], "#475569", "Marble"),
        part_at("ToriiPillarL", "Workspace.ApexLobby", [-9.5, 13.5, -71.0], [2.4, 16.5, 2.4], "#DC2626", "SmoothPlastic"),
        part_at("ToriiPillarR", "Workspace.ApexLobby", [9.5, 13.5, -71.0], [2.4, 16.5, 2.4], "#DC2626", "SmoothPlastic"),
        part_at("ToriiNukiBeam", "Workspace.ApexLobby", [0.0, 17.5, -71.0], [25.0, 1.4, 1.4], "#DC2626", "SmoothPlastic"),
        part_at("ToriiShimakiBeam", "Workspace.ApexLobby", [0.0, 21.4, -71.0], [27.5, 1.4, 2.0], "#DC2626", "SmoothPlastic"),
        part_at("ToriiKasagiCenter", "Workspace.ApexLobby", [0.0, 22.9, -71.0], [20.0, 1.5, 2.8], "#29201B", "Wood"),
        part_at("ToriiPlaque", "Workspace.ApexLobby", [0.0, 19.5, -71.0], [3.4, 3.4, 1.8], "#FACC15", "Metal"),
        part_at("SummonLightBeam", "Workspace.ApexLobby", [0.0, 18.0, -71.0], [4.0, 30.0, 4.0], "#7DD3FC", "Neon"),
        part_at("ToriiHaloOuter", "Workspace.ApexLobby", [0.0, 12.5, -71.0], [10.5, 0.7, 10.5], "#38BDF8", "Neon"),
        part_at("ToriiHaloInner", "Workspace.ApexLobby", [0.0, 12.5, -71.0], [7.2, 0.6, 7.2], "#FACC15", "Neon"),
        // Boss Raid Colosseum + Champion Statue + Damage Pillar
        part_at("ColosseumFloor", "Workspace.ApexLobby", [80.0, 1.4, -18.0], [46.0, 2.0, 46.0], "#E6D5B8", "Sand"),
        part_at("ColosseumInnerRing", "Workspace.ApexLobby", [80.0, 2.6, -18.0], [28.0, 0.6, 28.0], "#991B1B", "Marble"),
        part_at("StatuePlinth", "Workspace.ApexLobby", [80.0, 4.2, -24.0], [8.0, 3.2, 8.0], "#D6C5A9", "Marble"),
        part_at("StatueTorso", "Workspace.ApexLobby", [80.0, 11.4, -24.0], [4.8, 4.4, 2.8], "#DC2626", "Metal"),
        part_at("StatueHelm", "Workspace.ApexLobby", [80.0, 15.2, -24.0], [2.8, 2.8, 2.8], "#FACC15", "Metal"),
        part_at("StatueSwordL", "Workspace.ApexLobby", [75.2, 11.0, -22.5], [0.7, 9.0, 1.0], "#EF4444", "Neon"),
        part_at("StatueSwordR", "Workspace.ApexLobby", [84.8, 11.0, -22.5], [0.7, 9.0, 1.0], "#EF4444", "Neon"),
        part_at("DamagePillar", "Workspace.ApexLobby", [66.0, 7.5, -18.0], [5.5, 10.5, 2.2], "#334155", "SmoothPlastic"),
        // Infinite Tower Plinth + Alchemist Cauldron Pavilion + PvP Duel Arena + Hall Promenade
        part_at("TowerPlinth", "Workspace.ApexLobby", [-80.0, 1.4, -18.0], [44.0, 2.0, 44.0], "#E2E8F0", "Marble"),
        part_at("CauldronBase", "Workspace.ApexLobby", [-78.0, 3.0, 30.0], [8.0, 1.2, 8.0], "#475569", "Metal"),
        part_at("CauldronBody", "Workspace.ApexLobby", [-78.0, 5.4, 30.0], [6.5, 4.2, 6.5], "#1E293B", "Metal"),
        part_at("CauldronBrew", "Workspace.ApexLobby", [-78.0, 7.6, 30.0], [5.2, 0.5, 5.2], "#34D399", "Neon"),
        part_at("TraitAnvilBase", "Workspace.ApexLobby", [-67.0, 3.5, 30.0], [5.5, 2.2, 4.2], "#475569", "Metal"),
        part_at("TraitForgeCrystal", "Workspace.ApexLobby", [-67.0, 6.0, 30.0], [2.4, 3.2, 2.4], "#A855F7", "Neon"),
        part_at("DuelArenaStage", "Workspace.ApexLobby", [76.0, 1.4, 30.0], [46.0, 2.0, 36.0], "#F1F5F9", "Marble"),
        part_at("ClashCenterPlatform", "Workspace.ApexLobby", [76.0, 2.7, 30.0], [12.0, 0.8, 12.0], "#FACC15", "Neon"),
        part_at("HallPromenade", "Workspace.ApexLobby", [0.0, 1.4, 65.0], [96.0, 2.0, 28.0], "#EDE4D3", "Marble"),
        part_at("LeaderboardWall", "Workspace.ApexLobby", [0.0, 15.5, 79.0], [34.0, 13.0, 2.2], "#1E293B", "SmoothPlastic"),
    ];

    for (idx, (cx, cz)) in [(-40.0, -40.0), (40.0, -40.0), (-40.0, 40.0), (40.0, 40.0)]
        .into_iter()
        .enumerate()
    {
        let n = idx + 1;
        out.push(part_at(&format!("PlazaPillarBase{n}"), "Workspace.ApexLobby", [cx, 2.6, cz], [6.0, 2.0, 6.0], "#1E293B", "Marble"));
        out.push(part_at(&format!("PlazaPillarShaft{n}"), "Workspace.ApexLobby", [cx, 8.6, cz], [3.5, 10.0, 3.5], "#334155", "Marble"));
        out.push(part_at(&format!("PlazaPillarOrb{n}"), "Workspace.ApexLobby", [cx, 15.0, cz], [3.2, 3.2, 3.2], "#A855F7", "Neon"));
    }

    for (idx, (bx, bz)) in [(66.0, -30.0), (94.0, -30.0), (66.0, -6.0), (94.0, -6.0)]
        .into_iter()
        .enumerate()
    {
        let n = idx + 1;
        out.push(part_at(&format!("BrazierStand{n}"), "Workspace.ApexLobby", [bx, 4.5, bz], [2.6, 4.2, 2.6], "#1E1B29", "Metal"));
        out.push(part_at(&format!("BrazierFlame{n}"), "Workspace.ApexLobby", [bx, 7.4, bz], [2.2, 2.2, 2.2], "#F97316", "Neon"));
    }

    for s in 0..12 {
        let ang = (s as f64) * std::f64::consts::PI / 6.0;
        let sx = -80.0 + ang.cos() * 9.0;
        let sz = -18.0 + ang.sin() * 9.0;
        let sy = 3.2 + (s as f64) * 2.4;
        let col = if s % 2 == 0 { "#6366F1" } else { "#38BDF8" };
        out.push(part_at(&format!("SpiralStep{:02}", s + 1), "Workspace.ApexLobby", [sx, sy, sz], [5.0, 0.9, 3.2], col, "Neon"));
    }

    for slot in 1..=4 {
        let z_off = 19.5 + ((slot - 1) as f64) * 7.0;
        out.push(part_at(&format!("LeftStagePad{slot}"), "Workspace.ApexLobby", [60.0, 2.7, z_off], [5.0, 0.7, 5.0], "#38BDF8", "Neon"));
        out.push(part_at(&format!("RightStagePad{slot}"), "Workspace.ApexLobby", [92.0, 2.7, z_off], [5.0, 0.7, 5.0], "#EF4444", "Neon"));
    }

    for (tidx, (tx, tz)) in [
        (-31.0, -31.0), (31.0, -31.0), (-31.0, 31.0), (31.0, 31.0),
        (-44.0, 6.0), (44.0, 6.0), (-22.0, -46.0), (22.0, -46.0),
    ]
    .into_iter()
    .enumerate()
    {
        let n = tidx + 1;
        out.push(part_at(&format!("SakuraTrunk{n}"), "Workspace.ApexLobby", [tx, 6.2, tz], [2.2, 9.2, 2.2], "#5C3A21", "Wood"));
        out.push(part_at(&format!("SakuraCanopyMain{n}"), "Workspace.ApexLobby", [tx, 11.8, tz], [9.0, 6.0, 9.0], "#F472B6", "SmoothPlastic"));
        out.push(part_at(&format!("SakuraCanopyL{n}"), "Workspace.ApexLobby", [tx - 2.6, 10.6, tz + 1.4], [5.8, 4.5, 5.8], "#F9A8D4", "SmoothPlastic"));
        out.push(part_at(&format!("SakuraCanopyR{n}"), "Workspace.ApexLobby", [tx + 2.5, 10.9, tz - 1.5], [6.0, 4.5, 6.0], "#EC4899", "SmoothPlastic"));
        out.push(part_at(&format!("SakuraLantern{n}"), "Workspace.ApexLobby", [tx, 8.2, tz - 2.5], [1.4, 1.8, 1.4], "#FBBF24", "Neon"));
    }

    out
}

fn spawn_pad() -> Value {
    instance(
        "SpawnLocation",
        "SpawnPad",
        "Workspace",
        json!({ "Anchored": true }),
    )
}

fn lava_brick(damage: i64) -> Vec<Value> {
    vec![
        instance("Part", "LavaBrick", "Workspace", json!({ "Anchored": true })),
        skill("kill_brick", json!({ "damage": damage })),
        write("Workspace.LavaBrick.KillBrick", "kill_brick"),
    ]
}

/// The complete games pitched on an empty chat.
pub fn opening_ideas() -> Vec<Idea> {
    vec![
        Idea {
            id: "obby-starter",
            title: "Lava Obby",
            pitch: "A lava obby that plays fair: a spawn pad, checkpoints that \
                    remember your stage so a fall doesn't restart you, lava \
                    bricks that burn, and coins that keep paying while you \
                    climb — and save between sessions.",
            plan: [
                vec![spawn_pad()],
                vec![
                    skill("spawn_link", json!({ "instance_name": "SpawnPad" })),
                    write("ServerScriptService.SpawnLink", "spawn_link"),
                ],
                vec![
                    skill(
                        "checkpoints",
                        json!({ "stage_count": 6, "folder_name": "Checkpoints", "create_folder": true }),
                    ),
                    write("ServerScriptService.Checkpoints", "checkpoints"),
                ],
                lava_brick(25),
                vec![
                    skill(
                        "currency",
                        json!({ "currency": "Coins", "start_amount": 0, "earn_amount": 10, "earn_seconds": 5, "bind_close": true }),
                    ),
                    write("ServerScriptService.Currency", "currency"),
                ],
                vec![
                    skill("day_night", json!({ "cycle_minutes": 8 })),
                    write("ServerScriptService.DayNight", "day_night"),
                ],
                vec![playtest()],
            ]
            .into_iter()
            .flatten()
            .collect(),
            skills: vec!["spawn_link", "checkpoints", "kill_brick", "currency", "day_night"],
            needs: vec![],
        },
        Idea {
            id: "coin-tycoon",
            title: "Coin Tycoon",
            pitch: "A tycoon starter with a real economy: money that saves \
                    between sessions and keeps paying while you play, a shop \
                    selling a Speed Coil, and a proper game screen showing \
                    your balance with the shop button on it.",
            plan: [
                vec![
                    skill(
                        "currency",
                        json!({ "currency": "Coins", "start_amount": 50, "earn_amount": 10, "earn_seconds": 5, "bind_close": true }),
                    ),
                    write("ServerScriptService.Currency", "currency"),
                ],
                vec![
                    skill(
                        "shop",
                        json!({ "item_name": "Speed Coil", "price": 100, "currency": "Coins" }),
                    ),
                    write("ServerScriptService.Shop", "shop"),
                ],
                vec![
                    skill(
                        "game_gui",
                        json!({ "title": "Coin Tycoon", "button_label": "Buy Speed Coil (100)", "currency": "Coins" }),
                    ),
                    write("StarterGui.GameGui", "game_gui"),
                ],
                vec![
                    skill("day_night", json!({ "cycle_minutes": 10 })),
                    write("ServerScriptService.DayNight", "day_night"),
                ],
                vec![playtest()],
            ]
            .into_iter()
            .flatten()
            .collect(),
            skills: vec!["currency", "shop", "game_gui", "day_night"],
            needs: vec![],
        },
        Idea {
            id: "round-arena",
            title: "Round Arena",
            pitch: "A round-based arena: rounds that start themselves, a \
                    leaderboard counting wins, a spawn pad, and a lava pit \
                    in the middle for sudden death.",
            plan: [
                vec![
                    skill("round_manager", json!({ "create_round": true })),
                    write("ServerScriptService.RoundManager", "round_manager"),
                ],
                vec![
                    skill(
                        "leaderboard",
                        json!({ "currency": "Wins", "start_amount": 0 }),
                    ),
                    write("ServerScriptService.Leaderboard", "leaderboard"),
                ],
                vec![spawn_pad()],
                vec![
                    skill("spawn_link", json!({ "instance_name": "SpawnPad" })),
                    write("ServerScriptService.SpawnLink", "spawn_link"),
                ],
                lava_brick(50),
                vec![playtest()],
            ]
            .into_iter()
            .flatten()
            .collect(),
            skills: vec!["round_manager", "leaderboard", "spawn_link", "kill_brick"],
            needs: vec![],
        },
        Idea {
            id: "anime-card-battles-apex",
            title: "Untitled Anime Card Game",
            pitch: "A sunlit anime floating-archipelago card battler: 24 original anime-style cards \
                    across 6 packs & 6 rarity tiers (Common 1/2 to Secret 1/50,000), pity & \
                    Fusion Weather Storms (Crimson Eclipse 5x / Celestial Tempest 4.5x Luck), \
                    CHAIN OVERDRIVE dual finishers, Living 3D Guardian Auras, Traits & Charms, \
                    4v4 tactical combat, Infinite Tower & World Raid Boss.",
            plan: {
                let mut steps = apex_hub_world_instances();
                steps.extend(vec![
                    skill(
                        "leaderboard",
                        json!({ "currency": "Coins", "start_amount": 250 }),
                    ),
                    write("ServerScriptService.Leaderboard", "leaderboard"),
                    skill(
                        "datastore_save",
                        json!({ "store_name": "OpenROApexEconomy_v1", "stat_name": "Coins", "default_value": 250, "autosave_seconds": 60, "bind_close": true }),
                    ),
                    write("ServerScriptService.DataStoreSave", "datastore_save"),
                    skill(
                        "shop",
                        json!({ "item_name": "Lucky Charm", "price": 250, "currency": "Coins" }),
                    ),
                    write("ServerScriptService.Shop", "shop"),
                    skill(
                        "game_gui",
                        json!({ "title": "Untitled Anime Card Game", "button_label": "Buy Lucky Charm (250)", "currency": "Coins" }),
                    ),
                    write("StarterGui.GameGui", "game_gui"),
                    skill(
                        "soundtrack",
                        json!({ "sound_id": 0, "volume": 6, "looped": true }),
                    ),
                    write("ServerScriptService.Soundtrack", "soundtrack"),
                    skill(
                        "card_gacha_roller",
                        json!({ "base_luck_pct": 100, "pity_threshold": 50, "fast_roll_ms": 350, "enable_borders": true }),
                    ),
                    write("ServerScriptService.CardGachaRoller", "card_gacha_roller"),
                    skill(
                        "deck_builder",
                        json!({ "deck_slots": 6, "fusion_cost": 3, "rebirth_floor": 10, "store_name": "OpenROApexDecks_v1", "bind_close": true }),
                    ),
                    write("ServerScriptService.DeckBuilder", "deck_builder"),
                    skill(
                        "card_battle_arena",
                        json!({ "team_size": 4, "ultimate_charge_max": 100, "counter_window_ms": 800, "boss_hp_scale": 4 }),
                    ),
                    write("ServerScriptService.CardBattleArena", "card_battle_arena"),
                    skill(
                        "tower_progression",
                        json!({ "max_floors": 50, "world_count": 5, "base_reward": 120, "store_name": "OpenROTowerBoard_v1", "bind_close": true }),
                    ),
                    write("ServerScriptService.TowerProgression", "tower_progression"),
                    skill(
                        "potions_weather",
                        json!({ "weather_cycle_seconds": 90, "luck_potion_cost": 150, "speed_potion_cost": 120, "enable_codes": true }),
                    ),
                    write("ServerScriptService.PotionsWeather", "potions_weather"),
                    skill(
                        "card_gallery_world",
                        json!({ "lobby_name": "ApexLobby", "showcase_count": 8, "enable_holograms": true, "create_structures": true }),
                    ),
                    write("ServerScriptService.CardGalleryWorld", "card_gallery_world"),
                    playtest(),
                ]);
                steps
            },
            skills: vec![
                "leaderboard",
                "datastore_save",
                "shop",
                "game_gui",
                "soundtrack",
                "card_gacha_roller",
                "deck_builder",
                "card_battle_arena",
                "tower_progression",
                "potions_weather",
                "card_gallery_world",
            ],
            needs: vec![],
        },
    ]
}

/// Feature ideas offered after something is built.
fn feature_ideas() -> Vec<Idea> {
    vec![
        Idea {
            id: "add-shop",
            title: "A shop for the money",
            pitch: "Your game has money now — give players something to spend \
                    it on: a shop selling a Speed Coil, plus the game screen \
                    with the balance and the shop button on it.",
            plan: vec![
                skill(
                    "shop",
                    json!({ "item_name": "Speed Coil", "price": 100, "currency": "Coins" }),
                ),
                write("ServerScriptService.Shop", "shop"),
                skill(
                    "game_gui",
                    json!({ "title": "My Game", "button_label": "Buy Speed Coil (100)", "currency": "Coins" }),
                ),
                write("StarterGui.GameGui", "game_gui"),
                playtest(),
            ],
            skills: vec!["shop", "game_gui"],
            needs: vec!["currency"],
        },
        Idea {
            id: "add-currency",
            title: "Money that saves",
            pitch: "Give players a balance that lasts: a currency that loads \
                    from a DataStore when they join, keeps paying while they \
                    play, and saves when they leave — they come back to a \
                    balance, not zero.",
            plan: vec![
                skill(
                    "currency",
                    json!({ "currency": "Coins", "start_amount": 0, "earn_amount": 10, "earn_seconds": 5, "bind_close": true }),
                ),
                write("ServerScriptService.Currency", "currency"),
                playtest(),
            ],
            skills: vec!["currency"],
            needs: vec![],
        },
        Idea {
            id: "add-checkpoints",
            title: "Checkpoints",
            pitch: "Add stage checkpoints: pads that remember each player's \
                    progress and respawn them where they got to, not back at \
                    the start.",
            plan: vec![
                skill(
                    "checkpoints",
                    json!({ "stage_count": 5, "folder_name": "Checkpoints", "create_folder": true }),
                ),
                write("ServerScriptService.Checkpoints", "checkpoints"),
                playtest(),
            ],
            skills: vec!["checkpoints"],
            needs: vec![],
        },
        Idea {
            id: "add-soundtrack",
            title: "A soundtrack",
            pitch: "Wire up the music: a soundtrack slot in SoundService, \
                    looping, volume set — silent until you pick a real audio \
                    id, then it's just there.",
            plan: vec![
                skill(
                    "soundtrack",
                    json!({ "sound_id": 0, "volume": 5, "looped": true }),
                ),
                write("ServerScriptService.Soundtrack", "soundtrack"),
                playtest(),
            ],
            skills: vec!["soundtrack"],
            needs: vec![],
        },
        Idea {
            id: "add-day-night",
            title: "A living sky",
            pitch: "Add a day/night cycle so the place breathes — the light \
                    turns through a full day every few minutes.",
            plan: vec![
                skill("day_night", json!({ "cycle_minutes": 6 })),
                write("ServerScriptService.DayNight", "day_night"),
                playtest(),
            ],
            skills: vec!["day_night"],
            needs: vec![],
        },
        Idea {
            id: "add-lava",
            title: "Lava hazards",
            pitch: "Drop in a lava brick that burns anyone who touches it — \
                    instant stakes anywhere you place it.",
            plan: [lava_brick(40), vec![playtest()]]
                .into_iter()
                .flatten()
                .collect(),
            skills: vec!["kill_brick"],
            needs: vec![],
        },
        Idea {
            id: "add-jump-pad",
            title: "Velocity jump pads",
            pitch: "Drop in glowing neon bounce pads that launch players sky-high \
                    with per-player cooldowns, burst particles, and bounce SFX.",
            plan: vec![
                instance(
                    "Part",
                    "JumpPad",
                    "Workspace",
                    json!({
                        "Anchored": true,
                        "CanCollide": true,
                        "Material": "Neon",
                        "Color": "#3CB371",
                        "Size": [8, 1, 8],
                        "Position": [0, 1, 16]
                    }),
                ),
                skill(
                    "jump_pad",
                    json!({ "boost_power": 125, "cooldown_ms": 600, "pad_name": "JumpPad", "create_pad": false }),
                ),
                write("ServerScriptService.JumpPad", "jump_pad"),
                playtest(),
            ],
            skills: vec!["jump_pad"],
            needs: vec![],
        },
        Idea {
            id: "add-moving-platform",
            title: "Moving platforms",
            pitch: "Add a smooth Heartbeat-driven moving platform that carries \
                    standing players cleanly across gaps without slipping off.",
            plan: vec![
                instance(
                    "Part",
                    "MovingPlatform",
                    "Workspace",
                    json!({
                        "Anchored": true,
                        "CanCollide": true,
                        "Material": "SmoothPlastic",
                        "Color": "#4F7CF0",
                        "Size": [10, 1, 10],
                        "Position": [0, 6, 32]
                    }),
                ),
                skill(
                    "moving_platform",
                    json!({ "platform_name": "MovingPlatform", "travel_distance": 24, "travel_seconds": 4, "axis": "X", "create_platform": false }),
                ),
                write("ServerScriptService.MovingPlatform", "moving_platform"),
                playtest(),
            ],
            skills: vec!["moving_platform"],
            needs: vec![],
        },
        Idea {
            id: "add-datastore-save",
            title: "Resilient DataStore save",
            pitch: "Harden progress persistence with retry-backed UpdateAsync saves, \
                    bad-load protection, periodic autosave, and parallel BindToClose shutdown saves.",
            plan: vec![
                skill(
                    "datastore_save",
                    json!({ "store_name": "OpenROPlayerData", "stat_name": "Coins", "default_value": 0, "autosave_seconds": 60, "bind_close": true }),
                ),
                write("ServerScriptService.DataStoreSave", "datastore_save"),
                playtest(),
            ],
            skills: vec!["datastore_save"],
            needs: vec![],
        },
        Idea {
            id: "add-npc-patrol",
            title: "Patrolling NPC guard",
            pitch: "Spawn a PathfindingService guard NPC that walks a perimeter route, \
                    speaks overhead callouts, and deals debounced melee damage.",
            plan: vec![
                skill(
                    "npc_patrol",
                    json!({ "npc_name": "GuardNPC", "waypoint_count": 4, "walk_speed": 12, "damage": 15, "dialogue": "Halt! Restricted zone." }),
                ),
                write("ServerScriptService.NpcPatrol", "npc_patrol"),
                playtest(),
            ],
            skills: vec!["npc_patrol"],
            needs: vec![],
        },
        Idea {
            id: "add-portal-teleporter",
            title: "Paired warp portals",
            pitch: "Link two glowing CFrame warp pads (PortalA <-> PortalB) with \
                    per-player debounce cooldowns and particle warp bursts.",
            plan: vec![
                skill(
                    "portal_teleporter",
                    json!({ "pair_name": "WarpPortal", "cooldown_seconds": 3, "offset_distance": 60, "create_portals": true }),
                ),
                write("ServerScriptService.PortalTeleporter", "portal_teleporter"),
                playtest(),
            ],
            skills: vec!["portal_teleporter"],
            needs: vec![],
        },
        Idea {
            id: "add-combat-sword",
            title: "Server-validated sword combat",
            pitch: "Give players a ClassicSword tool with server-verified range, \
                    equipped checks, swing cooldowns, and a live KOs leaderboard hook.",
            plan: vec![
                skill(
                    "combat_sword",
                    json!({ "sword_name": "ClassicSword", "damage": 25, "cooldown_ms": 450, "range_studs": 10 }),
                ),
                write("ServerScriptService.CombatSword", "combat_sword"),
                playtest(),
            ],
            skills: vec!["combat_sword"],
            needs: vec![],
        },
        Idea {
            id: "add-inventory-backpack",
            title: "Authoritative inventory backpack",
            pitch: "Add a server-owned item inventory with slot caps, replicated \
                    Inventory folder, and rate-limited equip/unequip RemoteEvent.",
            plan: vec![
                skill(
                    "inventory_backpack",
                    json!({ "starter_item": "Torch", "max_slots": 12, "remote_name": "OpenROInventoryEquip" }),
                ),
                write("ServerScriptService.InventoryBackpack", "inventory_backpack"),
                playtest(),
            ],
            skills: vec!["inventory_backpack"],
            needs: vec![],
        },
        Idea {
            id: "add-quest-tracker",
            title: "Milestone quest tracker",
            pitch: "Track player objective milestones server-side with progress sync \
                    events and automatic currency rewards upon completion.",
            plan: vec![
                skill(
                    "quest_tracker",
                    json!({ "quest_name": "Crystal Hunt", "target_count": 5, "reward_amount": 150, "currency": "Coins" }),
                ),
                write("ServerScriptService.QuestTracker", "quest_tracker"),
                playtest(),
            ],
            skills: vec!["quest_tracker"],
            needs: vec![],
        },
        Idea {
            id: "add-vehicle-spawner",
            title: "Vehicle spawner bay",
            pitch: "Place a ProximityPrompt vehicle bay that spawns a welded VehicleSeat \
                    rover with a strict 1-vehicle-per-player cap and leave cleanup.",
            plan: vec![
                skill(
                    "vehicle_spawner",
                    json!({ "vehicle_name": "Rover", "max_speed": 65, "cooldown_seconds": 5, "create_pad": true }),
                ),
                write("ServerScriptService.VehicleSpawner", "vehicle_spawner"),
                playtest(),
            ],
            skills: vec!["vehicle_spawner"],
            needs: vec![],
        },
        Idea {
            id: "add-weather-cycle",
            title: "Dynamic weather cycle",
            pitch: "Transition smoothly across Clear, Dusk, Fog, and Rain using \
                    Lighting & Atmosphere tweens plus overhead rain particles.",
            plan: vec![
                skill(
                    "weather_cycle",
                    json!({ "cycle_seconds": 45, "start_weather": "Clear", "enable_particles": true }),
                ),
                write("ServerScriptService.WeatherCycle", "weather_cycle"),
                playtest(),
            ],
            skills: vec!["weather_cycle"],
            needs: vec![],
        },
        Idea {
            id: "add-tycoon-dropper",
            title: "Tycoon dropper & upgrader",
            pitch: "Drop in a full dropper -> conveyor -> collector -> x2 purchase \
                    button loop that pays directly into leaderstats currency.",
            plan: vec![
                skill(
                    "tycoon_dropper",
                    json!({ "tycoon_name": "FactoryTycoon", "drop_value": 10, "drop_interval": 2, "upgrade_cost": 100, "currency": "Coins" }),
                ),
                write("ServerScriptService.TycoonDropper", "tycoon_dropper"),
                playtest(),
            ],
            skills: vec!["tycoon_dropper"],
            needs: vec![],
        },
        Idea {
            id: "add-admin-spectate",
            title: "Spectator cycle & status feed",
            pitch: "Wire up a server-validated spectator target switcher with \
                    authorization checks and a live Alive/Total round status feed.",
            plan: vec![
                skill(
                    "admin_spectate",
                    json!({ "remote_name": "OpenROSpectate", "cycle_interval": 5, "allow_all_players": true }),
                ),
                write("ServerScriptService.AdminSpectate", "admin_spectate"),
                playtest(),
            ],
            skills: vec!["admin_spectate"],
            needs: vec![],
        },
        Idea {
            id: "add-card-gacha-roller",
            title: "Card gacha roller & foil borders",
            pitch: "Add a 7-tier card roller (Common to Secret) with Foil/Shiny/Gold/Rainbow/Void \
                    borders, pity guarantees, and fast/auto rolling.",
            plan: vec![
                skill(
                    "card_gacha_roller",
                    json!({ "base_luck_pct": 100, "pity_threshold": 50, "fast_roll_ms": 350, "enable_borders": true }),
                ),
                write("ServerScriptService.CardGachaRoller", "card_gacha_roller"),
                playtest(),
            ],
            skills: vec!["card_gacha_roller"],
            needs: vec![],
        },
        Idea {
            id: "add-deck-builder",
            title: "6-slot deck builder & Star Fusion",
            pitch: "Give players persistent 6-slot card decks, Collection Index bonuses, \
                    duplicate Star Fusion rank-ups, Auto-Equip Best, and Rebirth Ascension.",
            plan: vec![
                skill(
                    "deck_builder",
                    json!({ "deck_slots": 6, "fusion_cost": 3, "rebirth_floor": 10, "store_name": "OpenROApexDecks_v1", "bind_close": true }),
                ),
                write("ServerScriptService.DeckBuilder", "deck_builder"),
                playtest(),
            ],
            skills: vec!["deck_builder"],
            needs: vec![],
        },
        Idea {
            id: "add-card-battle-arena",
            title: "Tactical card battle arena & boss raids",
            pitch: "Add tactical and auto turn-based card combat with 5 Elemental Synergies, \
                    Ultimate Burst Gauges, Active Counter timing, and multi-phase Raid Bosses.",
            plan: vec![
                skill(
                    "card_battle_arena",
                    json!({ "team_size": 4, "ultimate_charge_max": 100, "counter_window_ms": 800, "boss_hp_scale": 4 }),
                ),
                write("ServerScriptService.CardBattleArena", "card_battle_arena"),
                playtest(),
            ],
            skills: vec!["card_battle_arena"],
            needs: vec![],
        },
        Idea {
            id: "add-tower-progression",
            title: "50-floor Endless Anime Tower",
            pitch: "Build a 50-floor progression tower across 5 themed World Portals with \
                    boss milestone bonuses and a persistent global Tower Leaderboard.",
            plan: vec![
                skill(
                    "tower_progression",
                    json!({ "max_floors": 50, "world_count": 5, "base_reward": 120, "store_name": "OpenROTowerBoard_v1", "bind_close": true }),
                ),
                write("ServerScriptService.TowerProgression", "tower_progression"),
                playtest(),
            ],
            skills: vec!["tower_progression"],
            needs: vec![],
        },
        Idea {
            id: "add-potions-weather",
            title: "Server weather, potions & codes",
            pitch: "Add Solar Flare, Void Eclipse, Thunderstorm, and Blood Moon weather \
                    events, craftable Luck & Speed potions, and redeemable reward codes.",
            plan: vec![
                skill(
                    "potions_weather",
                    json!({ "weather_cycle_seconds": 90, "luck_potion_cost": 150, "speed_potion_cost": 120, "enable_codes": true }),
                ),
                write("ServerScriptService.PotionsWeather", "potions_weather"),
                playtest(),
            ],
            skills: vec!["potions_weather"],
            needs: vec![],
        },
        Idea {
            id: "add-card-gallery-world",
            title: "3D Card Gallery Studio Lobby",
            pitch: "Construct the full 3D card-battler courtyard with Holographic Roll \
                    Pedestal, 3D Card Showcase Billboards, Tower Gate, Boss Shrine, and Leaderboard Wall.",
            plan: vec![
                skill(
                    "card_gallery_world",
                    json!({ "lobby_name": "ApexLobby", "showcase_count": 8, "enable_holograms": true, "create_structures": true }),
                ),
                write("ServerScriptService.CardGalleryWorld", "card_gallery_world"),
                playtest(),
            ],
            skills: vec!["card_gallery_world"],
            needs: vec![],
        },
    ]
}

/// Every idea the catalogue knows, opening games first.
pub fn catalogue() -> Vec<Idea> {
    let mut all = opening_ideas();
    all.extend(feature_ideas());
    all
}

/// Find one idea by id.
pub fn find(id: &str) -> Option<Idea> {
    catalogue().into_iter().find(|i| i.id == id)
}

/// The ideas to pitch for a chat whose game has `built` skills in it:
/// on an empty game, the opening complete games; otherwise up to three
/// follow-ups whose prerequisites are all built and at least one of
/// whose skills is not built yet (a half-built idea is still pitched —
/// building it completes the missing piece rather than stranding it).
pub fn ideas_for(built: &[String]) -> Vec<Idea> {
    if built.is_empty() {
        return opening_ideas();
    }
    feature_ideas()
        .into_iter()
        .filter(|idea| {
            idea.needs.iter().all(|n| built.iter().any(|b| b == n))
                && idea
                    .skills
                    .iter()
                    .any(|s| !built.iter().any(|b| b == s))
        })
        .take(3)
        .collect()
}

/// The thread lines for a pitch: the caller's intro line, then one
/// line per idea. These lines are the chat's record of the agent
/// speaking first — they persist with the chat like any other line.
pub fn pitch_lines(ideas: &[Idea], intro: &str) -> Vec<String> {
    if ideas.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![intro.to_string()];
    for idea in ideas {
        lines.push(format!("OpenRO: Idea — {}: {}", idea.title, idea.pitch));
    }
    lines
}
