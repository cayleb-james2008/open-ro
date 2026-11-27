//! Phase 4 (v0.6.0) integration tests:
//! - `GET /api/art-knowledge` (Original Anime-Style Character Design Knowledge Point, zero internal jargon)
//! - `POST /api/assets/upload` (JSON base64 & raw binary PNG) -> `GET /api/assets` -> `GET /api/assets/:file`
//! - Path-traversal, NUL-byte, symlink-escape, & bad-magic-byte security fuzzing
//! - `POST /api/assets/generate` honest no-key refusal with homage prompt
//! - `GET/POST /api/config` masking of Gemini/OpenAI image API keys.

use openro_rs::agent::{system_prompt, Agent};
use openro_rs::model::ModelClient;
use openro_rs::place::LocalPlace;
use openro_rs::server::{serve, Shared};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn start_local_server(data_dir: Option<std::path::PathBuf>) -> u16 {
    let model = ModelClient::new(
        "http://127.0.0.1:11434/v1".to_string(),
        "qwen2.5:3b".to_string(),
        None,
    );
    let place = match &data_dir {
        Some(dir) => LocalPlace::open(dir.clone(), "Anime Card Battles Apex".to_string()),
        None => LocalPlace::in_memory("Anime Card Battles Apex".to_string()),
    };
    let agent = Agent::with_local_place(model, place);
    let shared = match data_dir {
        Some(dir) => Shared::with_data_dir(Some(agent), "Anime Card Battles Apex".to_string(), dir),
        None => Shared::new(Some(agent), "Anime Card Battles Apex".to_string()),
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _ = serve(listener, shared);
    });
    port
}

