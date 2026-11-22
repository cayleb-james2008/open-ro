//! The agent loop.
//!
//! One user ask runs a model-driven loop. The model replies with exactly
//! one JSON step at a time:
//!   {"skill": "<rune skill>", "args": {...}}            — generate Luau
//!   {"tool": "write_script"|"delete_script"|"create_instance"|
//!            "delete_instance"|"playtest"|"get_console"|
//!            "get_place_tree", "args": {...}}           — Studio bridge
//!   {"done": true, "summary": "..."}                    — finish
//!
//! Luau comes ONLY from Rune skills: `write_script` writes a skill's
//! generated source (by skill name, or the most recent generation).
//! There is no way for the model to hand the loop raw Luau — an earlier
//! version accepted `args.source` directly, which bypassed the whole
//! skill system; that path is gone and the prompt no longer offers it.
//!
//! Before the first change to a path in an ask, the loop checkpoints
//! that path's previous state (script source, instance record, or its
//! absence). The change set belongs to the CHAT the ask ran in, and it
//! is harvested on EVERY exit path — success, honest failure, or a
//! mid-ask error — so `revert_chat` can always undo the most recent
//! change-making ask in that chat, even one that went wrong.
//!
//! Honesty rules the loop enforces itself, rather than trusting the
//! model or the bridge:
//!   - a `done` with changes that no playtest has passed since is fed
//!     back, not accepted;
//!   - a playtest verdict is read only in the documented bridge format
//!     (JSON `{"status": "passed"|"failed"}`, or the bare words
//!     "passed"/"failed"); anything else is "unproven", never a pass;
//!   - bridge tool errors surface as errors (see mcp.rs) and are fed
//!     back to the model — they are never narrated as success;
//!   - after MAX_REPAIRS failed playtests the ask ends in an honest
//!     failure naming the console output, change set still revertable.

use crate::mcp::McpClient;
use crate::model::ModelClient;
use crate::place::{
    serialize_rbxlx, CreatedPreset, DeleteResult, DuplicateResult, LocalPlace, PlaceBackend,
    ScriptUpdateResult,
};
use crate::rojo_export::{build_rojo_ship_pack, ShipPackAsset, ShipPackBundle};
use crate::skills;
use crate::snapshots::{compute_diff, PlaceDiff};
use crate::sse::BuildEventHub;
use crate::studio_stream::{StudioActionInput, StudioActionKind, StudioFrame, StudioStreamHub};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

/// 40 steps: a one-shot complete game (many skills, instances, a
/// playtest, repairs) legitimately needs more than a one-script ask.
const MAX_STEPS: usize = 40;
const MAX_REPAIRS: usize = 3;
/// Cap custom Luau writes (`write_luau` / `multi_edit`) cleanly at 512 KiB.
const MAX_LUAU_BYTES: usize = 512 * 1024;

/// Composer execution mode for `POST /api/chat` (`Agent | Plan | Ask`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AskMode {
    #[default]
    Agent,
    Plan,
    Ask,
}

impl AskMode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "plan" => AskMode::Plan,
            "ask" => AskMode::Ask,
            _ => AskMode::Agent,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AskMode::Agent => "agent",
            AskMode::Plan => "plan",
            AskMode::Ask => "ask",
        }
    }
}

/// Optional per-request RoPilot execution options for `POST /api/chat`.
#[derive(Clone, Debug, Default)]
pub struct AskOptions {
    pub mode: AskMode,
    pub reasoning: String,
    pub context_paths: Vec<String>,
    pub review_diffs: bool,
}

/// Outcome of a staged proposal run (`mode == "plan"` or `review_diffs == true`) executed
/// on an isolated shadow `LocalPlace` with zero live Studio mutations.
#[derive(Debug)]
pub struct StagedAskOutcome {
    pub outcome: RunOutcome,
    pub before_tree: Value,
    pub after_tree: Value,
    pub diagnostics: Value,
}

/// Returned by `revert_chat` when the chat has no change set waiting.
pub const NOTHING_TO_REVERT: &str = "nothing to revert";
/// Returned by `redo_chat` when the chat has no undone change set waiting.
pub const NOTHING_TO_REDO: &str = "nothing to redo";

/// Distill a large skill/dossier prompt block down to its highest-signal lines
/// (pack headers, active niche, Craft Bar, Anti-Slop Rules, and Automatic Twists)
/// so local loopback models with smaller context windows never truncate the core
/// JSON tool protocol.
fn distill_prompt_for_local_model(raw: &str, max_chars: usize) -> String {
    if raw.len() <= max_chars {
        return raw.to_string();
    }
    let mut out = String::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let high_signal = t.starts_with("##")
            || t.starts_with("Active Market Niche:")
            || t.starts_with("Top-Trending Craft Bar:")
            || t.starts_with("Craft Bar:")
            || t.starts_with("Visual Fidelity")
            || t.starts_with("Title-Epithet")
            || t.starts_with("8. ")
            || t.starts_with("- **Twist")
            || t.starts_with("- Anti-Slop")
            || t.contains("roblox-market-taste")
            || t.contains("Chain Overdrive")
            || t.contains("Fusion Weather");
        if !high_signal {
            continue;
        }
        if out.len() + t.len() + 1 > max_chars {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(t);
    }
    if out.is_empty() {
        let mut end = max_chars.min(raw.len());
        while end > 0 && !raw.is_char_boundary(end) {
            end -= 1;
        }
        raw[..end].to_string()
    } else {
        out
    }
}

/// Official Roblox Studio MCP tools exposed via `POST /api/studio/tools/:tool`
/// and natively usable by the OpenRO agent when a live Studio MCP bridge is attached.
pub const OFFICIAL_STUDIO_MCP_TOOLS: [&str; 13] = [
    "list_roblox_studios",
    "get_studio_state",
    "search_game_tree",
    "inspect_instance",
    "script_read",
    "script_grep",
    "multi_edit",
    "execute_luau",
    "get_console_output",
    "screen_capture",
    "start_stop_play",
    "user_mouse_input",
    "user_keyboard_input",
];

pub fn is_official_studio_tool(name: &str) -> bool {
    OFFICIAL_STUDIO_MCP_TOOLS.contains(&name)
}

/// The system prompt, generated from the skill registry so the model is
/// told exactly the game systems and RoPilot Studio tools that exist.
pub fn system_prompt() -> String {
    let mut skill_lines = String::new();
    for def in skills::registry() {
        skill_lines.push_str(&format!(
            "- {{\"skill\": \"{}\", \"args\": {}}} — {}\n",
            def.name, def.params, def.description
        ));
    }
    format!(
        "You are OpenRO, a RoPilot-grade Roblox Studio Copilot agent that builds and edits complete Roblox games. \
The user describes a game change in plain English and you inspect, author, and verify it. \
Reply with exactly ONE JSON object per turn, no prose outside the JSON.\n\n\
VERIFIED GAME-SYSTEM SKILLS (instant battle-tested Roblox systems):\n{skill_lines}\n\
STUDIO & WHOLE-GAME TOOLS:\n\
- {{\"tool\": \"write_script\", \"args\": {{\"path\": \"ServerScriptService.Name\", \"skill\": \"<skill name>\"}}}} writes a generated skill script (\"use_generated\": true writes the most recent skill result)\n\
- {{\"tool\": \"write_luau\", \"args\": {{\"path\": \"ServerScriptService.CustomSystem\", \"source\": \"-- custom Luau...\"}}}} writes custom server-authoritative Luau checked by the Static Luau Prover before saving\n\
- {{\"tool\": \"multi_edit\", \"args\": {{\"path\": \"ServerScriptService.Name\", \"edits\": [{{\"old_str\": \"...\", \"new_str\": \"...\"}}]}}}} applies targeted multi-hunk edits to an existing Luau script (Static Luau Prover gated)\n\
- {{\"tool\": \"create_instance\", \"args\": {{\"class_name\": \"SpawnLocation\", \"name\": \"SpawnPad\", \"parent\": \"Workspace\", \"properties\": {{...}}}}}} creates or updates a place instance\n\
- {{\"tool\": \"delete_instance\", \"args\": {{\"path\": \"Workspace.SpawnPad\"}}}} deletes an instance\n\
- {{\"tool\": \"delete_script\", \"args\": {{\"path\": \"ServerScriptService.Name\"}}}} deletes a script\n\
- {{\"tool\": \"get_place_tree\", \"args\": {{}}}} lists the place's scripts and instances\n\
- {{\"tool\": \"search_game_tree\", \"args\": {{\"query\": \"...\"}}}} searches instances and scripts across the place\n\
- {{\"tool\": \"inspect_instance\", \"args\": {{\"path\": \"Workspace.SpawnPad\"}}}} inspects properties, children, or script source\n\
- {{\"tool\": \"script_read\", \"args\": {{\"path\": \"ServerScriptService.Name\", \"start_line\": 1, \"end_line\": 120}}}} reads a Luau script\n\
- {{\"tool\": \"script_grep\", \"args\": {{\"query\": \"RemoteEvent\"}}}} searches across all Luau scripts\n\
- {{\"tool\": \"execute_luau\", \"args\": {{\"code\": \"...\"}}}} executes Luau inside live Roblox Studio\n\
- {{\"tool\": \"get_console_output\", \"args\": {{}}}} / {{\"tool\": \"get_console\", \"args\": {{}}}} reads Studio Output console lines\n\
- {{\"tool\": \"screen_capture\", \"args\": {{}}}} captures the live Studio 3D viewport\n\
- {{\"tool\": \"start_stop_play\", \"args\": {{\"mode\": \"start_play|run_server|stop\"}}}} controls live Studio play mode\n\
- {{\"tool\": \"user_mouse_input\", \"args\": {{\"action\": \"click\", \"x\": 0.5, \"y\": 0.5}}}} and {{\"tool\": \"user_keyboard_input\", \"args\": {{\"key\": \"W\"}}}} send live input to Studio\n\
- {{\"tool\": \"blender_generate\", \"args\": {{\"preset\": \"torii_gate|card_pedestal|colosseum_arch|cauldron|crystal_obelisk\", \"name\": \"ShrineGate\"}}}} generates a 3D .obj mesh + preview\n\
- {{\"tool\": \"audio_generate\", \"args\": {{\"preset\": \"summon_roll|mythic_fanfare|battle_crit|potion_drink|ui_click|level_up\", \"name\": \"CritSfx\"}}}} synthesizes a 44.1kHz WAV SFX\n\
- {{\"tool\": \"playtest\", \"args\": {{}}}} runs static + live playtest verification\n\
- {{\"done\": true, \"summary\": \"...\"}} finishes.\n\n\
Workflow (ONE JSON object per turn): For standard systems, call {{\"skill\": \"<name>\", \"args\": {{...}}}} then write_script; \
for custom mechanics or modifications, inspect with script_read/search_game_tree and edit with write_luau or multi_edit; \
then ALWAYS call {{\"tool\": \"playtest\", \"args\": {{}}}} to verify before {{\"done\": true, \"summary\": \"...\"}}.\n\
Repair from console or Static Luau Prover diagnostics until the playtest passes.\n\n{}",
        crate::art_knowledge::prompt_guidance()
    )
}

#[derive(Debug)]
pub struct RunOutcome {
    pub summary: String,
    /// Plain-words lines for the chat thread, produced by this ask.
    pub thread: Vec<String>,
    /// Step-by-step record of what the loop did (skills, tools, console).
    pub transcript: Vec<String>,
    /// Skills that generated Luau during this run — the server feeds
    /// these to the creativity engine, so follow-up pitches know what
    /// the game already has.
    pub skills_used: Vec<String>,
    /// True when the ask was refused rather than completed: `summary`
    /// then holds the plain refusal, not the model's claim of success.
    pub refused: bool,
    /// True when this ask (or plan) really changed the game. Only then
    /// does the server record skills as built and pitch the next set.
    pub changed: bool,
}

/// A failed ask, carrying the thread and transcript lines produced
/// before the failure — a failed ask is still part of the chat's
/// record, and its change set stays revertable.
#[derive(Debug)]
pub struct AskFailure {
    pub message: String,
    pub thread: Vec<String>,
    pub transcript: Vec<String>,
    /// Skills that generated before the failure. A failed build can
    /// still have landed scripts in the place, so the server records
    /// these as built — otherwise the pitch engine re-offers systems
    /// the game already (partly) has.
    pub skills_used: Vec<String>,
}

/// One checkpointed change: a script's previous source and the state
/// this ask LEFT it in, or an instance's previous record
/// (`{"class_name": ..., "properties": ...}`) and its left state.
/// `None` means the path did not exist at that point. The left state
/// is what makes revert safe: restore only proceeds when the place
/// still holds exactly what the ask left (or already holds the
/// previous state) — a path changed by anyone else since is refused,
/// never silently overwritten.
#[derive(Clone)]
enum Change {
    Script(String, Option<String>, Option<String>),
    Instance(String, Option<Value>, Option<Value>),
}

impl Change {
    /// True when this change left the path exactly as it found it
    /// (created then deleted, or rewritten with identical content) —
    /// a plan made only of these built nothing, whatever it claims.
    fn is_net_zero(&self) -> bool {
        match self {
            Change::Script(_, prev, post) => prev == post,
            Change::Instance(_, prev, post) => prev == post,
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Change::Script(path, prev, post) => json!({
                "kind": "script", "path": path,
                "prev": prev.clone().map(Value::String).unwrap_or(Value::Null),
                "post": post.clone().map(Value::String).unwrap_or(Value::Null),
            }),
            Change::Instance(path, prev, post) => json!({
                "kind": "instance", "path": path,
                "prev": prev.clone().unwrap_or(Value::Null),
                "post": post.clone().unwrap_or(Value::Null),
            }),
        }
    }

    /// Strict: a field of the wrong type makes the entry INVALID
    /// (None) — never coerced. (A numeric `prev` once coerced to
    /// "was absent", and a revert deleted a real script on the
    /// authority of an unreadable record.)
    fn from_json(v: &Value) -> Option<Change> {
        let path = v.get("path")?.as_str()?.to_string();
        fn script_state(v: &Value, key: &str) -> Option<Option<String>> {
            match v.get(key)? {
                Value::Null => Some(None),
                Value::String(s) => Some(Some(s.clone())),
                _ => None,
            }
        }
        fn instance_state(v: &Value, key: &str) -> Option<Option<Value>> {
            match v.get(key)? {
                Value::Null => Some(None),
                Value::Object(o) => {
                    // A record without a string class_name is not a
                    // record — restoring from it would fabricate one.
                    o.get("class_name").filter(|c| c.is_string())?;
                    Some(Some(Value::Object(o.clone())))
                }
                _ => None,
            }
        }
        match v.get("kind")?.as_str()? {
            "script" => Some(Change::Script(
                path,
                script_state(v, "prev")?,
                script_state(v, "post")?,
            )),
            "instance" => Some(Change::Instance(
                path,
                instance_state(v, "prev")?,
                instance_state(v, "post")?,
            )),
            _ => None,
        }
    }
}

/// One change-making ask's set: its checkpoints, and the skills it ran
/// (so a revert can tell the creativity engine what the place lost).
#[derive(Clone)]
struct ChangeSet {
    changes: Vec<Change>,
    skills: Vec<String>,
}

/// Whether a chat's place still holds changes no passing playtest has
/// covered, and the console of the last playtest that failed (until a
/// later pass clears it). Kept per chat, so a later ask cannot say
/// "done" over a game the app's own last test run rejected.
#[derive(Default, Clone)]
struct Proof {
    unproven: bool,
    failed_console: Option<String>,
    /// Skills that Undo took out of the place and nothing has rebuilt.
    undone: Vec<String>,
}

pub struct Agent {
    pub model: ModelClient,
    pub studio: Option<PlaceBackend>,
    /// Luau generated by each skill during this ask, by skill name — a
    /// second generation never overwrites an unwritten first one.
    generated: HashMap<String, String>,
    last_skill: Option<String>,
    /// Change sets per chat id, as a STACK: each change-making ask
    /// pushes its own set, and revert pops the most recent one. (A
    /// single-slot map used to silently replace the previous ask's
    /// set, leaving its changes unrevertable forever; merging instead
    /// would break per-ask prev state — each set's checkpoints are the
    /// place as it was when THAT ask started.)
    changes_by_chat: HashMap<String, Vec<ChangeSet>>,
    /// Undone change sets per chat id, as a STACK for `POST /api/redo`.
    redo_by_chat: HashMap<String, Vec<ChangeSet>>,
    /// Change set being accumulated by the ask currently running.
    current_changes: Vec<Change>,
    /// Where change sets persist across restarts, when set.
    changes_path: Option<std::path::PathBuf>,
    /// Proof state per chat, saved as proof.json next to changes.json.
    proof_by_chat: HashMap<String, Proof>,
    proof_path: Option<std::path::PathBuf>,
    /// Backup file names written when a corrupt changes file was set aside.
    pub recovered: Vec<String>,
    /// Shared set of chats with a non-empty change-set stack (for the page's Undo).
    undo_sink: Option<std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>>,
    /// Shared set of chats with a non-empty redo stack (for the page's Redo).
    redo_sink: Option<std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>>,
    /// Optional SSE build-progress event hub.
    events: Option<Arc<BuildEventHub>>,
    /// Optional realtime Studio 3D action & MCP frame stream hub.
    studio_stream: Option<Arc<StudioStreamHub>>,
    /// Active build correlation id & chat id for SSE phase updates.
    active_build_id: String,
    active_build_chat: String,
    /// Dynamic skill prompt block injected from installed Tool & Skill Library packs.
    library_prompt: String,
    /// Optional 8-dimension Taste Dossier prompt block from the most recently researched Roblox game.
    market_dossier_prompt: String,
    /// Root directory for generated 3D meshes (`blender_generate`) and `.wav` SFX (`audio_generate`).
    assets_root: Option<std::path::PathBuf>,
}

impl Agent {
    pub fn new(model: ModelClient, studio: Option<McpClient>) -> Agent {
        Self::with_backend(model, studio.map(PlaceBackend::Mcp))
    }

    pub fn with_local_place(model: ModelClient, place: LocalPlace) -> Agent {
        let mut recovered = place.recovered.clone();
        let mut agent = Self::with_backend(model, Some(PlaceBackend::Local(place)));
        agent.recovered.append(&mut recovered);
        agent
    }

    pub fn with_backend(model: ModelClient, studio: Option<PlaceBackend>) -> Agent {
        Agent {
            model,
            studio,
            generated: HashMap::new(),
            last_skill: None,
            changes_by_chat: HashMap::new(),
            redo_by_chat: HashMap::new(),
            current_changes: Vec::new(),
            changes_path: None,
            proof_by_chat: HashMap::new(),
            proof_path: None,
            recovered: Vec::new(),
            undo_sink: None,
            redo_sink: None,
            events: None,
            studio_stream: None,
            active_build_id: String::new(),
            active_build_chat: String::new(),
            library_prompt: String::new(),
            market_dossier_prompt: String::new(),
            assets_root: None,
        }
    }

    pub fn set_assets_root(&mut self, root: std::path::PathBuf) {
        self.assets_root = Some(root);
    }

