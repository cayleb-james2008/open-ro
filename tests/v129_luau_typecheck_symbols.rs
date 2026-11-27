//! v1.29.0 Full Luau Type Analysis, Workspace Symbol Index & Cross-File Contract Prover tests:
//! - `GET /api/projects/:id/luau/symbols` (functions, types, exports, remotes, require edges, path filter)
//! - `POST /api/projects/:id/luau/typecheck` (missing export, arity mismatch, RemoteEvent arg mismatch,
//!   passing `--!strict` project, built-in `--!strict` type error, optional `luau-analyze` execution)
//! - `GET /api/projects/:id/luau/definition` (`require()` target, exported function, Remote reference)
//! - CWE-22 / CWE-59 traversal/symlink 400 guards and loopback Host-403 guards
//! - `project_symbols` and `project_typecheck` agent tool dispatch + `project_system_prompt()` < 7 KB.

use openro_rs::agent::{project_system_prompt, system_prompt, Agent};
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard(Vec<(String, Option<String>)>);
impl EnvGuard {
    fn set(pairs: &[(&str, &str)]) -> Self {
        let mut prev = Vec::new();
        for (k, v) in pairs {
            prev.push(((*k).to_string(), std::env::var(k).ok()));
            std::env::set_var(k, v);
        }
        EnvGuard(prev)
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, old) in self.0.drain(..).rev() {
            match old {
                Some(v) => std::env::set_var(&k, v),
                None => std::env::remove_var(&k),
            }
        }
    }
}

struct DirCleanup(PathBuf);
impl Drop for DirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")));
    let dir = base
        .join("openro-v129-luau")
        .join(format!("{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn start_server(data_dir: &Path, agent: Option<Agent>) -> (u16, Arc<Shared>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(
        agent,
        "Luau Typecheck Demo".to_string(),
        data_dir.to_path_buf(),
    );
    let shared_srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, shared_srv);
    });
    (port, shared)
}

fn http_req(
    port: u16,
    host: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> (u16, Value) {
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let req = if body.is_some() {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
    } else {
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
    };
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let sep = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..sep]);
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let raw_body = if sep + 4 <= buf.len() {
        &buf[sep + 4..]
    } else {
        &[]
    };
    let parsed = serde_json::from_slice(raw_body)
        .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(raw_body).to_string() }));
    (status, parsed)
}