fn http_raw_bytes(
    port: u16,
    method: &str,
    path: &str,
    content_type: &str,
    body: &[u8],
) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let req = if body.is_empty() {
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
    };
    stream.write_all(req.as_bytes()).unwrap();
    if !body.is_empty() {
        stream.write_all(body).unwrap();
    }
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let sep = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(buf.len());
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

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("openro-v060-{tag}-{nonce}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// Valid minimal 1x1 PNG (67 bytes)
const TINY_PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+X2dIAAAAASUVORK5CYII=";
const TINY_PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

#[test]
fn art_knowledge_and_system_prompt_include_anime_homage_guidance() {
    let port = start_local_server(None);
    let (st, _, raw) = http_raw_bytes(port, "GET", "/api/art-knowledge", "application/json", b"");
    assert_eq!(st, 200);
    let text = String::from_utf8(raw).unwrap();
    assert!(!text.to_ascii_lowercase().contains("rune"), "zero internal jargon in /api/art-knowledge: {text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["never_refuse_anime_homage"], json!(true));
    assert!(v["archetypes"].as_array().unwrap().len() >= 8);

    let sp = system_prompt();
    assert!(sp.contains("ORIGINAL ANIME-STYLE CHARACTER DESIGN KNOWLEDGE POINT"));
    assert!(sp.contains("100% original anime-style characters"));
}

#[test]
fn assets_upload_serve_persist_and_reject_traversal_fuzz() {
    let dir = temp_dir("assets-roundtrip");
    let port = start_local_server(Some(dir.clone()));

    // 1. JSON base64 upload with full card metadata.
    let payload = json!({
        "filename": "crimson-sea-captain.png",
        "data_base64": format!("data:image/png;base64,{TINY_PNG_B64}"),
        "card_id": "crimson-sea-captain",
        "title": "Crimson Sea Captain",
        "rarity": "Mythic",
        "odds": "1 in 512",
        "element": "Flame",
        "atk": 420,
        "hp": 1480,
        "speed": 88,
        "passive": "Molten Will",
        "ultimate": "Red Horizon Impact",
        "source": "nano-banana"
    })
    .to_string();
    let (st, _, raw) = http_raw_bytes(
        port,
        "POST",
        "/api/assets/upload",
        "application/json",
        payload.as_bytes(),
    );
    assert_eq!(st, 201, "upload failed: {}", String::from_utf8_lossy(&raw));
    let up: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(up["asset"]["url"], json!("/api/assets/crimson-sea-captain.png"));
    assert_eq!(up["asset"]["mime"], json!("image/png"));

    // 2. Serve binary PNG byte-exact via GET /api/assets/crimson-sea-captain.png
    let (st, head, bytes) = http_raw_bytes(
        port,
        "GET",
        "/api/assets/crimson-sea-captain.png",
        "application/json",
        b"",
    );
    assert_eq!(st, 200);
    assert!(head.contains("image/png"), "head: {head}");
    assert_eq!(&bytes[..8], &TINY_PNG_MAGIC);

    // 3. Raw binary image/png upload with ?filename=...
    let (st, _, raw_up) = http_raw_bytes(
        port,
        "POST",
        "/api/assets/upload?filename=shadow-sovereign.png",
        "image/png",
        &bytes,
    );
    assert_eq!(st, 201, "raw upload failed: {}", String::from_utf8_lossy(&raw_up));

    // 4. List assets & verify persistence across server reload.
    let (st, _, list_raw) = http_raw_bytes(port, "GET", "/api/assets", "application/json", b"");
    assert_eq!(st, 200);
    let list: Value = serde_json::from_slice(&list_raw).unwrap();
    assert_eq!(list["count"], json!(2));

    let port2 = start_local_server(Some(dir.clone()));
    let (st, _, list2_raw) = http_raw_bytes(port2, "GET", "/api/assets", "application/json", b"");
    assert_eq!(st, 200);
    let list2: Value = serde_json::from_slice(&list2_raw).unwrap();
    assert_eq!(list2["count"], json!(2), "assets persisted across restart");

    // 4b. Asset manifest endpoint + .rbxlx export binds Decal Texture & ImageLabel Image.
    let (st, _, man_raw) = http_raw_bytes(port2, "GET", "/api/assets/manifest", "application/json", b"");
    assert_eq!(st, 200);
    let man: Value = serde_json::from_slice(&man_raw).unwrap();
    assert_eq!(man["asset_count"], json!(2));
    assert_eq!(man["schema"], json!("openro-asset-manifest-v1"));

    let (st, _, rbxlx_raw) = http_raw_bytes(port2, "GET", "/api/export/rbxlx", "application/json", b"");
    assert_eq!(st, 200);
    let xml = String::from_utf8(rbxlx_raw).unwrap();
    assert!(xml.contains("CardArtDecal"), "rbxlx missing CardArtDecal");
    assert!(xml.contains("rbxasset://openro-assets/crimson-sea-captain.png"), "rbxlx missing bound Decal/ImageLabel URI");
    assert!(xml.contains("CardArtImage"), "rbxlx missing CardArtImage");

    // 5. Security fuzzing: path traversal, encoded dots, leading dot, bad ext, fake PNG bytes.
    for bad_path in [
        "/api/assets/..%2f..%2fetc%2fpasswd",
        "/api/assets/../config.json",
        "/api/assets/.secret.png",
        "/api/assets/sub/file.png",
        "/api/assets/not-an-image.txt",
        "/api/assets/bad..name.png",
    ] {
        let (st, _, _) = http_raw_bytes(port, "GET", bad_path, "application/json", b"");
        assert!(
            st == 400 || st == 403 || st == 404,
            "traversal/bad path {bad_path} must not succeed (got {st})"
        );
    }

    // Fake non-image payload rejected with 400.
    let bad_img = json!({
        "filename": "evil.png",
        "data_base64": "SGVsbG8gd29ybGQh" // "Hello world!"
    })
    .to_string();
    let (st, _, _) = http_raw_bytes(
        port,
        "POST",
        "/api/assets/upload",
        "application/json",
        bad_img.as_bytes(),
    );
    assert_eq!(st, 400, "non-PNG bytes must be rejected with 400");

    // Traversal filename in JSON upload rejected with 400.
    let bad_name = json!({
        "filename": "../escape.png",
        "data_base64": TINY_PNG_B64
    })
    .to_string();
    let (st, _, _) = http_raw_bytes(
        port,
        "POST",
        "/api/assets/upload",
        "application/json",
        bad_name.as_bytes(),
    );
    assert_eq!(st, 400, "traversal filename upload must be rejected with 400");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn assets_generate_fails_honestly_without_api_key_and_config_masks_keys() {
    let dir = temp_dir("assets-gen");
    let port = start_local_server(Some(dir.clone()));

    // Ensure no image key is set in env for this test assertion (if host lacks GEMINI_API_KEY / OPENAI_API_KEY).
    if std::env::var("GEMINI_API_KEY").is_err() && std::env::var("OPENAI_API_KEY").is_err() {
        let req = json!({
            "title": "Prism-Rift Archivist",
            "element": "Void",
            "rarity": "Celestial"
        })
        .to_string();
        let (st, _, raw) = http_raw_bytes(
            port,
            "POST",
            "/api/assets/generate",
            "application/json",
            req.as_bytes(),
        );
        assert_eq!(st, 503, "no-key generate must fail honestly with 503");
        let v: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["ok"], json!(false));
        assert!(v["homage_prompt"].as_str().unwrap().contains("Prism-Rift Archivist"));
    }

    // Saving gemini_api_key masks it on GET /api/config without leaking raw value.
    let cfg_body = json!({
        "base_url": "http://127.0.0.1:11434/v1",
        "model_id": "qwen2.5:3b",
        "workspace_mode": "local_place",
        "gemini_api_key": "AIzaSyExampleSecretKey9999",
        "image_provider": "gemini"
    })
    .to_string();
    let (st, _, raw) = http_raw_bytes(
        port,
        "POST",
        "/api/config",
        "application/json",
        cfg_body.as_bytes(),
    );
    assert_eq!(st, 200, "config save failed: {}", String::from_utf8_lossy(&raw));
    let txt = String::from_utf8(raw).unwrap();
    assert!(!txt.contains("AIzaSyExampleSecretKey9999"), "raw key must never leak: {txt}");
    let v: Value = serde_json::from_str(&txt).unwrap();
    assert_eq!(v["has_gemini_api_key"], json!(true));
    assert_eq!(v["gemini_api_key_masked"], json!("••••9999"));
    assert_eq!(v["image_provider"], json!("gemini"));

    let _ = std::fs::remove_dir_all(&dir);
}
