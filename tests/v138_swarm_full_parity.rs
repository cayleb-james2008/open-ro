//! Offline integration tests for OpenRO v1.38.0 Full Built-In Swarm Tool Parity
//! (Gaps #97-#104):
//! 1. Gap #97: `teardown_play_live`, `teardown_youtube_frames`, `teardown_get`
//! 2. Gap #98: `market_taste_snapshot`, `market_taste_refresh`
//! 3. Gap #99: `economy_sim`, `monetization_audit`, `publish_guard`, `project_dependencies`, `project_snapshot`
//! 4. Gap #100: `contract_init`, `contract_get`, `contract_verify_update` (cold-verifier-only) & `finish_run` gate
//! 5. Gap #101: Pure-Rust `project_inspect_image` (PNG alpha%, border chroma, dominant swatches)
//! 6. Gap #102: On-demand OSS skill discovery & reading (`skills_search`, `skills_read`)
//! 7. Gaps #103 & #104: Studio roles & frontend Teamwork parity

use flate2::write::ZlibEncoder;
use flate2::Compression;
use openro_rs::agent::Agent;
use openro_rs::contract::{contract_get, contract_init, contract_verify_update};
use openro_rs::image_ops::inspect_project_image;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use openro_rs::skills::{read_oss_skill_pack, search_oss_skill_packs};
use openro_rs::teamwork::teamwork_role_prompt_preview;
use openro_rs::teardown::{get_teardown, teardown_youtube_frames};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn scratch(label: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base.join("v138-tests").join(format!(
        "openro_v138_{label}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png_crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn png_chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(body);
    let mut buf = Vec::with_capacity(4 + body.len());
    buf.extend_from_slice(tag);
    buf.extend_from_slice(body);
    out.extend_from_slice(&png_crc32(&buf).to_be_bytes());
}

fn make_sprite_png(w: u32, h: u32, interior: [u8; 4], border: [u8; 4]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let on_border = x < 2 || x + 2 >= w || y < 2 || y + 2 >= h;
            let px = if on_border { border } else { interior };
            raw.extend_from_slice(&px);
        }
    }
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&raw).unwrap();
    let idat = enc.finish().unwrap();
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&w.to_be_bytes());
    ihdr[4..8].copy_from_slice(&h.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6;
    png_chunk(&mut png, b"IHDR", &ihdr);
    png_chunk(&mut png, b"IDAT", &idat);
    png_chunk(&mut png, b"IEND", &[]);
    png
}

fn make_minimal_jpeg(w: u16, h: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0xFF, 0xD8]); // SOI
    out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]); // SOF0
    out.extend_from_slice(&h.to_be_bytes());
    out.extend_from_slice(&w.to_be_bytes());
    out.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00]);
    out.extend_from_slice(&[0xFF, 0xD9]); // EOI
    out
}

