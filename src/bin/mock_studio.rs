//! Mock Roblox Studio MCP server — a TEST FIXTURE, not product code.
//!
//! Speaks MCP-style JSON-RPC 2.0 over stdio and holds an in-memory place
//! tree, so the real OpenRO MCP client can be exercised end to end with no
//! Studio present.
//!
//! Deterministic playtest rules (documented fixture behaviour):
//!
//! 1. playtest FAILS with console error
//!    `ServerScriptService.RoundManager:4: attempt to index nil with 'Start'`
//!    if any written script contains `workspace.Round.` but no written
//!    script creates it (no `Name = "Round"` anywhere).
//! 2. playtest FAILS with console error
//!    `Workspace.SpawnPad is not a valid member of Workspace "Workspace"`
//!    if any written script references `workspace.SpawnPad` but no
//!    instance exists at `Workspace.SpawnPad`.
//! 3. playtest FAILS with console error
//!    `Workspace.Checkpoints is not a valid member of Workspace "Workspace"`
//!    if any written script references `workspace.Checkpoints` but no
//!    script creates it (no `Name = "Checkpoints"` anywhere) and no
//!    instance exists at `Workspace.Checkpoints`.
//! 4. playtest FAILS with console error
//!    `ServerScriptService.Currency: DataStore save failed: BindToClose was never bound`
//!    if any written script calls `GetDataStore(` without binding a
//!    shutdown save (`BindToClose` absent from that script).
//! 5. playtest FAILS with console error
//!    `StarterGui.GameGui: OpenROShopBuy RemoteEvent was never created in ReplicatedStorage`
//!    if any written script waits on `WaitForChild("OpenROShopBuy")`
//!    but no written script creates a RemoteEvent.
//! 6. playtest FAILS with console error
//!    `ServerScriptService.Shop: no leaderstats exists for the shop to charge`
//!    if any written script creates a RemoteEvent (the shop) but no
//!    written script CREATES leaderstats (`Name = "leaderstats"` —
//!    a currency or leaderboard script; the shop itself only looks
//!    the folder up) — a shop with nothing to charge is dead on
//!    arrival.
//!
//! Also advertises live Studio viewer tools:
//! - `screen_capture`: returns a genuine dynamically rendered `320x200` RGB PNG
//!   depicting the placed Workspace parts, colors, script indicators, and play mode.
//! - `get_console_output`: returns accumulated Studio Output console lines.
//! - `start_stop_play`: starts (`start_play`/`run_server`) or stops (`stop`) play mode.
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
const CHECKPOINT_FAIL_CONSOLE: &str =
    "Workspace.Checkpoints is not a valid member of Workspace \"Workspace\"";
const DATASTORE_FAIL_CONSOLE: &str = "ServerScriptService.Currency: DataStore save failed: BindToClose was never bound — balances would be lost at shutdown";
const SHOP_FAIL_CONSOLE: &str = "StarterGui.GameGui: OpenROShopBuy RemoteEvent was never created in ReplicatedStorage — the shop button has nothing to fire";
const SHOP_NO_CURRENCY_CONSOLE: &str = "ServerScriptService.Shop: no leaderstats exists for the shop to charge — add a currency or leaderboard first";
const OK_CONSOLE: &str = "Playtest OK: all scripts ran without errors";

const FRAME_W: usize = 320;
const FRAME_H: usize = 200;

struct MockState {
    scripts: BTreeMap<String, String>,
    instances: BTreeMap<String, Value>,
    console: String,
    console_lines: Vec<String>,
    playing: bool,
    frame_counter: u64,
}

impl MockState {
    fn new() -> Self {
        MockState {
            scripts: BTreeMap::new(),
            instances: BTreeMap::new(),
            console: OK_CONSOLE.to_string(),
            console_lines: vec!["[Studio] Connected to Mock Roblox Studio MCP bridge.".to_string()],
            playing: false,
            frame_counter: 0,
        }
    }

    fn log_line(&mut self, line: impl Into<String>) {
        if self.console_lines.len() >= 200 {
            self.console_lines.remove(0);
        }
        self.console_lines.push(line.into());
    }
}

