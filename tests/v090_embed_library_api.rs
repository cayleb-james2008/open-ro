//! Phase 7 (v0.9.0) HTTP integration tests for:
//! - Directly Embedded Roblox Studio lifecycle (`GET /api/studio/embed/status`,
//!   `POST /api/studio/embed/launch`, `POST /api/studio/embed/stop`)
//! - Live multipart MJPEG viewport stream (`GET /api/studio/stream.mjpg?once=1`)
//! - Interactive mouse & keyboard input pass-through (`POST /api/studio/input/{mouse,keyboard}`)
//!   producing distinct rendered PNG frames from the upgraded 13-tool `mock_studio`
//! - All 13 official Roblox Studio MCP tools (`POST /api/studio/tools/:tool` + `404` unknown + `403` non-loopback)
//! - Built-in Developer Skill & Connection Library (`GET /api/library`, `POST /api/library/{install,configure,test,uninstall}`)
//! - Real Blender 3D mesh generator (`POST /api/tools/blender/generate`, `GET /api/assets/meshes/:file`)
//! - Pure-Rust procedural PCM16 `.wav` SFX generator (`POST /api/tools/audio/generate`, `GET /api/assets/audio/:file`)

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
    body: &[u8],
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let header = if body.is_empty() {
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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
        body.as_bytes(),
    );
    let val: Value = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "invalid JSON from {method} {path}: {e} — {:?}",
            String::from_utf8_lossy(&bytes)
        )
    });
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
fn v090_embedded_studio_mjpeg_input_passthrough_and_all_13_mcp_tools() {
    let port = start_local_server("Embedded Studio v0.9.0");

    // 1. Status & embed status start idle in local_place (NEVER auto-launches Studio!)
    let (st, status) = http_json(port, "GET", "/api/status", "");
    assert_eq!(st, 200);
    assert_eq!(status["studio_embed"], json!(true));
    assert_eq!(status["studio_mjpeg"], json!(true));
    assert_eq!(status["library_api"], json!(true));
    assert_eq!(status["studio_tools"].as_array().unwrap().len(), 13);

    let (st, est) = http_json(port, "GET", "/api/studio/embed/status", "");
    assert_eq!(st, 200);
    assert_eq!(est["ok"], json!(true));
    assert_eq!(est["running"], json!(false));
    assert_eq!(est["stream_url"], json!("/api/studio/stream.mjpg"));

    // 2. Non-loopback Host MUST be refused 403 on embed launch, input, and tools
    for ep in [
        "/api/studio/embed/launch",
        "/api/studio/embed/stop",
        "/api/studio/input/mouse",
        "/api/studio/input/keyboard",
        "/api/studio/tools/get_studio_state",
    ] {
        let (st_evil, _, _) = http_req_host(port, "evil.example:8077", "POST", ep, b"{}");
        assert_eq!(st_evil, 403, "expected 403 non-loopback on {ep}");
    }

    // 3. Unknown tool rejected with 404
    let (st_unk, unk) = http_json(port, "POST", "/api/studio/tools/drop_database", "{}");
    assert_eq!(st_unk, 404);
    assert_eq!(unk["ok"], json!(false));

    // 4. Launch Embedded Studio attached to compiled 13-tool mock_studio
    let mock_bin = env!("CARGO_BIN_EXE_mock_studio");
    let (st, launched) = http_json(
        port,
        "POST",
        "/api/studio/embed/launch",
        &json!({ "command": mock_bin }).to_string(),
    );
    assert_eq!(st, 200, "embed launch failed: {launched}");
    assert_eq!(launched["ok"], json!(true));
    assert_eq!(launched["running"], json!(true));
    assert_eq!(launched["studio_mode"], json!("live_mcp"));
    assert_eq!(launched["has_live_frame"], json!(true));
    assert_eq!(launched["available_tools"].as_array().unwrap().len(), 13);

    // 5. GET /api/studio/stream.mjpg?once=1 emits valid multipart/x-mixed-replace with PNG bytes
    let (st_mj, head_mj, body_mj) = http_req_host(
        port,
        &format!("127.0.0.1:{port}"),
        "GET",
        "/api/studio/stream.mjpg?once=1",
        b"",
    );
    assert_eq!(st_mj, 200, "mjpeg failed: {head_mj}");
    assert!(
        head_mj
            .to_ascii_lowercase()
            .contains("multipart/x-mixed-replace; boundary=openro-frame"),
        "missing multipart content-type: {head_mj}"
    );
    assert!(
        head_mj.contains("X-Studio-Frame-Seq:"),
        "missing X-Studio-Frame-Seq header: {head_mj}"
    );
    assert!(
        body_mj.windows(8).any(|w| w == b"\x89PNG\r\n\x1a\n"),
        "MJPEG part must embed valid PNG frame bytes"
    );

    // 6. Capture baseline PNG, send mouse click + keyboard input, verify fresh distinct PNG frames!
    let (_, _, png0) = http_req_host(
        port,
        &format!("127.0.0.1:{port}"),
        "GET",
        "/api/studio/frame",
        b"",
    );
    assert!(png0.starts_with(b"\x89PNG\r\n\x1a\n"));

    let (st_m, mouse_res) = http_json(
        port,
        "POST",
        "/api/studio/input/mouse",
        &json!({ "action": "click", "x": 160, "y": 100, "button": "left" }).to_string(),
    );
    assert_eq!(st_m, 200, "mouse input failed: {mouse_res}");
    assert_eq!(mouse_res["ok"], json!(true));
    let seq1 = mouse_res["frame_seq"].as_u64().unwrap_or(0);
    assert!(seq1 >= 2, "mouse click should advance frame_seq: {mouse_res}");

    let (_, _, png1) = http_req_host(
        port,
        &format!("127.0.0.1:{port}"),
        "GET",
        "/api/studio/frame",
        b"",
    );
    assert_ne!(png0, png1, "mouse click should produce updated rendered PNG");

    let (st_k, kb_res) = http_json(
        port,
        "POST",
        "/api/studio/input/keyboard",
        &json!({ "action": "type", "text": "Hello Studio" }).to_string(),
    );
    assert_eq!(st_k, 200, "keyboard input failed: {kb_res}");
    assert_eq!(kb_res["ok"], json!(true));
    assert!(kb_res["frame_seq"].as_u64().unwrap_or(0) > seq1);

    // 7. Exercise official Studio MCP tools over POST /api/studio/tools/:tool
    let (st_ls, ls) = http_json(port, "POST", "/api/studio/tools/list_roblox_studios", "{}");
    assert_eq!(st_ls, 200, "list_roblox_studios failed: {ls}");
    assert_eq!(ls["ok"], json!(true));

    let (st_gs, gs) = http_json(port, "POST", "/api/studio/tools/get_studio_state", "{}");
    assert_eq!(st_gs, 200, "get_studio_state failed: {gs}");
    assert_eq!(gs["ok"], json!(true));

    let (st_ex, ex) = http_json(
        port,
        "POST",
        "/api/studio/tools/execute_luau",
        &json!({ "code": "print('v0.9.0 live luau ok')" }).to_string(),
    );
    assert_eq!(st_ex, 200, "execute_luau failed: {ex}");
    assert_eq!(ex["ok"], json!(true));

    let (st_sg, sg) = http_json(
        port,
        "POST",
        "/api/studio/tools/search_game_tree",
        &json!({ "query": "" }).to_string(),
    );
    assert_eq!(st_sg, 200, "search_game_tree failed: {sg}");
    assert_eq!(sg["ok"], json!(true));

    let (st_co, co) = http_json(port, "POST", "/api/studio/tools/get_console_output", "{}");
    assert_eq!(st_co, 200, "get_console_output failed: {co}");
    assert_eq!(co["ok"], json!(true));

    let (st_pl, pl) = http_json(
        port,
        "POST",
        "/api/studio/tools/start_stop_play",
        &json!({ "mode": "start_play" }).to_string(),
    );
    assert_eq!(st_pl, 200, "start_stop_play failed: {pl}");
    assert_eq!(pl["ok"], json!(true));

    // 8. Stop Embedded Studio -> cleanly returns to local_place
    let (st_stop, stopped) = http_json(port, "POST", "/api/studio/embed/stop", "{}");
    assert_eq!(st_stop, 200, "embed stop failed: {stopped}");
    assert_eq!(stopped["ok"], json!(true));
    assert_eq!(stopped["running"], json!(false));
    assert_eq!(stopped["studio_mode"], json!("local_place"));
}

