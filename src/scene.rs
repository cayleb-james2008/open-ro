//! Declarative 3D Scene Graph (`scene/workspace.model.json`), Lighting CRUD,
//! persistent undo/redo checkpoints, and `default.project.json` auto-wiring for Rojo projects.

use crate::projects::{self, Node};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub const SCENE_MODEL_REL: &str = "scene/workspace.model.json";
pub const UNDO_FILE_REL: &str = ".openro/scene-undo.json";
const MAX_UNDO_CHECKPOINTS: usize = 50;
const MAX_SCENE_OPS_PER_MUTATE: usize = 500;
const MAX_SCENE_PATH_DEPTH: usize = 32;
const MAX_SCENE_SEG_LEN: usize = 128;
const MAX_SCENE_MODEL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_MESH_BYTES: u64 = 10 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

static SCENE_LOCK: Mutex<()> = Mutex::new(());
static TMP_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
struct SceneCheckpoint {
    scene_revision: u64,
    workspace_model_raw: Option<String>,
    default_project_raw: Option<String>,
}

impl SceneCheckpoint {
    fn to_json(&self) -> Value {
        json!({
            "scene_revision": self.scene_revision,
            "workspace_model_raw": self.workspace_model_raw,
            "default_project_raw": self.default_project_raw,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        Some(SceneCheckpoint {
            scene_revision: v.get("scene_revision").and_then(|x| x.as_u64()).unwrap_or(0),
            workspace_model_raw: v
                .get("workspace_model_raw")
                .and_then(|x| x.as_str())
                .map(String::from),
            default_project_raw: v
                .get("default_project_raw")
                .and_then(|x| x.as_str())
                .map(String::from),
        })
    }
}

#[derive(Clone, Debug, Default)]
struct SceneUndoStore {
    revision: u64,
    undo_stack: Vec<SceneCheckpoint>,
    redo_stack: Vec<SceneCheckpoint>,
}

fn write_atomic_str(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create folder: {e}"))?;
    }
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("scene");
    let tmp = path.with_file_name(format!(".{file_name}.openro-tmp.{}.{seq}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| format!("create temp for {}: {e}", path.display()))?;
    if let Err(e) = file.write_all(content.as_bytes()).and_then(|_| file.flush()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("write {}: {e}", path.display()));
    }
    drop(file);
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("rename {}: {e}", path.display()));
    }
    Ok(())
}

fn safe_project_path(dir: &Path, rel: &str) -> Result<std::path::PathBuf, String> {
    if rel.contains('\0') || rel.contains('\\') {
        return Err(format!("invalid project relative path '{rel}'"));
    }
    projects::safe_join(dir, rel)
}

fn cap_checkpoints(stack: &mut Vec<SceneCheckpoint>) {
    if stack.len() > MAX_UNDO_CHECKPOINTS {
        let drop_n = stack.len() - MAX_UNDO_CHECKPOINTS;
        stack.drain(0..drop_n);
    }
}

fn load_undo_store(dir: &Path) -> SceneUndoStore {
    let Ok(path) = safe_project_path(dir, UNDO_FILE_REL) else {
        return SceneUndoStore::default();
    };
    let Ok(meta) = std::fs::symlink_metadata(&path) else {
        return SceneUndoStore::default();
    };
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_SCENE_MODEL_BYTES {
        return SceneUndoStore::default();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return SceneUndoStore::default();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return SceneUndoStore::default();
    };
    let parse_stack = |key: &str| -> Vec<SceneCheckpoint> {
        let mut stack: Vec<SceneCheckpoint> = v
            .get(key)
            .and_then(|a| a.as_array())
            .map(|arr| arr.iter().filter_map(SceneCheckpoint::from_json).collect())
            .unwrap_or_default();
        cap_checkpoints(&mut stack);
        stack
    };
    SceneUndoStore {
        revision: v.get("revision").and_then(|x| x.as_u64()).unwrap_or(0),
        undo_stack: parse_stack("undo_stack"),
        redo_stack: parse_stack("redo_stack"),
    }
}

fn save_undo_store(dir: &Path, store: &SceneUndoStore) -> Result<(), String> {
    let path = safe_project_path(dir, UNDO_FILE_REL)?;
    let payload = json!({
        "revision": store.revision,
        "undo_stack": store.undo_stack.iter().map(SceneCheckpoint::to_json).collect::<Vec<_>>(),
        "redo_stack": store.redo_stack.iter().map(SceneCheckpoint::to_json).collect::<Vec<_>>(),
    });
    let json_text = serde_json::to_string(&payload).map_err(|e| e.to_string())? + "\n";
    write_atomic_str(&path, &json_text)
}

/// Returns `(scene_revision, can_undo, can_redo)` for a project directory.
pub fn undo_status(dir: &Path) -> (u64, bool, bool) {
    let store = load_undo_store(dir);
    (store.revision, !store.undo_stack.is_empty(), !store.redo_stack.is_empty())
}

// ---------------------------------------------------------------------------
// Math / Color / Property Helpers (shared by projects.rs & scene.rs)
// ---------------------------------------------------------------------------

pub fn parse_vec3(v: Option<&Value>) -> Option<[f64; 3]> {
    let v = v?;
    let target = v.as_object().and_then(|o| o.get("Vector3")).unwrap_or(v);
    if let Some(arr) = target.as_array() {
        if arr.len() >= 3 {
            let x = arr[0].as_f64()?;
            let y = arr[1].as_f64()?;
            let z = arr[2].as_f64()?;
            if x.is_finite() && y.is_finite() && z.is_finite() {
                return Some([x, y, z]);
            }
        }
    }
    if let Some(obj) = target.as_object() {
        let x = obj.get("x").or_else(|| obj.get("X")).and_then(|n| n.as_f64())?;
        let y = obj.get("y").or_else(|| obj.get("Y")).and_then(|n| n.as_f64())?;
        let z = obj.get("z").or_else(|| obj.get("Z")).and_then(|n| n.as_f64())?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            return Some([x, y, z]);
        }
    }
    None
}

pub fn cframe_from_pos_rot(pos: [f64; 3], rot_deg: [f64; 3]) -> [f64; 12] {
    let (rx, ry, rz) = (
        rot_deg[0].to_radians(),
        rot_deg[1].to_radians(),
        rot_deg[2].to_radians(),
    );
    let (sx, cx) = rx.sin_cos();
    let (sy, cy) = ry.sin_cos();
    let (sz, cz) = rz.sin_cos();
    let clean = |n: f64| if n.abs() < 1e-12 { 0.0 } else { n };
    [
        pos[0],
        pos[1],
        pos[2],
        clean(cy * cz),
        clean(cz * sx * sy - cx * sz),
        clean(cx * cz * sy + sx * sz),
        clean(cy * sz),
        clean(cx * cz + sx * sy * sz),
        clean(cx * sy * sz - cz * sx),
        clean(-sy),
        clean(cy * sx),
        clean(cx * cy),
    ]
}

