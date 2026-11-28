//! Built-In Live Roblox Market Taste, 8-Dimension Taste Dossier & Anti-Slop Craft Engine
//! (`roblox-market-taste`).
//!
//! Grounds OpenRO's game pitches, architecture, and live builds in Roblox's live
//! `top-trending`, `up-and-coming`, and `top-playing-now` charts:
//! - Fetches live Explore charts on explicit refresh from
//!   `https://apis.roblox.com/explore-api/v1/get-sorts?sessionId=openro-taste`
//!   (overridable via `OPENRO_MARKET_API_URL`; NEVER called unsolicited on boot or tests).
//! - Classifies organic (non-sponsored) games into 8 profitable niches and persists
//!   `<data_dir>/market_taste_cache.json` atomically.
//! - Provides a full **8-Dimension Production Taste Dossier** (`market_leaders`,
//!   `gameplay_loop`, `ui_ux_style`, `animations_and_juice`, `model_and_world_3d_style`,
//!   `audio_cadence`, `monetization_and_retention`, `automatic_twists` + `quality_boosts`)
//!   and an opinionated **Top-Trending Craft Bar & Anti-Slop Rules** injected into the agent prompt.
//! - Supports `research_roblox_game(place_or_universe_id, niche_hint)` to inspect any live
//!   Roblox Place URL / ID or Universe ID via Roblox's public Universes, Games, Votes, and
//!   Thumbnails APIs (respecting `OPENRO_ROBLOX_API_BASE` for mock tests) and persist
//!   `<data_dir>/taste-dossiers/<universe_id>.json` & `TASTE-DOSSIER.md`.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Official public Roblox Explore sorts discovery endpoint (read-only GET, no auth required).
pub const DEFAULT_MARKET_API_URL: &str =
    "https://apis.roblox.com/explore-api/v1/get-sorts?sessionId=openro-taste";

/// Default active niche when no user selection has been persisted yet (general Top-Trending across all 8 genres).
pub const DEFAULT_ACTIVE_NICHE: &str = "top-trending";

/// Date stamp of the embedded deep-research chart baseline.
pub const BASELINE_DATE: &str = "2026-11-22";

/// Canonical list of all 8 specific profitable Roblox niches (plus `"top-trending"` all-genre overview).
pub const MARKET_NICHE_IDS: &[&str] = &[
    "anime-rng-cards",
    "viral-sim-tycoon",
    "physics-crash-ragdoll",
    "survival-expedition",
    "horror-coop-mascot",
    "tactical-arena-pvp",
    "anime-action-rpg-td",
    "social-rp-fashion",
];

/// Complete 8-Dimension Production Taste Profile for one Roblox niche.
#[derive(Clone, Debug)]
pub struct NicheProfile {
    pub id: &'static str,
    pub template_id: Option<&'static str>,
    pub name: &'static str,
    pub tagline: &'static str,
    pub craft_bar: &'static str,
    pub gameplay_loop: &'static [&'static str],
    pub ui_ux_style: &'static [&'static str],
    pub animations_and_juice: &'static [&'static str],
    pub model_and_world_3d_style: &'static [&'static str],
    pub audio_cadence: &'static [&'static str],
    pub monetization_loops: &'static [&'static str],
    pub retention_hooks: &'static [&'static str],
    pub anti_slop_guardrails: &'static [&'static str],
    pub automatic_twists: &'static [&'static str],
    pub quality_boosts: &'static [&'static str],
    pub pitch_prompt: &'static str,
}

/// One live-researched or baseline benchmark game inside a niche leaderboard.
#[derive(Clone, Debug, PartialEq)]
pub struct MarketGame {
    pub name: String,
    pub universe_id: u64,
    pub root_place_id: u64,
    pub ccu: u64,
    pub up_votes: u64,
    pub down_votes: u64,
    pub like_ratio_pct: f64,
    pub niche: String,
    pub sort_tags: Vec<String>,
    pub genre: String,
    /// `"live_roblox_explore"` or `"baseline_nov_2026"`
    pub source: String,
    pub why_it_wins: String,
}

impl MarketGame {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "universe_id": self.universe_id,
            "root_place_id": self.root_place_id,
            "ccu": self.ccu,
            "up_votes": self.up_votes,
            "down_votes": self.down_votes,
            "like_ratio_pct": self.like_ratio_pct,
            "niche": self.niche,
            "sort_tags": self.sort_tags,
            "genre": self.genre,
            "source": self.source,
            "why_it_wins": self.why_it_wins,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let name = v.get("name")?.as_str()?.trim().to_string();
        if name.is_empty() {
            return None;
        }
        let ccu = v.get("ccu").and_then(|x| x.as_u64()).unwrap_or(0);
        let up_votes = v.get("up_votes").and_then(|x| x.as_u64()).unwrap_or(0);
        let down_votes = v.get("down_votes").and_then(|x| x.as_u64()).unwrap_or(0);
        let like_ratio_pct = v
            .get("like_ratio_pct")
            .and_then(|x| x.as_f64())
            .unwrap_or_else(|| compute_like_ratio(up_votes, down_votes));
        let niche = v
            .get("niche")
            .and_then(|x| x.as_str())
            .and_then(|s| normalize_niche_id(s).ok())
            .unwrap_or(DEFAULT_ACTIVE_NICHE)
            .to_string();
        let sort_tags = v
            .get("sort_tags")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.as_str().map(ToString::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Some(MarketGame {
            name,
            universe_id: v.get("universe_id").and_then(|x| x.as_u64()).unwrap_or(0),
            root_place_id: v.get("root_place_id").and_then(|x| x.as_u64()).unwrap_or(0),
            ccu,
            up_votes,
            down_votes,
            like_ratio_pct,
            niche,
            sort_tags,
            genre: v
                .get("genre")
                .and_then(|x| x.as_str())
                .unwrap_or("Simulation")
                .to_string(),
            source: v
                .get("source")
                .and_then(|x| x.as_str())
                .unwrap_or("live_roblox_explore")
                .to_string(),
            why_it_wins: v
                .get("why_it_wins")
                .and_then(|x| x.as_str())
                .unwrap_or("Live Roblox Explore chart leader")
                .to_string(),
        })
    }
}

/// All 8 niche profiles.
pub fn all_niche_profiles() -> &'static [NicheProfile] {
    &NICHE_PROFILES
}

pub fn find_niche_profile(niche: &str) -> Option<&'static NicheProfile> {
    let id = normalize_niche_id(niche).ok()?;
    if id == "top-trending" {
        return Some(&TOP_TRENDING_PROFILE);
    }
    NICHE_PROFILES.iter().find(|p| p.id == id)
}

/// Normalize a niche identifier (accepting `"top-trending"` all-genre overview or any of the 8 niches).
pub fn normalize_niche_id(raw: &str) -> Result<&'static str, String> {
    let norm = raw.trim().to_ascii_lowercase().replace([' ', '_'], "-");
    if matches!(
        norm.as_str(),
        "top-trending" | "all" | "general" | "trending" | "all-genres"
    ) {
        return Ok("top-trending");
    }
    for &id in MARKET_NICHE_IDS {
        if norm == id {
            return Ok(id);
        }
    }
    Err(format!(
        "unknown market niche '{raw}' (expected 'top-trending' or one of: {})",
        MARKET_NICHE_IDS.join(", ")
    ))
}

/// Classify a live Roblox Explore game by title keywords + `genreL1` into one of the 8 niches.
pub fn classify_game_niche(name: &str, genre_l1: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let genre = genre_l1.trim().to_ascii_lowercase();

    if contains_any(
        &lower,
        &[
            "ragdoll",
            "crash",
            "bloxng",
            "physics",
            "destruction",
            "demolition",
            "beamng",
            "car crush",
            "euphoria",
            "fling",
        ],
    ) {
        return "physics-crash-ragdoll";
    }

    if contains_any(
        &lower,
        &[
            "dandy",
            "forsaken",
            "horror",
            "doors",
            "pressure",
            "butcher",
            "violence district",
            "murder mystery",
            "piggy",
            "mimic",
            "backrooms",
            "mascot",
            "scary",
            "nightmare",
            "haunted",
            "fnaf",
            "scp",
            "slenderman",
        ],
    ) {
        return "horror-coop-mascot";
    }

    if contains_any(
        &lower,
        &[
            "nights in the forest",
            "down the river",
            "dead rails",
            "expedition",
            "raft",
            "wilderness",
            "100 miles",
            "survive",
            "survival",
            "stranded",
            "apocalypse",
            "outbreak",
            "camp",
        ],
    ) {
        return "survival-expedition";
    }

    if contains_any(
        &lower,
        &[
            "card battle",
            "card game",
            "rng",
            "aura",
            "gacha",
            "roll",
            "card",
            "summon",
            "luck",
            "fisch",
            "fish it",
            "sol's",
            "tarot",
            "deck",
        ],
    ) {
        return "anime-rng-cards";
    }

    if contains_any(
        &lower,
        &[
            "blox fruits",
            "jujutsu",
            "anime",
            "shenanigans",
            "crusaders",
            "vanguard",
            "tower defense",
            " td",
            "slayer",
            "piece",
            "ninja",
            "shinobi",
            "kaiju",
            "arise",
            "solo",
            "saiki",
            "dragon ball",
            "ghoul",
            "jojo",
            "battlegrounds",
            "strongest",
        ],
    ) {
        return "anime-action-rpg-td";
    }

    if contains_any(
        &lower,
        &[
            "rivals",
            "stand out",
            "bedwars",
            "blade ball",
            "arsenal",
            "phantom forces",
            "frontlines",
            "volleyball",
            "blue lock",
            "boxing",
            "duel",
            "arena",
            "fps",
            "shooter",
            "sniper",
            "gunfight",
            "combat",
            "pvp",
            "wars",
        ],
    ) {
        return "tactical-arena-pvp";
    }

    if contains_any(
        &lower,
        &[
            "brookhaven",
            "dress to impress",
            "adopt me",
            "berry avenue",
            "catalog avatar",
            "roleplay",
            " rp",
            "rp ",
            "fashion",
            "runway",
            "avatar",
            "outfit",
            "salon",
            "high school",
            "livetopia",
            "meep",
            "club",
            "hangout",
        ],
    ) {
        return "social-rp-fashion";
    }

    if contains_any(
        &lower,
        &[
            "steal",
            "egg",
            "throw food",
            "brainrot",
            "tycoon",
            "simulator",
            "sim",
            "pet",
            "pets",
            "farm",
            "garden",
            "grow a",
            "merge",
            "clicker",
            "factory",
            "bakery",
            "shop",
            "mining",
            "dig",
        ],
    ) {
        return "viral-sim-tycoon";
    }

    match genre.as_str() {
        "roleplay & avatar sim" | "shopping" | "entertainment" => "social-rp-fashion",
        "shooter" | "sports & racing" => "tactical-arena-pvp",
        "survival" => "survival-expedition",
        "rpg" | "action" | "strategy" | "adventure" => "anime-action-rpg-td",
        "obby & platformer" | "party & casual" | "puzzle" => "physics-crash-ragdoll",
        _ => "viral-sim-tycoon",
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

fn compute_like_ratio(up: u64, down: u64) -> f64 {
    let total = up.saturating_add(down);
    if total == 0 {
        return 0.0;
    }
    ((up as f64 / total as f64) * 1000.0).round() / 10.0
}

/// Extract a Roblox Place ID or Universe ID from a raw numeric string or Roblox game URL.
/// Returns `(id, is_place_url)`.
pub fn parse_roblox_id_input(raw: &str) -> Result<(u64, bool), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 512 || trimmed.contains('\0') {
        return Err("Roblox game URL or numeric ID must not be empty".to_string());
    }
    if trimmed.bytes().all(|b| b.is_ascii_digit()) {
        let id = trimmed
            .parse::<u64>()
            .map_err(|_| format!("invalid Roblox numeric ID '{trimmed}'"))?;
        if id == 0 {
            return Err("Roblox ID must be greater than zero".to_string());
        }
        return Ok((id, false));
    }
    // Accept URLs like https://www.roblox.com/games/130253630950453/Anime-Card-Battles-Rebirth
    if let Some(idx) = trimmed.find("/games/") {
        let after = &trimmed[idx + "/games/".len()..];
        let digits: String = after
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            let id = digits
                .parse::<u64>()
                .map_err(|_| format!("invalid Roblox place ID in URL '{trimmed}'"))?;
            if id > 0 {
                return Ok((id, true));
            }
        }
    }
    // Accept query params like ?universeId=10092619788 or ?placeId=130253630950453
    for (needle, is_place) in [("placeId=", true), ("universeId=", false), ("universeIds=", false)] {
        if let Some(idx) = trimmed.find(needle) {
            let after = &trimmed[idx + needle.len()..];
            let digits: String = after
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(id) = digits.parse::<u64>() {
                if id > 0 {
                    return Ok((id, is_place));
                }
            }
        }
    }
    Err(format!(
        "could not extract a Roblox Place ID or Universe ID from '{trimmed}'"
    ))
}

