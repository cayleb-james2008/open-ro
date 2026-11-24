//! Integration tests for Gap #62 — Canonical Study & Evidence Ledger plus
//! Contract-Driven Game Checks (`src/study_check.rs`, `playtest.rs`, `projects.rs`,
//! `project_api.rs`, `market_taste.rs`).

use openro_rs::agent::Agent;
use openro_rs::mcp::McpClient;
use openro_rs::model::ModelClient;
use openro_rs::server::{serve, Shared};
use openro_rs::study_check;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static DIR_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(label: &str) -> PathBuf {
    let n = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("v011-study-tests");
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
    let shared = Shared::with_data_dir(agent, "Study Test Game".to_string(), data_dir);
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

fn make_png(w: u32, h: u32, pixel_fn: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((h as usize) * ((w as usize) * 3 + 1));
    for y in 0..h {
        raw.push(0u8);
        for x in 0..w {
            raw.extend_from_slice(&pixel_fn(x, y));
        }
    }
    let mut z = vec![0x78u8, 0x01u8];
    let mut pos = 0usize;
    while pos < raw.len() {
        let chunk = (raw.len() - pos).min(65535);
        let final_b = if pos + chunk >= raw.len() { 1u8 } else { 0u8 };
        z.push(final_b);
        let len = chunk as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(&raw[pos..pos + chunk]);
        pos += chunk;
    }
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for &b in &raw {
        s1 = (s1 + b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    z.extend_from_slice(&((s2 << 16) | s1).to_be_bytes());
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xffffffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if (c & 1) != 0 { (c >> 1) ^ 0xedb88320 } else { c >> 1 };
            }
        }
        !c
    }
    fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut buf = Vec::with_capacity(4 + data.len());
        buf.extend_from_slice(kind);
        buf.extend_from_slice(data);
        out.extend_from_slice(&crc32(&buf).to_be_bytes());
    }
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    push_chunk(&mut png, b"IHDR", &ihdr);
    push_chunk(&mut png, b"IDAT", &z);
    push_chunk(&mut png, b"IEND", &[]);
    png
}

fn spawn_mock_roblox_server(universe_id: u64, thumb_png: Vec<u8>) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock roblox");
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let base_cl = base.clone();
    std::thread::spawn(move || {
        for _ in 0..16 {
            let Ok((mut stream, _)) = listener.accept() else { break };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("/");
            if path.starts_with("/v1/games/votes") {
                let body = json!({ "data": [{ "id": universe_id, "upVotes": 9200, "downVotes": 800 }] }).to_string();
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(resp.as_bytes());
            } else if path.starts_with("/v1/games/multiget/thumbnails") {
                let body = json!({
                    "data": [{
                        "universeId": universe_id,
                        "thumbnails": [{ "imageUrl": format!("{base_cl}/thumb1.png") }]
                    }]
                })
                .to_string();
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(resp.as_bytes());
            } else if path.starts_with("/v1/games") {
                let body = json!({
                    "data": [{
                        "id": universe_id,
                        "rootPlaceId": 777001,
                        "name": "Anime Card Arena",
                        "description": "Collect stylized anime cards and battle bosses.",
                        "playing": 4200,
                        "visits": 12000000,
                        "favoritedCount": 85000,
                        "genre": "All",
                        "creator": { "name": "StudioTest" }
                    }]
                })
                .to_string();
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(resp.as_bytes());
            } else if path == "/thumb1.png" {
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", thumb_png.len());
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&thumb_png);
            } else {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        }
    });
    base
}

