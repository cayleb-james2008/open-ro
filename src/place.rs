//! Persistent Local Roblox Place Engine, Luau static/runtime playtest
//! verifier, and genuine Roblox Studio XML (`.rbxlx`) serializer + importer.
//!
//! When Roblox Studio MCP is not running, OpenRO operates directly on a real
//! persistent place stored in `data_dir/place.json` and mirrored atomically to
//! `data_dir/game.rbxlx` (`<roblox version="4">`) on every mutation and Undo.
//! `serialize_rbxlx` also works on live Studio MCP place trees so
//! `GET /api/export/rbxlx` always exports an authentic Roblox XML file, and
//! `Place::from_rbxlx_xml` parses Roblox Studio `.rbxlx` XML back into the
//! OpenRO Place DataModel cleanly.

use crate::luau_check::{self, AnalysisReport, Diagnostic, VALID_SERVICES};
use crate::mcp::McpClient;
use crate::skills;
use crate::studio_plugin::StudioPluginBridge;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Maximum depth of the bounded in-memory Undo / Redo history stacks.
pub const MAX_HISTORY_DEPTH: usize = 64;
const MAX_SCRIPT_BYTES: usize = 512 * 1024;

/// Canonical Place DataModel alias (`Place::from_rbxlx_xml(&str) -> Result<Place, String>`).
pub type Place = LocalPlace;

fn plugin_timeout() -> Duration {
    let secs = std::env::var("OPENRO_BRIDGE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|s| *s >= 1)
        .unwrap_or(15);
    Duration::from_secs(secs)
}

/// Unified backend for either a live Roblox Studio MCP bridge, a connected
/// Companion Studio Plugin (`OpenRoCompanion.lua`), or the built-in
/// persistent Local Place Engine.
pub enum PlaceBackend {
    Mcp(McpClient),
    Plugin(Arc<StudioPluginBridge>),
    Local(LocalPlace),
}

impl PlaceBackend {
    pub fn is_alive(&mut self) -> bool {
        match self {
            PlaceBackend::Mcp(c) => c.is_alive(),
            PlaceBackend::Plugin(p) => p.is_connected(),
            PlaceBackend::Local(_) => true,
        }
    }

    pub fn tools_known(&self) -> bool {
        match self {
            PlaceBackend::Mcp(c) => c.tools_known(),
            PlaceBackend::Plugin(_) | PlaceBackend::Local(_) => true,
        }
    }

    pub fn has_tool(&self, name: &str) -> bool {
        match self {
            PlaceBackend::Mcp(c) => c.has_tool(name),
            PlaceBackend::Plugin(_) => matches!(
                name,
                "get_place_tree"
                    | "get_place_snapshot"
                    | "write_script"
                    | "write_luau"
                    | "delete_script"
                    | "create_instance"
                    | "delete_instance"
                    | "playtest"
                    | "get_console"
                    | "get_console_output"
                    | "execute_luau"
                    | "start_stop_play"
                    | "search_game_tree"
                    | "inspect_instance"
                    | "script_read"
                    | "script_grep"
                    | "multi_edit"
            ),
            PlaceBackend::Local(_) => matches!(
                name,
                "get_place_tree"
                    | "write_script"
                    | "delete_script"
                    | "create_instance"
                    | "delete_instance"
                    | "playtest"
                    | "get_console"
            ),
        }
    }

    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<String, String> {
        match self {
            PlaceBackend::Mcp(c) => c.call_tool(name, arguments),
            PlaceBackend::Plugin(p) => {
                let val = p.execute_command(name, arguments, plugin_timeout())?;
                if let Some(s) = val.as_str() {
                    Ok(s.to_string())
                } else {
                    Ok(val.to_string())
                }
            }
            PlaceBackend::Local(p) => p.call_tool(name, arguments),
        }
    }

    pub fn get_place_tree(&mut self) -> Result<Value, String> {
        match self {
            PlaceBackend::Mcp(c) => c.get_place_tree(),
            PlaceBackend::Plugin(p) => {
                let val = p.execute_command("get_place_tree", json!({}), plugin_timeout())?;
                if val.is_object() {
                    Ok(val)
                } else if let Some(s) = val.as_str() {
                    serde_json::from_str(s)
                        .map_err(|e| format!("plugin get_place_tree returned non-JSON: {e}"))
                } else {
                    Ok(json!({ "instances": {}, "scripts": {} }))
                }
            }
            PlaceBackend::Local(p) => Ok(p.tree_json()),
        }
    }

    /// Fetch a full cursor-paginated Studio snapshot (scripts, script_meta, instances, lighting)
    /// across `Plugin` (`get_place_snapshot`), `Mcp` (`get_place_snapshot` / `execute_luau` / `get_place_tree`),
    /// or `Local` backends without exceeding 64 KiB per chunk.
    pub fn get_place_snapshot(&mut self) -> Result<Value, String> {
        match self {
            PlaceBackend::Plugin(p) => {
                let mut cursor: Option<String> = None;
                let mut snapshot_id: Option<String> = None;
                let mut scripts = Map::new();
                let mut script_parts: BTreeMap<String, BTreeMap<usize, String>> = BTreeMap::new();
                let mut script_meta = Map::new();
                let mut instances = Map::new();
                let mut lighting = Map::new();
                let mut finished = false;
                for _page in 0..512 {
                    let mut args = Map::new();
                    if let Some(c) = &cursor {
                        args.insert("cursor".into(), json!(c));
                    }
                    if let Some(sid) = &snapshot_id {
                        args.insert("snapshot_id".into(), json!(sid));
                    }
                    let val = p.execute_command(
                        "get_place_snapshot",
                        Value::Object(args),
                        plugin_timeout(),
                    )?;
                    let chunk: Value = if val.is_object() {
                        val
                    } else if let Some(s) = val.as_str() {
                        serde_json::from_str::<Value>(s).map_err(|e| {
                            format!("plugin get_place_snapshot returned non-JSON: {e}")
                        })?
                    } else {
                        return Err("plugin get_place_snapshot returned unexpected value".into());
                    };
                    if snapshot_id.is_none() {
                        if let Some(sid) = chunk.get("snapshot_id").and_then(|v| v.as_str()) {
                            snapshot_id = Some(sid.to_string());
                        }
                    }
                    if let Some(obj) = chunk.get("scripts").and_then(|v| v.as_object()) {
                        for (k, v) in obj {
                            scripts.insert(k.clone(), v.clone());
                        }
                    }
                    if let Some(parts_obj) = chunk.get("script_parts").and_then(|v| v.as_object()) {
                        for (path, part) in parts_obj {
                            if let (Some(idx), Some(data)) = (
                                part.get("index").and_then(|v| v.as_u64()),
                                part.get("data").and_then(|v| v.as_str()),
                            ) {
                                script_parts
                                    .entry(path.clone())
                                    .or_default()
                                    .insert(idx as usize, data.to_string());
                            }
                        }
                    }
                    if let Some(meta_obj) = chunk.get("script_meta").and_then(|v| v.as_object()) {
                        for (k, v) in meta_obj {
                            script_meta.insert(k.clone(), v.clone());
                        }
                    }
                    if let Some(inst_obj) = chunk.get("instances").and_then(|v| v.as_object()) {
                        for (k, v) in inst_obj {
                            instances.insert(k.clone(), v.clone());
                        }
                    }
                    if let Some(light_obj) = chunk.get("lighting").and_then(|v| v.as_object()) {
                        for (k, v) in light_obj {
                            lighting.insert(k.clone(), v.clone());
                        }
                    }
                    let done = chunk.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
                    let next = chunk
                        .get("next_cursor")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(String::from);
                    if done || next.is_none() {
                        finished = true;
                        break;
                    }
                    if next.as_deref() == cursor.as_deref() {
                        return Err("plugin get_place_snapshot pagination cursor stalled".into());
                    }
                    cursor = next;
                }
                if !finished {
                    return Err("plugin get_place_snapshot exceeded max 512 pages without completing".into());
                }
                for (path, parts) in script_parts {
                    let combined: String = parts.into_values().collect();
                    scripts.insert(path, Value::String(combined));
                }
                Ok(json!({
                    "snapshot_id": snapshot_id.unwrap_or_else(|| "snap-plugin".to_string()),
                    "scripts": Value::Object(scripts),
                    "script_meta": Value::Object(script_meta),
                    "instances": Value::Object(instances),
                    "lighting": Value::Object(lighting),
                }))
            }
            PlaceBackend::Mcp(c) => {
                if c.has_tool("get_place_snapshot") {
                    let mut cursor: Option<String> = None;
                    let mut snapshot_id: Option<String> = None;
                    let mut scripts = Map::new();
                    let mut script_parts: BTreeMap<String, BTreeMap<usize, String>> = BTreeMap::new();
                    let mut script_meta = Map::new();
                    let mut instances = Map::new();
                    let mut lighting = Map::new();
                    let mut finished = false;
                    for _page in 0..512 {
                        let mut args = Map::new();
                        if let Some(cur) = &cursor {
                            args.insert("cursor".into(), json!(cur));
                        }
                        if let Some(sid) = &snapshot_id {
                            args.insert("snapshot_id".into(), json!(sid));
                        }
                        let raw = c.call_tool("get_place_snapshot", Value::Object(args))?;
                        let chunk: Value = serde_json::from_str(&raw).map_err(|e| {
                            format!("mcp get_place_snapshot returned non-JSON: {e}")
                        })?;
                        if snapshot_id.is_none() {
                            if let Some(sid) = chunk.get("snapshot_id").and_then(|v| v.as_str()) {
                                snapshot_id = Some(sid.to_string());
                            }
                        }
                        if let Some(obj) = chunk.get("scripts").and_then(|v| v.as_object()) {
                            for (k, v) in obj {
                                scripts.insert(k.clone(), v.clone());
                            }
                        }
                        if let Some(parts_obj) = chunk.get("script_parts").and_then(|v| v.as_object()) {
                            for (path, part) in parts_obj {
                                if let (Some(idx), Some(data)) = (
                                    part.get("index").and_then(|v| v.as_u64()),
                                    part.get("data").and_then(|v| v.as_str()),
                                ) {
                                    script_parts
                                        .entry(path.clone())
                                        .or_default()
                                        .insert(idx as usize, data.to_string());
                                }
                            }
                        }
                        if let Some(meta_obj) = chunk.get("script_meta").and_then(|v| v.as_object()) {
                            for (k, v) in meta_obj {
                                script_meta.insert(k.clone(), v.clone());
                            }
                        }
                        if let Some(inst_obj) = chunk.get("instances").and_then(|v| v.as_object()) {
                            for (k, v) in inst_obj {
                                instances.insert(k.clone(), v.clone());
                            }
                        }
                        if let Some(light_obj) = chunk.get("lighting").and_then(|v| v.as_object()) {
                            for (k, v) in light_obj {
                                lighting.insert(k.clone(), v.clone());
                            }
                        }
                        let done = chunk.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
                        let next = chunk
                            .get("next_cursor")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(String::from);
                        if done || next.is_none() {
                            finished = true;
                            break;
                        }
                        if next.as_deref() == cursor.as_deref() {
                            return Err("mcp get_place_snapshot pagination cursor stalled".into());
                        }
                        cursor = next;
                    }
                    if !finished {
                        return Err("mcp get_place_snapshot exceeded max 512 pages without completing".into());
                    }
                    for (path, parts) in script_parts {
                        let combined: String = parts.into_values().collect();
                        scripts.insert(path, Value::String(combined));
                    }
                    return Ok(json!({
                        "snapshot_id": snapshot_id.unwrap_or_else(|| "snap-mcp".to_string()),
                        "scripts": Value::Object(scripts),
                        "script_meta": Value::Object(script_meta),
                        "instances": Value::Object(instances),
                        "lighting": Value::Object(lighting),
                    }));
                }
                let mut tree = c.get_place_tree()?;
                if let Some(obj) = tree.as_object_mut() {
                    obj.entry("snapshot_id").or_insert_with(|| json!("snap-mcp"));
                    obj.entry("lighting").or_insert_with(|| json!({}));
                    obj.entry("script_meta").or_insert_with(|| json!({}));
                }
                Ok(tree)
            }
            PlaceBackend::Local(p) => {
                let mut tree = p.tree_json();
                if let Some(obj) = tree.as_object_mut() {
                    obj.entry("snapshot_id")
                        .or_insert_with(|| json!(format!("snap-local-{}", p.revision())));
                    obj.entry("lighting").or_insert_with(|| json!({}));
                    obj.entry("script_meta").or_insert_with(|| json!({}));
                }
                Ok(tree)
            }
        }
    }

    pub fn get_place_tree_bytes(&mut self) -> Result<Vec<u8>, String> {
        match self {
            PlaceBackend::Local(p) => Ok(p.tree_bytes()),
            _ => {
                let tree = self.get_place_tree()?;
                serde_json::to_vec(&tree).map_err(|e| format!("serialize place_tree: {e}"))
            }
        }
    }

    pub fn mode_name(&self) -> &'static str {
        match self {
            PlaceBackend::Mcp(_) => "studio_mcp",
            PlaceBackend::Plugin(_) => "studio_plugin",
            PlaceBackend::Local(_) => "local_place",
        }
    }

    pub fn rbxlx_path(&self) -> Option<PathBuf> {
        match self {
            PlaceBackend::Mcp(_) | PlaceBackend::Plugin(_) => None,
            PlaceBackend::Local(p) => p.rbxlx_path(),
        }
    }

    pub fn local_revision(&self) -> Option<u64> {
        match self {
            PlaceBackend::Local(p) => Some(p.revision()),
            PlaceBackend::Mcp(_) | PlaceBackend::Plugin(_) => None,
        }
    }

    pub fn analyze_local_luau(&self) -> Option<AnalysisReport> {
        match self {
            PlaceBackend::Local(p) => Some(p.analyze_luau()),
            PlaceBackend::Mcp(_) | PlaceBackend::Plugin(_) => None,
        }
    }
}

