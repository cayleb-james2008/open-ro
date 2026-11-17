//! The agent loop.
//!
//! One user ask runs a model-driven loop. The model replies with exactly
//! one JSON step at a time:
//!   {"skill": "<rune skill>", "args": {...}}            — generate Luau
//!   {"tool": "write_script"|"playtest"|"get_console"|"get_place_tree"|
//!            "delete_script", "args": {...}}            — Studio bridge
//!   {"done": true, "summary": "..."}                    — finish
//!
//! Before the first write to a path in an ask, the loop checkpoints that
//! path's previous content (or its absence); `revert_last` restores the
//! whole change set of the last ask. On a failed playtest the loop fetches
//! the actual console output and feeds it back to the model so it can
//! repair autonomously.

use crate::mcp::McpClient;
use crate::model::ModelClient;
use crate::skills;
use serde_json::{json, Value};

const MAX_STEPS: usize = 16;

const SYSTEM_PROMPT: &str = "You are OpenRO, a proactive creative agent that builds Roblox games. \
The user describes a game change in plain English and you build it. \
Reply with exactly ONE JSON object per turn, no prose outside the JSON. Steps: \
{\"skill\": \"round_manager\", \"args\": {\"create_round\": true|false}} generates the RoundManager Luau script; \
{\"tool\": \"write_script\", \"args\": {\"path\": \"ServerScriptService.RoundManager\", \"use_generated\": true}} writes the last generated script; \
{\"tool\": \"playtest\", \"args\": {}} runs the game; \
{\"tool\": \"get_console\", \"args\": {}} reads the console; \
{\"tool\": \"get_place_tree\", \"args\": {}} lists scripts; \
{\"done\": true, \"summary\": \"...\"} finishes. \
Always playtest after writing, and repair from the console output until the playtest passes.";

pub struct RunOutcome {
    pub summary: String,
    /// Plain-words lines for the chat thread, produced by this ask.
    pub thread: Vec<String>,
    /// Step-by-step record of what the loop did (skills, tools, console).
    pub transcript: Vec<String>,
}

pub struct Agent {
    pub model: ModelClient,
    pub studio: Option<McpClient>,
    last_generated: Option<String>,
    /// Change set of the last completed ask: (path, previous source or None).
    last_changes: Vec<(String, Option<String>)>,
    /// Change set being accumulated by the ask currently running.
    current_changes: Vec<(String, Option<String>)>,
}

impl Agent {
    pub fn new(model: ModelClient, studio: Option<McpClient>) -> Agent {
        Agent {
            model,
            studio,
            last_generated: None,
            last_changes: Vec::new(),
            current_changes: Vec::new(),
        }
    }

    pub fn studio_connected(&mut self) -> bool {
        match &mut self.studio {
            Some(s) => s.is_alive(),
            None => false,
        }
    }

    pub fn run_ask(&mut self, ask: &str) -> Result<RunOutcome, String> {
        if !self.model.is_configured() {
            return Err(
                "no model key configured: set OPENRO_MODEL_API_KEY to enable chat"
                    .to_string(),
            );
        }
        if self.studio.is_none() {
            return Err(
                "Studio not connected: set OPENRO_MCP_COMMAND to a Studio MCP server"
                    .to_string(),
            );
        }
        self.current_changes.clear();
        let mut thread = vec![format!("You: {ask}")];
        let mut transcript: Vec<String> = Vec::new();
        let mut messages = vec![
            json!({ "role": "system", "content": SYSTEM_PROMPT }),
            json!({ "role": "user", "content": ask }),
        ];
        let mut summary: Option<String> = None;

        for _ in 0..MAX_STEPS {
            let content = self.model.chat(&messages)?;
            messages.push(json!({ "role": "assistant", "content": content }));
            transcript.push(format!("model step: {content}"));
            let step = parse_step(&content)?;

            if step.get("done").and_then(|d| d.as_bool()) == Some(true) {
                let s = step
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Done.")
                    .to_string();
                thread.push(format!("OpenRO: {s}"));
                transcript.push(format!("done: {s}"));
                summary = Some(s);
                break;
            }

            if let Some(skill) = step.get("skill").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or(json!({}));
                let note = self.run_skill(skill, &args, &mut transcript, &mut thread)?;
                messages.push(json!({ "role": "user", "content": note }));
                continue;
            }

            if let Some(tool) = step.get("tool").and_then(|v| v.as_str()) {
                let args = step.get("args").cloned().unwrap_or(json!({}));
                let note = self.run_tool(tool, &args, &mut transcript, &mut thread)?;
                messages.push(json!({ "role": "user", "content": note }));
                continue;
            }

            messages.push(json!({
                "role": "user",
                "content": "That step was not recognised. Reply with one JSON step: a skill, a tool, or done."
            }));
        }

