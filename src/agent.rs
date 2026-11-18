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

const MAX_STEPS: usize = 16;
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
}

/// A failed ask, carrying the thread and transcript lines produced
/// before the failure — a failed ask is still part of the chat's
/// record, and its change set stays revertable.
#[derive(Debug)]
pub struct AskFailure {
    pub message: String,
    pub thread: Vec<String>,
    pub transcript: Vec<String>,
}

/// One checkpointed change: a script's previous source, or an
/// instance's previous record (`{"class_name": ..., "properties": ...}`).
/// `None` means the path did not exist before this ask.
enum Change {
    Script(String, Option<String>),
    Instance(String, Option<Value>),
}

pub struct Agent {
    pub model: ModelClient,
    pub studio: Option<McpClient>,
    /// Luau generated by each skill during this ask, by skill name — a
    /// second generation never overwrites an unwritten first one.
    generated: HashMap<String, String>,
    last_skill: Option<String>,
    /// Change sets of the most recent change-making ask, per chat id.
    changes_by_chat: HashMap<String, Vec<Change>>,
    /// Change set being accumulated by the ask currently running.
    current_changes: Vec<Change>,
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
            self.changes_by_chat.insert(chat_id.to_string(), harvested);
        }
        result
    }

    fn run_ask_inner(&mut self, ask: &str) -> Result<RunOutcome, AskFailure> {
        let mut thread: Vec<String> = Vec::new();
        let mut transcript: Vec<String> = Vec::new();
        let fail = |message: String, thread: &Vec<String>, transcript: &Vec<String>| AskFailure {
            message,
            thread: thread.clone(),
            transcript: transcript.clone(),
        };
        if !self.model.is_configured() {
            return Err(fail(
                "no model key configured: set OPENRO_MODEL_API_KEY to enable chat".to_string(),
                &thread,
                &transcript,
            ));
        }
        if self.studio.is_none() {
            return Err(fail(
                "Studio not connected: set OPENRO_MCP_COMMAND to a Studio MCP server".to_string(),
                &thread,
                &transcript,
            ));
        }
        if let Err(e) = self.check_bridge_capabilities() {
            return Err(fail(e, &thread, &transcript));
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
        // and whether this ask changed or proved anything at all (the
        // claim flag on an empty-handed "done").
        let mut unproven_changes = false;
        let mut changes_made = 0usize;
        let mut playtest_passed = false;

        for _ in 0..MAX_STEPS {
            let content = match self.model.chat(&messages) {
                Ok(c) => c,
                Err(e) => return Err(fail(e, &thread, &transcript)),
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
                if changes_made == 0 && !playtest_passed {
                    let lower = s.to_lowercase();
                    let claims_building = ["built", "created", "wrote", "added", "placed", "generated"]
                        .iter()
                        .any(|w| lower.contains(w));
                    if claims_building {
                        thread.push(
                            "OpenRO: Note — that summary claims building work, but this ask made no changes and ran no playtest."
                                .to_string(),
                        );
                    }
                }
                transcript.push(format!("done: {s}"));
                summary = Some(s);
                break;
            }

            if let Some(skill) = step.get("skill").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or(json!({}));
                match self.run_skill(skill, &args, &mut transcript, &mut thread) {
                    Ok(note) => messages.push(json!({ "role": "user", "content": note })),
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
                            playtest_passed = true;
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
                                    &thread,
                                    &transcript,
                                ));
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
                                    &thread,
                                    &transcript,
                                ));
                            }
                            messages.push(json!({ "role": "user", "content": note }));
                        }
                        Err(e) => return Err(fail(e, &thread, &transcript)),
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
            }),
            None => Err(fail(
                format!("agent loop did not finish within {MAX_STEPS} steps"),
                &thread,
                &transcript,
            )),
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
        self.checkpoint_script(&path)?;
        self.studio_mut()?
            .call_tool("write_script", json!({ "path": path, "source": source }))?;
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
                // Feed the ACTUAL console output back to the model.
                let console = self.studio_mut()?.call_tool("get_console", json!({}))?;
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
                self.checkpoint_script(&path)?;
                self.studio_mut()?
                    .call_tool("delete_script", json!({ "path": path }))?;
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
                if name.contains('.') {
                    return Err(format!(
                        "create_instance name must be a single name without dots, got: {name}"
                    ));
                }
                check_path(&parent)?;
                let path = format!("{parent}.{name}");
                self.checkpoint_instance(&path)?;
                self.studio_mut()?
                    .call_tool("create_instance", args.clone())?;
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
                self.checkpoint_instance(&path)?;
                self.studio_mut()?
                    .call_tool("delete_instance", json!({ "path": path }))?;
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

    /// Record the previous source of `path` (or its absence) the first
    /// time this ask touches it.
    fn checkpoint_script(&mut self, path: &str) -> Result<(), String> {
        if self
            .current_changes
            .iter()
            .any(|c| matches!(c, Change::Script(p, _) if p == path))
        {
            return Ok(());
        }
        let tree = self.studio_mut()?.get_place_tree()?;
        let prev = tree
            .pointer("/scripts")
            .and_then(|s| s.get(path))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        self.current_changes.push(Change::Script(path.to_string(), prev));
        Ok(())
    }

    /// Record the previous instance record at `path` (or its absence)
    /// the first time this ask touches it.
    fn checkpoint_instance(&mut self, path: &str) -> Result<(), String> {
        if self
            .current_changes
            .iter()
            .any(|c| matches!(c, Change::Instance(p, _) if p == path))
        {
            return Ok(());
        }
        let tree = self.studio_mut()?.get_place_tree()?;
        let prev = tree
            .pointer("/instances")
            .and_then(|s| s.get(path))
            .cloned();
        self.current_changes.push(Change::Instance(path.to_string(), prev));
        Ok(())
    }

    /// Restore one chat's change set: rewrite previous content, delete
    /// what the ask created — scripts and instances alike. Returns the
    /// restored paths. An empty (or absent) set is NOTHING_TO_REVERT,
    /// never a fabricated success.
    pub fn revert_chat(&mut self, chat_id: &str) -> Result<Vec<String>, String> {
        if self.studio.is_none() {
            return Err("Studio not connected: nothing to revert".to_string());
        }
        let changes = match self.changes_by_chat.remove(chat_id) {
            Some(c) if !c.is_empty() => c,
            _ => return Err(NOTHING_TO_REVERT.to_string()),
        };
        let mut restored = Vec::new();
        for change in changes.iter().rev() {
            match change {
                Change::Script(path, prev) => match prev {
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
                },
                Change::Instance(path, prev) => match prev {
                    Some(record) => {
                        let (parent, name) = match path.rsplit_once('.') {
                            Some((p, n)) => (p.to_string(), n.to_string()),
                            None => (String::new(), path.clone()),
                        };
                        // Restore = replace whatever is there now with
                        // the previous record. Create-on-existing is an
                        // error on a strict bridge, so delete first
                        // (a missing instance is fine — the create is
                        // the step that must succeed).
                        let _ = self
                            .studio_mut()?
                            .call_tool("delete_instance", json!({ "path": path }));
                        self.studio_mut()?.call_tool(
                            "create_instance",
                            json!({
                                "class_name": record.get("class_name").cloned().unwrap_or(json!("Part")),
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
                },
            }
            restored.push(match change {
                Change::Script(p, _) => p.clone(),
                Change::Instance(p, _) => p.clone(),
            });
        }
        Ok(restored)
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
