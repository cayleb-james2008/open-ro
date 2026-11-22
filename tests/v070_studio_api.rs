//! Phase 5 (v0.7.0) HTTP & SSE integration tests for the Embedded Realtime
//! Roblox Studio 3D Viewport & Live Agent Build Viewer APIs:
//! - `GET /api/studio/stream` (SSE `status`, `studio_action`, `studio_frame`, `?since` replay)
//! - `GET /api/studio/actions`
//! - `GET /api/studio/frame` (`X-Frame-Seq`, `Cache-Control: no-store`, honest `404` when unset)
//! - `POST /api/studio/capture` (real MCP `screen_capture` PNG vs honest `503` in `local_place`)
//! - `POST /api/studio/playtest` (real MCP `start_stop_play` + `get_console_output` vs `local_place`)

use openro_rs::agent::Agent;
use openro_rs::mcp::McpClient;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn http_req(port: u16, method: &str, path: &str, body: &str) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let req = if body.is_empty() {
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let sep = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(buf.len());
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

fn read_sse_prefix(port: u16, path: &str, wait_for: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: text/event-stream\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut out = String::new();
    let mut buf = [0u8; 4096];
    for _ in 0..20 {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                out.push_str(&String::from_utf8_lossy(&buf[..n]));
                if out.contains(wait_for) {
                    break;
                }
            }
            Err(_) => {
                if out.contains(wait_for) {
                    break;
                }
            }
        }
    }
    out
}

#[test]
fn local_place_studio_stream_actions_playtest_and_honest_capture_503() {
    let model = ModelClient::new(
        "http://127.0.0.1:11434/v1".to_string(),
        "qwen2.5:3b".to_string(),
        None,
    );
    let place = LocalPlace::in_memory("Studio Viewer Local Test".to_string());
    let agent = Agent::with_local_place(model, place);
    let shared = Shared::new(Some(agent), "Studio Viewer Local Test".to_string());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });

    // 1. /api/status advertises studio_stream & local_place mode
    let (st, _, raw) = http_req(port, "GET", "/api/status", "");
    assert_eq!(st, 200);
    let status: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(status["studio_stream"], json!(true));
    assert_eq!(status["studio_mode"], json!("local_place"));

    // 2. /api/studio/frame is honest 404 & /api/studio/capture is honest 503 on local_place
    let (st, _, raw) = http_req(port, "GET", "/api/studio/frame", "");
    assert_eq!(st, 404);
    let f404: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(f404["has_live_frame"], json!(false));

    let (st, _, raw) = http_req(port, "POST", "/api/studio/capture", "{}");
    assert_eq!(st, 503);
    let c503: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(c503["ok"], json!(false));
    assert_eq!(c503["studio_mode"], json!("local_place"));

    // 3. Build Lava Obby blueprint -> emits granular PartCreated, ScriptWritten, PlaytestPassed actions
    let (st, _, raw) = http_req(
        port,
        "POST",
        "/api/ideas/build",
        &json!({ "id": "obby-starter" }).to_string(),
    );
    assert_eq!(st, 200, "build failed: {}", String::from_utf8_lossy(&raw));

    let (st, _, raw) = http_req(port, "GET", "/api/studio/actions", "");
    assert_eq!(st, 200);
    let acts: Value = serde_json::from_slice(&raw).unwrap();
    let arr = acts["actions"].as_array().unwrap();
    assert!(arr.len() >= 8, "expected >=8 actions, got {}", arr.len());
    assert!(arr.iter().any(|a| a["kind"] == "part_created"));
    assert!(arr.iter().any(|a| a["kind"] == "script_written" && a["code_snippet"].is_string()));
    assert!(arr.iter().any(|a| a["kind"] == "playtest_passed"));

    // 4. Edit a part via POST /api/place/instance -> emits part_updated with real 3D coordinates
    let (st, _, _) = http_req(
        port,
        "POST",
        "/api/place/instance",
        &json!({
            "path": "Workspace.LavaBrick",
            "properties": { "Position": [12.0, 4.0, -8.0], "Size": [8.0, 1.5, 8.0], "Color": "#EB532A" }
        })
        .to_string(),
    );
    assert_eq!(st, 200);

    // 5. Run direct Studio Playtest via POST /api/studio/playtest
    let (st, _, raw) = http_req(port, "POST", "/api/studio/playtest", "{}");
    assert_eq!(st, 200);
    let pt: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(pt["ok"], json!(true));
    assert_eq!(pt["passed"], json!(true));

    // 6. SSE /api/studio/stream?since=1 replays missed studio_action events cleanly
    let sse = read_sse_prefix(port, "/api/studio/stream?since=1", "event: studio_action");
    assert!(sse.contains("Content-Type: text/event-stream"), "sse head: {sse}");
    assert!(sse.contains("event: status"), "missing status frame: {sse}");
    assert!(sse.contains("event: studio_action"), "missing replayed studio_action: {sse}");
}