/// Summary returned after importing `.rbxlx` XML into an existing `LocalPlace`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportSummary {
    pub game_name: String,
    pub scripts_imported: usize,
    pub instances_imported: usize,
}

/// Structural diff between two Place snapshots (`before` -> `after`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaceDiff {
    pub scripts_added: Vec<String>,
    pub scripts_modified: Vec<String>,
    pub scripts_removed: Vec<String>,
    pub instances_added: Vec<String>,
    pub instances_modified: Vec<String>,
    pub instances_removed: Vec<String>,
}

impl PlaceDiff {
    pub fn to_json(&self) -> Value {
        json!({
            "scripts_added": self.scripts_added,
            "scripts_modified": self.scripts_modified,
            "scripts_removed": self.scripts_removed,
            "instances_added": self.instances_added,
            "instances_modified": self.instances_modified,
            "instances_removed": self.instances_removed,
        })
    }
}

/// Result of creating a 3D Part preset via `LocalPlace::create_part_preset`.
#[derive(Clone, Debug)]
pub struct CreatedPreset {
    pub path: String,
    pub class_name: String,
    pub preset: String,
    pub instance: Value,
    pub companion_script: Option<String>,
    pub created_paths: Vec<String>,
}

impl CreatedPreset {
    pub fn to_json(&self) -> Value {
        json!({
            "ok": true,
            "path": self.path,
            "class_name": self.class_name,
            "preset": self.preset,
            "instance": self.instance,
            "companion_script": self.companion_script,
            "created_paths": self.created_paths,
        })
    }
}

/// Result of duplicating an instance (and any child instances/scripts) via `LocalPlace::duplicate_instance`.
#[derive(Clone, Debug)]
pub struct DuplicateResult {
    pub source_path: String,
    pub new_path: String,
    pub class_name: String,
    pub instance: Value,
    pub duplicated_instances: Vec<String>,
    pub duplicated_scripts: Vec<String>,
}

impl DuplicateResult {
    pub fn to_json(&self) -> Value {
        json!({
            "ok": true,
            "source_path": self.source_path,
            "new_path": self.new_path,
            "path": self.new_path,
            "class_name": self.class_name,
            "instance": self.instance,
            "duplicated_instances": self.duplicated_instances,
            "duplicated_scripts": self.duplicated_scripts,
        })
    }
}

/// Result of deleting an instance (and any child instances/scripts) via `LocalPlace::delete_instance`.
#[derive(Clone, Debug)]
pub struct DeleteResult {
    pub path: String,
    pub removed_instances: Vec<String>,
    pub removed_scripts: Vec<String>,
}

impl DeleteResult {
    pub fn to_json(&self) -> Value {
        json!({
            "ok": true,
            "path": self.path,
            "removed_instances": self.removed_instances,
            "removed_scripts": self.removed_scripts,
        })
    }
}

/// Result of live-editing a Luau script via `LocalPlace::update_script_source`.
#[derive(Clone, Debug)]
pub struct ScriptUpdateResult {
    pub path: String,
    pub created: bool,
    pub bytes: usize,
    pub lines: usize,
    pub script_report: AnalysisReport,
    pub place_report: AnalysisReport,
}

impl ScriptUpdateResult {
    pub fn to_json(&self) -> Value {
        json!({
            "ok": true,
            "path": self.path,
            "created": self.created,
            "bytes": self.bytes,
            "lines": self.lines,
            "analysis": self.script_report.to_json(),
            "place_analysis": self.place_report.to_json(),
            "error_count": self.place_report.error_count(),
            "warning_count": self.place_report.warning_count(),
        })
    }
}

/// Result of an Undo or Redo operation on `LocalPlace`.
#[derive(Clone, Debug)]
pub struct UndoRedoResult {
    pub action: &'static str,
    pub label: String,
    pub diff: PlaceDiff,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_depth: usize,
    pub redo_depth: usize,
    pub instance_count: usize,
    pub script_count: usize,
}

impl UndoRedoResult {
    pub fn to_json(&self) -> Value {
        json!({
            "ok": true,
            "action": self.action,
            "label": self.label,
            "diff": self.diff.to_json(),
            "can_undo": self.can_undo,
            "can_redo": self.can_redo,
            "undo_depth": self.undo_depth,
            "redo_depth": self.redo_depth,
            "instance_count": self.instance_count,
            "script_count": self.script_count,
        })
    }
}

#[derive(Clone, Debug)]
struct PlaceStateSnapshot {
    scripts: BTreeMap<String, String>,
    instances: BTreeMap<String, Value>,
    label: String,
}

#[derive(Clone, Debug)]
pub struct LocalPlace {
    scripts: BTreeMap<String, String>,
    instances: BTreeMap<String, Value>,
    last_console: String,
    data_dir: Option<PathBuf>,
    game_name: String,
    pub recovered: Vec<String>,
    undo_stack: VecDeque<PlaceStateSnapshot>,
    redo_stack: VecDeque<PlaceStateSnapshot>,
    revision: Arc<std::sync::atomic::AtomicU64>,
}

