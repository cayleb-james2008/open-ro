//! Headless Local Roblox Luau Runtime Sandbox (Gap #2).
//!
//! Materializes a Rojo project (`default.project.json`) or in-memory `LocalPlace`
//! into an isolated temporary run directory and executes it against an embedded
//! Lune-compatible Roblox `DataModel`/Service runtime (`sandbox_runtime.luau`),
//! capturing real Luau stack traces, `DataStoreService` persistence across
//! `BindToClose`/`rejoin`, `ProcessReceipt` duplicate-`PurchaseId` idempotency,
//! `RemoteEvent`/`RemoteFunction` traffic, and live `PlayerGui` trees.

use crate::projects::{self, Node};
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::{BufWriter, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Embedded Lune Roblox DataModel/Service runtime shim.
pub const SANDBOX_RUNTIME_LUAU: &str = include_str!("sandbox_runtime.luau");

const DEFAULT_TIMEOUT_SECS: u64 = 15;
const MIN_TIMEOUT_SECS: u64 = 1;
const MAX_TIMEOUT_SECS: u64 = 30;
const STDIO_CAP_BYTES: u64 = 65_536;
const MAX_PLAYERS: usize = 32;
const MAX_JOURNEY_STEPS: usize = 200;
const MAX_REPORT_BYTES: u64 = 8 * 1024 * 1024;

static RUN_SEQ: AtomicU64 = AtomicU64::new(1);

struct ScratchDirGuard(PathBuf);

impl Drop for ScratchDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Resolve the `lune` executable across Unix (`PATH`, `$HOME/.local/bin/lune`)
/// and Windows (`PATH`, `%USERPROFILE%\.local\bin\lune.exe`, `%HOME%\.local\bin\lune.exe`).
/// Returns `(found_path, searched_candidates)` for structured diagnostics.
pub fn find_lune_binary() -> (Option<PathBuf>, Vec<String>) {
    let mut searched = Vec::new();
    if let Ok(override_val) = std::env::var("OPENRO_LUNE") {
        let trimmed = override_val.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            searched.push(format!("OPENRO_LUNE={}", p.display()));
            return (p.is_file().then_some(p), searched);
        }
    }

    let names: &[&str] = if cfg!(windows) {
        &["lune.exe", "lune"]
    } else {
        &["lune"]
    };

    if let Some(path_os) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_os) {
            for name in names {
                let cand = dir.join(name);
                if cand.is_file() {
                    searched.push(cand.display().to_string());
                    return (Some(cand), searched);
                }
            }
        }
        searched.push("PATH".to_string());
    }

    for env_key in ["HOME", "USERPROFILE"] {
        if let Some(home) = std::env::var_os(env_key) {
            let bin_dir = PathBuf::from(home).join(".local").join("bin");
            for name in names {
                let cand = bin_dir.join(name);
                searched.push(cand.display().to_string());
                if cand.is_file() {
                    return (Some(cand), searched);
                }
            }
        }
    }

    (None, searched)
}

fn node_to_json(node: &Node) -> Value {
    let children: Vec<Value> = node.children.iter().map(node_to_json).collect();
    json!({
        "name": node.name,
        "class_name": node.class,
        "props": node.props,
        "source": node.source,
        "source_file": node.source_file,
        "children": children,
    })
}

fn extract_product_ids(manifest: &Value) -> Vec<u64> {
    let mut ids = Vec::new();
    if let Some(products) = manifest.get("products").and_then(Value::as_array) {
        for p in products {
            let kind = p.get("kind").and_then(Value::as_str).unwrap_or("");
            if kind.eq_ignore_ascii_case("gamepass") {
                continue;
            }
            for key in ["robloxId", "productId", "id"] {
                if let Some(n) = p.get(key).and_then(Value::as_u64) {
                    if n > 0 && !ids.contains(&n) {
                        ids.push(n);
                    }
                }
            }
        }
    }
    ids
}

#[derive(Clone)]
struct MemNode {
    name: String,
    class_name: String,
    props: Map<String, Value>,
    source: Option<String>,
    source_file: Option<String>,
    children: BTreeMap<String, MemNode>,
}