/// Pure parser for Roblox Explore `get-sorts` JSON payloads (live or mock).
/// Skips sponsored ad slots, merges duplicate games across sorts, and classifies into the 8 niches.
pub fn parse_explore_payload(payload: &Value) -> Result<(Vec<MarketGame>, Vec<String>), String> {
    let Some(sorts) = payload.get("sorts").and_then(|v| v.as_array()) else {
        return Err("Roblox Explore payload missing 'sorts' array".to_string());
    };

    let mut by_key: BTreeMap<String, MarketGame> = BTreeMap::new();
    let mut sort_ids_seen: Vec<String> = Vec::new();

    for sort in sorts {
        let sort_id = sort
            .get("sortId")
            .or_else(|| sort.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .trim()
            .to_string();
        let Some(games) = sort.get("games").and_then(|v| v.as_array()) else {
            continue;
        };
        if games.is_empty() {
            continue;
        }
        if !sort_ids_seen.contains(&sort_id) {
            sort_ids_seen.push(sort_id.clone());
        }

        for g in games {
            if g.get("isSponsored")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                continue;
            }
            let Some(name) = g
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let universe_id = g.get("universeId").and_then(|v| v.as_u64()).unwrap_or(0);
            let root_place_id = g.get("rootPlaceId").and_then(|v| v.as_u64()).unwrap_or(0);
            let ccu = g
                .get("playerCount")
                .or_else(|| g.get("ccu"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let up = g.get("totalUpVotes").and_then(|v| v.as_u64()).unwrap_or(0);
            let down = g.get("totalDownVotes").and_then(|v| v.as_u64()).unwrap_or(0);
            let genre = g
                .get("genreL1")
                .or_else(|| g.get("genre"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let like_ratio_pct = compute_like_ratio(up, down);
            let niche = classify_game_niche(name, &genre).to_string();
            let dedup_key = if universe_id > 0 {
                format!("u:{universe_id}")
            } else {
                format!("n:{}", name.to_ascii_lowercase())
            };

            let why = if like_ratio_pct > 0.0 {
                format!("Live Roblox chart leader ({sort_id}) · {like_ratio_pct:.1}% likes")
            } else {
                format!("Live Roblox chart leader ({sort_id})")
            };

            by_key
                .entry(dedup_key)
                .and_modify(|existing| {
                    if ccu > existing.ccu {
                        existing.ccu = ccu;
                    }
                    if up > existing.up_votes {
                        existing.up_votes = up;
                        existing.down_votes = down;
                        existing.like_ratio_pct = like_ratio_pct;
                    }
                    if !existing.sort_tags.contains(&sort_id) {
                        existing.sort_tags.push(sort_id.clone());
                    }
                })
                .or_insert(MarketGame {
                    name: name.to_string(),
                    universe_id,
                    root_place_id,
                    ccu,
                    up_votes: up,
                    down_votes: down,
                    like_ratio_pct,
                    niche,
                    sort_tags: vec![sort_id.clone()],
                    genre: if genre.is_empty() {
                        "Experience".to_string()
                    } else {
                        genre
                    },
                    source: "live_roblox_explore".to_string(),
                    why_it_wins: why,
                });
        }
    }

    let mut games: Vec<MarketGame> = by_key.into_values().collect();
    games.sort_by(|a, b| b.ccu.cmp(&a.ccu).then_with(|| a.name.cmp(&b.name)));
    Ok((games, sort_ids_seen))
}

/// Build the structured 8-dimension Taste Dossier JSON for a niche profile + benchmark games.
pub fn build_niche_dossier_json(profile: &NicheProfile, top_games: &[MarketGame]) -> Value {
    json!({
        "template_id": profile.template_id,
        "market_leaders": top_games.iter().map(MarketGame::to_json).collect::<Vec<_>>(),
        "gameplay_loop": profile.gameplay_loop,
        "ui_ux_style": profile.ui_ux_style,
        "animations_and_juice": profile.animations_and_juice,
        "model_and_world_3d_style": profile.model_and_world_3d_style,
        "audio_cadence": profile.audio_cadence,
        "monetization_and_retention": {
            "monetization_loops": profile.monetization_loops,
            "retention_hooks": profile.retention_hooks,
        },
        "craft_bar": profile.craft_bar,
        "anti_slop_rules": profile.anti_slop_guardrails,
        "automatic_twists": profile.automatic_twists,
        "quality_boosts": profile.quality_boosts,
    })
}

/// Research any live Roblox game by Place URL / Place ID / Universe ID, synthesize a full
/// 8-Dimension Production Taste Dossier + automatic twists + quality boosts, and persist
/// `<data_dir>/taste-dossiers/<universe_id>.json` and `TASTE-DOSSIER.md`.
///
/// Honors `OPENRO_ROBLOX_API_BASE` for offline/mock testing.
pub fn research_roblox_game(
    data_dir: &Path,
    place_or_universe_id: &str,
    niche_hint: Option<&str>,
) -> Result<Value, String> {
    let env_base = std::env::var("OPENRO_ROBLOX_API_BASE")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| s.starts_with("http://") || s.starts_with("https://"));
    research_roblox_game_with_base(
        data_dir,
        place_or_universe_id,
        niche_hint,
        env_base.as_deref(),
        Duration::from_secs(12),
    )
}

/// Explicit-base variant of `research_roblox_game` for thread-safe mock HTTP tests.
pub fn research_roblox_game_with_base(
    data_dir: &Path,
    place_or_universe_id: &str,
    niche_hint: Option<&str>,
    api_base_override: Option<&str>,
    timeout: Duration,
) -> Result<Value, String> {
    let (raw_id, is_place_url) = parse_roblox_id_input(place_or_universe_id)?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout.min(Duration::from_secs(6)))
        .timeout(timeout)
        .build();

    let (universes_base, games_base, thumbs_base) = match api_base_override {
        Some(base) => {
            let clean = base.trim_end_matches('/');
            (
                format!("{clean}/universes"),
                clean.to_string(),
                clean.to_string(),
            )
        }
        None => (
            "https://apis.roblox.com/universes".to_string(),
            "https://games.roblox.com".to_string(),
            "https://thumbnails.roblox.com".to_string(),
        ),
    };

    // 1. Resolve Place ID -> Universe ID if input is a Place URL or if direct games lookup misses.
    let resolve_place_to_universe = |place_id: u64| -> Option<u64> {
        let url = format!("{universes_base}/v1/places/{place_id}/universe");
        let resp = agent
            .get(&url)
            .set("Accept", "application/json")
            .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")))
            .call()
            .ok()?;
        let val: Value = resp.into_json().ok()?;
        val.get("universeId")
            .and_then(|v| v.as_u64())
            .filter(|id| *id > 0)
    };

    let fetch_game_data = |uid: u64| -> Result<Option<Value>, String> {
        let url = format!("{games_base}/v1/games?universeIds={uid}");
        let resp = agent
            .get(&url)
            .set("Accept", "application/json")
            .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")))
            .call()
            .map_err(|e| format!("Roblox games API request failed: {e}"))?;
        let val: Value = resp
            .into_json()
            .map_err(|e| format!("invalid JSON from Roblox games API: {e}"))?;
        Ok(val
            .get("data")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .cloned())
    };

    let (universe_id, game_obj) = if is_place_url {
        let uid = resolve_place_to_universe(raw_id).unwrap_or(raw_id);
        let obj = fetch_game_data(uid)?.ok_or_else(|| {
            format!("no Roblox game metadata found for place {raw_id} (universe {uid})")
        })?;
        (uid, obj)
    } else {
        match fetch_game_data(raw_id)? {
            Some(obj) => (raw_id, obj),
            None => {
                let uid = resolve_place_to_universe(raw_id).ok_or_else(|| {
                    format!("could not resolve Roblox game ID {raw_id} as Universe or Place")
                })?;
                let obj = fetch_game_data(uid)?
                    .ok_or_else(|| format!("no Roblox game metadata found for universe {uid}"))?;
                (uid, obj)
            }
        }
    };

    let name = game_obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled Roblox Experience")
        .trim()
        .to_string();
    let description = game_obj
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let root_place_id = game_obj
        .get("rootPlaceId")
        .and_then(|v| v.as_u64())
        .unwrap_or(if is_place_url { raw_id } else { 0 });
    let playing = game_obj
        .get("playing")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let visits = game_obj
        .get("visits")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let favorited_count = game_obj
        .get("favoritedCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let genre = game_obj
        .get("genre_l1")
        .or_else(|| game_obj.get("genre"))
        .and_then(|v| v.as_str())
        .unwrap_or("Simulation")
        .trim()
        .to_string();
    let creator_name = game_obj
        .pointer("/creator/name")
        .and_then(|v| v.as_str())
        .unwrap_or("Roblox Creator")
        .to_string();

    // 2. Best-effort votes fetch
    let (up_votes, down_votes) = {
        let votes_url = format!("{games_base}/v1/games/votes?universeIds={universe_id}");
        agent
            .get(&votes_url)
            .set("Accept", "application/json")
            .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")))
            .call()
            .ok()
            .and_then(|r| r.into_json::<Value>().ok())
            .and_then(|v| v.get("data")?.as_array()?.first().cloned())
            .map(|entry| {
                (
                    entry.get("upVotes").and_then(|v| v.as_u64()).unwrap_or(0),
                    entry.get("downVotes").and_then(|v| v.as_u64()).unwrap_or(0),
                )
            })
            .unwrap_or((0, 0))
    };
    let like_ratio_pct = compute_like_ratio(up_votes, down_votes);

    // 3. Best-effort game thumbnails fetch
    let thumbnails: Vec<String> = {
        let thumb_url = format!(
            "{thumbs_base}/v1/games/multiget/thumbnails?universeIds={universe_id}&countPerUniverse=5&defaults=true&size=768x432&format=Png&isCircular=false"
        );
        agent
            .get(&thumb_url)
            .set("Accept", "application/json")
            .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")))
            .call()
            .ok()
            .and_then(|r| r.into_json::<Value>().ok())
            .and_then(|v| {
                v.get("data")?
                    .as_array()?
                    .first()?
                    .get("thumbnails")?
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|t| t.get("imageUrl").and_then(|u| u.as_str()))
                            .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
                            .map(ToString::to_string)
                            .collect()
                    })
            })
            .unwrap_or_default()
    };

    // 4. Resolve niche profile (honor explicit hint if valid, else classify from title + genre)
    let niche_id = niche_hint
        .and_then(|h| normalize_niche_id(h).ok())
        .unwrap_or_else(|| classify_game_niche(&name, &genre));
    let profile = find_niche_profile(niche_id)
        .unwrap_or_else(|| find_niche_profile(DEFAULT_ACTIVE_NICHE).unwrap());

    let researched_game = MarketGame {
        name: name.clone(),
        universe_id,
        root_place_id,
        ccu: playing,
        up_votes,
        down_votes,
        like_ratio_pct,
        niche: profile.id.to_string(),
        sort_tags: vec!["researched-reference".to_string()],
        genre: genre.clone(),
        source: "live_roblox_game_api".to_string(),
        why_it_wins: format!(
            "Researched reference ({} playing, {} visits, {:.1}% likes)",
            format_ccu(playing),
            format_ccu(visits),
            like_ratio_pct
        ),
    };

    let mut leaders = vec![researched_game];
    for bg in baseline_games_for_niche(profile.id) {
        if bg.universe_id != universe_id && !bg.name.eq_ignore_ascii_case(&name) {
            leaders.push(bg);
        }
    }
    leaders.truncate(6);

    let dossier_json = build_niche_dossier_json(profile, &leaders);
    let markdown = render_taste_dossier_markdown(
        &name,
        universe_id,
        root_place_id,
        &creator_name,
        playing,
        visits,
        favorited_count,
        like_ratio_pct,
        &genre,
        &description,
        &thumbnails,
        profile,
    );

    // 5. Persist under `<data_dir>/taste-dossiers/<universe_id>.json` and `TASTE-DOSSIER.md`
    let dossiers_dir = data_dir.join("taste-dossiers");
    std::fs::create_dir_all(&dossiers_dir)
        .map_err(|e| format!("create taste-dossiers dir: {e}"))?;

    let json_path = dossiers_dir.join(format!("{universe_id}.json"));
    let md_path = dossiers_dir.join("TASTE-DOSSIER.md");
    let per_game_md_path = dossiers_dir.join(format!("{universe_id}-TASTE-DOSSIER.md"));

    let result_payload = json!({
        "ok": true,
        "researched_at_ms": now_ms(),
        "universe_id": universe_id,
        "root_place_id": root_place_id,
        "name": name,
        "creator": creator_name,
        "description": description,
        "playing": playing,
        "visits": visits,
        "favorited_count": favorited_count,
        "up_votes": up_votes,
        "down_votes": down_votes,
        "like_ratio_pct": like_ratio_pct,
        "genre": genre,
        "niche": profile.id,
        "niche_name": profile.name,
        "thumbnails": thumbnails,
        "craft_bar": profile.craft_bar,
        "anti_slop_rules": profile.anti_slop_guardrails,
        "automatic_twists": profile.automatic_twists,
        "quality_boosts": profile.quality_boosts,
        "pitch_prompt": profile.pitch_prompt,
        "taste_dossier": dossier_json,
        "json_path": json_path.to_string_lossy(),
        "markdown_path": md_path.to_string_lossy(),
        "markdown": markdown,
    });

    let json_text = serde_json::to_string_pretty(&result_payload)
        .map_err(|e| format!("serialize taste dossier json: {e}"))?;
    write_atomic(&json_path, json_text.as_bytes())?;
    write_atomic(&md_path, markdown.as_bytes())?;
    write_atomic(&per_game_md_path, markdown.as_bytes())?;

    Ok(result_payload)
}

#[allow(clippy::too_many_arguments)]
fn render_taste_dossier_markdown(
    game_name: &str,
    universe_id: u64,
    root_place_id: u64,
    creator: &str,
    playing: u64,
    visits: u64,
    favorited: u64,
    like_ratio_pct: f64,
    genre: &str,
    description: &str,
    thumbnails: &[String],
    profile: &NicheProfile,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Taste Dossier: {game_name}\n\n"));
    out.push_str(&format!(
        "- **Universe ID:** `{universe_id}` | **Root Place ID:** `{root_place_id}` | **Creator:** {creator}\n"
    ));
    out.push_str(&format!(
        "- **Live Metrics:** {} playing · {} visits · {} favorites · {:.1}% likes · Genre: `{genre}`\n",
        format_ccu(playing),
        format_ccu(visits),
        format_ccu(favorited),
        like_ratio_pct
    ));
    out.push_str(&format!(
        "- **Classified Niche:** {} (`{}`)\n\n",
        profile.name, profile.id
    ));
    if !description.is_empty() {
        let preview: String = description.chars().take(420).collect();
        out.push_str("## Reference Pitch Snapshot\n");
        out.push_str(&format!("> {}\n\n", preview.replace('\n', "\n> ")));
    }
    if !thumbnails.is_empty() {
        out.push_str("## Fetched Reference Thumbnails\n");
        for (i, u) in thumbnails.iter().enumerate() {
            out.push_str(&format!("{}. {u}\n", i + 1));
        }
        out.push('\n');
    }

    out.push_str("## Top-Trending Craft Bar\n");
    out.push_str(&format!("{}\n\n", profile.craft_bar));

    let sections: &[(&str, &[&str])] = &[
        ("1. Gameplay Loop", profile.gameplay_loop),
        ("2. UI & UX Style", profile.ui_ux_style),
        ("3. Animations & Juice", profile.animations_and_juice),
        ("4. 3D World & Model Style", profile.model_and_world_3d_style),
        ("5. Audio Cadence", profile.audio_cadence),
        ("6. Monetization Loops", profile.monetization_loops),
        ("7. D1/D7 Retention Hooks", profile.retention_hooks),
        ("8. Anti-Slop Rules (Non-Negotiable)", profile.anti_slop_guardrails),
        ("Automatic Gameplay Twists (Similar-but-Different)", profile.automatic_twists),
        ("Automatic UX & Juice Quality Boosts", profile.quality_boosts),
    ];
    for (title, items) in sections {
        out.push_str(&format!("## {title}\n"));
        for item in *items {
            out.push_str(&format!("- {item}\n"));
        }
        out.push('\n');
    }
    out
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("commit {}: {e}", path.display()))?;
    Ok(())
}

/// Persistent store managing the active niche, live refreshed Explore cache, and baseline fallback.
#[derive(Clone, Debug)]
pub struct MarketTasteStore {
    data_dir: PathBuf,
    cache_path: PathBuf,
    active_niche: String,
    live_games: Vec<MarketGame>,
    sorts_seen: Vec<String>,
    last_refreshed_ms: Option<u64>,
    last_error: Option<String>,
}