fn create_rojo_project(port: u16, root: &Path, slug: &str) -> (String, PathBuf) {
    let proj_dir = root.join(slug);
    std::fs::create_dir_all(proj_dir.join("src/shared")).unwrap();
    std::fs::create_dir_all(proj_dir.join("src/server")).unwrap();
    std::fs::create_dir_all(proj_dir.join("src/client")).unwrap();
    std::fs::write(
        proj_dir.join("default.project.json"),
        json!({
            "name": slug,
            "tree": {
                "$className": "DataModel",
                "ReplicatedStorage": {
                    "$className": "ReplicatedStorage",
                    "Shared": { "$path": "src/shared" },
                    "Remotes": {
                        "$className": "Folder",
                        "PurchaseItem": { "$className": "RemoteEvent" }
                    }
                },
                "ServerScriptService": {
                    "$className": "ServerScriptService",
                    "Server": { "$path": "src/server" }
                },
                "StarterPlayer": {
                    "$className": "StarterPlayer",
                    "StarterPlayerScripts": {
                        "$className": "StarterPlayerScripts",
                        "Client": { "$path": "src/client" }
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        proj_dir.join("openro.project.json"),
        json!({
            "name": slug,
            "rojoProject": "default.project.json",
        })
        .to_string(),
    )
    .unwrap();

    let (st, opened) = http_req(
        port,
        &format!("127.0.0.1:{port}"),
        "POST",
        "/api/projects/open",
        Some(&json!({ "path": proj_dir.to_str().unwrap() })),
    );
    assert_eq!(st, 200, "open project failed: {opened}");
    let id = opened["project"]["id"].as_str().unwrap().to_string();
    (id, proj_dir)
}

fn write_strict_passing_scripts(proj_dir: &Path) {
    std::fs::write(
        proj_dir.join("src/shared/Combat.luau"),
        r#"--!strict
export type HitResult = {
    damage: number,
    critical: boolean,
}

local Combat = {}

local function clampDamage(amount: number): number
    if amount < 0 then
        return 0
    end
    return amount
end

function Combat.DealDamage(base: number, multiplier: number?): number
    local mult = multiplier or 1
    return clampDamage(base * mult)
end

return Combat
"#,
    )
    .unwrap();

    std::fs::write(
        proj_dir.join("src/server/Main.server.luau"),
        r#"--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Combat = require(ReplicatedStorage.Shared.Combat)
local Remotes = ReplicatedStorage:WaitForChild("Remotes")
local PurchaseItem = Remotes:WaitForChild("PurchaseItem")

local dmg: number = Combat.DealDamage(25, 2)
local _ = dmg

PurchaseItem.OnServerEvent:Connect(function(player: Player, itemId: string, qty: number)
    if typeof(itemId) ~= "string" or typeof(qty) ~= "number" then
        return
    end
    local _total = Combat.DealDamage(qty)
end)
"#,
    )
    .unwrap();

    std::fs::write(
        proj_dir.join("src/client/Shop.client.luau"),
        r#"--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Remotes = ReplicatedStorage:WaitForChild("Remotes")
local PurchaseItem = Remotes:WaitForChild("PurchaseItem")

PurchaseItem:FireServer("sword_iron", 1)
"#,
    )
    .unwrap();
}

#[test]
fn luau_symbols_typecheck_passing_and_definition_lookup() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _env = EnvGuard::set(&[("OPENRO_LUAU_ANALYZE", "")]);

    let root = scratch_dir("passing");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let (port, _shared) = start_server(&data_dir, None);
    let host = format!("127.0.0.1:{port}");
    let (pid, proj_dir) = create_rojo_project(port, &root, "StrictPassing");
    write_strict_passing_scripts(&proj_dir);

    // 1. GET /luau/symbols
    let (st_sym, sym_res) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{pid}/luau/symbols"),
        None,
    );
    assert_eq!(st_sym, 200, "{sym_res}");
    assert_eq!(sym_res["ok"], true, "{sym_res}");
    let symbols = sym_res["symbols"].as_array().unwrap();
    assert!(
        symbols
            .iter()
            .any(|s| s["name"] == "DealDamage" && s["kind"] == "export" && s["path"] == "src/shared/Combat.luau"),
        "expected DealDamage export in {symbols:?}"
    );
    assert!(
        symbols
            .iter()
            .any(|s| s["name"] == "clampDamage" && s["kind"] == "function"),
        "expected clampDamage function in {symbols:?}"
    );
    assert!(
        symbols
            .iter()
            .any(|s| s["name"] == "HitResult" && s["kind"] == "type"),
        "expected HitResult type in {symbols:?}"
    );
    assert!(
        symbols
            .iter()
            .any(|s| s["name"] == "PurchaseItem" && s["kind"] == "remote_event"),
        "expected PurchaseItem remote_event in {symbols:?}"
    );
    assert!(
        !sym_res["require_edges"].as_array().unwrap().is_empty(),
        "expected require_edges"
    );

    // 2. POST /luau/typecheck passes cleanly on valid --!strict project
    let (st_tc, tc_res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{pid}/luau/typecheck"),
        Some(&json!({})),
    );
    assert_eq!(st_tc, 200, "{tc_res}");
    assert_eq!(tc_res["ok"], true, "{tc_res}");
    assert_eq!(tc_res["diagnostics_source"], "builtin", "{tc_res}");
    assert!(
        tc_res["mismatches"].as_array().unwrap().is_empty(),
        "expected 0 mismatches: {}",
        tc_res["mismatches"]
    );

    // 3. GET /luau/definition resolves require(), exported function, and Remote reference
    // Line 3 of Main.server.luau: `local Combat = require(ReplicatedStorage.Shared.Combat)`
    let (st_def_req, def_req) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{pid}/luau/definition?path=src/server/Main.server.luau&line=3&col=25"),
        None,
    );
    assert_eq!(st_def_req, 200, "{def_req}");
    assert_eq!(def_req["ok"], true, "{def_req}");
    assert_eq!(def_req["target_path"], "src/shared/Combat.luau", "{def_req}");

    // Line 7 of Main.server.luau: `local dmg: number = Combat.DealDamage(25, 2)`
    let (st_def_fn, def_fn) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{pid}/luau/definition?path=src/server/Main.server.luau&line=7&col=28"),
        None,
    );
    assert_eq!(st_def_fn, 200, "{def_fn}");
    assert_eq!(def_fn["ok"], true, "{def_fn}");
    assert_eq!(def_fn["target_path"], "src/shared/Combat.luau", "{def_fn}");
    assert_eq!(def_fn["line"], 16, "{def_fn}");
    assert_eq!(def_fn["symbol"]["name"], "DealDamage", "{def_fn}");

    // Line 6 of Shop.client.luau: `PurchaseItem:FireServer("sword_iron", 1)`
    let (st_def_rem, def_rem) = http_req(
        port,
        &host,
        "GET",
        &format!("/api/projects/{pid}/luau/definition?path=src/client/Shop.client.luau&line=6&col=16"),
        None,
    );
    assert_eq!(st_def_rem, 200, "{def_rem}");
    assert_eq!(def_rem["ok"], true, "{def_rem}");
    assert_eq!(def_rem["target_path"], "src/server/Main.server.luau", "{def_rem}");
    assert_eq!(def_rem["symbol"]["name"], "PurchaseItem", "{def_rem}");
}

#[test]
fn luau_typecheck_detects_missing_export_arity_remote_and_strict_mismatches() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _env = EnvGuard::set(&[("OPENRO_LUAU_ANALYZE", "")]);

    let root = scratch_dir("mismatches");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let (port, _shared) = start_server(&data_dir, None);
    let host = format!("127.0.0.1:{port}");
    let (pid, proj_dir) = create_rojo_project(port, &root, "MismatchSuite");

    std::fs::write(
        proj_dir.join("src/shared/Combat.luau"),
        r#"--!strict
local Combat = {}

function Combat.DealDamage(targetName: string, amount: number): number
    return amount
end

return Combat
"#,
    )
    .unwrap();

    std::fs::write(
        proj_dir.join("src/server/Main.server.luau"),
        r#"--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Combat = require(ReplicatedStorage.Shared.Combat)
local Remotes = ReplicatedStorage:WaitForChild("Remotes")
local PurchaseItem = Remotes:WaitForChild("PurchaseItem")

local badStrict: number = "not_a_number"
local _a = Combat.MissingExport(10)
local _b = Combat.DealDamage("Goblin")

PurchaseItem.OnServerEvent:Connect(function(player: Player, itemId: string, qty: number)
    if typeof(itemId) ~= "string" or typeof(qty) ~= "number" then
        return
    end
    local _ = badStrict
end)
"#,
    )
    .unwrap();

    std::fs::write(
        proj_dir.join("src/client/Shop.client.luau"),
        r#"--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Remotes = ReplicatedStorage:WaitForChild("Remotes")
local PurchaseItem = Remotes:WaitForChild("PurchaseItem")

PurchaseItem:FireServer("sword_iron")
"#,
    )
    .unwrap();

    let (st, res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{pid}/luau/typecheck"),
        Some(&json!({})),
    );
    assert_eq!(st, 200, "{res}");
    assert_eq!(res["ok"], false, "{res}");
    let mismatches = res["mismatches"].as_array().unwrap();
    let kinds: Vec<&str> = mismatches
        .iter()
        .filter_map(|m| m["kind"].as_str())
        .collect();
    assert!(kinds.contains(&"missing_export"), "mismatches: {mismatches:?}");
    assert!(kinds.contains(&"arity_mismatch"), "mismatches: {mismatches:?}");
    assert!(kinds.contains(&"remote_arg_mismatch"), "mismatches: {mismatches:?}");
    assert!(kinds.contains(&"strict_type_error"), "mismatches: {mismatches:?}");
}

