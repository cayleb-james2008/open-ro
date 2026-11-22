//! Studio bridge: an MCP client speaking JSON-RPC 2.0 over stdio to a
//! child process (a Roblox Studio MCP server). The child command comes
//! from `OPENRO_MCP_COMMAND` or embedded Studio detection, so a real
//! Roblox Studio MCP server drops in unchanged.
//!
//! Supports both newline-delimited JSON-RPC (NDJSON) and `Content-Length`
//! framed MCP stdio streams, automatically tracks the connected `studio_id`
//! and `datamodel_type` (`"Edit"` by default), and exposes all 12 official
//! Roblox Studio MCP tools plus `list_roblox_studios`.

use crate::assets::{decode_base64, detect_image_mime};
use crate::studio_stream::StudioFrame;
use serde_json::{json, Map, Value};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Official Roblox Studio MCP tools exposed by OpenRO v0.9.0.
pub const OFFICIAL_STUDIO_TOOLS: &[&str] = &[
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

/// Legacy / mock Studio bridge tools also supported transparently.
pub const LEGACY_STUDIO_TOOLS: &[&str] = &[
    "get_place_tree",
    "write_script",
    "delete_script",
    "create_instance",
    "delete_instance",
    "playtest",
    "get_console",
];

/// Returns `true` when `tool` is an allowlisted official or legacy Roblox Studio MCP tool.
pub fn is_allowed_studio_tool(tool: &str) -> bool {
    OFFICIAL_STUDIO_TOOLS.contains(&tool) || LEGACY_STUDIO_TOOLS.contains(&tool)
}

/// Structured catalog of all official Roblox Studio MCP tools for the UI drawer & agent prompt.
pub fn official_studio_tool_catalog() -> Vec<Value> {
    vec![
        json!({
            "name": "list_roblox_studios",
            "category": "session",
            "description": "List running Roblox Studio instances and their studio_id handles",
            "requires_studio_id": false,
            "requires_datamodel_type": false
        }),
        json!({
            "name": "get_studio_state",
            "category": "session",
            "description": "Read active Roblox Studio mode, available DataModels, and play state",
            "requires_studio_id": true,
            "requires_datamodel_type": false
        }),
        json!({
            "name": "search_game_tree",
            "category": "explorer",
            "description": "Search Instances in Workspace, ServerScriptService, ReplicatedStorage, or StarterGui",
            "requires_studio_id": true,
            "requires_datamodel_type": true
        }),
        json!({
            "name": "inspect_instance",
            "category": "explorer",
            "description": "Inspect detailed properties, attributes, and children of an Instance path",
            "requires_studio_id": true,
            "requires_datamodel_type": false
        }),
        json!({
            "name": "script_read",
            "category": "scripts",
            "description": "Read a Luau Script, LocalScript, or ModuleScript by dot path and line range",
            "requires_studio_id": true,
            "requires_datamodel_type": false
        }),
        json!({
            "name": "script_grep",
            "category": "scripts",
            "description": "Search across all Luau scripts in the open Roblox Studio place",
            "requires_studio_id": true,
            "requires_datamodel_type": false
        }),
        json!({
            "name": "multi_edit",
            "category": "scripts",
            "description": "Create or apply targeted multi-hunk edits to a Luau script in Studio",
            "requires_studio_id": true,
            "requires_datamodel_type": true
        }),
        json!({
            "name": "execute_luau",
            "category": "execution",
            "description": "Execute Luau code directly inside Roblox Studio's Edit, Client, or Server DataModel",
            "requires_studio_id": true,
            "requires_datamodel_type": true
        }),
        json!({
            "name": "get_console_output",
            "category": "diagnostics",
            "description": "Fetch live Roblox Studio Output window console lines and playtest errors",
            "requires_studio_id": true,
            "requires_datamodel_type": false
        }),
        json!({
            "name": "screen_capture",
            "category": "viewport",
            "description": "Capture the live Roblox Studio 3D viewport as a PNG frame",
            "requires_studio_id": true,
            "requires_datamodel_type": false
        }),
        json!({
            "name": "start_stop_play",
            "category": "playtest",
            "description": "Start Play, Run Server, or Stop the live Roblox Studio playtest session",
            "requires_studio_id": true,
            "requires_datamodel_type": false
        }),
        json!({
            "name": "user_mouse_input",
            "category": "input",
            "description": "Forward live pointer move, click, drag, and scroll actions into Studio",
            "requires_studio_id": true,
            "requires_datamodel_type": true
        }),
        json!({
            "name": "user_keyboard_input",
            "category": "input",
            "description": "Forward live key press, hold, release, and text input into Studio",
            "requires_studio_id": true,
            "requires_datamodel_type": true
        }),
    ]
}

/// How long one bridge request may take before it is declared wedged.
/// `OPENRO_BRIDGE_TIMEOUT_SECS` overrides for tests and slow real servers.
fn request_timeout() -> Duration {
    let secs = std::env::var("OPENRO_BRIDGE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|s| *s >= 1)
        .unwrap_or(30);
    Duration::from_secs(secs)
}

/// Split a command string into `[program, args...]` preserving quoted paths
/// (important on Windows when paths contain spaces like `Program Files`).
pub fn split_command_line(command: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for ch in command.chars() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            c if c.is_whitespace() && !in_single && !in_double => {
                if !cur.is_empty() {
                    parts.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

pub struct McpClient {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    /// JSON payloads from the bridge's stdout, delivered by a reader thread so
    /// requests can time out instead of blocking forever.
    lines: Receiver<String>,
    /// True once the server uses LSP/MCP `Content-Length` framing or when forced via env.
    content_length_framing: Arc<AtomicBool>,
    timeout: Duration,
    next_id: i64,
    /// Tool names the server advertised via `tools/list` at connect.
    tools: Vec<String>,
    /// False when the server did not answer `tools/list` at all — its
    /// capabilities are then unknown, and the agent must not refuse an
    /// ask on a guess.
    tools_known: bool,
    /// Active Roblox Studio instance handle discovered via `list_roblox_studios`.
    studio_id: Option<String>,
    /// Active Roblox DataModel target (`"Edit"` | `"Client"` | `"Server"`).
    datamodel_type: String,
    /// Monotonic counter used to construct `capture_id` for `screen_capture`.
    capture_seq: u64,
    /// Optional frame captured during translated official-MCP `playtest`.
    last_playtest_frame: Option<StudioFrame>,
}

impl McpClient {
    /// Spawn the server command (supporting quoted paths) and run the MCP initialize handshake.
    pub fn spawn(command: &str) -> Result<McpClient, String> {
        Self::spawn_with_timeout(command, request_timeout())
    }

    /// Spawn with an explicit per-request timeout.
    pub fn spawn_with_timeout(command: &str, timeout: Duration) -> Result<McpClient, String> {
        let parts = split_command_line(command);
        let program = parts
            .first()
            .ok_or_else(|| "empty MCP command".to_string())?;
        let args = &parts[1..];
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to spawn Studio MCP server '{program}': {e}"))?;
        let stdin = BufWriter::new(child.stdin.take().expect("stdin piped"));
        let stdout = child.stdout.take().expect("stdout piped");
        let (tx, rx) = channel::<String>();
        let cl_flag = Arc::new(AtomicBool::new(
            std::env::var("OPENRO_MCP_FRAMING")
                .ok()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("content-length")),
        ));
        let cl_reader = Arc::clone(&cl_flag);

        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        // Support both LSP/MCP `Content-Length: N\r\n\r\n{...}` and NDJSON lines.
                        if let Some(rest) = strip_prefix_ascii_ci(trimmed, "content-length:") {
                            if let Ok(mut content_len) = rest.trim().parse::<usize>() {
                                cl_reader.store(true, Ordering::Relaxed);
                                // Consume remaining header lines until blank separator line.
                                loop {
                                    line.clear();
                                    match reader.read_line(&mut line) {
                                        Ok(0) | Err(_) => return,
                                        Ok(_) => {
                                            let h = line.trim();
                                            if h.is_empty() {
                                                break;
                                            }
                                            if let Some(r2) =
                                                strip_prefix_ascii_ci(h, "content-length:")
                                            {
                                                if let Ok(n) = r2.trim().parse::<usize>() {
                                                    content_len = n;
                                                }
                                            }
                                        }
                                    }
                                }
                                if content_len == 0 || content_len > 32 * 1024 * 1024 {
                                    continue;
                                }
                                let mut body = vec![0u8; content_len];
                                if reader.read_exact(&mut body).is_err() {
                                    break;
                                }
                                if let Ok(text) = String::from_utf8(body) {
                                    if tx.send(text).is_err() {
                                        break;
                                    }
                                }
                                continue;
                            }
                        }
                        if tx.send(line.clone()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let mut client = McpClient {
            child,
            stdin,
            lines: rx,
            content_length_framing: cl_flag,
            timeout,
            next_id: 1,
            tools: Vec::new(),
            tools_known: false,
            studio_id: None,
            datamodel_type: "Edit".to_string(),
            capture_seq: 0,
            last_playtest_frame: None,
        };
        client.initialize()?;
        client.discover_tools();
        // Auto-discover `studio_id` ONLY when the server explicitly advertises `list_roblox_studios`
        // so minimal/legacy fixtures that don't implement it receive zero unexpected calls.
        if client.tools_known && client.tools.iter().any(|t| t == "list_roblox_studios") {
            let _ = client.list_roblox_studios();
        }
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

    /// Count of official 13 Roblox Studio MCP tools currently advertised by this connection.
    pub fn official_tools_available(&self) -> Vec<String> {
        OFFICIAL_STUDIO_TOOLS
            .iter()
            .filter(|name| self.tools.iter().any(|t| t == **name))
            .map(|s| (*s).to_string())
            .collect()
    }

    /// Whether `name` is usable: advertised, or the server's list is
    /// unknown (then the call itself will tell us).
    pub fn has_tool(&self, name: &str) -> bool {
        !self.tools_known || self.tools.iter().any(|t| t == name)
    }

    /// True only when `tools/list` succeeded AND explicitly advertised `name`.
    pub fn has_exact_tool(&self, name: &str) -> bool {
        self.tools_known && self.tools.iter().any(|t| t == name)
    }

    /// True when the connected MCP server exposes the official Studio MCP execution
    /// set (`execute_luau` + `start_stop_play` + `get_console_output`), enabling
    /// transparent translation of legacy place/script/playtest operations.
    pub fn has_official_bridge(&self) -> bool {
        self.has_exact_tool("execute_luau")
            && self.has_exact_tool("start_stop_play")
            && self.has_exact_tool("get_console_output")
    }

    /// Consume the most recent frame captured inside a translated official `playtest`.
    pub fn take_last_playtest_frame(&mut self) -> Option<StudioFrame> {
        self.last_playtest_frame.take()
    }

    /// Whether the server's capabilities are known at all.
    pub fn tools_known(&self) -> bool {
        self.tools_known
    }

    /// Currently tracked Roblox Studio `studio_id` (if discovered or set).
    pub fn studio_id(&self) -> Option<&str> {
        self.studio_id.as_deref()
    }

    /// Override or clear the tracked Roblox Studio `studio_id`.
    pub fn set_studio_id(&mut self, id: Option<String>) {
        self.studio_id = id.and_then(|s| {
            let t = s.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        });
    }

    /// Current Roblox DataModel target (`"Edit"`, `"Client"`, or `"Server"`).
    pub fn datamodel_type(&self) -> &str {
        &self.datamodel_type
    }

    /// Set the default Roblox DataModel target (`"Edit"`, `"Client"`, or `"Server"`).
    pub fn set_datamodel_type(&mut self, dm: &str) {
        let normalized = match dm.trim().to_ascii_lowercase().as_str() {
            "client" => "Client",
            "server" => "Server",
            _ => "Edit",
        };
        self.datamodel_type = normalized.to_string();
    }

    /// Spawn from `OPENRO_MCP_COMMAND`; `Ok(None)` when it is not set.
    pub fn from_env() -> Result<Option<McpClient>, String> {
        match std::env::var("OPENRO_MCP_COMMAND") {
            Ok(cmd) if !cmd.trim().is_empty() => Ok(Some(McpClient::spawn(&cmd)?)),
            _ => Ok(None),
        }
    }

    /// Passively detect a running Vinegar Flatpak Roblox Studio instance with
    /// `~/.local/bin/roblox-studio-mcp` installed. NEVER launches Vinegar or
    /// downloads anything. Returns `None` on non-Unix platforms.
    pub fn detect_vinegar_studio_mcp() -> Option<String> {
        #[cfg(unix)]
        {
            let home = std::env::var("HOME").ok()?;
            let candidate = std::path::PathBuf::from(home).join(".local/bin/roblox-studio-mcp");
            if !candidate.is_file() {
                return None;
            }
            let ps = Command::new("flatpak")
                .args(["ps", "--columns=instance,application"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()?;
            if !ps.status.success() {
                return None;
            }
            let stdout = String::from_utf8_lossy(&ps.stdout);
            let running = stdout.lines().any(|line| {
                let mut parts = line.split_whitespace();
                let _inst = parts.next();
                parts.next() == Some("org.vinegarhq.Vinegar")
            });
            if running {
                Some(candidate.to_string_lossy().into_owned())
            } else {
                None
            }
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// Cross-platform passive detection of a local Roblox Studio MCP command
    /// (running Vinegar on Linux, or `%LOCALAPPDATA%\Roblox\Versions\version-*\StudioMCP.exe`
    /// / `mcp.bat` on Windows). Never launches Studio or downloads anything.
    pub fn detect_studio_mcp_command() -> Option<String> {
        crate::studio_embed::detect_host_mcp_command()
    }

    fn initialize(&mut self) -> Result<(), String> {
        let result = self.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "openro", "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        if result.get("serverInfo").is_none() {
            return Err("MCP initialize response had no serverInfo".to_string());
        }
        self.notify("notifications/initialized", json!({}))?;
        Ok(())
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        let payload = serde_json::to_string(message)
            .map_err(|e| format!("serialise JSON-RPC: {e}"))?;
        if self.content_length_framing.load(Ordering::Relaxed) {
            let header = format!("Content-Length: {}\r\n\r\n", payload.len());
            self.stdin
                .write_all(header.as_bytes())
                .and_then(|_| self.stdin.write_all(payload.as_bytes()))
                .and_then(|_| self.stdin.flush())
                .map_err(|e| format!("write framed message to Studio MCP server: {e}"))
        } else {
            self.stdin
                .write_all(payload.as_bytes())
                .and_then(|_| self.stdin.write_all(b"\n"))
                .and_then(|_| self.stdin.flush())
                .map_err(|e| format!("write to Studio MCP server: {e}"))
        }
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
        let timeout = self.timeout;
        loop {
            let line = match self.lines.recv_timeout(timeout) {
                Ok(l) => l,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "Studio MCP server did not answer '{method}' within {}s — the bridge looks wedged",
                        timeout.as_secs()
                    ));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Studio MCP server closed its stdout".to_string());
                }
            };
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

    /// Inject tracked `studio_id`, `datamodel_type`, and `capture_id` into official tool args
    /// when not explicitly provided by the caller.
    fn prepare_official_args(&mut self, tool: &str, arguments: Value) -> Value {
        let mut obj = match arguments {
            Value::Object(map) => map,
            _ => Map::new(),
        };

        if tool != "list_roblox_studios" && !obj.contains_key("studio_id") {
            if self.studio_id.is_none()
                && self.tools_known
                && self.tools.iter().any(|t| t == "list_roblox_studios")
            {
                let _ = self.list_roblox_studios();
            }
            if let Some(id) = self.studio_id.as_deref() {
                obj.insert("studio_id".to_string(), json!(id));
            }
        }

        let needs_datamodel = matches!(
            tool,
            "search_game_tree"
                | "multi_edit"
                | "execute_luau"
                | "user_mouse_input"
                | "user_keyboard_input"
        );
        if needs_datamodel && !obj.contains_key("datamodel_type") {
            obj.insert("datamodel_type".to_string(), json!(self.datamodel_type));
        }

        if tool == "screen_capture" && !obj.contains_key("capture_id") {
            self.capture_seq += 1;
            obj.insert(
                "capture_id".to_string(),
                json!(format!("ScreenCapture_{}", self.capture_seq)),
            );
        }

        Value::Object(obj)
    }

    /// Call one MCP tool; returns the text of its first content block.
    ///
    /// When connected to an official Roblox Studio MCP server (`execute_luau`,
    /// `start_stop_play`, `get_console_output`) that does not advertise legacy
    /// tool names (`get_place_tree`, `write_script`, `write_luau`, `create_instance`,
    /// `delete_instance`, `delete_script`, `playtest`, `get_console`), transparently
    /// translates those operations onto the official Studio MCP tools.
    ///
    /// A tool-level failure is an Err here, never a success string: the
    /// MCP `isError` flag is honoured, and so is an `{"error": ...}`
    /// JSON payload (some bridges report failures that way).
    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<String, String> {
        if self.tools_known && !self.has_exact_tool(name) {
            match name {
                "get_place_tree" if self.has_exact_tool("execute_luau") => {
                    return self.get_place_tree().map(|v| v.to_string());
                }
                "write_script" | "write_luau" if self.has_exact_tool("execute_luau") => {
                    let path = arguments
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| format!("{name} requires string path"))?;
                    let source = arguments
                        .get("source")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| format!("{name} requires string source"))?;
                    return self.official_write_script(path, source);
                }
                "delete_script" | "delete_instance" if self.has_exact_tool("execute_luau") => {
                    let path = arguments
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| format!("{name} requires string path"))?;
                    return self.official_delete_instance(path);
                }
                "create_instance" if self.has_exact_tool("execute_luau") => {
                    let class_name = arguments
                        .get("class_name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "create_instance requires string class_name".to_string())?;
                    let inst_name = arguments
                        .get("name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "create_instance requires string name".to_string())?;
                    let parent = arguments
                        .get("parent")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "create_instance requires string parent".to_string())?;
                    let props = arguments
                        .get("properties")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    return self.official_create_instance(class_name, inst_name, parent, &props);
                }
                "playtest" if self.has_exact_tool("start_stop_play") => {
                    return self.official_playtest();
                }
                "get_console" if self.has_exact_tool("get_console_output") => {
                    return self.get_console_output().map(|lines| lines.join("\n"));
                }
                _ => {}
            }
        }
        self.call_tool_raw(name, arguments)
    }

    fn call_tool_raw(&mut self, name: &str, arguments: Value) -> Result<String, String> {
        let result = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )?;
        let text = result
            .pointer("/content/0/text")
            .and_then(|t| t.as_str())
            .ok_or_else(|| format!("tool {name} returned no text content: {result}"))?;
        if result.get("isError").and_then(|v| v.as_bool()) == Some(true) {
            return Err(format!("Studio bridge error in {name}: {text}"));
        }
        if let Ok(payload) = serde_json::from_str::<Value>(text) {
            match payload.get("error") {
                Some(Value::String(s)) if !s.is_empty() => {
                    return Err(format!("Studio bridge error in {name}: {s}"));
                }
                Some(Value::Object(o)) => {
                    if let Some(msg) = o
                        .get("message")
                        .and_then(|m| m.as_str())
                        .filter(|m| !m.is_empty())
                    {
                        return Err(format!("Studio bridge error in {name}: {msg}"));
                    }
                }
                _ => {}
            }
        }
        Ok(text.to_string())
    }

    fn call_tool_json(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let prepared = self.prepare_official_args(name, arguments);
        let text = self.call_tool(name, prepared)?;
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            Ok(v)
        } else {
            Ok(json!({ "ok": true, "output": text }))
        }
    }

    /// Unified dispatcher for any of the 13 official (or legacy) Studio MCP tools.
    /// Rejects unknown/unlisted tool names cleanly and auto-injects `studio_id` & `datamodel_type`.
    pub fn call_official_tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let clean = name.trim();
        if !is_allowed_studio_tool(clean) {
            return Err(format!(
                "unsupported Studio MCP tool '{clean}' (must be one of the official Roblox Studio tools)"
            ));
        }
        match clean {
            "list_roblox_studios" => self.list_roblox_studios(),
            "get_studio_state" => self.get_studio_state(),
            "screen_capture" => {
                let cam = arguments.get("camera_position").and_then(parse_vec3_json);
                let look = arguments.get("look_at_position").and_then(parse_vec3_json);
                let frame = self.screen_capture_with_camera(cam, look)?;
                Ok(json!({
                    "ok": true,
                    "frame": frame.meta_json(),
                    "width": frame.width,
                    "height": frame.height,
                    "mime": frame.mime,
                    "bytes": frame.bytes.len(),
                }))
            }
            "start_stop_play" => {
                let mode = if let Some(m) = arguments.get("mode").and_then(|v| v.as_str()) {
                    m.to_string()
                } else if let Some(is_start) = arguments.get("is_start").and_then(|v| v.as_bool()) {
                    if is_start {
                        "start_play".to_string()
                    } else {
                        "stop".to_string()
                    }
                } else {
                    "start_play".to_string()
                };
                self.start_stop_play(&mode)
            }
            "get_console_output" => {
                let lines = self.get_console_output()?;
                Ok(json!({
                    "ok": true,
                    "lines": lines,
                    "count": lines.len(),
                }))
            }
            _ => self.call_tool_json(clean, arguments),
        }
    }

    /// 1. `list_roblox_studios` — list active Roblox Studio instances and update tracked `studio_id`.
    pub fn list_roblox_studios(&mut self) -> Result<Value, String> {
        let val = self.call_tool_json("list_roblox_studios", json!({}))?;
        if let Some(id) = extract_first_studio_id(&val) {
            self.studio_id = Some(id);
        }
        Ok(val)
    }

    /// 2. `get_studio_state` — query Studio play mode and available DataModels.
    pub fn get_studio_state(&mut self) -> Result<Value, String> {
        let val = self.call_tool_json("get_studio_state", json!({}))?;
        if let Some(dm) = val.get("active_datamodel").and_then(|v| v.as_str()) {
            self.set_datamodel_type(dm);
        }
        Ok(val)
    }

    /// 3. `search_game_tree` — search the Studio DataModel hierarchy.
    pub fn search_game_tree(
        &mut self,
        path: Option<&str>,
        keywords: Option<&str>,
        instance_type: Option<&str>,
        max_depth: Option<u32>,
        head_limit: Option<u32>,
    ) -> Result<Value, String> {
        let mut args = Map::new();
        if let Some(p) = path.filter(|s| !s.trim().is_empty()) {
            args.insert("path".to_string(), json!(p));
        }
        if let Some(kw) = keywords.filter(|s| !s.trim().is_empty()) {
            args.insert("keywords".to_string(), json!(kw));
        }
        if let Some(it) = instance_type.filter(|s| !s.trim().is_empty()) {
            args.insert("instance_type".to_string(), json!(it));
        }
        if let Some(md) = max_depth {
            args.insert("max_depth".to_string(), json!(md));
        }
        if let Some(hl) = head_limit {
            args.insert("head_limit".to_string(), json!(hl));
        }
        self.call_tool_json("search_game_tree", Value::Object(args))
    }

    /// 4. `inspect_instance` — inspect properties, attributes, and children of an Instance.
    pub fn inspect_instance(&mut self, path: &str) -> Result<Value, String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("inspect_instance requires a non-empty instance path".to_string());
        }
        self.call_tool_json("inspect_instance", json!({ "path": trimmed }))
    }

    /// 5. `script_read` — read a Luau script by path and optional 1-indexed line range.
    pub fn script_read(
        &mut self,
        target_file: &str,
        start_line: Option<u32>,
        end_line: Option<u32>,
    ) -> Result<Value, String> {
        let trimmed = target_file.trim();
        if trimmed.is_empty() {
            return Err("script_read requires a non-empty target_file path".to_string());
        }
        let mut args = Map::new();
        args.insert("target_file".to_string(), json!(trimmed));
        match (start_line, end_line) {
            (Some(s), Some(e)) => {
                args.insert("should_read_entire_file".to_string(), json!(false));
                args.insert("start_line_one_indexed".to_string(), json!(s.max(1)));
                args.insert("end_line_one_indexed_inclusive".to_string(), json!(e.max(s)));
            }
            _ => {
                args.insert("should_read_entire_file".to_string(), json!(true));
            }
        }
        self.call_tool_json("script_read", Value::Object(args))
    }

    /// 6. `script_grep` — search across all Luau scripts in the open place.
    pub fn script_grep(&mut self, query: &str) -> Result<Value, String> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err("script_grep requires a non-empty query".to_string());
        }
        self.call_tool_json("script_grep", json!({ "query": trimmed }))
    }

    /// 7. `multi_edit` — create or apply multi-hunk string edits to a Studio Luau script.
    pub fn multi_edit(
        &mut self,
        file_path: &str,
        edits: Vec<Value>,
        class_name: Option<&str>,
    ) -> Result<Value, String> {
        let trimmed = file_path.trim();
        if trimmed.is_empty() {
            return Err("multi_edit requires a non-empty file_path".to_string());
        }
        if edits.is_empty() {
            return Err("multi_edit requires at least one edit operation".to_string());
        }
        let mut args = Map::new();
        args.insert("file_path".to_string(), json!(trimmed));
        args.insert("edits".to_string(), Value::Array(edits));
        if let Some(cls) = class_name.filter(|s| !s.trim().is_empty()) {
            args.insert("className".to_string(), json!(cls));
        }
        self.call_tool_json("multi_edit", Value::Object(args))
    }

    /// 8. `execute_luau` — execute Luau code inside Roblox Studio's active DataModel.
    pub fn execute_luau(&mut self, code: &str) -> Result<Value, String> {
        if code.trim().is_empty() {
            return Err("execute_luau requires non-empty Luau code".to_string());
        }
        self.call_tool_json("execute_luau", json!({ "code": code }))
    }

    /// Read structured Studio Output console lines via `get_console_output`
    /// (falling back to `get_console` ONLY when the bridge explicitly advertises `get_console`,
    /// using `call_tool_raw` so legacy translation in `call_tool` can never recurse).
    pub fn get_console_output(&mut self) -> Result<Vec<String>, String> {
        let use_official = self.has_exact_tool("get_console_output")
            || (!self.tools_known && self.has_tool("get_console_output"));
        let raw = if use_official {
            let args = self.prepare_official_args("get_console_output", json!({}));
            match self.call_tool_raw("get_console_output", args) {
                Ok(text) => text,
                Err(_) if self.has_exact_tool("get_console") => {
                    self.call_tool_raw("get_console", json!({}))?
                }
                Err(e) => {
                    let low = e.to_ascii_lowercase();
                    if low.contains("no console") || low.contains("empty") {
                        return Ok(Vec::new());
                    }
                    return Err(e);
                }
            }
        } else {
            self.call_tool_raw("get_console", json!({}))?
        };
        if let Ok(val) = serde_json::from_str::<Value>(raw.trim()) {
            if let Some(arr) = val.as_array() {
                return Ok(arr
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect());
            }
            if let Some(arr) = val.get("lines").and_then(|v| v.as_array()) {
                return Ok(arr
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect());
            }
            for key in ["output", "console", "text"] {
                if let Some(s) = val.get(key).and_then(|v| v.as_str()) {
                    return Ok(s
                        .lines()
                        .map(str::trim_end)
                        .filter(|l| !l.is_empty())
                        .map(ToString::to_string)
                        .collect());
                }
            }
        }
        Ok(raw
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty())
            .map(ToString::to_string)
            .collect())
    }

    /// 10. `screen_capture` — capture a live screenshot frame from Roblox Studio.
    pub fn screen_capture(&mut self) -> Result<StudioFrame, String> {
        self.screen_capture_with_camera(None, None)
    }

    /// Capture a live screenshot frame with optional Studio camera override coordinates.
    pub fn screen_capture_with_camera(
        &mut self,
        camera_position: Option<[f64; 3]>,
        look_at_position: Option<[f64; 3]>,
    ) -> Result<StudioFrame, String> {
        if !self.has_tool("screen_capture") {
            return Err(
                "Connected Studio MCP bridge does not advertise the 'screen_capture' tool"
                    .to_string(),
            );
        }
        let mut args = Map::new();
        if let Some(cam) = camera_position {
            args.insert("camera_position".to_string(), json!(cam));
        }
        if let Some(look) = look_at_position {
            args.insert("look_at_position".to_string(), json!(look));
        }
        let prepared = self.prepare_official_args("screen_capture", Value::Object(args));

        let result = self.request(
            "tools/call",
            json!({ "name": "screen_capture", "arguments": prepared }),
        )?;
        if result.get("isError").and_then(|v| v.as_bool()) == Some(true) {
            let msg = result
                .pointer("/content/0/text")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown screen_capture error");
            return Err(format!("Studio bridge error in screen_capture: {msg}"));
        }

        let content = result
            .get("content")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("screen_capture returned no content array: {result}"))?;

        let mut b64_data: Option<String> = None;
        let mut hinted_w: Option<u32> = None;
        let mut hinted_h: Option<u32> = None;

        for block in content {
            let kind = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if kind == "image" {
                if let Some(data) = block.get("data").and_then(|v| v.as_str()) {
                    b64_data = Some(data.to_string());
                }
            } else if kind == "text" {
                if let Some(txt) = block.get("text").and_then(|v| v.as_str()) {
                    let trimmed = txt.trim();
                    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                        if let Some(err_s) = parsed
                            .get("error")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            return Err(format!("Studio bridge error in screen_capture: {err_s}"));
                        }
                        if hinted_w.is_none() {
                            hinted_w = parsed
                                .get("width")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as u32);
                        }
                        if hinted_h.is_none() {
                            hinted_h = parsed
                                .get("height")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as u32);
                        }
                        if b64_data.is_none() {
                            for key in ["data", "image", "image_base64", "png_base64", "data_url"] {
                                if let Some(s) = parsed.get(key).and_then(|v| v.as_str()) {
                                    if !s.trim().is_empty() {
                                        b64_data = Some(s.to_string());
                                        break;
                                    }
                                }
                            }
                        }
                    } else if b64_data.is_none() && trimmed.starts_with("data:image/") {
                        b64_data = Some(trimmed.to_string());
                    }
                }
            }
        }

        let raw_b64 = b64_data.ok_or_else(|| {
            "screen_capture returned no image base64 payload in MCP content blocks".to_string()
        })?;
        let bytes = decode_base64(&raw_b64)
            .map_err(|e| format!("screen_capture base64 decode failed: {e}"))?;
        let (mime, _) = detect_image_mime(&bytes)
            .map_err(|e| format!("screen_capture returned invalid image data: {e}"))?;
        let (parsed_w, parsed_h) = parse_image_dimensions(&bytes).unwrap_or((0, 0));
        let width = if parsed_w > 0 {
            parsed_w
        } else {
            hinted_w.unwrap_or(0)
        };
        let height = if parsed_h > 0 {
            parsed_h
        } else {
            hinted_h.unwrap_or(0)
        };
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Ok(StudioFrame {
            seq: 0,
            ts_ms,
            mime: mime.to_string(),
            bytes,
            width,
            height,
            source: "studio_mcp".to_string(),
        })
    }

    /// Control Roblox Studio playtest execution via `start_stop_play`
    /// (`"start_play"`, `"run_server"`, or `"stop"`). Emits both `mode` and
    /// official `is_start: bool` + `studio_id` so both real `StudioMCP.exe` and
    /// local mock servers accept it seamlessly.
    pub fn start_stop_play(&mut self, mode: &str) -> Result<Value, String> {
        let normalized = match mode.trim().to_ascii_lowercase().as_str() {
            "start_play" | "play" | "start" => "start_play",
            "run_server" | "server" | "run" => "run_server",
            "stop" | "stop_play" => "stop",
            other => {
                return Err(format!(
                    "unsupported start_stop_play mode '{other}' (expected start_play, run_server, or stop)"
                ));
            }
        };
        let is_start = normalized != "stop";
        if self.has_tool("start_stop_play") {
            let prepared = self.prepare_official_args(
                "start_stop_play",
                json!({
                    "mode": normalized,
                    "is_start": is_start,
                }),
            );
            let text = self.call_tool("start_stop_play", prepared)?;
            self.datamodel_type = match normalized {
                "start_play" => "Client".to_string(),
                "run_server" => "Server".to_string(),
                _ => "Edit".to_string(),
            };
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                return Ok(v);
            }
            return Ok(json!({
                "ok": true,
                "mode": normalized,
                "is_start": is_start,
                "playing": is_start,
                "output": text,
            }));
        }
        if !is_start {
            self.datamodel_type = "Edit".to_string();
            return Ok(json!({
                "ok": true,
                "mode": "stop",
                "is_start": false,
                "playing": false,
                "status": "stopped",
            }));
        }
        let text = self.call_tool("playtest", json!({}))?;
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            return Ok(v);
        }
        Ok(json!({
            "ok": true,
            "mode": normalized,
            "is_start": true,
            "output": text,
        }))
    }

    /// 12. `user_mouse_input` — forward ordered mouse actions into Studio.
    pub fn user_mouse_input(&mut self, actions: Vec<Value>) -> Result<Value, String> {
        if actions.is_empty() {
            return Err("user_mouse_input requires at least one mouse action".to_string());
        }
        self.call_tool_json("user_mouse_input", json!({ "actions": actions }))
    }

    /// 13. `user_keyboard_input` — forward ordered keyboard actions into Studio.
    pub fn user_keyboard_input(&mut self, actions: Vec<Value>) -> Result<Value, String> {
        if actions.is_empty() {
            return Err("user_keyboard_input requires at least one keyboard action".to_string());
        }
        self.call_tool_json("user_keyboard_input", json!({ "actions": actions }))
    }

    /// The place tree as JSON: `{ "scripts": { "<path>": "<source>", ... }, "instances": { ... } }`.
    /// Uses `get_place_tree` when advertised (e.g. `mock_studio`), and translates cleanly
    /// to budget-safe chunked read-only `execute_luau` DataModel walks across 7 Roblox services
    /// (`Workspace`, `ServerScriptService`, `ReplicatedStorage`, `StarterGui`, `StarterPlayer`,
    /// `Lighting`, `SoundService`) when attached to official Roblox `StudioMCP.exe` — staying
    /// strictly beneath StudioMCP's 100,000-character return truncation limit so large places
    /// sync cleanly into Explorer with zero JSON truncation.
    pub fn get_place_tree(&mut self) -> Result<Value, String> {
        if self.has_tool("get_place_tree") {
            let text = self.call_tool_raw("get_place_tree", json!({}))?;
            return serde_json::from_str(&text)
                .map_err(|e| format!("get_place_tree returned non-JSON text: {e}"));
        }
        if !self.has_tool("execute_luau") {
            return Ok(json!({ "scripts": {}, "instances": {} }));
        }

        // Phase 1: Walk instances (budget-capped under 68 KiB JSON so StudioMCP's 100k char
        // return limit never truncates the response) + collect ordered script paths.
        let walk_luau = r#"
local HttpService = game:GetService("HttpService")
local instances = {}
local scriptPaths = {}
local icount = 0
local budget = 0
local function fullPath(inst)
    local parts = {}
    local cur = inst
    while cur and cur ~= game do
        table.insert(parts, 1, cur.Name)
        cur = cur.Parent
    end
    return table.concat(parts, ".")
end
for _, svcName in ipairs({"ReplicatedStorage", "ServerScriptService", "ServerStorage", "StarterGui", "StarterPlayer", "Lighting", "SoundService", "Workspace"}) do
    local okSvc, svc = pcall(function() return game:GetService(svcName) end)
    if okSvc and svc then
        for _, d in ipairs(svc:GetDescendants()) do
            if d:IsA("LuaSourceContainer") and #scriptPaths < 160 then
                table.insert(scriptPaths, fullPath(d))
            elseif icount < 260 and budget < 68000 then
                if d:IsA("BasePart") then
                    local c = d.Color
                    local p = fullPath(d)
                    instances[p] = {
                        class_name = d.ClassName,
                        properties = {
                            Position = {math.round(d.Position.X * 100) / 100, math.round(d.Position.Y * 100) / 100, math.round(d.Position.Z * 100) / 100},
                            Size = {math.round(d.Size.X * 100) / 100, math.round(d.Size.Y * 100) / 100, math.round(d.Size.Z * 100) / 100},
                            Color = {math.round(c.R * 255), math.round(c.G * 255), math.round(c.B * 255)},
                            Anchored = d.Anchored,
                            Material = d.Material.Name,
                            Transparency = math.round(d.Transparency * 100) / 100,
                            CanCollide = d.CanCollide,
                        },
                    }
                    icount += 1
                    budget += #p + 210
                elseif d:IsA("Model") or d:IsA("Folder") or d:IsA("RemoteEvent") or d:IsA("RemoteFunction") or d:IsA("ValueBase") or d:IsA("Tool") or d:IsA("Atmosphere") or d:IsA("Sky") or d:IsA("Sound") or d:IsA("ScreenGui") or d:IsA("Decal") or d:IsA("SurfaceGui") or d:IsA("ImageLabel") or d:IsA("PointLight") then
                    local p = fullPath(d)
                    local props = {}
                    if d:IsA("Sound") then
                        props.SoundId = d.SoundId
                        props.Volume = d.Volume
                    elseif d:IsA("Decal") then
                        props.Texture = d.Texture
                    elseif d:IsA("ImageLabel") then
                        props.Image = d.Image
                    end
                    instances[p] = {
                        class_name = d.ClassName,
                        properties = props,
                    }
                    icount += 1
                    budget += #p + 96
                end
            end
        end
    end
end
return "OPENRO_TREE:" .. HttpService:JSONEncode({ instances = instances, script_paths = scriptPaths })
"#;

        let mut instances_map = Map::new();
        let mut scripts_map = Map::new();
        let mut script_paths: Vec<String> = Vec::new();

        match self.execute_luau(walk_luau) {
            Ok(val) => {
                if val.get("scripts").is_some() && val.get("instances").is_some() {
                    return Ok(val);
                }
                let raw = val
                    .get("output")
                    .or_else(|| val.get("result"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if let Some(parsed) = parse_openro_tree_payload(raw) {
                    if let Some(insts) = parsed.get("instances").and_then(|v| v.as_object()) {
                        instances_map = insts.clone();
                    }
                    if let Some(scrs) = parsed.get("scripts").and_then(|v| v.as_object()) {
                        scripts_map = scrs.clone();
                    }
                    if let Some(paths) = parsed.get("script_paths").and_then(|v| v.as_array()) {
                        for p in paths {
                            if let Some(s) = p.as_str() {
                                script_paths.push(s.to_string());
                            }
                        }
                    }
                }
            }
            Err(e) => {
                let low = e.to_ascii_lowercase();
                if low.contains("place is not open") || low.contains("no roblox studio") {
                    return Ok(json!({ "scripts": {}, "instances": {} }));
                }
                return Err(e);
            }
        }

        // Phase 2: Batch-fetch full Luau script sources in <= 54 KiB chunks so even
        // multi-thousand-line games sync every script intact beneath StudioMCP's 100k cap.
        let mut cursor = 0usize;
        let max_batches = 24usize;
        let mut batches = 0usize;
        while cursor < script_paths.len() && batches < max_batches {
            batches += 1;
            let start_idx = cursor + 1;
            let batch_luau = format!(
                r#"
local HttpService = game:GetService("HttpService")
local targetPaths = {{}}
local function fullPath(inst)
    local parts = {{}}
    local cur = inst
    while cur and cur ~= game do
        table.insert(parts, 1, cur.Name)
        cur = cur.Parent
    end
    return table.concat(parts, ".")
end
for _, svcName in ipairs({{"ReplicatedStorage", "ServerScriptService", "ServerStorage", "StarterGui", "StarterPlayer", "Lighting", "SoundService", "Workspace"}}) do
    local okSvc, svc = pcall(function() return game:GetService(svcName) end)
    if okSvc and svc then
        for _, d in ipairs(svc:GetDescendants()) do
            if d:IsA("LuaSourceContainer") and #targetPaths < 160 then
                table.insert(targetPaths, {{ fullPath(d), d }})
            end
        end
    end
end
local out = {{}}
local used = 0
local nextIdx = #targetPaths + 1
for i = {start_idx}, #targetPaths do
    local entry = targetPaths[i]
    local okSrc, src = pcall(function() return entry[2].Source end)
    local body = (okSrc and type(src) == "string") and src or ""
    if #body > 48000 then
        body = string.sub(body, 1, 48000) .. "\n-- [openro: truncated at 48 KiB preview]"
    end
    if used > 0 and used + #body + #entry[1] + 32 > 54000 then
        nextIdx = i
        break
    end
    out[entry[1]] = body
    used += #body + #entry[1] + 32
end
return "OPENRO_TREE:" .. HttpService:JSONEncode({{ scripts = out, next_idx = nextIdx }})
"#
            );
            match self.execute_luau(&batch_luau) {
                Ok(val) => {
                    let raw = val
                        .get("output")
                        .or_else(|| val.get("result"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if let Some(parsed) = parse_openro_tree_payload(raw) {
                        if let Some(scrs) = parsed.get("scripts").and_then(|v| v.as_object()) {
                            for (k, v) in scrs {
                                scripts_map.insert(k.clone(), v.clone());
                            }
                        }
                        let next_1based = parsed
                            .get("next_idx")
                            .and_then(|v| v.as_u64())
                            .map(|n| n as usize)
                            .unwrap_or(script_paths.len() + 1);
                        let next_0based = next_1based.saturating_sub(1);
                        if next_0based <= cursor {
                            break;
                        }
                        cursor = next_0based;
                    } else {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        Ok(json!({
            "scripts": Value::Object(scripts_map),
            "instances": Value::Object(instances_map),
        }))
    }

    /// Write a script via official `execute_luau` using collision-proof Luau long-bracket
    /// string literal escaping (`[=[...]=]` with dynamically scaled `=` depth), creating
    /// intermediate folders if needed, and setting a `ChangeHistoryService` undo waypoint.
    pub fn official_write_script(&mut self, path: &str, source: &str) -> Result<String, String> {
        let luau = build_write_script_luau(path, source)?;
        self.execute_luau(&luau)?;
        Ok(json!({ "ok": true, "path": path }).to_string())
    }

    /// Create or update an instance via official `execute_luau` + `ChangeHistoryService:SetWaypoint`.
    pub fn official_create_instance(
        &mut self,
        class_name: &str,
        name: &str,
        parent: &str,
        properties: &Value,
    ) -> Result<String, String> {
        let luau = build_create_instance_luau(class_name, name, parent, properties)?;
        self.execute_luau(&luau)?;
        Ok(json!({ "ok": true, "path": format!("{parent}.{name}") }).to_string())
    }

    /// Delete an instance or script at `path` via official `execute_luau` + `SetWaypoint`.
    pub fn official_delete_instance(&mut self, path: &str) -> Result<String, String> {
        let luau = build_delete_instance_luau(path)?;
        self.execute_luau(&luau)?;
        Ok(json!({ "ok": true, "path": path }).to_string())
    }

    /// Run a real Roblox Studio playtest via official tools:
    /// `start_stop_play("start_play")` -> brief settle -> `get_console_output()` +
    /// optional `screen_capture()` -> `start_stop_play("stop")` -> `{"status", "console"}`.
    pub fn official_playtest(&mut self) -> Result<String, String> {
        let start_res = self.start_stop_play("start_play");
        if let Err(e) = start_res {
            return Ok(json!({
                "status": "failed",
                "console": format!("playtest start error: {e}"),
            })
            .to_string());
        }
        let settle_ms = std::env::var("OPENRO_PLAYTEST_SETTLE_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(150)
            .min(5_000);
        if settle_ms > 0 {
            std::thread::sleep(Duration::from_millis(settle_ms));
        }
        let lines = self.get_console_output().unwrap_or_default();
        if self.has_exact_tool("screen_capture") {
            if let Ok(frame) = self.screen_capture() {
                self.last_playtest_frame = Some(frame);
            }
        }
        let _ = self.start_stop_play("stop");
        let has_error = lines.iter().any(|l| console_line_is_error(l));
        let console = lines.join("\n");
        Ok(json!({
            "status": if has_error { "failed" } else { "passed" },
            "console": console,
        })
        .to_string())
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

/// Encode any arbitrary UTF-8 string as a collision-proof Luau long-bracket literal
/// (`[=[...]=]`, dynamically increasing `=` depth until guaranteed non-colliding).
/// Preserves leading newlines verbatim by prepending a synthetic newline that Luau strips.
pub fn luau_long_string(s: &str) -> String {
    let payload = if s.starts_with('\n') || s.starts_with('\r') {
        format!("\n{s}")
    } else {
        s.to_string()
    };
    let mut depth = 1usize;
    loop {
        let eq = "=".repeat(depth);
        let close = format!("]{eq}]");
        let combined = format!("{payload}{close}");
        if combined.find(&close) == Some(payload.len()) {
            return format!("[{eq}[{payload}{close}");
        }
        depth += 1;
    }
}

/// Encode a short identifier or string as a safely escaped Luau double-quoted string literal.
pub fn luau_quoted_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Build the official-MCP `execute_luau` snippet that writes `source` into `path`,
/// creating missing intermediate Folders and recording a `ChangeHistoryService` waypoint.
pub fn build_write_script_luau(path: &str, source: &str) -> Result<String, String> {
    let segs: Vec<&str> = path.split('.').collect();
    if segs.len() < 2 || segs.iter().any(|s| s.is_empty() || *s == "..") {
        return Err(format!("invalid script path '{path}'"));
    }
    let root = segs[0];
    let script_name = *segs.last().unwrap();
    let folders = &segs[1..segs.len() - 1];
    let class_name = if root == "StarterGui" || root == "StarterPlayer" {
        "LocalScript"
    } else if root == "ReplicatedStorage" {
        "ModuleScript"
    } else {
        "Script"
    };

    let mut code = format!(
        "local ChangeHistoryService = game:GetService(\"ChangeHistoryService\")\n\
         local cur = game:GetService({})\n",
        luau_quoted_string(root)
    );
    for folder in folders {
        let q = luau_quoted_string(folder);
        code.push_str(&format!(
            "local nxt = cur:FindFirstChild({q})\n\
             if not nxt then\n\
                 nxt = Instance.new(\"Folder\")\n\
                 nxt.Name = {q}\n\
                 nxt.Parent = cur\n\
             end\n\
             cur = nxt\n"
        ));
    }
    let q_name = luau_quoted_string(script_name);
    let q_class = luau_quoted_string(class_name);
    let src_lit = luau_long_string(source);
    code.push_str(&format!(
        "local target = cur:FindFirstChild({q_name})\n\
         if not target or not target:IsA(\"LuaSourceContainer\") then\n\
             if target then target:Destroy() end\n\
             target = Instance.new({q_class})\n\
             target.Name = {q_name}\n\
             target.Parent = cur\n\
         end\n\
         target.Source = {src_lit}\n\
         pcall(function() ChangeHistoryService:SetWaypoint(\"OpenRO edit\") end)\n\
         return \"OPENRO_OK\"\n"
    ));
    Ok(code)
}

/// Build the official-MCP `execute_luau` snippet that creates/updates an instance + properties.
pub fn build_create_instance_luau(
    class_name: &str,
    name: &str,
    parent: &str,
    properties: &Value,
) -> Result<String, String> {
    let segs: Vec<&str> = parent.split('.').collect();
    if segs.is_empty() || segs.iter().any(|s| s.is_empty() || *s == "..") {
        return Err(format!("invalid parent path '{parent}'"));
    }
    let root = segs[0];
    let mut code = format!(
        "local ChangeHistoryService = game:GetService(\"ChangeHistoryService\")\n\
         local cur = game:GetService({})\n",
        luau_quoted_string(root)
    );
    for folder in &segs[1..] {
        let q = luau_quoted_string(folder);
        code.push_str(&format!(
            "local nxt = cur:FindFirstChild({q})\n\
             if not nxt then\n\
                 nxt = Instance.new(\"Folder\")\n\
                 nxt.Name = {q}\n\
                 nxt.Parent = cur\n\
             end\n\
             cur = nxt\n"
        ));
    }
    let q_name = luau_quoted_string(name);
    let q_class = luau_quoted_string(class_name);
    code.push_str(&format!(
        "local inst = cur:FindFirstChild({q_name})\n\
         if not inst or inst.ClassName ~= {q_class} then\n\
             inst = Instance.new({q_class})\n\
             inst.Name = {q_name}\n\
             inst.Parent = cur\n\
         end\n"
    ));
    if let Some(map) = properties.as_object() {
        for (k, v) in map {
            if !k
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
                || k.is_empty()
            {
                continue;
            }
            if let Some(expr) = property_value_to_luau(k, v) {
                code.push_str(&format!("pcall(function() inst.{k} = {expr} end)\n"));
            }
        }
    }
    code.push_str(
        "pcall(function() ChangeHistoryService:SetWaypoint(\"OpenRO edit\") end)\n\
         return \"OPENRO_OK\"\n",
    );
    Ok(code)
}

/// Build the official-MCP `execute_luau` snippet that deletes an instance or script at `path`.
pub fn build_delete_instance_luau(path: &str) -> Result<String, String> {
    let segs: Vec<&str> = path.split('.').collect();
    if segs.len() < 2 || segs.iter().any(|s| s.is_empty() || *s == "..") {
        return Err(format!("invalid path '{path}'"));
    }
    let mut code = format!(
        "local ChangeHistoryService = game:GetService(\"ChangeHistoryService\")\n\
         local cur = game:GetService({})\n",
        luau_quoted_string(segs[0])
    );
    for seg in &segs[1..] {
        code.push_str(&format!(
            "if cur then cur = cur:FindFirstChild({}) end\n",
            luau_quoted_string(seg)
        ));
    }
    code.push_str(
        "if cur then\n\
             cur:Destroy()\n\
             pcall(function() ChangeHistoryService:SetWaypoint(\"OpenRO edit\") end)\n\
         end\n\
         return \"OPENRO_OK\"\n",
    );
    Ok(code)
}

fn property_value_to_luau(key: &str, val: &Value) -> Option<String> {
    match key {
        "Position" | "Size" | "Orientation" => {
            let v = parse_vec3_json(val)?;
            Some(format!("Vector3.new({}, {}, {})", v[0], v[1], v[2]))
        }
        "Color" => {
            if let Some(arr) = val.as_array() {
                if arr.len() >= 3 {
                    let r = arr[0].as_f64()?;
                    let g = arr[1].as_f64()?;
                    let b = arr[2].as_f64()?;
                    if (0.0..=1.0).contains(&r)
                        && (0.0..=1.0).contains(&g)
                        && (0.0..=1.0).contains(&b)
                        && (r > 0.0 || g > 0.0 || b > 0.0)
                        && (r <= 1.0 && g <= 1.0 && b <= 1.0)
                        && (r.fract() != 0.0 || g.fract() != 0.0 || b.fract() != 0.0)
                    {
                        return Some(format!("Color3.new({r}, {g}, {b})"));
                    }
                    return Some(format!(
                        "Color3.fromRGB({}, {}, {})",
                        r.clamp(0.0, 255.0).round() as u8,
                        g.clamp(0.0, 255.0).round() as u8,
                        b.clamp(0.0, 255.0).round() as u8
                    ));
                }
            }
            if let Some(hex) = val.as_str() {
                let clean = hex.trim();
                if clean.starts_with('#') && clean.len() == 7 {
                    return Some(format!("Color3.fromHex({})", luau_quoted_string(clean)));
                }
            }
            None
        }
        "BrickColor" => val
            .as_str()
            .map(|s| format!("BrickColor.new({})", luau_quoted_string(s))),
        "Material" => val.as_str().and_then(|s| {
            let clean = s.trim().strip_prefix("Enum.Material.").unwrap_or(s.trim());
            if clean.chars().all(|c| c.is_ascii_alphanumeric()) && !clean.is_empty() {
                Some(format!("Enum.Material.{clean}"))
            } else {
                None
            }
        }),
        "Shape" => val.as_str().and_then(|s| {
            let clean = s.trim().strip_prefix("Enum.PartType.").unwrap_or(s.trim());
            if clean.chars().all(|c| c.is_ascii_alphanumeric()) && !clean.is_empty() {
                Some(format!("Enum.PartType.{clean}"))
            } else {
                None
            }
        }),
        _ => match val {
            Value::Bool(b) => Some(if *b { "true" } else { "false" }.to_string()),
            Value::Number(n) => Some(n.to_string()),
            Value::String(s) => Some(luau_quoted_string(s)),
            _ => None,
        },
    }
}

fn parse_openro_tree_payload(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let slice = if let Some(idx) = trimmed.find("OPENRO_TREE:") {
        trimmed[idx + "OPENRO_TREE:".len()..].trim()
    } else {
        trimmed
    };
    if let Ok(parsed) = serde_json::from_str::<Value>(slice) {
        if parsed.is_object() {
            return Some(parsed);
        }
    }
    // Handle quoted JSON strings returned by StudioMCP `return`
    if let Ok(unquoted) = serde_json::from_str::<String>(trimmed) {
        let inner = if let Some(idx) = unquoted.find("OPENRO_TREE:") {
            unquoted[idx + "OPENRO_TREE:".len()..].trim()
        } else {
            unquoted.trim()
        };
        if let Ok(parsed) = serde_json::from_str::<Value>(inner) {
            if parsed.is_object() {
                return Some(parsed);
            }
        }
    }
    // Fallback: locate JSON object directly after OPENRO_TREE: even with trailing output lines
    if let Some(brace) = slice.find('{') {
        let mut de = serde_json::Deserializer::from_str(&slice[brace..]).into_iter::<Value>();
        if let Some(Ok(v)) = de.next() {
            if v.is_object() {
                return Some(v);
            }
        }
    }
    None
}

fn console_line_is_error(line: &str) -> bool {
    let low = line.to_ascii_lowercase();
    if low.contains("0 errors") || low.contains("0 error(s)") {
        return false;
    }
    low.contains("[error]")
        || low.contains("syntax error")
        || low.contains("runtime error")
        || low.contains("attempt to ")
        || low.contains("is not a valid member")
        || low.contains("error:")
}

fn strip_prefix_ascii_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

fn parse_vec3_json(v: &Value) -> Option<[f64; 3]> {
    let arr = v.as_array()?;
    if arr.len() >= 3 {
        let x = arr[0].as_f64()?;
        let y = arr[1].as_f64()?;
        let z = arr[2].as_f64()?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Some([x, y, z]);
        }
    }
    None
}

fn extract_first_studio_id(val: &Value) -> Option<String> {
    if let Some(id) = val.get("studio_id").and_then(|v| v.as_str()) {
        if !id.trim().is_empty() {
            return Some(id.trim().to_string());
        }
    }
    if let Some(arr) = val.get("studios").and_then(|v| v.as_array()).or_else(|| val.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str().filter(|s| !s.trim().is_empty()) {
                return Some(s.trim().to_string());
            }
            for key in ["studio_id", "id", "studioId"] {
                if let Some(s) = item.get(key).and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) {
                    return Some(s.trim().to_string());
                }
            }
        }
    }
    None
}

/// Extract `(width, height)` directly from PNG `IHDR` or JPEG `SOF0`/`SOF2` headers.
fn parse_image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // PNG IHDR
    if bytes.len() >= 24 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" && &bytes[12..16] == b"IHDR" {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return Some((w, h));
    }
    // JPEG SOF0 / SOF1 / SOF2
    if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        let mut i = 2usize;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if marker == 0xD9 || marker == 0xDA {
                break;
            }
            if matches!(marker, 0xC0..=0xC2) {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Some((w, h));
            }
            if i + 3 < bytes.len() {
                let seg_len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
                if seg_len < 2 {
                    break;
                }
                i += 2 + seg_len;
            } else {
                break;
            }
        }
    }
    None
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luau_long_string_avoids_bracket_collisions_and_preserves_leading_newline() {
        let tricky = "local a = [=[nested]=]\nlocal b = [==[deeper]==]\n-- ends with ]=";
        let encoded = luau_long_string(tricky);
        assert!(encoded.starts_with("[===[") && encoded.ends_with("]===]"), "got: {encoded}");

        let with_nl = "\nprint('first line blank')";
        let enc_nl = luau_long_string(with_nl);
        assert!(enc_nl.starts_with("[=[\n\nprint("), "got: {enc_nl}");

        // Generated write_script, create_instance, and delete_instance Luau snippets must be valid Luau.
        let w = build_write_script_luau("ServerScriptService.Systems.Sprint", tricky).unwrap();
        assert!(w.contains("ChangeHistoryService:SetWaypoint"));
        assert_eq!(
            crate::luau_check::analyze_source("ServerScriptService.GenWrite", &w).error_count(),
            0,
            "write_script Luau must parse cleanly: {w}"
        );

        let c = build_create_instance_luau(
            "Part",
            "JumpPad",
            "Workspace.Arena",
            &json!({ "Position": [0, 4, 8], "Size": [8, 1, 8], "Color": "#3CB371", "Anchored": true, "Material": "Neon" }),
        )
        .unwrap();
        assert!(c.contains("ChangeHistoryService:SetWaypoint"));
        assert_eq!(
            crate::luau_check::analyze_source("ServerScriptService.GenCreate", &c).error_count(),
            0,
            "create_instance Luau must parse cleanly: {c}"
        );

        let d = build_delete_instance_luau("Workspace.Arena.JumpPad").unwrap();
        assert!(d.contains("Destroy()"));
        assert_eq!(
            crate::luau_check::analyze_source("ServerScriptService.GenDelete", &d).error_count(),
            0,
            "delete_instance Luau must parse cleanly: {d}"
        );
    }

    #[test]
    fn place_tree_walk_prioritizes_structural_services_before_workspace() {
        let src = include_str!("mcp.rs");
        let expected_order = r#"{"ReplicatedStorage", "ServerScriptService", "ServerStorage", "StarterGui", "StarterPlayer", "Lighting", "SoundService", "Workspace"}"#;
        assert!(
            src.contains(expected_order),
            "Phase 1 place-tree walk must visit ReplicatedStorage/ServerStorage before Workspace"
        );
        assert!(
            src.contains(r#"d:IsA("ValueBase")"#),
            "Phase 1 place-tree walk must include ValueBase nodes (e.g. ReplicatedStorage.State.*)"
        );
    }
}