    fn effective_assets_root(&self) -> std::path::PathBuf {
        self.assets_root.clone().unwrap_or_else(|| {
            let dir = std::env::temp_dir().join(format!("openro-agent-assets-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            dir
        })
    }

    pub fn set_library_prompt(&mut self, prompt: String) {
        self.library_prompt = prompt;
    }

    pub fn set_market_dossier_prompt(&mut self, prompt: String) {
        self.market_dossier_prompt = prompt;
    }

    /// Full system prompt sent to the model: core OpenRO rules + installed Tool & Skill
    /// Library packs (including active Roblox Market Taste Craft Bar & Automatic Twists)
    /// + any recently researched Roblox reference game Taste Dossier.
    ///
    /// Local loopback gateways (e.g. Ollama 3B/7B models with default 4k context) receive
    /// a distilled high-signal Craft Bar & Twists digest so the core JSON tool contract
    /// is never truncated by a small context window; remote/frontier models receive the
    /// complete unabridged dossier.
    pub fn effective_system_prompt(&self) -> String {
        let mut out = system_prompt();
        let compact = self.model.is_loopback();
        if !self.library_prompt.trim().is_empty() {
            out.push_str("\n\n");
            if compact {
                out.push_str(&distill_prompt_for_local_model(&self.library_prompt, 1100));
            } else {
                out.push_str(&self.library_prompt);
            }
        }
        if !self.market_dossier_prompt.trim().is_empty() {
            out.push_str("\n\n");
            if compact {
                out.push_str(&distill_prompt_for_local_model(&self.market_dossier_prompt, 450));
            } else {
                out.push_str(&self.market_dossier_prompt);
            }
        }
        if !self.library_prompt.trim().is_empty() || !self.market_dossier_prompt.trim().is_empty() {
            out.push_str("\n\nREMINDER: Reply with exactly ONE JSON object per turn, no prose outside the JSON.");
        }
        out
    }

    pub fn studio_id(&self) -> Option<String> {
        match &self.studio {
            Some(PlaceBackend::Mcp(mcp)) => mcp.studio_id().map(String::from),
            _ => None,
        }
    }

    /// Attach an SSE `BuildEventHub` so build phases stream live to `/api/build/stream`.
    pub fn attach_events(&mut self, hub: Arc<BuildEventHub>) {
        self.events = Some(hub);
    }

    /// Attach a realtime `StudioStreamHub` so granular Studio mutations and live MCP
    /// viewport frames stream to `/api/studio/stream`.
    pub fn attach_studio_stream(&mut self, hub: Arc<StudioStreamHub>) {
        hub.set_studio_mode(self.studio_mode_tag());
        self.studio_stream = Some(hub);
    }

    pub fn studio_mode_tag(&self) -> &'static str {
        match &self.studio {
            Some(PlaceBackend::Mcp(_)) => "live_mcp",
            Some(PlaceBackend::Plugin(_)) => "plugin_live",
            _ => "local_place",
        }
    }

    fn place_counts(&mut self) -> (usize, usize) {
        match &mut self.studio {
            Some(PlaceBackend::Local(local)) => (local.instances().len(), local.scripts().len()),
            Some(PlaceBackend::Mcp(mcp)) => mcp
                .get_place_tree()
                .ok()
                .map(|t| {
                    let ic = t.get("instances").and_then(|v| v.as_object()).map(|m| m.len()).unwrap_or(0);
                    let sc = t.get("scripts").and_then(|v| v.as_object()).map(|m| m.len()).unwrap_or(0);
                    (ic, sc)
                })
                .unwrap_or((0, 0)),
            Some(PlaceBackend::Plugin(_)) | None => (0, 0),
        }
    }

    fn emit_studio_action(&mut self, mut input: StudioActionInput) {
        if self.studio_stream.is_none() {
            return;
        }
        let mode = self.studio_mode_tag().to_string();
        let (inst_count, script_count) = self.place_counts();
        input.studio_mode = Some(mode);
        input.instance_count = inst_count;
        input.script_count = script_count;
        if let Some(hub) = &self.studio_stream {
            hub.record_action(input);
        }
    }

    /// Best-effort live MCP `screen_capture` right after a mutation when attached to
    /// a live Roblox Studio MCP bridge (never fails the underlying build action).
    fn auto_capture_mcp_frame(&mut self) -> Option<u64> {
        let hub = self.studio_stream.as_ref().map(Arc::clone)?;
        let Some(PlaceBackend::Mcp(mcp)) = self.studio.as_mut() else {
            return None;
        };
        if !mcp.has_tool("screen_capture") {
            return None;
        }
        match mcp.screen_capture() {
            Ok(frame) => {
                let stored = hub.store_frame(frame);
                Some(stored.seq)
            }
            Err(_) => None,
        }
    }

    fn take_playtest_frame_seq(&mut self) -> Option<u64> {
        let hub = self.studio_stream.as_ref().map(Arc::clone)?;
        let Some(PlaceBackend::Mcp(mcp)) = self.studio.as_mut() else {
            return None;
        };
        mcp.take_last_playtest_frame()
            .map(|frame| hub.store_frame(frame).seq)
    }

    /// Explicitly capture one live Roblox Studio MCP frame (`POST /api/studio/capture`).
    pub fn capture_studio_frame(&mut self) -> Result<StudioFrame, String> {
        let frame = match self.studio.as_mut() {
            Some(PlaceBackend::Mcp(mcp)) => mcp.screen_capture()?,
            Some(PlaceBackend::Plugin(_)) => {
                return Err(
                    "Live Studio Frame capture requires Embedded Studio / MCP (Companion Plugin uses Studio's native viewport)."
                        .to_string(),
                );
            }
            Some(PlaceBackend::Local(_)) => {
                return Err(
                    "Live Studio Frame capture requires a connected Roblox Studio MCP session (currently running Local Place Engine). Switch to Studio 3D or attach Roblox Studio MCP."
                        .to_string(),
                );
            }
            None => return Err("Studio not connected".to_string()),
        };
        if let Some(hub) = &self.studio_stream {
            Ok(hub.store_frame(frame))
        } else {
            Ok(frame)
        }
    }

    /// Execute a Studio playtest directly (`POST /api/studio/playtest`), streaming
    /// `PlaytestStarted`, `PlaytestLog`, and `PlaytestPassed`/`PlaytestFailed` actions
    /// over `StudioStreamHub` (plus capturing a live MCP frame when connected).
    pub fn run_studio_playtest(&mut self) -> Result<Value, String> {
        let mode = self.studio_mode_tag().to_string();
        let mut start_ev = StudioActionInput::new(
            StudioActionKind::PlaytestStarted,
            if mode == "live_mcp" {
                "Started live Roblox Studio playtest via MCP"
            } else {
                "Running Local Place static Luau & playtest verification"
            },
        );
        start_ev.path = "Workspace".to_string();
        start_ev.class_name = "RunService".to_string();
        self.emit_studio_action(start_ev);

        let (passed, lines, frame_seq) = match self.studio.as_mut() {
            Some(PlaceBackend::Mcp(mcp)) => {
                let start_res = mcp.start_stop_play("start_play");
                let start_ok = match &start_res {
                    Ok(v) => {
                        let st = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
                        !st.eq_ignore_ascii_case("failed") && v.get("ok").and_then(|b| b.as_bool()) != Some(false)
                    }
                    Err(_) => false,
                };
                let console_lines = mcp.get_console_output().unwrap_or_default();
                let fseq = self.auto_capture_mcp_frame();
                if let Some(PlaceBackend::Mcp(mcp_stop)) = self.studio.as_mut() {
                    let _ = mcp_stop.start_stop_play("stop");
                }
                let has_err = console_lines.iter().any(|l| {
                    let low = l.to_ascii_lowercase();
                    low.contains("[error]") || low.contains("syntax error") || low.contains("runtime error")
                });
                let mut out_lines = console_lines;
                if let Err(e) = start_res {
                    out_lines.push(format!("playtest error: {e}"));
                }
                (start_ok && !has_err, out_lines, fseq)
            }
            Some(PlaceBackend::Plugin(_)) => {
                let text = self.studio_mut()?.call_tool("playtest", json!({}))?;
                let verdict = playtest_verdict(&text);
                let mut out_lines: Vec<String> = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| {
                        v.get("lines")
                            .and_then(|a| a.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| x.as_str().map(String::from))
                                    .collect::<Vec<_>>()
                            })
                            .or_else(|| {
                                v.get("console").and_then(|c| c.as_str()).map(|s| {
                                    s.lines()
                                        .map(str::trim)
                                        .filter(|l| !l.is_empty())
                                        .map(String::from)
                                        .collect::<Vec<_>>()
                                })
                            })
                    })
                    .unwrap_or_default();
                if out_lines.is_empty() {
                    out_lines.push(if matches!(verdict, Verdict::Passed) {
                        "Companion Studio Plugin playtest passed.".to_string()
                    } else {
                        text.clone()
                    });
                }
                (matches!(verdict, Verdict::Passed), out_lines, None)
            }
            Some(PlaceBackend::Local(_)) => {
                let text = self.studio_mut()?.call_tool("playtest", json!({}))?;
                let verdict = playtest_verdict(&text);
                let raw_console = self
                    .studio_mut()?
                    .call_tool("get_console", json!({}))
                    .unwrap_or_default();
                let mut out_lines: Vec<String> = raw_console
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
                if out_lines.is_empty() {
                    out_lines.push(if matches!(verdict, Verdict::Passed) {
                        "Local Place playtest passed with 0 Luau errors.".to_string()
                    } else {
                        text.clone()
                    });
                }
                (matches!(verdict, Verdict::Passed), out_lines, None)
            }
            None => return Err("Studio not connected".to_string()),
        };

        for line in lines.iter().take(16) {
            let mut log_ev = StudioActionInput::new(StudioActionKind::PlaytestLog, line.clone());
            log_ev.path = "Output".to_string();
            log_ev.class_name = "LogService".to_string();
            log_ev.console_line = Some(line.clone());
            log_ev.frame_seq = frame_seq;
            self.emit_studio_action(log_ev);
        }

        let mut verdict_ev = StudioActionInput::new(
            if passed {
                StudioActionKind::PlaytestPassed
            } else {
                StudioActionKind::PlaytestFailed
            },
            if passed {
                format!("Studio playtest passed ({} console line(s))", lines.len())
            } else {
                format!("Studio playtest failed ({} console line(s))", lines.len())
            },
        );
        verdict_ev.path = "Workspace".to_string();
        verdict_ev.class_name = "RunService".to_string();
        verdict_ev.console_line = lines.last().cloned();
        verdict_ev.frame_seq = frame_seq;
        self.emit_studio_action(verdict_ev);

        Ok(json!({
            "ok": true,
            "passed": passed,
            "status": if passed { "passed" } else { "failed" },
            "studio_mode": mode,
            "console": lines.join("\n"),
            "console_lines": lines,
            "frame_seq": frame_seq,
        }))
    }

    fn begin_build_trace(&mut self, chat_id: &str, label: &str) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        self.active_build_id = format!("build-{nonce}");
        self.active_build_chat = chat_id.to_string();
        self.emit_event("planning", "Planning", label, None, None, 10);
    }

    fn emit_event(
        &self,
        phase: &str,
        label: &str,
        detail: &str,
        skill: Option<&str>,
        path: Option<&str>,
        progress: u8,
    ) {
        if let Some(hub) = &self.events {
            hub.publish(
                self.active_build_id.clone(),
                self.active_build_chat.clone(),
                phase,
                label,
                detail,
                skill.map(String::from),
                path.map(String::from),
                progress,
            );
        }
    }

    /// Persist change sets to `path` (loaded now, saved on change), so
    /// a restart does not silently orphan every revert. Loading is
    /// strict and loud: an unparseable file, a wrong shape, or ANY
    /// malformed entry backs the whole file up and starts empty —
    /// a partially-read change set is worse than none, because a
    /// revert acting on coerced records can destroy real work.
    pub fn set_changes_file(&mut self, path: std::path::PathBuf) {
        let proof_path = path.with_file_name("proof.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            let loaded = (|| -> Option<HashMap<String, Vec<ChangeSet>>> {
                let v: Value = serde_json::from_str(&text).ok()?;
                let obj = v.as_object()?;
                let mut out = HashMap::new();
                for (chat, stack) in obj {
                    let sets = stack.as_array()?;
                    let mut parsed_stack = Vec::new();
                    for set in sets {
                        let parsed = parse_set(set)?;
                        if !parsed.changes.is_empty() {
                            parsed_stack.push(parsed);
                        }
                    }
                    if !parsed_stack.is_empty() {
                        out.insert(chat.clone(), parsed_stack);
                    }
                }
                Some(out)
            })();
            match loaded {
                Some(map) => self.changes_by_chat = map,
                None => {
                    if let Some(bak) = crate::server::backup_corrupt(&path) {
                        self.recovered.push(bak.file_name().unwrap_or_default().to_string_lossy().into_owned());
                    }
                }
            }
        }
        self.changes_path = Some(path);
        self.sync_undo();
        // A chat with change sets but no saved verdict (proof file corrupt
        // or missing) is unproven until a passing playtest: conservative.
        self.proof_by_chat = load_proof(&proof_path, &mut self.recovered);
        for (chat, stack) in &self.changes_by_chat {
            if !stack.is_empty() {
                self.proof_by_chat.entry(chat.clone()).or_insert(Proof {
                    unproven: true,
                    ..Proof::default()
                });
            }
        }
        self.proof_path = Some(proof_path);
        self.save_proof();
    }

    fn save_changes(&self) {
        let Some(path) = &self.changes_path else { return };
        let mut obj = serde_json::Map::new();
        for (chat, stack) in &self.changes_by_chat {
            obj.insert(
                chat.clone(),
                Value::Array(
                    stack
                        .iter()
                        .map(|set| {
                            json!({
                                "changes": set.changes.iter().map(Change::to_json).collect::<Vec<_>>(),
                                "skills": set.skills,
                            })
                        })
                        .collect(),
                ),
            );
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, Value::Object(obj).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Keep the shared Undo set in step with the change-set stacks.
    pub fn attach_undo_sink(
        &mut self,
        sink: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    ) {
        self.undo_sink = Some(sink);
        self.sync_undo();
    }

    /// Keep the shared Redo set in step with the undone change-set stacks.
    pub fn attach_redo_sink(
        &mut self,
        sink: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    ) {
        self.redo_sink = Some(sink);
        self.sync_undo();
    }

    pub fn can_undo_chat(&self, chat_id: &str) -> bool {
        self.changes_by_chat
            .get(chat_id)
            .is_some_and(|s| !s.is_empty())
    }

    pub fn can_redo_chat(&self, chat_id: &str) -> bool {
        self.redo_by_chat
            .get(chat_id)
            .is_some_and(|s| !s.is_empty())
    }

    fn sync_undo(&self) {
        if let Some(sink) = &self.undo_sink {
            let mut set = sink.lock().unwrap();
            set.clear();
            set.extend(
                self.changes_by_chat
                    .iter()
                    .filter(|(_, stack)| !stack.is_empty())
                    .map(|(chat, _)| chat.clone()),
            );
        }
        if let Some(sink) = &self.redo_sink {
            let mut set = sink.lock().unwrap();
            set.clear();
            set.extend(
                self.redo_by_chat
                    .iter()
                    .filter(|(_, stack)| !stack.is_empty())
                    .map(|(chat, _)| chat.clone()),
            );
        }
    }

    fn save_proof(&self) {
        let Some(path) = &self.proof_path else { return };
        let obj: serde_json::Map<String, Value> = self
            .proof_by_chat
            .iter()
            .map(|(chat, p)| {
                (
                    chat.clone(),
                    json!({ "unproven": p.unproven, "failed_console": p.failed_console, "undone": p.undone }),
                )
            })
            .collect();
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, Value::Object(obj).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// The prerequisite a skill needs (per the creativity catalogue) when
    /// that prerequisite was undone in this chat and nothing rebuilt it.
    fn undone_prerequisite(&self, chat_id: &str, skill: &str) -> Option<String> {
        let proof = self.proof_by_chat.get(chat_id)?;
        let kept = self.chat_skills(chat_id);
        crate::ideas::catalogue()
            .into_iter()
            .filter(|i| i.skills.contains(&skill))
            .flat_map(|i| i.needs.into_iter().map(String::from).collect::<Vec<_>>())
            .find(|need| proof.undone.contains(need) && !kept.contains(need))
    }

    pub fn studio_connected(&mut self) -> bool {
        match &mut self.studio {
            Some(s) => s.is_alive(),
            None => false,
        }
    }

    pub fn backend_mode(&self) -> &'static str {
        match &self.studio {
            Some(b) => b.mode_name(),
            None => "disconnected",
        }
    }

    pub fn rbxlx_path(&self) -> Option<std::path::PathBuf> {
        self.studio.as_ref().and_then(|b| b.rbxlx_path())
    }

    pub fn export_rbxlx(&mut self, game_name: &str) -> Result<String, String> {
        let tree = self.place_tree()?;
        Ok(serialize_rbxlx(game_name, &tree))
    }

    /// Export a complete Roblox Studio & Rojo 7 Ship Pack `.zip` archive containing
    /// `game.rbxlx`, `default.project.json`, `src/{server,client,shared}/*.luau`,
    /// `asset-manifest.json`, `assets/*`, `luau-audit-report.json`, and `STUDIO-UPLOAD-CHECKLIST.md`.
    pub fn export_rojo_ship_pack(
        &mut self,
        game_name: &str,
        assets: &[ShipPackAsset],
    ) -> Result<ShipPackBundle, String> {
        match self.studio.as_mut() {
            Some(PlaceBackend::Local(local)) => {
                if local.game_name().trim().is_empty() && !game_name.trim().is_empty() {
                    local.set_game_name(game_name.to_string());
                }
                build_rojo_ship_pack(local, assets)
            }
            Some(PlaceBackend::Mcp(_)) | Some(PlaceBackend::Plugin(_)) => {
                let tree = self.place_tree()?;
                let mut tmp = LocalPlace::in_memory(game_name.to_string());
                tmp.replace_tree(&tree);
                build_rojo_ship_pack(&tmp, assets)
            }
            None => Err("Studio not connected".to_string()),
        }
    }

    /// Record a direct Studio IDE edit (part preset, duplicate, delete, live Luau edit,
    /// or property update) onto the active chat's per-chat Undo stack and clear Redo.
    pub fn record_direct_edit(&mut self, chat_id: &str, before: &Value, after: &Value) {
        let changes = changeset_from_trees(before, after);
        if !changes.is_empty() {
            let stack = self
                .changes_by_chat
                .entry(chat_id.to_string())
                .or_default();
            if stack.len() >= crate::place::MAX_HISTORY_DEPTH {
                stack.remove(0);
            }
            stack.push(ChangeSet {
                changes,
                skills: Vec::new(),
            });
            self.redo_by_chat.remove(chat_id);
            self.save_changes();
            self.sync_undo();
        }
    }

    /// Create a typed 3D part preset (`block`, `sphere`, `cylinder`, `spawn`,
    /// `jump_pad`, `kill_brick`, `card_billboard`) plus any companion Luau script
    /// and record it on the active chat's Undo stack.
    pub fn create_part_preset(
        &mut self,
        chat_id: &str,
        preset: &str,
        name: Option<&str>,
        position: Option<[f64; 3]>,
        size: Option<[f64; 3]>,
        color: Option<&str>,
    ) -> Result<CreatedPreset, String> {
        let before = self.place_tree()?;
        let created = match self.studio.as_mut() {
            Some(PlaceBackend::Local(local)) => {
                local.create_part_preset(preset, name, position, size, color)?
            }
            Some(PlaceBackend::Mcp(_)) | Some(PlaceBackend::Plugin(_)) => {
                let mut tmp = LocalPlace::in_memory("OpenRO".to_string());
                tmp.replace_tree(&before);
                let c = tmp.create_part_preset(preset, name, position, size, color)?;
                let target_tree = tmp.tree_json();
                self.apply_place_tree(&target_tree)?;
                c
            }
            None => return Err("Studio not connected".to_string()),
        };
        let after = self.place_tree().unwrap_or_else(|_| before.clone());
        self.record_direct_edit(chat_id, &before, &after);

        let props = created.instance.get("properties");
        let mut ev = StudioActionInput::new(
            StudioActionKind::PartCreated,
            format!("Added {} ({}) to Workspace", created.path, created.class_name),
        );
        ev.path = created.path.clone();
        ev.class_name = created.class_name.clone();
        ev.position = extract_vec3_prop(props, "Position");
        ev.size = extract_vec3_prop(props, "Size");
        ev.color = extract_color_prop(props);
        self.emit_studio_action(ev);

        if let Some(sc_path) = &created.companion_script {
            let src = after
                .pointer("/scripts")
                .and_then(|s| s.get(sc_path))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mut sev = StudioActionInput::new(
                StudioActionKind::ScriptWritten,
                format!("Wired companion script {sc_path} ({} bytes)", src.len()),
            );
            sev.path = sc_path.clone();
            sev.class_name = "Script".to_string();
            sev.code_snippet = Some(snippet_from_source(src));
            self.emit_studio_action(sev);
        }
        let _ = self.auto_capture_mcp_frame();
        Ok(created)
    }

    /// Duplicate an existing Workspace instance (and any descendants) with a +[4,0,4] offset.
    pub fn duplicate_instance(
        &mut self,
        chat_id: &str,
        path: &str,
    ) -> Result<DuplicateResult, String> {
        check_path(path)?;
        let before = self.place_tree()?;
        let dup = match self.studio.as_mut() {
            Some(PlaceBackend::Local(local)) => local.duplicate_instance(path)?,
            Some(PlaceBackend::Mcp(_)) | Some(PlaceBackend::Plugin(_)) => {
                let mut tmp = LocalPlace::in_memory("OpenRO".to_string());
                tmp.replace_tree(&before);
                let d = tmp.duplicate_instance(path)?;
                let target_tree = tmp.tree_json();
                self.apply_place_tree(&target_tree)?;
                d
            }
            None => return Err("Studio not connected".to_string()),
        };
        let after = self.place_tree().unwrap_or_else(|_| before.clone());
        self.record_direct_edit(chat_id, &before, &after);

        let props = dup.instance.get("properties");
        let mut ev = StudioActionInput::new(
            StudioActionKind::PartCreated,
            format!("Duplicated {} -> {}", dup.source_path, dup.new_path),
        );
        ev.path = dup.new_path.clone();
        ev.class_name = dup.class_name.clone();
        ev.position = extract_vec3_prop(props, "Position");
        ev.size = extract_vec3_prop(props, "Size");
        ev.color = extract_color_prop(props);
        self.emit_studio_action(ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(dup)
    }

    /// Delete an instance (with descendants) or a standalone Luau script at `path`.
    pub fn delete_at_path(&mut self, chat_id: &str, path: &str) -> Result<DeleteResult, String> {
        check_path(path)?;
        let before = self.place_tree()?;
        let is_instance = before
            .pointer("/instances")
            .and_then(|m| m.as_object())
            .is_some_and(|m| m.contains_key(path));
        let is_script = before
            .pointer("/scripts")
            .and_then(|m| m.as_object())
            .is_some_and(|m| m.contains_key(path));

        if !is_instance && !is_script {
            return Err(format!("no instance or script at {path}"));
        }

        let deleted = if is_instance {
            match self.studio.as_mut() {
                Some(PlaceBackend::Local(local)) => local.delete_instance(path)?,
                Some(PlaceBackend::Mcp(_)) | Some(PlaceBackend::Plugin(_)) => {
                    let mut tmp = LocalPlace::in_memory("OpenRO".to_string());
                    tmp.replace_tree(&before);
                    let d = tmp.delete_instance(path)?;
                    let target_tree = tmp.tree_json();
                    self.apply_place_tree(&target_tree)?;
                    d
                }
                None => return Err("Studio not connected".to_string()),
            }
        } else {
            self.studio_mut()?
                .call_tool("delete_script", json!({ "path": path }))?;
            DeleteResult {
                path: path.to_string(),
                removed_instances: Vec::new(),
                removed_scripts: vec![path.to_string()],
            }
        };

        let after = self.place_tree().unwrap_or_else(|_| before.clone());
        self.record_direct_edit(chat_id, &before, &after);

        let mut ev = StudioActionInput::new(
            StudioActionKind::PartUpdated,
            format!(
                "Deleted {} ({} instance(s), {} script(s))",
                path,
                deleted.removed_instances.len(),
                deleted.removed_scripts.len()
            ),
        );
        ev.path = path.to_string();
        ev.class_name = if is_instance {
            "Instance".to_string()
        } else {
            "Script".to_string()
        };
        self.emit_studio_action(ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(deleted)
    }

    /// Save live-edited Luau source for `path` and run immediate Static Luau Prover checks.
    pub fn update_script_source(
        &mut self,
        chat_id: &str,
        path: &str,
        source: &str,
    ) -> Result<ScriptUpdateResult, String> {
        check_path(path)?;
        let before = self.place_tree()?;
        let res = match self.studio.as_mut() {
            Some(PlaceBackend::Local(local)) => local.update_script_source(path, source)?,
            Some(PlaceBackend::Mcp(_)) | Some(PlaceBackend::Plugin(_)) => {
                let mut tmp = LocalPlace::in_memory("OpenRO".to_string());
                tmp.replace_tree(&before);
                let checked = tmp.update_script_source(path, source)?;
                self.studio_mut()?
                    .call_tool("write_script", json!({ "path": path, "source": source }))?;
                checked
            }
            None => return Err("Studio not connected".to_string()),
        };
        let after = self.place_tree().unwrap_or_else(|_| before.clone());
        self.record_direct_edit(chat_id, &before, &after);

        let mut ev = StudioActionInput::new(
            StudioActionKind::ScriptWritten,
            format!(
                "Saved Luau script {path} ({} lines, {} error(s))",
                res.lines,
                res.place_report.error_count()
            ),
        );
        ev.path = path.to_string();
        ev.class_name = if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.") {
            "LocalScript".to_string()
        } else {
            "Script".to_string()
        };
        ev.code_snippet = Some(snippet_from_source(source));
        self.emit_studio_action(ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(res)
    }

    /// List the official Roblox Studio MCP tools currently usable on the active backend.
    pub fn available_studio_tools(&self) -> Vec<String> {
        match &self.studio {
            Some(PlaceBackend::Mcp(mcp)) => {
                if mcp.tools_known() {
                    OFFICIAL_STUDIO_MCP_TOOLS
                        .iter()
                        .filter(|t| mcp.has_tool(t))
                        .map(|t| (*t).to_string())
                        .collect()
                } else {
                    OFFICIAL_STUDIO_MCP_TOOLS
                        .iter()
                        .map(|t| (*t).to_string())
                        .collect()
                }
            }
            Some(PlaceBackend::Plugin(_)) => [
                "list_roblox_studios",
                "get_studio_state",
                "search_game_tree",
                "inspect_instance",
                "script_read",
                "script_grep",
                "multi_edit",
                "execute_luau",
                "get_console_output",
                "start_stop_play",
            ]
            .iter()
            .map(|t| (*t).to_string())
            .collect(),
            Some(PlaceBackend::Local(_)) => [
                "list_roblox_studios",
                "get_studio_state",
                "search_game_tree",
                "inspect_instance",
                "script_read",
                "script_grep",
                "multi_edit",
                "get_console_output",
                "start_stop_play",
            ]
            .iter()
            .map(|t| (*t).to_string())
            .collect(),
            None => Vec::new(),
        }
    }

    /// Invoke one of the 13 official Roblox Studio MCP tools (`POST /api/studio/tools/:tool`
    /// or `POST /api/studio/input/{mouse,keyboard}`), streaming matching `StudioActionEvent`
    /// records and auto-capturing a fresh PNG frame on interactive/mutating tools.
    pub fn invoke_studio_tool(
        &mut self,
        chat_id: &str,
        tool: &str,
        args: Value,
    ) -> Result<(Value, Option<u64>), String> {
        if !is_official_studio_tool(tool) {
            return Err(format!("unknown or unsupported Studio tool: {tool}"));
        }
        let args_obj = if args.is_object() { args } else { json!({}) };
        match self.studio.as_mut() {
            Some(PlaceBackend::Mcp(_)) => {
                if tool == "screen_capture" {
                    let frame = self.capture_studio_frame()?;
                    return Ok((frame.meta_json(), Some(frame.seq)));
                }
                if tool == "get_console_output" {
                    let lines = match self.studio.as_mut() {
                        Some(PlaceBackend::Mcp(mcp)) => mcp.get_console_output()?,
                        _ => Vec::new(),
                    };
                    return Ok((
                        json!({
                            "ok": true,
                            "lines": lines,
                            "console": lines.join("\n"),
                        }),
                        None,
                    ));
                }
                if tool == "start_stop_play" {
                    let mode = args_obj
                        .get("mode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("start_play")
                        .to_string();
                    let val = match self.studio.as_mut() {
                        Some(PlaceBackend::Mcp(mcp)) => mcp.start_stop_play(&mode)?,
                        _ => return Err("Studio not connected".to_string()),
                    };
                    let fseq = self.auto_capture_mcp_frame();
                    let mut ev = StudioActionInput::new(
                        if mode == "stop" {
                            StudioActionKind::PlaytestPassed
                        } else {
                            StudioActionKind::PlaytestStarted
                        },
                        format!("Studio start_stop_play({mode})"),
                    );
                    ev.path = "Workspace".to_string();
                    ev.class_name = "RunService".to_string();
                    ev.frame_seq = fseq;
                    self.emit_studio_action(ev);
                    return Ok((val, fseq));
                }

                let parsed = match self.studio.as_mut() {
                    Some(PlaceBackend::Mcp(mcp)) => match tool {
                        "user_mouse_input" => crate::studio_embed::forward_mouse(mcp, &args_obj)?,
                        "user_keyboard_input" => {
                            crate::studio_embed::forward_keyboard(mcp, &args_obj)?
                        }
                        _ => mcp.call_official_tool(tool, args_obj.clone())?,
                    },
                    _ => return Err("Studio not connected".to_string()),
                };

                let captures_frame = matches!(
                    tool,
                    "multi_edit" | "execute_luau" | "user_mouse_input" | "user_keyboard_input"
                );
                let fseq = if captures_frame {
                    self.auto_capture_mcp_frame()
                } else {
                    None
                };

                let (kind, path, class_name, summary) = match tool {
                    "execute_luau" => (
                        StudioActionKind::LuauExecuted,
                        "ServerScriptService".to_string(),
                        "Script".to_string(),
                        "Executed Luau inside live Roblox Studio".to_string(),
                    ),
                    "multi_edit" => (
                        StudioActionKind::ScriptWritten,
                        args_obj
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("ServerScriptService")
                            .to_string(),
                        "Script".to_string(),
                        "Applied multi_edit to Studio script".to_string(),
                    ),
                    "user_mouse_input" => (
                        StudioActionKind::CameraFocus,
                        "Workspace.CurrentCamera".to_string(),
                        "Camera".to_string(),
                        format!(
                            "Mouse {} ({}, {}) in Embedded Studio",
                            args_obj
                                .get("action")
                                .and_then(|v| v.as_str())
                                .unwrap_or("click"),
                            args_obj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            args_obj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        ),
                    ),
                    "user_keyboard_input" => (
                        StudioActionKind::CameraFocus,
                        "Workspace.CurrentCamera".to_string(),
                        "UserInputService".to_string(),
                        format!(
                            "Keyboard {} in Embedded Studio",
                            args_obj
                                .get("key")
                                .or_else(|| args_obj.get("text"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("input")
                        ),
                    ),
                    _ => (
                        StudioActionKind::CameraFocus,
                        "Workspace".to_string(),
                        "StudioService".to_string(),
                        format!("Called Roblox Studio tool {tool}"),
                    ),
                };
                if captures_frame {
                    let mut ev = StudioActionInput::new(kind, summary);
                    ev.path = path;
                    ev.class_name = class_name;
                    ev.frame_seq = fseq;
                    if tool == "execute_luau" {
                        if let Some(code) = args_obj
                            .get("code")
                            .or_else(|| args_obj.get("source"))
                            .and_then(|v| v.as_str())
                        {
                            ev.code_snippet = Some(snippet_from_source(code));
                        }
                    }
                    self.emit_studio_action(ev);
                }
                Ok((parsed, fseq))
            }
            Some(PlaceBackend::Plugin(_)) => self.invoke_plugin_studio_tool(chat_id, tool, &args_obj),
            Some(PlaceBackend::Local(_)) => self.invoke_local_studio_tool(chat_id, tool, &args_obj),
            None => Err("Studio not connected".to_string()),
        }
    }

    fn invoke_plugin_studio_tool(
        &mut self,
        chat_id: &str,
        tool: &str,
        args: &Value,
    ) -> Result<(Value, Option<u64>), String> {
        match tool {
            "screen_capture" | "user_mouse_input" | "user_keyboard_input" => Err(format!(
                "Tool '{tool}' requires Embedded Studio / MCP (Companion Plugin uses Studio's native viewport)."
            )),
            "execute_luau" | "get_console_output" | "start_stop_play" => {
                let text = self.studio_mut()?.call_tool(tool, args.clone())?;
                let parsed = serde_json::from_str::<Value>(&text)
                    .unwrap_or_else(|_| json!({ "ok": true, "output": text }));
                Ok((parsed, None))
            }
            "list_roblox_studios" => Ok((
                json!({ "studios": ["companion_plugin"], "studio_mode": "plugin_live" }),
                None,
            )),
            "get_studio_state" => {
                let tree = self.place_tree()?;
                let ic = tree.get("instances").and_then(|v| v.as_object()).map(|m| m.len()).unwrap_or(0);
                let sc = tree.get("scripts").and_then(|v| v.as_object()).map(|m| m.len()).unwrap_or(0);
                Ok((
                    json!({
                        "studio_mode": "plugin_live",
                        "datamodel_type": "Edit",
                        "is_playing": false,
                        "instance_count": ic,
                        "script_count": sc,
                    }),
                    None,
                ))
            }
            "search_game_tree" | "inspect_instance" | "script_read" | "script_grep" => {
                let tree = self.place_tree()?;
                let mut tmp = LocalPlace::in_memory("OpenRO".to_string());
                tmp.replace_tree(&tree);
                let mut helper = Agent::with_local_place(self.model.clone(), tmp);
                helper.invoke_local_studio_tool(chat_id, tool, args)
            }
            "multi_edit" => {
                let path = args
                    .get("path")
                    .or_else(|| args.get("file_path"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "multi_edit requires string 'path'".to_string())?;
                let edits = args
                    .get("edits")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| "multi_edit requires array 'edits'".to_string())?;
                let mut src = self
                    .prev_script_current(path)?
                    .ok_or_else(|| format!("no script at {path}"))?;
                let mut applied = 0usize;
                for ed in edits {
                    let old_s = ed
                        .get("old_str")
                        .or_else(|| ed.get("old_text"))
                        .or_else(|| ed.get("find"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "each edit requires 'old_str'".to_string())?;
                    let new_s = ed
                        .get("new_str")
                        .or_else(|| ed.get("new_text"))
                        .or_else(|| ed.get("replace"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "each edit requires 'new_str'".to_string())?;
                    if !src.contains(old_s) {
                        return Err(format!("multi_edit target not found in {path}"));
                    }
                    src = src.replacen(old_s, new_s, 1);
                    applied += 1;
                }
                let updated = self.update_script_source(chat_id, path, &src)?;
                let mut out = updated.to_json();
                if let Some(obj) = out.as_object_mut() {
                    obj.insert("edits_applied".to_string(), json!(applied));
                }
                Ok((out, None))
            }
            other => Err(format!("unsupported Studio tool: {other}")),
        }
    }

    fn invoke_local_studio_tool(
        &mut self,
        chat_id: &str,
        tool: &str,
        args: &Value,
    ) -> Result<(Value, Option<u64>), String> {
        match tool {
            "screen_capture" | "user_mouse_input" | "user_keyboard_input" | "execute_luau" => Err(
                format!(
                    "Tool '{tool}' requires a connected live Roblox Studio MCP session (currently running Local Place Engine). Click Launch & Embed Studio or Connect MCP."
                ),
            ),
            "list_roblox_studios" => Ok((
                json!({
                    "studios": [],
                    "studio_mode": "local_place",
                    "note": "Running Local Place Engine; launch or connect Roblox Studio MCP to attach live instances."
                }),
                None,
            )),
            "get_studio_state" => {
                let Some(PlaceBackend::Local(local)) = self.studio.as_ref() else {
                    return Err("Studio not connected".to_string());
                };
                Ok((
                    json!({
                        "studio_mode": "local_place",
                        "datamodel_type": "Edit",
                        "is_playing": false,
                        "game_name": local.game_name(),
                        "instance_count": local.instances().len(),
                        "script_count": local.scripts().len(),
                    }),
                    None,
                ))
            }
            "search_game_tree" => {
                let Some(PlaceBackend::Local(local)) = self.studio.as_ref() else {
                    return Err("Studio not connected".to_string());
                };
                let q = args
                    .get("query")
                    .or_else(|| args.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                let class_filter = args
                    .get("class_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty());
                let mut matches = Vec::new();
                for (path, rec) in local.instances() {
                    let cls = rec
                        .get("class_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Instance");
                    if !q.is_empty() && !path.to_ascii_lowercase().contains(&q) {
                        continue;
                    }
                    if let Some(cf) = &class_filter {
                        if cls.to_ascii_lowercase() != *cf {
                            continue;
                        }
                    }
                    matches.push(json!({
                        "path": path,
                        "name": path.rsplit('.').next().unwrap_or(path),
                        "class_name": cls,
                        "kind": "instance",
                    }));
                }
                for (path, src) in local.scripts() {
                    let cls = if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.")
                    {
                        "LocalScript"
                    } else if path.starts_with("ReplicatedStorage.") {
                        "ModuleScript"
                    } else {
                        "Script"
                    };
                    if !q.is_empty() && !path.to_ascii_lowercase().contains(&q) {
                        continue;
                    }
                    if let Some(cf) = &class_filter {
                        if cls.to_ascii_lowercase() != *cf {
                            continue;
                        }
                    }
                    matches.push(json!({
                        "path": path,
                        "name": path.rsplit('.').next().unwrap_or(path),
                        "class_name": cls,
                        "kind": "script",
                        "lines": src.lines().count(),
                    }));
                }
                Ok((json!({ "count": matches.len(), "matches": matches }), None))
            }
            "inspect_instance" => {
                let Some(PlaceBackend::Local(local)) = self.studio.as_ref() else {
                    return Err("Studio not connected".to_string());
                };
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "inspect_instance requires string 'path'".to_string())?;
                if let Some(rec) = local.instances().get(path) {
                    let prefix = format!("{path}.");
                    let children: Vec<String> = local
                        .instances()
                        .keys()
                        .chain(local.scripts().keys())
                        .filter(|k| k.starts_with(&prefix))
                        .cloned()
                        .collect();
                    return Ok((
                        json!({
                            "path": path,
                            "kind": "instance",
                            "class_name": rec.get("class_name").and_then(|v| v.as_str()).unwrap_or("Instance"),
                            "properties": rec.get("properties").cloned().unwrap_or_else(|| json!({})),
                            "children": children,
                        }),
                        None,
                    ));
                }
                if let Some(src) = local.scripts().get(path) {
                    return Ok((
                        json!({
                            "path": path,
                            "kind": "script",
                            "class_name": if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.") { "LocalScript" } else { "Script" },
                            "lines": src.lines().count(),
                            "bytes": src.len(),
                            "source": src,
                        }),
                        None,
                    ));
                }
                Err(format!("no instance or script at {path}"))
            }
            "script_read" => {
                let Some(PlaceBackend::Local(local)) = self.studio.as_ref() else {
                    return Err("Studio not connected".to_string());
                };
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "script_read requires string 'path'".to_string())?;
                let src = local
                    .scripts()
                    .get(path)
                    .ok_or_else(|| format!("no script at {path}"))?;
                let all_lines: Vec<&str> = src.lines().collect();
                let start = args
                    .get("start_line")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1)
                    .max(1) as usize;
                let end = args
                    .get("end_line")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize)
                    .unwrap_or(all_lines.len())
                    .min(all_lines.len());
                let slice = if start <= end && start >= 1 {
                    all_lines[start - 1..end].join("\n")
                } else {
                    String::new()
                };
                Ok((
                    json!({
                        "path": path,
                        "total_lines": all_lines.len(),
                        "start_line": start,
                        "end_line": end,
                        "source": slice,
                    }),
                    None,
                ))
            }
            "script_grep" => {
                let Some(PlaceBackend::Local(local)) = self.studio.as_ref() else {
                    return Err("Studio not connected".to_string());
                };
                let needle = args
                    .get("query")
                    .or_else(|| args.get("pattern"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "script_grep requires string 'query' or 'pattern'".to_string())?;
                let low = needle.to_ascii_lowercase();
                let mut hits = Vec::new();
                for (path, src) in local.scripts() {
                    for (idx, line) in src.lines().enumerate() {
                        if line.to_ascii_lowercase().contains(&low) {
                            hits.push(json!({
                                "path": path,
                                "line": idx + 1,
                                "text": line,
                            }));
                            if hits.len() >= 200 {
                                break;
                            }
                        }
                    }
                    if hits.len() >= 200 {
                        break;
                    }
                }
                Ok((
                    json!({
                        "query": needle,
                        "count": hits.len(),
                        "matches": hits,
                    }),
                    None,
                ))
            }
            "multi_edit" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "multi_edit requires string 'path'".to_string())?;
                let edits = args
                    .get("edits")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| "multi_edit requires array 'edits'".to_string())?;
                let mut src = match self.studio.as_ref() {
                    Some(PlaceBackend::Local(local)) => local
                        .scripts()
                        .get(path)
                        .cloned()
                        .ok_or_else(|| format!("no script at {path}"))?,
                    _ => return Err("Studio not connected".to_string()),
                };
                let mut applied = 0usize;
                for ed in edits {
                    let old_s = ed
                        .get("old_str")
                        .or_else(|| ed.get("old_text"))
                        .or_else(|| ed.get("find"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "each edit requires 'old_str'".to_string())?;
                    let new_s = ed
                        .get("new_str")
                        .or_else(|| ed.get("new_text"))
                        .or_else(|| ed.get("replace"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "each edit requires 'new_str'".to_string())?;
                    if !src.contains(old_s) {
                        return Err(format!("multi_edit target not found in {path}"));
                    }
                    src = src.replacen(old_s, new_s, 1);
                    applied += 1;
                }
                let updated = self.update_script_source(chat_id, path, &src)?;
                let mut out = updated.to_json();
                if let Some(obj) = out.as_object_mut() {
                    obj.insert("edits_applied".to_string(), json!(applied));
                }
                Ok((out, None))
            }
            "get_console_output" => {
                let console = self
                    .studio_mut()?
                    .call_tool("get_console", json!({}))
                    .unwrap_or_default();
                let lines: Vec<String> = console
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
                Ok((json!({ "lines": lines, "console": lines.join("\n") }), None))
            }
            "start_stop_play" => {
                let mode = args
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("start_play");
                if mode == "stop" {
                    return Ok((json!({ "ok": true, "mode": "stop", "status": "stopped" }), None));
                }
                let res = self.run_studio_playtest()?;
                Ok((res, None))
            }
            other => Err(format!("unsupported Studio tool: {other}")),
        }
    }

    /// Bind uploaded/generated game card art assets into the active LocalPlace's
    /// `Workspace.CardGallery` as 3D Showcase Parts with `Decal` (`Texture`) and
    /// `SurfaceGui` -> `ImageLabel` (`Image`) instances for `.rbxlx` export.
    pub fn bind_assets(&mut self, assets: &[crate::assets::AssetMetadata]) -> usize {
        let count = match self.studio.as_mut() {
            Some(PlaceBackend::Local(place)) => crate::assets::bind_assets_to_place(place, assets),
            _ => 0,
        };
        if count > 0 && self.studio_stream.is_some() {
            let total = assets.len();
            for (idx, asset) in assets.iter().enumerate() {
                let safe_id = asset.card_id.replace('-', "_");
                let x_pos = ((idx as f64) - ((total as f64 - 1.0) / 2.0)) * 9.0;
                let mut ev = StudioActionInput::new(
                    StudioActionKind::DecalBound,
                    format!("Bound {} ({}) onto 3D CardGallery showcase", asset.title, asset.filename),
                );
                ev.path = format!("Workspace.CardGallery.Showcase_{safe_id}.CardArtDecal");
                ev.class_name = "Decal".to_string();
                ev.position = Some([x_pos, 5.0, 28.0]);
                ev.size = Some([6.0, 8.5, 0.8]);
                ev.color = Some("#4F7CF0".to_string());
                self.emit_studio_action(ev);
            }
        }
        count
    }

    /// Create or update a single Instance at `path` (e.g. `Workspace.JumpPad`),
    /// merging new properties onto any existing properties and preserving its
    /// existing `class_name` when `class_name` is `None`.
    pub fn set_instance_properties(
        &mut self,
        path: &str,
        class_name: Option<&str>,
        properties: &Value,
    ) -> Result<Value, String> {
        check_path(path)?;
        let (parent, name) = path
            .rsplit_once('.')
            .ok_or_else(|| format!("instance path must be Parent.Name (got '{path}')"))?;
        if !properties.is_object() {
            return Err("properties must be a JSON object".to_string());
        }
        let existing = self.prev_instance(path)?;
        let resolved_class = class_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|v| v.get("class_name").and_then(|c| c.as_str()).map(String::from))
            })
            .unwrap_or_else(|| "Part".to_string());
        let mut merged_props = existing
            .as_ref()
            .and_then(|v| v.get("properties").and_then(|p| p.as_object()).cloned())
            .unwrap_or_default();
        if let Some(new_obj) = properties.as_object() {
            for (k, v) in new_obj {
                merged_props.insert(k.clone(), v.clone());
            }
        }
        let props_val = Value::Object(merged_props);
        self.studio_mut()?.call_tool(
            "create_instance",
            json!({
                "class_name": resolved_class,
                "name": name,
                "parent": parent,
                "properties": props_val,
            }),
        )?;
        let kind = if existing.is_some() {
            StudioActionKind::PartUpdated
        } else {
            StudioActionKind::PartCreated
        };
        let mut ev = StudioActionInput::new(
            kind,
            if existing.is_some() {
                format!("Updated {path} ({resolved_class})")
            } else {
                format!("Created {path} ({resolved_class})")
            },
        );
        ev.path = path.to_string();
        ev.class_name = resolved_class.clone();
        ev.position = extract_vec3_prop(Some(&props_val), "Position");
        ev.size = extract_vec3_prop(Some(&props_val), "Size");
        ev.color = extract_color_prop(Some(&props_val));
        self.emit_studio_action(ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(json!({
            "path": path,
            "class_name": resolved_class,
            "properties": props_val,
        }))
    }

    /// Delete an Instance at `path` cleanly.
    pub fn delete_instance_at(&mut self, path: &str) -> Result<(), String> {
        check_path(path)?;
        self.studio_mut()?
            .call_tool("delete_instance", json!({ "path": path }))?;
        let mut ev = StudioActionInput::new(
            StudioActionKind::PartUpdated,
            format!("Removed {path} from Place"),
        );
        ev.path = path.to_string();
        ev.class_name = "Instance".to_string();
        self.emit_studio_action(ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(())
    }

    /// Replace the active Place tree with `target` (`{"instances": {...}, "scripts": {...}}`),
    /// using an O(N) atomic bulk replace for `LocalPlace` or exact delta calls for `Mcp`,
    /// and returning the computed `PlaceDiff`.
    pub fn apply_place_tree(&mut self, target: &Value) -> Result<PlaceDiff, String> {
        let before = self.place_tree()?;
        let diff = compute_diff(&before, target);
        if let Some(PlaceBackend::Local(local)) = self.studio.as_mut() {
            local.replace_tree(target);
            return Ok(diff);
        }
        let empty = serde_json::Map::new();
        let target_instances = target
            .get("instances")
            .and_then(|v| v.as_object())
            .unwrap_or(&empty);
        let target_scripts = target
            .get("scripts")
            .and_then(|v| v.as_object())
            .unwrap_or(&empty);

        for path in &diff.removed_scripts {
            self.studio_mut()?
                .call_tool("delete_script", json!({ "path": path }))?;
        }
        for path in &diff.removed_instances {
            self.studio_mut()?
                .call_tool("delete_instance", json!({ "path": path }))?;
        }
        for path in diff
            .added_instances
            .iter()
            .chain(diff.modified_instances.iter())
        {
            check_path(path)?;
            let Some((parent, name)) = path.rsplit_once('.') else {
                continue;
            };
            let rec = target_instances.get(path);
            let class_name = rec
                .and_then(|r| r.get("class_name").and_then(|c| c.as_str()))
                .unwrap_or("Part");
            let properties = rec
                .and_then(|r| r.get("properties"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            self.studio_mut()?.call_tool(
                "create_instance",
                json!({
                    "class_name": class_name,
                    "name": name,
                    "parent": parent,
                    "properties": properties,
                }),
            )?;
        }
        for path in diff.added_scripts.iter().chain(diff.modified_scripts.iter()) {
            check_path(path)?;
            let src = target_scripts
                .get(path)
                .and_then(|s| s.as_str())
                .unwrap_or("");
            self.studio_mut()?
                .call_tool("write_script", json!({ "path": path, "source": src }))?;
        }
        Ok(diff)
    }

    /// Refuse honestly when the connected bridge is known to lack a
    /// tool the agent cannot build without. Accepts:
    /// - Persistent `LocalPlace` or connected Companion Studio `Plugin`,
    /// - Official Roblox Studio MCP (`execute_luau` + `start_stop_play` + `get_console_output`), or
    /// - Legacy 7-tool Studio MCP bridges.
    fn check_bridge_capabilities(&self) -> Result<(), String> {
        let Some(studio) = &self.studio else {
            return Ok(());
        };
        if matches!(studio, PlaceBackend::Local(_) | PlaceBackend::Plugin(_)) {
            return Ok(());
        }
        if !studio.tools_known() {
            return Ok(());
        }
        const REQUIRED: [&str; 7] = [
            "write_script",
            "delete_script",
            "create_instance",
            "delete_instance",
            "playtest",
            "get_console",
            "get_place_tree",
        ];
        if let PlaceBackend::Mcp(mcp) = studio {
            let advertises_any_legacy = REQUIRED.iter().any(|t| mcp.has_exact_tool(t));
            if mcp.has_official_bridge() && !advertises_any_legacy {
                return Ok(());
            }
        }
        let missing: Vec<&str> = REQUIRED
            .iter()
            .filter(|t| !studio.has_tool(t))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "Studio bridge does not offer these tools: {} — this bridge cannot build. \
                 Connect a Studio MCP server that offers {} (or official execute_luau + start_stop_play + get_console_output).",
                missing.join(", "),
                REQUIRED.join(", ")
            ));
        }
        Ok(())
    }

    /// Run one ask in one chat (default `Agent` mode). Whatever happens —
    /// success, honest failure, mid-ask error — the changes this ask checkpointed
    /// are harvested into that chat's revertable change set.
    pub fn run_ask(&mut self, chat_id: &str, ask: &str) -> Result<RunOutcome, AskFailure> {
        self.run_ask_with_options(chat_id, ask, AskOptions::default())
    }

    /// Run one ask with explicit RoPilot composer options (`mode`, `reasoning`, `context_paths`).
    pub fn run_ask_with_options(
        &mut self,
        chat_id: &str,
        ask: &str,
        opts: AskOptions,
    ) -> Result<RunOutcome, AskFailure> {
        self.current_changes.clear();
        self.begin_build_trace(chat_id, ask);
        let result = self.run_ask_inner(chat_id, ask, &opts, false);
        match &result {
            Ok(o) if o.refused => self.emit_event("failed", "Build refused", &o.summary, None, None, 100),
            Ok(o) => self.emit_event("done", "Build verified", &o.summary, None, None, 100),
            Err(f) => self.emit_event("failed", "Build failed", &f.message, None, None, 100),
        }
        if opts.mode != AskMode::Ask {
            let skills = match &result {
                Ok(o) => o.skills_used.clone(),
                Err(f) => f.skills_used.clone(),
            };
            self.harvest(chat_id, skills);
        } else {
            self.current_changes.clear();
        }
        result
    }

    /// Run an ask in isolated staged/proposal mode (`mode == "plan"` or `review_diffs == true`):
    /// clones the active place tree into a shadow `LocalPlace`, runs all agent steps against the
    /// shadow copy (ZERO live Studio mutation!), computes Static Luau Prover diagnostics on the
    /// staged tree, and returns before/after trees for unified diff review & selective apply.
    pub fn run_ask_staged(
        &mut self,
        chat_id: &str,
        ask: &str,
        opts: AskOptions,
    ) -> Result<StagedAskOutcome, AskFailure> {
        let fail = |message: String| AskFailure {
            message,
            thread: Vec::new(),
            transcript: Vec::new(),
            skills_used: Vec::new(),
        };
        if !self.model.is_configured() {
            return Err(fail(
                "no model key configured: set OPENRO_MODEL_API_KEY to enable chat".to_string(),
            ));
        }
        if self.studio.is_none() {
            return Err(fail(
                "Studio not connected: set OPENRO_MCP_COMMAND to a Studio MCP server".to_string(),
            ));
        }
        if let Err(e) = self.check_bridge_capabilities() {
            return Err(fail(e));
        }
        let before_tree = self.place_tree().map_err(fail)?;
        let game_name = match &self.studio {
            Some(PlaceBackend::Local(l)) => l.game_name().to_string(),
            _ => "OpenRO".to_string(),
        };
        let mut shadow = LocalPlace::in_memory(game_name);
        shadow.replace_tree(&before_tree);

        let mut shadow_agent = Agent::with_local_place(self.model.clone(), shadow);
        shadow_agent.set_library_prompt(self.library_prompt.clone());
        shadow_agent.set_market_dossier_prompt(self.market_dossier_prompt.clone());
        if let Some(root) = &self.assets_root {
            shadow_agent.set_assets_root(root.clone());
        }
        if let Some(ev) = &self.events {
            shadow_agent.attach_events(Arc::clone(ev));
        }
        shadow_agent.begin_build_trace(chat_id, ask);
        let outcome = shadow_agent.run_ask_inner(chat_id, ask, &opts, true)?;
        let (after_tree, diagnostics) = match shadow_agent.studio.as_ref() {
            Some(PlaceBackend::Local(local)) => (local.tree_json(), local.analyze().to_json()),
            _ => (
                before_tree.clone(),
                json!({ "ok": true, "error_count": 0, "warning_count": 0, "diagnostics": [] }),
            ),
        };
        shadow_agent.emit_event("done", "Proposal ready", &outcome.summary, None, None, 100);
        Ok(StagedAskOutcome {
            outcome,
            before_tree,
            after_tree,
            diagnostics,
        })
    }

    /// Push this ask's checkpoints (if it changed anything) onto the
    /// chat's change-set stack, with the skills it ran.
    fn harvest(&mut self, chat_id: &str, skills: Vec<String>) {
        let harvested = std::mem::take(&mut self.current_changes);
        if !harvested.is_empty() {
            self.changes_by_chat
                .entry(chat_id.to_string())
                .or_default()
                .push(ChangeSet { changes: harvested, skills });
            self.redo_by_chat.remove(chat_id);
            self.save_changes();
            self.sync_undo();
        }
    }

    /// Skills the chat's newest change set ran (what a revert takes back).
    pub fn top_skills(&self, chat_id: &str) -> Vec<String> {
        self.changes_by_chat
            .get(chat_id)
            .and_then(|stack| stack.last())
            .map(|set| set.skills.clone())
            .unwrap_or_default()
    }

    /// Skills still held by the chat's remaining change sets.
    pub fn chat_skills(&self, chat_id: &str) -> Vec<String> {
        self.changes_by_chat
            .get(chat_id)
            .map(|stack| stack.iter().flat_map(|set| set.skills.clone()).collect())
            .unwrap_or_default()
    }

    /// Build one of the creativity engine's ideas by id: the idea's
    /// built-in plan runs through `run_plan`.
    pub fn build_idea(&mut self, chat_id: &str, idea_id: &str) -> Result<RunOutcome, AskFailure> {
        let idea = crate::ideas::find(idea_id).ok_or_else(|| AskFailure {
            message: format!("unknown idea: {idea_id}"),
            thread: Vec::new(),
            transcript: Vec::new(),
            skills_used: Vec::new(),
        })?;
        self.run_plan(chat_id, idea.title, &idea.plan)
    }

    /// Run a built-in plan (from the creativity engine) deterministically:
    /// the same step machinery as a model ask — skills generate, tools
    /// apply, changes checkpoint — but no model is consulted, and the
    /// thread says so. The done gate is identical: the plan is only
    /// called built when a playtest has passed since its last change;
    /// a failed playtest ends the build in an honest failure naming
    /// the console output, change set still revertable. Change-set
    /// harvesting matches `run_ask`: whatever the plan changed belongs
    /// to the chat, on every exit path.
    pub fn run_plan(
        &mut self,
        chat_id: &str,
        label: &str,
        steps: &[Value],
    ) -> Result<RunOutcome, AskFailure> {
        self.current_changes.clear();
        self.begin_build_trace(chat_id, label);
        let result = self.run_plan_inner(chat_id, label, steps);
        // Every failed idea build says Undo last build when any of its
        // changes landed (the playtest message already says it).
        // The pointer only when this plan's own changes landed; a try that
        // added nothing must not send the user to undo the previous build.
        let result = match result {
            Err(mut f) if !f.message.contains("Undo last build") && !f.message.contains("Nothing from this try") => {
                if !f.message.ends_with('.') {
                    f.message.push('.');
                }
                if self.current_changes.iter().any(|c| !c.is_net_zero()) {
                    f.message.push_str(" Undo last build undoes what it changed.");
                } else {
                    f.message.push_str(" Nothing from this try was added to your game.");
                }
                Err(f)
            }
            other => other,
        };
        match &result {
            Ok(o) => self.emit_event("done", "Blueprint verified", &o.summary, None, None, 100),
            Err(f) => self.emit_event("failed", "Blueprint stopped", &f.message, None, None, 100),
        }
        let skills = match &result {
            Ok(o) => o.skills_used.clone(),
            Err(f) => f.skills_used.clone(),
        };
        self.harvest(chat_id, skills);
        result
    }

    /// Record a playtest verdict for the chat's place.
    fn record_verdict(&mut self, chat_id: &str, outcome: &PlaytestOutcome) {
        let proof = self.proof_by_chat.entry(chat_id.to_string()).or_default();
        match outcome {
            PlaytestOutcome::Passed(_) => {
                proof.unproven = false;
                proof.failed_console = None;
            }
            PlaytestOutcome::Failed(_, console) => {
                proof.unproven = true;
                proof.failed_console = Some(console.clone());
            }
            PlaytestOutcome::Unreadable(..) => {
                proof.unproven = true;
                proof.failed_console = None;
            }
        }
        self.save_proof();
    }

    /// A change landed in the chat's place: it is unproven until a pass.
    fn note_change(&mut self, chat_id: &str) {
        self.proof_by_chat.entry(chat_id.to_string()).or_default().unproven = true;
        self.save_proof();
    }

    fn run_plan_inner(
        &mut self,
        chat_id: &str,
        label: &str,
        steps: &[Value],
    ) -> Result<RunOutcome, AskFailure> {
        let mut thread: Vec<String> = Vec::new();
        let mut transcript: Vec<String> = Vec::new();
        let mut skills_used: Vec<String> = Vec::new();
        let fail = |message: String,
                    thread: &Vec<String>,
                    transcript: &Vec<String>,
                    skills_used: &Vec<String>| AskFailure {
            message,
            thread: thread.clone(),
            transcript: transcript.clone(),
            skills_used: skills_used.clone(),
        };
        if self.studio.is_none() {
            return Err(fail(
                "Studio not connected: set OPENRO_MCP_COMMAND to a Studio MCP server".to_string(),
                &thread, &transcript, &skills_used));
        }
        if let Err(e) = self.check_bridge_capabilities() {
            return Err(fail(e, &thread, &transcript, &skills_used));
        }
        self.generated.clear();
        self.last_skill = None;
        thread.push(
            "OpenRO: Building it now — these are my built-in steps, no model call involved."
                .to_string(),
        );
        transcript.push(format!("built-in plan: {label} ({} steps)", steps.len()));
        let mut writes: HashMap<String, String> = HashMap::new();
        let mut changes_made = 0usize;
        let mut unproven_changes = false;

        // One playtest, plan semantics: a pass clears the done gate; a
        // failure ends the build honestly (a built-in plan has no
        // repairer — the failure names the console and the changes
        // stay revertable).
        macro_rules! plan_playtest {
            () => {
                match self.run_playtest(&mut transcript, &mut thread) {
                    Ok(outcome) => {
                        self.record_verdict(chat_id, &outcome);
                        match outcome {
                            PlaytestOutcome::Passed(_) => {
                                unproven_changes = false;
                            }
                            PlaytestOutcome::Failed(_, console) => {
                                // run_playtest narrated "reading the console
                                // and repairing" — a plan does not repair, so
                                // correct the line before it misleads.
                                if let Some(last) = thread.last_mut() {
                                    if last.starts_with("Playtest failed") {
                                        *last = "Playtest failed.".to_string();
                                    }
                                }
                                return Err(fail(
                                    format!(
                                        "my built-in plan for “{label}” failed its playtest — nothing \
                                         is called done. Console output: {console} Undo last build \
                                         undoes what it changed."
                                    ),
                                    &thread, &transcript, &skills_used));
                            }
                            PlaytestOutcome::Unreadable(_, raw) => {
                                return Err(fail(
                                    format!(
                                        "my built-in plan for “{label}” got a playtest result that \
                                         could not be read as a verdict ({raw}) — the build counts \
                                         as unproven, not done"
                                    ),
                                    &thread, &transcript, &skills_used));
                            }
                        }
                    }
                    Err(e) => {
                        transcript.push(format!("bridge detail: {e}"));
                        return Err(fail(plain_bridge(&e), &thread, &transcript, &skills_used));
                    }
                }
            };
        }

        let mut skipped = 0usize;
        for step in steps {
            if let Some(skill) = step.get("skill").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or(json!({}));
                match self.run_skill(skill, &args, &mut transcript, &mut thread) {
                    Ok(_) => skills_used.push(skill.to_string()),
                    Err(e) => {
                        return Err(fail(
                            format!("my built-in plan for “{label}” stopped while generating a step: {e}"),
                            &thread, &transcript, &skills_used))
                    }
                }
                continue;
            }
            if let Some(tool) = step.get("tool").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or(json!({}));
                if tool == "playtest" {
                    plan_playtest!();
                    continue;
                }
                if tool == "write_script" {
                    match self.run_write_script(
                        &args,
                        &mut transcript,
                        &mut thread,
                        &mut writes,
                        None,
                    ) {
                        Ok(_) => {
                            changes_made += 1;
                            unproven_changes = true;
                            self.note_change(chat_id);
                        }
                        Err(e) => {
                            transcript.push(format!("bridge detail: {e}"));
                            return Err(fail(
                                format!("built-in plan stopped at write_script: {}", plain_bridge(&e)),
                                &thread, &transcript, &skills_used))
                        }
                    }
                    continue;
                }
                // An instance the place already holds with the same
                // class is a success: the step is skipped, not failed.
                if tool == "create_instance" {
                    match self.already_in_place(&args) {
                        Ok(true) => {
                            skipped += 1;
                            transcript.push(format!("create_instance skipped: {args} already in place"));
                            continue;
                        }
                        Ok(false) => {}
                        Err(e) => return Err(fail(e, &thread, &transcript, &skills_used)),
                    }
                }
                match self.run_tool(tool, &args, &mut transcript, &mut thread) {
                    Ok(_) => {
                        if matches!(
                            tool,
                            "delete_script" | "create_instance" | "delete_instance"
                        ) {
                            changes_made += 1;
                            unproven_changes = true;
                            self.note_change(chat_id);
                        }
                    }
                    Err(e) => {
                        transcript.push(format!("bridge detail: {e}"));
                        return Err(fail(
                            format!("my built-in plan for “{label}” stopped while saving to the game: {}", plain_bridge(&e)),
                            &thread, &transcript, &skills_used))
                    }
                }
                continue;
            }
            return Err(fail(
                format!("built-in plan held an unreadable step: {step}"),
                &thread, &transcript, &skills_used));
        }

        if changes_made == 0 && skipped > 0 {
            return Err(fail(
                format!("my built-in plan for “{label}” is already in your game, so there was nothing to add."),
                &thread, &transcript, &skills_used));
        }
        if changes_made == 0 {
            return Err(fail(
                format!(
                    "my built-in plan for “{label}” made no changes — the plan is broken, \
                     and I won't pretend otherwise"
                ),
                &thread, &transcript, &skills_used));
        }
        if unproven_changes {
            plan_playtest!();
            debug_assert!(!unproven_changes);
        }
        // A plan whose changes cancel out (created then deleted,
        // rewritten identically) built nothing — the model path flags
        // the equivalent claim with a factual line; a plan is held to
        // the same standard and fails honestly instead.
        if self.current_changes.iter().all(|c| c.is_net_zero()) {
            return Err(fail(
                format!(
                    "my built-in plan for “{label}” left the game exactly as it found it — \
                     nothing was actually built, and I won't call it built"
                ),
                &thread,
                &transcript,
                &skills_used,
            ));
        }
        /* The verdict band directly above already states the pass —
           the summary names the build, once (elevation checker note). */
        let summary = format!("Built “{label}”.");
        thread.push(format!("OpenRO: {summary}"));
        transcript.push(format!("plan done: {summary}"));
        Ok(RunOutcome {
            summary,
            thread,
            transcript,
            skills_used,
            refused: false,
            changed: true,
        })
    }

    fn run_ask_inner(
        &mut self,
        chat_id: &str,
        ask: &str,
        opts: &AskOptions,
        staged: bool,
    ) -> Result<RunOutcome, AskFailure> {
        let read_only = opts.mode == AskMode::Ask;
        let mut thread: Vec<String> = Vec::new();
        let mut transcript: Vec<String> = Vec::new();
        let mut skills_used: Vec<String> = Vec::new();
        let fail = |message: String,
                    thread: &Vec<String>,
                    transcript: &Vec<String>,
                    skills_used: &Vec<String>| AskFailure {
            message,
            thread: thread.clone(),
            transcript: transcript.clone(),
            skills_used: skills_used.clone(),
        };
        if !self.model.is_configured() {
            return Err(fail(
                "no model key configured: set OPENRO_MODEL_API_KEY to enable chat".to_string(),
                &thread, &transcript, &skills_used));
        }
        if self.studio.is_none() {
            return Err(fail(
                "Studio not connected: set OPENRO_MCP_COMMAND to a Studio MCP server".to_string(),
                &thread, &transcript, &skills_used));
        }
        if let Err(e) = self.check_bridge_capabilities() {
            return Err(fail(e, &thread, &transcript, &skills_used));
        }
        self.generated.clear();
        self.last_skill = None;
        let mut full_system = self.effective_system_prompt();
        if read_only {
            full_system.push_str(
                "\n\nACTIVE MODE: ASK (READ-ONLY). You may inspect the game with get_place_tree, \
                 search_game_tree, inspect_instance, script_read, script_grep, or get_console_output. \
                 Do NOT call mutating tools (write_luau, write_script, multi_edit, create_instance, \
                 delete_instance, delete_script, execute_luau, playtest). Reply with {\"done\": true, \"summary\": \"...\"}.",
            );
        } else if staged {
            full_system.push_str(
                "\n\nACTIVE MODE: PLAN / STAGED DIFF REVIEW. Your edits run in an isolated preview so \
                 the user can review exact unified diffs before applying them to Roblox Studio.",
            );
        }
        let reasoning_norm = opts.reasoning.trim().to_ascii_lowercase();
        if matches!(reasoning_norm.as_str(), "low" | "medium" | "high") {
            full_system.push_str(&format!(
                "\nREASONING DEPTH: {reasoning_norm}."
            ));
        }

        // Inject compact Whole-Game Context Summary + requested context_paths on turn 1.
        let compact = self.model.is_loopback();
        let turn1_user = match self.place_tree().ok() {
            Some(tree) => {
                let ctx = build_whole_game_context(&tree, &opts.context_paths, compact);
                if ctx.is_empty() {
                    ask.to_string()
                } else {
                    format!("{ask}\n\n{ctx}")
                }
            }
            None => ask.to_string(),
        };

        let mut messages = vec![
            json!({ "role": "system", "content": full_system }),
            json!({ "role": "user", "content": turn1_user }),
        ];
        let mut summary: Option<String> = None;
        let mut failed_playtests = 0usize;
        let mut writes: HashMap<String, String> = HashMap::new();
        let mut last_failed_console: Option<String> = None;
        // Changes no passing playtest has covered yet (the done gate),
        // and whether this ask changed anything at all (the claim flag
        // on an empty-handed "done").
        let mut unproven_changes = false;
        let mut changes_made = 0usize;
        // Plain-words reasons for each step that failed in this ask, and
        // the last script this ask wrote (named when the ask gives up).
        let mut problems: Vec<String> = Vec::new();
        // The user-facing sentence when a prerequisite gate refused a step.
        let mut gate_sentence: Option<String> = None;
        let mut last_written: Option<String> = None;

        for _ in 0..MAX_STEPS {
            let content = match self.model.chat(&messages) {
                Ok(c) => c,
                Err(e) => return Err(fail(e, &thread, &transcript, &skills_used)),
            };
            messages.push(json!({ "role": "assistant", "content": content }));
            transcript.push(format!("model step: {content}"));
            let step = match parse_step(&content) {
                Ok(s) => s,
                Err(e) => {
                    transcript.push(format!("unparseable model step: {e}"));
                    messages.push(json!({
                        "role": "user",
                        "content": format!("That step could not be read: {e}. Reply with exactly one JSON step: a skill, a tool, or done.")
                    }));
                    continue;
                }
            };

            if step.get("done").and_then(|d| d.as_bool()) == Some(true) {
                if unproven_changes && !staged && !read_only {
                    transcript.push("done refused: changes are unproven by a playtest".to_string());
                    messages.push(json!({
                        "role": "user",
                        "content": "You changed the game but no playtest has passed since the last change. Run a playtest before finishing — OpenRO only calls work done once it is proven."
                    }));
                    continue;
                }
                let s = step
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Done.")
                    .to_string();
                // An ask that changed nothing because a step failed is
                // refused in plain words: its own summary is not the
                // result, and no pitch follows it.
                if !read_only && changes_made == 0 && !problems.is_empty() {
                    let message = match &gate_sentence {
                        Some(sentence) => sentence.clone(),
                        None => format!(
                            "I could not build that, so nothing in your game changed. What went wrong: {}.",
                            problems.join("; ")
                        ),
                    };
                    thread.push(format!("OpenRO: {message}"));
                    transcript.push("done refused: nothing changed and a step failed".to_string());
                    return Ok(RunOutcome {
                        summary: message,
                        thread,
                        transcript,
                        skills_used: Vec::new(),
                        refused: true,
                        changed: false,
                    });
                }
                // The place still holds a change an earlier ask made
                // that no passing playtest has covered (or the app's
                // last playtest failed): a success summary would be false.
                let proof = self.proof_by_chat.get(chat_id).cloned().unwrap_or_default();
                if !read_only && !staged && proof.unproven {
                    let why = match &proof.failed_console {
                        Some(console) => format!(
                            "the last test run of your game failed. Console output: {console}"
                        ),
                        None => "changes to your game have not passed a test run yet".to_string(),
                    };
                    transcript.push("done refused: the place is not proven".to_string());
                    let tail = if changes_made > 0 {
                        "Undo last build puts the game back as it was before that build."
                    } else {
                        "Nothing from this try was added to your game."
                    };
                    return Err(fail(
                        format!("I can't call this done: {why}. {tail}"),
                        &thread, &transcript, &skills_used));
                }
                // A vacuous playtest (nothing changed, so nothing to
                // break) does NOT excuse a build claim: when the
                // summary claims building work but the ask changed
                // nothing, the ledger line comes FIRST, then the claim.
                if !read_only && changes_made == 0 && claims_building(&s) {
                    thread.push(
                        "OpenRO: Note — this ask made no changes to the game.".to_string(),
                    );
                }
                thread.push(format!("OpenRO: {s}"));
                transcript.push(format!("done: {s}"));
                summary = Some(s);
                break;
            }

            if let Some(skill) = step.get("skill").and_then(|v| v.as_str()) {
                if read_only {
                    let msg = format!(
                        "Ask mode is read-only: skill '{skill}' is disabled. Use read-only inspection tools and reply with {{\"done\": true, \"summary\": \"...\"}}."
                    );
                    transcript.push(format!("skill {skill} refused (ask mode): {msg}"));
                    messages.push(json!({ "role": "user", "content": format!("skill_result: error: {msg}") }));
                    continue;
                }
                let args = step.get("args").cloned().unwrap_or(json!({}));
                // A skill whose prerequisite was undone in this chat and
                // not rebuilt since is refused, not generated.
                if let Some(need) = self.undone_prerequisite(chat_id, skill) {
                    let msg = format!("{skill} needs {need} first, and {need} was undone. Build {need} first, then {skill}");
                    transcript.push(format!("skill {skill} refused: {msg}"));
                    problems.push(msg.clone());
                    gate_sentence = Some(plain_prerequisite(skill, &need));
                    messages.push(json!({
                        "role": "user",
                        "content": format!("skill_result: error: {msg} Choose a different step.")
                    }));
                    continue;
                }
                match self.run_skill(skill, &args, &mut transcript, &mut thread) {
                    Ok(note) => {
                        skills_used.push(skill.to_string());
                        messages.push(json!({ "role": "user", "content": note }));
                    }
                    Err(e) => {
                        problems.push(format!("a building step failed: {e}"));
                        transcript.push(format!("skill {skill} error: {e}"));
                        messages.push(json!({
                            "role": "user",
                            "content": format!("skill_result: error: {e}. Choose a different step.")
                        }));
                    }
                }
                continue;
            }

            if let Some(tool) = step.get("tool").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or(json!({}));
                if read_only && !is_read_only_tool(tool) {
                    let msg = format!(
                        "Ask mode is read-only: tool '{tool}' cannot mutate the place. \
                         Use read-only tools (get_place_tree, search_game_tree, inspect_instance, \
                         script_read, script_grep, get_console_output) and reply with {{\"done\": true, \"summary\": \"...\"}}."
                    );
                    transcript.push(format!("tool {tool} refused (ask mode): {msg}"));
                    messages.push(json!({
                        "role": "user",
                        "content": format!("tool_result: error: {msg}")
                    }));
                    continue;
                }
                if tool == "write_luau" {
                    match self.run_write_luau(
                        &args,
                        &mut transcript,
                        &mut thread,
                        &mut writes,
                        last_failed_console.as_deref(),
                    ) {
                        Ok(note) => {
                            changes_made += 1;
                            unproven_changes = true;
                            self.note_change(chat_id);
                            last_written = args.get("path").and_then(|v| v.as_str()).map(|p| p.to_string());
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Err(e) => {
                            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("the script");
                            problems.push(format!("saving {path} failed: {e}"));
                            transcript.push(format!("tool write_luau error: {e}"));
                            messages.push(json!({
                                "role": "user",
                                "content": format!("tool_result: error: {e}. Fix the error and try again.")
                            }));
                        }
                    }
                    continue;
                }
                if tool == "multi_edit" {
                    match self.run_multi_edit(
                        &args,
                        &mut transcript,
                        &mut thread,
                        &mut writes,
                        last_failed_console.as_deref(),
                    ) {
                        Ok(note) => {
                            changes_made += 1;
                            unproven_changes = true;
                            self.note_change(chat_id);
                            last_written = args
                                .get("path")
                                .or_else(|| args.get("file_path"))
                                .and_then(|v| v.as_str())
                                .map(|p| p.to_string());
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Err(e) => {
                            let path = args
                                .get("path")
                                .or_else(|| args.get("file_path"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("the script");
                            problems.push(format!("editing {path} failed: {e}"));
                            transcript.push(format!("tool multi_edit error: {e}"));
                            messages.push(json!({
                                "role": "user",
                                "content": format!("tool_result: error: {e}. Fix the error and try again.")
                            }));
                        }
                    }
                    continue;
                }
                if tool == "playtest" {
                    // Nothing changed in this ask: there is nothing to
                    // test, and a run now would be a vacuous pass.
                    if changes_made == 0 {
                        transcript.push("playtest not run: this ask changed nothing".to_string());
                        thread.push("Playtest not run — nothing has changed yet, so there is nothing to test.".to_string());
                        messages.push(json!({
                            "role": "user",
                            "content": "tool_result: playtest not run: this ask has not changed the game, so there is nothing to test. Make the change first, then playtest."
                        }));
                        continue;
                    }
                    let verdict = self.run_playtest(&mut transcript, &mut thread);
                    if let Ok(outcome) = &verdict {
                        self.record_verdict(chat_id, outcome);
                    }
                    match verdict {
                        Ok(PlaytestOutcome::Passed(note)) => {
                            unproven_changes = false;
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Ok(PlaytestOutcome::Failed(note, console)) => {
                            failed_playtests += 1;
                            last_failed_console = Some(console.clone());
                            transcript.push(format!(
                                "repair attempt {failed_playtests}: playtest failed"
                            ));
                            if failed_playtests >= MAX_REPAIRS {
                                return Err(fail(
                                    format!(
                                        "playtest still failing after {failed_playtests} attempts — \
                                         I could not get this build to pass. Last console output: {console}"
                                    ),
                                    &thread, &transcript, &skills_used));
                            }
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Ok(PlaytestOutcome::Unreadable(note, raw)) => {
                            failed_playtests += 1;
                            transcript.push(format!(
                                "repair attempt {failed_playtests}: playtest verdict unreadable: {raw}"
                            ));
                            if failed_playtests >= MAX_REPAIRS {
                                return Err(fail(
                                    format!(
                                        "playtest verdict could not be read after {failed_playtests} attempts — \
                                         the bridge must return {{\"status\": \"passed\"}} or {{\"status\": \"failed\"}}. \
                                         Last playtest result: {raw}"
                                    ),
                                    &thread, &transcript, &skills_used));
                            }
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Err(e) => return Err(fail(e, &thread, &transcript, &skills_used)),
                    }
                    continue;
                }
                if tool == "write_script" {
                    // Local models frequently combine skill generation and script
                    // writing into one `write_script` call (passing `skill` + optional
                    // skill parameters without raw `source`). Compile the requested
                    // Rune skill on demand — still strictly refusing raw `source` and
                    // enforcing undone-prerequisite gates identically to a separate
                    // skill step.
                    if args.get("source").is_none() {
                        if let Some(skill_name) = args.get("skill").and_then(|v| v.as_str()) {
                            let inline_args = extract_inline_skill_args(&args);
                            let needs_compile = !self.generated.contains_key(skill_name)
                                || inline_args.is_some();
                            if needs_compile {
                                if let Some(need) = self.undone_prerequisite(chat_id, skill_name) {
                                    let msg = format!(
                                        "{skill_name} needs {need} first, and {need} was undone. Build {need} first, then {skill_name}"
                                    );
                                    transcript.push(format!("skill {skill_name} refused: {msg}"));
                                    problems.push(msg.clone());
                                    gate_sentence = Some(plain_prerequisite(skill_name, &need));
                                    messages.push(json!({
                                        "role": "user",
                                        "content": format!("skill_result: error: {msg} Choose a different step.")
                                    }));
                                    continue;
                                }
                                let skill_args = inline_args.unwrap_or_else(|| json!({}));
                                match self.run_skill(
                                    skill_name,
                                    &skill_args,
                                    &mut transcript,
                                    &mut thread,
                                ) {
                                    Ok(_) => skills_used.push(skill_name.to_string()),
                                    Err(e) => {
                                        problems.push(format!("a building step failed: {e}"));
                                        transcript.push(format!("skill {skill_name} error: {e}"));
                                        messages.push(json!({
                                            "role": "user",
                                            "content": format!("skill_result: error: {e}. Choose a different step.")
                                        }));
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    match self.run_write_script(
                        &args,
                        &mut transcript,
                        &mut thread,
                        &mut writes,
                        last_failed_console.as_deref(),
                    ) {
                        Ok(note) => {
                            changes_made += 1;
                            unproven_changes = true;
                            self.note_change(chat_id);
                            last_written = args.get("path").and_then(|v| v.as_str()).map(|p| p.to_string());
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Err(e) => {
                            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("the script");
                            problems.push(format!("saving {path} failed: {e}"));
                            transcript.push(format!("tool write_script error: {e}"));
                            messages.push(json!({
                                "role": "user",
                                "content": format!("tool_result: error: {e}. Choose a different step.")
                            }));
                        }
                    }
                    continue;
                }
                match self.run_tool(tool, &args, &mut transcript, &mut thread) {
                    Ok(note) => {
                        if matches!(
                            tool,
                            "delete_script" | "create_instance" | "delete_instance"
                        ) {
                            changes_made += 1;
                            unproven_changes = true;
                            self.note_change(chat_id);
                        }
                        messages.push(json!({ "role": "user", "content": note }));
                    }
                    Err(e) => {
                        problems.push(format!("a game step failed: {e}"));
                        transcript.push(format!("tool error: {e}"));
                        messages.push(json!({
                            "role": "user",
                            "content": format!("tool_result: error: {e}. Choose a different step.")
                        }));
                    }
                }
                continue;
            }

            messages.push(json!({
                "role": "user",
                "content": "That step was not recognised. Reply with one JSON step: a skill, a tool, or done."
            }));
        }

        match summary {
            Some(summary) => Ok(RunOutcome {
                summary,
                thread,
                transcript,
                skills_used,
                refused: false,
                changed: changes_made > 0,
            }),
            None => {
                let mut message = format!("agent loop did not finish within {MAX_STEPS} steps");
                if let Some(path) = &last_written {
                    let proof = self.proof_by_chat.get(chat_id).cloned().unwrap_or_default();
                    let status = if proof.failed_console.is_some() {
                        "its last test run failed"
                    } else {
                        "no test run has passed it"
                    };
                    message.push_str(&format!(
                        ". The game was left with the script {path}, and {status}. Undo last build undoes it."
                    ));
                }
                Err(fail(message, &thread, &transcript, &skills_used))
            }
        }
    }

    fn run_skill(
        &mut self,
        skill: &str,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
    ) -> Result<String, String> {
        self.emit_event(
            "compiling_rune",
            "Generating Luau",
            &format!("Generating Luau for '{skill}'"),
            Some(skill),
            None,
            35,
        );
        // The Luau is produced by executing the skill's Rune script.
        let source = skills::generate(skill, args)?;
        let args_note = args
            .as_object()
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        transcript.push(format!(
            "skill {skill} (Rune) {args_note} generated {} bytes of Luau",
            source.len()
        ));
        thread.push("Generated the script for that part.".to_string());
        self.generated.insert(skill.to_string(), source.clone());
        self.last_skill = Some(skill.to_string());
        Ok(format!(
            "skill_result: the {skill} Rune skill generated this Luau source:\n{source}\n\
             Next, write it with {{\"tool\":\"write_script\",\"args\":{{\"path\":\"<where it belongs>\",\"skill\":\"{skill}\"}}}} and then playtest."
        ))
    }

    fn run_write_script(
        &mut self,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
        writes: &mut HashMap<String, String>,
        last_failed_console: Option<&str>,
    ) -> Result<String, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("write_script needs args.path")?
            .to_string();
        check_path(&path)?;
        let source = self.resolve_source(args)?;
        // Checkpoint AFTER the bridge call succeeds: recording the
        // previous state before the attempt used to poison the change
        // set with changes that never happened when the call failed.
        let prev = self.prev_script(&path)?;
        self.studio_mut()?
            .call_tool("write_script", json!({ "path": path, "source": source.clone() }))?;
        self.record_script(&path, prev, Some(source.clone()));
        self.emit_event(
            "writing",
            "Writing to Place",
            &format!("Saved {path} ({} bytes)", source.len()),
            args.get("skill").and_then(|v| v.as_str()),
            Some(&path),
            60,
        );
        let mut st_ev = StudioActionInput::new(
            StudioActionKind::ScriptWritten,
            format!("Wrote {path} ({} bytes)", source.len()),
        );
        st_ev.path = path.clone();
        st_ev.class_name = if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.") {
            "LocalScript".to_string()
        } else {
            "Script".to_string()
        };
        st_ev.code_snippet = Some(snippet_from_source(&source));
        self.emit_studio_action(st_ev);
        let _ = self.auto_capture_mcp_frame();
        transcript.push(format!("tool write_script {path} ({} bytes)", source.len()));
        thread.push(format!("Wrote {path}."));
        let mut note = format!(
            "tool_result: write_script {path} succeeded. Write any remaining scripts/instances requested, or call {{\"tool\":\"playtest\",\"args\":{{}}}} when ready."
        );
        // A byte-identical rewrite of a version that just failed cannot
        // fix anything — tell the model so, with the console text.
        if let (Some(prev), Some(console)) = (writes.get(&path), last_failed_console) {
            if *prev == source {
                transcript.push(format!(
                    "note: write_script {path} was byte-identical to the failing version"
                ));
                note = format!(
                    "tool_result: write_script {path} succeeded — but this source is byte-identical \
                     to the version that just failed the playtest with: {console}\n\
                     Rewriting the same source cannot change the result. Change something material \
                     (create what it references, or generate it differently), then playtest again."
                );
            }
        }
        writes.insert(path, source);
        Ok(note)
    }

    /// RoPilot custom Luau writer (`write_luau`): validates `path`, caps size at `512 KiB`,
    /// runs `luau_check::analyze_source` (rejecting with exact line-numbered errors before
    /// writing if any `Severity::Error` is present), checkpoints previous state onto the
    /// per-chat Undo stack, writes to the active backend, emits `ScriptWritten` + auto-capture.
    fn run_write_luau(
        &mut self,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
        writes: &mut HashMap<String, String>,
        last_failed_console: Option<&str>,
    ) -> Result<String, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("write_luau needs args.path")?
            .to_string();
        let source = args
            .get("source")
            .and_then(|v| v.as_str())
            .ok_or("write_luau needs args.source")?
            .to_string();
        check_path(&path)?;
        if source.len() > MAX_LUAU_BYTES {
            return Err(format!(
                "write_luau source exceeds maximum allowed size ({MAX_LUAU_BYTES} bytes)"
            ));
        }
        if source.contains('\0') {
            return Err("write_luau source must not contain NUL bytes".to_string());
        }
        let report = crate::luau_check::analyze_source(&path, &source);
        if report.error_count() > 0 {
            let formatted = format_luau_errors(&report);
            return Err(format!(
                "Static Luau Prover rejected {path} ({} error(s)):\n{formatted}",
                report.error_count()
            ));
        }
        let prev = self.prev_script(&path)?;
        self.studio_mut()?
            .call_tool("write_script", json!({ "path": path, "source": source.clone() }))?;
        self.record_script(&path, prev, Some(source.clone()));
        self.emit_event(
            "writing",
            "Writing Custom Luau",
            &format!("Saved {path} ({} bytes)", source.len()),
            None,
            Some(&path),
            60,
        );
        let mut st_ev = StudioActionInput::new(
            StudioActionKind::ScriptWritten,
            format!("Wrote custom Luau {path} ({} bytes)", source.len()),
        );
        st_ev.path = path.clone();
        st_ev.class_name = if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.") {
            "LocalScript".to_string()
        } else if path.starts_with("ReplicatedStorage.") {
            "ModuleScript".to_string()
        } else {
            "Script".to_string()
        };
        st_ev.code_snippet = Some(snippet_from_source(&source));
        self.emit_studio_action(st_ev);
        let _ = self.auto_capture_mcp_frame();
        transcript.push(format!("tool write_luau {path} ({} bytes)", source.len()));
        thread.push(format!("Wrote {path}."));
        let warn_suffix = if report.warning_count() > 0 {
            format!(
                " ({} static warning(s): {})",
                report.warning_count(),
                report.warnings().join("; ")
            )
        } else {
            String::new()
        };
        let mut note = format!(
            "tool_result: write_luau {path} succeeded{warn_suffix}. Write any remaining scripts/instances, or call {{\"tool\":\"playtest\",\"args\":{{}}}} when ready."
        );
        if let (Some(prev_written), Some(console)) = (writes.get(&path), last_failed_console) {
            if *prev_written == source {
                transcript.push(format!(
                    "note: write_luau {path} was byte-identical to the failing version"
                ));
                note = format!(
                    "tool_result: write_luau {path} succeeded — but this source is byte-identical \
                     to the version that just failed the playtest with: {console}\n\
                     Change something material before playtesting again."
                );
            }
        }
        writes.insert(path, source);
        Ok(note)
    }

    /// RoPilot targeted multi-hunk Luau editor (`multi_edit`): validates `path`, applies
    /// ordered `old_str -> new_str` replacements, caps size at `512 KiB`, runs
    /// `luau_check::analyze_source` (rejecting on any `Severity::Error` before writing),
    /// checkpoints previous state onto per-chat Undo, writes & emits `ScriptWritten`.
    fn run_multi_edit(
        &mut self,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
        writes: &mut HashMap<String, String>,
        last_failed_console: Option<&str>,
    ) -> Result<String, String> {
        let path = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("multi_edit needs args.path")?
            .to_string();
        let edits = args
            .get("edits")
            .and_then(|v| v.as_array())
            .filter(|a| !a.is_empty())
            .ok_or("multi_edit needs non-empty args.edits array")?;
        check_path(&path)?;

        let current_opt = self.prev_script_current(&path)?;
        let mut updated = match current_opt {
            Some(s) => s,
            None => {
                let first_empty = edits
                    .first()
                    .and_then(|e| {
                        e.get("old_str")
                            .or_else(|| e.get("old_text"))
                            .or_else(|| e.get("find"))
                            .and_then(|v| v.as_str())
                    })
                    .is_some_and(|s| s.is_empty());
                if first_empty {
                    String::new()
                } else {
                    return Err(format!(
                        "no script at {path} — use write_luau to create it first"
                    ));
                }
            }
        };

        let mut applied = 0usize;
        for (idx, ed) in edits.iter().enumerate() {
            let old_s = ed
                .get("old_str")
                .or_else(|| ed.get("old_text"))
                .or_else(|| ed.get("find"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("multi_edit edit #{} needs string 'old_str'", idx + 1))?;
            let new_s = ed
                .get("new_str")
                .or_else(|| ed.get("new_text"))
                .or_else(|| ed.get("replace"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("multi_edit edit #{} needs string 'new_str'", idx + 1))?;
            if old_s.is_empty() {
                if !updated.is_empty() && !updated.ends_with('\n') {
                    updated.push('\n');
                }
                updated.push_str(new_s);
            } else {
                if !updated.contains(old_s) {
                    return Err(format!(
                        "multi_edit edit #{} target not found in {path} (read the script with script_read first)",
                        idx + 1
                    ));
                }
                updated = updated.replacen(old_s, new_s, 1);
            }
            applied += 1;
        }

        if updated.len() > MAX_LUAU_BYTES {
            return Err(format!(
                "multi_edit result exceeds maximum allowed size ({MAX_LUAU_BYTES} bytes)"
            ));
        }
        if updated.contains('\0') {
            return Err("multi_edit result must not contain NUL bytes".to_string());
        }
        let report = crate::luau_check::analyze_source(&path, &updated);
        if report.error_count() > 0 {
            let formatted = format_luau_errors(&report);
            return Err(format!(
                "Static Luau Prover rejected multi_edit on {path} ({} error(s)):\n{formatted}",
                report.error_count()
            ));
        }

        let prev = self.prev_script(&path)?;
        self.studio_mut()?
            .call_tool("write_script", json!({ "path": path, "source": updated.clone() }))?;
        self.record_script(&path, prev, Some(updated.clone()));
        self.emit_event(
            "writing",
            "Editing Luau",
            &format!("Applied {applied} edit(s) to {path} ({} bytes)", updated.len()),
            None,
            Some(&path),
            62,
        );
        let mut st_ev = StudioActionInput::new(
            StudioActionKind::ScriptWritten,
            format!("Edited {path} ({applied} hunk(s), {} bytes)", updated.len()),
        );
        st_ev.path = path.clone();
        st_ev.class_name = if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.") {
            "LocalScript".to_string()
        } else if path.starts_with("ReplicatedStorage.") {
            "ModuleScript".to_string()
        } else {
            "Script".to_string()
        };
        st_ev.code_snippet = Some(snippet_from_source(&updated));
        self.emit_studio_action(st_ev);
        let _ = self.auto_capture_mcp_frame();
        transcript.push(format!(
            "tool multi_edit {path} ({applied} edit(s), {} bytes)",
            updated.len()
        ));
        thread.push(format!("Edited {path} ({applied} change(s))."));
        let mut note = format!(
            "tool_result: multi_edit {path} applied {applied} edit(s). Call {{\"tool\":\"playtest\",\"args\":{{}}}} when ready."
        );
        if let (Some(prev_written), Some(console)) = (writes.get(&path), last_failed_console) {
            if *prev_written == updated {
                note = format!(
                    "tool_result: multi_edit {path} resulted in byte-identical source to the failing version ({console}). Make a material fix before playtesting again."
                );
            }
        }
        writes.insert(path, updated);
        Ok(note)
    }

    /// Resolve the source for a write_script step: a named skill's
    /// generated source, or the most recent generation. There is no
    /// hand-written-source path — Luau comes only from Rune skills.
    fn resolve_source(&self, args: &Value) -> Result<String, String> {
        if args.get("source").is_some() {
            return Err(
                "write_script does not take a hand-written \"source\" — Luau comes only from \
                 Rune skills. Run a skill first, then write its generated source."
                    .to_string(),
            );
        }
        if let Some(name) = args.get("skill").and_then(|v| v.as_str()) {
            return self
                .generated
                .get(name)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "no generated source for that part yet — run its step first: {{\"skill\": \"{name}\", \"args\": {{}}}}"
                    )
                });
        }
        match args.get("use_generated") {
            Some(Value::String(name)) => self
                .generated
                .get(name)
                .cloned()
                .ok_or_else(|| "no generated source for that part yet — run its step first".to_string()),
            Some(Value::Bool(true)) => {
                let name = self
                    .last_skill
                    .as_ref()
                    .ok_or("write_script needs args.skill or a prior skill result")?;
                self.generated
                    .get(name)
                    .cloned()
                    .ok_or_else(|| "generated source missing".to_string())
            }
            _ => Err("write_script needs args.skill (Luau comes only from Rune skills) or a prior skill result".to_string()),
        }
    }

    fn run_playtest(
        &mut self,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
    ) -> Result<PlaytestOutcome, String> {
        let check_detail = match self.studio.as_ref() {
            Some(PlaceBackend::Local(local)) => {
                let rep = local.analyze();
                format!(
                    "Checked {} script(s): {} error(s), {} warning(s)",
                    local.scripts().len(),
                    rep.error_count(),
                    rep.warning_count()
                )
            }
            _ => "Checking Luau syntax, service references, and script wiring".to_string(),
        };
        self.emit_event(
            "static_luau_check",
            "Static Luau Check",
            &check_detail,
            None,
            None,
            78,
        );
        self.emit_event(
            "playtest_proof",
            "Playtest Proof",
            "Running Studio playtest verification",
            None,
            None,
            88,
        );
        let mut pt_start = StudioActionInput::new(
            StudioActionKind::PlaytestStarted,
            "Running Studio playtest verification",
        );
        pt_start.path = "Workspace".to_string();
        pt_start.class_name = "RunService".to_string();
        self.emit_studio_action(pt_start);
        let text = self.studio_mut()?.call_tool("playtest", json!({}))?;
        transcript.push(format!("tool playtest -> {text}"));
        match playtest_verdict(&text) {
            Verdict::Failed => {
                self.emit_event(
                    "auto_repair",
                    "Auto-Repair",
                    "Playtest reported an error — reading console to repair",
                    None,
                    None,
                    70,
                );
                thread.push("Playtest failed — reading the console and repairing.".to_string());
                // Feed the ACTUAL console output back to the model. A
                // console-fetch failure must not abort the repair loop
                // (it once turned one failed playtest into a dead ask):
                // fall back to the console embedded in the playtest
                // result itself, or say plainly it is unavailable.
                let console = match self.studio_mut()?.call_tool("get_console", json!({})) {
                    Ok(c) => c,
                    Err(e) => serde_json::from_str::<Value>(&text)
                        .ok()
                        .and_then(|v| {
                            v.get("console")
                                .and_then(|c| c.as_str())
                                .map(|s| s.to_string())
                        })
                        .unwrap_or_else(|| format!("(could not read the console: {e})")),
                };
                let fseq = self
                    .take_playtest_frame_seq()
                    .or_else(|| self.auto_capture_mcp_frame());
                let mut pt_fail = StudioActionInput::new(
                    StudioActionKind::PlaytestFailed,
                    "Playtest failed — reading console output",
                );
                pt_fail.path = "Output".to_string();
                pt_fail.class_name = "RunService".to_string();
                pt_fail.console_line = Some(console.chars().take(240).collect());
                pt_fail.frame_seq = fseq;
                self.emit_studio_action(pt_fail);
                transcript.push(format!("console after failed playtest: {console}"));
                thread.push(format!("Console from the failed test run: {console}"));
                Ok(PlaytestOutcome::Failed(
                    format!(
                        "tool_result: playtest FAILED. Actual console output: {console}\n\
                         Repair the game from that console output (for example regenerate the \
                         script so it creates what it references, or create the missing instance), \
                         apply the fix, and playtest again."
                    ),
                    console,
                ))
            }
            Verdict::Passed => {
                self.emit_event(
                    "playtest_proof",
                    "Playtest Passed",
                    "Playtest ran cleanly with zero errors",
                    None,
                    None,
                    95,
                );
                let fseq = self
                    .take_playtest_frame_seq()
                    .or_else(|| self.auto_capture_mcp_frame());
                let mut pt_pass = StudioActionInput::new(
                    StudioActionKind::PlaytestPassed,
                    "Playtest passed with 0 Luau errors",
                );
                pt_pass.path = "Workspace".to_string();
                pt_pass.class_name = "RunService".to_string();
                pt_pass.frame_seq = fseq;
                self.emit_studio_action(pt_pass);
                thread.push("Playtest passed.".to_string());
                Ok(PlaytestOutcome::Passed(format!(
                    "tool_result: playtest PASSED ({text}). You may finish with done."
                )))
            }
            Verdict::Unreadable => {
                thread.push(
                    "Playtest result was not in the documented format — treating the build as unproven."
                        .to_string(),
                );
                Ok(PlaytestOutcome::Unreadable(
                    format!(
                        "tool_result: the playtest result could not be read as a verdict: {text}\n\
                         The bridge must return {{\"status\": \"passed\"}} or {{\"status\": \"failed\"}}. \
                         The build counts as UNPROVEN, not passed. Playtest again."
                    ),
                    text,
                ))
            }
        }
    }

    fn run_tool(
        &mut self,
        tool: &str,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
    ) -> Result<String, String> {
        match tool {
            "delete_script" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("delete_script needs args.path")?
                    .to_string();
                check_path(&path)?;
                let prev = self.prev_script(&path)?;
                self.studio_mut()?
                    .call_tool("delete_script", json!({ "path": path }))?;
                self.record_script(&path, prev, None);
                transcript.push(format!("tool delete_script {path}"));
                Ok(format!("tool_result: delete_script {path} succeeded."))
            }
            "create_instance" => {
                let class_name = args
                    .get("class_name")
                    .and_then(|v| v.as_str())
                    .ok_or("create_instance needs args.class_name")?
                    .to_string();
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or("create_instance needs args.name")?
                    .to_string();
                let parent = args
                    .get("parent")
                    .and_then(|v| v.as_str())
                    .ok_or("create_instance needs args.parent")?
                    .to_string();
                if name.is_empty()
                    || name.chars().any(|c| c == '.' || c == '/' || c == '\\' || c.is_control())
                {
                    return Err(format!(
                        "create_instance name must be a single non-empty name without dots, separators or control characters, got: {name}"
                    ));
                }
                if class_name.is_empty()
                    || class_name
                        .chars()
                        .any(|c| c == '.' || c == '/' || c == '\\' || c.is_control())
                {
                    return Err(format!(
                        "create_instance class_name must be a single class name without separators or control characters, got: {class_name}"
                    ));
                }
                check_path(&parent)?;
                let path = format!("{parent}.{name}");
                check_path(&path)?;
                let prev = self.prev_instance(&path)?;
                self.studio_mut()?
                    .call_tool("create_instance", args.clone())?;
                // The state this ask leaves is the record the BRIDGE
                // holds, read back from the tree — never a record
                // fabricated from the ask's args. A real place adds
                // its own default properties to every instance; an
                // args-shaped post would make every legitimate
                // instance revert refuse forever against it.
                let post = match self.prev_instance_current(&path) {
                    Ok(Some(record)) => Some(record),
                    _ => Some(json!({
                        "class_name": class_name,
                        "properties": args.get("properties").cloned().unwrap_or(json!({})),
                    })),
                };
                let was_existing = prev.is_some();
                let post_props = post.as_ref().and_then(|r| r.get("properties")).cloned();
                self.record_instance(&path, prev, post);
                let mut ev = StudioActionInput::new(
                    if was_existing {
                        StudioActionKind::PartUpdated
                    } else {
                        StudioActionKind::PartCreated
                    },
                    format!("Placed {path} ({class_name})"),
                );
                ev.path = path.clone();
                ev.class_name = class_name.clone();
                ev.position = extract_vec3_prop(post_props.as_ref(), "Position");
                ev.size = extract_vec3_prop(post_props.as_ref(), "Size");
                ev.color = extract_color_prop(post_props.as_ref());
                self.emit_studio_action(ev);
                let _ = self.auto_capture_mcp_frame();
                transcript.push(format!("tool create_instance {path} ({class_name})"));
                thread.push(format!("Created {path} ({class_name})."));
                Ok(format!("tool_result: create_instance {path} succeeded."))
            }
            "delete_instance" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("delete_instance needs args.path")?
                    .to_string();
                check_path(&path)?;
                let prev = self.prev_instance(&path)?;
                self.studio_mut()?
                    .call_tool("delete_instance", json!({ "path": path }))?;
                self.record_instance(&path, prev, None);
                let mut ev = StudioActionInput::new(
                    StudioActionKind::PartUpdated,
                    format!("Deleted {path}"),
                );
                ev.path = path.clone();
                ev.class_name = "Instance".to_string();
                self.emit_studio_action(ev);
                let _ = self.auto_capture_mcp_frame();
                transcript.push(format!("tool delete_instance {path}"));
                thread.push(format!("Deleted {path}."));
                Ok(format!("tool_result: delete_instance {path} succeeded."))
            }
            "get_console" | "get_place_tree" => {
                let text = self.studio_mut()?.call_tool(tool, json!({}))?;
                transcript.push(format!("tool {tool} -> {text}"));
                Ok(format!("tool_result: {tool}: {text}"))
            }
            "blender_generate" => {
                let root = self.effective_assets_root();
                let res = crate::library::generate_blender_mesh(&root, args, None)?;
                let text = res.to_string();
                let name = res
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("3D mesh");
                transcript.push(format!("tool blender_generate -> {text}"));
                thread.push(format!("Generated 3D mesh {name}."));
                Ok(format!("tool_result: blender_generate: {text}"))
            }
            "audio_generate" => {
                let root = self.effective_assets_root();
                let res = crate::audio_synth::generate_audio_asset(&root, args)?;
                let text = res.to_string();
                let name = res
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("SFX audio");
                transcript.push(format!("tool audio_generate -> {text}"));
                thread.push(format!("Synthesized WAV audio {name}."));
                Ok(format!("tool_result: audio_generate: {text}"))
            }
            t if is_official_studio_tool(t) => {
                let chat_id = self.active_build_chat.clone();
                let (val, _) = self.invoke_studio_tool(&chat_id, t, args.clone())?;
                let text = val.to_string();
                transcript.push(format!("tool {t} -> {text}"));
                Ok(format!("tool_result: {t}: {text}"))
            }
            other => Err(format!("unknown tool: {other}")),
        }
    }

    /// The previous source of `path` (or None if absent), read WITHOUT
    /// recording anything. Callers record it only after the change
    /// itself succeeds — a failed tool call must never enter the
    /// change set.
    /// The previous state of a script path recorded by THIS ask.
    /// Outer None = not recorded by this ask; inner None = recorded
    /// as absent. Stored sets from earlier asks are never consulted:
    /// their checkpoints describe the place before THOSE asks, and a
    /// new ask's prev must come from the tree as it is now.
    fn recorded_prev_script(&self, path: &str) -> Option<Option<String>> {
        self.current_changes.iter().find_map(|c| match c {
            Change::Script(p, prev, _) if p == path => Some(prev.clone()),
            _ => None,
        })
    }

    fn prev_script(&mut self, path: &str) -> Result<Option<String>, String> {
        if let Some(prev) = self.recorded_prev_script(path) {
            return Ok(prev);
        }
        let tree = self.studio_mut()?.get_place_tree()?;
        Ok(tree
            .pointer("/scripts")
            .and_then(|s| s.get(path))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    /// Record a successful change: the first touch of a path in an
    /// ask fixes its `prev`; every touch updates the `post` (the
    /// state the ask most recently left), which is what a later
    /// revert compares the live place against.
    fn record_script(&mut self, path: &str, prev: Option<String>, post: Option<String>) {
        for c in self.current_changes.iter_mut() {
            if let Change::Script(p, _, existing_post) = c {
                if p == path {
                    *existing_post = post;
                    return;
                }
            }
        }
        self.current_changes
            .push(Change::Script(path.to_string(), prev, post));
    }

    fn recorded_prev_instance(&self, path: &str) -> Option<Option<Value>> {
        self.current_changes.iter().find_map(|c| match c {
            Change::Instance(p, prev, _) if p == path => Some(prev.clone()),
            _ => None,
        })
    }

    /// True when the place already holds an instance at this step's path
    /// with the same class.
    fn already_in_place(&mut self, args: &Value) -> Result<bool, String> {
        let (Some(parent), Some(name), Some(class)) = (
            args.get("parent").and_then(|v| v.as_str()),
            args.get("name").and_then(|v| v.as_str()),
            args.get("class_name").and_then(|v| v.as_str()),
        ) else {
            return Ok(false);
        };
        let path = format!("{parent}.{name}");
        Ok(self
            .prev_instance_current(&path)?
            .and_then(|r| r.get("class_name").and_then(|c| c.as_str()).map(|c| c == class))
            .unwrap_or(false))
    }

    /// The previous instance record at `path` (or None if absent),
    /// read without recording.
    fn prev_instance(&mut self, path: &str) -> Result<Option<Value>, String> {
        if let Some(prev) = self.recorded_prev_instance(path) {
            return Ok(prev);
        }
        let tree = self.studio_mut()?.get_place_tree()?;
        Ok(tree
            .pointer("/instances")
            .and_then(|s| s.get(path))
            .cloned())
    }

    fn record_instance(&mut self, path: &str, prev: Option<Value>, post: Option<Value>) {
        for c in self.current_changes.iter_mut() {
            if let Change::Instance(p, _, existing_post) = c {
                if p == path {
                    *existing_post = post;
                    return;
                }
            }
        }
        self.current_changes
            .push(Change::Instance(path.to_string(), prev, post));
    }

    /// Restore one chat's change set: rewrite previous content, delete
    /// what the asks created — scripts and instances alike. Returns the
    /// restored paths. An empty (or absent) set is NOTHING_TO_REVERT,
    /// never a fabricated success.
    ///
    /// The restore is failure-atomic and idempotent: the set is only
    /// consumed once EVERY entry is restored, and each restore first
    /// checks the current state — restoring an already-absent instance
    /// to absent is success, not an error. A failed revert keeps its
    /// set, so it can be retried and converges.
    pub fn revert_chat(&mut self, chat_id: &str) -> Result<Vec<String>, String> {
        if self.studio.is_none() {
            return Err("Studio not connected: nothing to revert".to_string());
        }
        let changes = match self.changes_by_chat.get(chat_id) {
            Some(stack) => match stack.last() {
                Some(c) if !c.changes.is_empty() => c.changes.clone(),
                _ => return Err(NOTHING_TO_REVERT.to_string()),
            },
            None => return Err(NOTHING_TO_REVERT.to_string()),
        };
        let popped_skills = self.top_skills(chat_id);
        let mut restored = Vec::new();
        for change in changes.iter().rev() {
            let path = match change {
                Change::Script(p, _, _) | Change::Instance(p, _, _) => p.clone(),
            };
            if let Err(e) = self.restore_one(change) {
                return Err(format!(
                    "revert stopped at {path}: {e} — {} change(s) were restored, the rest are \
                     untouched, and the change set is kept so revert can be tried again",
                    restored.len()
                ));
            }
            restored.push(path);
        }
        // Success: pop this ask's set and save it onto the chat's Redo stack.
        // Earlier asks' sets stay on the undo stack, still revertable.
        if let Some(stack) = self.changes_by_chat.get_mut(chat_id) {
            if let Some(popped) = stack.pop() {
                let rstack = self.redo_by_chat.entry(chat_id.to_string()).or_default();
                if rstack.len() >= crate::place::MAX_HISTORY_DEPTH {
                    rstack.remove(0);
                }
                rstack.push(popped);
            }
            if stack.is_empty() {
                self.changes_by_chat.remove(chat_id);
            }
        }
        // The place is back to an earlier state. With no set left it is
        // the game as it was before any build; otherwise its verdict is
        // not known, so it stays unproven (and the failed run is gone).
        let remaining = self.changes_by_chat.contains_key(chat_id);
        let kept = self.chat_skills(chat_id);
        let proof = self.proof_by_chat.entry(chat_id.to_string()).or_default();
        proof.unproven = remaining;
        proof.failed_console = None;
        for skill in popped_skills {
            if !kept.contains(&skill) && !proof.undone.contains(&skill) {
                proof.undone.push(skill);
            }
        }
        self.save_proof();
        self.save_changes();
        self.sync_undo();
        let mut rev_ev = StudioActionInput::new(
            StudioActionKind::Reverted,
            format!("Reverted {} change(s) in Studio", restored.len()),
        );
        rev_ev.path = "Workspace".to_string();
        rev_ev.class_name = "ChangeHistoryService".to_string();
        self.emit_studio_action(rev_ev);
        let _ = self.auto_capture_mcp_frame();
        Ok(restored)
    }

    /// Re-apply the most recently undone change set for `chat_id` (`POST /api/redo`).
    /// Returns `(reapplied_paths, reapplied_skills)`.
    pub fn redo_chat(&mut self, chat_id: &str) -> Result<(Vec<String>, Vec<String>), String> {
        if self.studio.is_none() {
            return Err("Studio not connected: nothing to redo".to_string());
        }
        let set = match self.redo_by_chat.get(chat_id) {
            Some(stack) => match stack.last() {
                Some(c) if !c.changes.is_empty() => c.clone(),
                _ => return Err(NOTHING_TO_REDO.to_string()),
            },
            None => return Err(NOTHING_TO_REDO.to_string()),
        };
        let mut restored = Vec::new();
        for change in &set.changes {
            let path = match change {
                Change::Script(p, _, _) | Change::Instance(p, _, _) => p.clone(),
            };
            if let Err(e) = self.reapply_one(change) {
                return Err(format!(
                    "redo stopped at {path}: {e} — {} change(s) were re-applied",
                    restored.len()
                ));
            }
            restored.push(path);
        }
        if let Some(stack) = self.redo_by_chat.get_mut(chat_id) {
            stack.pop();
            if stack.is_empty() {
                self.redo_by_chat.remove(chat_id);
            }
        }
        let skills = set.skills.clone();
        self.changes_by_chat
            .entry(chat_id.to_string())
            .or_default()
            .push(set);
        let proof = self.proof_by_chat.entry(chat_id.to_string()).or_default();
        proof.undone.retain(|s| !skills.contains(s));
        self.save_proof();
        self.save_changes();
        self.sync_undo();
        let mut redo_ev = StudioActionInput::new(
            StudioActionKind::Restored,
            format!("Redid {} change(s) in Studio", restored.len()),
        );
        redo_ev.path = "Workspace".to_string();
        redo_ev.class_name = "ChangeHistoryService".to_string();
        self.emit_studio_action(redo_ev);
        let _ = self.auto_capture_mcp_frame();
        Ok((restored, skills))
    }

    /// Re-apply a single undone change (`prev` -> `post`), verifying that the
    /// live place still holds `prev` (or already holds `post`).
    fn reapply_one(&mut self, change: &Change) -> Result<(), String> {
        match change {
            Change::Script(path, prev, post) => {
                let current = self.prev_script_current(path)?;
                if current == *post {
                    return Ok(());
                }
                if current != *prev {
                    return Err(format!(
                        "{path} changed since it was undone — refusing to overwrite newer edits"
                    ));
                }
                match post {
                    Some(source) => {
                        self.studio_mut()?.call_tool(
                            "write_script",
                            json!({ "path": path, "source": source }),
                        )?;
                    }
                    None => {
                        self.studio_mut()?
                            .call_tool("delete_script", json!({ "path": path }))?;
                    }
                }
                Ok(())
            }
            Change::Instance(path, prev, post) => {
                let current = self.prev_instance_current(path)?;
                if current == *post {
                    return Ok(());
                }
                if current != *prev {
                    return Err(format!(
                        "{path} changed since it was undone — refusing to overwrite newer edits"
                    ));
                }
                match post {
                    Some(record) => {
                        let (parent, name) = match path.rsplit_once('.') {
                            Some((p, n)) => (p.to_string(), n.to_string()),
                            None => (String::new(), path.clone()),
                        };
                        let class_name = record
                            .get("class_name")
                            .and_then(|c| c.as_str())
                            .ok_or_else(|| {
                                format!("the redo record for {path} is unreadable (no class name)")
                            })?;
                        if current.is_some() {
                            self.studio_mut()?
                                .call_tool("delete_instance", json!({ "path": path }))?;
                        }
                        self.studio_mut()?.call_tool(
                            "create_instance",
                            json!({
                                "class_name": class_name,
                                "name": name,
                                "parent": parent,
                                "properties": record.get("properties").cloned().unwrap_or(json!({})),
                            }),
                        )?;
                    }
                    None => {
                        self.studio_mut()?
                            .call_tool("delete_instance", json!({ "path": path }))?;
                    }
                }
                Ok(())
            }
        }
    }

    /// Restore a single change. Identity-checked, not just
    /// existence-checked: the restore proceeds only when the place
    /// still holds exactly what this ask left (restore to prev), or
    /// already holds prev (nothing to do). Anything else means the
    /// path was changed by someone else after the ask — another chat,
    /// a hand edit in Studio — and revert REFUSES rather than
    /// silently destroying work it does not own.
    fn restore_one(&mut self, change: &Change) -> Result<(), String> {
        match change {
            Change::Script(path, prev, post) => {
                let current = self.prev_script_current(path)?;
                if current == *prev {
                    return Ok(()); // already at the previous state
                }
                if current != *post {
                    return Err(format!(
                        "{path} has changed since this ask — it no longer holds what the ask \
                         left, so reverting would destroy work this chat does not own"
                    ));
                }
                match prev {
                    Some(source) => {
                        self.studio_mut()?.call_tool(
                            "write_script",
                            json!({ "path": path, "source": source }),
                        )?;
                    }
                    None => {
                        self.studio_mut()?
                            .call_tool("delete_script", json!({ "path": path }))?;
                    }
                }
                Ok(())
            }
            Change::Instance(path, prev, post) => {
                let current = self.prev_instance_current(path)?;
                if current == *prev {
                    return Ok(());
                }
                if current != *post {
                    return Err(format!(
                        "{path} has changed since this ask — it no longer holds what the ask \
                         left, so reverting would destroy work this chat does not own"
                    ));
                }
                match prev {
                    Some(record) => {
                        let (parent, name) = match path.rsplit_once('.') {
                            Some((p, n)) => (p.to_string(), n.to_string()),
                            None => (String::new(), path.clone()),
                        };
                        let class_name = record
                            .get("class_name")
                            .and_then(|c| c.as_str())
                            .ok_or_else(|| {
                                format!("the change record for {path} is unreadable (no class name)")
                            })?;
                        if current.is_some() {
                            self.studio_mut()?
                                .call_tool("delete_instance", json!({ "path": path }))?;
                        }
                        self.studio_mut()?.call_tool(
                            "create_instance",
                            json!({
                                "class_name": class_name,
                                "name": name,
                                "parent": parent,
                                "properties": record.get("properties").cloned().unwrap_or(json!({})),
                            }),
                        )?;
                    }
                    None => {
                        self.studio_mut()?
                            .call_tool("delete_instance", json!({ "path": path }))?;
                    }
                }
                Ok(())
            }
        }
    }

    /// Current state reads for the revert path (no recorded-history
    /// shortcuts — the place as it is NOW).
    fn prev_script_current(&mut self, path: &str) -> Result<Option<String>, String> {
        let tree = self.studio_mut()?.get_place_tree()?;
        Ok(tree
            .pointer("/scripts")
            .and_then(|s| s.get(path))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    fn prev_instance_current(&mut self, path: &str) -> Result<Option<Value>, String> {
        let tree = self.studio_mut()?.get_place_tree()?;
        Ok(tree
            .pointer("/instances")
            .and_then(|s| s.get(path))
            .cloned())
    }

    pub fn place_tree(&mut self) -> Result<Value, String> {
        self.studio_mut()?.get_place_tree()
    }

    fn studio_mut(&mut self) -> Result<&mut PlaceBackend, String> {
        self.studio
            .as_mut()
            .ok_or_else(|| "Studio not connected".to_string())
    }
}

enum PlaytestOutcome {
    Passed(String),
    Failed(String, String),
    Unreadable(String, String),
}

fn changeset_from_trees(before: &Value, after: &Value) -> Vec<Change> {
    let empty = serde_json::Map::new();
    let b_inst = before
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let a_inst = after
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let b_scr = before
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let a_scr = after
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);

    let mut changes = Vec::new();
    let mut inst_keys: BTreeSet<&str> = BTreeSet::new();
    for k in b_inst.keys().chain(a_inst.keys()) {
        inst_keys.insert(k.as_str());
    }
    for k in inst_keys {
        let prev = b_inst.get(k).cloned();
        let post = a_inst.get(k).cloned();
        if prev != post {
            changes.push(Change::Instance(k.to_string(), prev, post));
        }
    }

    let mut scr_keys: BTreeSet<&str> = BTreeSet::new();
    for k in b_scr.keys().chain(a_scr.keys()) {
        scr_keys.insert(k.as_str());
    }
    for k in scr_keys {
        let prev = b_scr.get(k).and_then(|v| v.as_str()).map(String::from);
        let post = a_scr.get(k).and_then(|v| v.as_str()).map(String::from);
        if prev != post {
            changes.push(Change::Script(k.to_string(), prev, post));
        }
    }
    changes
}

fn extract_vec3_prop(props: Option<&Value>, key: &str) -> Option<[f64; 3]> {
    let val = props?.get(key)?;
    if let Some(arr) = val.as_array() {
        if arr.len() >= 3 {
            let x = arr[0].as_f64()?;
            let y = arr[1].as_f64()?;
            let z = arr[2].as_f64()?;
            if x.is_finite() && y.is_finite() && z.is_finite() {
                return Some([x, y, z]);
            }
        }
    }
    if let Some(obj) = val.as_object() {
        let x = obj.get("x").or_else(|| obj.get("X")).and_then(|v| v.as_f64())?;
        let y = obj.get("y").or_else(|| obj.get("Y")).and_then(|v| v.as_f64())?;
        let z = obj.get("z").or_else(|| obj.get("Z")).and_then(|v| v.as_f64())?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Some([x, y, z]);
        }
    }
    None
}

fn extract_color_prop(props: Option<&Value>) -> Option<String> {
    let val = props?.get("Color").or_else(|| props?.get("BrickColor"))?;
    if let Some(s) = val.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Some(arr) = val.as_array() {
        if arr.len() >= 3 {
            let r = arr[0].as_f64().unwrap_or(0.5);
            let g = arr[1].as_f64().unwrap_or(0.5);
            let b = arr[2].as_f64().unwrap_or(0.5);
            let scale = |c: f64| -> u8 {
                if (0.0..=1.0).contains(&c) {
                    (c * 255.0).round() as u8
                } else {
                    c.clamp(0.0, 255.0).round() as u8
                }
            };
            return Some(format!("#{:02X}{:02X}{:02X}", scale(r), scale(g), scale(b)));
        }
    }
    None
}

fn snippet_from_source(source: &str) -> String {
    let lines: Vec<&str> = source.lines().take(6).collect();
    let joined = lines.join("\n");
    joined.chars().take(280).collect()
}

/// Does this summary claim building work? Sentence-scoped: a claim
/// verb in a sentence with no suppressor (negation, or "already" for
/// pre-existing work) counts; suppressors in OTHER sentences do not
/// bleed across the boundary. Token-based, so "padded" never trips
/// "added". The flag this drives is a factual ledger line, not an
/// accusation — the line states what the ask did (nothing), and the
/// reader judges the summary against it.
fn claims_building(summary: &str) -> bool {
    const CLAIMS: [&str; 16] = [
        "built", "created", "wrote", "added", "placed", "generated", "constructed",
        "installed", "made", "shipped", "implemented", "assembled", "wired", "spawned",
        "fixed", "repaired",
    ];
    const SUPPRESSORS: [&str; 14] = [
        "nothing", "not", "no", "never", "didn't", "didnt", "wasn't", "wasnt", "haven't",
        "havent", "isn't", "isnt", "already", "couldn't",
    ];
    // Normalise curly apostrophes first: models emit them routinely,
    // and a U+2019 inside "wasn't" would otherwise split the token
    // and smuggle the claim past its own suppressor.
    let summary = summary.replace('\u{2019}', "'");
    for sentence in summary.split(['.', '!', '?', '\n', ';']) {
        let tokens: Vec<String> = sentence
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|t| !t.is_empty())
            .map(|t| t.to_string())
            .collect();
        if tokens.iter().any(|t| SUPPRESSORS.contains(&t.as_str())) {
            continue;
        }
        if tokens.iter().any(|t| CLAIMS.contains(&t.as_str())) {
            return true;
        }
    }
    false
}

/// One change set from changes.json: the current object form
/// (`changes` + `skills`) or the older bare array of changes (no skills
/// recorded). Anything else is malformed.
fn parse_set(set: &Value) -> Option<ChangeSet> {
    let (items, skills) = match set {
        Value::Array(items) => (items, Vec::new()),
        Value::Object(_) => {
            let skills = set
                .get("skills")?
                .as_array()?
                .iter()
                .map(|s| s.as_str().map(|x| x.to_string()))
                .collect::<Option<Vec<_>>>()?;
            (set.get("changes")?.as_array()?, skills)
        }
        _ => return None,
    };
    let mut changes = Vec::new();
    for item in items {
        changes.push(Change::from_json(item)?);
    }
    Some(ChangeSet { changes, skills })
}

/// The proof file: a corrupt one is set aside and the chats start with
/// no verdicts recorded (recorded in `recovered`).
fn load_proof(path: &std::path::Path, recovered: &mut Vec<String>) -> HashMap<String, Proof> {
    let Ok(text) = std::fs::read_to_string(path) else { return HashMap::new() };
    let parsed = (|| -> Option<HashMap<String, Proof>> {
        let v: Value = serde_json::from_str(&text).ok()?;
        let mut out = HashMap::new();
        for (chat, p) in v.as_object()? {
            let unproven = p.get("unproven")?.as_bool()?;
            let failed_console = match p.get("failed_console")? {
                Value::Null => None,
                Value::String(s) => Some(s.clone()),
                _ => return None,
            };
            let undone = p
                .get("undone")?
                .as_array()?
                .iter()
                .map(|x| x.as_str().map(String::from))
                .collect::<Option<Vec<_>>>()?;
            out.insert(chat.clone(), Proof { unproven, failed_console, undone });
        }
        Some(out)
    })();
    match parsed {
        Some(map) => map,
        None => {
            if let Some(bak) = crate::server::backup_corrupt(path) {
                recovered.push(bak.file_name().unwrap_or_default().to_string_lossy().into_owned());
            }
            HashMap::new()
        }
    }
}

/// The display name of the idea that adds a skill (e.g. "Money that saves"
/// for currency). Internal skill ids never reach the user through this.
pub fn idea_title_for_skill(sk: &str) -> String {
    let all = crate::ideas::catalogue();
    let id = format!("add-{sk}");
    all.iter()
        .find(|i| i.id == id.as_str())
        .or_else(|| all.iter().find(|i| i.skills.contains(&sk)))
        .map(|i| i.title.to_string())
        .unwrap_or_else(|| "That".to_string())
}

/// The one plain refusal used on every path (chat asks and idea buttons)
/// when a build needs a prerequisite the game does not have.
pub fn prerequisite_sentence(blocked_title: &str, need_skill: &str) -> String {
    let needed = idea_title_for_skill(need_skill);
    format!("{blocked_title} needs \"{needed}\" first. Build \"{needed}\" first, then ask again.")
}

/// The user's sentence for a chat-path step refused because its
/// prerequisite skill was undone: the display names, never skill ids.
fn plain_prerequisite(skill: &str, need: &str) -> String {
    prerequisite_sentence(&idea_title_for_skill(skill), need)
}

/// A plain sentence for a Studio refusal; the raw text goes to the transcript.
fn plain_bridge(e: &str) -> String {
    if e.contains("Studio bridge error") {
        "Studio did not accept that step.".to_string()
    } else {
        e.to_string()
    }
}

pub fn parse_step(content: &str) -> Result<Value, String> {
    // Strip optional <think>...</think> reasoning prefixes emitted by reasoning models.
    let mut cleaned = content.trim();
    while let Some(after_open) = cleaned.strip_prefix("<think>") {
        if let Some(end_idx) = after_open.find("</think>") {
            cleaned = after_open[end_idx + "</think>".len()..].trim();
        } else {
            break;
        }
    }
    // Try direct JSON parse first.
    if let Ok(v) = serde_json::from_str::<Value>(cleaned) {
        if v.is_object() {
            return Ok(v);
        }
    }
    // Extract the first balanced JSON object (handles ```json fences and trailing text/steps).
    let mut scan = cleaned;
    while let Some(start) = scan.find('{') {
        let slice = &scan[start..];
        let mut de = serde_json::Deserializer::from_str(slice).into_iter::<Value>();
        if let Some(Ok(v)) = de.next() {
            if v.is_object() {
                return Ok(v);
            }
        }
        scan = &slice[1..];
    }
    Err(format!("model reply was not a JSON step: {content}"))
}

/// Extract optional inline Rune skill parameters passed inside a combined
/// `write_script` call (either nested under `args`/`skill_args` or as extra
/// keys beside `path`/`skill`/`use_generated`). Returns `None` when no inline
/// parameters were provided.
fn extract_inline_skill_args(write_args: &Value) -> Option<Value> {
    if let Some(nested) = write_args
        .get("args")
        .or_else(|| write_args.get("skill_args"))
        .filter(|v| v.is_object())
    {
        return Some(nested.clone());
    }
    let obj = write_args.as_object()?;
    let mut extras = serde_json::Map::new();
    for (k, v) in obj {
        if !matches!(k.as_str(), "path" | "skill" | "use_generated" | "source") {
            extras.insert(k.clone(), v.clone());
        }
    }
    if extras.is_empty() {
        None
    } else {
        Some(Value::Object(extras))
    }
}

/// A dotted place path the bridge can be trusted with: non-empty
/// segments, no `.`/`..` segments, no separators or control characters.
/// The agent validates before forwarding — model output is not passed
/// to the bridge unchecked.
pub fn check_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("path must not be empty".to_string());
    }
    for seg in path.split('.') {
        if seg.is_empty() || seg == ".." {
            return Err(format!("invalid path: {path}"));
        }
        if seg.chars().any(|c| c == '/' || c == '\\' || c.is_control()) {
            return Err(format!("invalid path: {path}"));
        }
    }
    Ok(())
}

enum Verdict {
    Passed,
    Failed,
    Unreadable,
}

/// Read a playtest result in the documented bridge format only:
/// JSON with a `status` of "passed"/"failed", or the bare word
/// "passed"/"failed". Anything else is Unreadable — the old substring
/// heuristic called "Playtest passed. 0 failures, 0 errors." a failure
/// (it contains "fail") and would have called the reverse a pass; a
/// verdict this product acts on must be read exactly or not at all.
fn playtest_verdict(result_text: &str) -> Verdict {
    if let Ok(v) = serde_json::from_str::<Value>(result_text) {
        if let Some(status) = v.get("status").and_then(|s| s.as_str()) {
            return match status {
                "passed" => Verdict::Passed,
                "failed" => Verdict::Failed,
                _ => Verdict::Unreadable,
            };
        }
        return Verdict::Unreadable;
    }
    let t = result_text.trim().trim_end_matches('.').to_lowercase();
    match t.as_str() {
        "passed" | "playtest passed" => Verdict::Passed,
        "failed" | "playtest failed" => Verdict::Failed,
        _ => Verdict::Unreadable,
    }
}

fn is_read_only_tool(tool: &str) -> bool {
    matches!(
        tool,
        "get_place_tree"
            | "search_game_tree"
            | "inspect_instance"
            | "script_read"
            | "script_grep"
            | "get_console_output"
            | "get_console"
            | "list_roblox_studios"
            | "get_studio_state"
    )
}

fn format_luau_errors(report: &crate::luau_check::AnalysisReport) -> String {
    report
        .diagnostics
        .iter()
        .filter(|d| d.severity == crate::luau_check::Severity::Error)
        .map(|d| format!("Line {} [{}]: {}", d.line, d.code, d.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build a compact Whole-Game Context Summary injected into turn 1 so the model sees
/// the open Roblox place immediately (service counts, instance/script paths, and full
/// contents of caller-selected `context_paths` capped sensibly).
fn build_whole_game_context(tree: &Value, context_paths: &[String], compact: bool) -> String {
    let empty = serde_json::Map::new();
    let instances = tree
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let scripts = tree
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    if instances.is_empty() && scripts.is_empty() && context_paths.is_empty() {
        return String::new();
    }

    let mut svc_counts: std::collections::BTreeMap<&str, (usize, usize)> =
        std::collections::BTreeMap::new();
    for k in instances.keys() {
        let root = k.split('.').next().unwrap_or("Other");
        svc_counts.entry(root).or_default().0 += 1;
    }
    for k in scripts.keys() {
        let root = k.split('.').next().unwrap_or("Other");
        svc_counts.entry(root).or_default().1 += 1;
    }

    let mut out = String::from("[OPENRO WHOLE-GAME CONTEXT]\n");
    let svc_summary: Vec<String> = svc_counts
        .iter()
        .map(|(svc, (ic, sc))| format!("{svc}: {ic} instance(s), {sc} script(s)"))
        .collect();
    if !svc_summary.is_empty() {
        out.push_str("Services: ");
        out.push_str(&svc_summary.join(" | "));
        out.push('\n');
    }

    let path_cap = if compact { 16 } else { 40 };
    if !scripts.is_empty() {
        let mut s_paths: Vec<&str> = scripts.keys().map(String::as_str).collect();
        s_paths.sort();
        let shown: Vec<&str> = s_paths.iter().take(path_cap).copied().collect();
        out.push_str(&format!(
            "Scripts ({}): {}{}\n",
            s_paths.len(),
            shown.join(", "),
            if s_paths.len() > path_cap { ", ..." } else { "" }
        ));
    }
    if !instances.is_empty() {
        let mut i_paths: Vec<&str> = instances.keys().map(String::as_str).collect();
        i_paths.sort();
        let shown: Vec<&str> = i_paths.iter().take(path_cap).copied().collect();
        out.push_str(&format!(
            "Instances ({}): {}{}\n",
            i_paths.len(),
            shown.join(", "),
            if i_paths.len() > path_cap { ", ..." } else { "" }
        ));
    }

    if !context_paths.is_empty() {
        let max_per_file = if compact { 2048 } else { 8192 };
        let max_total = if compact { 4096 } else { 24576 };
        let mut used = 0usize;
        let mut seen = BTreeSet::new();
        for raw in context_paths {
            let p = raw.trim();
            if p.is_empty() || !seen.insert(p.to_string()) || check_path(p).is_err() {
                continue;
            }
            if used >= max_total {
                break;
            }
            if let Some(src) = scripts.get(p).and_then(|v| v.as_str()) {
                let budget = (max_total - used).min(max_per_file);
                let mut end = budget.min(src.len());
                while end > 0 && !src.is_char_boundary(end) {
                    end -= 1;
                }
                let slice = &src[..end];
                out.push_str(&format!(
                    "\n--- Attached Script: {p} ---\n{slice}{}\n",
                    if end < src.len() { "\n-- [truncated]" } else { "" }
                ));
                used += slice.len();
            } else if let Some(inst) = instances.get(p) {
                let text = inst.to_string();
                let budget = (max_total - used).min(max_per_file);
                let mut end = budget.min(text.len());
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                let slice = &text[..end];
                out.push_str(&format!("\n--- Attached Instance: {p} ---\n{slice}\n"));
                used += slice.len();
            }
        }
    }

    out
}
