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

    // Fix it -> luau passes
    api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/file"),
        Some(json!({
            "path": "src/shared/Broken.luau",
            "content": "local function ok()\n\treturn 1\nend\nreturn ok\n"
        })),
    );
    let (code, chk) = api_json(port, "POST", &format!("/api/projects/{id}/check"), Some(json!({})));
    assert_eq!(code, 200);
    assert_eq!(chk["luau"]["passed"], json!(true));

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
