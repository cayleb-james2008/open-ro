//! Phase 6 (v0.8.0) HTTP integration tests for the Co-Building 3D Studio IDE,
//! Live Luau Script Editor, Rojo Ship Pack `.zip` Exporter, Unified Undo/Redo,
//! and Dynamic Live MCP Connect/Disconnect APIs:
//! - `GET /api/status` & `GET /api/chats` (`ide_v080`, `rojo_export`, `can_undo`, `can_redo`)
//! - `POST /api/place/create` (typed presets + companion Luau scripts)
//! - `POST /api/place/duplicate` (+[4,0,4] clone)
//! - `POST /api/place/script` (live Luau save + immediate Static Luau Prover diagnostics)
//! - `POST /api/place/delete` (instance + script deletion)
//! - `POST /api/revert` + `POST /api/redo` (unified per-chat Undo/Redo round-trip)
//! - `GET /api/export/bundle` (real PKWARE `.zip` Rojo Ship Pack with `game.rbxlx` & assets)
//! - `POST /api/studio/mcp/connect` & `POST /api/studio/mcp/disconnect` (loopback 403 guard + live `mock_studio` PNG frame)

use openro_rs::agent::Agent;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn http_req_host(
    port: u16,
    host: &str,
    method: &str,
    path: &str,
    content_type: &str,
    body: &[u8],
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let header = if body.is_empty() {
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
    };
    stream.write_all(header.as_bytes()).unwrap();
    if !body.is_empty() {
        stream.write_all(body).unwrap();
    }
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let sep = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..sep]).to_string();
    let body_bytes = if sep + 4 <= buf.len() {
        buf[sep + 4..].to_vec()
    } else {
        Vec::new()
    };
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, head, body_bytes)
}

fn http_json(port: u16, method: &str, path: &str, body: &str) -> (u16, Value) {
    let (status, _, bytes) = http_req_host(
        port,
        &format!("127.0.0.1:{port}"),
        method,
        path,
        "application/json",
        body.as_bytes(),
    );
    let val: Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("invalid JSON from {method} {path}: {e} — {:?}", String::from_utf8_lossy(&bytes)));
    (status, val)
}

fn start_local_server(game_name: &str) -> u16 {
    let model = ModelClient::new(
        "http://127.0.0.1:11434/v1".to_string(),
        "qwen2.5:3b".to_string(),
        None,
    );
    let place = LocalPlace::in_memory(game_name.to_string());
    let agent = Agent::with_local_place(model, place);
    let shared = Shared::new(Some(agent), game_name.to_string());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    port
}

