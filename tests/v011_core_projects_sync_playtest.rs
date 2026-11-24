//! Integration tests for v0.11 OpenRO Core:
//! - Workspace projects (list, create, open, manifest + agent priceApproved guard, build)
//! - Project file API + preview + path escape / `.openro` / `.git` guards
//! - Project static check (`luau_check` / `selene` / `stylua`) & `lune` runner
//! - Project git hooks (`status`, `log`, `commit` scoped to files, `revert` scoped to project)
//! - Chunked one-way Studio push-sync against `mock_studio` (structural services before
//!   Workspace, >120 items multi-chunk compare, >65 KiB chunked script source, diff/apply/prune)
//! - Scripted Playtest Runner against `mock_studio` (`start_stop_play`, Server/Client checks,
//!   console error capture + `allow_errors` + stale-error isolation, PNG screenshot gallery,
//!   JSON + HTML reports saved per build and served over HTTP)
//! - CLI `--data-dir` and unknown-flag fail-fast guard

use openro_rs::agent::Agent;
use openro_rs::mcp::McpClient;
use openro_rs::model::ModelClient;
use openro_rs::projects;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static DIR_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(label: &str) -> PathBuf {
    let n = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("v011-core-tests");
    let dir = base.join(format!("{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test temp dir");
    dir
}

fn mock_studio_command() -> String {
    env!("CARGO_BIN_EXE_mock_studio").to_string()
}

fn start_server(agent: Option<Agent>, data_dir: PathBuf) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(agent, "Test Game".to_string(), data_dir);
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    port
}

fn http_req(
    port: u16,
    method: &str,
    path: &str,
    host: &str,
    body: Option<&str>,
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let payload = body.unwrap_or("");
    let ct = if body.is_some() {
        "Content-Type: application/json\r\n"
    } else {
        ""
    };
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{ct}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(req.as_bytes()).expect("write");
    stream.flush().ok();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read");
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("http header separator");
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let body_bytes = raw[split + 4..].to_vec();
    (status, head, body_bytes)
}

fn api_json(port: u16, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let body_str = body.map(|v| v.to_string());
    let (code, _, bytes) = http_req(
        port,
        method,
        path,
        &format!("127.0.0.1:{port}"),
        body_str.as_deref(),
    );
    let parsed = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "non-JSON response ({code}) for {method} {path}: {e}\n{}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (code, parsed)
}

