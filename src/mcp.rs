//! Studio bridge: an MCP client speaking JSON-RPC 2.0 over stdio to a
//! child process (a Roblox Studio MCP server). The child command comes
//! from `OPENRO_MCP_COMMAND`, so a real Studio MCP server drops in
//! unchanged. Performs the `initialize` handshake, then `tools/call`.

use crate::assets::{decode_base64, detect_image_mime};
use crate::studio_stream::StudioFrame;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long one bridge request may take before it is declared wedged.
/// A silent bridge used to hang the agent forever (blocking read, no
/// timeout anywhere); `OPENRO_BRIDGE_TIMEOUT_SECS` overrides for tests
/// and slow real servers.
fn request_timeout() -> Duration {
    let secs = std::env::var("OPENRO_BRIDGE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|s| *s >= 1)
        .unwrap_or(30);
    Duration::from_secs(secs)
}

pub struct McpClient {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    /// Lines from the bridge's stdout, delivered by a reader thread so
    /// requests can time out instead of blocking forever.
    lines: Receiver<String>,
    timeout: Duration,
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
        Self::spawn_with_timeout(command, request_timeout())
    }

    /// Spawn with an explicit per-request timeout.
    pub fn spawn_with_timeout(command: &str, timeout: Duration) -> Result<McpClient, String> {
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
        let stdout = child.stdout.take().expect("stdout piped");
        let (tx, rx) = channel::<String>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
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
            timeout,
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

    /// Passively detect a running Vinegar Flatpak Roblox Studio instance with
    /// `/home/<user>/.local/bin/roblox-studio-mcp` installed. NEVER launches
    /// Vinegar or downloads anything.
    pub fn detect_vinegar_studio_mcp() -> Option<String> {
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

    /// Call one MCP tool; returns the text of its first content block.
    ///
    /// A tool-level failure is an Err here, never a success string: the
    /// MCP `isError` flag is honoured, and so is an `{"error": ...}`
    /// JSON payload (some bridges report failures that way). Before
    /// this check existed, a bridge error was returned as Ok text and
    /// the agent narrated work that never happened.
    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<String, String> {
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
            // An `error` key is a bridge error only when it carries an
            // actual message: a string, or an object with one. A null
            // error field next to a valid result is a healthy payload
            // shape (several bridges emit it) and must not be refused.
            match payload.get("error") {
                Some(Value::String(s)) if !s.is_empty() => {
                    return Err(format!("Studio bridge error in {name}: {s}"));
                }
                // An object counts ONLY when it carries a non-empty
                // message — {"code": 0} beside a passing verdict is a
                // healthy shape, and inventing a "tool error" message
                // for it would fabricate a failure.
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

    /// The place tree as JSON: `{ "scripts": { "<path>": "<source>", ... } }`.
    pub fn get_place_tree(&mut self) -> Result<Value, String> {
        let text = self.call_tool("get_place_tree", json!({}))?;
        serde_json::from_str(&text)
            .map_err(|e| format!("get_place_tree returned non-JSON text: {e}"))
    }

    /// Capture a live screenshot frame from the connected Roblox Studio MCP bridge
    /// via the `screen_capture` tool. Safely decodes base64 PNG/JPEG/WebP from MCP
    /// `image` content blocks or JSON/data-URL `text` blocks and validates real magic bytes.
    pub fn screen_capture(&mut self) -> Result<StudioFrame, String> {
        if !self.has_tool("screen_capture") {
            return Err(
                "Connected Studio MCP bridge does not advertise the 'screen_capture' tool"
                    .to_string(),
            );
        }
        let result = self.request(
            "tools/call",
            json!({ "name": "screen_capture", "arguments": {} }),
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

    /// Read structured Studio Output console lines via `get_console_output`
    /// (falling back to `get_console` on bridges that only advertise the legacy tool).
    pub fn get_console_output(&mut self) -> Result<Vec<String>, String> {
        let tool = if self.has_tool("get_console_output") {
            "get_console_output"
        } else {
            "get_console"
        };
        let raw = match self.call_tool(tool, json!({})) {
            Ok(text) => text,
            Err(_) if tool == "get_console_output" => self.call_tool("get_console", json!({}))?,
            Err(e) => return Err(e),
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

    /// Control Roblox Studio playtest execution (`"start_play"`, `"run_server"`, or `"stop"`).
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
        if self.has_tool("start_stop_play") {
            let text = self.call_tool("start_stop_play", json!({ "mode": normalized }))?;
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                return Ok(v);
            }
            return Ok(json!({
                "ok": true,
                "mode": normalized,
                "output": text,
            }));
        }
        if normalized == "stop" {
            return Ok(json!({
                "ok": true,
                "mode": "stop",
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
            "output": text,
        }))
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
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
