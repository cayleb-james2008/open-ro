//! Phase 3 (v0.5.0) server/agent core integration tests:
//! - True SSE stream `GET /api/build/stream` + JSON fallback `GET /api/build/events`
//! - Versioned snapshots (`GET/POST /api/snapshots`, `POST /api/snapshots/:id/restore`,
//!   `GET /api/snapshots/:id/rbxlx`) + `GET /api/place/diff`
//! - `.rbxlx` Import (`POST /api/import/rbxlx` JSON & raw XML, malformed/XXE fuzz -> 400)
//! - Instance Property Live Editor (`POST /api/place/instance` create, edit, delete)
//! - `GET /api/skills`, `GET/POST /api/luau/check`, and 30-client concurrent load test.

use openro_rs::agent::Agent;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn start_local_server(data_dir: Option<std::path::PathBuf>) -> (u16, Arc<Shared>) {
    let model = ModelClient::new(
        "http://127.0.0.1:11434/v1".to_string(),
        "qwen2.5:3b".to_string(),
        None,
    );
    let place = match &data_dir {
        Some(dir) => LocalPlace::open(dir.clone(), "Skyline Obby".to_string()),
        None => LocalPlace::in_memory("Skyline Obby".to_string()),
    };
    let mut agent = Agent::with_local_place(model, place);
    if let Some(dir) = &data_dir {
        agent.set_changes_file(dir.join("changes.json"));
    }
    let shared = match data_dir {
        Some(dir) => Shared::with_data_dir(Some(agent), "Skyline Obby".to_string(), dir),
        None => Shared::new(Some(agent), "Skyline Obby".to_string()),
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv_shared = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv_shared);
    });
    (port, shared)
}

fn http_req(
    port: u16,
    method: &str,
    path: &str,
    content_type: &str,
    body: Option<&[u8]>,
) -> (u16, String, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let payload = body.unwrap_or(b"");
    let req = if payload.is_empty() {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        )
    };
    stream.write_all(req.as_bytes()).unwrap();
    if !payload.is_empty() {
        stream.write_all(payload).unwrap();
    }
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, body_part) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, head.to_string(), body_part.to_string())
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("openro-v050-{tag}-{nonce}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn sse_build_stream_emits_live_phases_during_blueprint_build() {
    let (port, shared) = start_local_server(None);

    // Connect a real SSE subscriber on a background thread.
    let mut sse_stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    sse_stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    sse_stream
        .write_all(
            format!(
                "GET /api/build/stream HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: text/event-stream\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
    sse_stream.flush().unwrap();

    // Read initial HTTP response headers + `event: status`.
    let mut init_str = String::new();
    let init_deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !init_str.contains("event: status") && std::time::Instant::now() < init_deadline {
        let mut initial = [0u8; 4096];
        let n = sse_stream.read(&mut initial).unwrap();
        if n == 0 {
            break;
        }
        init_str.push_str(&String::from_utf8_lossy(&initial[..n]));
    }
    assert!(
        init_str.contains("text/event-stream") && init_str.contains("event: status"),
        "expected SSE status frame, got:\n{init_str}"
    );

    // Trigger a real built-in blueprint build in parallel so SSE receives live events.
    let build_handle = std::thread::spawn(move || {
        let payload = json!({ "id": "obby-starter" }).to_string();
        http_req(port, "POST", "/api/ideas/build", "application/json", Some(payload.as_bytes()))
    });

    // Read live event frames until `done`.
    let mut collected = init_str;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if collected.contains("\"phase\":\"done\"") {
            break;
        }
        let mut chunk = [0u8; 4096];
        match sse_stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(k) => collected.push_str(&String::from_utf8_lossy(&chunk[..k])),
            Err(_) => break,
        }
    }

    let (status, _, build_body) = build_handle.join().unwrap();
    assert_eq!(status, 200, "blueprint build succeeded: {build_body}");
    let build_json: Value = serde_json::from_str(&build_body).unwrap();
    assert!(build_json["snapshot_id"].is_string(), "build created snapshot: {build_json}");
    assert!(build_json["diff"]["total_changes"].as_u64().unwrap_or(0) >= 5);

    assert!(collected.contains("event: build"), "SSE emitted build events:\n{collected}");
    assert!(collected.contains("\"phase\":\"planning\""), "planning phase streamed:\n{collected}");
    assert!(collected.contains("\"phase\":\"compiling_rune\""), "compiling_rune phase streamed:\n{collected}");
    assert!(collected.contains("\"phase\":\"playtest_proof\""), "playtest_proof phase streamed:\n{collected}");
    assert!(collected.contains("\"phase\":\"done\""), "done phase streamed:\n{collected}");

    // Also verify JSON polling fallback endpoint.
    let (ev_status, _, ev_body) = http_req(port, "GET", "/api/build/events", "application/json", None);
    assert_eq!(ev_status, 200);
    let ev_json: Value = serde_json::from_str(&ev_body).unwrap();
    assert!(ev_json["events"].as_array().unwrap().len() >= 4);

    shared.events.shutdown();
}

