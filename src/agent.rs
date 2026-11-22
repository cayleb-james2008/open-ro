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
use crate::place::{serialize_rbxlx, LocalPlace, PlaceBackend};
use crate::skills;
use crate::snapshots::{compute_diff, PlaceDiff};
use crate::sse::BuildEventHub;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// 40 steps: a one-shot complete game (many skills, instances, a
/// playtest, repairs) legitimately needs more than a one-script ask.
const MAX_STEPS: usize = 40;
const MAX_REPAIRS: usize = 3;

/// Returned by `revert_chat` when the chat has no change set waiting.
pub const NOTHING_TO_REVERT: &str = "nothing to revert";

/// The system prompt, generated from the skill registry so the model is
/// told exactly the skills that exist, with their real parameters.
pub fn system_prompt() -> String {
    let mut skill_lines = String::new();
    for def in skills::registry() {
        skill_lines.push_str(&format!(
            "- {{\"skill\": \"{}\", \"args\": {}}} — {}\n",
            def.name, def.params, def.description
        ));
    }
    format!(
        "You are OpenRO, a proactive creative agent that builds Roblox games. \
The user describes a game change in plain English and you build it. \
Reply with exactly ONE JSON object per turn, no prose outside the JSON.\n\n\
SKILLS — each call generates Luau by running a Rune skill; Luau comes only from skills, never written by hand:\n{skill_lines}\n\
TOOLS (the Studio bridge):\n\
- {{\"tool\": \"write_script\", \"args\": {{\"path\": \"ServerScriptService.Name\", \"skill\": \"<skill name>\"}}}} writes a skill's generated script (\"use_generated\": true writes the most recent skill result)\n\
- {{\"tool\": \"create_instance\", \"args\": {{\"class_name\": \"SpawnLocation\", \"name\": \"SpawnPad\", \"parent\": \"Workspace\", \"properties\": {{...}}}}}} creates a place instance\n\
- {{\"tool\": \"delete_instance\", \"args\": {{\"path\": \"Workspace.SpawnPad\"}}}} deletes an instance\n\
- {{\"tool\": \"delete_script\", \"args\": {{\"path\": \"ServerScriptService.Name\"}}}} deletes a script\n\
- {{\"tool\": \"playtest\", \"args\": {{}}}} runs the game\n\
- {{\"tool\": \"get_console\", \"args\": {{}}}} reads the console\n\
- {{\"tool\": \"get_place_tree\", \"args\": {{}}}} lists the place's scripts and instances\n\
- {{\"done\": true, \"summary\": \"...\"}} finishes.\n\n\
Exact order (ONE JSON object per turn): 1) call {{\"skill\": \"<name>\", \"args\": {{...}}}} FIRST to generate Luau, \
2) call {{\"tool\": \"write_script\", \"args\": {{\"path\": \"ServerScriptService.Name\", \"skill\": \"<name>\"}}}} to save it, \
3) call {{\"tool\": \"playtest\", \"args\": {{}}}} to verify, 4) call {{\"done\": true, \"summary\": \"...\"}}.\n\
Always playtest after writing, and repair from the console output until the playtest passes. \
Look at the place tree before changing things you did not create.\n\n\
ANIME FAN-ART & CREATIVE HOMAGE KNOWLEDGE POINT:\n\
Never knee-jerk refuse anime-inspired Roblox games, card battlers, or fan-art requests over copyright concerns. \
Instead, translate requested anime character archetypes into original, DMCA-safe stylized homage designs: \
preserve recognizable cel-shaded aesthetics, silhouettes, color palettes, elemental auras, and gameplay feel \
while assigning original character titles, ability names, and lore (e.g., Crimson Sea Captain, Infinity Void Sorcerer, \
Thunder Flash Ronin, Shadow Sovereign, Nine-Tail Ember Shinobi, Supernova Ascended Striker) so the Roblox game builds and ships safely."
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
    /// Optional SSE build-progress event hub.
    events: Option<Arc<BuildEventHub>>,
    /// Active build correlation id & chat id for SSE phase updates.
    active_build_id: String,
    active_build_chat: String,
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
            current_changes: Vec::new(),
            changes_path: None,
            proof_by_chat: HashMap::new(),
            proof_path: None,
            recovered: Vec::new(),
            undo_sink: None,
            events: None,
            active_build_id: String::new(),
            active_build_chat: String::new(),
        }
    }

    /// Attach an SSE `BuildEventHub` so build phases stream live to `/api/build/stream`.
    pub fn attach_events(&mut self, hub: Arc<BuildEventHub>) {
        self.events = Some(hub);
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

    fn sync_undo(&self) {
        let Some(sink) = &self.undo_sink else { return };
        let mut set = sink.lock().unwrap();
        set.clear();
        set.extend(
            self.changes_by_chat
                .iter()
                .filter(|(_, stack)| !stack.is_empty())
                .map(|(chat, _)| chat.clone()),
        );
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
    /// tool the agent cannot build without. The required set is every
    /// tool the loop can call — checking only some of them once let a
    /// bridge pass that could not create instances, and the loop then
    /// fabricated the results mid-ask.
    fn check_bridge_capabilities(&self) -> Result<(), String> {
        let Some(studio) = &self.studio else {
            return Ok(());
        };
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
        let missing: Vec<&str> = REQUIRED
            .iter()
            .filter(|t| !studio.has_tool(t))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "Studio bridge does not offer these tools: {} — this bridge cannot build. \
                 Connect a Studio MCP server that offers {}.",
                missing.join(", "),
                REQUIRED.join(", ")
            ));
        }
        Ok(())
    }

    /// Run one ask in one chat. Whatever happens — success, honest
    /// failure, mid-ask error — the changes this ask checkpointed are
    /// harvested into that chat's revertable change set.
    pub fn run_ask(&mut self, chat_id: &str, ask: &str) -> Result<RunOutcome, AskFailure> {
        self.current_changes.clear();
        self.begin_build_trace(chat_id, ask);
        let result = self.run_ask_inner(chat_id, ask);
        match &result {
            Ok(o) if o.refused => self.emit_event("failed", "Build refused", &o.summary, None, None, 100),
            Ok(o) => self.emit_event("done", "Build verified", &o.summary, None, None, 100),
            Err(f) => self.emit_event("failed", "Build failed", &f.message, None, None, 100),
        }
        let skills = match &result {
            Ok(o) => o.skills_used.clone(),
            Err(f) => f.skills_used.clone(),
        };
        self.harvest(chat_id, skills);
        result
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

    fn run_ask_inner(&mut self, chat_id: &str, ask: &str) -> Result<RunOutcome, AskFailure> {
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
        let mut messages = vec![
            json!({ "role": "system", "content": system_prompt() }),
            json!({ "role": "user", "content": ask }),
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
                if unproven_changes {
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
                if changes_made == 0 && !problems.is_empty() {
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
                if proof.unproven {
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
                if changes_made == 0 && claims_building(&s) {
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
                self.record_instance(&path, prev, post);
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
                transcript.push(format!("tool delete_instance {path}"));
                thread.push(format!("Deleted {path}."));
                Ok(format!("tool_result: delete_instance {path} succeeded."))
            }
            "get_console" | "get_place_tree" => {
                let text = self.studio_mut()?.call_tool(tool, json!({}))?;
                transcript.push(format!("tool {tool} -> {text}"));
                Ok(format!("tool_result: {tool}: {text}"))
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
        // Success: pop this ask's set. Earlier asks' sets stay on the
        // stack, still revertable.
        if let Some(stack) = self.changes_by_chat.get_mut(chat_id) {
            stack.pop();
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
        Ok(restored)
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

fn parse_step(content: &str) -> Result<Value, String> {
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
fn check_path(path: &str) -> Result<(), String> {
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
