//! Projects: a folder with a Rojo layout plus the optional `openro.project.json`
//! manifest. This module owns the workspace list, the manifest, a loader for the
//! Rojo project tree and the build to `.rbxlx` (real `rojo build` when present,
//! otherwise a small built-in writer). Sync, playtest and Open Cloud live in
//! their own modules and read projects through here.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MANIFEST_FILE: &str = "openro.project.json";
pub const DEFAULT_ROJO_FILE: &str = "default.project.json";

static ID_SEQ: AtomicU64 = AtomicU64::new(1);

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Collision-free, lexicographically sortable id (`<prefix>-<ms>-<seq>`).
pub fn unique_id(prefix: &str) -> String {
    let at = now_ms();
    let seq = ID_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{at:013}-{seq:04}")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Lower-case folder-style id: letters and digits, single dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Resolve `rel` inside `root`, refusing anything that leaves it (`..`, absolute
/// paths, or a symlink that points outside). The target need not exist yet.
pub fn safe_join(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err("path is empty".into());
    }
    let p = Path::new(rel);
    if p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err(format!("path '{rel}' must stay inside the project folder"));
    }
    let joined = root.join(p);
    // Reject any symlink on any existing path component (including dangling or in-tree symlinks).
    let mut walk = root.to_path_buf();
    for c in p.components() {
        if let Component::Normal(seg) = c {
            walk.push(seg);
            if let Ok(meta) = std::fs::symlink_metadata(&walk) {
                if meta.file_type().is_symlink() {
                    return Err(format!("path '{rel}' contains a symlink and must stay inside the project folder"));
                }
            }
        }
    }
    let root_canon = root.canonicalize().map_err(|e| format!("project folder: {e}"))?;
    // Canonicalise the deepest existing ancestor to catch symlinks that leave the folder.
    let mut probe = joined.clone();
    while !probe.exists() {
        if !probe.pop() {
            break;
        }
    }
    let canon = probe.canonicalize().map_err(|e| format!("path '{rel}': {e}"))?;
    if !canon.starts_with(&root_canon) {
        return Err(format!("path '{rel}' must stay inside the project folder"));
    }
    Ok(joined)
}

/// Project-relative display path with `/` separators (normalizes away `.` segments).
pub fn rel_display(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// `*` matches within one path segment, `**` matches any depth.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn go(p: &[&str], s: &[&str]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(&"**"), _) => go(&p[1..], s) || (!s.is_empty() && go(p, &s[1..])),
            (Some(pp), Some(ss)) => seg_match(pp.as_bytes(), ss.as_bytes()) && go(&p[1..], &s[1..]),
            _ => false,
        }
    }
    fn seg_match(p: &[u8], s: &[u8]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some(b'*') => seg_match(&p[1..], s) || (!s.is_empty() && seg_match(p, &s[1..])),
            Some(c) => s.first() == Some(c) && seg_match(&p[1..], &s[1..]),
        }
    }
    let p: Vec<&str> = pattern.split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    go(&p, &s)
}

// ---------------------------------------------------------------- workspace

#[derive(Default)]
pub struct ProjectStore {
    file: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    pub roots: Vec<PathBuf>,
    pub opened: Vec<PathBuf>,
    pub active: Option<String>,
    env_roots: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct ProjectRef {
    pub id: String,
    pub dir: PathBuf,
}

fn is_project_dir(dir: &Path) -> bool {
    dir.join(MANIFEST_FILE).is_file() || dir.join(DEFAULT_ROJO_FILE).is_file()
}

impl ProjectStore {
    pub fn load(data_dir: Option<&Path>) -> ProjectStore {
        let mut s = ProjectStore {
            file: data_dir.map(|d| d.join("projects.json")),
            data_dir: data_dir.map(|d| d.to_path_buf()),
            ..Default::default()
        };
        if let Ok(v) = std::env::var("OPENRO_WORKSPACE") {
            s.env_roots = v.split(':').filter(|p| !p.trim().is_empty()).map(PathBuf::from).collect();
        }
        if let Some(file) = &s.file {
            if let Ok(text) = std::fs::read_to_string(file) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    let paths = |k: &str| -> Vec<PathBuf> {
                        v.get(k)
                            .and_then(|x| x.as_array())
                            .map(|a| a.iter().filter_map(|p| p.as_str().map(PathBuf::from)).collect())
                            .unwrap_or_default()
                    };
                    s.roots = paths("roots");
                    s.opened = paths("opened");
                    s.active = v.get("active").and_then(|a| a.as_str()).map(String::from);
                }
            }
        }
        s
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        let v = json!({
            "roots": self.roots.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
            "opened": self.opened.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
            "active": self.active,
        });
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, v.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, file);
        }
    }

    pub fn all_roots(&self) -> Vec<PathBuf> {
        let mut out = self.env_roots.clone();
        for r in &self.roots {
            if !out.contains(r) {
                out.push(r.clone());
            }
        }
        out
    }

    pub fn add_root(&mut self, path: &str) -> Result<(), String> {
        let p = PathBuf::from(path.trim());
        if !p.is_absolute() {
            return Err("give the full path of the folder".into());
        }
        let p = p.canonicalize().map_err(|_| format!("'{path}' is not a folder on this machine"))?;
        if !p.is_dir() {
            return Err(format!("'{path}' is not a folder"));
        }
        if !self.roots.contains(&p) {
            self.roots.push(p);
        }
        self.save();
        Ok(())
    }

    pub fn remove_root(&mut self, path: &str) {
        let p = PathBuf::from(path.trim());
        let canon = p.canonicalize().unwrap_or(p);
        self.roots.retain(|r| *r != canon);
        self.save();
    }

    /// Register one project folder and make it the active project.
    pub fn open(&mut self, path: &str) -> Result<ProjectRef, String> {
        let p = PathBuf::from(path.trim());
        if !p.is_absolute() {
            return Err("give the full path of the project folder".into());
        }
        let p = p.canonicalize().map_err(|_| format!("'{path}' is not a folder on this machine"))?;
        if !is_project_dir(&p) {
            return Err(format!("'{path}' has no {DEFAULT_ROJO_FILE} or {MANIFEST_FILE}"));
        }
        if !self.opened.contains(&p) {
            self.opened.push(p.clone());
        }
        let id = self.list().into_iter().find(|r| r.dir == p).map(|r| r.id).ok_or("project not found")?;
        self.active = Some(id.clone());
        self.save();
        Ok(ProjectRef { id, dir: p })
    }

    pub fn set_active(&mut self, id: &str) {
        self.active = Some(id.to_string());
        self.save();
    }

    /// Every known project, ids made unique in a stable order.
    pub fn list(&self) -> Vec<ProjectRef> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let push = |d: PathBuf, dirs: &mut Vec<PathBuf>| {
            if let Ok(c) = d.canonicalize() {
                if is_project_dir(&c) && !dirs.contains(&c) {
                    dirs.push(c);
                }
            }
        };
        for root in self.all_roots() {
            push(root.clone(), &mut dirs);
            if let Ok(rd) = std::fs::read_dir(&root) {
                let mut kids: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
                kids.sort();
                for k in kids {
                    if !k.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(true) {
                        push(k, &mut dirs);
                    }
                }
            }
        }
        for o in &self.opened {
            push(o.clone(), &mut dirs);
        }
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        dirs.into_iter()
            .map(|dir| {
                let base = slug(&dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                let base = if base.is_empty() { "project".to_string() } else { base };
                let n = seen.entry(base.clone()).or_insert(0);
                *n += 1;
                let id = if *n == 1 { base } else { format!("{base}-{n}") };
                ProjectRef { id, dir }
            })
            .collect()
    }

    pub fn find(&self, id: &str) -> Option<ProjectRef> {
        self.list().into_iter().find(|r| r.id == id)
    }

    /// Create a Rojo-layout project under a registered workspace folder.
    pub fn create(&mut self, name: &str, parent: Option<&str>, universe: Option<u64>, place: Option<u64>) -> Result<ProjectRef, String> {
        let name = name.trim();
        let valid = !name.is_empty()
            && name.chars().count() <= 64
            && name.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '_');
        if !valid {
            return Err("project name must be 1-64 letters, digits, spaces, dashes or underscores".into());
        }
        let folder = slug(name);
        if folder.is_empty() {
            return Err("project name needs at least one letter or digit".into());
        }
        let parent_dir = match parent {
            Some(p) => {
                let want = PathBuf::from(p.trim()).canonicalize().map_err(|_| format!("'{p}' is not a folder"))?;
                let known = self.all_roots().iter().any(|r| r.canonicalize().map(|c| c == want).unwrap_or(false));
                if !known {
                    return Err("that folder is not a workspace folder; add it first".into());
                }
                want
            }
            None => match self.all_roots().first() {
                Some(r) => r.clone(),
                None => {
                    let d = self.data_dir.clone().ok_or("add a workspace folder first")?.join("projects");
                    std::fs::create_dir_all(&d).map_err(|e| format!("create workspace folder: {e}"))?;
                    let d = d.canonicalize().map_err(|e| e.to_string())?;
                    self.roots.push(d.clone());
                    d
                }
            },
        };
        let dir = parent_dir.join(&folder);
        if dir.exists() {
            return Err(format!("a folder named '{folder}' already exists there"));
        }
        for d in ["src/server", "src/client", "src/shared/Config"] {
            std::fs::create_dir_all(dir.join(d)).map_err(|e| format!("create {d}: {e}"))?;
        }
        let rojo = json!({
            "name": name,
            "tree": {
                "$className": "DataModel",
                "$ignoreUnknownInstances": true,
                "ReplicatedStorage": {
                    "$ignoreUnknownInstances": true,
                    "Shared": { "$path": "src/shared" }
                },
                "ServerScriptService": {
                    "$ignoreUnknownInstances": true,
                    "Server": { "$path": "src/server" }
                },
                "StarterPlayer": {
                    "$ignoreUnknownInstances": true,
                    "StarterPlayerScripts": {
                        "$className": "StarterPlayerScripts",
                        "$ignoreUnknownInstances": true,
                        "Client": { "$path": "src/client" }
                    }
                }
            }
        });
        let write = |rel: &str, text: String| std::fs::write(dir.join(rel), text).map_err(|e| format!("write {rel}: {e}"));
        write(DEFAULT_ROJO_FILE, serde_json::to_string_pretty(&rojo).unwrap() + "\n")?;
        write("src/server/Main.server.luau", "print(\"server ready\")\n".into())?;
        write("src/client/Main.client.luau", "print(\"client ready\")\n".into())?;
        write("src/shared/Config/init.luau", "return {}\n".into())?;
        write("selene.toml", "std = \"roblox\"\n".into())?;
        write(
            "stylua.toml",
            "syntax = \"Luau\"\ncolumn_width = 120\nline_endings = \"Unix\"\nindent_type = \"Tabs\"\nindent_width = 4\nquote_style = \"AutoPreferDouble\"\n".into(),
        )?;
        write(
            ".gitignore",
            "build/\n.openro/\nart/references/\nart/previews/\nrefs/\n*.rbxl\n*.rbxlx\n".into(),
        )?;
        let manifest = json!({
            "version": 1, "name": name, "rojoProject": DEFAULT_ROJO_FILE,
            "universeId": universe, "placeId": place, "clean_room": true, "products": [],
            "assets": { "lockFile": "assets.lock.json", "authorised": [] }
        });
        write_manifest(&dir, &manifest)?;
        let dir = dir.canonicalize().map_err(|e| e.to_string())?;
        if !self.opened.contains(&dir) {
            self.opened.push(dir.clone());
        }
        let id = self.list().into_iter().find(|r| r.dir == dir).map(|r| r.id).ok_or("project not found after create")?;
        self.active = Some(id.clone());
        self.save();
        Ok(ProjectRef { id, dir })
    }
}

// ----------------------------------------------------------------- manifest

pub fn read_manifest_file(dir: &Path) -> Result<Option<Value>, String> {
    let path = dir.join(MANIFEST_FILE);
    match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).map(Some).map_err(|e| format!("{MANIFEST_FILE} is not valid JSON: {e}")),
        Err(_) => Ok(None),
    }
}

/// The manifest with defaults filled in (what the rest of OpenRO reads).
pub fn manifest(dir: &Path) -> Result<Value, String> {
    let mut m = read_manifest_file(dir)?.unwrap_or_else(|| json!({}));
    let obj = m.as_object_mut().ok_or(format!("{MANIFEST_FILE} must be a JSON object"))?;
    let fallback = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    obj.entry("version").or_insert(json!(1));
    obj.entry("name").or_insert(json!(fallback));
    obj.entry("rojoProject").or_insert(json!(DEFAULT_ROJO_FILE));
    obj.entry("universeId").or_insert(Value::Null);
    obj.entry("placeId").or_insert(Value::Null);
    obj.entry("clean_room").or_insert(json!(true));
    obj.entry("products").or_insert(json!([]));
    let assets = obj.entry("assets").or_insert(json!({}));
    if let Some(a) = assets.as_object_mut() {
        a.entry("lockFile").or_insert(json!("assets.lock.json"));
        a.entry("authorised").or_insert(json!([]));
    }
    Ok(m)
}

pub fn write_manifest(dir: &Path, m: &Value) -> Result<(), String> {
    let path = dir.join(MANIFEST_FILE);
    let tmp = dir.join(format!("{MANIFEST_FILE}.tmp"));
    std::fs::write(&tmp, serde_json::to_string_pretty(m).map_err(|e| e.to_string())? + "\n").map_err(|e| format!("write manifest: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("write manifest: {e}"))
}

fn opt_id(v: &Value, key: &str) -> Result<(), String> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(()),
        Some(x) if x.as_u64().map(|n| n > 0).unwrap_or(false) => Ok(()),
        _ => Err(format!("{key} must be a positive whole number or null")),
    }
}

/// Validate and shallow-merge a manifest patch (`assets` and `playtest` merge one level deep).
pub fn merge_manifest(current: &Value, patch: &Value) -> Result<Value, String> {
    let patch_obj = patch.as_object().ok_or("send a JSON object")?;
    opt_id(patch, "universeId")?;
    opt_id(patch, "placeId")?;
    if let Some(cr) = patch.get("clean_room") {
        if !cr.is_boolean() {
            return Err("clean_room must be a boolean".into());
        }
    }
    if let Some(p) = patch.get("products") {
        let arr = p.as_array().ok_or("products must be a list")?;
        let mut keys = Vec::new();
        for item in arr {
            let key = item.get("key").and_then(|k| k.as_str()).filter(|k| !k.is_empty()).ok_or("every product needs a key")?;
            let kind = item.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            if kind != "developer_product" && kind != "game_pass" {
                return Err(format!("product '{key}': kind must be developer_product or game_pass"));
            }
            if item.get("name").and_then(|n| n.as_str()).filter(|n| !n.is_empty()).is_none() {
                return Err(format!("product '{key}' needs a name"));
            }
            if keys.contains(&key) {
                return Err(format!("product key '{key}' is used twice"));
            }
            keys.push(key);
        }
    }
    let mut out = current.as_object().cloned().unwrap_or_default();
    for (k, v) in patch_obj {
        match (k.as_str(), out.get(k), v) {
            ("assets" | "playtest", Some(Value::Object(old)), Value::Object(new)) => {
                let mut merged = old.clone();
                for (kk, vv) in new {
                    merged.insert(kk.clone(), vv.clone());
                }
                out.insert(k.clone(), Value::Object(merged));
            }
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(Value::Object(out))
}

/// Merge a manifest patch with a price-approval guard (#19).
/// When `allow_price_approval` is `false` (agent calls), `products[].priceApproved`
/// can never be set to `true` by the patch; an already-approved product only retains
/// `priceApproved: true` if its `kind`, `priceRobux`, and `forSale` are unchanged.
pub fn merge_manifest_for_actor(
    current: &Value,
    patch: &Value,
    allow_price_approval: bool,
) -> Result<Value, String> {
    let mut clean_patch = patch.clone();
    if let Some(o) = clean_patch.as_object_mut() {
        o.remove("actor");
    }
    let mut merged = merge_manifest(current, &clean_patch)?;
    if !allow_price_approval {
        let existing: BTreeMap<String, &Value> = current
            .get("products")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|p| p.get("key").and_then(|k| k.as_str()).map(|k| (k.to_string(), p)))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(arr) = merged.get_mut("products").and_then(|v| v.as_array_mut()) {
            for item in arr.iter_mut() {
                let Some(obj) = item.as_object_mut() else { continue };
                let key = obj.get("key").and_then(|k| k.as_str()).unwrap_or("");
                let keep_approved = existing.get(key).is_some_and(|prev| {
                    prev.get("priceApproved").and_then(|b| b.as_bool()) == Some(true)
                        && prev.get("kind") == obj.get("kind")
                        && prev.get("priceRobux") == obj.get("priceRobux")
                        && prev.get("forSale") == obj.get("forSale")
                });
                if keep_approved {
                    obj.insert("priceApproved".into(), json!(true));
                } else {
                    obj.insert("priceApproved".into(), json!(false));
                }
            }
        }
    }
    Ok(merged)
}

// --------------------------------------------------------------------- tree

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub class: String,
    pub props: BTreeMap<String, Value>,
    pub source: Option<String>,
    pub source_file: Option<String>,
    pub children: Vec<Node>,
    pub ignore_unknown: bool,
}