fn run_git_ok(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn projects_lifecycle_manifest_gate_files_preview_and_build() {
    let root = temp_dir("lifecycle");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    let port = start_server(None, data.clone());

    // Non-loopback host refused on projects API
    let (code, _, _) = http_req(port, "GET", "/api/projects", "evil.example.com", None);
    assert_eq!(code, 403);

    // Add workspace root and create project
    let (code, res) = api_json(
        port,
        "POST",
        "/api/projects/workspace",
        Some(json!({ "path": ws.to_string_lossy() })),
    );
    assert_eq!(code, 200);
    assert_eq!(res["ok"], json!(true));

    let (code, res) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({
            "name": "Anime Card Test",
            "parent": ws.to_string_lossy(),
            "universeId": 111222,
            "placeId": 333444
        })),
    );
    assert_eq!(code, 201);
    let proj_id = res["project"]["id"].as_str().unwrap().to_string();
    assert_eq!(proj_id, "anime-card-test");
    assert_eq!(res["project"]["has_rojo"], json!(true));
    assert_eq!(res["project"]["has_manifest"], json!(true));
    assert_eq!(res["project"]["universeId"], json!(111222));
    assert!(res["project"]["products"].is_array());

    // Check .gitignore includes .openro/, build/, art/references/, refs/
    let proj_dir = ws.join("anime-card-test");
    let gi = std::fs::read_to_string(proj_dir.join(".gitignore")).unwrap();
    assert!(gi.contains(".openro/"));
    assert!(gi.contains("build/"));
    assert!(gi.contains("art/references/"));

    // Manifest update: agent actor CANNOT set priceApproved: true (#19)
    let (code, res) = api_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/manifest"),
        Some(json!({
            "actor": "agent",
            "customFlag": "keep-me",
            "products": [
                {
                    "key": "vip",
                    "kind": "game_pass",
                    "name": "VIP",
                    "priceRobux": 399,
                    "forSale": true,
                    "priceApproved": true,
                    "customNote": "preserved"
                }
            ]
        })),
    );
    assert_eq!(code, 200);
    assert_eq!(res["manifest"]["customFlag"], json!("keep-me"));
    assert_eq!(res["manifest"]["products"][0]["priceApproved"], json!(false));
    assert_eq!(res["manifest"]["products"][0]["customNote"], json!("preserved"));
    assert!(res["manifest"].get("actor").is_none());

    // Human UI approves price
    let (code, res) = api_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/manifest"),
        Some(json!({
            "products": [
                {
                    "key": "vip",
                    "kind": "game_pass",
                    "name": "VIP",
                    "priceRobux": 399,
                    "forSale": true,
                    "priceApproved": true
                }
            ]
        })),
    );
    assert_eq!(code, 200);
    assert_eq!(res["manifest"]["products"][0]["priceApproved"], json!(true));

    // Agent adds a second product while leaving VIP unchanged -> VIP stays approved, new product is false
    let (code, res) = api_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/manifest"),
        Some(json!({
            "actor": "agent",
            "products": [
                { "key": "vip", "kind": "game_pass", "name": "VIP", "priceRobux": 399, "forSale": true, "priceApproved": true },
                { "key": "luck", "kind": "developer_product", "name": "Luck", "priceRobux": 49, "forSale": true, "priceApproved": true }
            ]
        })),
    );
    assert_eq!(code, 200);
    assert_eq!(res["manifest"]["products"][0]["priceApproved"], json!(true));
    assert_eq!(res["manifest"]["products"][1]["priceApproved"], json!(false));

    // Agent changing price of approved VIP resets approval to false
    let (code, res) = api_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/manifest"),
        Some(json!({
            "actor": "agent",
            "products": [
                { "key": "vip", "kind": "game_pass", "name": "VIP", "priceRobux": 999, "forSale": true, "priceApproved": true }
            ]
        })),
    );
    assert_eq!(code, 200);
    assert_eq!(res["manifest"]["products"][0]["priceApproved"], json!(false));

    // File API: write, read, edit, concurrency conflict, delete, security blocks
    let (code, w) = api_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/file"),
        Some(json!({ "path": "src/shared/Cards.luau", "content": "return { count = 10 }\n" })),
    );
    assert_eq!(code, 200);
    assert_eq!(w["created"], json!(true));
    let sha = w["sha256"].as_str().unwrap().to_string();

    let (code, r) = api_json(
        port,
        "GET",
        &format!("/api/projects/{proj_id}/file?path=src/shared/Cards.luau"),
        None,
    );
    assert_eq!(code, 200);
    assert_eq!(r["sha256"], json!(sha));

    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/file/edit"),
        Some(json!({
            "path": "src/shared/Cards.luau",
            "expect_sha256": sha,
            "edits": [{ "old": "count = 10", "new": "count = 40" }]
        })),
    );
    assert_eq!(code, 200);

    // Stale expect_sha256 fails with 409
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/file/edit"),
        Some(json!({
            "path": "src/shared/Cards.luau",
            "expect_sha256": sha,
            "edits": [{ "old": "count = 40", "new": "count = 99" }]
        })),
    );
    assert_eq!(code, 409);

    // Blocked paths: .., .git, .openro, build, manifest, lock
    for bad in [
        "../escape.txt",
        "/etc/passwd",
        ".git/config",
        ".openro/teamwork/board.jsonl",
        "build/out.rbxlx",
        "openro.project.json",
        "assets.lock.json",
    ] {
        let (code, _) = api_json(
            port,
            "POST",
            &format!("/api/projects/{proj_id}/file"),
            Some(json!({ "path": bad, "content": "nope" })),
        );
        assert_eq!(code, 400, "expected 400 writing {bad}");
    }
    // `.openro` also blocked on read
    std::fs::create_dir_all(proj_dir.join(".openro")).unwrap();
    std::fs::write(proj_dir.join(".openro/secret.txt"), "internal").unwrap();
    let (code, _) = api_json(
        port,
        "GET",
        &format!("/api/projects/{proj_id}/file?path=.openro/secret.txt"),
        None,
    );
    assert_eq!(code, 400);

    // Preview endpoint serves local PNGs inside project
    let png_sig: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    std::fs::create_dir_all(proj_dir.join("art/ui")).unwrap();
    std::fs::write(proj_dir.join("art/ui/coin.png"), png_sig).unwrap();
    let (code, head, bytes) = http_req(
        port,
        "GET",
        &format!("/api/projects/{proj_id}/preview?path=art/ui/coin.png"),
        &format!("127.0.0.1:{port}"),
        None,
    );
    assert_eq!(code, 200);
    assert!(head.contains("image/png"));
    assert_eq!(bytes, png_sig);

    // Built-in .rbxlx build + second rapid build produce two unique records
    let (code, b1) = api_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/build"),
        Some(json!({ "engine": "builtin" })),
    );
    assert_eq!(code, 200);
    assert_eq!(b1["engine"], json!("builtin"));
    assert!(b1["bytes"].as_u64().unwrap() > 100);
    let xml = std::fs::read_to_string(proj_dir.join(b1["path"].as_str().unwrap())).unwrap();
    assert!(xml.contains("<roblox version=\"4\">"));
    assert!(xml.contains("count = 40"));

    let (code, b2) = api_json(
        port,
        "POST",
        &format!("/api/projects/{proj_id}/build"),
        Some(json!({ "engine": "auto" })),
    );
    assert_eq!(code, 200);
    assert_ne!(b1["build_id"], b2["build_id"]);

    let (code, blist) = api_json(port, "GET", &format!("/api/projects/{proj_id}/builds"), None);
    assert_eq!(code, 200);
    assert_eq!(blist["builds"].as_array().unwrap().len(), 2);

    // Persistence across server restart
    let port2 = start_server(None, data);
    let (code, list2) = api_json(port2, "GET", "/api/projects", None);
    assert_eq!(code, 200);
    assert_eq!(list2["active"], json!(proj_id));
    assert!(list2["projects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == json!(proj_id)));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn project_git_commit_scoped_and_revert_scoped() {
    let root = temp_dir("git");
    let repo = root.join("repo");
    let proj = repo.join("projects/my-game");
    let data = root.join("data");
    std::fs::create_dir_all(&proj).unwrap();

    // Initialize a monorepo with a root file outside the project
    run_git_ok(&repo, &["init"]);
    run_git_ok(&repo, &["config", "user.name", "Test"]);
    run_git_ok(&repo, &["config", "user.email", "test@local"]);
    std::fs::write(repo.join("OUTSIDE.txt"), "root file\n").unwrap();
    std::fs::create_dir_all(proj.join("src/shared")).unwrap();
    std::fs::write(
        proj.join("default.project.json"),
        json!({ "name": "My Game", "tree": { "$className": "DataModel", "ReplicatedStorage": { "Shared": { "$path": "src/shared" } } } }).to_string(),
    )
    .unwrap();
    std::fs::write(proj.join("src/shared/Base.luau"), "return 1\n").unwrap();
    run_git_ok(&repo, &["add", "."]);
    run_git_ok(&repo, &["commit", "-m", "initial"]);
    let outside_sha = run_git_ok(&repo, &["rev-parse", "HEAD"]);

    let port = start_server(None, data);
    let (code, opened) = api_json(
        port,
        "POST",
        "/api/projects/open",
        Some(json!({ "path": proj.to_string_lossy() })),
    );
    assert_eq!(code, 200);
    let id = opened["project"]["id"].as_str().unwrap().to_string();

    // Write two project files + one blocked .openro file
    std::fs::write(proj.join("src/shared/A.luau"), "return 'A'\n").unwrap();
    std::fs::write(proj.join("src/shared/B.luau"), "return 'B'\n").unwrap();
    std::fs::create_dir_all(proj.join(".openro")).unwrap();
    std::fs::write(proj.join(".openro/board.jsonl"), "{}\n").unwrap();

    // Status omits .openro/
    let (code, st) = api_json(port, "GET", &format!("/api/projects/{id}/git/status"), None);
    assert_eq!(code, 200);
    assert_eq!(st["is_git_repo"], json!(true));
    assert_eq!(st["dirty"], json!(true));
    let paths: Vec<&str> = st["changed"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["path"].as_str())
        .collect();
    assert!(paths.contains(&"src/shared/A.luau"));
    assert!(paths.contains(&"src/shared/B.luau"));
    assert!(!paths.iter().any(|p| p.contains(".openro")));

    // Commit ONLY A.luau (+ pass .openro file to verify it is silently excluded)
    let (code, c) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/git/commit"),
        Some(json!({
            "message": "openro(systems): add A [team r1 task t1]",
            "files": ["src/shared/A.luau", ".openro/board.jsonl"]
        })),
    );
    assert_eq!(code, 200);
    assert_eq!(c["committed"], json!(true));
    let task_sha = c["commit"].as_str().unwrap().to_string();

    // Verify B.luau is still dirty and .openro was NOT committed
    let committed_files = run_git_ok(
        &repo,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", &task_sha],
    );
    assert_eq!(committed_files, "projects/my-game/src/shared/A.luau");

    // Re-committing unchanged A.luau via Rust hook returns Ok(None)
    let again = projects::git_commit(
        &proj,
        &["src/shared/A.luau".to_string()],
        "openro(systems): noop",
    )
    .unwrap();
    assert_eq!(again, None);

    // Log shows the task commit
    let (code, log) = api_json(port, "GET", &format!("/api/projects/{id}/git/log?limit=5"), None);
    assert_eq!(code, 200);
    assert_eq!(log["commits"][0]["commit"], json!(task_sha));

    // Refuses to revert the initial commit because it touched OUTSIDE.txt outside projects/my-game
    let (code, err) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/git/revert"),
        Some(json!({ "commit": outside_sha })),
    );
    assert_eq!(code, 400);
    assert!(err["error"].as_str().unwrap().contains("outside this project"));

    // Cleanly reverts the task commit -> A.luau removed!
    let (code, rev) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/git/revert"),
        Some(json!({ "commit": task_sha })),
    );
    assert_eq!(code, 200);
    assert_eq!(rev["reverted"], json!(true));
    assert!(!proj.join("src/shared/A.luau").exists());
    assert!(proj.join("src/shared/B.luau").exists());

    // Refuse > 5 MB file
    std::fs::write(proj.join("src/shared/huge.bin"), vec![0u8; 5 * 1024 * 1024 + 1]).unwrap();
    let res = projects::git_commit(&proj, &["src/shared/huge.bin".to_string()], "too big");
    assert!(res.is_err());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn project_check_and_run_lune() {
    let root = temp_dir("check-lune");
    let ws = root.join("ws");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    let port = start_server(None, data);

    api_json(
        port,
        "POST",
        "/api/projects/workspace",
        Some(json!({ "path": ws.to_string_lossy() })),
    );
    let (_, created) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "Check Demo", "parent": ws.to_string_lossy() })),
    );
    let id = created["project"]["id"].as_str().unwrap().to_string();

    // Write a syntax-broken Luau script -> /check reports luau.passed == false
    api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/file"),
        Some(json!({
            "path": "src/shared/Broken.luau",
            "content": "local function bad(\n    return 1\n"
        })),
    );
    let (code, chk) = api_json(port, "POST", &format!("/api/projects/{id}/check"), Some(json!({})));
    assert_eq!(code, 200);
    assert_eq!(chk["luau"]["passed"], json!(false));
    assert!(chk["luau"]["errors"].as_u64().unwrap() > 0);

    // Fix it + add src/shared/Config/Rarities.luau using Luau `+=` (proves #33 selene roblox std + #34 no Config collision)
    api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/file"),
        Some(json!({
            "path": "src/shared/Broken.luau",
            "content": "local function ok()\n\treturn 1\nend\nreturn ok\n"
        })),
    );
    api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/file"),
        Some(json!({
            "path": "src/shared/Config/Rarities.luau",
            "content": "local total = 0\nfor i = 1, 3 do\n\ttotal += i\nend\nreturn { total = total }\n"
        })),
    );
    let (code, chk) = api_json(port, "POST", &format!("/api/projects/{id}/check"), Some(json!({})));
    assert_eq!(code, 200);
    assert_eq!(chk["luau"]["passed"], json!(true), "luau failed: {chk}");
    assert_eq!(chk["selene"]["passed"], json!(true), "selene failed: {chk}");
    assert_eq!(chk["stylua"]["passed"], json!(true), "stylua failed: {chk}");
    assert_eq!(chk["passed"], json!(true), "overall check failed: {chk}");

    let (code, b) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/build"),
        Some(json!({ "engine": "auto" })),
    );
    assert_eq!(code, 200, "build failed with Config/Rarities.luau: {b}");

    // If lune is installed, exercise /run-lune end to end
    if projects::find_tool("lune", "OPENRO_LUNE").is_some() {
        api_json(
            port,
            "POST",
            &format!("/api/projects/{id}/file"),
            Some(json!({
                "path": "tools/sim.luau",
                "content": "local process = require(\"@lune/process\")\nprint(\"SIM_OK:\" .. tostring(process.args[1] or \"none\"))\n"
            })),
        );
        let (code, lr) = api_json(
            port,
            "POST",
            &format!("/api/projects/{id}/run-lune"),
            Some(json!({ "script": "tools/sim.luau", "args": ["42"], "timeout_secs": 10 })),
        );
        assert_eq!(code, 200);
        assert_eq!(lr["passed"], json!(true));
        assert_eq!(lr["exit_code"], json!(0));
        assert!(lr["stdout"].as_str().unwrap().contains("SIM_OK:42"));

        // Refuse non-luau / path escape
        let (code, _) = api_json(
            port,
            "POST",
            &format!("/api/projects/{id}/run-lune"),
            Some(json!({ "script": "../evil.luau" })),
        );
        assert_eq!(code, 400);
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn chunked_sync_order_prune_and_playtest_runner_against_mock_studio() {
    let root = temp_dir("sync-playtest");
    let proj = root.join("uacg-sim");
    let data = root.join("data");
    std::fs::create_dir_all(proj.join("src/server")).unwrap();
    std::fs::create_dir_all(proj.join("src/client")).unwrap();
    std::fs::create_dir_all(proj.join("src/shared")).unwrap();
    std::fs::create_dir_all(proj.join("workspace")).unwrap();

    // 135 Workspace modules/models + structural services to guarantee:
    //   (a) > 120 items -> multiple compare chunks
    //   (b) structural services walked BEFORE Workspace
    //   (c) one 68 KiB script -> multiple append pieces during apply
    std::fs::write(
        proj.join("default.project.json"),
        serde_json::to_string_pretty(&json!({
            "name": "Sync Playtest Demo",
            "tree": {
                "$className": "DataModel",
                "Workspace": { "Props": { "$path": "workspace" } },
                "Lighting": { "$className": "Lighting", "$properties": { "Brightness": 2 } },
                "ReplicatedStorage": { "Shared": { "$path": "src/shared" } },
                "ServerScriptService": { "Server": { "$path": "src/server" } },
                "StarterPlayer": {
                    "StarterPlayerScripts": { "$className": "StarterPlayerScripts", "Client": { "$path": "src/client" } }
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let big_src = format!("-- big module\nlocal x = \"{}\"\nreturn #x\n", "A".repeat(68_000));
    let big_len = big_src.len();
    std::fs::write(proj.join("src/shared/Big.luau"), &big_src).unwrap();
    std::fs::write(proj.join("src/shared/Cards.luau"), "return { count = 40 }\n").unwrap();
    std::fs::write(proj.join("src/server/Main.server.luau"), "print('server boot')\n").unwrap();
    std::fs::write(proj.join("src/client/Hud.client.luau"), "print('client boot')\n").unwrap();
    for i in 0..130 {
        std::fs::write(
            proj.join(format!("workspace/Prop{i:03}.model.json")),
            json!({ "ClassName": "Part", "Properties": { "Anchored": true } }).to_string(),
        )
        .unwrap();
    }

    // Boot OpenRO with real mock_studio MCP child process
    let mcp = McpClient::spawn(&mock_studio_command()).expect("spawn mock_studio");
    let agent = Agent::new(
        ModelClient::new("http://127.0.0.1:0".into(), "test".into(), None),
        Some(mcp),
    );
    let port = start_server(Some(agent), data);

    let (code, opened) = api_json(
        port,
        "POST",
        "/api/projects/open",
        Some(json!({ "path": proj.to_string_lossy() })),
    );
    assert_eq!(code, 200);
    let id = opened["project"]["id"].as_str().unwrap().to_string();

    // Build first so Playtest Runner attaches reports to a real build_id
    let (code, build_rec) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/build"),
        Some(json!({ "engine": "builtin" })),
    );
    assert_eq!(code, 200);
    let build_id = build_rec["build_id"].as_str().unwrap().to_string();

    // 1. Read-only sync/diff (multi-chunk)
    let (code, diff1) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/sync/diff"),
        Some(json!({})),
    );
    assert_eq!(code, 200);
    assert_eq!(diff1["dry_run"], json!(true));
    assert!(diff1["chunks"].as_u64().unwrap() >= 2, "expected multi-chunk diff: {diff1}");
    assert!(diff1["summary"]["create"].as_u64().unwrap() >= 135);

    // 2. Apply sync
    let (code, sync1) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/sync"),
        Some(json!({ "prune": false })),
    );
    assert_eq!(code, 200);
    assert_eq!(sync1["errors"].as_array().unwrap().len(), 0);
    assert_eq!(sync1["verify"]["remaining"], json!(0));
    assert!(sync1["applied"]["created"].as_u64().unwrap() >= 135);

    // 3. Re-diff is completely clean
    let (code, diff2) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/sync/diff"),
        Some(json!({})),
    );
    assert_eq!(code, 200);
    assert_eq!(diff2["summary"]["create"], json!(0));
    assert_eq!(diff2["summary"]["update"], json!(0));

    // 4. Modify one script + remove one prop -> test update + extra + prune
    std::fs::write(proj.join("src/shared/Cards.luau"), "return { count = 72 }\n").unwrap();
    std::fs::remove_file(proj.join("workspace/Prop000.model.json")).unwrap();

    let (code, diff3) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/sync/diff"),
        Some(json!({})),
    );
    assert_eq!(code, 200);
    assert_eq!(diff3["summary"]["update"], json!(1));
    assert_eq!(diff3["summary"]["extra"], json!(1));
    assert!(diff3["extra"]
        .as_array()
        .unwrap()
        .contains(&json!("Workspace/Props/Prop000")));

    // Sync without prune keeps extra; sync with prune deletes it
    let (code, sync_noprune) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/sync"),
        Some(json!({ "prune": false })),
    );
    assert_eq!(code, 200);
    assert_eq!(sync_noprune["applied"]["updated"], json!(1));
    assert_eq!(sync_noprune["applied"]["deleted"], json!(0));
    assert_eq!(sync_noprune["verify"]["extra"], json!(1));

    let (code, sync_prune) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/sync"),
        Some(json!({ "prune": true })),
    );
    assert_eq!(code, 200);
    assert_eq!(sync_prune["applied"]["deleted"], json!(1));
    assert_eq!(sync_prune["verify"]["remaining"], json!(0));
    assert_eq!(sync_prune["verify"]["extra"], json!(0));

    // 5. Playtest Runner (passing): verifies structural-before-Workspace order,
    //    exact 68 KiB script byte length, Server vs Client datamodel routing, and PNG gallery
    let (code, pt1) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/playtest"),
        Some(json!({
            "mode": "start_play",
            "settle_ms": 0,
            "checks": [
                { "name": "server dm", "datamodel": "Server", "luau": "return __dm__", "expect": { "equals": "Server" } },
                { "name": "client dm", "datamodel": "Client", "luau": "return __dm__", "expect": { "equals": "Client" } },
                { "name": "big script intact", "luau": "return __source_len__(\"ReplicatedStorage/Shared/Big\")", "expect": { "equals": big_len } },
                { "name": "pruned gone", "luau": "return __exists__(\"Workspace/Props/Prop000\")", "expect": { "equals": false } },
                { "name": "prop kept", "luau": "return __exists__(\"Workspace/Props/Prop001\")", "expect": { "truthy": true } },
                { "name": "compare order", "luau": "return __compare_order__" }
            ],
            "screenshots": [
                { "name": "hub", "camera_position": [0, 30, 50], "camera_look_at": [0, 0, 0] },
                { "name": "shrine", "camera_position": [20, 15, -30], "camera_look_at": [0, 5, 0] }
            ]
        })),
    );
    assert_eq!(code, 200);
    assert_eq!(pt1["ok"], json!(true));
    assert_eq!(pt1["passed"], json!(true), "playtest 1 failed: {pt1}");
    assert_eq!(pt1["build_id"], json!(build_id));
    assert_eq!(pt1["screenshots"].as_array().unwrap().len(), 2);

    // Prove structural services were walked strictly before Workspace during sync!
    let order = pt1["checks"][5]["value"].as_array().unwrap();
    let first_ws = order
        .iter()
        .position(|p| p.as_str().unwrap_or("").starts_with("Workspace"))
        .expect("Workspace walked");
    let first_rep = order
        .iter()
        .position(|p| p.as_str().unwrap_or("").starts_with("ReplicatedStorage"))
        .expect("ReplicatedStorage walked");
    let first_sss = order
        .iter()
        .position(|p| p.as_str().unwrap_or("").starts_with("ServerScriptService"))
        .expect("ServerScriptService walked");
    assert!(first_rep < first_ws, "ReplicatedStorage must precede Workspace");
    assert!(first_sss < first_ws, "ServerScriptService must precede Workspace");

    // Verify JSON report, HTML report, and PNG screenshot endpoints
    let run1 = pt1["run_id"].as_str().unwrap().to_string();
    let (code, rep_json) = api_json(
        port,
        "GET",
        &format!("/api/projects/{id}/playtests/{run1}"),
        None,
    );
    assert_eq!(code, 200);
    assert_eq!(rep_json["run_id"], json!(run1));

    let (code, head, html_bytes) = http_req(
        port,
        "GET",
        &format!("/api/projects/{id}/playtests/{run1}/report.html"),
        &format!("127.0.0.1:{port}"),
        None,
    );
    assert_eq!(code, 200);
    assert!(head.contains("text/html"));
    let html = String::from_utf8_lossy(&html_bytes);
    assert!(html.contains("Passed"));
    assert!(html.contains("shots/hub.png"));

    let (code, head, png_bytes) = http_req(
        port,
        "GET",
        &format!("/api/projects/{id}/playtests/{run1}/shots/hub.png"),
        &format!("127.0.0.1:{port}"),
        None,
    );
    assert_eq!(code, 200);
    assert!(head.contains("image/png"));
    assert!(png_bytes.starts_with(&[0x89, b'P', b'N', b'G']));

    // 6. Playtest Runner (failing check + console error)
    let (code, pt2) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/playtest"),
        Some(json!({
            "mode": "start_play",
            "settle_ms": 0,
            "checks": [
                { "name": "failing assert", "luau": "return 1", "expect": { "equals": 2 } },
                { "name": "runtime boom", "luau": "error(\"nil card index\")" }
            ]
        })),
    );
    assert_eq!(code, 200);
    assert_eq!(pt2["ok"], json!(true));
    assert_eq!(pt2["passed"], json!(false));
    assert_eq!(pt2["checks"][0]["passed"], json!(false));
    assert_eq!(pt2["checks"][1]["passed"], json!(false));
    assert!(!pt2["console"]["errors"].as_array().unwrap().is_empty());

    // 7. Subsequent clean run MUST NOT inherit stale console errors from pt2,
    //    and `allow_errors` filters expected warnings/errors
    let (code, pt3) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/playtest"),
        Some(json!({
            "mode": "start_play",
            "settle_ms": 0,
            "allow_errors": ["allowed expected error"],
            "checks": [
                { "name": "clean check", "luau": "print(\"allowed expected error\")\nreturn \"ok\"", "expect": { "contains": "ok" } }
            ]
        })),
    );
    assert_eq!(code, 200);
    assert_eq!(pt3["passed"], json!(true), "stale error leaked or allow_errors failed: {pt3}");
    assert_eq!(pt3["console"]["errors"].as_array().unwrap().len(), 0);

    let (code, runs) = api_json(port, "GET", &format!("/api/projects/{id}/playtests"), None);
    assert_eq!(code, 200);
    assert_eq!(runs["runs"].as_array().unwrap().len(), 3);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_rejects_unknown_flags_and_documents_data_dir() {
    let bin = env!("CARGO_BIN_EXE_openro-rs");
    let help = Command::new(bin).arg("--help").output().expect("run --help");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--data-dir"));

    let bad = Command::new(bin)
        .arg("--bogus-flag")
        .output()
        .expect("run --bogus-flag");
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unknown option"));
}