impl MemNode {
    fn container(name: &str, class_name: &str) -> Self {
        Self {
            name: name.to_string(),
            class_name: class_name.to_string(),
            props: Map::new(),
            source: None,
            source_file: None,
            children: BTreeMap::new(),
        }
    }

    fn into_json(self) -> Value {
        let children: Vec<Value> = self.children.into_values().map(MemNode::into_json).collect();
        json!({
            "name": self.name,
            "class_name": self.class_name,
            "props": Value::Object(self.props),
            "source": self.source,
            "source_file": self.source_file,
            "children": children,
        })
    }
}

fn infer_local_script_class(service: &str, leaf_name: &str) -> &'static str {
    if leaf_name.ends_with(".server") {
        "Script"
    } else if leaf_name.ends_with(".client") {
        "LocalScript"
    } else if leaf_name.ends_with(".module") {
        "ModuleScript"
    } else {
        match service {
            "StarterGui" | "StarterPlayer" => "LocalScript",
            "ReplicatedStorage" | "ServerStorage" => "ModuleScript",
            _ => "Script",
        }
    }
}

fn materialize_local_place_tree(
    game_name: &str,
    scripts: &BTreeMap<String, String>,
    instances: &BTreeMap<String, Value>,
) -> Value {
    let mut services: BTreeMap<String, MemNode> = BTreeMap::new();
    for svc in [
        "Workspace",
        "ServerScriptService",
        "StarterGui",
        "StarterPlayer",
        "ReplicatedStorage",
        "ServerStorage",
        "StarterPack",
        "Lighting",
        "SoundService",
    ] {
        services.insert(svc.to_string(), MemNode::container(svc, svc));
    }
    if let Some(sp) = services.get_mut("StarterPlayer") {
        sp.children.insert(
            "StarterPlayerScripts".to_string(),
            MemNode::container("StarterPlayerScripts", "StarterPlayerScripts"),
        );
    }

    for (path, rec) in instances {
        let segs: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
        if segs.is_empty() {
            continue;
        }
        let svc_name = segs[0];
        let mut cur = services
            .entry(svc_name.to_string())
            .or_insert_with(|| MemNode::container(svc_name, svc_name));
        let class_name = rec
            .get("class_name")
            .and_then(Value::as_str)
            .unwrap_or("Folder");
        let props = rec
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if segs.len() == 1 {
            cur.props.extend(props);
            continue;
        }
        for (idx, seg) in segs[1..].iter().enumerate() {
            let is_leaf = idx + 2 == segs.len();
            let default_cls = if svc_name == "StarterPlayer" && *seg == "StarterPlayerScripts" {
                "StarterPlayerScripts"
            } else if svc_name == "StarterPlayer" && *seg == "StarterCharacterScripts" {
                "StarterCharacterScripts"
            } else {
                "Folder"
            };
            cur = cur
                .children
                .entry((*seg).to_string())
                .or_insert_with(|| MemNode::container(seg, default_cls));
            if is_leaf {
                cur.class_name = class_name.to_string();
                cur.props.extend(props.clone());
            }
        }
    }

    for (path, src) in scripts {
        let raw: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
        if raw.is_empty() {
            continue;
        }
        let (svc_name, rest): (&str, Vec<&str>) = match raw.as_slice() {
            ["StarterPlayer", name]
                if *name != "StarterPlayerScripts" && *name != "StarterCharacterScripts" =>
            {
                ("StarterPlayer", vec!["StarterPlayerScripts", *name])
            }
            _ => (raw[0], raw[1..].to_vec()),
        };
        if rest.is_empty() {
            continue;
        }
        let mut cur = services
            .entry(svc_name.to_string())
            .or_insert_with(|| MemNode::container(svc_name, svc_name));
        for (idx, seg) in rest.iter().enumerate() {
            let is_leaf = idx + 1 == rest.len();
            let default_cls = if is_leaf {
                infer_local_script_class(svc_name, seg)
            } else if svc_name == "StarterPlayer" && *seg == "StarterPlayerScripts" {
                "StarterPlayerScripts"
            } else if svc_name == "StarterPlayer" && *seg == "StarterCharacterScripts" {
                "StarterCharacterScripts"
            } else {
                "Folder"
            };
            cur = cur
                .children
                .entry((*seg).to_string())
                .or_insert_with(|| MemNode::container(seg, default_cls));
            if is_leaf {
                if !projects::is_script_class(&cur.class_name) {
                    cur.class_name = infer_local_script_class(svc_name, seg).to_string();
                }
                cur.source = Some(src.clone());
                cur.source_file = Some(path.clone());
            }
        }
    }

    let svc_list: Vec<Value> = services.into_values().map(MemNode::into_json).collect();
    json!({
        "name": game_name,
        "services": svc_list,
    })
}