impl LocalPlace {
    /// Ephemeral in-memory place (for tests or non-persistent runs).
    pub fn in_memory(game_name: String) -> Self {
        LocalPlace {
            scripts: BTreeMap::new(),
            instances: BTreeMap::new(),
            last_console: String::new(),
            data_dir: None,
            game_name,
            recovered: Vec::new(),
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            revision: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Persistent place stored as `place.json` + `game.rbxlx` inside `data_dir`.
    pub fn open(data_dir: PathBuf, game_name: String) -> Self {
        let _ = std::fs::create_dir_all(&data_dir);
        let has_json = data_dir.join("place.json").is_file();
        let has_rbxlx = data_dir.join("game.rbxlx").is_file();
        let mut recovered = Vec::new();
        let (scripts, instances) = load_place(&data_dir, &mut recovered);
        let place = LocalPlace {
            scripts,
            instances,
            last_console: String::new(),
            data_dir: Some(data_dir),
            game_name,
            recovered,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            revision: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        };
        if !has_json || !has_rbxlx || !place.recovered.is_empty() {
            place.persist();
        }
        place
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn analyze_luau(&self) -> AnalysisReport {
        luau_check::analyze_place(&self.scripts, &self.instances)
    }

    /// Parse a Roblox Studio `.rbxlx` XML document back into an in-memory `Place` (`LocalPlace`).
    pub fn from_rbxlx_xml(xml: &str) -> Result<Place, String> {
        parse_rbxlx_xml(xml)
    }

    /// Serialize this Place into a genuine Roblox Studio `.rbxlx` (`<roblox version="4">`) document.
    pub fn to_rbxlx_xml(&self) -> String {
        serialize_rbxlx(&self.game_name, &self.tree_json())
    }

    /// Record the current Place state onto the bounded Undo stack and clear Redo.
    fn push_undo_snapshot(&mut self, label: impl Into<String>) {
        if self.undo_stack.len() >= MAX_HISTORY_DEPTH {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(PlaceStateSnapshot {
            scripts: self.scripts.clone(),
            instances: self.instances.clone(),
            label: label.into(),
        });
        self.redo_stack.clear();
    }

    /// Explicitly record a Redo entry (useful when an external server-level revert
    /// restores an earlier tree and wants `POST /api/redo` to step forward again).
    pub fn push_redo_state(
        &mut self,
        scripts: BTreeMap<String, String>,
        instances: BTreeMap<String, Value>,
        label: impl Into<String>,
    ) {
        if self.redo_stack.len() >= MAX_HISTORY_DEPTH {
            self.redo_stack.pop_front();
        }
        self.redo_stack.push_back(PlaceStateSnapshot {
            scripts,
            instances,
            label: label.into(),
        });
    }

    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Step backward to the previous state, pushing the current state onto the bounded Redo stack.
    pub fn undo(&mut self) -> Result<UndoRedoResult, String> {
        let prev = self
            .undo_stack
            .pop_back()
            .ok_or_else(|| "nothing to undo".to_string())?;
        let before = self.clone_state_only();
        if self.redo_stack.len() >= MAX_HISTORY_DEPTH {
            self.redo_stack.pop_front();
        }
        self.redo_stack.push_back(PlaceStateSnapshot {
            scripts: self.scripts.clone(),
            instances: self.instances.clone(),
            label: prev.label.clone(),
        });
        self.scripts = prev.scripts;
        self.instances = prev.instances;
        self.persist();
        let diff = self.diff_from(&before);
        Ok(UndoRedoResult {
            action: "undo",
            label: prev.label,
            diff,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
            undo_depth: self.undo_depth(),
            redo_depth: self.redo_depth(),
            instance_count: self.instances.len(),
            script_count: self.scripts.len(),
        })
    }

    /// Step forward to the most recently undone state, pushing the current state back onto Undo.
    pub fn redo(&mut self) -> Result<UndoRedoResult, String> {
        let next = self
            .redo_stack
            .pop_back()
            .ok_or_else(|| "nothing to redo".to_string())?;
        let before = self.clone_state_only();
        if self.undo_stack.len() >= MAX_HISTORY_DEPTH {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(PlaceStateSnapshot {
            scripts: self.scripts.clone(),
            instances: self.instances.clone(),
            label: next.label.clone(),
        });
        self.scripts = next.scripts;
        self.instances = next.instances;
        self.persist();
        let diff = self.diff_from(&before);
        Ok(UndoRedoResult {
            action: "redo",
            label: next.label,
            diff,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
            undo_depth: self.undo_depth(),
            redo_depth: self.redo_depth(),
            instance_count: self.instances.len(),
            script_count: self.scripts.len(),
        })
    }

    fn clone_state_only(&self) -> LocalPlace {
        LocalPlace {
            scripts: self.scripts.clone(),
            instances: self.instances.clone(),
            last_console: String::new(),
            data_dir: None,
            game_name: self.game_name.clone(),
            recovered: Vec::new(),
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            revision: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Create a ready-to-use 3D Part preset (`block`, `sphere`, `cylinder`, `spawn`,
    /// `jump_pad`, `kill_brick`, `card_billboard`) in `Workspace`, attaching companion
    /// Luau scripts or Decal/ImageLabel children automatically for interactive presets.
    pub fn create_part_preset(
        &mut self,
        preset: &str,
        name: Option<&str>,
        position: Option<[f64; 3]>,
        size: Option<[f64; 3]>,
        color: Option<&str>,
    ) -> Result<CreatedPreset, String> {
        let norm_preset = match preset.trim().to_ascii_lowercase().as_str() {
            "block" | "part" => "block",
            "sphere" | "ball" => "sphere",
            "cylinder" => "cylinder",
            "spawn" | "spawnlocation" | "spawn_pad" => "spawn",
            "jump_pad" | "jumppad" => "jump_pad",
            "kill_brick" | "killbrick" | "lava" => "kill_brick",
            "card_billboard" | "cardbillboard" | "billboard" | "decal_board" | "decalboard" => "card_billboard",
            other => {
                return Err(format!(
                    "unknown part preset '{other}' (expected block, sphere, cylinder, spawn, jump_pad, kill_brick, or card_billboard)"
                ));
            }
        };

        let (class_name, default_name, default_pos, default_size, default_color, material, shape) =
            match norm_preset {
                "block" => (
                    "Part",
                    "Block",
                    [0.0, 2.0, 0.0],
                    [8.0, 2.0, 8.0],
                    "#64748B",
                    "SmoothPlastic",
                    Some("Block"),
                ),
                "sphere" => (
                    "Part",
                    "Sphere",
                    [0.0, 4.0, 0.0],
                    [6.0, 6.0, 6.0],
                    "#38BDF8",
                    "SmoothPlastic",
                    Some("Ball"),
                ),
                "cylinder" => (
                    "Part",
                    "Cylinder",
                    [0.0, 4.0, 0.0],
                    [6.0, 6.0, 6.0],
                    "#F59E0B",
                    "Metal",
                    Some("Cylinder"),
                ),
                "spawn" => (
                    "SpawnLocation",
                    "SpawnPad",
                    [0.0, 1.0, 0.0],
                    [10.0, 1.0, 10.0],
                    "#4F7CF0",
                    "SmoothPlastic",
                    None,
                ),
                "jump_pad" => (
                    "Part",
                    "JumpPad",
                    [0.0, 1.0, 16.0],
                    [8.0, 1.0, 8.0],
                    "#3CB371",
                    "Neon",
                    Some("Block"),
                ),
                "kill_brick" => (
                    "Part",
                    "LavaBrick",
                    [0.0, 1.0, 24.0],
                    [10.0, 1.0, 10.0],
                    "#E0532A",
                    "Neon",
                    Some("Block"),
                ),
                "card_billboard" => (
                    "Part",
                    "CardBillboard",
                    [0.0, 5.0, 24.0],
                    [6.0, 8.5, 0.8],
                    "#A855F7",
                    "SmoothPlastic",
                    Some("Block"),
                ),
                _ => unreachable!(),
            };

        let requested_base = match name.map(str::trim).filter(|s| !s.is_empty()) {
            Some(n) => {
                if !valid_part_identifier(n) {
                    return Err(format!(
                        "part name '{n}' must start with a letter/underscore and contain only letters, digits, or underscores"
                    ));
                }
                n
            }
            None => default_name,
        };

        let pos = position.unwrap_or(default_pos);
        let sz = size.unwrap_or(default_size);
        if !pos.iter().all(|v| v.is_finite()) {
            return Err("position coordinates must be finite numbers".to_string());
        }
        if !sz.iter().all(|v| v.is_finite() && *v > 0.0) {
            return Err("size dimensions must be finite positive numbers".to_string());
        }
        let resolved_color = match color.map(str::trim).filter(|s| !s.is_empty()) {
            Some(hex) => normalize_hex_color(hex)?,
            None => default_color.to_string(),
        };

        // Pick a collision-free name under Workspace
        let mut resolved_name = requested_base.to_string();
        let mut suffix = 2usize;
        while self
            .instances
            .contains_key(&format!("Workspace.{resolved_name}"))
        {
            resolved_name = format!("{requested_base}_{suffix}");
            suffix += 1;
        }
        let path = format!("Workspace.{resolved_name}");

        let mut props = Map::new();
        props.insert("Anchored".to_string(), json!(true));
        props.insert("CanCollide".to_string(), json!(true));
        props.insert("Position".to_string(), json!([pos[0], pos[1], pos[2]]));
        if norm_preset == "cylinder" {
            // Roblox PartType.Cylinder runs along local X; upright orientation maps
            // requested (diameter_x, height_y, diameter_z) -> Size=(height_y, diameter_x, diameter_z) + Z+90 deg.
            props.insert("Size".to_string(), json!([sz[1], sz[0], sz[2]]));
            props.insert("Rotation".to_string(), json!([0.0, 0.0, 90.0]));
        } else {
            props.insert("Size".to_string(), json!([sz[0], sz[1], sz[2]]));
        }
        props.insert("Color".to_string(), json!(resolved_color));
        props.insert("Material".to_string(), json!(material));
        props.insert("Transparency".to_string(), json!(0.0));
        if let Some(sh) = shape {
            props.insert("Shape".to_string(), json!(sh));
        }
        if class_name == "SpawnLocation" {
            props.insert("Neutral".to_string(), json!(true));
        }

        // Pre-compile any companion script before mutating state so errors leave place untouched
        let companion: Option<(String, String)> = match norm_preset {
            "jump_pad" => {
                let src = skills::generate(
                    "jump_pad",
                    &json!({
                        "boost_power": 125,
                        "cooldown_ms": 600,
                        "pad_name": resolved_name,
                        "create_pad": false
                    }),
                )?;
                Some((format!("ServerScriptService.{resolved_name}Controller"), src))
            }
            "kill_brick" => {
                let src = skills::generate("kill_brick", &json!({ "damage": 100 }))?;
                Some((format!("{path}.KillBrick"), src))
            }
            _ => None,
        };

        self.push_undo_snapshot(format!("Create {norm_preset} {path}"));

        let record = json!({
            "class_name": class_name,
            "properties": Value::Object(props),
        });
        self.instances.insert(path.clone(), record.clone());
        let mut created_paths = vec![path.clone()];

        if norm_preset == "card_billboard" {
            let decal_path = format!("{path}.CardArtDecal");
            let gui_path = format!("{path}.CardDisplayGui");
            let img_path = format!("{gui_path}.CardArtImage");
            self.instances.insert(
                decal_path.clone(),
                json!({
                    "class_name": "Decal",
                    "properties": {
                        "Texture": "rbxasset://openro-assets/supernova-ascended-striker.png",
                        "Face": "Front"
                    }
                }),
            );
            self.instances.insert(
                gui_path.clone(),
                json!({
                    "class_name": "SurfaceGui",
                    "properties": { "Face": "Front" }
                }),
            );
            self.instances.insert(
                img_path.clone(),
                json!({
                    "class_name": "ImageLabel",
                    "properties": {
                        "Image": "rbxasset://openro-assets/supernova-ascended-striker.png",
                        "BackgroundTransparency": 1.0
                    }
                }),
            );
            created_paths.push(decal_path);
            created_paths.push(gui_path);
            created_paths.push(img_path);
        }

        let companion_script = if let Some((sc_path, sc_src)) = companion {
            self.scripts.insert(sc_path.clone(), sc_src);
            created_paths.push(sc_path.clone());
            Some(sc_path)
        } else {
            None
        };

        self.persist();
        Ok(CreatedPreset {
            path,
            class_name: class_name.to_string(),
            preset: norm_preset.to_string(),
            instance: record,
            companion_script,
            created_paths,
        })
    }

    /// Duplicate an existing instance (along with all descendant child instances and
    /// child scripts), offsetting the duplicated root part's Position by `[4, 0, 4]`.
    pub fn duplicate_instance(&mut self, path: &str) -> Result<DuplicateResult, String> {
        let orig = self
            .instances
            .get(path)
            .cloned()
            .ok_or_else(|| format!("no instance at {path}"))?;
        let (parent, leaf) = path
            .rsplit_once('.')
            .ok_or_else(|| format!("invalid instance path '{path}'"))?;

        let mut new_leaf = format!("{leaf}_Copy");
        let mut suffix = 2usize;
        while self
            .instances
            .contains_key(&format!("{parent}.{new_leaf}"))
        {
            new_leaf = format!("{leaf}_Copy{suffix}");
            suffix += 1;
        }
        let new_path = format!("{parent}.{new_leaf}");

        let mut cloned_root = orig;
        if let Some(props) = cloned_root
            .get_mut("properties")
            .and_then(|v| v.as_object_mut())
        {
            if let Some((x, y, z)) = parse_vec3(props.get("Position")) {
                props.insert("Position".to_string(), json!([x + 4.0, y, z + 4.0]));
            }
            if let Some(arr) = props.get_mut("CFrame").and_then(|v| v.as_array_mut()) {
                if arr.len() == 12 {
                    if let (Some(x), Some(z)) = (arr[0].as_f64(), arr[2].as_f64()) {
                        arr[0] = json!(x + 4.0);
                        arr[2] = json!(z + 4.0);
                    }
                }
            }
        }

        let child_prefix = format!("{path}.");
        let child_instances: Vec<(String, Value)> = self
            .instances
            .iter()
            .filter(|(k, _)| k.starts_with(&child_prefix))
            .map(|(k, v)| (format!("{new_path}.{}", &k[child_prefix.len()..]), v.clone()))
            .collect();
        let child_scripts: Vec<(String, String)> = self
            .scripts
            .iter()
            .filter(|(k, _)| k.starts_with(&child_prefix))
            .map(|(k, v)| (format!("{new_path}.{}", &k[child_prefix.len()..]), v.clone()))
            .collect();

        self.push_undo_snapshot(format!("Duplicate {path} -> {new_path}"));

        let class_name = cloned_root
            .get("class_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Part")
            .to_string();
        self.instances.insert(new_path.clone(), cloned_root.clone());

        let mut duplicated_instances = vec![new_path.clone()];
        for (ck, cv) in child_instances {
            self.instances.insert(ck.clone(), cv);
            duplicated_instances.push(ck);
        }
        let mut duplicated_scripts = Vec::new();
        for (sk, sv) in child_scripts {
            self.scripts.insert(sk.clone(), sv);
            duplicated_scripts.push(sk);
        }

        self.persist();
        Ok(DuplicateResult {
            source_path: path.to_string(),
            new_path,
            class_name,
            instance: cloned_root,
            duplicated_instances,
            duplicated_scripts,
        })
    }

    /// Delete an instance and all of its descendant child instances and child scripts.
    pub fn delete_instance(&mut self, path: &str) -> Result<DeleteResult, String> {
        if !self.instances.contains_key(path) {
            return Err(format!("no instance at {path}"));
        }
        let child_prefix = format!("{path}.");
        let inst_keys: Vec<String> = self
            .instances
            .keys()
            .filter(|k| k.as_str() == path || k.starts_with(&child_prefix))
            .cloned()
            .collect();
        let script_keys: Vec<String> = self
            .scripts
            .keys()
            .filter(|k| k.starts_with(&child_prefix))
            .cloned()
            .collect();

        self.push_undo_snapshot(format!("Delete {path}"));

        for k in &inst_keys {
            self.instances.remove(k);
        }
        for k in &script_keys {
            self.scripts.remove(k);
        }

        self.persist();
        Ok(DeleteResult {
            path: path.to_string(),
            removed_instances: inst_keys,
            removed_scripts: script_keys,
        })
    }

    /// Save live-edited Luau source for a script and return immediate Static Luau Prover
    /// diagnostics (`script_report` + full cross-referenced `place_report`).
    pub fn update_script_source(
        &mut self,
        path: &str,
        source: &str,
    ) -> Result<ScriptUpdateResult, String> {
        validate_script_path(path)?;
        if source.len() > MAX_SCRIPT_BYTES {
            return Err(format!(
                "script source exceeds maximum allowed size ({MAX_SCRIPT_BYTES} bytes)"
            ));
        }
        if source.contains('\0') {
            return Err("script source must not contain NUL bytes".to_string());
        }

        let created = !self.scripts.contains_key(path);
        self.push_undo_snapshot(format!(
            "{} {path}",
            if created { "Create script" } else { "Edit script" }
        ));
        self.scripts.insert(path.to_string(), source.to_string());
        self.persist();

        let script_report = luau_check::analyze_script(path, source);
        let place_report = self.analyze();
        let lines = if source.is_empty() {
            0
        } else {
            source.lines().count()
        };

        Ok(ScriptUpdateResult {
            path: path.to_string(),
            created,
            bytes: source.len(),
            lines,
            script_report,
            place_report,
        })
    }

    /// Replace this Place's contents directly from a normalized JSON tree
    /// `{"instances": {...}, "scripts": {...}}` with a single atomic disk persist.
    pub fn replace_tree(&mut self, tree: &Value) {
        self.push_undo_snapshot("Replace place tree");
        self.scripts.clear();
        self.instances.clear();
        if let Some(sc_map) = tree.get("scripts").and_then(|v| v.as_object()) {
            for (k, v) in sc_map {
                if let Some(src) = v.as_str() {
                    self.scripts.insert(k.clone(), src.to_string());
                }
            }
        }
        if let Some(inst_map) = tree.get("instances").and_then(|v| v.as_object()) {
            for (k, v) in inst_map {
                if v.is_object() {
                    self.instances.insert(k.clone(), v.clone());
                }
            }
        }
        self.persist();
    }

    /// Replace this Place's contents from an imported `.rbxlx` XML document and persist atomically.
    pub fn import_rbxlx_xml(&mut self, xml: &str) -> Result<ImportSummary, String> {
        let parsed = parse_rbxlx_xml(xml)?;
        self.push_undo_snapshot(format!("Import {}", parsed.game_name));
        self.scripts = parsed.scripts;
        self.instances = parsed.instances;
        if !parsed.game_name.is_empty() && parsed.game_name != "Imported Place" {
            self.game_name = parsed.game_name.clone();
        }
        self.persist();
        Ok(ImportSummary {
            game_name: self.game_name.clone(),
            scripts_imported: self.scripts.len(),
            instances_imported: self.instances.len(),
        })
    }

    /// Merge property updates onto an existing instance (e.g. from `POST /api/place/instance`)
    /// and persist atomically.
    pub fn update_instance_properties(
        &mut self,
        path: &str,
        patch: Value,
    ) -> Result<Value, String> {
        let patch_obj = patch
            .as_object()
            .ok_or_else(|| "instance property patch must be a JSON object".to_string())?;
        if !self.instances.contains_key(path) {
            return Err(format!("no instance at {path}"));
        }
        self.push_undo_snapshot(format!("Update {path}"));
        let rec = self
            .instances
            .get_mut(path)
            .ok_or_else(|| format!("no instance at {path}"))?;
        let obj = rec
            .as_object_mut()
            .ok_or_else(|| format!("corrupt instance record at {path}"))?;
        let props_entry = obj
            .entry("properties".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !props_entry.is_object() {
            *props_entry = Value::Object(Map::new());
        }
        if let Some(props_map) = props_entry.as_object_mut() {
            for (k, v) in patch_obj {
                props_map.insert(k.clone(), v.clone());
            }
        }
        let updated = rec.clone();
        self.persist();
        Ok(updated)
    }

    /// Run the full Static Luau Analyzer & Cross-Reference Prover against this Place.
    pub fn analyze(&self) -> AnalysisReport {
        luau_check::analyze_place(&self.scripts, &self.instances)
    }

    /// Compute the exact instance & script diff from `before` to `self`.
    pub fn diff_from(&self, before: &LocalPlace) -> PlaceDiff {
        let mut diff = PlaceDiff::default();
        for (k, v) in &self.scripts {
            match before.scripts.get(k) {
                None => diff.scripts_added.push(k.clone()),
                Some(old) if old != v => diff.scripts_modified.push(k.clone()),
                _ => {}
            }
        }
        for k in before.scripts.keys() {
            if !self.scripts.contains_key(k) {
                diff.scripts_removed.push(k.clone());
            }
        }
        for (k, v) in &self.instances {
            match before.instances.get(k) {
                None => diff.instances_added.push(k.clone()),
                Some(old) if old != v => diff.instances_modified.push(k.clone()),
                _ => {}
            }
        }
        for k in before.instances.keys() {
            if !self.instances.contains_key(k) {
                diff.instances_removed.push(k.clone());
            }
        }
        diff
    }

    pub fn scripts(&self) -> &BTreeMap<String, String> {
        &self.scripts
    }

    pub fn instances(&self) -> &BTreeMap<String, Value> {
        &self.instances
    }

    pub fn game_name(&self) -> &str {
        &self.game_name
    }

    pub fn set_game_name(&mut self, name: String) {
        self.game_name = name;
        self.persist();
    }

    pub fn rbxlx_path(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().map(|d| d.join("game.rbxlx"))
    }

    pub fn tree_json(&self) -> Value {
        json!({
            "scripts": self.scripts,
            "instances": self.instances,
        })
    }

    pub fn tree_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(64 * 1024);
        buf.extend_from_slice(br#"{"scripts":"#);
        let _ = serde_json::to_writer(&mut buf, &self.scripts);
        buf.extend_from_slice(br#","instances":"#);
        let _ = serde_json::to_writer(&mut buf, &self.instances);
        buf.push(b'}');
        buf
    }

    fn persist(&self) {
        self.revision.fetch_add(1, std::sync::atomic::Ordering::Release);
        let Some(dir) = &self.data_dir else { return };
        let _ = std::fs::create_dir_all(dir);
        let tree = self.tree_json();
        let json_body =
            serde_json::to_string_pretty(&tree).unwrap_or_else(|_| "{}".to_string());
        let json_path = dir.join("place.json");
        let json_tmp = dir.join("place.json.tmp");
        if std::fs::write(&json_tmp, json_body).is_ok() {
            let _ = std::fs::rename(&json_tmp, &json_path);
        }
        let xml = serialize_rbxlx(&self.game_name, &tree);
        let xml_path = dir.join("game.rbxlx");
        let xml_tmp = dir.join("game.rbxlx.tmp");
        if std::fs::write(&xml_tmp, xml).is_ok() {
            let _ = std::fs::rename(&xml_tmp, &xml_path);
        }
    }

    pub fn call_tool(&mut self, name: &str, args: Value) -> Result<String, String> {
        match name {
            "get_place_tree" => Ok(self.tree_json().to_string()),
            "write_script" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "write_script requires string path".to_string())?;
                let source = args
                    .get("source")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "write_script requires string source".to_string())?;
                self.push_undo_snapshot(format!("Write script {path}"));
                self.scripts.insert(path.to_string(), source.to_string());
                self.persist();
                Ok(json!({ "ok": true, "path": path }).to_string())
            }
            "delete_script" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "delete_script requires string path".to_string())?;
                self.push_undo_snapshot(format!("Delete script {path}"));
                self.scripts.remove(path);
                self.persist();
                Ok(json!({ "ok": true, "path": path }).to_string())
            }
            "create_instance" => {
                let class_name = args
                    .get("class_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "create_instance requires string class_name".to_string())?;
                let inst_name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "create_instance requires string name".to_string())?;
                let parent = args
                    .get("parent")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "create_instance requires string parent".to_string())?;
                let path = format!("{parent}.{inst_name}");
                let properties = args.get("properties").cloned().unwrap_or_else(|| json!({}));
                self.push_undo_snapshot(format!("Create {class_name} {path}"));
                self.instances.insert(
                    path.clone(),
                    json!({ "class_name": class_name, "properties": properties }),
                );
                self.persist();
                Ok(json!({ "ok": true, "path": path }).to_string())
            }
            "delete_instance" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "delete_instance requires string path".to_string())?;
                self.push_undo_snapshot(format!("Delete instance {path}"));
                self.instances.remove(path);
                self.persist();
                Ok(json!({ "ok": true, "path": path }).to_string())
            }
            "playtest" => {
                let report = validate_place(&self.scripts, &self.instances);
                if report.errors.is_empty() {
                    let warn_suffix = if report.warnings.is_empty() {
                        String::new()
                    } else {
                        format!(", {} warning(s)", report.warnings.len())
                    };
                    self.last_console = format!(
                        "[OpenRO Playtest] Verified {} Luau script(s) and {} Workspace instance(s) — syntax OK, Roblox service roots OK, instance & RemoteEvent wiring OK (0 errors{warn_suffix}).",
                        self.scripts.len(),
                        self.instances.len()
                    );
                    Ok(json!({ "status": "passed", "console": self.last_console }).to_string())
                } else {
                    self.last_console = report.errors.join("\n");
                    Ok(json!({ "status": "failed", "console": self.last_console }).to_string())
                }
            }
            "get_console" => Ok(self.last_console.clone()),
            other => Err(format!("unknown local place tool: {other}")),
        }
    }
}

fn valid_part_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    s.len() <= 64 && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn validate_script_path(path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("script path must not be empty".to_string());
    }
    if path.chars().any(|c| c.is_control()) {
        return Err("script path must not contain control characters".to_string());
    }
    let segs: Vec<&str> = path.split('.').collect();
    if segs.len() < 2 || segs.iter().any(|s| s.is_empty() || *s == "..") {
        return Err(format!(
            "invalid script path '{path}' (expected <Service>.<Name>)"
        ));
    }
    if !VALID_SERVICES.contains(&segs[0]) {
        return Err(format!(
            "invalid Roblox service root '{}' (expected one of {})",
            segs[0],
            VALID_SERVICES.join(", ")
        ));
    }
    Ok(())
}

fn normalize_hex_color(hex: &str) -> Result<String, String> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() != 6 || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("color '{hex}' must be a 6-digit hex color like #4F7CF0"));
    }
    Ok(format!("#{}", clean.to_ascii_uppercase()))
}