pub fn cframe_to_euler_deg(cf: [f64; 12]) -> [f64; 3] {
    let r00 = cf[3];
    let r10 = cf[6];
    let r11 = cf[7];
    let r12 = cf[8];
    let r20 = cf[9];
    let r21 = cf[10];
    let r22 = cf[11];
    let sy = (-r20).clamp(-1.0, 1.0);
    let ry = sy.asin();
    let (rx, rz) = if r20.abs() < 0.99999 {
        (r21.atan2(r22), r10.atan2(r00))
    } else {
        ((-r12).atan2(r11), 0.0)
    };
    let deg = |rad: f64| {
        let d = rad.to_degrees();
        if d.abs() < 1e-6 {
            0.0
        } else {
            (d * 10_000.0).round() / 10_000.0
        }
    };
    [deg(rx), deg(ry), deg(rz)]
}

pub fn parse_cframe_value(v: &Value) -> Option<[f64; 12]> {
    let inner = v.as_object().and_then(|o| o.get("CFrame")).unwrap_or(v);
    if let Some(arr) = inner.as_array() {
        if arr.len() == 12 && arr.iter().all(|n| n.as_f64().is_some()) {
            let mut out = [0.0; 12];
            for (i, n) in arr.iter().enumerate() {
                out[i] = n.as_f64()?;
            }
            return Some(out);
        }
        if arr.len() == 3 && arr.iter().all(|n| n.as_f64().is_some()) {
            return Some([
                arr[0].as_f64()?,
                arr[1].as_f64()?,
                arr[2].as_f64()?,
                1.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                0.0,
                1.0,
            ]);
        }
    }
    if let Some(obj) = inner.as_object() {
        let pos = parse_vec3(obj.get("position").or_else(|| obj.get("Position")))?;
        if let Some(ori) = obj.get("orientation").and_then(|o| o.as_array()) {
            if ori.len() == 3 {
                let mut rows = [[0.0; 3]; 3];
                for (r_idx, row_val) in ori.iter().enumerate() {
                    let row = row_val.as_array()?;
                    if row.len() < 3 {
                        return None;
                    }
                    for (c_idx, cell) in row.iter().enumerate().take(3) {
                        rows[r_idx][c_idx] = cell.as_f64()?;
                    }
                }
                return Some([
                    pos[0], pos[1], pos[2], rows[0][0], rows[0][1], rows[0][2], rows[1][0],
                    rows[1][1], rows[1][2], rows[2][0], rows[2][1], rows[2][2],
                ]);
            }
        }
        return Some(cframe_from_pos_rot(pos, [0.0, 0.0, 0.0]));
    }
    None
}

pub fn cframe_to_rojo_json(cf: [f64; 12]) -> Value {
    json!({
        "CFrame": {
            "position": [cf[0], cf[1], cf[2]],
            "orientation": [
                [cf[3], cf[4], cf[5]],
                [cf[6], cf[7], cf[8]],
                [cf[9], cf[10], cf[11]],
            ]
        }
    })
}

pub fn cframe_to_flat_json(cf: [f64; 12]) -> Value {
    json!({
        "position": [cf[0], cf[1], cf[2]],
        "orientation": [
            [cf[3], cf[4], cf[5]],
            [cf[6], cf[7], cf[8]],
            [cf[9], cf[10], cf[11]],
        ]
    })
}

/// Convert any color representation (`#RRGGBB`, `[r,g,b]`, `{"Color3":...}`, `{"Color3uint8":...}`, u32) to `[r, g, b]` in `0.0..=1.0`.
pub fn color_to_rgb_floats(v: &Value) -> Option<[f64; 3]> {
    if let Some(obj) = v.as_object() {
        if let Some(inner) = obj.get("Color3") {
            return color_to_rgb_floats(inner);
        }
        if let Some(inner) = obj.get("Color3uint8") {
            if let Some(arr) = inner.as_array() {
                if arr.len() >= 3 {
                    let r = arr[0].as_f64()?;
                    let g = arr[1].as_f64()?;
                    let b = arr[2].as_f64()?;
                    return Some([
                        (r / 255.0).clamp(0.0, 1.0),
                        (g / 255.0).clamp(0.0, 1.0),
                        (b / 255.0).clamp(0.0, 1.0),
                    ]);
                }
            }
            return color_to_rgb_floats(inner);
        }
    }
    if let Some(s) = v.as_str() {
        let clean = s.trim().trim_start_matches('#');
        if clean.len() == 6 {
            if let Ok(rgb) = u32::from_str_radix(clean, 16) {
                let r = ((rgb >> 16) & 0xFF) as f64 / 255.0;
                let g = ((rgb >> 8) & 0xFF) as f64 / 255.0;
                let b = (rgb & 0xFF) as f64 / 255.0;
                return Some([r, g, b]);
            }
        }
    }
    if let Some(n) = v.as_u64() {
        let rgb = (n as u32) & 0x00FF_FFFF;
        return Some([
            ((rgb >> 16) & 0xFF) as f64 / 255.0,
            ((rgb >> 8) & 0xFF) as f64 / 255.0,
            (rgb & 0xFF) as f64 / 255.0,
        ]);
    }
    if let Some(arr) = v.as_array() {
        if arr.len() >= 3 {
            let (r, g, b) = (arr[0].as_f64()?, arr[1].as_f64()?, arr[2].as_f64()?);
            if r > 1.0 || g > 1.0 || b > 1.0 {
                return Some([
                    (r / 255.0).clamp(0.0, 1.0),
                    (g / 255.0).clamp(0.0, 1.0),
                    (b / 255.0).clamp(0.0, 1.0),
                ]);
            }
            return Some([r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]);
        }
    }
    None
}

pub fn color_to_hex(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        let clean = s.trim().trim_start_matches('#');
        if clean.len() == 6 && u32::from_str_radix(clean, 16).is_ok() {
            return Some(format!("#{}", clean.to_ascii_uppercase()));
        }
    }
    let [r, g, b] = color_to_rgb_floats(v)?;
    let ri = (r.clamp(0.0, 1.0) * 255.0).round() as u32;
    let gi = (g.clamp(0.0, 1.0) * 255.0).round() as u32;
    let bi = (b.clamp(0.0, 1.0) * 255.0).round() as u32;
    Some(format!("#{ri:02X}{gi:02X}{bi:02X}"))
}

pub fn color_to_u32_packed(v: &Value) -> Option<u32> {
    if let Some(obj) = v.as_object() {
        if let Some(inner) = obj.get("Color3uint8") {
            if let Some(arr) = inner.as_array() {
                if arr.len() >= 3 {
                    let ri = arr[0].as_f64()?.clamp(0.0, 255.0).round() as u32;
                    let gi = arr[1].as_f64()?.clamp(0.0, 255.0).round() as u32;
                    let bi = arr[2].as_f64()?.clamp(0.0, 255.0).round() as u32;
                    return Some(0xFF00_0000 | (ri << 16) | (gi << 8) | bi);
                }
            }
            return color_to_u32_packed(inner);
        }
    }
    if let Some(n) = v.as_u64() {
        return Some(0xFF00_0000 | (n as u32 & 0x00FF_FFFF));
    }
    let [r, g, b] = color_to_rgb_floats(v)?;
    let ri = (r.clamp(0.0, 1.0) * 255.0).round() as u32;
    let gi = (g.clamp(0.0, 1.0) * 255.0).round() as u32;
    let bi = (b.clamp(0.0, 1.0) * 255.0).round() as u32;
    Some(0xFF00_0000 | (ri << 16) | (gi << 8) | bi)
}

