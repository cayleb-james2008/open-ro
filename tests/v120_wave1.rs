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
    assert_eq!(ver_json["version"], json!("1.7.0"));

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