impl MarketTasteStore {
    /// Load cached live charts & active niche from `<data_dir>/market_taste_cache.json`.
    /// NEVER makes unsolicited network requests.
    pub fn open(data_dir: &Path) -> Self {
        let cache_path = data_dir.join("market_taste_cache.json");
        let mut store = MarketTasteStore {
            data_dir: data_dir.to_path_buf(),
            cache_path,
            active_niche: DEFAULT_ACTIVE_NICHE.to_string(),
            live_games: Vec::new(),
            sorts_seen: Vec::new(),
            last_refreshed_ms: None,
            last_error: None,
        };
        store.load();
        store
    }

    fn load(&mut self) {
        let Ok(raw) = std::fs::read_to_string(&self.cache_path) else {
            return;
        };
        let Ok(val) = serde_json::from_str::<Value>(&raw) else {
            return;
        };
        if let Some(niche) = val
            .get("active_niche")
            .and_then(|v| v.as_str())
            .and_then(|s| normalize_niche_id(s).ok())
        {
            self.active_niche = niche.to_string();
        }
        self.last_refreshed_ms = val.get("last_refreshed_ms").and_then(|v| v.as_u64());
        if let Some(arr) = val.get("sorts_seen").and_then(|v| v.as_array()) {
            self.sorts_seen = arr
                .iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect();
        }
        if let Some(arr) = val.get("live_games").and_then(|v| v.as_array()) {
            self.live_games = arr.iter().filter_map(MarketGame::from_json).collect();
        }
    }

    fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.cache_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create market taste cache dir: {e}"))?;
        }
        let payload = json!({
            "version": 1,
            "active_niche": self.active_niche,
            "last_refreshed_ms": self.last_refreshed_ms,
            "sorts_seen": self.sorts_seen,
            "live_games": self.live_games.iter().map(MarketGame::to_json).collect::<Vec<_>>(),
        });
        let text = serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("serialize market_taste_cache.json: {e}"))?;
        write_atomic(&self.cache_path, text.as_bytes())
    }

    pub fn active_niche(&self) -> &str {
        &self.active_niche
    }

    pub fn live_games_count(&self) -> usize {
        self.live_games.len()
    }

    pub fn last_refreshed_ms(&self) -> Option<u64> {
        self.last_refreshed_ms
    }

    pub fn api_url() -> String {
        std::env::var("OPENRO_MARKET_API_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
            .unwrap_or_else(|| DEFAULT_MARKET_API_URL.to_string())
    }

    /// Change the active niche (`POST /api/market-taste/niche`), persist, and return fresh snapshot.
    pub fn set_active_niche(&mut self, niche: &str) -> Result<Value, String> {
        let canonical = normalize_niche_id(niche)?;
        self.active_niche = canonical.to_string();
        self.save()?;
        Ok(self.snapshot_json(Some(canonical)))
    }

    /// Fetch live Roblox Explore charts via `ureq`, classify into the 8 niches, persist cache,
    /// and return updated snapshot (`POST /api/market-taste/refresh`).
    pub fn refresh_live(&mut self, timeout: Duration) -> Result<Value, String> {
        let url = Self::api_url();
        self.refresh_live_from_url(&url, timeout)
    }

    /// Fetch live Roblox Explore charts from an explicit URL (useful for thread-safe tests).
    pub fn refresh_live_from_url(&mut self, url: &str, timeout: Duration) -> Result<Value, String> {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeout.min(Duration::from_secs(6)))
            .timeout(timeout)
            .build();

        let resp = agent
            .get(url)
            .set("Accept", "application/json")
            .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")))
            .call()
            .map_err(|e| {
                let msg = format!("live Roblox Explore chart request failed: {e}");
                self.last_error = Some(msg.clone());
                msg
            })?;

        let body_val: Value = resp.into_json().map_err(|e| {
            let msg = format!("invalid JSON from Roblox Explore API: {e}");
            self.last_error = Some(msg.clone());
            msg
        })?;

        let (games, sorts_seen) = parse_explore_payload(&body_val)?;
        if games.is_empty() {
            let msg = "Roblox Explore response contained zero organic games".to_string();
            self.last_error = Some(msg.clone());
            return Err(msg);
        }

        self.live_games = games;
        self.sorts_seen = sorts_seen;
        self.last_refreshed_ms = Some(now_ms());
        self.last_error = None;
        self.save()?;
        Ok(self.snapshot_json(None))
    }

    /// Convenience method calling `research_roblox_game` against this store's `data_dir`.
    pub fn research_game(
        &self,
        place_or_universe_id: &str,
        niche_hint: Option<&str>,
    ) -> Result<Value, String> {
        research_roblox_game(&self.data_dir, place_or_universe_id, niche_hint)
    }

    /// Return ranked games for `niche_id` (or across ALL 8 genres when `niche_id == "top-trending"`):
    /// live Explore hits first, supplemented by verified baseline benchmarks when needed.
    pub fn games_for_niche(&self, niche_id: &str) -> Vec<MarketGame> {
        let mut out: Vec<MarketGame> = if niche_id == "top-trending" {
            self.live_games.clone()
        } else {
            self.live_games
                .iter()
                .filter(|g| g.niche == niche_id)
                .cloned()
                .collect()
        };
        out.sort_by(|a, b| b.ccu.cmp(&a.ccu).then_with(|| a.name.cmp(&b.name)));

        let min_entries = if niche_id == "top-trending" { 8 } else { 4 };
        if out.len() < min_entries {
            let baseline = if niche_id == "top-trending" {
                baseline_all_games()
            } else {
                baseline_games_for_niche(niche_id)
            };
            for bg in baseline {
                let dup = out.iter().any(|existing| {
                    (bg.universe_id > 0 && existing.universe_id == bg.universe_id)
                        || existing.name.eq_ignore_ascii_case(&bg.name)
                });
                if !dup {
                    out.push(bg);
                }
            }
            out.sort_by(|a, b| b.ccu.cmp(&a.ccu).then_with(|| a.name.cmp(&b.name)));
        }
        out.truncate(12);
        out
    }

    /// Build the full `GET /api/market-taste[?niche=...]` JSON payload with all 8 Taste Dossier dimensions.
    pub fn snapshot_json(&self, niche_override: Option<&str>) -> Value {
        let selected_id = niche_override
            .and_then(|s| normalize_niche_id(s).ok())
            .unwrap_or(&self.active_niche);
        let profile = find_niche_profile(selected_id)
            .unwrap_or_else(|| find_niche_profile(DEFAULT_ACTIVE_NICHE).unwrap());

        let top_games = self.games_for_niche(profile.id);
        let total_niche_ccu: u64 = top_games.iter().map(|g| g.ccu).sum();
        let source = if !self.live_games.is_empty() {
            "live_roblox_explore"
        } else {
            "baseline_nov_2026"
        };

        let niches_summary: Vec<Value> = std::iter::once(&TOP_TRENDING_PROFILE)
            .chain(NICHE_PROFILES.iter())
            .map(|p| {
                let ng = self.games_for_niche(p.id);
                let top_ccu = ng.first().map(|g| g.ccu).unwrap_or(0);
                let total_ccu: u64 = ng.iter().map(|g| g.ccu).sum();
                let live_count = ng
                    .iter()
                    .filter(|g| g.source == "live_roblox_explore")
                    .count();
                json!({
                    "id": p.id,
                    "template_id": p.template_id,
                    "name": p.name,
                    "tagline": p.tagline,
                    "craft_bar": p.craft_bar,
                    "game_count": ng.len(),
                    "live_count": live_count,
                    "top_ccu": top_ccu,
                    "total_ccu": total_ccu,
                    "pitch_prompt": p.pitch_prompt,
                    "active": p.id == self.active_niche,
                })
            })
            .collect();

        let taste_dossier = build_niche_dossier_json(profile, &top_games);
        let prompt_block = self.prompt_section_for(Some(profile.id));

        json!({
            "ok": true,
            "active_niche": self.active_niche,
            "selected_niche": profile.id,
            "source": source,
            "baseline_date": BASELINE_DATE,
            "last_refreshed_ms": self.last_refreshed_ms,
            "sorts_seen": self.sorts_seen,
            "api_url": Self::api_url(),
            "total_tracked_games": if self.live_games.is_empty() {
                baseline_all_games().len()
            } else {
                self.live_games.len()
            },
            "live_games_count": self.live_games.len(),
            "last_error": self.last_error,
            "niche": {
                "id": profile.id,
                "template_id": profile.template_id,
                "name": profile.name,
                "tagline": profile.tagline,
                "craft_bar": profile.craft_bar,
                "gameplay_loop": profile.gameplay_loop,
                "ui_ux_style": profile.ui_ux_style,
                "animations_and_juice": profile.animations_and_juice,
                "model_and_world_3d_style": profile.model_and_world_3d_style,
                "audio_cadence": profile.audio_cadence,
                "monetization_loops": profile.monetization_loops,
                "retention_hooks": profile.retention_hooks,
                "anti_slop_guardrails": profile.anti_slop_guardrails,
                "anti_slop_rules": profile.anti_slop_guardrails,
                "automatic_twists": profile.automatic_twists,
                "quality_boosts": profile.quality_boosts,
                "pitch_prompt": profile.pitch_prompt,
                "top_games": top_games.iter().map(MarketGame::to_json).collect::<Vec<_>>(),
                "total_niche_ccu": total_niche_ccu,
                "taste_dossier": taste_dossier,
            },
            "niches": niches_summary,
            "prompt_section": prompt_block,
        })
    }

    /// Generate the full 8-dimension Craft Bar + Taste Dossier prompt block for the active niche.
    pub fn prompt_section_for(&self, niche_override: Option<&str>) -> String {
        let selected_id = niche_override
            .and_then(|s| normalize_niche_id(s).ok())
            .unwrap_or(&self.active_niche);
        let profile = find_niche_profile(selected_id)
            .unwrap_or_else(|| find_niche_profile(DEFAULT_ACTIVE_NICHE).unwrap());
        let games = self.games_for_niche(profile.id);
        let source_label = if !self.live_games.is_empty() {
            "Live Roblox Explore Charts"
        } else {
            "Verified Roblox Chart Baseline (Nov 2026)"
        };
        format_prompt_section(profile, &games, source_label)
    }
}

/// Pure prompt section builder for any niche (uses baseline benchmarks when no store is open).
pub fn prompt_section(active_niche: Option<&str>) -> String {
    let id = active_niche
        .and_then(|s| normalize_niche_id(s).ok())
        .unwrap_or(DEFAULT_ACTIVE_NICHE);
    let profile = find_niche_profile(id)
        .unwrap_or_else(|| find_niche_profile(DEFAULT_ACTIVE_NICHE).unwrap());
    let games = if profile.id == "top-trending" {
        let mut all = baseline_all_games();
        all.sort_by(|a, b| b.ccu.cmp(&a.ccu).then_with(|| a.name.cmp(&b.name)));
        all.truncate(12);
        all
    } else {
        baseline_games_for_niche(profile.id)
    };
    format_prompt_section(profile, &games, "Verified Roblox Chart Baseline (Nov 2026)")
}

fn format_prompt_section(
    profile: &NicheProfile,
    games: &[MarketGame],
    source_label: &str,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Active Market Niche: **{}** (`{}`) — {}\n",
        profile.name, profile.id, profile.tagline
    ));
    out.push_str(&format!("Top-Trending Craft Bar: {}\n", profile.craft_bar));
    out.push_str(&format!("1. Market Leaders ({source_label}):\n"));
    for g in games.iter().take(5) {
        out.push_str(&format!(
            "- **{}** ({} CCU, {:.1}% likes) — {}\n",
            g.name,
            format_ccu(g.ccu),
            g.like_ratio_pct,
            g.why_it_wins
        ));
    }
    let dims: &[(&str, &[&str])] = &[
        ("2. Gameplay Loop", profile.gameplay_loop),
        ("3. UI & UX Style", profile.ui_ux_style),
        ("4. Animations & Juice", profile.animations_and_juice),
        ("5. 3D World & Model Style", profile.model_and_world_3d_style),
        ("6. Audio Cadence", profile.audio_cadence),
        ("7a. Monetization Loops", profile.monetization_loops),
        ("7b. D1/D7 Retention Hooks", profile.retention_hooks),
        ("8. Anti-Slop Taste Guardrails", profile.anti_slop_guardrails),
        (
            "Mandatory Automatic Twists (Similar-but-Different)",
            profile.automatic_twists,
        ),
        (
            "Mandatory UX & Juice Quality Boosts",
            profile.quality_boosts,
        ),
    ];
    for (heading, lines) in dims {
        out.push_str(&format!("{heading}:\n"));
        for line in *lines {
            out.push_str(&format!("- {line}\n"));
        }
    }
    out
}