pub struct Tree {
    pub name: String,
    /// Services (depth 1 under the DataModel).
    pub services: Vec<Node>,
    /// Files OpenRO's loader cannot read (binary models etc.); `rojo build` still can.
    pub skipped: Vec<String>,
}

/// One instance of the project, flattened. Paths use `/`.
#[derive(Clone, Debug)]
pub struct Flat {
    pub path: String,
    pub class: String,
    pub props: BTreeMap<String, Value>,
    pub source: Option<String>,
    pub service: bool,
    pub ignore_unknown: bool,
}

pub fn is_script_class(c: &str) -> bool {
    matches!(c, "Script" | "LocalScript" | "ModuleScript")
}

impl Tree {
    pub fn flatten(&self) -> Vec<Flat> {
        self.flatten_with_source().into_iter().map(|(f, _)| f).collect()
    }

    pub fn flatten_with_source(&self) -> Vec<(Flat, Option<String>)> {
        fn go(n: &Node, prefix: &str, service: bool, out: &mut Vec<(Flat, Option<String>)>) {
            let path = if prefix.is_empty() { n.name.clone() } else { format!("{prefix}/{}", n.name) };
            out.push((
                Flat {
                    path: path.clone(),
                    class: n.class.clone(),
                    props: n.props.clone(),
                    source: n.source.clone(),
                    service,
                    ignore_unknown: n.ignore_unknown,
                },
                n.source_file.clone(),
            ));
            for c in &n.children {
                go(c, &path, false, out);
            }
        }
        let mut out = Vec::new();
        for s in &self.services {
            go(s, "", true, &mut out);
        }
        out
    }

    pub fn counts(&self) -> (usize, usize) {
        let flat = self.flatten();
        (flat.len(), flat.iter().filter(|f| is_script_class(&f.class)).count())
    }
}

fn script_class_for(file: &str) -> Option<(&'static str, String)> {
    for ext in [".luau", ".lua"] {
        if let Some(stem) = file.strip_suffix(ext) {
            if let Some(s) = stem.strip_suffix(".server") {
                return Some(("Script", s.to_string()));
            }
            if let Some(s) = stem.strip_suffix(".client") {
                return Some(("LocalScript", s.to_string()));
            }
            return Some(("ModuleScript", stem.to_string()));
        }
    }
    None
}

fn props_from(v: Option<&Value>) -> BTreeMap<String, Value> {
    v.and_then(|p| p.as_object())
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

fn apply_meta(node: &mut Node, meta_path: &Path) -> Result<(), String> {
    let Ok(text) = std::fs::read_to_string(meta_path) else { return Ok(()) };
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", meta_path.display()))?;
    if let Some(c) = v.get("className").and_then(|c| c.as_str()) {
        node.class = c.to_string();
    }
    for (k, val) in props_from(v.get("properties")) {
        node.props.insert(k, val);
    }
    if let Some(b) = v.get("ignoreUnknownInstances").and_then(|b| b.as_bool()) {
        node.ignore_unknown = b;
    }
    Ok(())
}

fn model_json_node(v: &Value, name_hint: Option<&str>, source_file: Option<&str>) -> Result<Node, String> {
    let class = v
        .get("ClassName")
        .or_else(|| v.get("className"))
        .and_then(|c| c.as_str())
        .ok_or("model.json needs ClassName or className")?;
    let name = v
        .get("Name")
        .or_else(|| v.get("name"))
        .and_then(|n| n.as_str())
        .or(name_hint)
        .ok_or("model.json child needs Name or name")?;
    let mut node = Node {
        name: name.to_string(),
        class: class.to_string(),
        props: props_from(v.get("Properties").or_else(|| v.get("properties"))),
        source: None,
        source_file: source_file.map(String::from),
        children: Vec::new(),
        ignore_unknown: false,
    };
    if let Some(kids) = v
        .get("Children")
        .or_else(|| v.get("children"))
        .and_then(|c| c.as_array())
    {
        for k in kids {
            node.children.push(model_json_node(k, None, source_file)?);
        }
    }
    Ok(node)
}

fn load_path(path: &Path, skipped: &mut Vec<String>, root: &Path) -> Result<Option<Node>, String> {
    let fname = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if fname.starts_with('.') {
        return Ok(None);
    }
    if path.is_dir() {
        let mut node = Node {
            name: fname.clone(),
            class: "Folder".into(),
            props: BTreeMap::new(),
            source: None,
            source_file: Some(rel_display(root, path)),
            children: Vec::new(),
            ignore_unknown: false,
        };
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| format!("read {}: {e}", rel_display(root, path)))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        entries.sort();
        let mut init_names: Vec<String> = Vec::new();
        for e in &entries {
            let n = e.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if let Some((class, stem)) = script_class_for(&n) {
                if stem == "init" {
                    node.class = class.to_string();
                    node.source = Some(std::fs::read_to_string(e).map_err(|e2| format!("read {n}: {e2}"))?);
                    node.source_file = Some(rel_display(root, e));
                    init_names.push(n);
                }
            }
        }
        apply_meta(&mut node, &path.join("init.meta.json"))?;
        for e in entries {
            let n = e.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if init_names.contains(&n) || n == "init.meta.json" || n.ends_with(".meta.json") {
                continue;
            }
            if let Some(child) = load_path(&e, skipped, root)? {
                if node.children.iter().any(|c| c.name == child.name) {
                    return Err(format!("two items named '{}' in {}", child.name, rel_display(root, path)));
                }
                node.children.push(child);
            }
        }
        return Ok(Some(node));
    }
    if let Some((class, stem)) = script_class_for(&fname) {
        let mut node = Node {
            name: stem.clone(),
            class: class.to_string(),
            props: BTreeMap::new(),
            source: Some(std::fs::read_to_string(path).map_err(|e| format!("read {fname}: {e}"))?),
            source_file: Some(rel_display(root, path)),
            children: Vec::new(),
            ignore_unknown: false,
        };
        let meta = path.with_file_name(format!("{stem}.meta.json"));
        apply_meta(&mut node, &meta)?;
        return Ok(Some(node));
    }
    if let Some(stem) = fname.strip_suffix(".model.json") {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {fname}: {e}"))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("{fname}: {e}"))?;
        let sf = rel_display(root, path);
        return model_json_node(&v, Some(stem), Some(&sf)).map(Some);
    }
    if let Some(stem) = fname.strip_suffix(".txt") {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {fname}: {e}"))?;
        let mut props = BTreeMap::new();
        props.insert("Value".to_string(), json!(text));
        return Ok(Some(Node {
            name: stem.to_string(),
            class: "StringValue".into(),
            props,
            source: None,
            source_file: Some(rel_display(root, path)),
            children: Vec::new(),
            ignore_unknown: false,
        }));
    }
    if fname.ends_with(".meta.json") {
        return Ok(None);
    }
    skipped.push(rel_display(root, path));
    Ok(None)
}

fn merge_overlay_into_node(
    existing: &mut Node,
    v: &Value,
    root: &Path,
    rojo_file: &str,
    skipped: &mut Vec<String>,
) -> Result<(), String> {
    let Some(obj) = v.as_object() else {
        return Err(format!("'{}' overlay in {rojo_file} must be an object", existing.name));
    };
    if obj.contains_key("$path") || obj.contains_key("$className") {
        return Err(format!("'{}' appears twice under parent", existing.name));
    }
    if let Some(b) = obj.get("$ignoreUnknownInstances").and_then(|b| b.as_bool()) {
        existing.ignore_unknown = b;
    }
    for (k, child) in obj {
        if k.starts_with('$') {
            continue;
        }
        if let Some(pos) = existing.children.iter().position(|c| &c.name == k) {
            merge_overlay_into_node(&mut existing.children[pos], child, root, rojo_file, skipped)
                .map_err(|_| format!("'{k}' appears twice under '{}'", existing.name))?;
        } else {
            existing
                .children
                .push(load_project_node(k, child, root, rojo_file, skipped, false)?);
        }
    }
    Ok(())
}

fn load_project_node(
    name: &str,
    v: &Value,
    root: &Path,
    rojo_file: &str,
    skipped: &mut Vec<String>,
    service_level: bool,
) -> Result<Node, String> {
    let obj = v.as_object().ok_or(format!("'{name}' in the project file must be an object"))?;
    let class_name = obj.get("$className").and_then(|c| c.as_str());
    let loaded_from_model = obj
        .get("$path")
        .and_then(|p| p.as_str())
        .is_some_and(|p| p.ends_with(".model.json"));
    let mut node = if let Some(p) = obj.get("$path").and_then(|p| p.as_str()) {
        let full = safe_join(root, p).map_err(|e| format!("'{name}' $path: {e}"))?;
        if !full.exists() {
            return Err(format!("'{name}' $path '{p}' does not exist"));
        }
        let mut n = load_path(&full, skipped, root)?.ok_or(format!("'{name}' $path '{p}' is not a loadable file or folder"))?;
        n.name = name.to_string();
        n
    } else {
        let class = class_name.map(String::from).or_else(|| service_level.then(|| name.to_string())).ok_or(format!("'{name}' needs $className or $path"))?;
        Node {
            name: name.to_string(),
            class,
            props: BTreeMap::new(),
            source: None,
            source_file: Some(rojo_file.to_string()),
            children: Vec::new(),
            ignore_unknown: true,
        }
    };
    if let Some(c) = class_name {
        node.class = c.to_string();
    }
    for (k, val) in props_from(obj.get("$properties")) {
        node.props.insert(k, val);
    }
    if let Some(b) = obj.get("$ignoreUnknownInstances").and_then(|b| b.as_bool()) {
        node.ignore_unknown = b;
    }
    for (k, child) in obj {
        if k.starts_with('$') {
            continue;
        }
        if let Some(pos) = node.children.iter().position(|c| &c.name == k) {
            if loaded_from_model
                && child
                    .as_object()
                    .is_some_and(|co| !co.contains_key("$path") && !co.contains_key("$className"))
            {
                merge_overlay_into_node(&mut node.children[pos], child, root, rojo_file, skipped)
                    .map_err(|_| format!("'{k}' appears twice under '{name}'"))?;
                continue;
            }
            return Err(format!("'{k}' appears twice under '{name}'"));
        }
        node.children.push(load_project_node(k, child, root, rojo_file, skipped, false)?);
    }
    Ok(node)
}

