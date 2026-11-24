//! Integration tests for OpenRO v0.11 assets, shared secrets, Open Cloud,
//! reference search, ChatGPT/Gemini/OpenAI art generation, Pillow image processing,
//! card packing, AssetIds.luau codegen, and expanded audio synthesis.
//!
//! Zero live external calls: uses a local loopback mock HTTP server for Open Cloud /
//! Gemini / OpenAI and mock CLI scripts for `chatgpt-camoufox-imagegen` and
//! `reference_search`.

use openro_rs::agent::Agent;
use openro_rs::assets::encode_base64;
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard(Vec<(String, Option<String>)>);
impl EnvGuard {
    fn set(pairs: &[(&str, &str)]) -> Self {
        let mut prev = Vec::new();
        for (k, v) in pairs {
            prev.push(((*k).to_string(), std::env::var(k).ok()));
            std::env::set_var(k, v);
        }
        EnvGuard(prev)
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, old) in self.0.drain(..).rev() {
            match old {
                Some(v) => std::env::set_var(&k, v),
                None => std::env::remove_var(&k),
            }
        }
    }
}

fn scratch_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_TARGET_TMPDIR")));
    let dir = base
        .join("openro-assets-tests")
        .join(format!("{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Valid 2x2 RGBA PNG (white background with red center-ish pixel) for tests.
fn sample_png_bytes() -> Vec<u8> {
    // 1x1 solid red PNG (valid IHDR + IDAT + IEND)
    let b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    openro_rs::assets::decode_base64(b64).unwrap()
}

fn http_req(
    port: u16,
    host: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let header = if body.is_empty() {
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
    };
    stream.write_all(header.as_bytes()).unwrap();
    if !body.is_empty() {
        stream.write_all(body).unwrap();
    }
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let sep = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..sep]).to_string();
    let body_bytes = if sep + 4 <= buf.len() { buf[sep + 4..].to_vec() } else { Vec::new() };
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, head, body_bytes)
}

fn http_json(port: u16, method: &str, path: &str, body: &Value) -> (u16, Value) {
    let raw = if method == "GET" {
        Vec::new()
    } else {
        serde_json::to_vec(body).unwrap()
    };
    let (st, _, bytes) = http_req(port, &format!("127.0.0.1:{port}"), method, path, &raw);
    let val: Value = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "invalid JSON ({st}) from {method} {path}: {e} -> {:?}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (st, val)
}

fn start_server(data_dir: &Path) -> u16 {
    let model = ModelClient::new("http://127.0.0.1:11434/v1".into(), "test".into(), None);
    let place = LocalPlace::in_memory("TestGame".into());
    let agent = Agent::with_local_place(model, place);
    let shared = Shared::with_data_dir(Some(agent), "TestGame".into(), data_dir.to_path_buf());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    port
}

#[derive(Default)]
struct MockState {
    calls: Vec<(String, String, String)>, // (method, path, body_snippet)
    upload_seq: u64,
    op_polls: std::collections::HashMap<String, u32>,
    image_rejected_once: bool,
    dev_products: Vec<Value>,
    game_passes: Vec<Value>,
    universe: Value,
    next_product_id: u64,
}

fn spawn_mock_upstream(png: Vec<u8>) -> (String, Arc<Mutex<MockState>>) {
    let state = Arc::new(Mutex::new(MockState {
        next_product_id: 5000,
        dev_products: vec![json!({
            "productId": 4100,
            "name": "Existing Boost",
            "description": "Old desc",
            "priceInformation": { "defaultPriceInRobux": 99 },
            "isForSale": true
        })],
        game_passes: vec![json!({
            "gamePassId": 4200,
            "name": "VIP Pass",
            "description": "Old VIP",
            "priceInformation": { "defaultPriceInRobux": 399 },
            "isForSale": true
        })],
        universe: json!({
            "path": "universes/123456",
            "displayName": "Old Name",
            "description": "Old Desc",
            "visibility": "PRIVATE",
            "desktopEnabled": true,
            "mobileEnabled": true
        }),
        ..MockState::default()
    }));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let st = Arc::clone(&state);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let st = Arc::clone(&st);
            let png = png.clone();
            std::thread::spawn(move || handle_mock_conn(stream, &st, &png));
        }
    });
    (format!("http://127.0.0.1:{}", addr.port()), state)
}

