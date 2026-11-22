//! Versioned Place Snapshots, Instance/Script Build Diffing, and Defensive
//! Roblox Studio `.rbxlx` XML Import Parser.
//!
//! Snapshots persist atomically to `data_dir/snapshots.json` (bounded to
//! `MAX_SNAPSHOTS` entries) so creators can inspect instance-level diffs,
//! restore any earlier build cleanly, or export any historical snapshot as
//! genuine `<roblox version="4">` XML.

use crate::place::serialize_rbxlx;
use crate::server::backup_corrupt;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Cap persisted snapshot history so memory and disk stay bounded at scale.
pub const MAX_SNAPSHOTS: usize = 64;
/// Guard against pathological deeply-nested XML payloads.
const MAX_XML_DEPTH: usize = 64;
/// Guard against XML bombs creating tens of thousands of nodes.
const MAX_XML_ITEMS: usize = 10_000;

static SNAP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_snapshot_id() -> String {
    let n = SNAP_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("snap-{}-{n}", now_ms())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaceDiff {
    pub added_instances: Vec<String>,
    pub modified_instances: Vec<String>,
    pub removed_instances: Vec<String>,
    pub added_scripts: Vec<String>,
    pub modified_scripts: Vec<String>,
    pub removed_scripts: Vec<String>,
}

impl PlaceDiff {
    pub fn total_changes(&self) -> usize {
        self.added_instances.len()
            + self.modified_instances.len()
            + self.removed_instances.len()
            + self.added_scripts.len()
            + self.modified_scripts.len()
            + self.removed_scripts.len()
    }

    pub fn summary(&self) -> String {
        let added = self.added_instances.len() + self.added_scripts.len();
        let modified = self.modified_instances.len() + self.modified_scripts.len();
        let removed = self.removed_instances.len() + self.removed_scripts.len();
        if added == 0 && modified == 0 && removed == 0 {
            return "No changes".to_string();
        }
        let mut parts = Vec::new();
        if added > 0 {
            parts.push(format!("+{added} added"));
        }
        if modified > 0 {
            parts.push(format!("~{modified} modified"));
        }
        if removed > 0 {
            parts.push(format!("-{removed} removed"));
        }
        parts.join(", ")
    }

    pub fn to_json(&self) -> Value {
        json!({
            "added_instances": self.added_instances,
            "modified_instances": self.modified_instances,
            "removed_instances": self.removed_instances,
            "added_scripts": self.added_scripts,
            "modified_scripts": self.modified_scripts,
            "removed_scripts": self.removed_scripts,
            "total_changes": self.total_changes(),
            "summary": self.summary(),
        })
    }

    pub fn from_json(v: &Value) -> Option<Self> {
        let list = |k: &str| -> Option<Vec<String>> {
            v.get(k)?
                .as_array()?
                .iter()
                .map(|x| x.as_str().map(String::from))
                .collect()
        };
        Some(PlaceDiff {
            added_instances: list("added_instances")?,
            modified_instances: list("modified_instances")?,
            removed_instances: list("removed_instances")?,
            added_scripts: list("added_scripts")?,
            modified_scripts: list("modified_scripts")?,
            removed_scripts: list("removed_scripts")?,
        })
    }
}

/// Compute an exact sorted instance- and script-level diff between two place trees.
pub fn compute_diff(before: &Value, after: &Value) -> PlaceDiff {
    let mut diff = PlaceDiff::default();

    let empty = Map::new();
    let b_inst = before
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let a_inst = after
        .get("instances")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);

    for (k, av) in a_inst {
        match b_inst.get(k) {
            None => diff.added_instances.push(k.clone()),
            Some(bv) if bv != av => diff.modified_instances.push(k.clone()),
            Some(_) => {}
        }
    }
    for k in b_inst.keys() {
        if !a_inst.contains_key(k) {
            diff.removed_instances.push(k.clone());
        }
    }

    let b_sc = before
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let a_sc = after
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);

    for (k, av) in a_sc {
        match b_sc.get(k) {
            None => diff.added_scripts.push(k.clone()),
            Some(bv) if bv != av => diff.modified_scripts.push(k.clone()),
            Some(_) => {}
        }
    }
    for k in b_sc.keys() {
        if !a_sc.contains_key(k) {
            diff.removed_scripts.push(k.clone());
        }
    }

    diff.added_instances.sort();
    diff.modified_instances.sort();
    diff.removed_instances.sort();
    diff.added_scripts.sort();
    diff.modified_scripts.sort();
    diff.removed_scripts.sort();
    diff
}