#[test]
fn live_mcp_mock_studio_captures_real_png_frames_and_streams_playtest() {
    let mock_bin = env!("CARGO_BIN_EXE_mock_studio");
    let mcp = McpClient::spawn(mock_bin).expect("spawn mock_studio MCP");
    let model = ModelClient::new(
        "http://127.0.0.1:11434/v1".to_string(),
        "qwen2.5:3b".to_string(),
        None,
    );
    let agent = Agent::new(model, Some(mcp));
    let shared = Shared::new(Some(agent), "Live MCP Studio Test".to_string());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });

    let (st, _, raw) = http_req(port, "GET", "/api/status", "");
    assert_eq!(st, 200);
    let status: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(status["studio_mode"], json!("live_mcp"));

    // 1. Explicit capture via POST /api/studio/capture returns valid PNG metadata
    let (st, _, raw) = http_req(port, "POST", "/api/studio/capture", "{}");
    assert_eq!(st, 200, "capture failed: {}", String::from_utf8_lossy(&raw));
    let cap: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(cap["ok"], json!(true));
    assert_eq!(cap["frame_seq"], json!(1));
    assert_eq!(cap["mime"], json!("image/png"));
    assert_eq!(cap["width"], json!(320));
    assert_eq!(cap["height"], json!(200));

    // 2. GET /api/studio/frame serves real PNG magic bytes with X-Frame-Seq & Cache-Control: no-store
    let (st, head, png1) = http_req(port, "GET", "/api/studio/frame", "");
    assert_eq!(st, 200);
    assert!(head.contains("Content-Type: image/png"), "head: {head}");
    assert!(head.contains("Cache-Control: no-store"), "head: {head}");
    assert!(head.contains("X-Frame-Seq: 1"), "head: {head}");
    assert_eq!(&png1[..8], b"\x89PNG\r\n\x1a\n");

    // 3. Building an idea auto-captures updated live Studio PNG frames and changes pixels!
    let (st, _, raw) = http_req(
        port,
        "POST",
        "/api/ideas/build",
        &json!({ "id": "obby-starter" }).to_string(),
    );
    assert_eq!(st, 200, "mcp build failed: {}", String::from_utf8_lossy(&raw));

    let (st, head2, png2) = http_req(port, "GET", "/api/studio/frame", "");
    assert_eq!(st, 200);
    assert!(head2.contains("X-Frame-Seq:"), "missing X-Frame-Seq: {head2}");
    assert_ne!(png1, png2, "live MCP frame must visually change after building parts/scripts");

    // 4. POST /api/studio/playtest calls MCP start_stop_play + get_console_output
    let (st, _, raw) = http_req(port, "POST", "/api/studio/playtest", "{}");
    assert_eq!(st, 200);
    let pt: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(pt["ok"], json!(true));
    assert_eq!(pt["passed"], json!(true));
    assert_eq!(pt["studio_mode"], json!("live_mcp"));
    assert!(pt["frame_seq"].as_u64().unwrap_or(0) >= 2);

    // 5. SSE /api/studio/stream?since=1 replays BOTH studio_frame and studio_action
    let sse = read_sse_prefix(port, "/api/studio/stream?since=1", "event: studio_frame");
    assert!(sse.contains("event: studio_frame"), "missing studio_frame SSE event: {sse}");
}