#[test]
fn evidence_and_unknowns_ledger_loopback_revision_and_fail_closed_resolution() {
    let root = temp_dir("ledger");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    let port = start_server(None, data);

    let (code, _) = api_json(port, "POST", "/api/projects/workspace", Some(json!({ "path": ws })));
    assert_eq!(code, 200);
    let (code, created) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "Study Ledger Demo", "parent": ws, "universeId": 990011 })),
    );
    assert_eq!(code, 201);
    let id = created["project"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["project"]["clean_room"], json!(true));

    // Loopback-only guard on all new study/investigation routes
    for (method, route) in [
        ("GET", format!("/api/projects/{id}/investigation/evidence")),
        ("POST", format!("/api/projects/{id}/investigation/evidence")),
        ("GET", format!("/api/projects/{id}/investigation/unknowns")),
        ("POST", format!("/api/projects/{id}/investigation/unknowns")),
        ("PATCH", format!("/api/projects/{id}/investigation/unknowns")),
        ("POST", format!("/api/projects/{id}/study/check-mechanics")),
        ("GET", format!("/api/projects/{id}/study/status")),
    ] {
        let (c, _, _) = http_req(port, method, &route, "evil.example.com", Some("{}"));
        assert_eq!(c, 403, "expected 403 non-loopback on {method} {route}");
    }

    // clean_room manifest validation & toggle
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/manifest"),
        Some(json!({ "clean_room": "yes" })),
    );
    assert_eq!(code, 400);
    let (code, mf) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/manifest"),
        Some(json!({ "clean_room": false })),
    );
    assert_eq!(code, 200);
    assert_eq!(mf["manifest"]["clean_room"], json!(false));
    let (code, mf) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/manifest"),
        Some(json!({ "clean_room": true })),
    );
    assert_eq!(code, 200);
    assert_eq!(mf["manifest"]["clean_room"], json!(true));

    // Evidence: invalid confidence rejected; valid recorded & deduplicated by canonical SHA-256
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/investigation/evidence"),
        Some(json!({ "summary": "bad enum", "confidence": "guessed" })),
    );
    assert_eq!(code, 400);

    let ev_payload = json!({
        "kind": "mechanics_probe",
        "source": "playtest_obs",
        "summary": "Hard pity triggered on roll #80 during live server check",
        "confidence": "observed",
        "authority": "self_test",
        "coverage": "single_player_session",
        "limitations": ["Tested in Studio Edit/Play Solo only"],
        "data": { "b": 2, "a": 1 }
    });
    let (code, ev1) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/investigation/evidence"),
        Some(ev_payload.clone()),
    );
    assert_eq!(code, 201, "record evidence failed: {ev1}");
    assert_eq!(ev1["deduplicated"], json!(false));
    let ev_id = ev1["evidence"]["id"].as_str().unwrap().to_string();
    assert!(ev_id.starts_with("ev_") && ev_id.len() == 19, "bad id: {ev_id}");
    assert_eq!(ev1["evidence"]["clean_room"], json!(true));

    // Same core content with reordered keys deduplicates to the exact same ev_<16hex>
    let mut ev_reordered = ev_payload.clone();
    ev_reordered["data"] = json!({ "a": 1, "b": 2 });
    let (code, ev2) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/investigation/evidence"),
        Some(ev_reordered),
    );
    assert_eq!(code, 200);
    assert_eq!(ev2["deduplicated"], json!(true));
    assert_eq!(ev2["evidence"]["id"], json!(ev_id));

    let (code, ev_list) = api_json(
        port,
        "GET",
        &format!("/api/projects/{id}/investigation/evidence?confidence=observed"),
        None,
    );
    assert_eq!(code, 200);
    assert_eq!(ev_list["total"], json!(1));
    assert_eq!(ev_list["counts_by_confidence"]["observed"], json!(1));

    // Unknowns: add, duplicate conflict, optimistic revision conflict, fail-closed resolution
    let (code, u1) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/investigation/unknowns"),
        Some(json!({
            "title": "Pity persistence across rejoins",
            "question": "Does the 80-roll pity counter persist across server hops?",
            "severity": "high",
            "domain": "economy",
            "probes": ["Run save/rejoin playtest assertion"]
        })),
    );
    assert_eq!(code, 201, "add unknown failed: {u1}");
    assert_eq!(u1["revision"], json!(1));
    let unk_id = u1["unknown"]["id"].as_str().unwrap().to_string();
    assert!(unk_id.starts_with("unk_") && unk_id.len() == 20, "bad unk id: {unk_id}");
    assert_eq!(u1["unknown"]["revision"], json!(1));

    // Duplicate unknown -> 409
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/investigation/unknowns"),
        Some(json!({
            "title": "Pity persistence across rejoins",
            "question": "Does the 80-roll pity counter persist across server hops?",
            "domain": "economy"
        })),
    );
    assert_eq!(code, 409);

    // Fail-closed: resolving without resolution -> 400
    let (code, _) = api_json(
        port,
        "PATCH",
        &format!("/api/projects/{id}/investigation/unknowns/{unk_id}"),
        Some(json!({ "status": "resolved", "evidence_for": [ev_id] })),
    );
    assert_eq!(code, 400);

    // Fail-closed: resolving with resolution but empty evidence_for -> 400
    let (code, _) = api_json(
        port,
        "PATCH",
        &format!("/api/projects/{id}/investigation/unknowns/{unk_id}"),
        Some(json!({ "status": "resolved", "resolution": "Persists in DataStore" })),
    );
    assert_eq!(code, 400);

    // Fail-closed: resolving with fabricated ev_* ID -> 400
    let (code, _) = api_json(
        port,
        "PATCH",
        &format!("/api/projects/{id}/investigation/unknowns/{unk_id}"),
        Some(json!({
            "status": "resolved",
            "resolution": "Persists in DataStore",
            "evidence_for": ["ev_deadbeef00000000"]
        })),
    );
    assert_eq!(code, 400);

    // Optimistic concurrency mismatch -> 409
    let (code, conflict) = api_json(
        port,
        "PATCH",
        &format!("/api/projects/{id}/investigation/unknowns"),
        Some(json!({
            "id": unk_id,
            "expected_revision": 99,
            "status": "investigating"
        })),
    );
    assert_eq!(code, 409, "expected 409 conflict: {conflict}");
    assert_eq!(conflict["current_revision"], json!(1));

    // Valid resolution with expected_revision=1 + non-empty resolution + recorded evidence_for -> 200
    let (code, resolved) = api_json(
        port,
        "PATCH",
        &format!("/api/projects/{id}/investigation/unknowns/{unk_id}"),
        Some(json!({
            "expected_revision": 1,
            "status": "resolved",
            "resolution": "Confirmed hard pity counter persists in DataStore across rejoins.",
            "evidence_for": [ev_id]
        })),
    );
    assert_eq!(code, 200, "resolve unknown failed: {resolved}");
    assert_eq!(resolved["revision"], json!(2));
    assert_eq!(resolved["unknown"]["revision"], json!(2));
    assert_eq!(resolved["unknown"]["status"], json!("resolved"));

    let (code, u_list) = api_json(
        port,
        "GET",
        &format!("/api/projects/{id}/investigation/unknowns"),
        None,
    );
    assert_eq!(code, 200);
    assert_eq!(u_list["open_count"], json!(0));
    assert_eq!(u_list["counts_by_status"]["resolved"], json!(1));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn mechanics_provenance_checker_counts_and_config_vs_runtime_warnings() {
    let root = temp_dir("mechanics");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    let port = start_server(None, data);

    api_json(port, "POST", "/api/projects/workspace", Some(json!({ "path": ws })));
    let (_, created) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "Mech Check Demo", "parent": ws })),
    );
    let id = created["project"]["id"].as_str().unwrap().to_string();
    let proj_dir = PathBuf::from(created["project"]["path"].as_str().unwrap());

    // Missing mechanics.json -> found=false, passed=false
    let (code, missing) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/study/check-mechanics"),
        Some(json!({})),
    );
    assert_eq!(code, 200);
    assert_eq!(missing["found"], json!(false));
    assert_eq!(missing["passed"], json!(false));

    // Path escape refused
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/study/check-mechanics"),
        Some(json!({ "path": "../escape.json" })),
    );
    assert_eq!(code, 400);

    // Record real evidence ID first
    let (_, ev) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/investigation/evidence"),
        Some(json!({ "summary": "Observed pity at 80", "confidence": "observed" })),
    );
    let ev_id = ev["evidence"]["id"].as_str().unwrap().to_string();

    // Write docs/mechanics.json with mixed provenance + literal config without runtime verification
    std::fs::create_dir_all(proj_dir.join("docs")).unwrap();
    std::fs::write(
        proj_dir.join("docs/mechanics.json"),
        json!({
            "version": 1,
            "mechanics": [
                {
                    "id": "pity-80",
                    "name": "Hard pity at 80",
                    "provenance": "sourced",
                    "evidence_id": ev_id,
                    "config_path": "src/shared/Config/Rarities.luau",
                    "config_value": 80,
                    "verified_runtime": true
                },
                {
                    "id": "boss-hp",
                    "name": "Boss HP scaling",
                    "provenance": "estimated",
                    "config_path": "src/shared/Config/Worlds.luau",
                    "config_value": 500,
                    "verified_runtime": false
                },
                {
                    "id": "unverified-drop",
                    "name": "Secret drop rate",
                    "provenance": "unsourced"
                },
                {
                    "id": "fake-sourced",
                    "name": "Fabricated citation",
                    "provenance": "sourced",
                    "evidence_id": "ev_0000000000000000"
                }
            ]
        })
        .to_string(),
    )
    .unwrap();

    let (code, chk) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/study/check-mechanics"),
        Some(json!({})),
    );
    assert_eq!(code, 200, "check-mechanics failed: {chk}");
    assert_eq!(chk["found"], json!(true));
    assert_eq!(chk["passed"], json!(false));
    assert_eq!(chk["counts"]["total"], json!(4));
    assert_eq!(chk["counts"]["sourced"], json!(1));
    assert_eq!(chk["counts"]["estimated"], json!(1));
    assert_eq!(chk["counts"]["unsourced"], json!(2));
    assert_eq!(chk["counts"]["runtime_verified"], json!(1));
    assert_eq!(chk["counts"]["config_only_warnings"], json!(1));
    let warns = chk["warnings"].as_array().unwrap();
    assert!(
        warns
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("literal config != verified runtime")),
        "expected config!=runtime warning, got: {warns:?}"
    );

    // Config value != runtime value mismatch -> fails even if provenance is sourced
    std::fs::write(
        proj_dir.join("docs/mechanics.json"),
        json!({
            "mechanics": [{
                "id": "pity-80",
                "provenance": "sourced",
                "evidence_id": ev_id,
                "config_value": 80,
                "runtime_value": 100,
                "verified_runtime": true
            }]
        })
        .to_string(),
    )
    .unwrap();
    let (code, mm) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/study/check-mechanics"),
        Some(json!({})),
    );
    assert_eq!(code, 200);
    assert_eq!(mm["passed"], json!(false));
    assert_eq!(mm["counts"]["config_mismatches"], json!(1));

    // Clean sourced + runtime-verified mechanics -> passed: true, 0 warnings
    std::fs::write(
        proj_dir.join("docs/mechanics.json"),
        json!({
            "mechanics": [{
                "id": "pity-80",
                "provenance": "sourced",
                "evidence_id": ev_id,
                "config_path": "src/shared/Config/Rarities.luau",
                "config_value": 80,
                "runtime_value": 80,
                "verified_runtime": true
            }]
        })
        .to_string(),
    )
    .unwrap();
    let (code, ok_res) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/study/check-mechanics"),
        Some(json!({ "strict": true })),
    );
    assert_eq!(code, 200);
    assert_eq!(ok_res["passed"], json!(true), "expected pass: {ok_res}");
    assert_eq!(ok_res["warnings"].as_array().unwrap().len(), 0);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn auto_evidence_core_loop_visual_likeness_blank_reject_and_kept_checks() {
    let root = temp_dir("contract-e2e");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();

    let mcp = McpClient::spawn(&mock_studio_command()).expect("spawn mock_studio");
    let model = ModelClient::new(
        "http://127.0.0.1:1/v1".to_string(),
        "test-model".to_string(),
        None,
    );
    let agent = Agent::new(model, Some(mcp));
    let port = start_server(Some(agent), data);

    api_json(port, "POST", "/api/projects/workspace", Some(json!({ "path": ws })));
    let (_, created) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "Contract Check E2E", "parent": ws, "universeId": 884422 })),
    );
    let id = created["project"]["id"].as_str().unwrap().to_string();
    let proj_dir = PathBuf::from(created["project"]["path"].as_str().unwrap());

    // 1. research_roblox_game_to_project_with_base downloads refs/<universeId>/thumb_1.png
    //    AND auto-records `observed` evidence in .openro/investigation/evidence.jsonl
    let thumb_png = make_png(320, 200, |x, y| {
        if y < 90 {
            [110, 175, 240]
        } else if (x + y) % 16 < 8 {
            [140, 165, 110]
        } else {
            [120, 145, 95]
        }
    });
    let mock_roblox = spawn_mock_roblox_server(884422, thumb_png);
    let res_game = openro_rs::market_taste::research_roblox_game_to_project_with_base(
        &proj_dir,
        "884422",
        None,
        Some(&mock_roblox),
        Duration::from_secs(5),
    )
    .expect("research_roblox_game_to_project_with_base");
    let res_ev_id = res_game["evidence_id"].as_str().expect("auto evidence_id").to_string();
    assert!(res_ev_id.starts_with("ev_"));
    assert!(proj_dir.join("refs/884422/thumb_1.png").is_file());

    // 2. KeptChecks on /check: failures are NEVER cached; passing check caches & respects force:true
    api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/file"),
        Some(json!({ "path": "src/shared/Bad.luau", "content": "local function broken(\n" })),
    );
    let (code, fail1) = api_json(port, "POST", &format!("/api/projects/{id}/check"), Some(json!({})));
    assert_eq!(code, 200);
    assert_eq!(fail1["passed"], json!(false));
    assert_eq!(fail1["kept"], json!(false));
    let (code, fail2) = api_json(port, "POST", &format!("/api/projects/{id}/check"), Some(json!({})));
    assert_eq!(code, 200);
    assert_eq!(fail2["passed"], json!(false));
    assert_eq!(fail2["kept"], json!(false), "failures must NEVER be cached");

    // Fix Bad.luau -> first pass kept=false, second pass kept=true, force=true kept=false
    api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/file"),
        Some(json!({ "path": "src/shared/Bad.luau", "content": "local x = 1\nreturn x\n" })),
    );
    let (code, pass1) = api_json(port, "POST", &format!("/api/projects/{id}/check"), Some(json!({})));
    assert_eq!(code, 200);
    assert_eq!(pass1["passed"], json!(true), "check failed: {pass1}");
    assert_eq!(pass1["kept"], json!(false));
    let fp1 = pass1["fingerprint"].as_str().unwrap().to_string();

    let (code, pass2) = api_json(port, "POST", &format!("/api/projects/{id}/check"), Some(json!({})));
    assert_eq!(code, 200);
    assert_eq!(pass2["passed"], json!(true));
    assert_eq!(pass2["kept"], json!(true), "unchanged passing check should be kept: {pass2}");
    assert_eq!(pass2["cached"], json!(true));
    assert_eq!(pass2["fingerprint"], json!(fp1));

    let (code, forced) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/check"),
        Some(json!({ "force": true })),
    );
    assert_eq!(code, 200);
    assert_eq!(forced["passed"], json!(true));
    assert_eq!(forced["kept"], json!(false), "force:true must bypass KeptChecks");

    // Edit file -> fingerprint changes -> cache miss
    api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/file"),
        Some(json!({ "path": "src/shared/Bad.luau", "content": "local x = 2\nreturn x\n" })),
    );
    let (code, pass3) = api_json(port, "POST", &format!("/api/projects/{id}/check"), Some(json!({})));
    assert_eq!(code, 200);
    assert_eq!(pass3["passed"], json!(true));
    assert_eq!(pass3["kept"], json!(false), "changed file must invalidate KeptChecks");
    assert_ne!(pass3["fingerprint"].as_str().unwrap(), fp1);

    // 3. Build project + Playtest with require_core_loop: true (missing claims -> "not tested" + failed + NOT cached)
    let (code, _) = api_json(port, "POST", &format!("/api/projects/{id}/build"), Some(json!({})));
    assert_eq!(code, 200);

    let incomplete_pt = json!({
        "mode": "start_play",
        "settle_ms": 0,
        "require_core_loop": true,
        "checks": [
            { "name": "boot sanity", "luau": "return true", "expect": { "truthy": true } }
        ]
    });
    let (code, pt_fail) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/playtest"),
        Some(incomplete_pt.clone()),
    );
    assert_eq!(code, 200);
    assert_eq!(pt_fail["passed"], json!(false), "require_core_loop must fail when claims missing");
    assert_eq!(pt_fail["core_loop"]["move"]["status"], json!("not tested"));
    assert_eq!(pt_fail["core_loop"]["act"]["status"], json!("not tested"));
    assert_eq!(pt_fail["core_loop"]["hud"]["status"], json!("not tested"));
    assert_eq!(pt_fail["kept"], json!(false));

    // Re-running failed playtest is NEVER cached
    let (code, pt_fail2) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/playtest"),
        Some(incomplete_pt),
    );
    assert_eq!(code, 200);
    assert_eq!(pt_fail2["passed"], json!(false));
    assert_eq!(pt_fail2["kept"], json!(false));
    assert_ne!(pt_fail2["run_id"], pt_fail["run_id"]);

    // 4. Full core-loop Playtest (move + act + hud + screenshot -> visual_likeness vs refs/884422/thumb_1.png + KeptChecks!)
    let full_pt = json!({
        "mode": "start_play",
        "settle_ms": 0,
        "require_core_loop": true,
        "checks": [
            { "name": "player spawn movement", "claim": "move", "luau": "return true", "expect": { "truthy": true } },
            { "name": "roll card action", "claim": "act", "luau": "return 1", "expect": { "equals": 1 } },
            { "name": "main hud visible", "claim": "hud", "datamodel": "Client", "luau": "return true", "expect": { "truthy": true } }
        ],
        "screenshots": [
            { "name": "hub", "camera_position": [0, 30, 50], "camera_look_at": [0, 0, 0] }
        ]
    });
    let (code, pt_ok) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/playtest"),
        Some(full_pt.clone()),
    );
    assert_eq!(code, 200, "full playtest failed: {pt_ok}");
    assert_eq!(pt_ok["passed"], json!(true), "expected passing playtest: {pt_ok}");
    assert_eq!(pt_ok["kept"], json!(false));
    assert_eq!(pt_ok["core_loop"]["move"]["status"], json!("passed"));
    assert_eq!(pt_ok["core_loop"]["act"]["status"], json!("passed"));
    assert_eq!(pt_ok["core_loop"]["hud"]["status"], json!("passed"));
    assert_eq!(pt_ok["core_loop"]["all_core_passed"], json!(true));
    assert_eq!(pt_ok["blank_screenshots"], json!(0));
    assert_eq!(pt_ok["screenshots"][0]["passed"], json!(true));
    assert_eq!(pt_ok["screenshots"][0]["rejected"], json!(false));
    assert_eq!(pt_ok["visual_likeness"]["normalized_size"], json!([640, 360]));
    assert_eq!(pt_ok["visual_likeness"]["references_compared"], json!(1));
    assert_eq!(pt_ok["visual_likeness"]["best_reference"], json!("refs/884422/thumb_1.png"));
    let score = pt_ok["visual_likeness"]["best_score"].as_f64().unwrap();
    assert!((0.0..=1.0).contains(&score), "score out of range: {score}");
    assert!(pt_ok["evidence_id"].as_str().unwrap_or("").starts_with("ev_"));

    // Repeat identical playtest -> KeptChecks cache hit!
    let (code, pt_kept) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/playtest"),
        Some(full_pt.clone()),
    );
    assert_eq!(code, 200);
    assert_eq!(pt_kept["passed"], json!(true));
    assert_eq!(pt_kept["kept"], json!(true), "expected kept playtest: {pt_kept}");
    assert_eq!(pt_kept["cached"], json!(true));
    assert_eq!(pt_kept["run_id"], pt_ok["run_id"]);

    // force: true bypasses KeptChecks and runs fresh
    let mut forced_pt = full_pt;
    forced_pt["force"] = json!(true);
    let (code, pt_forced) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/playtest"),
        Some(forced_pt),
    );
    assert_eq!(code, 200);
    assert_eq!(pt_forced["passed"], json!(true));
    assert_eq!(pt_forced["kept"], json!(false));
    assert_ne!(pt_forced["run_id"], pt_ok["run_id"]);

    // 5. Study status reports stage="ready" when study + build + check + playtest pass with 0 open unknowns
    let (code, st) = api_json(port, "GET", &format!("/api/projects/{id}/study/status"), None);
    assert_eq!(code, 200, "study/status failed: {st}");
    assert_eq!(st["stage"], json!("ready"), "expected ready stage: {st}");
    assert_eq!(st["clean_room"], json!(true));
    assert!(st["evidence"]["observed"].as_u64().unwrap() >= 2);
    assert_eq!(st["unknowns"]["open_count"], json!(0));
    assert_eq!(st["checks"]["static_check_kept"], json!(true));
    assert_eq!(st["checks"]["playtest_kept"], json!(true));

    // Adding an open unknown moves stage out of "ready" back to "check"
    let (code, _) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/investigation/unknowns"),
        Some(json!({ "title": "Verify mobile safe area", "severity": "medium", "domain": "ui" })),
    );
    assert_eq!(code, 201);
    let (code, st2) = api_json(port, "GET", &format!("/api/projects/{id}/study/status"), None);
    assert_eq!(code, 200);
    assert_eq!(st2["stage"], json!("check"));
    assert_eq!(st2["unknowns"]["open_count"], json!(1));

    // 6. Pure-Rust PNG decoder rejects single-color blank PNG & handles compressed Pillow PNGs if python3+PIL present
    let solid_black = make_png(64, 36, |_, _| [0, 0, 0]);
    let (_, blank_meta) = study_check::inspect_screenshot_bytes(&solid_black);
    assert_eq!(blank_meta["valid"], json!(true));
    assert_eq!(blank_meta["blank"], json!(true));

    if let Ok(out) = std::process::Command::new("python3")
        .args([
            "-c",
            "import io, sys; from PIL import Image; im=Image.new('RGBA',(48,32),(40,120,210,255)); \
             [im.putpixel((x,y),(240,190,60,255)) for y in range(16,32) for x in range(48)]; \
             buf=io.BytesIO(); im.save(buf,format='PNG',compress_level=9); sys.stdout.buffer.write(buf.getvalue())",
        ])
        .output()
    {
        if out.status.success() && out.stdout.starts_with(b"\x89PNG\r\n\x1a\n") {
            let (decoded, meta) = study_check::inspect_screenshot_bytes(&out.stdout);
            assert!(decoded.is_some(), "pure-Rust decoder failed on zlib-compressed RGBA PNG: {meta}");
            assert_eq!(meta["valid"], json!(true));
            assert_eq!(meta["blank"], json!(false));
            assert_eq!(meta["width"], json!(48));
            assert_eq!(meta["height"], json!(32));
        }
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn playtest_rejects_blank_single_color_screenshot_end_to_end() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_dir("blank-shot");
    let ws = root.join("workspace");
    let data = root.join("data");
    std::fs::create_dir_all(&ws).unwrap();

    // Build a tiny Python MCP proxy over mock_studio that substitutes a solid-black PNG on screen_capture
    let blank_png = make_png(64, 36, |_, _| [16, 16, 16]);
    let blank_b64 = openro_rs::assets::encode_base64(&blank_png);
    let proxy = root.join("blank_shot_mcp.py");
    let mock_bin = mock_studio_command();
    let py_code = r#"#!/usr/bin/env python3
import json, subprocess, sys

BLANK_B64 = "__BLANK_B64__"
MOCK_BIN = "__MOCK_BIN__"
p = subprocess.Popen([MOCK_BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
for line in sys.stdin:
    p.stdin.write(line)
    p.stdin.flush()
    try:
        req = json.loads(line)
    except Exception:
        req = {}
    if "id" not in req:
        continue
    out = p.stdout.readline()
    if not out:
        break
    if req.get("method") == "tools/call" and req.get("params", {}).get("name") == "screen_capture":
        resp = json.loads(out)
        resp["result"] = {"content": [{"type": "image", "mimeType": "image/png", "data": BLANK_B64}]}
        out = json.dumps(resp) + "\n"
    sys.stdout.write(out)
    sys.stdout.flush()
"#
    .replace("__BLANK_B64__", &blank_b64)
    .replace("__MOCK_BIN__", &mock_bin);
    std::fs::write(&proxy, py_code).unwrap();
    let mut perms = std::fs::metadata(&proxy).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&proxy, perms).unwrap();

    let mcp = McpClient::spawn(&proxy.to_string_lossy()).expect("spawn blank proxy");
    let model = ModelClient::new(
        "http://127.0.0.1:1/v1".to_string(),
        "test-model".to_string(),
        None,
    );
    let agent = Agent::new(model, Some(mcp));
    let port = start_server(Some(agent), data);

    api_json(port, "POST", "/api/projects/workspace", Some(json!({ "path": ws })));
    let (_, created) = api_json(
        port,
        "POST",
        "/api/projects/create",
        Some(json!({ "name": "Blank Shot Demo", "parent": ws })),
    );
    let id = created["project"]["id"].as_str().unwrap().to_string();

    let (code, pt) = api_json(
        port,
        "POST",
        &format!("/api/projects/{id}/playtest"),
        Some(json!({
            "mode": "start_play",
            "settle_ms": 0,
            "checks": [
                { "name": "ok check", "luau": "return true", "expect": { "truthy": true } }
            ],
            "screenshots": [
                { "name": "black_screen" }
            ]
        })),
    );
    assert_eq!(code, 200, "playtest failed unexpectedly: {pt}");
    assert_eq!(pt["blank_screenshots"], json!(1), "expected 1 blank shot: {pt}");
    assert_eq!(pt["screenshots"][0]["rejected"], json!(true));
    assert_eq!(pt["screenshots"][0]["passed"], json!(false));
    assert_eq!(pt["screenshots"][0]["visual"]["blank"], json!(true));
    assert_eq!(pt["passed"], json!(false), "blank screenshot must fail playtest: {pt}");
    assert_eq!(pt["kept"], json!(false));

    let _ = std::fs::remove_dir_all(&root);
}
