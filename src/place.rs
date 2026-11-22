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
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Canonical Place DataModel alias (`Place::from_rbxlx_xml(&str) -> Result<Place, String>`).
pub type Place = LocalPlace;

/// Unified backend for either a live Roblox Studio MCP bridge or the built-in
/// persistent Local Place Engine.
pub enum PlaceBackend {
    Mcp(McpClient),
    Local(LocalPlace),
}

impl PlaceBackend {
    pub fn is_alive(&mut self) -> bool {
        match self {
            PlaceBackend::Mcp(c) => c.is_alive(),
            PlaceBackend::Local(_) => true,
        }
    }

    pub fn tools_known(&self) -> bool {
        match self {
            PlaceBackend::Mcp(c) => c.tools_known(),
            PlaceBackend::Local(_) => true,
        }
    }

    pub fn has_tool(&self, name: &str) -> bool {
        match self {
            PlaceBackend::Mcp(c) => c.has_tool(name),
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
            PlaceBackend::Local(p) => p.call_tool(name, arguments),
        }
    }

    pub fn get_place_tree(&mut self) -> Result<Value, String> {
        match self {
            PlaceBackend::Mcp(c) => c.get_place_tree(),
            PlaceBackend::Local(p) => Ok(p.tree_json()),
        }
    }

    pub fn mode_name(&self) -> &'static str {
        match self {
            PlaceBackend::Mcp(_) => "studio_mcp",
            PlaceBackend::Local(_) => "local_place",
        }
    }

    pub fn rbxlx_path(&self) -> Option<PathBuf> {
        match self {
            PlaceBackend::Mcp(_) => None,
            PlaceBackend::Local(p) => p.rbxlx_path(),
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

#[derive(Clone, Debug)]
pub struct LocalPlace {
    scripts: BTreeMap<String, String>,
    instances: BTreeMap<String, Value>,
    last_console: String,
    data_dir: Option<PathBuf>,
    game_name: String,
    pub recovered: Vec<String>,
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
        }
    }

    /// Persistent place stored as `place.json` + `game.rbxlx` inside `data_dir`.
    pub fn open(data_dir: PathBuf, game_name: String) -> Self {
        let _ = std::fs::create_dir_all(&data_dir);
        let mut recovered = Vec::new();
        let (scripts, instances) = load_place(&data_dir, &mut recovered);
        let place = LocalPlace {
            scripts,
            instances,
            last_console: String::new(),
            data_dir: Some(data_dir),
            game_name,
            recovered,
        };
        place.persist();
        place
    }

    /// Parse a Roblox Studio `.rbxlx` XML document back into an in-memory `Place` (`LocalPlace`).
    pub fn from_rbxlx_xml(xml: &str) -> Result<Place, String> {
        parse_rbxlx_xml(xml)
    }

    /// Serialize this Place into a genuine Roblox Studio `.rbxlx` (`<roblox version="4">`) document.
    pub fn to_rbxlx_xml(&self) -> String {
        serialize_rbxlx(&self.game_name, &self.tree_json())
    }

    /// Replace this Place's contents from an imported `.rbxlx` XML document and persist atomically.
    pub fn import_rbxlx_xml(&mut self, xml: &str) -> Result<ImportSummary, String> {
        let parsed = parse_rbxlx_xml(xml)?;
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

    fn persist(&self) {
        let Some(dir) = &self.data_dir else { return };
        let _ = std::fs::create_dir_all(dir);
        let json_body =
            serde_json::to_string_pretty(&self.tree_json()).unwrap_or_else(|_| "{}".to_string());
        let json_path = dir.join("place.json");
        let json_tmp = dir.join("place.json.tmp");
        if std::fs::write(&json_tmp, json_body).is_ok() {
            let _ = std::fs::rename(&json_tmp, &json_path);
        }
        let xml = serialize_rbxlx(&self.game_name, &self.tree_json());
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
                self.scripts.insert(path.to_string(), source.to_string());
                self.persist();
                Ok(json!({ "ok": true, "path": path }).to_string())
            }
            "delete_script" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "delete_script requires string path".to_string())?;
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
                    let _ = writeln!(
                        self.out,
                        "<string name=\"{ek}\">{}</string>",
                        xml_escape(s)
                    );
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
                ],
            );
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
            } else if self.starts_with("<!DOCTYPE") || self.starts_with("<!doctype") || self.starts_with("<!ENTITY") || self.starts_with("<!entity") {
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
                    } else if !prop_name.is_empty() {
                        props.insert(prop_name, json!(tok));
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