fn empty_state_snapshot() -> Value {
    json!({
        "leaderstats": {},
        "datastores": {},
        "receipts_processed": [],
        "remotes_fired": 0,
        "player_gui_tree": [],
    })
}

fn default_core_loop(status: &str) -> Value {
    json!({
        "move": { "status": status, "tested": false, "passed": false },
        "act": { "status": status, "tested": false, "passed": false },
        "hud": { "status": status, "tested": false, "passed": false },
    })
}

fn missing_lune_report(searched: &[String]) -> Value {
    let msg = "lune binary is not installed (checked PATH, $HOME/.local/bin/lune, %USERPROFILE%\\.local\\bin\\lune.exe, and OPENRO_LUNE)";
    json!({
        "ok": false,
        "engine": "lune_roblox_shim",
        "passed": false,
        "duration_ms": 0,
        "error": msg,
        "diagnostics": {
            "lune_found": false,
            "searched_paths": searched,
        },
        "steps": [],
        "assertions": [],
        "console": {
            "lines": [format!("[ERROR] {msg}")],
            "errors": [msg],
            "warnings": [],
        },
        "state_snapshot": empty_state_snapshot(),
        "core_loop": default_core_loop("not tested (lune missing)"),
    })
}

fn parse_timeout_secs(req: &Value) -> Result<u64, String> {
    match req.get("timeout_secs") {
        None | Some(Value::Null) => Ok(DEFAULT_TIMEOUT_SECS),
        Some(v) => {
            if let Some(u) = v.as_u64() {
                Ok(u.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS))
            } else if let Some(f) = v.as_f64() {
                if !f.is_finite() || f < 0.0 {
                    return Err("timeout_secs must be a positive number".to_string());
                }
                Ok((f.ceil() as u64).clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS))
            } else {
                Err("timeout_secs must be a positive number".to_string())
            }
        }
    }
}

fn parse_players(req: &Value) -> Result<Cow<'_, Value>, String> {
    match req.get("players") {
        None | Some(Value::Null) => Ok(Cow::Owned(json!([
            { "user_id": 1001, "name": "TestPlayer", "owns_passes": [] }
        ]))),
        Some(v @ Value::Array(arr)) => {
            if arr.is_empty() {
                return Ok(Cow::Owned(json!([
                    { "user_id": 1001, "name": "TestPlayer", "owns_passes": [] }
                ])));
            }
            if arr.len() > MAX_PLAYERS {
                return Err(format!("players exceeds max count of {MAX_PLAYERS}"));
            }
            Ok(Cow::Borrowed(v))
        }
        Some(_) => Err("players must be an array of player objects".into()),
    }
}

fn parse_journey(req: &Value) -> Result<(bool, Cow<'_, Value>), String> {
    match req.get("journey") {
        None | Some(Value::Null) => Ok((true, Cow::Owned(Value::Null))),
        Some(v @ Value::Array(arr)) => {
            if arr.len() > MAX_JOURNEY_STEPS {
                return Err(format!("journey exceeds max {MAX_JOURNEY_STEPS} steps"));
            }
            Ok((false, Cow::Borrowed(v)))
        }
        Some(_) => Err("journey must be an array of step objects".into()),
    }
}

fn ensure_array_field(obj: &mut Map<String, Value>, key: &str) {
    match obj.get(key) {
        Some(Value::Array(_)) => {}
        Some(Value::Object(m)) if m.is_empty() => {
            obj.insert(key.to_string(), Value::Array(Vec::new()));
        }
        None => {
            obj.insert(key.to_string(), Value::Array(Vec::new()));
        }
        _ => {}
    }
}

fn normalize_gui_children(node: &mut Value) {
    if let Some(obj) = node.as_object_mut() {
        ensure_array_field(obj, "children");
        if let Some(Value::Array(children)) = obj.get_mut("children") {
            for ch in children {
                normalize_gui_children(ch);
            }
        }
    }
}