fn format_ccu(ccu: u64) -> String {
    if ccu >= 1_000_000 {
        format!("{:.2}M", ccu as f64 / 1_000_000.0)
    } else if ccu >= 1_000 {
        format!("{}k", (ccu + 500) / 1_000)
    } else {
        ccu.to_string()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Embedded Deep Research Baseline (Nov 2026 Live Roblox Charts)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn baseline_game(
    name: &str,
    universe_id: u64,
    root_place_id: u64,
    ccu: u64,
    like_ratio_pct: f64,
    niche: &str,
    genre: &str,
    why_it_wins: &str,
) -> MarketGame {
    let up = ((ccu.max(1000) * 4) as f64 * (like_ratio_pct / 100.0)) as u64;
    let down = ((ccu.max(1000) * 4) as f64 * ((100.0 - like_ratio_pct) / 100.0)) as u64;
    MarketGame {
        name: name.to_string(),
        universe_id,
        root_place_id,
        ccu,
        up_votes: up,
        down_votes: down,
        like_ratio_pct,
        niche: niche.to_string(),
        sort_tags: vec!["top-playing-now".to_string(), "top-trending".to_string()],
        genre: genre.to_string(),
        source: "baseline_nov_2026".to_string(),
        why_it_wins: why_it_wins.to_string(),
    }
}

fn baseline_all_games() -> Vec<MarketGame> {
    vec![
        // 1. anime-rng-cards
        baseline_game(
            "Anime Card Battles: Rebirth",
            10_092_619_788,
            130_253_630_950_453,
            68_000,
            96.9,
            "anime-rng-cards",
            "Simulation",
            "Sunlit shrine roll spectacle + dynamic weather luck multipliers + 4v4 strategic anime card clashes",
        ),
        baseline_game(
            "Fisch",
            5_750_914_919,
            0,
            91_000,
            92.1,
            "anime-rng-cards",
            "Simulation",
            "Tactile RNG rarity hunts paired with weather totems, mutation index, and social flexing",
        ),
        baseline_game(
            "Fish It!",
            6_701_277_882,
            0,
            69_000,
            91.8,
            "anime-rng-cards",
            "Simulation",
            "Fast 5-second roll feedback loop with potion crafting and mythic drop broadcasts",
        ),
        baseline_game(
            "Sol's RNG",
            5_361_032_378,
            0,
            48_000,
            89.6,
            "anime-rng-cards",
            "Simulation",
            "High-drama aura reveal cutscenes, biome rarity boosts, and luck-stacking gauntlets",
        ),
        // 2. viral-sim-tycoon
        baseline_game(
            "Steal An Egg",
            10_563_114_921,
            107_778_070_777_162,
            963_000,
            91.3,
            "viral-sim-tycoon",
            "Simulation",
            "Instant hilarious core verb + base defense steals + escalating rarity economy",
        ),
        baseline_game(
            "Throw Food For Animals",
            10_688_120_401,
            0,
            361_000,
            92.4,
            "viral-sim-tycoon",
            "Simulation",
            "Physical slapstick progression with rebirth multipliers and viral clip moments",
        ),
        baseline_game(
            "Break and Steal an Egg",
            10_612_440_811,
            0,
            123_000,
            89.9,
            "viral-sim-tycoon",
            "Simulation",
            "High-tempo raid/upgrade tycoon loop with immediate 10-second payout",
        ),
        baseline_game(
            "Steal a Brainrot",
            7_709_344_486,
            0,
            63_500,
            88.7,
            "viral-sim-tycoon",
            "Simulation",
            "Memeable collection bases, timed server events, and competitive lock timers",
        ),
        // 3. physics-crash-ragdoll
        baseline_game(
            "Hyper Realistic Ragdoll",
            10_773_618_503,
            101_277_003_916_102,
            149_000,
            91.3,
            "physics-crash-ragdoll",
            "Simulation",
            "Ultra-juicy impulse physics playground that creates organic Shorts/TikTok clips",
        ),
        baseline_game(
            "Ragdoll Soccer",
            9_441_220_310,
            0,
            67_800,
            90.4,
            "physics-crash-ragdoll",
            "Sports & Racing",
            "Chaotic shared-physics party matches with instant rematches and cosmetic trails",
        ),
        baseline_game(
            "BloxNG Crash Physics",
            10_402_881_224,
            0,
            56_000,
            91.0,
            "physics-crash-ragdoll",
            "Sports & Racing",
            "Destructible vehicle crumple zones, stunt ramps, and slow-mo replay cameras",
        ),
        // 4. survival-expedition
        baseline_game(
            "99 Nights in the Forest",
            7_326_934_954,
            0,
            253_000,
            92.8,
            "survival-expedition",
            "Survival",
            "Escalating night-by-night co-op tension, campfire upgrades, and clear extraction goal",
        ),
        baseline_game(
            "100 Miles Down the River",
            10_511_902_112,
            0,
            72_700,
            91.6,
            "survival-expedition",
            "Survival",
            "Road-trip raft expedition with landmark biomes, shared fuel/loot economy, and replayable runs",
        ),
        baseline_game(
            "Dead Rails",
            7_018_190_066,
            0,
            54_000,
            93.1,
            "survival-expedition",
            "Survival",
            "Armored train co-op progression, class loadouts, and milestone distance checkpoints",
        ),
        // 5. horror-coop-mascot
        baseline_game(
            "Dandy's World",
            5_569_032_992,
            16_116_270_224,
            206_200,
            88.8,
            "horror-coop-mascot",
            "Survival",
            "Iconic collectible mascot cast + co-op floor objectives + unlockable character perks",
        ),
        baseline_game(
            "Murder Mystery 2",
            66_654_135,
            0,
            186_800,
            90.9,
            "horror-coop-mascot",
            "Survival",
            "Tight 3-minute social deduction rounds paired with tradable weapon skin economy",
        ),
        baseline_game(
            "Violence District",
            8_820_119_401,
            0,
            62_700,
            89.2,
            "horror-coop-mascot",
            "Survival",
            "Asymmetrical survivor-vs-hunter tension with distinct map callouts and seasonal drops",
        ),
        baseline_game(
            "Forsaken",
            6_331_902_150,
            0,
            59_500,
            91.7,
            "horror-coop-mascot",
            "Survival",
            "Polished asymmetrical chase kit design, stamina mindgames, and character skins",
        ),
        // 6. tactical-arena-pvp
        baseline_game(
            "RIVALS",
            6_035_872_082,
            0,
            194_200,
            93.6,
            "tactical-arena-pvp",
            "Shooter",
            "Crisp 1v1/2v2/5v5 matchmaking in <5s, responsive movement, and mastery weapon wraps",
        ),
        baseline_game(
            "Volleyball Legends",
            6_931_042_565,
            0,
            60_600,
            92.5,
            "tactical-arena-pvp",
            "Sports & Racing",
            "High-skill anime sports timing mechanics + spin styles + ranked division badges",
        ),
        baseline_game(
            "STAND OUT",
            9_912_441_772,
            0,
            35_000,
            90.8,
            "tactical-arena-pvp",
            "Action",
            "Readable arena combat abilities, rapid round cadence, and leaderboard glory",
        ),
        // 7. anime-action-rpg-td
        baseline_game(
            "Blox Fruits",
            994_732_206,
            0,
            358_800,
            90.2,
            "anime-action-rpg-td",
            "RPG",
            "Deep power-fruit mastery progression, sea exploration, raid bosses, and trading hubs",
        ),
        baseline_game(
            "Jujutsu Shenanigans",
            3_508_322_461,
            0,
            105_400,
            93.9,
            "anime-action-rpg-td",
            "Action",
            "Destructible arenas, fluid combo awakenings, and hilarious ragdoll finishers",
        ),
        baseline_game(
            "Anime Adventures",
            3_183_403_065,
            0,
            91_900,
            92.3,
            "anime-action-rpg-td",
            "Strategy",
            "Summonable hero squads, trait rerolls, infinite wave raids, and seasonal banners",
        ),
        baseline_game(
            "Anime Crusaders X",
            9_218_554_102,
            0,
            78_200,
            91.5,
            "anime-action-rpg-td",
            "Strategy",
            "Co-op wave defense with evolution quests, mythic banner pity, and guild challenges",
        ),
        // 8. social-rp-fashion
        baseline_game(
            "Brookhaven RP",
            1_686_885_941,
            0,
            564_000,
            87.9,
            "social-rp-fashion",
            "Roleplay & Avatar Sim",
            "Zero-friction town sandbox, instant house/vehicle spawns, and secret lore triggers",
        ),
        baseline_game(
            "Adopt Me!",
            383_310_974,
            0,
            331_500,
            85.4,
            "social-rp-fashion",
            "Roleplay & Avatar Sim",
            "Pet raising + Neon/Mega crafting goals + regular seasonal egg events",
        ),
        baseline_game(
            "Dress To Impress",
            5_203_828_273,
            0,
            79_200,
            91.4,
            "social-rp-fashion",
            "Roleplay & Avatar Sim",
            "Timed 5-minute runway themes, peer voting drama, and VIP accessory rooms",
        ),
        baseline_game(
            "Catalog Avatar Creator",
            2_597_113_912,
            0,
            77_700,
            94.1,
            "social-rp-fashion",
            "Shopping",
            "Try-before-you-buy UGC outfits with 40% affiliate marketplace commission payouts",
        ),
    ]
}

fn baseline_games_for_niche(niche_id: &str) -> Vec<MarketGame> {
    baseline_all_games()
        .into_iter()
        .filter(|g| g.niche == niche_id)
        .collect()
}

static TOP_TRENDING_PROFILE: NicheProfile = NicheProfile {
    id: "top-trending",
    template_id: None,
    name: "Top Trending (All 8 Genres)",
    tagline: "Cross-genre Roblox chart synthesis covering live top-trending, up-and-coming, and top-playing-now hits.",
    craft_bar: "Ship top-trending Roblox quality across ANY genre the user asks for: cohesive sunlit/stylized 3D architecture with clear landmarks, Blender-silhouetted hero props, non-overlapping tactile HUDs, crisp TweenService juice & sound cadence, 100% server-authoritative economy/combat, and 5-second first-touch joy.",
    gameplay_loop: &[
        "5-to-10-second First-Touch Hook: deliver an immediate, tactile core action right from spawn with zero menu wall",
        "Escalating Core Loop: clear action -> instant visual/audio reward -> meaningful upgrade/unlock -> social showcase",
        "Timed World Beats: 10-to-15-minute server-wide events (weather shifts, meteor drops, raid portals, chaos rounds) creating shared peaks",
        "Deep Meta Loop: persistent DataStore progression, leaderstats, collection milestones, and replayable co-op or PvP modes",
    ],
    ui_ux_style: &[
        "Clean, non-overlapping HUD hierarchy with high contrast, crisp iconography, and 80%+ unobstructed 3D viewport",
        "Auto-hide or dim bottom action bars whenever a modal or dramatic reward reveal opens",
        "Mobile-safe 44px+ touch targets and concise readable stats across desktop, tablet, and phone screens",
    ],
    animations_and_juice: &[
        "TweenService spring/Quint easing on every button, reward pop, door, and world upgrade",
        "Micro hitstop, subtle camera punch, floating feedback numbers, and translucent particle bursts on key actions",
        "Smooth dynamic Lighting & Atmosphere transitions when switching time of day or triggering server events",
    ],
    model_and_world_3d_style: &[
        "Cohesive, inviting sunlit worlds by default (`ClockTime = 13.5`, crisp sky & balanced Atmosphere) — NEVER flat black/gray baseplates or opaque neon slabs in daylight",
        "Layered elevation, distinct visual landmarks, stylized color palettes, and translucent VFX (`Transparency = 0.65..0.8`) so 3D architecture shines",
        "Clean hero prop silhouettes (Blender `.obj` or multi-part assemblies) grounded firmly with zero Z-fighting or floating seams",
    ],
    audio_cadence: &[
        "Crisp UI confirmation clicks (`ui_click.wav`) paired with distinct reward chimes (`level_up.wav` / `summon_roll.wav`)",
        "Punchy action/impact SFX (`battle_crit.wav`) synced tightly to visual hit frames and animations",
        "Grand fanfare stings (`mythic_fanfare.wav`) reserved for rare milestones so highs feel truly earned",
    ],
    monetization_loops: &[
        "Fair Speed/Multiplier/VIP Quality-of-Life Gamepasses that accelerate fun without walling core gameplay",
        "Social-Flex Server Boost Dev Products (global luck/weather/event triggers rewarding the whole lobby)",
        "Cosmetic Skins, Trails, Emotes, Titles & Private Friend Server passes",
    ],
    retention_hooks: &[
        "Daily login streak + visible pity/milestone progress bars giving a concrete reason to return tomorrow",
        "Timed server-wide events and rotating shops/challenges creating natural session rhythms",
        "Co-op friend bonuses, leaderstats prestige badges, and social showcase hubs",
    ],
    anti_slop_guardrails: &[
        "Never ship a bare gray baseplate, dark neon-box room, or broken overlapping HUD",
        "Never trust client RemoteEvent amounts, damage, or positions — validate all gameplay strictly on the server",
        "Always wrap DataStoreService in pcall + UpdateAsync + game:BindToClose so player progress is never lost",
        "Every build must pass OpenRO's Static Luau Prover and Playtest with zero runtime or syntax errors",
    ],
    automatic_twists: &[
        "ACTIVE TIMING BURST: weave a crisp skill-timing or combo window into the core loop so active play beats passive AFK",
        "DYNAMIC SERVER WORLD EVENTS: pair core progression with visible sky/lighting/physics shifts that transform the hub every 10 minutes",
        "SOCIAL SHOWCASE IDENTITY: display each player's rarest unlock, streak crown, or custom emblem visibly in the 3D hub",
    ],
    quality_boosts: &[
        "Cohesive sunlit landmark hub with clear spawn sightlines guiding players into the core loop within 5 seconds",
        "Non-overlapping modal/HUD layering + TweenService spring animations + matched procedural .wav SFX",
        "Zero-slop server-authoritative Luau architecture verified end-to-end with DataStore persistence",
    ],
    pitch_prompt: "Build a top-trending, high-retention Roblox experience tailored to live player demand: a cohesive sunlit stylized 3D hub with strong landmarks and Blender-grade silhouettes, a juicy 5-second first-touch core loop with an Active Timing Burst twist, timed Dynamic Server World Events shifting lighting & rewards, crisp non-overlapping HUD with TweenService juice and procedural .wav SFX, fair VIP/cosmetic monetization hooks, and server-authoritative DataStore persistence.",
};

static NICHE_PROFILES: [NicheProfile; 8] = [
    NicheProfile {
        id: "anime-rng-cards",
        template_id: Some("anime-card-battles-apex"),
        name: "Anime RNG & Card Battlers",
        tagline: "High-clarity anime gacha collection, progression depth, and responsive card battles.",
        craft_bar: "Match top-charting anime RNG and card-game craft: cohesive stylized lighting and readable hub landmarks, 100% original cel-shaded character art (original silhouettes, costumes, lore, and names), non-overlapping responsive HUDs, snappy TweenService reveal & combat feedback, and server-authoritative odds and saves.",
        gameplay_loop: &[
            "Fast first-touch core loop: clear entry action within seconds -> unmistakable rarity reveal -> immediate equip/upgrade feedback",
            "Multi-mode progression: pair core rolling/collection with meaningful combat, world progression, or challenge modes defined by the project GDD",
            "Transparent luck and progression modifiers: display active multipliers, odds tables, and pity progress cleanly from server state",
            "Long-term collection goals: index milestones, ascension/duplicate sinks, and rebirth or tier scaling tuned to the project economy",
        ],
        ui_ux_style: &[
            "Clean high-contrast rarity hierarchy using distinct colour, label, and shape/glyph (never colour alone)",
            "Non-overlapping HUD layering: dim or hide navigation docks while reveal, modal, or confirmation overlays are open",
            "Keep top-left Roblox menu bounds unobstructed and provide Settings-only Light/Dark theme and reduced-motion/flash toggles",
            "44px+ mobile-safe tap targets with responsive UIScale/layout across phone, tablet, and desktop viewports",
        ],
        animations_and_juice: &[
            "TweenService spring/easing curves for card reveals, button presses, and modal transitions (respecting Flash/Reduced-Motion Settings)",
            "Crisp combat/action feedback: anticipation, impact hitstop, floating numbers, and subtle camera feedback within mobile budgets",
            "Clear affordances for unaffordable actions: explain the exact missing requirement and route the player directly to earn it",
            "Zero visual clutter: keep particle emitters and lighting budgets disciplined on mobile devices",
        ],
        model_and_world_3d_style: &[
            "Cohesive stylized world architecture and lighting driven by the project's GDD and reference art — avoid empty dark slabs",
            "Custom UV-unwrapped, AO-baked Blender meshes (<= 20,000 tris/mesh) wired through manifest stud dimensions and AssetIds",
            "Respect PartType.Cylinder local-X orientation (rotate Z+90deg for upright pillars) and StreamingEnabled mobile budgets",
            "Keep interactive hubs navigable with clear sightlines from spawn to core stations and landmarks",
        ],
        audio_cadence: &[
            "Distinct rarity-scaled reveal cues and clean UI click/confirm feedback authored into the project's assets/audio/ folder",
            "Punchy, non-fatiguing combat and reward impacts with pooled Sound cleanup via Debris",
            "Volume and mute toggles accessible in Settings without interrupting gameplay",
            "Bind uploaded audio through AssetIds.luau using proper Content/url asset references",
        ],
        monetization_loops: &[
            "Server-authoritative Game Passes and Developer Products aligned with an owner-approved price sheet (never auto-published unapproved)",
            "Transparent odds tables and pity guarantees with fair free-to-play earn paths alongside convenience boosts",
            "Social server-wide boosts or cosmetic flex unlocks that reward the whole lobby without hard paywalls",
            "Clear confirmation dialogs and receipt idempotency (`ProcessReceipt` / `UserOwnsGamePassAsync` cached safely on server)",
        ],
        retention_hooks: &[
            "Visible pity counters, daily login streaks, and clear 'Next Goal' strip guiding each session's immediate objective",
            "Timed server events, rotating banners, and leaderboard ladders synced cleanly on the server",
            "Collection index milestones and meaningful duplicate conversion so every roll advances the account",
            "Reliable `pcall` + `UpdateAsync` + `BindToClose` DataStore persistence with schema versioning",
        ],
        anti_slop_guardrails: &[
            "Never inject generic placeholder mechanics that contradict the user's brief or project GDD",
            "Never overlap HUD buttons over modals or Roblox's top-left CoreGui controls",
            "Keep RNG odds, pity counters, inventory, currencies, and combat outcomes 100% server-authoritative",
            "Verify every script with static Luau/Selene checks and live Playtest Runner with 0 console errors",
        ],
        automatic_twists: &[
            "Server-authoritative progression telemetry & transparent odds/pity UI so players always trust every roll",
            "Mobile-first responsive layout verification + Settings-only Light/Dark and reduced-motion accessibility",
            "End-to-end automated asset pipeline: manifest-driven 3D sizing, AssetIds.luau binding, and zero-error playtest checks",
        ],
        quality_boosts: &[
            "Cohesive stylized hub landmarks with manifest-backed meshes and disciplined mobile emitter/light budgets",
            "Clean modal HUD occlusion and clear 'Next Goal' guidance from the first 5 seconds",
            "Complete server-authoritative DataStore persistence, RemoteEvent validation, and zero console error verification",
        ],
        pitch_prompt: "Build an original Roblox Anime RNG & Card game tailored strictly to the user's brief and GDD: server-authoritative RNG/pity/economy/DataStore systems, responsive non-overlapping UI with Settings-only Light/Dark & flash toggles, custom UV-textured Blender meshes, synthesized project audio, reference-guided anime character art bound via AssetIds.luau, and verified Playtest Runner checks.",
    },
    NicheProfile {
        id: "viral-sim-tycoon",
        template_id: Some("viral-sim-tycoon-template"),
        name: "Viral Simulators & Steal/Collect Tycoons",
        tagline: "Instant hilarious core verbs, escalating base economies, and high-tempo server events.",
        craft_bar: "Match top-charting viral sim polish: saturated playful daylight palettes, readable plot landmarks, hilarious physical animations, and zero-friction 10-second first purchase.",
        gameplay_loop: &[
            "10-second Hook: Grab/spawn starter collectible -> carry to glowing base pedestal -> hear instant cash chime",
            "Escalating Risk-Reward: Upgrade base security lock timers while scouting rival bases for rare golden/mythic drops",
            "Server Event Spikes: Rush central plaza during timed Golden Meteor or Mystery Egg drops",
            "Prestige Loop: Rebirth to multiply base income and unlock higher-tier sanctuary wings",
        ],
        ui_ux_style: &[
            "Chunky high-contrast cartoon typography with crisp dark stroke, big cash/rebirth pills, and directional home beacon",
            "Minimal 3-button side rail (Shop, Index, Rebirth) keeping 80%+ of screen open for chaotic world action",
            "Overhead BillboardGuis displaying base owner, lock timer countdown, and rare item value at a glance",
        ],
        animations_and_juice: &[
            "Squash-and-stretch TweenService bounce on every dropped egg/pet and purchase pad activation",
            "Coin burst magnet trails flying cleanly into the player's HUD wallet counter with pitch-escalating chimes",
            "Dramatic screen-edge pulse + siren icon when an intruder enters your unlocked plot",
        ],
        model_and_world_3d_style: &[
            "Bright, cheerful sunlit meadow/island ring with distinct color-coded player plots facing a vibrant central event plaza",
            "Stylized rounded props, clear glowing lock-shield domes (`Transparency = 0.75`), and zero gray raw-box droppers",
            "Landmark verticality (giant mascot statue or golden egg fountain in center) so navigation is instant",
        ],
        audio_cadence: &[
            "Pitch-escalating coin/cash pickup pings (`level_up.wav`) as streak combos rise",
            "Comedic pop + boing cue on grab/throw actions and resonant alarm chime on base lock expiry",
            "Triumphant horn/bell chord on Rebirth and server-wide Golden Meteor spawn",
        ],
        monetization_loops: &[
            "2x Cash / 2x Luck / Auto-Collect Gamepasses + Starter Bundle with immediate visual upgrade",
            "Rebirth Multiplier Boosts & Extra Base Lock/Shield Duration Dev Products",
            "Server-Wide Golden/Mythic Spawn Event Trigger (social flex purchase rewarding the whole lobby)",
            "Private VIP Servers for uninterrupted farming or friend-only raids",
        ],
        retention_hooks: &[
            "First purchase & dopamine pop within 10 seconds of spawning (zero sluggish tutorial walls)",
            "Escaping/stealing tension timers + visible rarity tiers displayed proudly outside each player's base",
            "Hourly Server Mutation/Meteor Events creating shared FOMO spikes and D1/D7 return rituals",
            "Rebirth milestones unlocking new zones, pedestals, and persistent leaderstats prestige badges",
        ],
        anti_slop_guardrails: &[
            "Never ship raw gray dropper rectangles on a flat baseplate: craft expressive stylized props and bright terrain",
            "Validate every collect/steal/purchase distance and cooldown on the server to prevent teleport exploits",
            "Include clear directional beams/arrows guiding new players to their own plot immediately on spawn",
            "Persist bases, rebirths, and unlocks safely with pcall + UpdateAsync + BindToClose",
        ],
        automatic_twists: &[
            "CHAOS BOUNTY BOARD: leading player's rarest item gets a glowing crown & 2x steal bounty for 90s while granting them extra income if defended",
            "COMBO CARRY CHAIN: delivering 3 collectibles back-to-back without dropping triggers a temporary Speed + Golden Mutation aura",
            "CO-OP VAULT MERGE: two friends can temporarily link neighboring base shields to double server-event meteor drop rates",
        ],
        quality_boosts: &[
            "10-second onboarding path beam from spawn straight to your first free pedestal + starter collectible",
            "Translucent hex-pattern shield domes with clear 3D countdown badges visible across the plaza",
            "Juice-packed purchase pads with spring scale bounce, confetti burst, and escalating harmonic pickup SFX",
        ],
        pitch_prompt: "Build a high-retention Viral Collect & Raid Tycoon inspired by Steal An Egg and Throw Food For Animals: a sunlit colorful island ring with stylized personal sanctuary plots, 10-second first upgrade loop, translucent base security domes, Chaos Bounty Board twist, Combo Carry Chain aura, timed Golden Meteor plaza events, Rebirth multiplier progression, and server-authoritative DataStore saves.",
    },
    NicheProfile {
        id: "physics-crash-ragdoll",
        template_id: None,
        name: "Physics, Crash & Ragdoll Playgrounds",
        tagline: "Clip-worthy kinetic chaos, destructible ramps, and hilarious shared-physics party rounds.",
        craft_bar: "Match top-trending physics sandbox craft: crisp sunlit stunt geometry, tactile impact camera punch, instant zero-latency respawn, and hilarious clip-ready ragdoll chains.",
        gameplay_loop: &[
            "3-second Hook: Step onto mega launch ramp or vehicle pad -> smash through destructible glass/block tower -> score massive Impact Combo",
            "Toy Sandbox: Unlock gravity coils, grapple launchers, and stunt vehicles with earned Impact Points",
            "Rotating 60s Chaos Modes: Low-Gravity Bounce, Giant Boulder Avalanche, and Ragdoll Soccer scrimmages",
            "Clip Mastery: Chase highest Impact Score & longest airborne flight distance on the lobby Hall of Fame",
        ],
        ui_ux_style: &[
            "Minimal kinetic HUD: crisp Impact Score counter, combo multiplier badge, and clean bottom-right R-to-Reset + Toy Wheel",
            "Color-coded stunt zone markers (Boost Cyan, Breakable Amber, Hazard Crimson) readable at high velocity",
            "Zero cluttered popups blocking the action — award unlocks via sleek slide-in top toast banners",
        ],
        animations_and_juice: &[
            "Brief slow-mo micro-dip + radial speed lines on huge multi-piece tower crashes",
            "Spring-loaded bounce pad compression, sparks on metal scrapes, and dynamic floating impact numbers",
            "Smooth FOV widen on turbo boosts + camera shake scaled gently to impact velocity",
        ],
        model_and_world_3d_style: &[
            "Bright sunlit sky-park stunt arena with crisp sky dome, terraced launch ramps, glass shatter walls, and colorful obstacle rings",
            "Clean high-contrast material hierarchy (SmoothPlastic stunt tracks, Neon boost strips, Glass breakables, Metal trusses)",
            "Auto-regenerating destructible towers that snap back smoothly every 20 seconds so the playground stays fresh",
        ],
        audio_cadence: &[
            "Punchy kinetic crunch + metallic clang (`battle_crit.wav`) scaled by collision speed",
            "Ascending whoosh on jump pads and snappy spring boing on ragdoll bounces",
            "Airhorn/fanfare chord when a player breaks the server's top stunt distance record",
        ],
        monetization_loops: &[
            "VIP Physics Toy Pack (Grapple Hook, Gravity Coil, Slap Glove, Turbo Booster)",
            "Custom Vehicle Skins, Impact Particle Trails & Slow-Mo Replay FX Gamepasses",
            "Server-Wide Chaos Modifier Triggers (Low Gravity, Giant Boulders, Bumper Rain)",
            "Private Sandbox Servers with custom ramp & obstacle spawner controls",
        ],
        retention_hooks: &[
            "Instant physical comedy within 3 seconds of spawning (mega ramp, launch pads, destructible towers)",
            "Distance/Impact Score leaderstats & Trick Combo multipliers rewarding wild stunts",
            "60-second rotating Chaos Arena challenges (Survive the Crumble, Ragdoll Soccer, Mega Jump)",
            "Unlockable stunt vehicles, ragdoll emotes, and weekly top-impact hall of fame statues",
        ],
        anti_slop_guardrails: &[
            "Keep physics smooth and server-safe: cap active spawned vehicles/props per player with automatic cleanup",
            "Never let client remotes arbitrarily fling other players without server proximity & cooldown checks",
            "Design landmark verticality (towering launch ramps, glass shatter walls, neon booster rings) for instant readability",
            "Provide instant R-to-reset respawn so players never get stuck upside-down",
        ],
        automatic_twists: &[
            "STUNT CHAIN MULTIPLIER: chaining ramp -> hoop -> breakable wall within 6 seconds multiplies points up to 5x with blazing neon trail",
            "AUTO-REBUILDING DOMINO TOWERS: demolished glass/block skyscrapers reconstruct in a snappy wave animation every 20s",
            "LIVE CHAOS ROULETTE: every 60s the arena smoothly shifts physics modifiers (Moon Gravity, Giant Pinballs, Hyper Wind)",
        ],
        quality_boosts: &[
            "Sunlit sky-park vertical stunt architecture with clear neon sightlines and zero flat gray emptiness",
            "Instant 1-key R-to-reset + auto vehicle despawn preventing laggy physics pileups",
            "Dynamic FOV boost, floating stunt combo numbers, and crisp impact crunch SFX",
        ],
        pitch_prompt: "Build a clip-worthy Physics Crash & Ragdoll Playground inspired by Hyper Realistic Ragdoll and BloxNG Crash Physics: a sunlit vertical stunt sky-park with mega launch ramps, auto-rebuilding destructible towers, Stunt Chain Multiplier rings, per-player vehicle spawners with cleanup limits, 60s Live Chaos Roulette modifiers, instant R-to-reset, impact score leaderstats, and VIP Physics Toy Pack hooks.",
    },
    NicheProfile {
        id: "survival-expedition",
        template_id: None,
        name: "Co-op Survival & Road-Trip Expeditions",
        tagline: "Escalating co-op journeys (Forest Nights / River Rafts / Armored Trains) with clear extraction milestones.",
        craft_bar: "Match top-trending expedition craft: rich atmospheric day-to-night contrast, tactile lit camp/vehicle refuge, readable landmark horizons, and tight squad synergy.",
        gameplay_loop: &[
            "Daytime Scavenge: Explore roadside landmarks/islands for fuel, timber, food, and rare relics",
            "Upgrade Refuge: Refuel the campfire/raft/train and craft defenses before sunset warning horn",
            "Nighttime Defense: Hold off escalating nocturnal threats using teamwork, lanterns, and crafted traps",
            "Milestone Push: Advance through Mile/Night checkpoints toward extraction while earning permanent meta-currency",
        ],
        ui_ux_style: &[
            "Warm diegetic expedition HUD: sleek top Day/Night sundial + Mile/Night progress tracker + compact 4-slot hotbar",
            "Clear fuel/campfire gauge with amber low-fuel warning pulse so the whole squad shares urgency",
            "Clean party status pills showing teammate health and down/revive beacon direction",
        ],
        animations_and_juice: &[
            "Smooth Lighting ClockTime + Atmosphere dusk transition with warm flickering campfire PointLight bloom",
            "Tactile tool swing, wood-chop particle chips, and raft/train bobbing sway on water/rails",
            "Milestone banner slide-in + lantern flare pulse when reaching every 10th Mile or Night",
        ],
        model_and_world_3d_style: &[
            "Crisp sunlit golden-hour wilderness by day (pine/sakura ridges, winding turquoise river, wooden outpost docks) shifting to moody moonlit fog strictly at night",
            "Distinct silhouetted landmarks (watchtowers, ruined bridges, supply cabins) spaced along clear sightlines",
            "Never spawn players in pitch-black fog with zero guidance — start beside a glowing safe campfire & directional compass",
        ],
        audio_cadence: &[
            "Warm acoustic chime at sunrise + low distant horn cue 30s before dusk so players regroup naturally",
            "Crisp resource pickup thuds, crackling fire proximity warmth, and urgent pulse during night waves",
            "Triumphant milestone chord (`level_up.wav`) when unlocking a new biome checkpoint",
        ],
        monetization_loops: &[
            "Starter Class Loadouts (Medic, Engineer, Scout, Pyromancer) unlockable via currency OR fast-track gamepass",
            "Team Revive Token & Emergency Supply Drop Dev Products (clutch co-op rescues)",
            "Custom Raft/Train Chassis Skins, Lantern Trails & Campfire Cosmetics",
            "Private Expedition Lobbies so friend squads launch together cleanly",
        ],
        retention_hooks: &[
            "Clear distance/night progression counter (Night 1..99 or Mile 0..100) giving constant forward pull",
            "Daytime scavenging vs. Nighttime defense rhythm with dynamic Lighting/Atmosphere weather shifts",
            "Meta-progression currency earned every run to unlock permanent perks and harder biomes",
            "Squad synergy roles (driver/repairer/lookout) driving high Fun-with-Friends organic invites",
        ],
        anti_slop_guardrails: &[
            "Never spawn players into empty fog with zero objective: place a lit starting camp/vehicle + clear milestone compass",
            "Synchronize fuel, health, inventory, and day/night threat waves strictly on the server",
            "Ensure solo players scale cleanly alongside 4-player squads so solo runs remain winnable",
            "Save meta-currency, unlocked classes, and furthest mile/night record via pcall DataStore + BindToClose",
        ],
        automatic_twists: &[
            "MODULAR RAFT/CAMP BLUEPRINTS: snapped turrets, searchlights, and storage crates visibly upgrade your moving refuge mid-run",
            "DYNAMIC RIVER/FOREST FORKS: at every 10th milestone, the squad votes left (Safe Supply Town) vs right (High-Risk Relic Storm)",
            "FLARE BEACON SYNERGY: firing a craftable signal flare reveals hidden night caches and rallies downed teammates with a temporary speed aura",
        ],
        quality_boosts: &[
            "Golden-hour daytime wilderness shifting smoothly via TweenService Atmosphere into moonlit night waves",
            "Shared diegetic campfire/fuel beacon gauge visible right on the vehicle and top HUD",
            "Solo-to-4-player dynamic difficulty scaling so single-player sessions feel balanced and rewarding",
        ],
        pitch_prompt: "Build a high-retention Co-op Survival Expedition inspired by 99 Nights in the Forest and 100 Miles Down the River: a golden-hour riverfront outpost + modular upgradeable squad raft/camp, Dynamic Biome Fork votes every 10 milestones, scavenged fuel & salvage loop, smooth Day/Night/Storm lighting shifts, escalating night waves, Flare Beacon Synergy, permanent class unlocks, and server-authoritative DataStore saves.",
    },
    NicheProfile {
        id: "horror-coop-mascot",
        template_id: None,
        name: "Co-op Mascot & Asymmetrical Horror",
        tagline: "Iconic stylized characters, tense objective floors, and tight 3-minute social survival rounds.",
        craft_bar: "Match top-trending mascot horror craft: unmistakable stylized character silhouettes, rim-lit moody rooms with warm guide lamps (never unreadable pitch-black), and tight 3-minute tension arcs.",
        gameplay_loop: &[
            "Lobby Prep: Pick an original mascot role (Extractor, Runner, Medic) and enter the 4-player elevator queue",
            "Floor Objectives: Complete 4 tactile skill-check machines across interconnected themed rooms while dodging patrolling hunters",
            "Extraction Rush: Elevator chimes open -> 25-second sprint back to safety before doors lock",
            "Deeper Floors: Escalate difficulty and payout each floor or cash out with rare character unlock currency",
        ],
        ui_ux_style: &[
            "Clean compact objective tracker (Machines 2/4, Floor 3, Stamina bar) with crisp radial skill-check ring",
            "Playful stylized lobby cards showing mascot role stats (Speed, Extraction, Stealth, Stamina)",
            "Spectator camera bar with live player switcher when eliminated so dead teammates stay engaged",
        ],
        animations_and_juice: &[
            "Subtle vignette heartbeat pulse + soft audio thump intensifying as the hunter enters line-of-sight",
            "Tactile machine gauge sparks, lever pull TweenService motion, and elevator gate chime flash",
            "Expressive stylized character run/crouch/emote animations readable from across hallways",
        ],
        model_and_world_3d_style: &[
            "Bright playful Lobby Atrium contrasting with moody, rim-lit stylized Facility Floors (warm amber guide lamps + colored hallway stripes — NEVER pitch-black unreadable rooms)",
            "Clear room loop layouts with two exits per room so players can always outmaneuver pursuers",
            "Original charming mascot designs with bold color-coded props and zero copyrighted IP copying",
        ],
        audio_cadence: &[
            "Proximity heartbeat pulse + distant footstep cadence telegraphing hunter distance fairly",
            "Satisfying mechanical clunk + chime when a skill-check machine hits 100%",
            "Iconic elevator ding + countdown tick during the final 25-second extraction dash",
        ],
        monetization_loops: &[
            "Stylized Character Skins, Emote Packs & Flashlight/Effect Cosmetics (never pay-to-win stats)",
            "2x Currency / XP Boost Gamepass for unlocking the collectible survivor & hunter roster faster",
            "Map Vote Priority & Private Squad Lobby Gamepasses",
            "Seasonal Event Crates with guaranteed pity progression for limited mascot cosmetics",
        ],
        retention_hooks: &[
            "Distinct playable character cast with clear roles (Extractor, Distractor, Support) and mastery quests",
            "Escalating floor/round loop (repair machines -> open elevator -> deeper floor with higher payout)",
            "Atmospheric audio tension cues + readable line-of-sight silhouettes and heartbeat proximity feedback",
            "Lobby social showcase area where players show off unlocked characters, badges, and highest floor reached",
        ],
        anti_slop_guardrails: &[
            "Avoid pitch-black unreadable rooms: pair atmospheric lighting with rim-lit objectives and warm guide lanterns",
            "Keep killer/monster patrol pathfinding, hit validation, and objective progress strictly server-authoritative",
            "Build clean intermission -> match -> results -> lobby round loops with spectator camera for eliminated players",
            "Design charming, memorable original mascot silhouettes (zero copied copyrighted characters)",
        ],
        automatic_twists: &[
            "CO-OP CIRCUIT OVERCHARGE: two players repairing opposite machines simultaneously build a Surge meter that briefly stuns the hunter with a power flash",
            "SHAPESHIFTING FLOOR MODIFIERS: every 3rd floor introduces a visual twist (Power Outage Glowsticks, Speed Conveyor Halls, Double Bonus Vault)",
            "DYNAMIC RESCUE BEACON: surviving teammates can trigger a 1-time defibrillator station to bring back a downed friend for massive team XP",
        ],
        quality_boosts: &[
            "Rim-lit stylized hallways with color-coded floor stripes and glowing objective beacons (zero blind black walls)",
            "Clean radial skill-check ring with generous feedback + instant elevator spectator camera cycle",
            "Expressive original mascot role pedestals in a vibrant sunlit lobby atrium",
        ],
        pitch_prompt: "Build a Co-op Mascot Horror Floor-Survival experience inspired by Dandy's World and Forsaken: a vibrant lobby atrium with original character role pedestals (Extractor, Runner, Medic), rim-lit multi-room facility floors with warm guide lanterns, skill-check machines + Co-op Circuit Overcharge twist, server-authoritative PathfindingService hunter patrol with proximity heartbeat cues, spectator camera cycle, elevator extraction, and DataStore unlock saves.",
    },
    NicheProfile {
        id: "tactical-arena-pvp",
        template_id: None,
        name: "Tactical Arena & Skill PvP",
        tagline: "Sub-5-second matchmaking, razor-sharp combat feel, and mastery cosmetic progression.",
        craft_bar: "Match top-trending competitive arena craft: ultra-readable sunlit symmetrical geometry, crisp hit confirmation, sub-5-second rematch cadence, and 100% server-validated fair combat.",
        gameplay_loop: &[
            "Instant Queue: Step onto glowing 1v1 / 2v2 / 4v4 lobby duel pads -> teleport into arena in <5 seconds",
            "Tactical Rounds: Best-of-5 45-second rounds with high-ground jump pads, cover flanks, and skill abilities",
            "Mastery Progression: Earn Win Streaks, ELO Rank Badges, and Cosmetic Weapon Wraps through contracts",
            "Zero Downtime: Instant rematch vote or return-to-pad flow keeping players in constant action",
        ],
        ui_ux_style: &[
            "Razor-clean esports HUD: top centered round score (3 - 2) + timer, crisp center crosshair with hitmarker flash, compact bottom cooldown pills",
            "Compact top-right KO feed with weapon/ability icon and floating overhead win-streak flame badge",
            "Zero obstructive menus during combat; loadout swaps happen cleanly during 5-second intermission",
        ],
        animations_and_juice: &[
            "Crisp crosshair hitmarker expand + golden crit flash + subtle camera punch on confirmed server hits",
            "Smooth slide/dash trail ribbons, jump-pad launch arcs, and snappy victory pose spotlight",
            "3-2-1 round countdown banner with subtle slow-mo final-KO camera freeze",
        ],
        model_and_world_3d_style: &[
            "Crisp sunlit modern tournament arena (clean white/slate concrete, team-colored trim accents, vibrant cerulean sky) for maximum target silhouette contrast",
            "Symmetrical 3-lane cover geometry with clear high-ground bridges, sightline callouts, and jump pads",
            "High-visibility translucent ability telegraphs so every duel feels readable and skill-based",
        ],
        audio_cadence: &[
            "Razor-crisp hitmarker tick + deeper metallic crack on headshots/crits (`battle_crit.wav`)",
            "Distinctive ability cast/parry clangs and clean 3-2-1 countdown beeps",
            "Short triumphant round-win sting (`level_up.wav`) keeping pace snappy",
        ],
        monetization_loops: &[
            "Weapon Wraps, Kill Effect Finishers, Victory Poses & Banner Titles (100% skill-fair cosmetics)",
            "Seasonal Battle Pass with free & premium tracks advancing on matches, eliminations, and daily quests",
            "Private Tournament Servers with custom match rules & spectator camera controls",
            "Loadout Slot Expansion & Emote Wheel Gamepasses",
        ],
        retention_hooks: &[
            "Instant 1v1 / 2v2 / 4v4 queue pads directly in the lobby — zero menu friction before first duel",
            "Win-streak counter floating above players + ELO/Rank division badges driving competitive pride",
            "Daily/Weekly mastery contracts (eliminations, parry streaks, clutch wins) paying cosmetic crate currency",
            "Fast best-of-5 rounds (30-45s each) keeping adrenaline high and rage-quits low",
        ],
        anti_slop_guardrails: &[
            "Every hit, cooldown, ammo check, and KO credit MUST be validated on the server (never trust client damage)",
            "Design symmetrical competitive cover geometry with clear high-ground sightlines and jump pads",
            "Provide crisp hitmarkers, sound feedback, and clean KO feed HUD so every exchange feels responsive",
            "Include anti-spawn-camp invulnerability frames and clean spectator camera during rounds",
        ],
        automatic_twists: &[
            "MID-ROUND OVERDRIVE ORB: a glowing high-ground energy orb spawns at 20s remaining, forcing decisive duels instead of stalling",
            "CLUTCH PARRY REFLECT: timed defensive block reflects incoming projectiles back with 1.5x speed and anime flash VFX",
            "ADAPTIVE ROUND BANNER: winning a 3+ streak visibly evolves your lobby pedestal & overhead flame aura",
        ],
        quality_boosts: &[
            "High-contrast sunlit tournament architecture guaranteeing crisp player silhouette readability",
            "Sub-5-second lobby pad queue -> symmetrical arena teleport -> instant rematch flow",
            "Server-verified hitmarkers, KO feed, spectator camera, and zero-lag cooldown HUD",
        ],
        pitch_prompt: "Build a fast-paced Tactical Arena PvP Duel game inspired by RIVALS and STAND OUT: a sunlit high-contrast tournament lobby with instant 1v1/2v2 queue pads, symmetrical cover arena with jump pads and Mid-Round Overdrive Orb, server-validated melee/ranged combat with Clutch Parry Reflect, best-of-5 round manager, crisp hitmarkers & KO feed, spectator camera, and cosmetic wrap shop with DataStore saves.",
    },
    NicheProfile {
        id: "anime-action-rpg-td",
        template_id: Some("anime-action-rpg-td-template"),
        name: "Anime Action RPG & Tower Defense",
        tagline: "Impactful awakening combos, summonable hero squads, raid bosses, and deep mastery trees.",
        craft_bar: "Match top-trending anime action/TD craft: vibrant stylized islands & arenas, bold elemental slash/impact VFX, clear boss attack telegraphs, and deeply satisfying awakening power curves.",
        gameplay_loop: &[
            "60-second Hook: Claim starter elemental combat style/unit -> clear Act 1 wave with flashy awakening finisher",
            "Squad & Mastery Growth: Level skills, roll traits, and evolve favorite units via Quest Tracker milestones",
            "Co-op Boss Raids: Team up through portal gates against telegraphed multi-phase raid bosses",
            "Endless Glory: Climb Infinite Wave leaderboards and spend earned gems on hourly rotating banners",
        ],
        ui_ux_style: &[
            "Sleek anime action HUD: crisp 4-move cooldown hotbar with radial fill, glowing Awakening ultimate bar, and compact quest tracker on right",
            "Readable floating damage numbers (white normal, gold crit, elemental icons) and chunky boss HP bar at top center",
            "Clean Summon Plaza banner preview displaying transparent rarity odds & pity progress",
        ],
        animations_and_juice: &[
            "Snappy melee combo strings with directional slash arcs, 50ms hitstop on final finisher, and knockback dust puffs",
            "Dramatic Awakening aura burst with camera zoom pulse and elemental color shift",
            "Clear ground telegraph rings (red/amber translucent decals) before boss slams so dodging feels fair",
        ],
        model_and_world_3d_style: &[
            "Vibrant sunlit stylized anime archipelago hub (stone plaza, glowing summon fountain, distinct themed World Portals, stylized trees & banners)",
            "Clean readable raid colosseums with raised perimeter arches and uncluttered combat floor so elemental VFX pop",
            "Zero flat untextured corridors — layer elevation steps, stone lanterns, and fluttering pennant flags",
        ],
        audio_cadence: &[
            "Layered melee whoosh + punchy impact crunch (`battle_crit.wav`) escalating across 4-hit combos",
            "Resonant power-up swell + chime (`mythic_fanfare.wav`) on Awakening activation and banner summons",
            "Warning bell/rumble cue on raid boss phase transition",
        ],
        monetization_loops: &[
            "Banner Summon Currency Packs + Trait Reroll Tokens (always paired with transparent pity & quest rewards)",
            "2x Speed, Auto-Skip Wave, 2x Mastery XP & Extra Unit/Ability Slot Gamepasses",
            "VIP Hub Access (AFK Time Chamber granting steady summon gems while chatting/idle)",
            "Exclusive Awakening Aura Cosmetics & Mount Speed Upgrades",
        ],
        retention_hooks: &[
            "Hourly Rotating Summon Banner + Daily Raid Boss schedule keeping players checking back",
            "Satisfying unit/ability Evolution Quests turning mid-tier drops into awakened powerhouses",
            "Co-op Story Acts + Endless Leaderboard Mode with weekly gem payouts",
            "Clear visual power progression: glowing awakenings, floating damage numbers, and milestone titles",
        ],
        anti_slop_guardrails: &[
            "Never ship static unreactive dummies: wire clear attack telegraphs, cooldown bars, and elemental hit FX",
            "Keep wave spawning, damage math, banner rolls, and unit upgrades strictly server-authoritative",
            "Give new players a free starter summon & winnable Act 1 within their first 60 seconds",
            "Persist unlocked units, mastery levels, and quest progress safely with pcall + UpdateAsync + BindToClose",
        ],
        automatic_twists: &[
            "ELEMENTAL RESONANCE COMBOS: pairing Flame + Tempest or Void + Frost skills triggers explosive chain-reaction debuffs",
            "AWAKENING BREAK METER: staggering a Raid Boss opens a 6-second 2x damage burst window with golden sky flash",
            "INTERACTIVE AFK SPIRIT CHAMBER: idle shrine accumulates gems while displaying your top awakened hero statue live to the server",
        ],
        quality_boosts: &[
            "Sunlit stylized Summon Plaza & Raid Colosseum with clear combat floor contrast and translucent ground telegraphs",
            "Instant winnable Act 1 within 60 seconds plus transparent banner pity + quest gem flow",
            "Juicy 4-hit combo hitstop, floating damage numbers, and Awakening ultimate camera punch",
        ],
        pitch_prompt: "Build an Anime Action RPG & Co-op Wave Arena inspired by Blox Fruits and Anime Adventures: a sunlit stylized Summon Plaza + Raid Colosseum hub, 3 elemental combat kits with Elemental Resonance Combos and server-validated cooldowns, telegraphed multi-phase bosses with Awakening Break Meter, Quest Tracker + currency payouts, hourly banner pity, AFK Spirit Chamber, and DataStore saves.",
    },
    NicheProfile {
        id: "social-rp-fashion",
        template_id: None,
        name: "Social Roleplay, Town & Fashion",
        tagline: "Zero-friction self-expression, timed runway shows, customizable spaces, and social UGC.",
        craft_bar: "Match top-trending social/fashion craft: inviting warm architecture, flattering studio lighting, tactile zero-friction styling/spawning, and share-worthy podium moments.",
        gameplay_loop: &[
            "Theme Reveal: Crisp 3-minute Fashion/Build Theme announced with inspiring visual palette prompt",
            "Style & Customize: Browse organized wardrobe/prop stations, layer accessories, pick poses, and test in photo booths",
            "Runway & Podium: Strut across spotlighted runway while peers rate looks (1-5 stars) -> Top 3 podium celebration",
            "Social Hub Life: Hang out in sunlit town square, unlock new poses/VIP lounge, and spawn vehicles/houses with friends",
        ],
        ui_ux_style: &[
            "Clean pastel/editorial HUD: sleek top theme & timer pill, organized color-swatched wardrobe drawer, and expressive Pose/Emote wheel",
            "Smooth 1-to-5 star voting tray during runway walks that stays completely clear of the model's silhouette",
            "Zero cluttered walls of text — icon-first controls friendly to desktop and mobile thumbs alike",
        ],
        animations_and_juice: &[
            "Smooth camera track sweeping down the runway with soft spotlight bloom and confetti cannon on Podium 1st/2nd/3rd",
            "Sparkle chime pop when equipping an accessory or toggling a lighting preset (Golden Hour / Pastel Studio / Neon Gala)",
            "Expressive multi-pose transitions and duo friend pose sync",
        ],
        model_and_world_3d_style: &[
            "Warm inviting sunlit plaza & boutique studio (cream marble floors, pastel awning trim, glass skylights, lush floral planters — zero gloomy boxes)",
            "Flattering soft lighting rig (`ClockTime = 14.5`, warm SunRays + subtle Bloom) plus instant Photo-Booth backdrop presets",
            "Thoughtfully scaled doors, chairs, runway ramps, and mirrors aligned cleanly for Roblox R15/R6 avatars",
        ],
        audio_cadence: &[
            "Bright sparkling chime (`summon_roll.wav`) on theme reveal and wardrobe equip",
            "Crisp camera shutter clicks + applause swell during runway pose holds",
            "Celebratory podium fanfare (`mythic_fanfare.wav`) when Top 3 winners step up",
        ],
        monetization_loops: &[
            "VIP Lounge / Premium Props / Penthouse & Luxury Vehicle Gamepasses",
            "UGC Avatar Try-On & Outfit Purchase Integration (earns Roblox 40% affiliate commission)",
            "Custom Theme Picker, Double Votes/Stars & Runway Pose Pack Gamepasses",
            "Private Roleplay Servers for friend groups and content creators",
        ],
        retention_hooks: &[
            "5-minute timed Fashion/Build Theme Rounds with dramatic podium voting and seasonal rank tiers",
            "Customizable homes, outfits, and interactive town jobs (Cafe, Studio, Arcade) with instant switching",
            "Secret interactive map Easter eggs & photo-booth lighting zones built for social sharing",
            "Daily Style Challenges & friend duo-posing bonuses driving strong word-of-mouth invites",
        ],
        anti_slop_guardrails: &[
            "Prioritize warm inviting architecture, flattering lighting presets (Day/Sunset/Neon Gala), and crisp camera framing",
            "Keep outfit/prop spawning rate-limited per player on the server so trolls cannot lag the lobby",
            "Ensure every interactive chair, runway stage, and prop works smoothly on both desktop and mobile touch",
            "Save unlocked poses, ranks, stars, and favorite outfits reliably via pcall DataStore + BindToClose",
        ],
        automatic_twists: &[
            "DUO SYNC RUNWAY: two friends can link before the timer ends to perform a synchronized dual-pose walk with bonus synergy stars",
            "SURPRISE MID-ROUND ACCESSORY DROP: at 60 seconds left, a glowing boutique display opens with 3 rare theme-matched statement props",
            "INTERACTIVE PHOTO-BOOTH STUDIO: players can switch studio backdrop lighting & save favorite looks onto lobby showcase mannequins",
        ],
        quality_boosts: &[
            "Warm sunlit boutique & marble plaza architecture with flattering skylight lighting presets",
            "Unobstructed runway camera tracking + confetti podium celebration + mobile-safe star voting bar",
            "Rate-limited server-authoritative prop/vehicle spawning and persistent DataStore style ranks",
        ],
        pitch_prompt: "Build a polished Social Fashion & Town Hub inspired by Dress To Impress and Brookhaven RP: a warm sunlit marble plaza and boutique studio connecting a spotlighted Runway Stage, 3-minute theme rounds with Duo Sync Runway twist and Surprise Mid-Round Accessory Drop, peer star voting podium with confetti, Interactive Photo-Booth lighting presets, rate-limited prop/vehicle spawner, and DataStore rank & wardrobe saves.",
    },
];

// ============================================================================
// Project-scoped Roblox research (real fetched facts + downloaded thumbnails)
// and SSRF-safe read-only web_fetch / web_search helpers (Gap #17).
// ============================================================================

/// Fetch live Roblox metadata and download thumbnail PNGs into
/// `<project_dir>/refs/<universe_id>/` with ONLY real fetched facts (no
/// hardcoded game-design boilerplate and never touching `TASTE-DOSSIER.md`).
pub fn research_roblox_game_to_project(
    project_dir: &Path,
    target: &str,
    niche_hint: Option<&str>,
) -> Result<Value, String> {
    research_roblox_game_to_project_with_base(
        project_dir,
        target,
        niche_hint,
        None,
        Duration::from_secs(12),
    )
}

pub fn research_roblox_game_to_project_with_base(
    project_dir: &Path,
    target: &str,
    niche_hint: Option<&str>,
    api_base_override: Option<&str>,
    timeout: Duration,
) -> Result<Value, String> {
    let (raw_id, is_place_id) = parse_roblox_id_input(target)?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout.min(Duration::from_secs(5)))
        .timeout_read(timeout)
        .timeout(timeout)
        .build();

    let (apis_base, games_base, thumbs_base) = match api_base_override {
        Some(base) => {
            let clean = base.trim_end_matches('/').to_string();
            (clean.clone(), clean.clone(), clean)
        }
        None => (
            "https://apis.roblox.com".to_string(),
            "https://games.roblox.com".to_string(),
            "https://thumbnails.roblox.com".to_string(),
        ),
    };

    let universe_id: u64 = if is_place_id {
        let url = format!("{apis_base}/universes/v1/places/{raw_id}/universe");
        let resp = agent
            .get(&url)
            .set("Accept", "application/json")
            .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")))
            .call()
            .map_err(|e| format!("Roblox place->universe lookup failed: {e}"))?;
        let val: Value = resp
            .into_json()
            .map_err(|e| format!("parse universe lookup json: {e}"))?;
        val.get("universeId")
            .and_then(|v| v.as_u64())
            .filter(|&u| u > 0)
            .ok_or_else(|| format!("Place ID {raw_id} did not resolve to a valid Roblox Universe ID"))?
    } else {
        raw_id
    };

    let game_url = format!("{games_base}/v1/games?universeIds={universe_id}");
    let game_val: Value = agent
        .get(&game_url)
        .set("Accept", "application/json")
        .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| format!("Roblox games API failed for universe {universe_id}: {e}"))?
        .into_json()
        .map_err(|e| format!("parse Roblox games API json: {e}"))?;

    let game_obj = game_val
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| format!("Roblox returned no game data for universe {universe_id}"))?;

    let name = game_obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown Roblox Game")
        .trim()
        .to_string();
    let description = game_obj
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let root_place_id = game_obj
        .get("rootPlaceId")
        .and_then(|v| v.as_u64())
        .unwrap_or(if is_place_id { raw_id } else { 0 });
    let playing = game_obj.get("playing").and_then(|v| v.as_u64()).unwrap_or(0);
    let visits = game_obj.get("visits").and_then(|v| v.as_u64()).unwrap_or(0);
    let favorited_count = game_obj
        .get("favoritedCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let genre = game_obj
        .get("genre_l1")
        .or_else(|| game_obj.get("genre"))
        .and_then(|v| v.as_str())
        .unwrap_or("All")
        .trim()
        .to_string();
    let creator_name = game_obj
        .pointer("/creator/name")
        .and_then(|v| v.as_str())
        .unwrap_or("Roblox Creator")
        .to_string();

    let (up_votes, down_votes) = {
        let votes_url = format!("{games_base}/v1/games/votes?universeIds={universe_id}");
        agent
            .get(&votes_url)
            .set("Accept", "application/json")
            .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")))
            .call()
            .ok()
            .and_then(|r| r.into_json::<Value>().ok())
            .and_then(|v| v.get("data")?.as_array()?.first().cloned())
            .map(|entry| {
                (
                    entry.get("upVotes").and_then(|v| v.as_u64()).unwrap_or(0),
                    entry.get("downVotes").and_then(|v| v.as_u64()).unwrap_or(0),
                )
            })
            .unwrap_or((0, 0))
    };
    let like_ratio_pct = compute_like_ratio(up_votes, down_votes);

    let thumbnails: Vec<String> = {
        let thumb_url = format!(
            "{thumbs_base}/v1/games/multiget/thumbnails?universeIds={universe_id}&countPerUniverse=6&defaults=true&size=768x432&format=Png&isCircular=false"
        );
        agent
            .get(&thumb_url)
            .set("Accept", "application/json")
            .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")))
            .call()
            .ok()
            .and_then(|r| r.into_json::<Value>().ok())
            .and_then(|v| {
                v.get("data")?
                    .as_array()?
                    .first()?
                    .get("thumbnails")?
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|t| t.get("imageUrl").and_then(|u| u.as_str()))
                            .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
                            .map(ToString::to_string)
                            .collect()
                    })
            })
            .unwrap_or_default()
    };

    let niche_id = niche_hint
        .and_then(|h| normalize_niche_id(h).ok())
        .unwrap_or_else(|| classify_game_niche(&name, &genre));

    let ref_dir = project_dir.join("refs").join(universe_id.to_string());
    std::fs::create_dir_all(&ref_dir)
        .map_err(|e| format!("create project refs dir: {e}"))?;

    // Download thumbnails locally (SSRF-checked unless api_base_override is set for tests).
    let allow_loopback = api_base_override.is_some();
    let mut local_thumbs: Vec<String> = Vec::new();
    for (idx, url) in thumbnails.iter().take(6).enumerate() {
        if let Ok(bytes) = download_image_bytes(url, allow_loopback, timeout) {
            let ext = detect_image_ext(&bytes);
            if let Some(ext) = ext {
                let rel = format!("refs/{universe_id}/thumb_{}.{ext}", idx + 1);
                let out_path = project_dir.join(&rel);
                if write_atomic(&out_path, &bytes).is_ok() {
                    local_thumbs.push(rel);
                }
            }
        }
    }

    let dossier_rel = format!("refs/{universe_id}/dossier.json");
    let readme_rel = format!("refs/{universe_id}/README.md");

    let mut md = String::new();
    md.push_str(&format!("# Reference Game: {name}\n\n"));
    md.push_str(&format!("- **Universe ID:** `{universe_id}`\n"));
    md.push_str(&format!("- **Root Place ID:** `{root_place_id}`\n"));
    md.push_str(&format!("- **Creator:** {creator_name}\n"));
    md.push_str(&format!("- **Genre:** `{genre}` (classified niche: `{niche_id}`)\n"));
    md.push_str(&format!(
        "- **Live Metrics:** {} playing · {} visits · {} favorites · {:.1}% likes ({} up / {} down)\n\n",
        format_ccu(playing),
        format_ccu(visits),
        format_ccu(favorited_count),
        like_ratio_pct,
        up_votes,
        down_votes
    ));
    if !description.is_empty() {
        md.push_str("## Official Description\n\n");
        md.push_str(&description);
        md.push_str("\n\n");
    }
    if !thumbnails.is_empty() {
        md.push_str("## Thumbnails\n\n");
        for (i, url) in thumbnails.iter().enumerate() {
            let local = local_thumbs
                .get(i)
                .map(|p| format!(" -> `{p}`"))
                .unwrap_or_default();
            md.push_str(&format!("{}. {url}{local}\n", i + 1));
        }
        md.push('\n');
    }

    let mut payload = json!({
        "ok": true,
        "source": "live_roblox_game_api",
        "researched_at_ms": now_ms(),
        "universe_id": universe_id,
        "root_place_id": root_place_id,
        "name": name,
        "creator": creator_name,
        "description": description,
        "playing": playing,
        "visits": visits,
        "favorited_count": favorited_count,
        "up_votes": up_votes,
        "down_votes": down_votes,
        "like_ratio_pct": like_ratio_pct,
        "genre": genre,
        "niche": niche_id,
        "thumbnails": thumbnails,
        "local_thumbnails": local_thumbs,
        "dossier_path": dossier_rel,
        "markdown_path": readme_rel,
    });

    // Auto-record `observed` evidence in `<project>/.openro/investigation/evidence.jsonl` (#62).
    if let Ok(ev_res) = crate::study_check::record_evidence(
        project_dir,
        &json!({
            "kind": "roblox_game_research",
            "source": "research_roblox_game",
            "summary": format!(
                "Observed Roblox game '{}' (universe {}, place {}): {} playing, {} visits, {:.1}% likes, {} local thumbnail(s)",
                name,
                universe_id,
                root_place_id,
                format_ccu(playing),
                format_ccu(visits),
                like_ratio_pct,
                local_thumbs.len()
            ),
            "confidence": "observed",
            "authority": "external_surface",
            "coverage": format!("public_roblox_api+{}thumbnails", local_thumbs.len()),
            "limitations": [
                "Public Roblox storefront metadata, votes, and thumbnails only; live gameplay mechanics not directly observed"
            ],
            "refs": [dossier_rel, readme_rel],
            "data": {
                "universe_id": universe_id,
                "root_place_id": root_place_id,
                "name": name,
                "playing": playing,
                "visits": visits,
                "like_ratio_pct": like_ratio_pct,
                "niche": niche_id,
                "local_thumbnails": local_thumbs,
            }
        }),
    ) {
        if let Some(eid) = ev_res.pointer("/evidence/id").and_then(Value::as_str) {
            payload["evidence_id"] = json!(eid);
        }
    }

    let json_text = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("serialize project dossier: {e}"))?;
    write_atomic(&project_dir.join(&dossier_rel), json_text.as_bytes())?;
    write_atomic(&project_dir.join(&readme_rel), md.as_bytes())?;
    Ok(payload)
}

