//! Mock Roblox Studio MCP server — a TEST FIXTURE, not product code.
//!
//! Speaks MCP-style JSON-RPC 2.0 over stdio and holds an in-memory place
//! tree, so the real OpenRO MCP client can be exercised end to end with no
//! Studio present.
//!
//! Advertises and implements all 13 official Roblox Studio MCP tools:
//! - `list_roblox_studios`, `get_studio_state`, `search_game_tree`, `inspect_instance`
//! - `script_read`, `script_grep`, `multi_edit`, `execute_luau`
//! - `get_console_output`, `screen_capture`, `start_stop_play`
//! - `user_mouse_input`, `user_keyboard_input`
//!
//! Plus all 7 legacy place-tree/playtest fixture tools for backwards compatibility.
//!
//! `screen_capture` renders a genuine dynamically updated `320x200` RGB PNG that
//! visually reflects placed parts, camera orbit/zoom, live mouse cursor/selection,
//! keyboard input indicators, `multi_edit` / `execute_luau` activity, and play mode.

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

const STUDIO_ID: &str = "mock-studio-01";
const FRAME_W: usize = 320;
const FRAME_H: usize = 200;

struct MockState {
    scripts: BTreeMap<String, String>,
    instances: BTreeMap<String, Value>,
    console: String,
    console_lines: Vec<String>,
    playing: bool,
    play_mode: String,
    active_datamodel: String,
    frame_counter: u64,
    // Interactive Studio viewport state driven by mouse/keyboard/edit/luau tools
    cursor_x: f64,
    cursor_y: f64,
    mouse_down: bool,
    mouse_clicks: u64,
    selected_instance: Option<String>,
    last_key: Option<String>,
    typed_text: String,
    key_events: u64,
    camera_yaw: f64,
    camera_pitch: f64,
    camera_zoom: f64,
    camera_pan_x: f64,
    camera_pan_z: f64,
    multi_edit_count: u64,
    luau_exec_count: u64,
    // v0.11 structured DataModel mirror for chunked sync & playtest check simulation
    v011_tree: BTreeMap<String, MockNode>,
    v011_compare_order: Vec<String>,
}

#[derive(Clone)]
struct MockNode {
    class: String,
    props: BTreeMap<String, Value>,
    source: Option<String>,
}

fn default_v011_services() -> BTreeMap<String, MockNode> {
    let mut m = BTreeMap::new();
    for (p, cls) in [
        ("ReplicatedStorage", "ReplicatedStorage"),
        ("ServerScriptService", "ServerScriptService"),
        ("ServerStorage", "ServerStorage"),
        ("StarterGui", "StarterGui"),
        ("StarterPlayer", "StarterPlayer"),
        ("StarterPlayer/StarterPlayerScripts", "StarterPlayerScripts"),
        ("StarterPlayer/StarterCharacterScripts", "StarterCharacterScripts"),
        ("StarterPack", "StarterPack"),
        ("ReplicatedFirst", "ReplicatedFirst"),
        ("Lighting", "Lighting"),
        ("SoundService", "SoundService"),
        ("Teams", "Teams"),
        ("Workspace", "Workspace"),
    ] {
        m.insert(
            p.to_string(),
            MockNode {
                class: cls.to_string(),
                props: BTreeMap::new(),
                source: None,
            },
        );
    }
    m
}

impl MockState {
    fn new() -> Self {
        MockState {
            scripts: BTreeMap::new(),
            instances: BTreeMap::new(),
            console: OK_CONSOLE.to_string(),
            console_lines: vec!["[Studio] Connected to Mock Roblox Studio MCP bridge.".to_string()],
            playing: false,
            play_mode: "edit".to_string(),
            active_datamodel: "Edit".to_string(),
            frame_counter: 0,
            cursor_x: (FRAME_W as f64) * 0.5,
            cursor_y: (FRAME_H as f64) * 0.5,
            mouse_down: false,
            mouse_clicks: 0,
            selected_instance: None,
            last_key: None,
            typed_text: String::new(),
            key_events: 0,
            camera_yaw: 0.0,
            camera_pitch: 0.0,
            camera_zoom: 1.0,
            camera_pan_x: 0.0,
            camera_pan_z: 0.0,
            multi_edit_count: 0,
            luau_exec_count: 0,
            v011_tree: default_v011_services(),
            v011_compare_order: Vec::new(),
        }
    }

    fn log_line(&mut self, line: impl Into<String>) {
        if self.console_lines.len() >= 200 {
            self.console_lines.remove(0);
        }
        self.console_lines.push(line.into());
    }

    fn datamodel_types(&self) -> Vec<&'static str> {
        if self.playing {
            if self.play_mode == "run_server" {
                vec!["Edit", "Server"]
            } else {
                vec!["Edit", "Client", "Server"]
            }
        } else {
            vec!["Edit"]
        }
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
                "serverInfo": { "name": "mock-studio", "version": "0.9.0" }
            }),
            "tools/list" => {
                // MOCK_DROP=<tool> simulates a bridge that lacks a tool.
                let drop = std::env::var("MOCK_DROP").unwrap_or_default();
                let all = [
                    // Official 13 Roblox Studio MCP tools
                    (
                        "list_roblox_studios",
                        "List running Roblox Studio instances and their studio_id handles",
                    ),
                    (
                        "get_studio_state",
                        "Get current Studio mode, available DataModels, and selection state",
                    ),
                    (
                        "search_game_tree",
                        "Search Instances in the Roblox DataModel by path, class, or keywords",
                    ),
                    (
                        "inspect_instance",
                        "Inspect properties, attributes, and children of an Instance path",
                    ),
                    (
                        "script_read",
                        "Read a Luau script by path and optional line range",
                    ),
                    (
                        "script_grep",
                        "Search text across all Luau scripts in the open place",
                    ),
                    (
                        "multi_edit",
                        "Create or apply multi-hunk string edits to a Luau script",
                    ),
                    (
                        "execute_luau",
                        "Execute Luau code inside Studio's Edit, Client, or Server DataModel",
                    ),
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
                        "Start or stop Roblox Studio playtest mode (start_play|run_server|stop / is_start)",
                    ),
                    (
                        "user_mouse_input",
                        "Forward mouse move, click, drag, and scroll actions into Studio",
                    ),
                    (
                        "user_keyboard_input",
                        "Forward key press, hold, release, and text input into Studio",
                    ),
                    // Legacy place-tree & playtest fixture tools
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

fn error_result(text: String) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": true
    })
}