fn normalize_report_json(report: &mut Value) {
    let Some(obj) = report.as_object_mut() else {
        return;
    };
    ensure_array_field(obj, "steps");
    ensure_array_field(obj, "assertions");
    if let Some(console) = obj.get_mut("console").and_then(Value::as_object_mut) {
        ensure_array_field(console, "lines");
        ensure_array_field(console, "errors");
        ensure_array_field(console, "warnings");
    }
    if let Some(snap) = obj.get_mut("state_snapshot").and_then(Value::as_object_mut) {
        ensure_array_field(snap, "receipts_processed");
        ensure_array_field(snap, "player_gui_tree");
        if let Some(Value::Array(trees)) = snap.get_mut("player_gui_tree") {
            for t in trees {
                normalize_gui_children(t);
            }
        }
    }
}

fn run_lune_sandbox(
    project_name: &str,
    tree_json: Value,
    product_ids: Vec<u64>,
    req: &Value,
    lune_override: Option<Option<&Path>>,
) -> Result<Value, String> {
    if !req.is_object() && !req.is_null() {
        return Err("sandbox request body must be a JSON object".to_string());
    }
    let timeout_secs = parse_timeout_secs(req)?;
    let players = parse_players(req)?;
    let (auto_journey, journey) = parse_journey(req)?;

    let (lune_bin, searched) = match lune_override {
        Some(Some(p)) => (
            p.is_file().then(|| p.to_path_buf()),
            vec![p.display().to_string()],
        ),
        Some(None) => (None, vec!["override:none".to_string()]),
        None => find_lune_binary(),
    };
    let Some(bin) = lune_bin else {
        return Ok(missing_lune_report(&searched));
    };

    let seq = RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let scratch_root = std::env::temp_dir().join("openro-sandbox");
    std::fs::create_dir_all(&scratch_root)
        .map_err(|e| format!("create sandbox scratch root: {e}"))?;
    if std::fs::symlink_metadata(&scratch_root)
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("sandbox scratch root must not be a symlink".to_string());
    }
    let scratch_dir = scratch_root.join(format!("run-{}-{seq}", std::process::id()));
    std::fs::create_dir(&scratch_dir)
        .map_err(|e| format!("create sandbox scratch dir: {e}"))?;
    let _guard = ScratchDirGuard(scratch_dir.clone());

    let runtime_path = scratch_dir.join("runtime.luau");
    let input_path = scratch_dir.join("input.json");
    let report_path = scratch_dir.join("report.json");

    std::fs::write(&runtime_path, SANDBOX_RUNTIME_LUAU.as_bytes())
        .map_err(|e| format!("write sandbox runtime: {e}"))?;

    let input_payload = json!({
        "project_name": project_name,
        "tree": tree_json,
        "players": players.as_ref(),
        "auto_journey": auto_journey,
        "journey": journey.as_ref(),
        "product_ids": product_ids,
        "timeout_secs": timeout_secs,
    });
    {
        let input_file = std::fs::File::create(&input_path)
            .map_err(|e| format!("create sandbox input: {e}"))?;
        let mut writer = BufWriter::new(input_file);
        serde_json::to_writer(&mut writer, &input_payload)
            .map_err(|e| format!("write sandbox input: {e}"))?;
        std::io::Write::flush(&mut writer)
            .map_err(|e| format!("flush sandbox input: {e}"))?;
    }

    let started = Instant::now();
    let mut child = Command::new(&bin)
        .arg("run")
        .arg(&runtime_path)
        .arg(&input_path)
        .arg(&report_path)
        .current_dir(&scratch_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start lune ({}): {e}", bin.display()))?;

    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::with_capacity(1024);
        if let Some(ref mut r) = out_pipe {
            let _ = r.take(STDIO_CAP_BYTES).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::with_capacity(1024);
        if let Some(ref mut r) = err_pipe {
            let _ = r.take(STDIO_CAP_BYTES).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let (exit_code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code().unwrap_or(-1), false),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break (-1, true);
            }
            Err(e) => return Err(format!("wait lune: {e}")),
        }
    };

    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    let elapsed_ms = started.elapsed().as_millis() as u64;

    if timed_out {
        let err_msg = format!("Sandbox execution timed out after {timeout_secs}s");
        return Ok(json!({
            "ok": false,
            "engine": "lune_roblox_shim",
            "passed": false,
            "timed_out": true,
            "duration_ms": elapsed_ms,
            "error": err_msg,
            "steps": [],
            "assertions": [],
            "console": {
                "lines": [format!("[ERROR] {err_msg}")],
                "errors": [err_msg],
                "warnings": [],
            },
            "state_snapshot": empty_state_snapshot(),
            "core_loop": default_core_loop("timed out"),
        }));
    }

    if let Ok(meta) = std::fs::symlink_metadata(&report_path) {
        if !meta.file_type().is_symlink() && meta.is_file() && meta.len() <= MAX_REPORT_BYTES {
            let raw = std::fs::read_to_string(&report_path)
                .map_err(|e| format!("read sandbox report: {e}"))?;
            if let Ok(mut report) = serde_json::from_str::<Value>(&raw) {
                normalize_report_json(&mut report);
                if let Some(obj) = report.as_object_mut() {
                    obj.entry("ok").or_insert(json!(true));
                    obj.entry("engine").or_insert(json!("lune_roblox_shim"));
                    obj.insert("duration_ms".to_string(), json!(elapsed_ms));
                    if exit_code != 0 && obj.get("passed").and_then(Value::as_bool) == Some(true) {
                        obj.insert("passed".to_string(), json!(false));
                    }
                }
                return Ok(report);
            }
        }
    }

    let detail = if !stderr.trim().is_empty() {
        stderr.trim().to_string()
    } else if !stdout.trim().is_empty() {
        stdout.trim().to_string()
    } else {
        format!("lune exited with status {exit_code} without producing report.json")
    };
    Ok(json!({
        "ok": false,
        "engine": "lune_roblox_shim",
        "passed": false,
        "exit_code": exit_code,
        "duration_ms": elapsed_ms,
        "error": detail,
        "steps": [],
        "assertions": [],
        "console": {
            "lines": [format!("[ERROR] {detail}")],
            "errors": [detail],
            "warnings": [],
        },
        "state_snapshot": empty_state_snapshot(),
        "core_loop": default_core_loop("crashed"),
    }))
}