#[test]
fn v090_library_packs_blender_mesh_and_wav_audio_api() {
    let port = start_local_server("Library & Media Tools v0.9.0");

    // 1. GET /api/library returns 4 pre-installed defaults + 5 installable packs (9 total)
    let (st, lib) = http_json(port, "GET", "/api/library", "");
    assert_eq!(st, 200, "GET /api/library failed: {lib}");
    assert_eq!(lib["ok"], json!(true));
    let packs = lib["packs"].as_array().unwrap();
    assert_eq!(packs.len(), 9, "expected 9 library packs: {lib}");
    let defaults = packs
        .iter()
        .filter(|p| p["default_installed"] == json!(true))
        .count();
    assert_eq!(defaults, 4);
    assert!(
        packs.iter().any(|p| p["id"] == json!("roblox-market-taste") && p["default_installed"] == json!(true)),
        "roblox-market-taste must be pre-installed as the 4th default pack: {lib}"
    );

    // 2. Non-loopback refused 403 on library install & media tools
    let (st_evil, _, _) = http_req_host(
        port,
        "evil.example:8077",
        "POST",
        "/api/library/install",
        b"{\"id\":\"audio-sfx-gen\"}",
    );
    assert_eq!(st_evil, 403);

    // 3. Install, configure (masked secret!), test, and uninstall an installable pack
    let (st, inst) = http_json(
        port,
        "POST",
        "/api/library/install",
        &json!({ "id": "audio-sfx-gen" }).to_string(),
    );
    assert_eq!(st, 200, "install failed: {inst}");
    assert_eq!(inst["ok"], json!(true));

    let (st, cfg) = http_json(
        port,
        "POST",
        "/api/library/configure",
        &json!({ "id": "audio-sfx-gen", "api_key": "sk-test-secret-987654" }).to_string(),
    );
    assert_eq!(st, 200, "configure failed: {cfg}");
    let cfg_str = cfg.to_string();
    assert!(
        !cfg_str.contains("sk-test-secret-987654"),
        "raw secret MUST NEVER be echoed back: {cfg_str}"
    );

    let (st, tested) = http_json(
        port,
        "POST",
        "/api/library/test",
        &json!({ "id": "audio-sfx-gen" }).to_string(),
    );
    assert_eq!(st, 200, "test pack failed: {tested}");
    assert_eq!(tested["passed"], json!(true));

    // Default pack cannot be uninstalled (400)
    let (st_def, _) = http_json(
        port,
        "POST",
        "/api/library/uninstall",
        &json!({ "id": "roblox-studio-core" }).to_string(),
    );
    assert_eq!(st_def, 400);

    // Installable pack uninstalls cleanly (200)
    let (st_un, un) = http_json(
        port,
        "POST",
        "/api/library/uninstall",
        &json!({ "id": "audio-sfx-gen" }).to_string(),
    );
    assert_eq!(st_un, 200, "uninstall failed: {un}");

    // 4. POST /api/tools/blender/generate + GET /api/assets/meshes/:file
    let (st, mesh) = http_json(
        port,
        "POST",
        "/api/tools/blender/generate",
        &json!({ "preset": "torii_gate", "name": "TestTorii", "render_preview": true }).to_string(),
    );
    assert_eq!(st, 200, "blender generate failed: {mesh}");
    assert_eq!(mesh["ok"], json!(true));
    let obj_url = mesh["obj_url"].as_str().unwrap();
    let preview_url = mesh["preview_url"].as_str().unwrap();

    let (st_obj, _, obj_bytes) = http_req_host(
        port,
        &format!("127.0.0.1:{port}"),
        "GET",
        obj_url,
        b"",
    );
    assert_eq!(st_obj, 200);
    assert!(String::from_utf8_lossy(&obj_bytes).contains("v "));

    let (st_png, _, png_bytes) = http_req_host(
        port,
        &format!("127.0.0.1:{port}"),
        "GET",
        preview_url,
        b"",
    );
    assert_eq!(st_png, 200);
    assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    // 5. POST /api/tools/audio/generate + GET /api/assets/audio/:file (RIFF/WAVE)
    let (st, wav) = http_json(
        port,
        "POST",
        "/api/tools/audio/generate",
        &json!({ "preset": "mythic_fanfare", "name": "MythicPull" }).to_string(),
    );
    assert_eq!(st, 200, "audio generate failed: {wav}");
    assert_eq!(wav["ok"], json!(true));
    let wav_url = wav["wav_url"].as_str().unwrap();
    let (st_w, head_w, wav_bytes) = http_req_host(
        port,
        &format!("127.0.0.1:{port}"),
        "GET",
        wav_url,
        b"",
    );
    assert_eq!(st_w, 200, "audio fetch failed: {head_w}");
    assert!(head_w.to_ascii_lowercase().contains("content-type: audio/wav"));
    assert!(wav_bytes.starts_with(b"RIFF"));
    assert_eq!(&wav_bytes[8..12], b"WAVE");
}