fn bad_path(path: &str) -> bool {
    path.is_empty() || path.split('.').any(|seg| seg.is_empty() || seg == "..")
}

fn normalize_game_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("game.") {
        rest.to_string()
    } else if let Some(rest) = trimmed.strip_prefix("Game.") {
        rest.to_string()
    } else {
        trimmed.to_string()
    }
}

fn script_class_for_path(path: &str) -> &'static str {
    if path.starts_with("StarterGui.") || path.starts_with("StarterPlayer.") {
        "LocalScript"
    } else if path.starts_with("ReplicatedStorage.") {
        "ModuleScript"
    } else {
        "Script"
    }
}

fn class_matches_filter(class_name: &str, filter: &str) -> bool {
    if class_name == filter {
        return true;
    }
    match filter {
        "Instance" => true,
        "BasePart" => matches!(class_name, "Part" | "SpawnLocation" | "MeshPart" | "WedgePart"),
        "BaseScript" | "LuaSourceContainer" => {
            matches!(class_name, "Script" | "LocalScript" | "ModuleScript")
        }
        "GuiObject" | "GuiBase2d" => matches!(
            class_name,
            "ScreenGui" | "SurfaceGui" | "BillboardGui" | "Frame" | "ImageLabel" | "TextLabel" | "TextButton"
        ),
        _ => false,
    }
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
    // MOCK_ERR_CONSOLE=1: only get_console / get_console_output fails.
    if matches!(name, "get_console" | "get_console_output")
        && std::env::var("MOCK_ERR_CONSOLE").is_ok()
    {
        return error_result("console subsystem exploded".to_string());
    }

    match name {
        // -------------------------------------------------------------------
        // Official 13 Roblox Studio MCP tools
        // -------------------------------------------------------------------
        "list_roblox_studios" => {
            if std::env::var("MOCK_NO_STUDIOS").is_ok() {
                text_result(json!({ "studios": [], "count": 0 }).to_string())
            } else {
                text_result(
                    json!({
                        "studios": [{
                            "studio_id": STUDIO_ID,
                            "place_name": "OpenRO Mock Place",
                            "datamodel_types": state.datamodel_types(),
                            "active_datamodel": state.active_datamodel,
                            "playing": state.playing,
                        }],
                        "count": 1,
                    })
                    .to_string(),
                )
            }
        }
        "get_studio_state" => text_result(
            json!({
                "studio_id": STUDIO_ID,
                "playing": state.playing,
                "mode": state.play_mode,
                "datamodel_types": state.datamodel_types(),
                "active_datamodel": state.active_datamodel,
                "scripts_count": state.scripts.len(),
                "instances_count": state.instances.len(),
                "selected_instance": state.selected_instance,
                "cursor": [state.cursor_x, state.cursor_y],
                "last_key": state.last_key,
                "typed_text": state.typed_text,
                "camera": {
                    "yaw": state.camera_yaw,
                    "pitch": state.camera_pitch,
                    "zoom": state.camera_zoom,
                    "pan": [state.camera_pan_x, state.camera_pan_z],
                },
                "multi_edit_count": state.multi_edit_count,
                "luau_exec_count": state.luau_exec_count,
                "frame_counter": state.frame_counter,
            })
            .to_string(),
        ),
        "search_game_tree" => {
            let dm = args
                .get("datamodel_type")
                .and_then(|v| v.as_str())
                .unwrap_or("Edit");
            let path_filter = args
                .get("path")
                .and_then(|v| v.as_str())
                .map(normalize_game_path)
                .filter(|s| !s.is_empty());
            let class_filter = args
                .get("instance_type")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let keywords: Vec<String> = args
                .get("keywords")
                .and_then(|v| v.as_str())
                .map(|raw| {
                    raw.split([',', ' '])
                        .map(|s| s.trim().to_ascii_lowercase())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let max_depth = args
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize);
            let head_limit = args
                .get("head_limit")
                .and_then(|v| v.as_u64())
                .map(|n| (n as usize).clamp(1, 500))
                .unwrap_or(200);

            let mut results: Vec<Value> = Vec::new();
            let mut push_if_match = |path: &str, class_name: &str, kind: &str| {
                if let Some(pf) = path_filter.as_deref() {
                    let pl = path.to_ascii_lowercase();
                    let pfl = pf.to_ascii_lowercase();
                    if pl != pfl && !pl.starts_with(&format!("{pfl}.")) {
                        return;
                    }
                }
                let depth = path.split('.').count();
                if let Some(md) = max_depth {
                    if depth > md {
                        return;
                    }
                }
                if let Some(cf) = class_filter {
                    if !class_matches_filter(class_name, cf) {
                        return;
                    }
                }
                let leaf = path.rsplit('.').next().unwrap_or(path);
                if !keywords.is_empty() {
                    let lower_leaf = leaf.to_ascii_lowercase();
                    let lower_path = path.to_ascii_lowercase();
                    if !keywords
                        .iter()
                        .any(|kw| lower_leaf.contains(kw) || lower_path.contains(kw))
                    {
                        return;
                    }
                }
                results.push(json!({
                    "path": path,
                    "name": leaf,
                    "class_name": class_name,
                    "kind": kind,
                    "depth": depth,
                }));
            };

            for (p, rec) in &state.instances {
                let cls = rec
                    .get("class_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Part");
                push_if_match(p, cls, "instance");
            }
            for p in state.scripts.keys() {
                push_if_match(p, script_class_for_path(p), "script");
            }

            let total = results.len();
            let truncated = total > head_limit;
            results.truncate(head_limit);

            text_result(
                json!({
                    "datamodel_type": dm,
                    "count": results.len(),
                    "total": total,
                    "truncated": truncated,
                    "results": results,
                })
                .to_string(),
            )
        }
        "inspect_instance" => {
            let raw_path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let target = normalize_game_path(raw_path);
            if target.is_empty() {
                return error_result("inspect_instance requires a non-empty path".to_string());
            }
            let target_lower = target.to_ascii_lowercase();

            if let Some((path, rec)) = state
                .instances
                .iter()
                .find(|(k, _)| k.to_ascii_lowercase() == target_lower)
            {
                let prefix = format!("{path}.");
                let mut children: Vec<String> = state
                    .instances
                    .keys()
                    .chain(state.scripts.keys())
                    .filter(|k| k.starts_with(&prefix))
                    .cloned()
                    .collect();
                children.sort();
                let leaf = path.rsplit('.').next().unwrap_or(path);
                return text_result(
                    json!({
                        "path": path,
                        "name": leaf,
                        "class_name": rec.get("class_name").and_then(|v| v.as_str()).unwrap_or("Part"),
                        "properties": rec.get("properties").cloned().unwrap_or(json!({})),
                        "attributes": {},
                        "children": children,
                    })
                    .to_string(),
                );
            }
            if let Some((path, src)) = state
                .scripts
                .iter()
                .find(|(k, _)| k.to_ascii_lowercase() == target_lower)
            {
                let leaf = path.rsplit('.').next().unwrap_or(path);
                return text_result(
                    json!({
                        "path": path,
                        "name": leaf,
                        "class_name": script_class_for_path(path),
                        "properties": {
                            "Disabled": false,
                            "LineCount": src.lines().count(),
                            "ByteLength": src.len(),
                        },
                        "attributes": {},
                        "children": [],
                    })
                    .to_string(),
                );
            }
            error_result(format!("no instance or script found at '{target}'"))
        }
        "script_read" => {
            let raw_file = args
                .get("target_file")
                .or_else(|| args.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target = normalize_game_path(raw_file);
            let target_lower = target.to_ascii_lowercase();
            let Some((path, source)) = state
                .scripts
                .iter()
                .find(|(k, _)| k.to_ascii_lowercase() == target_lower)
            else {
                return error_result(format!("script not found: '{target}'"));
            };

            let all_lines: Vec<&str> = source.lines().collect();
            let total_lines = all_lines.len();
            let read_all = args
                .get("should_read_entire_file")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let (start_1, end_1) = if read_all || total_lines == 0 {
                (1usize, total_lines.max(1))
            } else {
                let s = args
                    .get("start_line_one_indexed")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1)
                    .max(1) as usize;
                let e = args
                    .get("end_line_one_indexed_inclusive")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(total_lines as u64)
                    .max(s as u64) as usize;
                (s.min(total_lines), e.min(total_lines))
            };

            let slice: Vec<String> = if total_lines == 0 {
                Vec::new()
            } else {
                all_lines[start_1 - 1..end_1]
                    .iter()
                    .enumerate()
                    .map(|(i, line)| format!("{}: {line}", start_1 + i))
                    .collect()
            };
            let content = if total_lines == 0 {
                String::new()
            } else {
                all_lines[start_1 - 1..end_1].join("\n")
            };

            text_result(
                json!({
                    "path": path,
                    "class_name": script_class_for_path(path),
                    "total_lines": total_lines,
                    "start_line": start_1,
                    "end_line": end_1,
                    "source": content,
                    "numbered_lines": slice,
                })
                .to_string(),
            )
        }
        "script_grep" => {
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            if query.is_empty() {
                return error_result("script_grep requires a non-empty query".to_string());
            }
            let q_lower = query.to_ascii_lowercase();
            let mut matches = Vec::new();
            for (path, source) in &state.scripts {
                for (idx, line) in source.lines().enumerate() {
                    if line.to_ascii_lowercase().contains(&q_lower) {
                        matches.push(json!({
                            "path": path,
                            "line": idx + 1,
                            "text": line,
                        }));
                    }
                }
            }
            text_result(
                json!({
                    "query": query,
                    "match_count": matches.len(),
                    "matches": matches,
                })
                .to_string(),
            )
        }
        "multi_edit" => {
            let raw_path = args
                .get("file_path")
                .or_else(|| args.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let path = normalize_game_path(raw_path);
            if bad_path(&path) {
                return error_result(format!("invalid multi_edit file_path: '{raw_path}'"));
            }
            let Some(edits) = args.get("edits").and_then(|v| v.as_array()) else {
                return error_result("multi_edit requires an 'edits' array".to_string());
            };
            if edits.is_empty() {
                return error_result("multi_edit requires at least one edit".to_string());
            }
            let mut working = state.scripts.get(&path).cloned().unwrap_or_default();
            for (idx, edit) in edits.iter().enumerate() {
                let old_s = edit
                    .get("old_string")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let new_s = edit
                    .get("new_string")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let replace_all = edit
                    .get("replace_all")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if old_s.is_empty() {
                    working = new_s.to_string();
                } else if !working.contains(old_s) {
                    return error_result(format!(
                        "multi_edit edit[{idx}] old_string not found in '{path}'"
                    ));
                } else if replace_all {
                    working = working.replace(old_s, new_s);
                } else {
                    working = working.replacen(old_s, new_s, 1);
                }
            }
            let lines = working.lines().count();
            let bytes = working.len();
            state.scripts.insert(path.clone(), working);
            state.multi_edit_count += 1;
            state.log_line(format!(
                "[MultiEdit] Updated {path} ({} edit(s), {lines} lines)",
                edits.len()
            ));
            text_result(
                json!({
                    "ok": true,
                    "path": path,
                    "edits_applied": edits.len(),
                    "lines": lines,
                    "bytes": bytes,
                })
                .to_string(),
            )
        }
        "execute_luau" => {
            let code = args.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let dm = args
                .get("datamodel_type")
                .and_then(|v| v.as_str())
                .unwrap_or("Edit");
            if code.trim().is_empty() {
                return error_result("execute_luau requires non-empty code".to_string());
            }
            if let Some(reply) = handle_v011_luau(code, dm, state) {
                state.luau_exec_count += 1;
                return reply;
            }
            let mut outputs: Vec<String> = Vec::new();
            let mut created_instances: Vec<String> = Vec::new();

            // Capture deterministic print("...") / print('...') calls into Studio Output
            for line in code.lines() {
                let t = line.trim();
                if let Some(inner) = extract_print_literal(t) {
                    outputs.push(inner.clone());
                    state.log_line(format!("[Luau:{dm}] {inner}"));
                }
            }

            // Deterministically apply simple `Instance.new("Part") ... .Name = "Foo"` statements
            if let Some((cls, inst_name, pos, color)) = parse_simple_luau_instance(code) {
                let parent = if cls == "RemoteEvent" || cls == "RemoteFunction" {
                    "ReplicatedStorage"
                } else {
                    "Workspace"
                };
                let full_path = format!("{parent}.{inst_name}");
                let mut props = serde_json::Map::new();
                props.insert("Anchored".to_string(), json!(true));
                props.insert("Position".to_string(), json!(pos));
                props.insert("Size".to_string(), json!([6.0, 2.0, 6.0]));
                if let Some(hex) = color {
                    props.insert("Color".to_string(), json!(hex));
                }
                state.instances.insert(
                    full_path.clone(),
                    json!({ "class_name": cls, "properties": Value::Object(props) }),
                );
                created_instances.push(full_path.clone());
                state.log_line(format!("[ExecuteLuau:{dm}] Created {cls} at {full_path}"));
            } else {
                state.log_line(format!(
                    "[ExecuteLuau:{dm}] Executed {} bytes of Luau",
                    code.len()
                ));
            }

            state.luau_exec_count += 1;
            text_result(
                json!({
                    "ok": true,
                    "datamodel_type": dm,
                    "executed_bytes": code.len(),
                    "created_instances": created_instances,
                    "output": outputs.join("\n"),
                    "lines": outputs,
                })
                .to_string(),
            )
        }
        "user_mouse_input" => {
            let Some(actions) = args.get("actions").and_then(|v| v.as_array()) else {
                return error_result("user_mouse_input requires an 'actions' array".to_string());
            };
            if actions.is_empty() {
                return error_result("user_mouse_input actions must not be empty".to_string());
            }
            for act in actions {
                let kind = act.get("action").and_then(|v| v.as_str()).unwrap_or("");
                let new_x = act
                    .get("x")
                    .and_then(|v| v.as_f64())
                    .map(|v| scale_to_viewport(v, FRAME_W));
                let new_y = act
                    .get("y")
                    .and_then(|v| v.as_f64())
                    .map(|v| scale_to_viewport(v, FRAME_H));

                match kind {
                    "moveTo" => {
                        if let (Some(nx), Some(ny)) = (new_x, new_y) {
                            if state.mouse_down {
                                let dx = nx - state.cursor_x;
                                let dy = ny - state.cursor_y;
                                state.camera_yaw = (state.camera_yaw + dx * 0.025)
                                    .clamp(-std::f64::consts::PI, std::f64::consts::PI);
                                state.camera_pitch =
                                    (state.camera_pitch + dy * 0.015).clamp(-0.9, 0.9);
                            }
                            state.cursor_x = nx;
                            state.cursor_y = ny;
                        }
                    }
                    "mouseButtonDown" => {
                        if let (Some(nx), Some(ny)) = (new_x, new_y) {
                            state.cursor_x = nx;
                            state.cursor_y = ny;
                        }
                        state.mouse_down = true;
                    }
                    "mouseButtonUp" => {
                        if let (Some(nx), Some(ny)) = (new_x, new_y) {
                            state.cursor_x = nx;
                            state.cursor_y = ny;
                        }
                        state.mouse_down = false;
                    }
                    "mouseButtonClick" => {
                        if let (Some(nx), Some(ny)) = (new_x, new_y) {
                            state.cursor_x = nx;
                            state.cursor_y = ny;
                        }
                        state.mouse_clicks += 1;
                        if let Some(ip) = act.get("instance_path").and_then(|v| v.as_str()) {
                            state.selected_instance = Some(normalize_game_path(ip));
                        } else {
                            state.selected_instance = pick_nearest_instance(state);
                        }
                    }
                    "scrollUp" => {
                        state.camera_zoom = (state.camera_zoom * 1.15).clamp(0.5, 2.2);
                    }
                    "scrollDown" => {
                        state.camera_zoom = (state.camera_zoom / 1.15).clamp(0.5, 2.2);
                    }
                    "wait" => {}
                    other => {
                        return error_result(format!("unsupported mouse action '{other}'"));
                    }
                }
            }
            state.log_line(format!(
                "[Input:Mouse] {} action(s) cursor=({:.0},{:.0}) yaw={:.2} zoom={:.2}",
                actions.len(),
                state.cursor_x,
                state.cursor_y,
                state.camera_yaw,
                state.camera_zoom
            ));
            text_result(
                json!({
                    "ok": true,
                    "applied": actions.len(),
                    "cursor": [state.cursor_x, state.cursor_y],
                    "mouse_clicks": state.mouse_clicks,
                    "selected_instance": state.selected_instance,
                    "camera": {
                        "yaw": state.camera_yaw,
                        "pitch": state.camera_pitch,
                        "zoom": state.camera_zoom,
                    },
                })
                .to_string(),
            )
        }
        "user_keyboard_input" => {
            let Some(actions) = args.get("actions").and_then(|v| v.as_array()) else {
                return error_result("user_keyboard_input requires an 'actions' array".to_string());
            };
            if actions.is_empty() {
                return error_result("user_keyboard_input actions must not be empty".to_string());
            }
            for act in actions {
                let kind = act.get("action").and_then(|v| v.as_str()).unwrap_or("");
                match kind {
                    "keyDown" | "keyPress" => {
                        let key = act
                            .get("key_code")
                            .and_then(|v| v.as_str())
                            .unwrap_or("None");
                        state.last_key = Some(key.to_string());
                        state.key_events += 1;
                        match key {
                            "W" | "Up" => state.camera_pan_z -= 4.0,
                            "S" | "Down" => state.camera_pan_z += 4.0,
                            "A" | "Left" => state.camera_pan_x -= 4.0,
                            "D" | "Right" => state.camera_pan_x += 4.0,
                            "F" => {
                                state.camera_pan_x = 0.0;
                                state.camera_pan_z = 0.0;
                                state.camera_zoom = 1.15;
                            }
                            "Escape" => {
                                state.selected_instance = None;
                            }
                            _ => {}
                        }
                    }
                    "keyUp" => {
                        if let Some(key) = act.get("key_code").and_then(|v| v.as_str()) {
                            state.last_key = Some(key.to_string());
                        }
                    }
                    "textInput" => {
                        if let Some(txt) = act.get("text_inputs").and_then(|v| v.as_str()) {
                            state.typed_text = txt.chars().take(64).collect();
                            state.last_key = Some("TextInput".to_string());
                            state.key_events += 1;
                        }
                    }
                    "wait" => {}
                    other => {
                        return error_result(format!("unsupported keyboard action '{other}'"));
                    }
                }
            }
            state.log_line(format!(
                "[Input:Keyboard] {} action(s) last_key={:?}",
                actions.len(),
                state.last_key
            ));
            text_result(
                json!({
                    "ok": true,
                    "applied": actions.len(),
                    "last_key": state.last_key,
                    "typed_text": state.typed_text,
                    "key_events": state.key_events,
                    "camera": {
                        "pan": [state.camera_pan_x, state.camera_pan_z],
                        "zoom": state.camera_zoom,
                    },
                })
                .to_string(),
            )
        }

        // -------------------------------------------------------------------
        // Legacy + Live Play/Console/Capture tools
        // -------------------------------------------------------------------
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
            if state.instances.contains_key(&path) {
                return error_result(format!("instance already exists at {path}"));
            }
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
            if state.selected_instance.as_deref() == Some(path) {
                state.selected_instance = None;
            }
            state.log_line(format!("[Studio] Deleted instance {path}"));
            text_result(json!({ "ok": true, "path": path }).to_string())
        }
        "playtest" => {
            let (passed, msg) = evaluate_playtest(&state.scripts, &state.instances);
            state.console = msg.to_string();
            state.log_line(format!("[Playtest] {msg}"));
            if std::env::var("MOCK_PLAYTEST_TEXT").is_ok() {
                return text_result(if passed {
                    "Playtest passed. 0 failures, 0 errors.".to_string()
                } else {
                    format!("Playtest failed. {msg}")
                });
            }
            if std::env::var("MOCK_ERR_CODE_ONLY").is_ok() {
                return text_result(
                    json!({ "status": "passed", "console": msg, "error": { "code": 0 } })
                        .to_string(),
                );
            }
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
            let resolved_mode = if let Some(m) = args.get("mode").and_then(|v| v.as_str()) {
                m
            } else if let Some(is_start) = args.get("is_start").and_then(|v| v.as_bool()) {
                if is_start {
                    "start_play"
                } else {
                    "stop"
                }
            } else {
                "start_play"
            };
            match resolved_mode {
                "start_play" | "run_server" => {
                    let (passed, msg) = evaluate_playtest(&state.scripts, &state.instances);
                    state.playing = true;
                    state.play_mode = resolved_mode.to_string();
                    state.active_datamodel = if resolved_mode == "run_server" {
                        "Server".to_string()
                    } else {
                        "Client".to_string()
                    };
                    state.console = msg.to_string();
                    state.log_line(format!(
                        "[Studio Play] Started {resolved_mode} ({} script(s), {} instance(s))",
                        state.scripts.len(),
                        state.instances.len()
                    ));
                    state.log_line(format!("[Studio Play] {msg}"));
                    text_result(
                        json!({
                            "ok": true,
                            "mode": resolved_mode,
                            "is_start": true,
                            "playing": true,
                            "active_datamodel": state.active_datamodel,
                            "status": if passed { "passed" } else { "failed" },
                            "console": msg,
                            "lines": state.console_lines,
                        })
                        .to_string(),
                    )
                }
                "stop" => {
                    state.playing = false;
                    state.play_mode = "edit".to_string();
                    state.active_datamodel = "Edit".to_string();
                    state.log_line("[Studio Play] Stopped playtest session.".to_string());
                    text_result(
                        json!({
                            "ok": true,
                            "mode": "stop",
                            "is_start": false,
                            "playing": false,
                            "active_datamodel": "Edit",
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
            if let Some(cam) = args.get("camera_position").and_then(|v| v.as_array()) {
                if cam.len() >= 3 {
                    if let (Some(cx), Some(cz)) = (cam[0].as_f64(), cam[2].as_f64()) {
                        state.camera_yaw =
                            (cx / 40.0).clamp(-std::f64::consts::PI, std::f64::consts::PI);
                        state.camera_pan_z = (cz / 4.0).clamp(-30.0, 30.0);
                    }
                }
            }
            state.frame_counter += 1;
            let png = render_studio_scene_png(state);
            let b64 = encode_base64(&png);
            let meta = json!({
                "width": FRAME_W,
                "height": FRAME_H,
                "instances": state.instances.len(),
                "scripts": state.scripts.len(),
                "playing": state.playing,
                "selected_instance": state.selected_instance,
                "cursor": [state.cursor_x, state.cursor_y],
                "last_key": state.last_key,
                "camera_yaw": state.camera_yaw,
                "camera_zoom": state.camera_zoom,
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

fn scale_to_viewport(coord: f64, dim: usize) -> f64 {
    if !coord.is_finite() {
        return (dim as f64) * 0.5;
    }
    // Accept normalized [0..1] or full-resolution coordinates cleanly
    let scaled = if (0.0..=1.0).contains(&coord) {
        coord * (dim as f64)
    } else if coord > dim as f64 {
        (coord / 1440.0) * (dim as f64)
    } else {
        coord
    };
    scaled.clamp(0.0, (dim - 1) as f64)
}

fn extract_print_literal(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix("print(")?;
    let inner = rest.strip_suffix(')')?.trim();
    if (inner.starts_with('"') && inner.ends_with('"') && inner.len() >= 2)
        || (inner.starts_with('\'') && inner.ends_with('\'') && inner.len() >= 2)
    {
        Some(inner[1..inner.len() - 1].to_string())
    } else if !inner.is_empty() {
        Some(inner.to_string())
    } else {
        None
    }
}

fn parse_simple_luau_instance(code: &str) -> Option<(String, String, [f64; 3], Option<String>)> {
    let marker = "Instance.new(\"";
    let start = code.find(marker)? + marker.len();
    let end = code[start..].find('"')? + start;
    let cls = code[start..end].to_string();

    let name_marker = ".Name = \"";
    let name = if let Some(ns) = code.find(name_marker) {
        let s = ns + name_marker.len();
        code[s..]
            .find('"')
            .map(|e| code[s..s + e].to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("Luau{cls}"))
    } else {
        format!("Luau{cls}")
    };

    let pos = [0.0, 4.0, 0.0];
    let color = if code.contains("Bright red") || code.contains("255, 0, 0") {
        Some("#EF4444".to_string())
    } else if code.contains("Bright green") || code.contains("0, 255, 0") {
        Some("#22C55E".to_string())
    } else if code.contains("Cyan") || code.contains("0, 200, 255") {
        Some("#06B6D4".to_string())
    } else {
        Some("#38BDF8".to_string())
    };
    Some((cls, name, pos, color))
}

fn pick_nearest_instance(state: &MockState) -> Option<String> {
    let horizon = 74.0 + state.camera_pitch * 26.0;
    let mut best: Option<(f64, String)> = None;
    for (idx, (path, rec)) in state.instances.iter().enumerate() {
        let props = rec.get("properties").unwrap_or(&Value::Null);
        let pos = parse_vec3(props.get("Position")).unwrap_or([
            ((idx as f64 % 5.0) - 2.0) * 14.0,
            2.0,
            (idx as f64 / 5.0).floor() * 14.0,
        ]);
        let (sx, sy) = project_part(pos, state, horizon);
        let dx = sx - state.cursor_x;
        let dy = sy - state.cursor_y;
        let d2 = dx * dx + dy * dy;
        if best.as_ref().is_none_or(|(bd, _)| d2 < *bd) {
            best = Some((d2, path.clone()));
        }
    }
    best.map(|(_, p)| p)
}

fn project_part(pos: [f64; 3], state: &MockState, horizon: f64) -> (f64, f64) {
    let px = pos[0] - state.camera_pan_x;
    let pz = pos[2] - state.camera_pan_z;
    let cos_y = state.camera_yaw.cos();
    let sin_y = state.camera_yaw.sin();
    let rx = px * cos_y - pz * sin_y;
    let rz = px * sin_y + pz * cos_y;
    let zoom = state.camera_zoom;
    let sx = (FRAME_W as f64) * 0.5 + (rx - rz) * 2.1 * zoom;
    let sy = horizon + 54.0 + (rx + rz) * 0.72 * zoom - pos[1] * 2.4 * zoom;
    (sx, sy)
}

// ---------------------------------------------------------------------------
// Dynamic 320x200 Roblox Studio Viewport PNG Renderer (zero dependencies)
// ---------------------------------------------------------------------------

fn render_studio_scene_png(state: &MockState) -> Vec<u8> {
    let mut rgb = vec![0u8; FRAME_W * FRAME_H * 3];

    let horizon_f = (74.0 + state.camera_pitch * 26.0).clamp(42.0, 110.0);
    let horizon = horizon_f as usize;

    // 1. Sky gradient + perspective baseplate floor + stud grid
    for y in 0..FRAME_H {
        for x in 0..FRAME_W {
            let color = if y < 6 && state.playing {
                // Play-mode green indicator ribbon across top of Studio viewport
                [46, 196, 118]
            } else if y < horizon {
                let t = y as f32 / horizon.max(1) as f32;
                let r = (128.0 + 70.0 * t) as u8;
                let g = (176.0 + 52.0 * t) as u8;
                let b = (232.0 + 18.0 * t) as u8;
                [r, g, b]
            } else {
                let dy = (y - horizon) as f32 + 1.0;
                let yaw_shift = (state.camera_yaw * 18.0) as f32;
                let gx = (((x as f32) + yaw_shift - (FRAME_W as f32 * 0.5)) / (dy * 0.14 + 6.0))
                    .floor() as i32;
                let gz = (dy / (14.0 * state.camera_zoom as f32).max(4.0)).floor() as i32;
                let is_line = ((x as i32 + y as i32 + yaw_shift as i32) % 16 == 0)
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
        path: String,
        depth: f64,
        pos: [f64; 3],
        size: [f64; 3],
        color: [u8; 3],
    }

    // 2. Project each Workspace Part/SpawnLocation as a shaded isometric 3D box
    let mut parts: Vec<RenderPart> = Vec::new();
    for (idx, (path, rec)) in state.instances.iter().enumerate() {
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
            path: path.clone(),
            depth,
            pos,
            size,
            color,
        });
    }
    parts.sort_by(|a, b| a.depth.partial_cmp(&b.depth).unwrap_or(std::cmp::Ordering::Equal));

    for part in parts {
        let (sx_f, sy_f) = project_part(part.pos, state, horizon_f);
        let sx = sx_f as i32;
        let sy = sy_f as i32;
        let zoom = state.camera_zoom;
        let w = (part.size[0].max(part.size[2]) * 2.6 * zoom).clamp(10.0, 96.0) as i32;
        let h = (part.size[1] * 3.2 * zoom).clamp(8.0, 64.0) as i32;
        let top_h = (w / 4).clamp(4, 18);

        let top_col = shade(part.color, 1.18);
        let side_col = shade(part.color, 0.78);
        let selected = state.selected_instance.as_deref() == Some(part.path.as_str());
        let border_col = if selected {
            [250, 204, 21] // bright Studio selection gold highlight
        } else {
            shade(part.color, 0.45)
        };

        fill_rect(&mut rgb, sx - w / 2, sy - h, w, h, side_col);
        fill_rect(&mut rgb, sx - w / 2, sy - h - top_h, w, top_h, top_col);
        stroke_rect(&mut rgb, sx - w / 2, sy - h - top_h, w, h + top_h, border_col);
        if selected {
            stroke_rect(
                &mut rgb,
                sx - w / 2 - 2,
                sy - h - top_h - 2,
                w + 4,
                h + top_h + 4,
                [250, 204, 21],
            );
        }
    }

    // 3. Top-left HUD strip encoding script count, multi_edit, and execute_luau activity
    let script_bars = state.scripts.len().min(14);
    let edit_pips = (state.multi_edit_count as usize).min(4);
    let luau_pips = (state.luau_exec_count as usize).min(4);
    let hud_w = 16 + (script_bars + edit_pips + luau_pips) as i32 * 8;
    fill_rect(&mut rgb, 8, 10, hud_w.max(26), 14, [24, 28, 38]);
    let mut hx = 12i32;
    for _ in 0..script_bars {
        fill_rect(&mut rgb, hx, 13, 5, 8, [96, 165, 250]); // blue = scripts
        hx += 8;
    }
    for _ in 0..edit_pips {
        fill_rect(&mut rgb, hx, 13, 5, 8, [251, 191, 36]); // amber = multi_edit
        hx += 8;
    }
    for _ in 0..luau_pips {
        fill_rect(&mut rgb, hx, 13, 5, 8, [34, 211, 238]); // cyan = execute_luau
        hx += 8;
    }

    // 4. Top-right keyboard activity badge when user_keyboard_input has fired
    if state.key_events > 0 {
        let key_bars = (state.key_events as usize).min(8);
        let kw = 10 + (key_bars as i32) * 6;
        let kx = (FRAME_W as i32) - kw - 8;
        fill_rect(&mut rgb, kx, 10, kw, 14, [30, 24, 48]);
        for i in 0..key_bars {
            fill_rect(&mut rgb, kx + 4 + (i as i32) * 6, 13, 4, 8, [192, 132, 252]);
        }
    }

    // 5. Interactive cursor crosshair & click ripple ring when mouse has interacted
    if state.mouse_clicks > 0 || state.mouse_down || state.camera_yaw.abs() > 0.001 {
        let cx = state.cursor_x as i32;
        let cy = state.cursor_y as i32;
        let cross_col = if state.mouse_down {
            [249, 115, 22] // orange drag
        } else {
            [56, 189, 248] // sky cyan pointer
        };
        fill_rect(&mut rgb, cx - 6, cy, 13, 1, cross_col);
        fill_rect(&mut rgb, cx, cy - 6, 1, 13, cross_col);
        if state.mouse_clicks > 0 {
            let r = 6 + ((state.mouse_clicks % 3) as i32) * 3;
            stroke_rect(&mut rgb, cx - r, cy - r, r * 2 + 1, r * 2 + 1, [250, 204, 21]);
        }
    }

    // 6. Frame heartbeat dot (bottom-right)
    let pulse = if state.frame_counter.is_multiple_of(2) {
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

fn v011_hash31(s: &str) -> u64 {
    let mut h: u64 = 0;
    for b in s.as_bytes() {
        h = (h * 31 + *b as u64) % 4_294_967_296;
    }
    h
}

fn parse_luau_payload(code: &str) -> Option<Value> {
    let idx = code.find("local PAYLOAD = [")?;
    let after = &code[idx + "local PAYLOAD = [".len()..];
    let eq_count = after.bytes().take_while(|b| *b == b'=').count();
    let open_rest = &after[eq_count..];
    if !open_rest.starts_with('[') {
        return None;
    }
    let body = &open_rest[1..];
    let close = format!("]{}]", "=".repeat(eq_count));
    let end = body.find(&close)?;
    let raw = &body[..end];
    let raw = raw.strip_prefix('\n').unwrap_or(raw);
    serde_json::from_str(raw).ok()
}

fn truncate_100k(s: String) -> String {
    if s.len() <= 100_000 {
        s
    } else {
        s.chars().take(100_000).collect()
    }
}

fn handle_v011_luau(code: &str, dm: &str, state: &mut MockState) -> Option<Value> {
    let first = code.lines().next().unwrap_or("").trim();
    if first == "-- openro:v011:sync_compare" {
        let p = parse_luau_payload(code)?;
        let mut out: Vec<Value> = Vec::new();
        let mut extra: Vec<String> = Vec::new();
        let mut ok_count = 0usize;
        for it in p["items"].as_array().cloned().unwrap_or_default() {
            let path = it["p"].as_str().unwrap_or("").to_string();
            state.v011_compare_order.push(path.clone());
            let cls = it["c"].as_str().unwrap_or("");
            let is_svc = it.get("svc").is_some();
            match state.v011_tree.get(&path) {
                None => out.push(json!({ "p": path, "s": "missing" })),
                Some(node) if !is_svc && node.class != cls => {
                    out.push(json!({ "p": path, "s": "class", "a": node.class }));
                }
                Some(node) => {
                    let mut bad: Vec<String> = Vec::new();
                    if let Some(h) = it["h"].as_u64() {
                        let want_len = it["n"].as_u64().unwrap_or(0) as usize;
                        let src = node.source.as_deref().unwrap_or("");
                        if src.len() != want_len || v011_hash31(src) != h {
                            bad.push("Source".into());
                        }
                    }
                    if let Some(pr) = it["pr"].as_object() {
                        for (k, v) in pr {
                            if node.props.get(k) != Some(v) {
                                bad.push(k.clone());
                            }
                        }
                    }
                    if !bad.is_empty() {
                        out.push(json!({ "p": path, "s": "props", "b": bad }));
                    } else {
                        ok_count += 1;
                    }
                    if let Some(known) = it["k"].as_array() {
                        let names: Vec<&str> = known.iter().filter_map(|x| x.as_str()).collect();
                        let prefix = format!("{path}/");
                        for key in state.v011_tree.keys() {
                            if let Some(rest) = key.strip_prefix(&prefix) {
                                if !rest.contains('/') && !names.contains(&rest) && extra.len() < 200 {
                                    extra.push(key.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
        let res = truncate_100k(format!(
            "OPENRO_V011:{}",
            json!({ "r": out, "x": extra, "ok": ok_count })
        ));
        return Some(text_result(
            json!({ "ok": true, "datamodel_type": dm, "output": res }).to_string(),
        ));
    }

    if first == "-- openro:v011:sync_apply" {
        let p = parse_luau_payload(code)?;
        let mut applied = 0usize;
        let mut errors: Vec<Value> = Vec::new();
        for op in p["ops"].as_array().cloned().unwrap_or_default() {
            let kind = op["k"].as_str().unwrap_or("");
            let path = op["p"].as_str().unwrap_or("").to_string();
            let cls = op["c"].as_str().unwrap_or("Folder").to_string();
            match kind {
                "delete" => {
                    let prefix = format!("{path}/");
                    let remove_keys: Vec<String> = state
                        .v011_tree
                        .keys()
                        .filter(|k| *k == &path || k.starts_with(&prefix))
                        .cloned()
                        .collect();
                    for k in remove_keys {
                        state.v011_tree.remove(&k);
                        let dot = k.replace('/', ".");
                        state.scripts.remove(&dot);
                        state.instances.remove(&dot);
                    }
                    applied += 1;
                }
                "create" | "replace" => {
                    let Some((parent, _)) = path.rsplit_once('/') else {
                        errors.push(json!({ "p": path, "m": "cannot create a service" }));
                        continue;
                    };
                    if !state.v011_tree.contains_key(parent) {
                        errors.push(json!({ "p": path, "m": format!("parent is missing: {parent}") }));
                        continue;
                    }
                    if kind == "replace" {
                        let prefix = format!("{path}/");
                        let remove_keys: Vec<String> = state
                            .v011_tree
                            .keys()
                            .filter(|k| *k == &path || k.starts_with(&prefix))
                            .cloned()
                            .collect();
                        for k in remove_keys {
                            state.v011_tree.remove(&k);
                            let dot = k.replace('/', ".");
                            state.scripts.remove(&dot);
                            state.instances.remove(&dot);
                        }
                    }
                    let mut props = BTreeMap::new();
                    if let Some(pr) = op["pr"].as_object() {
                        for (k, v) in pr {
                            props.insert(k.clone(), v.clone());
                        }
                    }
                    let src = op["s"].as_str().map(String::from);
                    let dot = path.replace('/', ".");
                    if let Some(ref s) = src {
                        state.scripts.insert(dot, s.clone());
                    } else {
                        state.instances.insert(
                            dot,
                            json!({ "class_name": cls, "properties": props }),
                        );
                    }
                    state.v011_tree.insert(path, MockNode { class: cls, props, source: src });
                    applied += 1;
                }
                _ => {
                    let Some(node) = state.v011_tree.get_mut(&path) else {
                        errors.push(json!({ "p": path, "m": "missing" }));
                        continue;
                    };
                    if let Some(pr) = op["pr"].as_object() {
                        for (k, v) in pr {
                            node.props.insert(k.clone(), v.clone());
                        }
                    }
                    if let Some(s) = op["s"].as_str() {
                        if op["a"].as_bool() == Some(true) {
                            let cur = node.source.get_or_insert_with(String::new);
                            cur.push_str(s);
                        } else {
                            node.source = Some(s.to_string());
                        }
                        if let Some(ref full_src) = node.source {
                            state.scripts.insert(path.replace('/', "."), full_src.clone());
                        }
                    }
                    applied += 1;
                }
            }
        }
        let res = truncate_100k(format!(
            "OPENRO_V011:{}",
            json!({ "n": applied, "e": errors })
        ));
        return Some(text_result(
            json!({ "ok": true, "datamodel_type": dm, "output": res }).to_string(),
        ));
    }

    if first == "-- openro:v011:check" {
        let start = code.find("--[[USER]]\n")? + "--[[USER]]\n".len();
        let end = code[start..].find("\n--[[/USER]]")? + start;
        let user = code[start..end].trim();

        for line in user.lines() {
            if let Some(msg) = extract_print_literal(line.trim()) {
                state.log_line(msg);
            }
        }

        // error("...") simulation
        if let Some(rest) = user.strip_prefix("error(") {
            if let Some(inner) = rest.strip_suffix(')') {
                let msg = inner.trim().trim_matches('"').trim_matches('\'');
                state.log_line(format!("Error: {msg}"));
                let res = format!("OPENRO_V011:{}", json!({ "ok": false, "e": msg }));
                return Some(text_result(
                    json!({ "ok": true, "datamodel_type": dm, "output": res }).to_string(),
                ));
            }
        }

        // Last line `return <expr>` deterministic evaluator
        let ret_line = user.lines().rev().find(|l| l.trim().starts_with("return ")).map(|l| l.trim());
        let val: Value = match ret_line.and_then(|l| l.strip_prefix("return ")).map(str::trim) {
            Some("__dm__") => json!(dm),
            Some("__compare_order__") => json!(state.v011_compare_order),
            Some(expr) if expr.starts_with("__exists__(") && expr.ends_with(')') => {
                let p = expr["__exists__(".len()..expr.len() - 1].trim().trim_matches('"').trim_matches('\'');
                json!(state.v011_tree.contains_key(p))
            }
            Some(expr) if expr.starts_with("__source_len__(") && expr.ends_with(')') => {
                let p = expr["__source_len__(".len()..expr.len() - 1].trim().trim_matches('"').trim_matches('\'');
                let len = state.v011_tree.get(p).and_then(|n| n.source.as_ref()).map(|s| s.len()).unwrap_or(0);
                json!(len)
            }
            Some(expr) if expr.starts_with("__prop__(") && expr.ends_with(')') => {
                let inner = &expr["__prop__(".len()..expr.len() - 1];
                if let Some((p, k)) = inner.split_once(',') {
                    let p = p.trim().trim_matches('"').trim_matches('\'');
                    let k = k.trim().trim_matches('"').trim_matches('\'');
                    state.v011_tree.get(p).and_then(|n| n.props.get(k)).cloned().unwrap_or(Value::Null)
                } else {
                    Value::Null
                }
            }
            Some("true") => json!(true),
            Some("false") => json!(false),
            Some("nil") => Value::Null,
            Some(expr) if expr.starts_with('"') && expr.ends_with('"') && expr.len() >= 2 => {
                json!(&expr[1..expr.len() - 1])
            }
            Some(expr) if expr.parse::<f64>().is_ok() => {
                if let Ok(i) = expr.parse::<i64>() {
                    json!(i)
                } else {
                    json!(expr.parse::<f64>().unwrap_or(0.0))
                }
            }
            _ => json!(true),
        };
        let encoded = json!({ "v": val }).to_string();
        let res = format!("OPENRO_V011:{}", json!({ "ok": true, "j": encoded }));
        return Some(text_result(
            json!({ "ok": true, "datamodel_type": dm, "output": res }).to_string(),
        ));
    }

    None
}
