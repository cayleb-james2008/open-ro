//! M2 PR D (Gap #81): Lean Studio-Native Workbench + Live Agent CUA over headed/embedded Studio.
//!
//! Covers:
//! - Loopback-only CUA HTTP endpoints (`/api/studio/cua/{status,events,frame/*,screenshot,mouse,keyboard,window}`)
//! - Mouse click/drag validation + `cua_paused` 409 lock + pause/resume
//! - Hotkey chords + strict OS-escape denylist 400 (`Alt+F4`, `Super`, `Meta`, `Ctrl+Alt+Delete`, `Ctrl+Alt+F*`)
//! - `capture_studio_frame_png` CWE-22/59 safe storage, pruning to 40 frames, and symlink containment
//! - `StudioCuaLog` 80-entry bounded ring buffer & live SSE `studio_cua` broadcast
//! - `Agent` and `Teamwork` `studio_cua_*` tool wiring and Studio-Native First prompt guidance

use openro_rs::agent::{project_system_prompt, system_prompt, Agent};
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::projects::ProjectStore;
use openro_rs::server::{capture_studio_frame_png, ensure_agent_hooks, serve, Shared};
use openro_rs::studio_embed::{
    check_hotkey_safety, validate_cua_mouse_payload, StudioCuaEventInput, StudioCuaLog,
};
use openro_rs::studio_stream::{StudioFrame, StudioStreamHub};
use openro_rs::teamwork::teamwork_role_prompt_preview;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base
        .join("m2-studio-cua-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn boot_server(dir: &Path) -> (u16, PathBuf, PathBuf, Arc<Shared>) {
    let data = dir.join("data");
    let proj = dir.join("proj");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(proj.join("src/shared")).unwrap();
    std::fs::write(
        proj.join("openro.project.json"),
        json!({ "name": "CuaTest", "slug": "cua-test" }).to_string(),
    )
    .unwrap();

    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None);
    let agent = Agent::with_local_place(model, LocalPlace::open(data.clone(), "CuaTest".into()));
    let shared = Shared::with_data_dir(Some(agent), "CuaTest".into(), data.clone());
    {
        let mut store: std::sync::MutexGuard<'_, ProjectStore> = shared.projects.lock().unwrap();
        let entry = store.open(proj.to_str().unwrap()).unwrap();
        let id = entry.id.clone();
        store.set_active(&id);
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv);
    });
    (port, data, proj, shared)
}

