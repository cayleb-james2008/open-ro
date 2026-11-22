//! Phase 7.5 (v0.10.0) Comprehensive Integration & Security Tests for RoPilot-Grade
//! OpenRO Backend:
//! - Custom Luau (`write_luau`) + targeted `multi_edit` with Static Luau Prover pre-write
//!   rejection, per-chat Undo checkpointing, unified `file_diffs`, and preserved `write_script`
//!   skill-only guard
//! - Whole-Game Context Summary + `context_paths` + `reasoning` injection and in-loop
//!   inspection + Blender/Audio asset tools
//! - Composer modes (`Agent | Plan | Ask`) + `review_diffs: true` Staged Proposals
//!   (`GET /api/proposals`, selective `POST /api/proposals/:id/apply`, `POST /api/proposals/:id/reject`)
//! - Transparent Official Studio MCP translation (official-ONLY 13-tool bridge with zero
//!   legacy tools advertised: 7-service `get_place_tree`, collision-proof `[=[...]=]` escaping,
//!   `ChangeHistoryService:SetWaypoint`, `create_instance`, `delete_instance`, `playtest`, `get_console`)
//! - Lightweight Companion Studio Plugin Bridge (`OpenRoCompanion.lua`, `0600` CSPRNG bearer token,
//!   strict verified `Roblox/Plugins` install guard, `401`/`403` security, live `PlaceBackend::Plugin`
//!   long-poll execution)

use openro_rs::agent::Agent;
use openro_rs::mcp::McpClient;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Minimal OpenAI-compatible stub model returning scripted JSON steps and recording
/// every inbound request body so tests can assert exact prompt/context contents.
struct StubModel {
    base_url: String,
    recorded_requests: Arc<Mutex<Vec<String>>>,
}

fn start_stub_model(steps: Vec<String>) -> StubModel {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let queue = Arc::new(Mutex::new(VecDeque::from(steps)));
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let recorded_clone = Arc::clone(&recorded);

    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut s = stream;
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let mut header_end = None;
            let mut content_len = 0usize;
            while let Ok(n) = s.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                if header_end.is_none() {
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(pos + 4);
                        let head = String::from_utf8_lossy(&buf[..pos]);
                        for line in head.lines() {
                            if let Some((k, v)) = line.split_once(':') {
                                if k.trim().eq_ignore_ascii_case("content-length") {
                                    content_len = v.trim().parse().unwrap_or(0);
                                }
                            }
                        }
                    }
                }
                if let Some(he) = header_end {
                    if buf.len() >= he + content_len {
                        break;
                    }
                }
            }
            if let Some(he) = header_end {
                let body = String::from_utf8_lossy(&buf[he..buf.len().min(he + content_len)])
                    .to_string();
                recorded_clone.lock().unwrap().push(body);
            }
            let reply = queue
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| json!({ "done": true, "summary": "Stub finished." }).to_string());
            let payload = json!({
                "id": "chatcmpl-v0100-stub",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": reply },
                    "finish_reason": "stop"
                }]
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            let _ = s.write_all(resp.as_bytes());
            let _ = s.flush();
        }
    });

    StubModel {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        recorded_requests: recorded,
    }
}

fn http_req_with_headers(
    port: u16,
    host: &str,
    method: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let mut header = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n");
    for (k, v) in extra_headers {
        header.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() {
        header.push_str("Content-Type: application/json\r\n");
        header.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    header.push_str("Connection: close\r\n\r\n");
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
    let (status, _, bytes) = http_req_with_headers(
        port,
        &format!("127.0.0.1:{port}"),
        method,
        path,
        &[],
        body.as_bytes(),
    );
    let val: Value = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "invalid JSON from {method} {path} (status {status}): {e} — {:?}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, val)
}

fn start_server_with(
    steps: Vec<String>,
    initial_tree: Option<Value>,
    data_dir: Option<std::path::PathBuf>,
) -> (u16, StubModel, Arc<Shared>) {
    let stub = start_stub_model(steps);
    let model = ModelClient::new(
        stub.base_url.clone(),
        "ropilot-test-model".to_string(),
        Some("test-key-not-real".to_string()),
    );
    let mut place = match &data_dir {
        Some(dir) => LocalPlace::open(dir.clone(), "RoPilot Test Place".to_string()),
        None => LocalPlace::in_memory("RoPilot Test Place".to_string()),
    };
    if let Some(tree) = initial_tree {
        place.replace_tree(&tree);
    }
    let agent = Agent::with_local_place(model, place);
    let shared = match data_dir {
        Some(dir) => Shared::with_data_dir(Some(agent), "RoPilot Test Place".to_string(), dir),
        None => Shared::new(Some(agent), "RoPilot Test Place".to_string()),
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared_for_serve = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, shared_for_serve);
    });
    (port, stub, shared)
}