#[derive(Clone, Debug)]
pub struct PlaceSnapshot {
    pub id: String,
    pub created_at_ms: u64,
    pub label: String,
    pub source: String,
    pub chat_id: Option<String>,
    pub instance_count: usize,
    pub script_count: usize,
    pub tree: Value,
    pub diff: PlaceDiff,
}

impl PlaceSnapshot {
    pub fn summary_json(&self) -> Value {
        json!({
            "id": self.id,
            "created_at_ms": self.created_at_ms,
            "label": self.label,
            "source": self.source,
            "chat_id": self.chat_id,
            "instance_count": self.instance_count,
            "script_count": self.script_count,
            "diff": self.diff.to_json(),
        })
    }

    pub fn full_json(&self) -> Value {
        let mut obj = self.summary_json();
        obj["tree"] = self.tree.clone();
        obj
    }

    fn from_json(v: &Value) -> Option<Self> {
        let id = v.get("id")?.as_str()?.to_string();
        let created_at_ms = v.get("created_at_ms")?.as_u64()?;
        let label = v.get("label")?.as_str()?.to_string();
        let source = v.get("source")?.as_str()?.to_string();
        let chat_id = match v.get("chat_id") {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Null) | None => None,
            _ => return None,
        };
        let tree = v.get("tree")?.clone();
        if !tree.is_object() {
            return None;
        }
        let instance_count = tree
            .get("instances")
            .and_then(|x| x.as_object())
            .map(|m| m.len())
            .unwrap_or(0);
        let script_count = tree
            .get("scripts")
            .and_then(|x| x.as_object())
            .map(|m| m.len())
            .unwrap_or(0);
        let diff = v
            .get("diff")
            .and_then(PlaceDiff::from_json)
            .unwrap_or_default();
        Some(PlaceSnapshot {
            id,
            created_at_ms,
            label,
            source,
            chat_id,
            instance_count,
            script_count,
            tree,
            diff,
        })
    }
}

pub struct SnapshotStore {
    snapshots: Vec<PlaceSnapshot>,
    path: Option<PathBuf>,
    pub recovered: Vec<String>,
}

impl Default for SnapshotStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl SnapshotStore {
    pub fn in_memory() -> Self {
        SnapshotStore {
            snapshots: Vec::new(),
            path: None,
            recovered: Vec::new(),
        }
    }

