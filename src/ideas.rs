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

#[derive(Clone, Debug)]
pub struct RevealTierSpec {
    pub tier: u8,
    pub name: &'static str,
    pub rarities: &'static str,
    pub duration_ms: u32,
    pub camera_kick_deg: f64,
    pub scrim_opacity: f64,
    pub border_hex_dark: &'static str,
    pub border_hex_light: &'static str,
    pub glyph: &'static str,
}

#[derive(Clone, Debug)]
pub struct JuiceSpec {
    pub windup_ms: u32,
    pub lunge_ms: u32,
    pub hit_stop_ms: u32,
    pub recoil_ms: u32,
    pub defeat_ms: u32,
    pub crit_camera_kick_deg: f64,
    pub reduced_motion_fade_ms: u32,
    pub damage_popups: bool,
    pub timing_bar_hook: bool,
}

#[derive(Clone, Debug)]
pub struct HudRolesSpec {
    pub phone_844x390_layout: &'static str,
    pub top_bar_max_pct: u8,
    pub center_clear_min_pct: u8,
    pub bottom_dock_max_pct: u8,
    pub reserve_strip_max_pct: u8,
    pub min_tap_target_px: u32,
    pub number_format: &'static str,
    pub anchors: &'static [&'static str],
}

#[derive(Clone, Debug)]
pub struct WorldDirectionSpec {
    pub setting: &'static str,
    pub lighting_mood: &'static str,
    pub landmark_hub: &'static str,
    pub landmarks: &'static [&'static str],
}

#[derive(Clone, Debug)]
pub struct EconomyTargetsSpec {
    pub currency: &'static str,
    pub earn_per_min: f64,
    pub rebirth_cost: f64,
    pub rebirth_mult: f64,
    pub gamepass_mult: f64,
    pub max_price: f64,
    pub zero_paid_luck: bool,
}

#[derive(Clone, Debug)]
pub struct FeelProfile {
    pub reveal_tiers: &'static [RevealTierSpec],
    pub juice: JuiceSpec,
    pub hud_roles: HudRolesSpec,
    pub gui_screen_slug: &'static str,
    pub audio_cues: &'static [&'static str],
    pub world_direction: WorldDirectionSpec,
    pub economy_targets: EconomyTargetsSpec,
    pub playtest_assertions: &'static [&'static str],
}

impl FeelProfile {
    pub fn to_json(&self) -> Value {
        json!({
            "reveal_tiers": self.reveal_tiers.iter().map(|t| json!({
                "tier": t.tier,
                "name": t.name,
                "rarities": t.rarities,
                "duration_ms": t.duration_ms,
                "camera_kick_deg": t.camera_kick_deg,
                "scrim_opacity": t.scrim_opacity,
                "border_hex_dark": t.border_hex_dark,
                "border_hex_light": t.border_hex_light,
                "glyph": t.glyph,
            })).collect::<Vec<_>>(),
            "juice": {
                "windup_ms": self.juice.windup_ms,
                "lunge_ms": self.juice.lunge_ms,
                "hit_stop_ms": self.juice.hit_stop_ms,
                "recoil_ms": self.juice.recoil_ms,
                "defeat_ms": self.juice.defeat_ms,
                "crit_camera_kick_deg": self.juice.crit_camera_kick_deg,
                "reduced_motion_fade_ms": self.juice.reduced_motion_fade_ms,
                "damage_popups": self.juice.damage_popups,
                "timing_bar_hook": self.juice.timing_bar_hook,
            },
            "hud_roles": {
                "phone_844x390_layout": self.hud_roles.phone_844x390_layout,
                "top_bar_max_pct": self.hud_roles.top_bar_max_pct,
                "center_clear_min_pct": self.hud_roles.center_clear_min_pct,
                "bottom_dock_max_pct": self.hud_roles.bottom_dock_max_pct,
                "reserve_strip_max_pct": self.hud_roles.reserve_strip_max_pct,
                "min_tap_target_px": self.hud_roles.min_tap_target_px,
                "number_format": self.hud_roles.number_format,
                "anchors": self.hud_roles.anchors,
            },
            "gui_screen_slug": self.gui_screen_slug,
            "audio_cues": self.audio_cues,
            "world_direction": {
                "setting": self.world_direction.setting,
                "lighting_mood": self.world_direction.lighting_mood,
                "landmark_hub": self.world_direction.landmark_hub,
                "landmarks": self.world_direction.landmarks,
            },
            "economy_targets": {
                "currency": self.economy_targets.currency,
                "earn_per_min": self.economy_targets.earn_per_min,
                "rebirth_cost": self.economy_targets.rebirth_cost,
                "rebirth_mult": self.economy_targets.rebirth_mult,
                "gamepass_mult": self.economy_targets.gamepass_mult,
                "max_price": self.economy_targets.max_price,
                "zero_paid_luck": self.economy_targets.zero_paid_luck,
            },
            "playtest_assertions": self.playtest_assertions,
        })
    }
}

