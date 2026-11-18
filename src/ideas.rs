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
/// follow-ups whose prerequisites are built and whose own skills are
/// not built yet.
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
                    .all(|s| !built.iter().any(|b| b == s))
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