#[test]
fn luau_typecheck_runs_optional_luau_analyze_when_present() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("luau-analyze-mock");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    // Create deterministic mock `luau-analyze` binary
    let fake_bin = root.join("luau-analyze");
    std::fs::write(
        &fake_bin,
        "#!/bin/sh\necho 'src/server/Main.server.luau(4,7): TypeError: Type string could not be converted into number'\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&fake_bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_bin, perms).unwrap();
    }

    let _env = EnvGuard::set(&[("OPENRO_LUAU_ANALYZE", fake_bin.to_str().unwrap())]);

    let (port, _shared) = start_server(&data_dir, None);
    let host = format!("127.0.0.1:{port}");
    let (pid, proj_dir) = create_rojo_project(port, &root, "MockLuauAnalyze");
    write_strict_passing_scripts(&proj_dir);

    let (st, res) = http_req(
        port,
        &host,
        "POST",
        &format!("/api/projects/{pid}/luau/typecheck"),
        Some(&json!({})),
    );
    assert_eq!(st, 200, "{res}");
    assert_eq!(res["luau_analyze_available"], true, "{res}");
    assert_eq!(res["diagnostics_source"], "luau-analyze+builtin", "{res}");
    assert_eq!(res["ok"], false, "{res}");
    let diags = res["diagnostics"].as_array().unwrap();
    assert!(
        diags
            .iter()
            .any(|d| d["source"] == "luau-analyze" && d["line"] == 4 && d["col"] == 7),
        "expected parsed luau-analyze diagnostic: {diags:?}"
    );
}