pub fn load_tree(dir: &Path, rojo_file: &str) -> Result<Tree, String> {
    let file = safe_join(dir, rojo_file)?;
    let text = std::fs::read_to_string(&file).map_err(|_| format!("{rojo_file} not found in the project folder"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{rojo_file} is not valid JSON: {e}"))?;
    let tree = v.get("tree").and_then(|t| t.as_object()).ok_or(format!("{rojo_file} has no \"tree\""))?;
    let mut skipped = Vec::new();
    let mut services = Vec::new();
    for (k, child) in tree {
        if k.starts_with('$') {
            continue;
        }
        services.push(load_project_node(k, child, dir, rojo_file, &mut skipped, true)?);
    }
    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("Project").to_string();
    Ok(Tree { name, services, skipped })
}

// -------------------------------------------------------------------- build

pub fn find_tool(bin: &str, env_override: &str) -> Option<PathBuf> {
    if !env_override.is_empty() {
        if let Ok(p) = std::env::var(env_override) {
            let p = PathBuf::from(p.trim());
            return p.is_file().then_some(p);
        }
    }
    let mut cands: Vec<PathBuf> = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join(bin))
        .collect();
    if let Ok(home) = std::env::var("HOME") {
        cands.push(Path::new(&home).join(".local/bin").join(bin));
    }
    cands.into_iter().find(|p| p.is_file())
}

pub fn find_rojo() -> Option<PathBuf> {
    find_tool("rojo", "OPENRO_ROJO")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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

fn is_color3_property(class_name: &str, key: &str) -> bool {
    if matches!(
        key,
        "Ambient"
            | "OutdoorAmbient"
            | "FogColor"
            | "ColorShift_Top"
            | "ColorShift_Bottom"
            | "Decay"
            | "TextColor3"
            | "BackgroundColor3"
            | "BorderColor3"
            | "ImageColor3"
            | "PlaceholderColor3"
            | "TextStrokeColor3"
    ) {
        return true;
    }
    key == "Color" && !is_base_part_class(class_name)
}

fn enum_token_lookup(enum_type: &str, item: &str) -> Option<u32> {
    let clean = item.trim();
    match enum_type {
        "Material" => match clean.to_ascii_lowercase().as_str() {
            "plastic" => Some(256),
            "smoothplastic" => Some(272),
            "neon" => Some(288),
            "wood" => Some(512),
            "woodplanks" => Some(528),
            "cardboard" => Some(560),
            "carpet" => Some(576),
            "ceramictiles" => Some(592),
            "clayrooftiles" => Some(608),
            "roofshingles" => Some(624),
            "leather" => Some(640),
            "plaster" => Some(656),
            "rubber" => Some(672),
            "marble" => Some(784),
            "basalt" => Some(788),
            "slate" => Some(800),
            "crackedlava" => Some(804),
            "concrete" => Some(816),
            "limestone" => Some(820),
            "granite" => Some(832),
            "pavement" => Some(836),
            "brick" => Some(848),
            "pebble" => Some(864),
            "cobblestone" => Some(880),
            "rock" => Some(896),
            "sandstone" => Some(912),
            "metal" => Some(1040),
            "corrodedmetal" => Some(1056),
            "diamondplate" => Some(1072),
            "foil" => Some(1088),
            "grass" => Some(1280),
            "leafygrass" => Some(1284),
            "sand" => Some(1296),
            "fabric" => Some(1312),
            "snow" => Some(1328),
            "mud" => Some(1344),
            "ground" => Some(1360),
            "asphalt" => Some(1376),
            "salt" => Some(1392),
            "ice" => Some(1536),
            "glacier" => Some(1552),
            "glass" => Some(1568),
            "forcefield" => Some(1584),
            "air" => Some(1792),
            "water" => Some(2048),
            _ => None,
        },
        "PartType" | "Shape" => match clean.to_ascii_lowercase().as_str() {
            "ball" | "sphere" => Some(0),
            "block" | "box" => Some(1),
            "cylinder" => Some(2),
            "wedge" => Some(3),
            "cornerwedge" => Some(4),
            _ => None,
        },
        "NormalId" | "Face" => match clean.to_ascii_lowercase().as_str() {
            "right" => Some(0),
            "top" => Some(1),
            "back" => Some(2),
            "left" => Some(3),
            "bottom" => Some(4),
            "front" => Some(5),
            _ => None,
        },
        "Technology" => match clean.to_ascii_lowercase().as_str() {
            "legacy" => Some(0),
            "voxel" => Some(1),
            "compatibility" => Some(2),
            "shadowmap" => Some(3),
            "future" => Some(4),
            "unified" => Some(5),
            _ => None,
        },
        "SurfaceType" => match clean.to_ascii_lowercase().as_str() {
            "smooth" => Some(0),
            "glue" => Some(1),
            "weld" => Some(2),
            "studs" => Some(3),
            "inlet" => Some(4),
            "universal" => Some(5),
            "hinge" => Some(6),
            "motor" => Some(7),
            "steppingmotor" => Some(8),
            "smoothnooutlines" => Some(10),
            _ => None,
        },
        "CollisionFidelity" => match clean.to_ascii_lowercase().as_str() {
            "default" => Some(0),
            "hull" => Some(1),
            "box" => Some(2),
            "preciseconvexdecomposition" => Some(3),
            _ => None,
        },
        "RenderFidelity" => match clean.to_ascii_lowercase().as_str() {
            "automatic" => Some(0),
            "performance" => Some(1),
            "precise" => Some(2),
            _ => None,
        },
        "SortOrder" => match clean.to_ascii_lowercase().as_str() {
            "name" => Some(0),
            "custom" => Some(1),
            "layoutorder" => Some(2),
            _ => None,
        },
        "FillDirection" => match clean.to_ascii_lowercase().as_str() {
            "horizontal" => Some(0),
            "vertical" => Some(1),
            _ => None,
        },
        "HorizontalAlignment" => match clean.to_ascii_lowercase().as_str() {
            "center" => Some(0),
            "left" => Some(1),
            "right" => Some(2),
            _ => None,
        },
        "VerticalAlignment" => match clean.to_ascii_lowercase().as_str() {
            "center" => Some(0),
            "top" => Some(1),
            "bottom" => Some(2),
            _ => None,
        },
        "ScaleType" => match clean.to_ascii_lowercase().as_str() {
            "stretch" => Some(0),
            "slice" => Some(1),
            "tile" => Some(2),
            "fit" => Some(3),
            "crop" => Some(4),
            _ => None,
        },
        "RollOffMode" => match clean.to_ascii_lowercase().as_str() {
            "inverse" => Some(0),
            "linear" => Some(1),
            "linearsquare" => Some(2),
            "inversetapered" => Some(3),
            _ => None,
        },
        "AlphaMode" => match clean.to_ascii_lowercase().as_str() {
            "overlay" => Some(0),
            "transparency" => Some(1),
            "tintmask" => Some(2),
            _ => None,
        },
        "Font" => match clean.to_ascii_lowercase().as_str() {
            "legacy" => Some(0),
            "arial" => Some(1),
            "arialbold" => Some(2),
            "sourcesans" => Some(3),
            "sourcesansbold" => Some(4),
            "sourcesanssemibold" => Some(5),
            "sourcesanslight" => Some(6),
            "sourcesansitalic" => Some(7),
            "bodoni" => Some(8),
            "garamond" => Some(9),
            "cartoon" => Some(10),
            "code" => Some(11),
            "highway" => Some(12),
            "scifi" => Some(13),
            "arcade" => Some(14),
            "fantasy" => Some(15),
            "antique" => Some(16),
            "gotham" => Some(17),
            "gothammedium" => Some(18),
            "gothambold" => Some(19),
            "gothamblack" => Some(20),
            "roboto" => Some(30),
            "robotomono" => Some(33),
            "fredokaone" => Some(36),
            _ => None,
        },
        _ => None,
    }
}

fn resolve_enum_token(prop_name: &str, val: &Value) -> Option<u32> {
    if let Some(n) = val.as_u64() {
        return Some(n as u32);
    }
    let s = val.as_str()?.trim();
    if let Some(rest) = s.strip_prefix("Enum.") {
        if let Some((enum_ty, item)) = rest.split_once('.') {
            if let Some(tok) = enum_token_lookup(enum_ty, item) {
                return Some(tok);
            }
        }
    }
    let enum_ty = match prop_name {
        "Material" => "Material",
        "Shape" | "shape" => "PartType",
        "Face" | "EmissionDirection" => "NormalId",
        "AlphaMode" => "AlphaMode",
        "Technology" => "Technology",
        "TopSurface" | "BottomSurface" | "FrontSurface" | "BackSurface" | "LeftSurface" | "RightSurface" => "SurfaceType",
        "CollisionFidelity" => "CollisionFidelity",
        "RenderFidelity" => "RenderFidelity",
        "SortOrder" => "SortOrder",
        "FillDirection" => "FillDirection",
        "HorizontalAlignment" => "HorizontalAlignment",
        "VerticalAlignment" => "VerticalAlignment",
        "ScaleType" => "ScaleType",
        "RollOffMode" => "RollOffMode",
        "Font" => "Font",
        _ => return None,
    };
    enum_token_lookup(enum_ty, s)
}

fn format_cframe_xml(name: &str, cf: [f64; 12]) -> String {
    let n = xml_escape(name);
    format!(
        "<CoordinateFrame name=\"{n}\"><X>{}</X><Y>{}</Y><Z>{}</Z><R00>{}</R00><R01>{}</R01><R02>{}</R02><R10>{}</R10><R11>{}</R11><R12>{}</R12><R20>{}</R20><R21>{}</R21><R22>{}</R22></CoordinateFrame>",
        fmt_num(cf[0]),
        fmt_num(cf[1]),
        fmt_num(cf[2]),
        fmt_num(cf[3]),
        fmt_num(cf[4]),
        fmt_num(cf[5]),
        fmt_num(cf[6]),
        fmt_num(cf[7]),
        fmt_num(cf[8]),
        fmt_num(cf[9]),
        fmt_num(cf[10]),
        fmt_num(cf[11]),
    )
}

fn format_content_xml(name: &str, s: &str) -> String {
    let n = xml_escape(name);
    if s.is_empty() {
        format!("<Content name=\"{n}\"><null></null></Content>")
    } else {
        format!("<Content name=\"{n}\"><url>{}</url></Content>", xml_escape(s))
    }
}

fn xml_prop_for_class(
    class_name: &str,
    name: &str,
    v: &Value,
    warnings: &mut Vec<String>,
    owner: &str,
) -> Option<String> {
    let xml_name = if is_base_part_class(class_name) && name == "Size" {
        "size"
    } else if is_base_part_class(class_name) && name == "Shape" {
        "shape"
    } else {
        name
    };
    let n = xml_escape(xml_name);
    let num = |x: &Value| x.as_f64().unwrap_or(0.0);
    let arr = |x: &Value| x.as_array().cloned().unwrap_or_default();

    if name == "Color3uint8" || (is_base_part_class(class_name) && name == "Color") {
        if let Some(packed) = crate::scene::color_to_u32_packed(v) {
            return Some(format!("<Color3uint8 name=\"Color3uint8\">{packed}</Color3uint8>"));
        }
    }
    if is_color3_property(class_name, name) {
        if let Some([r, g, b]) = crate::scene::color_to_rgb_floats(v) {
            return Some(format!(
                "<Color3 name=\"{n}\"><R>{}</R><G>{}</G><B>{}</B></Color3>",
                fmt_num(r),
                fmt_num(g),
                fmt_num(b)
            ));
        }
    }
    if name == "CFrame" {
        if let Some(cf) = crate::scene::parse_cframe_value(v) {
            return Some(format_cframe_xml(xml_name, cf));
        }
    }

    match v {
        Value::Bool(b) => Some(format!("<bool name=\"{n}\">{b}</bool>")),
        Value::Number(x) => {
            if let Some(tok) = resolve_enum_token(name, v) {
                if matches!(
                    name,
                    "Material"
                        | "Shape"
                        | "shape"
                        | "Face"
                        | "Technology"
                        | "TopSurface"
                        | "BottomSurface"
                        | "CollisionFidelity"
                        | "RenderFidelity"
                ) {
                    return Some(format!("<token name=\"{n}\">{tok}</token>"));
                }
            }
            Some(format!("<float name=\"{n}\">{}</float>", fmt_num(x.as_f64().unwrap_or(0.0))))
        }
        Value::String(s) => {
            if is_content_property(name) {
                Some(format_content_xml(xml_name, s))
            } else if let Some(tok) = resolve_enum_token(name, v) {
                Some(format!("<token name=\"{n}\">{tok}</token>"))
            } else {
                Some(format!("<string name=\"{n}\">{}</string>", xml_escape(s)))
            }
        }
        Value::Array(a) => match a.len() {
            3 => Some(format!(
                "<Vector3 name=\"{n}\"><X>{}</X><Y>{}</Y><Z>{}</Z></Vector3>",
                fmt_num(num(&a[0])),
                fmt_num(num(&a[1])),
                fmt_num(num(&a[2]))
            )),
            2 => Some(format!(
                "<Vector2 name=\"{n}\"><X>{}</X><Y>{}</Y></Vector2>",
                fmt_num(num(&a[0])),
                fmt_num(num(&a[1]))
            )),
            _ => {
                warnings.push(format!("{owner}: property {name} needs rojo to build; left out"));
                None
            }
        },
        Value::Object(o) if o.len() == 1 => {
            let (kind, val) = o.iter().next().unwrap();
            match kind.as_str() {
                "CFrame" => {
                    if let Some(cf) = crate::scene::parse_cframe_value(v) {
                        Some(format_cframe_xml(xml_name, cf))
                    } else {
                        warnings.push(format!("{owner}: property {name} (CFrame) is malformed; left out"));
                        None
                    }
                }
                "Enum" => {
                    if let Some(tok) = resolve_enum_token(name, val) {
                        Some(format!("<token name=\"{n}\">{tok}</token>"))
                    } else {
                        warnings.push(format!("{owner}: property {name} (Enum) could not resolve token; left out"));
                        None
                    }
                }
                "Content" => {
                    let s = val.as_str().unwrap_or("");
                    Some(format_content_xml(xml_name, s))
                }
                "Color3uint8" => {
                    if let Some(packed) = crate::scene::color_to_u32_packed(v) {
                        Some(format!("<Color3uint8 name=\"{n}\">{packed}</Color3uint8>"))
                    } else {
                        warnings.push(format!("{owner}: property {name} (Color3uint8) is malformed; left out"));
                        None
                    }
                }
                "Color3" => {
                    if let Some([r, g, b]) = crate::scene::color_to_rgb_floats(v) {
                        Some(format!(
                            "<Color3 name=\"{n}\"><R>{}</R><G>{}</G><B>{}</B></Color3>",
                            fmt_num(r),
                            fmt_num(g),
                            fmt_num(b)
                        ))
                    } else {
                        warnings.push(format!("{owner}: property {name} (Color3) is malformed; left out"));
                        None
                    }
                }
                "Vector3" => {
                    let a = arr(val);
                    if a.len() == 3 {
                        Some(format!(
                            "<Vector3 name=\"{n}\"><X>{}</X><Y>{}</Y><Z>{}</Z></Vector3>",
                            fmt_num(num(&a[0])),
                            fmt_num(num(&a[1])),
                            fmt_num(num(&a[2]))
                        ))
                    } else {
                        warnings.push(format!("{owner}: property {name} is malformed; left out"));
                        None
                    }
                }
                "Vector2" => {
                    let a = arr(val);
                    if a.len() == 2 {
                        Some(format!(
                            "<Vector2 name=\"{n}\"><X>{}</X><Y>{}</Y></Vector2>",
                            fmt_num(num(&a[0])),
                            fmt_num(num(&a[1]))
                        ))
                    } else {
                        warnings.push(format!("{owner}: property {name} is malformed; left out"));
                        None
                    }
                }
                "UDim2" => {
                    let a = arr(val);
                    if a.len() == 2 {
                        let (x, y) = (arr(&a[0]), arr(&a[1]));
                        if x.len() == 2 && y.len() == 2 {
                            return Some(format!(
                                "<UDim2 name=\"{n}\"><XS>{}</XS><XO>{}</XO><YS>{}</YS><YO>{}</YO></UDim2>",
                                fmt_num(num(&x[0])),
                                fmt_num(num(&x[1])),
                                fmt_num(num(&y[0])),
                                fmt_num(num(&y[1]))
                            ));
                        }
                    }
                    warnings.push(format!("{owner}: property {name} is malformed; left out"));
                    None
                }
                "UDim" => {
                    let a = arr(val);
                    if a.len() == 2 {
                        Some(format!(
                            "<UDim name=\"{n}\"><S>{}</S><O>{}</O></UDim>",
                            fmt_num(num(&a[0])),
                            fmt_num(num(&a[1]))
                        ))
                    } else {
                        warnings.push(format!("{owner}: property {name} is malformed; left out"));
                        None
                    }
                }
                "NumberRange" => {
                    let a = arr(val);
                    if a.len() == 2 {
                        Some(format!(
                            "<NumberRange name=\"{n}\">{} {} </NumberRange>",
                            fmt_num(num(&a[0])),
                            fmt_num(num(&a[1]))
                        ))
                    } else {
                        warnings.push(format!("{owner}: property {name} is malformed; left out"));
                        None
                    }
                }
                "BrickColor" => {
                    if let Some(code) = val.as_i64() {
                        Some(format!("<int name=\"{n}\">{code}</int>"))
                    } else {
                        warnings.push(format!("{owner}: property {name} (BrickColor) needs numeric ID; left out"));
                        None
                    }
                }
                "Int32" | "Int64" => {
                    if let Some(i) = val.as_i64() {
                        Some(format!("<int name=\"{n}\">{i}</int>"))
                    } else {
                        warnings.push(format!("{owner}: property {name} is malformed; left out"));
                        None
                    }
                }
                _ => {
                    warnings.push(format!("{owner}: property {name} ({kind}) needs rojo to build; left out"));
                    None
                }
            }
        }
        _ => {
            warnings.push(format!("{owner}: property {name} needs rojo to build; left out"));
            None
        }
    }
}

/// Built-in `.rbxlx` writer supporting scripts, folders, `CFrame`, `Enum` tokens,
/// `Content` URLs (`MeshId`/`TextureID`/`SoundId`), `Color3uint8`, and `Lighting` properties.
pub fn builtin_rbxlx(tree: &Tree, warnings: &mut Vec<String>) -> String {
    fn item(n: &Node, prefix: &str, counter: &mut usize, out: &mut String, warnings: &mut Vec<String>) {
        let path = if prefix.is_empty() { n.name.clone() } else { format!("{prefix}/{}", n.name) };
        *counter += 1;
        out.push_str(&format!(
            "<Item class=\"{}\" referent=\"RBX{}\"><Properties><string name=\"Name\">{}</string>",
            xml_escape(&n.class),
            counter,
            xml_escape(&n.name)
        ));
        if let Some(src) = &n.source {
            out.push_str(&format!(
                "<ProtectedString name=\"Source\"><![CDATA[{}]]></ProtectedString>",
                src.replace("]]>", "]]]]><![CDATA[>")
            ));
        }
        let synth_cframe = is_base_part_class(&n.class)
            && !n.props.contains_key("CFrame")
            && (n.props.contains_key("Position") || n.props.contains_key("Rotation"));
        if synth_cframe {
            let pos = crate::scene::parse_vec3(n.props.get("Position")).unwrap_or([0.0, 0.0, 0.0]);
            let rot = crate::scene::parse_vec3(n.props.get("Rotation")).unwrap_or([0.0, 0.0, 0.0]);
            let cf = crate::scene::cframe_from_pos_rot(pos, rot);
            out.push_str(&format_cframe_xml("CFrame", cf));
        }
        let has_color_u8 = n.props.contains_key("Color3uint8");
        for (k, v) in &n.props {
            if synth_cframe && (k == "Position" || k == "Rotation") {
                continue;
            }
            if is_base_part_class(&n.class) && k == "Color" && has_color_u8 {
                continue;
            }
            if let Some(x) = xml_prop_for_class(&n.class, k, v, warnings, &path) {
                out.push_str(&x);
            }
        }
        out.push_str("</Properties>");
        for c in &n.children {
            item(c, &path, counter, out, warnings);
        }
        out.push_str("</Item>");
    }
    let mut out = String::from("<roblox version=\"4\">\n");
    let mut counter = 0usize;
    for s in &tree.services {
        item(s, "", &mut counter, &mut out, warnings);
        out.push('\n');
    }
    out.push_str("</roblox>\n");
    out
}

/// Load the unified Rojo project workspace bundle for `GET /api/projects/:id/workspace`
/// (`scripts`, `instances`, `meshes`, `lighting`, `diagnostics`, `scene_revision`).
pub fn load_workspace_bundle(dir: &Path, project_id: &str) -> Result<Value, String> {
    let m = manifest(dir)?;
    let rojo_file = m["rojoProject"]
        .as_str()
        .unwrap_or(DEFAULT_ROJO_FILE)
        .to_string();
    let project_name = m["name"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(project_id)
        .to_string();

    let (scene_revision, can_undo, can_redo) = crate::scene::undo_status(dir);
    let mut meshes = crate::scene::scan_project_meshes(dir, project_id);
    let terrain = crate::scene::load_terrain_for_workspace(dir);

    let mut scripts_map: BTreeMap<String, Value> = BTreeMap::new();
    let mut instances_map: BTreeMap<String, Value> = BTreeMap::new();
    let mut luau_scripts: BTreeMap<String, String> = BTreeMap::new();
    let mut luau_instances: BTreeMap<String, Value> = BTreeMap::new();
    let mut service_to_file: BTreeMap<String, String> = BTreeMap::new();

    match load_tree(dir, &rojo_file) {
        Ok(tree) => {
            let lighting = crate::scene::extract_lighting_from_tree(&tree.services);
            let lights = crate::scene::extract_lights_from_tree(&tree.services);
            crate::scene::enrich_meshes_with_surface_appearance(
                dir,
                project_id,
                &mut meshes,
                &tree.services,
            );
            for (f, source_file) in tree.flatten_with_source() {
                let dm_path = f.path.replace('/', ".");
                if let Some(src) = f.source {
                    let rel_path = source_file.unwrap_or_else(|| dm_path.clone());
                    let sha256 = sha256_hex(src.as_bytes());
                    let lines = if src.is_empty() { 0 } else { src.lines().count() };
                    service_to_file.insert(dm_path.clone(), rel_path.clone());
                    luau_scripts.insert(dm_path.clone(), src.clone());
                    scripts_map.insert(
                        rel_path,
                        json!({
                            "source": src,
                            "service_path": dm_path,
                            "class_name": f.class,
                            "sha256": sha256,
                            "lines": lines,
                        }),
                    );
                } else if !f.service {
                    let norm_props = crate::scene::normalize_instance_properties(&f.class, &f.props);
                    let props_val = Value::Object(norm_props);
                    luau_instances.insert(
                        dm_path.clone(),
                        json!({
                            "class_name": f.class,
                            "properties": props_val,
                        }),
                    );
                    instances_map.insert(
                        dm_path,
                        json!({
                            "class_name": f.class,
                            "properties": props_val,
                            "source_file": source_file,
                        }),
                    );
                }
            }
            let report = crate::luau_check::analyze_place(&luau_scripts, &luau_instances);
            let diagnostics: Vec<Value> = report
                .diagnostics
                .iter()
                .map(|d| {
                    let file = service_to_file.get(&d.path).cloned();
                    json!({
                        "severity": d.severity.as_str(),
                        "file": file,
                        "service_path": d.path,
                        "line": d.line,
                        "col": Value::Null,
                        "message": d.message,
                        "code": d.code,
                    })
                })
                .collect();

            Ok(json!({
                "project_id": project_id,
                "project_name": project_name,
                "mode": "rojo",
                "scene_revision": scene_revision,
                "can_undo": can_undo,
                "can_redo": can_redo,
                "scripts": scripts_map,
                "instances": instances_map,
                "meshes": meshes,
                "lighting": lighting,
                "lights": lights,
                "terrain": terrain,
                "diagnostics": diagnostics,
                "skipped": tree.skipped,
                "tree_error": Value::Null,
            }))
        }
        Err(err) => {
            let lighting = crate::scene::extract_lighting_from_tree(&[]);
            Ok(json!({
                "project_id": project_id,
                "project_name": project_name,
                "mode": "rojo",
                "scene_revision": scene_revision,
                "can_undo": can_undo,
                "can_redo": can_redo,
                "scripts": {},
                "instances": {},
                "meshes": meshes,
                "lighting": lighting,
                "lights": [],
                "terrain": terrain,
                "diagnostics": [{
                    "severity": "error",
                    "file": rojo_file,
                    "service_path": "DataModel",
                    "line": 1,
                    "col": Value::Null,
                    "message": err,
                    "code": "rojo_tree_error",
                }],
                "skipped": [],
                "tree_error": err,
            }))
        }
    }
}

/// Run `rojo build`, capped at two minutes.
fn rojo_build(rojo: &Path, dir: &Path, rojo_file: &str, out: &Path) -> Result<(), String> {
    let mut child = std::process::Command::new(rojo)
        .arg("build")
        .arg(rojo_file)
        .arg("-o")
        .arg(out)
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start rojo: {e}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    use std::io::Read;
                    let _ = e.read_to_string(&mut err);
                }
                let err: String = err.chars().take(600).collect();
                return Err(format!("rojo build failed: {}", err.trim()));
            }
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("rojo build took longer than two minutes and was stopped".into());
            }
            Err(e) => return Err(format!("rojo build: {e}")),
        }
    }
}