    pub fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("snapshots.json");
        let mut recovered = Vec::new();
        let snapshots = load_snapshots(&path, &mut recovered);
        SnapshotStore {
            snapshots,
            path: Some(path),
            recovered,
        }
    }

    pub fn record(
        &mut self,
        label: impl Into<String>,
        source: impl Into<String>,
        chat_id: Option<String>,
        tree: Value,
        explicit_diff: Option<PlaceDiff>,
    ) -> PlaceSnapshot {
        let normalized = normalize_tree(&tree);
        let diff = explicit_diff.unwrap_or_else(|| {
            let empty_tree = json!({ "instances": {}, "scripts": {} });
            let prev = self
                .snapshots
                .last()
                .map(|s| &s.tree)
                .unwrap_or(&empty_tree);
            compute_diff(prev, &normalized)
        });
        let instance_count = normalized["instances"]
            .as_object()
            .map(|m| m.len())
            .unwrap_or(0);
        let script_count = normalized["scripts"]
            .as_object()
            .map(|m| m.len())
            .unwrap_or(0);
        let snap = PlaceSnapshot {
            id: new_snapshot_id(),
            created_at_ms: now_ms(),
            label: label.into(),
            source: source.into(),
            chat_id,
            instance_count,
            script_count,
            tree: normalized,
            diff,
        };
        self.snapshots.push(snap.clone());
        if self.snapshots.len() > MAX_SNAPSHOTS {
            let drop_n = self.snapshots.len() - MAX_SNAPSHOTS;
            self.snapshots.drain(0..drop_n);
        }
        self.persist();
        snap
    }

    pub fn list(&self, include_tree: bool) -> Vec<Value> {
        self.snapshots
            .iter()
            .rev()
            .map(|s| {
                if include_tree {
                    s.full_json()
                } else {
                    s.summary_json()
                }
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<&PlaceSnapshot> {
        self.snapshots.iter().find(|s| s.id == id)
    }

    pub fn latest_diff(&self) -> Value {
        match self.snapshots.last() {
            Some(s) => json!({
                "snapshot_id": s.id,
                "label": s.label,
                "created_at_ms": s.created_at_ms,
                "diff": s.diff.to_json(),
            }),
            None => json!({
                "snapshot_id": Value::Null,
                "label": "No builds yet",
                "created_at_ms": 0,
                "diff": PlaceDiff::default().to_json(),
            }),
        }
    }

    pub fn export_rbxlx(&self, id: &str, game_name: &str) -> Option<String> {
        let snap = self.get(id)?;
        Some(serialize_rbxlx(game_name, &snap.tree))
    }

    fn persist(&self) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = json!({
            "snapshots": self.snapshots.iter().map(|s| s.full_json()).collect::<Vec<_>>(),
        });
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, body.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

fn load_snapshots(path: &Path, recovered: &mut Vec<String>) -> Vec<PlaceSnapshot> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let parsed = (|| -> Option<Vec<PlaceSnapshot>> {
        let v: Value = serde_json::from_str(&text).ok()?;
        let arr = v.get("snapshots")?.as_array()?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            out.push(PlaceSnapshot::from_json(item)?);
        }
        Some(out)
    })();
    match parsed {
        Some(list) => list,
        None => {
            if let Some(bak) = backup_corrupt(path) {
                recovered.push(
                    bak.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            Vec::new()
        }
    }
}

pub fn normalize_tree(tree: &Value) -> Value {
    let instances = tree
        .get("instances")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let scripts = tree
        .get("scripts")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    json!({
        "instances": Value::Object(instances),
        "scripts": Value::Object(scripts),
    })
}

/// Defensive Roblox Studio `.rbxlx` XML parser.
///
/// Rejects empty inputs, XXE (`<!DOCTYPE` / `<!ENTITY`), missing `<roblox>` root,
/// mismatched/unclosed tags, excessive nesting, and item bombs with a clean `Err`
/// (never panics). Produces a normalized place tree `{"instances": {...}, "scripts": {...}}`.
pub fn parse_rbxlx_xml(xml: &str) -> Result<Value, String> {
    let trimmed = xml.trim();
    if trimmed.is_empty() {
        return Err("empty .rbxlx XML payload".to_string());
    }
    let upper_prefix = trimmed.chars().take(4096).collect::<String>().to_ascii_uppercase();
    if upper_prefix.contains("<!DOCTYPE") || upper_prefix.contains("<!ENTITY") {
        return Err("XML DOCTYPE/ENTITY declarations are not allowed in .rbxlx imports".to_string());
    }
    if !trimmed.contains("<roblox") || !trimmed.contains("</roblox>") {
        return Err("invalid .rbxlx file: missing <roblox>...</roblox> root element".to_string());
    }

    let mut parser = XmlCursor::new(trimmed);
    let root = parser.parse_root()?;
    if root.tag != "roblox" {
        return Err(format!(
            "invalid .rbxlx root element <{}> (expected <roblox>)",
            root.tag
        ));
    }

    let mut instances: BTreeMap<String, Value> = BTreeMap::new();
    let mut scripts: BTreeMap<String, String> = BTreeMap::new();

    for child in &root.children {
        if child.tag == "Item" {
            collect_item(child, None, &mut instances, &mut scripts, 0)?;
        }
    }

    Ok(json!({
        "instances": instances,
        "scripts": scripts,
    }))
}

const SERVICE_CLASSES: [&str; 8] = [
    "Workspace",
    "ServerScriptService",
    "ReplicatedStorage",
    "StarterGui",
    "StarterPlayer",
    "StarterPlayerScripts",
    "Lighting",
    "SoundService",
];

fn collect_item(
    node: &XmlNode,
    parent_path: Option<&str>,
    instances: &mut BTreeMap<String, Value>,
    scripts: &mut BTreeMap<String, String>,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_XML_DEPTH {
        return Err("XML nesting depth exceeds maximum safe limit".to_string());
    }
    let class_name = node
        .attr("class")
        .unwrap_or("Folder")
        .trim()
        .to_string();
    if class_name.is_empty() {
        return Err("an <Item> element is missing its class attribute".to_string());
    }

    let mut name: Option<String> = None;
    let mut source: Option<String> = None;
    let mut props = Map::new();

    if let Some(prop_node) = node.children.iter().find(|c| c.tag == "Properties") {
        for p in &prop_node.children {
            let Some(pname) = p.attr("name") else {
                continue;
            };
            match (p.tag.as_str(), pname) {
                ("string", "Name") => name = Some(p.text.clone()),
                ("ProtectedString", "Source") | ("string", "Source") => {
                    source = Some(p.text.clone());
                }
                ("bool", _) => {
                    let b = p.text.trim().eq_ignore_ascii_case("true");
                    props.insert(pname.to_string(), json!(b));
                }
                ("int" | "int64" | "token", _) => {
                    if let Ok(n) = p.text.trim().parse::<i64>() {
                        props.insert(pname.to_string(), json!(n));
                    }
                }
                ("float" | "double", _) => {
                    if let Ok(n) = p.text.trim().parse::<f64>() {
                        if n.is_finite() {
                            props.insert(pname.to_string(), json!(n));
                        }
                    }
                }
                ("string", _) => {
                    props.insert(pname.to_string(), json!(p.text));
                }
                ("Vector3", _) => {
                    if let Some(vec3) = extract_vec3(p) {
                        props.insert(pname.to_string(), vec3);
                    }
                }
                ("CoordinateFrame", _) => {
                    if let Some(pos) = extract_vec3(p) {
                        props.insert("Position".to_string(), pos);
                    }
                }
                ("Color3uint8", _) => {
                    if let Ok(raw) = p.text.trim().parse::<u32>() {
                        let r = (raw >> 16) & 0xFF;
                        let g = (raw >> 8) & 0xFF;
                        let b = raw & 0xFF;
                        props.insert("Color".to_string(), json!([r, g, b]));
                    }
                }
                ("Color3", _) => {
                    if let Some(rgb) = extract_color3(p) {
                        props.insert("Color".to_string(), rgb);
                    }
                }
                _ => {}
            }
        }
    }

    let item_name = name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| class_name.clone());
    let safe_seg = sanitize_segment(&item_name);

    let full_path = match parent_path {
        None => safe_seg.clone(),
        Some(p) => format!("{p}.{safe_seg}"),
    };

    let is_service_root = parent_path.is_none() && SERVICE_CLASSES.contains(&class_name.as_str());
    let is_script = matches!(
        class_name.as_str(),
        "Script" | "LocalScript" | "ModuleScript"
    );

    if is_script {
        scripts.insert(full_path.clone(), source.unwrap_or_default());
    } else if !is_service_root {
        // Also skip empty StarterPlayer.StarterPlayerScripts container shell if it only groups scripts.
        let is_starter_scripts_shell =
            full_path == "StarterPlayer.StarterPlayerScripts" && class_name == "StarterPlayerScripts";
        if !is_starter_scripts_shell {
            instances.insert(
                full_path.clone(),
                json!({
                    "class_name": class_name,
                    "properties": Value::Object(props),
                }),
            );
        }
    }

    for child in &node.children {
        if child.tag == "Item" {
            collect_item(child, Some(&full_path), instances, scripts, depth + 1)?;
        }
    }
    Ok(())
}

fn sanitize_segment(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| !matches!(c, '.' | '/' | '\\') && !c.is_control())
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "Item".to_string()
    } else {
        trimmed.to_string()
    }
}

