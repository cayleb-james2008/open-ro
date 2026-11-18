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
use crate::skills;
use serde_json::{json, Value};
use std::collections::HashMap;

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
Always playtest after writing, and repair from the console output until the playtest passes. \
Look at the place tree before changing things you did not create."
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

pub struct Agent {
    pub model: ModelClient,
    pub studio: Option<McpClient>,
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
    changes_by_chat: HashMap<String, Vec<Vec<Change>>>,
    /// Change set being accumulated by the ask currently running.
    current_changes: Vec<Change>,
    /// Where change sets persist across restarts, when set.
    changes_path: Option<std::path::PathBuf>,
}

impl Agent {
    pub fn new(model: ModelClient, studio: Option<McpClient>) -> Agent {
        Agent {
            model,
            studio,
            generated: HashMap::new(),
            last_skill: None,
            changes_by_chat: HashMap::new(),
            current_changes: Vec::new(),
            changes_path: None,
        }
    }

    /// Persist change sets to `path` (loaded now, saved on change), so
    /// a restart does not silently orphan every revert. Loading is
    /// strict and loud: an unparseable file, a wrong shape, or ANY
    /// malformed entry backs the whole file up and starts empty —
    /// a partially-read change set is worse than none, because a
    /// revert acting on coerced records can destroy real work.
    pub fn set_changes_file(&mut self, path: std::path::PathBuf) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let loaded = (|| -> Option<HashMap<String, Vec<Vec<Change>>>> {
                let v: Value = serde_json::from_str(&text).ok()?;
                let obj = v.as_object()?;
                let mut out = HashMap::new();
                for (chat, stack) in obj {
                    let sets = stack.as_array()?;
                    let mut parsed_stack = Vec::new();
                    for set in sets {
                        let items = set.as_array()?;
                        let mut parsed_set = Vec::new();
                        for item in items {
                            parsed_set.push(Change::from_json(item)?);
                        }
                        if !parsed_set.is_empty() {
                            parsed_stack.push(parsed_set);
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
                    let _ = crate::server::backup_corrupt(&path);
                }
            }
        }
        self.changes_path = Some(path);
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
                            Value::Array(set.iter().map(Change::to_json).collect())
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

    pub fn studio_connected(&mut self) -> bool {
        match &mut self.studio {
            Some(s) => s.is_alive(),
            None => false,
        }
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
        let result = self.run_ask_inner(ask);
        let harvested = std::mem::take(&mut self.current_changes);
        if !harvested.is_empty() {
            self.changes_by_chat
                .entry(chat_id.to_string())
                .or_default()
                .push(harvested);
            self.save_changes();
        }
        result
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
        let result = self.run_plan_inner(label, steps);
        let harvested = std::mem::take(&mut self.current_changes);
        if !harvested.is_empty() {
            self.changes_by_chat
                .entry(chat_id.to_string())
                .or_default()
                .push(harvested);
            self.save_changes();
        }
        result
    }

    fn run_plan_inner(
        &mut self,
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
                    Ok(PlaytestOutcome::Passed(_)) => {
                        unproven_changes = false;
                    }
                    Ok(PlaytestOutcome::Failed(_, console)) => {
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
                                 is called done. Console output: {console}"
                            ),
                            &thread, &transcript, &skills_used));
                    }
                    Ok(PlaytestOutcome::Unreadable(_, raw)) => {
                        return Err(fail(
                            format!(
                                "my built-in plan for “{label}” got a playtest result that \
                                 could not be read as a verdict ({raw}) — the build counts \
                                 as unproven, not done"
                            ),
                            &thread, &transcript, &skills_used));
                    }
                    Err(e) => return Err(fail(e, &thread, &transcript, &skills_used)),
                }
            };
        }

        for step in steps {
            if let Some(skill) = step.get("skill").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or(json!({}));
                match self.run_skill(skill, &args, &mut transcript, &mut thread) {
                    Ok(_) => skills_used.push(skill.to_string()),
                    Err(e) => {
                        return Err(fail(
                            format!("built-in plan stopped at the {skill} skill: {e}"),
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
                        }
                        Err(e) => {
                            return Err(fail(
                                format!("built-in plan stopped at write_script: {e}"),
                                &thread, &transcript, &skills_used))
                        }
                    }
                    continue;
                }
                match self.run_tool(tool, &args, &mut transcript, &mut thread) {
                    Ok(_) => {
                        if matches!(
                            tool,
                            "delete_script" | "create_instance" | "delete_instance"
                        ) {
                            changes_made += 1;
                            unproven_changes = true;
                        }
                    }
                    Err(e) => {
                        return Err(fail(
                            format!("built-in plan stopped at {tool}: {e}"),
                            &thread, &transcript, &skills_used))
                    }
                }
                continue;
            }
            return Err(fail(
                format!("built-in plan held an unreadable step: {step}"),
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
        let summary = format!("Built “{label}” — playtest passed.");
        thread.push(format!("OpenRO: {summary}"));
        transcript.push(format!("plan done: {summary}"));
        Ok(RunOutcome {
            summary,
            thread,
            transcript,
            skills_used,
        })
    }

    fn run_ask_inner(&mut self, ask: &str) -> Result<RunOutcome, AskFailure> {
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
                thread.push(format!("OpenRO: {s}"));
                // A vacuous playtest (nothing changed, so nothing to
                // break) does NOT excuse a build claim: when the
                // summary claims building work but the ask changed
                // nothing, the thread gains a factual ledger line —
                // what the ask did (nothing) — next to the claim.
                if changes_made == 0 && claims_building(&s) {
                    thread.push(
                        "OpenRO: Note — this ask made no changes to the game.".to_string(),
                    );
                }
                transcript.push(format!("done: {s}"));
                summary = Some(s);
                break;
            }

            if let Some(skill) = step.get("skill").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or(json!({}));
                match self.run_skill(skill, &args, &mut transcript, &mut thread) {
                    Ok(note) => {
                        skills_used.push(skill.to_string());
                        messages.push(json!({ "role": "user", "content": note }));
                    }
                    Err(e) => {
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
                    match self.run_playtest(&mut transcript, &mut thread) {
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
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Err(e) => {
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
                        }
                        messages.push(json!({ "role": "user", "content": note }));
                    }
                    Err(e) => {
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
            }),
            None => Err(fail(
                format!("agent loop did not finish within {MAX_STEPS} steps"),
                &thread, &transcript, &skills_used)),
        }
    }

    fn run_skill(
        &mut self,
        skill: &str,
        args: &Value,
        transcript: &mut Vec<String>,
        thread: &mut Vec<String>,
    ) -> Result<String, String> {
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
        thread.push(format!("Generated the {skill} script."));
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
        transcript.push(format!("tool write_script {path} ({} bytes)", source.len()));
        thread.push(format!("Wrote {path}."));
        let mut note = format!("tool_result: write_script {path} succeeded. Next, playtest.");
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
                .ok_or_else(|| format!("no generated source for skill '{name}' — run the skill first"));
        }
        match args.get("use_generated") {
            Some(Value::String(name)) => self
                .generated
                .get(name)
                .cloned()
                .ok_or_else(|| format!("no generated source for skill '{name}' — run the skill first")),
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
        let text = self.studio_mut()?.call_tool("playtest", json!({}))?;
        transcript.push(format!("tool playtest -> {text}"));
        match playtest_verdict(&text) {
            Verdict::Failed => {
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
                Some(c) if !c.is_empty() => c.clone(),
                _ => return Err(NOTHING_TO_REVERT.to_string()),
            },
            None => return Err(NOTHING_TO_REVERT.to_string()),
        };
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
        self.save_changes();
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

    fn studio_mut(&mut self) -> Result<&mut McpClient, String> {
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
    for sentence in summary.split(|c| matches!(c, '.' | '!' | '?' | '\n' | ';')) {
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

fn parse_step(content: &str) -> Result<Value, String> {
    let start = content
        .find('{')
        .ok_or_else(|| format!("model reply was not a JSON step: {content}"))?;
    let end = content
        .rfind('}')
        .ok_or_else(|| format!("model reply was not valid JSON: {content}"))?;
    serde_json::from_str(&content[start..=end])
        .map_err(|e| format!("model reply was not valid JSON: {e}"))
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