/// Build the project to `.rbxlx`; returns the build record (also saved under `build/builds/`).
pub fn build(dir: &Path, engine: &str, out_rel: Option<&str>) -> Result<Value, String> {
    let started = std::time::Instant::now();
    let m = manifest(dir)?;
    let rojo_file = m["rojoProject"].as_str().unwrap_or(DEFAULT_ROJO_FILE).to_string();
    let tree = load_tree(dir, &rojo_file)?;
    let (instances, scripts) = tree.counts();
    let default_out = format!("build/{}.rbxlx", slug(m["name"].as_str().unwrap_or("place")));
    let out_rel = out_rel.unwrap_or(&default_out);
    if !out_rel.ends_with(".rbxlx") {
        return Err("the build output must end in .rbxlx".into());
    }
    let out = safe_join(dir, out_rel)?;
    std::fs::create_dir_all(out.parent().unwrap_or(dir)).map_err(|e| format!("create build folder: {e}"))?;
    let mut warnings: Vec<String> = tree.skipped.iter().map(|s| format!("{s}: not read by the built-in loader")).collect();
    let rojo = find_rojo();
    let use_rojo = match engine {
        "rojo" => {
            if rojo.is_none() {
                return Err("rojo is not installed (put it on PATH or set OPENRO_ROJO)".into());
            }
            true
        }
        "builtin" => false,
        "auto" | "" => rojo.is_some(),
        other => return Err(format!("engine must be auto, rojo or builtin, not '{other}'")),
    };
    let engine_used;
    if use_rojo {
        engine_used = "rojo";
        warnings.clear();
        rojo_build(rojo.as_ref().unwrap(), dir, &rojo_file, &out)?;
    } else {
        engine_used = "builtin";
        let xml = builtin_rbxlx(&tree, &mut warnings);
        std::fs::write(&out, xml).map_err(|e| format!("write {out_rel}: {e}"))?;
    }
    let bytes = std::fs::read(&out).map_err(|e| format!("read build: {e}"))?;
    let at = now_ms();
    let build_id = unique_id("b");
    let record = json!({
        "build_id": build_id,
        "engine": engine_used,
        "path": rel_display(dir, &out),
        "bytes": bytes.len(),
        "sha256": sha256_hex(&bytes),
        "instances": instances,
        "scripts": scripts,
        "warnings": warnings,
        "duration_ms": started.elapsed().as_millis() as u64,
        "at": at,
    });
    let rec_dir = dir.join("build/builds");
    std::fs::create_dir_all(&rec_dir).map_err(|e| format!("create build records: {e}"))?;
    std::fs::write(rec_dir.join(format!("{build_id}.json")), record.to_string()).map_err(|e| format!("save build record: {e}"))?;
    Ok(record)
}

/// Build records, newest first.
pub fn builds(dir: &Path) -> Vec<Value> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("build/builds"))
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "json").unwrap_or(false)).collect())
        .unwrap_or_default();
    files.sort();
    files.reverse();
    files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).filter_map(|t| serde_json::from_str(&t).ok()).collect()
}

pub fn summary(r: &ProjectRef) -> Value {
    let m = manifest(&r.dir).unwrap_or_else(|_| json!({}));
    json!({
        "id": r.id,
        "name": m.get("name").cloned().unwrap_or(Value::Null),
        "path": r.dir.to_string_lossy(),
        "universeId": m.get("universeId").cloned().unwrap_or(Value::Null),
        "placeId": m.get("placeId").cloned().unwrap_or(Value::Null),
        "clean_room": m.get("clean_room").and_then(|v| v.as_bool()).unwrap_or(true),
        "has_manifest": r.dir.join(MANIFEST_FILE).is_file(),
        "has_rojo": r.dir.join(DEFAULT_ROJO_FILE).is_file(),
        "last_build": builds(&r.dir).into_iter().next(),
        "products": m.get("products").cloned().unwrap_or_else(|| json!([])),
    })
}

/// Parse an optional positive id from a request body.
pub fn body_id(body: &Value, key: &str) -> Result<Option<u64>, String> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().filter(|n| *n > 0).map(Some).ok_or(format!("{key} must be a positive whole number")),
    }
}

// ---------------------------------------------------------------------- git

const MAX_COMMIT_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Paths that must never be staged or committed by OpenRO's git hooks.
pub fn is_blocked_git_rel(rel: &str) -> bool {
    let clean = rel.trim_start_matches("./");
    clean.is_empty()
        || clean == ".git"
        || clean.starts_with(".git/")
        || clean.split('/').any(|s| s == ".git")
        || clean == ".openro"
        || clean.starts_with(".openro/")
        || clean == "build"
        || clean.starts_with("build/")
        || clean == "refs"
        || clean.starts_with("refs/")
        || clean == "art/references"
        || clean.starts_with("art/references/")
}

fn run_git(dir: &Path, args: &[&str]) -> Result<(bool, String, String), String> {
    let mut child = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run git: {e}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                use std::io::Read;
                let mut out = String::new();
                let mut err = String::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_string(&mut out);
                }
                if let Some(mut s) = child.stderr.take() {
                    let _ = s.read_to_string(&mut err);
                }
                return Ok((status.success(), out, err));
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("git command timed out after 30s".into());
            }
            Err(e) => return Err(format!("git failed: {e}")),
        }
    }
}

fn git_repo_root(dir: &Path) -> Option<PathBuf> {
    let (ok, out, _) = run_git(dir, &["rev-parse", "--show-toplevel"]).ok()?;
    if !ok {
        return None;
    }
    let root = out.trim();
    if root.is_empty() {
        return None;
    }
    PathBuf::from(root).canonicalize().ok()
}

fn git_identity_args(dir: &Path) -> Vec<&'static str> {
    let has_name = run_git(dir, &["config", "--get", "user.name"])
        .map(|(ok, out, _)| ok && !out.trim().is_empty())
        .unwrap_or(false);
    let has_email = run_git(dir, &["config", "--get", "user.email"])
        .map(|(ok, out, _)| ok && !out.trim().is_empty())
        .unwrap_or(false);
    let mut extra = Vec::new();
    if !has_name {
        extra.extend(["-c", "user.name=OpenRO"]);
    }
    if !has_email {
        extra.extend(["-c", "user.email=openro@local"]);
    }
    extra
}

pub fn git_status(dir: &Path) -> Value {
    let Some(root) = git_repo_root(dir) else {
        return json!({
            "is_git_repo": false,
            "branch": null,
            "head": null,
            "dirty": false,
            "changed": [],
        });
    };
    let branch = run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|(ok, _, _)| *ok)
        .map(|(_, o, _)| o.trim().to_string())
        .filter(|s| !s.is_empty());
    let head = run_git(dir, &["rev-parse", "--short", "HEAD"])
        .ok()
        .filter(|(ok, _, _)| *ok)
        .map(|(_, o, _)| o.trim().to_string())
        .filter(|s| !s.is_empty());
    let prefix = dir
        .canonicalize()
        .ok()
        .and_then(|c| c.strip_prefix(&root).ok().map(PathBuf::from))
        .unwrap_or_default();
    let prefix_str = prefix
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    let changed: Vec<Value> = run_git(dir, &["status", "--porcelain", "--", "."])
        .ok()
        .filter(|(ok, _, _)| *ok)
        .map(|(_, out, _)| {
            out.lines()
                .filter(|l| l.len() >= 4)
                .filter_map(|line| {
                    let st = line[..2].trim().to_string();
                    let raw = line[3..].trim().trim_matches('"');
                    let rel = if prefix_str.is_empty() {
                        raw.to_string()
                    } else {
                        raw.strip_prefix(&format!("{prefix_str}/")).unwrap_or(raw).to_string()
                    };
                    if is_blocked_git_rel(&rel) {
                        return None;
                    }
                    Some(json!({ "status": st, "path": rel }))
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "is_git_repo": true,
        "branch": branch,
        "head": head,
        "dirty": !changed.is_empty(),
        "changed": changed,
    })
}

/// Return the subset of project-relative paths that exist inside `dir` and are neither
/// OpenRO-blocked (`is_blocked_git_rel`, `art/previews/`, `*.rbxl*`) nor excluded by `.gitignore`.
pub fn filter_unignored_rel_paths(dir: &Path, rels: &[String]) -> Vec<String> {
    let mut candidates = Vec::new();
    for raw in rels {
        let Ok(full) = safe_join(dir, raw) else { continue };
        let rel = rel_display(dir, &full);
        if rel.is_empty()
            || rel.starts_with(':')
            || is_blocked_git_rel(&rel)
            || rel == "art/previews"
            || rel.starts_with("art/previews/")
            || rel.ends_with(".rbxl")
            || rel.ends_with(".rbxlx")
            || !full.is_file()
        {
            continue;
        }
        if !candidates.contains(&rel) {
            candidates.push(rel);
        }
    }
    if candidates.is_empty() || git_repo_root(dir).is_none() {
        return candidates;
    }
    let mut ls_args = vec!["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--"];
    ls_args.extend(candidates.iter().map(String::as_str));
    let Ok((true, out, _)) = run_git(dir, &ls_args) else {
        return candidates;
    };
    let unignored: std::collections::BTreeSet<String> = out
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_start_matches("./").replace('\\', "/"))
        .collect();
    candidates
        .into_iter()
        .filter(|c| unignored.contains(c))
        .collect()
}

/// Stage ONLY `files` (or matching pathspecs/globs) inside `dir` (via `safe_join`, skipping
/// `.openro/`, `.git/`, `build/`, `art/references/`, `refs/`, gitignored paths, non-matching
/// globs, and refusing files over 5 MB) and create one local commit with `message`. Returns
/// `Ok(Some(sha))` on commit, or `Ok(None)` when `dir` is not in a git repo or none of the
/// matched files have changes to commit.
pub fn git_commit(dir: &Path, files: &[String], message: &str) -> Result<Option<String>, String> {
    let msg = message.trim();
    if msg.is_empty() || msg.contains('\0') {
        return Err("commit message must be non-empty".into());
    }
    if git_repo_root(dir).is_none() {
        return Ok(None);
    }
    let mut candidate_rels: Vec<String> = Vec::new();
    for raw in files {
        let full = safe_join(dir, raw)?;
        let rel = rel_display(dir, &full);
        if rel.starts_with(':') {
            return Err(format!("pathspec '{raw}' is not allowed"));
        }
        if is_blocked_git_rel(&rel) {
            continue;
        }
        if let Ok(meta) = std::fs::metadata(&full) {
            if meta.is_file() && meta.len() > MAX_COMMIT_FILE_BYTES {
                return Err(format!("refusing to commit '{rel}': file is over 5 MB"));
            }
        }
        if !candidate_rels.contains(&rel) {
            candidate_rels.push(rel);
        }
    }
    if candidate_rels.is_empty() {
        return Ok(None);
    }

    // Resolve pathspecs/globs via git ls-files so non-matching globs or ignored files are skipped cleanly.
    let mut ls_args = vec!["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--"];
    ls_args.extend(candidate_rels.iter().map(String::as_str));
    let (ok, matched_raw, err) = run_git(dir, &ls_args)?;
    if !ok {
        return Err(format!("git ls-files failed: {}", err.trim()));
    }
    let mut clean_rels: Vec<String> = Vec::new();
    for raw_m in matched_raw.split('\0').filter(|s| !s.is_empty()) {
        let rel = raw_m.trim_start_matches("./").replace('\\', "/");
        if is_blocked_git_rel(&rel) {
            continue;
        }
        let full = safe_join(dir, &rel)?;
        if let Ok(meta) = std::fs::metadata(&full) {
            if meta.is_file() && meta.len() > MAX_COMMIT_FILE_BYTES {
                return Err(format!("refusing to commit '{rel}': file is over 5 MB"));
            }
        }
        if !clean_rels.contains(&rel) {
            clean_rels.push(rel);
        }
    }
    if clean_rels.is_empty() {
        return Ok(None);
    }
    let mut add_args: Vec<&str> = vec!["add", "--"];
    for rel in &clean_rels {
        add_args.push(rel.as_str());
    }
    let (ok, _, err) = run_git(dir, &add_args)?;
    if !ok {
        return Err(format!("git add failed: {}", err.trim()));
    }
    // Check if any of the requested paths actually have staged changes.
    let mut diff_args: Vec<&str> = vec!["diff", "--cached", "--name-only", "--"];
    for rel in &clean_rels {
        diff_args.push(rel.as_str());
    }
    let (ok, staged_out, err) = run_git(dir, &diff_args)?;
    if !ok {
        return Err(format!("git diff --cached failed: {}", err.trim()));
    }
    if staged_out.trim().is_empty() {
        return Ok(None);
    }
    let id_args = git_identity_args(dir);
    let mut commit_args: Vec<&str> = Vec::new();
    commit_args.extend(id_args);
    commit_args.extend(["commit", "-m", msg, "--"]);
    for rel in &clean_rels {
        commit_args.push(rel.as_str());
    }
    let (ok, _, err) = run_git(dir, &commit_args)?;
    if !ok {
        return Err(format!("git commit failed: {}", err.trim()));
    }
    let (ok, sha, err) = run_git(dir, &["rev-parse", "HEAD"])?;
    if !ok {
        return Err(format!("git rev-parse HEAD failed: {}", err.trim()));
    }
    Ok(Some(sha.trim().to_string()))
}