fn handle_mock_conn(mut stream: TcpStream, state: &Mutex<MockState>, png: &[u8]) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let mut header_end = None;
    let mut content_len = 0usize;
    loop {
        let Ok(n) = stream.read(&mut tmp) else { break };
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if header_end.is_none() {
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = Some(pos + 4);
                let head = String::from_utf8_lossy(&buf[..pos]);
                for l in head.lines() {
                    if let Some((k, v)) = l.split_once(':') {
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
    let Some(he) = header_end else { return };
    let head = String::from_utf8_lossy(&buf[..he]).to_string();
    let body = &buf[he..buf.len().min(he + content_len)];
    let body_str = String::from_utf8_lossy(body).to_string();
    let first = head.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    let api_key_hdr = head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case("x-api-key").then(|| v.trim().to_string())
    });

    let mut st = state.lock().unwrap();
    st.calls.push((method.clone(), path.clone(), body_str.chars().take(600).collect()));

    let (code, resp_json): (&str, Value) = if api_key_hdr.as_deref() == Some("BAD_KEY_LEAK_TEST_999999") {
        ("401 Unauthorized", json!({ "message": "Invalid key BAD_KEY_LEAK_TEST_999999 rejected" }))
    } else if method == "POST" && path == "/assets/v1/assets" {
        if !st.image_rejected_once && body_str.contains("\"assetType\":\"Image\"") && body_str.contains("fallback_decal") {
            st.image_rejected_once = true;
            ("400 Bad Request", json!({ "message": "Image not supported, use Decal" }))
        } else {
            st.upload_seq += 1;
            let op_id = if body_str.contains("bad_mod") {
                format!("op-reject-{}", st.upload_seq)
            } else {
                format!("op-ok-{}", st.upload_seq)
            };
            st.op_polls.insert(op_id.clone(), 0);
            ("200 OK", json!({ "path": format!("operations/{op_id}"), "done": false }))
        }
    } else if method == "GET" && path.starts_with("/assets/v1/operations/") {
        let op_id = path.trim_start_matches("/assets/v1/operations/").to_string();
        let count = st.op_polls.entry(op_id.clone()).or_insert(0);
        *count += 1;
        if *count < 2 {
            ("200 OK", json!({ "path": format!("operations/{op_id}"), "done": false }))
        } else if op_id.starts_with("op-reject-") {
            (
                "200 OK",
                json!({
                    "path": format!("operations/{op_id}"),
                    "done": true,
                    "response": {
                        "assetId": "999999",
                        "moderationResult": { "moderationState": "MODERATION_STATE_REJECTED" }
                    }
                }),
            )
        } else {
            let num: u64 = op_id.rsplit('-').next().and_then(|s| s.parse().ok()).unwrap_or(1);
            (
                "200 OK",
                json!({
                    "path": format!("operations/{op_id}"),
                    "done": true,
                    "response": {
                        "assetId": (700_000 + num).to_string(),
                        "moderationResult": { "moderationState": "MODERATION_STATE_APPROVED" }
                    }
                }),
            )
        }
    } else if method == "POST" && path.starts_with("/universes/v1/") && path.contains("/versions") {
        ("200 OK", json!({ "versionNumber": 42 }))
    } else if method == "GET" && path.contains("/developer-products/creator") {
        ("200 OK", json!({ "developerProducts": st.dev_products.clone() }))
    } else if method == "POST" && path.ends_with("/developer-products") {
        st.next_product_id += 1;
        let id = st.next_product_id;
        ("200 OK", json!({ "productId": id }))
    } else if method == "PATCH" && path.contains("/developer-products/") {
        ("200 OK", json!({ "updated": true }))
    } else if method == "GET" && path.contains("/game-passes/creator") {
        ("200 OK", json!({ "gamePasses": st.game_passes.clone() }))
    } else if method == "POST" && path.ends_with("/game-passes") {
        st.next_product_id += 1;
        let id = st.next_product_id;
        ("200 OK", json!({ "gamePassId": id }))
    } else if method == "PATCH" && path.contains("/game-passes/") {
        ("200 OK", json!({ "updated": true }))
    } else if method == "GET" && path.starts_with("/cloud/v2/universes/") {
        ("200 OK", st.universe.clone())
    } else if method == "PATCH" && path.starts_with("/cloud/v2/universes/") {
        if let Ok(patch) = serde_json::from_slice::<Value>(body) {
            if let (Some(u), Some(p)) = (st.universe.as_object_mut(), patch.as_object()) {
                for (k, v) in p {
                    u.insert(k.clone(), v.clone());
                }
            }
        }
        ("200 OK", st.universe.clone())
    } else if method == "POST" && path.contains(":generateContent") {
        if body_str.contains("TRIGGER_SAFETY_BLOCK") {
            ("200 OK", json!({ "promptFeedback": { "blockReason": "SAFETY" } }))
        } else {
            let has_ref = body_str.contains("inlineData");
            assert!(has_ref || !body_str.contains("EXPECT_REF"), "expected inlineData ref in Gemini request");
            (
                "200 OK",
                json!({
                    "candidates": [{
                        "finishReason": "STOP",
                        "content": {
                            "parts": [{
                                "inlineData": { "mimeType": "image/png", "data": encode_base64(png) }
                            }]
                        }
                    }]
                }),
            )
        }
    } else if method == "POST" && (path.ends_with("/images/edits") || path.ends_with("/images/generations")) {
        ("200 OK", json!({ "data": [{ "b64_json": encode_base64(png) }] }))
    } else if method == "GET" && path.starts_with("/images/search") {
        let html = if path.contains("CAPTCHA_TEST") {
            "<html><body>Please verify you are human</body></html>".to_string()
        } else {
            let img_url = format!("http://127.0.0.1:{}/mock-img/goku-action.png", stream.local_addr().unwrap().port());
            let sheet_url = format!("http://127.0.0.1:{}/mock-img/goku-sheet.png", stream.local_addr().unwrap().port());
            format!(
                r#"<html><body>
                <a class="iusc" m="{{&quot;murl&quot;:&quot;{sheet_url}&quot;,&quot;purl&quot;:&quot;https://wiki.example/goku&quot;,&quot;t&quot;:&quot;Goku official character sheet standing pose&quot;,&quot;w&quot;:600,&quot;h&quot;:1200}}"></a>
                <a class="iusc" m="{{&quot;murl&quot;:&quot;{img_url}&quot;,&quot;purl&quot;:&quot;https://anime.example/ep130&quot;,&quot;t&quot;:&quot;Goku dynamic battle action scene sakuga&quot;,&quot;w&quot;:1280,&quot;h&quot;:720}}"></a>
                </body></html>"#
            )
        };
        drop(st);
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
            html.len()
        );
        let _ = stream.write_all(resp.as_bytes());
        return;
    } else if method == "GET" && path.starts_with("/mock-img/") {
        drop(st);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            png.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(png);
        return;
    } else {
        ("404 Not Found", json!({ "message": format!("unmocked {method} {path}") }))
    };
    drop(st);

    let payload = resp_json.to_string();
    let resp = format!(
        "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let _ = stream.write_all(resp.as_bytes());
}

#[test]
fn v011_secrets_opencloud_art_references_imageops_and_audio() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let root = scratch_dir("v011-assets");
    let data_dir = root.join("data");
    let secrets_dir = root.join("shared-secrets");
    let project_dir = data_dir.join("projects").join("demo-uacg");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(project_dir.join("art/cards/naruto")).unwrap();
    std::fs::create_dir_all(project_dir.join("art/ui")).unwrap();
    std::fs::create_dir_all(project_dir.join("art/frames/preview")).unwrap();
    std::fs::create_dir_all(project_dir.join("art/references")).unwrap();
    std::fs::create_dir_all(project_dir.join("art/extra")).unwrap();
    std::fs::create_dir_all(project_dir.join("assets/meshes")).unwrap();
    std::fs::create_dir_all(project_dir.join("build/builds")).unwrap();

    // Seed legacy config.json with gemini_api_key to verify automatic migration to OPENRO_SECRETS_DIR!
    std::fs::write(
        data_dir.join("config.json"),
        json!({
            "gemini_api_key": "LEGACY_GEMINI_SECRET_8899",
            "openai_image_api_key": "LEGACY_OPENAI_SECRET_7766"
        })
        .to_string(),
    )
    .unwrap();

    let png = sample_png_bytes();
    let mut png_b = png.clone();
    png_b.extend_from_slice(b"extra-comment-b");
    let mut png_c = png.clone();
    png_c.extend_from_slice(b"extra-comment-c");
    let mut png_d = png.clone();
    png_d.extend_from_slice(b"extra-comment-d");

    std::fs::write(project_dir.join("art/cards/naruto/art.png"), &png).unwrap();
    std::fs::write(project_dir.join("art/cards/fallback_decal.png"), &png_b).unwrap();
    std::fs::write(project_dir.join("art/cards/bad_mod.png"), &png_c).unwrap();
    std::fs::write(project_dir.join("art/ui/coin.png"), &png_d).unwrap();
    std::fs::write(project_dir.join("art/frames/preview/sheet.png"), &png).unwrap();
    std::fs::write(project_dir.join("art/references/owner_secret_ref.png"), &png).unwrap();
    std::fs::write(project_dir.join("art/extra/unauth.png"), &png).unwrap();
    std::fs::write(project_dir.join("build/demo.rbxlx"), "<roblox version=\"4\"></roblox>").unwrap();
    std::fs::write(
        project_dir.join("build/builds/b-1.json"),
        json!({ "build_id": "b-1", "path": "build/demo.rbxlx", "at": 1700000001u64 }).to_string(),
    )
    .unwrap();
    std::fs::write(
        project_dir.join("assets/meshes/manifest.json"),
        json!({ "meshes": { "torii_gate": { "bbox_studs": [18.0, 14.0, 4.0], "tris": 1820 } } }).to_string(),
    )
    .unwrap();
    std::fs::write(
        project_dir.join("openro.project.json"),
        json!({
            "version": 1,
            "name": "Demo UACG",
            "universeId": 123456,
            "placeId": 654321,
            "customKeepField": "do-not-drop",
            "products": [
                { "key": "unapproved_pack", "kind": "developer_product", "name": "Unapproved Pack", "description": "No", "priceRobux": 999, "forSale": true, "priceApproved": false, "id": null },
                { "key": "boost_existing", "kind": "developer_product", "name": "Existing Boost", "description": "Updated desc", "priceRobux": 149, "forSale": true, "priceApproved": true, "id": null },
                { "key": "new_potion", "kind": "developer_product", "name": "Luck Potion", "description": "+2x Luck", "priceRobux": 49, "forSale": true, "priceApproved": true, "icon": "art/ui/coin.png", "id": null },
                { "key": "vip", "kind": "game_pass", "name": "VIP Pass", "description": "Updated VIP", "priceRobux": 399, "forSale": true, "priceApproved": true, "id": 4200 }
            ],
            "assets": { "lockFile": "assets.lock.json", "authorised": [] }
        })
        .to_string(),
    )
    .unwrap();

    let (mock_base, mock_state) = spawn_mock_upstream(png.clone());

    // Write mock reference_search CLI & mock chatgpt-camoufox-imagegen CLI scripts
    let mock_ref_bin = root.join("mock_ref_search.py");
    let mock_cand_1 = root.join("cand_action_1.png");
    let mock_cand_2 = root.join("cand_action_2.png");
    let mock_cand_sheet = root.join("cand_sheet.png");
    std::fs::write(&mock_cand_1, &png).unwrap();
    std::fs::write(&mock_cand_2, &png_b).unwrap();
    std::fs::write(&mock_cand_sheet, &png_c).unwrap();
    std::fs::write(
        &mock_ref_bin,
        format!(
            r#"#!/usr/bin/env python3
import json, sys
print(json.dumps({{
  "ok": True,
  "candidates": [
    {{"image_url": "https://wiki.example/sheet.png", "source_url": "https://wiki.example/1", "title": "Naruto official character sheet standing pose", "width": 600, "height": 1200, "local_file": {sheet:?}}},
    {{"image_url": "https://anime.example/rasengan.png", "source_url": "https://anime.example/ep476", "title": "Naruto dynamic battle action scene sakuga frame", "width": 1280, "height": 720, "local_file": {c1:?}}},
    {{"image_url": "https://anime.example/clash.png", "source_url": "https://anime.example/ep477", "title": "Naruto fight attack scene aura", "width": 1024, "height": 768, "local_file": {c2:?}}}
  ]
}}))
"#,
            sheet = mock_cand_sheet.to_string_lossy(),
            c1 = mock_cand_1.to_string_lossy(),
            c2 = mock_cand_2.to_string_lossy(),
        ),
    )
    .unwrap();

    let mock_chatgpt_bin = root.join("mock_chatgpt_imagegen.py");
    std::fs::write(
        &mock_chatgpt_bin,
        format!(
            r#"#!/usr/bin/env python3
import json, sys, shutil
from pathlib import Path

args = sys.argv[1:]
prompt = args[0]
out = args[args.index("-o") + 1]
char_art = "--character-art" in args
refs = [args[i+1] for i, a in enumerate(args) if a == "--ref-image"]
assert "\n" not in prompt and "\r" not in prompt, f"multiline prompt: {{prompt!r}}"

if "MOCK_EXIT_2" in prompt:
    print(json.dumps({{"ok": False, "error_kind": "LOGIN_REQUIRED", "error": "logged out"}}))
    sys.exit(2)
if "MOCK_EXIT_4" in prompt:
    print(json.dumps({{"ok": False, "error_kind": "UI_DRIFT", "error": "composer missing"}}))
    sys.exit(4)
if "MOCK_EXIT_6" in prompt:
    print(json.dumps({{"ok": False, "error_kind": "RATE_LIMITED", "error": "too many requests"}}))
    sys.exit(6)
if "MOCK_EXIT_7" in prompt:
    print(json.dumps({{"ok": False, "error_kind": "REFUSED", "error": "safety policy refusal"}}))
    sys.exit(7)
if "MOCK_EXIT_8" in prompt:
    print(json.dumps({{"ok": False, "error_kind": "DRIVER", "error": "lease busy"}}))
    sys.exit(8)

if char_art:
    assert "eye-level" in prompt.lower() or "neutral three-quarter" in prompt.lower(), f"missing neutral camera in {{prompt!r}}"
    cand = out + ".pending-angle.png"
    shutil.copyfile({src:?}, cand)
    print(json.dumps({{
        "ok": False,
        "exit_code": 9,
        "error_kind": "ANGLE_REVIEW_REQUIRED",
        "model_ui": "GPT-6 Instant",
        "angle_review": {{"status": "PENDING_VISION", "candidate_path": cand, "width": 1, "height": 1}}
    }}))
    sys.exit(9)

shutil.copyfile({src:?}, out)
print(json.dumps({{"ok": True, "out": out, "width": 1, "height": 1, "model_ui": "GPT-6 Instant", "refs": len(refs)}}))
sys.exit(0)
"#,
            src = mock_cand_1.to_string_lossy(),
        ),
    )
    .unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mock_ref_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&mock_chatgpt_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _env = EnvGuard::set(&[
        ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
        ("OPENRO_OPENCLOUD_BASE", &mock_base),
        ("OPENRO_OPENCLOUD_POLL_MS", "5"),
        ("OPENRO_GEMINI_IMAGE_BASE", &mock_base),
        ("OPENRO_OPENAI_IMAGE_BASE", &mock_base),
        ("OPENRO_REFERENCE_SEARCH_BIN", mock_ref_bin.to_str().unwrap()),
        ("OPENRO_CHATGPT_IMAGEGEN_BIN", mock_chatgpt_bin.to_str().unwrap()),
        ("OPENRO_SHARED_BROWSER_PY", ""),
    ]);

    let port = start_server(&data_dir);

    // ---- 1. Secrets migration, permissions & masking ----
    let (st, cfg) = http_json(port, "GET", "/api/config", &json!({}));
    assert_eq!(st, 200);
    assert_eq!(cfg["has_gemini_api_key"], true);
    assert_eq!(cfg["gemini_api_key_masked"], "••••8899");
    assert_eq!(cfg["has_openai_image_api_key"], true);
    assert_eq!(cfg["openai_image_api_key_masked"], "••••7766");
    assert_eq!(cfg["image_provider"], "chatgpt");
    assert_eq!(cfg["secrets_dir_shared"], true);
    assert!(!cfg.to_string().contains("LEGACY_GEMINI_SECRET_8899"));
    // Legacy config.json should have secrets cleared out after migration
    let cfg_disk = std::fs::read_to_string(data_dir.join("config.json")).unwrap();
    assert!(!cfg_disk.contains("LEGACY_GEMINI_SECRET_8899"));
    assert!(!cfg_disk.contains("LEGACY_OPENAI_SECRET_7766"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let d_mode = std::fs::metadata(&secrets_dir).unwrap().permissions().mode() & 0o777;
        let f_mode = std::fs::metadata(secrets_dir.join("gemini_api_key")).unwrap().permissions().mode() & 0o777;
        assert_eq!(d_mode, 0o700);
        assert_eq!(f_mode, 0o600);
    }

    // Foreign Host -> 403
    let (st_evil, _, _) = http_req(port, "evil.example.com", "GET", "/api/opencloud", b"");
    assert_eq!(st_evil, 403);

    // Without Open Cloud key -> 503
    let (st_nokey, nokey) = http_json(port, "GET", "/api/projects/demo-uacg/universe", &json!({}));
    assert_eq!(st_nokey, 503);
    assert!(nokey["error"].as_str().unwrap().contains("Settings"));

    // Scrubbed error when upstream echoes bad key
    let (st_badk, _) = http_json(port, "POST", "/api/opencloud/key", &json!({ "api_key": "BAD_KEY_LEAK_TEST_999999" }));
    assert_eq!(st_badk, 200);
    let (st_401, e401) = http_json(port, "GET", "/api/projects/demo-uacg/universe", &json!({}));
    assert_eq!(st_401, 502);
    assert!(!e401.to_string().contains("BAD_KEY_LEAK_TEST_999999"), "secret leaked in upstream error: {e401}");

    // Set real mock key + creator owner
    let (st_k, oc) = http_json(port, "POST", "/api/opencloud/key", &json!({ "api_key": "valid_opencloud_key_1234" }));
    assert_eq!(st_k, 200);
    assert_eq!(oc["has_key"], true);
    assert_eq!(oc["key_masked"], "••••1234");
    assert!(!oc.to_string().contains("valid_opencloud_key_1234"));
    let (st_c, oc2) = http_json(port, "POST", "/api/opencloud/settings", &json!({ "creator": { "userId": 998877 } }));
    assert_eq!(st_c, 200);
    assert_eq!(oc2["creator"]["userId"], "998877");

    // ---- 2. Open Cloud Assets: list, authorise, upload, fallback, rejection, everything overrun, codegen ----
    let (st_list, list) = http_json(port, "GET", "/api/projects/demo-uacg/assets", &json!({}));
    assert_eq!(st_list, 200);
    let paths: Vec<&str> = list["files"].as_array().unwrap().iter().filter_map(|f| f["path"].as_str()).collect();
    assert!(paths.contains(&"art/cards/naruto/art.png"));
    assert!(paths.contains(&"art/extra/unauth.png"));
    assert!(!paths.iter().any(|p| p.contains("references")), "references leaked into asset walk");
    assert!(!paths.iter().any(|p| p.contains("frames/preview")), "frame previews leaked into asset walk");

    // Empty authorised set uploads 0
    let (st_empty, up_empty) = http_json(port, "POST", "/api/projects/demo-uacg/assets/upload", &json!({}));
    assert_eq!(st_empty, 200);
    assert_eq!(up_empty["summary"]["uploaded"], 0);
    assert!(up_empty["summary"]["note"].as_str().unwrap().contains("Authorise"));

    // Authorise cards + ui
    let (st_auth, auth) = http_json(
        port,
        "POST",
        "/api/projects/demo-uacg/assets/authorise",
        &json!({ "paths": ["art/cards/**", "art/ui/*.png", "!art/frames/preview/**"] }),
    );
    assert_eq!(st_auth, 200);
    assert_eq!(auth["authorised"].as_array().unwrap().len(), 3);

    // Upload authorised set: naruto/art.png (ok), fallback_decal.png (Image->Decal fallback!), bad_mod.png (rejected!), coin.png (ok)
    let (st_up, up) = http_json(port, "POST", "/api/projects/demo-uacg/assets/upload", &json!({}));
    assert_eq!(st_up, 200);
    assert_eq!(up["summary"]["uploaded"], 3, "upload result: {up}");
    assert_eq!(up["summary"]["rejected"], 1, "expected 1 rejected moderation: {up}");
    let results = up["results"].as_array().unwrap();
    let decal_row = results.iter().find(|r| r["path"] == "art/cards/fallback_decal.png").unwrap();
    assert_eq!(decal_row["assetType"], "Decal");
    let rej_row = results.iter().find(|r| r["path"] == "art/cards/bad_mod.png").unwrap();
    assert_eq!(rej_row["state"], "rejected");
    assert!(rej_row["assetId"].is_null());

    // Lock file must NOT contain bad_mod.png!
    let (st_lock, lock) = http_json(port, "GET", "/api/projects/demo-uacg/assets/lock", &json!({}));
    assert_eq!(st_lock, 200);
    assert!(lock["assets"].get("art/cards/naruto/art.png").is_some());
    assert!(lock["assets"].get("art/cards/bad_mod.png").is_none(), "rejected asset must never be locked");

    // Auto-generated src/shared/AssetIds.luau exists and contains meshes + approved keys, NOT bad_mod
    let luau = std::fs::read_to_string(project_dir.join("src/shared/AssetIds.luau")).unwrap();
    assert!(luau.contains("naruto_art"), "AssetIds.luau missing naruto_art: {luau}");
    assert!(luau.contains("coin"), "AssetIds.luau missing coin: {luau}");
    assert!(luau.contains("torii_gate"), "AssetIds.luau missing mesh bbox entry: {luau}");
    assert!(!luau.contains("bad_mod"), "AssetIds.luau must not include rejected asset");

    // Second upload is unchanged; everything + paths is 400; everything prints & returns overrun!
    let (_, up2) = http_json(port, "POST", "/api/projects/demo-uacg/assets/upload", &json!({ "paths": ["art/cards/naruto/art.png"] }));
    assert_eq!(up2["summary"]["unchanged"], 1);
    let (st_both, _) = http_json(port, "POST", "/api/projects/demo-uacg/assets/upload", &json!({ "everything": true, "paths": ["art/ui/coin.png"] }));
    assert_eq!(st_both, 400);
    let (st_ev, ev) = http_json(port, "POST", "/api/projects/demo-uacg/assets/upload", &json!({ "everything": true }));
    assert_eq!(st_ev, 200);
    assert_eq!(ev["summary"]["overrun"], json!(["art/extra/unauth.png"]));
    assert_eq!(ev["summary"]["reused"], 1, "unauth.png shares sha256 + Image type with art/cards/naruto/art.png: {ev}");

    // ---- 3. Publish, Products (priceApproved gate!), Universe ----
    let (st_pub_no, _) = http_json(port, "POST", "/api/projects/demo-uacg/publish", &json!({ "versionType": "Published" }));
    assert_eq!(st_pub_no, 400);
    let (st_pub_ok, pub_res) = http_json(port, "POST", "/api/projects/demo-uacg/publish", &json!({ "versionType": "Saved" }));
    assert_eq!(st_pub_ok, 200);
    assert_eq!(pub_res["versionNumber"], 42);
    let (st_pub_live, _) = http_json(port, "POST", "/api/projects/demo-uacg/publish", &json!({ "versionType": "Published", "confirm": true }));
    assert_eq!(st_pub_live, 200);

    let (st_prod, prod) = http_json(port, "POST", "/api/projects/demo-uacg/products/sync", &json!({}));
    assert_eq!(st_prod, 200);
    let pr = prod["results"].as_array().unwrap();
    let find_p = |k: &str| pr.iter().find(|x| x["key"] == k).unwrap().clone();
    assert_eq!(find_p("unapproved_pack")["action"], "skipped");
    assert_eq!(find_p("unapproved_pack")["error"], "price not approved");
    assert_eq!(find_p("boost_existing")["action"], "updated"); // adopted by name & updated!
    assert_eq!(find_p("boost_existing")["id"], "4100");
    assert_eq!(find_p("new_potion")["action"], "created");
    assert_eq!(find_p("vip")["action"], "updated");
    // Unknown manifest field preserved + new product id written back
    let manifest_after: Value = serde_json::from_str(&std::fs::read_to_string(project_dir.join("openro.project.json")).unwrap()).unwrap();
    assert_eq!(manifest_after["customKeepField"], "do-not-drop");
    assert!(manifest_after["products"][2]["id"].as_u64().is_some());

    let (st_u_bad, _) = http_json(port, "POST", "/api/projects/demo-uacg/universe", &json!({ "settings": { "icon": "a.png" } }));
    assert_eq!(st_u_bad, 400);
    let (st_u_ok, u_ok) = http_json(
        port,
        "POST",
        "/api/projects/demo-uacg/universe",
        &json!({ "settings": { "displayName": "Untitled Anime Card Game", "consoleEnabled": false } }),
    );
    assert_eq!(st_u_ok, 200);
    assert_eq!(u_ok["universe"]["displayName"], "Untitled Anime Card Game");
    assert!(u_ok["manual_steps"].as_array().unwrap().len() >= 3);

    // ---- 4. Reference search + candidate override ----
    let (st_rs, rs) = http_json(
        port,
        "POST",
        "/api/art/references/search",
        &json!({ "project": "demo-uacg", "character": "Naruto Uzumaki", "show": "Naruto Shippuden", "card_id": "naruto_sage" }),
    );
    assert_eq!(st_rs, 200, "reference search failed: {rs}");
    // Candidate 0 must be the dynamic battle action scene, NOT the official character sheet!
    assert!(rs["candidates"][0]["image_url"].as_str().unwrap().contains("rasengan"));
    assert!(rs["candidates"][2]["rejected_reason"].is_string());
    assert!(project_dir.join(rs["reference_path"].as_str().unwrap()).is_file());

    let (st_sel, sel) = http_json(
        port,
        "POST",
        "/api/art/references/select",
        &json!({ "project": "demo-uacg", "slug": "naruto_sage", "candidate_index": 1 }),
    );
    assert_eq!(st_sel, 200);
    assert_eq!(sel["chosen_index"], 1);

    // ---- 5. Reference-image art generation: ChatGPT default (exit 9, approve, exit 0, exit 2/4/6/7/8) + Gemini + OpenAI ----
    // Prohibits Higgsfield/Soul Cinema
    let (st_hf, _) = http_json(port, "POST", "/api/art/generate", &json!({ "project": "demo-uacg", "provider": "higgsfield-soul-cinema", "prompt": "test" }));
    assert_eq!(st_hf, 400);
    // Rejects low-angle phrase before calling CLI
    let (st_la, _) = http_json(port, "POST", "/api/art/generate", &json!({ "project": "demo-uacg", "prompt": "Naruto worm's-eye low-angle shot", "character_art": true }));
    assert_eq!(st_la, 400);

    // Character art with reference image + multiline prompt -> collapses to 1 line, appends eye-level, returns pending_review (exit 9)!
    let (st_g9, g9) = http_json(
        port,
        "POST",
        "/api/art/generate",
        &json!({
            "project": "demo-uacg",
            "card_id": "naruto_sage",
            "prompt": "Anime card illustration charging rasengan\n\nvibrant golden energy",
            "reference_images": [sel["reference_path"]],
            "out_path": "art/cards/naruto_sage_raw.png"
        }),
    );
    assert_eq!(st_g9, 200, "gen exit 9 failed: {g9}");
    assert_eq!(g9["status"], "pending_review");
    assert_eq!(g9["pending_review"], true);
    assert!(!project_dir.join("art/cards/naruto_sage_raw.png").exists(), "pending candidate must NOT write final path");
    let cand_path = g9["candidate_path"].as_str().unwrap().to_string();
    let cand_sha = g9["candidate_sha256"].as_str().unwrap().to_string();
    assert!(project_dir.join(&cand_path).is_file());

    // Approve with wrong sha -> 409; approve with matching sha -> promotes cleanly!
    let (st_bad_sha, _) = http_json(
        port,
        "POST",
        "/api/art/approve",
        &json!({ "project": "demo-uacg", "candidate_path": cand_path, "expected_sha256": "0000", "reviewer": "owner" }),
    );
    assert_eq!(st_bad_sha, 409);
    let (st_app, app) = http_json(
        port,
        "POST",
        "/api/art/approve",
        &json!({ "project": "demo-uacg", "candidate_path": cand_path, "expected_sha256": cand_sha, "reviewer": "owner", "notes": "eye-level ok" }),
    );
    assert_eq!(st_app, 200, "approve failed: {app}");
    assert_eq!(app["status"], "approved");
    assert!(project_dir.join("art/cards/naruto_sage_raw.png").is_file());

    // Non-character icon via ChatGPT (exit 0)
    let (st_icon, icon_res) = http_json(
        port,
        "POST",
        "/api/art/generate",
        &json!({ "project": "demo-uacg", "kind": "icon", "character_art": false, "name": "gold_coin", "prompt": "glossy gold coin icon" }),
    );
    assert_eq!(st_icon, 200);
    assert_eq!(icon_res["status"], "generated");

    // ChatGPT exit code mappings: 7 (refused), 2 (login), 4 (ui drift), 6 (rate limit), 8 (driver)
    let (st_e7, e7) = http_json(port, "POST", "/api/art/generate", &json!({ "project": "demo-uacg", "character_art": false, "prompt": "MOCK_EXIT_7" }));
    assert_eq!(st_e7, 200);
    assert_eq!(e7["refused"], true);
    assert_eq!(http_json(port, "POST", "/api/art/generate", &json!({ "project": "demo-uacg", "character_art": false, "prompt": "MOCK_EXIT_2" })).0, 503);
    assert_eq!(http_json(port, "POST", "/api/art/generate", &json!({ "project": "demo-uacg", "character_art": false, "prompt": "MOCK_EXIT_4" })).0, 502);
    assert_eq!(http_json(port, "POST", "/api/art/generate", &json!({ "project": "demo-uacg", "character_art": false, "prompt": "MOCK_EXIT_6" })).0, 429);
    assert_eq!(http_json(port, "POST", "/api/art/generate", &json!({ "project": "demo-uacg", "character_art": false, "prompt": "MOCK_EXIT_8" })).0, 503);

    // Gemini with 1-N reference images + safety block handling
    let (st_gem, gem) = http_json(
        port,
        "POST",
        "/api/art/generate",
        &json!({
            "project": "demo-uacg",
            "provider": "gemini",
            "prompt": "EXPECT_REF vibrant anime action pose",
            "reference_images": [sel["reference_path"]],
            "out_path": "art/cards/naruto_gemini.png"
        }),
    );
    assert_eq!(st_gem, 200, "gemini failed: {gem}");
    assert_eq!(gem["provider"], "gemini");
    let (_, gem_ref) = http_json(
        port,
        "POST",
        "/api/art/generate",
        &json!({ "project": "demo-uacg", "provider": "gemini", "prompt": "TRIGGER_SAFETY_BLOCK" }),
    );
    assert_eq!(gem_ref["refused"], true);

    // OpenAI alternative with reference image (/images/edits multipart)
    let (st_oai, oai) = http_json(
        port,
        "POST",
        "/api/art/generate",
        &json!({
            "project": "demo-uacg",
            "provider": "openai",
            "prompt": "stylized anime card pose",
            "reference_images": [sel["reference_path"]],
            "out_path": "art/cards/naruto_openai.png"
        }),
    );
    assert_eq!(st_oai, 200, "openai failed: {oai}");
    assert_eq!(oai["provider"], "openai");

    // ---- 6. Pillow image_ops: transparent 9-slice frame + remove_bg + pack-card + script denylist ----
    let (st_frame, frame) = http_json(
        port,
        "POST",
        "/api/art/process",
        &json!({
            "project": "demo-uacg",
            "output": "art/ui/panel_dark.png",
            "slice": [32, 32, 96, 96],
            "ops": [
                { "op": "canvas", "width": 128, "height": 128, "color": "#00000000" },
                { "op": "draw", "shapes": [
                    { "kind": "ring", "box": [4, 4, 124, 124], "radius": 18, "thickness": 14, "stroke": "#FFD700FF" }
                ]}
            ]
        }),
    );
    assert_eq!(st_frame, 200, "frame draw failed: {frame}");
    assert_eq!(frame["has_alpha"], true);
    let ratio = frame["opaque_ratio"].as_f64().unwrap();
    assert!(ratio > 0.1 && ratio < 0.9, "ring should have transparent center: {ratio}");
    assert!(project_dir.join("art/ui/slices.json").is_file());

    // Create opaque white PNG with center colored square and test corner_flood remove_bg
    let (st_bg, bg) = http_json(
        port,
        "POST",
        "/api/art/process",
        &json!({
            "project": "demo-uacg",
            "output": "art/ui/icon_cutout.png",
            "ops": [
                { "op": "canvas", "width": 64, "height": 64, "color": "#FFFFFFFF" },
                { "op": "draw", "shapes": [{ "kind": "ellipse", "box": [16, 16, 48, 48], "fill": "#FF3344FF" }] },
                { "op": "remove_bg", "method": "corner_flood", "tolerance": 20, "feather": 1 }
            ]
        }),
    );
    assert_eq!(st_bg, 200, "remove_bg failed: {bg}");
    assert_eq!(bg["has_alpha"], true);
    assert!(bg["opaque_ratio"].as_f64().unwrap() < 0.5);

    // Denylisted Pillow script refused
    let (st_deny, _) = http_json(
        port,
        "POST",
        "/api/art/process",
        &json!({ "project": "demo-uacg", "script": "import subprocess\nsubprocess.run(['id'])" }),
    );
    assert_eq!(st_deny, 400);

    // #37 regression: size:[w,h] alias + flat single-shape draw works, and 0-shape draw / missing canvas size / unknown op -> 400
    let (st_alias, alias_res) = http_json(
        port,
        "POST",
        "/api/art/process",
        &json!({
            "project": "demo-uacg",
            "output": "art/ui/card_bg_alias.png",
            "ops": [
                { "op": "canvas", "size": [256, 356], "color": "#00000000" },
                { "op": "draw", "shape": "rounded_rect", "box": [8, 8, 248, 348], "radius": 18, "fill": "#1B2440FF" }
            ]
        }),
    );
    assert_eq!(st_alias, 200, "size + flat draw alias failed: {alias_res}");
    assert_eq!(alias_res["width"], 256);
    assert_eq!(alias_res["height"], 356);
    assert!(alias_res["opaque_ratio"].as_f64().unwrap() > 0.7, "flat draw must not produce blank image: {alias_res}");

    let (st_empty_draw, empty_draw) = http_json(
        port,
        "POST",
        "/api/art/process",
        &json!({
            "project": "demo-uacg",
            "output": "art/ui/should_fail.png",
            "ops": [
                { "op": "canvas", "size": [128, 128] },
                { "op": "draw", "shapes": [] }
            ]
        }),
    );
    assert_eq!(st_empty_draw, 400, "empty draw must 400: {empty_draw}");

    let (st_bad_canvas, _) = http_json(
        port,
        "POST",
        "/api/art/process",
        &json!({
            "project": "demo-uacg",
            "output": "art/ui/should_fail2.png",
            "ops": [{ "op": "canvas" }]
        }),
    );
    assert_eq!(st_bad_canvas, 400);

    let (st_bad_shape, _) = http_json(
        port,
        "POST",
        "/api/art/process",
        &json!({
            "project": "demo-uacg",
            "output": "art/ui/should_fail3.png",
            "ops": [
                { "op": "canvas", "width": 64, "height": 64 },
                { "op": "draw", "shape": "bogus_hex", "fill": "#FFFFFFFF" }
            ]
        }),
    );
    assert_eq!(st_bad_shape, 400);

    // #38 regression: reference_search falls back cleanly to direct HTTP search when shared browser is absent,
    // and stops cleanly (502) if the HTTP search page shows a CAPTCHA challenge!
    {
        let _fb_env = EnvGuard::set(&[
            ("OPENRO_REFERENCE_SEARCH_BIN", ""),
            ("OPENRO_SHARED_BROWSER_PY", "/nonexistent/openro-test-no-shared-browser"),
            ("OPENRO_REFERENCE_SEARCH_HTTP_BASE", &mock_base),
        ]);
        let (st_fb, fb) = http_json(
            port,
            "POST",
            "/api/art/references/search",
            &json!({ "project": "demo-uacg", "character": "Son Goku", "show": "Dragon Ball Super", "card_id": "goku_ui" }),
        );
        assert_eq!(st_fb, 200, "http fallback search failed: {fb}");
        assert_eq!(fb["search_backend"], "http_fallback");
        assert!(fb["candidates"][0]["image_url"].as_str().unwrap().contains("goku-action"));
        assert!(project_dir.join(fb["reference_path"].as_str().unwrap()).is_file());

        let (st_cap, cap) = http_json(
            port,
            "POST",
            "/api/art/references/search",
            &json!({ "project": "demo-uacg", "character": "CAPTCHA_TEST", "show": "Anime" }),
        );
        assert_eq!(st_cap, 502, "CAPTCHA page must fail 502 without bypassing: {cap}");
    }

    // One-shot pack-card (art.png 512x712 + icon.png 256x256 + full.png + contact sheet)
    let (st_pack, pack) = http_json(
        port,
        "POST",
        "/api/art/pack-card",
        &json!({
            "project": "demo-uacg",
            "card_id": "naruto_sage",
            "source_image": "art/cards/naruto_sage_raw.png",
            "reference_image": sel["reference_path"],
            "frame_image": "art/ui/panel_dark.png"
        }),
    );
    assert_eq!(st_pack, 200, "pack-card failed: {pack}");
    assert_eq!(pack["art"]["info"]["width"], 512);
    assert_eq!(pack["art"]["info"]["height"], 712);
    assert_eq!(pack["icon"]["info"]["width"], 256);
    assert_eq!(pack["icon"]["info"]["height"], 256);
    assert!(project_dir.join("art/previews/naruto_sage_contact.png").is_file());

    // ---- 7. Audio synth: death_shatter preset + custom 8s recipe -> project assets/audio/ ----
    let (st_aud1, aud1) = http_json(
        port,
        "POST",
        "/api/tools/audio/generate",
        &json!({ "project": "demo-uacg", "preset": "death_shatter", "name": "DeathShatter" }),
    );
    assert_eq!(st_aud1, 200, "audio preset failed: {aud1}");
    assert_eq!(aud1["project_path"], "assets/audio/deathshatter.wav");
    assert!(project_dir.join("assets/audio/deathshatter.wav").is_file());

    let (st_aud2, aud2) = http_json(
        port,
        "POST",
        "/api/tools/audio/generate",
        &json!({
            "project": "demo-uacg",
            "name": "NexusAmbience",
            "duration_ms": 8000,
            "layers": [
                { "wave": "sine", "start_ms": 0, "duration_ms": 8000, "freq_start": 220, "freq_end": 225, "gain": 0.4, "attack_ms": 400, "release_ms": 600 },
                { "wave": "triangle", "start_ms": 500, "duration_ms": 3000, "freq_start": 440, "freq_end": 880, "gain": 0.3 }
            ]
        }),
    );
    assert_eq!(st_aud2, 200, "audio recipe failed: {aud2}");
    assert_eq!(aud2["duration_ms"], 8000);
    let wav_bytes = std::fs::read(project_dir.join("assets/audio/nexusambience.wav")).unwrap();
    assert!(wav_bytes.starts_with(b"RIFF") && &wav_bytes[8..12] == b"WAVE");

    // Verify OpenAI multipart /images/edits was hit
    let calls = mock_state.lock().unwrap().calls.clone();
    assert!(calls.iter().any(|(m, p, _)| m == "POST" && p.ends_with("/images/edits")));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn gap65_chatgpt_cli_submit_method_and_ref_image_aliases() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("gap65");
    let data_dir = root.join("data");
    let project_dir = root.join("proj");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(project_dir.join("art/references")).unwrap();
    std::fs::create_dir_all(project_dir.join("art/cards")).unwrap();
    let ref1 = project_dir.join("art/references/naruto_uzumaki.png");
    let ref2 = project_dir.join("art/references/sasuke_uchiha.png");
    std::fs::write(&ref1, sample_png_bytes()).unwrap();
    std::fs::write(&ref2, sample_png_bytes()).unwrap();

    let argv_dump = root.join("last_argv.json");
    let mock_cli = root.join("mock_chatgpt_g65.py");
    std::fs::write(
        &mock_cli,
        format!(
            r#"#!/usr/bin/env python3
import json, shutil, sys
args = sys.argv[1:]
open({dump:?}, "w").write(json.dumps(args))
out = args[args.index("-o") + 1]
shutil.copyfile({src:?}, out)
print(json.dumps({{"ok": True, "out": out, "width": 1, "height": 1, "model_ui": "GPT-6.2 Instant"}}))
"#,
            dump = argv_dump.to_string_lossy(),
            src = ref1.to_string_lossy(),
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mock_cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _env = EnvGuard::set(&[
        ("OPENRO_CHATGPT_IMAGEGEN_BIN", mock_cli.to_str().unwrap()),
        ("OPENRO_SHARED_BROWSER_PY", ""),
        ("OPENRO_CHATGPT_SUBMIT_METHOD", ""),
    ]);

    let store = openro_rs::secrets::SecretStore::for_data_dir(&data_dir);

    // 1. Default `--submit-method keyboard-button` + `ref_image` alias passes `--mode edit --ref-image <path>`!
    let res = openro_rs::assets::art_generate(
        &store,
        &project_dir,
        &json!({
            "prompt": "eye-level neutral three-quarter anime ninja",
            "out_path": "art/cards/naruto_g65.png",
            "character_art": false,
            "ref_image": "art/references/naruto_uzumaki.png"
        }),
    )
    .expect("ref_image alias should succeed");
    assert_eq!(res["ok"], true);
    let args: Vec<String> = serde_json::from_str(&std::fs::read_to_string(&argv_dump).unwrap()).unwrap();
    let sm_pos = args.iter().position(|a| a == "--submit-method").expect("missing --submit-method");
    assert_eq!(args[sm_pos + 1], "keyboard-button");
    let mode_pos = args.iter().position(|a| a == "--mode").expect("missing --mode");
    assert_eq!(args[mode_pos + 1], "edit", "ref_image must trigger --mode edit, not generate");
    let passed_refs: Vec<&String> = args
        .windows(2)
        .filter(|w| w[0] == "--ref-image")
        .map(|w| &w[1])
        .collect();
    assert_eq!(passed_refs.len(), 1);
    assert!(passed_refs[0].ends_with("naruto_uzumaki.png"));

    // 2. `ref_images` alias + deduplication + `body.submit_method` override.
    let res2 = openro_rs::assets::art_generate(
        &store,
        &project_dir,
        &json!({
            "prompt": "eye-level neutral three-quarter duo",
            "out_path": "art/cards/duo_g65.png",
            "character_art": false,
            "submit_method": "button",
            "ref_images": ["art/references/naruto_uzumaki.png", "art/references/sasuke_uchiha.png"],
            "ref_image": "art/references/naruto_uzumaki.png"
        }),
    )
    .expect("ref_images + dedup + submit_method override should succeed");
    assert_eq!(res2["ok"], true);
    let args2: Vec<String> = serde_json::from_str(&std::fs::read_to_string(&argv_dump).unwrap()).unwrap();
    let sm_pos2 = args2.iter().position(|a| a == "--submit-method").unwrap();
    assert_eq!(args2[sm_pos2 + 1], "button");
    let passed_refs2: Vec<&String> = args2
        .windows(2)
        .filter(|w| w[0] == "--ref-image")
        .map(|w| &w[1])
        .collect();
    assert_eq!(passed_refs2.len(), 2, "duplicate ref_image should be deduplicated");

    // 3. `OPENRO_CHATGPT_SUBMIT_METHOD` env override applies when `body.submit_method` is omitted.
    {
        let _sm_env = EnvGuard::set(&[("OPENRO_CHATGPT_SUBMIT_METHOD", "enter")]);
        let res3 = openro_rs::assets::art_generate(
            &store,
            &project_dir,
            &json!({
                "prompt": "clean anime emblem",
                "out_path": "art/cards/env_g65.png",
                "character_art": false
            }),
        )
        .expect("env submit_method override should succeed");
        assert_eq!(res3["ok"], true);
        let args3: Vec<String> = serde_json::from_str(&std::fs::read_to_string(&argv_dump).unwrap()).unwrap();
        let sm_pos3 = args3.iter().position(|a| a == "--submit-method").unwrap();
        assert_eq!(args3[sm_pos3 + 1], "enter");
    }

    // 4. Invalid / flag-injection `submit_method` rejected with 400 (both body and env).
    let bad_body = openro_rs::assets::art_generate(
        &store,
        &project_dir,
        &json!({
            "prompt": "test",
            "out_path": "art/cards/bad_g65.png",
            "character_art": false,
            "submit_method": "--evil-flag;rm"
        }),
    )
    .unwrap_err();
    assert_eq!(bad_body.status, 400, "{bad_body:?}");
    assert!(bad_body.msg.contains("Invalid submit_method"));

    {
        let _bad_env = EnvGuard::set(&[("OPENRO_CHATGPT_SUBMIT_METHOD", "bogus-method")]);
        let bad_env_res = openro_rs::assets::art_generate(
            &store,
            &project_dir,
            &json!({
                "prompt": "test",
                "out_path": "art/cards/bad_env_g65.png",
                "character_art": false
            }),
        )
        .unwrap_err();
        assert_eq!(bad_env_res.status, 400, "{bad_env_res:?}");
    }

    let _ = std::fs::remove_dir_all(&root);
}
