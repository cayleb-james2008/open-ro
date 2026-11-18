//! Minimal mock Studio MCP server — a TEST FIXTURE, not product code.
//!
//! Speaks MCP-style JSON-RPC 2.0 over stdio like the full mock, but only
//! offers read-only tools (`get_place_tree`, `get_console`) — no
//! `write_script`, no `playtest`. It exists to prove that OpenRO checks
//! the bridge's advertised tool list and refuses honestly, naming the
//! missing tool, instead of failing obscurely mid-ask.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
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
                "serverInfo": { "name": "mock-studio-min", "version": "0.1.0" }
            }),
            "tools/list" => json!({
                "tools": [
                    { "name": "get_place_tree", "description": "List scripts and instances in the place" },
                    { "name": "get_console", "description": "Read the console output" }
                ]
            }),
            "tools/call" => {
                let name = request
                    .pointer("/params/name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                match name.as_str() {
                    "get_place_tree" => json!({
                        "content": [{ "type": "text", "text": json!({ "scripts": {}, "instances": {} }).to_string() }],
                        "isError": false
                    }),
                    "get_console" => json!({
                        "content": [{ "type": "text", "text": "Playtest OK: all scripts ran without errors" }],
                        "isError": false
                    }),
                    other => json!({
                        "content": [{ "type": "text", "text": json!({ "error": format!("unknown tool {other}") }).to_string() }],
                        "isError": false
                    }),
                }
            }
            _ => {
                if id.is_none() {
                    continue;
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
