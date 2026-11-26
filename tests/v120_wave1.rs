//! v1.2.0 Wave 1 tests: gzip/ETag/304 + security headers, prompt raw-TCP 413 with open write half,
//! loopback-403 guards, CWE-59 symlink guards, and Ship & Monetization Readiness Autopilot.

use openro_rs::agent::Agent;
use openro_rs::model::ModelClient;
use openro_rs::opencloud::safe_rel;
use openro_rs::place::LocalPlace;
use openro_rs::projects::safe_join;
use openro_rs::secrets::{SecretStore, OPENCLOUD_KEY};
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

fn start_server_with_dir(data_dir: Option<std::path::PathBuf>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let place = LocalPlace::in_memory("Wave1 Obby".to_string());
    let model = ModelClient::new("http://127.0.0.1:1".into(), "stub".into(), None);
    let agent = Agent::with_local_place(model, place);
    let shared = match data_dir {
        Some(dir) => Shared::with_data_dir(Some(agent), "Wave1 Obby".to_string(), dir),
        None => Shared::new(Some(agent), "Wave1 Obby".to_string()),
    };
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    port
}

fn send_raw_bytes(port: u16, raw: &[u8]) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.write_all(raw).unwrap();
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let sep = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..sep]).to_string();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = if sep + 4 <= buf.len() {
        buf[sep + 4..].to_vec()
    } else {
        Vec::new()
    };
    (status, head, body)
}

fn send_json(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let req = if body.is_some() {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
    } else {
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
    };
    let (st, _head, raw_body) = send_raw_bytes(port, req.as_bytes());
    let parsed = serde_json::from_slice(&raw_body).unwrap_or_else(|_| json!({}));
    (st, parsed)
}

#[test]
fn index_serves_gzip_strong_etag_304_and_security_headers() {
    let port = start_server_with_dir(None);

    let req_gz = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept-Encoding: gzip, deflate\r\nConnection: close\r\n\r\n"
    );
    let (st, head, body) = send_raw_bytes(port, req_gz.as_bytes());
    assert_eq!(st, 200);
    let low = head.to_ascii_lowercase();
    assert!(low.contains("content-encoding: gzip"), "missing gzip: {head}");
    assert!(low.contains("vary: accept-encoding"), "missing Vary: {head}");
    assert!(low.contains("x-content-type-options: nosniff"), "{head}");
    assert!(low.contains("x-frame-options: deny"), "{head}");
    assert!(low.contains("referrer-policy: no-referrer"), "{head}");
    assert!(body.starts_with(&[0x1f, 0x8b]), "gzip magic bytes expected");

    let etag = head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("etag:"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .expect("ETag header missing");
    assert!(etag.starts_with('"') && etag.ends_with('"'), "strong ETag expected: {etag}");

    // Verify 304 Not Modified on matching If-None-Match
    let req_304 = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nIf-None-Match: {etag}\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n"
    );
    let (st304, head304, body304) = send_raw_bytes(port, req_304.as_bytes());
    assert_eq!(st304, 304, "{head304}");
    assert!(body304.is_empty());
    let low304 = head304.to_ascii_lowercase();
    assert!(low304.contains("vary: accept-encoding"), "{head304}");
    assert!(low304.contains("x-frame-options: deny"), "{head304}");

    let (st_ver, ver_json) = send_json(port, "GET", "/api/version", None);
    assert_eq!(st_ver, 200);
    assert_eq!(ver_json["version"], json!("1.14.0"));

    // Verify lazy UI JS chunks under /chunks/* also serve gzip + strong ETag + 304 + immutable + security headers
    if let Ok(entries) = std::fs::read_dir("assets/chunks") {
        if let Some(chunk_name) = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|n| n.strip_suffix(".gz").map(String::from).or(Some(n)))
            .find(|n| n.ends_with(".js"))
        {
            let req_chunk = format!(
                "GET /chunks/{chunk_name} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n"
            );
            let (stc, headc, bodyc) = send_raw_bytes(port, req_chunk.as_bytes());
            assert_eq!(stc, 200, "chunk {chunk_name}: {headc}");
            let lowc = headc.to_ascii_lowercase();
            assert!(lowc.contains("content-encoding: gzip"), "{headc}");
            assert!(lowc.contains("x-content-type-options: nosniff"), "{headc}");
            assert!(lowc.contains("immutable"), "{headc}");
            assert!(bodyc.starts_with(&[0x1f, 0x8b]));
            let etagc = headc
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("etag:"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
                .unwrap();
            let req_c304 = format!(
                "GET /chunks/{chunk_name} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nIf-None-Match: {etagc}\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n"
            );
            let (stc304, _, bodyc304) = send_raw_bytes(port, req_c304.as_bytes());
            assert_eq!(stc304, 304);
            assert!(bodyc304.is_empty());
        }
    }
}

#[test]
fn raw_tcp_oversized_content_length_returns_413_promptly_with_write_half_open() {
    let port = start_server_with_dir(None);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let req = format!(
        "POST /api/chat HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: 50000000\r\nConnection: close\r\n\r\n"
    );
    let started = Instant::now();
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().unwrap();
    // Deliberately keep client write half open (no shutdown(Write), no 50MB body).
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let elapsed = started.elapsed();
    let text = String::from_utf8_lossy(&buf);
    assert!(text.starts_with("HTTP/1.1 413"), "expected prompt 413, got: {text}");
    assert!(
        elapsed < Duration::from_secs(1),
        "413 stalled for {elapsed:?} while client write half stayed open"
    );
}