fn shape_to_token(val: Option<&Value>) -> Option<u32> {
    let v = val?;
    if let Some(n) = v.as_u64() {
        return Some(n as u32);
    }
    let s = v.as_str()?;
    let clean = s.trim().strip_prefix("Enum.PartType.").unwrap_or(s.trim());
    match clean.to_ascii_lowercase().as_str() {
        "ball" | "sphere" => Some(0),
        "block" => Some(1),
        "cylinder" => Some(2),
        "wedge" => Some(3),
        "cornerwedge" => Some(4),
        _ => None,
    }
}

fn token_to_shape(tok: u32) -> Option<&'static str> {
    match tok {
        0 => Some("Ball"),
        1 => Some("Block"),
        2 => Some("Cylinder"),
        3 => Some("Wedge"),
        4 => Some("CornerWedge"),
        _ => None,
    }
}

fn load_place(
    dir: &Path,
    recovered: &mut Vec<String>,
) -> (BTreeMap<String, String>, BTreeMap<String, Value>) {
    let path = dir.join("place.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (BTreeMap::new(), BTreeMap::new());
    };
    let parsed = (|| -> Option<(BTreeMap<String, String>, BTreeMap<String, Value>)> {
        let v: Value = serde_json::from_str(&text).ok()?;
        let mut scripts = BTreeMap::new();
        if let Some(obj) = v.get("scripts")?.as_object() {
            for (k, val) in obj {
                scripts.insert(k.clone(), val.as_str()?.to_string());
            }
        } else {
            return None;
        }
        let mut instances = BTreeMap::new();
        if let Some(obj) = v.get("instances")?.as_object() {
            for (k, val) in obj {
                if !val.is_object() {
                    return None;
                }
                instances.insert(k.clone(), val.clone());
            }
        } else {
            return None;
        }
        Some((scripts, instances))
    })();
    match parsed {
        Some(pair) => pair,
        None => {
            if let Some(bak) = crate::server::backup_corrupt(&path) {
                recovered.push(
                    bak.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            (BTreeMap::new(), BTreeMap::new())
        }
    }
}

pub struct ValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Static Luau syntax, security, Roblox service-root, and cross-script runtime
/// wiring verifier used by the Local Place Engine playtest.
pub fn validate_place(
    scripts: &BTreeMap<String, String>,
    instances: &BTreeMap<String, Value>,
) -> ValidationReport {
    let report = luau_check::analyze_place(scripts, instances);
    ValidationReport {
        errors: report.errors(),
        warnings: report.warnings(),
        diagnostics: report.diagnostics,
    }
}

/// Backwards-compatible re-export of the static Luau syntax checker.
pub fn check_luau_syntax(source: &str) -> Result<(), String> {
    luau_check::check_luau_syntax(source)
}

// ---------------------------------------------------------------------------
// Roblox Enum.Material bidirectional token table
// ---------------------------------------------------------------------------

const MATERIAL_TOKENS: [(&str, u32); 23] = [
    ("Plastic", 256),
    ("SmoothPlastic", 272),
    ("Neon", 288),
    ("Wood", 512),
    ("WoodPlanks", 528),
    ("Marble", 784),
    ("Slate", 800),
    ("Concrete", 816),
    ("Granite", 832),
    ("Brick", 848),
    ("Pebble", 864),
    ("Cobblestone", 880),
    ("Rock", 896),
    ("Metal", 1040),
    ("CorrodedMetal", 1056),
    ("DiamondPlate", 1072),
    ("Foil", 1088),
    ("Grass", 1280),
    ("Sand", 1296),
    ("Fabric", 1312),
    ("Ice", 1536),
    ("Glass", 1568),
    ("ForceField", 1584),
];

fn material_to_token(val: Option<&Value>, class_name: &str, name: &str) -> u32 {
    if let Some(v) = val {
        if let Some(n) = v.as_u64() {
            return n as u32;
        }
        if let Some(s) = v.as_str() {
            let clean = s.trim().strip_prefix("Enum.Material.").unwrap_or(s.trim());
            for (mat_name, tok) in MATERIAL_TOKENS {
                if mat_name.eq_ignore_ascii_case(clean) {
                    return tok;
                }
            }
        }
    }
    let lower = name.to_ascii_lowercase();
    if lower.contains("lava")
        || lower.contains("kill")
        || lower.contains("jump")
        || lower.contains("boost")
        || lower.contains("portal")
        || lower.contains("neon")
    {
        288 // Neon
    } else if lower.contains("ice") || lower.contains("frost") {
        1536 // Ice
    } else if lower.contains("glass") {
        1568 // Glass
    } else if lower.contains("metal") || lower.contains("conveyor") || lower.contains("dropper") {
        1040 // Metal
    } else if lower.contains("grass") {
        1280 // Grass
    } else {
        let _ = class_name;
        272 // SmoothPlastic
    }
}

fn token_to_material(tok: u32) -> Option<&'static str> {
    MATERIAL_TOKENS
        .iter()
        .find(|(_, id)| *id == tok)
        .map(|(name, _)| *name)
}

fn face_to_token(val: Option<&Value>) -> u32 {
    if let Some(v) = val {
        if let Some(n) = v.as_u64() {
            return n as u32;
        }
        if let Some(s) = v.as_str() {
            let clean = s.trim().strip_prefix("Enum.NormalId.").unwrap_or(s.trim());
            return match clean.to_ascii_lowercase().as_str() {
                "right" => 0,
                "top" => 1,
                "back" => 2,
                "left" => 3,
                "bottom" => 4,
                "front" => 5,
                _ => 5,
            };
        }
    }
    5 // Front
}

fn token_to_face(tok: u32) -> Option<&'static str> {
    match tok {
        0 => Some("Right"),
        1 => Some("Top"),
        2 => Some("Back"),
        3 => Some("Left"),
        4 => Some("Bottom"),
        5 => Some("Front"),
        _ => None,
    }
}