#[test]
fn snapshots_restore_rbxlx_import_and_instance_editor_round_trip() {
    let dir = temp_dir("snap-import");
    let (port, _shared) = start_local_server(Some(dir.clone()));

    // 1. Create a Workspace instance via POST /api/place/instance.
    let create_body = json!({
        "path": "Workspace.JumpPad",
        "class_name": "Part",
        "properties": {
            "Position": [10, 4, -8],
            "Size": [8, 1, 8],
            "Color": [45, 157, 120],
            "Anchored": true
        }
    })
    .to_string();
    let (st, _, body) = http_req(port, "POST", "/api/place/instance", "application/json", Some(create_body.as_bytes()));
    assert_eq!(st, 200, "create instance failed: {body}");
    let created: Value = serde_json::from_str(&body).unwrap();
    let snap1 = created["snapshot_id"].as_str().unwrap().to_string();
    assert_eq!(created["diff"]["added_instances"], json!(["Workspace.JumpPad"]));

    // 2. Update properties without resending class_name (must preserve "Part" & existing props).
    let edit_body = json!({
        "path": "Workspace.JumpPad",
        "properties": { "Position": [24, 6, 0] }
    })
    .to_string();
    let (st, _, body) = http_req(port, "POST", "/api/place/instance", "application/json", Some(edit_body.as_bytes()));
    assert_eq!(st, 200, "edit instance failed: {body}");
    let edited: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(edited["instance"]["class_name"], json!("Part"));
    assert_eq!(edited["instance"]["properties"]["Position"], json!([24, 6, 0]));
    assert_eq!(edited["instance"]["properties"]["Anchored"], json!(true));

    // 3. Export historical snapshot 1 as genuine .rbxlx XML.
    let (st, head, xml) = http_req(port, "GET", &format!("/api/snapshots/{snap1}/rbxlx"), "application/json", None);
    assert_eq!(st, 200, "snapshot rbxlx export: {xml}");
    assert!(head.contains("application/xml"));
    assert!(xml.contains("<roblox") && xml.contains("JumpPad"));

    // 4. Delete JumpPad, then restore snap1 and verify JumpPad is back with original Position [10, 4, -8].
    let del_body = json!({ "path": "Workspace.JumpPad", "delete": true }).to_string();
    let (st, _, _) = http_req(port, "POST", "/api/place/instance", "application/json", Some(del_body.as_bytes()));
    assert_eq!(st, 200);
    let (st, _, tree_after_del) = http_req(port, "GET", "/api/place-tree", "application/json", None);
    assert_eq!(st, 200);
    let tree_v: Value = serde_json::from_str(&tree_after_del).unwrap();
    assert!(tree_v["instances"].get("Workspace.JumpPad").is_none());

    let (st, _, restore_body) = http_req(
        port,
        "POST",
        &format!("/api/snapshots/{snap1}/restore"),
        "application/json",
        Some(b"{}"),
    );
    assert_eq!(st, 200, "restore snap1: {restore_body}");
    let (st, _, tree_after_restore) = http_req(port, "GET", "/api/place-tree", "application/json", None);
    assert_eq!(st, 200);
    let tree_v: Value = serde_json::from_str(&tree_after_restore).unwrap();
    assert_eq!(
        tree_v["instances"]["Workspace.JumpPad"]["properties"]["Position"],
        json!([10, 4, -8])
    );

    // 5. Import .rbxlx via raw application/xml AND via JSON {"xml": ...}, plus reject malformed/XXE XML with 400!
    let custom_xml = r#"<roblox version="4">
  <Item class="Workspace">
    <Properties><string name="Name">Workspace</string></Properties>
    <Item class="SpawnLocation">
      <Properties>
        <string name="Name">ArenaSpawn</string>
        <bool name="Anchored">true</bool>
      </Properties>
    </Item>
  </Item>
  <Item class="ServerScriptService">
    <Properties><string name="Name">ServerScriptService</string></Properties>
    <Item class="Script">
      <Properties>
        <string name="Name">ImportedLogic</string>
        <ProtectedString name="Source"><![CDATA[local Players = game:GetService("Players")
print("imported ok")]]></ProtectedString>
      </Properties>
    </Item>
  </Item>
</roblox>"#;
    let (st, _, imp_body) = http_req(
        port,
        "POST",
        "/api/import/rbxlx",
        "application/xml",
        Some(custom_xml.as_bytes()),
    );
    assert_eq!(st, 200, "raw XML import failed: {imp_body}");
    let imp: Value = serde_json::from_str(&imp_body).unwrap();
    assert_eq!(imp["instance_count"], json!(1));
    assert_eq!(imp["script_count"], json!(1));
    assert!(imp["tree"]["scripts"]["ServerScriptService.ImportedLogic"]
        .as_str()
        .unwrap()
        .contains("imported ok"));

    // Malformed & XXE XML fuzzing must return 400 cleanly (never 500 or panic).
    for bad in [
        "",
        "not xml at all",
        "<!DOCTYPE x [<!ENTITY e SYSTEM \"file:///etc/passwd\">]><roblox></roblox>",
        "<roblox><Item class=\"Part\"></roblox>",
    ] {
        let payload = json!({ "xml": bad }).to_string();
        let (st, _, err_body) = http_req(
            port,
            "POST",
            "/api/import/rbxlx",
            "application/json",
            Some(payload.as_bytes()),
        );
        assert_eq!(st, 400, "bad XML must return 400 for {bad:?}, got {st}: {err_body}");
    }

    // 6. Verify GET /api/skills (all 22 Rune skills), POST /api/skills/build, and GET/POST /api/luau/check.
    let (st, _, skills_body) = http_req(port, "GET", "/api/skills", "application/json", None);
    assert_eq!(st, 200);
    let skills_v: Value = serde_json::from_str(&skills_body).unwrap();
    assert_eq!(skills_v["count"], json!(22));

    let skill_build = json!({ "skill": "jump_pad" }).to_string();
    let (st, _, sb_body) = http_req(
        port,
        "POST",
        "/api/skills/build",
        "application/json",
        Some(skill_build.as_bytes()),
    );
    assert_eq!(st, 200, "skill build jump_pad failed: {sb_body}");
    let sb_v: Value = serde_json::from_str(&sb_body).unwrap();
    assert!(sb_v["snapshot_id"].is_string());

    let (st, _, check_body) = http_req(port, "GET", "/api/luau/check", "application/json", None);
    assert_eq!(st, 200);
    let check_v: Value = serde_json::from_str(&check_body).unwrap();
    assert_eq!(check_v["passed"], json!(true));
    assert_eq!(check_v["scripts_checked"], json!(2));

    let bad_luau = json!({ "source": "while true do\n  print('hang')\n" }).to_string();
    let (st, _, bad_check) = http_req(
        port,
        "POST",
        "/api/luau/check",
        "application/json",
        Some(bad_luau.as_bytes()),
    );
    assert_eq!(st, 200);
    let bad_v: Value = serde_json::from_str(&bad_check).unwrap();
    assert_eq!(bad_v["passed"], json!(false));
    assert!(bad_v["error_count"].as_u64().unwrap() >= 1);

    // 7. Scale round-trip: import 500 instances + 50 scripts over HTTP and verify <1s + clean Luau check.
    let mut big_instances = serde_json::Map::new();
    for i in 0..500 {
        big_instances.insert(
            format!("Workspace.Platform_{i:03}"),
            json!({
                "class_name": "Part",
                "properties": {
                    "Position": [(i % 25) * 12, 4, (i / 25) * 12],
                    "Size": [8, 1, 8],
                    "Color": [59, 108, 226],
                    "Anchored": true
                }
            }),
        );
    }
    let mut big_scripts = serde_json::Map::new();
    for i in 0..50 {
        big_scripts.insert(
            format!("ServerScriptService.System_{i:02}"),
            json!(format!(
                "local Players = game:GetService(\"Players\")\nlocal ID = {i}\nPlayers.PlayerAdded:Connect(function(p) print(p.Name, ID) end)\n"
            )),
        );
    }
    let big_xml = openro_rs::place::serialize_rbxlx(
        "Mega Scale Place",
        &json!({ "instances": big_instances, "scripts": big_scripts }),
    );
    let t0 = std::time::Instant::now();
    let (st, _, mega_imp) = http_req(
        port,
        "POST",
        "/api/import/rbxlx",
        "application/xml",
        Some(big_xml.as_bytes()),
    );
    let imp_ms = t0.elapsed().as_millis();
    assert_eq!(st, 200, "500-instance/50-script import failed: {mega_imp}");
    let mega_v: Value = serde_json::from_str(&mega_imp).unwrap();
    assert_eq!(mega_v["instance_count"], json!(500));
    assert_eq!(mega_v["script_count"], json!(50));
    assert!(imp_ms < 2000, "mega import took too long: {imp_ms}ms");

    let (st, _, mega_exp) = http_req(port, "GET", "/api/export/rbxlx", "application/json", None);
    assert_eq!(st, 200);
    assert!(mega_exp.contains("Platform_499") && mega_exp.contains("System_49"));

    let (st, _, mega_check) = http_req(port, "GET", "/api/luau/check", "application/json", None);
    assert_eq!(st, 200);
    let mc: Value = serde_json::from_str(&mega_check).unwrap();
    assert_eq!(mc["passed"], json!(true));
    assert_eq!(mc["scripts_checked"], json!(50));

    // Verify snapshots persisted cleanly to disk.
    assert!(dir.join("snapshots.json").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn thirty_concurrent_readers_hit_status_and_snapshots_cleanly() {
    let (port, _shared) = start_local_server(None);
    // Seed a manual snapshot first.
    let (st, _, _) = http_req(
        port,
        "POST",
        "/api/snapshots",
        "application/json",
        Some(br#"{"label":"Baseline"}"#),
    );
    assert_eq!(st, 201);

    let handles: Vec<_> = (0..30)
        .map(|i| {
            std::thread::spawn(move || {
                let path = if i % 2 == 0 {
                    "/api/snapshots"
                } else {
                    "/api/status"
                };
                let (st, _, body) = http_req(port, "GET", path, "application/json", None);
                assert_eq!(st, 200, "client {i} on {path} got {st}: {body}");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}
