//! Mock Roblox Studio MCP server — a TEST FIXTURE, not product code.
//!
//! Speaks MCP-style JSON-RPC 2.0 over stdio and holds an in-memory place
//! tree, so the real OpenRO MCP client can be exercised end to end with no
//! Studio present.
//!
//! Deterministic playtest rules (documented fixture behaviour):
//!   1. playtest FAILS with console error
//!        `ServerScriptService.RoundManager:4: attempt to index nil with 'Start'`
//!      if any written script contains `workspace.Round.` but no written
//!      script creates it (no `Name = "Round"` anywhere).
//!   2. playtest FAILS with console error
//!        `Workspace.SpawnPad is not a valid member of Workspace "Workspace"`
//!      if any written script references `workspace.SpawnPad` but no
//!      instance exists at `Workspace.SpawnPad`.
//!   Otherwise the playtest PASSES and `get_console` returns an OK line.
//!
//! The place tree holds scripts AND instances:
//! `{ "scripts": {path: source}, "instances": {path: {class_name, properties}} }`.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};

const FAIL_CONSOLE: &str =
    "ServerScriptService.RoundManager:4: attempt to index nil with 'Start'";
const SPAWN_FAIL_CONSOLE: &str =
    "Workspace.SpawnPad is not a valid member of Workspace \"Workspace\"";
const OK_CONSOLE: &str = "Playtest OK: all scripts ran without errors";

fn main() {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let mut scripts: BTreeMap<String, String> = BTreeMap::new();
    let mut instances: BTreeMap<String, Value> = BTreeMap::new();
    let mut console: String = OK_CONSOLE.to_string();

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break, // EOF: parent went away
            Ok(_) => {}
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = request.get("id").cloned();
        let method = request
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();

        let result: Value = match method.as_str() {
            "initialize" => json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "mock-studio", "version": "0.1.0" }
            }),
            "tools/list" => json!({
                "tools": [
                    { "name": "get_place_tree", "description": "List scripts and instances in the place" },
                    { "name": "write_script", "description": "Create or overwrite a script" },
                    { "name": "delete_script", "description": "Delete a script" },
                    { "name": "create_instance", "description": "Create an instance (class, name, parent, properties)" },
                    { "name": "delete_instance", "description": "Delete an instance" },
                    { "name": "playtest", "description": "Run the place" },
                    { "name": "get_console", "description": "Read the console output" }
                ]
            }),
            "tools/call" => {
                let name = request
                    .pointer("/params/name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = request
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or(json!({}));
                tool_call(&name, &args, &mut scripts, &mut instances, &mut console)
            }
            _ => {
                if id.is_none() {
                    continue; // unknown notification: no reply
                }
                json!(null)
            }
        };

        if let Some(id) = id {
            let response = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            });
            let _ = writeln!(out, "{response}");
            let _ = out.flush();
        }
    }
}

fn text_result(text: String) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false
    })
}

fn tool_call(
    name: &str,
    args: &Value,
    scripts: &mut BTreeMap<String, String>,
    instances: &mut BTreeMap<String, Value>,
    console: &mut String,
) -> Value {
    match name {
        "get_place_tree" => {
            text_result(json!({ "scripts": scripts, "instances": instances }).to_string())
        }
        "write_script" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let source = args.get("source").and_then(|v| v.as_str()).unwrap_or("");
            scripts.insert(path.to_string(), source.to_string());
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "delete_script" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            scripts.remove(path);
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "create_instance" => {
            let class_name = args.get("class_name").and_then(|v| v.as_str()).unwrap_or("");
            let inst_name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let parent = args.get("parent").and_then(|v| v.as_str()).unwrap_or("");
            let properties = args.get("properties").cloned().unwrap_or(json!({}));
            let path = format!("{parent}.{inst_name}");
            instances.insert(
                path.clone(),
                json!({ "class_name": class_name, "properties": properties }),
            );
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "delete_instance" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            instances.remove(path);
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "playtest" => {
            let references_round = scripts
                .values()
                .any(|s| s.contains("workspace.Round."));
            let creates_round = scripts
                .values()
                .any(|s| s.contains("Name = \"Round\""));
            let references_spawn = scripts
                .values()
                .any(|s| s.contains("workspace.SpawnPad"));
            let has_spawn = instances.contains_key("Workspace.SpawnPad");
            if references_round && !creates_round {
                *console = FAIL_CONSOLE.to_string();
                text_result(
                    json!({ "status": "failed", "console": FAIL_CONSOLE }).to_string(),
                )
            } else if references_spawn && !has_spawn {
                *console = SPAWN_FAIL_CONSOLE.to_string();
                text_result(
                    json!({ "status": "failed", "console": SPAWN_FAIL_CONSOLE }).to_string(),
                )
            } else {
                *console = OK_CONSOLE.to_string();
                text_result(
                    json!({ "status": "passed", "console": OK_CONSOLE }).to_string(),
                )
            }
        }
        "get_console" => text_result(console.clone()),
        other => text_result(json!({ "error": format!("unknown tool {other}") }).to_string()),
    }
}
