//! Studio bridge: an MCP client speaking JSON-RPC 2.0 over stdio to a
//! child process (a Roblox Studio MCP server). The child command comes
//! from `OPENRO_MCP_COMMAND`, so a real Studio MCP server drops in
//! unchanged. Performs the `initialize` handshake, then `tools/call`.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct McpClient {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
    /// Tool names the server advertised via `tools/list` at connect.
    tools: Vec<String>,
    /// False when the server did not answer `tools/list` at all — its
    /// capabilities are then unknown, and the agent must not refuse an
    /// ask on a guess.
    tools_known: bool,
}

impl McpClient {
    /// Spawn the server command (whitespace-split: program + args) and run
    /// the MCP initialize handshake.
    pub fn spawn(command: &str) -> Result<McpClient, String> {
        let mut parts = command.split_whitespace();
        let program = parts
            .next()
            .ok_or_else(|| "empty MCP command".to_string())?;
        let args: Vec<&str> = parts.collect();
        let mut child = Command::new(program)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to spawn Studio MCP server '{program}': {e}"))?;
        let stdin = BufWriter::new(child.stdin.take().expect("stdin piped"));
        let stdout = BufReader::new(child.stdout.take().expect("stdout piped"));
        let mut client = McpClient {
            child,
            stdin,
            stdout,
            next_id: 1,
            tools: Vec::new(),
            tools_known: false,
        };
        client.initialize()?;
        client.discover_tools();
        Ok(client)
    }

    /// Fetch the server's tool list (`tools/list`). Best-effort: a server
    /// that does not answer leaves `tools_known` false.
    fn discover_tools(&mut self) {
        if let Ok(result) = self.request("tools/list", json!({})) {
            if let Some(items) = result.get("tools").and_then(|t| t.as_array()) {
                self.tools = items
                    .iter()
                    .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                    .collect();
                self.tools_known = true;
            }
        }
    }

    /// The tools the server advertised at connect (empty when unknown).
    pub fn tools(&self) -> &[String] {
        &self.tools
    }

    /// Whether `name` is usable: advertised, or the server's list is
    /// unknown (then the call itself will tell us).
    pub fn has_tool(&self, name: &str) -> bool {
        !self.tools_known || self.tools.iter().any(|t| t == name)
    }

    /// Whether the server's capabilities are known at all.
    pub fn tools_known(&self) -> bool {
        self.tools_known
    }

    /// Spawn from `OPENRO_MCP_COMMAND`; `Ok(None)` when it is not set.
    pub fn from_env() -> Result<Option<McpClient>, String> {
        match std::env::var("OPENRO_MCP_COMMAND") {
            Ok(cmd) if !cmd.trim().is_empty() => Ok(Some(McpClient::spawn(&cmd)?)),
            _ => Ok(None),
        }
    }

    fn initialize(&mut self) -> Result<(), String> {
        let result = self.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "openro", "version": "0.1.0" }
            }),
        )?;
        if result.get("serverInfo").is_none() {
            return Err("MCP initialize response had no serverInfo".to_string());
        }
        self.notify("notifications/initialized", json!({}))?;
        Ok(())
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        let line = serde_json::to_string(message)
            .map_err(|e| format!("serialise JSON-RPC: {e}"))?;
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("write to Studio MCP server: {e}"))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        loop {
            let mut line = String::new();
            let n = self
                .stdout
                .read_line(&mut line)
                .map_err(|e| format!("read from Studio MCP server: {e}"))?;
            if n == 0 {
                return Err("Studio MCP server closed its stdout".to_string());
            }
            let value: Value = match serde_json::from_str(line.trim()) {
                Ok(v) => v,
                Err(_) => continue, // skip non-JSON noise lines
            };
            if value.get("id").and_then(|i| i.as_i64()) != Some(id) {
                continue; // a notification or a reply to another request
            }
            if let Some(err) = value.get("error") {
                return Err(format!("MCP {method} error: {err}"));
            }
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    /// Call one MCP tool; returns the text of its first content block.
    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<String, String> {
        let result = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )?;
        let text = result
            .pointer("/content/0/text")
            .and_then(|t| t.as_str())
            .ok_or_else(|| format!("tool {name} returned no text content: {result}"))?;
        Ok(text.to_string())
    }

    /// The place tree as JSON: `{ "scripts": { "<path>": "<source>", ... } }`.
    pub fn get_place_tree(&mut self) -> Result<Value, String> {
        let text = self.call_tool("get_place_tree", json!({}))?;
        serde_json::from_str(&text)
            .map_err(|e| format!("get_place_tree returned non-JSON text: {e}"))
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