pub(crate) fn detect_image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

fn is_forbidden_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_unspecified()
                || (o[0] == 100 && (64..=127).contains(&o[1])) // CGNAT 100.64.0.0/10
                || (o[0] == 0)
        }
        std::net::IpAddr::V6(v6) => {
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
                || (seg0 & 0xfe00) == 0xfc00 // ULA fc00::/7
                || (seg0 & 0xffc0) == 0xfe80 // link-local fe80::/10
                || v6.to_ipv4_mapped().is_some_and(|v4| is_forbidden_ip(std::net::IpAddr::V4(v4)))
        }
    }
}

/// Parse scheme, host, port, and path from an HTTP(S) URL without external crates.
fn split_http_url(raw: &str) -> Result<(&str, &str, u16, &str), String> {
    let trimmed = raw.trim();
    let (scheme, rest, default_port) = if let Some(r) = trimmed.strip_prefix("https://") {
        ("https", r, 443u16)
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        ("http", r, 80u16)
    } else {
        return Err(format!("unsupported URL scheme in '{trimmed}' (only http/https allowed)"));
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => match rest.find('?') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        },
    };
    if authority.is_empty() {
        return Err("URL has no host".to_string());
    }
    if authority.contains('@') {
        return Err("URLs with embedded credentials are not allowed".to_string());
    }
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let end = bracketed.find(']').ok_or("malformed IPv6 host in URL")?;
        let h = &bracketed[..end];
        let after = &bracketed[end + 1..];
        let p = if let Some(ps) = after.strip_prefix(':') {
            ps.parse::<u16>().map_err(|_| "invalid port in URL")?
        } else {
            default_port
        };
        (h, p)
    } else if let Some((h, ps)) = authority.rsplit_once(':') {
        let p = ps.parse::<u16>().map_err(|_| "invalid port in URL")?;
        (h, p)
    } else {
        (authority, default_port)
    };
    if host.is_empty() || host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("invalid host '{host}'"));
    }
    Ok((scheme, host, port, path))
}