fn init_rojo_project(proj_dir: &Path) {
    std::fs::create_dir_all(proj_dir.join("src/server")).unwrap();
    std::fs::create_dir_all(proj_dir.join("src/shared")).unwrap();
    std::fs::create_dir_all(proj_dir.join("src/client")).unwrap();
    std::fs::create_dir_all(proj_dir.join("art/ui")).unwrap();
    std::fs::write(
        proj_dir.join("default.project.json"),
        json!({
            "name": "ParityTestGame",
            "tree": {
                "$className": "DataModel",
                "ReplicatedStorage": { "$className": "ReplicatedStorage", "Shared": { "$path": "src/shared" } },
                "ServerScriptService": { "$className": "ServerScriptService", "Server": { "$path": "src/server" } },
                "StarterPlayer": {
                    "$className": "StarterPlayer",
                    "StarterPlayerScripts": { "$className": "StarterPlayerScripts", "Client": { "$path": "src/client" } }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        proj_dir.join("src/shared/Config.luau"),
        "--!strict\nlocal Config = {}\nConfig.EARN_AMOUNT = 15\nConfig.EARN_SECONDS = 3\nConfig.PACK_COST = 100\nreturn Config\n",
    )
    .unwrap();
    std::fs::write(
        proj_dir.join("src/server/Main.server.luau"),
        "--!strict\nlocal ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\nlocal _Config = require(ReplicatedStorage.Shared.Config)\nprint(\"ready\")\n",
    )
    .unwrap();
    std::fs::write(
        proj_dir.join("art/ui/badge.png"),
        make_sprite_png(32, 32, [34, 197, 94, 255], [0, 0, 0, 0]),
    )
    .unwrap();
}

fn init_git_repo(proj_dir: &Path) {
    std::fs::write(proj_dir.join(".gitignore"), ".openro/\nbuild/\n").unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "Test User"],
        vec!["config", "user.email", "test@example.com"],
        vec!["add", "."],
        vec!["commit", "-q", "-m", "init"],
    ] {
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(proj_dir)
            .args(&args)
            .status()
            .unwrap()
            .success());
    }
}

fn boot_server(dir: &Path, proj_dir: &Path) -> (u16, Arc<Shared>) {
    let data_dir = dir.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let slug = proj_dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_ascii_lowercase();
    std::fs::write(
        data_dir.join("projects.json"),
        json!({
            "roots": [],
            "opened": [proj_dir.to_string_lossy()],
            "active": slug,
        })
        .to_string(),
    )
    .unwrap();
    let shared = Shared::with_data_dir(
        Some(Agent::with_local_place(
            ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None),
            LocalPlace::in_memory("ParityGame".into()),
        )),
        "ParityGame".into(),
        data_dir,
    );
    {
        let mut guard = shared.agent.lock().unwrap();
        if let Some(a) = guard.as_mut() {
            a.set_project(Some(proj_dir.to_path_buf()), Some(slug));
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv);
    });
    (port, shared)
}

fn http_req(port: u16, method: &str, path: &str, host_hdr: Option<&str>, body: Option<&Value>) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let host = host_hdr.unwrap_or("127.0.0.1");
    let body_str = body.map(|v| v.to_string()).unwrap_or_default();
    let mut raw = format!("{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n");
    if body.is_some() {
        raw.push_str("Content-Type: application/json\r\n");
        raw.push_str(&format!("Origin: http://{host}:{port}\r\n"));
        raw.push_str("Sec-Fetch-Site: same-origin\r\n");
        raw.push_str(&format!("Content-Length: {}\r\n", body_str.len()));
    }
    raw.push_str("\r\n");
    raw.push_str(&body_str);
    s.write_all(raw.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = s.read_to_end(&mut resp);
    let txt = String::from_utf8_lossy(&resp);
    let (head, body_part) = txt.split_once("\r\n\r\n").unwrap_or((&txt, "{}"));
    let code = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0);
    let val = serde_json::from_str(body_part).unwrap_or_else(|_| json!({ "raw": body_part }));
    (code, val)
}

