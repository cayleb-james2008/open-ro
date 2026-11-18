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
    ]
}

/// A present-but-wrong-typed argument is an error, never a silent
/// default: a model typo must change nothing quietly. (Absent args
/// still take their documented defaults.)
fn arg_i64(args: &Value, name: &str, default: i64) -> Result<i64, String> {
    match args.get(name) {
        None => Ok(default),
        Some(v) => v
            .as_i64()
            .ok_or_else(|| format!("skill arg '{name}' must be an integer, got: {v}")),
    }
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
        Some(v) => v
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("skill arg '{name}' must be a string, got: {v}")),
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
            let start_amount = arg_i64(args, "start_amount", 0)?;
            run_skill(def.source, def.entry, (currency, start_amount))
        }
        "day_night" => {
            let cycle_minutes = arg_i64(args, "cycle_minutes", 10)?;
            run_skill(def.source, def.entry, (cycle_minutes,))
        }
        "kill_brick" => {
            let damage = arg_i64(args, "damage", 100)?;
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