#[test]
fn status_reports_v0100_ropilot_flags_and_job_md_is_jargon_free() {
    let (port, _stub, _shared) = start_server_with(vec![], None, None);
    let (st, status) = http_json(port, "GET", "/api/status", "");
    assert_eq!(st, 200);
    assert_eq!(status["version"], json!("0.10.0"));
    assert_eq!(status["ropilot_v0100"], json!(true));
    assert_eq!(status["proposals_api"], json!(true));
    assert_eq!(status["plugin_bridge"], json!(true));
    assert_eq!(status["plugin_connected"], json!(false));
    assert_eq!(status["pending_proposals"], json!(0));

    let job_md = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/JOB.md")).unwrap();
    assert!(
        !job_md.contains("Rune"),
        "JOB.md must contain zero user-facing Rune jargon (#155): {job_md}"
    );
}

#[test]
fn write_luau_and_multi_edit_enforce_static_prover_checkpoint_undo_and_preserve_write_script_guard() {
    let broken_luau = "local function bad()\n    while true do\n        print('hang')\n    end\n";
    let valid_sprint = "local Players = game:GetService(\"Players\")\n\
                        local SPRINT_SPEED = 24\n\
                        Players.PlayerAdded:Connect(function(player)\n\
                            player.CharacterAdded:Connect(function(char)\n\
                                local hum = char:WaitForChild(\"Humanoid\")\n\
                                hum.WalkSpeed = SPRINT_SPEED\n\
                            end)\n\
                        end)\n";

    let steps = vec![
        // 1. write_script with raw source MUST still be refused (existing invariant).
        json!({ "tool": "write_script", "args": { "path": "ServerScriptService.RawBypass", "source": "print(1)" } }).to_string(),
        // 2. write_luau with broken Luau (unclosed function + infinite loop) MUST be rejected BEFORE writing.
        json!({ "tool": "write_luau", "args": { "path": "ServerScriptService.Sprint", "source": broken_luau } }).to_string(),
        // 3. write_luau with clean server-authoritative Luau succeeds.
        json!({ "tool": "write_luau", "args": { "path": "ServerScriptService.Sprint", "source": valid_sprint } }).to_string(),
        // 4. multi_edit introducing a syntax error MUST be rejected BEFORE writing.
        json!({ "tool": "multi_edit", "args": { "path": "ServerScriptService.Sprint", "edits": [{ "old_str": "end)\nend)", "new_str": "end)" }] } }).to_string(),
        // 5. multi_edit applying valid change succeeds.
        json!({ "tool": "multi_edit", "args": { "path": "ServerScriptService.Sprint", "edits": [{ "old_str": "local SPRINT_SPEED = 24", "new_str": "local SPRINT_SPEED = 32" }] } }).to_string(),
        // 6. Playtest passes and done returns unified file_diffs.
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Added custom Sprint controller with WalkSpeed 32." }).to_string(),
    ];

    let (port, stub, _shared) = start_server_with(steps, None, None);
    let (st, res) = http_json(
        port,
        "POST",
        "/api/chat",
        &json!({ "message": "add a custom sprint script" }).to_string(),
    );
    assert_eq!(st, 200, "{res}");
    assert_eq!(res["ok"], json!(true));
    assert_eq!(res["mode"], json!("agent"));

    // Verify model received both write_script refusal AND line-numbered Static Luau Prover rejections.
    let recorded = stub.recorded_requests.lock().unwrap();
    assert!(
        recorded.iter().any(|r| r.contains("Luau comes only from")),
        "write_script raw source refusal preserved"
    );
    assert!(
        recorded
            .iter()
            .any(|r| r.contains("Static Luau Prover rejected") && r.contains("Line ")),
        "write_luau & multi_edit return exact line-numbered static errors to model"
    );

    // Verify file_diffs included on direct Agent build.
    let file_diffs = res["file_diffs"].as_array().expect("file_diffs array");
    assert_eq!(file_diffs.len(), 1);
    assert_eq!(file_diffs[0]["path"], json!("ServerScriptService.Sprint"));
    assert_eq!(file_diffs[0]["status"], json!("added"));
    assert!(file_diffs[0]["additions"].as_u64().unwrap() >= 8);
    assert!(!file_diffs[0]["hunks"].as_array().unwrap().is_empty());

    // Verify final script in live tree has the multi_edit change (32) and NOT the rejected bypass.
    let (st, tree) = http_json(port, "GET", "/api/place-tree", "");
    assert_eq!(st, 200);
    assert!(tree["scripts"].get("ServerScriptService.RawBypass").is_none());
    let src = tree["scripts"]["ServerScriptService.Sprint"].as_str().unwrap();
    assert!(src.contains("local SPRINT_SPEED = 32"), "got: {src}");

    // Verify per-chat Undo cleanly reverts both write_luau + multi_edit back to absent.
    let (st, rev) = http_json(port, "POST", "/api/revert", "");
    assert_eq!(st, 200, "{rev}");
    assert_eq!(rev["ok"], json!(true));
    let (_, after_rev) = http_json(port, "GET", "/api/place-tree", "");
    assert!(after_rev["scripts"].get("ServerScriptService.Sprint").is_none());
}