fn is_content_property(key: &str) -> bool {
    matches!(
        key,
        "MeshId"
            | "TextureID"
            | "TextureId"
            | "SoundId"
            | "Texture"
            | "ColorMap"
            | "MetalnessMap"
            | "NormalMap"
            | "RoughnessMap"
            | "Image"
            | "AnimationId"
            | "SkyboxBk"
            | "SkyboxDn"
            | "SkyboxFt"
            | "SkyboxLf"
            | "SkyboxRt"
            | "SkyboxUp"
            | "MoonTextureId"
            | "SunTextureId"
            | "ShirtTemplate"
            | "PantsTemplate"
            | "Graphic"
            | "OverlayTextureId"
    )
}

fn is_base_part_class(class_name: &str) -> bool {
    matches!(
        class_name,
        "Part"
            | "SpawnLocation"
            | "WedgePart"
            | "TrussPart"
            | "CornerWedgePart"
            | "Seat"
            | "VehicleSeat"
            | "MeshPart"
    )
}

// ---------------------------------------------------------------------------
// Authentic Roblox Studio `.rbxlx` (`<roblox version="4">`) XML Serializer
// ---------------------------------------------------------------------------

/// Escape text content for XML elements.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// Wrap text in CDATA safely (splitting any embedded `]]>`).
fn xml_cdata(s: &str) -> String {
    format!("<![CDATA[{}]]>", s.replace("]]>", "]]]]><![CDATA[>"))
}

fn parse_vec3(v: Option<&Value>) -> Option<(f64, f64, f64)> {
    let v = v?;
    if let Some(arr) = v.as_array() {
        if arr.len() >= 3 {
            return Some((arr[0].as_f64()?, arr[1].as_f64()?, arr[2].as_f64()?));
        }
    }
    if let Some(obj) = v.as_object() {
        let x = obj
            .get("x")
            .or_else(|| obj.get("X"))
            .and_then(|n| n.as_f64())?;
        let y = obj
            .get("y")
            .or_else(|| obj.get("Y"))
            .and_then(|n| n.as_f64())?;
        let z = obj
            .get("z")
            .or_else(|| obj.get("Z"))
            .and_then(|n| n.as_f64())?;
        return Some((x, y, z));
    }
    None
}

fn parse_cframe_12(props: &Value, default_pos: (f64, f64, f64)) -> [f64; 12] {
    if let Some(arr) = props.get("CFrame").and_then(|v| v.as_array()) {
        if arr.len() == 12 && arr.iter().all(|n| n.as_f64().is_some()) {
            let mut out = [0.0; 12];
            for (i, v) in arr.iter().enumerate() {
                out[i] = v.as_f64().unwrap_or(0.0);
            }
            return out;
        }
    }
    let (px, py, pz) = parse_vec3(props.get("Position")).unwrap_or(default_pos);
    if let Some((rx_deg, ry_deg, rz_deg)) = parse_vec3(props.get("Rotation")) {
        let (rx, ry, rz) = (
            rx_deg.to_radians(),
            ry_deg.to_radians(),
            rz_deg.to_radians(),
        );
        let (sx, cx) = rx.sin_cos();
        let (sy, cy) = ry.sin_cos();
        let (sz, cz) = rz.sin_cos();
        // R = Rz * Ry * Rx (standard Euler rotation matrix)
        let r00 = cy * cz;
        let r01 = cz * sx * sy - cx * sz;
        let r02 = cx * cz * sy + sx * sz;
        let r10 = cy * sz;
        let r11 = cx * cz + sx * sy * sz;
        let r12 = cx * sy * sz - cz * sx;
        let r20 = -sy;
        let r21 = cy * sx;
        let r22 = cx * cy;
        return [px, py, pz, r00, r01, r02, r10, r11, r12, r20, r21, r22];
    }
    [px, py, pz, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
}

fn pick_color_u32(class_name: &str, name: &str, props: &Value) -> u32 {
    if let Some(n) = props.get("Color3uint8").and_then(|v| v.as_u64()) {
        return 0xFF00_0000 | (n as u32 & 0x00FF_FFFF);
    }
    if let Some(hex) = props.get("Color").and_then(|v| v.as_str()) {
        let clean = hex.trim().trim_start_matches('#');
        if clean.len() == 6 {
            if let Ok(rgb) = u32::from_str_radix(clean, 16) {
                return 0xFF00_0000 | rgb;
            }
        }
    }
    if let Some(arr) = props.get("Color").and_then(|v| v.as_array()) {
        if arr.len() >= 3 {
            if let (Some(r), Some(g), Some(b)) =
                (arr[0].as_f64(), arr[1].as_f64(), arr[2].as_f64())
            {
                let (ri, gi, bi) = if r <= 1.0 && g <= 1.0 && b <= 1.0 {
                    (
                        (r.clamp(0.0, 1.0) * 255.0).round() as u32,
                        (g.clamp(0.0, 1.0) * 255.0).round() as u32,
                        (b.clamp(0.0, 1.0) * 255.0).round() as u32,
                    )
                } else {
                    (
                        r.clamp(0.0, 255.0).round() as u32,
                        g.clamp(0.0, 255.0).round() as u32,
                        b.clamp(0.0, 255.0).round() as u32,
                    )
                };
                return 0xFF00_0000 | (ri << 16) | (gi << 8) | bi;
            }
        }
    }
    let lower = name.to_ascii_lowercase();
    if lower.contains("lava") || lower.contains("kill") {
        0xFF_E0532A
    } else if lower.contains("ice") || lower.contains("frost") {
        0xFF_67B7E8
    } else if lower.contains("jump") || lower.contains("boost") {
        0xFF_3CB371
    } else if lower.contains("portal") || lower.contains("warp") {
        0xFF_A855F7
    } else if lower.contains("coin") || lower.contains("gold") || lower.contains("collector") {
        0xFF_E6B830
    } else if class_name == "SpawnLocation" || lower.contains("spawn") {
        0xFF_4F7CF0
    } else {
        0xFF_A3A2A5
    }
}

fn color_u32_to_hex(color: u32) -> String {
    let rgb = color & 0x00FF_FFFF;
    format!("#{rgb:06X}")
}

fn fmt_num(v: f64) -> String {
    if v.abs() < 1e-12 {
        "0".to_string()
    } else if (v - v.round()).abs() < 1e-9 && v.abs() < 1e12 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v}")
    }
}

/// Tree node used to serialize arbitrarily nested Place hierarchies into authentic `.rbxlx`.
struct ExportNode {
    name: String,
    class_name: String,
    properties: Value,
    script_source: Option<String>,
    children: BTreeMap<String, ExportNode>,
}

impl ExportNode {
    fn container(name: &str, class_name: &str) -> Self {
        ExportNode {
            name: name.to_string(),
            class_name: class_name.to_string(),
            properties: json!({}),
            script_source: None,
            children: BTreeMap::new(),
        }
    }
}

fn default_script_class(service: &str) -> &'static str {
    match service {
        "StarterGui" | "StarterPlayer" => "LocalScript",
        _ => "Script",
    }
}

struct XmlGen {
    next_ref: usize,
    part_counter: usize,
    out: String,
}

impl XmlGen {
    fn new(capacity_hint: usize) -> Self {
        XmlGen {
            next_ref: 1,
            part_counter: 0,
            out: String::with_capacity(capacity_hint.max(4096)),
        }
    }

    fn alloc_ref(&mut self) -> String {
        let id = self.next_ref;
        self.next_ref += 1;
        format!("RBX{id:08X}")
    }

