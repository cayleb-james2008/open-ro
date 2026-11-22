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
            "universeId": universe, "placeId": place, "products": [],
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
        fn go(n: &Node, prefix: &str, service: bool, out: &mut Vec<Flat>) {
            let path = if prefix.is_empty() { n.name.clone() } else { format!("{prefix}/{}", n.name) };
            out.push(Flat {
                path: path.clone(),
                class: n.class.clone(),
                props: n.props.clone(),
                source: n.source.clone(),
                service,
                ignore_unknown: n.ignore_unknown,
            });
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

fn model_json_node(v: &Value, name_hint: Option<&str>) -> Result<Node, String> {
    let class = v.get("ClassName").and_then(|c| c.as_str()).ok_or("model.json needs ClassName")?;
    let name = v.get("Name").and_then(|n| n.as_str()).or(name_hint).ok_or("model.json child needs Name")?;
    let mut node = Node {
        name: name.to_string(),
        class: class.to_string(),
        props: props_from(v.get("Properties")),
        source: None,
        children: Vec::new(),
        ignore_unknown: false,
    };
    if let Some(kids) = v.get("Children").and_then(|c| c.as_array()) {
        for k in kids {
            node.children.push(model_json_node(k, None)?);
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
        let mut node = Node { name: fname.clone(), class: "Folder".into(), props: BTreeMap::new(), source: None, children: Vec::new(), ignore_unknown: false };
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
        return model_json_node(&v, Some(stem)).map(Some);
    }
    if let Some(stem) = fname.strip_suffix(".txt") {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {fname}: {e}"))?;
        let mut props = BTreeMap::new();
        props.insert("Value".to_string(), json!(text));
        return Ok(Some(Node { name: stem.to_string(), class: "StringValue".into(), props, source: None, children: Vec::new(), ignore_unknown: false }));
    }
    if fname.ends_with(".meta.json") {
        return Ok(None);
    }
    skipped.push(rel_display(root, path));
    Ok(None)
}

fn load_project_node(name: &str, v: &Value, root: &Path, skipped: &mut Vec<String>, service_level: bool) -> Result<Node, String> {
    let obj = v.as_object().ok_or(format!("'{name}' in the project file must be an object"))?;
    let class_name = obj.get("$className").and_then(|c| c.as_str());
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
        Node { name: name.to_string(), class, props: BTreeMap::new(), source: None, children: Vec::new(), ignore_unknown: true }
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
        if node.children.iter().any(|c| &c.name == k) {
            return Err(format!("'{k}' appears twice under '{name}'"));
        }
        node.children.push(load_project_node(k, child, root, skipped, false)?);
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
        services.push(load_project_node(k, child, dir, &mut skipped, true)?);
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
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn xml_prop(name: &str, v: &Value, warnings: &mut Vec<String>, owner: &str) -> Option<String> {
    let n = xml_escape(name);
    let num = |x: &Value| x.as_f64().unwrap_or(0.0);
    let arr = |x: &Value| x.as_array().cloned().unwrap_or_default();
    match v {
        Value::Bool(b) => Some(format!("<bool name=\"{n}\">{b}</bool>")),
        Value::Number(x) => Some(format!("<float name=\"{n}\">{}</float>", x.as_f64().unwrap_or(0.0))),
        Value::String(s) => Some(format!("<string name=\"{n}\">{}</string>", xml_escape(s))),
        Value::Object(o) if o.len() == 1 => {
            let (kind, val) = o.iter().next().unwrap();
            let a = arr(val);
            match (kind.as_str(), a.len()) {
                ("Vector3", 3) => Some(format!("<Vector3 name=\"{n}\"><X>{}</X><Y>{}</Y><Z>{}</Z></Vector3>", num(&a[0]), num(&a[1]), num(&a[2]))),
                ("Vector2", 2) => Some(format!("<Vector2 name=\"{n}\"><X>{}</X><Y>{}</Y></Vector2>", num(&a[0]), num(&a[1]))),
                ("Color3", 3) => Some(format!("<Color3 name=\"{n}\"><R>{}</R><G>{}</G><B>{}</B></Color3>", num(&a[0]), num(&a[1]), num(&a[2]))),
                ("UDim2", 2) => {
                    let (x, y) = (arr(&a[0]), arr(&a[1]));
                    if x.len() == 2 && y.len() == 2 {
                        Some(format!(
                            "<UDim2 name=\"{n}\"><XS>{}</XS><XO>{}</XO><YS>{}</YS><YO>{}</YO></UDim2>",
                            num(&x[0]), num(&x[1]), num(&y[0]), num(&y[1])
                        ))
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

/// Built-in `.rbxlx` writer: scripts, folders and plain properties only.
pub fn builtin_rbxlx(tree: &Tree, warnings: &mut Vec<String>) -> String {
    fn item(n: &Node, prefix: &str, counter: &mut usize, out: &mut String, warnings: &mut Vec<String>) {
        let path = if prefix.is_empty() { n.name.clone() } else { format!("{prefix}/{}", n.name) };
        *counter += 1;
        out.push_str(&format!("<Item class=\"{}\" referent=\"RBX{}\"><Properties><string name=\"Name\">{}</string>", xml_escape(&n.class), counter, xml_escape(&n.name)));
        if let Some(src) = &n.source {
            out.push_str(&format!("<ProtectedString name=\"Source\"><![CDATA[{}]]></ProtectedString>", src.replace("]]>", "]]]]><![CDATA[>")));
        }
        for (k, v) in &n.props {
            if let Some(x) = xml_prop(k, v, warnings, &path) {
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

/// Stage ONLY `files` inside `dir` (via `safe_join`, skipping `.openro/`, `.git/`,
/// `build/`, `art/references/`, `refs/`, and refusing files over 5 MB) and create one
/// local commit with `message`. Returns `Ok(Some(sha))` on commit, or `Ok(None)` when
/// `dir` is not in a git repo or none of the files have changes to commit.
pub fn git_commit(dir: &Path, files: &[String], message: &str) -> Result<Option<String>, String> {
    let msg = message.trim();
    if msg.is_empty() || msg.contains('\0') {
        return Err("commit message must be non-empty".into());
    }
    if git_repo_root(dir).is_none() {
        return Ok(None);
    }
    let mut clean_rels: Vec<String> = Vec::new();
    for raw in files {
        let full = safe_join(dir, raw)?;
        let rel = rel_display(dir, &full);
        if is_blocked_git_rel(&rel) {
            continue;
        }
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

/// Static project verification: Luau static analyzer + `selene` + `stylua --check` (#18).
pub fn check_project(dir: &Path) -> Result<Value, String> {
    let m = manifest(dir)?;
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
            let mut args = vec!["--check".to_string()];
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
    Ok(json!({
        "passed": passed,
        "luau": luau_json,
        "selene": selene_json,
        "stylua": stylua_json,
    }))
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
}