#[test]
fn v080_create_duplicate_script_delete_undo_redo_roundtrip() {
    let port = start_local_server("Studio IDE Roundtrip");

    // 1. Fresh status & chats expose v0.8.0 capability flags + false undo/redo
    let (st, status) = http_json(port, "GET", "/api/status", "");
    assert_eq!(st, 200);
    assert_eq!(status["ide_api"], json!(true));
    assert_eq!(status["ide_v080"], json!(true));
    assert_eq!(status["rojo_export"], json!(true));
    assert_eq!(status["can_undo"], json!(false));
    assert_eq!(status["can_redo"], json!(false));

    let (st, chats) = http_json(port, "GET", "/api/chats", "");
    assert_eq!(st, 200);
    assert_eq!(chats["can_undo"], json!(false));
    assert_eq!(chats["can_redo"], json!(false));

    // 2. POST /api/place/create (jump_pad creates part + companion Luau script)
    let (st, created) = http_json(
        port,
        "POST",
        "/api/place/create",
        &json!({ "preset": "jump_pad", "name": "LobbyBounce" }).to_string(),
    );
    assert_eq!(st, 200, "create jump_pad failed: {created}");
    assert_eq!(created["ok"], json!(true));
    assert_eq!(created["path"], json!("Workspace.LobbyBounce"));
    assert_eq!(
        created["companion_script"],
        json!("ServerScriptService.LobbyBounceController")
    );
    assert_eq!(created["can_undo"], json!(true));
    assert_eq!(created["can_redo"], json!(false));

    // 3. POST /api/place/duplicate clones Workspace.LobbyBounce with +[4, 0, 4] offset
    let (st, dup) = http_json(
        port,
        "POST",
        "/api/place/duplicate",
        &json!({ "path": "Workspace.LobbyBounce" }).to_string(),
    );
    assert_eq!(st, 200, "duplicate failed: {dup}");
    assert_eq!(dup["ok"], json!(true));
    let dup_path = dup["path"].as_str().unwrap().to_string();
    assert_eq!(dup_path, "Workspace.LobbyBounce_Copy");
    assert_eq!(dup["instance"]["properties"]["Position"], json!([4.0, 1.0, 20.0]));

    // 4. POST /api/place/script saves live Luau edit and catches real syntax errors immediately
    let (st, bad_script) = http_json(
        port,
        "POST",
        "/api/place/script",
        &json!({
            "path": "ServerScriptService.LobbyBounceController",
            "source": "local x = 1\nif x > 0 then\n  print(x)\n"
        })
        .to_string(),
    );
    assert_eq!(st, 200, "bad script check failed: {bad_script}");
    assert_eq!(bad_script["ok"], json!(true));
    assert!(
        bad_script["error_count"].as_u64().unwrap_or(0) >= 1,
        "unclosed if should report error: {bad_script}"
    );
    assert_eq!(bad_script["passed"], json!(false));

    let clean_source = "-- Live edited JumpPad controller\nlocal Players = game:GetService(\"Players\")\nprint(\"LobbyBounce ready for\", #Players:GetPlayers())\n";
    let (st, good_script) = http_json(
        port,
        "POST",
        "/api/place/script",
        &json!({
            "path": "ServerScriptService.LobbyBounceController",
            "source": clean_source
        })
        .to_string(),
    );
    assert_eq!(st, 200, "clean script save failed: {good_script}");
    assert_eq!(good_script["ok"], json!(true));
    assert_eq!(good_script["error_count"], json!(0));
    assert_eq!(good_script["errors"], json!(0));
    assert_eq!(good_script["passed"], json!(true));

    // 5. POST /api/place/delete removes the duplicated part
    let (st, del) = http_json(
        port,
        "POST",
        "/api/place/delete",
        &json!({ "path": dup_path }).to_string(),
    );
    assert_eq!(st, 200, "delete failed: {del}");
    assert_eq!(del["ok"], json!(true));
    let (_, tree) = http_json(port, "GET", "/api/place-tree", "");
    assert!(tree["instances"].get("Workspace.LobbyBounce_Copy").is_none());

    // 6. POST /api/revert undoes the delete and populates Redo stack
    let (st, rev) = http_json(port, "POST", "/api/revert", "{}");
    assert_eq!(st, 200, "revert failed: {rev}");
    assert_eq!(rev["ok"], json!(true));
    assert_eq!(rev["can_redo"], json!(true));
    let (_, tree_after_undo) = http_json(port, "GET", "/api/place-tree", "");
    assert!(
        tree_after_undo["instances"]
            .get("Workspace.LobbyBounce_Copy")
            .is_some(),
        "Undo must restore deleted duplicate: {tree_after_undo}"
    );

    let (_, chats_after_undo) = http_json(port, "GET", "/api/chats", "");
    assert_eq!(chats_after_undo["can_redo"], json!(true));

    // 7. POST /api/redo re-applies the delete and empties Redo stack
    let (st, redo) = http_json(port, "POST", "/api/redo", "{}");
    assert_eq!(st, 200, "redo failed: {redo}");
    assert_eq!(redo["ok"], json!(true));
    assert_eq!(redo["can_undo"], json!(true));
    assert_eq!(redo["can_redo"], json!(false));
    let (_, tree_after_redo) = http_json(port, "GET", "/api/place-tree", "");
    assert!(
        tree_after_redo["instances"]
            .get("Workspace.LobbyBounce_Copy")
            .is_none(),
        "Redo must re-apply the deletion: {tree_after_redo}"
    );

    // 8. Second POST /api/redo reports honest nothing-to-redo
    let (st, empty_redo) = http_json(port, "POST", "/api/redo", "{}");
    assert_eq!(st, 200);
    assert_eq!(empty_redo["ok"], json!(false));
    assert_eq!(empty_redo["error"], json!("nothing to redo"));
}