#[test]
fn luau_routes_enforce_loopback_403_and_cwe22_cwe59_guards() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _env = EnvGuard::set(&[("OPENRO_LUAU_ANALYZE", "")]);

    let root = scratch_dir("security-guards");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let (port, _shared) = start_server(&data_dir, None);
    let host = format!("127.0.0.1:{port}");
    let (pid, proj_dir) = create_rojo_project(port, &root, "SecurityGuards");
    write_strict_passing_scripts(&proj_dir);

    // 1. Non-loopback Host rejected with 403 on all 3 Luau routes
    for (method, path) in [
        ("GET", format!("/api/projects/{pid}/luau/symbols")),
        ("POST", format!("/api/projects/{pid}/luau/typecheck")),
        (
            "GET",
            format!("/api/projects/{pid}/luau/definition?path=src/server/Main.server.luau&line=1&col=1"),
        ),
    ] {
        let body = if method == "POST" { Some(json!({})) } else { None };
        let (st, _) = http_req(port, "attacker.example:8080", method, &path, body.as_ref());
        assert_eq!(st, 403, "expected 403 for non-loopback {method} {path}");
    }

    // 2. CWE-22 path traversal rejected with 400
    for bad_path in [
        "../outside.luau",
        "%2e%2e%2foutside.luau",
        "src/../../etc/passwd",
        r"src\server\Main.server.luau",
    ] {
        let (st_def, _) = http_req(
            port,
            &host,
            "GET",
            &format!("/api/projects/{pid}/luau/definition?path={bad_path}&line=1&col=1"),
            None,
        );
        assert_eq!(st_def, 400, "expected 400 on traversal path {bad_path}");

        let (st_sym, _) = http_req(
            port,
            &host,
            "GET",
            &format!("/api/projects/{pid}/luau/symbols?path={bad_path}"),
            None,
        );
        assert_eq!(st_sym, 400, "expected 400 on symbols traversal path {bad_path}");
    }

    // 3. CWE-59 symlink escape rejected with 400
    #[cfg(unix)]
    {
        let outside = root.join("secret.luau");
        std::fs::write(&outside, "return { secret = true }\n").unwrap();
        std::os::unix::fs::symlink(&outside, proj_dir.join("src/shared/Link.luau")).unwrap();
        let (st_link, _) = http_req(
            port,
            &host,
            "GET",
            &format!("/api/projects/{pid}/luau/definition?path=src/shared/Link.luau&line=1&col=1"),
            None,
        );
        assert_eq!(st_link, 400, "expected 400 on symlink path");
    }
}