fn resolve_redirect_url(current: &str, location: &str) -> Result<String, String> {
    let loc = location.trim();
    if loc.starts_with("https://") || loc.starts_with("http://") {
        return Ok(loc.to_string());
    }
    let (scheme, host, port, cur_path) = split_http_url(current)?;
    let default_port = if scheme == "https" { 443 } else { 80 };
    let host_part = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
    let origin = if port == default_port {
        format!("{scheme}://{host_part}")
    } else {
        format!("{scheme}://{host_part}:{port}")
    };
    if loc.starts_with("//") {
        Ok(format!("{scheme}:{loc}"))
    } else if loc.starts_with('/') {
        Ok(format!("{origin}{loc}"))
    } else {
        let base_dir = cur_path
            .rsplit_once('/')
            .map(|(d, _)| format!("{d}/"))
            .unwrap_or_else(|| "/".to_string());
        Ok(format!("{origin}{base_dir}{loc}"))
    }
}

/// Validate a URL for SSRF safety: scheme must be http/https, hostname cannot
/// be a literal or DNS-resolved loopback/private/link-local IP unless
/// `allow_loopback` is true (used strictly by unit tests).
pub fn validate_public_url(raw_url: &str, allow_loopback: bool) -> Result<String, String> {
    let trimmed = raw_url.trim();
    let (_scheme, host, port, _path) = split_http_url(trimmed)?;
    let host_lower = host.to_ascii_lowercase();
    if !allow_loopback
        && (host_lower == "localhost"
            || host_lower.ends_with(".localhost")
            || host_lower.ends_with(".local")
            || host_lower.ends_with(".internal"))
    {
        return Err(format!("blocked private/loopback host '{host}'"));
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if !allow_loopback && is_forbidden_ip(ip) {
            return Err(format!("blocked private/loopback IP '{ip}'"));
        }
        return Ok(trimmed.to_string());
    }
    use std::net::ToSocketAddrs;
    let addrs: Vec<std::net::SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolution failed for '{host}': {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("DNS returned no addresses for '{host}'"));
    }
    if !allow_loopback {
        for addr in &addrs {
            if is_forbidden_ip(addr.ip()) {
                return Err(format!(
                    "blocked private/loopback address '{}' resolved for '{host}'",
                    addr.ip()
                ));
            }
        }
    }
    Ok(trimmed.to_string())
}