#[test]
fn v090_market_taste_api_niche_guards_and_prompt_injection() {
    let model = ModelClient::new(
        "http://127.0.0.1:11434/v1".to_string(),
        "qwen2.5:3b".to_string(),
        None,
    );
    let place = LocalPlace::in_memory("Market Taste API v0.9.0".to_string());
    let agent = Agent::with_local_place(model, place);
    let shared = Shared::new(Some(agent), "Market Taste API v0.9.0".to_string());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv_shared = std::sync::Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv_shared);
    });

    // 1. GET /api/status exposes market_taste_api & active_niche
    let (st, status) = http_json(port, "GET", "/api/status", "");
    assert_eq!(st, 200);
    assert_eq!(status["market_taste_api"], json!(true));
    assert_eq!(status["active_niche"], json!("anime-rng-cards"));

    // 2. GET /api/market-taste returns all 8 niches + full 8-dimension Taste Dossier + automatic twists
    let (st, mt) = http_json(port, "GET", "/api/market-taste", "");
    assert_eq!(st, 200, "GET /api/market-taste failed: {mt}");
    assert_eq!(mt["ok"], json!(true));
    assert_eq!(mt["active_niche"], json!("anime-rng-cards"));
    assert_eq!(mt["niches"].as_array().unwrap().len(), 8);
    assert!(mt["niche"]["taste_dossier"].is_object());
    assert!(mt["niche"]["automatic_twists"].as_array().unwrap().len() >= 3);
    assert!(mt["niche"]["anti_slop_rules"].as_array().unwrap().len() >= 3);

    // 3. Query param ?niche= switches preview without mutating active niche; bad niche -> 400
    let (st, mt_q) = http_json(port, "GET", "/api/market-taste?niche=viral-sim-tycoon", "");
    assert_eq!(st, 200);
    assert_eq!(mt_q["selected_niche"], json!("viral-sim-tycoon"));
    assert_eq!(mt_q["active_niche"], json!("anime-rng-cards"));

    let (st_bad, _) = http_json(port, "GET", "/api/market-taste?niche=not-a-niche", "");
    assert_eq!(st_bad, 400);

    // 4. Non-loopback refused 403 on all Market Taste POST endpoints
    for ep in [
        "/api/market-taste/niche",
        "/api/market-taste/refresh",
        "/api/market-taste/research",
    ] {
        let (st_evil, _, _) = http_req_host(
            port,
            "evil.example:8077",
            "POST",
            ep,
            b"{\"niche\":\"viral-sim-tycoon\",\"target\":\"10092619788\"}",
        );
        assert_eq!(st_evil, 403, "expected 403 on non-loopback {ep}");
    }

    // 5. POST /api/market-taste/niche switches active niche & updates Agent's injected system prompt
    let (st, sw) = http_json(
        port,
        "POST",
        "/api/market-taste/niche",
        &json!({ "niche": "tactical-arena-pvp" }).to_string(),
    );
    assert_eq!(st, 200, "switch niche failed: {sw}");
    assert_eq!(sw["active_niche"], json!("tactical-arena-pvp"));

    let (st, status2) = http_json(port, "GET", "/api/status", "");
    assert_eq!(st, 200);
    assert_eq!(status2["active_niche"], json!("tactical-arena-pvp"));

    let sys_prompt = shared
        .agent
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .effective_system_prompt();
    assert!(
        sys_prompt.contains("roblox-market-taste") && sys_prompt.contains("tactical-arena-pvp"),
        "Agent effective_system_prompt must include updated active niche Craft Bar"
    );

    // 6. Malformed / empty research input returns 400 (never 500)
    let (st_r400, _) = http_json(port, "POST", "/api/market-taste/research", "{}");
    assert_eq!(st_r400, 400);
}