/// Commit every changed/untracked non-blocked file inside `dir`.
pub fn git_commit_all(dir: &Path, message: &str) -> Result<Option<String>, String> {
    let st = git_status(dir);
    if st["is_git_repo"] != json!(true) {
        return Ok(None);
    }
    let files: Vec<String> = st["changed"]
        .as_array()
        .map(|a| a.iter().filter_map(|e| e["path"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    git_commit(dir, &files, message)
}

/// Revert one project-scoped commit cleanly (`git revert --no-edit <sha>`).
/// Refuses commits that touch files outside the project folder; aborts cleanly on conflict.
pub fn git_revert(dir: &Path, commit: &str) -> Result<String, String> {
    let c = commit.trim();
    if c.len() < 4 || c.len() > 40 || !c.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("commit must be a 4-40 character hex SHA".into());
    }
    let root = git_repo_root(dir).ok_or("project is not inside a git repository")?;
    let verify_ref = format!("{c}^{{commit}}");
    let (ok, full_sha, err) = run_git(dir, &["rev-parse", "--verify", &verify_ref])?;
    if !ok {
        return Err(format!("unknown commit '{c}': {}", err.trim()));
    }
    let full_sha = full_sha.trim().to_string();

    // Ensure the commit only modified files inside this project directory.
    let canon_dir = dir.canonicalize().map_err(|e| format!("project folder: {e}"))?;
    let prefix = canon_dir
        .strip_prefix(&root)
        .map(|p| {
            p.components()
                .map(|seg| seg.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let (ok, touched_out, err) = run_git(dir, &["diff-tree", "--no-commit-id", "--name-only", "-r", "--root", &full_sha])?;
    if !ok {
        return Err(format!("could not inspect commit '{c}': {}", err.trim()));
    }
    let touched: Vec<&str> = touched_out.lines().filter(|l| !l.trim().is_empty()).collect();
    if touched.is_empty() {
        return Err(format!("commit '{c}' has no files to revert"));
    }
    if !prefix.is_empty() {
        let want_prefix = format!("{prefix}/");
        if let Some(outside) = touched.iter().find(|p| !p.starts_with(&want_prefix)) {
            return Err(format!("commit '{c}' touches '{outside}' outside this project; refusing to revert"));
        }
    }

    let id_args = git_identity_args(dir);
    let mut rev_args: Vec<&str> = Vec::new();
    rev_args.extend(id_args);
    rev_args.extend(["revert", "--no-edit", &full_sha]);
    let (ok, _, err) = run_git(dir, &rev_args)?;
    if !ok {
        let _ = run_git(dir, &["revert", "--abort"]);
        return Err(format!("could not revert '{c}' cleanly (aborted): {}", err.trim()));
    }
    let (ok, new_sha, err) = run_git(dir, &["rev-parse", "HEAD"])?;
    if !ok {
        return Err(format!("git rev-parse HEAD failed: {}", err.trim()));
    }
    Ok(new_sha.trim().to_string())
}

/// Recent commits touching `dir` (newest first).
pub fn git_log(dir: &Path, limit: usize) -> Result<Vec<Value>, String> {
    if git_repo_root(dir).is_none() {
        return Ok(Vec::new());
    }
    let cap = limit.clamp(1, 100).to_string();
    let (ok, out, err) = run_git(
        dir,
        &["log", "-n", &cap, "--format=%H%x1f%h%x1f%cI%x1f%s", "--", "."],
    )?;
    if !ok {
        // Fresh repo with no commits yet
        if err.to_ascii_lowercase().contains("does not have any commits") {
            return Ok(Vec::new());
        }
        return Err(format!("git log failed: {}", err.trim()));
    }
    let rows = out
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\x1f');
            let sha = parts.next()?.to_string();
            let short = parts.next()?.to_string();
            let at = parts.next()?.to_string();
            let subject = parts.next()?.to_string();
            Some(json!({ "commit": sha, "short": short, "at": at, "message": subject }))
        })
        .collect();
    Ok(rows)
}

// ------------------------------------------------------ project check & lune

const TOOL_OUT_CAP: usize = 65_536;

fn run_bounded(
    bin: &Path,
    dir: &Path,
    args: &[String],
    timeout_secs: u64,
) -> Result<(i32, bool, u64, String, String), String> {
    let started = std::time::Instant::now();
    let mut child = std::process::Command::new(bin)
        .args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", bin.display()))?;
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut r) = out_pipe {
            use std::io::Read;
            let _ = r.take(TOOL_OUT_CAP as u64).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut r) = err_pipe {
            use std::io::Read;
            let _ = r.take(TOOL_OUT_CAP as u64).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let (exit_code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code().unwrap_or(-1), false),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break (-1, true);
            }
            Err(e) => return Err(format!("wait {}: {e}", bin.display())),
        }
    };
    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    Ok((exit_code, timed_out, started.elapsed().as_millis() as u64, stdout, stderr))
}

fn luau_target_dirs(dir: &Path) -> Vec<String> {
    let mut targets: Vec<String> = ["src", "ui", "tools"]
        .iter()
        .filter(|d| dir.join(d).is_dir())
        .map(|d| (*d).to_string())
        .collect();
    if targets.is_empty() {
        targets.push(".".to_string());
    }
    targets
}

const MAX_FORMAT_PATHS: usize = 200;

fn is_blocked_format_rel(rel: &str) -> bool {
    let clean = rel.trim_start_matches("./");
    clean == ".git"
        || clean.starts_with(".git/")
        || clean.split('/').any(|s| s == ".git")
        || clean == ".openro"
        || clean.starts_with(".openro/")
        || clean == "build"
        || clean.starts_with("build/")
}

fn resolve_format_targets(dir: &Path, paths: Option<&[String]>) -> Result<Vec<String>, String> {
    let Some(list) = paths.filter(|p| !p.is_empty()) else {
        return Ok(luau_target_dirs(dir));
    };
    if list.len() > MAX_FORMAT_PATHS {
        return Err(format!("too many format paths (max {MAX_FORMAT_PATHS})"));
    }
    let root_canon = dir.canonicalize().map_err(|e| format!("project folder: {e}"))?;
    let mut targets: Vec<String> = Vec::with_capacity(list.len());
    for raw in list {
        if raw.chars().any(|c| c.is_control()) {
            return Err("format path cannot contain control characters".into());
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("format path is empty".into());
        }
        if trimmed.starts_with('-') {
            return Err(format!("invalid format path '{trimmed}'"));
        }
        let full = safe_join(dir, trimmed)?;
        if !full.exists() {
            let rel = rel_display(dir, &full);
            let label = if rel.is_empty() { trimmed } else { rel.as_str() };
            return Err(format!("format target '{label}' does not exist"));
        }
        let canon = full
            .canonicalize()
            .map_err(|e| format!("format target '{trimmed}': {e}"))?;
        if !canon.starts_with(&root_canon) {
            return Err(format!("path '{trimmed}' must stay inside the project folder"));
        }
        let rel = rel_display(dir, &full);
        let target = if rel.is_empty() {
            ".".to_string()
        } else {
            if is_blocked_format_rel(&rel) {
                return Err(format!("refusing to format blocked path '{rel}'"));
            }
            if rel.starts_with('-') {
                return Err(format!("invalid format path '{rel}'"));
            }
            if canon.is_file() {
                let is_luau = canon
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| {
                        e.eq_ignore_ascii_case("luau") || e.eq_ignore_ascii_case("lua")
                    });
                if !is_luau {
                    return Err(format!(
                        "format target '{rel}' must be a directory or a .luau/.lua file"
                    ));
                }
            }
            rel
        };
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    Ok(targets)
}

/// Format project Luau files in-place with `stylua`, jailed to `dir` (#56).
/// When `paths` is `None` or empty, formats `luau_target_dirs(dir)` (`src/`, `ui/`, `tools/` or `.`).
pub fn format_project(dir: &Path, paths: Option<&[String]>) -> Result<Value, String> {
    let targets = resolve_format_targets(dir, paths)?;
    match find_tool("stylua", "OPENRO_STYLUA") {
        Some(bin) => {
            let mut args = vec!["--".to_string()];
            args.extend(targets.iter().cloned());
            match run_bounded(&bin, dir, &args, 60) {
                Ok((code, timed_out, ms, out, err)) => Ok(json!({
                    "available": true,
                    "formatted": code == 0 && !timed_out,
                    "exit_code": code,
                    "timed_out": timed_out,
                    "duration_ms": ms,
                    "stdout": out,
                    "stderr": err,
                    "targets": targets,
                })),
                Err(e) => Ok(json!({
                    "available": true,
                    "formatted": false,
                    "exit_code": -1,
                    "timed_out": false,
                    "duration_ms": 0,
                    "stdout": "",
                    "stderr": "",
                    "targets": targets,
                    "error": e,
                })),
            }
        }
        None => Ok(json!({
            "available": false,
            "formatted": false,
            "exit_code": Value::Null,
            "timed_out": false,
            "duration_ms": 0,
            "stdout": "",
            "stderr": "",
            "targets": targets,
            "note": "stylua is not installed",
        })),
    }
}

/// Static project verification: Luau static analyzer + `selene` + `stylua --check` (#18).
pub fn check_project(dir: &Path) -> Result<Value, String> {
    check_project_with_full_options(dir, false, None, false)
}

/// Static project verification with optional pre-check `stylua` auto-formatting (#18, #56).
/// When `format` is `true`, runs `format_project(dir, format_paths)` before static analysis,
/// `selene`, and `stylua --check`, and includes `"formatted"` and `"format"` in the reply.
pub fn check_project_with_options(
    dir: &Path,
    format: bool,
    format_paths: Option<&[String]>,
) -> Result<Value, String> {
    check_project_with_full_options(dir, format, format_paths, false)
}

/// Static project verification with optional `stylua` auto-formatting and `KeptChecks`
/// SHA-256 project fingerprinting (#18, #56, #62). Reuses a previously passing result
/// when inputs & options are unchanged; NEVER caches failures; `force: true` bypasses cache.
pub fn check_project_with_full_options(
    dir: &Path,
    format: bool,
    format_paths: Option<&[String]>,
    force: bool,
) -> Result<Value, String> {
    let m = manifest(dir)?;
    let resolved_paths = if format || format_paths.is_some_and(|p| !p.is_empty()) {
        Some(resolve_format_targets(dir, format_paths)?)
    } else {
        None
    };
    let mode_spec = json!({
        "format": format,
        "targets": resolved_paths,
    });
    let fp_before = crate::study_check::project_fingerprint(dir)?;
    if let Some(cached) =
        crate::study_check::lookup_kept_check(dir, "check", &fp_before, &mode_spec, force)
    {
        return Ok(cached);
    }

    let (formatted_flag, format_json) = if format {
        let f = format_project(dir, format_paths)?;
        let did_format = f["formatted"].as_bool().unwrap_or(false);
        (did_format, Some(f))
    } else {
        (false, None)
    };
    let rojo_file = m["rojoProject"].as_str().unwrap_or(DEFAULT_ROJO_FILE).to_string();
    let mut scripts: BTreeMap<String, String> = BTreeMap::new();
    let mut instances: BTreeMap<String, Value> = BTreeMap::new();
    let mut tree_err: Option<String> = None;
    match load_tree(dir, &rojo_file) {
        Ok(tree) => {
            for f in tree.flatten() {
                let dot = f.path.replace('/', ".");
                if let Some(src) = f.source {
                    scripts.insert(dot, src);
                } else if !f.service {
                    instances.insert(dot, json!({ "class_name": f.class, "properties": f.props }));
                }
            }
        }
        Err(e) => tree_err = Some(e),
    }
    let report = crate::luau_check::analyze_place(&scripts, &instances);
    let luau_passed = tree_err.is_none() && report.is_ok();
    let luau_json = json!({
        "passed": luau_passed,
        "scripts_checked": scripts.len(),
        "errors": report.error_count(),
        "warnings": report.warning_count(),
        "tree_error": tree_err,
        "diagnostics": report.diagnostics.iter().map(|d| d.to_json()).collect::<Vec<_>>(),
    });

    let targets = luau_target_dirs(dir);
    // Scope selene to Roblox game source (`src/`, plus `ui/` if present) so Lune tooling
    // under `tools/` is not rejected against the `roblox` standard library (#33).
    let selene_targets: Vec<String> = if dir.join("src").is_dir() {
        let mut st = vec!["src".to_string()];
        if dir.join("ui").is_dir() {
            st.push("ui".to_string());
        }
        st
    } else {
        targets.clone()
    };
    let selene_json = match find_tool("selene", "OPENRO_SELENE") {
        Some(bin) => {
            let mut args = vec!["--no-summary".to_string()];
            args.extend(selene_targets.iter().cloned());
            match run_bounded(&bin, dir, &args, 60) {
                Ok((code, timed_out, ms, out, err)) => json!({
                    "available": true,
                    "passed": code == 0 && !timed_out,
                    "exit_code": code,
                    "timed_out": timed_out,
                    "duration_ms": ms,
                    "stdout": out,
                    "stderr": err,
                }),
                Err(e) => json!({ "available": true, "passed": false, "error": e }),
            }
        }
        None => json!({ "available": false, "passed": true, "note": "selene is not installed" }),
    };

    let stylua_json = match find_tool("stylua", "OPENRO_STYLUA") {
        Some(bin) => {
            let mut args = vec!["--check".to_string(), "--".to_string()];
            args.extend(targets.iter().cloned());
            match run_bounded(&bin, dir, &args, 60) {
                Ok((code, timed_out, ms, out, err)) => json!({
                    "available": true,
                    "passed": code == 0 && !timed_out,
                    "exit_code": code,
                    "timed_out": timed_out,
                    "duration_ms": ms,
                    "stdout": out,
                    "stderr": err,
                }),
                Err(e) => json!({ "available": true, "passed": false, "error": e }),
            }
        }
        None => json!({ "available": false, "passed": true, "note": "stylua is not installed" }),
    };

    let passed = luau_passed
        && selene_json["passed"].as_bool().unwrap_or(false)
        && stylua_json["passed"].as_bool().unwrap_or(false);
    let fp_after = if format {
        crate::study_check::project_fingerprint(dir).unwrap_or(fp_before)
    } else {
        fp_before
    };
    let mut out = json!({
        "passed": passed,
        "formatted": formatted_flag,
        "kept": false,
        "cached": false,
        "fingerprint": fp_after,
        "luau": luau_json,
        "selene": selene_json,
        "stylua": stylua_json,
    });
    if let Some(f) = format_json {
        out.as_object_mut().unwrap().insert("format".into(), f);
    }
    crate::study_check::store_kept_check(dir, "check", &fp_after, &mode_spec, &out);
    Ok(out)
}

/// Run a project-relative `.luau`/`.lua` script with `lune run`, timeout-bounded (#18).
pub fn run_lune(
    dir: &Path,
    script_rel: &str,
    extra_args: &[String],
    timeout_secs: u64,
) -> Result<Value, String> {
    let bin = find_tool("lune", "OPENRO_LUNE")
        .ok_or("lune is not installed (put it on PATH, in ~/.local/bin, or set OPENRO_LUNE)")?;
    let full = safe_join(dir, script_rel)?;
    let rel = rel_display(dir, &full);
    if is_blocked_git_rel(&rel) {
        return Err(format!("refusing to run script in blocked folder '{rel}'"));
    }
    if !(rel.ends_with(".luau") || rel.ends_with(".lua")) {
        return Err("lune script path must end in .luau or .lua".into());
    }
    if !full.is_file() {
        return Err(format!("script '{rel}' does not exist"));
    }
    if extra_args.len() > 32 {
        return Err("too many script arguments (max 32)".into());
    }
    if extra_args.iter().any(|a| a.contains('\0')) {
        return Err("script arguments cannot contain NUL bytes".into());
    }
    let cap_secs = timeout_secs.clamp(1, 120);
    let mut args = vec!["run".to_string(), rel.clone()];
    args.extend(extra_args.iter().cloned());
    let (exit_code, timed_out, duration_ms, stdout, stderr) =
        run_bounded(&bin, dir, &args, cap_secs)?;
    Ok(json!({
        "script": rel,
        "passed": exit_code == 0 && !timed_out,
        "exit_code": exit_code,
        "timed_out": timed_out,
        "duration_ms": duration_ms,
        "stdout": stdout,
        "stderr": stderr,
    }))
}

// ---------------------------------------------------------------------------
// Prefab Model Import (.rbxmx / .rbxm / .rbxlx) -> scene/models/<slug>.model.json
// ---------------------------------------------------------------------------