fn unwrap_rojo_prop_for_ui(key: &str, v: &Value) -> Value {
    if let Some(obj) = v.as_object() {
        if obj.len() == 1 {
            if let Some(inner) = obj.get("Vector3") {
                if let Some(xyz) = parse_vec3(Some(inner)) {
                    return json!(xyz);
                }
            }
            if let Some(inner) = obj.get("Enum") {
                return inner.clone();
            }
            if let Some(inner) = obj.get("Content") {
                return inner.clone();
            }
            if obj.contains_key("CFrame") {
                if let Some(cf) = parse_cframe_value(v) {
                    return cframe_to_flat_json(cf);
                }
            }
            if obj.contains_key("Color3") || obj.contains_key("Color3uint8") {
                if let Some(hex) = color_to_hex(v) {
                    return json!(hex);
                }
            }
        }
    }
    if matches!(
        key,
        "Color"
            | "Color3"
            | "Color3uint8"
            | "Ambient"
            | "OutdoorAmbient"
            | "FogColor"
            | "Decay"
            | "TextColor3"
            | "BackgroundColor3"
            | "BorderColor3"
            | "ImageColor3"
    ) {
        if let Some(hex) = color_to_hex(v) {
            return json!(hex);
        }
    }
    v.clone()
}

/// Normalize an instance's `properties` map for the workspace API / 3D Viewport UI
/// (extracting `Position` & `Rotation` from `CFrame`, `#RRGGBB` hex colors, flat `Vector3`s).
pub fn normalize_instance_properties(
    _class_name: &str,
    props: &BTreeMap<String, Value>,
) -> Map<String, Value> {
    let mut out = Map::new();
    for (k, v) in props {
        out.insert(k.clone(), unwrap_rojo_prop_for_ui(k, v));
    }
    if let Some(cf_val) = props.get("CFrame") {
        if let Some(cf) = parse_cframe_value(cf_val) {
            out.insert("CFrame".into(), cframe_to_flat_json(cf));
            out.entry("Position".to_string())
                .or_insert_with(|| json!([cf[0], cf[1], cf[2]]));
            out.entry("Rotation".to_string())
                .or_insert_with(|| json!(cframe_to_euler_deg(cf)));
        }
    }
    if !out.contains_key("Color") {
        if let Some(c3) = props.get("Color3uint8") {
            if let Some(hex) = color_to_hex(c3) {
                out.insert("Color".into(), json!(hex));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Lighting extraction & mutation
// ---------------------------------------------------------------------------

pub fn extract_lighting_from_tree(services: &[Node]) -> Value {
    let lighting_node = services
        .iter()
        .find(|s| s.name == "Lighting" || s.class == "Lighting");
    let props = lighting_node.map(|n| &n.props);
    let f_prop = |key: &str, default: f64| -> f64 {
        props
            .and_then(|p| p.get(key))
            .and_then(|v| v.as_f64())
            .unwrap_or(default)
    };
    let color_prop = |key: &str, default: &str| -> String {
        props
            .and_then(|p| p.get(key))
            .and_then(color_to_hex)
            .unwrap_or_else(|| default.to_string())
    };
    let tech = props
        .and_then(|p| p.get("Technology"))
        .and_then(|v| {
            v.as_str()
                .map(|s| s.strip_prefix("Enum.Technology.").unwrap_or(s).to_string())
                .or_else(|| {
                    v.get("Enum")
                        .and_then(|e| e.as_str())
                        .map(|s| s.strip_prefix("Enum.Technology.").unwrap_or(s).to_string())
                })
        })
        .unwrap_or_else(|| "ShadowMap".to_string());

    let atmosphere = lighting_node
        .and_then(|n| {
            n.children
                .iter()
                .find(|c| c.class == "Atmosphere" || c.name == "Atmosphere")
        })
        .map(|atmo| {
            let ap = &atmo.props;
            let af = |k: &str, d: f64| ap.get(k).and_then(|v| v.as_f64()).unwrap_or(d);
            let ac = |k: &str, d: &str| {
                ap.get(k)
                    .and_then(color_to_hex)
                    .unwrap_or_else(|| d.to_string())
            };
            json!({
                "density": af("Density", 0.3),
                "offset": af("Offset", 0.25),
                "color": ac("Color", "#C7C7CC"),
                "decay": ac("Decay", "#6A7185"),
                "glare": af("Glare", 0.0),
                "haze": af("Haze", 0.0),
            })
        });

    json!({
        "clock_time": f_prop("ClockTime", 14.0),
        "brightness": f_prop("Brightness", 2.0),
        "ambient": color_prop("Ambient", "#464650"),
        "outdoor_ambient": color_prop("OutdoorAmbient", "#808080"),
        "fog_end": f_prop("FogEnd", 10000.0),
        "fog_start": f_prop("FogStart", 0.0),
        "fog_color": color_prop("FogColor", "#C0C0C0"),
        "technology": tech,
        "atmosphere": atmosphere,
    })
}

// ---------------------------------------------------------------------------
// Project Mesh Scanner (assets/meshes/*.obj + manifest.json)
// ---------------------------------------------------------------------------

fn pct_encode_query_val(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn inspect_obj_metrics(obj_path: &Path) -> ([f64; 3], u64) {
    let Ok(meta) = std::fs::symlink_metadata(obj_path) else {
        return ([1.0, 1.0, 1.0], 0);
    };
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_MESH_BYTES {
        return ([1.0, 1.0, 1.0], 0);
    }
    let Ok(file) = std::fs::File::open(obj_path) else {
        return ([1.0, 1.0, 1.0], 0);
    };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut min_v = [f64::INFINITY; 3];
    let mut max_v = [f64::NEG_INFINITY; 3];
    let mut any_v = false;
    let mut tris = 0u64;

    loop {
        line.clear();
        let Ok(n) = reader.read_line(&mut line) else {
            break;
        };
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("v ") {
            let mut nums = rest
                .split_whitespace()
                .filter_map(|s| s.parse::<f64>().ok().filter(|v| v.is_finite()));
            if let (Some(x), Some(y), Some(z)) = (nums.next(), nums.next(), nums.next()) {
                any_v = true;
                min_v[0] = min_v[0].min(x);
                min_v[1] = min_v[1].min(y);
                min_v[2] = min_v[2].min(z);
                max_v[0] = max_v[0].max(x);
                max_v[1] = max_v[1].max(y);
                max_v[2] = max_v[2].max(z);
            }
        } else if let Some(rest) = trimmed.strip_prefix("f ") {
            let verts = rest.split_whitespace().count();
            if verts >= 3 {
                tris += (verts - 2) as u64;
            }
        }
    }

    let size = if any_v {
        [
            ((max_v[0] - min_v[0]).abs().max(0.01) * 1000.0).round() / 1000.0,
            ((max_v[1] - min_v[1]).abs().max(0.01) * 1000.0).round() / 1000.0,
            ((max_v[2] - min_v[2]).abs().max(0.01) * 1000.0).round() / 1000.0,
        ]
    } else {
        [1.0, 1.0, 1.0]
    };
    (size, tris)
}

pub fn scan_project_meshes(dir: &Path, project_id: &str) -> Vec<Value> {
    let Ok(meshes_dir) = safe_project_path(dir, "assets/meshes") else {
        return Vec::new();
    };
    let Ok(dir_meta) = std::fs::symlink_metadata(&meshes_dir) else {
        return Vec::new();
    };
    if dir_meta.file_type().is_symlink() || !dir_meta.is_dir() {
        return Vec::new();
    }
    let manifest_val: Value = safe_project_path(dir, "assets/meshes/manifest.json")
        .ok()
        .filter(|p| {
            std::fs::symlink_metadata(p)
                .is_ok_and(|m| !m.file_type().is_symlink() && m.is_file() && m.len() <= MAX_MANIFEST_BYTES)
        })
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));

    let mut obj_files: Vec<String> = std::fs::read_dir(&meshes_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let ft = e.file_type().ok()?;
            if ft.is_symlink() || !ft.is_file() {
                return None;
            }
            let p = e.path();
            if p.extension()
                .and_then(|ex| ex.to_str())
                .is_some_and(|ex| ex.eq_ignore_ascii_case("obj"))
            {
                Some(e.file_name().to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    obj_files.sort();

    let encoded_id = pct_encode_query_val(project_id);
    let mut out = Vec::with_capacity(obj_files.len());
    for fname in obj_files {
        let stem = fname
            .strip_suffix(".obj")
            .or_else(|| fname.strip_suffix(".OBJ"))
            .unwrap_or(&fname)
            .to_string();
        let obj_rel = format!("assets/meshes/{fname}");
        let obj_url = format!(
            "/api/projects/{encoded_id}/mesh-asset?path={}",
            pct_encode_query_val(&obj_rel)
        );

        let manifest_entry = manifest_val.as_object().and_then(|m| {
            m.get(&stem).or_else(|| {
                m.values().find(|v| {
                    v.get("mesh").and_then(|x| x.as_str()) == Some(fname.as_str())
                        || v.get("glow_mesh").and_then(|x| x.as_str()) == Some(fname.as_str())
                })
            })
        });

        let is_glow = manifest_entry
            .and_then(|e| e.get("glow_mesh").and_then(|x| x.as_str()))
            == Some(fname.as_str());

        let tex_candidate = manifest_entry
            .and_then(|e| {
                let key = if is_glow { "glow_texture" } else { "texture" };
                e.get(key).and_then(|x| x.as_str()).map(String::from)
            })
            .or_else(|| {
                let png = format!("{stem}.png");
                safe_project_path(dir, &format!("assets/meshes/{png}"))
                    .ok()
                    .filter(|p| {
                        std::fs::symlink_metadata(p)
                            .is_ok_and(|m| !m.file_type().is_symlink() && m.is_file())
                    })
                    .map(|_| png)
            });

        let texture_path = tex_candidate.and_then(|tex| {
            if tex.contains('\\') || tex.contains('\0') {
                return None;
            }
            let rel = format!("assets/meshes/{tex}");
            safe_project_path(dir, &rel)
                .ok()
                .filter(|p| {
                    std::fs::symlink_metadata(p)
                        .is_ok_and(|m| !m.file_type().is_symlink() && m.is_file())
                })
                .map(|_| rel)
        });
        let texture_url = texture_path.as_ref().map(|tp| {
            format!(
                "/api/projects/{encoded_id}/mesh-asset?path={}",
                pct_encode_query_val(tp)
            )
        });

        let (computed_size, computed_tris) = inspect_obj_metrics(&meshes_dir.join(&fname));
        let size = if !is_glow {
            manifest_entry
                .and_then(|e| parse_vec3(e.get("size")))
                .unwrap_or(computed_size)
        } else {
            computed_size
        };
        let tris = manifest_entry
            .and_then(|e| {
                if is_glow {
                    e.get("glow_tris").and_then(|v| v.as_u64())
                } else {
                    e.get("solid_tris")
                        .or_else(|| e.get("tris"))
                        .and_then(|v| v.as_u64())
                }
            })
            .unwrap_or(computed_tris);

        out.push(json!({
            "name": stem,
            "obj_url": obj_url,
            "texture_url": texture_url,
            "size": size,
            "tris": tris,
            "obj_path": obj_rel,
            "texture_path": texture_path,
        }));
    }
    out
}

// ---------------------------------------------------------------------------
// Declarative Scene Model (`scene/workspace.model.json`) + Undo + Auto-Wire
// ---------------------------------------------------------------------------

fn default_workspace_model() -> Value {
    json!({
        "className": "Workspace",
        "properties": {
            "Gravity": 196.2
        },
        "children": []
    })
}

fn load_workspace_model(dir: &Path) -> Result<(Value, Option<String>), String> {
    let full = safe_project_path(dir, SCENE_MODEL_REL)?;
    let meta = match std::fs::symlink_metadata(&full) {
        Ok(m) => m,
        Err(_) => return Ok((default_workspace_model(), None)),
    };
    if meta.file_type().is_symlink() {
        return Err(format!("{SCENE_MODEL_REL} must not be a symlink"));
    }
    if !meta.is_file() {
        return Ok((default_workspace_model(), None));
    }
    if meta.len() > MAX_SCENE_MODEL_BYTES {
        return Err(format!("{SCENE_MODEL_REL} exceeds 16 MB limit"));
    }
    let raw = std::fs::read_to_string(&full)
        .map_err(|e| format!("read {SCENE_MODEL_REL}: {e}"))?;
    let mut val: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("{SCENE_MODEL_REL} is invalid JSON: {e}"))?;
    if !val.is_object() {
        return Err(format!("{SCENE_MODEL_REL} root must be a JSON object"));
    }
    normalize_model_node_keys(&mut val);
    Ok((val, Some(raw)))
}

/// Canonicalize `.model.json` keys (`className`, `name`, `properties`, `children`) while preserving contents.
fn normalize_model_node_keys(v: &mut Value) {
    let Some(obj) = v.as_object_mut() else {
        return;
    };
    if !obj.contains_key("className") {
        if let Some(c) = obj.remove("ClassName") {
            obj.insert("className".into(), c);
        }
    } else {
        obj.remove("ClassName");
    }
    if !obj.contains_key("name") {
        if let Some(n) = obj.remove("Name") {
            obj.insert("name".into(), n);
        }
    } else {
        obj.remove("Name");
    }
    if !obj.contains_key("properties") {
        if let Some(p) = obj.remove("Properties") {
            obj.insert("properties".into(), p);
        }
    } else {
        obj.remove("Properties");
    }
    if !obj.contains_key("children") {
        if let Some(c) = obj.remove("Children") {
            obj.insert("children".into(), c);
        }
    } else {
        obj.remove("Children");
    }
    if let Some(kids) = obj.get_mut("children").and_then(|c| c.as_array_mut()) {
        for kid in kids.iter_mut() {
            normalize_model_node_keys(kid);
        }
    }
}

fn validate_workspace_path(raw: &str) -> Result<Vec<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.contains('\0')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.chars().any(|c| c.is_control())
    {
        return Err(format!("invalid scene instance path '{raw}'"));
    }
    let segs: Vec<&str> = trimmed.split('.').collect();
    if segs.len() < 2
        || segs.len() > MAX_SCENE_PATH_DEPTH + 1
        || segs[0] != "Workspace"
        || segs
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > MAX_SCENE_SEG_LEN || *s == "." || *s == "..")
    {
        return Err(format!(
            "scene instance path '{raw}' must start with 'Workspace.' and have 1..={MAX_SCENE_PATH_DEPTH} valid segments"
        ));
    }
    Ok(segs[1..].iter().map(|s| (*s).to_string()).collect())
}

fn convert_props_for_rojo_storage(
    existing: Option<&Map<String, Value>>,
    patch: &Map<String, Value>,
) -> Map<String, Value> {
    let mut out = existing.cloned().unwrap_or_default();

    let mut pending_pos: Option<[f64; 3]> = None;
    let mut pending_rot: Option<[f64; 3]> = None;
    let mut explicit_cframe = false;

    for (k, v) in patch {
        match k.as_str() {
            "Position" => {
                if let Some(p) = parse_vec3(Some(v)) {
                    pending_pos = Some(p);
                }
            }
            "Rotation" => {
                if let Some(r) = parse_vec3(Some(v)) {
                    pending_rot = Some(r);
                }
            }
            "CFrame" => {
                if let Some(cf) = parse_cframe_value(v) {
                    out.insert("CFrame".into(), cframe_to_rojo_json(cf));
                    explicit_cframe = true;
                }
            }
            "Size" | "size" => {
                if let Some(sz) = parse_vec3(Some(v)) {
                    out.insert("Size".into(), json!(sz));
                } else {
                    out.insert("Size".into(), v.clone());
                }
            }
            "Color" => {
                if let Some([r, g, b]) = color_to_rgb_floats(v) {
                    let round4 = |x: f64| (x * 10_000.0).round() / 10_000.0;
                    out.insert("Color".into(), json!([round4(r), round4(g), round4(b)]));
                } else {
                    out.insert("Color".into(), v.clone());
                }
            }
            "Material" => {
                let s = v
                    .as_str()
                    .or_else(|| v.get("Enum").and_then(|e| e.as_str()))
                    .map(|raw| raw.trim().strip_prefix("Enum.Material.").unwrap_or(raw.trim()));
                if let Some(mat) = s {
                    out.insert("Material".into(), json!(mat));
                } else {
                    out.insert("Material".into(), v.clone());
                }
            }
            "Shape" | "shape" => {
                let s = v
                    .as_str()
                    .or_else(|| v.get("Enum").and_then(|e| e.as_str()))
                    .map(|raw| raw.trim().strip_prefix("Enum.PartType.").unwrap_or(raw.trim()));
                if let Some(sh) = s {
                    out.insert("Shape".into(), json!(sh));
                } else {
                    out.insert("Shape".into(), v.clone());
                }
            }
            "MeshId" | "TextureID" | "TextureId" | "SoundId" | "Texture" | "Image" => {
                let s = v
                    .as_str()
                    .or_else(|| v.get("Content").and_then(|c| c.as_str()));
                if let Some(url) = s {
                    out.insert(k.clone(), json!(url));
                } else {
                    out.insert(k.clone(), v.clone());
                }
            }
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }

    if !explicit_cframe && (pending_pos.is_some() || pending_rot.is_some()) {
        let cur_cf = out
            .get("CFrame")
            .and_then(parse_cframe_value)
            .unwrap_or([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        let pos = pending_pos.unwrap_or([cur_cf[0], cur_cf[1], cur_cf[2]]);
        let rot = pending_rot.unwrap_or_else(|| cframe_to_euler_deg(cur_cf));
        out.insert("CFrame".into(), cframe_to_rojo_json(cframe_from_pos_rot(pos, rot)));
        out.remove("Position");
        out.remove("Rotation");
    }

    out
}

fn upsert_model_node(
    root: &mut Value,
    segs: &[String],
    class_name: Option<&str>,
    props_patch: Option<&Map<String, Value>>,
) -> Result<(), String> {
    let mut cur = root;
    for (idx, seg) in segs.iter().enumerate() {
        let is_leaf = idx + 1 == segs.len();
        let obj = cur
            .as_object_mut()
            .ok_or_else(|| "corrupt scene model: node must be a JSON object".to_string())?;
        let kids = obj
            .entry("children")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| "corrupt scene model: 'children' must be a JSON array".to_string())?;
        let existing_idx = kids
            .iter()
            .position(|c| c.get("name").and_then(|n| n.as_str()) == Some(seg.as_str()));
        let target_idx = match existing_idx {
            Some(i) => i,
            None => {
                let cls = if is_leaf {
                    class_name.unwrap_or("Part")
                } else {
                    "Folder"
                };
                kids.push(json!({
                    "name": seg,
                    "className": cls,
                    "properties": {},
                    "children": []
                }));
                kids.len() - 1
            }
        };
        cur = &mut kids[target_idx];
        if is_leaf {
            let leaf_obj = cur
                .as_object_mut()
                .ok_or_else(|| "corrupt scene model: leaf node must be a JSON object".to_string())?;
            if let Some(cls) = class_name {
                leaf_obj.insert("className".into(), json!(cls));
            }
            if let Some(patch) = props_patch {
                let merged = convert_props_for_rojo_storage(
                    leaf_obj.get("properties").and_then(|p| p.as_object()),
                    patch,
                );
                leaf_obj.insert("properties".into(), Value::Object(merged));
            }
        }
    }
    Ok(())
}

fn delete_model_node(root: &mut Value, segs: &[String]) -> Result<(), String> {
    let mut cur = root;
    for seg in &segs[..segs.len() - 1] {
        let kids = cur
            .get_mut("children")
            .and_then(|c| c.as_array_mut())
            .ok_or_else(|| format!("parent path segment '{seg}' not found"))?;
        cur = kids
            .iter_mut()
            .find(|c| c.get("name").and_then(|n| n.as_str()) == Some(seg.as_str()))
            .ok_or_else(|| format!("parent path segment '{seg}' not found"))?;
    }
    let leaf = &segs[segs.len() - 1];
    let kids = cur
        .get_mut("children")
        .and_then(|c| c.as_array_mut())
        .ok_or_else(|| format!("instance '{leaf}' not found"))?;
    let pos = kids
        .iter()
        .position(|c| c.get("name").and_then(|n| n.as_str()) == Some(leaf.as_str()))
        .ok_or_else(|| format!("instance 'Workspace.{}' not found", segs.join(".")))?;
    kids.remove(pos);
    Ok(())
}

fn offset_node_cframe(node: &mut Value, offset: [f64; 3]) {
    if let Some(props) = node.get_mut("properties").and_then(|p| p.as_object_mut()) {
        if let Some(cf_val) = props.get("CFrame").cloned() {
            if let Some(mut cf) = parse_cframe_value(&cf_val) {
                cf[0] += offset[0];
                cf[1] += offset[1];
                cf[2] += offset[2];
                props.insert("CFrame".into(), cframe_to_rojo_json(cf));
            }
        } else if let Some(pos) = parse_vec3(props.get("Position")) {
            let new_pos = [pos[0] + offset[0], pos[1] + offset[1], pos[2] + offset[2]];
            props.insert("CFrame".into(), cframe_to_rojo_json(cframe_from_pos_rot(new_pos, [0.0, 0.0, 0.0])));
            props.remove("Position");
        } else {
            props.insert(
                "CFrame".into(),
                cframe_to_rojo_json(cframe_from_pos_rot(offset, [0.0, 0.0, 0.0])),
            );
        }
    }
}

fn duplicate_model_node(
    root: &mut Value,
    src_segs: &[String],
    target_segs: Option<&[String]>,
    explicit_name: Option<&str>,
    offset: [f64; 3],
) -> Result<(), String> {
    // Locate source parent & clone source node
    let mut cur = &*root;
    for seg in &src_segs[..src_segs.len() - 1] {
        let kids = cur
            .get("children")
            .and_then(|c| c.as_array())
            .ok_or_else(|| format!("instance 'Workspace.{}' not found", src_segs.join(".")))?;
        cur = kids
            .iter()
            .find(|c| c.get("name").and_then(|n| n.as_str()) == Some(seg.as_str()))
            .ok_or_else(|| format!("instance 'Workspace.{}' not found", src_segs.join(".")))?;
    }
    let src_leaf = &src_segs[src_segs.len() - 1];
    let src_kids = cur
        .get("children")
        .and_then(|c| c.as_array())
        .ok_or_else(|| format!("instance 'Workspace.{}' not found", src_segs.join(".")))?;
    let mut cloned = src_kids
        .iter()
        .find(|c| c.get("name").and_then(|n| n.as_str()) == Some(src_leaf.as_str()))
        .cloned()
        .ok_or_else(|| format!("instance 'Workspace.{}' not found", src_segs.join(".")))?;

    offset_node_cframe(&mut cloned, offset);

    // Determine target parent & new leaf name
    let (parent_segs, desired_name): (&[String], Option<String>) = match target_segs {
        Some(t) => (&t[..t.len() - 1], Some(t[t.len() - 1].clone())),
        None => (&src_segs[..src_segs.len() - 1], explicit_name.map(String::from)),
    };

    let mut dst_parent = root;
    for seg in parent_segs {
        let obj = dst_parent.as_object_mut().ok_or("invalid parent")?;
        let kids = obj
            .entry("children")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("children must be array")?;
        let idx = match kids
            .iter()
            .position(|c| c.get("name").and_then(|n| n.as_str()) == Some(seg.as_str()))
        {
            Some(i) => i,
            None => {
                kids.push(json!({
                    "name": seg,
                    "className": "Folder",
                    "properties": {},
                    "children": []
                }));
                kids.len() - 1
            }
        };
        dst_parent = &mut kids[idx];
    }

    let kids = dst_parent
        .as_object_mut()
        .ok_or("invalid destination parent")?
        .entry("children")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("children must be array")?;

    let final_name = match desired_name {
        Some(n) => {
            if kids.iter().any(|c| c.get("name").and_then(|x| x.as_str()) == Some(n.as_str())) {
                return Err(format!("duplicate target '{n}' already exists"));
            }
            n
        }
        None => {
            let mut idx = 1usize;
            loop {
                let cand = if idx == 1 {
                    format!("{src_leaf}_Copy")
                } else {
                    format!("{src_leaf}_Copy{idx}")
                };
                if !kids
                    .iter()
                    .any(|c| c.get("name").and_then(|x| x.as_str()) == Some(cand.as_str()))
                {
                    break cand;
                }
                idx += 1;
            }
        }
    };

    if let Some(obj) = cloned.as_object_mut() {
        obj.insert("name".into(), json!(final_name));
    }
    kids.push(cloned);
    Ok(())
}

fn ensure_rojo_autowire(rojo: &mut Value) -> bool {
    let mut changed = false;
    let Some(root_obj) = rojo.as_object_mut() else {
        return false;
    };
    let tree = root_obj
        .entry("tree")
        .or_insert_with(|| json!({ "$className": "DataModel" }));
    let Some(tree_obj) = tree.as_object_mut() else {
        return false;
    };
    if !tree_obj.contains_key("$className") {
        tree_obj.insert("$className".into(), json!("DataModel"));
        changed = true;
    }

    match tree_obj.get_mut("Workspace") {
        None => {
            tree_obj.insert("Workspace".into(), json!({ "$path": SCENE_MODEL_REL }));
            changed = true;
        }
        Some(ws) => {
            if let Some(ws_obj) = ws.as_object_mut() {
                if !ws_obj.contains_key("$path") {
                    ws_obj.insert("$path".into(), json!(SCENE_MODEL_REL));
                    ws_obj.remove("$className");
                    changed = true;
                }
            }
        }
    }

    if !tree_obj.contains_key("Lighting") {
        tree_obj.insert(
            "Lighting".into(),
            json!({
                "$className": "Lighting",
                "$ignoreUnknownInstances": true,
                "$properties": {
                    "ClockTime": 14.0,
                    "Brightness": 2.0,
                    "Ambient": [0.2745, 0.2745, 0.3137],
                    "OutdoorAmbient": [0.502, 0.502, 0.502],
                    "FogEnd": 10000.0
                }
            }),
        );
        changed = true;
    }
    changed
}

fn apply_lighting_mutation(rojo: &mut Value, op: &Value) -> Result<(), String> {
    ensure_rojo_autowire(rojo);
    let tree = rojo
        .get_mut("tree")
        .and_then(|t| t.as_object_mut())
        .ok_or("default.project.json has no tree object")?;
    let lighting = tree
        .entry("Lighting")
        .or_insert_with(|| json!({ "$className": "Lighting", "$ignoreUnknownInstances": true }))
        .as_object_mut()
        .ok_or("Lighting in default.project.json must be an object")?;
    lighting.entry("$className").or_insert_with(|| json!("Lighting"));
    lighting.entry("$ignoreUnknownInstances").or_insert_with(|| json!(true));

    let props_source = match op.get("properties") {
        None | Some(Value::Null) => op
            .as_object()
            .ok_or_else(|| "set_lighting op must be a JSON object".to_string())?,
        Some(Value::Object(m)) => m,
        Some(_) => return Err("set_lighting 'properties' must be a JSON object".to_string()),
    };

    let path = op.get("path").and_then(|v| v.as_str()).unwrap_or("Lighting");
    let round4 = |x: f64| (x * 10_000.0).round() / 10_000.0;

    let set_color = |target: &mut Map<String, Value>, key: &str, v: &Value| {
        if let Some([r, g, b]) = color_to_rgb_floats(v) {
            target.insert(key.into(), json!([round4(r), round4(g), round4(b)]));
        }
    };

    if path.eq_ignore_ascii_case("Lighting.Atmosphere") {
        let atmo = lighting
            .entry("Atmosphere")
            .or_insert_with(|| json!({ "$className": "Atmosphere", "$properties": {} }))
            .as_object_mut()
            .ok_or("Lighting.Atmosphere must be an object")?;
        atmo.entry("$className").or_insert_with(|| json!("Atmosphere"));
        let ap = atmo
            .entry("$properties")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("Atmosphere $properties must be an object")?;
        for (k, v) in props_source {
            match k.as_str() {
                "density" | "Density" => {
                    if let Some(n) = v.as_f64().filter(|x| x.is_finite()) {
                        ap.insert("Density".into(), json!(n.clamp(0.0, 1.0)));
                    }
                }
                "offset" | "Offset" => {
                    if let Some(n) = v.as_f64().filter(|x| x.is_finite()) {
                        ap.insert("Offset".into(), json!(n.clamp(0.0, 1.0)));
                    }
                }
                "glare" | "Glare" => {
                    if let Some(n) = v.as_f64().filter(|x| x.is_finite()) {
                        ap.insert("Glare".into(), json!(n.clamp(0.0, 10.0)));
                    }
                }
                "haze" | "Haze" => {
                    if let Some(n) = v.as_f64().filter(|x| x.is_finite()) {
                        ap.insert("Haze".into(), json!(n.clamp(0.0, 10.0)));
                    }
                }
                "color" | "Color" => set_color(ap, "Color", v),
                "decay" | "Decay" => set_color(ap, "Decay", v),
                _ => {}
            }
        }
        return Ok(());
    }

    let lprops = lighting
        .entry("$properties")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("Lighting $properties must be an object")?;

    let mut atmo_val: Option<&Value> = None;
    for (k, v) in props_source {
        match k.as_str() {
            "clock_time" | "ClockTime" => {
                if let Some(n) = v.as_f64().filter(|x| x.is_finite()) {
                    lprops.insert("ClockTime".into(), json!(n.clamp(0.0, 24.0)));
                }
            }
            "brightness" | "Brightness" => {
                if let Some(n) = v.as_f64().filter(|x| x.is_finite()) {
                    lprops.insert("Brightness".into(), json!(n.clamp(0.0, 100.0)));
                }
            }
            "ambient" | "Ambient" => set_color(lprops, "Ambient", v),
            "outdoor_ambient" | "OutdoorAmbient" => set_color(lprops, "OutdoorAmbient", v),
            "fog_end" | "FogEnd" => {
                if let Some(n) = v.as_f64().filter(|x| x.is_finite()) {
                    lprops.insert("FogEnd".into(), json!(n.clamp(0.0, 1_000_000.0)));
                }
            }
            "fog_start" | "FogStart" => {
                if let Some(n) = v.as_f64().filter(|x| x.is_finite()) {
                    lprops.insert("FogStart".into(), json!(n.clamp(0.0, 1_000_000.0)));
                }
            }
            "fog_color" | "FogColor" => set_color(lprops, "FogColor", v),
            "technology" | "Technology" => {
                if let Some(s) = v.as_str() {
                    let clean = s.strip_prefix("Enum.Technology.").unwrap_or(s);
                    lprops.insert("Technology".into(), json!(clean));
                }
            }
            "global_shadows" | "GlobalShadows" => {
                if let Some(b) = v.as_bool() {
                    lprops.insert("GlobalShadows".into(), json!(b));
                }
            }
            "exposure_compensation" | "ExposureCompensation" => {
                if let Some(n) = v.as_f64().filter(|x| x.is_finite()) {
                    lprops.insert("ExposureCompensation".into(), json!(n.clamp(-10.0, 10.0)));
                }
            }
            "atmosphere" | "Atmosphere" => {
                atmo_val = Some(v);
            }
            "op" | "path" | "class_name" | "properties" => {}
            other => {
                lprops.insert(other.to_string(), v.clone());
            }
        }
    }

    if let Some(av) = atmo_val.cloned() {
        if av.is_null() {
            lighting.remove("Atmosphere");
        } else if let Some(aobj) = av.as_object() {
            let atmo_op = json!({
                "op": "set_lighting",
                "path": "Lighting.Atmosphere",
                "properties": aobj
            });
            apply_lighting_mutation(rojo, &atmo_op)?;
        }
    }
    Ok(())
}

/// Apply declarative scene mutations (`upsert`, `delete`, `duplicate`, `set_lighting`, `undo`, `redo`),
/// atomically updating `scene/workspace.model.json` and auto-wiring `default.project.json`.
pub fn mutate_scene(dir: &Path, project_id: &str, body: &Value) -> Result<Value, String> {
    if !body.is_object() {
        return Err("scene/mutate request body must be a JSON object".to_string());
    }
    let _guard = SCENE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let m = projects::manifest(dir)?;
    let rojo_rel = m["rojoProject"]
        .as_str()
        .unwrap_or(projects::DEFAULT_ROJO_FILE)
        .to_string();
    let rojo_full = safe_project_path(dir, &rojo_rel)?;
    let rojo_meta = std::fs::symlink_metadata(&rojo_full)
        .map_err(|_| format!("{rojo_rel} not found in project"))?;
    if rojo_meta.file_type().is_symlink() || !rojo_meta.is_file() {
        return Err(format!("{rojo_rel} must be a regular non-symlink file"));
    }
    if rojo_meta.len() > MAX_SCENE_MODEL_BYTES {
        return Err(format!("{rojo_rel} exceeds 16 MB limit"));
    }
    let rojo_raw_before = std::fs::read_to_string(&rojo_full)
        .map_err(|_| format!("{rojo_rel} not found in project"))?;
    let mut rojo_val: Value = serde_json::from_str(&rojo_raw_before)
        .map_err(|e| format!("{rojo_rel} is not valid JSON: {e}"))?;

    let scene_full = safe_project_path(dir, SCENE_MODEL_REL)?;
    let (mut ws_model, ws_raw_before) = load_workspace_model(dir)?;
    let mut undo_store = load_undo_store(dir);

    let wants_undo = body.get("undo").and_then(|v| v.as_bool()) == Some(true)
        || body.get("ops").and_then(|v| v.as_array()).is_some_and(|ops| {
            ops.len() == 1 && ops[0].get("op").and_then(|o| o.as_str()) == Some("undo")
        });
    let wants_redo = body.get("redo").and_then(|v| v.as_bool()) == Some(true)
        || body.get("ops").and_then(|v| v.as_array()).is_some_and(|ops| {
            ops.len() == 1 && ops[0].get("op").and_then(|o| o.as_str()) == Some("redo")
        });

    if wants_undo && wants_redo {
        return Err("scene/mutate cannot specify both undo and redo".to_string());
    }

    if wants_undo {
        let cp = undo_store
            .undo_stack
            .pop()
            .ok_or_else(|| "nothing to undo".to_string())?;
        undo_store.redo_stack.push(SceneCheckpoint {
            scene_revision: undo_store.revision,
            workspace_model_raw: ws_raw_before.clone(),
            default_project_raw: Some(rojo_raw_before.clone()),
        });
        cap_checkpoints(&mut undo_store.redo_stack);
        undo_store.revision += 1;

        let mut modified_files = Vec::new();
        match &cp.workspace_model_raw {
            Some(prev_ws) => {
                if ws_raw_before.as_deref() != Some(prev_ws.as_str()) {
                    write_atomic_str(&scene_full, prev_ws)?;
                    modified_files.push(SCENE_MODEL_REL.to_string());
                }
            }
            None => {
                if scene_full.exists() {
                    std::fs::remove_file(&scene_full).map_err(|e| format!("remove {SCENE_MODEL_REL}: {e}"))?;
                    modified_files.push(SCENE_MODEL_REL.to_string());
                }
            }
        }
        if let Some(prev_rojo) = &cp.default_project_raw {
            if prev_rojo != &rojo_raw_before {
                write_atomic_str(&rojo_full, prev_rojo)?;
                modified_files.push(rojo_rel.clone());
            }
        }
        save_undo_store(dir, &undo_store)?;
        let bundle = projects::load_workspace_bundle(dir, project_id)?;
        return Ok(json!({
            "project_id": project_id,
            "scene_revision": undo_store.revision,
            "can_undo": !undo_store.undo_stack.is_empty(),
            "can_redo": !undo_store.redo_stack.is_empty(),
            "undone": true,
            "redone": false,
            "instances": bundle["instances"],
            "lighting": bundle["lighting"],
            "modified_files": modified_files,
        }));
    }

    if wants_redo {
        let cp = undo_store
            .redo_stack
            .pop()
            .ok_or_else(|| "nothing to redo".to_string())?;
        undo_store.undo_stack.push(SceneCheckpoint {
            scene_revision: undo_store.revision,
            workspace_model_raw: ws_raw_before.clone(),
            default_project_raw: Some(rojo_raw_before.clone()),
        });
        cap_checkpoints(&mut undo_store.undo_stack);
        undo_store.revision += 1;

        let mut modified_files = Vec::new();
        match &cp.workspace_model_raw {
            Some(next_ws) => {
                if ws_raw_before.as_deref() != Some(next_ws.as_str()) {
                    write_atomic_str(&scene_full, next_ws)?;
                    modified_files.push(SCENE_MODEL_REL.to_string());
                }
            }
            None => {
                if scene_full.exists() {
                    std::fs::remove_file(&scene_full).map_err(|e| format!("remove {SCENE_MODEL_REL}: {e}"))?;
                    modified_files.push(SCENE_MODEL_REL.to_string());
                }
            }
        }
        if let Some(next_rojo) = &cp.default_project_raw {
            if next_rojo != &rojo_raw_before {
                write_atomic_str(&rojo_full, next_rojo)?;
                modified_files.push(rojo_rel.clone());
            }
        }
        save_undo_store(dir, &undo_store)?;
        let bundle = projects::load_workspace_bundle(dir, project_id)?;
        return Ok(json!({
            "project_id": project_id,
            "scene_revision": undo_store.revision,
            "can_undo": !undo_store.undo_stack.is_empty(),
            "can_redo": !undo_store.redo_stack.is_empty(),
            "undone": false,
            "redone": true,
            "instances": bundle["instances"],
            "lighting": bundle["lighting"],
            "modified_files": modified_files,
        }));
    }

    let ops = body
        .get("ops")
        .and_then(|v| v.as_array())
        .filter(|a| !a.is_empty())
        .ok_or_else(|| "scene/mutate requires non-empty 'ops' array (or undo/redo: true)".to_string())?;
    if ops.len() > MAX_SCENE_OPS_PER_MUTATE {
        return Err(format!(
            "scene/mutate exceeds max {MAX_SCENE_OPS_PER_MUTATE} ops per request"
        ));
    }

    let mut touched_workspace = false;
    let mut touched_lighting = false;

    for (idx, op) in ops.iter().enumerate() {
        let kind = op
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("ops[{idx}] missing string 'op'"))?;
        match kind {
            "upsert" => {
                let raw_path = op
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("ops[{idx}] upsert requires 'path'"))?;
                let segs = validate_workspace_path(raw_path)?;
                let class_name = match op.get("class_name") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => {
                        let t = s.trim();
                        if t.is_empty()
                            || t.len() > 64
                            || t.chars().any(|c| c.is_control())
                        {
                            return Err(format!("ops[{idx}] invalid class_name '{s}'"));
                        }
                        Some(t)
                    }
                    Some(_) => return Err(format!("ops[{idx}] class_name must be a string")),
                };
                let props = match op.get("properties") {
                    None | Some(Value::Null) => None,
                    Some(Value::Object(m)) => Some(m),
                    Some(_) => {
                        return Err(format!("ops[{idx}] properties must be a JSON object"))
                    }
                };
                upsert_model_node(&mut ws_model, &segs, class_name, props)?;
                touched_workspace = true;
            }
            "delete" => {
                let raw_path = op
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("ops[{idx}] delete requires 'path'"))?;
                let segs = validate_workspace_path(raw_path)?;
                delete_model_node(&mut ws_model, &segs)?;
                touched_workspace = true;
            }
            "duplicate" => {
                let raw_path = op
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("ops[{idx}] duplicate requires 'path'"))?;
                let src_segs = validate_workspace_path(raw_path)?;
                let target_segs = match op.get("new_path").and_then(|v| v.as_str()) {
                    Some(np) => Some(validate_workspace_path(np)?),
                    None => None,
                };
                let explicit_name = op
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let offset = match op.get("offset") {
                    None | Some(Value::Null) => [4.0, 0.0, 4.0],
                    Some(v) => parse_vec3(Some(v)).ok_or_else(|| {
                        format!("ops[{idx}] duplicate offset must be a finite [x, y, z] Vector3")
                    })?,
                };
                duplicate_model_node(
                    &mut ws_model,
                    &src_segs,
                    target_segs.as_deref(),
                    explicit_name,
                    offset,
                )?;
                touched_workspace = true;
            }
            "set_lighting" => {
                apply_lighting_mutation(&mut rojo_val, op)?;
                touched_lighting = true;
            }
            other => {
                return Err(format!("ops[{idx}] unsupported op '{other}'"));
            }
        }
    }

    let autowired = ensure_rojo_autowire(&mut rojo_val);

    // Record checkpoint before writing
    undo_store.undo_stack.push(SceneCheckpoint {
        scene_revision: undo_store.revision,
        workspace_model_raw: ws_raw_before.clone(),
        default_project_raw: Some(rojo_raw_before.clone()),
    });
    cap_checkpoints(&mut undo_store.undo_stack);
    undo_store.redo_stack.clear();
    undo_store.revision += 1;

    let mut modified_files = Vec::new();
    if touched_workspace || !scene_full.is_file() {
        let ws_text = serde_json::to_string_pretty(&ws_model).map_err(|e| e.to_string())? + "\n";
        if ws_raw_before.as_deref() != Some(ws_text.as_str()) {
            write_atomic_str(&scene_full, &ws_text)?;
            modified_files.push(SCENE_MODEL_REL.to_string());
        }
    }
    if touched_lighting || autowired {
        let rojo_text =
            serde_json::to_string_pretty(&rojo_val).map_err(|e| e.to_string())? + "\n";
        if rojo_text != rojo_raw_before {
            write_atomic_str(&rojo_full, &rojo_text)?;
            modified_files.push(rojo_rel);
        }
    }
    save_undo_store(dir, &undo_store)?;

    let bundle = projects::load_workspace_bundle(dir, project_id)?;
    Ok(json!({
        "project_id": project_id,
        "scene_revision": undo_store.revision,
        "can_undo": !undo_store.undo_stack.is_empty(),
        "can_redo": !undo_store.redo_stack.is_empty(),
        "undone": false,
        "redone": false,
        "instances": bundle["instances"],
        "lighting": bundle["lighting"],
        "modified_files": modified_files,
    }))
}
