//! Offline integration tests for OpenRO v1.37.0 backend deliverables (Gaps #93-#96 + mesh preset):
//! 1. Live Game Teardown Engine (`src/teardown.rs` + `/api/projects/:id/teardown*` loopback routes)
//! 2. Reference-Screenshot UI Variant Default & `meme-pop-arcade` 7th skin (`src/ui_kit.rs`)
//! 3. SHA-256 + `force` `stage-content` fix (`src/studio_embed.rs`, `src/server.rs`)
//! 4. Rojo `default.project.json` module mapping for `assets_codegen` / `products_codegen` (`src/opencloud.rs`)
//! 5. `"meme-arcade-plaza"` 8-prop Blender map kit preset (`src/mesh.rs`)

use flate2::write::ZlibEncoder;
use flate2::Compression;
use openro_rs::agent::Agent;
use openro_rs::mesh::map_kit_prop_specs;
use openro_rs::model::ModelClient;
use openro_rs::opencloud::{assets_codegen, products_codegen, resolve_rojo_shared_module_out};
use openro_rs::place::LocalPlace;
use openro_rs::projects::ProjectStore;
use openro_rs::secrets::SecretStore;
use openro_rs::server::{serve, Shared};
use openro_rs::studio_embed::{
    stage_project_studio_content, stage_project_studio_content_with_options,
};
use openro_rs::teardown::{
    default_teardown_ui_references, get_teardown, merge_teardown_spec,
    record_teardown_screenshot, teardown_roblox_game, E2E_SPEC_PILLARS,
};
use openro_rs::ui_kit::{
    build_ui_variant_prompt, find_skin, generate_ui_kit, presets_json, visual_skins,
    wcag_contrast_ratio,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn scratch(label: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base.join("v137-tests").join(format!(
        "openro_v137_{label}_{}_{}",
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

fn make_png(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut row = Vec::with_capacity(1 + w as usize * 4);
    row.push(0);
    for _ in 0..w {
        row.extend_from_slice(&[r, g, b, 255]);
    }
    let mut raw = Vec::with_capacity(row.len() * h as usize);
    for _ in 0..h {
        raw.extend_from_slice(&row);
    }
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&raw).unwrap();
    let idat = enc.finish().unwrap();
    let mut png = Vec::new();
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
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

fn make_sheet_png(w: u32, h: u32, cols: u32, rows: u32) -> Vec<u8> {
    let cw = w / cols;
    let ch = h / rows;
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for y in 0..h {
        raw.push(0);
        let ly = y % ch;
        for x in 0..w {
            let lx = x % cw;
            let in_fg = lx >= 8 && lx < cw - 8 && ly >= 8 && ly < ch - 8;
            if in_fg {
                raw.extend_from_slice(&[34, 197, 94, 255]);
            } else {
                raw.extend_from_slice(&[255, 0, 255, 255]);
            }
        }
    }
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&raw).unwrap();
    let idat = enc.finish().unwrap();
    let mut png = Vec::new();
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
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

fn start_mock_roblox_server(png_bytes: Vec<u8>) -> (String, std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_c = Arc::clone(&stop);

    std::thread::spawn(move || {
        let base = format!("http://{addr}");
        while !stop_c.load(std::sync::atomic::Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut s, _)) => {
                    let mut buf = [0u8; 4096];
                    let n = s.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (ctype, body): (&str, Vec<u8>) = if path.starts_with("/universes/v1/places/") {
                        ("application/json", json!({ "universeId": 99887766u64 }).to_string().into_bytes())
                    } else if path.starts_with("/v1/games/votes") {
                        ("application/json", json!({ "data": [{ "id": 99887766u64, "upVotes": 9500, "downVotes": 500 }] }).to_string().into_bytes())
                    } else if path.starts_with("/v1/games/multiget/thumbnails") {
                        ("application/json", json!({
                            "data": [{
                                "universeId": 99887766u64,
                                "thumbnails": [{ "imageUrl": format!("{base}/img/thumb1.png") }]
                            }]
                        }).to_string().into_bytes())
                    } else if path.starts_with("/v1/games") {
                        ("application/json", json!({
                            "data": [{
                                "id": 99887766u64,
                                "rootPlaceId": 11223344u64,
                                "name": "Meme Arcade Rush",
                                "description": "Collect memes, fuse units, raid dungeons.",
                                "playing": 4200,
                                "visits": 12500000,
                                "favoritedCount": 310000,
                                "genre_l1": "Simulator",
                                "creator": { "name": "ArcadeStudio" }
                            }]
                        }).to_string().into_bytes())
                    } else if path.starts_with("/v1/universes/99887766/badges") {
                        ("application/json", json!({
                            "data": [
                                { "id": 1, "name": "First Roll", "description": "Open your first meme pack", "enabled": true, "statistics": { "awardedCount": 50000, "winRatePercentage": 0.72 } },
                                { "id": 2, "name": "Speed Demon", "description": "Reach Zone 3", "enabled": true, "statistics": { "awardedCount": 12000, "winRatePercentage": 0.21 } }
                            ]
                        }).to_string().into_bytes())
                    } else if path.starts_with("/game-passes/v1/universes/99887766/game-passes") {
                        ("application/json", json!({
                            "gamePasses": [
                                { "id": 701, "name": "VIP", "price": 299, "isForSale": true },
                                { "id": 702, "name": "2x Speed", "price": 149, "isForSale": true }
                            ]
                        }).to_string().into_bytes())
                    } else if path.starts_with("/v2/games/99887766/media") {
                        ("application/json", json!({
                            "data": [
                                { "assetTypeId": 1, "assetType": "Image", "imageId": 55501, "approved": true }
                            ]
                        }).to_string().into_bytes())
                    } else if path.starts_with("/v1/assets") {
                        ("application/json", json!({
                            "data": [{ "targetId": 55501, "state": "Completed", "imageUrl": format!("{base}/img/media1.png") }]
                        }).to_string().into_bytes())
                    } else if path.starts_with("/img/") {
                        ("image/png", png_bytes.clone())
                    } else {
                        ("application/json", b"{}".to_vec())
                    };
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(hdr.as_bytes());
                    let _ = s.write_all(&body);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    (format!("http://{addr}"), stop)
}

fn http_req_host(
    port: u16,
    method: &str,
    path: &str,
    host_hdr: &str,
    body: Option<&Value>,
) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let req = if body.is_some() {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host_hdr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
    } else {
        format!("{method} {path} HTTP/1.1\r\nHost: {host_hdr}\r\nConnection: close\r\n\r\n")
    };
    s.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    s.read_to_end(&mut resp).unwrap();
    let text = String::from_utf8_lossy(&resp);
    let (head, body_str) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0);
    let json_val = serde_json::from_str(body_str).unwrap_or_else(|_| json!({ "raw": body_str }));
    (code, json_val)
}

fn boot_server(dir: &Path) -> (u16, String, PathBuf, Arc<Shared>) {
    let data = dir.join("data");
    let pdir = dir.join("project");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(pdir.join("src/shared")).unwrap();
    std::fs::create_dir_all(pdir.join("art")).unwrap();
    std::fs::write(
        pdir.join("openro.project.json"),
        json!({ "name": "V137 Test Game", "slug": "v137-test-game" }).to_string(),
    )
    .unwrap();
    std::fs::write(
        pdir.join("default.project.json"),
        json!({
            "name": "V137 Test Game",
            "tree": {
                "$className": "DataModel",
                "ReplicatedStorage": {
                    "$className": "ReplicatedStorage",
                    "Shared": { "$path": "src/shared" }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None);
    let agent = Agent::with_local_place(model, LocalPlace::open(data.clone(), "V137Test".into()));
    let shared = Shared::with_data_dir(Some(agent), "V137Test".into(), data);
    let pid = {
        let mut store: std::sync::MutexGuard<'_, ProjectStore> = shared.projects.lock().unwrap();
        let entry = store.open(pdir.to_str().unwrap()).unwrap();
        let id = entry.id.clone();
        store.set_active(&id);
        id
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv = Arc::clone(&shared);
    std::thread::spawn(move || {
        let _ = serve(listener, srv);
    });
    (port, pid, pdir, shared)
}

#[test]
fn teardown_engine_and_ui_variant_end_to_end_offline() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("teardown_ui");
    let (port, pid, pdir, _shared) = boot_server(&dir);
    let png = make_png(64, 64, 34, 197, 94);
    let (api_base, stop_srv) = start_mock_roblox_server(png.clone());

    std::env::set_var("OPENRO_ROBLOX_API_BASE", &api_base);
    std::env::set_var(
        "OPENRO_TEARDOWN_YT_MOCK_JSON",
        json!({
            "videos": [
                {
                    "id": "dQw4w9WgXcQ",
                    "title": "Meme Arcade Rush Full Guide & Tier List",
                    "channel": "RoGuide",
                    "duration": 310,
                    "view_count": 48000,
                    "captions": "First roll a common meme, upgrade speed on treadmill, fuse 3 units."
                }
            ]
        })
        .to_string(),
    );
    std::env::set_var(
        "OPENRO_TEARDOWN_WEB_MOCK_JSON",
        json!({
            "search_results": [{ "title": "Meme Arcade Rush Wiki", "url": "https://example.com/wiki" }],
            "pages": [{ "url": "https://example.com/wiki", "excerpt": "Zones: Plaza, Training Track, Dungeon Gate." }]
        })
        .to_string(),
    );

    // 1. Non-loopback host rejected with 403 Forbidden
    let (st_forb, _) = http_req_host(
        port,
        "POST",
        &format!("/api/projects/{pid}/teardown"),
        "evil.example.com:80",
        Some(&json!({ "target": "11223344" })),
    );
    assert_eq!(st_forb, 403);

    // 2. Run POST /api/projects/:id/teardown
    let (st_td, td) = http_req_host(
        port,
        "POST",
        &format!("/api/projects/{pid}/teardown"),
        &format!("127.0.0.1:{port}"),
        Some(&json!({
            "target": "https://www.roblox.com/games/11223344/Meme-Arcade-Rush",
            "new_niche": "meme-arcade",
            "include_youtube": true,
            "include_wiki": true,
            "max_videos": 4
        })),
    );
    assert_eq!(st_td, 200, "teardown failed: {td}");
    assert_eq!(td["ok"], true);
    assert_eq!(td["universe_id"], 99887766);
    assert_eq!(td["badges"].as_array().unwrap().len(), 2);
    assert_eq!(td["game_passes"].as_array().unwrap().len(), 2);
    assert_eq!(td["media"].as_array().unwrap().len(), 1);
    assert_eq!(td["youtube_research"]["video_count"], 1);
    assert_eq!(td["wiki_research"]["page_count"], 1);
    for pillar in E2E_SPEC_PILLARS {
        assert!(td["e2e_spec"][pillar].is_object(), "missing pillar {pillar}");
    }
    assert!(pdir.join("refs/teardowns/99887766/teardown.json").is_file());
    assert!(pdir.join("refs/teardowns/99887766/TEARDOWN.md").is_file());
    assert!(pdir.join("refs/teardowns/latest.json").is_file());
    assert!(pdir.join(".openro/investigation/evidence.jsonl").is_file());

    // 3. GET /api/projects/:id/teardown
    let (st_get, got) = http_req_host(
        port,
        "GET",
        &format!("/api/projects/{pid}/teardown"),
        &format!("127.0.0.1:{port}"),
        None,
    );
    assert_eq!(st_get, 200);
    assert_eq!(got["universe_id"], 99887766);

    // 4. Record live HUD screenshot -> prioritized ahead of storefront_thumbnail
    std::fs::create_dir_all(pdir.join("captures")).unwrap();
    std::fs::write(pdir.join("captures/live_hud.png"), &png).unwrap();
    std::fs::write(pdir.join("captures/bad.png"), b"NOT_A_PNG").unwrap();

    let (st_bad_shot, _) = http_req_host(
        port,
        "POST",
        &format!("/api/projects/{pid}/teardown/screenshot"),
        &format!("127.0.0.1:{port}"),
        Some(&json!({ "path": "captures/bad.png", "role": "hud" })),
    );
    assert_eq!(st_bad_shot, 400);

    let (st_shot, shot_res) = http_req_host(
        port,
        "POST",
        &format!("/api/projects/{pid}/teardown/screenshot"),
        &format!("127.0.0.1:{port}"),
        Some(&json!({
            "path": "captures/live_hud.png",
            "role": "hud",
            "timestamp_s": 12.5,
            "notes": "Bottom-center dock + top-right coins",
            "copy_observed": ["ROLL", "EQUIP BEST"]
        })),
    );
    assert_eq!(st_shot, 200, "screenshot failed: {shot_res}");
    let rec_refs = shot_res["recommended_ui_reference_screenshots"]
        .as_array()
        .unwrap();
    assert!(
        rec_refs[0].as_str().unwrap().contains("refs/teardowns/99887766/screenshots/"),
        "live hud screenshot should be prioritized first: {rec_refs:?}"
    );

    // 5. Merge E2E spec pillar
    let (st_spec, spec_res) = http_req_host(
        port,
        "POST",
        &format!("/api/projects/{pid}/teardown/spec"),
        &format!("127.0.0.1:{port}"),
        Some(&json!({
            "e2e_spec": {
                "core_loop": {
                    "verified_loop_ms": 18000
                }
            }
        })),
    );
    assert_eq!(st_spec, 200, "spec merge failed: {spec_res}");
    assert_eq!(spec_res["e2e_spec"]["core_loop"]["verified_loop_ms"], 18000);

    // Verify unknown pillar fails with 400
    let err_spec = merge_teardown_spec(&pdir, &json!({ "pillar": "unknown_pillar", "summary": "x" }));
    assert!(err_spec.is_err());

    // 6. default_teardown_ui_references returns up to 3 valid PNGs
    let default_refs = default_teardown_ui_references(&pdir, None);
    assert!(!default_refs.is_empty() && default_refs.len() <= 3);
    assert!(default_refs[0].starts_with("refs/teardowns/99887766/screenshots/"));

    // 7. Gap #94: meme-pop-arcade skin + WCAG >= 5.25 + reference-guided UI variant default
    assert_eq!(visual_skins().len(), 7);
    assert_eq!(presets_json()["skins"].as_array().unwrap().len(), 7);
    let meme_skin = find_skin("meme-pop-arcade").expect("meme-pop-arcade skin");
    for (mode, tok) in [("dark", &meme_skin.dark), ("light", &meme_skin.light)] {
        assert!(wcag_contrast_ratio(tok.text, tok.surface) >= 5.25, "{mode} text");
        assert!(wcag_contrast_ratio(tok.text_muted, tok.surface) >= 5.25, "{mode} muted");
        assert!(wcag_contrast_ratio(tok.accent_fg, tok.accent) >= 5.25, "{mode} accent");
        assert!(wcag_contrast_ratio(tok.danger_fg, tok.danger) >= 5.25, "{mode} danger");
    }

    let var_prompt =
        build_ui_variant_prompt("meme-pop-arcade", "panels_sheet", Some("Meme Arcade Pop"), 2)
            .unwrap();
    assert!(!var_prompt.contains('\n'));
    assert!(var_prompt.contains("NOT a 1:1 clone"));
    assert!(var_prompt.contains("Meme Arcade Pop"));

    // Mock ChatGPT imagegen CLI to verify `--mode edit --ref-image ...` and zero `--character-art`
    let captured_argv_json = dir.join("captured_argv.json");
    let sheet_png = make_sheet_png(512, 256, 4, 2);
    let mock_sheet_src = dir.join("mock_sheet.png");
    std::fs::write(&mock_sheet_src, &sheet_png).unwrap();
    let mock_chatgpt_py = dir.join("mock_chatgpt_imagegen.py");
    std::fs::write(
        &mock_chatgpt_py,
        format!(
            r#"import json, os, shutil, sys
argv = sys.argv[1:]
open({:?}, "w").write(json.dumps(argv))
out = argv[argv.index("-o") + 1]
os.makedirs(os.path.dirname(out), exist_ok=True)
shutil.copyfile({:?}, out)
print(json.dumps({{"ok": True, "path": out}}))
"#,
            captured_argv_json.to_string_lossy(),
            mock_sheet_src.to_string_lossy()
        ),
    )
    .unwrap();
    std::env::set_var("OPENRO_SHARED_BROWSER_PY", "python3");
    std::env::set_var("OPENRO_CHATGPT_IMAGEGEN_BIN", &mock_chatgpt_py);

    let data_dir = dir.join("data");
    let store = SecretStore::for_data_dir(&data_dir);
    let ui_res = generate_ui_kit(
        &pdir,
        &store,
        Some(&pid),
        &json!({
            "data_dir": data_dir.to_string_lossy(),
            "provider": "chatgpt-browser-cli",
            "skin": "meme-pop-arcade",
            "sheet": "panels_sheet",
            "layout": "skybridge-crest",
            "variant_theme": "Meme Arcade Pop"
        }),
    )
    .expect("generate_ui_kit reference variant");
    assert_eq!(ui_res["ok"], true);
    assert_eq!(ui_res["reference_guided"], true);
    assert_eq!(ui_res["variant_mode"], "reference_variant");
    assert_eq!(ui_res["variant_theme"], "Meme Arcade Pop");
    assert!(!ui_res["reference_images"].as_array().unwrap().is_empty());

    let captured_argv: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(&captured_argv_json).unwrap()).unwrap();
    assert!(captured_argv.windows(2).any(|w| w == ["--mode", "edit"]), "expected --mode edit: {captured_argv:?}");
    assert!(captured_argv.iter().any(|a| a == "--ref-image"), "expected --ref-image: {captured_argv:?}");
    assert!(!captured_argv.iter().any(|a| a == "--character-art"), "character_art must be false: {captured_argv:?}");

    // Verify use_teardown_reference=false disables auto-attaching teardown screenshots
    let ui_clean = generate_ui_kit(
        &pdir,
        &store,
        Some(&pid),
        &json!({
            "data_dir": data_dir.to_string_lossy(),
            "provider": "chatgpt-browser-cli",
            "skin": "meme-pop-arcade",
            "sheet": "panels_sheet",
            "use_teardown_reference": false
        }),
    )
    .expect("generate_ui_kit clean");
    assert_eq!(ui_clean["reference_guided"], false);
    assert_eq!(ui_clean["variant_mode"], "clean_sheet");

    // Verify direct teardown helpers work too
    let td_got = get_teardown(&pdir, None).unwrap();
    assert_eq!(td_got["universe_id"], 99887766);
    let _ = teardown_roblox_game;
    let _ = record_teardown_screenshot;

    std::env::remove_var("OPENRO_SHARED_BROWSER_PY");
    std::env::remove_var("OPENRO_CHATGPT_IMAGEGEN_BIN");
    std::env::remove_var("OPENRO_ROBLOX_API_BASE");
    std::env::remove_var("OPENRO_TEARDOWN_YT_MOCK_JSON");
    std::env::remove_var("OPENRO_TEARDOWN_WEB_MOCK_JSON");
    stop_srv.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[test]
fn stage_content_detects_same_len_sha256_change_and_force_overwrite() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("stage_hash");
    let (port, pid, pdir, _shared) = boot_server(&dir);
    let studio_content = dir.join("studio_content");
    std::fs::create_dir_all(&studio_content).unwrap();
    std::env::set_var("OPENRO_STUDIO_CONTENT_DIR", &studio_content);

    let ui_dir = pdir.join("art/ui");
    std::fs::create_dir_all(&ui_dir).unwrap();
    let img_path = ui_dir.join("panel.png");
    let png_a = make_png(16, 16, 10, 20, 30);
    let png_b = make_png(16, 16, 200, 100, 50);
    assert_eq!(png_a.len(), png_b.len(), "test PNGs must have identical byte length");
    std::fs::write(&img_path, &png_a).unwrap();

    // First stage
    let s1 = stage_project_studio_content(&pdir, "hash-test").unwrap();
    assert_eq!(s1["staged_count"], 1);
    assert_eq!(s1["overwritten_count"], 0);
    assert_eq!(s1["skipped_count"], 0);

    // Second stage unchanged -> skipped
    let s2 = stage_project_studio_content(&pdir, "hash-test").unwrap();
    assert_eq!(s2["staged_count"], 0);
    assert_eq!(s2["overwritten_count"], 0);
    assert_eq!(s2["skipped_count"], 1);

    // Replace with identical length & preserve destination mtime (or older mtime) -> SHA-256 differs -> MUST overwrite!
    let staged_dst = studio_content.join("openro/hash-test/art/ui/panel.png");
    let dst_mtime = std::fs::metadata(&staged_dst).unwrap().modified().unwrap();
    std::fs::write(&img_path, &png_b).unwrap();
    {
        let f = std::fs::OpenOptions::new().write(true).open(&img_path).unwrap();
        let _ = f.set_modified(dst_mtime);
    }

    let s3 = stage_project_studio_content(&pdir, "hash-test").unwrap();
    assert_eq!(s3["staged_count"], 1, "must overwrite on sha256 mismatch: {s3}");
    assert_eq!(s3["overwritten_count"], 1);
    assert_eq!(s3["skipped_count"], 0);
    assert_eq!(std::fs::read(&staged_dst).unwrap(), png_b);

    // Force=true overwrites even when bytes match
    let s4 = stage_project_studio_content_with_options(&pdir, "hash-test", true).unwrap();
    assert_eq!(s4["staged_count"], 1);
    assert_eq!(s4["overwritten_count"], 1);
    assert_eq!(s4["force"], true);

    // HTTP endpoint wires body.force
    let (st_http, res_http) = http_req_host(
        port,
        "POST",
        &format!("/api/projects/{pid}/studio/stage-content"),
        &format!("127.0.0.1:{port}"),
        Some(&json!({ "slug": "hash-test", "force": true })),
    );
    assert_eq!(st_http, 200);
    assert_eq!(res_http["staged_count"], 1);
    assert_eq!(res_http["overwritten_count"], 1);
    assert_eq!(res_http["force"], true);

    std::env::remove_var("OPENRO_STUDIO_CONTENT_DIR");
}

#[test]
fn rojo_shared_module_resolution_for_assets_and_products_codegen() {
    let dir = scratch("rojo_codegen");

    // 1. Explicit module $path + casing preservation
    let p1 = dir.join("p_explicit");
    std::fs::create_dir_all(p1.join("src/CustomShared")).unwrap();
    std::fs::write(
        p1.join("default.project.json"),
        json!({
            "name": "explicit-proj",
            "tree": {
                "$className": "DataModel",
                "ReplicatedStorage": {
                    "$className": "ReplicatedStorage",
                    "Shared": { "$path": "src/CustomShared" },
                    "AssetIds": { "$path": "src/CustomShared/MyAssetIds.luau" }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let (out1, from1, node1) =
        resolve_rojo_shared_module_out(&p1, "AssetIds.luau", "src/shared/AssetIds.luau");
    assert_eq!(out1, "src/CustomShared/MyAssetIds.luau");
    assert!(from1);
    assert_eq!(node1.as_deref(), Some("ReplicatedStorage.AssetIds"));

    let cg_a = assets_codegen(&p1, &json!({})).unwrap();
    assert_eq!(cg_a["path"], "src/CustomShared/MyAssetIds.luau");
    assert_eq!(cg_a["resolved_from_rojo"], true);
    assert_eq!(cg_a["rojo_node"], "ReplicatedStorage.AssetIds");
    assert!(p1.join("src/CustomShared/MyAssetIds.luau").is_file());

    // ProductIds falls back to ReplicatedStorage.Shared.$path ("src/CustomShared/ProductIds.luau")
    let cg_p = products_codegen(&p1, &json!({})).unwrap();
    assert_eq!(cg_p["path"], "src/CustomShared/ProductIds.luau");
    assert_eq!(cg_p["resolved_from_rojo"], true);
    assert_eq!(cg_p["rojo_node"], "ReplicatedStorage.Shared");
    assert!(p1.join("src/CustomShared/ProductIds.luau").is_file());

    // 2. Existing file under any ReplicatedStorage child $path
    let p2 = dir.join("p_existing");
    std::fs::create_dir_all(p2.join("src/GameData")).unwrap();
    std::fs::write(p2.join("src/GameData/AssetIds.luau"), "-- old\n").unwrap();
    std::fs::write(
        p2.join("default.project.json"),
        json!({
            "tree": {
                "ReplicatedStorage": {
                    "Data": { "$path": "src/GameData" }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let (out2, from2, node2) =
        resolve_rojo_shared_module_out(&p2, "AssetIds.luau", "src/shared/AssetIds.luau");
    assert_eq!(out2, "src/GameData/AssetIds.luau");
    assert!(from2);
    assert_eq!(node2.as_deref(), Some("ReplicatedStorage.Data"));

    // 3. Direct ReplicatedStorage.$path
    let p3 = dir.join("p_direct");
    std::fs::create_dir_all(p3.join("src/ReplicatedStorage")).unwrap();
    std::fs::write(
        p3.join("default.project.json"),
        json!({
            "tree": {
                "ReplicatedStorage": { "$path": "src/ReplicatedStorage" }
            }
        })
        .to_string(),
    )
    .unwrap();
    let (out3, from3, node3) =
        resolve_rojo_shared_module_out(&p3, "AssetIds.luau", "src/shared/AssetIds.luau");
    assert_eq!(out3, "src/ReplicatedStorage/AssetIds.luau");
    assert!(from3);
    assert_eq!(node3.as_deref(), Some("ReplicatedStorage"));

    // 4. CWE-22 traversal rejected -> falls back safely
    let p4 = dir.join("p_traversal");
    std::fs::create_dir_all(&p4).unwrap();
    std::fs::write(
        p4.join("default.project.json"),
        json!({
            "tree": {
                "ReplicatedStorage": {
                    "Shared": { "$path": "../../outside" }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let (out4, from4, node4) =
        resolve_rojo_shared_module_out(&p4, "AssetIds.luau", "src/shared/AssetIds.luau");
    assert_eq!(out4, "src/shared/AssetIds.luau");
    assert!(!from4);
    assert!(node4.is_none());
}

#[test]
fn meme_arcade_plaza_mesh_preset_exposes_8_props() {
    let specs = map_kit_prop_specs("meme-arcade-plaza").expect("meme-arcade-plaza preset");
    let names: Vec<&str> = specs.iter().map(|(n, _, _)| *n).collect();
    assert_eq!(
        names,
        vec![
            "plaza_floor_tile",
            "meme_pedestal",
            "speed_treadmill",
            "fuse_chamber",
            "dungeon_portal_arch",
            "domain_gate_arch",
            "meme_pack_crate",
            "trophy_crown_statue",
        ]
    );
    for (name, desc, script) in specs {
        assert!(!desc.trim().is_empty(), "{name} missing description");
        assert!(script.contains("bpy.ops.mesh."), "{name} missing mesh ops");
    }
}