const MAX_PREFAB_BYTES: usize = 16 * 1024 * 1024;
const MAX_PREFAB_XML_DEPTH: usize = 64;
const MAX_PREFAB_XML_ITEMS: usize = 50_000;
const ALLOWED_PREFAB_SERVICES: [&str; 9] = [
    "Workspace",
    "ReplicatedStorage",
    "ServerStorage",
    "ServerScriptService",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "Lighting",
    "SoundService",
];

#[derive(Clone, Debug)]
struct ImportedNode {
    class_name: String,
    name: String,
    properties: BTreeMap<String, Value>,
    source: Option<String>,
    children: Vec<ImportedNode>,
}

fn prefab_write_atomic(dir: &Path, rel: &str, content: &str) -> Result<(), String> {
    if rel.contains('\0') || rel.contains('\\') {
        return Err(format!("invalid path '{rel}'"));
    }
    let full = safe_join(dir, rel)?;
    if let Ok(meta) = std::fs::symlink_metadata(&full) {
        if meta.file_type().is_symlink() {
            return Err(format!("refusing to write through symlink '{rel}'"));
        }
    }
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir for {rel}: {e}"))?;
    }
    static PREFAB_TMP_SEQ: AtomicU64 = AtomicU64::new(1);
    let seq = PREFAB_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let fname = full.file_name().and_then(|s| s.to_str()).unwrap_or("prefab");
    let tmp = full.with_file_name(format!(".{fname}.openro-tmp.{}.{seq}", std::process::id()));
    std::fs::write(&tmp, content.as_bytes()).map_err(|e| format!("write {rel}: {e}"))?;
    if let Err(e) = std::fs::rename(&tmp, &full) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("rename {rel}: {e}"));
    }
    Ok(())
}

fn sanitize_prefab_node_name(raw: &str, fallback: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        fallback.to_string()
    } else {
        trimmed.chars().take(128).collect()
    }
}

fn prefab_xml_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(semi) = s[i..].find(';') {
                let ent = &s[i + 1..i + semi];
                let ch = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                        u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
                    }
                    _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
                    _ => None,
                };
                if let Some(c) = ch {
                    out.push(c);
                    i += semi + 1;
                    continue;
                }
            }
        }
        let c = s[i..].chars().next().unwrap_or('\0');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

type XmlAttrs = Vec<(String, String)>;
type XmlOpenTag = (String, XmlAttrs, bool);
type ParsedPrefabProps = (Option<String>, Option<String>, BTreeMap<String, Value>);

struct DecomposeCtx<'a> {
    dir: &'a Path,
    slug_name: &'a str,
    extracted_scripts: Vec<String>,
    direct_root_scripts: Vec<(String, String)>,
    deep_scripts: Vec<(String, String, String)>,
    used_script_paths: std::collections::HashSet<String>,
}

struct PrefabXmlParser<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    items_seen: usize,
}

impl<'a> PrefabXmlParser<'a> {
    fn new(src: &'a str) -> Self {
        let trimmed = src.strip_prefix('\u{FEFF}').unwrap_or(src);
        Self {
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
                let end = self
                    .remaining()
                    .find("?>")
                    .ok_or_else(|| "unclosed XML processing instruction <?".to_string())?;
                self.pos += end + 2;
            } else if self.starts_with("<!--") {
                let end = self
                    .remaining()
                    .find("-->")
                    .ok_or_else(|| "unclosed XML comment <!--".to_string())?;
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
        let q = self.bytes[self.pos];
        if q != b'"' && q != b'\'' {
            return Err(format!("expected quote for XML attribute at byte {}", self.pos));
        }
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != q {
            self.pos += 1;
        }
        if self.pos >= self.bytes.len() {
            return Err("unclosed XML attribute value".to_string());
        }
        let raw = &self.src[start..self.pos];
        self.pos += 1;
        Ok(prefab_xml_unescape(raw))
    }

    fn read_open_tag(&mut self) -> Result<XmlOpenTag, String> {
        if self.starts_with("<!") && !self.starts_with("<![CDATA[") {
            return Err("XML DOCTYPE/ENTITY declarations are forbidden (XXE protection)".to_string());
        }
        if !self.starts_with("<") || self.starts_with("</") || self.starts_with("<?") {
            return Err(format!("expected XML start tag at byte {}", self.pos));
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
                return Err(format!("expected '=' after '{attr_name}' in <{name}>"));
            }
            self.pos += 1;
            self.skip_ws();
            let val = self.read_attr_value()?;
            attrs.push((attr_name, val));
        }
    }

    fn read_close_tag(&mut self, expected: &str) -> Result<(), String> {
        if !self.starts_with("</") {
            return Err(format!("expected </{expected}> at byte {}", self.pos));
        }
        self.pos += 2;
        let actual = self.read_name()?;
        self.skip_ws();
        if !self.starts_with(">") {
            return Err(format!("unclosed </{actual}>"));
        }
        self.pos += 1;
        if actual != expected {
            return Err(format!("mismatched closing tag </{actual}> (expected </{expected}>)"));
        }
        Ok(())
    }

    fn read_text_body(&mut self, expected: &str) -> Result<String, String> {
        let mut out = String::new();
        loop {
            if self.is_eof() {
                return Err(format!("unclosed <{expected}>"));
            }
            if self.starts_with("</") {
                self.read_close_tag(expected)?;
                return Ok(out);
            }
            if self.starts_with("<![CDATA[") {
                self.pos += 9;
                let end = self
                    .remaining()
                    .find("]]>")
                    .ok_or_else(|| format!("unclosed CDATA in <{expected}>"))?;
                out.push_str(&self.remaining()[..end]);
                self.pos += end + 3;
                continue;
            }
            if self.starts_with("<!--") {
                let end = self
                    .remaining()
                    .find("-->")
                    .ok_or_else(|| "unclosed XML comment".to_string())?;
                self.pos += end + 3;
                continue;
            }
            if self.starts_with("<!") {
                return Err("XML DOCTYPE/ENTITY declarations are forbidden (XXE protection)".to_string());
            }
            if self.starts_with("<") {
                return Err(format!("unexpected element inside <{expected}>"));
            }
            let next_lt = self.remaining().find('<').unwrap_or(self.remaining().len());
            out.push_str(&prefab_xml_unescape(&self.remaining()[..next_lt]));
            self.pos += next_lt;
        }
    }

    fn skip_element_body(&mut self, expected: &str, depth: usize) -> Result<(), String> {
        if depth > MAX_PREFAB_XML_DEPTH {
            return Err(format!("XML nesting exceeds maximum depth {MAX_PREFAB_XML_DEPTH}"));
        }
        loop {
            self.skip_misc()?;
            if self.is_eof() {
                return Err(format!("unclosed <{expected}>"));
            }
            if self.starts_with("</") {
                self.read_close_tag(expected)?;
                return Ok(());
            }
            if self.starts_with("<![CDATA[") {
                self.pos += 9;
                let end = self
                    .remaining()
                    .find("]]>")
                    .ok_or_else(|| "unclosed CDATA".to_string())?;
                self.pos += end + 3;
                continue;
            }
            if self.starts_with("<") {
                let (tag, _, sc) = self.read_open_tag()?;
                if !sc {
                    self.skip_element_body(&tag, depth + 1)?;
                }
                continue;
            }
            let next_lt = self.remaining().find('<').unwrap_or(self.remaining().len());
            self.pos += next_lt;
        }
    }

    fn read_numeric_fields(&mut self, parent: &str, depth: usize) -> Result<BTreeMap<String, f64>, String> {
        if depth > MAX_PREFAB_XML_DEPTH {
            return Err(format!("XML nesting exceeds maximum depth {MAX_PREFAB_XML_DEPTH}"));
        }
        let mut fields = BTreeMap::new();
        loop {
            self.skip_misc()?;
            if self.is_eof() {
                return Err(format!("unclosed <{parent}>"));
            }
            if self.starts_with("</") {
                self.read_close_tag(parent)?;
                return Ok(fields);
            }
            let (tag, _, sc) = self.read_open_tag()?;
            let text = if sc { String::new() } else { self.read_text_body(&tag)? };
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let v = trimmed
                    .parse::<f64>()
                    .map_err(|_| format!("invalid number '{trimmed}' in <{parent}><{tag}>"))?;
                if !v.is_finite() {
                    return Err(format!("non-finite number in <{parent}><{tag}>"));
                }
                fields.insert(tag, v);
            }
        }
    }

    fn read_content_url(&mut self, depth: usize) -> Result<String, String> {
        if depth > MAX_PREFAB_XML_DEPTH {
            return Err(format!("XML nesting exceeds maximum depth {MAX_PREFAB_XML_DEPTH}"));
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
                let (tag, _, sc) = self.read_open_tag()?;
                if !sc {
                    if tag == "url" || tag == "uri" {
                        url = self.read_text_body(&tag)?;
                    } else {
                        self.skip_element_body(&tag, depth + 1)?;
                    }
                }
                continue;
            }
            let next_lt = self.remaining().find('<').unwrap_or(self.remaining().len());
            let t = prefab_xml_unescape(self.remaining()[..next_lt].trim());
            if !t.is_empty() && url.is_empty() {
                url = t;
            }
            self.pos += next_lt;
        }
    }

    fn parse_properties(&mut self, depth: usize) -> Result<ParsedPrefabProps, String> {
        if depth > MAX_PREFAB_XML_DEPTH {
            return Err(format!("XML nesting exceeds maximum depth {MAX_PREFAB_XML_DEPTH}"));
        }
        let mut item_name: Option<String> = None;
        let mut source: Option<String> = None;
        let mut props: BTreeMap<String, Value> = BTreeMap::new();
        loop {
            self.skip_misc()?;
            if self.is_eof() {
                return Err("unclosed <Properties>".to_string());
            }
            if self.starts_with("</") {
                self.read_close_tag("Properties")?;
                return Ok((item_name, source, props));
            }
            let (tag, attrs, sc) = self.read_open_tag()?;
            let prop_name = attrs
                .iter()
                .find(|(k, _)| k == "name")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            if sc {
                if tag == "ProtectedString" && prop_name == "Source" {
                    source = Some(String::new());
                } else if tag == "string" && prop_name == "Name" {
                    item_name = Some(String::new());
                }
                continue;
            }
            match tag.as_str() {
                "string" => {
                    let val = self.read_text_body(&tag)?;
                    if prop_name == "Name" {
                        item_name = Some(val);
                    } else if !prop_name.is_empty() {
                        props.insert(prop_name, Value::String(val));
                    }
                }
                "ProtectedString" => {
                    let val = self.read_text_body(&tag)?;
                    if prop_name == "Source" {
                        source = Some(val);
                    } else if !prop_name.is_empty() {
                        props.insert(prop_name, Value::String(val));
                    }
                }
                "bool" => {
                    let raw = self.read_text_body(&tag)?;
                    let b = match raw.trim() {
                        "true" => true,
                        "false" => false,
                        other => return Err(format!("invalid bool '{other}' for '{prop_name}'")),
                    };
                    if !prop_name.is_empty() {
                        props.insert(prop_name, Value::Bool(b));
                    }
                }
                "int" | "int64" => {
                    let raw = self.read_text_body(&tag)?;
                    let n = raw
                        .trim()
                        .parse::<i64>()
                        .map_err(|_| format!("invalid integer for '{prop_name}'"))?;
                    if !prop_name.is_empty() {
                        props.insert(prop_name, json!(n));
                    }
                }
                "float" | "double" => {
                    let raw = self.read_text_body(&tag)?;
                    let f = raw
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| format!("invalid float for '{prop_name}'"))?;
                    if !f.is_finite() {
                        return Err(format!("non-finite float for '{prop_name}'"));
                    }
                    if !prop_name.is_empty() {
                        props.insert(prop_name, json!(f));
                    }
                }
                "token" => {
                    let raw = self.read_text_body(&tag)?;
                    let tok = raw
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| format!("invalid token for '{prop_name}'"))?;
                    if prop_name == "Material" {
                        if let Some(m) = token_to_material_name(tok) {
                            props.insert("Material".into(), json!(m));
                        } else {
                            props.insert("Material".into(), json!(tok));
                        }
                    } else if prop_name == "shape" || prop_name == "Shape" {
                        let s = match tok {
                            0 => Some("Ball"),
                            1 => Some("Block"),
                            2 => Some("Cylinder"),
                            3 => Some("Wedge"),
                            4 => Some("CornerWedge"),
                            _ => None,
                        };
                        if let Some(sh) = s {
                            props.insert("Shape".into(), json!(sh));
                        } else {
                            props.insert("Shape".into(), json!(tok));
                        }
                    } else if !prop_name.is_empty() {
                        props.insert(prop_name, json!(tok));
                    }
                }
                "Content" => {
                    let url = self.read_content_url(depth + 1)?;
                    if !prop_name.is_empty() {
                        props.insert(prop_name, Value::String(url));
                    }
                }
                "Color3uint8" => {
                    let raw = self.read_text_body(&tag)?;
                    let c = raw
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| format!("invalid Color3uint8 for '{prop_name}'"))?;
                    let r = ((c >> 16) & 0xFF) as f64 / 255.0;
                    let g = ((c >> 8) & 0xFF) as f64 / 255.0;
                    let b = (c & 0xFF) as f64 / 255.0;
                    let round4 = |x: f64| (x * 10_000.0).round() / 10_000.0;
                    props.insert("Color".into(), json!([round4(r), round4(g), round4(b)]));
                }
                "Color3" => {
                    let fields = self.read_numeric_fields(&tag, depth + 1)?;
                    let round4 = |x: f64| (x.clamp(0.0, 1.0) * 10_000.0).round() / 10_000.0;
                    let r = round4(fields.get("R").copied().unwrap_or(0.0));
                    let g = round4(fields.get("G").copied().unwrap_or(0.0));
                    let b = round4(fields.get("B").copied().unwrap_or(0.0));
                    let key = if prop_name == "Color3" || prop_name.is_empty() {
                        "Color".to_string()
                    } else {
                        prop_name
                    };
                    props.insert(key, json!([r, g, b]));
                }
                "Vector3" => {
                    let fields = self.read_numeric_fields(&tag, depth + 1)?;
                    let key = if prop_name == "size" { "Size".to_string() } else { prop_name };
                    if !key.is_empty() {
                        props.insert(
                            key,
                            json!([
                                fields.get("X").copied().unwrap_or(0.0),
                                fields.get("Y").copied().unwrap_or(0.0),
                                fields.get("Z").copied().unwrap_or(0.0),
                            ]),
                        );
                    }
                }
                "Vector2" => {
                    let fields = self.read_numeric_fields(&tag, depth + 1)?;
                    if !prop_name.is_empty() {
                        props.insert(
                            prop_name,
                            json!([
                                fields.get("X").copied().unwrap_or(0.0),
                                fields.get("Y").copied().unwrap_or(0.0),
                            ]),
                        );
                    }
                }
                "UDim2" => {
                    let fields = self.read_numeric_fields(&tag, depth + 1)?;
                    if !prop_name.is_empty() {
                        props.insert(
                            prop_name,
                            json!({
                                "UDim2": [
                                    [fields.get("XS").copied().unwrap_or(0.0), fields.get("XO").copied().unwrap_or(0.0)],
                                    [fields.get("YS").copied().unwrap_or(0.0), fields.get("YO").copied().unwrap_or(0.0)],
                                ]
                            }),
                        );
                    }
                }
                "UDim" => {
                    let fields = self.read_numeric_fields(&tag, depth + 1)?;
                    if !prop_name.is_empty() {
                        props.insert(
                            prop_name,
                            json!({
                                "UDim": [
                                    fields.get("S").copied().unwrap_or(0.0),
                                    fields.get("O").copied().unwrap_or(0.0),
                                ]
                            }),
                        );
                    }
                }
                "CoordinateFrame" => {
                    let fields = self.read_numeric_fields(&tag, depth + 1)?;
                    let cf = [
                        fields.get("X").copied().unwrap_or(0.0),
                        fields.get("Y").copied().unwrap_or(0.0),
                        fields.get("Z").copied().unwrap_or(0.0),
                        fields.get("R00").copied().unwrap_or(1.0),
                        fields.get("R01").copied().unwrap_or(0.0),
                        fields.get("R02").copied().unwrap_or(0.0),
                        fields.get("R10").copied().unwrap_or(0.0),
                        fields.get("R11").copied().unwrap_or(1.0),
                        fields.get("R12").copied().unwrap_or(0.0),
                        fields.get("R20").copied().unwrap_or(0.0),
                        fields.get("R21").copied().unwrap_or(0.0),
                        fields.get("R22").copied().unwrap_or(1.0),
                    ];
                    if !prop_name.is_empty() {
                        props.insert(prop_name, crate::scene::cframe_to_rojo_json(cf));
                    }
                }
                _ => {
                    self.skip_element_body(&tag, depth + 1)?;
                }
            }
        }
    }

    fn parse_item(
        &mut self,
        attrs: &[(String, String)],
        self_closing: bool,
        depth: usize,
    ) -> Result<ImportedNode, String> {
        if depth > MAX_PREFAB_XML_DEPTH {
            return Err(format!("XML nesting exceeds maximum depth {MAX_PREFAB_XML_DEPTH}"));
        }
        self.items_seen += 1;
        if self.items_seen > MAX_PREFAB_XML_ITEMS {
            return Err(format!("XML exceeds maximum item count ({MAX_PREFAB_XML_ITEMS})"));
        }
        let class_name = attrs
            .iter()
            .find(|(k, _)| k == "class")
            .map(|(_, v)| v.clone())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "Item element missing 'class' attribute".to_string())?;
        let mut node = ImportedNode {
            class_name: class_name.clone(),
            name: class_name,
            properties: BTreeMap::new(),
            source: None,
            children: Vec::new(),
        };
        if self_closing {
            return Ok(node);
        }
        loop {
            self.skip_misc()?;
            if self.is_eof() {
                return Err(format!("unclosed <Item class=\"{}\">", node.class_name));
            }
            if self.starts_with("</") {
                self.read_close_tag("Item")?;
                return Ok(node);
            }
            let (tag, child_attrs, sc) = self.read_open_tag()?;
            if tag == "Properties" {
                if !sc {
                    let (n, src, props) = self.parse_properties(depth + 1)?;
                    if let Some(name) = n.filter(|s| !s.trim().is_empty()) {
                        node.name = name;
                    }
                    if src.is_some() {
                        node.source = src;
                    }
                    node.properties.extend(props);
                }
            } else if tag == "Item" {
                node.children.push(self.parse_item(&child_attrs, sc, depth + 1)?);
            } else if !sc {
                self.skip_element_body(&tag, depth + 1)?;
            }
        }
    }
}

