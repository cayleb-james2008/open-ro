//! HTTP face of projects, project files, sync and the playtest runner.
//! `route` returns `None` for paths it does not own, so other route tables
//! (Open Cloud) can answer the rest of `/api/projects/{id}/...`.

use crate::mcp::McpClient;
use crate::place::PlaceBackend;
use crate::projects::{self, ProjectRef};
use crate::server::Shared;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::Ordering;

pub struct Reply {
    pub status: &'static str,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(status: &'static str, v: Value) -> Reply {
        Reply { status, content_type: "application/json", body: v.to_string().into_bytes() }
    }
    pub fn ok(mut v: Value) -> Reply {
        if let Some(o) = v.as_object_mut() {
            o.insert("ok".into(), json!(true));
        }
        Reply::json("200 OK", v)
    }
    pub fn created(mut v: Value) -> Reply {
        if let Some(o) = v.as_object_mut() {
            o.insert("ok".into(), json!(true));
        }
        Reply::json("201 Created", v)
    }
    pub fn err(status: &'static str, msg: impl Into<String>) -> Reply {
        Reply::json(status, json!({ "ok": false, "error": msg.into() }))
    }
    pub fn bad(msg: impl Into<String>) -> Reply {
        Reply::err("400 Bad Request", msg)
    }
}

/// Counts a long request (sync, playtest) as "busy" so a restart waits for it.
struct Busy<'a>(&'a std::sync::atomic::AtomicUsize);
impl<'a> Busy<'a> {
    fn new(c: &'a std::sync::atomic::AtomicUsize) -> Busy<'a> {
        c.fetch_add(1, Ordering::SeqCst);
        Busy(c)
    }
}
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').filter_map(|p| p.split_once('=')).find(|(k, _)| *k == key).map(|(_, v)| pct_decode(v))
}

fn parse_body(body: &[u8]) -> Result<Value, Reply> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(body).map_err(|_| Reply::bad("the request body is not valid JSON"))
}

/// Run `f` against the connected Studio MCP bridge.
pub fn with_studio<T>(shared: &Shared, f: impl FnOnce(&mut McpClient) -> Result<T, String>) -> Result<T, Reply> {
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        return Err(Reply::err("503 Service Unavailable", "Studio not connected"));
    };
    match agent.studio.as_mut() {
        Some(PlaceBackend::Mcp(m)) => f(m).map_err(|e| Reply::err("502 Bad Gateway", e)),
        Some(_) => Err(Reply::err(
            "503 Service Unavailable",
            "Sync and playtests need the Roblox Studio MCP bridge; the connected backend does not offer it",
        )),
        None => Err(Reply::err("503 Service Unavailable", "Studio not connected")),
    }
}

fn project_of(shared: &Shared, id: &str) -> Result<ProjectRef, Reply> {
    shared.projects.lock().unwrap().find(id).ok_or_else(|| Reply::err("404 Not Found", format!("no project '{id}'")))
}

/// `Some(reply)` when this table owns the request.
pub fn route(shared: &Shared, method: &str, full_path: &str, body: &[u8], loopback: bool) -> Option<Reply> {
    let (path, query) = full_path.split_once('?').unwrap_or((full_path, ""));
    let rest = path.strip_prefix("/api/projects")?;
    if !rest.is_empty() && !rest.starts_with('/') {
        return None;
    }
    let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    // Which routes does this table own at all? Anything else falls through.
    let owned = matches!(
        (method, segs.as_slice()),
        ("GET", [])
            | ("POST", ["workspace"] | ["workspace", "remove"] | ["create"] | ["open"])
            | ("GET", [_])
            | ("POST", [_, "manifest" | "build" | "sync" | "playtest" | "check" | "run-lune"])
            | ("POST", [_, "sync", "diff"])
            | ("GET", [_, "builds" | "playtests" | "files" | "file" | "preview"])
            | ("POST", [_, "file"] | [_, "file", "delete"] | [_, "file", "edit"])
            | ("GET", [_, "git", "status" | "log"])
            | ("POST", [_, "git", "commit" | "revert"])
            | (
                "GET",
                [_, "playtests", _]
                    | [_, "playtests", _, "report.html"]
                    | [_, "playtests", _, "shots", _],
            )
    );
    if !owned {
        return None;
    }
    if !loopback {
        return Some(Reply::err("403 Forbidden", "projects are only available from this machine (loopback)"));
    }
    Some(dispatch(shared, method, &segs, query, body).unwrap_or_else(|r| r))
}