fn extract_vec3(node: &XmlNode) -> Option<Value> {
    let find = |t: &str| -> Option<f64> {
        let v = node
            .children
            .iter()
            .find(|c| c.tag == t)?
            .text
            .trim()
            .parse::<f64>()
            .ok()?;
        if v.is_finite() { Some(v) } else { None }
    };
    Some(json!([find("X")?, find("Y")?, find("Z")?]))
}

fn extract_color3(node: &XmlNode) -> Option<Value> {
    let find = |t: &str| -> Option<u8> {
        let v = node
            .children
            .iter()
            .find(|c| c.tag == t)?
            .text
            .trim()
            .parse::<f64>()
            .ok()?;
        if !v.is_finite() {
            return None;
        }
        Some((v.clamp(0.0, 1.0) * 255.0).round() as u8)
    };
    Some(json!([find("R")?, find("G")?, find("B")?]))
}

#[derive(Debug)]
struct XmlNode {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlNode>,
    text: String,
}

impl XmlNode {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

struct XmlCursor<'a> {
    bytes: &'a [u8],
    src: &'a str,
    pos: usize,
    item_count: usize,
}

impl<'a> XmlCursor<'a> {
    fn new(src: &'a str) -> Self {
        XmlCursor {
            bytes: src.as_bytes(),
            src,
            pos: 0,
            item_count: 0,
        }
    }