fn main() {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let mut state = MockState::new();

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
                "serverInfo": { "name": "mock-studio", "version": "0.2.0" }
            }),
            "tools/list" => {
                // MOCK_DROP=<tool> simulates a bridge that lacks a tool.
                let drop = std::env::var("MOCK_DROP").unwrap_or_default();
                let all = [
                    ("get_place_tree", "List scripts and instances in the place"),
                    ("write_script", "Create or overwrite a script"),
                    ("delete_script", "Delete a script"),
                    (
                        "create_instance",
                        "Create an instance (class, name, parent, properties)",
                    ),
                    ("delete_instance", "Delete an instance"),
                    ("playtest", "Run the place"),
                    ("get_console", "Read the console output"),
                    (
                        "get_console_output",
                        "Read structured Studio Output console lines",
                    ),
                    (
                        "screen_capture",
                        "Capture the current Roblox Studio 3D viewport as PNG",
                    ),
                    (
                        "start_stop_play",
                        "Start or stop Roblox Studio playtest mode (start_play|run_server|stop)",
                    ),
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
                tool_call(&name, &args, &mut state)
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

fn evaluate_playtest(
    scripts: &BTreeMap<String, String>,
    instances: &BTreeMap<String, Value>,
) -> (bool, &'static str) {
    let references_round = scripts.values().any(|s| s.contains("workspace.Round."));
    let creates_round = scripts.values().any(|s| s.contains("Name = \"Round\""));
    let references_spawn = scripts.values().any(|s| s.contains("workspace.SpawnPad"));
    let has_spawn = instances.contains_key("Workspace.SpawnPad");
    let references_checkpoints = scripts
        .values()
        .any(|s| s.contains("workspace.Checkpoints"));
    let creates_checkpoints = scripts
        .values()
        .any(|s| s.contains("Name = \"Checkpoints\""));
    let has_checkpoints = instances.contains_key("Workspace.Checkpoints");
    let datastore_unbound = scripts
        .values()
        .any(|s| s.contains("GetDataStore(") && !s.contains("BindToClose"));
    let waits_remote = scripts
        .values()
        .any(|s| s.contains("WaitForChild(\"OpenROShopBuy\")"));
    let creates_shop_remote = scripts
        .values()
        .any(|s| s.contains("Instance.new(\"RemoteEvent\")") && s.contains("OpenROShopBuy"));
    let has_leaderstats = scripts
        .values()
        .any(|s| s.contains("Name = \"leaderstats\""));
    if references_round && !creates_round {
        (false, FAIL_CONSOLE)
    } else if references_spawn && !has_spawn {
        (false, SPAWN_FAIL_CONSOLE)
    } else if references_checkpoints && !creates_checkpoints && !has_checkpoints {
        (false, CHECKPOINT_FAIL_CONSOLE)
    } else if datastore_unbound {
        (false, DATASTORE_FAIL_CONSOLE)
    } else if waits_remote && !creates_shop_remote {
        (false, SHOP_FAIL_CONSOLE)
    } else if creates_shop_remote && !has_leaderstats {
        (false, SHOP_NO_CURRENCY_CONSOLE)
    } else {
        (true, OK_CONSOLE)
    }
}

fn tool_call(name: &str, args: &Value, state: &mut MockState) -> Value {
    // MOCK_ERR_TOOLS=1: every tool call fails, like a broken bridge.
    if std::env::var("MOCK_ERR_TOOLS").is_ok() {
        return error_result(format!("bridge exploded: {name}"));
    }
    // A dropped tool (MOCK_DROP) errors when called, like a real bridge.
    if std::env::var("MOCK_DROP")
        .map(|d| d == name)
        .unwrap_or(false)
    {
        return error_result(format!("unknown tool {name}"));
    }
    // MOCK_ERR_CONSOLE=1: only get_console / get_console_output fails (a console
    // subsystem outage while playtests themselves still answer).
    if matches!(name, "get_console" | "get_console_output")
        && std::env::var("MOCK_ERR_CONSOLE").is_ok()
    {
        return error_result("console subsystem exploded".to_string());
    }
    match name {
        "get_place_tree" => text_result(
            json!({ "scripts": state.scripts, "instances": state.instances }).to_string(),
        ),
        "write_script" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if bad_path(path) {
                return error_result(format!("invalid script path: {path}"));
            }
            let source = args.get("source").and_then(|v| v.as_str()).unwrap_or("");
            state.scripts.insert(path.to_string(), source.to_string());
            state.log_line(format!(
                "[Studio] Wrote script {path} ({} bytes)",
                source.len()
            ));
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "delete_script" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if state.scripts.remove(path).is_none() {
                return error_result(format!("no script at {path}"));
            }
            state.log_line(format!("[Studio] Deleted script {path}"));
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "create_instance" => {
            let class_name = args
                .get("class_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let inst_name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let parent = args.get("parent").and_then(|v| v.as_str()).unwrap_or("");
            let properties = args.get("properties").cloned().unwrap_or(json!({}));
            let path = format!("{parent}.{inst_name}");
            if bad_path(&path) {
                return error_result(format!("invalid instance path: {path}"));
            }
            // Strict, like a real place: creating over an existing
            // instance is an error, not a silent overwrite.
            if state.instances.contains_key(&path) {
                return error_result(format!("instance already exists at {path}"));
            }
            // MOCK_INST_EXTRA=1: the bridge adds one of its own
            // default properties to every instance record, as a real
            // place tree does.
            let properties = if std::env::var("MOCK_INST_EXTRA").is_ok() {
                let mut p = properties.as_object().cloned().unwrap_or_default();
                p.insert("BridgeDefault".to_string(), json!(true));
                Value::Object(p)
            } else {
                properties
            };
            state.instances.insert(
                path.clone(),
                json!({ "class_name": class_name, "properties": properties }),
            );
            state.log_line(format!("[Studio] Created {class_name} at {path}"));
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "delete_instance" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if state.instances.remove(path).is_none() {
                return error_result(format!("no instance at {path}"));
            }
            state.log_line(format!("[Studio] Deleted instance {path}"));
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "playtest" => {
            let (passed, msg) = evaluate_playtest(&state.scripts, &state.instances);
            state.console = msg.to_string();
            state.log_line(format!("[Playtest] {msg}"));
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
        "get_console" => text_result(state.console.clone()),
        "get_console_output" => text_result(
            json!({
                "lines": state.console_lines,
                "count": state.console_lines.len(),
                "last": state.console,
            })
            .to_string(),
        ),
        "start_stop_play" => {
            let mode = args
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("start_play");
            match mode {
                "start_play" | "run_server" => {
                    let (passed, msg) = evaluate_playtest(&state.scripts, &state.instances);
                    state.playing = true;
                    state.console = msg.to_string();
                    state.log_line(format!(
                        "[Studio Play] Started {mode} ({} script(s), {} instance(s))",
                        state.scripts.len(),
                        state.instances.len()
                    ));
                    state.log_line(format!("[Studio Play] {msg}"));
                    text_result(
                        json!({
                            "ok": true,
                            "mode": mode,
                            "playing": true,
                            "status": if passed { "passed" } else { "failed" },
                            "console": msg,
                            "lines": state.console_lines,
                        })
                        .to_string(),
                    )
                }
                "stop" => {
                    state.playing = false;
                    state.log_line("[Studio Play] Stopped playtest session.".to_string());
                    text_result(
                        json!({
                            "ok": true,
                            "mode": "stop",
                            "playing": false,
                            "status": "stopped",
                            "console": state.console,
                        })
                        .to_string(),
                    )
                }
                other => error_result(format!("unsupported start_stop_play mode: {other}")),
            }
        }
        "screen_capture" => {
            state.frame_counter += 1;
            let png = render_studio_scene_png(
                &state.scripts,
                &state.instances,
                state.playing,
                state.frame_counter,
            );
            let b64 = encode_base64(&png);
            let meta = json!({
                "width": FRAME_W,
                "height": FRAME_H,
                "instances": state.instances.len(),
                "scripts": state.scripts.len(),
                "playing": state.playing,
                "frame": state.frame_counter,
            })
            .to_string();
            json!({
                "content": [
                    {
                        "type": "image",
                        "data": b64,
                        "mimeType": "image/png"
                    },
                    {
                        "type": "text",
                        "text": meta
                    }
                ],
                "isError": false
            })
        }
        other => error_result(format!("unknown tool {other}")),
    }
}

// ---------------------------------------------------------------------------
// Dynamic 320x200 Roblox Studio Viewport PNG Renderer (zero dependencies)
// ---------------------------------------------------------------------------

fn render_studio_scene_png(
    scripts: &BTreeMap<String, String>,
    instances: &BTreeMap<String, Value>,
    playing: bool,
    frame_counter: u64,
) -> Vec<u8> {
    let mut rgb = vec![0u8; FRAME_W * FRAME_H * 3];

    // 1. Sky gradient + perspective baseplate floor + stud grid
    let horizon = 74usize;
    for y in 0..FRAME_H {
        for x in 0..FRAME_W {
            let color = if y < 6 && playing {
                // Play-mode green indicator ribbon across top of Studio viewport
                [46, 196, 118]
            } else if y < horizon {
                let t = y as f32 / horizon as f32;
                let r = (128.0 + 70.0 * t) as u8;
                let g = (176.0 + 52.0 * t) as u8;
                let b = (232.0 + 18.0 * t) as u8;
                [r, g, b]
            } else {
                // Studio Baseplate with perspective grid lines
                let dy = (y - horizon) as f32 + 1.0;
                let gx = ((x as f32 - (FRAME_W as f32 * 0.5)) / (dy * 0.14 + 6.0)).floor() as i32;
                let gz = (dy / 14.0).floor() as i32;
                let is_line = ((x as i32 + y as i32) % 16 == 0)
                    || (y - horizon).is_multiple_of(14)
                    || ((gx + gz) % 2 == 0 && (y - horizon) % 7 == 0);
                if is_line {
                    [92, 99, 112]
                } else {
                    [108, 115, 128]
                }
            };
            put_px(&mut rgb, x, y, color);
        }
    }

    struct RenderPart {
        depth: f64,
        pos: [f64; 3],
        size: [f64; 3],
        color: [u8; 3],
    }

    // 2. Project each Workspace Part/SpawnLocation as a shaded isometric 3D box
    let mut parts: Vec<RenderPart> = Vec::new();
    for (idx, (path, rec)) in instances.iter().enumerate() {
        let class_name = rec
            .get("class_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Part")
            .to_string();
        let props = rec.get("properties").unwrap_or(&Value::Null);
        let pos = parse_vec3(props.get("Position")).unwrap_or([
            ((idx as f64 % 5.0) - 2.0) * 14.0,
            2.0,
            (idx as f64 / 5.0).floor() * 14.0,
        ]);
        let size = parse_vec3(props.get("Size")).unwrap_or([8.0, 2.0, 8.0]);
        let color = parse_color_rgb(path, &class_name, props);
        let depth = pos[0] + pos[2] - pos[1] * 0.25;
        parts.push(RenderPart {
            depth,
            pos,
            size,
            color,
        });
    }
    // Painter's sort back-to-front
    parts.sort_by(|a, b| a.depth.partial_cmp(&b.depth).unwrap_or(std::cmp::Ordering::Equal));

    for part in parts {
        let pos = part.pos;
        let size = part.size;
        let col = part.color;
        let sx = (FRAME_W as f64 * 0.5 + (pos[0] - pos[2]) * 2.1) as i32;
        let sy = (horizon as f64 + 54.0 + (pos[0] + pos[2]) * 0.72 - pos[1] * 2.4) as i32;
        let w = (size[0].max(size[2]) * 2.6).clamp(12.0, 74.0) as i32;
        let h = (size[1] * 3.2).clamp(8.0, 48.0) as i32;
        let top_h = (w / 4).clamp(4, 16);

        let top_col = shade(col, 1.18);
        let side_col = shade(col, 0.78);
        let border_col = shade(col, 0.45);

        // Body
        fill_rect(&mut rgb, sx - w / 2, sy - h, w, h, side_col);
        // Top highlight
        fill_rect(&mut rgb, sx - w / 2, sy - h - top_h, w, top_h, top_col);
        // Crisp outline
        stroke_rect(&mut rgb, sx - w / 2, sy - h - top_h, w, h + top_h, border_col);
    }

    // 3. Top-left HUD strip encoding script count & capture sequence visibly
    let script_bars = scripts.len().min(16);
    fill_rect(&mut rgb, 8, 10, 8 + (script_bars as i32) * 8 + 14, 14, [24, 28, 38]);
    for i in 0..script_bars {
        fill_rect(&mut rgb, 12 + (i as i32) * 8, 13, 5, 8, [96, 165, 250]);
    }
    // Frame heartbeat dot (bottom-right)
    let pulse = if frame_counter.is_multiple_of(2) {
        [74, 222, 128]
    } else {
        [56, 189, 248]
    };
    fill_rect(&mut rgb, (FRAME_W - 14) as i32, (FRAME_H - 14) as i32, 6, 6, pulse);

    encode_png_rgb(FRAME_W as u32, FRAME_H as u32, &rgb)
}

fn put_px(rgb: &mut [u8], x: usize, y: usize, col: [u8; 3]) {
    if x < FRAME_W && y < FRAME_H {
        let i = (y * FRAME_W + x) * 3;
        rgb[i] = col[0];
        rgb[i + 1] = col[1];
        rgb[i + 2] = col[2];
    }
}

fn fill_rect(rgb: &mut [u8], x0: i32, y0: i32, w: i32, h: i32, col: [u8; 3]) {
    let x_start = x0.max(0) as usize;
    let y_start = y0.max(0) as usize;
    let x_end = (x0 + w).clamp(0, FRAME_W as i32) as usize;
    let y_end = (y0 + h).clamp(0, FRAME_H as i32) as usize;
    for y in y_start..y_end {
        for x in x_start..x_end {
            put_px(rgb, x, y, col);
        }
    }
}

fn stroke_rect(rgb: &mut [u8], x0: i32, y0: i32, w: i32, h: i32, col: [u8; 3]) {
    fill_rect(rgb, x0, y0, w, 1, col);
    fill_rect(rgb, x0, y0 + h - 1, w, 1, col);
    fill_rect(rgb, x0, y0, 1, h, col);
    fill_rect(rgb, x0 + w - 1, y0, 1, h, col);
}

fn shade(col: [u8; 3], factor: f32) -> [u8; 3] {
    [
        ((col[0] as f32) * factor).clamp(0.0, 255.0) as u8,
        ((col[1] as f32) * factor).clamp(0.0, 255.0) as u8,
        ((col[2] as f32) * factor).clamp(0.0, 255.0) as u8,
    ]
}

fn parse_vec3(v: Option<&Value>) -> Option<[f64; 3]> {
    let arr = v?.as_array()?;
    if arr.len() >= 3 {
        Some([arr[0].as_f64()?, arr[1].as_f64()?, arr[2].as_f64()?])
    } else {
        None
    }
}

fn parse_color_rgb(path: &str, class_name: &str, props: &Value) -> [u8; 3] {
    if let Some(hex) = props.get("Color").and_then(|v| v.as_str()) {
        let clean = hex.trim().trim_start_matches('#');
        if clean.len() == 6 {
            if let Ok(rgb) = u32::from_str_radix(clean, 16) {
                return [
                    ((rgb >> 16) & 0xFF) as u8,
                    ((rgb >> 8) & 0xFF) as u8,
                    (rgb & 0xFF) as u8,
                ];
            }
        }
    }
    let lower = path.to_ascii_lowercase();
    if lower.contains("lava") || lower.contains("kill") || lower.contains("boss") {
        [224, 83, 42]
    } else if lower.contains("jump") || lower.contains("potion") {
        [60, 179, 113]
    } else if lower.contains("portal") || lower.contains("showcase") {
        [168, 85, 247]
    } else if class_name == "SpawnLocation" || lower.contains("spawn") || lower.contains("pedestal")
    {
        [79, 124, 240]
    } else {
        [230, 184, 48]
    }
}

fn encode_png_rgb(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let stride = (width as usize) * 3;
    let mut raw = Vec::with_capacity((stride + 1) * (height as usize));
    for y in 0..(height as usize) {
        raw.push(0u8); // PNG filter type 0 (None)
        raw.extend_from_slice(&rgb[y * stride..(y + 1) * stride]);
    }
    let zlib_data = zlib_store(&raw);

    let mut out = Vec::with_capacity(zlib_data.len() + 64);
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(2); // color type: Truecolor RGB
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace
    push_png_chunk(&mut out, b"IHDR", &ihdr);
    push_png_chunk(&mut out, b"IDAT", &zlib_data);
    push_png_chunk(&mut out, b"IEND", &[]);
    out
}

fn push_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 32);
    out.push(0x78); // CMF: deflate, 32K window
    out.push(0x01); // FLG: FCHECK valid (0x7801 % 31 == 0)

    let mut offset = 0usize;
    if data.is_empty() {
        out.push(0x01);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    } else {
        while offset < data.len() {
            let chunk_len = (data.len() - offset).min(65_535);
            let is_final = offset + chunk_len >= data.len();
            out.push(if is_final { 0x01 } else { 0x00 });
            let len16 = chunk_len as u16;
            out.extend_from_slice(&len16.to_le_bytes());
            out.extend_from_slice(&(!len16).to_le_bytes());
            out.extend_from_slice(&data[offset..offset + chunk_len]);
            offset += chunk_len;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for &b in data {
        s1 = (s1 + b as u32) % MOD;
        s2 = (s2 + s1) % MOD;
    }
    (s2 << 16) | s1
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() {
            bytes[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < bytes.len() {
            bytes[i + 2] as u32
        } else {
            0
        };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < bytes.len() {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}
