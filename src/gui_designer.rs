//! Visual ScreenGui / HUD Designer, Multi-Device Safe-Area Analyzer,
//! Deterministic `--!strict` Luau Compiler, and Rojo `.model.json` Emitter.
//!
//! Stores declarative `ScreenDoc` v1 files at `<project>/ui/<slug>.screen.json`
//! (or `<data_dir>/ui/<slug>.screen.json` / in-memory for LocalPlace).

use crate::luau_check::{self, AnalysisReport};
use crate::projects;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

pub const UI_DIR_REL: &str = "ui";
pub const MAX_GUI_DEPTH: usize = 32;
pub const MAX_GUI_NODES: usize = 500;
pub const MAX_SCREEN_JSON_BYTES: usize = 2 * 1024 * 1024;
const MAX_NODE_PROPS: usize = 64;
const UDIM_SCALE_MIN: f64 = -10.0;
const UDIM_SCALE_MAX: f64 = 10.0;
const UDIM_OFFSET_MIN: f64 = -16384.0;
const UDIM_OFFSET_MAX: f64 = 16384.0;
const ANCHOR_MIN: f64 = -10.0;
const ANCHOR_MAX: f64 = 10.0;
const RECT_COORD_MIN: f64 = -16384.0;
const RECT_COORD_MAX: f64 = 16384.0;
pub const TOPBAR_INSET_PX: f64 = 58.0;
pub const MOBILE_THUMBSTICK_ZONE_PX: f64 = 170.0;
pub const MIN_TAP_TARGET_PX: f64 = 44.0;

static TMP_SEQ: AtomicU64 = AtomicU64::new(1);

pub const ALLOWED_CLASSES: [&str; 12] = [
    "Frame",
    "TextLabel",
    "TextButton",
    "ImageLabel",
    "ImageButton",
    "ScrollingFrame",
    "UICorner",
    "UIStroke",
    "UIListLayout",
    "UIGridLayout",
    "UIAspectRatioConstraint",
    "UIPadding",
];

const MODIFIER_CLASSES: [&str; 6] = [
    "UICorner",
    "UIStroke",
    "UIListLayout",
    "UIGridLayout",
    "UIAspectRatioConstraint",
    "UIPadding",
];

const GUI_OBJECT_PROPS: [&str; 17] = [
    "Position",
    "Size",
    "AnchorPoint",
    "Visible",
    "ZIndex",
    "LayoutOrder",
    "BackgroundColor3",
    "BackgroundTransparency",
    "BorderSizePixel",
    "BorderColor3",
    "ClipsDescendants",
    "Active",
    "Selectable",
    "Rotation",
    "AutomaticSize",
    "SizeConstraint",
    "Name",
];

const TEXT_PROPS: [&str; 12] = [
    "Text",
    "TextColor3",
    "TextSize",
    "TextScaled",
    "TextWrapped",
    "Font",
    "TextXAlignment",
    "TextYAlignment",
    "TextTransparency",
    "TextStrokeTransparency",
    "TextStrokeColor3",
    "RichText",
];

const BUTTON_PROPS: [&str; 11] = [
    "AutoButtonColor",
    "Modal",
    "Callback",
    "callback",
    "CallbackName",
    "OnClick",
    "on_click",
    "ProductKey",
    "product_key",
    "ProductId",
    "product_id",
];

const IMAGE_PROPS: [&str; 13] = [
    "Image",
    "ImageColor3",
    "ImageTransparency",
    "ScaleType",
    "SliceCenter",
    "SliceScale",
    "TileSize",
    "HoverImage",
    "PressedImage",
    "AssetKey",
    "asset_key",
    "AssetPath",
    "asset_path",
];

const SCROLLING_FRAME_PROPS: [&str; 10] = [
    "CanvasSize",
    "AutomaticCanvasSize",
    "ScrollBarThickness",
    "ScrollBarImageColor3",
    "ScrollingDirection",
    "ElasticBehavior",
    "ScrollBarImage",
    "TopImage",
    "MidImage",
    "BottomImage",
];

const UI_CORNER_PROPS: [&str; 1] = ["CornerRadius"];
const UI_STROKE_PROPS: [&str; 5] = [
    "Color",
    "Thickness",
    "Transparency",
    "ApplyStrokeMode",
    "LineJoinMode",
];
const UI_LIST_LAYOUT_PROPS: [&str; 6] = [
    "FillDirection",
    "HorizontalAlignment",
    "VerticalAlignment",
    "Padding",
    "SortOrder",
    "Wraps",
];
const UI_GRID_LAYOUT_PROPS: [&str; 8] = [
    "CellSize",
    "CellPadding",
    "FillDirection",
    "FillDirectionMaxCells",
    "HorizontalAlignment",
    "VerticalAlignment",
    "SortOrder",
    "StartCorner",
];
const UI_ASPECT_RATIO_PROPS: [&str; 3] = ["AspectRatio", "AspectType", "DominantAxis"];
const UI_PADDING_PROPS: [&str; 4] = [
    "PaddingTop",
    "PaddingBottom",
    "PaddingLeft",
    "PaddingRight",
];

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenTheme {
    pub accent: String,
    pub surface: String,
    pub text: String,
}

impl Default for ScreenTheme {
    fn default() -> Self {
        Self {
            accent: "#3B82F6".to_string(),
            surface: "#0F172A".to_string(),
            text: "#F8FAFC".to_string(),
        }
    }
}

