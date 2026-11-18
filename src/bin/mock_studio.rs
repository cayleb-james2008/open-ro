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
            "tools/list" => {
                // MOCK_DROP=<tool> simulates a bridge that lacks a tool.
                let drop = std::env::var("MOCK_DROP").unwrap_or_default();
                let all = [
                    ("get_place_tree", "List scripts and instances in the place"),
                    ("write_script", "Create or overwrite a script"),
                    ("delete_script", "Delete a script"),
                    ("create_instance", "Create an instance (class, name, parent, properties)"),
                    ("delete_instance", "Delete an instance"),
                    ("playtest", "Run the place"),
                    ("get_console", "Read the console output"),
                ];
                let tools: Vec<Value> = all
                    .iter()
                    .filter(|(name, _)| *name != drop)
                    .map(|(name, desc)| json!({ "name": name, "description": desc }))
                    .collect();
                json!({ "tools": tools })
            }
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

/// A tool failure, reported the way a real MCP server reports it:
/// `isError: true`. (This fixture used to return errors as successful
/// text with an "error" key — which taught the client to swallow them.)
fn error_result(text: String) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": true
    })
}

fn bad_path(path: &str) -> bool {
    path.is_empty() || path.split('.').any(|seg| seg.is_empty() || seg == "..")
}

fn tool_call(
    name: &str,
    args: &Value,
    scripts: &mut BTreeMap<String, String>,
    instances: &mut BTreeMap<String, Value>,
    console: &mut String,
) -> Value {
    // MOCK_ERR_TOOLS=1: every tool call fails, like a broken bridge.
    if std::env::var("MOCK_ERR_TOOLS").is_ok() {
        return error_result(format!("bridge exploded: {name}"));
    }
    // A dropped tool (MOCK_DROP) errors when called, like a real bridge.
    if std::env::var("MOCK_DROP").map(|d| d == name).unwrap_or(false) {
        return error_result(format!("unknown tool {name}"));
    }
    // MOCK_ERR_CONSOLE=1: only get_console fails (a console subsystem
    // outage while playtests themselves still answer).
    if name == "get_console" && std::env::var("MOCK_ERR_CONSOLE").is_ok() {
        return error_result("console subsystem exploded".to_string());
    }
    match name {
        "get_place_tree" => {
            text_result(json!({ "scripts": scripts, "instances": instances }).to_string())
        }
        "write_script" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if bad_path(path) {
                return error_result(format!("invalid script path: {path}"));
            }
            let source = args.get("source").and_then(|v| v.as_str()).unwrap_or("");
            scripts.insert(path.to_string(), source.to_string());
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "delete_script" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if scripts.remove(path).is_none() {
                return error_result(format!("no script at {path}"));
            }
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "create_instance" => {
            let class_name = args.get("class_name").and_then(|v| v.as_str()).unwrap_or("");
            let inst_name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let parent = args.get("parent").and_then(|v| v.as_str()).unwrap_or("");
            let properties = args.get("properties").cloned().unwrap_or(json!({}));
            let path = format!("{parent}.{inst_name}");
            if bad_path(&path) {
                return error_result(format!("invalid instance path: {path}"));
            }
            // Strict, like a real place: creating over an existing
            // instance is an error, not a silent overwrite.
            if instances.contains_key(&path) {
                return error_result(format!("instance already exists at {path}"));
            }
            instances.insert(
                path.clone(),
                json!({ "class_name": class_name, "properties": properties }),
            );
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "delete_instance" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if instances.remove(path).is_none() {
                return error_result(format!("no instance at {path}"));
            }
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
            let (passed, msg) = if references_round && !creates_round {
                (false, FAIL_CONSOLE)
            } else if references_spawn && !has_spawn {
                (false, SPAWN_FAIL_CONSOLE)
            } else {
                (true, OK_CONSOLE)
            };
            *console = msg.to_string();
            // MOCK_PLAYTEST_TEXT=1: verdict as loose plain text, the
            // shape that used to fool the client's substring heuristic.
            if std::env::var("MOCK_PLAYTEST_TEXT").is_ok() {
                return text_result(if passed {
                    "Playtest passed. 0 failures, 0 errors.".to_string()
                } else {
                    format!("Playtest failed. {msg}")
                });
            }
            // MOCK_ERR_CODE_ONLY=1: a passing verdict carrying an
            // error OBJECT with a code but no message — a healthy
            // shape some bridges emit.
            if std::env::var("MOCK_ERR_CODE_ONLY").is_ok() {
                return text_result(
                    json!({ "status": "passed", "console": msg, "error": { "code": 0 } })
                        .to_string(),
                );
            }
            // MOCK_NULL_ERROR=1: healthy payloads that also carry an
            // "error": null field, as several real bridges emit.
            if std::env::var("MOCK_NULL_ERROR").is_ok() {
                return text_result(
                    json!({ "status": if passed { "passed" } else { "failed" }, "console": msg, "error": null })
                        .to_string(),
                );
            }
            text_result(
                json!({ "status": if passed { "passed" } else { "failed" }, "console": msg })
                    .to_string(),
            )
        }
        "get_console" => text_result(console.clone()),
        other => error_result(format!("unknown tool {other}")),
    }
}