pub(crate) fn fetch_bytes_checked(
    raw_url: &str,
    allow_loopback: bool,
    max_bytes: usize,
    timeout: Duration,
) -> Result<(String, String, Vec<u8>), String> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(timeout.min(Duration::from_secs(5)))
        .timeout_read(timeout)
        .timeout(timeout)
        .build();

    let mut current = validate_public_url(raw_url, allow_loopback)?;
    for _ in 0..5 {
        let resp = match agent
            .get(&current)
            .set("Accept", "text/html,application/json,text/plain,image/*;q=0.8,*/*;q=0.5")
            .set("User-Agent", concat!("OpenRO/", env!("CARGO_PKG_VERSION")))
            .call()
        {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) if (300..400).contains(&code) => {
                let loc = r
                    .header("Location")
                    .ok_or_else(|| format!("redirect HTTP {code} missing Location header"))?;
                let next = resolve_redirect_url(&current, loc)?;
                current = validate_public_url(&next, allow_loopback)?;
                continue;
            }
            Err(e) => return Err(format!("HTTP fetch failed for {current}: {e}")),
        };
        let status = resp.status();
        if (300..400).contains(&status) {
            let loc = resp
                .header("Location")
                .ok_or_else(|| format!("redirect HTTP {status} missing Location header"))?;
            let next = resolve_redirect_url(&current, loc)?;
            current = validate_public_url(&next, allow_loopback)?;
            continue;
        }
        let content_type = resp
            .header("Content-Type")
            .unwrap_or("application/octet-stream")
            .to_string();
        use std::io::Read;
        let mut buf = Vec::new();
        resp.into_reader()
            .take((max_bytes as u64) + 1)
            .read_to_end(&mut buf)
            .map_err(|e| format!("read response body: {e}"))?;
        if buf.len() > max_bytes {
            buf.truncate(max_bytes);
        }
        return Ok((current, content_type, buf));
    }
    Err("too many HTTP redirects".to_string())
}

