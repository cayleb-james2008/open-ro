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
        .run_ask("chat-1", "add a round manager that starts the round")
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
    let restored = agent.revert_chat("chat-1").unwrap();
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

// ---------------------------------------------------------------------------
// Scale-up proofs: skill system, instances, repair loop, capability check
// ---------------------------------------------------------------------------

fn mock_studio_min_command() -> String {
    env!("CARGO_BIN_EXE_mock_studio_min").to_string()
}

fn agent_with_script(script: Vec<String>) -> (Agent, StubModel) {
    let stub = start_stub_model(script);
    let model = ModelClient::new(
        stub.base_url.clone(),
        "deepseek-v4.1-flash".to_string(),
        Some("test-key-not-real".to_string()),
    );
    let studio = McpClient::spawn(&mock_studio_command()).unwrap();
    (Agent::new(model, Some(studio)), stub)
}

#[test]
fn skill_registry_generates_distinct_parameterised_luau() {
    let registry = openro_rs::skills::registry();
    let names: Vec<&str> = registry.iter().map(|s| s.name).collect();
    assert!(names.len() >= 4, "at least four skills registered: {names:?}");
    for expected in ["round_manager", "leaderboard", "day_night", "kill_brick"] {
        assert!(names.contains(&expected), "registry has {expected}: {names:?}");
    }
    // The system prompt is generated from the registry.
    let prompt = openro_rs::agent::system_prompt();
    for name in &names {
        assert!(prompt.contains(name), "system prompt lists skill {name}");
    }
    // Parameters change the generated Luau.
    let a = openro_rs::skills::generate("leaderboard", &json!({"currency": "Coins", "start_amount": 0})).unwrap();
    let b = openro_rs::skills::generate("leaderboard", &json!({"currency": "Gems", "start_amount": 50})).unwrap();
    assert!(a.contains("\"Coins\"") && a.contains("Value = 0"), "{a}");
    assert!(b.contains("\"Gems\"") && b.contains("Value = 50"), "{b}");
    assert_ne!(a, b);
    let kb = openro_rs::skills::generate("kill_brick", &json!({"damage": 25})).unwrap();
    assert!(kb.contains("TakeDamage") && kb.contains("25"), "{kb}");
    let dn = openro_rs::skills::generate("day_night", &json!({"cycle_minutes": 5})).unwrap();
    assert!(dn.contains("ClockTime") && dn.contains("5"), "{dn}");
    // Unknown skills are refused, not fabricated.
    assert!(openro_rs::skills::generate("fly_hack", &json!({})).is_err());
}