fn token_to_material_name(tok: u32) -> Option<&'static str> {
    Some(match tok {
        256 => "Plastic",
        272 => "SmoothPlastic",
        288 => "Neon",
        512 => "Wood",
        528 => "WoodPlanks",
        784 => "Marble",
        800 => "Slate",
        816 => "Concrete",
        832 => "Granite",
        848 => "Brick",
        864 => "Pebble",
        880 => "Cobblestone",
        896 => "Rock",
        912 => "Sandstone",
        1040 => "Metal",
        1056 => "CorrodedMetal",
        1072 => "DiamondPlate",
        1088 => "Foil",
        1280 => "Grass",
        1296 => "Sand",
        1312 => "Fabric",
        1328 => "Snow",
        1344 => "Mud",
        1360 => "Ground",
        1376 => "Asphalt",
        1536 => "Ice",
        1568 => "Glass",
        1584 => "ForceField",
        _ => return None,
    })
}

fn parse_prefab_xml_items(bytes: &[u8]) -> Result<Vec<ImportedNode>, String> {
    if bytes.len() > MAX_PREFAB_BYTES {
        return Err(format!("XML exceeds {MAX_PREFAB_BYTES} byte limit"));
    }
    if bytes.contains(&0) {
        return Err("XML payload must not contain NUL bytes".to_string());
    }
    let xml = std::str::from_utf8(bytes).map_err(|_| "XML payload is not valid UTF-8".to_string())?;
    let upper = xml.to_ascii_uppercase();
    if upper.contains("<!DOCTYPE") || upper.contains("<!ENTITY") {
        return Err("XML DOCTYPE/ENTITY declarations are forbidden (XXE protection)".to_string());
    }
    if xml.trim().is_empty() {
        return Err("empty XML document".to_string());
    }
    let mut parser = PrefabXmlParser::new(xml);
    parser.skip_misc()?;
    if parser.is_eof() {
        return Err("empty XML document".to_string());
    }
    let (root_tag, _, root_sc) = parser.read_open_tag()?;
    if root_tag != "roblox" {
        return Err(format!("expected <roblox> root element, found <{root_tag}>"));
    }
    let mut roots = Vec::new();
    if !root_sc {
        loop {
            parser.skip_misc()?;
            if parser.is_eof() {
                return Err("unclosed <roblox> root element".to_string());
            }
            if parser.starts_with("</") {
                parser.read_close_tag("roblox")?;
                break;
            }
            let (tag, attrs, sc) = parser.read_open_tag()?;
            if tag == "Item" {
                roots.push(parser.parse_item(&attrs, sc, 1)?);
            } else if !sc {
                parser.skip_element_body(&tag, 1)?;
            }
        }
    }
    parser.skip_misc()?;
    if !parser.is_eof() {
        return Err("unexpected trailing content after </roblox>".to_string());
    }
    Ok(roots)
}

fn decompress_lz4_block(src: &[u8], expected_len: usize) -> Result<Vec<u8>, String> {
    if expected_len > MAX_PREFAB_BYTES {
        return Err("rbxm chunk exceeds 16 MiB limit".to_string());
    }
    let mut out = Vec::with_capacity(expected_len);
    let mut i = 0usize;
    while i < src.len() {
        let token = src[i];
        i += 1;
        let mut lit_len = (token >> 4) as usize;
        if lit_len == 15 {
            loop {
                let b = *src.get(i).ok_or("corrupt rbxm LZ4 literal length")?;
                i += 1;
                lit_len = lit_len.checked_add(b as usize).ok_or("rbxm LZ4 overflow")?;
                if b != 255 {
                    break;
                }
            }
        }
        let end_lit = i.checked_add(lit_len).ok_or("rbxm LZ4 overflow")?;
        let slice = src.get(i..end_lit).ok_or("corrupt rbxm LZ4 literal slice")?;
        if out.len().saturating_add(slice.len()) > expected_len {
            return Err("corrupt rbxm LZ4 output overflow".to_string());
        }
        out.extend_from_slice(slice);
        i = end_lit;
        if i >= src.len() {
            break;
        }
        let off_lo = *src.get(i).ok_or("corrupt rbxm LZ4 offset")? as usize;
        let off_hi = *src.get(i + 1).ok_or("corrupt rbxm LZ4 offset")? as usize;
        i += 2;
        let offset = off_lo | (off_hi << 8);
        if offset == 0 || offset > out.len() {
            return Err("corrupt rbxm LZ4 match offset".to_string());
        }
        let mut match_len = ((token & 0x0F) as usize) + 4;
        if (token & 0x0F) == 15 {
            loop {
                let b = *src.get(i).ok_or("corrupt rbxm LZ4 match length")?;
                i += 1;
                match_len = match_len.checked_add(b as usize).ok_or("rbxm LZ4 overflow")?;
                if b != 255 {
                    break;
                }
            }
        }
        if out.len().saturating_add(match_len) > expected_len {
            return Err("corrupt rbxm LZ4 match overflow".to_string());
        }
        for _ in 0..match_len {
            let idx = out.len() - offset;
            let byte = out[idx];
            out.push(byte);
        }
    }
    if out.len() != expected_len {
        return Err("corrupt rbxm LZ4 length mismatch".to_string());
    }
    Ok(out)
}

fn read_u32_le(bytes: &[u8], off: &mut usize) -> Result<u32, String> {
    let slice = bytes
        .get(*off..*off + 4)
        .ok_or_else(|| "unexpected EOF in binary rbxm".to_string())?;
    *off += 4;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_rbxm_referents(bytes: &[u8], off: &mut usize, count: usize) -> Result<Vec<i32>, String> {
    let need = count.checked_mul(4).ok_or("rbxm referent count overflow")?;
    let slice = bytes
        .get(*off..*off + need)
        .ok_or_else(|| "truncated rbxm referent array".to_string())?;
    *off += need;
    let mut out = Vec::with_capacity(count);
    let mut acc: i32 = 0;
    for i in 0..count {
        let u = u32::from_be_bytes([
            slice[i],
            slice[count + i],
            slice[2 * count + i],
            slice[3 * count + i],
        ]);
        let signed = ((u >> 1) as i32) ^ (-((u & 1) as i32));
        acc = acc.wrapping_add(signed);
        out.push(acc);
    }
    Ok(out)
}

fn parse_rbxm_binary(bytes: &[u8], fallback_name: &str) -> Result<(Vec<ImportedNode>, Vec<String>), String> {
    const ROBLOX_BIN_MAGIC: &[u8; 14] = b"<roblox!\x89\xff\x0d\x0a\x1a\x0a";
    if bytes.len() < 32 {
        return Err("invalid .rbxm binary: header smaller than 32 bytes".to_string());
    }
    if bytes.get(..14) != Some(ROBLOX_BIN_MAGIC.as_slice()) {
        if bytes.starts_with(b"<roblox") {
            return Err("file looks like XML (.rbxmx/.rbxlx), not binary .rbxm".to_string());
        }
        return Err("invalid .rbxm binary: missing <roblox! binary signature".to_string());
    }
    let version = u16::from_le_bytes([bytes[14], bytes[15]]);
    if version > 1 {
        return Err(format!("unsupported .rbxm binary version {version}"));
    }
    let class_count = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let inst_count = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if class_count > 100_000 || inst_count > 100_000 {
        return Err("invalid .rbxm header: class/instance count exceeds bounds".to_string());
    }

    let mut pos = 32usize;
    let mut warnings = Vec::new();
    let mut zstd_seen = false;
    let mut classes: BTreeMap<u32, (String, Vec<i32>)> = BTreeMap::new();
    let mut nodes: BTreeMap<i32, ImportedNode> = BTreeMap::new();
    let mut parent_links: Vec<(i32, i32)> = Vec::new();

    while pos < bytes.len() {
        if bytes.len() - pos < 16 {
            return Err("corrupt .rbxm binary: truncated chunk header".to_string());
        }
        let tag = &bytes[pos..pos + 4];
        let comp_len = u32::from_le_bytes([bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], bytes[pos + 7]]) as usize;
        let uncomp_len = u32::from_le_bytes([bytes[pos + 8], bytes[pos + 9], bytes[pos + 10], bytes[pos + 11]]) as usize;
        pos += 16;

        let payload_len = if comp_len == 0 { uncomp_len } else { comp_len };
        if payload_len > MAX_PREFAB_BYTES || uncomp_len > MAX_PREFAB_BYTES {
            return Err("corrupt .rbxm binary: chunk size exceeds 16 MiB".to_string());
        }
        let next_pos = pos
            .checked_add(payload_len)
            .ok_or_else(|| "corrupt .rbxm binary: chunk length overflow".to_string())?;
        if next_pos > bytes.len() {
            return Err("corrupt .rbxm binary: chunk extends past EOF".to_string());
        }
        let raw_payload = &bytes[pos..next_pos];
        pos = next_pos;

        if tag == b"END\0" {
            break;
        }
        let data_buf: Vec<u8>;
        let payload: &[u8] = if comp_len == 0 {
            raw_payload
        } else if raw_payload.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
            zstd_seen = true;
            continue;
        } else {
            data_buf = decompress_lz4_block(raw_payload, uncomp_len)?;
            &data_buf
        };

        if tag == b"INST" {
            let mut off = 0usize;
            let cid = read_u32_le(payload, &mut off)?;
            let cname_len = read_u32_le(payload, &mut off)? as usize;
            let cname_end = off.checked_add(cname_len).ok_or("corrupt INST class name")?;
            let cname_bytes = payload.get(off..cname_end).ok_or("truncated INST class name")?;
            let cname = std::str::from_utf8(cname_bytes)
                .map_err(|_| "non-UTF8 INST class name".to_string())?
                .to_string();
            off = cname_end;
            let _obj_fmt = *payload.get(off).ok_or("truncated INST format byte")?;
            off += 1;
            let count = read_u32_le(payload, &mut off)? as usize;
            if count > 100_000 {
                return Err("INST instance count exceeds limit".to_string());
            }
            let refs = read_rbxm_referents(payload, &mut off, count)?;
            for &r in &refs {
                nodes.insert(
                    r,
                    ImportedNode {
                        class_name: cname.clone(),
                        name: cname.clone(),
                        properties: BTreeMap::new(),
                        source: None,
                        children: Vec::new(),
                    },
                );
            }
            classes.insert(cid, (cname, refs));
        } else if tag == b"PROP" {
            let mut off = 0usize;
            let cid = read_u32_le(payload, &mut off)?;
            let pname_len = read_u32_le(payload, &mut off)? as usize;
            let pname_end = off.checked_add(pname_len).ok_or("corrupt PROP name")?;
            let pname_bytes = payload.get(off..pname_end).ok_or("truncated PROP name")?;
            let pname = std::str::from_utf8(pname_bytes)
                .map_err(|_| "non-UTF8 PROP name".to_string())?
                .to_string();
            off = pname_end;
            let type_id = *payload.get(off).ok_or("truncated PROP type id")?;
            off += 1;
            if let Some((_cname, refs)) = classes.get(&cid) {
                let count = refs.len();
                match type_id {
                    0x01 => {
                        for &r in refs {
                            let slen = read_u32_le(payload, &mut off)? as usize;
                            let send = off.checked_add(slen).ok_or("corrupt PROP string")?;
                            let sbytes = payload.get(off..send).ok_or("truncated PROP string")?;
                            off = send;
                            let val = String::from_utf8_lossy(sbytes).into_owned();
                            if let Some(node) = nodes.get_mut(&r) {
                                if pname == "Name" && !val.trim().is_empty() {
                                    node.name = val;
                                } else if pname == "Source" {
                                    node.source = Some(val);
                                } else {
                                    node.properties.insert(pname.clone(), Value::String(val));
                                }
                            }
                        }
                    }
                    0x02 => {
                        let bslice = payload.get(off..off + count).ok_or("truncated PROP bools")?;
                        for (idx, &r) in refs.iter().enumerate() {
                            if let Some(node) = nodes.get_mut(&r) {
                                node.properties.insert(pname.clone(), Value::Bool(bslice[idx] != 0));
                            }
                        }
                    }
                    0x03 => {
                        let need = count.checked_mul(4).ok_or("PROP int overflow")?;
                        let slice = payload.get(off..off + need).ok_or("truncated PROP ints")?;
                        for (idx, &r) in refs.iter().enumerate() {
                            let u = u32::from_be_bytes([
                                slice[idx],
                                slice[count + idx],
                                slice[2 * count + idx],
                                slice[3 * count + idx],
                            ]);
                            let v = ((u >> 1) as i32) ^ (-((u & 1) as i32));
                            if let Some(node) = nodes.get_mut(&r) {
                                node.properties.insert(pname.clone(), json!(v));
                            }
                        }
                    }
                    _ => {}
                }
            }
        } else if tag == b"PRNT" {
            let mut off = 0usize;
            let _ver = *payload.get(off).ok_or("truncated PRNT version")?;
            off += 1;
            let count = read_u32_le(payload, &mut off)? as usize;
            if count > 100_000 {
                return Err("PRNT count exceeds limit".to_string());
            }
            let child_refs = read_rbxm_referents(payload, &mut off, count)?;
            let parent_refs = read_rbxm_referents(payload, &mut off, count)?;
            for (c, p) in child_refs.into_iter().zip(parent_refs.into_iter()) {
                parent_links.push((c, p));
            }
        }
    }

    if zstd_seen && nodes.is_empty() {
        warnings.push(
            "Binary .rbxm uses Zstd-compressed chunks; validated header and created placeholder Model container (re-export as .rbxmx in Studio for full property extraction)."
                .to_string(),
        );
    }

    if nodes.is_empty() {
        if !zstd_seen && inst_count == 0 {
            warnings.push(
                "Binary .rbxm header contained 0 instances; created empty Model container."
                    .to_string(),
            );
        }
        return Ok((
            vec![ImportedNode {
                class_name: "Model".to_string(),
                name: fallback_name.to_string(),
                properties: BTreeMap::new(),
                source: None,
                children: Vec::new(),
            }],
            warnings,
        ));
    }

    let mut child_map: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    let mut root_refs: Vec<i32> = Vec::new();
    for (c, p) in parent_links {
        if p < 0 || !nodes.contains_key(&p) {
            root_refs.push(c);
        } else {
            child_map.entry(p).or_default().push(c);
        }
    }
    if root_refs.is_empty() {
        root_refs = nodes.keys().copied().collect();
    }

    fn build_subtree(
        id: i32,
        nodes: &mut BTreeMap<i32, ImportedNode>,
        child_map: &BTreeMap<i32, Vec<i32>>,
        depth: usize,
    ) -> Option<ImportedNode> {
        if depth > MAX_PREFAB_XML_DEPTH {
            return None;
        }
        let mut node = nodes.remove(&id)?;
        if let Some(kids) = child_map.get(&id) {
            for &kid in kids {
                if let Some(child) = build_subtree(kid, nodes, child_map, depth + 1) {
                    node.children.push(child);
                }
            }
        }
        Some(node)
    }

    let mut roots = Vec::new();
    for r in root_refs {
        if let Some(tree) = build_subtree(r, &mut nodes, &child_map, 0) {
            roots.push(tree);
        }
    }
    Ok((roots, warnings))
}