impl ScreenTheme {
    pub fn to_json(&self) -> Value {
        json!({
            "accent": self.accent,
            "surface": self.surface,
            "text": self.text,
        })
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        if v.is_null() {
            return Ok(Self::default());
        }
        let obj = v.as_object().ok_or("theme must be a JSON object")?;
        let def = Self::default();
        Ok(Self {
            accent: obj
                .get("accent")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or(def.accent),
            surface: obj
                .get("surface")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or(def.surface),
            text: obj
                .get("text")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or(def.text),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuiNode {
    pub id: String,
    pub name: String,
    pub class_name: String,
    pub props: BTreeMap<String, Value>,
    pub children: Vec<GuiNode>,
}

impl GuiNode {
    pub fn to_json(&self) -> Value {
        let mut props_map = Map::new();
        for (k, v) in &self.props {
            props_map.insert(k.clone(), v.clone());
        }
        json!({
            "id": self.id,
            "name": self.name,
            "class_name": self.class_name,
            "props": Value::Object(props_map),
            "children": self.children.iter().map(GuiNode::to_json).collect::<Vec<_>>(),
        })
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        let mut total = 0usize;
        Self::from_json_bounded(v, 0, &mut total)
    }

    fn from_json_bounded(v: &Value, depth: usize, total: &mut usize) -> Result<Self, String> {
        if depth > MAX_GUI_DEPTH {
            return Err(format!("GUI tree exceeds max depth of {MAX_GUI_DEPTH}"));
        }
        *total += 1;
        if *total > MAX_GUI_NODES {
            return Err(format!("GUI tree exceeds max node count of {MAX_GUI_NODES}"));
        }
        let obj = v.as_object().ok_or("GuiNode must be a JSON object")?;
        let id = match obj.get("id") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(_) => return Err("GuiNode.id must be a string".to_string()),
        };
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .ok_or("GuiNode missing 'name'")?
            .to_string();
        let class_name = obj
            .get("class_name")
            .or_else(|| obj.get("className"))
            .or_else(|| obj.get("class"))
            .and_then(Value::as_str)
            .ok_or("GuiNode missing 'class_name'")?
            .to_string();
        let mut props = BTreeMap::new();
        if let Some(p) = obj.get("props").or_else(|| obj.get("properties")) {
            let p_obj = p.as_object().ok_or("GuiNode.props must be a JSON object")?;
            if p_obj.len() > MAX_NODE_PROPS {
                return Err(format!("GuiNode exceeds max {MAX_NODE_PROPS} properties"));
            }
            for (k, val) in p_obj {
                props.insert(k.clone(), val.clone());
            }
        }
        let mut children = Vec::new();
        if let Some(ch) = obj.get("children") {
            let arr = ch.as_array().ok_or("GuiNode.children must be a JSON array")?;
            if *total + arr.len() > MAX_GUI_NODES {
                return Err(format!("GUI tree exceeds max node count of {MAX_GUI_NODES}"));
            }
            children.reserve(arr.len());
            for item in arr {
                children.push(Self::from_json_bounded(item, depth + 1, total)?);
            }
        }
        Ok(Self {
            id,
            name,
            class_name,
            props,
            children,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenDoc {
    pub version: u32,
    pub name: String,
    pub ignore_gui_inset: bool,
    pub reset_on_spawn: bool,
    pub theme: ScreenTheme,
    pub root: GuiNode,
}

impl ScreenDoc {
    pub fn to_json(&self) -> Value {
        json!({
            "version": self.version,
            "name": self.name,
            "ignore_gui_inset": self.ignore_gui_inset,
            "reset_on_spawn": self.reset_on_spawn,
            "theme": self.theme.to_json(),
            "root": self.root.to_json(),
        })
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        let obj = v.as_object().ok_or("ScreenDoc must be a JSON object")?;
        let version_u64 = obj
            .get("version")
            .and_then(Value::as_u64)
            .ok_or("ScreenDoc missing integer 'version'")?;
        let version = u32::try_from(version_u64)
            .map_err(|_| format!("unsupported ScreenDoc version {version_u64}; expected version 1"))?;
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .ok_or("ScreenDoc missing 'name'")?
            .to_string();
        let ignore_gui_inset = obj
            .get("ignore_gui_inset")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let reset_on_spawn = obj
            .get("reset_on_spawn")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let theme = match obj.get("theme") {
            Some(tv) => ScreenTheme::from_json(tv)?,
            None => ScreenTheme::default(),
        };
        let root_val = obj.get("root").ok_or("ScreenDoc missing 'root'")?;
        let root = GuiNode::from_json(root_val)?;
        Ok(Self {
            version,
            name,
            ignore_gui_inset,
            reset_on_spawn,
            theme,
            root,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DeviceViewport {
    pub id: &'static str,
    pub label: &'static str,
    pub width: f64,
    pub height: f64,
}

pub const DEVICES: [DeviceViewport; 3] = [
    DeviceViewport {
        id: "desktop",
        label: "Desktop 16:9",
        width: 1920.0,
        height: 1080.0,
    },
    DeviceViewport {
        id: "tablet",
        label: "Tablet 4:3",
        width: 1024.0,
        height: 768.0,
    },
    DeviceViewport {
        id: "mobile_landscape",
        label: "Mobile Landscape",
        width: 844.0,
        height: 390.0,
    },
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl RectBox {
    pub fn to_json(&self) -> Value {
        json!({
            "x": self.x,
            "y": self.y,
            "w": self.w,
            "h": self.h,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SafeAreaWarning {
    pub code: String,
    pub severity: String,
    pub device: String,
    pub node_id: String,
    pub node_name: String,
    pub class_name: String,
    pub message: String,
    pub bounds: RectBox,
}

impl SafeAreaWarning {
    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "severity": self.severity,
            "device": self.device,
            "node_id": self.node_id,
            "node_name": self.node_name,
            "class_name": self.class_name,
            "message": self.message,
            "bounds": self.bounds.to_json(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedNodeBounds {
    pub id: String,
    pub name: String,
    pub class_name: String,
    pub bounds: RectBox,
    pub visible: bool,
}

impl ResolvedNodeBounds {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "class_name": self.class_name,
            "bounds": self.bounds.to_json(),
            "visible": self.visible,
        })
    }
}

#[derive(Clone, Debug)]
pub struct DeviceAnalysis {
    pub device: String,
    pub label: String,
    pub width: f64,
    pub height: f64,
    pub topbar_inset: f64,
    pub safe_zones: Vec<Value>,
    pub warnings: Vec<SafeAreaWarning>,
    pub nodes: Vec<ResolvedNodeBounds>,
}

impl DeviceAnalysis {
    pub fn to_json(&self) -> Value {
        json!({
            "device": self.device,
            "label": self.label,
            "width": self.width,
            "height": self.height,
            "topbar_inset": self.topbar_inset,
            "safe_zones": self.safe_zones,
            "warnings": self.warnings.iter().map(SafeAreaWarning::to_json).collect::<Vec<_>>(),
            "nodes": self.nodes.iter().map(ResolvedNodeBounds::to_json).collect::<Vec<_>>(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct SafeAreaAnalysis {
    pub passed: bool,
    pub warning_count: usize,
    pub warnings: Vec<SafeAreaWarning>,
    pub devices: BTreeMap<String, DeviceAnalysis>,
}

impl SafeAreaAnalysis {
    pub fn to_json(&self) -> Value {
        let mut dev_map = Map::new();
        for (k, v) in &self.devices {
            dev_map.insert(k.clone(), v.to_json());
        }
        json!({
            "passed": self.passed,
            "warning_count": self.warning_count,
            "warnings": self.warnings.iter().map(SafeAreaWarning::to_json).collect::<Vec<_>>(),
            "devices": Value::Object(dev_map),
        })
    }
}

/// In-memory / `data_dir`-backed ScreenDoc store for LocalPlace / `/api/studio/gui`.
#[derive(Clone, Debug, Default)]
pub struct LocalGuiStore {
    data_dir: Option<PathBuf>,
    docs: BTreeMap<String, ScreenDoc>,
}

impl LocalGuiStore {
    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn open(data_dir: Option<&Path>) -> Self {
        let mut docs = BTreeMap::new();
        if let Some(dir) = data_dir {
            if let Ok(ui_dir) = projects::safe_join(dir, UI_DIR_REL) {
                let ui_ok = std::fs::symlink_metadata(&ui_dir)
                    .is_ok_and(|m| !m.file_type().is_symlink() && m.is_dir());
                if ui_ok {
                    if let Ok(entries) = std::fs::read_dir(&ui_dir) {
                        for entry in entries.flatten() {
                            let Ok(ft) = entry.file_type() else {
                                continue;
                            };
                            if ft.is_symlink() || !ft.is_file() {
                                continue;
                            }
                            let path = entry.path();
                            let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
                                continue;
                            };
                            let Some(slug) = fname.strip_suffix(".screen.json") else {
                                continue;
                            };
                            if validate_slug(slug).is_err() {
                                continue;
                            }
                            if entry
                                .metadata()
                                .is_ok_and(|m| m.len() <= MAX_SCREEN_JSON_BYTES as u64)
                            {
                                if let Ok(raw) = std::fs::read_to_string(&path) {
                                    if let Ok(doc) = parse_and_validate_screen_str(&raw) {
                                        docs.insert(slug.to_string(), doc);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Self {
            data_dir: data_dir.map(Path::to_path_buf),
            docs,
        }
    }

    pub fn list(&self) -> Vec<(String, ScreenDoc)> {
        self.docs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn get(&self, slug: &str) -> Result<Option<ScreenDoc>, String> {
        let clean = validate_slug(slug)?;
        Ok(self.docs.get(&clean).cloned())
    }

    pub fn save(&mut self, slug: &str, doc: ScreenDoc) -> Result<(String, ScreenDoc, Option<String>), String> {
        let clean = validate_slug(slug)?;
        let validated = validate_screen_doc(doc)?;
        let rel = if let Some(dir) = &self.data_dir {
            std::fs::create_dir_all(dir).map_err(|e| format!("create data dir: {e}"))?;
            let rel_path = format!("{UI_DIR_REL}/{clean}.screen.json");
            let full = projects::safe_join(dir, &rel_path)?;
            write_atomic_json(&full, &validated.to_json())?;
            Some(rel_path)
        } else {
            None
        };
        self.docs.insert(clean.clone(), validated.clone());
        Ok((clean, validated, rel))
    }
}

/// Validate a GUI slug strictly against CWE-22 path traversal & unsafe characters.
/// Rejects `.`, `..`, `/`, `\`, `%`, `:`, whitespace, control chars, or non-ASCII.
pub fn validate_slug(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err("gui slug must be 1..=64 characters".to_string());
    }
    if trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('%')
        || trimmed.contains('\0')
    {
        return Err(format!("invalid gui slug '{raw}': path traversal characters are forbidden"));
    }
    let bytes = trimmed.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        return Err(format!("invalid gui slug '{raw}': must start with an ASCII letter or digit"));
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
    {
        return Err(format!(
            "invalid gui slug '{raw}': only ASCII letters, digits, '-' and '_' are allowed"
        ));
    }
    Ok(trimmed.to_string())
}

fn write_atomic_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create folder: {e}"))?;
    }
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("screen");
    let tmp = path.with_file_name(format!(".{file_name}.openro-tmp.{}.{seq}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| format!("create temp for {}: {e}", path.display()))?;
    if let Err(e) = file.write_all(bytes).and_then(|_| file.flush()) {
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

fn write_atomic_json(path: &Path, val: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(val).map_err(|e| e.to_string())? + "\n";
    write_atomic_bytes(path, text.as_bytes())
}

fn is_prop_allowed(class_name: &str, prop: &str) -> bool {
    match class_name {
        "Frame" => GUI_OBJECT_PROPS.contains(&prop),
        "TextLabel" => GUI_OBJECT_PROPS.contains(&prop) || TEXT_PROPS.contains(&prop),
        "TextButton" => {
            GUI_OBJECT_PROPS.contains(&prop)
                || TEXT_PROPS.contains(&prop)
                || BUTTON_PROPS.contains(&prop)
        }
        "ImageLabel" => GUI_OBJECT_PROPS.contains(&prop) || IMAGE_PROPS.contains(&prop),
        "ImageButton" => {
            GUI_OBJECT_PROPS.contains(&prop)
                || IMAGE_PROPS.contains(&prop)
                || BUTTON_PROPS.contains(&prop)
        }
        "ScrollingFrame" => {
            GUI_OBJECT_PROPS.contains(&prop) || SCROLLING_FRAME_PROPS.contains(&prop)
        }
        "UICorner" => UI_CORNER_PROPS.contains(&prop),
        "UIStroke" => UI_STROKE_PROPS.contains(&prop),
        "UIListLayout" => UI_LIST_LAYOUT_PROPS.contains(&prop),
        "UIGridLayout" => UI_GRID_LAYOUT_PROPS.contains(&prop),
        "UIAspectRatioConstraint" => UI_ASPECT_RATIO_PROPS.contains(&prop),
        "UIPadding" => UI_PADDING_PROPS.contains(&prop),
        _ => false,
    }
}

fn canonical_prop_name(raw: &str) -> &str {
    match raw {
        "callback" | "on_click" | "OnClick" | "CallbackName" | "callback_name" => "Callback",
        "asset_key" => "AssetKey",
        "asset_path" => "AssetPath",
        "product_key" => "ProductKey",
        "product_id" => "ProductId",
        other => other,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct UDimVal {
    scale: f64,
    offset: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct UDim2Val {
    x: UDimVal,
    y: UDimVal,
}

fn clamp_udim(u: UDimVal) -> (UDimVal, bool) {
    let s = u.scale.clamp(UDIM_SCALE_MIN, UDIM_SCALE_MAX);
    let o = u.offset.clamp(UDIM_OFFSET_MIN, UDIM_OFFSET_MAX);
    let clamped = (s - u.scale).abs() > f64::EPSILON || (o - u.offset).abs() > f64::EPSILON;
    (UDimVal { scale: s, offset: o }, clamped)
}

fn clamp_udim2(u: UDim2Val) -> (UDim2Val, bool) {
    let (x, cx) = clamp_udim(u.x);
    let (y, cy) = clamp_udim(u.y);
    (UDim2Val { x, y }, cx || cy)
}

fn opt_finite_f64(v: Option<&Value>, field: &str) -> Result<Option<f64>, String> {
    match v {
        None => Ok(None),
        Some(raw) => {
            let n = raw
                .as_f64()
                .ok_or_else(|| format!("{field} must be a finite number"))?;
            if !n.is_finite() {
                return Err(format!("{field} must be finite"));
            }
            Ok(Some(n))
        }
    }
}

fn parse_udim_raw(v: &Value) -> Result<(UDimVal, bool), String> {
    let target = v.as_object().and_then(|o| o.get("UDim")).unwrap_or(v);
    if let Some(arr) = target.as_array() {
        if arr.len() == 2 {
            let s = arr[0].as_f64().ok_or("UDim scale must be numeric")?;
            let o = arr[1].as_f64().ok_or("UDim offset must be numeric")?;
            if s.is_finite() && o.is_finite() {
                return Ok(clamp_udim(UDimVal { scale: s, offset: o }));
            }
            return Err("UDim components must be finite".to_string());
        }
    }
    if let Some(obj) = target.as_object() {
        let s_opt = opt_finite_f64(
            obj.get("scale")
                .or_else(|| obj.get("Scale"))
                .or_else(|| obj.get("s")),
            "UDim.scale",
        )?;
        let o_opt = opt_finite_f64(
            obj.get("offset")
                .or_else(|| obj.get("Offset"))
                .or_else(|| obj.get("o")),
            "UDim.offset",
        )?;
        if s_opt.is_some() || o_opt.is_some() {
            return Ok(clamp_udim(UDimVal {
                scale: s_opt.unwrap_or(0.0),
                offset: o_opt.unwrap_or(0.0),
            }));
        }
    }
    if let Some(n) = target.as_f64() {
        if n.is_finite() {
            return Ok(clamp_udim(UDimVal {
                scale: 0.0,
                offset: n,
            }));
        }
    }
    Err("expected UDim as [scale, offset] or { scale, offset }".to_string())
}

fn parse_udim(v: &Value) -> Result<UDimVal, String> {
    parse_udim_raw(v).map(|(u, _)| u)
}

fn parse_udim2_raw(v: &Value) -> Result<(UDim2Val, bool), String> {
    let target = v.as_object().and_then(|o| o.get("UDim2")).unwrap_or(v);
    if let Some(arr) = target.as_array() {
        if arr.len() == 2 {
            let (x, cx) = parse_udim_raw(&arr[0])?;
            let (y, cy) = parse_udim_raw(&arr[1])?;
            return Ok((UDim2Val { x, y }, cx || cy));
        }
        if arr.len() == 4 {
            let xs = arr[0].as_f64().ok_or("UDim2[0] must be numeric")?;
            let xo = arr[1].as_f64().ok_or("UDim2[1] must be numeric")?;
            let ys = arr[2].as_f64().ok_or("UDim2[2] must be numeric")?;
            let yo = arr[3].as_f64().ok_or("UDim2[3] must be numeric")?;
            if xs.is_finite() && xo.is_finite() && ys.is_finite() && yo.is_finite() {
                return Ok(clamp_udim2(UDim2Val {
                    x: UDimVal {
                        scale: xs,
                        offset: xo,
                    },
                    y: UDimVal {
                        scale: ys,
                        offset: yo,
                    },
                }));
            }
            return Err("UDim2 components must be finite".to_string());
        }
    }
    if let Some(obj) = target.as_object() {
        if let (Some(xv), Some(yv)) = (
            obj.get("x").or_else(|| obj.get("X")),
            obj.get("y").or_else(|| obj.get("Y")),
        ) {
            let (x, cx) = parse_udim_raw(xv)?;
            let (y, cy) = parse_udim_raw(yv)?;
            return Ok((UDim2Val { x, y }, cx || cy));
        }
        let xs = opt_finite_f64(
            obj.get("xs")
                .or_else(|| obj.get("xScale"))
                .or_else(|| obj.get("x_scale")),
            "UDim2.xs",
        )?;
        let xo = opt_finite_f64(
            obj.get("xo")
                .or_else(|| obj.get("xOffset"))
                .or_else(|| obj.get("x_offset")),
            "UDim2.xo",
        )?;
        let ys = opt_finite_f64(
            obj.get("ys")
                .or_else(|| obj.get("yScale"))
                .or_else(|| obj.get("y_scale")),
            "UDim2.ys",
        )?;
        let yo = opt_finite_f64(
            obj.get("yo")
                .or_else(|| obj.get("yOffset"))
                .or_else(|| obj.get("y_offset")),
            "UDim2.yo",
        )?;
        if xs.is_some() || xo.is_some() || ys.is_some() || yo.is_some() {
            return Ok(clamp_udim2(UDim2Val {
                x: UDimVal {
                    scale: xs.unwrap_or(0.0),
                    offset: xo.unwrap_or(0.0),
                },
                y: UDimVal {
                    scale: ys.unwrap_or(0.0),
                    offset: yo.unwrap_or(0.0),
                },
            }));
        }
    }
    if let Some(s) = target.as_str() {
        let parts: Vec<f64> = s
            .split(',')
            .map(|p| p.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "invalid UDim2 string".to_string())?;
        if parts.len() == 4 && parts.iter().all(|n| n.is_finite()) {
            return Ok(clamp_udim2(UDim2Val {
                x: UDimVal {
                    scale: parts[0],
                    offset: parts[1],
                },
                y: UDimVal {
                    scale: parts[2],
                    offset: parts[3],
                },
            }));
        }
    }
    Err("expected UDim2 as [[xs, xo], [ys, yo]], [xs, xo, ys, yo], or { x, y }".to_string())
}

fn parse_udim2(v: &Value) -> Result<UDim2Val, String> {
    parse_udim2_raw(v).map(|(u, _)| u)
}

fn parse_vec2_raw(v: &Value) -> Result<([f64; 2], bool), String> {
    let target = v.as_object().and_then(|o| o.get("Vector2")).unwrap_or(v);
    let clamp_pair = |x: f64, y: f64| -> ([f64; 2], bool) {
        let cx = x.clamp(ANCHOR_MIN, ANCHOR_MAX);
        let cy = y.clamp(ANCHOR_MIN, ANCHOR_MAX);
        let changed = (cx - x).abs() > f64::EPSILON || (cy - y).abs() > f64::EPSILON;
        ([cx, cy], changed)
    };
    if let Some(arr) = target.as_array() {
        if arr.len() == 2 {
            let x = arr[0].as_f64().ok_or("Vector2 x must be numeric")?;
            let y = arr[1].as_f64().ok_or("Vector2 y must be numeric")?;
            if x.is_finite() && y.is_finite() {
                return Ok(clamp_pair(x, y));
            }
        }
    }
    if let Some(obj) = target.as_object() {
        if let (Some(x), Some(y)) = (
            obj.get("x").or_else(|| obj.get("X")).and_then(Value::as_f64),
            obj.get("y").or_else(|| obj.get("Y")).and_then(Value::as_f64),
        ) {
            if x.is_finite() && y.is_finite() {
                return Ok(clamp_pair(x, y));
            }
        }
    }
    Err("expected Vector2 as [x, y] or { x, y }".to_string())
}

fn parse_vec2(v: &Value) -> Result<[f64; 2], String> {
    parse_vec2_raw(v).map(|(xy, _)| xy)
}

fn parse_rect_raw(v: &Value) -> Result<([f64; 4], bool), String> {
    let target = v.as_object().and_then(|o| o.get("Rect")).unwrap_or(v);
    if let Some(arr) = target.as_array() {
        if arr.len() == 4 {
            let a = arr[0].as_f64().ok_or("Rect[0] must be numeric")?;
            let b = arr[1].as_f64().ok_or("Rect[1] must be numeric")?;
            let c = arr[2].as_f64().ok_or("Rect[2] must be numeric")?;
            let d = arr[3].as_f64().ok_or("Rect[3] must be numeric")?;
            if a.is_finite() && b.is_finite() && c.is_finite() && d.is_finite() {
                let ca = a.clamp(RECT_COORD_MIN, RECT_COORD_MAX);
                let cb = b.clamp(RECT_COORD_MIN, RECT_COORD_MAX);
                let cc = c.clamp(RECT_COORD_MIN, RECT_COORD_MAX);
                let cd = d.clamp(RECT_COORD_MIN, RECT_COORD_MAX);
                let changed = (ca - a).abs() > f64::EPSILON
                    || (cb - b).abs() > f64::EPSILON
                    || (cc - c).abs() > f64::EPSILON
                    || (cd - d).abs() > f64::EPSILON;
                return Ok(([ca, cb, cc, cd], changed));
            }
        }
    }
    Err("expected Rect as [minX, minY, maxX, maxY]".to_string())
}

fn parse_rect(v: &Value) -> Result<[f64; 4], String> {
    parse_rect_raw(v).map(|(r, _)| r)
}

fn parse_color_rgb_u8_raw(v: &Value) -> Result<([u8; 3], bool), String> {
    if let Some(s) = v.as_str() {
        let hex = s.trim();
        if let Some(h) = hex.strip_prefix('#') {
            if h.len() == 6 {
                let r = u8::from_str_radix(&h[0..2], 16).map_err(|_| "invalid hex color")?;
                let g = u8::from_str_radix(&h[2..4], 16).map_err(|_| "invalid hex color")?;
                let b = u8::from_str_radix(&h[4..6], 16).map_err(|_| "invalid hex color")?;
                return Ok(([r, g, b], false));
            }
            if h.len() == 3 {
                let r = u8::from_str_radix(&h[0..1], 16).map_err(|_| "invalid hex color")? * 17;
                let g = u8::from_str_radix(&h[1..2], 16).map_err(|_| "invalid hex color")? * 17;
                let b = u8::from_str_radix(&h[2..3], 16).map_err(|_| "invalid hex color")? * 17;
                return Ok(([r, g, b], false));
            }
        }
    }
    if let Some(obj) = v.as_object() {
        if let Some(inner) = obj.get("Color3") {
            if let Some(arr) = inner.as_array() {
                if arr.len() >= 3 {
                    let r = arr[0].as_f64().ok_or("Color3[0] must be numeric")?;
                    let g = arr[1].as_f64().ok_or("Color3[1] must be numeric")?;
                    let b = arr[2].as_f64().ok_or("Color3[2] must be numeric")?;
                    if !r.is_finite() || !g.is_finite() || !b.is_finite() {
                        return Err("Color3 components must be finite".to_string());
                    }
                    let clamped = !(0.0..=1.0).contains(&r)
                        || !(0.0..=1.0).contains(&g)
                        || !(0.0..=1.0).contains(&b);
                    return Ok((
                        [
                            (r.clamp(0.0, 1.0) * 255.0).round() as u8,
                            (g.clamp(0.0, 1.0) * 255.0).round() as u8,
                            (b.clamp(0.0, 1.0) * 255.0).round() as u8,
                        ],
                        clamped,
                    ));
                }
            }
            return parse_color_rgb_u8_raw(inner);
        }
        if let Some(inner) = obj.get("Color3uint8") {
            if let Some(arr) = inner.as_array() {
                if arr.len() >= 3 {
                    let r = arr[0].as_f64().ok_or("Color3uint8[0] must be numeric")?;
                    let g = arr[1].as_f64().ok_or("Color3uint8[1] must be numeric")?;
                    let b = arr[2].as_f64().ok_or("Color3uint8[2] must be numeric")?;
                    if !r.is_finite() || !g.is_finite() || !b.is_finite() {
                        return Err("Color3uint8 components must be finite".to_string());
                    }
                    let clamped = !(0.0..=255.0).contains(&r)
                        || !(0.0..=255.0).contains(&g)
                        || !(0.0..=255.0).contains(&b);
                    return Ok((
                        [
                            r.clamp(0.0, 255.0).round() as u8,
                            g.clamp(0.0, 255.0).round() as u8,
                            b.clamp(0.0, 255.0).round() as u8,
                        ],
                        clamped,
                    ));
                }
            }
            return parse_color_rgb_u8_raw(inner);
        }
    }
    if let Some(arr) = v.as_array() {
        if arr.len() >= 3 {
            let r = arr[0].as_f64().ok_or("Color[0] must be numeric")?;
            let g = arr[1].as_f64().ok_or("Color[1] must be numeric")?;
            let b = arr[2].as_f64().ok_or("Color[2] must be numeric")?;
            if !r.is_finite() || !g.is_finite() || !b.is_finite() {
                return Err("Color components must be finite".to_string());
            }
            let clamped = if r > 1.0 || g > 1.0 || b > 1.0 {
                !(0.0..=255.0).contains(&r)
                    || !(0.0..=255.0).contains(&g)
                    || !(0.0..=255.0).contains(&b)
            } else {
                r < 0.0 || g < 0.0 || b < 0.0
            };
            if let Some([rf, gf, bf]) = crate::scene::color_to_rgb_floats(v) {
                return Ok((
                    [
                        (rf.clamp(0.0, 1.0) * 255.0).round() as u8,
                        (gf.clamp(0.0, 1.0) * 255.0).round() as u8,
                        (bf.clamp(0.0, 1.0) * 255.0).round() as u8,
                    ],
                    clamped,
                ));
            }
        }
    }
    if let Some([r, g, b]) = crate::scene::color_to_rgb_floats(v) {
        return Ok((
            [
                (r.clamp(0.0, 1.0) * 255.0).round() as u8,
                (g.clamp(0.0, 1.0) * 255.0).round() as u8,
                (b.clamp(0.0, 1.0) * 255.0).round() as u8,
            ],
            false,
        ));
    }
    Err("expected Color3 as #RRGGBB, #RGB, [r, g, b], or { Color3: [r, g, b] }".to_string())
}

fn parse_color_rgb_u8(v: &Value) -> Result<[u8; 3], String> {
    parse_color_rgb_u8_raw(v).map(|(rgb, _)| rgb)
}

fn validate_safe_ident(raw: &str, field: &str) -> Result<(), String> {
    let s = raw.trim();
    if s.is_empty() || s.len() > 64 {
        return Err(format!("{field} must be 1..=64 characters"));
    }
    let b = s.as_bytes();
    if !(b[0].is_ascii_alphabetic() || b[0] == b'_') {
        return Err(format!("{field} '{s}' must start with an ASCII letter or '_'"));
    }
    if !b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_') {
        return Err(format!("{field} '{s}' must contain only ASCII letters, digits, or '_'"));
    }
    Ok(())
}

fn validate_enum_token(raw: &str, field: &str) -> Result<String, String> {
    let clean = raw.trim();
    let tail = clean.rsplit('.').next().unwrap_or(clean);
    if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(format!("invalid enum value for {field}: '{raw}'"));
    }
    Ok(tail.to_string())
}

fn clamp_numeric_prop(canon: &str, n: f64) -> f64 {
    match canon {
        "BackgroundTransparency"
        | "TextTransparency"
        | "TextStrokeTransparency"
        | "ImageTransparency"
        | "Transparency" => n.clamp(0.0, 1.0),
        "TextSize" => n.clamp(1.0, 256.0),
        "BorderSizePixel" | "ScrollBarThickness" | "Thickness" => n.clamp(0.0, 64.0),
        "AspectRatio" | "SliceScale" => n.clamp(0.01, 100.0),
        "Rotation" => n.clamp(-3600.0, 3600.0),
        "ZIndex" => n.clamp(-10000.0, 10000.0),
        "LayoutOrder" => n.clamp(-100000.0, 100000.0),
        "FillDirectionMaxCells" => n.clamp(0.0, 1000.0),
        _ => n,
    }
}

fn validate_and_clamp_prop_value(class_name: &str, prop: &str, val: &mut Value) -> Result<(), String> {
    if !is_prop_allowed(class_name, prop) {
        return Err(format!(
            "property '{prop}' is not in the allowlist for GUI class '{class_name}'"
        ));
    }
    let canon = canonical_prop_name(prop);
    match canon {
        "Position" | "Size" | "TileSize" | "CanvasSize" | "CellSize" | "CellPadding" => {
            let (u, clamped) = parse_udim2_raw(val)?;
            if clamped {
                *val = json!([[u.x.scale, u.x.offset], [u.y.scale, u.y.offset]]);
            }
        }
        "AnchorPoint" => {
            let ([x, y], clamped) = parse_vec2_raw(val)?;
            if clamped {
                *val = json!([x, y]);
            }
        }
        "CornerRadius" | "Padding" | "PaddingTop" | "PaddingBottom" | "PaddingLeft"
        | "PaddingRight" => {
            let (u, clamped) = parse_udim_raw(val)?;
            if clamped {
                *val = json!([u.scale, u.offset]);
            }
        }
        "SliceCenter" => {
            let (r, clamped) = parse_rect_raw(val)?;
            if clamped {
                *val = json!([r[0], r[1], r[2], r[3]]);
            }
        }
        "BackgroundColor3" | "BorderColor3" | "TextColor3" | "TextStrokeColor3"
        | "ImageColor3" | "ScrollBarImageColor3" | "Color" => {
            let ([r, g, b], clamped) = parse_color_rgb_u8_raw(val)?;
            if clamped {
                *val = json!(format!("#{r:02X}{g:02X}{b:02X}"));
            }
        }
        "Visible" | "ClipsDescendants" | "Active" | "Selectable" | "TextScaled"
        | "TextWrapped" | "RichText" | "AutoButtonColor" | "Modal" | "Wraps" => {
            if !val.is_boolean() {
                return Err(format!("property '{prop}' must be a boolean"));
            }
        }
        "ZIndex" | "LayoutOrder" | "BorderSizePixel" | "TextSize" | "ScrollBarThickness"
        | "FillDirectionMaxCells" | "BackgroundTransparency" | "TextTransparency"
        | "TextStrokeTransparency" | "ImageTransparency" | "Transparency" | "Rotation"
        | "SliceScale" | "Thickness" | "AspectRatio" => {
            let n = val
                .as_f64()
                .ok_or_else(|| format!("property '{prop}' must be numeric"))?;
            if !n.is_finite() {
                return Err(format!("property '{prop}' must be finite"));
            }
            let clamped = clamp_numeric_prop(canon, n);
            if (clamped - n).abs() > f64::EPSILON {
                *val = json!(clamped);
            }
        }
        "Text" | "Name" => {
            let s = val
                .as_str()
                .ok_or_else(|| format!("property '{prop}' must be a string"))?;
            if s.chars().any(|c| c.is_control() && c != '\n') {
                return Err(format!("property '{prop}' contains forbidden control characters"));
            }
        }
        "Image" | "HoverImage" | "PressedImage" | "ScrollBarImage" | "TopImage" | "MidImage"
        | "BottomImage" | "AssetKey" | "AssetPath" | "ProductKey" => {
            let s = val
                .as_str()
                .ok_or_else(|| format!("property '{prop}' must be a string"))?;
            if s.chars().any(|c| c.is_control()) {
                return Err(format!("property '{prop}' contains forbidden control characters"));
            }
        }
        "Callback" => {
            let s = val
                .as_str()
                .ok_or_else(|| format!("property '{prop}' must be a string"))?;
            if !s.trim().is_empty() {
                validate_safe_ident(s, prop)?;
            }
        }
        "ProductId" => {
            if val.is_null() {
                // cleared optional ProductId
            } else if let Some(n) = val.as_u64() {
                let _ = n;
            } else if let Some(s) = val.as_str() {
                if s.chars().any(|c| c.is_control()) {
                    return Err(format!("property '{prop}' contains forbidden control characters"));
                }
            } else {
                return Err(format!("property '{prop}' must be a positive integer, string, or null"));
            }
        }
        "Font"
        | "TextXAlignment"
        | "TextYAlignment"
        | "ScaleType"
        | "AutomaticSize"
        | "SizeConstraint"
        | "AutomaticCanvasSize"
        | "ScrollingDirection"
        | "ElasticBehavior"
        | "ApplyStrokeMode"
        | "LineJoinMode"
        | "FillDirection"
        | "HorizontalAlignment"
        | "VerticalAlignment"
        | "SortOrder"
        | "StartCorner"
        | "AspectType"
        | "DominantAxis" => {
            let s = val
                .as_str()
                .ok_or_else(|| format!("property '{prop}' must be an enum string"))?;
            validate_enum_token(s, prop)?;
        }
        _ => {
            return Err(format!(
                "property '{prop}' is not supported for GUI class '{class_name}'"
            ));
        }
    }
    Ok(())
}

fn validate_node_recursive(
    node: &mut GuiNode,
    depth: usize,
    total: &mut usize,
    path_prefix: &str,
) -> Result<(), String> {
    if depth > MAX_GUI_DEPTH {
        return Err(format!("GUI tree exceeds max depth of {MAX_GUI_DEPTH}"));
    }
    *total += 1;
    if *total > MAX_GUI_NODES {
        return Err(format!("GUI tree exceeds max node count of {MAX_GUI_NODES}"));
    }
    let trimmed_name = node.name.trim();
    if trimmed_name.is_empty() || trimmed_name.len() > 64 {
        return Err("GUI node name must be 1..=64 characters".to_string());
    }
    if trimmed_name.chars().any(|c| c.is_control()) {
        return Err(format!(
            "GUI node name '{trimmed_name}' contains forbidden control characters"
        ));
    }
    node.name = trimmed_name.to_string();
    if !ALLOWED_CLASSES.contains(&node.class_name.as_str()) {
        return Err(format!(
            "GUI class '{}' is not in the strict allowlist ({})",
            node.class_name,
            ALLOWED_CLASSES.join(", ")
        ));
    }
    if node.id.trim().is_empty() {
        node.id = if path_prefix.is_empty() {
            "root".to_string()
        } else {
            format!("{path_prefix}.{}", projects::slug(&node.name))
        };
    } else if node.id.chars().any(|c| c.is_control()) || node.id.len() > 128 {
        return Err(format!("invalid node id '{}'", node.id));
    }
    if MODIFIER_CLASSES.contains(&node.class_name.as_str()) && !node.children.is_empty() {
        return Err(format!(
            "UI modifier '{}' ('{}') cannot contain child nodes",
            node.class_name, node.name
        ));
    }
    if node.props.len() > MAX_NODE_PROPS {
        return Err(format!(
            "GUI node '{}' exceeds max {MAX_NODE_PROPS} properties",
            node.name
        ));
    }
    for (k, v) in &mut node.props {
        validate_and_clamp_prop_value(&node.class_name, k, v)?;
    }
    if !node.children.is_empty() {
        let child_prefix = node.id.clone();
        for child in &mut node.children {
            validate_node_recursive(child, depth + 1, total, &child_prefix)?;
        }
    }
    Ok(())
}

/// Validate and normalize a `ScreenDoc` v1 against strict class, prop, theme, and bounds rules.
pub fn validate_screen_doc(mut doc: ScreenDoc) -> Result<ScreenDoc, String> {
    if doc.version != 1 {
        return Err(format!(
            "unsupported ScreenDoc version {}; expected version 1",
            doc.version
        ));
    }
    let trimmed_name = doc.name.trim();
    if trimmed_name.is_empty() || trimmed_name.len() > 128 {
        return Err("ScreenDoc name must be 1..=128 characters".to_string());
    }
    if trimmed_name.chars().any(|c| c.is_control()) {
        return Err("ScreenDoc name contains forbidden control characters".to_string());
    }
    doc.name = trimmed_name.to_string();

    parse_color_rgb_u8(&Value::String(doc.theme.accent.clone()))
        .map_err(|_| format!("invalid theme.accent color '{}'", doc.theme.accent))?;
    parse_color_rgb_u8(&Value::String(doc.theme.surface.clone()))
        .map_err(|_| format!("invalid theme.surface color '{}'", doc.theme.surface))?;
    parse_color_rgb_u8(&Value::String(doc.theme.text.clone()))
        .map_err(|_| format!("invalid theme.text color '{}'", doc.theme.text))?;

    let mut total = 0usize;
    validate_node_recursive(&mut doc.root, 0, &mut total, "")?;
    Ok(doc)
}

pub fn parse_and_validate_screen_str(raw: &str) -> Result<ScreenDoc, String> {
    if raw.len() > MAX_SCREEN_JSON_BYTES {
        return Err(format!(
            "ScreenDoc JSON exceeds {MAX_SCREEN_JSON_BYTES} byte limit"
        ));
    }
    let val: Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid ScreenDoc JSON: {e}"))?;
    parse_and_validate_screen_value(&val)
}

pub fn parse_and_validate_screen_value(val: &Value) -> Result<ScreenDoc, String> {
    let doc = ScreenDoc::from_json(val)?;
    validate_screen_doc(doc)
}

// ---------------------------------------------------------------------------
// Multi-Device Safe-Area Analyzer
// ---------------------------------------------------------------------------

fn prop_value<'a>(node: &'a GuiNode, key: &str) -> Option<&'a Value> {
    node.props.get(key)
}

fn prop_bool(node: &GuiNode, key: &str, default: bool) -> bool {
    prop_value(node, key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn prop_f64(node: &GuiNode, key: &str, default: f64) -> f64 {
    prop_value(node, key)
        .and_then(Value::as_f64)
        .unwrap_or(default)
}

fn prop_udim2(node: &GuiNode, key: &str, default: UDim2Val) -> UDim2Val {
    prop_value(node, key)
        .and_then(|v| parse_udim2(v).ok())
        .unwrap_or(default)
}

fn prop_vec2(node: &GuiNode, key: &str, default: [f64; 2]) -> [f64; 2] {
    prop_value(node, key)
        .and_then(|v| parse_vec2(v).ok())
        .unwrap_or(default)
}

fn prop_udim(node: &GuiNode, key: &str, default: UDimVal) -> UDimVal {
    prop_value(node, key)
        .and_then(|v| parse_udim(v).ok())
        .unwrap_or(default)
}

fn resolve_udim(u: UDimVal, parent_dim: f64) -> f64 {
    let v = u.scale * parent_dim + u.offset;
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

fn rects_overlap(a: RectBox, b: RectBox) -> bool {
    const EPS: f64 = 0.5;
    a.w > EPS
        && a.h > EPS
        && b.w > EPS
        && b.h > EPS
        && a.x < (b.x + b.w - EPS)
        && (a.x + a.w) > (b.x + EPS)
        && a.y < (b.y + b.h - EPS)
        && (a.y + a.h) > (b.y + EPS)
}

fn compute_node_rect(node: &GuiNode, parent: RectBox) -> RectBox {
    let default_size = UDim2Val {
        x: UDimVal {
            scale: 0.0,
            offset: 100.0,
        },
        y: UDimVal {
            scale: 0.0,
            offset: 100.0,
        },
    };
    let default_pos = UDim2Val {
        x: UDimVal {
            scale: 0.0,
            offset: 0.0,
        },
        y: UDimVal {
            scale: 0.0,
            offset: 0.0,
        },
    };
    let size = prop_udim2(node, "Size", default_size);
    let pos = prop_udim2(node, "Position", default_pos);
    let anchor = prop_vec2(node, "AnchorPoint", [0.0, 0.0]);

    let mut w = resolve_udim(size.x, parent.w);
    let mut h = resolve_udim(size.y, parent.h);

    if let Some(aspect_node) = node
        .children
        .iter()
        .find(|c| c.class_name == "UIAspectRatioConstraint")
    {
        let ratio = prop_f64(aspect_node, "AspectRatio", 1.0).max(0.01);
        let dom = aspect_node
            .props
            .get("DominantAxis")
            .and_then(Value::as_str)
            .unwrap_or("Width");
        if dom.ends_with("Height") {
            w = h * ratio;
        } else {
            h = w / ratio;
        }
    }

    let x = parent.x + resolve_udim(pos.x, parent.w) - anchor[0] * w;
    let y = parent.y + resolve_udim(pos.y, parent.h) - anchor[1] * h;
    RectBox { x, y, w, h }
}

fn apply_padding(rect: RectBox, node: &GuiNode) -> RectBox {
    let Some(pad) = node.children.iter().find(|c| c.class_name == "UIPadding") else {
        return rect;
    };
    let zero = UDimVal {
        scale: 0.0,
        offset: 0.0,
    };
    let left = resolve_udim(prop_udim(pad, "PaddingLeft", zero), rect.w);
    let right = resolve_udim(prop_udim(pad, "PaddingRight", zero), rect.w);
    let top = resolve_udim(prop_udim(pad, "PaddingTop", zero), rect.h);
    let bottom = resolve_udim(prop_udim(pad, "PaddingBottom", zero), rect.h);
    RectBox {
        x: rect.x + left,
        y: rect.y + top,
        w: (rect.w - left - right).max(0.0),
        h: (rect.h - top - bottom).max(0.0),
    }
}

fn is_fullscreen_backdrop(
    node: &GuiNode,
    is_root: bool,
    rect: RectBox,
    container: RectBox,
) -> bool {
    if !is_root || node.class_name != "Frame" {
        return false;
    }
    let covers = rect.x <= container.x + 1.0
        && rect.y <= container.y + 1.0
        && (rect.x + rect.w) >= (container.x + container.w - 1.0)
        && (rect.y + rect.h) >= (container.y + container.h - 1.0);
    let bg_t = prop_f64(node, "BackgroundTransparency", 0.0);
    covers && bg_t >= 0.99
}

struct AnalyzeCtx<'a> {
    doc: &'a ScreenDoc,
    device: DeviceViewport,
    screen_bounds: RectBox,
    usable_bounds: RectBox,
    warnings: &'a mut Vec<SafeAreaWarning>,
    nodes: &'a mut Vec<ResolvedNodeBounds>,
}

fn analyze_node_on_device(
    ctx: &mut AnalyzeCtx<'_>,
    node: &GuiNode,
    parent_rect: RectBox,
    override_rect: Option<RectBox>,
    is_root: bool,
    inside_scroll: bool,
) {
    if MODIFIER_CLASSES.contains(&node.class_name.as_str()) {
        return;
    }
    let visible = prop_bool(node, "Visible", true);
    let rect = override_rect.unwrap_or_else(|| compute_node_rect(node, parent_rect));

    ctx.nodes.push(ResolvedNodeBounds {
        id: node.id.clone(),
        name: node.name.clone(),
        class_name: node.class_name.clone(),
        bounds: rect,
        visible,
    });

    if visible {
        let backdrop = is_fullscreen_backdrop(node, is_root, rect, ctx.usable_bounds);
        if !backdrop {
            // 1. TopBar overlap (y < 58px when ignore_gui_inset = true)
            if ctx.doc.ignore_gui_inset {
                let topbar = RectBox {
                    x: 0.0,
                    y: 0.0,
                    w: ctx.device.width,
                    h: TOPBAR_INSET_PX,
                };
                if rects_overlap(rect, topbar) {
                    ctx.warnings.push(SafeAreaWarning {
                        code: "topbar_overlap".to_string(),
                        severity: "warning".to_string(),
                        device: ctx.device.id.to_string(),
                        node_id: node.id.clone(),
                        node_name: node.name.clone(),
                        class_name: node.class_name.clone(),
                        message: format!(
                            "{} '{}' overlaps the {}px Roblox TopBar on {} (y={:.1}px)",
                            node.class_name, node.name, TOPBAR_INSET_PX as i32, ctx.device.label, rect.y
                        ),
                        bounds: rect,
                    });
                }
            }

            // 2. Mobile thumbstick & jump button safe-zone overlap
            if ctx.device.id == "mobile_landscape" {
                let z = MOBILE_THUMBSTICK_ZONE_PX;
                let left_zone = RectBox {
                    x: 0.0,
                    y: ctx.device.height - z,
                    w: z,
                    h: z,
                };
                let right_zone = RectBox {
                    x: ctx.device.width - z,
                    y: ctx.device.height - z,
                    w: z,
                    h: z,
                };
                if rects_overlap(rect, left_zone) || rects_overlap(rect, right_zone) {
                    let side = if rects_overlap(rect, left_zone) {
                        "bottom-left thumbstick"
                    } else {
                        "bottom-right action/jump"
                    };
                    ctx.warnings.push(SafeAreaWarning {
                        code: "thumbstick_overlap".to_string(),
                        severity: "warning".to_string(),
                        device: ctx.device.id.to_string(),
                        node_id: node.id.clone(),
                        node_name: node.name.clone(),
                        class_name: node.class_name.clone(),
                        message: format!(
                            "{} '{}' overlaps the mobile {} safe zone on {}",
                            node.class_name, node.name, side, ctx.device.label
                        ),
                        bounds: rect,
                    });
                }
            }
        }

        // 3. Offscreen bounds
        if !inside_scroll {
            let min_x = ctx.usable_bounds.x - 0.5;
            let min_y = ctx.usable_bounds.y - 0.5;
            let max_x = ctx.screen_bounds.x + ctx.screen_bounds.w + 0.5;
            let max_y = ctx.screen_bounds.y + ctx.screen_bounds.h + 0.5;
            if rect.x < min_x
                || rect.y < min_y
                || (rect.x + rect.w) > max_x
                || (rect.y + rect.h) > max_y
            {
                ctx.warnings.push(SafeAreaWarning {
                    code: "offscreen".to_string(),
                    severity: "warning".to_string(),
                    device: ctx.device.id.to_string(),
                    node_id: node.id.clone(),
                    node_name: node.name.clone(),
                    class_name: node.class_name.clone(),
                    message: format!(
                        "{} '{}' extends outside {} viewport ({:.0},{:.0} {:.0}x{:.0})",
                        node.class_name,
                        node.name,
                        ctx.device.label,
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h
                    ),
                    bounds: rect,
                });
            }
        }

        // 4. Tiny tap target (<44x44px on buttons)
        if matches!(node.class_name.as_str(), "TextButton" | "ImageButton")
            && (rect.w < MIN_TAP_TARGET_PX - 0.01 || rect.h < MIN_TAP_TARGET_PX - 0.01)
        {
            ctx.warnings.push(SafeAreaWarning {
                code: "tiny_tap_target".to_string(),
                severity: "warning".to_string(),
                device: ctx.device.id.to_string(),
                node_id: node.id.clone(),
                node_name: node.name.clone(),
                class_name: node.class_name.clone(),
                message: format!(
                    "{} '{}' tap target is {:.0}x{:.0}px (<44x44px minimum) on {}",
                    node.class_name, node.name, rect.w, rect.h, ctx.device.label
                ),
                bounds: rect,
            });
        }
    }

    let inner_rect = apply_padding(rect, node);
    let child_scroll = inside_scroll || node.class_name == "ScrollingFrame";

    let mut gui_children: Vec<&GuiNode> = node
        .children
        .iter()
        .filter(|c| !MODIFIER_CLASSES.contains(&c.class_name.as_str()))
        .collect();

    if let Some(list_layout) = node.children.iter().find(|c| c.class_name == "UIListLayout") {
        let sort_order = list_layout
            .props
            .get("SortOrder")
            .and_then(Value::as_str)
            .unwrap_or("LayoutOrder");
        if sort_order.ends_with("Name") {
            gui_children.sort_by(|a, b| a.name.cmp(&b.name));
        } else {
            gui_children.sort_by_key(|c| prop_f64(c, "LayoutOrder", 0.0) as i64);
        }
        let horizontal = list_layout
            .props
            .get("FillDirection")
            .and_then(Value::as_str)
            .is_some_and(|s| s.ends_with("Horizontal"));
        let pad_udim = prop_udim(
            list_layout,
            "Padding",
            UDimVal {
                scale: 0.0,
                offset: 0.0,
            },
        );
        let gap = resolve_udim(pad_udim, if horizontal { inner_rect.w } else { inner_rect.h });

        let sizes: Vec<(f64, f64)> = gui_children
            .iter()
            .map(|c| {
                let r = compute_node_rect(c, inner_rect);
                (r.w, r.h)
            })
            .collect();
        let total_main: f64 = if sizes.is_empty() {
            0.0
        } else {
            sizes
                .iter()
                .map(|(w, h)| if horizontal { *w } else { *h })
                .sum::<f64>()
                + gap * ((sizes.len() - 1) as f64)
        };

        let h_align = list_layout
            .props
            .get("HorizontalAlignment")
            .and_then(Value::as_str)
            .unwrap_or("Left");
        let v_align = list_layout
            .props
            .get("VerticalAlignment")
            .and_then(Value::as_str)
            .unwrap_or("Top");

        let mut cursor = if horizontal {
            if h_align.ends_with("Center") {
                inner_rect.x + (inner_rect.w - total_main) * 0.5
            } else if h_align.ends_with("Right") {
                inner_rect.x + inner_rect.w - total_main
            } else {
                inner_rect.x
            }
        } else if v_align.ends_with("Center") {
            inner_rect.y + (inner_rect.h - total_main) * 0.5
        } else if v_align.ends_with("Bottom") {
            inner_rect.y + inner_rect.h - total_main
        } else {
            inner_rect.y
        };

        for (child, (cw, ch)) in gui_children.into_iter().zip(sizes.into_iter()) {
            let (cx, cy) = if horizontal {
                let y = if v_align.ends_with("Center") {
                    inner_rect.y + (inner_rect.h - ch) * 0.5
                } else if v_align.ends_with("Bottom") {
                    inner_rect.y + inner_rect.h - ch
                } else {
                    inner_rect.y
                };
                let x = cursor;
                cursor += cw + gap;
                (x, y)
            } else {
                let x = if h_align.ends_with("Center") {
                    inner_rect.x + (inner_rect.w - cw) * 0.5
                } else if h_align.ends_with("Right") {
                    inner_rect.x + inner_rect.w - cw
                } else {
                    inner_rect.x
                };
                let y = cursor;
                cursor += ch + gap;
                (x, y)
            };
            let child_rect = RectBox {
                x: cx,
                y: cy,
                w: cw,
                h: ch,
            };
            analyze_node_on_device(ctx, child, inner_rect, Some(child_rect), false, child_scroll);
        }
        return;
    }

    if let Some(grid_layout) = node.children.iter().find(|c| c.class_name == "UIGridLayout") {
        let cell_size = prop_udim2(
            grid_layout,
            "CellSize",
            UDim2Val {
                x: UDimVal {
                    scale: 0.0,
                    offset: 100.0,
                },
                y: UDimVal {
                    scale: 0.0,
                    offset: 100.0,
                },
            },
        );
        let cell_pad = prop_udim2(
            grid_layout,
            "CellPadding",
            UDim2Val {
                x: UDimVal {
                    scale: 0.0,
                    offset: 5.0,
                },
                y: UDimVal {
                    scale: 0.0,
                    offset: 5.0,
                },
            },
        );
        let cw = resolve_udim(cell_size.x, inner_rect.w);
        let ch = resolve_udim(cell_size.y, inner_rect.h);
        let px = resolve_udim(cell_pad.x, inner_rect.w);
        let py = resolve_udim(cell_pad.y, inner_rect.h);
        let cols = (((inner_rect.w + px) / (cw + px).max(1.0)).floor() as usize).max(1);
        for (idx, child) in gui_children.into_iter().enumerate() {
            let col = idx % cols;
            let row = idx / cols;
            let child_rect = RectBox {
                x: inner_rect.x + (col as f64) * (cw + px),
                y: inner_rect.y + (row as f64) * (ch + py),
                w: cw,
                h: ch,
            };
            analyze_node_on_device(ctx, child, inner_rect, Some(child_rect), false, child_scroll);
        }
        return;
    }

    for child in gui_children {
        analyze_node_on_device(ctx, child, inner_rect, None, false, child_scroll);
    }
}

/// Analyze a `ScreenDoc` across Desktop (1920x1080), Tablet (1024x768), and Mobile Landscape (844x390).
pub fn analyze_safe_areas(doc: &ScreenDoc) -> SafeAreaAnalysis {
    let mut all_warnings = Vec::new();
    let mut devices = BTreeMap::new();

    for dev in DEVICES {
        let screen_bounds = RectBox {
            x: 0.0,
            y: 0.0,
            w: dev.width,
            h: dev.height,
        };
        let topbar_inset = if doc.ignore_gui_inset {
            0.0
        } else {
            TOPBAR_INSET_PX
        };
        let usable_bounds = RectBox {
            x: 0.0,
            y: topbar_inset,
            w: dev.width,
            h: (dev.height - topbar_inset).max(0.0),
        };
        let mut dev_warnings = Vec::new();
        let mut dev_nodes = Vec::new();
        {
            let mut ctx = AnalyzeCtx {
                doc,
                device: dev,
                screen_bounds,
                usable_bounds,
                warnings: &mut dev_warnings,
                nodes: &mut dev_nodes,
            };
            analyze_node_on_device(&mut ctx, &doc.root, usable_bounds, None, true, false);
        }

        let mut safe_zones = vec![json!({
            "id": "topbar",
            "label": "CoreGui TopBar (58px)",
            "x": 0.0,
            "y": 0.0,
            "w": dev.width,
            "h": TOPBAR_INSET_PX,
            "active": doc.ignore_gui_inset,
        })];
        if dev.id == "mobile_landscape" {
            let z = MOBILE_THUMBSTICK_ZONE_PX;
            safe_zones.push(json!({
                "id": "thumbstick_left",
                "label": "Mobile Thumbstick",
                "x": 0.0,
                "y": dev.height - z,
                "w": z,
                "h": z,
                "active": true,
            }));
            safe_zones.push(json!({
                "id": "action_right",
                "label": "Mobile Jump / Action",
                "x": dev.width - z,
                "y": dev.height - z,
                "w": z,
                "h": z,
                "active": true,
            }));
        }

        all_warnings.extend(dev_warnings.iter().cloned());
        devices.insert(
            dev.id.to_string(),
            DeviceAnalysis {
                device: dev.id.to_string(),
                label: dev.label.to_string(),
                width: dev.width,
                height: dev.height,
                topbar_inset: TOPBAR_INSET_PX,
                safe_zones,
                warnings: dev_warnings,
                nodes: dev_nodes,
            },
        );
    }

    SafeAreaAnalysis {
        passed: all_warnings.is_empty(),
        warning_count: all_warnings.len(),
        warnings: all_warnings,
        devices,
    }
}

// ---------------------------------------------------------------------------
// Deterministic --!strict Luau Compiler + Rojo .model.json Emitter
// ---------------------------------------------------------------------------

fn luau_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn fmt_f64(n: f64) -> String {
    if !n.is_finite() {
        return "0".to_string();
    }
    if (n - n.round()).abs() < 1e-6 && n.abs() <= 9_000_000_000_000_000.0 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{n:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn enum_type_for_prop(prop: &str) -> &'static str {
    match prop {
        "Font" => "Font",
        "TextXAlignment" => "TextXAlignment",
        "TextYAlignment" => "TextYAlignment",
        "ScaleType" => "ScaleType",
        "AutomaticSize" => "AutomaticSize",
        "SizeConstraint" => "SizeConstraint",
        "AutomaticCanvasSize" => "AutomaticSize",
        "ScrollingDirection" => "ScrollingDirection",
        "ElasticBehavior" => "ElasticBehavior",
        "ApplyStrokeMode" => "ApplyStrokeMode",
        "LineJoinMode" => "LineJoinMode",
        "FillDirection" => "FillDirection",
        "HorizontalAlignment" => "HorizontalAlignment",
        "VerticalAlignment" => "VerticalAlignment",
        "SortOrder" => "SortOrder",
        "StartCorner" => "StartCorner",
        "AspectType" => "AspectType",
        "DominantAxis" => "DominantAxis",
        _ => "Unknown",
    }
}

fn format_luau_prop_rhs(prop: &str, val: &Value) -> Result<String, String> {
    let canon = canonical_prop_name(prop);
    match canon {
        "Position" | "Size" | "TileSize" | "CanvasSize" | "CellSize" | "CellPadding" => {
            let u = parse_udim2(val)?;
            Ok(format!(
                "UDim2.new({}, {}, {}, {})",
                fmt_f64(u.x.scale),
                fmt_f64(u.x.offset),
                fmt_f64(u.y.scale),
                fmt_f64(u.y.offset)
            ))
        }
        "AnchorPoint" => {
            let v = parse_vec2(val)?;
            Ok(format!("Vector2.new({}, {})", fmt_f64(v[0]), fmt_f64(v[1])))
        }
        "CornerRadius" | "Padding" | "PaddingTop" | "PaddingBottom" | "PaddingLeft"
        | "PaddingRight" => {
            let u = parse_udim(val)?;
            Ok(format!("UDim.new({}, {})", fmt_f64(u.scale), fmt_f64(u.offset)))
        }
        "SliceCenter" => {
            let r = parse_rect(val)?;
            Ok(format!(
                "Rect.new({}, {}, {}, {})",
                fmt_f64(r[0]),
                fmt_f64(r[1]),
                fmt_f64(r[2]),
                fmt_f64(r[3])
            ))
        }
        "BackgroundColor3" | "BorderColor3" | "TextColor3" | "TextStrokeColor3"
        | "ImageColor3" | "ScrollBarImageColor3" | "Color" => {
            let [r, g, b] = parse_color_rgb_u8(val)?;
            Ok(format!("Color3.fromRGB({r}, {g}, {b})"))
        }
        "Visible" | "ClipsDescendants" | "Active" | "Selectable" | "TextScaled"
        | "TextWrapped" | "RichText" | "AutoButtonColor" | "Modal" | "Wraps" => Ok(
            if val.as_bool().unwrap_or(false) {
                "true"
            } else {
                "false"
            }
            .to_string(),
        ),
        "ZIndex" | "LayoutOrder" | "BorderSizePixel" | "TextSize" | "ScrollBarThickness"
        | "FillDirectionMaxCells" | "BackgroundTransparency" | "TextTransparency"
        | "TextStrokeTransparency" | "ImageTransparency" | "Transparency" | "Rotation"
        | "SliceScale" | "Thickness" | "AspectRatio" => {
            let n = val.as_f64().unwrap_or(0.0);
            Ok(fmt_f64(n))
        }
        "Text" | "Name" | "Image" | "HoverImage" | "PressedImage" | "ScrollBarImage"
        | "TopImage" | "MidImage" | "BottomImage" => {
            Ok(luau_str(val.as_str().unwrap_or("")))
        }
        "Font"
        | "TextXAlignment"
        | "TextYAlignment"
        | "ScaleType"
        | "AutomaticSize"
        | "SizeConstraint"
        | "AutomaticCanvasSize"
        | "ScrollingDirection"
        | "ElasticBehavior"
        | "ApplyStrokeMode"
        | "LineJoinMode"
        | "FillDirection"
        | "HorizontalAlignment"
        | "VerticalAlignment"
        | "SortOrder"
        | "StartCorner"
        | "AspectType"
        | "DominantAxis" => {
            let item = validate_enum_token(val.as_str().unwrap_or(""), canon)?;
            let etype = enum_type_for_prop(canon);
            Ok(format!("Enum.{etype}.{item}"))
        }
        _ => Err(format!("cannot compile property '{prop}'")),
    }
}

fn sanitize_luau_var(raw: &str, idx: usize) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let clean = out.trim_matches('_');
    if clean.is_empty() {
        format!("node_{idx}")
    } else {
        format!("{clean}_{idx}")
    }
}

pub fn pascal_case_slug(slug: &str) -> String {
    let mut out = String::new();
    for part in slug.split(['-', '_']) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    if out.is_empty() {
        "ScreenHud".to_string()
    } else {
        out
    }
}

fn node_uses_assets(node: &GuiNode) -> bool {
    if node.props.contains_key("AssetKey")
        || node.props.contains_key("asset_key")
        || node.props.contains_key("AssetPath")
        || node.props.contains_key("asset_path")
    {
        return true;
    }
    node.children.iter().any(node_uses_assets)
}

fn node_uses_products(node: &GuiNode) -> bool {
    if node.props.contains_key("ProductKey")
        || node.props.contains_key("product_key")
        || node
            .props
            .get("ProductId")
            .or_else(|| node.props.get("product_id"))
            .is_some_and(|v| !v.is_null())
    {
        return true;
    }
    node.children.iter().any(node_uses_products)
}

fn emit_luau_node(
    buf: &mut String,
    node: &GuiNode,
    parent_var: &str,
    counter: &mut usize,
    used_vars: &mut BTreeSet<String>,
) -> Result<(), String> {
    let idx = *counter;
    *counter += 1;
    let mut var = sanitize_luau_var(&node.name, idx);
    while used_vars.contains(&var) {
        var.push('_');
    }
    used_vars.insert(var.clone());

    let _ = writeln!(
        buf,
        "\tlocal {var} = Instance.new({})",
        luau_str(&node.class_name)
    );
    let _ = writeln!(buf, "\t{var}.Name = {}", luau_str(&node.name));

    let mut asset_ref: Option<String> = None;
    let mut fallback_image: Option<String> = None;
    let mut product_key: Option<String> = None;
    let mut product_id_num: Option<u64> = None;
    let mut callback_name: Option<String> = None;

    for (k, v) in &node.props {
        let canon = canonical_prop_name(k);
        match canon {
            "Name" => {}
            "AssetKey" | "AssetPath" => {
                if let Some(s) = v.as_str() {
                    if !s.trim().is_empty() {
                        asset_ref = Some(s.trim().to_string());
                    }
                }
            }
            "Image" => {
                let s = v.as_str().unwrap_or("");
                fallback_image = Some(s.to_string());
            }
            "ProductKey" => {
                if let Some(s) = v.as_str() {
                    if !s.trim().is_empty() {
                        product_key = Some(s.trim().to_string());
                    }
                }
            }
            "ProductId" => {
                if let Some(n) = v.as_u64() {
                    if n > 0 {
                        product_id_num = Some(n);
                    }
                } else if let Some(s) = v.as_str() {
                    let trimmed = s.trim();
                    if let Ok(n) = trimmed.parse::<u64>() {
                        if n > 0 {
                            product_id_num = Some(n);
                        }
                    } else if !trimmed.is_empty() && product_key.is_none() {
                        product_key = Some(trimmed.to_string());
                    }
                }
            }
            "Callback" => {
                if let Some(s) = v.as_str() {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        callback_name = Some(trimmed.to_string());
                    }
                }
            }
            _ => {
                let rhs = format_luau_prop_rhs(canon, v)?;
                let _ = writeln!(buf, "\t{var}.{canon} = {rhs}");
            }
        }
    }

    if let Some(asset_key) = asset_ref {
        let fb = fallback_image.as_deref().unwrap_or("");
        let _ = writeln!(
            buf,
            "\t{var}.Image = resolveAssetId(assetIds, {}, {})",
            luau_str(&asset_key),
            luau_str(fb)
        );
    } else if let Some(fb) = fallback_image {
        let _ = writeln!(buf, "\t{var}.Image = {}", luau_str(&fb));
    }

    if matches!(node.class_name.as_str(), "TextButton" | "ImageButton")
        && (callback_name.is_some() || product_key.is_some() || product_id_num.is_some())
    {
        let _ = writeln!(buf, "\t{var}.Activated:Connect(function()");
        if let Some(pk) = &product_key {
            let fallback_id_expr = product_id_num
                .map(|n| format!(" or {n}"))
                .unwrap_or_default();
            let _ = writeln!(
                buf,
                "\t\tlocal productId = resolveProductId(productIds, {}){fallback_id_expr}",
                luau_str(pk)
            );
            let cb_key = callback_name.as_deref().unwrap_or(pk);
            let _ = writeln!(
                buf,
                "\t\tif callbacks and type(callbacks[{}]) == \"function\" then",
                luau_str(cb_key)
            );
            let _ = writeln!(buf, "\t\t\tcallbacks[{}](productId)", luau_str(cb_key));
            buf.push_str("\t\telseif productId then\n");
            let _ = writeln!(
                buf,
                "\t\t\tpromptProductOrPass(productIds, {}, productId)",
                luau_str(pk)
            );
            buf.push_str("\t\tend\n");
        } else if let Some(pid) = product_id_num {
            if let Some(cb) = &callback_name {
                let _ = writeln!(
                    buf,
                    "\t\tif callbacks and type(callbacks[{}]) == \"function\" then",
                    luau_str(cb)
                );
                let _ = writeln!(buf, "\t\t\tcallbacks[{}]({pid})", luau_str(cb));
                buf.push_str("\t\telse\n");
                let _ = writeln!(
                    buf,
                    "\t\t\tpromptProductOrPass(productIds, \"\", {pid})"
                );
                buf.push_str("\t\tend\n");
            } else {
                let _ = writeln!(
                    buf,
                    "\t\tpromptProductOrPass(productIds, \"\", {pid})"
                );
            }
        } else if let Some(cb) = &callback_name {
            let _ = writeln!(
                buf,
                "\t\tif callbacks and type(callbacks[{}]) == \"function\" then",
                luau_str(cb)
            );
            let _ = writeln!(buf, "\t\t\tcallbacks[{}]()", luau_str(cb));
            buf.push_str("\t\tend\n");
        }
        buf.push_str("\tend)\n");
    }

    let _ = writeln!(buf, "\t{var}.Parent = {parent_var}");

    for child in &node.children {
        emit_luau_node(buf, child, &var, counter, used_vars)?;
    }
    Ok(())
}

/// Compile a validated `ScreenDoc` deterministically into a `--!strict` Luau module.
pub fn compile_screen_to_luau(slug: &str, doc: &ScreenDoc) -> Result<String, String> {
    let clean_slug = validate_slug(slug)?;
    let validated = validate_screen_doc(doc.clone())?;
    compile_validated_to_luau(&clean_slug, &validated)
}

fn compile_validated_to_luau(clean_slug: &str, validated: &ScreenDoc) -> Result<String, String> {
    let uses_assets = node_uses_assets(&validated.root);
    let uses_products = node_uses_products(&validated.root);

    let mut out = String::with_capacity(2048);
    out.push_str("--!strict\n");
    let _ = writeln!(
        out,
        "-- Generated by OpenRO GUI Designer from ui/{clean_slug}.screen.json.\n"
    );
    if uses_products {
        out.push_str("local MarketplaceService = game:GetService(\"MarketplaceService\")\n");
    }
    out.push_str("local Players = game:GetService(\"Players\")\n");
    if uses_assets || uses_products {
        out.push_str("local ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\n");
    }
    out.push('\n');
    out.push_str("export type CallbackMap = { [string]: (...any) -> () }\n\n");

    let [ar, ag, ab] = parse_color_rgb_u8(&Value::String(validated.theme.accent.clone()))?;
    let [sr, sg, sb] = parse_color_rgb_u8(&Value::String(validated.theme.surface.clone()))?;
    let [tr, tg, tb] = parse_color_rgb_u8(&Value::String(validated.theme.text.clone()))?;

    out.push_str("local ScreenModule = {}\n\n");
    out.push_str("ScreenModule.THEME = {\n");
    out.push_str(&format!("\tAccent = Color3.fromRGB({ar}, {ag}, {ab}),\n"));
    out.push_str(&format!("\tSurface = Color3.fromRGB({sr}, {sg}, {sb}),\n"));
    out.push_str(&format!("\tText = Color3.fromRGB({tr}, {tg}, {tb}),\n"));
    out.push_str("}\n\n");

    if uses_assets || uses_products {
        out.push_str("local function loadOptionalShared(moduleName: string): { [string]: any }?\n");
        out.push_str("\tlocal sharedFolder = ReplicatedStorage:FindFirstChild(\"Shared\")\n");
        out.push_str("\tif not sharedFolder then\n");
        out.push_str("\t\treturn nil\n");
        out.push_str("\tend\n");
        out.push_str("\tlocal mod = sharedFolder:FindFirstChild(moduleName)\n");
        out.push_str("\tif not mod or not mod:IsA(\"ModuleScript\") then\n");
        out.push_str("\t\treturn nil\n");
        out.push_str("\tend\n");
        out.push_str("\tlocal ok, result = pcall(require, mod)\n");
        out.push_str("\tif ok and type(result) == \"table\" then\n");
        out.push_str("\t\treturn result :: { [string]: any }\n");
        out.push_str("\tend\n");
        out.push_str("\treturn nil\n");
        out.push_str("end\n\n");
    }

    if uses_assets {
        out.push_str(
            "local function resolveAssetId(assetIds: { [string]: any }?, keyOrPath: string, fallback: string): string\n",
        );
        out.push_str("\tif assetIds then\n");
        out.push_str("\t\tif type(assetIds.get) == \"function\" then\n");
        out.push_str("\t\t\tlocal resolved = assetIds.get(keyOrPath, fallback)\n");
        out.push_str("\t\t\tif type(resolved) == \"string\" and resolved ~= \"\" then\n");
        out.push_str("\t\t\t\treturn resolved\n");
        out.push_str("\t\t\tend\n");
        out.push_str("\t\tend\n");
        out.push_str("\t\tif type(assetIds.byKey) == \"table\" and type(assetIds.byKey[keyOrPath]) == \"string\" then\n");
        out.push_str("\t\t\treturn assetIds.byKey[keyOrPath]\n");
        out.push_str("\t\tend\n");
        out.push_str("\t\tif type(assetIds.byPath) == \"table\" and type(assetIds.byPath[keyOrPath]) == \"string\" then\n");
        out.push_str("\t\t\treturn assetIds.byPath[keyOrPath]\n");
        out.push_str("\t\tend\n");
        out.push_str("\tend\n");
        out.push_str("\treturn fallback\n");
        out.push_str("end\n\n");
    }

    if uses_products {
        out.push_str(
            "local function resolveProductId(productIds: { [string]: any }?, key: string): number?\n",
        );
        out.push_str("\tif not productIds then\n");
        out.push_str("\t\treturn nil\n");
        out.push_str("\tend\n");
        out.push_str("\tlocal byKey = productIds.byKey\n");
        out.push_str("\tif type(byKey) == \"table\" and type(byKey[key]) == \"number\" then\n");
        out.push_str("\t\treturn byKey[key]\n");
        out.push_str("\tend\n");
        out.push_str("\tlocal dev = productIds.DevProducts\n");
        out.push_str("\tif type(dev) == \"table\" then\n");
        out.push_str("\t\tlocal entry = dev[key]\n");
        out.push_str("\t\tif type(entry) == \"number\" then\n");
        out.push_str("\t\t\treturn entry\n");
        out.push_str("\t\telseif type(entry) == \"table\" and type(entry.robloxId) == \"number\" then\n");
        out.push_str("\t\t\treturn entry.robloxId\n");
        out.push_str("\t\tend\n");
        out.push_str("\tend\n");
        out.push_str("\tlocal passes = productIds.GamePasses\n");
        out.push_str("\tif type(passes) == \"table\" then\n");
        out.push_str("\t\tlocal entry = passes[key]\n");
        out.push_str("\t\tif type(entry) == \"number\" then\n");
        out.push_str("\t\t\treturn entry\n");
        out.push_str("\t\telseif type(entry) == \"table\" and type(entry.robloxId) == \"number\" then\n");
        out.push_str("\t\t\treturn entry.robloxId\n");
        out.push_str("\t\tend\n");
        out.push_str("\tend\n");
        out.push_str("\treturn nil\n");
        out.push_str("end\n\n");

        out.push_str(
            "local function promptProductOrPass(productIds: { [string]: any }?, key: string, productId: number)\n",
        );
        out.push_str("\tlocal player = Players.LocalPlayer\n");
        out.push_str("\tif not player or productId <= 0 then\n");
        out.push_str("\t\treturn\n");
        out.push_str("\tend\n");
        out.push_str("\tlocal passes = if productIds then productIds.GamePasses else nil\n");
        out.push_str("\tif type(passes) == \"table\" and passes[key] ~= nil then\n");
        out.push_str("\t\tMarketplaceService:PromptGamePassPurchase(player, productId)\n");
        out.push_str("\telse\n");
        out.push_str("\t\tMarketplaceService:PromptProductPurchase(player, productId)\n");
        out.push_str("\tend\n");
        out.push_str("end\n\n");
    }

    out.push_str("function ScreenModule.create(parent: Instance?, callbacks: CallbackMap?): ScreenGui\n");
    out.push_str("\tlocal targetParent: Instance? = parent\n");
    out.push_str("\tif not targetParent then\n");
    out.push_str("\t\tlocal localPlayer = Players.LocalPlayer\n");
    out.push_str("\t\tif localPlayer then\n");
    out.push_str("\t\t\ttargetParent = localPlayer:WaitForChild(\"PlayerGui\")\n");
    out.push_str("\t\tend\n");
    out.push_str("\tend\n");
    if uses_assets {
        out.push_str("\tlocal assetIds = loadOptionalShared(\"AssetIds\")\n");
    }
    if uses_products {
        out.push_str("\tlocal productIds = loadOptionalShared(\"ProductIds\")\n");
    }
    out.push_str("\tlocal screenGui = Instance.new(\"ScreenGui\")\n");
    out.push_str(&format!("\tscreenGui.Name = {}\n", luau_str(&validated.name)));
    out.push_str(&format!(
        "\tscreenGui.IgnoreGuiInset = {}\n",
        if validated.ignore_gui_inset {
            "true"
        } else {
            "false"
        }
    ));
    out.push_str(&format!(
        "\tscreenGui.ResetOnSpawn = {}\n",
        if validated.reset_on_spawn {
            "true"
        } else {
            "false"
        }
    ));
    out.push_str("\tscreenGui.ZIndexBehavior = Enum.ZIndexBehavior.Sibling\n");

    let mut counter = 0usize;
    let mut used_vars = BTreeSet::new();
    emit_luau_node(&mut out, &validated.root, "screenGui", &mut counter, &mut used_vars)?;

    out.push_str("\tscreenGui.Parent = targetParent\n");
    out.push_str("\treturn screenGui\n");
    out.push_str("end\n\n");
    out.push_str("return ScreenModule\n");

    Ok(out)
}

fn rojo_prop_value(prop: &str, val: &Value) -> Result<Option<Value>, String> {
    let canon = canonical_prop_name(prop);
    match canon {
        "Name" | "AssetKey" | "AssetPath" | "ProductKey" | "ProductId" | "Callback" => Ok(None),
        "Position" | "Size" | "TileSize" | "CanvasSize" | "CellSize" | "CellPadding" => {
            let u = parse_udim2(val)?;
            Ok(Some(json!({
                "UDim2": [[u.x.scale, u.x.offset], [u.y.scale, u.y.offset]]
            })))
        }
        "AnchorPoint" => {
            let v = parse_vec2(val)?;
            Ok(Some(json!({ "Vector2": [v[0], v[1]] })))
        }
        "CornerRadius" | "Padding" | "PaddingTop" | "PaddingBottom" | "PaddingLeft"
        | "PaddingRight" => {
            let u = parse_udim(val)?;
            Ok(Some(json!({ "UDim": [u.scale, u.offset] })))
        }
        "SliceCenter" => {
            let r = parse_rect(val)?;
            Ok(Some(json!({ "Rect": [[r[0], r[1]], [r[2], r[3]]] })))
        }
        "BackgroundColor3" | "BorderColor3" | "TextColor3" | "TextStrokeColor3"
        | "ImageColor3" | "ScrollBarImageColor3" | "Color" => {
            let [r, g, b] = parse_color_rgb_u8(val)?;
            Ok(Some(json!({
                "Color3": [
                    (f64::from(r) / 255.0 * 1000.0).round() / 1000.0,
                    (f64::from(g) / 255.0 * 1000.0).round() / 1000.0,
                    (f64::from(b) / 255.0 * 1000.0).round() / 1000.0
                ]
            })))
        }
        "Image" | "HoverImage" | "PressedImage" | "ScrollBarImage" | "TopImage" | "MidImage"
        | "BottomImage" => Ok(Some(json!({ "Content": val.as_str().unwrap_or("") }))),
        "Font"
        | "TextXAlignment"
        | "TextYAlignment"
        | "ScaleType"
        | "AutomaticSize"
        | "SizeConstraint"
        | "AutomaticCanvasSize"
        | "ScrollingDirection"
        | "ElasticBehavior"
        | "ApplyStrokeMode"
        | "LineJoinMode"
        | "FillDirection"
        | "HorizontalAlignment"
        | "VerticalAlignment"
        | "SortOrder"
        | "StartCorner"
        | "AspectType"
        | "DominantAxis" => {
            let tok = validate_enum_token(val.as_str().unwrap_or(""), canon)?;
            Ok(Some(json!(tok)))
        }
        _ => Ok(Some(val.clone())),
    }
}

fn emit_rojo_model_node(node: &GuiNode) -> Result<Value, String> {
    let mut props = Map::new();
    for (k, v) in &node.props {
        let canon = canonical_prop_name(k);
        if let Some(rv) = rojo_prop_value(canon, v)? {
            props.insert(canon.to_string(), rv);
        }
    }
    let mut children = Vec::with_capacity(node.children.len());
    for c in &node.children {
        children.push(emit_rojo_model_node(c)?);
    }
    let mut obj = Map::new();
    obj.insert("className".to_string(), json!(node.class_name));
    obj.insert("name".to_string(), json!(node.name));
    if !props.is_empty() {
        obj.insert("properties".to_string(), Value::Object(props));
    }
    if !children.is_empty() {
        obj.insert("children".to_string(), Value::Array(children));
    }
    Ok(Value::Object(obj))
}

/// Emit a deterministic Rojo `.model.json` representation of the `ScreenGui`.
pub fn compile_screen_to_rojo_model(doc: &ScreenDoc) -> Result<Value, String> {
    let validated = validate_screen_doc(doc.clone())?;
    compile_validated_to_rojo_model(&validated)
}

fn compile_validated_to_rojo_model(validated: &ScreenDoc) -> Result<Value, String> {
    let root_child = emit_rojo_model_node(&validated.root)?;
    Ok(json!({
        "className": "ScreenGui",
        "name": validated.name,
        "properties": {
            "IgnoreGuiInset": validated.ignore_gui_inset,
            "ResetOnSpawn": validated.reset_on_spawn,
            "ZIndexBehavior": "Sibling",
        },
        "children": [root_child],
    }))
}

fn run_stylua_stdin(
    stylua_bin: &Path,
    project_dir: Option<&Path>,
    source: &str,
    check_only: bool,
) -> Result<(bool, String, String), String> {
    let mut cmd = Command::new(stylua_bin);
    if let Some(dir) = project_dir {
        let cfg = dir.join("stylua.toml");
        if cfg.is_file() {
            cmd.arg("--config-path").arg(cfg);
        }
    }
    if check_only {
        cmd.arg("--check");
    }
    cmd.arg("--stdin-filepath").arg("ScreenHud.luau").arg("-");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("spawn stylua: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(source.as_bytes())
            .map_err(|e| format!("write stylua stdin: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("wait stylua: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    Ok((out.status.success(), stdout, stderr))
}

fn report_to_json(report: &AnalysisReport) -> Value {
    json!({
        "passed": report.is_ok(),
        "error_count": report.error_count(),
        "warning_count": report.warning_count(),
        "diagnostics": report.diagnostics.iter().map(|d| d.to_json()).collect::<Vec<_>>(),
    })
}

/// Compile a `ScreenDoc`, run `luau_check` and optional `stylua` formatting/check,
/// run multi-device safe-area analysis, and emit optional Rojo `.model.json`.
pub fn compile_screen_doc(
    slug: &str,
    doc: &ScreenDoc,
    project_dir: Option<&Path>,
) -> Result<Value, String> {
    let clean_slug = validate_slug(slug)?;
    let validated = validate_screen_doc(doc.clone())?;
    let analysis = analyze_safe_areas(&validated);
    let mut luau = compile_validated_to_luau(&clean_slug, &validated)?;
    let model_json = compile_validated_to_rojo_model(&validated)?;

    let stylua_json = match projects::find_tool("stylua", "OPENRO_STYLUA") {
        Some(bin) => {
            if let Ok((fmt_ok, formatted, _)) = run_stylua_stdin(&bin, project_dir, &luau, false) {
                if fmt_ok && !formatted.is_empty() {
                    luau = formatted;
                }
            }
            match run_stylua_stdin(&bin, project_dir, &luau, true) {
                Ok((passed, _, stderr)) => json!({
                    "available": true,
                    "passed": passed,
                    "stderr": stderr,
                }),
                Err(e) => json!({
                    "available": true,
                    "passed": false,
                    "error": e,
                }),
            }
        }
        None => json!({
            "available": false,
            "passed": true,
            "note": "stylua is not installed",
        }),
    };

    let pascal = pascal_case_slug(&clean_slug);
    let check_path = format!("StarterPlayer.StarterPlayerScripts.Client.ui.{pascal}");
    let luau_report = luau_check::analyze_source(&check_path, &luau);
    if !luau_report.is_ok() {
        return Err(format!(
            "compiled Luau failed luau_check: {}",
            luau_report.errors().join("; ")
        ));
    }

    let luau_path = format!("src/client/ui/{pascal}.luau");
    let model_path = format!("{UI_DIR_REL}/{clean_slug}.model.json");

    Ok(json!({
        "ok": true,
        "slug": clean_slug,
        "luau_path": luau_path,
        "luau": luau,
        "model_path": model_path,
        "model_json": model_json,
        "analysis": analysis.to_json(),
        "safe_area_warnings": analysis.warnings.iter().map(SafeAreaWarning::to_json).collect::<Vec<_>>(),
        "luau_check": report_to_json(&luau_report),
        "stylua": stylua_json,
    }))
}

// ---------------------------------------------------------------------------
// Project & LocalPlace Save / Load / Compile Helpers
// ---------------------------------------------------------------------------

pub fn list_project_screens(dir: &Path) -> Result<Value, String> {
    let ui_dir = projects::safe_join(dir, UI_DIR_REL)?;
    let mut screens = Vec::new();
    if let Ok(meta) = std::fs::symlink_metadata(&ui_dir) {
        if meta.file_type().is_symlink() {
            return Err("ui directory must not be a symlink".to_string());
        }
        if meta.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(&ui_dir)
                .map_err(|e| format!("read ui dir: {e}"))?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|ft| !ft.is_symlink() && ft.is_file()))
                .map(|e| e.path())
                .collect();
            entries.sort();
            for path in entries {
                let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let Some(slug) = fname.strip_suffix(".screen.json") else {
                    continue;
                };
                if validate_slug(slug).is_err() {
                    continue;
                }
                if std::fs::symlink_metadata(&path)
                    .is_ok_and(|m| !m.file_type().is_symlink() && m.is_file() && m.len() <= MAX_SCREEN_JSON_BYTES as u64)
                {
                    if let Ok(raw) = std::fs::read_to_string(&path) {
                        if let Ok(doc) = parse_and_validate_screen_str(&raw) {
                            let analysis = analyze_safe_areas(&doc);
                            screens.push(json!({
                                "slug": slug,
                                "path": format!("{UI_DIR_REL}/{fname}"),
                                "name": doc.name,
                                "version": doc.version,
                                "warning_count": analysis.warning_count,
                                "doc": doc.to_json(),
                            }));
                        }
                    }
                }
            }
        }
    }
    Ok(json!({
        "screens": screens,
        "presets": list_presets_metadata(),
    }))
}

pub fn load_project_screen(dir: &Path, slug: &str) -> Result<(String, ScreenDoc), String> {
    let clean = validate_slug(slug)?;
    let rel = format!("{UI_DIR_REL}/{clean}.screen.json");
    let full = projects::safe_join(dir, &rel)?;
    let meta = std::fs::symlink_metadata(&full)
        .map_err(|_| format!("screen '{clean}' not found at {rel}"))?;
    if meta.file_type().is_symlink() {
        return Err(format!("screen '{clean}' must not be a symlink"));
    }
    if !meta.is_file() {
        return Err(format!("screen '{clean}' not found at {rel}"));
    }
    if meta.len() > MAX_SCREEN_JSON_BYTES as u64 {
        return Err(format!("screen '{clean}' exceeds {MAX_SCREEN_JSON_BYTES} byte limit"));
    }
    let raw = std::fs::read_to_string(&full)
        .map_err(|_| format!("screen '{clean}' not found at {rel}"))?;
    let doc = parse_and_validate_screen_str(&raw)?;
    Ok((clean, doc))
}

pub fn get_project_screen(dir: &Path, slug: &str) -> Result<Value, String> {
    let (clean, doc) = load_project_screen(dir, slug)?;
    let analysis = analyze_safe_areas(&doc);
    Ok(json!({
        "slug": clean,
        "path": format!("{UI_DIR_REL}/{clean}.screen.json"),
        "doc": doc.to_json(),
        "analysis": analysis.to_json(),
        "safe_area_warnings": analysis.warnings.iter().map(SafeAreaWarning::to_json).collect::<Vec<_>>(),
    }))
}

pub fn doc_from_save_body(slug: &str, body: &Value) -> Result<ScreenDoc, String> {
    if let Some(doc_val) = body.get("doc") {
        return parse_and_validate_screen_value(doc_val);
    }
    if let Some(preset_name) = body.get("preset").and_then(Value::as_str) {
        return preset_screen_doc(preset_name, Some(slug));
    }
    if body.get("root").is_some() && body.get("version").is_some() {
        return parse_and_validate_screen_value(body);
    }
    Err("request body must include 'doc', 'preset', or a ScreenDoc v1 object".to_string())
}

pub fn save_project_screen(dir: &Path, slug: &str, body: &Value) -> Result<Value, String> {
    let clean = validate_slug(slug)?;
    let doc = doc_from_save_body(&clean, body)?;
    let rel = format!("{UI_DIR_REL}/{clean}.screen.json");
    let full = projects::safe_join(dir, &rel)?;
    write_atomic_json(&full, &doc.to_json())?;
    let analysis = analyze_safe_areas(&doc);
    Ok(json!({
        "ok": true,
        "slug": clean,
        "path": rel,
        "doc": doc.to_json(),
        "analysis": analysis.to_json(),
        "safe_area_warnings": analysis.warnings.iter().map(SafeAreaWarning::to_json).collect::<Vec<_>>(),
    }))
}

pub fn compile_project_screen(dir: &Path, slug: &str, body: &Value) -> Result<Value, String> {
    let clean = validate_slug(slug)?;
    let doc = if body.get("doc").is_some() || (body.get("root").is_some() && body.get("version").is_some()) {
        doc_from_save_body(&clean, body)?
    } else if let Some(preset_name) = body.get("preset").and_then(Value::as_str) {
        preset_screen_doc(preset_name, Some(&clean))?
    } else {
        load_project_screen(dir, &clean)?.1
    };

    let mut result = compile_screen_doc(&clean, &doc, Some(dir))?;
    let should_write = body
        .get("write")
        .or_else(|| body.get("save_luau"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let emit_model = body
        .get("emit_model_json")
        .or_else(|| body.get("write_model_json"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let custom_out = body
        .get("out_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let luau_rel = if let Some(out_p) = custom_out {
        if out_p.contains('\0')
            || out_p.contains('\\')
            || out_p.chars().any(|c| c.is_control())
        {
            return Err("out_path contains forbidden characters".to_string());
        }
        if !out_p.ends_with(".luau") && !out_p.ends_with(".lua") {
            return Err("out_path must end in .luau or .lua".to_string());
        }
        let check_full = projects::safe_join(dir, out_p)?;
        let shown = projects::rel_display(dir, &check_full);
        let top = shown.split('/').next().unwrap_or("");
        if matches!(top, ".git" | ".openro" | "build")
            || shown.split('/').any(|s| s == ".git" || s == ".openro")
        {
            return Err("out_path cannot target .git, .openro, or build folders".to_string());
        }
        out_p.to_string()
    } else {
        result["luau_path"]
            .as_str()
            .unwrap_or("src/client/ui/ScreenHud.luau")
            .to_string()
    };

    let mut modified_files = Vec::new();
    if should_write {
        let luau_full = projects::safe_join(dir, &luau_rel)?;
        let luau_src = result["luau"].as_str().unwrap_or("");
        write_atomic_bytes(&luau_full, luau_src.as_bytes())?;
        modified_files.push(luau_rel.clone());

        if emit_model {
            let model_rel = result["model_path"]
                .as_str()
                .unwrap_or("ui/screen.model.json")
                .to_string();
            let model_full = projects::safe_join(dir, &model_rel)?;
            write_atomic_json(&model_full, &result["model_json"])?;
            modified_files.push(model_rel);
        }
    }

    if let Some(obj) = result.as_object_mut() {
        obj.insert("luau_path".to_string(), json!(luau_rel));
        obj.insert("written".to_string(), json!(should_write));
        obj.insert("modified_files".to_string(), json!(modified_files));
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Starter HUD Presets (RPG, Obby, Tycoon, Gacha)
// ---------------------------------------------------------------------------

pub fn list_presets_metadata() -> Vec<Value> {
    vec![
        json!({ "id": "rpg", "name": "RPG HUD", "description": "Health/XP status card + bottom-center ability hotbar" }),
        json!({ "id": "obby", "name": "Obby HUD", "description": "Stage progress pill + Skip Stage monetized action" }),
        json!({ "id": "tycoon", "name": "Tycoon HUD", "description": "Cash/Rebirth header + bottom-center Boost Shop bar" }),
        json!({ "id": "gacha", "name": "Gacha HUD", "description": "Gems/Pity banner + Roll x1/x10/Inventory dock" }),
    ]
}

fn make_node(
    id: &str,
    name: &str,
    class_name: &str,
    props: Vec<(&str, Value)>,
    children: Vec<GuiNode>,
) -> GuiNode {
    let mut map = BTreeMap::new();
    for (k, v) in props {
        map.insert(k.to_string(), v);
    }
    GuiNode {
        id: id.to_string(),
        name: name.to_string(),
        class_name: class_name.to_string(),
        props: map,
        children,
    }
}

fn preset_root_frame(children: Vec<GuiNode>) -> GuiNode {
    make_node(
        "root",
        "HudRoot",
        "Frame",
        vec![
            ("Position", json!([[0, 0], [0, 0]])),
            ("Size", json!([[1, 0], [1, 0]])),
            ("BackgroundTransparency", json!(1)),
        ],
        children,
    )
}

fn corner_node(id: &str, px: f64) -> GuiNode {
    make_node(
        id,
        "UICorner",
        "UICorner",
        vec![("CornerRadius", json!([0, px]))],
        vec![],
    )
}

fn stroke_node(id: &str, color: &str, thickness: f64) -> GuiNode {
    make_node(
        id,
        "UIStroke",
        "UIStroke",
        vec![
            ("Color", json!(color)),
            ("Thickness", json!(thickness)),
            ("Transparency", json!(0.35)),
        ],
        vec![],
    )
}

/// Build a safe-area-compliant starter HUD preset (`rpg`, `obby`, `tycoon`, `gacha`).
pub fn preset_screen_doc(preset: &str, slug_override: Option<&str>) -> Result<ScreenDoc, String> {
    let key = preset.trim().to_ascii_lowercase();
    let title = slug_override.map(pascal_case_slug);
    let doc = match key.as_str() {
        "rpg" => ScreenDoc {
            version: 1,
            name: title.unwrap_or_else(|| "RpgHud".to_string()),
            ignore_gui_inset: true,
            reset_on_spawn: false,
            theme: ScreenTheme {
                accent: "#F59E0B".to_string(),
                surface: "#0F172A".to_string(),
                text: "#F8FAFC".to_string(),
            },
            root: preset_root_frame(vec![
                make_node(
                    "status_card",
                    "StatusCard",
                    "Frame",
                    vec![
                        ("AnchorPoint", json!([1, 0])),
                        ("Position", json!([[1, -16], [0, 66]])),
                        ("Size", json!([[0, 236], [0, 64]])),
                        ("BackgroundColor3", json!("#0F172A")),
                        ("BackgroundTransparency", json!(0.15)),
                    ],
                    vec![
                        corner_node("status_corner", 10.0),
                        stroke_node("status_stroke", "#F59E0B", 1.5),
                        make_node(
                            "hp_label",
                            "HpLabel",
                            "TextLabel",
                            vec![
                                ("Position", json!([[0, 12], [0, 8]])),
                                ("Size", json!([[1, -24], [0, 22]])),
                                ("BackgroundTransparency", json!(1)),
                                ("Text", json!("HP 100 / 100")),
                                ("TextColor3", json!("#F8FAFC")),
                                ("TextSize", json!(16)),
                                ("Font", json!("GothamBold")),
                                ("TextXAlignment", json!("Left")),
                            ],
                            vec![],
                        ),
                        make_node(
                            "xp_label",
                            "XpLabel",
                            "TextLabel",
                            vec![
                                ("Position", json!([[0, 12], [0, 34]])),
                                ("Size", json!([[1, -24], [0, 20]])),
                                ("BackgroundTransparency", json!(1)),
                                ("Text", json!("LVL 1  •  0 / 250 XP")),
                                ("TextColor3", json!("#F59E0B")),
                                ("TextSize", json!(14)),
                                ("Font", json!("Gotham")),
                                ("TextXAlignment", json!("Left")),
                            ],
                            vec![],
                        ),
                    ],
                ),
                make_node(
                    "ability_dock",
                    "AbilityDock",
                    "Frame",
                    vec![
                        ("AnchorPoint", json!([0.5, 1])),
                        ("Position", json!([[0.5, 0], [1, -16]])),
                        ("Size", json!([[0, 316], [0, 64]])),
                        ("BackgroundColor3", json!("#0F172A")),
                        ("BackgroundTransparency", json!(0.2)),
                    ],
                    vec![
                        corner_node("dock_corner", 12.0),
                        make_node(
                            "dock_list",
                            "UIListLayout",
                            "UIListLayout",
                            vec![
                                ("FillDirection", json!("Horizontal")),
                                ("HorizontalAlignment", json!("Center")),
                                ("VerticalAlignment", json!("Center")),
                                ("Padding", json!([0, 10])),
                            ],
                            vec![],
                        ),
                        make_node(
                            "strike_btn",
                            "StrikeButton",
                            "TextButton",
                            vec![
                                ("Size", json!([[0, 92], [0, 48]])),
                                ("LayoutOrder", json!(1)),
                                ("BackgroundColor3", json!("#F59E0B")),
                                ("Text", json!("[1] Strike")),
                                ("TextColor3", json!("#0F172A")),
                                ("TextSize", json!(15)),
                                ("Font", json!("GothamBold")),
                                ("Callback", json!("onStrike")),
                            ],
                            vec![corner_node("strike_corner", 8.0)],
                        ),
                        make_node(
                            "dash_btn",
                            "DashButton",
                            "TextButton",
                            vec![
                                ("Size", json!([[0, 92], [0, 48]])),
                                ("LayoutOrder", json!(2)),
                                ("BackgroundColor3", json!("#1E293B")),
                                ("Text", json!("[2] Dash")),
                                ("TextColor3", json!("#F8FAFC")),
                                ("TextSize", json!(15)),
                                ("Font", json!("GothamBold")),
                                ("Callback", json!("onDash")),
                            ],
                            vec![corner_node("dash_corner", 8.0)],
                        ),
                        make_node(
                            "bag_btn",
                            "BagButton",
                            "TextButton",
                            vec![
                                ("Size", json!([[0, 92], [0, 48]])),
                                ("LayoutOrder", json!(3)),
                                ("BackgroundColor3", json!("#1E293B")),
                                ("Text", json!("Inventory")),
                                ("TextColor3", json!("#F8FAFC")),
                                ("TextSize", json!(15)),
                                ("Font", json!("GothamBold")),
                                ("Callback", json!("onToggleInventory")),
                            ],
                            vec![corner_node("bag_corner", 8.0)],
                        ),
                    ],
                ),
            ]),
        },
        "obby" => ScreenDoc {
            version: 1,
            name: title.unwrap_or_else(|| "ObbyHud".to_string()),
            ignore_gui_inset: true,
            reset_on_spawn: false,
            theme: ScreenTheme {
                accent: "#22C55E".to_string(),
                surface: "#0F172A".to_string(),
                text: "#F8FAFC".to_string(),
            },
            root: preset_root_frame(vec![
                make_node(
                    "stage_pill",
                    "StagePill",
                    "Frame",
                    vec![
                        ("AnchorPoint", json!([0.5, 0])),
                        ("Position", json!([[0.5, 0], [0, 66]])),
                        ("Size", json!([[0, 220], [0, 48]])),
                        ("BackgroundColor3", json!("#0F172A")),
                        ("BackgroundTransparency", json!(0.15)),
                    ],
                    vec![
                        corner_node("stage_corner", 12.0),
                        stroke_node("stage_stroke", "#22C55E", 1.5),
                        make_node(
                            "stage_label",
                            "StageLabel",
                            "TextLabel",
                            vec![
                                ("Size", json!([[1, 0], [1, 0]])),
                                ("BackgroundTransparency", json!(1)),
                                ("Text", json!("Stage 1 / 50")),
                                ("TextColor3", json!("#F8FAFC")),
                                ("TextSize", json!(18)),
                                ("Font", json!("GothamBold")),
                            ],
                            vec![],
                        ),
                    ],
                ),
                make_node(
                    "skip_btn",
                    "SkipStageButton",
                    "TextButton",
                    vec![
                        ("AnchorPoint", json!([1, 0])),
                        ("Position", json!([[1, -16], [0, 66]])),
                        ("Size", json!([[0, 148], [0, 48]])),
                        ("BackgroundColor3", json!("#22C55E")),
                        ("Text", json!("Skip Stage")),
                        ("TextColor3", json!("#0F172A")),
                        ("TextSize", json!(16)),
                        ("Font", json!("GothamBold")),
                        ("ProductKey", json!("skip_stage")),
                        ("Callback", json!("onSkipStage")),
                    ],
                    vec![corner_node("skip_corner", 10.0)],
                ),
            ]),
        },
        "tycoon" => ScreenDoc {
            version: 1,
            name: title.unwrap_or_else(|| "TycoonHud".to_string()),
            ignore_gui_inset: true,
            reset_on_spawn: false,
            theme: ScreenTheme {
                accent: "#10B981".to_string(),
                surface: "#111827".to_string(),
                text: "#F9FAFB".to_string(),
            },
            root: preset_root_frame(vec![
                make_node(
                    "cash_card",
                    "CashCard",
                    "Frame",
                    vec![
                        ("AnchorPoint", json!([1, 0])),
                        ("Position", json!([[1, -16], [0, 66]])),
                        ("Size", json!([[0, 224], [0, 56]])),
                        ("BackgroundColor3", json!("#111827")),
                        ("BackgroundTransparency", json!(0.15)),
                    ],
                    vec![
                        corner_node("cash_corner", 10.0),
                        stroke_node("cash_stroke", "#10B981", 1.5),
                        make_node(
                            "cash_label",
                            "CashLabel",
                            "TextLabel",
                            vec![
                                ("Size", json!([[1, -16], [1, 0]])),
                                ("Position", json!([[0, 8], [0, 0]])),
                                ("BackgroundTransparency", json!(1)),
                                ("Text", json!("$1,250  •  Rebirths: 0")),
                                ("TextColor3", json!("#F9FAFB")),
                                ("TextSize", json!(16)),
                                ("Font", json!("GothamBold")),
                            ],
                            vec![],
                        ),
                    ],
                ),
                make_node(
                    "boost_btn",
                    "BoostShopButton",
                    "TextButton",
                    vec![
                        ("AnchorPoint", json!([0.5, 1])),
                        ("Position", json!([[0.5, 0], [1, -16]])),
                        ("Size", json!([[0, 220], [0, 52]])),
                        ("BackgroundColor3", json!("#10B981")),
                        ("Text", json!("2x Cash Boost")),
                        ("TextColor3", json!("#111827")),
                        ("TextSize", json!(16)),
                        ("Font", json!("GothamBold")),
                        ("ProductKey", json!("cash_boost_2x")),
                        ("Callback", json!("onOpenBoostShop")),
                    ],
                    vec![corner_node("boost_corner", 10.0)],
                ),
            ]),
        },
        "gacha" => ScreenDoc {
            version: 1,
            name: title.unwrap_or_else(|| "GachaHud".to_string()),
            ignore_gui_inset: true,
            reset_on_spawn: false,
            theme: ScreenTheme {
                accent: "#8B5CF6".to_string(),
                surface: "#0F172A".to_string(),
                text: "#F8FAFC".to_string(),
            },
            root: preset_root_frame(vec![
                make_node(
                    "gems_banner",
                    "GemsBanner",
                    "Frame",
                    vec![
                        ("AnchorPoint", json!([1, 0])),
                        ("Position", json!([[1, -16], [0, 66]])),
                        ("Size", json!([[0, 240], [0, 56]])),
                        ("BackgroundColor3", json!("#0F172A")),
                        ("BackgroundTransparency", json!(0.15)),
                    ],
                    vec![
                        corner_node("gems_corner", 10.0),
                        stroke_node("gems_stroke", "#8B5CF6", 1.5),
                        make_node(
                            "gem_icon",
                            "GemIcon",
                            "ImageLabel",
                            vec![
                                ("Position", json!([[0, 10], [0, 10]])),
                                ("Size", json!([[0, 36], [0, 36]])),
                                ("BackgroundTransparency", json!(1)),
                                ("AssetKey", json!("gem_icon")),
                                ("Image", json!("rbxassetid://0")),
                            ],
                            vec![],
                        ),
                        make_node(
                            "gems_label",
                            "GemsLabel",
                            "TextLabel",
                            vec![
                                ("Position", json!([[0, 54], [0, 8]])),
                                ("Size", json!([[1, -64], [0, 40]])),
                                ("BackgroundTransparency", json!(1)),
                                ("Text", json!("500 Gems  •  Pity 0/50")),
                                ("TextColor3", json!("#F8FAFC")),
                                ("TextSize", json!(15)),
                                ("Font", json!("GothamBold")),
                                ("TextXAlignment", json!("Left")),
                            ],
                            vec![],
                        ),
                    ],
                ),
                make_node(
                    "roll_dock",
                    "RollDock",
                    "Frame",
                    vec![
                        ("AnchorPoint", json!([0.5, 1])),
                        ("Position", json!([[0.5, 0], [1, -16]])),
                        ("Size", json!([[0, 356], [0, 64]])),
                        ("BackgroundColor3", json!("#0F172A")),
                        ("BackgroundTransparency", json!(0.2)),
                    ],
                    vec![
                        corner_node("roll_dock_corner", 12.0),
                        make_node(
                            "roll_list",
                            "UIListLayout",
                            "UIListLayout",
                            vec![
                                ("FillDirection", json!("Horizontal")),
                                ("HorizontalAlignment", json!("Center")),
                                ("VerticalAlignment", json!("Center")),
                                ("Padding", json!([0, 10])),
                            ],
                            vec![],
                        ),
                        make_node(
                            "roll_one_btn",
                            "RollOneButton",
                            "TextButton",
                            vec![
                                ("Size", json!([[0, 104], [0, 48]])),
                                ("LayoutOrder", json!(1)),
                                ("BackgroundColor3", json!("#8B5CF6")),
                                ("Text", json!("Roll x1")),
                                ("TextColor3", json!("#F8FAFC")),
                                ("TextSize", json!(15)),
                                ("Font", json!("GothamBold")),
                                ("Callback", json!("onRollOne")),
                            ],
                            vec![corner_node("r1_corner", 8.0)],
                        ),
                        make_node(
                            "roll_ten_btn",
                            "RollTenButton",
                            "TextButton",
                            vec![
                                ("Size", json!([[0, 108], [0, 48]])),
                                ("LayoutOrder", json!(2)),
                                ("BackgroundColor3", json!("#EC4899")),
                                ("Text", json!("Roll x10")),
                                ("TextColor3", json!("#F8FAFC")),
                                ("TextSize", json!(15)),
                                ("Font", json!("GothamBold")),
                                ("ProductKey", json!("roll_ten_pack")),
                                ("Callback", json!("onRollTen")),
                            ],
                            vec![corner_node("r10_corner", 8.0)],
                        ),
                        make_node(
                            "index_btn",
                            "IndexButton",
                            "TextButton",
                            vec![
                                ("Size", json!([[0, 104], [0, 48]])),
                                ("LayoutOrder", json!(3)),
                                ("BackgroundColor3", json!("#1E293B")),
                                ("Text", json!("Cards")),
                                ("TextColor3", json!("#F8FAFC")),
                                ("TextSize", json!(15)),
                                ("Font", json!("GothamBold")),
                                ("Callback", json!("onOpenIndex")),
                            ],
                            vec![corner_node("idx_corner", 8.0)],
                        ),
                    ],
                ),
            ]),
        },
        other => {
            return Err(format!(
                "unknown HUD preset '{other}' (expected rpg, obby, tycoon, or gacha)"
            ))
        }
    };
    validate_screen_doc(doc)
}