#[test]
fn one_ask_writes_two_scripts_from_two_skills() {
    // Both skills are generated BEFORE either write — the case the old
    // single-slot `last_generated` got wrong.
    let script = vec![
        json!({ "skill": "leaderboard", "args": { "currency": "Coins", "start_amount": 10 } }).to_string(),
        json!({ "skill": "day_night", "args": { "cycle_minutes": 4 } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Leaderboard", "skill": "leaderboard" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.DayNight", "skill": "day_night" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Added a leaderboard and a day/night cycle." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    let outcome = agent
        .run_ask("chat-1", "add a coin leaderboard and a day/night cycle")
        .expect("multi-script ask completes");
    assert_eq!(outcome.summary, "Added a leaderboard and a day/night cycle.");

    let tree = agent.place_tree().unwrap();
    let lb = tree["scripts"]["ServerScriptService.Leaderboard"].as_str().expect("leaderboard written");
    assert!(lb.contains("leaderstats"), "Leaderboard path holds the leaderboard Luau: {lb}");
    let dn = tree["scripts"]["ServerScriptService.DayNight"].as_str().expect("day/night written");
    assert!(dn.contains("ClockTime"), "DayNight path holds the day/night Luau: {dn}");

    let restored = agent.revert_chat("chat-1").unwrap();
    assert_eq!(restored.len(), 2, "both scripts reverted: {restored:?}");
    let tree = agent.place_tree().unwrap();
    assert_eq!(tree["scripts"], json!({}), "tree is empty after revert");
}

#[test]
fn instances_round_trip_and_revert() {
    let script = vec![
        json!({ "tool": "create_instance", "args": { "class_name": "SpawnLocation", "name": "SpawnPad", "parent": "Workspace", "properties": { "Anchored": true } } }).to_string(),
        json!({ "skill": "spawn_link", "args": { "instance_name": "SpawnPad" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.SpawnChecker", "skill": "spawn_link" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Spawn pad placed and checked." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    agent.run_ask("chat-1", "add a spawn pad").expect("instance ask completes");

    let tree = agent.place_tree().unwrap();
    let inst = &tree["instances"]["Workspace.SpawnPad"];
    assert_eq!(inst["class_name"], json!("SpawnLocation"), "instance recorded: {tree}");
    assert_eq!(inst["properties"]["Anchored"], json!(true), "properties recorded: {tree}");

    let restored = agent.revert_chat("chat-1").unwrap();
    assert_eq!(restored.len(), 2, "script and instance reverted: {restored:?}");
    let tree = agent.place_tree().unwrap();
    assert_eq!(tree["scripts"], json!({}), "scripts empty after revert");
    assert_eq!(tree["instances"], json!({}), "instances empty after revert");
}

#[test]
fn playtest_failure_repaired_by_creating_the_missing_instance() {
    let script = vec![
        json!({ "skill": "spawn_link", "args": { "instance_name": "SpawnPad" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.SpawnChecker", "skill": "spawn_link" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "tool": "create_instance", "args": { "class_name": "SpawnLocation", "name": "SpawnPad", "parent": "Workspace", "properties": {} } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Created the missing spawn pad; playtest passes." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    let outcome = agent
        .run_ask("chat-1", "make the spawn checker work")
        .expect("repair via instance creation completes");
    let transcript = outcome.transcript.join("\n");
    assert!(transcript.contains("SpawnPad is not a valid member"), "console error captured: {transcript}");
    assert!(transcript.contains("repair attempt 1"), "repair attempt recorded: {transcript}");
    assert_eq!(outcome.summary, "Created the missing spawn pad; playtest passes.");
}

#[test]
fn repair_loop_survives_a_repeated_identical_attempt() {
    let script = vec![
        json!({ "skill": "round_manager", "args": { "create_round": false } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.RoundManager", "use_generated": true } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        // The model repeats the exact same failing generation once...
        json!({ "skill": "round_manager", "args": { "create_round": false } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.RoundManager", "use_generated": true } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        // ...then actually fixes it.
        json!({ "skill": "round_manager", "args": { "create_round": true } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.RoundManager", "use_generated": true } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Round manager added; playtest passes." }).to_string(),
    ];
    let (mut agent, stub) = agent_with_script(script);
    let outcome = agent
        .run_ask("chat-1", "add a round manager that starts the round")
        .expect("loop converges despite the repeated attempt");
    let transcript = outcome.transcript.join("\n");
    assert!(transcript.contains("repair attempt 1"), "first repair recorded: {transcript}");
    assert!(transcript.contains("repair attempt 2"), "second repair recorded: {transcript}");
    // The loop told the model its rewrite was byte-identical to the failing one.
    let recorded = stub.recorded_requests.lock().unwrap();
    assert!(
        recorded.iter().any(|r| r.contains("byte-identical")),
        "model was warned about the identical rewrite; requests: {recorded:?}"
    );
}

#[test]
fn repair_loop_gives_up_honestly_when_the_model_never_fixes() {
    let mut script = Vec::new();
    for _ in 0..4 {
        script.push(json!({ "skill": "round_manager", "args": { "create_round": false } }).to_string());
        script.push(json!({ "tool": "write_script", "args": { "path": "ServerScriptService.RoundManager", "use_generated": true } }).to_string());
        script.push(json!({ "tool": "playtest", "args": {} }).to_string());
    }
    script.push(json!({ "done": true, "summary": "this done must never be reached" }).to_string());
    let (mut agent, _stub) = agent_with_script(script);
    let err = agent
        .run_ask("chat-1", "add a round manager that starts the round")
        .expect_err("an unfixable game must end in an honest failure");
    assert!(err.message.contains(CONSOLE_ERROR), "failure names the actual console output: {}", err.message);
    assert!(err.message.contains("attempt"), "failure states the attempt count: {}", err.message);
    assert!(!err.message.contains("did not finish within"), "failure is specific, not the opaque step-cap message: {}", err.message);
    // The failed ask's change set stays revertable (this was claimed in
    // the loop ledger but asserted nowhere): reverting removes the
    // script the failed ask wrote.
    let restored = agent.revert_chat("chat-1").expect("failed ask leaves a revertable change set");
    assert_eq!(restored, vec!["ServerScriptService.RoundManager".to_string()]);
    let tree = agent.place_tree().unwrap();
    assert_eq!(tree["scripts"], json!({}), "tree is back to pre-ask after reverting the failed ask");
}

#[test]
fn missing_bridge_tool_refuses_honestly() {
    let stub = start_stub_model(vec![]);
    let model = ModelClient::new(
        stub.base_url.clone(),
        "deepseek-v4.1-flash".to_string(),
        Some("test-key-not-real".to_string()),
    );
    let studio = McpClient::spawn(&mock_studio_min_command()).unwrap();
    let mut agent = Agent::new(model, Some(studio));
    let err = agent
        .run_ask("chat-1", "add a leaderboard")
        .expect_err("a bridge without write_script cannot build");
    assert!(err.message.contains("write_script"), "refusal names the missing tool: {}", err.message);
    assert!(err.message.contains("does not offer"), "refusal is honest about the bridge: {}", err.message);
}

#[test]
fn shell_has_themes_and_settings() {
    let port = start_server(None);
    let (status, html) = http_request(port, "GET", "/", None);
    assert_eq!(status, 200);
    assert!(html.contains("data-theme=\"dark\""), "dark is the default theme");
    assert!(html.contains("[data-theme=\"light\"]"), "a light theme must be defined");
    assert!(html.contains("id=\"settings\""), "settings area missing");
    assert!(html.contains("Appearance"), "appearance toggle missing from settings");
    assert!(html.contains("localStorage.setItem('openro-theme'"), "the toggle must SAVE the theme choice");
    assert!(html.contains("localStorage.getItem('openro-theme')"), "the page must READ the saved theme choice");
    assert!(html.contains(":focus-visible"), "keyboard focus must be visible");
    assert!(html.contains("for=\"composer-input\""), "composer input must be labelled");
    assert!(!html.contains("linear-gradient"), "no hue gradients (unslop gate)");
}

fn start_server_with_data(agent: Option<Agent>, data_dir: std::path::PathBuf) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::with_data_dir(agent, "Round Arena".to_string(), data_dir);
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    port
}

#[test]
fn chats_persist_across_restart_and_switch() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-data")
        .join(format!("chats-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let data_dir = root.join("openro-data");

    // First run: one full ask in the default chat.
    let stub = start_stub_model(scenario_script());
    let model = ModelClient::new(
        stub.base_url.clone(),
        "deepseek-v4.1-flash".to_string(),
        Some("test-key-not-real".to_string()),
    );
    let studio = McpClient::spawn(&mock_studio_command()).unwrap();
    let agent = Agent::new(model, Some(studio));
    let port = start_server_with_data(Some(agent), data_dir.clone());

    let (status, body) = http_request(
        port,
        "POST",
        "/api/chat",
        Some(&json!({ "message": "add a round manager that starts the round" }).to_string()),
    );
    assert_eq!(status, 200, "chat succeeds: {body}");

    // A second, empty chat.
    let (status, body) = http_request(port, "POST", "/api/chats", Some("{}"));
    assert_eq!(status, 201, "create chat: {body}");
    let second: Value = serde_json::from_str(&body).unwrap();
    let second_id = second["id"].as_str().unwrap().to_string();
    let (status, body) = http_request(port, "GET", "/api/thread", None);
    assert_eq!(status, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["thread"], json!([]), "the new chat starts empty: {body}");

    // "Restart": a fresh server over the same data dir.
    let port2 = start_server_with_data(None, data_dir.clone());
    let (status, body) = http_request(port2, "GET", "/api/chats", None);
    assert_eq!(status, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    let chats = v["chats"].as_array().expect("chats list");
    assert_eq!(chats.len(), 2, "both chats survive the restart: {body}");
    assert!(
        chats.iter().any(|c| c["title"].as_str().unwrap_or("").contains("round manager")),
        "the first chat is titled from its ask: {body}"
    );
    let first_id = chats[0]["id"].as_str().unwrap().to_string();
    assert_ne!(first_id, second_id);

    // Switch back to the first chat: its whole thread is still there.
    let (status, _) = http_request(
        port2,
        "POST",
        "/api/chats/select",
        Some(&json!({ "id": first_id }).to_string()),
    );
    assert_eq!(status, 200);
    let (status, body) = http_request(port2, "GET", "/api/thread", None);
    assert_eq!(status, 200);
    assert!(body.contains("playtest passes"), "first chat's thread persisted: {body}");
    assert!(body.contains("You: add a round manager"), "the ask itself persisted: {body}");
}

// ---------------------------------------------------------------------------
// Regression tests from the scale-up thermonuclear + checks + lens
// reviews. Each test names the defect it pins down.
// ---------------------------------------------------------------------------

fn agent_with_mock_command(script: Vec<String>, command: String) -> (Agent, StubModel) {
    let stub = start_stub_model(script);
    let model = ModelClient::new(
        stub.base_url.clone(),
        "deepseek-v4.1-flash".to_string(),
        Some("test-key-not-real".to_string()),
    );
    let studio = McpClient::spawn(&command).unwrap();
    (Agent::new(model, Some(studio)), stub)
}

/// F1: write_script accepted hand-written `source`, bypassing the Rune
/// skill system entirely — the product's core invariant. The bypass is
/// refused and fed back; the skill path still works.
#[test]
fn luau_comes_only_from_skills() {
    let script = vec![
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Raw", "source": "print('hand-written')" } }).to_string(),
        json!({ "skill": "leaderboard", "args": { "currency": "Coins", "start_amount": 5 } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Leaderboard", "skill": "leaderboard" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Leaderboard added." }).to_string(),
    ];
    let (mut agent, stub) = agent_with_script(script);
    let outcome = agent
        .run_ask("chat-1", "add a leaderboard")
        .expect("ask completes via the skill path");
    let recorded = stub.recorded_requests.lock().unwrap();
    assert!(
        recorded.iter().any(|r| r.contains("Luau comes only from")),
        "the model was told hand-written source is refused: {recorded:?}"
    );
    let tree = agent.place_tree().unwrap();
    assert!(
        tree["scripts"].get("ServerScriptService.Raw").is_none(),
        "no hand-written script landed in the tree: {tree}"
    );
    let lb = tree["scripts"]["ServerScriptService.Leaderboard"].as_str().unwrap();
    assert!(lb.contains("leaderstats"), "the written script is the skill's Luau: {lb}");
    assert_eq!(outcome.summary, "Leaderboard added.");
}

/// F2: the leaderboard currency was interpolated unescaped into a Luau
/// string literal — a crafted currency injected arbitrary Luau.
#[test]
fn leaderboard_currency_cannot_inject_luau() {
    let out = openro_rs::skills::generate(
        "leaderboard",
        &json!({ "currency": "Coins\"; game.Players:ClearAllChildren(); x = \"", "start_amount": 0 }),
    )
    .unwrap();
    assert!(
        out.contains("Coins\\\";"),
        "the quote is escaped inside the literal: {out}"
    );
    assert!(
        !out.contains("stat.Name = \"Coins\";"),
        "the literal cannot be broken out of: {out}"
    );
}

/// F12: wrong-typed skill args were silently coerced to defaults.
#[test]
fn skill_args_are_validated_not_coerced() {
    let err = openro_rs::skills::generate("kill_brick", &json!({ "damage": "not-a-number" }))
        .expect_err("a string damage must be refused");
    assert!(err.contains("must be an integer"), "{err}");
    assert!(openro_rs::skills::generate("leaderboard", &json!({ "currency": 42 })).is_err());
    assert!(openro_rs::skills::generate("round_manager", &json!({ "create_round": "yes" })).is_err());
    assert!(openro_rs::skills::generate("spawn_link", &json!({ "instance_name": "Pad; evil()" })).is_err());
    // Absent args still take their documented defaults.
    assert!(openro_rs::skills::generate("kill_brick", &json!({})).is_ok());
}

/// F3 (change-making case): a done with changes no playtest has passed
/// is fed back, not accepted.
#[test]
fn done_with_unproven_changes_is_fed_back() {
    let script = vec![
        json!({ "skill": "leaderboard", "args": { "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Leaderboard", "skill": "leaderboard" } }).to_string(),
        json!({ "done": true, "summary": "Done — trust me." }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Leaderboard added and playtested." }).to_string(),
    ];
    let (mut agent, stub) = agent_with_script(script);
    let outcome = agent
        .run_ask("chat-1", "add a leaderboard")
        .expect("ask completes after proving the change");
    assert_eq!(outcome.summary, "Leaderboard added and playtested.");
    let transcript = outcome.transcript.join("\n");
    assert!(transcript.contains("done refused"), "the gate fired: {transcript}");
    let recorded = stub.recorded_requests.lock().unwrap();
    assert!(
        recorded.iter().any(|r| r.contains("no playtest has passed since")),
        "the model was told why done was refused: {recorded:?}"
    );
}

/// F4 (client level): a bridge tool error (isError) is an Err, never a
/// success string the agent can narrate.
#[test]
fn bridge_tool_errors_are_errors() {
    let command = format!("env MOCK_ERR_TOOLS=1 {}", mock_studio_command());
    let mut client = McpClient::spawn(&command).unwrap();
    let err = client
        .call_tool("write_script", json!({ "path": "X.Y", "source": "z" }))
        .expect_err("an isError result must surface as an error");
    assert!(err.contains("bridge exploded"), "{err}");
    // And an unknown tool on the normal mock errors too.
    let mut client2 = McpClient::spawn(&mock_studio_command()).unwrap();
    assert!(client2.call_tool("definitely_not_a_tool", json!({})).is_err());
}

/// F6: a silent bridge hangs forever no longer — requests time out.
#[test]
fn silent_bridge_times_out() {
    let started = std::time::Instant::now();
    let res = McpClient::spawn_with_timeout("sleep 5", std::time::Duration::from_secs(1));
    assert!(res.is_err(), "a bridge that never answers cannot connect");
    let err = res.err().unwrap();
    assert!(err.contains("did not answer"), "{err}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "the timeout fired promptly: {:?}",
        started.elapsed()
    );
}

/// F7: a playtest verdict outside the documented format is "unproven",
/// never a pass — and never misread as a failure either. The loose
/// plain-text shape "Playtest passed. 0 failures, 0 errors." used to be
/// substring-matched as a FAILURE.
#[test]
fn playtest_verdict_must_be_in_documented_format() {
    let script = vec![
        json!({ "skill": "leaderboard", "args": { "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Leaderboard", "skill": "leaderboard" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "must never be reached" }).to_string(),
    ];
    let command = format!("env MOCK_PLAYTEST_TEXT=1 {}", mock_studio_command());
    let (mut agent, _stub) = agent_with_mock_command(script, command);
    let err = agent
        .run_ask("chat-1", "add a leaderboard")
        .expect_err("an unreadable verdict cannot be called a pass");
    assert!(
        err.message.contains("verdict could not be read"),
        "failure names the format problem: {}", err.message
    );
    let thread = err.thread.join("\n");
    assert!(
        thread.contains("not in the documented format"),
        "thread states the verdict was unproven: {thread}"
    );
    assert!(
        !thread.contains("Playtest passed."),
        "the loose text was never narrated as a pass: {thread}"
    );
}

/// Mut-A (checks finder): every revert test used to start from an empty
/// tree, so restoring PRE-EXISTING content was unproven. Here ask 2
/// overwrites a script, deletes an instance and creates another;
/// revert must restore the exact originals. Ask 3 deletes a script;
/// revert must bring it back.
#[test]
fn revert_restores_preexisting_content_exactly() {
    let ask1 = vec![
        json!({ "skill": "round_manager", "args": { "create_round": true } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.RoundManager", "skill": "round_manager" } }).to_string(),
        json!({ "tool": "create_instance", "args": { "class_name": "SpawnLocation", "name": "SpawnPad", "parent": "Workspace", "properties": { "Anchored": true } } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Round manager and spawn pad in." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(ask1);
    agent.run_ask("chat-1", "set up the round").expect("ask 1 completes");
    let original_source = openro_rs::skills::generate("round_manager", &json!({ "create_round": true })).unwrap();

    // A second ask against the same agent + bridge, new stub script.
    let ask2 = vec![
        json!({ "skill": "day_night", "args": { "cycle_minutes": 3 } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.RoundManager", "skill": "day_night" } }).to_string(),
        json!({ "tool": "delete_instance", "args": { "path": "Workspace.SpawnPad" } }).to_string(),
        json!({ "tool": "create_instance", "args": { "class_name": "Part", "name": "NewPad", "parent": "Workspace", "properties": {} } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Changed things." }).to_string(),
    ];
    let stub2 = start_stub_model(ask2);
    agent.model = ModelClient::new(
        stub2.base_url.clone(),
        "deepseek-v4.1-flash".to_string(),
        Some("test-key-not-real".to_string()),
    );
    agent.run_ask("chat-1", "change things").expect("ask 2 completes");
    let tree = agent.place_tree().unwrap();
    assert!(tree["scripts"]["ServerScriptService.RoundManager"].as_str().unwrap().contains("ClockTime"), "ask 2 overwrote the script");
    assert!(tree["instances"].get("Workspace.SpawnPad").is_none(), "ask 2 deleted the pad");

    let restored = agent.revert_chat("chat-1").expect("revert of ask 2 works");
    assert_eq!(restored.len(), 3, "all three changes reverted: {restored:?}");
    let tree = agent.place_tree().unwrap();
    assert_eq!(
        tree["scripts"]["ServerScriptService.RoundManager"].as_str().unwrap(),
        original_source,
        "the overwritten script is restored byte-for-byte"
    );
    let pad = &tree["instances"]["Workspace.SpawnPad"];
    assert_eq!(pad["class_name"], json!("SpawnLocation"), "the deleted instance is restored: {tree}");
    assert_eq!(pad["properties"]["Anchored"], json!(true), "with its original properties");
    assert!(tree["instances"].get("Workspace.NewPad").is_none(), "the created instance is removed");

    // Ask 3: delete_script as a model step, then revert restores it.
    let ask3 = vec![
        json!({ "tool": "delete_script", "args": { "path": "ServerScriptService.RoundManager" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Deleted the round manager." }).to_string(),
    ];
    let stub3 = start_stub_model(ask3);
    agent.model = ModelClient::new(
        stub3.base_url.clone(),
        "deepseek-v4.1-flash".to_string(),
        Some("test-key-not-real".to_string()),
    );
    agent.run_ask("chat-1", "delete the round manager").expect("ask 3 completes");
    let tree = agent.place_tree().unwrap();
    assert!(tree["scripts"].get("ServerScriptService.RoundManager").is_none(), "script deleted");
    agent.revert_chat("chat-1").expect("revert of ask 3 works");
    let tree = agent.place_tree().unwrap();
    assert_eq!(
        tree["scripts"]["ServerScriptService.RoundManager"].as_str().unwrap(),
        original_source,
        "the deleted script is restored byte-for-byte"
    );
}

/// F4 (capability): the required-tool check covers the instance tools,
/// not just the four the old check named.
#[test]
fn bridge_missing_an_instance_tool_refuses_honestly() {
    let script = vec![json!({ "done": true, "summary": "x" }).to_string()];
    let command = format!("env MOCK_DROP=create_instance {}", mock_studio_command());
    let (mut agent, _stub) = agent_with_mock_command(script, command);
    let err = agent
        .run_ask("chat-1", "add a spawn pad")
        .expect_err("a bridge without create_instance cannot build");
    assert!(err.message.contains("create_instance"), "refusal names the missing tool: {}", err.message);
    assert!(err.message.contains("does not offer"), "{}", err.message);
}

/// Lens HIGH: Revert with nothing waiting fabricated "Reverted the
/// last change set." — persisted into the thread, repeatably.
#[test]
fn http_revert_with_nothing_waiting_is_honest() {
    let (agent, _stub) = agent_with_script(vec![]);
    let port = start_server(Some(agent));
    let (status, body) = http_request(port, "POST", "/api/revert", Some(""));
    assert_eq!(status, 200, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["ok"], false, "nothing-to-revert is not a success: {body}");
    let (status, body) = http_request(port, "POST", "/api/revert", Some(""));
    assert_eq!(status, 200);
    let (_, body) = http_request(port, "GET", "/api/thread", None);
    assert!(body.contains("Nothing to revert"), "thread states it plainly: {body}");
    assert!(
        !body.contains("Reverted the last change set."),
        "no fabricated revert in the thread: {body}"
    );
}

/// F9: change sets belong to their chat — reverting in another chat
/// finds nothing, and the owning chat's set survives.
#[test]
fn http_revert_is_per_chat() {
    let script = vec![
        json!({ "skill": "leaderboard", "args": { "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Leaderboard", "skill": "leaderboard" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Leaderboard added." }).to_string(),
    ];
    let (agent, _stub) = agent_with_script(script);
    let port = start_server(Some(agent));
    let (status, body) = http_request(
        port,
        "POST",
        "/api/chat",
        Some(&json!({ "message": "add a leaderboard" }).to_string()),
    );
    assert_eq!(status, 200, "{body}");
    // A second chat becomes active; revert there finds nothing.
    let (status, _) = http_request(port, "POST", "/api/chats", Some("{}"));
    assert_eq!(status, 201);
    let (status, body) = http_request(port, "POST", "/api/revert", Some(""));
    assert_eq!(status, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["ok"], false, "the other chat has nothing to revert: {body}");
    let (_, body) = http_request(port, "GET", "/api/thread", None);
    assert!(body.contains("Nothing to revert"), "chat 2's thread tells its own truth: {body}");
    // Back in the first chat, its change set is intact and reverts.
    let (_, body) = http_request(port, "GET", "/api/chats", None);
    let chats: Value = serde_json::from_str(&body).unwrap();
    let first_id = chats["chats"][0]["id"].as_str().unwrap().to_string();
    let (status, _) = http_request(
        port,
        "POST",
        "/api/chats/select",
        Some(&json!({ "id": first_id }).to_string()),
    );
    assert_eq!(status, 200);
    let (status, body) = http_request(port, "POST", "/api/revert", Some(""));
    assert_eq!(status, 200, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["ok"], true, "the owning chat reverts: {body}");
    let (_, body) = http_request(port, "GET", "/api/place-tree", None);
    let tree: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(tree["scripts"], json!({}), "the script is gone after the revert");
}

/// Lens MEDIUM: the user's message (and the refusal) used to vanish in
/// the no-key state — nothing was persisted. Both now stay in the
/// thread. A mid-ask failure's lines persist too.
#[test]
fn http_refused_and_failed_asks_stay_in_the_thread() {
    // No agent at all (no key): the exchange is recorded.
    let port = start_server(None);
    let (status, _) = http_request(
        port,
        "POST",
        "/api/chat",
        Some(&json!({ "message": "make a lava obby with checkpoints" }).to_string()),
    );
    assert_eq!(status, 503);
    let (_, body) = http_request(port, "GET", "/api/thread", None);
    assert!(body.contains("You: make a lava obby with checkpoints"), "the ask persists: {body}");
    assert!(body.contains("no model key configured"), "the refusal persists: {body}");

    // A model that never produces a valid step: the ask fails honestly
    // and the thread keeps the exchange. (One garbage reply per step
    // the loop allows — MAX_STEPS is 40 since the completion round.)
    let garbage: Vec<String> = (0..40).map(|_| "this is not json at all".to_string()).collect();
    let (agent, _stub) = agent_with_script(garbage);
    let port = start_server(Some(agent));
    let (status, _) = http_request(
        port,
        "POST",
        "/api/chat",
        Some(&json!({ "message": "build something" }).to_string()),
    );
    assert_eq!(status, 502);
    let (_, body) = http_request(port, "GET", "/api/thread", None);
    assert!(body.contains("You: build something"), "the ask persists: {body}");
    assert!(body.contains("did not finish within"), "the failure persists: {body}");
}

/// F11 + lens pins: the game name is escaped in the page; the faint
/// text values meet AA contrast in both themes.
#[test]
fn shell_escapes_game_name_and_meets_contrast() {
    let html = openro_rs::server::render_page("<script>alert(1)</script>", false, false);
    assert!(html.contains("&lt;script&gt;alert(1)"), "game name is escaped");
    assert!(!html.contains("<title>OpenRO — <script>"), "no raw injection into the title");
    assert!(html.contains("--faint: #8e8ea5"), "dark faint meets AA on the dark surfaces");
    assert!(html.contains("--faint: #62627a"), "light faint meets AA on the light surfaces");
    assert!(html.contains("no-studio-banner"), "the no-Studio state gets a banner, not just a chip");
    assert!(html.contains("chat-btn"), "chat list items are real buttons");
}

// ---------------------------------------------------------------------------
// Regression tests from thermonuclear review ROUND 2.
// ---------------------------------------------------------------------------

/// H1: a vacuous playtest (nothing changed, so nothing to break) used
/// to suppress the empty-handed claim flag — a clean false "done".
#[test]
fn vacuous_playtest_does_not_excuse_a_build_claim() {
    let script = vec![
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Built the leaderboard and added coins for you." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    let outcome = agent
        .run_ask("chat-1", "build me a leaderboard")
        .expect("the ask completes (with the flag in the thread)");
    let thread = outcome.thread.join("\n");
    assert!(
        thread.contains("Note — this ask made no changes to the game."),
        "a build claim with zero changes must get the ledger line even after a playtest: {thread}"
    );
    let tree = agent.place_tree().unwrap();
    assert_eq!(tree["scripts"], json!({}), "nothing was built");
}

/// L1: the flag must not accuse honest negations.
#[test]
fn honest_negation_is_not_flagged_as_a_build_claim() {
    let script = vec![
        json!({ "done": true, "summary": "Nothing was built or added — the game already had a leaderboard." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    let outcome = agent
        .run_ask("chat-1", "does the game have a leaderboard?")
        .expect("the ask completes");
    let thread = outcome.thread.join("\n");
    assert!(
        !thread.contains("made no changes to the game"),
        "an honest negation must not get the ledger line: {thread}"
    );
}

/// H3: a failed tool call must not poison the change set — the
/// checkpoint is recorded only after the change succeeds.
#[test]
fn failed_tool_call_does_not_poison_the_change_set() {
    let script = vec![
        json!({ "tool": "delete_script", "args": { "path": "ServerScriptService.Ghost" } }).to_string(),
        json!({ "skill": "leaderboard", "args": { "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Leaderboard", "skill": "leaderboard" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Leaderboard added." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    agent
        .run_ask("chat-1", "remove the ghost script and add a leaderboard")
        .expect("the ask completes honestly despite the failed delete");
    let restored = agent
        .revert_chat("chat-1")
        .expect("revert must not trip over a change that never happened");
    assert_eq!(restored.len(), 1, "only the real change reverts: {restored:?}");
    let tree = agent.place_tree().unwrap();
    assert_eq!(tree["scripts"], json!({}), "the leaderboard is reverted away");
}

/// H2: revert is failure-atomic and state-checked — when another chat
/// already achieved part of the goal state, revert still succeeds,
/// and a failed revert would keep its set for a retry.
#[test]
fn revert_survives_cross_chat_interference() {
    let script = vec![
        // Chat A: create the pad and its checker script.
        json!({ "tool": "create_instance", "args": { "class_name": "SpawnLocation", "name": "SpawnPad", "parent": "Workspace", "properties": {} } }).to_string(),
        json!({ "skill": "spawn_link", "args": { "instance_name": "SpawnPad" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.SpawnChecker", "skill": "spawn_link" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Spawn pad placed and checked." }).to_string(),
        // Chat B: remove both again.
        json!({ "tool": "delete_instance", "args": { "path": "Workspace.SpawnPad" } }).to_string(),
        json!({ "tool": "delete_script", "args": { "path": "ServerScriptService.SpawnChecker" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Removed the spawn pad and its checker." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    agent.run_ask("chat-a", "add a spawn pad").expect("chat A completes");
    agent.run_ask("chat-b", "remove the spawn pad").expect("chat B completes");
    // A's revert targets state B already achieved (both absent):
    // that is success, not an error — and it must not consume B's set.
    agent
        .revert_chat("chat-a")
        .expect("reverting to an already-achieved state succeeds");
    // B's revert then restores the pad and the script for real.
    let restored = agent.revert_chat("chat-b").expect("chat B reverts");
    assert_eq!(restored.len(), 2, "both of B's changes restore: {restored:?}");
    let tree = agent.place_tree().unwrap();
    assert!(
        tree["instances"].get("Workspace.SpawnPad").is_some(),
        "the pad is back: {tree}"
    );
    assert!(
        tree["scripts"].get("ServerScriptService.SpawnChecker").is_some(),
        "the checker script is back: {tree}"
    );
}

/// M4: change sets form a per-chat STACK — a second change-making ask
/// must not silently orphan the first ask's changes; each ask's set
/// stays revertable, most recent first.
#[test]
fn change_sets_stack_across_asks_in_one_chat() {
    let script = vec![
        json!({ "skill": "leaderboard", "args": { "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Leaderboard", "skill": "leaderboard" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Leaderboard added." }).to_string(),
        json!({ "skill": "day_night", "args": { "cycle_minutes": 5 } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.DayNight", "skill": "day_night" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Day/night added." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    agent.run_ask("chat-1", "add a leaderboard").expect("ask 1 completes");
    agent.run_ask("chat-1", "add day/night").expect("ask 2 completes");
    // First revert pops ask 2's set only.
    let restored = agent.revert_chat("chat-1").expect("revert of ask 2 works");
    assert_eq!(restored.len(), 1, "ask 2's change reverts: {restored:?}");
    let tree = agent.place_tree().unwrap();
    assert!(tree["scripts"].get("ServerScriptService.DayNight").is_none(), "day/night gone");
    assert!(tree["scripts"].get("ServerScriptService.Leaderboard").is_some(), "ask 1's work is untouched");
    // Second revert pops ask 1's set — it was never discarded.
    let restored = agent.revert_chat("chat-1").expect("revert of ask 1 works");
    assert_eq!(restored.len(), 1);
    let tree = agent.place_tree().unwrap();
    assert_eq!(tree["scripts"], json!({}), "everything is reverted");
    assert!(agent.revert_chat("chat-1").is_err(), "the stack is empty now");
}

/// M1: a healthy payload carrying "error": null is not a bridge error.
#[test]
fn null_error_field_is_a_healthy_payload() {
    let command = format!("env MOCK_NULL_ERROR=1 {}", mock_studio_command());
    let script = vec![
        json!({ "skill": "leaderboard", "args": { "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Leaderboard", "skill": "leaderboard" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Leaderboard added." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_mock_command(script, command);
    agent
        .run_ask("chat-1", "add a leaderboard")
        .expect("a bridge whose payloads carry error:null must work end to end");
}

/// M2: a get_console failure after a failed verdict must not abort
/// the repair loop — the console embedded in the verdict is used.
#[test]
fn console_outage_does_not_abort_the_repair_loop() {
    let command = format!("env MOCK_ERR_CONSOLE=1 {}", mock_studio_command());
    let script = vec![
        json!({ "skill": "spawn_link", "args": { "instance_name": "SpawnPad" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.SpawnChecker", "skill": "spawn_link" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "tool": "create_instance", "args": { "class_name": "SpawnLocation", "name": "SpawnPad", "parent": "Workspace", "properties": {} } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Created the missing spawn pad; playtest passes." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_mock_command(script, command);
    let outcome = agent
        .run_ask("chat-1", "link the spawn pad")
        .expect("the repair loop survives a console outage");
    assert_eq!(outcome.summary, "Created the missing spawn pad; playtest passes.");
}

/// L2/L5: skill args reject control characters and out-of-range
/// numbers instead of generating broken Luau.
#[test]
fn skill_args_reject_control_chars_and_bad_ranges() {
    assert!(
        openro_rs::skills::generate("leaderboard", &json!({ "currency": "Co\u{0}ins" })).is_err(),
        "a NUL in the currency must be refused"
    );
    assert!(
        openro_rs::skills::generate("day_night", &json!({ "cycle_minutes": 0 })).is_err(),
        "cycle_minutes 0 would divide by zero in the Luau"
    );
    assert!(
        openro_rs::skills::generate("kill_brick", &json!({ "damage": -5 })).is_err(),
        "negative damage would heal"
    );
    assert!(
        openro_rs::skills::generate("leaderboard", &json!({ "start_amount": -1 })).is_err(),
        "negative start amount is out of range"
    );
    // And the valid edges still generate.
    assert!(openro_rs::skills::generate("day_night", &json!({ "cycle_minutes": 1 })).is_ok());
    assert!(openro_rs::skills::generate("kill_brick", &json!({ "damage": 1 })).is_ok());
}

/// L4: create_instance names are validated before the bridge sees them.
#[test]
fn create_instance_rejects_bad_names_before_the_bridge() {
    let script = vec![
        json!({ "tool": "create_instance", "args": { "class_name": "Part", "name": "Bad/Name", "parent": "Workspace", "properties": {} } }).to_string(),
        json!({ "done": true, "summary": "Understood." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    let outcome = agent
        .run_ask("chat-1", "make a part")
        .expect("the ask completes with the error fed back");
    let transcript = outcome.transcript.join("\n");
    assert!(
        transcript.contains("single non-empty name"),
        "the bad name is refused by the agent's own validation: {transcript}"
    );
    let tree = agent.place_tree().unwrap();
    assert_eq!(tree["instances"], json!({}), "nothing reached the place");
}

/// L3: change sets persist across a restart (same data file), so a
/// pre-restart change is still revertable afterwards.
#[test]
fn change_sets_survive_a_restart() {
    let dir = std::env::temp_dir().join(format!("openro-changes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let changes_file = dir.join("changes.json");
    let script = vec![
        json!({ "skill": "leaderboard", "args": { "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Leaderboard", "skill": "leaderboard" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Leaderboard added." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    agent.set_changes_file(changes_file.clone());
    agent.run_ask("chat-1", "add a leaderboard").expect("ask completes");
    assert!(changes_file.exists(), "the change set is written to disk");
    drop(agent);
    // A fresh agent (fresh bridge, empty place) loads the set; the
    // state-checked revert treats the already-absent script as
    // restored rather than erroring.
    let (mut agent2, _stub2) = agent_with_script(vec![]);
    agent2.set_changes_file(changes_file);
    let restored = agent2
        .revert_chat("chat-1")
        .expect("the pre-restart change set is still revertable");
    assert_eq!(restored.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Regression tests from thermonuclear review ROUND 3.
// ---------------------------------------------------------------------------

/// F1: the claim check is sentence-scoped with suppressors — honest
/// negations and pre-existing-work reports stay clean in every
/// position, and synonyms / cross-sentence tricks do not dodge it.
#[test]
fn claim_flag_sentence_scoped_both_directions() {
    let cases: Vec<(&str, bool)> = vec![
        ("I built nothing.", false),
        ("Nothing at all in this place was built by me.", false),
        ("The leaderboard was already built before you asked.", false),
        ("Constructed the leaderboard and installed the spawn pad.", true),
        ("No problems were found. Built the leaderboard from scratch.", true),
        ("I made a leaderboard for you.", true),
    ];
    for (summary, should_flag) in cases {
        let script = vec![json!({ "done": true, "summary": summary }).to_string()];
        let (mut agent, _stub) = agent_with_script(script);
        let outcome = agent
            .run_ask("chat-1", "do the thing")
            .expect("the ask completes");
        let thread = outcome.thread.join("\n");
        assert_eq!(
            thread.contains("Note — this ask made no changes to the game."),
            should_flag,
            "summary {summary:?} should_flag={should_flag}: {thread}"
        );
    }
}

/// F2: revert is identity-checked — it must REFUSE to destroy another
/// chat's newer work at the same path, and the two reverts then
/// converge in stack order.
#[test]
fn revert_refuses_to_destroy_another_chats_newer_work() {
    let script = vec![
        // Chat A writes Shared from the leaderboard skill.
        json!({ "skill": "leaderboard", "args": { "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Shared", "skill": "leaderboard" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Leaderboard in." }).to_string(),
        // Chat B overwrites the same path with day/night.
        json!({ "skill": "day_night", "args": { "cycle_minutes": 5 } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Shared", "skill": "day_night" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Day/night in." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    agent.run_ask("chat-a", "add a leaderboard").expect("A completes");
    agent.run_ask("chat-b", "add day/night instead").expect("B completes");
    // A's revert must refuse: the path now holds B's work.
    let err = agent
        .revert_chat("chat-a")
        .expect_err("reverting A over B's newer work must refuse");
    assert!(err.contains("has changed since this ask"), "the refusal names the divergence: {err}");
    let tree = agent.place_tree().unwrap();
    let shared = tree["scripts"]["ServerScriptService.Shared"].as_str().unwrap();
    assert!(shared.contains("ClockTime"), "B's work is intact: {shared}");
    // B reverts first (restoring A's content), then A can revert.
    agent.revert_chat("chat-b").expect("B reverts");
    let tree = agent.place_tree().unwrap();
    let shared = tree["scripts"]["ServerScriptService.Shared"].as_str().unwrap();
    assert!(shared.contains("leaderstats"), "A's content is restored: {shared}");
    agent.revert_chat("chat-a").expect("A reverts now");
    let tree = agent.place_tree().unwrap();
    assert_eq!(tree["scripts"], json!({}), "both reverts converge to the original empty tree");
}

/// F4: an error object with a code but no message is a healthy
/// payload, not a bridge error.
#[test]
fn error_object_without_a_message_is_healthy() {
    let command = format!("env MOCK_ERR_CODE_ONLY=1 {}", mock_studio_command());
    let script = vec![
        json!({ "skill": "leaderboard", "args": { "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Leaderboard", "skill": "leaderboard" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Leaderboard added." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_mock_command(script, command);
    agent
        .run_ask("chat-1", "add a leaderboard")
        .expect("a passing verdict with error:{code:0} must be accepted");
}

/// F5/F6: a corrupt or malformed changes.json is backed up and starts
/// empty — never silently dropped, never coerced into a destructive
/// revert.
#[test]
fn corrupt_changes_file_is_backed_up_never_coerced() {
    let dir = std::env::temp_dir().join(format!("openro-changes-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Case 1: unparseable.
    let f1 = dir.join("changes.json");
    std::fs::write(&f1, "{not json at all").unwrap();
    let (mut agent, _stub) = agent_with_script(vec![]);
    agent.set_changes_file(f1.clone());
    assert!(dir.join("changes.json.bak").exists(), "unparseable file is backed up");
    assert_eq!(
        std::fs::read_to_string(dir.join("changes.json.bak")).unwrap(),
        "{not json at all"
    );
    assert!(agent.revert_chat("chat-1").is_err(), "nothing to revert from a corrupt file");
    // Case 2: a malformed entry (numeric prev) — the whole file is
    // treated as corrupt rather than coercing prev to "was absent".
    let f2 = dir.join("changes2.json");
    std::fs::write(
        &f2,
        "{\"chat-1\": [[{\"kind\": \"script\", \"path\": \"ServerScriptService.X\", \"prev\": 12345, \"post\": null}]]}",
    )
    .unwrap();
    let (mut agent2, _stub2) = agent_with_script(vec![]);
    agent2.set_changes_file(f2.clone());
    assert!(dir.join("changes2.json.bak").exists(), "a malformed entry backs the file up");
    assert!(
        agent2.revert_chat("chat-1").is_err(),
        "no destructive revert is built from an unreadable record"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// F7: class_name is validated like name.
#[test]
fn create_instance_rejects_bad_class_names() {
    let script = vec![
        json!({ "tool": "create_instance", "args": { "class_name": "Pa\u{1}rt", "name": "Thing", "parent": "Workspace", "properties": {} } }).to_string(),
        json!({ "done": true, "summary": "Understood." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    let outcome = agent.run_ask("chat-1", "make a thing").expect("ask completes");
    let transcript = outcome.transcript.join("\n");
    assert!(
        transcript.contains("class_name must be"),
        "the bad class name is refused by the agent: {transcript}"
    );
}

/// F8: numeric ceilings — absurd values are refused, sane edges pass.
#[test]
fn skill_args_reject_absurd_magnitudes() {
    assert!(openro_rs::skills::generate("day_night", &json!({ "cycle_minutes": i64::MAX })).is_err());
    assert!(openro_rs::skills::generate("kill_brick", &json!({ "damage": i64::MAX })).is_err());
    assert!(openro_rs::skills::generate("leaderboard", &json!({ "start_amount": i64::MAX })).is_err());
    assert!(openro_rs::skills::generate("day_night", &json!({ "cycle_minutes": 10_080 })).is_ok());
    assert!(openro_rs::skills::generate("kill_brick", &json!({ "damage": 100_000 })).is_ok());
}

/// F3: an ask's outcome lines are filed into the chat the ask ran in,
/// even if the user switches chats while it runs. (Slow stub: the
/// first model reply is delayed so the switch lands mid-ask.)
#[test]
fn outcome_lines_follow_the_ask_not_the_active_chat() {
    // Slow stub model: same shape as start_stub_model, with a delay
    // before the first reply.
    let script = vec![
        json!({ "skill": "leaderboard", "args": { "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Leaderboard", "skill": "leaderboard" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Leaderboard added." }).to_string(),
    ];
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let stub_port = listener.local_addr().unwrap().port();
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(script)));
    std::thread::spawn(move || {
        let mut first = true;
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let (_head, _body) = read_http_request(&mut stream);
            if first {
                first = false;
                std::thread::sleep(std::time::Duration::from_millis(600));
            }
            let reply = queue
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
    let model = ModelClient::new(
        format!("http://127.0.0.1:{stub_port}"),
        "deepseek-v4.1-flash".to_string(),
        Some("test-key-not-real".to_string()),
    );
    let studio = McpClient::spawn(&mock_studio_command()).unwrap();
    let agent = Agent::new(model, Some(studio));
    let dir = std::env::temp_dir().join(format!("openro-f3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let port = start_server_with_data(Some(agent), dir.clone());

    // The ask runs in the default (first) chat, in a background thread.
    let ask_thread = std::thread::spawn(move || {
        http_request(port, "POST", "/api/chat", Some("{\"message\": \"add a leaderboard\"}"))
    });
    // Mid-ask, the user creates and switches to a second chat.
    std::thread::sleep(std::time::Duration::from_millis(150));
    let (status, _) = http_request(port, "POST", "/api/chats", Some("{}"));
    assert_eq!(status, 201, "chat creation returns 201");
    let (status, body) = ask_thread.join().unwrap();
    assert_eq!(status, 200, "the ask completes: {body}");

    // Chat 2 (currently active) must NOT hold the build narration.
    let (_, thread2) = http_request(port, "GET", "/api/thread", None);
    assert!(
        !thread2.contains("Leaderboard added"),
        "the outcome must not leak into the chat switched to mid-ask: {thread2}"
    );
    // Switch back to chat 1: the whole exchange is there.
    let (_, chats_body) = http_request(port, "GET", "/api/chats", None);
    let chats: Value = serde_json::from_str(&chats_body).unwrap();
    let first_id = chats["chats"][0]["id"].as_str().unwrap().to_string();
    let (status, _) = http_request(
        port,
        "POST",
        "/api/chats/select",
        Some(&format!("{{\"id\": \"{first_id}\"}}")),
    );
    assert_eq!(status, 200);
    let (_, thread1) = http_request(port, "GET", "/api/thread", None);
    assert!(
        thread1.contains("You: add a leaderboard") && thread1.contains("Leaderboard added"),
        "chat 1 holds its own exchange end to end: {thread1}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Regression tests from thermonuclear review ROUND 4.
// ---------------------------------------------------------------------------

/// A9: the instance `post` state is read back from the place, so a
/// bridge that adds its own default properties does not make every
/// legitimate instance revert refuse forever.
#[test]
fn instance_revert_survives_bridge_owned_defaults() {
    let command = format!("env MOCK_INST_EXTRA=1 {}", mock_studio_command());
    let script = vec![
        json!({ "tool": "create_instance", "args": { "class_name": "SpawnLocation", "name": "SpawnPad", "parent": "Workspace", "properties": { "Anchored": true } } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Spawn pad placed." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_mock_command(script, command);
    agent.run_ask("chat-1", "add a spawn pad").expect("ask completes");
    let tree = agent.place_tree().unwrap();
    assert_eq!(
        tree["instances"]["Workspace.SpawnPad"]["properties"]["BridgeDefault"],
        json!(true),
        "the bridge added its own default property: {tree}"
    );
    let restored = agent
        .revert_chat("chat-1")
        .expect("reverting what the ask left — bridge defaults included — must succeed");
    assert_eq!(restored.len(), 1);
    let tree = agent.place_tree().unwrap();
    assert_eq!(tree["instances"], json!({}), "the instance is reverted away");
}

/// Round-4 (C): chats.json gets the same strict loading as
/// changes.json — a malformed entry backs the whole file up instead
/// of being silently dropped and overwritten by the next save.
#[test]
fn malformed_chats_file_is_backed_up_at_startup() {
    let dir = std::env::temp_dir().join(format!("openro-chats-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bad = "{\"chats\": [{\"title\": \"T\", \"lines\": [\"You: hi\"]}], \"active\": \"x\"}";
    std::fs::write(dir.join("chats.json"), bad).unwrap();
    let port = start_server_with_data(None, dir.clone());
    let (status, _) = http_request(port, "GET", "/api/chats", None);
    assert_eq!(status, 200, "the server still starts");
    let bak = dir.join("chats.json.bak");
    assert!(bak.exists(), "the malformed file is preserved as a backup");
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), bad);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Round-4 (B) edges: curly apostrophes are normalised before the
/// suppressor check; semicolons end sentences; spawned/fixed count.
#[test]
fn claim_flag_curly_apostrophes_and_semicolons() {
    let cases: Vec<(&str, bool)> = vec![
        ("It wasn\u{2019}t built by me.", false),
        ("Nothing changed in the place; I built the leaderboard anyway.", true),
        ("I spawned the pad and fixed the leaderboard.", true),
    ];
    for (summary, should_flag) in cases {
        let script = vec![json!({ "done": true, "summary": summary }).to_string()];
        let (mut agent, _stub) = agent_with_script(script);
        let outcome = agent.run_ask("chat-1", "do the thing").expect("ask completes");
        let thread = outcome.thread.join("\n");
        assert_eq!(
            thread.contains("Note — this ask made no changes to the game."),
            should_flag,
            "summary {summary:?} should_flag={should_flag}: {thread}"
        );
    }
}

// ---------------------------------------------------------------------------
// Completion round (18 Nov 2026): the full objective — a proactive
// creativity engine, a generation kit that can one-shot a COMPLETE
// game, and the one-shot proof itself.
// ---------------------------------------------------------------------------

#[test]
fn new_skills_generate_parameterised_luau() {
    let registry = openro_rs::skills::registry();
    assert_eq!(registry.len(), 10, "ten skills registered: {:?}", registry.iter().map(|s| s.name).collect::<Vec<_>>());
    let prompt = openro_rs::agent::system_prompt();
    for def in &registry {
        assert!(prompt.contains(def.name), "system prompt lists skill {}", def.name);
    }
    // currency: persistence-shaped — DataStore load/save + BindToClose.
    let cur = openro_rs::skills::generate(
        "currency",
        &json!({ "currency": "Coins", "start_amount": 25, "earn_amount": 10, "earn_seconds": 5, "bind_close": true }),
    )
    .unwrap();
    assert!(cur.contains("GetDataStore("), "currency uses a DataStore: {cur}");
    assert!(cur.contains("BindToClose"), "currency binds the shutdown save: {cur}");
    assert!(cur.contains("START_AMOUNT = 25"), "parameters land in the Luau: {cur}");
    let unbound = openro_rs::skills::generate(
        "currency",
        &json!({ "currency": "Coins", "bind_close": false }),
    )
    .unwrap();
    assert!(unbound.contains("GetDataStore(") && !unbound.contains("BindToClose"), "bind_close=false omits the shutdown save: {unbound}");
    // A quote in the currency is escaped, never an injection.
    let evil = openro_rs::skills::generate("currency", &json!({ "currency": "x\"; hacked(" })).unwrap();
    assert!(evil.contains("\\\""), "currency quote is escaped in the Luau: {evil}");
    // shop: creates the RemoteEvent the GUI fires.
    let shop = openro_rs::skills::generate(
        "shop",
        &json!({ "item_name": "Speed Coil", "price": 100, "currency": "Coins" }),
    )
    .unwrap();
    assert!(shop.contains("Instance.new(\"RemoteEvent\")") && shop.contains("OpenROShopBuy"), "shop creates the buy event: {shop}");
    assert!(shop.contains("PRICE = 100"), "price lands in the Luau: {shop}");
    // game_gui: a StarterGui LocalScript with the balance label + button.
    let gui = openro_rs::skills::generate(
        "game_gui",
        &json!({ "title": "Coin Tycoon", "button_label": "Buy", "currency": "Coins" }),
    )
    .unwrap();
    assert!(gui.contains("ScreenGui") && gui.contains("Coin Tycoon"), "gui builds the screen: {gui}");
    assert!(gui.contains("WaitForChild(\"OpenROShopBuy\")"), "gui fires the shop event: {gui}");
    // checkpoints: creates its folder, or references it when told not to.
    let cp = openro_rs::skills::generate(
        "checkpoints",
        &json!({ "stage_count": 6, "folder_name": "Checkpoints", "create_folder": true }),
    )
    .unwrap();
    assert!(cp.contains("Name = \"Checkpoints\"") && cp.contains("STAGES = 6"), "checkpoints create their folder: {cp}");
    let cp_ref = openro_rs::skills::generate(
        "checkpoints",
        &json!({ "stage_count": 6, "folder_name": "Checkpoints", "create_folder": false }),
    )
    .unwrap();
    assert!(cp_ref.contains("workspace.Checkpoints") && !cp_ref.contains("Name = \"Checkpoints\""), "create_folder=false only references: {cp_ref}");
    // soundtrack.
    let st = openro_rs::skills::generate(
        "soundtrack",
        &json!({ "sound_id": 123, "volume": 7, "looped": true }),
    )
    .unwrap();
    assert!(st.contains("rbxassetid://123") && st.contains("Volume = 7 / 10") && st.contains("Looped = true"), "soundtrack parameters land: {st}");
    // Validation, both directions: bad args are refused, not coerced.
    assert!(openro_rs::skills::generate("currency", &json!({ "earn_seconds": 0 })).is_err());
    assert!(openro_rs::skills::generate("currency", &json!({ "earn_amount": 0 })).is_err());
    assert!(openro_rs::skills::generate("shop", &json!({ "price": 0 })).is_err());
    assert!(openro_rs::skills::generate("checkpoints", &json!({ "stage_count": 1 })).is_err());
    assert!(openro_rs::skills::generate("checkpoints", &json!({ "stage_count": 51 })).is_err());
    assert!(openro_rs::skills::generate("checkpoints", &json!({ "folder_name": "Bad Name" })).is_err());
    assert!(openro_rs::skills::generate("soundtrack", &json!({ "volume": 11 })).is_err());
    assert!(openro_rs::skills::generate("soundtrack", &json!({ "sound_id": -1 })).is_err());
    assert!(openro_rs::skills::generate("game_gui", &json!({ "title": "bad\u{0007}title" })).is_err());
}

#[test]
fn one_shot_complete_game_with_repair() {
    // ONE ask builds a whole small game: saved currency, a GUI whose
    // shop button has nothing to fire (playtest FAILS), the shop added
    // as the repair, then checkpoints, day/night, spawn and lava —
    // playtest passes, and the whole ask reverts.
    let script = vec![
        json!({ "skill": "currency", "args": { "currency": "Coins", "start_amount": 50, "earn_amount": 10, "earn_seconds": 5, "bind_close": true } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Currency", "skill": "currency" } }).to_string(),
        json!({ "skill": "game_gui", "args": { "title": "Coin Tycoon", "button_label": "Buy Speed Coil (100)", "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "StarterGui.GameGui", "skill": "game_gui" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "skill": "shop", "args": { "item_name": "Speed Coil", "price": 100, "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Shop", "skill": "shop" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "skill": "checkpoints", "args": { "stage_count": 6, "folder_name": "Checkpoints", "create_folder": true } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Checkpoints", "skill": "checkpoints" } }).to_string(),
        json!({ "skill": "day_night", "args": { "cycle_minutes": 10 } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.DayNight", "skill": "day_night" } }).to_string(),
        json!({ "tool": "create_instance", "args": { "class_name": "SpawnLocation", "name": "SpawnPad", "parent": "Workspace", "properties": { "Anchored": true } } }).to_string(),
        json!({ "skill": "spawn_link", "args": { "instance_name": "SpawnPad" } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.SpawnLink", "skill": "spawn_link" } }).to_string(),
        json!({ "tool": "create_instance", "args": { "class_name": "Part", "name": "LavaBrick", "parent": "Workspace", "properties": { "Anchored": true } } }).to_string(),
        json!({ "skill": "kill_brick", "args": { "damage": 25 } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "Workspace.LavaBrick.KillBrick", "skill": "kill_brick" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Built the whole Coin Tycoon starter — saved money, shop, screen, checkpoints, spawn and lava. Playtest passes." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    let outcome = agent
        .run_ask("chat-1", "build me a complete coin tycoon game with a shop, checkpoints and lava")
        .expect("the one-shot ask completes");
    let transcript = outcome.transcript.join("\n");
    assert!(transcript.contains("repair attempt 1"), "the GUI-first playtest failed and was repaired: {transcript}");
    assert!(transcript.contains("OpenROShopBuy RemoteEvent was never created"), "the failure was the real missing-shop error: {transcript}");
    for skill in ["currency", "game_gui", "shop", "checkpoints", "day_night", "spawn_link", "kill_brick"] {
        assert!(outcome.skills_used.iter().any(|s| s == skill), "skill {skill} was used: {:?}", outcome.skills_used);
    }
    let tree = agent.place_tree().unwrap();
    let scripts = tree["scripts"].as_object().unwrap();
    for path in [
        "ServerScriptService.Currency",
        "StarterGui.GameGui",
        "ServerScriptService.Shop",
        "ServerScriptService.Checkpoints",
        "ServerScriptService.DayNight",
        "ServerScriptService.SpawnLink",
        "Workspace.LavaBrick.KillBrick",
    ] {
        assert!(scripts.contains_key(path), "script {path} landed: {:?}", scripts.keys().collect::<Vec<_>>());
    }
    assert!(tree["scripts"]["ServerScriptService.Currency"].as_str().unwrap().contains("GetDataStore("));
    assert!(tree["scripts"]["StarterGui.GameGui"].as_str().unwrap().contains("ScreenGui"));
    let instances = tree["instances"].as_object().unwrap();
    assert!(instances.contains_key("Workspace.SpawnPad") && instances.contains_key("Workspace.LavaBrick"), "instances landed: {instances:?}");
    // The whole game is ONE change set: revert removes all of it.
    let restored = agent.revert_chat("chat-1").unwrap();
    assert_eq!(restored.len(), 9, "seven scripts + two instances revert together: {restored:?}");
    let tree = agent.place_tree().unwrap();
    assert_eq!(tree["scripts"], json!({}), "scripts empty after revert");
    assert_eq!(tree["instances"], json!({}), "instances empty after revert");
}

#[test]
fn currency_without_bind_close_fails_and_repairs() {
    let script = vec![
        json!({ "skill": "currency", "args": { "currency": "Coins", "bind_close": false } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Currency", "use_generated": true } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "skill": "currency", "args": { "currency": "Coins", "bind_close": true } }).to_string(),
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.Currency", "use_generated": true } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Currency added with saving bound; playtest passes." }).to_string(),
    ];
    let (mut agent, _stub) = agent_with_script(script);
    let outcome = agent
        .run_ask("chat-1", "add money that saves between sessions")
        .expect("the repair converges");
    let transcript = outcome.transcript.join("\n");
    assert!(transcript.contains("BindToClose was never bound"), "the DataStore failure was the real console error: {transcript}");
    assert!(transcript.contains("repair attempt 1"), "the repair is recorded: {transcript}");
}

#[test]
fn builtin_plan_builds_a_complete_game() {
    // No model replies at all: a built-in idea plan runs through the
    // loop's own machinery — skills, instances, playtest — and lands a
    // complete game as one revertable change set.
    let (mut agent, _stub) = agent_with_script(vec![]);
    let outcome = agent
        .build_idea("chat-1", "obby-starter")
        .expect("the built-in plan builds the game");
    assert!(outcome.summary.contains("Lava Obby"), "summary names the game: {}", outcome.summary);
    let thread = outcome.thread.join("\n");
    assert!(thread.contains("no model call involved"), "the build is honest about having no model: {thread}");
    assert!(thread.contains("Playtest passed."), "the plan's playtest passed: {thread}");
    for skill in ["spawn_link", "checkpoints", "kill_brick", "currency", "day_night"] {
        assert!(outcome.skills_used.iter().any(|s| s == skill), "plan used {skill}: {:?}", outcome.skills_used);
    }
    let tree = agent.place_tree().unwrap();
    let scripts = tree["scripts"].as_object().unwrap();
    for path in [
        "ServerScriptService.SpawnLink",
        "ServerScriptService.Checkpoints",
        "ServerScriptService.Currency",
        "ServerScriptService.DayNight",
        "Workspace.LavaBrick.KillBrick",
    ] {
        assert!(scripts.contains_key(path), "plan wrote {path}: {:?}", scripts.keys().collect::<Vec<_>>());
    }
    let instances = tree["instances"].as_object().unwrap();
    assert!(instances.contains_key("Workspace.SpawnPad") && instances.contains_key("Workspace.LavaBrick"), "plan placed its instances: {instances:?}");
    let restored = agent.revert_chat("chat-1").unwrap();
    assert_eq!(restored.len(), 7, "the whole plan reverts as one set: {restored:?}");
    let tree = agent.place_tree().unwrap();
    assert_eq!(tree["scripts"], json!({}), "scripts empty after revert");
    assert_eq!(tree["instances"], json!({}), "instances empty after revert");
    // An unknown idea is refused by name, never improvised.
    let err = agent.build_idea("chat-1", "no-such-idea").expect_err("unknown ideas refuse");
    assert!(err.message.contains("unknown idea"), "refusal names the problem: {}", err.message);
}

#[test]
fn ideas_engine_pitches_by_context() {
    // An empty game gets the complete-game pitches.
    let opening = openro_rs::ideas::ideas_for(&[]);
    assert_eq!(opening.len(), 3, "three opening games pitched");
    assert!(opening.iter().any(|i| i.id == "obby-starter"));
    // With money built, the shop follow-up unlocks; built skills are
    // never re-pitched.
    let built = vec!["currency".to_string()];
    let follow = openro_rs::ideas::ideas_for(&built);
    assert!(follow.iter().any(|i| i.id == "add-shop"), "the shop pitch unlocks once money exists: {:?}", follow.iter().map(|i| i.id).collect::<Vec<_>>());
    assert!(follow.iter().all(|i| !i.skills.contains(&"currency")), "built skills are not re-pitched");
    // Everything built → nothing left to pitch (no empty promises).
    let all: Vec<String> = ["currency", "shop", "game_gui", "checkpoints", "soundtrack", "day_night", "kill_brick"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(openro_rs::ideas::ideas_for(&all).is_empty(), "a complete game gets no stale pitches");
    // Every pitched plan only uses steps the loop understands, and
    // every skill it names exists in the registry.
    let names: Vec<&str> = openro_rs::skills::registry().iter().map(|s| s.name).collect();
    for idea in openro_rs::ideas::catalogue() {
        assert!(!idea.plan.is_empty(), "idea {} has a plan", idea.id);
        for step in &idea.plan {
            let is_skill = step.get("skill").and_then(|s| s.as_str()).map(|s| names.contains(&s)).unwrap_or(false);
            let is_tool = step.get("tool").and_then(|s| s.as_str()).map(|t| ["write_script", "create_instance", "delete_instance", "delete_script", "playtest"].contains(&t)).unwrap_or(false);
            assert!(is_skill || is_tool, "idea {} plan step is executable: {step}", idea.id);
        }
    }
}

#[test]
fn http_ideas_pitch_and_build() {
    let (agent, _stub) = agent_with_script(vec![]);
    let port = start_server(Some(agent));
    // The agent speaks first: asking for ideas pitches the opening
    // games INTO the thread (the record), not just to the buttons.
    let (status, body) = http_request(port, "POST", "/api/ideas", Some("{}"));
    assert_eq!(status, 200);
    assert!(body.contains("obby-starter") && body.contains("coin-tycoon"), "opening ideas returned: {body}");
    let (_, thread) = http_request(port, "GET", "/api/thread", None);
    assert!(thread.contains("games I can build for you right now"), "the pitch intro is in the record: {thread}");
    assert!(thread.contains("Idea — Lava Obby"), "the pitch itself is in the record: {thread}");
    // The same set is not pitched twice.
    let (_, body) = http_request(port, "POST", "/api/ideas", Some("{}"));
    assert!(body.contains("\"pitched_new\":false"), "a repeat call does not re-pitch: {body}");
    // One tap builds the whole game through the real bridge.
    let (status, body) = http_request(
        port,
        "POST",
        "/api/ideas/build",
        Some(&json!({ "id": "obby-starter" }).to_string()),
    );
    assert_eq!(status, 200, "the plan build succeeds: {body}");
    assert!(body.contains("Lava Obby"), "the response names the build: {body}");
    let (_, tree_body) = http_request(port, "GET", "/api/place-tree", None);
    assert!(tree_body.contains("ServerScriptService.Currency"), "the game landed: {tree_body}");
    assert!(tree_body.contains("Workspace.SpawnPad"), "the instances landed: {tree_body}");
    let (_, thread) = http_request(port, "GET", "/api/thread", None);
    assert!(thread.contains("You: Build this — Lava Obby"), "the tap is recorded: {thread}");
    assert!(thread.contains("Playtest passed."), "the build proved itself: {thread}");
    assert!(thread.contains("Next, I could add:"), "the follow-up pitch follows the build, unprompted: {thread}");
    // Unknown ideas 404; they are never improvised.
    let (status, _) = http_request(
        port,
        "POST",
        "/api/ideas/build",
        Some(&json!({ "id": "no-such-idea" }).to_string()),
    );
    assert_eq!(status, 404);
}
