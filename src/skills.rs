//! Rune-backed code-generation skills.
//!
//! The Luau the agent applies is genuinely produced by executing embedded
//! Rune scripts (`.rn`) on a Rune VM — there is no Rust-side string template
//! for game code. Every skill is one Rune script with one entry function;
//! the registry below is the single source the agent's system prompt and
//! the dispatch both read, so a skill that exists is a skill the model is
//! told about, with the parameters it actually takes.

use serde_json::Value;
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

pub struct SkillDef {
    pub name: &'static str,
    pub description: &'static str,
    /// Human-readable parameter spec, shown to the model in the prompt.
    pub params: &'static str,
    source: &'static str,
    entry: &'static str,
}

/// The skill registry: every game-building skill the agent can call.
pub fn registry() -> Vec<SkillDef> {
    vec![
        SkillDef {
            name: "round_manager",
            description: "a RoundManager script (ServerScriptService) that starts the round; with create_round it first creates the workspace.Round folder it starts",
            params: "{\"create_round\": bool}",
            source: ROUND_MANAGER_RN,
            entry: "generate_round_manager",
        },
        SkillDef {
            name: "leaderboard",
            description: "a Leaderboard script (ServerScriptService) that gives every joining player a leaderstats folder with one currency stat",
            params: "{\"currency\": string, \"start_amount\": int}",
            source: LEADERBOARD_RN,
            entry: "generate_leaderboard",
        },
        SkillDef {
            name: "day_night",
            description: "a DayNight script (ServerScriptService) that cycles Lighting.ClockTime through a full day",
            params: "{\"cycle_minutes\": int}",
            source: DAY_NIGHT_RN,
            entry: "generate_day_night",
        },
        SkillDef {
            name: "kill_brick",
            description: "a KillBrick script placed inside a part; any humanoid touching the part takes damage",
            params: "{\"damage\": int}",
            source: KILL_BRICK_RN,
            entry: "generate_kill_brick",
        },
        SkillDef {
            name: "spawn_link",
            description: "a SpawnLink script (ServerScriptService) that links to a named Workspace instance at startup — write it, playtest, and if the instance is missing the playtest says so and you create it",
            params: "{\"instance_name\": string}",
            source: SPAWN_LINK_RN,
            entry: "generate_spawn_link",
        },
        SkillDef {
            name: "currency",
            description: "a Currency script (ServerScriptService): leaderstats whose balance is saved in a DataStore between sessions — loaded on join, topped up passively over time, saved on leave and at shutdown (bind_close). Supersedes the plain leaderboard skill when the game needs money that persists",
            params: "{\"currency\": string, \"start_amount\": int, \"earn_amount\": int, \"earn_seconds\": int, \"bind_close\": bool}",
            source: CURRENCY_RN,
            entry: "generate_currency",
        },
        SkillDef {
            name: "shop",
            description: "a Shop script (ServerScriptService) that creates the OpenROShopBuy RemoteEvent in ReplicatedStorage and sells one item for the currency: checks the buyer's balance, deducts the price, records the purchase on the player",
            params: "{\"item_name\": string, \"price\": int, \"currency\": string}",
            source: SHOP_RN,
            entry: "generate_shop",
        },
        SkillDef {
            name: "game_gui",
            description: "a GameGui LocalScript — it belongs in StarterGui, NOT ServerScriptService: a title, a live balance label that follows the player's currency stat, and a shop button that fires the shop's OpenROShopBuy RemoteEvent. The game also needs the shop skill, or the playtest fails because the button has nothing to fire",
            params: "{\"title\": string, \"button_label\": string, \"currency\": string}",
            source: GAME_GUI_RN,
            entry: "generate_game_gui",
        },
        SkillDef {
            name: "checkpoints",
            description: "a Checkpoints script (ServerScriptService): stage pads that record each player's progress and respawn them at their highest stage; with create_folder it first creates the checkpoints folder and pads it uses",
            params: "{\"stage_count\": int, \"folder_name\": string, \"create_folder\": bool}",
            source: CHECKPOINTS_RN,
            entry: "generate_checkpoints",
        },
        SkillDef {
            name: "soundtrack",
            description: "a Soundtrack script (ServerScriptService) that plays a music track from SoundService for every player; sound_id 0 leaves the track silent until a real audio id is chosen",
            params: "{\"sound_id\": int, \"volume\": int (tenths, 0-10), \"looped\": bool}",
            source: SOUNDTRACK_RN,
            entry: "generate_soundtrack",
        },
    ]
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
    let def = registry()
        .into_iter()
        .find(|d| d.name == skill)
        .ok_or_else(|| format!("unknown skill: {skill}"))?;
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
            let instance_name = arg_string(args, "instance_name", "SpawnPad")?;
            if !valid_identifier(&instance_name) {
                return Err(format!(
                    "skill arg 'instance_name' must be a plain name (letters, digits, underscores), got: {instance_name}"
                ));
            }
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
            let folder_name = arg_string(args, "folder_name", "Checkpoints")?;
            if !valid_identifier(&folder_name) {
                return Err(format!(
                    "skill arg 'folder_name' must be a plain name (letters, digits, underscores), got: {folder_name}"
                ));
            }
            let create_folder = arg_bool(args, "create_folder", true)?;
            run_skill(def.source, def.entry, (stage_count, folder_name, create_folder))
        }
        "soundtrack" => {
            let sound_id = arg_i64_range(args, "sound_id", 0, 0, 9_999_999_999)?;
            let volume = arg_i64_range(args, "volume", 5, 0, 10)?;
            let looped = arg_bool(args, "looped", true)?;
            run_skill(def.source, def.entry, (sound_id, volume, looped))
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
