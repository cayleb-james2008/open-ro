//! Integration proof for OpenRO.
//!
//! * The model is a stub HTTP server (std TcpListener) with scripted
//!   replies; the agent reaches it through the REAL ureq path, with the
//!   base URL supplied via `OPENRO_MODEL_BASE_URL`.
//! * Studio is the real `mock_studio` binary spawned as a child process
//!   via `OPENRO_MCP_COMMAND` — real stdio JSON-RPC, no in-process fakes.
//!
//! Scenario: "add a round manager that starts the round"
//!   Rune skill (no round creation) -> write_script -> playtest FAILS ->
//!   console fed back to the model -> Rune skill (with round creation) ->
//!   write_script -> playtest PASSES -> plain-words success in the thread.
//! Then revert restores the place tree to its pre-ask state.

use openro_rs::agent::Agent;
use openro_rs::mcp::McpClient;
use openro_rs::model::ModelClient;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

const CONSOLE_ERROR: &str =
    "ServerScriptService.RoundManager:4: attempt to index nil with 'Start'";

/// The scripted model replies for the full repair scenario, in order.
fn scenario_script() -> Vec<String> {
    vec![
        json!({ "skill": "round_manager", "args": { "create_round": false } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.RoundManager", "use_generated": true } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "skill": "round_manager", "args": { "create_round": true } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.RoundManager", "use_generated": true } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Added the round manager and the playtest passes — your round now starts." }).to_string(),
    ]
}

struct StubModel {
    base_url: String,
    recorded_requests: Arc<Mutex<Vec<String>>>,
    recorded_headers: Arc<Mutex<Vec<String>>>,
}

fn start_stub_model(script: Vec<String>) -> StubModel {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let queue = Arc::new(Mutex::new(VecDeque::from(script)));
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let headers = Arc::new(Mutex::new(Vec::new()));
    let q = Arc::clone(&queue);
    let r = Arc::clone(&recorded);
    let h = Arc::clone(&headers);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let (head, body) = read_http_request(&mut stream);
            assert!(
                head.starts_with("POST /chat/completions"),
                "stub model got unexpected request line: {head}"
            );
            h.lock().unwrap().push(head.clone());
            r.lock().unwrap().push(body.clone());
            let reply = q
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| json!({ "done": true, "summary": "stub queue empty" }).to_string());
            let payload = json!({
                "choices": [{ "message": { "role": "assistant", "content": reply } }]
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    StubModel {
        base_url: format!("http://127.0.0.1:{port}"),
        recorded_requests: recorded,
        recorded_headers: headers,
    }
}

fn read_http_request(stream: &mut TcpStream) -> (String, String) {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0, "connection closed mid-request");
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
        {
            break pos + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let content_length: usize = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("content-length") {
                v.trim().parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).unwrap();
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    (head, String::from_utf8_lossy(&body).to_string())
}

fn mock_studio_command() -> String {
    env!("CARGO_BIN_EXE_mock_studio").to_string()
}

fn http_request(port: u16, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let body = body.unwrap_or("");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let status: u16 = raw
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body_out = raw
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body_out)
}

fn start_server(agent: Option<Agent>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::new(agent, "Round Arena".to_string());
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    port
}

#[test]
fn rune_skill_generates_both_luau_variants() {
    let without = openro_rs::skills::generate_round_manager(false).unwrap();
    assert!(without.contains("workspace.Round."), "first pass references the round: {without}");
    assert!(
        !without.contains("Name = \"Round\""),
        "first pass must NOT create the round: {without}"
    );
    let with = openro_rs::skills::generate_round_manager(true).unwrap();
    assert!(with.contains("Name = \"Round\""), "repair pass creates the round: {with}");
    assert!(with.contains("workspace.Round."), "repair pass still starts it: {with}");
}

#[test]
fn full_loop_repairs_failed_playtest_then_revert_restores_tree() {
    // Model reached through the REAL ureq path; base URL via the env var.
    let stub = start_stub_model(scenario_script());
    std::env::set_var("OPENRO_MODEL_BASE_URL", &stub.base_url);
    std::env::set_var("OPENRO_MODEL_API_KEY", "test-key-not-real");
    // Mock Studio spawned as the real child process via the env var.
    std::env::set_var("OPENRO_MCP_COMMAND", mock_studio_command());

    let model = ModelClient::from_env();
    assert!(model.is_configured());
    let studio = McpClient::from_env().unwrap().expect("mock studio spawned");
    let mut agent = Agent::new(model, Some(studio));

    // Pre-ask state: empty place tree.
    let before = agent.place_tree().unwrap();
    assert_eq!(before["scripts"], json!({}), "tree starts empty");

    let outcome = agent
        .run_ask("add a round manager that starts the round")
        .expect("agent loop completes");
    let transcript = outcome.transcript.join("\n");

    // Stage 1: the Rune skill ran first WITHOUT round creation.
    assert!(
        transcript.contains("skill round_manager (Rune) create_round=false"),
        "transcript shows first Rune pass:\n{transcript}"
    );
    // Stage 2: the script was written and the playtest failed.
    assert!(
        transcript.contains("tool write_script ServerScriptService.RoundManager"),
        "transcript shows write_script:\n{transcript}"
    );
    assert!(
        transcript.contains("playtest") && transcript.contains("\"failed\""),
        "transcript shows failed playtest:\n{transcript}"
    );
    // Stage 3: the actual console output was captured by the loop...
    assert!(
        transcript.contains(&format!("console after failed playtest: {CONSOLE_ERROR}")),
        "transcript shows the console error:\n{transcript}"
    );
    // ...and fed back to the model (a later recorded request contains it).
    let recorded = stub.recorded_requests.lock().unwrap();
    assert!(
        recorded.iter().any(|r| r.contains(CONSOLE_ERROR)),
        "model was fed the console error; recorded requests: {recorded:?}"
    );
    // Every model call carried the session header + a browser-like UA.
    let headers = stub.recorded_headers.lock().unwrap();
    assert!(!headers.is_empty());
    for head in headers.iter() {
        let lower = head.to_lowercase();
        assert!(lower.contains("x-opencode-session:"), "session header present: {head}");
        assert!(lower.contains("mozilla/5.0"), "browser-like user-agent: {head}");
        assert!(lower.contains("authorization: bearer test-key-not-real"), "bearer key sent: {head}");
    }
    drop(recorded);
    drop(headers);

    // Stage 4: repair pass regenerated via Rune WITH round creation, rewrote, passed.
    assert!(
        transcript.contains("skill round_manager (Rune) create_round=true"),
        "transcript shows repair Rune pass:\n{transcript}"
    );
    assert!(
        transcript.contains("\"passed\""),
        "transcript shows passed playtest:\n{transcript}"
    );
    // Stage 5: the thread reports success in plain words.
    let thread = outcome.thread.join("\n");
    assert!(
        thread.contains("playtest passes"),
        "thread reports success in plain words:\n{thread}"
    );
    assert_eq!(
        outcome.summary,
        "Added the round manager and the playtest passes — your round now starts."
    );

    // Final tree state in the mock: the repaired script is in place.
    let after = agent.place_tree().unwrap();
    let source = after["scripts"]["ServerScriptService.RoundManager"]
        .as_str()
        .expect("RoundManager script exists in final tree");
    assert!(source.contains("Name = \"Round\""), "final script creates the round: {source}");
    assert!(source.contains("workspace.Round."), "final script starts the round: {source}");

    // Revert proof: the place tree returns to its pre-ask state.
    let restored = agent.revert_last().unwrap();
    assert_eq!(restored, vec!["ServerScriptService.RoundManager".to_string()]);
    let reverted = agent.place_tree().unwrap();
    assert_eq!(reverted, before, "tree is back to its pre-ask state");
}

#[test]
fn http_shell_and_no_key_honesty() {
    // No agent at all: no model key, no Studio.
    let port = start_server(None);
    let (status, html) = http_request(port, "GET", "/", None);
    assert_eq!(status, 200);
    assert!(html.contains("id=\"sidebar\""), "shell has a sidebar");
    assert!(html.contains("id=\"chat-list\""), "sidebar has the chat list");
    assert!(html.contains("New chat"), "sidebar has New chat");
    assert!(html.contains("Round Arena"), "sidebar shows the game's name");
    assert!(html.contains("id=\"thread\""), "shell has one thread");
    assert!(html.contains("id=\"composer\""), "shell has the composer row");
    assert!(
        html.contains("id=\"status-chip\"") && html.contains("Studio not connected"),
        "status chip honestly reports the bridge state"
    );
    assert!(
        html.contains("no model key configured"),
        "page honestly shows the no-key state"
    );

    let (status, body) = http_request(
        port,
        "POST",
        "/api/chat",
        Some(&json!({ "message": "add a round manager" }).to_string()),
    );
    assert_eq!(status, 503);
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["ok"], false);
    assert!(
        value["error"].as_str().unwrap().contains("no model key configured"),
        "chat refuses honestly without a key: {body}"
    );

    let (status, body) = http_request(port, "GET", "/api/status", None);
    assert_eq!(status, 200);
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["studio_connected"], false);
    assert_eq!(value["model_configured"], false);
}