fn start_dynamic_stub<F>(handler: F) -> String
where
    F: Fn(&str, usize) -> Value + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handler = Arc::new(handler);
    let counter = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let h = Arc::clone(&handler);
            let cnt = Arc::clone(&counter);
            std::thread::spawn(move || {
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
                            for line in String::from_utf8_lossy(&buf[..pos]).lines() {
                                if let Some((k, v)) = line.split_once(':') {
                                    if k.trim().eq_ignore_ascii_case("content-length") {
                                        content_len = v.trim().parse().unwrap_or(0);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(hend) = header_end {
                        if buf.len() >= hend + content_len {
                            break;
                        }
                    }
                }
                let hend = header_end.unwrap_or(buf.len());
                let body_str = String::from_utf8_lossy(&buf[hend..]);
                let n = cnt.fetch_add(1, Ordering::SeqCst);
                let reply_json = h(&body_str, n);
                let payload = json!({
                    "choices": [{ "message": { "content": reply_json.to_string() } }]
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });
    format!("http://127.0.0.1:{port}/v1")
}

#[test]
fn v138_teardown_play_live_and_youtube_frames_and_http_guards() {
    let _g = ENV_LOCK.lock().unwrap();
    let dir = scratch("teardown-live-yt");
    let proj = dir.join("proj");
    init_rojo_project(&proj);
    let (port, _shared) = boot_server(&dir, &proj);
    let pid = "proj";

    // Uninitialized teardown_get returns 200 has_teardown: false
    let (code, empty_td) = http_req(port, "GET", &format!("/api/projects/{pid}/teardown"), None, None);
    assert_eq!(code, 200);
    assert_eq!(empty_td["has_teardown"], false);

    // Auth required fails closed cleanly with action_required
    std::env::set_var("OPENRO_TEARDOWN_LIVE_AUTH_REQUIRED", "1");
    let (code, auth_res) = http_req(
        port,
        "POST",
        &format!("/api/projects/{pid}/teardown/play-live"),
        None,
        Some(&json!({ "target": "https://www.roblox.com/games/130253630950453/TestGame", "universe_id": 777001 })),
    );
    std::env::remove_var("OPENRO_TEARDOWN_LIVE_AUTH_REQUIRED");
    assert_eq!(code, 200);
    assert_eq!(auth_res["status"], "action_required");
    assert_eq!(auth_res["blocked_reason"], "auth_required");

    // Mock live player PNG capture
    let mock_png = dir.join("live_hud.png");
    std::fs::write(&mock_png, make_sprite_png(64, 48, [16, 185, 129, 255], [0, 0, 0, 255])).unwrap();
    std::env::set_var("OPENRO_TEARDOWN_LIVE_MOCK_PNG", &mock_png);
    let (code, live_res) = http_req(
        port,
        "POST",
        &format!("/api/projects/{pid}/teardown/play-live"),
        None,
        Some(&json!({
            "target": "https://www.roblox.com/games/130253630950453/TestGame",
            "universe_id": 777001,
            "wait_secs": 8,
            "role": "hud"
        })),
    );
    std::env::remove_var("OPENRO_TEARDOWN_LIVE_MOCK_PNG");
    assert_eq!(code, 200, "live_res: {live_res}");
    assert_eq!(live_res["ok"], true);
    assert_eq!(live_res["screenshot"]["source"], "live_player");
    assert_eq!(live_res["screenshot"]["role"], "hud");
    let shot_rel = live_res["screenshot"]["path"].as_str().unwrap();
    assert!(shot_rel.starts_with("refs/teardowns/777001/screenshots/"));
    assert!(proj.join(shot_rel).is_file());

    // Strict YouTube ID validation rejects flag injection & bad URLs
    let bad_yt = teardown_youtube_frames(
        &proj,
        &json!({ "video": "--exec=rm", "timestamps_s": [10], "universe_id": 777001 }),
    );
    assert!(bad_yt.is_err());

    // Mock YouTube keyframe extraction
    let yt_mock_dir = dir.join("yt_frames_mock");
    std::fs::create_dir_all(&yt_mock_dir).unwrap();
    std::fs::write(
        yt_mock_dir.join("dQw4w9WgXcQ_30.png"),
        make_sprite_png(48, 32, [250, 204, 21, 255], [0, 0, 0, 255]),
    )
    .unwrap();
    std::fs::write(
        yt_mock_dir.join("dQw4w9WgXcQ_90.png"),
        make_sprite_png(48, 32, [59, 130, 246, 255], [0, 0, 0, 255]),
    )
    .unwrap();
    std::env::set_var("OPENRO_TEARDOWN_YT_FRAMES_MOCK_DIR", &yt_mock_dir);
    let (code, yt_res) = http_req(
        port,
        "POST",
        &format!("/api/projects/{pid}/teardown/youtube-frames"),
        None,
        Some(&json!({
            "video": "dQw4w9WgXcQ",
            "timestamps_s": [30, 90],
            "roles": ["modal_shop", "pack_open"],
            "universe_id": 777001
        })),
    );
    std::env::remove_var("OPENRO_TEARDOWN_YT_FRAMES_MOCK_DIR");
    assert_eq!(code, 200, "yt_res: {yt_res}");
    assert_eq!(yt_res["captured_count"], 2);
    assert_eq!(yt_res["video_url"], "https://www.youtube.com/watch?v=dQw4w9WgXcQ");

    let td = get_teardown(&proj, Some(777001)).unwrap();
    assert_eq!(td["screenshots"].as_array().unwrap().len(), 3);
    let md = std::fs::read_to_string(proj.join("refs/teardowns/777001/TEARDOWN.md")).unwrap();
    assert!(md.contains("live_player"));
    assert!(md.contains("modal_shop"));

    // Non-loopback Host rejected with 403
    let (code, _) = http_req(
        port,
        "POST",
        &format!("/api/projects/{pid}/teardown/play-live"),
        Some("evil.example"),
        Some(&json!({ "target": "123" })),
    );
    assert_eq!(code, 403);
}

#[test]
fn v138_pure_rust_image_inspector_and_security() {
    let dir = scratch("inspect-image");
    let proj = dir.join("proj");
    init_rojo_project(&proj);
    std::fs::write(
        proj.join("art/ui/dirty_border.png"),
        make_sprite_png(32, 32, [34, 197, 94, 255], [255, 0, 255, 255]),
    )
    .unwrap();
    std::fs::write(proj.join("art/ui/photo.jpg"), make_minimal_jpeg(320, 180)).unwrap();

    let clean = inspect_project_image(&proj, &json!({ "path": "art/ui/badge.png" })).unwrap();
    assert_eq!(clean["width"], 32);
    assert_eq!(clean["height"], 32);
    assert_eq!(clean["format"], "png");
    assert_eq!(clean["has_alpha"], true);
    assert_eq!(clean["exterior_border"]["clean_transparent_border"], true);
    let hexes = clean["dominant_hexes"].as_array().unwrap();
    assert!(hexes.iter().any(|h| h.as_str() == Some("#22C55E")), "hexes: {hexes:?}");

    let dirty = inspect_project_image(&proj, &json!({ "path": "art/ui/dirty_border.png" })).unwrap();
    assert_eq!(dirty["exterior_border"]["clean_transparent_border"], false);
    assert!(dirty["exterior_border"]["residual_chroma_pct"].as_f64().unwrap() > 10.0);

    let jpg = inspect_project_image(&proj, &json!({ "path": "art/ui/photo.jpg" })).unwrap();
    assert_eq!(jpg["format"], "jpg");
    assert_eq!(jpg["width"], 320);
    assert_eq!(jpg["height"], 180);

    // Traversal blocked (CWE-22)
    assert!(inspect_project_image(&proj, &json!({ "path": "../escape.png" })).is_err());

    #[cfg(unix)]
    {
        let outside = dir.join("outside.png");
        std::fs::write(&outside, make_sprite_png(8, 8, [1, 2, 3, 255], [1, 2, 3, 255])).unwrap();
        std::os::unix::fs::symlink(&outside, proj.join("art/ui/sym.png")).unwrap();
        let sym_res = inspect_project_image(&proj, &json!({ "path": "art/ui/sym.png" }));
        assert!(sym_res.is_err());
    }

    let (port, _shared) = boot_server(&dir, &proj);
    let (code, http_res) = http_req(
        port,
        "POST",
        "/api/projects/proj/inspect-image",
        None,
        Some(&json!({ "path": "art/ui/badge.png" })),
    );
    assert_eq!(code, 200);
    assert_eq!(http_res["width"], 32);
    let (code403, _) = http_req(
        port,
        "POST",
        "/api/projects/proj/inspect-image",
        Some("evil.example"),
        Some(&json!({ "path": "art/ui/badge.png" })),
    );
    assert_eq!(code403, 403);
}

#[test]
fn v138_oss_skills_discovery_reading_and_security() {
    let _g = ENV_LOCK.lock().unwrap();
    let dir = scratch("oss-skills");
    let packs_dir = dir.join("packs");
    let luau_dir = packs_dir.join("roblox/roblox-luau");
    let ds_dir = packs_dir.join("roblox/roblox-datastores");
    std::fs::create_dir_all(&luau_dir).unwrap();
    std::fs::create_dir_all(&ds_dir).unwrap();
    std::fs::write(
        luau_dir.join("SKILL.md"),
        "---\nname: roblox-luau\ndescription: Strict Luau patterns and server validation.\n---\n# Roblox Luau\nAlways use --!strict.\n",
    )
    .unwrap();
    std::fs::write(
        ds_dir.join("SKILL.md"),
        "---\nname: roblox-datastores\ndescription: Safe DataStore UpdateAsync retries.\n---\n# DataStores\nUse BindToClose.\n",
    )
    .unwrap();

    #[cfg(unix)]
    {
        let outside_dir = dir.join("outside_skill");
        std::fs::create_dir_all(&outside_dir).unwrap();
        std::fs::write(outside_dir.join("SKILL.md"), "# Secret\n").unwrap();
        std::os::unix::fs::symlink(&outside_dir, packs_dir.join("roblox/evil-sym")).unwrap();
    }

    std::env::set_var("OPENRO_SKILL_PACKS_DIR", &packs_dir);
    let search_all = search_oss_skill_packs("roblox", 10);
    assert_eq!(search_all["count"], 2, "search_all: {search_all}");
    let read_ok = read_oss_skill_pack(&json!({ "path": "roblox/roblox-luau/SKILL.md", "max_chars": 500 })).unwrap();
    assert!(read_ok["content"].as_str().unwrap().contains("Always use --!strict"));

    // Reject traversal & symlink
    assert!(read_oss_skill_pack(&json!({ "path": "../outside_skill/SKILL.md" })).is_err());
    assert!(read_oss_skill_pack(&json!({ "path": "roblox/evil-sym/SKILL.md" })).is_err());

    let proj = dir.join("proj");
    init_rojo_project(&proj);
    let (port, _shared) = boot_server(&dir, &proj);
    let (code, http_search) = http_req(port, "GET", "/api/skills/packs?query=datastore", None, None);
    assert_eq!(code, 200);
    assert_eq!(http_search["count"], 1);
    let (code_read, http_read) = http_req(
        port,
        "POST",
        "/api/skills/packs/read",
        None,
        Some(&json!({ "path": "roblox/roblox-datastores/SKILL.md" })),
    );
    assert_eq!(code_read, 200);
    assert!(http_read["content"].as_str().unwrap().contains("BindToClose"));

    let (code403, _) = http_req(port, "GET", "/api/skills/packs", Some("evil.example"), None);
    assert_eq!(code403, 403);
    std::env::remove_var("OPENRO_SKILL_PACKS_DIR");
}

#[test]
fn v138_contract_engine_and_teamwork_finish_gate() {
    let _g = ENV_LOCK.lock().unwrap();
    let dir = scratch("contract-gate");
    let proj = dir.join("proj");
    init_rojo_project(&proj);
    init_git_repo(&proj);

    // 1. Default-FAIL invariant even if caller attempts passed: true
    let init_res = contract_init(
        &proj,
        &json!({
            "goal": "Ship v1.38 parity",
            "features": [
                { "id": "f1", "title": "Economy balanced", "passed": true, "passes": true },
                { "id": "f2", "title": "HUD badge clean" }
            ]
        }),
    )
    .unwrap();
    assert_eq!(init_res["total"], 2);
    assert_eq!(init_res["passed_count"], 0);
    assert_eq!(init_res["failing_ids"], json!(["f1", "f2"]));

    // 2. Non-verifier rejected, empty evidence rejected
    assert!(contract_verify_update(&proj, &json!({ "id": "f1", "passed": true, "evidence": "ok" }), "builder").is_err());
    assert!(contract_verify_update(&proj, &json!({ "id": "f1", "passed": true, "evidence": "ok" }), "coordinator").is_err());
    assert!(contract_verify_update(&proj, &json!({ "id": "f1", "passed": true, "evidence": "   " }), "verifier").is_err());

    // 3. HTTP endpoints + 403 non-loopback
    let (port, shared) = boot_server(&dir, &proj);
    let (c_get, g_val) = http_req(port, "GET", "/api/projects/proj/contract", None, None);
    assert_eq!(c_get, 200);
    assert_eq!(g_val["has_contract"], true);
    assert_eq!(g_val["passed_count"], 0);
    let (c_patch_bad, _) = http_req(
        port,
        "PATCH",
        "/api/projects/proj/contract",
        None,
        Some(&json!({ "id": "f1", "passed": true, "evidence": "spoof", "role": "builder" })),
    );
    assert_eq!(c_patch_bad, 403);
    let (c_403, _) = http_req(port, "GET", "/api/projects/proj/contract", Some("evil.example"), None);
    assert_eq!(c_403, 403);

    // 4. End-to-end Teamwork run enforcing verifier-only contract grading + finish_run gate
    let stub_url = start_dynamic_stub(|body, _n| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().cloned().unwrap_or_default();
        let sys = msgs.first().and_then(|m| m["content"].as_str()).unwrap_or("");
        let last_user = msgs
            .iter()
            .rev()
            .find(|m| m["role"] == "user")
            .and_then(|m| m["content"].as_str())
            .unwrap_or("");

        if sys.contains("[Agent: coordinator]") {
            if !msgs.iter().skip(1).any(|m| m["content"].as_str().is_some_and(|s| s.contains("create_task"))) {
                return json!({
                    "steps": [
                        { "tool": "contract_init", "args": { "features": [{ "id": "f1", "title": "Economy" }, { "id": "f2", "title": "Badge PNG" }] } },
                        { "tool": "create_task", "args": { "title": "Inspect & audit", "owner": "builder", "verifier": "verifier", "kind": "docs", "files": ["docs/AUDIT.md"] } }
                    ],
                    "done": true,
                    "summary": "waiting"
                });
            }
            if last_user.contains("Verified task t1") && !msgs.iter().skip(1).any(|m| m["content"].as_str().is_some_and(|s| s.contains("cannot finish run while contract checks are failing"))) {
                // First attempt to finish_run MUST fail because f1, f2 are still failing!
                return json!({ "tool": "finish_run", "args": { "summary": "premature finish" } });
            }
            if last_user.contains("cannot finish run while contract checks are failing") {
                return json!({
                    "steps": [
                        { "tool": "board_post", "args": { "text": "@verifier please grade contract features f1 and f2" } }
                    ],
                    "done": true,
                    "summary": "waiting for verifier contract grade"
                });
            }
            if last_user.contains("[done] contract f1 and f2 verified") {
                return json!({ "tool": "finish_run", "args": { "summary": "All tasks and contracts verified." } });
            }
            return json!({ "done": true, "summary": "waiting" });
        }

        if sys.contains("[Agent: builder]") {
            if !msgs.iter().skip(1).any(|m| m["content"].as_str().is_some_and(|s| s.contains("contract_verify_update"))) {
                return json!({
                    "steps": [
                        { "tool": "contract_verify_update", "args": { "id": "f1", "passed": true, "evidence": "self-grade" } }
                    ]
                });
            }
            if last_user.contains("only role 'verifier'") {
                return json!({
                    "steps": [
                        { "tool": "project_inspect_image", "args": { "path": "art/ui/badge.png" } },
                        { "tool": "economy_sim", "args": {} },
                        { "tool": "publish_guard", "args": { "autofix": false } },
                        { "tool": "project_dependencies", "args": { "prune": false } },
                        { "tool": "project_snapshot", "args": { "action": "create", "label": "builder-cp" } },
                        { "tool": "project_write_file", "args": { "path": "docs/AUDIT.md", "content": "# Audit\nVerified tools.\n" } },
                        { "tool": "submit_task", "args": { "summary": "Audit doc & snapshot ready" } }
                    ],
                    "done": true,
                    "summary": "submitted"
                });
            }
            return json!({
                "steps": [
                    { "tool": "project_inspect_image", "args": { "path": "art/ui/badge.png" } },
                    { "tool": "project_write_file", "args": { "path": "docs/AUDIT.md", "content": "# Audit\n" } },
                    { "tool": "submit_task", "args": { "summary": "done" } }
                ],
                "done": true,
                "summary": "submitted"
            });
        }

        if sys.contains("[Agent: verifier]") {
            if last_user.contains("please grade contract features f1 and f2") {
                return json!({
                    "steps": [
                        { "tool": "contract_verify_update", "args": { "id": "f1", "passed": true, "evidence": "economy_sim verified" } },
                        { "tool": "contract_verify_update", "args": { "id": "f2", "passed": true, "evidence": "badge.png 32x32 transparent border" } },
                        { "tool": "board_post", "args": { "text": "@coordinator [done] contract f1 and f2 verified" } }
                    ],
                    "done": true,
                    "summary": "contract verified"
                });
            }
            return json!({
                "steps": [
                    { "tool": "contract_get", "args": {} },
                    { "tool": "verify_task", "args": { "task_id": "t1", "passed": true, "notes": "docs/AUDIT.md ok" } }
                ],
                "done": true,
                "summary": "task verified"
            });
        }

        json!({ "done": true, "summary": "idle" })
    });

    let (cfg_code, _) = http_req(
        port,
        "POST",
        "/api/config",
        None,
        Some(&json!({ "base_url": stub_url, "model_id": "stub-model" })),
    );
    assert_eq!(cfg_code, 200);

    let (code_run, run_res) = http_req(
        port,
        "POST",
        "/api/teamwork",
        None,
        Some(&json!({
            "goal": "Verify contract + parity tools",
            "project_id": "proj",
            "roles": ["builder", "verifier"],
            "max_concurrency": 2
        })),
    );
    assert!(code_run == 200 || code_run == 201, "run_res: {run_res}");
    let run_id = run_res["run"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(12);
    let mut final_snap = Value::Null;
    let mut last_snap = Value::Null;
    while Instant::now() < deadline {
        let (_, s) = http_req(port, "GET", &format!("/api/teamwork/{run_id}"), None, None);
        last_snap = s.clone();
        if s["run"]["status"] == "completed" || s["run"]["status"] == "failed" {
            final_snap = s;
            break;
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    assert_eq!(final_snap["run"]["status"], "completed", "final_snap: {final_snap}\nlast_snap: {last_snap}");
    let final_contract = contract_get(&proj).unwrap();
    assert_eq!(
        final_contract["passed_count"], 2,
        "final_contract: {final_contract}\nboard: {}",
        final_snap["board"]
    );
    assert_eq!(final_contract["failing_ids"], json!([]));
    let _ = shared;
}

#[test]
fn v138_agent_parity_tools_and_studio_roles() {
    let _g = ENV_LOCK.lock().unwrap();
    let dir = scratch("agent-parity");
    let proj = dir.join("proj");
    init_rojo_project(&proj);

    // Local mock market-taste server
    let mt_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mt_port = mt_listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in mt_listener.incoming().flatten() {
            let body = json!({
                "sorts": [{
                    "sortId": "top-trending",
                    "sortDisplayName": "Top Trending",
                    "games": [{
                        "universeId": 998877,
                        "rootPlaceId": 112233,
                        "name": "Anime Card Clash",
                        "playerCount": 25000,
                        "totalUpVotes": 950,
                        "totalDownVotes": 50
                    }]
                }]
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    std::env::set_var("OPENRO_ROBLOX_API_BASE", format!("http://127.0.0.1:{mt_port}"));

    let packs_dir = dir.join("packs");
    std::fs::create_dir_all(packs_dir.join("roblox/roblox-luau")).unwrap();
    std::fs::write(packs_dir.join("roblox/roblox-luau/SKILL.md"), "# Luau\nStrict types.\n").unwrap();
    std::env::set_var("OPENRO_SKILL_PACKS_DIR", &packs_dir);

    let mock_png = dir.join("agent_hud.png");
    std::fs::write(&mock_png, make_sprite_png(32, 32, [255, 0, 0, 255], [0, 0, 0, 255])).unwrap();
    std::env::set_var("OPENRO_TEARDOWN_LIVE_MOCK_PNG", &mock_png);

    let yt_dir = dir.join("yt_mock");
    std::fs::create_dir_all(&yt_dir).unwrap();
    std::fs::write(yt_dir.join("abc12345678_15.png"), make_sprite_png(32, 32, [0, 255, 0, 255], [0, 0, 0, 255])).unwrap();
    std::env::set_var("OPENRO_TEARDOWN_YT_FRAMES_MOCK_DIR", &yt_dir);

    let stub_url = start_dynamic_stub(|body, _n| {
        let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let msgs = req["messages"].as_array().cloned().unwrap_or_default();
        if msgs.len() <= 2 {
            return json!({
                "steps": [
                    { "tool": "teardown_get", "args": {} },
                    { "tool": "teardown_play_live", "args": { "target": "https://www.roblox.com/games/112233/Test", "universe_id": 998877 } },
                    { "tool": "teardown_youtube_frames", "args": { "video": "abc12345678", "timestamps_s": [15], "universe_id": 998877 } },
                    { "tool": "market_taste_snapshot", "args": {} },
                    { "tool": "economy_sim", "args": {} },
                    { "tool": "monetization_audit", "args": {} },
                    { "tool": "publish_guard", "args": { "autofix": false } },
                    { "tool": "project_dependencies", "args": { "prune": false } },
                    { "tool": "project_snapshot", "args": { "action": "create", "label": "agent-cp" } },
                    { "tool": "project_snapshot", "args": { "action": "list" } },
                    { "tool": "contract_init", "args": { "features": [{ "id": "a1", "title": "Check" }] } },
                    { "tool": "contract_get", "args": {} },
                    { "tool": "project_inspect_image", "args": { "path": "art/ui/badge.png" } },
                    { "tool": "skills_search", "args": { "query": "luau" } },
                    { "tool": "skills_read", "args": { "path": "roblox/roblox-luau/SKILL.md" } }
                ],
                "done": true,
                "summary": "All 15 agent parity tools ran"
            });
        }
        json!({ "done": true, "summary": "done" })
    });

    let mut agent = Agent::with_local_place(
        ModelClient::new(stub_url, "stub".into(), None),
        LocalPlace::in_memory("ParityGame".into()),
    );
    agent.set_project(Some(proj.clone()), Some("proj".into()));
    let outcome = agent.run_ask("chat1", "Run all v1.38 parity tools").unwrap();
    assert!(!outcome.refused, "outcome refused: {}", outcome.summary);
    let joined = outcome.transcript.join("\n");
    for tool in [
        "teardown_get",
        "teardown_play_live",
        "teardown_youtube_frames",
        "market_taste_snapshot",
        "economy_sim",
        "monetization_audit",
        "publish_guard",
        "project_dependencies",
        "project_snapshot",
        "contract_init",
        "contract_get",
        "project_inspect_image",
        "skills_search",
        "skills_read",
    ] {
        assert!(joined.contains(tool), "transcript missing {tool}:\n{joined}");
    }

    // Role rules & frontend TeamworkPanel parity checks
    for role in [
        "coordinator",
        "researcher",
        "world",
        "art",
        "audio",
        "ui",
        "systems",
        "builder",
        "verifier",
    ] {
        let p = teamwork_role_prompt_preview(role, "Build game", &proj);
        assert!(p.contains("contract_get"), "{role} missing contract_get");
    }
    let tsx = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend/src/app/teamwork/TeamworkPanel.tsx"),
    )
    .unwrap();
    assert!(tsx.contains(r#"["researcher", "world", "art", "audio", "systems", "ui", "verifier"]"#));
    assert!(tsx.contains("data-contract-pill"));
    assert!(!tsx.contains("transition-colors"));

    std::env::remove_var("OPENRO_ROBLOX_API_BASE");
    std::env::remove_var("OPENRO_SKILL_PACKS_DIR");
    std::env::remove_var("OPENRO_TEARDOWN_LIVE_MOCK_PNG");
    std::env::remove_var("OPENRO_TEARDOWN_YT_FRAMES_MOCK_DIR");
}