#[test]
fn file_delete_refuses_project_root_and_dot_variants() {
    let root = temp_dir("dot-delete");
    let ws = root.join("ws");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    let port = start_server(None, data);

    api_json(
        port,
        "POST",
        "/api/projects/workspace",
        Some(json!({ "path": ws.to_string_lossy() })),
    );
    let (code, created) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "Dot Test", "parent": ws.to_string_lossy() })),
    );
    assert_eq!(code, 201);
    let id = created["project"]["id"].as_str().unwrap().to_string();
    let proj_dir = ws.join("dot-test");

    for bad in [".", "./", "./.", "", "./openro.project.json", "./build/out.rbxlx", "./.openro/x"] {
        let (code, _) = api_json(
            port,
            "POST",
            &format!("/api/projects/{id}/file/delete"),
            Some(json!({ "path": bad })),
        );
        assert_eq!(code, 400, "expected 400 deleting '{bad}'");
    }

    // Project files MUST all remain completely intact!
    assert!(proj_dir.join("default.project.json").is_file());
    assert!(proj_dir.join("openro.project.json").is_file());
    assert!(proj_dir.join("src/server/Main.server.luau").is_file());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn place_cylinder_upright_content_xml_and_media_in_assets_and_bundle() {
    let root = temp_dir("gap11");
    let data = root.join("data");
    let local = openro_rs::place::LocalPlace::open(data.clone(), "Gap11".into());
    let agent = Agent::with_local_place(
        ModelClient::new("http://127.0.0.1:0".into(), "test".into(), None),
        local,
    );
    let port = start_server(Some(agent), data.clone());

    // 1. Cylinder preset exports upright (Size=(H,D,D) + Z+90deg CFrame)
    let (code, cyl) = api_json(
        port,
        "POST",
        "/api/place/create",
        Some(json!({ "preset": "cylinder", "name": "Pillar", "size": [2, 16, 2] })),
    );
    assert_eq!(code, 200);
    assert_eq!(cyl["instance"]["properties"]["Size"], json!([16.0, 2.0, 2.0]));
    assert_eq!(cyl["instance"]["properties"]["Rotation"], json!([0.0, 0.0, 90.0]));

    // 2. MeshPart / Sound / ParticleEmitter / SurfaceAppearance emit <Content><url>
    let mut place = openro_rs::place::LocalPlace::in_memory("ContentTest".into());
    place.replace_tree(&json!({
        "scripts": {},
        "instances": {
            "Workspace.Lantern": {
                "class_name": "MeshPart",
                "properties": {
                    "MeshId": "rbxassetid://111",
                    "TextureID": "rbxassetid://222"
                }
            },
            "SoundService.Hit": {
                "class_name": "Sound",
                "properties": {
                    "SoundId": "rbxassetid://333"
                }
            },
            "Workspace.Lantern.Sparks": {
                "class_name": "ParticleEmitter",
                "properties": {
                    "Texture": "rbxassetid://444"
                }
            },
            "Workspace.Lantern.Mat": {
                "class_name": "SurfaceAppearance",
                "properties": {
                    "ColorMap": "rbxassetid://555",
                    "RoughnessMap": "rbxassetid://666"
                }
            }
        }
    }));
    let xml = place.to_rbxlx_xml();
    assert!(xml.contains("<Content name=\"MeshId\"><url>rbxassetid://111</url></Content>"));
    assert!(xml.contains("<Content name=\"TextureID\"><url>rbxassetid://222</url></Content>"));
    assert!(xml.contains("<Content name=\"SoundId\"><url>rbxassetid://333</url></Content>"));
    assert!(xml.contains("<Content name=\"Texture\"><url>rbxassetid://444</url></Content>"));
    assert!(xml.contains("<Content name=\"ColorMap\"><url>rbxassetid://555</url></Content>"));
    assert!(xml.contains("<Content name=\"RoughnessMap\"><url>rbxassetid://666</url></Content>"));

    // Verify exported rbxlx from HTTP has upright cylinder Z+90 CFrame
    let (code, _, xml_bytes) = http_req(
        port,
        "GET",
        "/api/export/rbxlx",
        &format!("127.0.0.1:{port}"),
        None,
    );
    assert_eq!(code, 200);
    let srv_xml = String::from_utf8_lossy(&xml_bytes);
    assert!(srv_xml.contains("<token name=\"shape\">2</token>"));
    assert!(srv_xml.contains("<Vector3 name=\"size\"><X>16</X><Y>2</Y><Z>2</Z></Vector3>"));
    assert!(srv_xml.contains("<R00>0</R00><R01>-1</R01>"));
    assert!(srv_xml.contains("<R10>1</R10><R11>0</R11>"));

    // 3. Generated meshes & audio appear in /api/assets, /api/assets/manifest, and /api/export/bundle
    std::fs::create_dir_all(data.join("game-assets/meshes")).unwrap();
    std::fs::create_dir_all(data.join("game-assets/audio")).unwrap();
    std::fs::write(data.join("game-assets/meshes/shrine.obj"), "v 0 0 0\n").unwrap();
    std::fs::write(data.join("game-assets/audio/hit.wav"), b"RIFF0000WAVE").unwrap();

    let (code, assets) = api_json(port, "GET", "/api/assets", None);
    assert_eq!(code, 200);
    assert_eq!(assets["count"], json!(2));

    let (code, manifest) = api_json(port, "GET", "/api/assets/manifest", None);
    assert_eq!(code, 200);
    assert_eq!(manifest["asset_count"], json!(2));

    let (code, _, zip_bytes) = http_req(
        port,
        "GET",
        "/api/export/bundle",
        &format!("127.0.0.1:{port}"),
        None,
    );
    assert_eq!(code, 200);
    let zip_str = String::from_utf8_lossy(&zip_bytes);
    assert!(zip_str.contains("assets/meshes/shrine.obj"));
    assert!(zip_str.contains("assets/audio/hit.wav"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn luau_check_warns_unrotated_cylinder_and_loop_vfx() {
    let bad_cyl = r#"
local p = Instance.new("Part")
p.Shape = Enum.PartType.Cylinder
p.Size = Vector3.new(2, 16, 2)
p.Parent = workspace
"#;
    let rep = openro_rs::luau_check::analyze_script("ServerScriptService.BadPillar", bad_cyl);
    assert!(rep.is_ok(), "warnings should not block syntax");
    assert!(rep.has_code("UPRIGHT_CYLINDER_AXIS"), "expected UPRIGHT_CYLINDER_AXIS: {:?}", rep.diagnostics);

    let good_cyl = r#"
local p = Instance.new("Part")
p.Shape = Enum.PartType.Cylinder
p.Size = Vector3.new(16, 2, 2)
p.CFrame = CFrame.new(0, 8, 0) * CFrame.Angles(0, 0, math.rad(90))
p.Parent = workspace
"#;
    let rep_ok = openro_rs::luau_check::analyze_script("ServerScriptService.GoodPillar", good_cyl);
    assert!(!rep_ok.has_code("UPRIGHT_CYLINDER_AXIS"));

    let bad_vfx = r#"
while true do
    task.wait(0.2)
    local em = Instance.new("ParticleEmitter")
    em.Parent = workspace
end
"#;
    let rep_vfx = openro_rs::luau_check::analyze_script("ServerScriptService.BadVfx", bad_vfx);
    assert!(rep_vfx.has_code("UNBOUNDED_LOOP_VFX"), "expected UNBOUNDED_LOOP_VFX: {:?}", rep_vfx.diagnostics);

    let good_vfx = r#"
local Debris = game:GetService("Debris")
while true do
    task.wait(0.2)
    local em = Instance.new("ParticleEmitter")
    em.Parent = workspace
    Debris:AddItem(em, 1)
end
"#;
    let rep_vfx_ok = openro_rs::luau_check::analyze_script("ServerScriptService.GoodVfx", good_vfx);
    assert!(!rep_vfx_ok.has_code("UNBOUNDED_LOOP_VFX"));
}

#[cfg(unix)]
#[test]
fn empty_studio_mcp_falls_back_to_local_place_and_auto_plugin_pref() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_dir("gap24-29");
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();

    // Wrapper invoking real mock_studio with MOCK_NO_STUDIOS=1 (simulating Vinegar at Sign-In screen)
    let wrapper = root.join("empty_studio_mcp.sh");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\nMOCK_NO_STUDIOS=1 exec \"{}\"\n", mock_studio_command()),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();

    let local = openro_rs::place::LocalPlace::open(data.clone(), "Gap24".into());
    let agent = Agent::with_local_place(
        ModelClient::new("http://127.0.0.1:0".into(), "test".into(), None),
        local,
    );
    let port = start_server(Some(agent), data.clone());

    // 1. Auto mode with an empty-studio MCP bridge MUST fall back to local_place (not false studio_mcp)
    let (code, cfg) = api_json(
        port,
        "POST",
        "/api/config",
        Some(json!({
            "workspace_mode": "auto",
            "mcp_command": wrapper.to_string_lossy(),
            "auto_install_plugin": false
        })),
    );
    assert_eq!(code, 200);
    assert_eq!(cfg["active_backend"], json!("local_place"));
    assert_eq!(cfg["auto_install_plugin"], json!(false));

    // Verify plugin status reflects persisted auto_install_plugin=false
    let (code, pst) = api_json(port, "GET", "/api/studio/plugin/status", None);
    assert_eq!(code, 200);
    assert_eq!(pst["auto_install_plugin"], json!(false));

    // 2. Explicit MCP connect against empty-studio bridge returns 502 with plain-words message
    let (code, err) = api_json(
        port,
        "POST",
        "/api/studio/mcp/connect",
        Some(json!({ "command": wrapper.to_string_lossy() })),
    );
    assert_eq!(code, 502);
    assert!(err["error"].as_str().unwrap().contains("no open Roblox Studio place"));

    // 3. Connecting a live mock_studio succeeds and launch-and-continue reports already_connected + ready
    let (code, ok) = api_json(
        port,
        "POST",
        "/api/studio/mcp/connect",
        Some(json!({ "command": mock_studio_command() })),
    );
    assert_eq!(code, 200);
    assert_eq!(ok["connected"], json!(true));

    let (code, lc) = api_json(
        port,
        "POST",
        "/api/studio/launch-and-continue",
        Some(json!({})),
    );
    assert_eq!(code, 200);
    assert_eq!(lc["already_connected"], json!(true));
    assert_eq!(lc["ready"], json!(true));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn place_tree_and_config_never_block_while_agent_mutex_is_held_and_config_validates_keys() {
    let root = temp_dir("gap31-32");
    let data = root.join("data");
    let mut local = openro_rs::place::LocalPlace::open(data.clone(), "Gap31".into());
    local.replace_tree(&json!({
        "scripts": { "ServerScriptService.Boot": "print('hi')" },
        "instances": {}
    }));
    let agent = Agent::with_local_place(
        ModelClient::new("http://127.0.0.1:0".into(), "test".into(), None),
        local,
    );
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(Some(agent), "Gap31".to_string(), data);
    let shared_srv = std::sync::Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, shared_srv);
    });

    // Warm the cached place tree
    let (code, t0) = api_json(port, "GET", "/api/place-tree", None);
    assert_eq!(code, 200);
    assert!(t0["scripts"]["ServerScriptService.Boot"].is_string());

    // Hold shared.agent mutex to simulate a long /api/chat model turn (#31)
    let hold_guard = shared.agent.lock().unwrap();
    let started = std::time::Instant::now();
    let (code, t_busy) = api_json(port, "GET", "/api/place-tree", None);
    let (code_cfg, cfg_busy) = api_json(port, "GET", "/api/config", None);
    let elapsed = started.elapsed();
    drop(hold_guard);

    assert_eq!(code, 200);
    assert_eq!(t_busy["cached"], json!(true));
    assert_eq!(t_busy["busy"], json!(true));
    assert!(t_busy["scripts"]["ServerScriptService.Boot"].is_string());
    assert_eq!(code_cfg, 200);
    assert_eq!(cfg_busy["ok"], json!(true));
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "reads blocked while agent mutex held: {elapsed:?}"
    );

    // #32: active_backend alias works, unknown keys fail with 400
    let (code, res) = api_json(
        port,
        "POST",
        "/api/config",
        Some(json!({ "active_backend": "local_place" })),
    );
    assert_eq!(code, 200);
    assert_eq!(res["workspace_mode"], json!("local_place"));
    assert_eq!(res["active_backend"], json!("local_place"));

    let (code, err) = api_json(
        port,
        "POST",
        "/api/config",
        Some(json!({ "not_a_real_setting": true })),
    );
    assert_eq!(code, 400);
    assert!(err["error"]
        .as_str()
        .unwrap()
        .contains("unknown setting key 'not_a_real_setting'"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn embed_status_detection_helpers_and_attach_existing_studio() {
    // 1. Unit checks for display normalization & running-studio parser (#118)
    assert_eq!(
        openro_rs::studio_embed::normalize_x11_display_env(":99.0"),
        Some(":99".to_string())
    );
    assert_eq!(
        openro_rs::studio_embed::normalize_x11_display_env(":0"),
        Some(":0".to_string())
    );
    assert_eq!(openro_rs::studio_embed::normalize_x11_display_env(""), None);
    assert_eq!(
        openro_rs::studio_embed::normalize_x11_display_env("invalid"),
        None
    );

    let parsed = openro_rs::studio_embed::parse_running_studios(&json!({
        "studios": [
            { "studio_id": "studio-abc", "place_name": "UACG Lobby", "active_datamodel": "Edit" },
            { "studio_id": "" }
        ]
    }));
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["studio_id"], json!("studio-abc"));
    assert_eq!(parsed[0]["place_name"], json!("UACG Lobby"));
    assert_eq!(parsed[0]["datamodel_type"], json!("Edit"));

    // 2. Server embed status + attach existing studio without spawning duplicate (#118)
    let root = temp_dir("embed-attach");
    let data = root.join("data");
    let port = start_server(None, data);

    let (code, st) = api_json(port, "GET", "/api/studio/embed/status", None);
    assert_eq!(code, 200);
    assert_eq!(st["ok"], json!(true));
    assert!(st["studio_running"].is_boolean());
    assert!(st["already_logged_in"].is_boolean());
    assert!(st["running_studios"].is_array());
    assert_eq!(st["awaiting_signin"], json!(false));

    // Loopback guard on /api/studio/embed/attach
    let (evil_code, _, _) = http_req(
        port,
        "POST",
        "/api/studio/embed/attach",
        "evil.example:8077",
        Some("{}"),
    );
    assert_eq!(evil_code, 403);

    // Attach to existing mock_studio immediately (zero Studio process spawned)
    let (code, att) = api_json(
        port,
        "POST",
        "/api/studio/embed/attach",
        Some(json!({ "mcp_command": mock_studio_command() })),
    );
    assert_eq!(code, 200, "embed/attach failed: {att}");
    assert_eq!(att["ok"], json!(true));
    assert_eq!(att["running"], json!(true));
    assert_eq!(att["connected"], json!(true));
    assert_eq!(att["awaiting_signin"], json!(false));
    assert_eq!(att["phase"], json!("connected"));
    assert_eq!(att["status"]["already_logged_in"], json!(true));
    assert!(att["status"]["studio_pid"].is_null());
    assert!(
        att["status"]["running_studios"]
            .as_array()
            .is_some_and(|a| !a.is_empty()),
        "expected non-empty running_studios after attach: {att}"
    );

    let (code, stopped) = api_json(port, "POST", "/api/studio/embed/stop", Some(json!({})));
    assert_eq!(code, 200);
    assert_eq!(stopped["running"], json!(false));

    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn embed_live_auth_streams_awaiting_signin_and_auto_promotes_on_login() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_dir("embed-live-auth");
    let data = root.join("data");
    let marker = root.join("signed_in.flag");
    let wrapper = root.join("studio_auth_mock.sh");
    let mock_bin = env!("CARGO_BIN_EXE_mock_studio");
    let script = format!(
        "#!/bin/sh\nif [ ! -f '{}' ]; then\n  export MOCK_NO_STUDIOS=1\nfi\nexec '{}'\n",
        marker.display(),
        mock_bin
    );
    std::fs::write(&wrapper, script).expect("write wrapper");
    let mut perms = std::fs::metadata(&wrapper).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&wrapper, perms).unwrap();

    let port = start_server(None, data);

    // 1. Before sign-in (marker absent): /embed/attach returns 200 awaiting_signin (never 502)
    let (code, resp) = api_json(
        port,
        "POST",
        "/api/studio/embed/attach",
        Some(json!({ "mcp_command": wrapper.to_string_lossy() })),
    );
    assert_eq!(code, 200, "expected 200 awaiting_signin: {resp}");
    assert_eq!(resp["ok"], json!(true));
    assert_eq!(resp["running"], json!(true));
    assert_eq!(resp["connected"], json!(false));
    assert_eq!(resp["awaiting_signin"], json!(true));
    assert_eq!(resp["phase"], json!("awaiting_signin"));
    assert_eq!(resp["status"]["state"], json!("awaiting_signin"));
    assert_eq!(resp["status"]["phase"], json!("awaiting_signin"));
    assert_eq!(resp["status"]["mcp_connected"], json!(false));
    assert_eq!(resp["stream_url"], json!("/api/studio/stream.mjpg"));

    // 2. Simulate user finishing sign-in / opening a place -> background poller auto-attaches MCP
    std::fs::write(&marker, "logged-in").expect("write sign-in marker");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    let mut promoted = None;
    while std::time::Instant::now() < deadline {
        let (st_code, st) = api_json(port, "GET", "/api/studio/embed/status", None);
        if st_code == 200 && st["phase"] == json!("connected") && st["mcp_connected"] == json!(true)
        {
            promoted = Some(st);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    let st = promoted.expect("background MCP auto-attacher did not promote awaiting_signin -> connected");
    assert_eq!(st["state"], json!("running"));
    assert_eq!(st["awaiting_signin"], json!(false));
    assert_eq!(st["already_logged_in"], json!(true));
    assert_eq!(st["studio_id"], json!("mock-studio-01"));
    assert_eq!(st["has_live_frame"], json!(true));
    assert!(
        st["running_studios"]
            .as_array()
            .is_some_and(|a| !a.is_empty()),
        "expected running_studios populated after auto-attach: {st}"
    );

    // 3. Stop cleans up session cleanly
    let (code, stopped) = api_json(port, "POST", "/api/studio/embed/stop", Some(json!({})));
    assert_eq!(code, 200);
    assert_eq!(stopped["running"], json!(false));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn start_page_detection_reports_place_open_false_and_hints_sync() {
    use openro_rs::studio_embed::infer_place_open_from_state;

    // 1. Unit checks across all real/wrapped Start Page vs open-place responses (#47)
    assert!(!infer_place_open_from_state(Ok(&json!({
        "mode": "Unavailable",
        "state": "Place is not open"
    })))
    .0);
    assert!(!infer_place_open_from_state(Ok(&json!({
        "ok": true,
        "output": "Place is not open"
    })))
    .0);
    assert!(!infer_place_open_from_state(Err("Studio bridge error: Place is not open")).0);
    assert!(!infer_place_open_from_state(Ok(&json!({
        "datamodel_types": []
    })))
    .0);
    assert!(infer_place_open_from_state(Ok(&json!({
        "mode": "edit",
        "active_datamodel": "Edit",
        "datamodel_types": ["Edit"]
    })))
    .0);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_dir("start-page-detect");
        let data = root.join("data");
        let ws = root.join("ws");
        std::fs::create_dir_all(&ws).unwrap();

        let mock_bin = env!("CARGO_BIN_EXE_mock_studio");
        let wrapper = root.join("start_page_mock.sh");
        let script = format!("#!/bin/sh\nexport MOCK_START_PAGE=1\nexec '{mock_bin}'\n");
        std::fs::write(&wrapper, script).unwrap();
        let mut perms = std::fs::metadata(&wrapper).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&wrapper, perms).unwrap();

        // 2. Passive probe_running_studios flags place_open: false on Start Page
        let probed = openro_rs::studio_embed::probe_running_studios(&wrapper.to_string_lossy());
        assert_eq!(probed.len(), 1, "expected 1 running studio: {probed:?}");
        assert_eq!(probed[0]["place_open"], json!(false));
        assert_eq!(probed[0]["datamodel_type"], json!("Unavailable"));
        assert_eq!(probed[0]["studio_state"], json!("Place is not open"));

        // 3. Attach via HTTP -> status reports already_logged_in: true, place_open: false, Start Page guidance
        let port = start_server(None, data);
        let (code, _) = api_json(port, "POST", "/api/projects/workspace", Some(json!({ "path": ws })));
        assert_eq!(code, 200);
        let (code, created) = api_json(
            port,
            "POST",
            "/api/projects/create",
            Some(json!({ "name": "StartPageTest", "parent": ws })),
        );
        assert_eq!(code, 201);
        let pid = created["project"]["id"].as_str().unwrap().to_string();

        let (code, att) = api_json(
            port,
            "POST",
            "/api/studio/embed/attach",
            Some(json!({ "mcp_command": wrapper.to_string_lossy() })),
        );
        assert_eq!(code, 200, "attach failed: {att}");
        assert_eq!(att["place_open"], json!(false));
        assert_eq!(att["status"]["already_logged_in"], json!(true));
        assert_eq!(att["status"]["place_open"], json!(false));
        assert!(
            att["status"]["guidance"]
                .as_str()
                .unwrap_or("")
                .contains("Studio is on the Start Page"),
            "missing Start Page guidance: {att}"
        );

        // 4. sync/diff surfaces clear Start Page hint pointing to open-in-studio
        let (code, diff_err) = api_json(
            port,
            "POST",
            &format!("/api/projects/{pid}/sync/diff"),
            Some(json!({})),
        );
        assert_eq!(code, 502);
        let err_msg = diff_err["error"].as_str().unwrap_or("");
        assert!(err_msg.contains("Place is not open"), "{err_msg}");
        assert!(err_msg.contains("open-in-studio"), "{err_msg}");

        let _ = api_json(port, "POST", "/api/studio/embed/stop", Some(json!({})));
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[test]
fn project_open_in_studio_builds_and_opens_rbxlx_with_dry_run_and_reuse() {
    let root = temp_dir("open-in-studio");
    let data = root.join("data");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).unwrap();

    let port = start_server(None, data);
    let (code, _) = api_json(port, "POST", "/api/projects/workspace", Some(json!({ "path": ws })));
    assert_eq!(code, 200);
    let (code, created) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "OpenStudioGame", "parent": ws })),
    );
    assert_eq!(code, 201);
    let pid = created["project"]["id"].as_str().unwrap().to_string();

    // 1. Non-loopback refused 403
    let (evil_code, _, _) = http_req(
        port,
        "POST",
        &format!("/api/projects/{pid}/open-in-studio"),
        "evil.example:8077",
        Some("{}"),
    );
    assert_eq!(evil_code, 403);

    // 2. dry_run builds fresh .rbxlx without spawning Studio
    let (code, dry1) = api_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/open-in-studio"),
        Some(json!({ "dry_run": true, "engine": "builtin" })),
    );
    assert_eq!(code, 200, "open-in-studio dry_run failed: {dry1}");
    assert_eq!(dry1["ok"], json!(true));
    assert_eq!(dry1["rebuilt"], json!(true));
    assert_eq!(dry1["launched"], json!(false));
    assert_eq!(dry1["dry_run"], json!(true));
    let build_id = dry1["build_id"].as_str().unwrap().to_string();
    let rbxlx_path = dry1["rbxlx_path"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&rbxlx_path).is_file(), "missing built rbxlx: {rbxlx_path}");
    let cmd_arr = dry1["command"].as_array().unwrap();
    assert!(!cmd_arr.is_empty());
    #[cfg(target_os = "linux")]
    {
        let win_path = dry1["windows_path"].as_str().unwrap_or("");
        assert!(
            win_path.starts_with("C:\\users\\")
                && win_path.contains("\\Documents\\OpenRO\\")
                && win_path.ends_with(".rbxlx"),
            "expected C:\\users\\<user>\\Documents\\OpenRO\\... Windows path on Linux Vinegar: {dry1}"
        );
        assert!(
            cmd_arr.iter().any(|v| v.as_str() == Some(win_path)),
            "expected Vinegar command to pass Windows path '{win_path}', not Unix path: {cmd_arr:?}"
        );
        assert!(
            !cmd_arr.iter().any(|v| v.as_str() == Some("@@")),
            "must not pass broken --file-forwarding @@ Unix path to Vinegar: {cmd_arr:?}"
        );
    }

    // 2b. Unit verification of Vinegar drive_c staging, blank baseplate synthesis, & duplicate guard (#50, #51)
    let fake_vinegar = root.join("fake-vinegar");
    let (staged, win_p) = openro_rs::studio_embed::stage_vinegar_place_file(
        std::path::Path::new(&rbxlx_path),
        &fake_vinegar,
    )
    .expect("stage_vinegar_place_file");
    assert!(staged.is_file(), "staged file missing: {}", staged.display());
    assert!(
        win_p.starts_with("C:\\users\\") && win_p.contains("\\Documents\\OpenRO\\"),
        "unexpected win_p: {win_p}"
    );
    let bp = openro_rs::studio_embed::ensure_blank_baseplate_rbxlx(&root.join("bp"), "Test Place")
        .expect("ensure_blank_baseplate_rbxlx");
    let bp_xml = std::fs::read_to_string(&bp).unwrap();
    assert!(bp_xml.contains("<Item class=\"Workspace\""));
    assert!(bp_xml.contains("Baseplate"));
    assert!(bp_xml.contains("SpawnLocation"));
    assert_eq!(
        openro_rs::studio_embed::sanitize_vinegar_place_filename(std::path::Path::new(
            "/tmp/..my game!.rbxlx"
        )),
        "my_game_.rbxlx"
    );
    let args = openro_rs::studio_embed::build_vinegar_open_args(&win_p);
    assert_eq!(
        args,
        vec![
            "run".to_string(),
            "--command=vinegar".to_string(),
            "org.vinegarhq.Vinegar".to_string(),
            win_p.clone(),
        ]
    );
    // Duplicate guard: when studio_running=true and force=false, NEVER spawn a duplicate Studio
    let dup = openro_rs::studio_embed::open_place_in_studio_inner(
        std::path::Path::new(&rbxlx_path),
        Some(":0"),
        true,
        false,
        Some((true, false)),
    )
    .expect("open_place_in_studio_inner dry_run");
    assert_eq!(dup["already_running"], json!(true));
    assert_eq!(dup["place_open"], json!(false));
    assert_eq!(dup["launched"], json!(false));

    // Also prove non-dry-run duplicate refusal when studio_running=true, force=false (zero spawn, stages copy)
    #[cfg(target_os = "linux")]
    {
        let dup_non_dry = openro_rs::studio_embed::open_place_in_studio_inner(
            std::path::Path::new(&rbxlx_path),
            Some(":0"),
            false,
            false,
            Some((true, false)),
        );
        if let Ok(val) = dup_non_dry {
            assert_eq!(val["launched"], json!(false));
            assert_eq!(val["already_running"], json!(true));
            assert!(val["pid"].is_null());
            assert!(
                val["windows_path"]
                    .as_str()
                    .unwrap_or("")
                    .contains("\\Documents\\OpenRO\\"),
                "expected Documents\\OpenRO windows_path: {val}"
            );
            if let Some(sp) = val["staged_path"].as_str() {
                let _ = std::fs::remove_file(sp);
            }
        }
    }

    // 3. rebuild: false reuses existing built .rbxlx
    let (code, dry2) = api_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/open-in-studio"),
        Some(json!({ "rebuild": false, "dry_run": true })),
    );
    assert_eq!(code, 200, "open-in-studio reuse failed: {dry2}");
    assert_eq!(dry2["rebuilt"], json!(false));
    assert_eq!(dry2["build_id"], json!(build_id));
    assert_eq!(dry2["rbxlx_path"], json!(rbxlx_path));

    // 4. End-to-end launch via OPENRO_OPEN_STUDIO_CMD against isolated openro-rs child binary
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let launched_log = root.join("launched_rbxlx.txt");
        let launcher = root.join("fake_studio_open.sh");
        std::fs::write(
            &launcher,
            format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", launched_log.display()),
        )
        .unwrap();
        let mut perms = std::fs::metadata(&launcher).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&launcher, perms).unwrap();

        let probe_sock = TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral port");
        let child_port = probe_sock.local_addr().unwrap().port();
        drop(probe_sock);

        let child_data = root.join("child-data");
        std::fs::create_dir_all(&child_data).unwrap();
        // Keep the isolated test server strictly on local_place (never probe host Vinegar MCP).
        std::fs::write(
            child_data.join("config.json"),
            r#"{"workspace_mode":"local_place"}"#,
        )
        .unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_openro-rs"))
            .args([
                "--no-open",
                "--port",
                &child_port.to_string(),
                "--data-dir",
                &child_data.to_string_lossy(),
            ])
            .env("OPENRO_NO_OPEN", "1")
            .env("OPENRO_NO_AUTO_MODEL", "1")
            .env("OPENRO_NO_AUTO_PLUGIN", "1")
            .env("OPENRO_NO_STATUS_MCP_PROBE", "1")
            .env("OPENRO_OPEN_STUDIO_CMD", &launcher)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn openro-rs");

        // Wait for child server ready
        let ready_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < ready_deadline {
            if std::net::TcpStream::connect(("127.0.0.1", child_port)).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let (code, _) = api_json(
            child_port,
            "POST",
            "/api/projects/workspace",
            Some(json!({ "path": ws })),
        );
        assert_eq!(code, 200);
        // Pass force: true so test launches cleanly even when desktop Studio is running on this host (#50)
        let (code, res) = api_json(
            child_port,
            "POST",
            &format!("/api/projects/{pid}/open-in-studio"),
            Some(json!({ "rebuild": false, "force": true })),
        );
        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(code, 200, "child open-in-studio failed: {res}");
        assert_eq!(res["launched"], json!(true));
        assert!(res["pid"].as_u64().unwrap_or(0) > 0);

        // Wait briefly for tiny shell script to finish writing launched_rbxlx.txt
        let write_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !launched_log.exists() && std::time::Instant::now() < write_deadline {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let recorded = std::fs::read_to_string(&launched_log).unwrap_or_default();
        assert!(
            recorded.ends_with(".rbxlx"),
            "launcher did not receive built .rbxlx path: '{recorded}'"
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn embed_transfer_and_sync_auto_open_against_mock_studio() {
    let root = temp_dir("embed-transfer-auto-open");
    let data = root.join("data");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).unwrap();

    let port = start_server(None, data);
    let (code, _) = api_json(port, "POST", "/api/projects/workspace", Some(json!({ "path": ws })));
    assert_eq!(code, 200);
    let (code, created) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "TransferGame", "parent": ws })),
    );
    assert_eq!(code, 201);
    let pid = created["project"]["id"].as_str().unwrap().to_string();

    // 1. /api/studio/embed/transfer enforces loopback 403 and rejects '..' traversal
    let (evil_code, _, _) = http_req(
        port,
        "POST",
        "/api/studio/embed/transfer",
        "evil.example:8077",
        Some("{}"),
    );
    assert_eq!(evil_code, 403);
    let (bad_code, bad_body) = api_json(
        port,
        "POST",
        "/api/studio/embed/transfer",
        Some(json!({ "place_file": "../escape.rbxlx" })),
    );
    assert_eq!(bad_code, 400, "expected 400 traversal refusal: {bad_body}");

    // 2. /api/studio/embed/transfer attaches mock_studio with transferred=true & place_open=true
    let (code, tr) = api_json(
        port,
        "POST",
        "/api/studio/embed/transfer",
        Some(json!({
            "project_id": pid,
            "transfer": true,
            "mcp_command": mock_studio_command(),
        })),
    );
    assert_eq!(code, 200, "embed/transfer failed: {tr}");
    assert_eq!(tr["ok"], json!(true));
    assert_eq!(tr["transferred"], json!(true));
    assert_eq!(tr["connected"], json!(true));
    assert_eq!(tr["place_open"], json!(true));
    assert_eq!(tr["phase"], json!("connected"));
    assert_eq!(tr["studio_id"], json!("mock-studio-01"));
    assert_eq!(tr["has_live_frame"], json!(true));
    assert_eq!(tr["status"]["transferred"], json!(true));
    assert!(tr["status"]["can_transfer"].is_boolean());

    let (code, _) = api_json(port, "POST", "/api/studio/embed/stop", Some(json!({})));
    assert_eq!(code, 200);

    // 3. Isolated child binary: sync/diff respects auto_open:false (503) and auto-attaches when auto_open:true (default)
    let probe_sock = TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral port");
    let child_port = probe_sock.local_addr().unwrap().port();
    drop(probe_sock);

    let child_data = root.join("child-data");
    std::fs::create_dir_all(&child_data).unwrap();
    std::fs::write(
        child_data.join("config.json"),
        r#"{"workspace_mode":"local_place"}"#,
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_openro-rs"))
        .args([
            "--no-open",
            "--port",
            &child_port.to_string(),
            "--data-dir",
            &child_data.to_string_lossy(),
        ])
        .env("OPENRO_NO_OPEN", "1")
        .env("OPENRO_NO_AUTO_MODEL", "1")
        .env("OPENRO_NO_AUTO_PLUGIN", "1")
        .env("OPENRO_NO_STATUS_MCP_PROBE", "1")
        .env("OPENRO_AUTO_OPEN_MCP_CMD", mock_studio_command())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn openro-rs");

    let ready_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < ready_deadline {
        if std::net::TcpStream::connect(("127.0.0.1", child_port)).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let (code, _) = api_json(
        child_port,
        "POST",
        "/api/projects/workspace",
        Some(json!({ "path": ws })),
    );
    assert_eq!(code, 200);

    // auto_open: false opts out cleanly -> 503
    let (code_off, _) = api_json(
        child_port,
        "POST",
        &format!("/api/projects/{pid}/sync/diff"),
        Some(json!({ "auto_open": false })),
    );
    assert_eq!(code_off, 503);

    // default auto_open: true auto-attaches mock_studio and sync/diff succeeds 200 on first call!
    let (code_on, diff) = api_json(
        child_port,
        "POST",
        &format!("/api/projects/{pid}/sync/diff"),
        Some(json!({})),
    );
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(code_on, 200, "sync/diff auto_open failed: {diff}");
    assert_eq!(diff["ok"], json!(true));
    assert_eq!(diff["dry_run"], json!(true));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn project_format_and_check_auto_format_gap_56() {
    let root = temp_dir("format-gap56");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    let port = start_server(None, data);

    let (code, _) = api_json(
        port,
        "POST",
        "/api/projects/workspace",
        Some(json!({ "path": ws.to_string_lossy() })),
    );
    assert_eq!(code, 200);

    let (code, created) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "Format Demo", "parent": ws.to_string_lossy() })),
    );
    assert_eq!(code, 201);
    let id = created["project"]["id"].as_str().unwrap().to_string();
    let proj_dir = PathBuf::from(created["project"]["path"].as_str().unwrap());
    std::fs::create_dir_all(proj_dir.join(".openro")).unwrap();

    // Non-loopback host refused on /format
    let (code, _, _) = http_req(
        port,
        "POST",
        &format!("/api/projects/{id}/format"),
        "evil.example.com",
        Some("{}"),
    );
    assert_eq!(code, 403);

    // Write two intentionally misformatted (but semantically valid) Luau scripts
    let messy_a = "local   x={1,2,3}\nreturn   x\n";
    let messy_b = "local   y={4,5,6}\nreturn   y\n";
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/file"),
        Some(json!({ "path": "src/shared/MessyA.luau", "content": messy_a })),
    );
    assert_eq!(code, 200);
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/file"),
        Some(json!({ "path": "src/shared/MessyB.luau", "content": messy_b })),
    );
    assert_eq!(code, 200);

    // Empty body on /check stays backward-compatible (format defaults to false)
    let (code, _, bytes) = http_req(
        port,
        "POST",
        &format!("/api/projects/{id}/check"),
        &format!("127.0.0.1:{port}"),
        None,
    );
    assert_eq!(code, 200);
    let chk0: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(chk0["ok"], json!(true));
    assert_eq!(chk0["formatted"], json!(false));
    assert!(chk0.get("format").is_none());
    assert_eq!(chk0["luau"]["passed"], json!(true), "luau failed: {chk0}");
    assert_eq!(chk0["selene"]["passed"], json!(true), "selene failed: {chk0}");

    let has_stylua = projects::find_tool("stylua", "OPENRO_STYLUA").is_some();
    if has_stylua {
        assert_eq!(chk0["stylua"]["passed"], json!(false), "expected unformatted failure: {chk0}");
        assert_eq!(chk0["passed"], json!(false));
    }

    // Fail-closed validation on bad options & path escapes
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/check"),
        Some(json!({ "format": "yes" })),
    );
    assert_eq!(code, 400);
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/check"),
        Some(json!({ "format": true, "paths": 123 })),
    );
    assert_eq!(code, 400);
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/check"),
        Some(json!({ "format": true, "paths": ["../escape.luau"] })),
    );
    assert_eq!(code, 400);

    for bad_path in [
        "../escape.luau",
        "/etc/passwd",
        "--check",
        "openro.project.json",
        "src/shared/DoesNotExist.luau",
        ".openro",
    ] {
        let (code, body) = api_json(
            port,
            "POST",
            &format!("/api/projects/{id}/format"),
            Some(json!({ "paths": [bad_path] })),
        );
        assert_eq!(code, 400, "expected 400 for '{bad_path}', got {body}");
    }

    // Scoped POST /api/projects/{id}/format formats ONLY MessyA.luau, leaving MessyB.luau untouched
    let (code, fmt_a) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/format"),
        Some(json!({ "paths": ["src/shared/MessyA.luau"] })),
    );
    assert_eq!(code, 200, "scoped format failed: {fmt_a}");
    assert_eq!(fmt_a["ok"], json!(true));
    assert_eq!(fmt_a["targets"], json!(["src/shared/MessyA.luau"]));
    assert!(fmt_a.get("duration_ms").is_some());
    assert!(fmt_a.get("stdout").is_some());
    assert!(fmt_a.get("stderr").is_some());

    if has_stylua {
        assert_eq!(fmt_a["available"], json!(true));
        assert_eq!(fmt_a["formatted"], json!(true));
        assert_eq!(fmt_a["exit_code"], json!(0));

        let a_after = std::fs::read_to_string(proj_dir.join("src/shared/MessyA.luau")).unwrap();
        let b_after = std::fs::read_to_string(proj_dir.join("src/shared/MessyB.luau")).unwrap();
        assert_eq!(a_after, "local x = { 1, 2, 3 }\nreturn x\n");
        assert_eq!(b_after, messy_b, "MessyB.luau should remain untouched by scoped format");

        // Regular check still catches unformatted MessyB.luau
        let (code, chk_mid) = api_json(
            port,
            "POST",
            &format!("/api/projects/{id}/check"),
            Some(json!({})),
        );
        assert_eq!(code, 200);
        assert_eq!(chk_mid["formatted"], json!(false));
        assert_eq!(chk_mid["stylua"]["passed"], json!(false));
        assert_eq!(chk_mid["passed"], json!(false));
    }

    // POST /api/projects/{id}/check with { format: true } formats remaining scripts before checks
    let (code, chk_fmt) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/check"),
        Some(json!({ "format": true })),
    );
    assert_eq!(code, 200, "check with format:true failed: {chk_fmt}");
    assert_eq!(chk_fmt["ok"], json!(true));
    assert!(chk_fmt.get("format").is_some());
    assert_eq!(chk_fmt["format"]["targets"], json!(["src"]));
    assert_eq!(chk_fmt["luau"]["passed"], json!(true), "luau failed: {chk_fmt}");
    assert_eq!(chk_fmt["selene"]["passed"], json!(true), "selene failed: {chk_fmt}");
    assert_eq!(chk_fmt["stylua"]["passed"], json!(true), "stylua failed: {chk_fmt}");
    assert_eq!(chk_fmt["passed"], json!(true), "overall check failed: {chk_fmt}");

    if has_stylua {
        assert_eq!(chk_fmt["formatted"], json!(true));
        assert_eq!(chk_fmt["format"]["available"], json!(true));
        assert_eq!(chk_fmt["format"]["formatted"], json!(true));
        assert_eq!(chk_fmt["format"]["exit_code"], json!(0));
        let b_formatted = std::fs::read_to_string(proj_dir.join("src/shared/MessyB.luau")).unwrap();
        assert_eq!(b_formatted, "local y = { 4, 5, 6 }\nreturn y\n");
    }

    // Empty body on POST /api/projects/{id}/format formats default luau_target_dirs
    let (code, _, bytes) = http_req(
        port,
        "POST",
        &format!("/api/projects/{id}/format"),
        &format!("127.0.0.1:{port}"),
        None,
    );
    assert_eq!(code, 200);
    let fmt_all: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(fmt_all["ok"], json!(true));
    assert_eq!(fmt_all["targets"], json!(["src"]));
    if has_stylua {
        assert_eq!(fmt_all["formatted"], json!(true));
    }

    // Direct Rust library hooks used by openro-team (#56)
    let direct_chk = projects::check_project(&proj_dir).unwrap();
    assert_eq!(direct_chk["formatted"], json!(false));
    assert_eq!(direct_chk["passed"], json!(true));

    let direct_opt = projects::check_project_with_options(
        &proj_dir,
        true,
        Some(&["src/shared".to_string()]),
    )
    .unwrap();
    assert_eq!(direct_opt["format"]["targets"], json!(["src/shared"]));
    assert_eq!(direct_opt["passed"], json!(true));
    if has_stylua {
        assert_eq!(direct_opt["formatted"], json!(true));
    }
    assert!(projects::format_project(&proj_dir, Some(&["../outside.luau".to_string()])).is_err());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn asset_ids_codegen_passes_project_check_stylua_gap_58() {
    let root = temp_dir("codegen-gap58");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    let port = start_server(None, data);

    let (code, _) = api_json(
        port,
        "POST",
        "/api/projects/workspace",
        Some(json!({ "path": ws.to_string_lossy() })),
    );
    assert_eq!(code, 200);

    let (code, created) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "Codegen Check", "parent": ws.to_string_lossy() })),
    );
    assert_eq!(code, 201);
    let id = created["project"]["id"].as_str().unwrap().to_string();
    let proj_dir = PathBuf::from(created["project"]["path"].as_str().unwrap());

    // Simulate mesh_generate populating assets/meshes/manifest.json before any Open Cloud upload
    std::fs::create_dir_all(proj_dir.join("assets/meshes")).unwrap();
    std::fs::write(
        proj_dir.join("assets/meshes/manifest.json"),
        json!({
            "meshes": {
                "shrine_pedestal": { "size": [6.0, 4.5, 6.0], "tris": 920 }
            }
        })
        .to_string(),
    )
    .unwrap();

    let cg = openro_rs::opencloud::assets_codegen(&proj_dir, &json!({})).expect("assets_codegen");
    assert_eq!(cg["ok"], json!(true));
    assert_eq!(cg["count"], json!(0));
    assert_eq!(cg["mesh_count"], json!(1));

    let generated = std::fs::read_to_string(proj_dir.join("src/shared/AssetIds.luau")).unwrap();
    assert!(generated.contains("AssetIds.byPath = {}\n\n"));
    assert!(generated.contains("AssetIds.byKey = {}\n\n"));
    assert!(!generated.contains("{\n}"));

    // POST /api/projects/{id}/check with format:false (default) passes cleanly with 0 stylua/selene/luau issues
    let (code, chk) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/check"),
        Some(json!({})),
    );
    assert_eq!(code, 200, "check failed: {chk}");
    assert_eq!(chk["formatted"], json!(false));
    assert_eq!(chk["luau"]["passed"], json!(true), "luau failed: {chk}");
    assert_eq!(chk["selene"]["passed"], json!(true), "selene failed: {chk}");
    assert_eq!(chk["stylua"]["passed"], json!(true), "stylua failed: {chk}");
    assert_eq!(chk["passed"], json!(true), "overall check failed: {chk}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn mcp_probe_timeout_resets_to_default_on_attach_and_connected_guidance_is_clean_gap_61() {
    use openro_rs::place::PlaceBackend;
    use openro_rs::studio_embed::{HostStudioDetection, StudioEmbedState};
    use std::time::Duration;

    let default_to = openro_rs::mcp::default_request_timeout();
    assert!(
        default_to >= Duration::from_secs(10),
        "default MCP timeout should not be a short probe timeout: {default_to:?}"
    );

    // 1. Direct McpClient::spawn_with_timeout + reset_default_timeout
    let mut client =
        McpClient::spawn_with_timeout(&mock_studio_command(), Duration::from_secs(3))
            .expect("spawn mock_studio");
    assert_eq!(client.timeout(), Duration::from_secs(3));
    assert!(client.has_live_studio());
    client.reset_default_timeout();
    assert_eq!(client.timeout(), default_to);
    drop(client);

    // 2. Server auto-mode config attach (`server.rs:4110` 4s probe) restores default timeout on PlaceBackend::Mcp
    let root = temp_dir("mcp-timeout-gap61");
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(None, "Timeout Test".to_string(), data);
    let shared_inspect = std::sync::Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });

    let (code, cfg_res) = api_json(
        port,
        "POST",
        "/api/config",
        Some(json!({
            "workspace_mode": "auto",
            "mcp_command": mock_studio_command(),
        })),
    );
    assert_eq!(code, 200, "POST /api/config auto attach failed: {cfg_res}");
    {
        let guard = shared_inspect.agent.lock().unwrap();
        let studio = guard
            .as_ref()
            .and_then(|a| a.studio.as_ref())
            .expect("agent studio attached");
        match studio {
            PlaceBackend::Mcp(m) => assert_eq!(
                m.timeout(),
                default_to,
                "auto-mode attach leaked short probe timeout"
            ),
            _ => panic!("expected PlaceBackend::Mcp after auto-mode attach"),
        }
    }

    // 3. Embed attach + GET /api/studio/embed/status clears stale 'awaiting sign-in or open place' guidance
    let (code, att) = api_json(
        port,
        "POST",
        "/api/studio/embed/attach",
        Some(json!({ "mcp_command": mock_studio_command() })),
    );
    assert_eq!(code, 200, "embed/attach failed: {att}");
    {
        let guard = shared_inspect.agent.lock().unwrap();
        match guard.as_ref().and_then(|a| a.studio.as_ref()) {
            Some(PlaceBackend::Mcp(m)) => assert_eq!(
                m.timeout(),
                default_to,
                "embed attach leaked short probe timeout"
            ),
            _ => panic!("expected PlaceBackend::Mcp after embed attach"),
        }
    }

    let (code, st) = api_json(port, "GET", "/api/studio/embed/status", None);
    assert_eq!(code, 200, "embed/status failed: {st}");
    assert_eq!(st["phase"], json!("connected"));
    assert_eq!(st["place_open"], json!(true));
    let top_guidance = st["guidance"].as_str().unwrap_or("");
    let det_guidance = st["detection"]["guidance"].as_str().unwrap_or("");
    assert!(
        !top_guidance.to_ascii_lowercase().contains("awaiting sign-in"),
        "stale guidance on connected+place_open status: {top_guidance}"
    );
    assert!(
        !det_guidance.to_ascii_lowercase().contains("awaiting sign-in"),
        "stale detection.guidance on connected+place_open status: {det_guidance}"
    );
    assert!(
        top_guidance.to_ascii_lowercase().contains("connected"),
        "expected connected guidance, got: {top_guidance}"
    );

    // 4. StudioEmbedState::to_json_with_detection overrides stale fast-detect guidance when connected & place_open
    let stale_detect = HostStudioDetection {
        platform: "linux",
        studio_installed: true,
        studio_running: true,
        studio_display: Some(":99".to_string()),
        already_logged_in: false,
        place_open: false,
        running_studios: Vec::new(),
        xvfb_available: true,
        display_ready: true,
        studio_exe: Some("flatpak:org.vinegarhq.Vinegar".to_string()),
        mcp_command: Some(mock_studio_command()),
        guidance: "Roblox Studio is running (awaiting sign-in or open place) — click 'Launch & Embed Studio' to stream and sign in live.".to_string(),
    };
    let mut state = StudioEmbedState::idle();
    state.phase = "connected".to_string();
    state.mcp_connected = true;
    state.place_open = true;
    let payload = state.to_json_with_detection(&stale_detect);
    let dg = payload["detection"]["guidance"].as_str().unwrap_or("");
    assert!(
        !dg.to_ascii_lowercase().contains("awaiting sign-in"),
        "to_json_with_detection kept stale guidance: {dg}"
    );
    assert_eq!(payload["detection"]["place_open"], json!(true));
    assert_eq!(payload["detection"]["already_logged_in"], json!(true));

    let _ = api_json(port, "POST", "/api/studio/embed/stop", Some(json!({})));
    let _ = std::fs::remove_dir_all(&root);
}