#[test]
fn http_loop_and_revert_via_api() {
    let stub = start_stub_model(scenario_script());
    let model = ModelClient::new(
        stub.base_url.clone(),
        "deepseek-v4.1-flash".to_string(),
        Some("test-key-not-real".to_string()),
    );
    let studio = McpClient::spawn(&mock_studio_command()).unwrap();
    let agent = Agent::new(model, Some(studio));
    let port = start_server(Some(agent));

    // Shell over HTTP with a live bridge: the chip flips to connected.
    let (status, html) = http_request(port, "GET", "/", None);
    assert_eq!(status, 200);
    assert!(html.contains("Studio connected"), "chip shows the real connected state");
    assert!(!html.contains("no model key configured"), "no no-key banner with a key set");

    let (status, body) = http_request(
        port,
        "POST",
        "/api/chat",
        Some(&json!({ "message": "add a round manager that starts the round" }).to_string()),
    );
    assert_eq!(status, 200, "chat succeeds: {body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["ok"], true);
    let transcript = value["transcript"].to_string();
    assert!(transcript.contains("create_round=false"), "first Rune pass ran: {transcript}");
    assert!(transcript.contains(CONSOLE_ERROR), "console error surfaced: {transcript}");
    assert!(transcript.contains("create_round=true"), "repair Rune pass ran: {transcript}");

    let (status, body) = http_request(port, "GET", "/api/place-tree", None);
    assert_eq!(status, 200);
    let tree: Value = serde_json::from_str(&body).unwrap();
    assert!(
        tree["scripts"]["ServerScriptService.RoundManager"]
            .as_str()
            .unwrap()
            .contains("Name = \"Round\""),
        "tree holds the repaired script: {body}"
    );

    // One-click revert over HTTP restores the pre-ask tree.
    let (status, body) = http_request(port, "POST", "/api/revert", None);
    assert_eq!(status, 200, "revert succeeds: {body}");
    let (status, body) = http_request(port, "GET", "/api/place-tree", None);
    assert_eq!(status, 200);
    let tree: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(tree["scripts"], json!({}), "tree is empty again after revert: {body}");
}