    fn skip_ws_and_misc(&mut self) -> Result<(), String> {
        loop {
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if self.starts_with("<?") {
                let end = self.find_from("?>").ok_or("unclosed XML processing instruction")?;
                self.pos = end + 2;
                continue;
            }
            if self.starts_with("<!--") {
                let end = self.find_from("-->").ok_or("unclosed XML comment")?;
                self.pos = end + 3;
                continue;
            }
            break;
        }
        Ok(())
    }

    fn starts_with(&self, pat: &str) -> bool {
        self.src[self.pos..].starts_with(pat)
    }

    fn find_from(&self, pat: &str) -> Option<usize> {
        self.src[self.pos..].find(pat).map(|i| self.pos + i)
    }

    fn parse_root(&mut self) -> Result<XmlNode, String> {
        self.skip_ws_and_misc()?;
        if !self.starts_with("<") {
            return Err("expected '<' at start of XML root".to_string());
        }
        let node = self.parse_element(0)?;
        self.skip_ws_and_misc()?;
        Ok(node)
    }

    fn parse_element(&mut self, depth: usize) -> Result<XmlNode, String> {
        if depth > MAX_XML_DEPTH {
            return Err("XML element nesting exceeds maximum depth".to_string());
        }
        if !self.starts_with("<") {
            return Err("expected '<' starting XML element".to_string());
        }
        self.pos += 1; // '<'
        if self.starts_with("!") {
            return Err("unexpected XML declaration inside document".to_string());
        }
        let tag = self.read_name()?;
        if tag == "Item" {
            self.item_count += 1;
            if self.item_count > MAX_XML_ITEMS {
                return Err("too many <Item> elements in .rbxlx import".to_string());
            }
        }

        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            if self.pos >= self.bytes.len() {
                return Err(format!("unclosed start tag <{tag}>"));
            }
            if self.starts_with("/>") {
                self.pos += 2;
                return Ok(XmlNode {
                    tag,
                    attrs,
                    children: Vec::new(),
                    text: String::new(),
                });
            }
            if self.starts_with(">") {
                self.pos += 1;
                break;
            }
            let k = self.read_name()?;
            self.skip_ws();
            if !self.starts_with("=") {
                return Err(format!("expected '=' after attribute '{k}' in <{tag}>"));
            }
            self.pos += 1;
            self.skip_ws();
            let v = self.read_attr_value()?;
            attrs.push((k, v));
        }

        let mut children = Vec::new();
        let mut text = String::new();
        loop {
            if self.pos >= self.bytes.len() {
                return Err(format!("unclosed XML element <{tag}>"));
            }
            if self.starts_with("</") {
                self.pos += 2;
                let end_tag = self.read_name()?;
                self.skip_ws();
                if !self.starts_with(">") {
                    return Err(format!("malformed closing tag </{end_tag}>"));
                }
                self.pos += 1;
                if end_tag != tag {
                    return Err(format!("mismatched closing tag </{end_tag}> for <{tag}>"));
                }
                break;
            }
            if self.starts_with("<![CDATA[") {
                self.pos += 9;
                let end = self.find_from("]]>").ok_or("unclosed <![CDATA[ section")?;
                text.push_str(&self.src[self.pos..end]);
                self.pos = end + 3;
                continue;
            }
            if self.starts_with("<!--") {
                let end = self.find_from("-->").ok_or("unclosed XML comment")?;
                self.pos = end + 3;
                continue;
            }
            if self.starts_with("<") {
                children.push(self.parse_element(depth + 1)?);
                continue;
            }
            let next_lt = self.find_from("<").unwrap_or(self.bytes.len());
            let raw = &self.src[self.pos..next_lt];
            text.push_str(&unescape_xml(raw));
            self.pos = next_lt;
        }

        Ok(XmlNode {
            tag,
            attrs,
            children,
            text,
        })
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn read_name(&mut self) -> Result<String, String> {
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
            return Err("expected XML identifier".to_string());
        }
        Ok(self.src[start..self.pos].to_string())
    }

    fn read_attr_value(&mut self) -> Result<String, String> {
        if self.pos >= self.bytes.len() {
            return Err("unexpected EOF in attribute value".to_string());
        }
        let quote = self.bytes[self.pos];
        if quote != b'"' && quote != b'\'' {
            return Err("attribute value must be quoted".to_string());
        }
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != quote {
            self.pos += 1;
        }
        if self.pos >= self.bytes.len() {
            return Err("unclosed XML attribute quote".to_string());
        }
        let raw = &self.src[start..self.pos];
        self.pos += 1;
        Ok(unescape_xml(raw))
    }
}