fn decompose_imported_node(
    node: &ImportedNode,
    ctx: &mut DecomposeCtx<'_>,
    rel_segs: &[String],
    depth: usize,
) -> Result<Option<Value>, String> {
    let safe_name = sanitize_prefab_node_name(&node.name, &node.class_name);
    if is_script_class(&node.class_name) {
        let suffix = match node.class_name.as_str() {
            "Script" => ".server.luau",
            "LocalScript" => ".client.luau",
            _ => ".luau",
        };
        let parent_dir = if rel_segs.is_empty() {
            format!("src/prefabs/{}", ctx.slug_name)
        } else {
            format!("src/prefabs/{}/{}", ctx.slug_name, rel_segs.join("/"))
        };
        let file_stem = slug(&safe_name);
        let file_stem = if file_stem.is_empty() { "Script".to_string() } else { safe_name.clone() };
        let mut candidate = format!("{parent_dir}/{file_stem}{suffix}");
        let mut counter = 2usize;
        while ctx.used_script_paths.contains(&candidate) {
            candidate = format!("{parent_dir}/{file_stem}_{counter}{suffix}");
            counter += 1;
        }
        ctx.used_script_paths.insert(candidate.clone());
        let src = node.source.as_deref().unwrap_or("");
        prefab_write_atomic(ctx.dir, &candidate, src)?;
        ctx.extracted_scripts.push(candidate.clone());
        if depth == 1 {
            ctx.direct_root_scripts.push((safe_name, candidate));
        } else {
            ctx.deep_scripts
                .push((node.class_name.clone(), safe_name, candidate));
        }
        return Ok(None);
    }

    let mut next_segs = rel_segs.to_vec();
    if depth > 0 {
        next_segs.push(safe_name.clone());
    }
    let mut children_json = Vec::new();
    for child in &node.children {
        if let Some(child_val) = decompose_imported_node(child, ctx, &next_segs, depth + 1)? {
            children_json.push(child_val);
        }
    }
    let props_map: serde_json::Map<String, Value> = node
        .properties
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Ok(Some(json!({
        "className": node.class_name,
        "name": safe_name,
        "properties": Value::Object(props_map),
        "children": children_json,
    })))
}

/// Import a `.rbxmx`, `.rbxm`, or `.rbxlx` prefab payload (base64, <= 16 MiB),
/// decomposing its hierarchy into `scene/models/<slug>.model.json` and extracted
/// `.luau` scripts, and wiring them into `default.project.json`.
pub fn import_prefab_model(
    dir: &Path,
    file_name: &str,
    format: &str,
    bytes_base64: &str,
    target_service: Option<&str>,
) -> Result<Value, String> {
    let raw_fname = file_name.trim();
    if raw_fname.is_empty()
        || raw_fname.contains('\0')
        || raw_fname.contains("..")
        || raw_fname.chars().any(|c| c.is_control())
    {
        return Err("invalid file_name (path traversal or control characters forbidden)".into());
    }
    let base_name = raw_fname.rsplit(['/', '\\']).next().unwrap_or(raw_fname);
    let stem = base_name
        .strip_suffix(".rbxmx")
        .or_else(|| base_name.strip_suffix(".rbxm"))
        .or_else(|| base_name.strip_suffix(".rbxlx"))
        .or_else(|| base_name.strip_suffix(".RBXMX"))
        .or_else(|| base_name.strip_suffix(".RBXM"))
        .or_else(|| base_name.strip_suffix(".RBXLX"))
        .unwrap_or(base_name);
    let model_slug = slug(stem);
    if model_slug.is_empty() {
        return Err("file_name must contain alphanumeric characters".into());
    }

    let fmt = format.trim().to_ascii_lowercase();
    if !matches!(fmt.as_str(), "rbxmx" | "rbxm" | "rbxlx") {
        return Err(format!("unsupported format '{format}': expected rbxmx, rbxm, or rbxlx"));
    }

    let wired_service = target_service
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Workspace");
    if !ALLOWED_PREFAB_SERVICES.contains(&wired_service) {
        return Err(format!("invalid target_service '{wired_service}'"));
    }

    let b64_clean = bytes_base64
        .trim()
        .split_once(";base64,")
        .map(|(_, rest)| rest)
        .unwrap_or(bytes_base64.trim());
    if b64_clean.is_empty() {
        return Err("bytes_base64 must not be empty".into());
    }
    // Quick pre-decode upper bound check (~22.4 MB base64 for 16 MiB binary)
    let non_ws_len = b64_clean.bytes().filter(|b| !b.is_ascii_whitespace()).count();
    if (non_ws_len / 4) * 3 > MAX_PREFAB_BYTES + 3 {
        return Err(format!("decoded prefab exceeds {MAX_PREFAB_BYTES} byte (16 MiB) limit"));
    }
    let decoded = crate::assets::decode_base64(b64_clean)?;
    if decoded.is_empty() {
        return Err("decoded prefab payload is empty".into());
    }
    if decoded.len() > MAX_PREFAB_BYTES {
        return Err(format!("decoded prefab exceeds {MAX_PREFAB_BYTES} byte (16 MiB) limit"));
    }

    let mut warnings: Vec<String> = Vec::new();
    let display_name = sanitize_prefab_node_name(stem, &model_slug);

    let root_node = match fmt.as_str() {
        "rbxmx" => {
            let roots = parse_prefab_xml_items(&decoded)?;
            if roots.is_empty() {
                warnings.push("XML .rbxmx contained no <Item> elements; created empty Model.".into());
                ImportedNode {
                    class_name: "Model".into(),
                    name: display_name.clone(),
                    properties: BTreeMap::new(),
                    source: None,
                    children: Vec::new(),
                }
            } else if roots.len() == 1 {
                roots.into_iter().next().unwrap()
            } else {
                ImportedNode {
                    class_name: "Model".into(),
                    name: display_name.clone(),
                    properties: BTreeMap::new(),
                    source: None,
                    children: roots,
                }
            }
        }
        "rbxlx" => {
            let roots = parse_prefab_xml_items(&decoded)?;
            let mut target_children = Vec::new();
            let mut extra_scripts = Vec::new();
            for r in roots {
                if r.class_name == wired_service || r.name == wired_service {
                    target_children.extend(r.children);
                } else if ALLOWED_PREFAB_SERVICES.contains(&r.class_name.as_str()) {
                    fn collect_svc_scripts(n: &ImportedNode, out: &mut Vec<ImportedNode>) {
                        if is_script_class(&n.class_name) {
                            out.push(n.clone());
                        } else {
                            for c in &n.children {
                                collect_svc_scripts(c, out);
                            }
                        }
                    }
                    collect_svc_scripts(&r, &mut extra_scripts);
                    if !r.children.is_empty() && r.class_name != "ServerScriptService" && r.class_name != "StarterPlayer" && r.class_name != "ReplicatedStorage" {
                        target_children.extend(r.children);
                    }
                } else {
                    target_children.push(r);
                }
            }
            if !extra_scripts.is_empty() {
                warnings.push(format!(
                    "Extracted {} script(s) from non-{wired_service} services in .rbxlx.",
                    extra_scripts.len()
                ));
                target_children.extend(extra_scripts);
            }
            ImportedNode {
                class_name: "Model".into(),
                name: display_name.clone(),
                properties: BTreeMap::new(),
                source: None,
                children: target_children,
            }
        }
        "rbxm" => {
            let (roots, rbxm_warnings) = parse_rbxm_binary(&decoded, &display_name)?;
            warnings.extend(rbxm_warnings);
            if roots.len() == 1 {
                roots.into_iter().next().unwrap()
            } else {
                ImportedNode {
                    class_name: "Model".into(),
                    name: display_name.clone(),
                    properties: BTreeMap::new(),
                    source: None,
                    children: roots,
                }
            }
        }
        _ => unreachable!(),
    };

    let mut ctx = DecomposeCtx {
        dir,
        slug_name: &model_slug,
        extracted_scripts: Vec::new(),
        direct_root_scripts: Vec::new(),
        deep_scripts: Vec::new(),
        used_script_paths: std::collections::HashSet::new(),
    };

    let model_json = match decompose_imported_node(&root_node, &mut ctx, &[], 0)? {
        Some(v) => v,
        None => {
            // Root itself was a script; emit an empty Folder placeholder for model_path
            json!({
                "className": "Folder",
                "name": display_name,
                "properties": {},
                "children": []
            })
        }
    };
    let extracted_scripts = ctx.extracted_scripts;
    let direct_root_scripts = ctx.direct_root_scripts;
    let deep_scripts = ctx.deep_scripts;

    let model_rel = format!("scene/models/{model_slug}.model.json");
    let model_text = serde_json::to_string_pretty(&model_json).map_err(|e| e.to_string())? + "\n";
    prefab_write_atomic(dir, &model_rel, &model_text)?;

    let m = manifest(dir)?;
    let rojo_file = m["rojoProject"]
        .as_str()
        .unwrap_or(DEFAULT_ROJO_FILE)
        .to_string();
    let rojo_full = safe_join(dir, &rojo_file)?;
    let mut rojo: Value = if rojo_full.is_file() {
        let raw = std::fs::read_to_string(&rojo_full).map_err(|e| format!("read {rojo_file}: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("{rojo_file}: {e}"))?
    } else {
        json!({
            "name": m["name"].as_str().unwrap_or("Project"),
            "tree": {
                "$className": "DataModel",
                "$ignoreUnknownInstances": true
            }
        })
    };

    let model_node_name = model_json
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(&model_slug)
        .to_string();

    {
        let root_obj = rojo
            .as_object_mut()
            .ok_or_else(|| format!("{rojo_file} must be a JSON object"))?;
        let tree_obj = root_obj
            .entry("tree")
            .or_insert_with(|| json!({ "$className": "DataModel" }))
            .as_object_mut()
            .ok_or_else(|| format!("{rojo_file} tree must be an object"))?;

        let svc_obj = tree_obj
            .entry(wired_service.to_string())
            .or_insert_with(|| {
                json!({
                    "$className": wired_service,
                    "$ignoreUnknownInstances": true
                })
            })
            .as_object_mut()
            .ok_or_else(|| format!("{rojo_file} {wired_service} must be an object"))?;
        if !svc_obj.contains_key("$path") && !svc_obj.contains_key("$className") {
            svc_obj.insert("$className".into(), json!(wired_service));
        }
        svc_obj.entry("$ignoreUnknownInstances").or_insert(json!(true));

        let mut model_wire = serde_json::Map::new();
        model_wire.insert("$path".into(), json!(model_rel));
        for (s_name, s_rel) in &direct_root_scripts {
            model_wire.insert(s_name.clone(), json!({ "$path": s_rel }));
        }
        svc_obj.insert(model_node_name.clone(), Value::Object(model_wire));

        if !deep_scripts.is_empty() {
            warnings.push(format!(
                "Relocated {} nested prefab script(s) under service Prefabs/{model_slug} to keep Rojo tree valid.",
                deep_scripts.len()
            ));
            for (idx, (cls, s_name, s_rel)) in deep_scripts.iter().enumerate() {
                let target_svc = match cls.as_str() {
                    "LocalScript" => "StarterPlayer",
                    "ModuleScript" => "ReplicatedStorage",
                    _ => "ServerScriptService",
                };
                let container = if target_svc == "StarterPlayer" {
                    tree_obj
                        .entry("StarterPlayer".to_string())
                        .or_insert_with(|| json!({ "$className": "StarterPlayer", "$ignoreUnknownInstances": true }))
                        .as_object_mut()
                        .ok_or("StarterPlayer must be object")?
                        .entry("StarterPlayerScripts".to_string())
                        .or_insert_with(|| json!({ "$className": "StarterPlayerScripts", "$ignoreUnknownInstances": true }))
                        .as_object_mut()
                        .ok_or("StarterPlayerScripts must be object")?
                } else {
                    tree_obj
                        .entry(target_svc.to_string())
                        .or_insert_with(|| json!({ "$className": target_svc, "$ignoreUnknownInstances": true }))
                        .as_object_mut()
                        .ok_or("target script service must be object")?
                };
                let prefabs_folder = container
                    .entry("Prefabs".to_string())
                    .or_insert_with(|| json!({ "$className": "Folder", "$ignoreUnknownInstances": true }))
                    .as_object_mut()
                    .ok_or("Prefabs must be object")?
                    .entry(model_slug.clone())
                    .or_insert_with(|| json!({ "$className": "Folder", "$ignoreUnknownInstances": true }))
                    .as_object_mut()
                    .ok_or("Prefab slug folder must be object")?;
                let mut key = s_name.clone();
                if prefabs_folder.contains_key(&key) {
                    key = format!("{s_name}_{}", idx + 2);
                }
                prefabs_folder.insert(key, json!({ "$path": s_rel }));
            }
        }
    }

    let rojo_text = serde_json::to_string_pretty(&rojo).map_err(|e| e.to_string())? + "\n";
    prefab_write_atomic(dir, &rojo_file, &rojo_text)?;

    // Validate the resulting Rojo tree loads cleanly
    let _ = load_tree(dir, &rojo_file)?;

    Ok(json!({
        "ok": true,
        "model_path": model_rel,
        "extracted_scripts": extracted_scripts,
        "wired_service": wired_service,
        "warnings": warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("art/**", "art/cards/a.png"));
        assert!(glob_match("art/ui/*.png", "art/ui/a.png"));
        assert!(!glob_match("art/ui/*.png", "art/ui/x/a.png"));
        assert!(glob_match("**/*.png", "a/b/c.png"));
        assert!(!glob_match("art/*", "other/a.png"));
    }

    #[test]
    fn safe_join_refuses_escapes() {
        let d = std::env::temp_dir();
        assert!(safe_join(&d, "../x").is_err());
        assert!(safe_join(&d, "/etc/passwd").is_err());
        assert!(safe_join(&d, "a/b.txt").is_ok());
    }

    #[test]
    fn format_targets_fail_closed_and_dedup() {
        let base = std::env::var("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join(format!("openro-fmt-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let proj = base.join("proj");
        std::fs::create_dir_all(proj.join("src")).unwrap();
        std::fs::create_dir_all(proj.join(".git")).unwrap();
        std::fs::create_dir_all(proj.join(".openro")).unwrap();
        std::fs::create_dir_all(proj.join("build")).unwrap();
        std::fs::write(proj.join("src/A.luau"), "local x = 1\nreturn x\n").unwrap();
        std::fs::write(proj.join("openro.project.json"), "{}\n").unwrap();
        std::fs::write(base.join("outside.luau"), "local y = 2\n").unwrap();

        // Default None / empty falls back to luau_target_dirs
        assert_eq!(resolve_format_targets(&proj, None).unwrap(), vec!["src"]);
        assert_eq!(resolve_format_targets(&proj, Some(&[])).unwrap(), vec!["src"]);

        // Valid paths normalize and deduplicate
        let ok = resolve_format_targets(
            &proj,
            Some(&["./src/A.luau".to_string(), "src/A.luau".to_string(), "src".to_string()]),
        )
        .unwrap();
        assert_eq!(ok, vec!["src/A.luau", "src"]);

        // Refuses escapes, CLI flags, blocked folders, non-luau files, missing targets, control chars
        for bad in [
            "../outside.luau",
            "src/../../outside.luau",
            "/etc/passwd",
            "--check",
            "-f",
            "",
            "   ",
            "src/A.luau\0",
            ".git",
            ".openro",
            "build",
            "openro.project.json",
            "src/missing.luau",
        ] {
            let res = resolve_format_targets(&proj, Some(&[bad.to_string()]));
            assert!(res.is_err(), "expected refusal for '{bad}', got {res:?}");
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("outside.luau"), proj.join("src/link.luau"))
                .unwrap();
            let res = resolve_format_targets(&proj, Some(&["src/link.luau".to_string()]));
            assert!(res.is_err(), "expected escaping symlink refusal, got {res:?}");
        }

        let _ = std::fs::remove_dir_all(&base);
    }
}
