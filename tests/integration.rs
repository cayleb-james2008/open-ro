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
    // and the thread keeps the exchange.
    let garbage: Vec<String> = (0..16).map(|_| "this is not json at all".to_string()).collect();
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