/// Run the Headless Roblox Luau Runtime Sandbox against a Rojo project on disk.
pub fn run_project_sandbox(project_dir: &Path, req: &Value) -> Result<Value, String> {
    run_project_sandbox_with_lune(project_dir, req, None)
}

/// Run the Headless Roblox Luau Runtime Sandbox with an optional explicit `lune`
/// binary override (`Some(None)` simulates a missing `lune` binary in tests).
pub fn run_project_sandbox_with_lune(
    project_dir: &Path,
    req: &Value,
    lune_override: Option<Option<&Path>>,
) -> Result<Value, String> {
    let manifest = projects::manifest(project_dir)?;
    let rojo_file = manifest["rojoProject"]
        .as_str()
        .unwrap_or(projects::DEFAULT_ROJO_FILE);
    let tree = projects::load_tree(project_dir, rojo_file)?;
    let product_ids = extract_product_ids(&manifest);
    let services: Vec<Value> = tree.services.iter().map(node_to_json).collect();
    let tree_json = json!({
        "name": tree.name,
        "services": services,
    });
    run_lune_sandbox(&tree.name, tree_json, product_ids, req, lune_override)
}

/// Run the Headless Roblox Luau Runtime Sandbox against an in-memory `LocalPlace`.
pub fn run_local_place_sandbox(
    game_name: &str,
    scripts: &BTreeMap<String, String>,
    instances: &BTreeMap<String, Value>,
    req: &Value,
) -> Result<Value, String> {
    run_local_place_sandbox_with_lune(game_name, scripts, instances, req, None)
}

pub fn run_local_place_sandbox_with_lune(
    game_name: &str,
    scripts: &BTreeMap<String, String>,
    instances: &BTreeMap<String, Value>,
    req: &Value,
    lune_override: Option<Option<&Path>>,
) -> Result<Value, String> {
    let tree_json = materialize_local_place_tree(game_name, scripts, instances);
    run_lune_sandbox(game_name, tree_json, Vec::new(), req, lune_override)
}
