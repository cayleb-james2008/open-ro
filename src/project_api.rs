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
use std::sync::Arc;

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

fn parse_format_paths(b: &Value) -> Result<Option<Vec<String>>, Reply> {
    match b.get("paths") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                let s = item
                    .as_str()
                    .ok_or_else(|| Reply::bad("paths must be an array of strings"))?;
                out.push(s.to_string());
            }
            Ok(Some(out))
        }
        Some(_) => Err(Reply::bad("paths must be an array of strings")),
    }
}

/// Run `f` against the connected Studio MCP bridge.
pub fn with_studio<T>(shared: &Shared, f: impl FnOnce(&mut McpClient) -> Result<T, String>) -> Result<T, Reply> {
    let mut guard = shared.agent.lock().unwrap();
    let Some(agent) = guard.as_mut() else {
        return Err(Reply::err("503 Service Unavailable", "Studio not connected"));
    };
    match agent.studio.as_mut() {
        Some(PlaceBackend::Mcp(m)) => f(m).map_err(|e| {
            let msg = if e.to_ascii_lowercase().contains("place is not open") {
                format!(
                    "{e} — Studio is on the Start Page; open a blank Baseplate or call POST /api/projects/{{id}}/open-in-studio first"
                )
            } else {
                e
            };
            Reply::err("502 Bad Gateway", msg)
        }),
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

/// Auto-open / transfer a project's `.rbxlx` into Embedded Studio before `sync` / `playtest`
/// when `auto_open` is `true` (default) and Studio is installed & signed in without an open place (#51, #57).
pub(crate) fn ensure_studio_place_open_for_dir(
    shared: &Arc<Shared>,
    project_id: &str,
    project_dir: &Path,
    b: &Value,
) {
    let auto_open = b.get("auto_open").and_then(|v| v.as_bool()).unwrap_or(true);
    if !auto_open {
        return;
    }
    if std::env::var("OPENRO_NO_AUTO_OPEN_STUDIO")
        .ok()
        .is_some_and(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
    {
        return;
    }
    let (has_mcp, mcp_open) = {
        let mut guard = shared.agent.lock().unwrap();
        match guard.as_mut().and_then(|a| a.studio.as_mut()) {
            Some(PlaceBackend::Mcp(m)) => {
                let open = m.has_live_studio() && crate::studio_embed::probe_client_place_open(m);
                (true, open)
            }
            _ => (false, false),
        }
    };
    if mcp_open {
        return;
    }
    // Deterministic test hook (`OPENRO_AUTO_OPEN_MCP_CMD`) for integration tests (never spawns Vinegar).
    if let Ok(test_cmd) = std::env::var("OPENRO_AUTO_OPEN_MCP_CMD") {
        let trimmed = test_cmd.trim();
        if !trimmed.is_empty() {
            let _ = crate::server::perform_studio_embed_transfer(
                shared,
                crate::studio_embed::TransferOptions {
                    mcp_command: Some(trimmed.to_string()),
                    ..Default::default()
                },
            );
            return;
        }
    }
    // Respect explicit local_place workspace mode or custom mock MCP bridges.
    if shared
        .config
        .lock()
        .unwrap()
        .workspace_mode
        .as_deref()
        == Some("local_place")
    {
        return;
    }
    if has_mcp {
        let is_custom = shared.studio_embed.try_lock().ok().is_some_and(|g| {
            let st = g.state();
            st.mcp_command.as_deref().is_some_and(|cmd| {
                let default_mcp = crate::studio_embed::detect_host_mcp_command();
                default_mcp.as_deref() != Some(cmd)
            })
        });
        if is_custom {
            return;
        }
    }
    let det = crate::studio_embed::detect_studio_environment();
    if !det.studio_installed || (!det.already_logged_in && !det.studio_running) {
        return;
    }
    let rbxlx = projects::builds(project_dir)
        .into_iter()
        .find_map(|rec| {
            let rel = rec.get("path")?.as_str()?;
            let full = projects::safe_join(project_dir, rel).ok()?;
            full.is_file().then_some(full)
        })
        .or_else(|| {
            projects::build(project_dir, "auto", None).ok().and_then(|rec| {
                let rel = rec.get("path")?.as_str()?;
                projects::safe_join(project_dir, rel).ok()
            })
        })
        .or_else(|| {
            crate::studio_embed::ensure_blank_baseplate_rbxlx(&project_dir.join("build"), project_id)
                .ok()
        });
    let _ = crate::server::perform_studio_embed_transfer(
        shared,
        crate::studio_embed::TransferOptions {
            place_file: rbxlx,
            force_transfer: false,
            wait_secs: 45,
            ..Default::default()
        },
    );
}

pub(crate) fn ensure_studio_place_open(shared: &Arc<Shared>, r: &ProjectRef, b: &Value) {
    ensure_studio_place_open_for_dir(shared, &r.id, &r.dir, b);
}

/// `Some(reply)` when this table owns the request.
pub fn route(shared: &Arc<Shared>, method: &str, full_path: &str, body: &[u8], loopback: bool) -> Option<Reply> {
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
            | ("POST", [_, "manifest" | "build" | "sync" | "playtest" | "check" | "format" | "run-lune" | "open-in-studio"])
            | ("POST", [_, "sync", "diff"] | [_, "playtests", "rerun"] | [_, "playtests", "fix-plan"] | [_, "playtests", "fix-apply"] | [_, "playtests", "fix-all"])
            | ("GET", [_, "builds" | "playtests" | "dependencies" | "files" | "file" | "preview"])
            | ("POST", [_, "dependencies", "prune"])
            | ("POST", [_, "file"] | [_, "file", "delete"] | [_, "file", "edit"])
            | ("GET", [_, "git", "status" | "log"])
            | ("POST", [_, "git", "commit" | "revert"])
            | ("GET", [_, "investigation", "evidence" | "unknowns"])
            | ("POST", [_, "investigation", "evidence" | "unknowns"])
            | ("PATCH", [_, "investigation", "unknowns"] | [_, "investigation", "unknowns", _])
            | ("POST", [_, "study", "check-mechanics"])
            | ("GET", [_, "study", "status"])
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

fn dispatch(shared: &Arc<Shared>, method: &str, segs: &[&str], query: &str, body: &[u8]) -> Result<Reply, Reply> {
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
            let b = parse_body(body)?;
            let format = match b.get("format") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(v)) => *v,
                Some(_) => return Err(Reply::bad("format must be a boolean")),
            };
            let force = match b.get("force") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(v)) => *v,
                Some(_) => return Err(Reply::bad("force must be a boolean")),
            };
            let paths = parse_format_paths(&b)?;
            let _busy = Busy::new(&shared.busy);
            let res = projects::check_project_with_full_options(
                &r.dir,
                format,
                paths.as_deref(),
                force,
            )
            .map_err(Reply::bad)?;
            Ok(Reply::ok(res))
        }
        ("POST", [id, "format"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let paths = parse_format_paths(&b)?;
            let _busy = Busy::new(&shared.busy);
            let res = projects::format_project(&r.dir, paths.as_deref()).map_err(Reply::bad)?;
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
        ("POST", [id, "open-in-studio"]) => {
            let r = project_of(shared, id)?;
            shared.projects.lock().unwrap().set_active(&r.id);
            let b = parse_body(body)?;
            let _busy = Busy::new(&shared.busy);
            let rebuild = b.get("rebuild").and_then(|v| v.as_bool()).unwrap_or(true);
            let dry_run = b.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
            let force = b.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
            let transfer_opt = b.get("transfer").and_then(|v| v.as_bool());
            let engine = b["engine"].as_str().unwrap_or("auto");
            let wait_secs = b
                .get("wait_secs")
                .and_then(|v| v.as_u64())
                .unwrap_or(45)
                .clamp(1, 90);
            let display_opt = b
                .get("display")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());

            // Reuse latest built .rbxlx when rebuild=false and it exists; otherwise build fresh
            // (synthesizing a clean blank Baseplate .rbxlx if no project build can be loaded).
            let existing = if !rebuild {
                projects::builds(&r.dir).into_iter().find_map(|rec| {
                    let rel = rec.get("path")?.as_str()?;
                    let full = projects::safe_join(&r.dir, rel).ok()?;
                    full.is_file().then_some((rec, full))
                })
            } else {
                None
            };
            let (build_rec, rbxlx_full, rebuilt) = match existing {
                Some((rec, full)) => (rec, full, false),
                None => match projects::build(&r.dir, engine, b["out"].as_str()) {
                    Ok(rec) => {
                        let rel = rec["path"].as_str().unwrap_or("");
                        let full = projects::safe_join(&r.dir, rel).map_err(Reply::bad)?;
                        (rec, full, true)
                    }
                    Err(_) => {
                        let bp = crate::studio_embed::ensure_blank_baseplate_rbxlx(
                            &r.dir.join("build"),
                            &r.id,
                        )
                        .map_err(Reply::bad)?;
                        let rel = bp
                            .strip_prefix(&r.dir)
                            .map(|p| p.to_string_lossy().replace('\\', "/"))
                            .unwrap_or_else(|_| bp.to_string_lossy().into_owned());
                        (json!({ "build_id": Value::Null, "path": rel }), bp, true)
                    }
                },
            };

            let has_test_launcher = std::env::var("OPENRO_OPEN_STUDIO_CMD")
                .ok()
                .is_some_and(|v| !v.trim().is_empty());
            if dry_run || has_test_launcher {
                let launch = crate::studio_embed::open_place_in_studio_with_options(
                    &rbxlx_full,
                    display_opt,
                    dry_run,
                    force || transfer_opt == Some(true),
                )
                .map_err(|e| Reply::err("502 Bad Gateway", e))?;
                crate::server::invalidate_embed_detect_cache(shared);
                return Ok(Reply::ok(json!({
                    "project_id": r.id,
                    "build_id": build_rec.get("build_id").cloned().unwrap_or(Value::Null),
                    "rebuilt": rebuilt,
                    "rbxlx_path": rbxlx_full.to_string_lossy(),
                    "rbxlx_rel": build_rec.get("path").cloned().unwrap_or(Value::Null),
                    "staged_path": launch["staged_path"],
                    "windows_path": launch["windows_path"],
                    "already_running": launch["already_running"],
                    "place_open": launch["place_open"],
                    "transferred": false,
                    "launched": launch["launched"],
                    "dry_run": launch["dry_run"],
                    "pid": launch["pid"],
                    "display": launch["display"],
                    "platform": launch["platform"],
                    "command": launch["command"],
                    "message": launch["message"],
                })));
            }

            // Live seamless zero-manual-step open/transfer into Embedded Studio (#51):
            // auto-transfers Start Page -> :99, launches cold -> :99, or attaches open desktop place
            // unless `transfer: true` is passed.
            let display = match display_opt {
                Some(d) => crate::studio_embed::validate_x11_display(d).map_err(Reply::bad)?,
                None => crate::studio_embed::DEFAULT_EMBED_DISPLAY.to_string(),
            };
            let opts = crate::studio_embed::TransferOptions {
                display,
                width: crate::studio_embed::DEFAULT_EMBED_WIDTH,
                height: crate::studio_embed::DEFAULT_EMBED_HEIGHT,
                place_file: Some(rbxlx_full.clone()),
                force_transfer: transfer_opt.unwrap_or(force),
                wait_secs,
                mcp_command: b
                    .get("mcp_command")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from),
            };
            let mut payload = crate::server::perform_studio_embed_transfer(shared, opts)
                .map_err(|(st, msg)| Reply::err(st, msg))?;
            if let Some(map) = payload.as_object_mut() {
                map.insert("project_id".to_string(), json!(r.id));
                map.insert(
                    "build_id".to_string(),
                    build_rec.get("build_id").cloned().unwrap_or(Value::Null),
                );
                map.insert("rebuilt".to_string(), json!(rebuilt));
                map.insert("rbxlx_path".to_string(), json!(rbxlx_full.to_string_lossy()));
                map.insert(
                    "rbxlx_rel".to_string(),
                    build_rec.get("path").cloned().unwrap_or(Value::Null),
                );
            }
            Ok(Reply::ok(payload))
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
            let b = parse_body(body)?;
            let _busy = Busy::new(&shared.busy);
            ensure_studio_place_open(shared, &r, &b);
            crate::studio_sync::run(shared, &r, false, true)
        }
        ("POST", [id, "sync"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let _busy = Busy::new(&shared.busy);
            ensure_studio_place_open(shared, &r, &b);
            let dry = b["dry_run"].as_bool().unwrap_or(false);
            crate::studio_sync::run(shared, &r, b["prune"].as_bool().unwrap_or(false), dry)
        }
        ("POST", [id, "playtest"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let _busy = Busy::new(&shared.busy);
            if let Some(kept) = crate::playtest::try_kept(&r, &b).map_err(Reply::bad)? {
                return Ok(kept);
            }
            ensure_studio_place_open(shared, &r, &b);
            crate::playtest::run(shared, &r, &b)
        }
        ("POST", [id, "playtests", "rerun"]) => {
            let r = project_of(shared, id)?;
            let b = if body.is_empty() { json!({}) } else { parse_body(body)? };
            let _busy = Busy::new(&shared.busy);
            crate::playtest::rerun_assertions(shared, &r, &b)
        }
        ("POST", [id, "playtests", "fix-plan"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let aid = b["assertion_id"].as_str().unwrap_or("");
            Ok(Reply::ok(crate::playtest::draft_assertion_fix_plan(&r, aid)?))
        }
        ("POST", [id, "playtests", "fix-apply"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let aid = b["assertion_id"].as_str().unwrap_or("");
            let _busy = Busy::new(&shared.busy);
            crate::playtest::apply_assertion_fix_and_rerun(shared, &r, aid, &b)
        }
        ("POST", [id, "playtests", "fix-all"]) => {
            let r = project_of(shared, id)?;
            let b = if body.is_empty() { json!({}) } else { parse_body(body)? };
            let _busy = Busy::new(&shared.busy);
            crate::playtest::fix_all_and_prepare_export(shared, &r, &b)
        }
        ("GET", [id, "investigation", "evidence"]) => {
            let r = project_of(shared, id)?;
            let filter = crate::study_check::EvidenceFilter {
                confidence: query_param(query, "confidence").filter(|s| !s.trim().is_empty()),
                kind: query_param(query, "kind").filter(|s| !s.trim().is_empty()),
                source: query_param(query, "source").filter(|s| !s.trim().is_empty()),
                limit: query_param(query, "limit").and_then(|s| s.parse::<usize>().ok()),
            };
            let res = crate::study_check::list_evidence(&r.dir, &filter)
                .map_err(|e| Reply::json(e.http_status(), e.to_json()))?;
            Ok(Reply::ok(res))
        }
        ("POST", [id, "investigation", "evidence"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let res = crate::study_check::record_evidence(&r.dir, &b)
                .map_err(|e| Reply::json(e.http_status(), e.to_json()))?;
            if res["deduplicated"].as_bool() == Some(true) {
                Ok(Reply::ok(res))
            } else {
                Ok(Reply::created(res))
            }
        }
        ("GET", [id, "investigation", "unknowns"]) => {
            let r = project_of(shared, id)?;
            let status = query_param(query, "status").filter(|s| !s.trim().is_empty());
            let severity = query_param(query, "severity").filter(|s| !s.trim().is_empty());
            let domain = query_param(query, "domain").filter(|s| !s.trim().is_empty());
            let res = crate::study_check::list_unknowns(
                &r.dir,
                status.as_deref(),
                severity.as_deref(),
                domain.as_deref(),
            )
            .map_err(|e| Reply::json(e.http_status(), e.to_json()))?;
            Ok(Reply::ok(res))
        }
        ("POST", [id, "investigation", "unknowns"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let res = crate::study_check::add_unknown(&r.dir, &b)
                .map_err(|e| Reply::json(e.http_status(), e.to_json()))?;
            Ok(Reply::created(res))
        }
        ("PATCH", [id, "investigation", "unknowns"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let unk_id = b["id"].as_str().unwrap_or("");
            let res = crate::study_check::update_unknown(&r.dir, unk_id, &b)
                .map_err(|e| Reply::json(e.http_status(), e.to_json()))?;
            Ok(Reply::ok(res))
        }
        ("PATCH", [id, "investigation", "unknowns", unk_id]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let res = crate::study_check::update_unknown(&r.dir, unk_id, &b)
                .map_err(|e| Reply::json(e.http_status(), e.to_json()))?;
            Ok(Reply::ok(res))
        }
        ("POST", [id, "study", "check-mechanics"]) => {
            let r = project_of(shared, id)?;
            let b = parse_body(body)?;
            let path = b["path"].as_str();
            let strict = match b.get("strict") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(v)) => *v,
                Some(_) => return Err(Reply::bad("strict must be a boolean")),
            };
            let res = crate::study_check::check_mechanics(&r.dir, path, strict)
                .map_err(|e| Reply::json(e.http_status(), e.to_json()))?;
            Ok(Reply::ok(res))
        }
        ("GET", [id, "study", "status"]) => {
            let r = project_of(shared, id)?;
            let res = crate::study_check::study_status(&r.dir)
                .map_err(|e| Reply::json(e.http_status(), e.to_json()))?;
            Ok(Reply::ok(res))
        }
        ("GET", [id, "playtests"]) => {
            let r = project_of(shared, id)?;
            Ok(Reply::ok(json!({ "runs": crate::playtest::list_runs(&r.dir) })))
        }
        ("GET", [id, "dependencies"]) => {
            let r = project_of(shared, id)?;
            crate::playtest::project_dependencies_report(&r).map(Reply::ok)
        }
        ("POST", [id, "dependencies", "prune"]) => {
            let r = project_of(shared, id)?;
            crate::playtest::prune_or_fix_dependencies(&r)
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