#[test]
fn whole_game_context_summary_context_paths_and_inspection_plus_asset_tools_in_agent_loop() {
    let initial = json!({
        "instances": {
            "Workspace.ArenaPad": {
                "class_name": "Part",
                "properties": { "Position": [0, 2, 0], "Size": [12, 1, 12], "Anchored": true }
            }
        },
        "scripts": {
            "ServerScriptService.Config": "local MAX_DASH_SPEED = 48\nreturn { MaxDashSpeed = MAX_DASH_SPEED }\n"
        }
    });

    let steps = vec![
        json!({ "tool": "search_game_tree", "args": { "query": "Arena" } }).to_string(),
        json!({ "tool": "inspect_instance", "args": { "path": "Workspace.ArenaPad" } }).to_string(),
        json!({ "tool": "script_read", "args": { "path": "ServerScriptService.Config" } }).to_string(),
        json!({ "tool": "script_grep", "args": { "query": "MAX_DASH_SPEED" } }).to_string(),
        json!({ "tool": "blender_generate", "args": { "preset": "torii_gate", "name": "ArenaTorii" } }).to_string(),
        json!({ "tool": "audio_generate", "args": { "preset": "ui_click", "name": "DashClick" } }).to_string(),
        json!({ "done": true, "summary": "Inspected ArenaPad & Config and generated ArenaTorii mesh + DashClick SFX." }).to_string(),
    ];

    let (port, stub, _shared) = start_server_with(steps, Some(initial), None);
    let (st, res) = http_json(
        port,
        "POST",
        "/api/chat",
        &json!({
            "message": "inspect the arena config and generate a torii mesh and click sound",
            "reasoning": "high",
            "context_paths": ["ServerScriptService.Config", "Workspace.ArenaPad"]
        })
        .to_string(),
    );
    assert_eq!(st, 200, "{res}");
    assert_eq!(res["ok"], json!(true));

    let recorded = stub.recorded_requests.lock().unwrap();
    let turn1 = &recorded[0];
    assert!(turn1.contains("[OPENRO WHOLE-GAME CONTEXT]"), "turn1 missing whole-game context: {turn1}");
    assert!(turn1.contains("Workspace.ArenaPad"), "turn1 missing instance path: {turn1}");
    assert!(turn1.contains("ServerScriptService.Config"), "turn1 missing script path: {turn1}");
    assert!(turn1.contains("MAX_DASH_SPEED = 48"), "turn1 missing attached script source: {turn1}");
    assert!(turn1.contains("REASONING DEPTH: high"), "turn1 missing reasoning cue: {turn1}");

    // Confirm blender_generate & audio_generate returned real asset URLs in tool results.
    assert!(
        recorded.iter().any(|r| r.contains("blender_generate:") && r.contains("/api/assets/meshes/")),
        "blender_generate produced mesh asset URL"
    );
    assert!(
        recorded.iter().any(|r| r.contains("audio_generate:") && r.contains("/api/assets/audio/")),
        "audio_generate produced wav asset URL"
    );
}

#[test]
fn ask_mode_is_strictly_read_only_and_refuses_all_mutations() {
    let initial = json!({
        "instances": {
            "Workspace.SpawnPad": { "class_name": "SpawnLocation", "properties": { "Anchored": true } }
        },
        "scripts": {
            "ServerScriptService.KeepMe": "local Players = game:GetService(\"Players\")\nprint(\"ok\")\n"
        }
    });

    let steps = vec![
        // Model tries several mutations in Ask mode — ALL must be refused with zero live changes.
        json!({ "skill": "leaderboard", "args": { "currency": "Coins" } }).to_string(),
        json!({ "tool": "write_luau", "args": { "path": "ServerScriptService.Injected", "source": "print(1)" } }).to_string(),
        json!({ "tool": "delete_script", "args": { "path": "ServerScriptService.KeepMe" } }).to_string(),
        json!({ "tool": "create_instance", "args": { "class_name": "Part", "name": "Bad", "parent": "Workspace" } }).to_string(),
        // Model falls back to read-only inspection and answers honestly.
        json!({ "tool": "script_read", "args": { "path": "ServerScriptService.KeepMe" } }).to_string(),
        json!({ "done": true, "summary": "ServerScriptService.KeepMe prints ok and references Players." }).to_string(),
    ];

    let (port, stub, _shared) = start_server_with(steps, Some(initial), None);
    let (st, res) = http_json(
        port,
        "POST",
        "/api/chat",
        &json!({
            "message": "what does KeepMe do?",
            "mode": "ask",
            "context_paths": ["ServerScriptService.KeepMe"]
        })
        .to_string(),
    );
    assert_eq!(st, 200, "{res}");
    assert_eq!(res["ok"], json!(true));
    assert_eq!(res["mode"], json!("ask"));
    assert_eq!(res["file_diffs"], json!([]));
    assert!(res["summary"].as_str().unwrap().contains("KeepMe"));

    let recorded = stub.recorded_requests.lock().unwrap();
    assert!(
        recorded.iter().any(|r| r.contains("Ask mode is read-only")),
        "mutating tools refused in Ask mode"
    );

    // Live place tree is 100% unchanged!
    let (_, tree) = http_json(port, "GET", "/api/place-tree", "");
    assert!(tree["scripts"].get("ServerScriptService.KeepMe").is_some());
    assert!(tree["scripts"].get("ServerScriptService.Injected").is_none());
    assert!(tree["instances"].get("Workspace.Bad").is_none());
}