    fn write_indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.out.push_str("  ");
        }
    }

    fn write_extra_properties(&mut self, depth: usize, props: &Value, skip_keys: &[&str]) {
        let Some(map) = props.as_object() else { return };
        for (k, v) in map {
            if skip_keys.contains(&k.as_str()) {
                continue;
            }
            let ek = xml_escape(k);
            match v {
                Value::Bool(b) => {
                    self.write_indent(depth);
                    let _ = writeln!(self.out, "<bool name=\"{ek}\">{b}</bool>");
                }
                Value::Number(n) => {
                    self.write_indent(depth);
                    if let Some(i) = n.as_i64() {
                        let _ = writeln!(self.out, "<int name=\"{ek}\">{i}</int>");
                    } else if let Some(f) = n.as_f64() {
                        let _ = writeln!(self.out, "<float name=\"{ek}\">{}</float>", fmt_num(f));
                    }
                }
                Value::String(s) => {
                    self.write_indent(depth);
                    if is_content_property(k) {
                        if s.is_empty() {
                            let _ = writeln!(self.out, "<Content name=\"{ek}\"><null></null></Content>");
                        } else {
                            let _ = writeln!(
                                self.out,
                                "<Content name=\"{ek}\"><url>{}</url></Content>",
                                xml_escape(s)
                            );
                        }
                    } else {
                        let _ = writeln!(
                            self.out,
                            "<string name=\"{ek}\">{}</string>",
                            xml_escape(s)
                        );
                    }
                }
                Value::Array(arr) if arr.len() == 3 => {
                    if let (Some(x), Some(y), Some(z)) =
                        (arr[0].as_f64(), arr[1].as_f64(), arr[2].as_f64())
                    {
                        self.write_indent(depth);
                        let _ = writeln!(
                            self.out,
                            "<Vector3 name=\"{ek}\"><X>{}</X><Y>{}</Y><Z>{}</Z></Vector3>",
                            fmt_num(x),
                            fmt_num(y),
                            fmt_num(z)
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn write_node(&mut self, node: &ExportNode, depth: usize) {
        let r = self.alloc_ref();
        self.write_indent(depth);
        let _ = writeln!(
            self.out,
            "<Item class=\"{}\" referent=\"{r}\">",
            xml_escape(&node.class_name)
        );
        self.write_indent(depth + 1);
        self.out.push_str("<Properties>\n");
        self.write_indent(depth + 2);
        let _ = writeln!(
            self.out,
            "<string name=\"Name\">{}</string>",
            xml_escape(&node.name)
        );

        if let Some(source) = &node.script_source {
            let disabled = node
                .properties
                .get("Disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            self.write_indent(depth + 2);
            let _ = writeln!(self.out, "<bool name=\"Disabled\">{disabled}</bool>");
            self.write_indent(depth + 2);
            let _ = writeln!(
                self.out,
                "<ProtectedString name=\"Source\">{}</ProtectedString>",
                xml_cdata(source)
            );
        } else if is_base_part_class(&node.class_name) {
            let props = &node.properties;
            let anchored = props
                .get("Anchored")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let can_collide = props
                .get("CanCollide")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let transparency = props
                .get("Transparency")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let (sx, sy, sz) = parse_vec3(props.get("Size").or_else(|| props.get("size")))
                .unwrap_or(if node.class_name == "SpawnLocation" {
                    (8.0, 1.0, 8.0)
                } else {
                    (10.0, 1.0, 10.0)
                });
            let idx = self.part_counter;
            self.part_counter += 1;
            let cf = parse_cframe_12(props, (0.0, 1.0, (idx as f64) * 16.0));
            let color_u32 = pick_color_u32(&node.class_name, &node.name, props);
            let material_tok = material_to_token(props.get("Material"), &node.class_name, &node.name);

            self.write_indent(depth + 2);
            let _ = writeln!(self.out, "<bool name=\"Anchored\">{anchored}</bool>");
            self.write_indent(depth + 2);
            let _ = writeln!(self.out, "<bool name=\"CanCollide\">{can_collide}</bool>");
            self.write_indent(depth + 2);
            self.out.push_str("<CoordinateFrame name=\"CFrame\">\n");
            self.write_indent(depth + 3);
            let _ = writeln!(
                self.out,
                "<X>{}</X><Y>{}</Y><Z>{}</Z>",
                fmt_num(cf[0]),
                fmt_num(cf[1]),
                fmt_num(cf[2])
            );
            self.write_indent(depth + 3);
            let _ = writeln!(
                self.out,
                "<R00>{}</R00><R01>{}</R01><R02>{}</R02>",
                fmt_num(cf[3]),
                fmt_num(cf[4]),
                fmt_num(cf[5])
            );
            self.write_indent(depth + 3);
            let _ = writeln!(
                self.out,
                "<R10>{}</R10><R11>{}</R11><R12>{}</R12>",
                fmt_num(cf[6]),
                fmt_num(cf[7]),
                fmt_num(cf[8])
            );
            self.write_indent(depth + 3);
            let _ = writeln!(
                self.out,
                "<R20>{}</R20><R21>{}</R21><R22>{}</R22>",
                fmt_num(cf[9]),
                fmt_num(cf[10]),
                fmt_num(cf[11])
            );
            self.write_indent(depth + 2);
            self.out.push_str("</CoordinateFrame>\n");
            self.write_indent(depth + 2);
            let _ = writeln!(
                self.out,
                "<Vector3 name=\"size\"><X>{}</X><Y>{}</Y><Z>{}</Z></Vector3>",
                fmt_num(sx),
                fmt_num(sy),
                fmt_num(sz)
            );
            self.write_indent(depth + 2);
            let _ = writeln!(
                self.out,
                "<Color3uint8 name=\"Color3uint8\">{color_u32}</Color3uint8>"
            );
            self.write_indent(depth + 2);
            let _ = writeln!(self.out, "<token name=\"Material\">{material_tok}</token>");
            self.write_indent(depth + 2);
            let _ = writeln!(
                self.out,
                "<float name=\"Transparency\">{}</float>",
                fmt_num(transparency)
            );
            if node.class_name == "Part" {
                if let Some(shape_tok) = shape_to_token(props.get("Shape").or_else(|| props.get("shape"))) {
                    self.write_indent(depth + 2);
                    let _ = writeln!(self.out, "<token name=\"shape\">{shape_tok}</token>");
                }
            }
            if node.class_name == "SpawnLocation" {
                let neutral = props
                    .get("Neutral")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                self.write_indent(depth + 2);
                let _ = writeln!(self.out, "<bool name=\"Neutral\">{neutral}</bool>");
            }
            self.write_extra_properties(
                depth + 2,
                props,
                &[
                    "Name",
                    "Anchored",
                    "CanCollide",
                    "CFrame",
                    "Position",
                    "Rotation",
                    "Size",
                    "size",
                    "Color",
                    "Color3uint8",
                    "Material",
                    "Transparency",
                    "Neutral",
                    "Shape",
                    "shape",
                ],
            );
        } else if matches!(node.class_name.as_str(), "Decal" | "Texture") {
            let props = &node.properties;
            let tex = props
                .get("Texture")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let face_tok = face_to_token(props.get("Face"));
            self.write_indent(depth + 2);
            let _ = writeln!(
                self.out,
                "<Content name=\"Texture\"><url>{}</url></Content>",
                xml_escape(tex)
            );
            self.write_indent(depth + 2);
            let _ = writeln!(self.out, "<token name=\"Face\">{face_tok}</token>");
            self.write_extra_properties(depth + 2, props, &["Name", "Texture", "Face"]);
        } else if matches!(node.class_name.as_str(), "ImageLabel" | "ImageButton") {
            let props = &node.properties;
            let img = props
                .get("Image")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let bg_trans = props
                .get("BackgroundTransparency")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            self.write_indent(depth + 2);
            let _ = writeln!(
                self.out,
                "<Content name=\"Image\"><url>{}</url></Content>",
                xml_escape(img)
            );
            self.write_indent(depth + 2);
            let _ = writeln!(
                self.out,
                "<float name=\"BackgroundTransparency\">{}</float>",
                fmt_num(bg_trans)
            );
            self.write_extra_properties(
                depth + 2,
                props,
                &["Name", "Image", "BackgroundTransparency"],
            );
        } else if node.class_name == "SurfaceGui" {
            let props = &node.properties;
            let face_tok = face_to_token(props.get("Face"));
            self.write_indent(depth + 2);
            let _ = writeln!(self.out, "<token name=\"Face\">{face_tok}</token>");
            self.write_extra_properties(depth + 2, props, &["Name", "Face"]);
        } else {
            self.write_extra_properties(depth + 2, &node.properties, &["Name"]);
        }

        self.write_indent(depth + 1);
        self.out.push_str("</Properties>\n");

        for child in node.children.values() {
            self.write_node(child, depth + 1);
        }

        self.write_indent(depth);
        self.out.push_str("</Item>\n");
    }
}

fn insert_instance_path(
    services: &mut BTreeMap<String, ExportNode>,
    path: &str,
    class_name: &str,
    props: Value,
) {
    let segs: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return;
    }
    let service = segs[0];
    let root = services
        .entry(service.to_string())
        .or_insert_with(|| ExportNode::container(service, service));
    let mut cur = root;
    for seg in &segs[1..segs.len() - 1] {
        let default_container = if service == "StarterPlayer" && *seg == "StarterPlayerScripts" {
            "StarterPlayerScripts"
        } else if service == "StarterPlayer" && *seg == "StarterCharacterScripts" {
            "StarterCharacterScripts"
        } else {
            "Folder"
        };
        cur = cur
            .children
            .entry((*seg).to_string())
            .or_insert_with(|| ExportNode::container(seg, default_container));
    }
    let leaf_name = segs[segs.len() - 1];
    let leaf = cur
        .children
        .entry(leaf_name.to_string())
        .or_insert_with(|| ExportNode::container(leaf_name, class_name));
    leaf.class_name = class_name.to_string();
    leaf.properties = props;
}

fn insert_script_path(
    services: &mut BTreeMap<String, ExportNode>,
    path: &str,
    source: &str,
) {
    let raw_segs: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    if raw_segs.is_empty() {
        return;
    }
    // Normalize `StarterPlayer.<Script>` into `StarterPlayer.StarterPlayerScripts.<Script>`
    let (service, rest): (&str, Vec<&str>) = match raw_segs.as_slice() {
        [single] => ("ServerScriptService", vec![*single]),
        ["StarterPlayer", name] if *name != "StarterPlayerScripts" && *name != "StarterCharacterScripts" => {
            ("StarterPlayer", vec!["StarterPlayerScripts", *name])
        }
        [svc, tail @ ..] => (*svc, tail.to_vec()),
        _ => return,
    };
    if rest.is_empty() {
        return;
    }
    let root = services
        .entry(service.to_string())
        .or_insert_with(|| ExportNode::container(service, service));
    let mut cur = root;
    for (idx, seg) in rest[..rest.len() - 1].iter().enumerate() {
        let inferred_class = if service == "Workspace" && idx == 0 {
            "Part"
        } else if service == "StarterPlayer" && *seg == "StarterPlayerScripts" {
            "StarterPlayerScripts"
        } else if service == "StarterPlayer" && *seg == "StarterCharacterScripts" {
            "StarterCharacterScripts"
        } else {
            "Folder"
        };
        cur = cur.children.entry((*seg).to_string()).or_insert_with(|| {
            let mut n = ExportNode::container(seg, inferred_class);
            if inferred_class == "Part" {
                n.properties = json!({ "Anchored": true });
            }
            n
        });
    }
    let leaf_name = rest[rest.len() - 1];
    let script_class = default_script_class(service);
    let leaf = cur
        .children
        .entry(leaf_name.to_string())
        .or_insert_with(|| ExportNode::container(leaf_name, script_class));
    leaf.class_name = script_class.to_string();
    leaf.script_source = Some(source.to_string());
}

/// Serialize any OpenRO place tree (`{ "scripts": {...}, "instances": {...} }`)
/// into a genuine Roblox Studio `.rbxlx` (`<roblox version="4">`) XML document.
pub fn serialize_rbxlx(game_name: &str, tree: &Value) -> String {
    let empty_map = Map::new();
    let scripts_obj = tree
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty_map);
    let inst_obj = tree
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty_map);

    let mut services: BTreeMap<String, ExportNode> = BTreeMap::new();
    for core in [
        "Workspace",
        "ServerScriptService",
        "StarterGui",
        "StarterPlayer",
        "ReplicatedStorage",
        "Lighting",
        "SoundService",
    ] {
        services.insert(core.to_string(), ExportNode::container(core, core));
    }
    if let Some(ws) = services.get_mut("Workspace") {
        ws.properties = json!({ "Gravity": 196.2 });
    }
    if let Some(sp) = services.get_mut("StarterPlayer") {
        sp.children.insert(
            "StarterPlayerScripts".to_string(),
            ExportNode::container("StarterPlayerScripts", "StarterPlayerScripts"),
        );
    }

    // Insert instances first so explicit Part/Folder classes win over inferred containers
    for (path, rec) in inst_obj {
        let class_name = rec
            .get("class_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Part");
        let props = rec.get("properties").cloned().unwrap_or_else(|| json!({}));
        insert_instance_path(&mut services, path, class_name, props);
    }

    // Insert scripts (can nest inside Workspace parts, ServerScriptService, StarterGui, etc.)
    for (path, src_val) in scripts_obj {
        if let Some(src) = src_val.as_str() {
            insert_script_path(&mut services, path, src);
        }
    }

    let approx_size = 2048 + inst_obj.len() * 512 + scripts_obj.len() * 1024;
    let mut g = XmlGen::new(approx_size);
    g.out.push_str(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <roblox xmlns:xmime=\"http://www.w3.org/2005/05/xmlmime\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
         xsi:noNamespaceSchemaLocation=\"http://www.roblox.com/roblox.xsd\" \
         version=\"4\">\n\
         \x20 <Meta name=\"ExplicitAutoJoints\">true</Meta>\n\
         \x20 <Meta name=\"OpenROGameName\">",
    );
    g.out.push_str(&xml_escape(game_name));
    g.out.push_str("</Meta>\n  <External>null</External>\n  <External>nil</External>\n");

    // Emit canonical Roblox Studio service order first, then any extra services (e.g. ServerStorage, StarterPack)
    let canonical_order = [
        "Workspace",
        "ServerScriptService",
        "StarterGui",
        "StarterPlayer",
        "ReplicatedStorage",
        "ServerStorage",
        "StarterPack",
        "Lighting",
        "SoundService",
    ];
    let mut emitted: BTreeSet<&str> = BTreeSet::new();
    for svc in canonical_order {
        if let Some(node) = services.get(svc) {
            g.write_node(node, 1);
            emitted.insert(svc);
        }
    }
    for (svc, node) in &services {
        if !emitted.contains(svc.as_str()) {
            g.write_node(node, 1);
        }
    }

    g.out.push_str("</roblox>\n");
    g.out
}

// ---------------------------------------------------------------------------
// Fuzz-Hardened Zero-Dependency Roblox Studio `.rbxlx` XML Importer
// ---------------------------------------------------------------------------

const MAX_XML_BYTES: usize = 16 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 64;
const MAX_XML_ITEMS: usize = 50_000;

type XmlAttrs = Vec<(String, String)>;
type OpenTag = (String, XmlAttrs, bool);
type ParsedProperties = (Option<String>, Option<String>, Map<String, Value>);

struct XmlParser<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    items_seen: usize,
}

impl<'a> XmlParser<'a> {
    fn new(src: &'a str) -> Self {
        let trimmed = src.strip_prefix('\u{FEFF}').unwrap_or(src);
        XmlParser {
            src: trimmed,
            bytes: trimmed.as_bytes(),
            pos: 0,
            items_seen: 0,
        }
    }

    fn remaining(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn starts_with(&self, pat: &str) -> bool {
        self.remaining().starts_with(pat)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn skip_misc(&mut self) -> Result<(), String> {
        loop {
            self.skip_ws();
            if self.starts_with("<?") {
                let Some(end) = self.remaining().find("?>") else {
                    return Err("unclosed XML processing instruction <?".to_string());
                };
                self.pos += end + 2;
            } else if self.starts_with("<!--") {
                let Some(end) = self.remaining().find("-->") else {
                    return Err("unclosed XML comment <!--".to_string());
                };
                self.pos += end + 3;
            } else if self.starts_with("<!") && !self.starts_with("<![CDATA[") {
                return Err("XML DOCTYPE/ENTITY declarations are forbidden (XXE protection)".to_string());
            } else {
                break;
            }
        }
        Ok(())
    }

    fn read_name(&mut self) -> Result<&'a str, String> {
        let start = self.pos;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':' | b'.') {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(format!("expected XML name at byte {}", self.pos));
        }
        Ok(&self.src[start..self.pos])
    }

    fn read_attr_value(&mut self) -> Result<String, String> {
        if self.is_eof() {
            return Err("unexpected EOF reading XML attribute value".to_string());
        }
        let quote = self.bytes[self.pos];
        if quote != b'"' && quote != b'\'' {
            return Err(format!(
                "expected quote for XML attribute value at byte {}",
                self.pos
            ));
        }
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != quote {
            self.pos += 1;
        }
        if self.pos >= self.bytes.len() {
            return Err("unclosed XML attribute value".to_string());
        }
        let raw = &self.src[start..self.pos];
        self.pos += 1;
        Ok(xml_unescape(raw))
    }

    /// Parse `<TagName attr="val" ...>` or `<TagName ... />`.
    /// Returns `(tag_name, attributes, self_closing)`.
    fn read_open_tag(&mut self) -> Result<OpenTag, String> {
        if !self.starts_with("<") {
            return Err(format!("expected '<' at byte {}", self.pos));
        }
        if self.starts_with("</") || self.starts_with("<!") || self.starts_with("<?") {
            return Err(format!("unexpected tag token at byte {}", self.pos));
        }
        self.pos += 1;
        let name = self.read_name()?.to_string();
        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            if self.is_eof() {
                return Err(format!("unclosed start tag <{name}>"));
            }
            if self.starts_with("/>") {
                self.pos += 2;
                return Ok((name, attrs, true));
            }
            if self.starts_with(">") {
                self.pos += 1;
                return Ok((name, attrs, false));
            }
            let attr_name = self.read_name()?.to_string();
            self.skip_ws();
            if !self.starts_with("=") {
                return Err(format!(
                    "expected '=' after attribute '{attr_name}' in <{name}>"
                ));
            }
            self.pos += 1;
            self.skip_ws();
            let attr_val = self.read_attr_value()?;
            attrs.push((attr_name, attr_val));
        }
    }

    fn read_close_tag(&mut self, expected: &str) -> Result<(), String> {
        if !self.starts_with("</") {
            return Err(format!(
                "expected closing tag </{expected}> at byte {}",
                self.pos
            ));
        }
        self.pos += 2;
        let actual = self.read_name()?;
        self.skip_ws();
        if !self.starts_with(">") {
            return Err(format!("unclosed closing tag </{actual}>"));
        }
        self.pos += 1;
        if actual != expected {
            return Err(format!(
                "mismatched closing tag </{actual}> (expected </{expected}>)"
            ));
        }
        Ok(())
    }

    /// Read text + CDATA content until `</expected>`, validating and consuming `</expected>`.
    fn read_text_element_body(&mut self, expected: &str) -> Result<String, String> {
        let mut out = String::new();
        loop {
            if self.is_eof() {
                return Err(format!("unclosed element <{expected}>"));
            }
            if self.starts_with("</") {
                self.read_close_tag(expected)?;
                return Ok(out);
            }
            if self.starts_with("<![CDATA[") {
                self.pos += 9;
                let Some(end) = self.remaining().find("]]>") else {
                    return Err(format!("unclosed CDATA section inside <{expected}>"));
                };
                out.push_str(&self.remaining()[..end]);
                self.pos += end + 3;
                continue;
            }
            if self.starts_with("<!--") {
                let Some(end) = self.remaining().find("-->") else {
                    return Err("unclosed XML comment".to_string());
                };
                self.pos += end + 3;
                continue;
            }
            if self.starts_with("<") {
                return Err(format!(
                    "unexpected nested element inside text element <{expected}> at byte {}",
                    self.pos
                ));
            }
            let next_lt = self.remaining().find('<').unwrap_or(self.remaining().len());
            out.push_str(&xml_unescape(&self.remaining()[..next_lt]));
            self.pos += next_lt;
        }
    }

    /// Skip an arbitrary element body (including nested sub-elements) until its matching `</expected>`.
    fn skip_element_body(&mut self, expected: &str, depth: usize) -> Result<(), String> {
        if depth > MAX_XML_DEPTH {
            return Err(format!("XML nesting exceeds maximum depth {MAX_XML_DEPTH}"));
        }
        loop {
            self.skip_misc()?;
            if self.is_eof() {
                return Err(format!("unclosed element <{expected}>"));
            }
            if self.starts_with("</") {
                self.read_close_tag(expected)?;
                return Ok(());
            }
            if self.starts_with("<![CDATA[") {
                self.pos += 9;
                let Some(end) = self.remaining().find("]]>") else {
                    return Err("unclosed CDATA section".to_string());
                };
                self.pos += end + 3;
                continue;
            }
            if self.starts_with("<") {
                let (child, _, self_closing) = self.read_open_tag()?;
                if !self_closing {
                    self.skip_element_body(&child, depth + 1)?;
                }
                continue;
            }
            let next_lt = self.remaining().find('<').unwrap_or(self.remaining().len());
            self.pos += next_lt;
        }
    }

    /// Read child numeric tags like `<X>1</X><Y>2</Y><Z>3</Z>` inside `<Vector3>` / `<CoordinateFrame>` / `<Color3>`.
    fn read_numeric_fields(
        &mut self,
        parent_tag: &str,
        depth: usize,
    ) -> Result<BTreeMap<String, f64>, String> {
        if depth > MAX_XML_DEPTH {
            return Err(format!("XML nesting exceeds maximum depth {MAX_XML_DEPTH}"));
        }
        let mut fields = BTreeMap::new();
        loop {
            self.skip_misc()?;
            if self.is_eof() {
                return Err(format!("unclosed <{parent_tag}>"));
            }
            if self.starts_with("</") {
                self.read_close_tag(parent_tag)?;
                return Ok(fields);
            }
            let (tag, _, self_closing) = self.read_open_tag()?;
            let text = if self_closing {
                String::new()
            } else {
                self.read_text_element_body(&tag)?
            };
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let val = trimmed.parse::<f64>().map_err(|_| {
                    format!("invalid numeric value '{trimmed}' in <{parent_tag}><{tag}>")
                })?;
                if !val.is_finite() {
                    return Err(format!("non-finite numeric value in <{parent_tag}><{tag}>"));
                }
                fields.insert(tag, val);
            }
        }
    }

    /// Read `<Content name="..."><url>...</url></Content>` or `<Content><null></null></Content>`.
    fn read_content_url(&mut self, depth: usize) -> Result<String, String> {
        if depth > MAX_XML_DEPTH {
            return Err(format!("XML nesting exceeds maximum depth {MAX_XML_DEPTH}"));
        }
        let mut url = String::new();
        loop {
            self.skip_misc()?;
            if self.is_eof() {
                return Err("unclosed <Content>".to_string());
            }
            if self.starts_with("</") {
                self.read_close_tag("Content")?;
                return Ok(url);
            }
            if self.starts_with("<") {
                let (tag, _, self_closing) = self.read_open_tag()?;
                if !self_closing {
                    if tag == "url" || tag == "uri" {
                        url = self.read_text_element_body(&tag)?;
                    } else {
                        self.skip_element_body(&tag, depth + 1)?;
                    }
                }
                continue;
            }
            let next_lt = self.remaining().find('<').unwrap_or(self.remaining().len());
            let txt = xml_unescape(self.remaining()[..next_lt].trim());
            if !txt.is_empty() && url.is_empty() {
                url = txt;
            }
            self.pos += next_lt;
        }
    }

    fn parse_properties(&mut self, depth: usize) -> Result<ParsedProperties, String> {
        if depth > MAX_XML_DEPTH {
            return Err(format!("XML nesting exceeds maximum depth {MAX_XML_DEPTH}"));
        }
        let mut item_name: Option<String> = None;
        let mut script_source: Option<String> = None;
        let mut props: Map<String, Value> = Map::new();

        loop {
            self.skip_misc()?;
            if self.is_eof() {
                return Err("unclosed <Properties>".to_string());
            }
            if self.starts_with("</") {
                self.read_close_tag("Properties")?;
                return Ok((item_name, script_source, props));
            }
            let (tag, attrs, self_closing) = self.read_open_tag()?;
            let prop_name = attrs
                .iter()
                .find(|(k, _)| k == "name")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();

            if self_closing {
                if tag == "ProtectedString" && prop_name == "Source" {
                    script_source = Some(String::new());
                } else if tag == "string" && prop_name == "Name" {
                    item_name = Some(String::new());
                }
                continue;
            }

            match tag.as_str() {
                "string" => {
                    let val = self.read_text_element_body(&tag)?;
                    if prop_name == "Name" {
                        item_name = Some(val);
                    } else if !prop_name.is_empty() {
                        props.insert(prop_name, Value::String(val));
                    }
                }
                "ProtectedString" => {
                    let val = self.read_text_element_body(&tag)?;
                    if prop_name == "Source" {
                        script_source = Some(val);
                    } else if !prop_name.is_empty() {
                        props.insert(prop_name, Value::String(val));
                    }
                }
                "bool" => {
                    let raw = self.read_text_element_body(&tag)?;
                    let b = match raw.trim() {
                        "true" => true,
                        "false" => false,
                        other => {
                            return Err(format!(
                                "invalid bool '{other}' for property '{prop_name}'"
                            ))
                        }
                    };
                    if !prop_name.is_empty() {
                        props.insert(prop_name, Value::Bool(b));
                    }
                }
                "int" | "int64" => {
                    let raw = self.read_text_element_body(&tag)?;
                    let n = raw.trim().parse::<i64>().map_err(|_| {
                        format!("invalid integer '{}' for property '{prop_name}'", raw.trim())
                    })?;
                    if !prop_name.is_empty() {
                        props.insert(prop_name, json!(n));
                    }
                }
                "float" | "double" => {
                    let raw = self.read_text_element_body(&tag)?;
                    let f = raw.trim().parse::<f64>().map_err(|_| {
                        format!("invalid float '{}' for property '{prop_name}'", raw.trim())
                    })?;
                    if !f.is_finite() {
                        return Err(format!("non-finite float for property '{prop_name}'"));
                    }
                    if !prop_name.is_empty() {
                        props.insert(prop_name, json!(f));
                    }
                }
                "token" => {
                    let raw = self.read_text_element_body(&tag)?;
                    let tok = raw.trim().parse::<u32>().map_err(|_| {
                        format!("invalid token '{}' for property '{prop_name}'", raw.trim())
                    })?;
                    if prop_name == "Material" {
                        if let Some(mat_name) = token_to_material(tok) {
                            props.insert("Material".to_string(), Value::String(mat_name.to_string()));
                        } else {
                            props.insert("Material".to_string(), json!(tok));
                        }
                    } else if prop_name == "Face" {
                        if let Some(face_name) = token_to_face(tok) {
                            props.insert("Face".to_string(), Value::String(face_name.to_string()));
                        } else {
                            props.insert("Face".to_string(), json!(tok));
                        }
                    } else if prop_name == "shape" || prop_name == "Shape" {
                        if let Some(shape_name) = token_to_shape(tok) {
                            props.insert("Shape".to_string(), Value::String(shape_name.to_string()));
                        } else {
                            props.insert("Shape".to_string(), json!(tok));
                        }
                    } else if !prop_name.is_empty() {
                        props.insert(prop_name, json!(tok));
                    }
                }
                "Content" => {
                    let url_val = self.read_content_url(depth + 1)?;
                    if !prop_name.is_empty() {
                        props.insert(prop_name, Value::String(url_val));
                    }
                }
                "Color3uint8" => {
                    let raw = self.read_text_element_body(&tag)?;
                    let c = raw.trim().parse::<u32>().map_err(|_| {
                        format!(
                            "invalid Color3uint8 '{}' for property '{prop_name}'",
                            raw.trim()
                        )
                    })?;
                    props.insert("Color".to_string(), Value::String(color_u32_to_hex(c)));
                }
                "Color3" => {
                    let fields = self.read_numeric_fields(&tag, depth + 1)?;
                    let r = (fields.get("R").copied().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u32;
                    let g = (fields.get("G").copied().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u32;
                    let b = (fields.get("B").copied().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u32;
                    let hex = format!("#{r:02X}{g:02X}{b:02X}");
                    let key = if prop_name == "Color3" || prop_name == "Color" || prop_name.is_empty() {
                        "Color".to_string()
                    } else {
                        prop_name
                    };
                    props.insert(key, Value::String(hex));
                }
                "Vector3" => {
                    let fields = self.read_numeric_fields(&tag, depth + 1)?;
                    let x = fields.get("X").copied().unwrap_or(0.0);
                    let y = fields.get("Y").copied().unwrap_or(0.0);
                    let z = fields.get("Z").copied().unwrap_or(0.0);
                    let key = if prop_name == "size" {
                        "Size".to_string()
                    } else {
                        prop_name
                    };
                    if !key.is_empty() {
                        props.insert(key, json!([x, y, z]));
                    }
                }
                "CoordinateFrame" => {
                    let fields = self.read_numeric_fields(&tag, depth + 1)?;
                    let x = fields.get("X").copied().unwrap_or(0.0);
                    let y = fields.get("Y").copied().unwrap_or(0.0);
                    let z = fields.get("Z").copied().unwrap_or(0.0);
                    let r00 = fields.get("R00").copied().unwrap_or(1.0);
                    let r01 = fields.get("R01").copied().unwrap_or(0.0);
                    let r02 = fields.get("R02").copied().unwrap_or(0.0);
                    let r10 = fields.get("R10").copied().unwrap_or(0.0);
                    let r11 = fields.get("R11").copied().unwrap_or(1.0);
                    let r12 = fields.get("R12").copied().unwrap_or(0.0);
                    let r20 = fields.get("R20").copied().unwrap_or(0.0);
                    let r21 = fields.get("R21").copied().unwrap_or(0.0);
                    let r22 = fields.get("R22").copied().unwrap_or(1.0);
                    if prop_name == "CFrame" {
                        props.insert("Position".to_string(), json!([x, y, z]));
                        let is_identity = (r00 - 1.0).abs() < 1e-6
                            && r01.abs() < 1e-6
                            && r02.abs() < 1e-6
                            && r10.abs() < 1e-6
                            && (r11 - 1.0).abs() < 1e-6
                            && r12.abs() < 1e-6
                            && r20.abs() < 1e-6
                            && r21.abs() < 1e-6
                            && (r22 - 1.0).abs() < 1e-6;
                        if !is_identity {
                            props.insert(
                                "CFrame".to_string(),
                                json!([x, y, z, r00, r01, r02, r10, r11, r12, r20, r21, r22]),
                            );
                        }
                    } else if !prop_name.is_empty() {
                        props.insert(
                            prop_name,
                            json!([x, y, z, r00, r01, r02, r10, r11, r12, r20, r21, r22]),
                        );
                    }
                }
                _ => {
                    // Skip additional Roblox Studio property types (`Ref`, `BinaryString`, `PhysicalProperties`, etc.)
                    self.skip_element_body(&tag, depth + 1)?;
                }
            }
        }
    }

    fn parse_item(
        &mut self,
        attrs: &[(String, String)],
        self_closing: bool,
        parent_path: Option<&str>,
        depth: usize,
        scripts: &mut BTreeMap<String, String>,
        instances: &mut BTreeMap<String, Value>,
    ) -> Result<(), String> {
        if depth > MAX_XML_DEPTH {
            return Err(format!("XML nesting exceeds maximum depth {MAX_XML_DEPTH}"));
        }
        self.items_seen += 1;
        if self.items_seen > MAX_XML_ITEMS {
            return Err(format!("XML exceeds maximum item count ({MAX_XML_ITEMS})"));
        }

        let class_name = attrs
            .iter()
            .find(|(k, _)| k == "class")
            .map(|(_, v)| v.clone())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "Item element missing required 'class' attribute".to_string())?;

        if self_closing {
            return Ok(());
        }

        let mut item_name: Option<String> = None;
        let mut script_source: Option<String> = None;
        let mut props: Map<String, Value> = Map::new();
        let mut children_parsed = false;

        // First collect <Properties> (Roblox XML always places <Properties> before child <Item>s,
        // and we register the parent path as soon as its Name is known).
        let mut current_path: Option<String> = None;

        loop {
            self.skip_misc()?;
            if self.is_eof() {
                return Err(format!("unclosed <Item class=\"{class_name}\">"));
            }
            if self.starts_with("</") {
                self.read_close_tag("Item")?;
                break;
            }
            let (tag, child_attrs, child_self_closing) = self.read_open_tag()?;
            if tag == "Properties" {
                if !child_self_closing {
                    let (n, src, p) = self.parse_properties(depth + 1)?;
                    if n.is_some() {
                        item_name = n;
                    }
                    if src.is_some() {
                        script_source = src;
                    }
                    props.extend(p);
                }
            } else if tag == "Item" {
                if !children_parsed {
                    let resolved = register_item(
                        &class_name,
                        item_name.as_deref(),
                        script_source.as_deref(),
                        &props,
                        parent_path,
                        scripts,
                        instances,
                    );
                    current_path = Some(resolved);
                    children_parsed = true;
                }
                self.parse_item(
                    &child_attrs,
                    child_self_closing,
                    current_path.as_deref(),
                    depth + 1,
                    scripts,
                    instances,
                )?;
            } else {
                // Unknown element inside <Item>; skip cleanly while checking tag balance
                if !child_self_closing {
                    self.skip_element_body(&tag, depth + 1)?;
                }
            }
        }

        if !children_parsed {
            register_item(
                &class_name,
                item_name.as_deref(),
                script_source.as_deref(),
                &props,
                parent_path,
                scripts,
                instances,
            );
        }
        Ok(())
    }
}

fn register_item(
    class_name: &str,
    item_name: Option<&str>,
    script_source: Option<&str>,
    props: &Map<String, Value>,
    parent_path: Option<&str>,
    scripts: &mut BTreeMap<String, String>,
    instances: &mut BTreeMap<String, Value>,
) -> String {
    let resolved_name = item_name
        .filter(|n| !n.is_empty())
        .unwrap_or(class_name);

    // Top-level Roblox service container (Workspace, ServerScriptService, StarterGui, ...)
    if parent_path.is_none() {
        let svc_name = if VALID_SERVICES.contains(&class_name) {
            class_name
        } else {
            resolved_name
        };
        return svc_name.to_string();
    }

    let parent = parent_path.unwrap_or("Workspace");
    let full_path = format!("{parent}.{resolved_name}");

    // Structural StarterPlayer sub-services do not emit synthetic instance entries
    if parent == "StarterPlayer"
        && matches!(
            class_name,
            "StarterPlayerScripts" | "StarterCharacterScripts"
        )
    {
        return full_path;
    }

    if matches!(class_name, "Script" | "LocalScript" | "ModuleScript") {
        let source = script_source.unwrap_or("");
        scripts.insert(full_path.clone(), source.to_string());
    } else {
        instances.insert(
            full_path.clone(),
            json!({
                "class_name": class_name,
                "properties": Value::Object(props.clone()),
            }),
        );
    }
    full_path
}

fn xml_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(semi_rel) = s[i..].find(';') {
                let entity = &s[i + 1..i + semi_rel];
                let decoded = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                        u32::from_str_radix(&entity[2..], 16)
                            .ok()
                            .and_then(char::from_u32)
                    }
                    _ if entity.starts_with('#') => entity[1..]
                        .parse::<u32>()
                        .ok()
                        .and_then(char::from_u32),
                    _ => None,
                };
                if let Some(ch) = decoded {
                    out.push(ch);
                    i += semi_rel + 1;
                    continue;
                }
            }
        }
        let ch = s[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Parse a Roblox Studio `.rbxlx` (`<roblox version="4">`) XML document into a `Place`.
pub fn parse_rbxlx_xml(xml: &str) -> Result<Place, String> {
    if xml.len() > MAX_XML_BYTES {
        return Err(format!(
            "rbxlx XML exceeds maximum allowed size ({MAX_XML_BYTES} bytes)"
        ));
    }
    if xml.contains('\0') {
        return Err("rbxlx XML must not contain NUL bytes".to_string());
    }
    if xml.trim().is_empty() {
        return Err("empty rbxlx XML document".to_string());
    }

    let mut parser = XmlParser::new(xml);
    parser.skip_misc()?;
    if parser.is_eof() {
        return Err("empty rbxlx XML document".to_string());
    }

    let (root_tag, _root_attrs, root_self_closing) = parser.read_open_tag()?;
    if root_tag != "roblox" {
        return Err(format!(
            "expected <roblox> root element, found <{root_tag}>"
        ));
    }

    let mut game_name = "Imported Place".to_string();
    let mut scripts: BTreeMap<String, String> = BTreeMap::new();
    let mut instances: BTreeMap<String, Value> = BTreeMap::new();

    if !root_self_closing {
        loop {
            parser.skip_misc()?;
            if parser.is_eof() {
                return Err("unclosed <roblox> root element".to_string());
            }
            if parser.starts_with("</") {
                parser.read_close_tag("roblox")?;
                break;
            }
            let (tag, attrs, self_closing) = parser.read_open_tag()?;
            match tag.as_str() {
                "Meta" => {
                    let meta_name = attrs
                        .iter()
                        .find(|(k, _)| k == "name")
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("");
                    let body = if self_closing {
                        String::new()
                    } else {
                        parser.read_text_element_body("Meta")?
                    };
                    if meta_name == "OpenROGameName" && !body.trim().is_empty() {
                        game_name = body.trim().to_string();
                    }
                }
                "Item" => {
                    parser.parse_item(
                        &attrs,
                        self_closing,
                        None,
                        1,
                        &mut scripts,
                        &mut instances,
                    )?;
                }
                _ => {
                    if !self_closing {
                        parser.skip_element_body(&tag, 1)?;
                    }
                }
            }
        }
    }

    parser.skip_misc()?;
    if !parser.is_eof() {
        return Err(format!(
            "unexpected trailing content after </roblox> at byte {}",
            parser.pos
        ));
    }

    Ok(LocalPlace {
        scripts,
        instances,
        last_console: String::new(),
        data_dir: None,
        game_name,
        recovered: Vec::new(),
        undo_stack: VecDeque::new(),
        redo_stack: VecDeque::new(),
        revision: Arc::new(std::sync::atomic::AtomicU64::new(1)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luau_syntax_checker_passes_valid_rune_scripts_and_catches_errors() {
        let valid = crate::skills::generate(
            "checkpoints",
            &json!({ "stage_count": 6, "folder_name": "Checkpoints", "create_folder": true }),
        )
        .unwrap();
        assert!(
            check_luau_syntax(&valid).is_ok(),
            "valid checkpoints Luau passes"
        );

        let unclosed_block = "local function broken()\n    print('hi')\n";
        assert!(
            check_luau_syntax(unclosed_block).is_err(),
            "missing end caught"
        );

        let unclosed_str = "local msg = \"hello\nprint(msg)";
        assert!(
            check_luau_syntax(unclosed_str).is_err(),
            "unclosed string caught"
        );

        let mismatched = "local x = (1 + 2]";
        assert!(
            check_luau_syntax(mismatched).is_err(),
            "mismatched delimiter caught"
        );
    }

    #[test]
    fn local_place_persists_rbxlx_and_validates_playtest() {
        let dir = std::env::temp_dir().join(format!("openro-place-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut place = LocalPlace::open(dir.clone(), "Skyline Obby".to_string());

        // Writing spawn_link without Workspace.SpawnPad fails playtest honestly
        let link =
            crate::skills::generate("spawn_link", &json!({ "instance_name": "SpawnPad" })).unwrap();
        place
            .call_tool(
                "write_script",
                json!({ "path": "ServerScriptService.SpawnLink", "source": link }),
            )
            .unwrap();
        let failed: Value =
            serde_json::from_str(&place.call_tool("playtest", json!({})).unwrap()).unwrap();
        assert_eq!(failed["status"], "failed");
        assert!(place
            .call_tool("get_console", json!({}))
            .unwrap()
            .contains("Workspace.SpawnPad"));

        // Creating Workspace.SpawnPad + LavaBrick + KillBrick makes playtest pass and writes game.rbxlx
        place
            .call_tool("create_instance", json!({ "class_name": "SpawnLocation", "name": "SpawnPad", "parent": "Workspace", "properties": { "Anchored": true } }))
            .unwrap();
        place
            .call_tool("create_instance", json!({ "class_name": "Part", "name": "LavaBrick", "parent": "Workspace", "properties": { "Anchored": true } }))
            .unwrap();
        let kb = crate::skills::generate("kill_brick", &json!({ "damage": 25 })).unwrap();
        place
            .call_tool(
                "write_script",
                json!({ "path": "Workspace.LavaBrick.KillBrick", "source": kb }),
            )
            .unwrap();

        let passed: Value =
            serde_json::from_str(&place.call_tool("playtest", json!({})).unwrap()).unwrap();
        assert_eq!(passed["status"], "passed");

        let xml = std::fs::read_to_string(dir.join("game.rbxlx")).expect("game.rbxlx written");
        assert!(xml.contains("<roblox ") && xml.contains("version=\"4\""));
        assert!(xml.contains("<Item class=\"SpawnLocation\""));
        assert!(xml.contains("<string name=\"Name\">SpawnPad</string>"));
        assert!(xml.contains("<Item class=\"Script\""));
        assert!(xml.contains("<string name=\"Name\">KillBrick</string>"));
        assert!(xml.contains("<token name=\"Material\">"));
        assert!(xml.contains("<float name=\"Transparency\">"));
        assert!(xml.contains("<bool name=\"CanCollide\">"));
        assert!(xml.contains("TakeDamage(DAMAGE)"));

        // Round-trip through Place::from_rbxlx_xml
        let imported = Place::from_rbxlx_xml(&xml).expect("valid rbxlx round-trips");
        assert_eq!(imported.game_name(), "Skyline Obby");
        assert_eq!(imported.scripts().len(), 2);
        assert_eq!(imported.instances().len(), 2);
        assert!(imported
            .scripts()
            .get("Workspace.LavaBrick.KillBrick")
            .unwrap()
            .contains("TakeDamage(DAMAGE)"));

        // Reload from disk preserves scripts & instances
        let reloaded = LocalPlace::open(dir.clone(), "Skyline Obby".to_string());
        let tree = reloaded.tree_json();
        assert!(tree["scripts"]["ServerScriptService.SpawnLink"].is_string());
        assert!(tree["instances"]["Workspace.SpawnPad"].is_object());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