#[test]
fn loopback_403_enforced_across_studio_mcp_creator_hub_opencloud_and_projects() {
    let port = start_server_with_dir(None);
    for (method, path) in [
        ("POST", "/api/studio/mcp/connect"),
        ("POST", "/api/studio/mcp/disconnect"),
        ("GET", "/api/creator-hub/status"),
        ("POST", "/api/creator-hub/setup"),
        ("GET", "/api/opencloud"),
        ("GET", "/api/projects"),
        ("POST", "/api/projects/demo/autopilot"),
        ("POST", "/api/projects/demo/publish-gate"),
    ] {
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: attacker.example:{port}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
        );
        let (st, _head, _body) = send_raw_bytes(port, req.as_bytes());
        assert_eq!(st, 403, "{method} {path} must reject non-loopback Host");
    }
}

#[cfg(unix)]
#[test]
fn cwe59_dangling_and_nested_symlinks_rejected_across_project_mesh_and_audio() {
    use std::os::unix::fs::symlink;

    let base = std::env::temp_dir().join(format!("openro-v120-cwe59-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let proj = base.join("proj");
    let data = base.join("data");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::create_dir_all(data.join("game-assets/audio")).unwrap();
    std::fs::create_dir_all(data.join("game-assets/meshes")).unwrap();

    // Dangling symlink inside project
    symlink(base.join("nonexistent-secret.luau"), proj.join("dangling.luau")).unwrap();
    assert!(safe_join(&proj, "dangling.luau").is_err());
    assert!(safe_rel(&proj, "dangling.luau").is_err());

    // Dangling symlinks in audio & meshes
    symlink(base.join("nonexistent.wav"), data.join("game-assets/audio/trap.wav")).unwrap();
    symlink(base.join("nonexistent.png"), data.join("game-assets/meshes/trap.png")).unwrap();

    let port = start_server_with_dir(Some(data.clone()));
    let (st_aud, _, _) = send_raw_bytes(
        port,
        format!("GET /api/assets/audio/trap.wav HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes(),
    );
    assert_eq!(st_aud, 403);

    let (st_mesh, _, _) = send_raw_bytes(
        port,
        format!("GET /api/assets/meshes/trap.png HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes(),
    );
    assert_eq!(st_mesh, 403);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn ship_readiness_autopilot_fixes_luau_installs_monetization_builds_and_preserves_price_gate() {
    let base = std::env::temp_dir().join(format!("openro-v120-autopilot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let data = base.join("data");
    let ws = base.join("workspace");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();

    // Put a real secret in SecretStore and verify it is never echoed in readiness/autopilot
    let secret_val = "rbx_secret_DO_NOT_ECHO_987654";
    SecretStore::for_data_dir(&data)
        .set(OPENCLOUD_KEY, secret_val)
        .unwrap();

    let port = start_server_with_dir(Some(data));
    let (st_ws, _) = send_json(port, "POST", "/api/projects/workspace", Some(&json!({ "path": ws })));
    assert_eq!(st_ws, 200);

    let (st_cr, cr) = send_json(
        port,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": "Autopilot Game", "universeId": 111222, "placeId": 333444 })),
    );
    assert_eq!(st_cr, 201, "{cr}");
    let pid = cr["project"]["id"].as_str().unwrap().to_string();

    // Add an empty placeholder Luau file and an unapproved product in manifest
    let (st_wf, _) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/file"),
        Some(&json!({ "path": "src/shared/EmptyStub.luau", "content": "   \n" })),
    );
    assert_eq!(st_wf, 200);

    let (st_mf, _) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/manifest"),
        Some(&json!({
            "products": [
                { "key": "vip_pass", "kind": "game_pass", "name": "VIP", "priceRobux": 199, "priceApproved": false }
            ]
        })),
    );
    assert_eq!(st_mf, 200);

    // Before autopilot: readiness has weighted score, pillars, blockers, and fixable actions
    let (st_r0, r0) = send_json(port, "GET", &format!("/api/projects/{pid}/readiness"), None);
    assert_eq!(st_r0, 200, "{r0}");
    assert!(r0["auto_fixable_count"].as_u64().unwrap() >= 3, "{r0}");
    assert!(r0["score_pct"].as_u64().unwrap() < 100, "{r0}");
    assert!(r0["pillars"]["luau"]["max"].as_u64() == Some(20), "{r0}");
    assert!(r0["pillars"]["playtest"]["max"].as_u64() == Some(20), "{r0}");
    assert!(r0["blockers"].as_array().is_some_and(|b| !b.is_empty()), "{r0}");
    assert_eq!(r0["ship_pack_integrity"]["ok"], json!(true), "{r0}");
    assert_eq!(r0["ship_pack_integrity"]["crc_verified"], json!(true), "{r0}");
    assert_eq!(r0["ship_pack_integrity"]["zip_slip_clean"], json!(true), "{r0}");
    assert_eq!(r0["monetization_parity"]["unapproved_prices"], json!(1), "{r0}");
    assert!(!r0.to_string().contains(secret_val));

    // Targeted single-item Auto-Fix: format_and_fix_luau clears Luau empty stub without installing monetization yet
    let (st_fx, fx) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/autopilot"),
        Some(&json!({ "action": "format_and_fix_luau" })),
    );
    assert_eq!(st_fx, 200, "{fx}");
    assert_eq!(fx["readiness"]["luau_errors"], json!(0), "{fx}");
    assert_eq!(fx["readiness"]["has_process_receipt"], json!(false), "{fx}");

    // Run 1-click Ship & Monetization Autopilot (fixes remaining automatable checks and reports +Δ pts)
    let (st_ap, ap) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/autopilot"),
        Some(&json!({ "currency": "Gems", "daily_base_reward": 250 })),
    );
    assert_eq!(st_ap, 200, "{ap}");
    assert!(!ap.to_string().contains(secret_val));
    assert_eq!(ap["readiness"]["luau_errors"], json!(0), "{ap}");
    assert_eq!(ap["readiness"]["has_process_receipt"], json!(true), "{ap}");
    assert_eq!(ap["readiness"]["has_daily_retention"], json!(true), "{ap}");
    assert!(ap["score_delta"].as_i64().unwrap() > 0, "{ap}");
    assert_eq!(ap["after_score"], ap["readiness"]["score_pct"], "{ap}");
    assert!(ap["pillar_deltas"]["monetization"]["delta"].as_i64().unwrap() > 0, "{ap}");
    assert!(ap["readiness"]["score_pct"].as_u64().unwrap() > r0["score_pct"].as_u64().unwrap(), "{ap}");
    // Human price approval must NEVER be bypassed by autopilot
    assert_eq!(ap["readiness"]["unapproved_prices"], json!(1), "{ap}");
    assert_eq!(ap["readiness"]["ready_to_publish"], json!(false), "{ap}");

    // Run 1-click Monetization & Economy Codegen (writes EconomyShopHandler + ShopPromptBridge with 0 Luau errors)
    let (st_ec, ec) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/monetization/codegen"),
        Some(&json!({ "currency": "Gems", "seed_defaults": true })),
    );
    assert_eq!(st_ec, 200, "{ec}");
    assert_eq!(ec["luau_errors"], json!(0), "{ec}");
    let written = ec["written"].as_array().unwrap();
    assert!(written.iter().any(|v| v == "src/server/EconomyShopHandler.server.luau"), "{ec}");
    assert!(written.iter().any(|v| v == "src/client/ShopPromptBridge.client.luau"), "{ec}");

    // Pre-Publish Revenue & Playtest Gate checks all shop bindings, retention, build + ZIP CRC-32, and playtest status
    let (st_pg, pg) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/publish-gate"),
        Some(&json!({})),
    );
    assert_eq!(st_pg, 200, "{pg}");
    assert_eq!(pg["economy_bindings_ok"], json!(true), "{pg}");
    assert_eq!(pg["retention_ok"], json!(true), "{pg}");
    assert_eq!(pg["build_and_zip_ok"], json!(true), "{pg}");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn v170_wave6_tier_presets_and_comeback_retention() {
    let base = std::env::temp_dir().join(format!(
        "openro-v170-wave6-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let data = base.join("data");
    let ws = base.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();

    let port = start_server_with_dir(Some(data));
    let (st_ws, _) = send_json(port, "POST", "/api/projects/workspace", Some(&json!({ "path": ws })));
    assert_eq!(st_ws, 200);

    let (st_cr, cr) = send_json(
        port,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": "Anime Gacha Demo", "universeId": 555666, "placeId": 777888 })),
    );
    assert_eq!(st_cr, 201, "{cr}");
    let pid = cr["project"]["id"].as_str().unwrap().to_string();

    // Apply Anime Gacha / RPG tier preset in project mode
    let (st_pk, pk) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/monetization/pack"),
        Some(&json!({ "tier_preset": "anime_gacha_rpg", "daily_base_reward": 150 })),
    );
    assert_eq!(st_pk, 200, "{pk}");
    assert_eq!(pk["tier_preset"], json!("anime_gacha_rpg"), "{pk}");
    assert_eq!(pk["comeback_multiplier"], json!(3), "{pk}");
    assert_eq!(
        pk["retention_curve"],
        json!([150, 300, 450, 750, 1050, 1500, 2250]),
        "{pk}"
    );
    let diags = pk["diagnostics"].as_array().unwrap();
    assert!(
        !diags.iter().any(|d| d["severity"] == "error"),
        "expected 0 Luau errors: {pk}"
    );

    // Every seeded draft product must keep priceApproved = false
    let (st_rd, rd) = send_json(port, "GET", &format!("/api/projects/{pid}/readiness"), None);
    assert_eq!(st_rd, 200, "{rd}");
    assert!(rd["unapproved_prices"].as_u64().unwrap() >= 5, "{rd}");

    // Also verify Place-level /api/monetization/pack supports competitive_arena preset
    let (st_loc, loc) = send_json(
        port,
        "POST",
        "/api/monetization/pack",
        Some(&json!({ "tier_preset": "competitive_arena" })),
    );
    assert_eq!(st_loc, 200, "{loc}");
    assert_eq!(loc["tier_preset"], json!("competitive_arena"), "{loc}");
    assert_eq!(loc["comeback_multiplier"], json!(2), "{loc}");
    assert_eq!(
        loc["retention_curve"],
        json!([120, 240, 360, 480, 720, 960, 1440]),
        "{loc}"
    );
    assert_eq!(loc["audit"]["error_count"], json!(0), "{loc}");
    assert_eq!(loc["audit"]["ok"], json!(true), "{loc}");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn v180_wave7_deterministic_playtest_assertions_and_bundle_diet() {
    let base = std::env::temp_dir().join(format!(
        "openro-v180-wave7-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let data = base.join("data");
    let ws = base.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();

    let port = start_server_with_dir(Some(data));

    // Verify /play deep link serves 200 OK and font/woff2 chunk asset serves cleanly
    let req_play = format!("GET /play HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n");
    let (st_play, head_play, body_play) = send_raw_bytes(port, req_play.as_bytes());
    assert_eq!(st_play, 200, "{head_play}");
    assert!(body_play.len() < 115 * 1024, "expected initial gzip <115 KB, got {}", body_play.len());

    if let Ok(entries) = std::fs::read_dir("assets/chunks") {
        if let Some(woff) = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .find(|n| n.ends_with(".woff2"))
        {
            let req_w = format!("GET /chunks/{woff} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n");
            let (stw, headw, _bodyw) = send_raw_bytes(port, req_w.as_bytes());
            assert_eq!(stw, 200, "{headw}");
            assert!(headw.to_ascii_lowercase().contains("content-type: font/woff2"), "{headw}");
        }
    }

    let (st_ws, _) = send_json(port, "POST", "/api/projects/workspace", Some(&json!({ "path": ws })));
    assert_eq!(st_ws, 200);
    let (st_cr, cr) = send_json(
        port,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": "Playtest Lab Game" })),
    );
    assert_eq!(st_cr, 201, "{cr}");
    let pid = cr["project"]["id"].as_str().unwrap().to_string();

    // First deterministic assertion run (monetization_guard initially fails before installing pack)
    let (st_r1, r1) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/playtests/rerun"),
        Some(&json!({ "offline": true })),
    );
    assert_eq!(st_r1, 200, "{r1}");
    assert_eq!(r1["mode"], json!("deterministic_suite"), "{r1}");
    assert!(r1["assertions"].as_array().is_some_and(|a| a.len() >= 6), "{r1}");

    // Install monetization pack and re-run assertions -> monetization_guard flips to delta: "fixed"
    let (st_pk, _) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/monetization/pack"),
        Some(&json!({ "tier_preset": "starter_obby" })),
    );
    assert_eq!(st_pk, 200);

    let (st_r2, r2) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/playtests/rerun"),
        Some(&json!({ "offline": true })),
    );
    assert_eq!(st_r2, 200, "{r2}");
    assert!(r2["assertion_diff"]["fixed"].as_u64().unwrap_or(0) >= 1, "{r2}");
    let econ = r2["assertion_diff"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == "monetization_guard")
        .unwrap();
    assert_eq!(econ["delta"], json!("fixed"), "{r2}");

    // Place-level /api/studio/playtest also returns structured assertions
    let (st_sp, sp) = send_json(port, "POST", "/api/studio/playtest", Some(&json!({})));
    assert_eq!(st_sp, 200, "{sp}");
    assert!(sp["assertions"].as_array().is_some_and(|a| !a.is_empty()), "{sp}");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn v190_wave8_targeted_assertion_fix_plan_and_apply() {
    let base = std::env::temp_dir().join(format!(
        "openro-v190-wave8-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let data = base.join("data");
    let ws = base.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();

    let port = start_server_with_dir(Some(data));
    let (st_ws, _) = send_json(port, "POST", "/api/projects/workspace", Some(&json!({ "path": ws })));
    assert_eq!(st_ws, 200);
    let (st_cr, cr) = send_json(
        port,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": "AutoFix Assertion Game" })),
    );
    assert_eq!(st_cr, 201, "{cr}");
    let pid = cr["project"]["id"].as_str().unwrap().to_string();

    // Baseline assertion run: monetization_guard fails before fix
    let (st_r1, r1) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/playtests/rerun"),
        Some(&json!({ "offline": true })),
    );
    assert_eq!(st_r1, 200, "{r1}");
    let econ1 = r1["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "monetization_guard")
        .unwrap();
    assert_eq!(econ1["passed"], json!(false), "{r1}");

    // Draft targeted fix plan for monetization_guard
    let (st_fp, fp) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/playtests/fix-plan"),
        Some(&json!({ "assertion_id": "monetization_guard" })),
    );
    assert_eq!(st_fp, 200, "{fp}");
    assert_eq!(fp["plan"]["assertion_id"], json!("monetization_guard"), "{fp}");
    assert_eq!(fp["plan"]["auto_fixable"], json!(true), "{fp}");

    // Apply targeted fix and re-run assertions -> monetization_guard becomes fixed & passed
    let (st_fa, fa) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/playtests/fix-apply"),
        Some(&json!({ "assertion_id": "monetization_guard", "offline": true })),
    );
    assert_eq!(st_fa, 200, "{fa}");
    assert!(fa["applied_files"].as_array().is_some_and(|a| a.len() == 3), "{fa}");
    let econ2 = fa["rerun"]["assertion_diff"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "monetization_guard")
        .unwrap();
    assert_eq!(econ2["passed"], json!(true), "{fa}");
    assert_eq!(econ2["delta"], json!("fixed"), "{fa}");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn v1100_wave9_launch_autopilot_fix_all_and_export_ready() {
    let base = std::env::temp_dir().join(format!(
        "openro-v1100-wave9-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let data = base.join("data");
    let ws = base.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();

    let port = start_server_with_dir(Some(data));
    let (st_ws, _) = send_json(port, "POST", "/api/projects/workspace", Some(&json!({ "path": ws })));
    assert_eq!(st_ws, 200);
    let (st_cr, cr) = send_json(
        port,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": "Wave9Autopilot" })),
    );
    assert_eq!(st_cr, 201, "{cr}");
    let pid = cr["project"]["id"].as_str().unwrap().to_string();

    // Baseline rerun fails monetization_guard before fix-all
    let (st_r1, r1) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/playtests/rerun"),
        Some(&json!({ "offline": true })),
    );
    assert_eq!(st_r1, 200, "{r1}");
    assert_eq!(r1["passed"], json!(false), "{r1}");

    // 1-click Launch Autopilot batch-fixes failing assertions, builds .rbxl, and checks Ship Pack ZIP CRC
    let (st_all, all) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/playtests/fix-all"),
        Some(&json!({ "offline": true })),
    );
    assert_eq!(st_all, 200, "{all}");
    assert_eq!(all["export_ready"], json!(true), "{all}");
    assert!(all["fixed_ids"].as_array().is_some_and(|a| a.iter().any(|x| x == "monetization_guard")), "{all}");
    assert_eq!(all["ship_pack_integrity"]["ok"], json!(true), "{all}");
    assert_eq!(all["ship_pack_integrity"]["crc_verified"], json!(true), "{all}");
    assert_eq!(all["ship_pack_integrity"]["zip_slip_clean"], json!(true), "{all}");
    assert_eq!(all["rerun"]["passed"], json!(true), "{all}");
    assert_eq!(all["export_urls"]["rbxlx"], json!("/api/export/rbxlx"));
    assert_eq!(all["export_urls"]["ship_pack"], json!("/api/export/bundle"));

    // Active studio place fix-all endpoint also fixes monetization scripts & returns export_ready
    let (st_studio, studio) = send_json(port, "POST", "/api/studio/playtest/fix-all", Some(&json!({})));
    assert_eq!(st_studio, 200, "{studio}");
    assert_eq!(studio["ok"], json!(true), "{studio}");
    assert_eq!(studio["export_ready"], json!(true), "{studio}");
    assert_eq!(studio["playtest"]["passed"], json!(true), "{studio}");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn v1110_wave10_economy_funnel_simulator_and_keep_alive() {
    let base = std::env::temp_dir().join(format!(
        "openro-v1110-wave10-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let data = base.join("data");
    let ws = base.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();

    let port = start_server_with_dir(Some(data));
    let (st_ws, _) = send_json(port, "POST", "/api/projects/workspace", Some(&json!({ "path": ws })));
    assert_eq!(st_ws, 200);
    let (st_cr, cr) = send_json(
        port,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": "Wave10EconomySim" })),
    );
    assert_eq!(st_cr, 201, "{cr}");
    let pid = cr["project"]["id"].as_str().unwrap().to_string();

    // Baseline simulation before monetization pack shows high-severity blockers
    let (st_s1, s1) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/monetization/simulate"),
        Some(&json!({})),
    );
    assert_eq!(st_s1, 200, "{s1}");
    assert_eq!(s1["schema"], json!("openro.economy_funnel.v1"));
    assert!(s1["blockers"].as_array().is_some_and(|b| b.iter().any(|x| x["id"] == "missing_process_receipt")));
    let base_arpdau = s1["projections"]["arpdau_robux"].as_f64().unwrap();

    // Wire anime_gacha_rpg codegen -> ARPDAU, D1/D7 lift, and conversion jump cleanly
    let (st_cg, _) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/monetization/codegen"),
        Some(&json!({ "tier_preset": "anime_gacha_rpg", "seed_defaults": true })),
    );
    assert_eq!(st_cg, 200);
    let (st_s2, s2) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/monetization/simulate"),
        Some(&json!({})),
    );
    assert_eq!(st_s2, 200, "{s2}");
    assert_eq!(s2["tier_preset"], json!("anime_gacha_rpg"));
    assert!(s2["projections"]["arpdau_robux"].as_f64().unwrap() > base_arpdau, "{s2}");
    assert!(s2["projections"]["d1_lift_pct"].as_f64().unwrap() >= 8.5, "{s2}");
    assert!(s2["projections"]["d7_lift_pct"].as_f64().unwrap() >= 5.0, "{s2}");

    // Local studio economy simulate + loopback 403 guard
    let (st_loc, loc) = send_json(port, "POST", "/api/studio/economy/simulate", Some(&json!({ "tier_preset": "competitive_arena" })));
    assert_eq!(st_loc, 200, "{loc}");
    assert_eq!(loc["tier_preset"], json!("competitive_arena"));
    let bad_req = format!(
        "POST /api/studio/economy/simulate HTTP/1.1\r\nHost: attacker.example:{port}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    let (st_403, _, _) = send_raw_bytes(port, bad_req.as_bytes());
    assert_eq!(st_403, 403);

    // HTTP/1.1 Keep-Alive reuse: two sequential requests over one socket
    let mut ka = TcpStream::connect(("127.0.0.1", port)).unwrap();
    ka.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let r1 = format!("GET /api/version HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: keep-alive\r\n\r\n");
    ka.write_all(r1.as_bytes()).unwrap();
    ka.flush().unwrap();
    let mut first_buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        let n = ka.read(&mut tmp).unwrap();
        assert!(n > 0, "unexpected EOF on first keep-alive response");
        first_buf.extend_from_slice(&tmp[..n]);
        let s = String::from_utf8_lossy(&first_buf);
        if s.contains("1.14.0") {
            assert!(s.contains("Connection: keep-alive"), "{s}");
            break;
        }
    }
    let r2 = format!("GET /api/status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    ka.write_all(r2.as_bytes()).unwrap();
    ka.flush().unwrap();
    let mut second_buf = Vec::new();
    let _ = ka.read_to_end(&mut second_buf);
    let s2_txt = String::from_utf8_lossy(&second_buf);
    assert!(s2_txt.contains("HTTP/1.1 200 OK"), "{s2_txt}");
    assert!(s2_txt.contains("1.14.0"), "{s2_txt}");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn v1120_wave11_luau_dependency_graph_and_pruner() {
    let base = std::env::temp_dir().join(format!(
        "openro-v1120-wave11-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let data = base.join("data");
    let ws = base.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();

    let port = start_server_with_dir(Some(data));
    let (st_ws, _) = send_json(port, "POST", "/api/projects/workspace", Some(&json!({ "path": ws })));
    assert_eq!(st_ws, 200);
    let (st_cr, cr) = send_json(
        port,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": "Wave11DepGraph" })),
    );
    assert_eq!(st_cr, 201, "{cr}");
    let pid = cr["project"]["id"].as_str().unwrap().to_string();
    let proj_dir = std::path::PathBuf::from(cr["project"]["path"].as_str().unwrap());

    // Seed: 1 valid ModuleScript (required with typo), 1 unused ModuleScript, 1 broken dead require, 1 orphan RemoteEvent
    std::fs::create_dir_all(proj_dir.join("src/shared")).unwrap();
    std::fs::create_dir_all(proj_dir.join("src/server")).unwrap();
    std::fs::write(
        proj_dir.join("src/shared/RewardMath.luau"),
        "--!strict\nreturn { Double = function(x: number): number return x * 2 end }\n",
    )
    .unwrap();
    std::fs::write(
        proj_dir.join("src/shared/UnusedDeadModule.luau"),
        "--!strict\nreturn { dead = true }\n",
    )
    .unwrap();
    std::fs::write(
        proj_dir.join("src/server/DepTest.server.luau"),
        "--!strict\nlocal ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\nlocal Shared = ReplicatedStorage:WaitForChild(\"Shared\")\nlocal _dead = require(Shared:WaitForChild(\"NoSuchModule\"))\nlocal mathMod = require(ReplicatedStorage:WaitForChild(\"rewardmath\"))\nlocal orphanEv = Instance.new(\"RemoteEvent\")\norphanEv.Name = \"DeadOrphanRemote\"\norphanEv.Parent = ReplicatedStorage\nprint(mathMod.Double(4))\n",
    )
    .unwrap();

    // GET dependencies flags broken require, unused module, and orphan remote
    let (st_g1, g1) = send_json(port, "GET", &format!("/api/projects/{pid}/dependencies"), None);
    assert_eq!(st_g1, 200, "{g1}");
    assert_eq!(g1["schema"], json!("openro.dependency_graph.v1"));
    assert_eq!(g1["summary"]["healthy"], json!(false), "{g1}");
    assert!(g1["summary"]["broken_count"].as_u64().unwrap() >= 2, "{g1}");
    assert_eq!(g1["summary"]["unused_count"], json!(1), "{g1}");
    assert_eq!(g1["summary"]["orphan_count"], json!(1), "{g1}");

    // 1-Click Prune / Fix Links resolves typo link, prunes dead require, deletes unused module, removes orphan remote
    let (st_pr, pr) = send_json(port, "POST", &format!("/api/projects/{pid}/dependencies/prune"), Some(&json!({})));
    assert_eq!(st_pr, 200, "{pr}");
    assert_eq!(pr["pruned_modules"], json!(1), "{pr}");
    assert!(pr["fixed_requires"].as_u64().unwrap() >= 2, "{pr}");
    assert_eq!(pr["pruned_remotes"], json!(1), "{pr}");
    assert_eq!(pr["after"]["summary"]["healthy"], json!(true), "{pr}");
    assert_eq!(pr["after"]["summary"]["broken_count"], json!(0), "{pr}");
    assert_eq!(pr["after"]["summary"]["unused_count"], json!(0), "{pr}");
    assert_eq!(pr["after"]["summary"]["orphan_count"], json!(0), "{pr}");
    assert!(!proj_dir.join("src/shared/UnusedDeadModule.luau").exists());

    // Local studio dependency routes work & enforce loopback 403
    let (st_loc, loc) = send_json(port, "GET", "/api/studio/dependencies", None);
    assert_eq!(st_loc, 200, "{loc}");
    assert_eq!(loc["schema"], json!("openro.dependency_graph.v1"));
    let bad_req = format!(
        "POST /api/studio/dependencies/prune HTTP/1.1\r\nHost: attacker.example:{port}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    let (st_403, _, _) = send_raw_bytes(port, bad_req.as_bytes());
    assert_eq!(st_403, 403);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn v1130_wave12_snapshot_diff_and_safe_rollback() {
    let base = std::env::temp_dir().join(format!(
        "openro-v1130-wave12-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let data = base.join("data");
    let ws = base.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();

    let port = start_server_with_dir(Some(data));
    let (st_ws, _) = send_json(port, "POST", "/api/projects/workspace", Some(&json!({ "path": ws })));
    assert_eq!(st_ws, 200);
    let (st_cr, cr) = send_json(
        port,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": "Wave12Snapshots" })),
    );
    assert_eq!(st_cr, 201, "{cr}");
    let pid = cr["project"]["id"].as_str().unwrap().to_string();
    let proj_dir = std::path::PathBuf::from(cr["project"]["path"].as_str().unwrap());

    // Ensure clean verified baseline via fix-all
    let (st_fix, _) = send_json(port, "POST", &format!("/api/projects/{pid}/playtests/fix-all"), Some(&json!({})));
    assert_eq!(st_fix, 200);
    let good_src = "--!strict\nlocal Players = game:GetService(\"Players\")\nPlayers.PlayerAdded:Connect(function(p)\n  print(p.Name)\nend)\n";
    std::fs::create_dir_all(proj_dir.join("src/server")).unwrap();
    std::fs::write(proj_dir.join("src/server/GameLoop.server.luau"), good_src).unwrap();

    let (st_s1, s1) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/snapshots"),
        Some(&json!({ "label": "Verified Baseline" })),
    );
    assert_eq!(st_s1, 200, "{s1}");
    let s1_id = s1["snapshot"]["snapshot_id"].as_str().unwrap().to_string();
    assert_eq!(s1["snapshot"]["verified"], json!(true), "{s1}");
    assert_eq!(s1["latest_verified_id"], json!(s1_id), "{s1}");

    // Introduce broken Luau + extra file, then take second snapshot
    std::fs::write(
        proj_dir.join("src/server/GameLoop.server.luau"),
        "function brokenLoop(\n  local bad = 1\n",
    )
    .unwrap();
    std::fs::write(
        proj_dir.join("src/server/TempBroken.server.luau"),
        "local x = 1\n",
    )
    .unwrap();
    let (st_s2, s2) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/snapshots"),
        Some(&json!({ "label": "Broken Experiment" })),
    );
    assert_eq!(st_s2, 200, "{s2}");
    let s2_id = s2["snapshot"]["snapshot_id"].as_str().unwrap().to_string();
    assert_eq!(s2["snapshot"]["verified"], json!(false), "{s2}");
    assert_eq!(s2["latest_verified_id"], json!(s1_id), "{s2}");

    // GET snapshots returns sequential diff + assertion delta
    let (st_list, list) = send_json(port, "GET", &format!("/api/projects/{pid}/snapshots"), None);
    assert_eq!(st_list, 200, "{list}");
    assert_eq!(list["schema"], json!("openro.project_snapshots.v1"));
    assert_eq!(list["latest_verified_id"], json!(s1_id));
    let snaps = list["snapshots"].as_array().unwrap();
    let broken_item = snaps.iter().find(|x| x["snapshot_id"] == json!(s2_id)).unwrap();
    let changes = broken_item["changed_scripts"].as_array().unwrap();
    assert!(changes.iter().any(|c| c["path"] == json!("src/server/GameLoop.server.luau") && c["op"] == json!("modified")), "{changes:?}");
    assert!(changes.iter().any(|c| c["path"] == json!("src/server/TempBroken.server.luau") && c["op"] == json!("added")), "{changes:?}");
    assert!(broken_item["assertion_delta"]["passed_delta"].as_i64().unwrap() < 0, "{broken_item}");

    // 1-Click Safe Rollback to verified snapshot restores clean script & deletes newer file
    let (st_rst, rst) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/snapshots/{s1_id}/restore"),
        Some(&json!({ "auto_checkpoint": true })),
    );
    assert_eq!(st_rst, 200, "{rst}");
    assert_eq!(rst["restored_id"], json!(s1_id), "{rst}");
    assert_eq!(rst["rerun"]["passed"], json!(true), "{rst}");
    assert!(!proj_dir.join("src/server/TempBroken.server.luau").exists());
    assert_eq!(std::fs::read_to_string(proj_dir.join("src/server/GameLoop.server.luau")).unwrap(), good_src);

    // Traversal + 404 + loopback-403 protections
    let (st_bad, _) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/snapshots/..%2Fevil/restore"),
        Some(&json!({})),
    );
    assert_eq!(st_bad, 400);
    let (st_miss, _) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/snapshots/psnap-nonexistent/restore"),
        Some(&json!({})),
    );
    assert_eq!(st_miss, 404);
    let bad_req = format!(
        "POST /api/projects/{pid}/snapshots/{s1_id}/restore HTTP/1.1\r\nHost: attacker.example:{port}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    let (st_403, _, _) = send_raw_bytes(port, bad_req.as_bytes());
    assert_eq!(st_403, 403);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn v1140_wave13_hook_integrity_and_surgical_restore() {
    let base = std::env::temp_dir().join(format!(
        "openro-v1140-wave13-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let data = base.join("data");
    let ws = base.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();

    let port = start_server_with_dir(Some(data));
    let (st_ws, _) = send_json(port, "POST", "/api/projects/workspace", Some(&json!({ "path": ws })));
    assert_eq!(st_ws, 200);
    let (st_cr, cr) = send_json(
        port,
        "POST",
        "/api/projects/create",
        Some(&json!({ "name": "Wave13HookGuard" })),
    );
    assert_eq!(st_cr, 201, "{cr}");
    let pid = cr["project"]["id"].as_str().unwrap().to_string();
    let proj_dir = std::path::PathBuf::from(cr["project"]["path"].as_str().unwrap());

    // Seed verified monetization + SpawnLocation + --!strict baseline
    let (st_cg, _) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/monetization/codegen"),
        Some(&json!({ "tier_preset": "anime_gacha_rpg", "vip_pass_id": 777111, "starter_pack_id": 888222, "seed_defaults": true })),
    );
    assert_eq!(st_cg, 200);
    let (st_fix, _) = send_json(port, "POST", &format!("/api/projects/{pid}/playtests/fix-all"), Some(&json!({})));
    assert_eq!(st_fix, 200);
    std::fs::write(
        proj_dir.join("src/server/SpawnArena.server.luau"),
        "--!strict\nlocal Players = game:GetService(\"Players\")\nlocal Workspace = game:GetService(\"Workspace\")\nlocal sp = Instance.new(\"SpawnLocation\")\nsp.Name = \"SpawnLocation\"\nsp.Anchored = true\nsp.Parent = Workspace\nPlayers.PlayerAdded:Connect(function(p) print(p.Name) end)\n",
    )
    .unwrap();

    let (st_s1, s1) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/snapshots"),
        Some(&json!({ "label": "Verified Hook Baseline" })),
    );
    assert_eq!(st_s1, 200, "{s1}");
    let s1_id = s1["snapshot"]["snapshot_id"].as_str().unwrap().to_string();
    assert_eq!(s1["snapshot"]["verified"], json!(true), "{s1}");

    // Simulate AI regression: add a NEW feature script that MUST be preserved, while dropping --!strict, SpawnLocation, ProcessReceipt, and zeroing VipGamePassId
    let new_feature_src = "--!strict\nlocal BossArena = {}\nfunction BossArena.Start() print(\"Boss Arena active\") end\nreturn BossArena\n";
    std::fs::write(proj_dir.join("src/server/BossArenaFeature.server.luau"), new_feature_src).unwrap();
    std::fs::write(
        proj_dir.join("src/server/SpawnArena.server.luau"),
        "local Workspace = game:GetService(\"Workspace\")\nprint(Workspace.Name)\n",
    )
    .unwrap();
    let cfg_path = proj_dir.join("src/shared/MonetizationConfig.luau");
    let cfg_raw = std::fs::read_to_string(&cfg_path).unwrap();
    std::fs::write(&cfg_path, cfg_raw.replace("VipGamePassId = 777111", "VipGamePassId = 0")).unwrap();
    let svc_path = proj_dir.join("src/server/MonetizationService.server.luau");
    std::fs::remove_file(&svc_path).unwrap();

    // Preflight Hook-Integrity detects regressions while tracking preserved new script
    let (st_hi, hi) = send_json(port, "GET", &format!("/api/projects/{pid}/snapshots/hook-integrity"), None);
    assert_eq!(st_hi, 200, "{hi}");
    assert_eq!(hi["schema"], json!("openro.hook_integrity.v1"));
    assert_eq!(hi["healthy"], json!(false), "{hi}");
    assert!(hi["missing_count"].as_u64().unwrap() >= 4, "{hi}");
    assert!(
        hi["preserved_new_scripts"].as_array().unwrap().iter().any(|p| p == &json!("src/server/BossArenaFeature.server.luau")),
        "{hi}"
    );

    // 1-Click Surgical Restore restores missing hooks WITHOUT clobbering BossArenaFeature.server.luau
    let (st_rh, rh) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/snapshots/{s1_id}/restore-hooks"),
        Some(&json!({ "auto_checkpoint": true })),
    );
    assert_eq!(st_rh, 200, "{rh}");
    assert!(rh["hooks_restored"].as_u64().unwrap() >= 4, "{rh}");
    assert_eq!(rh["after"]["healthy"], json!(true), "{rh}");
    assert_eq!(rh["rerun"]["passed"], json!(true), "{rh}");
    assert_eq!(
        std::fs::read_to_string(proj_dir.join("src/server/BossArenaFeature.server.luau")).unwrap(),
        new_feature_src,
        "new feature script must be preserved untouched"
    );
    assert!(svc_path.is_file(), "MonetizationService.server.luau must be restored");
    assert!(std::fs::read_to_string(&cfg_path).unwrap().contains("VipGamePassId = 777111"));
    let spawn_after = std::fs::read_to_string(proj_dir.join("src/server/SpawnArena.server.luau")).unwrap();
    assert!(spawn_after.starts_with("--!strict\n"), "{spawn_after}");
    assert!(spawn_after.contains("SpawnLocation"), "{spawn_after}");
    let svc_after = std::fs::read_to_string(&svc_path).unwrap();
    assert!(svc_after.contains("ProcessReceipt"), "{svc_after}");

    // Traversal + 404 + loopback-403 protections
    let (st_bad, _) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/snapshots/..%2Fevil/restore-hooks"),
        Some(&json!({})),
    );
    assert_eq!(st_bad, 400);
    let (st_miss, _) = send_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/snapshots/psnap-missing/restore-hooks"),
        Some(&json!({})),
    );
    assert_eq!(st_miss, 404);
    let bad_req = format!(
        "GET /api/studio/snapshots/hook-integrity HTTP/1.1\r\nHost: attacker.example:{port}\r\nConnection: close\r\n\r\n"
    );
    let (st_403, _, _) = send_raw_bytes(port, bad_req.as_bytes());
    assert_eq!(st_403, 403);

    let _ = std::fs::remove_dir_all(&base);
}