fn read_http_request(stream: &mut TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut raw = Vec::new();
    let mut tmp = [0u8; 4096];
    let mut header_end = None;
    let mut content_len = 0usize;
    loop {
        let n = stream.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&tmp[..n]);
        if header_end.is_none() {
            if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = Some(pos + 4);
                let hdr = String::from_utf8_lossy(&raw[..pos]);
                for line in hdr.lines() {
                    if let Some((k, v)) = line.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            content_len = v.trim().parse().unwrap_or(0);
                        }
                    }
                }
            }
        }
        if let Some(end) = header_end {
            if raw.len() >= end + content_len {
                break;
            }
        }
    }
}

#[test]
fn luau_agent_tools_and_prompt_budget() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _env = EnvGuard::set(&[("OPENRO_LUAU_ANALYZE", "")]);

    let psp = project_system_prompt();
    let sp = system_prompt();
    for tool in ["project_symbols", "project_typecheck"] {
        assert!(psp.contains(tool), "project_system_prompt missing {tool}");
        assert!(sp.contains(tool), "system_prompt missing {tool}");
    }
    assert!(
        psp.len() < 7_000,
        "project_system_prompt exceeded 7 KB cap: {} bytes",
        psp.len()
    );

    let root = scratch_dir("agent-tools");
    let _cleanup = DirCleanup(root.clone());
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    let turn = Arc::new(AtomicUsize::new(0));
    let turn_c = Arc::clone(&turn);
    let llm_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let llm_port = llm_listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in llm_listener.incoming().flatten() {
            read_http_request(&mut stream);
            let n = turn_c.fetch_add(1, Ordering::SeqCst);
            let content = match n {
                0 => json!({
                    "steps": [
                        { "tool": "project_symbols", "args": { "path": "src/shared/Combat.luau" } },
                        { "tool": "project_typecheck", "args": {} }
                    ],
                    "done": true,
                    "summary": "Checked workspace symbols and Luau type contracts."
                })
                .to_string(),
                _ => json!({ "done": true, "summary": "done" }).to_string(),
            };
            let body = json!({
                "choices": [{ "message": { "role": "assistant", "content": content } }]
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let model = ModelClient::new(format!("http://127.0.0.1:{llm_port}/v1"), "mock".into(), None);
    let agent = Agent::with_local_place(model, LocalPlace::open(data_dir.clone(), "LuauAgent".into()));
    let (port, shared) = start_server(&data_dir, Some(agent));
    let host = format!("127.0.0.1:{port}");
    let (_pid, proj_dir) = create_rojo_project(port, &root, "AgentLuau");
    write_strict_passing_scripts(&proj_dir);
    {
        let mut store = shared.projects.lock().unwrap();
        let entry = store.open(proj_dir.to_str().unwrap()).unwrap();
        store.set_active(&entry.id);
    }

    let (st, chat) = http_req(
        port,
        &host,
        "POST",
        "/api/chat",
        Some(&json!({ "message": "Inspect Combat symbols and typecheck the project." })),
    );
    assert_eq!(st, 200, "{chat}");
    assert_eq!(chat["ok"], true, "{chat}");
    let transcript = chat["transcript"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    assert!(transcript.contains("tool project_symbols ->"), "{transcript}");
    assert!(transcript.contains("tool project_typecheck -> ok=true"), "{transcript}");
}