fn dispatch(shared: &Shared, method: &str, segs: &[&str], query: &str, body: &[u8]) -> Result<Reply, Reply> {
    match (method, segs) {
        ("GET", []) => {
            let store = shared.projects.lock().unwrap();
            let list = store.list();
            Ok(Reply::ok(json!({
                "roots": store.all_roots().iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
                "active": store.active,
                "projects": list.iter().map(projects::summary).collect::<Vec<_>>(),
            })))
        }
        ("POST", ["workspace"]) => {
            let b = parse_body(body)?;
            let mut store = shared.projects.lock().unwrap();
            store.add_root(b["path"].as_str().unwrap_or("")).map_err(Reply::bad)?;
            Ok(Reply::ok(json!({ "roots": store.all_roots().iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>() })))
        }
        ("POST", ["workspace", "remove"]) => {
            let b = parse_body(body)?;
            let mut store = shared.projects.lock().unwrap();
            store.remove_root(b["path"].as_str().unwrap_or(""));
            Ok(Reply::ok(json!({ "roots": store.all_roots().iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>() })))
        }
        ("POST", ["create"]) => {
            let b = parse_body(body)?;
            let universe = projects::body_id(&b, "universeId").map_err(Reply::bad)?;
            let place = projects::body_id(&b, "placeId").map_err(Reply::bad)?;
            let r = shared
                .projects
                .lock()
                .unwrap()
                .create(b["name"].as_str().unwrap_or(""), b["parent"].as_str(), universe, place)
                .map_err(Reply::bad)?;
            Ok(Reply::created(json!({ "project": projects::summary(&r) })))
        }
        ("POST", ["open"]) => {
            let b = parse_body(body)?;
            let r = shared.projects.lock().unwrap().open(b["path"].as_str().unwrap_or("")).map_err(Reply::bad)?;
            Ok(Reply::ok(json!({ "project": projects::summary(&r) })))
        }
        ("GET", [id]) => {
            let r = project_of(shared, id)?;
            shared.projects.lock().unwrap().set_active(&r.id);
            Ok(Reply::ok(json!({ "project": detail(&r) })))
        }
        ("POST", [id, "manifest"]) => {
            let r = project_of(shared, id)?;
            let patch = parse_body(body)?;
            let current = projects::manifest(&r.dir).map_err(Reply::bad)?;
            let allow_price_approval = patch.get("actor").and_then(|v| v.as_str()) != Some("agent");
            let merged = projects::merge_manifest_for_actor(&current, &patch, allow_price_approval)
                .map_err(Reply::bad)?;
            projects::write_manifest(&r.dir, &merged).map_err(|e| Reply::err("500 Internal Server Error", e))?;
            Ok(Reply::ok(json!({ "manifest": merged })))
        }
        ("POST", [id, "check"]) => {
            let r = project_of(shared, id)?;
            let _busy = Busy::new(&shared.busy);
            let res = projects::check_project(&r.dir).map_err(Reply::bad)?;
            Ok(Reply::ok(res))
        }
        ("POST", [id, "run-lune"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let _busy = Busy::new(&shared.busy);
            let script = b["script"].as_str().or_else(|| b["path"].as_str()).unwrap_or("");
            let args: Vec<String> = b["args"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let timeout = b["timeout_secs"].as_u64().unwrap_or(30);
            let res = projects::run_lune(&r.dir, script, &args, timeout).map_err(Reply::bad)?;
            Ok(Reply::ok(res))
        }
        ("POST", [id, "build"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let _busy = Busy::new(&shared.busy);
            let rec = projects::build(&r.dir, b["engine"].as_str().unwrap_or("auto"), b["out"].as_str()).map_err(Reply::bad)?;
            Ok(Reply::ok(rec))
        }
        ("GET", [id, "builds"]) => {
            let r = project_of(shared, id)?;
            Ok(Reply::ok(json!({ "builds": projects::builds(&r.dir) })))
        }
        ("GET", [id, "files"]) => {
            let r = project_of(shared, id)?;
            list_files(&r.dir, &crate::project_api::query_param(query, "dir").unwrap_or_default())
        }
        ("GET", [id, "file"]) => {
            let r = project_of(shared, id)?;
            read_file(&r.dir, &query_param(query, "path").unwrap_or_default())
        }
        ("POST", [id, "file"]) => {
            let r = project_of(shared, id)?;
            write_file(&r.dir, &parse_body(body)?)
        }
        ("POST", [id, "file", "delete"]) => {
            let r = project_of(shared, id)?;
            delete_file(&r.dir, &parse_body(body)?)
        }
        ("POST", [id, "file", "edit"]) => {
            let r = project_of(shared, id)?;
            edit_file(&r.dir, &parse_body(body)?)
        }
        ("GET", [id, "preview"]) => {
            let r = project_of(shared, id)?;
            preview_image(&r.dir, &query_param(query, "path").unwrap_or_default())
        }
        ("GET", [id, "git", "status"]) => {
            let r = project_of(shared, id)?;
            Ok(Reply::ok(projects::git_status(&r.dir)))
        }
        ("GET", [id, "git", "log"]) => {
            let r = project_of(shared, id)?;
            let limit = query_param(query, "limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(20);
            let commits = projects::git_log(&r.dir, limit).map_err(Reply::bad)?;
            Ok(Reply::ok(json!({ "commits": commits })))
        }
        ("POST", [id, "git", "commit"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let message = b["message"].as_str().unwrap_or("");
            let files: Option<Vec<String>> = b
                .get("files")
                .or_else(|| b.get("paths"))
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect());
            let sha = match files {
                Some(list) => projects::git_commit(&r.dir, &list, message).map_err(Reply::bad)?,
                None => projects::git_commit_all(&r.dir, message).map_err(Reply::bad)?,
            };
            match sha {
                Some(full) => {
                    let short: String = full.chars().take(7).collect();
                    Ok(Reply::ok(json!({ "committed": true, "commit": full, "short": short, "message": message.trim() })))
                }
                None => Ok(Reply::ok(json!({ "committed": false, "commit": null, "note": "nothing to commit" }))),
            }
        }
        ("POST", [id, "git", "revert"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let target = b["commit"].as_str().unwrap_or("");
            match projects::git_revert(&r.dir, target) {
                Ok(new_sha) => {
                    let short: String = new_sha.chars().take(7).collect();
                    Ok(Reply::ok(json!({ "reverted": true, "original": target, "commit": new_sha, "short": short })))
                }
                Err(e) if e.contains("aborted") => Err(Reply::err("409 Conflict", e)),
                Err(e) => Err(Reply::bad(e)),
            }
        }
        ("POST", [id, "sync", "diff"]) => {
            let r = project_of(shared, id)?;
            let _busy = Busy::new(&shared.busy);
            crate::studio_sync::run(shared, &r, false, true)
        }
        ("POST", [id, "sync"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let _busy = Busy::new(&shared.busy);
            let dry = b["dry_run"].as_bool().unwrap_or(false);
            crate::studio_sync::run(shared, &r, b["prune"].as_bool().unwrap_or(false), dry)
        }
        ("POST", [id, "playtest"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let _busy = Busy::new(&shared.busy);
            crate::playtest::run(shared, &r, &b)
        }
        ("GET", [id, "playtests"]) => {
            let r = project_of(shared, id)?;
            Ok(Reply::ok(json!({ "runs": crate::playtest::list_runs(&r.dir) })))
        }
        ("GET", [id, "playtests", run]) => {
            let r = project_of(shared, id)?;
            crate::playtest::stored(&r.dir, run, "report.json", "application/json")
        }
        ("GET", [id, "playtests", run, "report.html"]) => {
            let r = project_of(shared, id)?;
            crate::playtest::stored(&r.dir, run, "report.html", "text/html; charset=utf-8")
        }
        ("GET", [id, "playtests", run, "shots", file]) => {
            let r = project_of(shared, id)?;
            crate::playtest::stored(&r.dir, run, &format!("shots/{file}"), if file.ends_with(".jpg") { "image/jpeg" } else { "image/png" })
        }
        _ => Err(Reply::err("404 Not Found", "not found")),
    }
}

fn detail(r: &ProjectRef) -> Value {
    let mut v = projects::summary(r);
    let manifest = projects::manifest(&r.dir).unwrap_or_else(|_| json!({}));
    let rojo = manifest["rojoProject"].as_str().unwrap_or(projects::DEFAULT_ROJO_FILE).to_string();
    let (tree, skipped, tree_error) = match projects::load_tree(&r.dir, &rojo) {
        Ok(t) => {
            let (instances, scripts) = t.counts();
            (json!({ "instances": instances, "scripts": scripts, "services": t.services.iter().map(|s| s.name.clone()).collect::<Vec<_>>() }), t.skipped, Value::Null)
        }
        Err(e) => (Value::Null, Vec::new(), json!(e)),
    };
    let lock_file = manifest["assets"]["lockFile"].as_str().unwrap_or("assets.lock.json");
    let uploaded = projects::safe_join(&r.dir, lock_file)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|l| l["assets"].as_object().map(|a| a.len()))
        .unwrap_or(0);
    let patterns: Vec<String> = manifest["assets"]["authorised"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let (total_assets, authorised_assets) = count_project_assets(&r.dir, &patterns);
    let o = v.as_object_mut().unwrap();
    o.insert("manifest".into(), manifest.clone());
    o.insert("tree".into(), tree);
    o.insert("tree_error".into(), tree_error);
    o.insert("skipped".into(), json!(skipped));
    o.insert("assets".into(), json!({
        "total": total_assets,
        "authorised": authorised_assets,
        "authorised_patterns": patterns,
        "uploaded": uploaded,
    }));
    o.insert("products".into(), manifest["products"].clone());
    v
}

fn is_uploadable_ext(ext: &str) -> bool {
    matches!(
        ext,
        "png" | "jpg" | "jpeg" | "bmp" | "tga" | "mp3" | "ogg" | "wav" | "flac" | "fbx" | "gltf" | "glb" | "rbxm" | "rbxmx" | "mp4" | "mov"
    )
}

fn count_project_assets(root: &Path, patterns: &[String]) -> (usize, usize) {
    fn walk(root: &Path, dir: &Path, patterns: &[String], total: &mut usize, auth: &mut usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for entry in rd.filter_map(|e| e.ok()) {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "build" || name == "node_modules" || name == "refs" {
                continue;
            }
            let rel = projects::rel_display(root, &p);
            if rel == "art/references" || rel.starts_with("art/references/") {
                continue;
            }
            if p.is_dir() {
                walk(root, &p, patterns, total, auth);
            } else if let Some(ext) = p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()) {
                if is_uploadable_ext(&ext) {
                    *total += 1;
                    if patterns.iter().any(|pat| projects::glob_match(pat, &rel)) {
                        *auth += 1;
                    }
                }
            }
        }
    }
    let (mut total, mut auth) = (0, 0);
    walk(root, root, patterns, &mut total, &mut auth);
    (total, auth)
}

// ------------------------------------------------------------ project files

const FILE_CAP: usize = 1_048_576;
const PREVIEW_CAP: usize = 10 * 1024 * 1024;

/// May this relative path be read or written through the file API?
fn file_path(dir: &Path, rel: &str, writing: bool) -> Result<std::path::PathBuf, Reply> {
    let full = projects::safe_join(dir, rel).map_err(Reply::bad)?;
    let shown = projects::rel_display(dir, &full);
    if shown.is_empty() || shown == "." {
        return Err(Reply::bad(if writing {
            "cannot modify or delete the project root folder"
        } else {
            "path refers to the project root folder; list it with /files"
        }));
    }
    if let (Ok(canon_dir), Ok(canon_full)) = (dir.canonicalize(), full.canonicalize()) {
        if canon_full == canon_dir {
            return Err(Reply::bad("cannot modify or delete the project root folder"));
        }
    }
    let top = shown.split('/').next().unwrap_or("");
    if top == ".git" || shown.split('/').any(|s| s == ".git") {
        return Err(Reply::bad("the .git folder is off limits"));
    }
    if top == ".openro" || shown.split('/').any(|s| s == ".openro") {
        return Err(Reply::bad("the .openro folder is internal to OpenRO"));
    }
    if writing {
        if top == "build" {
            return Err(Reply::bad("build/ is OpenRO's output folder; it is not written through the file API"));
        }
        if shown == projects::MANIFEST_FILE {
            return Err(Reply::bad("change the manifest with POST /api/projects/{id}/manifest"));
        }
        if shown.ends_with(".lock.json") {
            return Err(Reply::bad("lock files are written by OpenRO's upload, not by hand"));
        }
    }
    Ok(full)
}

fn preview_image(dir: &Path, rel: &str) -> Result<Reply, Reply> {
    let full = file_path(dir, rel, false)?;
    let ext = full
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let content_type = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return Err(Reply::bad("preview supports png, jpg, webp, or gif images only")),
    };
    let meta = std::fs::metadata(&full).map_err(|_| Reply::err("404 Not Found", "that image does not exist"))?;
    if !meta.is_file() {
        return Err(Reply::bad("that path is not a file"));
    }
    if meta.len() as usize > PREVIEW_CAP {
        return Err(Reply::bad("image is over 10 MB"));
    }
    let body = std::fs::read(&full).map_err(|e| Reply::bad(e.to_string()))?;
    Ok(Reply { status: "200 OK", content_type, body })
}

fn list_files(dir: &Path, rel: &str) -> Result<Reply, Reply> {
    let base = if rel.trim().is_empty() || rel == "." { dir.to_path_buf() } else { file_path(dir, rel, false)? };
    if !base.is_dir() {
        return Err(Reply::err("404 Not Found", "that folder does not exist"));
    }
    let mut entries: Vec<_> = std::fs::read_dir(&base).map_err(|e| Reply::bad(e.to_string()))?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    let items: Vec<Value> = entries
        .iter()
        .filter(|e| !matches!(e.file_name().to_string_lossy().as_ref(), ".git" | ".openro"))
        .map(|e| {
            let p = e.path();
            let meta = e.metadata().ok();
            json!({
                "path": projects::rel_display(dir, &p),
                "type": if p.is_dir() { "dir" } else { "file" },
                "bytes": meta.map(|m| m.len()).unwrap_or(0),
            })
        })
        .collect();
    Ok(Reply::ok(json!({ "dir": projects::rel_display(dir, &base), "entries": items })))
}

fn read_file(dir: &Path, rel: &str) -> Result<Reply, Reply> {
    let full = file_path(dir, rel, false)?;
    let meta = std::fs::metadata(&full).map_err(|_| Reply::err("404 Not Found", "that file does not exist"))?;
    if !meta.is_file() {
        return Err(Reply::bad("that is a folder; list it with /files"));
    }
    if meta.len() as usize > FILE_CAP {
        return Err(Reply::bad("that file is over 1 MB; the file API only handles text up to that size"));
    }
    let bytes = std::fs::read(&full).map_err(|e| Reply::bad(e.to_string()))?;
    let text = String::from_utf8(bytes.clone()).map_err(|_| Reply::bad("that file is not text"))?;
    Ok(Reply::ok(json!({ "path": projects::rel_display(dir, &full), "content": text, "bytes": bytes.len(), "sha256": projects::sha256_hex(&bytes) })))
}

fn check_expect(full: &Path, body: &Value) -> Result<(), Reply> {
    if let Some(want) = body["expect_sha256"].as_str() {
        let have = std::fs::read(full).map(|b| projects::sha256_hex(&b)).unwrap_or_default();
        if have != want {
            return Err(Reply::err("409 Conflict", "the file changed since you read it; read it again before editing"));
        }
    }
    Ok(())
}

fn write_atomic(full: &Path, content: &[u8]) -> Result<(), Reply> {
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Reply::bad(format!("create folder: {e}")))?;
    }
    let tmp = full.with_extension("openro-tmp");
    std::fs::write(&tmp, content).map_err(|e| Reply::bad(format!("write file: {e}")))?;
    std::fs::rename(&tmp, full).map_err(|e| Reply::bad(format!("write file: {e}")))
}

fn write_file(dir: &Path, body: &Value) -> Result<Reply, Reply> {
    let full = file_path(dir, body["path"].as_str().unwrap_or(""), true)?;
    let content = body["content"].as_str().ok_or_else(|| Reply::bad("send the file text as content"))?;
    if content.len() > FILE_CAP {
        return Err(Reply::bad("content is over 1 MB"));
    }
    if full.is_dir() {
        return Err(Reply::bad("that path is a folder"));
    }
    check_expect(&full, body)?;
    let created = !full.exists();
    write_atomic(&full, content.as_bytes())?;
    Ok(Reply::ok(json!({ "path": projects::rel_display(dir, &full), "created": created, "bytes": content.len(), "sha256": projects::sha256_hex(content.as_bytes()) })))
}

fn edit_file(dir: &Path, body: &Value) -> Result<Reply, Reply> {
    let full = file_path(dir, body["path"].as_str().unwrap_or(""), true)?;
    let edits = body["edits"].as_array().filter(|e| !e.is_empty()).ok_or_else(|| Reply::bad("send edits: [{ old, new }]"))?;
    let bytes = std::fs::read(&full).map_err(|_| Reply::err("404 Not Found", "that file does not exist"))?;
    check_expect(&full, body)?;
    let mut text = String::from_utf8(bytes).map_err(|_| Reply::bad("that file is not text"))?;
    for (i, e) in edits.iter().enumerate() {
        let old = e["old"].as_str().filter(|o| !o.is_empty()).ok_or_else(|| Reply::bad(format!("edit {}: old must be non-empty text", i + 1)))?;
        let new = e["new"].as_str().ok_or_else(|| Reply::bad(format!("edit {}: new must be text", i + 1)))?;
        match text.matches(old).count() {
            1 => text = text.replacen(old, new, 1),
            0 => return Err(Reply::err("409 Conflict", format!("edit {}: the text to replace was not found; nothing was changed", i + 1))),
            n => return Err(Reply::err("409 Conflict", format!("edit {}: the text to replace appears {n} times; include more around it. Nothing was changed", i + 1))),
        }
    }
    if text.len() > FILE_CAP {
        return Err(Reply::bad("the edited file would be over 1 MB"));
    }
    write_atomic(&full, text.as_bytes())?;
    Ok(Reply::ok(json!({ "path": projects::rel_display(dir, &full), "edits": edits.len(), "bytes": text.len(), "sha256": projects::sha256_hex(text.as_bytes()) })))
}

fn delete_file(dir: &Path, body: &Value) -> Result<Reply, Reply> {
    let full = file_path(dir, body["path"].as_str().unwrap_or(""), true)?;
    let shown = projects::rel_display(dir, &full);
    if shown.is_empty() || shown == "." {
        return Err(Reply::bad("cannot delete the project root folder"));
    }
    if let (Ok(canon_dir), Ok(canon_full)) = (dir.canonicalize(), full.canonicalize()) {
        if canon_full == canon_dir || !canon_full.starts_with(&canon_dir) {
            return Err(Reply::bad("cannot delete the project root folder"));
        }
    }
    if !full.exists() {
        return Err(Reply::err("404 Not Found", "that file does not exist"));
    }
    if full.is_dir() {
        if full.join(".git").exists()
            || full.join(".openro").exists()
            || full.join(projects::MANIFEST_FILE).exists()
        {
            return Err(Reply::bad("cannot delete a folder containing protected project metadata"));
        }
        std::fs::remove_dir_all(&full).map_err(|e| Reply::bad(e.to_string()))?;
    } else {
        std::fs::remove_file(&full).map_err(|e| Reply::bad(e.to_string()))?;
    }
    Ok(Reply::ok(json!({ "path": shown, "deleted": true })))
}