#[test]
fn plan_mode_and_review_diffs_stage_proposals_with_selective_apply_undo_and_reject() {
    let initial = json!({
        "instances": {},
        "scripts": {
            "ServerScriptService.Existing": "local SPEED = 16\nprint(SPEED)\n"
        }
    });

    let sprint_src = "local Players = game:GetService(\"Players\")\nprint(\"sprint\", Players)\n";
    let dash_src = "local Players = game:GetService(\"Players\")\nprint(\"dash\", Players)\n";
    let reject_src = "local Players = game:GetService(\"Players\")\nprint(\"reject\", Players)\n";

    let steps = vec![
        // Plan mode run: stages 2 new scripts + 1 modified script (zero live mutation!).
        json!({ "tool": "write_luau", "args": { "path": "ServerScriptService.Sprint", "source": sprint_src } }).to_string(),
        json!({ "tool": "write_luau", "args": { "path": "ServerScriptService.Dash", "source": dash_src } }).to_string(),
        json!({ "tool": "multi_edit", "args": { "path": "ServerScriptService.Existing", "edits": [{ "old_str": "local SPEED = 16", "new_str": "local SPEED = 20" }] } }).to_string(),
        json!({ "done": true, "summary": "Staged Sprint, Dash, and Existing speed bump." }).to_string(),
        // Second run with review_diffs: true (to test reject).
        json!({ "tool": "write_luau", "args": { "path": "ServerScriptService.RejectMe", "source": reject_src } }).to_string(),
        json!({ "done": true, "summary": "Staged RejectMe script." }).to_string(),
    ];

    let data_dir = unique_temp_dir("openro-v0100-props");
    let (port, _stub, _shared) = start_server_with(steps, Some(initial), Some(data_dir.clone()));

    // 1. Run mode="plan"
    let (st, plan_res) = http_json(
        port,
        "POST",
        "/api/chat",
        &json!({ "message": "plan sprint and dash mechanics", "mode": "plan" }).to_string(),
    );
    assert_eq!(st, 200, "{plan_res}");
    assert_eq!(plan_res["ok"], json!(true));
    assert_eq!(plan_res["mode"], json!("plan"));
    assert_eq!(plan_res["staged"], json!(true));
    let prop_id = plan_res["proposal"]["id"].as_str().unwrap().to_string();
    assert_eq!(plan_res["proposal"]["status"], json!("pending"));
    assert_eq!(plan_res["file_diffs"].as_array().unwrap().len(), 3);
    assert_eq!(plan_res["diagnostics"]["ok"], json!(true));

    // Verify ZERO live Studio mutation before Apply!
    let (_, live_before) = http_json(port, "GET", "/api/place-tree", "");
    assert!(live_before["scripts"].get("ServerScriptService.Sprint").is_none());
    assert!(live_before["scripts"].get("ServerScriptService.Dash").is_none());
    assert_eq!(
        live_before["scripts"]["ServerScriptService.Existing"].as_str().unwrap(),
        "local SPEED = 16\nprint(SPEED)\n"
    );

    // 2. GET /api/proposals lists the pending proposal.
    let (st, list_res) = http_json(port, "GET", "/api/proposals", "");
    assert_eq!(st, 200);
    assert_eq!(list_res["pending_count"], json!(1));
    assert_eq!(list_res["proposals"].as_array().unwrap().len(), 1);

    // Invalid selected_paths rejected cleanly (400).
    let (st, _) = http_json(
        port,
        "POST",
        &format!("/api/proposals/{prop_id}/apply"),
        &json!({ "selected_paths": [] }).to_string(),
    );
    assert_eq!(st, 400);
    let (st, _) = http_json(
        port,
        "POST",
        &format!("/api/proposals/{prop_id}/apply"),
        &json!({ "selected_paths": ["../etc/passwd"] }).to_string(),
    );
    assert_eq!(st, 400);

    // 3. Selective Apply: apply ONLY Sprint + Existing (skip Dash!).
    let (st, apply_res) = http_json(
        port,
        "POST",
        &format!("/api/proposals/{prop_id}/apply"),
        &json!({ "selected_paths": ["ServerScriptService.Sprint", "ServerScriptService.Existing"] })
            .to_string(),
    );
    assert_eq!(st, 200, "{apply_res}");
    assert_eq!(apply_res["ok"], json!(true));
    assert_eq!(apply_res["proposal"]["status"], json!("applied"));
    assert_eq!(apply_res["playtest"]["passed"], json!(true));
    assert!(apply_res["snapshot_id"].as_str().unwrap().starts_with("snap-"));
    let applied_paths = apply_res["applied_paths"].as_array().unwrap();
    assert_eq!(applied_paths.len(), 2);

    // Live tree now has Sprint + updated Existing, and STILL does NOT have unselected Dash!
    let (_, live_after) = http_json(port, "GET", "/api/place-tree", "");
    assert!(live_after["scripts"].get("ServerScriptService.Sprint").is_some());
    assert!(live_after["scripts"].get("ServerScriptService.Dash").is_none());
    assert!(live_after["scripts"]["ServerScriptService.Existing"]
        .as_str()
        .unwrap()
        .contains("SPEED = 20"));

    // Double-apply returns 409 Conflict.
    let (st, _) = http_json(port, "POST", &format!("/api/proposals/{prop_id}/apply"), "{}");
    assert_eq!(st, 409);

    // Per-chat Undo reverts the applied proposal cleanly!
    let (st, rev) = http_json(port, "POST", "/api/revert", "");
    assert_eq!(st, 200, "{rev}");
    assert_eq!(rev["ok"], json!(true));
    let (_, after_undo) = http_json(port, "GET", "/api/place-tree", "");
    assert!(after_undo["scripts"].get("ServerScriptService.Sprint").is_none());
    assert!(after_undo["scripts"]["ServerScriptService.Existing"]
        .as_str()
        .unwrap()
        .contains("SPEED = 16"));

    // 4. review_diffs: true in Agent mode + Reject leaves live Studio untouched.
    let (st, rev_res) = http_json(
        port,
        "POST",
        "/api/chat",
        &json!({ "message": "draft reject script", "mode": "agent", "review_diffs": true })
            .to_string(),
    );
    assert_eq!(st, 200, "{rev_res}");
    let prop2_id = rev_res["proposal"]["id"].as_str().unwrap().to_string();
    let (st, rej_res) = http_json(port, "POST", &format!("/api/proposals/{prop2_id}/reject"), "");
    assert_eq!(st, 200, "{rej_res}");
    assert_eq!(rej_res["proposal"]["status"], json!("rejected"));
    let (_, after_rej) = http_json(port, "GET", "/api/place-tree", "");
    assert!(after_rej["scripts"].get("ServerScriptService.RejectMe").is_none());

    let _ = std::fs::remove_dir_all(&data_dir);
}