#[derive(Clone)]
pub struct Idea {
    pub id: &'static str,
    pub title: &'static str,
    pub pitch: &'static str,
    pub niche_id: Option<&'static str>,
    pub feel_profile: Option<FeelProfile>,
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

fn tycoon_workshop_world_instances() -> Vec<Value> {
    vec![
        instance(
            "SpawnLocation",
            "SpawnPad",
            "Workspace",
            json!({
                "Anchored": true,
                "CanCollide": true,
                "Position": [0.0, 2.0, 24.0],
                "Size": [10.0, 1.0, 10.0],
                "Color": "#10B981",
                "Material": "SmoothPlastic",
            }),
        ),
        instance("Folder", "SunlitWorkshop", "Workspace", json!({})),
        instance("Sky", "WorkshopSky", "Lighting", json!({ "StarCount": 1200 })),
        instance("Atmosphere", "WorkshopAtmosphere", "Lighting", json!({ "Density": 0.14, "Haze": 0.35 })),
        instance("BloomEffect", "WorkshopBloom", "Lighting", json!({ "Enabled": true, "Intensity": 0.35, "Size": 14 })),
        part_at("WorkshopMeadow", "Workspace.SunlitWorkshop", [0.0, -1.5, 0.0], [140.0, 3.0, 140.0], "#4ADE80", "Grass"),
        part_at("AssemblyFloor", "Workspace.SunlitWorkshop", [0.0, 0.6, 0.0], [56.0, 1.2, 56.0], "#F8FAFC", "SmoothPlastic"),
        part_at("FactoryGantryL", "Workspace.SunlitWorkshop", [-14.0, 8.0, -6.0], [2.5, 14.0, 2.5], "#334155", "Metal"),
        part_at("FactoryGantryR", "Workspace.SunlitWorkshop", [14.0, 8.0, -6.0], [2.5, 14.0, 2.5], "#334155", "Metal"),
        part_at("FactoryCrossbeam", "Workspace.SunlitWorkshop", [0.0, 15.0, -6.0], [30.5, 2.0, 2.5], "#F59E0B", "Metal"),
        part_at("CollectorBeacon", "Workspace.SunlitWorkshop", [0.0, 5.5, 12.0], [3.0, 8.0, 3.0], "#FACC15", "Neon"),
        part_at("ShopCanopy", "Workspace.SunlitWorkshop", [-18.0, 4.5, 16.0], [12.0, 1.2, 10.0], "#0EA5E9", "SmoothPlastic"),
        part_at("QuestBoard", "Workspace.SunlitWorkshop", [18.0, 5.0, 16.0], [10.0, 7.0, 1.5], "#1E293B", "SmoothPlastic"),
    ]
}

fn citadel_arena_world_instances() -> Vec<Value> {
    vec![
        instance(
            "SpawnLocation",
            "SpawnPad",
            "Workspace",
            json!({
                "Anchored": true,
                "CanCollide": true,
                "Position": [0.0, 2.0, 32.0],
                "Size": [10.0, 1.0, 10.0],
                "Color": "#38BDF8",
                "Material": "SmoothPlastic",
            }),
        ),
        instance("Folder", "CitadelPlaza", "Workspace", json!({})),
        instance("Sky", "CitadelSky", "Lighting", json!({ "StarCount": 2400 })),
        instance("Atmosphere", "CitadelAtmosphere", "Lighting", json!({ "Density": 0.16, "Haze": 0.40 })),
        instance("BloomEffect", "CitadelBloom", "Lighting", json!({ "Enabled": true, "Intensity": 0.42, "Size": 16 })),
        part_at("CitadelCourtyard", "Workspace.CitadelPlaza", [0.0, -1.5, 0.0], [150.0, 3.0, 150.0], "#E2E8F0", "Marble"),
        part_at("ArenaRing", "Workspace.CitadelPlaza", [0.0, 0.5, -12.0], [52.0, 1.4, 52.0], "#1E293B", "Slate"),
        part_at("PortalArchL", "Workspace.CitadelPlaza", [-12.0, 7.0, -48.0], [3.0, 13.0, 3.0], "#6366F1", "Neon"),
        part_at("PortalArchR", "Workspace.CitadelPlaza", [12.0, 7.0, -48.0], [3.0, 13.0, 3.0], "#6366F1", "Neon"),
        part_at("SummonFountain", "Workspace.CitadelPlaza", [-26.0, 2.5, 14.0], [12.0, 3.0, 12.0], "#38BDF8", "Neon"),
        part_at("BossAltar", "Workspace.CitadelPlaza", [0.0, 2.2, -32.0], [18.0, 2.2, 12.0], "#DC2626", "Marble"),
        part_at("QuestObelisk", "Workspace.CitadelPlaza", [26.0, 6.0, 14.0], [4.0, 10.0, 4.0], "#F59E0B", "Neon"),
    ]
}

const CARD_BATTLER_REVEAL_TIERS: &[RevealTierSpec] = &[
    RevealTierSpec {
        tier: 1,
        name: "Common & Uncommon",
        rarities: "Common / Uncommon",
        duration_ms: 450,
        camera_kick_deg: 0.0,
        scrim_opacity: 0.15,
        border_hex_dark: "#CBD5E1",
        border_hex_light: "#334155",
        glyph: "●",
    },
    RevealTierSpec {
        tier: 2,
        name: "Rare & Epic",
        rarities: "Rare / Epic",
        duration_ms: 950,
        camera_kick_deg: 0.5,
        scrim_opacity: 0.25,
        border_hex_dark: "#7DD3FC",
        border_hex_light: "#1D4ED8",
        glyph: "★",
    },
    RevealTierSpec {
        tier: 3,
        name: "Legendary",
        rarities: "Legendary",
        duration_ms: 1650,
        camera_kick_deg: 2.5,
        scrim_opacity: 0.45,
        border_hex_dark: "#FCD34D",
        border_hex_light: "#92400E",
        glyph: "♛",
    },
    RevealTierSpec {
        tier: 4,
        name: "Mythic & Legacy",
        rarities: "Mythic / Legacy",
        duration_ms: 2600,
        camera_kick_deg: 3.5,
        scrim_opacity: 0.65,
        border_hex_dark: "#FDA4AF",
        border_hex_light: "#BE123C",
        glyph: "✦",
    },
];

const TYCOON_REVEAL_TIERS: &[RevealTierSpec] = &[
    RevealTierSpec {
        tier: 1,
        name: "Standard Drop",
        rarities: "Standard Ore",
        duration_ms: 350,
        camera_kick_deg: 0.0,
        scrim_opacity: 0.0,
        border_hex_dark: "#6EE7B7",
        border_hex_light: "#047857",
        glyph: "◆",
    },
    RevealTierSpec {
        tier: 2,
        name: "Upgraded Surge",
        rarities: "2x / Golden Surge",
        duration_ms: 750,
        camera_kick_deg: 1.0,
        scrim_opacity: 0.20,
        border_hex_dark: "#FCD34D",
        border_hex_light: "#92400E",
        glyph: "★",
    },
];

const ACTION_RPG_REVEAL_TIERS: &[RevealTierSpec] = &[
    RevealTierSpec {
        tier: 1,
        name: "Wave Clear",
        rarities: "Standard Wave",
        duration_ms: 650,
        camera_kick_deg: 0.8,
        scrim_opacity: 0.15,
        border_hex_dark: "#7DD3FC",
        border_hex_light: "#1D4ED8",
        glyph: "★",
    },
    RevealTierSpec {
        tier: 2,
        name: "Boss Phase & Awakening",
        rarities: "Boss / Awakening",
        duration_ms: 1400,
        camera_kick_deg: 2.0,
        scrim_opacity: 0.45,
        border_hex_dark: "#FCD34D",
        border_hex_light: "#92400E",
        glyph: "♛",
    },
];

fn card_battler_feel_profile() -> FeelProfile {
    FeelProfile {
        reveal_tiers: CARD_BATTLER_REVEAL_TIERS,
        juice: JuiceSpec {
            windup_ms: 130,
            lunge_ms: 150,
            hit_stop_ms: 50,
            recoil_ms: 220,
            defeat_ms: 400,
            crit_camera_kick_deg: 2.0,
            reduced_motion_fade_ms: 180,
            damage_popups: true,
            timing_bar_hook: true,
        },
        hud_roles: HudRolesSpec {
            phone_844x390_layout: "2-col split (44% RevealCard / 56% Pity+Roll), top<=14%, center>=58%, bottom<=26%, reserve<=16%",
            top_bar_max_pct: 14,
            center_clear_min_pct: 58,
            bottom_dock_max_pct: 26,
            reserve_strip_max_pct: 16,
            min_tap_target_px: 44,
            number_format: "1 in N + K/M",
            anchors: &[
                "TopEdge: Player/Enemy HP + Turn Pill",
                "BottomCenter: Reveal Free Card + Mythic Pity + Auto Roll",
                "BottomLeft: Luck & Buffs Pill (pausable)",
                "BattleReserveDock: 4-card ordered strip (<=16% height)",
            ],
        },
        gui_screen_slug: "anime-rng-cards-hud",
        audio_cues: &[
            "summon_roll",
            "rare_chime",
            "legendary_drop",
            "mythic_fanfare",
            "battle_hit",
            "battle_crit",
            "death_shatter",
            "ui_click",
        ],
        world_direction: WorldDirectionSpec {
            setting: "Sunlit floating sky-archipelago courtyard with Torii summon shrine and colosseum",
            lighting_mood: "Clear daylight sky with crisp BloomEffect (0.45) and SunRaysEffect (0.14)",
            landmark_hub: "ApexLobby",
            landmarks: &[
                "SpawnPad",
                "ToriiSummonShrine",
                "RollPedestal",
                "TowerPortal",
                "BossRaidShrine",
                "DuelArenaStage",
            ],
        },
        economy_targets: EconomyTargetsSpec {
            currency: "Coins",
            earn_per_min: 120.0,
            rebirth_cost: 1800.0,
            rebirth_mult: 1.5,
            gamepass_mult: 1.0,
            max_price: 250.0,
            zero_paid_luck: true,
        },
        playtest_assertions: &[
            "First free card draw + battle + reward claim completes within 60s",
            "Zero paid luck (gamepass_mult == 1.0) and balanced 60m curve",
            "Safe areas, >=44x44 tap targets, and >=4.5:1 contrast across 1920x1080, 1024x768, 844x390",
            "DataStore BindToClose persistence + 0 Luau errors in headless sandbox",
        ],
    }
}

fn tycoon_feel_profile() -> FeelProfile {
    FeelProfile {
        reveal_tiers: TYCOON_REVEAL_TIERS,
        juice: JuiceSpec {
            windup_ms: 90,
            lunge_ms: 140,
            hit_stop_ms: 40,
            recoil_ms: 180,
            defeat_ms: 300,
            crit_camera_kick_deg: 1.0,
            reduced_motion_fade_ms: 150,
            damage_popups: false,
            timing_bar_hook: true,
        },
        hud_roles: HudRolesSpec {
            phone_844x390_layout: "Top-right Balance/Quest pill + bottom-center Upgrade/Shop dock clear of thumbsticks",
            top_bar_max_pct: 14,
            center_clear_min_pct: 64,
            bottom_dock_max_pct: 22,
            reserve_strip_max_pct: 14,
            min_tap_target_px: 44,
            number_format: "Comma <10k, K/M >=10k",
            anchors: &[
                "TopRight: Coins + Upgrade Lv Pill",
                "TopCenter: Milestone Quest Tracker",
                "BottomCenter: Upgrade Dropper + Shop Dock",
            ],
        },
        gui_screen_slug: "viral-sim-tycoon-hud",
        audio_cues: &[
            "level_up",
            "pack_open",
            "potion_drink",
            "ui_click",
            "button_hover",
            "ambient_plaza",
        ],
        world_direction: WorldDirectionSpec {
            setting: "Sunlit open-air workshop meadow with brass gantry, conveyor foundry, and collector beacon",
            lighting_mood: "Bright meadow daylight with gentle BloomEffect (0.35) and crisp contrast silhouettes",
            landmark_hub: "SunlitWorkshop",
            landmarks: &[
                "SpawnPad",
                "AssemblyFloor",
                "FactoryCrossbeam",
                "CollectorBeacon",
                "ShopCanopy",
                "QuestBoard",
            ],
        },
        economy_targets: EconomyTargetsSpec {
            currency: "Coins",
            earn_per_min: 120.0,
            rebirth_cost: 1800.0,
            rebirth_mult: 1.5,
            gamepass_mult: 1.5,
            max_price: 250.0,
            zero_paid_luck: false,
        },
        playtest_assertions: &[
            "10-second dropper -> conveyor -> collector -> upgrade loop awards Coins cleanly",
            "Quest tracker payout + shop purchase validate server-side",
            "DataStore UpdateAsync + BindToClose persistence passes rejoin check",
            "Safe areas, >=44x44 tap targets, and >=4.5:1 contrast across 3 viewports",
        ],
    }
}

fn action_rpg_td_feel_profile() -> FeelProfile {
    FeelProfile {
        reveal_tiers: ACTION_RPG_REVEAL_TIERS,
        juice: JuiceSpec {
            windup_ms: 110,
            lunge_ms: 150,
            hit_stop_ms: 50,
            recoil_ms: 200,
            defeat_ms: 360,
            crit_camera_kick_deg: 2.0,
            reduced_motion_fade_ms: 180,
            damage_popups: true,
            timing_bar_hook: true,
        },
        hud_roles: HudRolesSpec {
            phone_844x390_layout: "Top-right HP/Wave/Floor pill + bottom-center 3-button Strike/Portal/Shop dock",
            top_bar_max_pct: 14,
            center_clear_min_pct: 62,
            bottom_dock_max_pct: 24,
            reserve_strip_max_pct: 14,
            min_tap_target_px: 44,
            number_format: "Comma <10k, K/M >=10k",
            anchors: &[
                "TopRight: Coins + TowerFloor + KOs Pill",
                "TopCenter: Wave / Boss Phase Banner",
                "BottomCenter: Strike + World Warp + Shop Dock",
            ],
        },
        gui_screen_slug: "anime-action-rpg-td-hud",
        audio_cues: &[
            "battle_hit",
            "battle_crit",
            "death_shatter",
            "mythic_fanfare",
            "secret_alarm",
            "level_up",
            "ui_click",
        ],
        world_direction: WorldDirectionSpec {
            setting: "Sunlit stone citadel courtyard with arena ring, world portal arches, and summon fountain",
            lighting_mood: "High-clarity daylight arena with BloomEffect (0.42) and dark slate combat ring",
            landmark_hub: "CitadelPlaza",
            landmarks: &[
                "SpawnPad",
                "ArenaRing",
                "PortalArchL",
                "PortalArchR",
                "SummonFountain",
                "BossAltar",
                "QuestObelisk",
            ],
        },
        economy_targets: EconomyTargetsSpec {
            currency: "Coins",
            earn_per_min: 120.0,
            rebirth_cost: 2000.0,
            rebirth_mult: 1.5,
            gamepass_mult: 1.5,
            max_price: 250.0,
            zero_paid_luck: false,
        },
        playtest_assertions: &[
            "Server-validated melee sword + tower progression + quest tracker compose with 0 errors",
            "OrderedDataStore leaderboard + DataStore BindToClose pass rejoin check",
            "Safe areas, >=44x44 tap targets, and >=4.5:1 contrast across 3 viewports",
        ],
    }
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            title: "Anime RNG Card Battler",
            pitch: "A sunlit floating-archipelago anime card battler with 100% original \
                    sky-archipelago Orders, 4-tier rarity reveals (450/950/1650/2600ms), \
                    whole-card battle juice (50ms hit-stop, 150ms lunge, 220ms recoil, 2deg crit), \
                    844x390 2-column HUD, zero-paid-luck balanced economy, and 50-floor Tower.",
            niche_id: Some("anime-rng-cards"),
            feel_profile: Some(card_battler_feel_profile()),
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
                        json!({ "title": "Anime RNG Card Battler", "button_label": "Buy Lucky Charm (250)", "currency": "Coins" }),
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
        Idea {
            id: "viral-sim-tycoon-template",
            title: "Viral Sim & Tycoon",
            pitch: "A sunlit workshop & factory tycoon template with instant dropper -> conveyor -> \
                    collector -> x2 upgrade juice, milestone quests, shop upgrades, BindToClose \
                    DataStore saves, balanced 60-minute economy, and safe-area mobile HUD.",
            niche_id: Some("viral-sim-tycoon"),
            feel_profile: Some(tycoon_feel_profile()),
            plan: {
                let mut steps = tycoon_workshop_world_instances();
                steps.extend(vec![
                    skill("spawn_link", json!({ "instance_name": "SpawnPad" })),
                    write("ServerScriptService.SpawnLink", "spawn_link"),
                    skill(
                        "currency",
                        json!({ "currency": "Coins", "start_amount": 100, "earn_amount": 120, "earn_seconds": 60, "bind_close": true }),
                    ),
                    write("ServerScriptService.Currency", "currency"),
                    skill(
                        "tycoon_dropper",
                        json!({ "tycoon_name": "SunlitFoundry", "drop_value": 20, "drop_interval": 2, "upgrade_cost": 150, "currency": "Coins" }),
                    ),
                    write("ServerScriptService.TycoonDropper", "tycoon_dropper"),
                    skill(
                        "shop",
                        json!({ "item_name": "Magnet Collector", "price": 250, "currency": "Coins" }),
                    ),
                    write("ServerScriptService.Shop", "shop"),
                    skill(
                        "datastore_save",
                        json!({ "store_name": "OpenROWorkshopSave_v1", "stat_name": "Coins", "default_value": 100, "autosave_seconds": 60, "bind_close": true }),
                    ),
                    write("ServerScriptService.DataStoreSave", "datastore_save"),
                    skill(
                        "quest_tracker",
                        json!({ "quest_name": "Foundry Output", "target_count": 5, "reward_amount": 120, "currency": "Coins" }),
                    ),
                    write("ServerScriptService.QuestTracker", "quest_tracker"),
                    skill(
                        "game_gui",
                        json!({ "title": "Viral Sim & Tycoon", "button_label": "Buy Magnet Collector (250)", "currency": "Coins" }),
                    ),
                    write("StarterGui.GameGui", "game_gui"),
                    skill(
                        "soundtrack",
                        json!({ "sound_id": 0, "volume": 5, "looped": true }),
                    ),
                    write("ServerScriptService.Soundtrack", "soundtrack"),
                    playtest(),
                ]);
                steps
            },
            skills: vec![
                "spawn_link",
                "currency",
                "tycoon_dropper",
                "shop",
                "datastore_save",
                "quest_tracker",
                "game_gui",
                "soundtrack",
            ],
            needs: vec![],
        },
        Idea {
            id: "anime-action-rpg-td-template",
            title: "Anime Action RPG / Tower Defense",
            pitch: "A sunlit citadel action RPG & tower defense template with server-validated \
                    blade combat, 5-world tower portals & OrderedDataStore leaderboard, quest \
                    milestones, hit-stop boss phases, balanced economy, and safe-area HUD.",
            niche_id: Some("anime-action-rpg-td"),
            feel_profile: Some(action_rpg_td_feel_profile()),
            plan: {
                let mut steps = citadel_arena_world_instances();
                steps.extend(vec![
                    skill("spawn_link", json!({ "instance_name": "SpawnPad" })),
                    write("ServerScriptService.SpawnLink", "spawn_link"),
                    skill(
                        "currency",
                        json!({ "currency": "Coins", "start_amount": 150, "earn_amount": 120, "earn_seconds": 60, "bind_close": true }),
                    ),
                    write("ServerScriptService.Currency", "currency"),
                    skill(
                        "datastore_save",
                        json!({ "store_name": "OpenROCitadelSave_v1", "stat_name": "Coins", "default_value": 150, "autosave_seconds": 60, "bind_close": true }),
                    ),
                    write("ServerScriptService.DataStoreSave", "datastore_save"),
                    skill(
                        "combat_sword",
                        json!({ "sword_name": "SkycrestBlade", "damage": 35, "cooldown_ms": 400, "range_studs": 12 }),
                    ),
                    write("ServerScriptService.CombatSword", "combat_sword"),
                    skill(
                        "tower_progression",
                        json!({ "max_floors": 50, "world_count": 5, "base_reward": 120, "store_name": "OpenROCitadelTower_v1", "bind_close": true }),
                    ),
                    write("ServerScriptService.TowerProgression", "tower_progression"),
                    skill(
                        "quest_tracker",
                        json!({ "quest_name": "Citadel Vanguard", "target_count": 5, "reward_amount": 120, "currency": "Coins" }),
                    ),
                    write("ServerScriptService.QuestTracker", "quest_tracker"),
                    skill(
                        "shop",
                        json!({ "item_name": "Tempest Greaves", "price": 250, "currency": "Coins" }),
                    ),
                    write("ServerScriptService.Shop", "shop"),
                    skill(
                        "game_gui",
                        json!({ "title": "Anime Action RPG / Tower Defense", "button_label": "Buy Tempest Greaves (250)", "currency": "Coins" }),
                    ),
                    write("StarterGui.GameGui", "game_gui"),
                    skill(
                        "soundtrack",
                        json!({ "sound_id": 0, "volume": 6, "looped": true }),
                    ),
                    write("ServerScriptService.Soundtrack", "soundtrack"),
                    playtest(),
                ]);
                steps
            },
            skills: vec![
                "spawn_link",
                "currency",
                "datastore_save",
                "combat_sword",
                "tower_progression",
                "quest_tracker",
                "shop",
                "game_gui",
                "soundtrack",
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
            niche_id: None,
            feel_profile: None,
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