        self.last_changes = std::mem::take(&mut self.current_changes);
        match summary {
            Some(summary) => Ok(RunOutcome {
                summary,
                thread,
                transcript,
            }),
            None => Err(format!(
                "agent loop did not finish within {MAX_STEPS} steps"
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
        match skill {
            "round_manager" => {
                let create_round = args
                    .get("create_round")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                // The Luau is produced by executing the Rune skill.
                let source = skills::generate_round_manager(create_round)?;
                transcript.push(format!(
                    "skill round_manager (Rune) create_round={create_round} generated {} bytes of Luau",
                    source.len()
                ));
                thread.push("Generated the RoundManager script.".to_string());
                self.last_generated = Some(source.clone());
                Ok(format!(
                    "skill_result: the round_manager Rune skill generated this Luau source:\n{source}\n\
                     Next, write it with {{\"tool\":\"write_script\",\"args\":{{\"path\":\"ServerScriptService.RoundManager\",\"use_generated\":true}}}} and then playtest."
                ))
            }
            other => Err(format!("unknown skill: {other}")),
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
            "write_script" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("write_script needs args.path")?
                    .to_string();
                let source = match args.get("source").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => self
                        .last_generated
                        .clone()
                        .ok_or("write_script needs args.source or a prior skill result")?,
                };
                self.checkpoint(&path)?;
                self.studio_mut()?
                    .call_tool("write_script", json!({ "path": path, "source": source }))?;
                transcript.push(format!("tool write_script {path} ({} bytes)", source.len()));
                thread.push(format!("Wrote {path}."));
                Ok(format!(
                    "tool_result: write_script {path} succeeded. Next, playtest."
                ))
            }
            "delete_script" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or("delete_script needs args.path")?
                    .to_string();
                self.checkpoint(&path)?;
                self.studio_mut()?
                    .call_tool("delete_script", json!({ "path": path }))?;
                transcript.push(format!("tool delete_script {path}"));
                Ok(format!("tool_result: delete_script {path} succeeded."))
            }
            "playtest" => {
                let text = self.studio_mut()?.call_tool("playtest", json!({}))?;
                transcript.push(format!("tool playtest -> {text}"));
                let failed = playtest_failed(&text);
                if failed {
                    thread.push("Playtest failed — reading the console and repairing.".to_string());
                    // Feed the ACTUAL console output back to the model.
                    let console = self.studio_mut()?.call_tool("get_console", json!({}))?;
                    transcript.push(format!("console after failed playtest: {console}"));
                    Ok(format!(
                        "tool_result: playtest FAILED. Actual console output: {console}\n\
                         Repair the game from that console output (for example regenerate the \
                         script so it creates what it references), write it again, and playtest again."
                    ))
                } else {
                    thread.push("Playtest passed.".to_string());
                    Ok(format!(
                        "tool_result: playtest PASSED ({text}). You may finish with done."
                    ))
                }
            }
            "get_console" | "get_place_tree" => {
                let text = self.studio_mut()?.call_tool(tool, json!({}))?;
                transcript.push(format!("tool {tool} -> {text}"));
                Ok(format!("tool_result: {tool}: {text}"))
            }
            other => Err(format!("unknown tool: {other}")),
        }
    }

    /// Record the previous content of `path` (or its absence) the first
    /// time this ask touches it.
    fn checkpoint(&mut self, path: &str) -> Result<(), String> {
        if self.current_changes.iter().any(|(p, _)| p == path) {
            return Ok(());
        }
        let tree = self.studio_mut()?.get_place_tree()?;
        let prev = tree
            .pointer("/scripts")
            .and_then(|s| s.get(path))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        self.current_changes.push((path.to_string(), prev));
        Ok(())
    }

    /// Restore the last ask's change set: rewrite previous content, delete
    /// scripts the ask created. Returns the restored paths.
    pub fn revert_last(&mut self) -> Result<Vec<String>, String> {
        if self.studio.is_none() {
            return Err("Studio not connected: nothing to revert".to_string());
        }
        let changes = std::mem::take(&mut self.last_changes);
        let mut restored = Vec::new();
        for (path, prev) in changes.iter().rev() {
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
            restored.push(path.clone());
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

fn parse_step(content: &str) -> Result<Value, String> {
    let start = content
        .find('{')
        .ok_or_else(|| format!("model reply was not a JSON step: {content}"))?;
    let end = content
        .rfind('}')
        .ok_or_else(|| format!("model reply was not a JSON step: {content}"))?;
    serde_json::from_str(&content[start..=end])
        .map_err(|e| format!("model reply was not valid JSON: {e}"))
}

fn playtest_failed(result_text: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<Value>(result_text) {
        if let Some(status) = v.get("status").and_then(|s| s.as_str()) {
            return status != "passed";
        }
    }
    result_text.to_lowercase().contains("fail")
}