fn unescape_xml(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(semi) = after.find(';') {
            let ent = &after[..semi];
            match ent {
                "amp" => out.push('&'),
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    if let Some(ch) = u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32) {
                        out.push(ch);
                    }
                }
                _ if ent.starts_with('#') => {
                    if let Some(ch) = ent[1..].parse::<u32>().ok().and_then(char::from_u32) {
                        out.push(ch);
                    }
                }
                _ => {
                    out.push('&');
                    out.push_str(ent);
                    out.push(';');
                }
            }
            rest = &after[semi + 1..];
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_diff_tracks_added_modified_removed() {
        let before = json!({
            "instances": {
                "Workspace.SpawnPad": { "class_name": "SpawnLocation", "properties": { "Anchored": true } },
                "Workspace.OldPart": { "class_name": "Part", "properties": {} }
            },
            "scripts": {
                "ServerScriptService.Leaderboard": "print(1)",
                "ServerScriptService.OldScript": "print(0)"
            }
        });
        let after = json!({
            "instances": {
                "Workspace.SpawnPad": { "class_name": "SpawnLocation", "properties": { "Anchored": false } },
                "Workspace.JumpPad": { "class_name": "Part", "properties": { "Anchored": true } }
            },
            "scripts": {
                "ServerScriptService.Leaderboard": "print(2)",
                "ServerScriptService.DayNight": "print(3)"
            }
        });
        let d = compute_diff(&before, &after);
        assert_eq!(d.added_instances, vec!["Workspace.JumpPad"]);
        assert_eq!(d.modified_instances, vec!["Workspace.SpawnPad"]);
        assert_eq!(d.removed_instances, vec!["Workspace.OldPart"]);
        assert_eq!(d.added_scripts, vec!["ServerScriptService.DayNight"]);
        assert_eq!(d.modified_scripts, vec!["ServerScriptService.Leaderboard"]);
        assert_eq!(d.removed_scripts, vec!["ServerScriptService.OldScript"]);
        assert_eq!(d.total_changes(), 6);
    }

    #[test]
    fn rbxlx_round_trip_and_malformed_rejection() {
        let tree = json!({
            "instances": {
                "Workspace.SpawnPad": { "class_name": "SpawnLocation", "properties": { "Anchored": true, "Position": [0, 3, 0] } },
                "Workspace.LavaBrick": { "class_name": "Part", "properties": { "Anchored": true, "Color": [220, 60, 40] } }
            },
            "scripts": {
                "ServerScriptService.Leaderboard": "local Players = game:GetService(\"Players\")\nprint(\"a < b & c\")\n",
                "StarterGui.GameGui": "local gui = Instance.new(\"ScreenGui\")\n"
            }
        });
        let xml = serialize_rbxlx("Skyline Obby", &tree);
        let parsed = parse_rbxlx_xml(&xml).expect("valid exported .rbxlx parses back");
        assert!(parsed["instances"]["Workspace.SpawnPad"].is_object());
        assert!(parsed["instances"]["Workspace.LavaBrick"].is_object());
        assert!(parsed["scripts"]["ServerScriptService.Leaderboard"]
            .as_str()
            .unwrap()
            .contains("a < b & c"));
        assert!(parsed["scripts"]["StarterGui.GameGui"]
            .as_str()
            .unwrap()
            .contains("ScreenGui"));

        // Malformed / XXE XML rejected cleanly without panic.
        assert!(parse_rbxlx_xml("").is_err());
        assert!(parse_rbxlx_xml("<!DOCTYPE foo [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]><roblox></roblox>").is_err());
        assert!(parse_rbxlx_xml("<roblox><Item class=\"Part\"></roblox>").is_err());
        assert!(parse_rbxlx_xml("<notroblox></notroblox>").is_err());
    }

    #[test]
    fn snapshot_store_persists_and_bounds_history() {
        let dir = std::env::temp_dir().join(format!("openro-snap-test-{}", now_ms()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = SnapshotStore::open(&dir);
        for i in 0..(MAX_SNAPSHOTS + 5) {
            store.record(
                format!("Build {i}"),
                "build",
                Some("chat-1".to_string()),
                json!({ "instances": {}, "scripts": { "ServerScriptService.S": format!("-- {i}") } }),
                None,
            );
        }
        assert_eq!(store.list(false).len(), MAX_SNAPSHOTS);
        let reloaded = SnapshotStore::open(&dir);
        assert_eq!(reloaded.list(false).len(), MAX_SNAPSHOTS);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