fn http_raw(
    port: u16,
    host: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> (u16, String, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let hdr = if body.is_empty() {
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
    };
    s.write_all(hdr.as_bytes()).unwrap();
    if !body.is_empty() {
        s.write_all(body).unwrap();
    }
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
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
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    (status, head, body_bytes)
}

fn http_json(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let bytes = body.map(|v| v.to_string().into_bytes()).unwrap_or_default();
    let (status, _head, resp) = http_raw(
        port,
        &format!("127.0.0.1:{port}"),
        method,
        path,
        &bytes,
    );
    (status, serde_json::from_slice(&resp).unwrap_or(Value::Null))
}

fn tiny_png() -> Vec<u8> {
    // Minimal 1x1 RGBA PNG
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

#[test]
fn cua_status_events_and_loopback_guard() {
    let dir = scratch("loopback");
    let (port, _data, _proj, _shared) = boot_server(&dir);

    let (st, status) = http_json(port, "GET", "/api/studio/cua/status", None);
    assert_eq!(st, 200, "status: {status}");
    assert_eq!(status["ok"], true);
    assert_eq!(status["cua_paused"], false);
    assert!(status["recent_events"].is_array());

    let (st_ev, evs) = http_json(port, "GET", "/api/studio/cua/events", None);
    assert_eq!(st_ev, 200, "events: {evs}");
    assert_eq!(evs["ok"], true);
    assert_eq!(evs["cua_paused"], false);
    assert!(evs["events"].is_array());

    for (method, ep) in [
        ("GET", "/api/studio/cua/status"),
        ("GET", "/api/studio/cua/events"),
        ("POST", "/api/studio/cua/screenshot"),
        ("POST", "/api/studio/cua/mouse"),
        ("POST", "/api/studio/cua/keyboard"),
        ("POST", "/api/studio/cua/window"),
    ] {
        let body = if method == "POST" { b"{}".as_slice() } else { b"".as_slice() };
        let (bad_st, _, _) = http_raw(port, "evil.example:8077", method, ep, body);
        assert_eq!(bad_st, 403, "expected 403 non-loopback on {method} {ep}");
    }
}

#[test]
fn cua_mouse_drag_validation_and_paused_409_lock() {
    // Pure validation checks
    assert!(validate_cua_mouse_payload(&json!({ "action": "teleport", "x": 0.5, "y": 0.5 }), [1440, 900]).is_err());
    assert!(validate_cua_mouse_payload(&json!({ "action": "click", "x": -0.1, "y": 0.5 }), [1440, 900]).is_err());
    assert!(validate_cua_mouse_payload(&json!({ "action": "drag", "from_x": 0.1, "from_y": 0.2 }), [1440, 900]).is_err());
    let valid_drag = validate_cua_mouse_payload(
        &json!({ "action": "drag", "from_x": 0.2, "from_y": 0.3, "to_x": 0.7, "to_y": 0.8, "normalized": true }),
        [1440, 900],
    )
    .expect("valid drag");
    assert_eq!(valid_drag.action, "drag");
    assert_eq!(valid_drag.to_x, Some((0.7 * 1440.0_f64).round() as i32));

    let dir = scratch("mouse-pause");
    let (port, _data, _proj, _shared) = boot_server(&dir);
    let mock_bin = env!("CARGO_BIN_EXE_mock_studio");
    let (st_launch, launched) = http_json(
        port,
        "POST",
        "/api/studio/embed/launch",
        Some(&json!({ "command": mock_bin })),
    );
    assert_eq!(st_launch, 200, "launch failed: {launched}");

    // 400 on invalid action / negative coords / missing drag target
    let (st_bad1, _) = http_json(port, "POST", "/api/studio/cua/mouse", Some(&json!({ "action": "teleport", "x": 0.5, "y": 0.5 })));
    assert_eq!(st_bad1, 400);
    let (st_bad2, _) = http_json(port, "POST", "/api/studio/cua/mouse", Some(&json!({ "action": "click", "x": -1, "y": 10 })));
    assert_eq!(st_bad2, 400);
    let (st_bad3, _) = http_json(port, "POST", "/api/studio/cua/mouse", Some(&json!({ "action": "drag", "from_x": 0.2, "from_y": 0.3 })));
    assert_eq!(st_bad3, 400);

    // Valid click & drag
    let (st_clk, clk) = http_json(
        port,
        "POST",
        "/api/studio/cua/mouse",
        Some(&json!({ "action": "click", "x": 0.5, "y": 0.5, "normalized": true })),
    );
    assert_eq!(st_clk, 200, "click failed: {clk}");
    assert_eq!(clk["event"]["kind"], "click");

    let (st_drg, drg) = http_json(
        port,
        "POST",
        "/api/studio/cua/mouse",
        Some(&json!({ "action": "drag", "from_x": 0.2, "from_y": 0.3, "to_x": 0.7, "to_y": 0.8, "normalized": true })),
    );
    assert_eq!(st_drg, 200, "drag failed: {drg}");
    assert_eq!(drg["event"]["kind"], "drag");
    assert_eq!(drg["event"]["to_norm_x"], 0.7);
    assert_eq!(drg["event"]["to_norm_y"], 0.8);

    // Pause -> 409 lock on mouse and keyboard -> Resume -> 200
    let (st_pause, paused) = http_json(
        port,
        "POST",
        "/api/studio/cua/window",
        Some(&json!({ "action": "pause" })),
    );
    assert_eq!(st_pause, 200, "pause failed: {paused}");
    assert_eq!(paused["cua_paused"], true);

    let (st_m409, m409) = http_json(
        port,
        "POST",
        "/api/studio/cua/mouse",
        Some(&json!({ "action": "click", "x": 0.5, "y": 0.5 })),
    );
    assert_eq!(st_m409, 409, "expected 409 when paused: {m409}");
    let (st_k409, k409) = http_json(
        port,
        "POST",
        "/api/studio/cua/keyboard",
        Some(&json!({ "hotkey": ["ctrl", "s"] })),
    );
    assert_eq!(st_k409, 409, "expected 409 keyboard when paused: {k409}");

    let (st_res, resumed) = http_json(
        port,
        "POST",
        "/api/studio/cua/window",
        Some(&json!({ "action": "resume" })),
    );
    assert_eq!(st_res, 200, "resume failed: {resumed}");
    assert_eq!(resumed["cua_paused"], false);

    let (st_ok, _) = http_json(
        port,
        "POST",
        "/api/studio/cua/mouse",
        Some(&json!({ "action": "click", "x": 0.4, "y": 0.4, "normalized": true })),
    );
    assert_eq!(st_ok, 200);
}

#[test]
fn cua_keyboard_hotkeys_and_os_escape_denylist() {
    // Pure safety denylist checks
    for denied in [
        vec!["alt".to_string(), "f4".to_string()],
        vec!["super".to_string()],
        vec!["meta".to_string(), "r".to_string()],
        vec!["ctrl".to_string(), "alt".to_string(), "delete".to_string()],
        vec!["ctrl".to_string(), "alt".to_string(), "f3".to_string()],
    ] {
        assert!(check_hotkey_safety(&denied).is_err(), "should deny {denied:?}");
    }
    for allowed in [
        vec!["ctrl".to_string(), "s".to_string()],
        vec!["ctrl".to_string(), "shift".to_string(), "p".to_string()],
        vec!["alt".to_string(), "p".to_string()],
        vec!["f5".to_string()],
        vec!["f8".to_string()],
    ] {
        assert!(check_hotkey_safety(&allowed).is_ok(), "should allow {allowed:?}");
    }

    let dir = scratch("keyboard");
    let (port, _data, _proj, _shared) = boot_server(&dir);
    let mock_bin = env!("CARGO_BIN_EXE_mock_studio");
    let (st_launch, _) = http_json(
        port,
        "POST",
        "/api/studio/embed/launch",
        Some(&json!({ "command": mock_bin })),
    );
    assert_eq!(st_launch, 200);

    for bad in [
        json!({ "hotkey": ["alt", "f4"] }),
        json!({ "hotkey": ["super"] }),
        json!({ "hotkey": ["ctrl", "alt", "delete"] }),
        json!({ "hotkey": ["ctrl", "alt", "f1"] }),
        json!({ "action": "press", "key": "F4", "modifiers": { "alt": true } }),
    ] {
        let (st, res) = http_json(port, "POST", "/api/studio/cua/keyboard", Some(&bad));
        assert_eq!(st, 400, "expected 400 OS-escape denylist for {bad}: {res}");
    }

    let (st_ok, res_ok) = http_json(
        port,
        "POST",
        "/api/studio/cua/keyboard",
        Some(&json!({ "hotkey": ["ctrl", "shift", "p"] })),
    );
    assert_eq!(st_ok, 200, "ctrl+shift+p failed: {res_ok}");
    assert_eq!(res_ok["event"]["kind"], "hotkey");
}

#[test]
fn cua_screenshot_frame_containment_and_pruning() {
    let dir = scratch("frames");
    let (port, data, _proj, shared) = boot_server(&dir);
    shared.studio_stream.push_frame(StudioFrame {
        seq: 0,
        ts_ms: 0,
        mime: "image/png".to_string(),
        bytes: tiny_png(),
        width: 1,
        height: 1,
        source: "test_png".to_string(),
    });

    let shot = capture_studio_frame_png(&shared, &data, Some("../evil/shot!")).unwrap();
    assert_eq!(shot["ok"], true);
    let url = shot["url"].as_str().unwrap();
    assert!(url.starts_with("/api/studio/cua/frame/"));
    let (st_img, head_img, body_img) =
        http_raw(port, &format!("127.0.0.1:{port}"), "GET", url, b"");
    assert_eq!(st_img, 200, "frame fetch failed: {head_img}");
    assert!(body_img.starts_with(b"\x89PNG\r\n\x1a\n"));

    // Traversal and extension rejection
    for bad_path in [
        "/api/studio/cua/frame/..%2Fchats.json",
        "/api/studio/cua/frame/../chats.json",
        "/api/studio/cua/frame/not_png.txt",
    ] {
        let (bad_st, _, _) =
            http_raw(port, &format!("127.0.0.1:{port}"), "GET", bad_path, b"");
        assert!(bad_st == 400 || bad_st == 403 || bad_st == 404, "bad_path {bad_path} returned {bad_st}");
    }

    // Symlink CWE-59 refusal
    #[cfg(unix)]
    {
        let target_secret = data.join("secret.png");
        std::fs::write(&target_secret, tiny_png()).unwrap();
        let link_path = data.join("cua-frames").join("symlink.png");
        std::os::unix::fs::symlink(&target_secret, &link_path).unwrap();
        let (sym_st, _, _) = http_raw(
            port,
            &format!("127.0.0.1:{port}"),
            "GET",
            "/api/studio/cua/frame/symlink.png",
            b"",
        );
        assert_eq!(sym_st, 403, "expected 403 for symlink frame");
        let _ = std::fs::remove_file(&link_path);
    }

    // Pruned to 40 frames
    for i in 0..45 {
        let _ = capture_studio_frame_png(&shared, &data, Some(&format!("p{i}"))).unwrap();
    }
    let count = std::fs::read_dir(data.join("cua-frames"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("png"))
        .count();
    assert!(count <= 40, "expected <= 40 pruned frames, got {count}");
}

#[test]
fn cua_log_ring_buffer_and_sse_broadcast() {
    let hub = Arc::new(StudioStreamHub::new("live_mcp"));
    let mut sub = hub.subscribe(0).unwrap();
    let log = StudioCuaLog::new(Some(Arc::clone(&hub)));

    for i in 1..=95 {
        log.record(StudioCuaEventInput {
            actor: "agent".to_string(),
            kind: "click".to_string(),
            summary: format!("click #{i}"),
            norm_x: Some(0.5),
            norm_y: Some(0.5),
            to_norm_x: None,
            to_norm_y: None,
            key: None,
            hotkey: None,
            frame_url: None,
            ok: true,
        });
    }
    let recent = log.recent(100);
    assert_eq!(recent.len(), 80);
    assert_eq!(recent.first().unwrap().summary, "click #16");
    assert_eq!(recent.last().unwrap().summary, "click #95");

    let drained = sub.drain_pending();
    assert!(
        drained.iter().any(|m| m.event_name == "studio_cua" && m.data["summary"] == "click #95"),
        "expected studio_cua SSE event in hub"
    );
}

#[test]
fn agent_and_teamwork_cua_tool_wiring_and_prompts() {
    let sys = system_prompt();
    let psys = project_system_prompt();
    for p in [&sys, &psys] {
        for tool in [
            "studio_cua_screenshot",
            "studio_cua_mouse",
            "studio_cua_keyboard",
            "studio_cua_window",
        ] {
            assert!(p.contains(tool), "prompt missing {tool}");
        }
        assert!(
            p.contains("Studio-Native") || p.contains("Studio-native"),
            "prompt missing Studio-Native First guidance"
        );
    }

    let tw_worker = teamwork_role_prompt_preview("worker", "Build game", Path::new("/tmp/p"));
    for tool in [
        "studio_cua_screenshot",
        "studio_cua_mouse",
        "studio_cua_keyboard",
        "studio_cua_window",
    ] {
        assert!(tw_worker.contains(tool), "teamwork prompt missing {tool}");
    }

    // Verify Agent hooks wire all 4 studio_cua_* tools cleanly
    let dir = scratch("agent-hooks");
    let (_port, _data, _proj, shared) = boot_server(&dir);
    let mut agent = shared.agent.lock().unwrap().take().unwrap();
    ensure_agent_hooks(&shared, &mut agent);
    assert!(agent.hooks.studio_cua_screenshot.is_some());
    assert!(agent.hooks.studio_cua_mouse.is_some());
    assert!(agent.hooks.studio_cua_keyboard.is_some());
    assert!(agent.hooks.studio_cua_window.is_some());
    let win_res = (agent.hooks.studio_cua_window.as_ref().unwrap())(&json!({ "action": "status" })).unwrap();
    assert_eq!(win_res["ok"], true);
}