fn percent_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub(crate) fn download_image_bytes(
    url: &str,
    allow_loopback: bool,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let (_, _, bytes) = fetch_bytes_checked(url, allow_loopback, 5 * 1024 * 1024, timeout)?;
    if bytes.is_empty() {
        return Err("empty image".to_string());
    }
    Ok(bytes)
}

/// Strip HTML tags, scripts, styles, and collapse whitespace into clean text.
pub fn html_to_plain_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut stripped = String::with_capacity(html.len().min(32_768));
    let mut i = 0usize;
    while i < bytes.len() {
        if lower[i..].starts_with("<script") {
            if let Some(end) = lower[i..].find("</script>") {
                i += end + "</script>".len();
                stripped.push(' ');
                continue;
            }
            break;
        }
        if lower[i..].starts_with("<style") {
            if let Some(end) = lower[i..].find("</style>") {
                i += end + "</style>".len();
                stripped.push(' ');
                continue;
            }
            break;
        }
        if bytes[i] == b'<' {
            if let Some(close) = html[i..].find('>') {
                let tag = &lower[i + 1..i + close];
                if tag.starts_with("br")
                    || tag.starts_with("p")
                    || tag.starts_with("/p")
                    || tag.starts_with("li")
                    || tag.starts_with("/h")
                    || tag.starts_with("div")
                    || tag.starts_with("/div")
                    || tag.starts_with("tr")
                {
                    stripped.push('\n');
                } else {
                    stripped.push(' ');
                }
                i += close + 1;
                continue;
            }
        }
        let ch = html[i..].chars().next().unwrap();
        stripped.push(ch);
        i += ch.len_utf8();
    }
    let decoded = stripped
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    let mut lines = Vec::new();
    for line in decoded.lines() {
        let collapsed: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !collapsed.is_empty() {
            lines.push(collapsed);
        }
    }
    lines.join("\n")
}

/// Safe read-only web fetch (SSRF-blocked, size-capped, timeout-bounded).
pub fn web_fetch(raw_url: &str, max_chars: usize) -> Result<Value, String> {
    web_fetch_with_options(raw_url, max_chars, false, Duration::from_secs(15))
}

pub fn web_fetch_with_options(
    raw_url: &str,
    max_chars: usize,
    allow_loopback: bool,
    timeout: Duration,
) -> Result<Value, String> {
    let cap = max_chars.clamp(512, 24_000);
    let (final_url, content_type, bytes) =
        fetch_bytes_checked(raw_url, allow_loopback, 256 * 1024, timeout)?;
    let raw_text = String::from_utf8_lossy(&bytes);
    let text = if content_type.to_ascii_lowercase().contains("html") {
        html_to_plain_text(&raw_text)
    } else {
        raw_text.to_string()
    };
    let truncated = text.chars().count() > cap;
    let body: String = text.chars().take(cap).collect();
    Ok(json!({
        "ok": true,
        "url": final_url,
        "content_type": content_type,
        "bytes": bytes.len(),
        "truncated": truncated,
        "text": body,
    }))
}

/// Read-only web search via DuckDuckGo HTML endpoint (SSRF-safe, no API key).
pub fn web_search(query: &str, limit: usize) -> Result<Value, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("web_search requires a non-empty query".to_string());
    }
    let encoded = percent_encode_query(q);
    let endpoint = format!("https://html.duckduckgo.com/html/?q={encoded}");
    web_search_with_endpoint(q, &endpoint, limit, false, Duration::from_secs(15))
}

pub fn web_search_with_endpoint(
    query: &str,
    endpoint: &str,
    limit: usize,
    allow_loopback: bool,
    timeout: Duration,
) -> Result<Value, String> {
    let cap = limit.clamp(1, 12);
    let (_, _, bytes) = fetch_bytes_checked(endpoint, allow_loopback, 256 * 1024, timeout)?;
    let html = String::from_utf8_lossy(&bytes);
    let results = parse_ddg_html_results(&html, cap);
    Ok(json!({
        "ok": true,
        "query": query,
        "count": results.len(),
        "results": results,
    }))
}

fn parse_ddg_html_results(html: &str, limit: usize) -> Vec<Value> {
    let mut results = Vec::new();
    let mut cursor = html;
    while results.len() < limit {
        let Some(pos) = cursor.find("class=\"result__a\"") else {
            break;
        };
        let chunk = &cursor[pos..];
        let href = extract_attr(chunk, "href=")
            .map(|h| decode_ddg_redirect(&h))
            .unwrap_or_default();
        let title = chunk
            .find('>')
            .and_then(|s| {
                let after = &chunk[s + 1..];
                let e = after.find("</a>")?;
                Some(html_to_plain_text(&after[..e]))
            })
            .unwrap_or_default();
        let snippet = chunk
            .find("result__snippet")
            .and_then(|sp| {
                let sub = &chunk[sp..];
                let s = sub.find('>')?;
                let after = &sub[s + 1..];
                let e = after.find("</a").or_else(|| after.find("</div"))?;
                Some(html_to_plain_text(&after[..e]))
            })
            .unwrap_or_default();
        if !href.is_empty() && !title.is_empty() {
            results.push(json!({
                "title": title,
                "url": href,
                "snippet": snippet,
            }));
        }
        cursor = &chunk["class=\"result__a\"".len()..];
    }
    results
}

fn extract_attr(s: &str, attr: &str) -> Option<String> {
    let tag_end = s.find('>')?;
    let tag = &s[..tag_end];
    let i = tag.find(attr)?;
    let rest = &tag[i + attr.len()..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let after = &rest[1..];
    let end = after.find(quote)?;
    Some(after[..end].replace("&amp;", "&"))
}

fn decode_ddg_redirect(href: &str) -> String {
    if let Some(pos) = href.find("uddg=") {
        let raw = &href[pos + 5..];
        let end = raw.find('&').unwrap_or(raw.len());
        let u = percent_decode(&raw[..end]);
        if !u.is_empty() {
            return u;
        }
    }
    if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    }
}