#[test]
fn v080_export_bundle_returns_valid_rojo_ship_pack_zip() {
    let port = start_local_server("Apex Card Arena");

    // Build a full blueprint so the Ship Pack has rich scripts + 3D instances
    let (st, built) = http_json(
        port,
        "POST",
        "/api/ideas/build",
        &json!({ "id": "anime-card-battles-apex" }).to_string(),
    );
    assert_eq!(st, 200, "blueprint build failed: {built}");
    assert_eq!(built["ok"], json!(true));

    // Upload a minimal valid 1x1 PNG asset so assets/ is populated too
    let tiny_png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let (st, _, up_body) = http_req_host(
        port,
        &format!("127.0.0.1:{port}"),
        "POST",
        "/api/assets/upload?filename=hero-card.png&card_id=hero_card&title=Hero%20Card&rarity=Mythic&element=Fire",
        "image/png",
        tiny_png,
    );
    assert_eq!(st, 201, "asset upload failed: {}", String::from_utf8_lossy(&up_body));

    // Download GET /api/export/bundle
    let (st, head, zip_bytes) = http_req_host(
        port,
        &format!("127.0.0.1:{port}"),
        "GET",
        "/api/export/bundle",
        "application/octet-stream",
        b"",
    );
    assert_eq!(st, 200, "export bundle failed: {head}");
    assert!(
        head.to_ascii_lowercase().contains("content-type: application/zip"),
        "missing application/zip header: {head}"
    );
    assert!(
        head.contains("apex-card-arena-ship-pack.zip"),
        "missing ship-pack filename in Content-Disposition: {head}"
    );
    assert!(
        zip_bytes.starts_with(b"PK\x03\x04"),
        "ZIP must start with PK\\x03\\x04 magic bytes"
    );
    assert!(
        zip_bytes.windows(4).any(|w| w == b"PK\x05\x06"),
        "ZIP must contain End of Central Directory signature"
    );

    let lossy = String::from_utf8_lossy(&zip_bytes);
    for expected in [
        "game.rbxlx",
        "default.project.json",
        "src/shared/PlaceMeta.luau",
        "asset-manifest.json",
        "assets/hero-card.png",
        "luau-audit-report.json",
        "STUDIO-UPLOAD-CHECKLIST.md",
    ] {
        assert!(
            lossy.contains(expected),
            "Rojo Ship Pack ZIP missing expected entry '{expected}'"
        );
    }
}

#[test]
fn v080_dynamic_mcp_connect_guards_non_loopback_and_streams_live_frame() {
    let port = start_local_server("Live MCP Connect Test");

    // 1. Starts in local_place mode
    let (st, status) = http_json(port, "GET", "/api/status", "");
    assert_eq!(st, 200);
    assert_eq!(status["studio_mode"], json!("local_place"));

    // 2. Non-loopback Host MUST be rejected with 403 Forbidden
    let (st_evil, _, _) = http_req_host(
        port,
        "evil.example:8077",
        "POST",
        "/api/studio/mcp/connect",
        "application/json",
        b"{}",
    );
    assert_eq!(
        st_evil, 403,
        "non-loopback Host must be rejected on /api/studio/mcp/connect"
    );

    // 3. Connect dynamically to the compiled mock_studio binary over loopback
    let mock_bin = env!("CARGO_BIN_EXE_mock_studio");
    let (st, conn) = http_json(
        port,
        "POST",
        "/api/studio/mcp/connect",
        &json!({ "command": mock_bin }).to_string(),
    );
    assert_eq!(st, 200, "dynamic MCP connect failed: {conn}");
    assert_eq!(conn["ok"], json!(true));
    assert_eq!(conn["connected"], json!(true));
    assert_eq!(conn["studio_mode"], json!("live_mcp"));
    assert_eq!(conn["workspace_mode"], json!("studio_mcp"));
    assert_eq!(conn["has_live_frame"], json!(true));
    assert!(conn["frame_seq"].as_u64().unwrap_or(0) >= 1);

    // 4. GET /api/studio/frame returns real 200 OK PNG bytes
    let (st, head, png) = http_req_host(
        port,
        &format!("127.0.0.1:{port}"),
        "GET",
        "/api/studio/frame",
        "application/octet-stream",
        b"",
    );
    assert_eq!(st, 200, "live frame fetch failed: {head}");
    assert!(head.to_ascii_lowercase().contains("content-type: image/png"));
    assert!(
        png.starts_with(b"\x89PNG\r\n\x1a\n"),
        "frame bytes must be valid PNG"
    );

    // 5. Playtest executes on the connected live MCP session
    let (st, pt) = http_json(port, "POST", "/api/studio/playtest", "{}");
    assert_eq!(st, 200, "live MCP playtest failed: {pt}");
    assert_eq!(pt["ok"], json!(true));
    assert_eq!(pt["passed"], json!(true));
    assert_eq!(pt["studio_mode"], json!("live_mcp"));

    // 6. Disconnect returns cleanly to local_place
    let (st, disc) = http_json(port, "POST", "/api/studio/mcp/disconnect", "{}");
    assert_eq!(st, 200, "MCP disconnect failed: {disc}");
    assert_eq!(disc["ok"], json!(true));
    assert_eq!(disc["connected"], json!(false));
    assert_eq!(disc["studio_mode"], json!("local_place"));
    assert_eq!(disc["workspace_mode"], json!("local_place"));

    let (st, after_status) = http_json(port, "GET", "/api/status", "");
    assert_eq!(st, 200);
    assert_eq!(after_status["studio_mode"], json!("local_place"));
}