#[test]
fn official_only_studio_mcp_translates_tree_write_create_delete_playtest_and_console() {
    // Official-ONLY MCP stdio server in Python: advertises ONLY the 13 official Roblox Studio
    // MCP tools (ZERO legacy tools like get_place_tree / write_script / create_instance / playtest).
    let dir = unique_temp_dir("openro-v0100-official-mcp");
    let log_file = dir.join("calls.jsonl");
    let server_py = dir.join("official_mcp_fixture.py");
    let py_code = r#"import base64, json, sys

log_path = sys.argv[1]
PNG_1X1 = base64.b64encode(
    b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde"
    b"\x00\x00\x00\x0cIDATx\x9cc```\x00\x00\x00\x04\x00\x01\xf6\x178U\x00\x00\x00\x00IEND\xaeB`\x82"
).decode("ascii")

OFFICIAL = [
    "list_roblox_studios", "get_studio_state", "search_game_tree", "inspect_instance",
    "script_read", "script_grep", "multi_edit", "execute_luau",
    "get_console_output", "screen_capture", "start_stop_play",
    "user_mouse_input", "user_keyboard_input",
]

tree = {
    "instances": {
        "Workspace.SpawnPad": {"class_name": "SpawnLocation", "properties": {"Anchored": True}},
        "Lighting.SkyAtmosphere": {"class_name": "Atmosphere", "properties": {}},
    },
    "scripts": {
        "ServerScriptService.Existing": "print('hello studio')\n",
    },
}
console_lines = ["[Studio] Playtest booted cleanly (0 errors)"]

def record(obj):
    with open(log_path, "a", encoding="utf-8") as f:
        f.write(json.dumps(obj) + "\n")

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    msg = json.loads(raw)
    mid = msg.get("id")
    method = msg.get("method")
    params = msg.get("params") or {}
    if method == "notifications/initialized":
        continue
    if method == "initialize":
        out = {"jsonrpc": "2.0", "id": mid, "result": {"serverInfo": {"name": "official-only-studio", "version": "1.0"}}}
    elif method == "tools/list":
        out = {"jsonrpc": "2.0", "id": mid, "result": {"tools": [{"name": n} for n in OFFICIAL]}}
    elif method == "tools/call":
        name = params.get("name")
        args = params.get("arguments") or {}
        record({"tool": name, "args": args})
        if name == "list_roblox_studios":
            res = {"content": [{"type": "text", "text": json.dumps({"studios": [{"studio_id": "studio-official-1"}]})}]}
        elif name == "execute_luau":
            code = args.get("code", "")
            if "OPENRO_TREE:" in code:
                payload = "OPENRO_TREE:" + json.dumps(tree)
                res = {"content": [{"type": "text", "text": json.dumps({"output": payload})}]}
            else:
                if "target.Source =" in code and "Sprint" in code:
                    tree["scripts"]["ServerScriptService.Sprint"] = "local s = [=[nested]=]\nprint(s)\n"
                if "JumpPad" in code and "Instance.new" in code:
                    tree["instances"]["Workspace.JumpPad"] = {"class_name": "Part", "properties": {"Anchored": True}}
                if "Destroy()" in code and "JumpPad" in code:
                    tree["instances"].pop("Workspace.JumpPad", None)
                res = {"content": [{"type": "text", "text": json.dumps({"output": "OPENRO_OK"})}]}
        elif name == "start_stop_play":
            mode = args.get("mode", "start_play")
            res = {"content": [{"type": "text", "text": json.dumps({"ok": True, "mode": mode})}]}
        elif name == "get_console_output":
            res = {"content": [{"type": "text", "text": json.dumps({"lines": console_lines})}]}
        elif name == "screen_capture":
            res = {"content": [{"type": "image", "mimeType": "image/png", "data": PNG_1X1}]}
        else:
            res = {"content": [{"type": "text", "text": json.dumps({"ok": True})}]}
        out = {"jsonrpc": "2.0", "id": mid, "result": res}
    else:
        out = {"jsonrpc": "2.0", "id": mid, "result": {}}
    sys.stdout.write(json.dumps(out) + "\n")
    sys.stdout.flush()
"#;
    std::fs::write(&server_py, py_code).unwrap();

    let cmd = format!(
        "python3 \"{}\" \"{}\"",
        server_py.display(),
        log_file.display()
    );
    let client = McpClient::spawn(&cmd).expect("spawn official-only MCP fixture");
    assert!(client.has_official_bridge());
    assert!(!client.has_exact_tool("get_place_tree"));
    assert!(!client.has_exact_tool("write_script"));
    assert!(!client.has_exact_tool("playtest"));

    let tricky_src = "local s = [=[nested]=]\nprint(s)\n";
    let steps = vec![
        json!({ "tool": "get_place_tree", "args": {} }).to_string(),
        json!({ "tool": "write_luau", "args": { "path": "ServerScriptService.Sprint", "source": tricky_src } }).to_string(),
        json!({ "tool": "create_instance", "args": { "class_name": "Part", "name": "JumpPad", "parent": "Workspace", "properties": { "Anchored": true, "Position": [0, 3, 10] } } }).to_string(),
        json!({ "tool": "delete_instance", "args": { "path": "Workspace.JumpPad" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Verified full official Studio MCP translation." }).to_string(),
    ];
    let stub = start_stub_model(steps);
    let model = ModelClient::new(
        stub.base_url.clone(),
        "ropilot-official-test".to_string(),
        Some("test-key".to_string()),
    );
    let mut agent = Agent::new(model, Some(client));

    // Must pass check_bridge_capabilities() and complete all translated steps!
    let outcome = agent
        .run_ask("chat-official", "add Sprint script, test JumpPad, playtest")
        .expect("official-only Studio MCP bridge builds & playtests cleanly");
    assert!(!outcome.refused);
    assert!(outcome.changed);

    // Inspect recorded official MCP calls to prove transparent translation details!
    let log_text = std::fs::read_to_string(&log_file).unwrap();
    let calls: Vec<Value> = log_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    // 1. 7-service DataModel walk in get_place_tree
    assert!(
        calls.iter().any(|c| c["tool"] == "execute_luau"
            && c["args"]["code"].as_str().unwrap_or("").contains("SoundService")
            && c["args"]["code"].as_str().unwrap_or("").contains("Lighting")),
        "get_place_tree walks all 7 Roblox services including Lighting & SoundService"
    );
    // 2. Collision-proof long-bracket literal [==[...]==] + ChangeHistoryService:SetWaypoint
    assert!(
        calls.iter().any(|c| c["tool"] == "execute_luau"
            && c["args"]["code"].as_str().unwrap_or("").contains("[==[local s = [=[nested]=]")
            && c["args"]["code"].as_str().unwrap_or("").contains("ChangeHistoryService:SetWaypoint")),
        "write_luau translated with collision-proof [==[...]==] and SetWaypoint"
    );
    // 3. playtest translated to start_play -> get_console_output + screen_capture -> stop
    let play_starts = calls
        .iter()
        .filter(|c| c["tool"] == "start_stop_play" && c["args"]["mode"] == "start_play")
        .count();
    let play_stops = calls
        .iter()
        .filter(|c| c["tool"] == "start_stop_play" && c["args"]["mode"] == "stop")
        .count();
    assert!(play_starts >= 1 && play_stops >= 1, "start_play & stop called");
    assert!(calls.iter().any(|c| c["tool"] == "get_console_output"));
    assert!(calls.iter().any(|c| c["tool"] == "screen_capture"));

    // 4. get_place_tree returned real populated instances & scripts (fixes '0 pieces in your game')!
    let tree = agent.place_tree().unwrap();
    assert!(tree["instances"]["Workspace.SpawnPad"].is_object());
    assert!(tree["instances"]["Lighting.SkyAtmosphere"].is_object());
    assert!(tree["scripts"]["ServerScriptService.Sprint"].is_string());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn companion_plugin_bridge_auth_install_download_and_live_plugin_backend_execution() {
    let data_dir = unique_temp_dir("openro-v0100-plugin-data");
    let steps = vec![
        json!({ "tool": "write_luau", "args": { "path": "ServerScriptService.PluginScript", "source": "local Players = game:GetService(\"Players\")\nprint(Players)\n" } }).to_string(),
        json!({ "tool": "playtest", "args": {} }).to_string(),
        json!({ "done": true, "summary": "Built directly inside live Roblox Studio via Companion Plugin." }).to_string(),
    ];
    let (port, _stub, _shared) = start_server_with(steps, None, Some(data_dir.clone()));

    // 1. Token persisted with 0600 permissions; status does NOT leak raw token.
    let token_path = data_dir.join("plugin_token");
    assert!(token_path.is_file());
    let token = std::fs::read_to_string(&token_path).unwrap().trim().to_string();
    assert_eq!(token.len(), 64);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&token_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let (st, plugin_status) = http_json(port, "GET", "/api/studio/plugin/status", "");
    assert_eq!(st, 200);
    assert_eq!(plugin_status["connected"], json!(false));
    assert_eq!(plugin_status["token_configured"], json!(true));
    assert!(
        !plugin_status.to_string().contains(&token),
        "plugin status MUST NEVER echo raw CSPRNG bearer token"
    );

    // 2. Download endpoint returns attachment with exact port & token and 0 static Luau errors.
    let (st, head, body) = http_req_with_headers(
        port,
        &format!("127.0.0.1:{port}"),
        "GET",
        "/api/studio/plugin/download",
        &[],
        b"",
    );
    assert_eq!(st, 200);
    assert!(
        head.contains("attachment; filename=\"OpenRoCompanion.lua\""),
        "missing attachment header: {head}"
    );
    let lua = String::from_utf8(body).unwrap();
    assert!(lua.contains(&format!("http://127.0.0.1:{port}")));
    assert!(lua.contains(&token));
    assert_eq!(
        openro_rs::luau_check::analyze_source("ServerScriptService.OpenRoCompanion", &lua)
            .error_count(),
        0
    );

    // 3. Install endpoint MUST reject traversal and arbitrary non-Plugins paths.
    let (st, _) = http_json(
        port,
        "POST",
        "/api/studio/plugin/install",
        &json!({ "target_dir": "/tmp/../etc/Plugins" }).to_string(),
    );
    assert!(st == 400 || st == 403 || st == 404, "unexpected status {st}");
    let (st, _) = http_json(
        port,
        "POST",
        "/api/studio/plugin/install",
        &json!({ "target_dir": "/tmp" }).to_string(),
    );
    assert!(st == 400 || st == 403 || st == 404, "unexpected status {st}");

    // 4. Token & loopback security on /handshake, /poll, /result.
    let (st, _) = http_json(port, "POST", "/api/studio/plugin/handshake", "{}");
    assert_eq!(st, 401, "missing token must be 401");
    let (st, _, _) = http_req_with_headers(
        port,
        &format!("127.0.0.1:{port}"),
        "GET",
        "/api/studio/plugin/poll?timeout_ms=50",
        &[("X-OpenRo-Plugin-Token", "bad-token")],
        b"",
    );
    assert_eq!(st, 401, "wrong token must be 401");
    let (st, _, _) = http_req_with_headers(
        port,
        "evil.example.com",
        "POST",
        "/api/studio/plugin/handshake",
        &[("X-OpenRo-Plugin-Token", &token)],
        b"{}",
    );
    assert_eq!(st, 403, "non-loopback Host must be 403");

    // 5. Authenticated Companion Plugin connects via /handshake -> automatically attaches PlaceBackend::Plugin!
    let (st, _, hs_bytes) = http_req_with_headers(
        port,
        &format!("127.0.0.1:{port}"),
        "POST",
        "/api/studio/plugin/handshake",
        &[("X-OpenRo-Plugin-Token", &token)],
        json!({
            "studio_name": "Roblox Studio (Edit)",
            "place_name": "Live Companion Arena",
            "selection": ["Workspace.ArenaCenter"]
        })
        .to_string()
        .as_bytes(),
    );
    assert_eq!(st, 200);
    let hs: Value = serde_json::from_slice(&hs_bytes).unwrap();
    assert_eq!(hs["connected"], json!(true));
    assert_eq!(hs["selection"], json!(["Workspace.ArenaCenter"]));

    let (_, st_after_hs) = http_json(port, "GET", "/api/status", "");
    assert_eq!(st_after_hs["plugin_connected"], json!(true));
    assert_eq!(st_after_hs["studio_mode"], json!("plugin_live"));
    assert_eq!(st_after_hs["workspace_mode"], json!("studio_plugin"));

    // 6. Spawn a simulated OpenRoCompanion.lua worker thread that long-polls /poll and posts /result.
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop);
    let tok_worker = token.clone();
    let plugin_state = Arc::new(Mutex::new(json!({
        "instances": {
            "Workspace.ArenaCenter": { "class_name": "Part", "properties": { "Anchored": true } }
        },
        "scripts": {}
    })));
    let state_worker = Arc::clone(&plugin_state);
    let worker = std::thread::spawn(move || {
        while !stop_worker.load(Ordering::Relaxed) {
            let (pst, _, pbytes) = http_req_with_headers(
                port,
                &format!("127.0.0.1:{port}"),
                "GET",
                "/api/studio/plugin/poll?timeout_ms=400",
                &[("X-OpenRo-Plugin-Token", &tok_worker)],
                b"",
            );
            if pst != 200 {
                continue;
            }
            let pv: Value = serde_json::from_slice(&pbytes).unwrap_or(json!({}));
            if let Some(cmd) = pv.get("command").filter(|c| c.is_object()) {
                let id = cmd["id"].as_str().unwrap_or("").to_string();
                let tool = cmd["tool"].as_str().unwrap_or("");
                let args = &cmd["args"];
                let result = match tool {
                    "get_place_tree" => state_worker.lock().unwrap().clone(),
                    "write_script" => {
                        let p = args["path"].as_str().unwrap().to_string();
                        let s = args["source"].as_str().unwrap().to_string();
                        state_worker.lock().unwrap()["scripts"][p] = json!(s);
                        json!({ "ok": true })
                    }
                    "playtest" => json!({ "status": "passed", "console": "Plugin playtest OK" }),
                    _ => json!({ "ok": true }),
                };
                let body = json!({
                    "id": id,
                    "ok": true,
                    "result": result,
                    "selection": ["ServerScriptService.PluginScript"]
                })
                .to_string();
                let _ = http_req_with_headers(
                    port,
                    &format!("127.0.0.1:{port}"),
                    "POST",
                    "/api/studio/plugin/result",
                    &[("X-OpenRo-Plugin-Token", &tok_worker)],
                    body.as_bytes(),
                );
            }
        }
    });

    // 7. GET /api/place-tree & POST /api/chat execute live through PlaceBackend::Plugin!
    let (st, tree_live) = http_json(port, "GET", "/api/place-tree", "");
    assert_eq!(st, 200, "{tree_live}");
    assert!(tree_live["instances"]["Workspace.ArenaCenter"].is_object());

    let (st, chat_res) = http_json(
        port,
        "POST",
        "/api/chat",
        &json!({ "message": "add PluginScript into connected Roblox Studio" }).to_string(),
    );
    assert_eq!(st, 200, "{chat_res}");
    assert_eq!(chat_res["ok"], json!(true));
    assert_eq!(chat_res["file_diffs"].as_array().unwrap().len(), 1);
    assert_eq!(
        chat_res["file_diffs"][0]["path"],
        json!("ServerScriptService.PluginScript")
    );

    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();

    // Verify plugin state received the written Luau script!
    assert!(plugin_state.lock().unwrap()["scripts"]["ServerScriptService.PluginScript"]
        .as_str()
        .unwrap()
        .contains("Players"));

    let _ = std::fs::remove_dir_all(&data_dir);
}
