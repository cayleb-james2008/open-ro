//! Integration tests for OpenRO v0.12 Creator Hub CDP automation (#68)
//! and Open Cloud pipeline upgrades (#69):
//! - Loopback-only CDP URL validation & Host guard
//! - Mock `OPENRO_CREATOR_HUB_CMD` secret handoff -> 0600 SecretStore with zero leaks
//! - `openro.project.json` `universeId`/`placeId` auto-linking & `action_required` propagation
//! - `creator-hub/configure` path jail + Private Servers / Icon / Thumbnails / Questionnaire check
//! - Transparent `.wav -> .ogg` & `.obj -> .fbx` upload conversions keeping original lock keys
//! - Decal -> Image XML resolution -> `imageAssetId` in `assets.lock.json` & `AssetIds.luau`
//! - Incremental `assets.lock.json` persistence & 6x `429` retries
//! - `products/sync` `id`/`robloxId` dual support + `default_for_sale` + `products/codegen` (`ProductIds.luau`)
//!
//! Zero live Roblox or browser calls.

use openro_rs::agent::Agent;
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
        .join("openro-v012-tests")
        .join(format!("{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample_png_bytes() -> Vec<u8> {
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

#[test]
fn creator_hub_loopback_secret_handoff_manifest_and_blockers_gap_68() {
    let _env_lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("gap68");
    let data_dir = root.join("data");
    let secrets_dir = root.join("secrets");
    let project_dir = data_dir.join("projects").join("uacg-demo");
    std::fs::create_dir_all(project_dir.join("art/store")).unwrap();
    let png = sample_png_bytes();
    std::fs::write(project_dir.join("art/store/icon.png"), &png).unwrap();
    std::fs::write(project_dir.join("art/store/thumb1.png"), &png).unwrap();
    std::fs::write(
        project_dir.join("openro.project.json"),
        json!({
            "version": 1,
            "name": "Untitled Anime Card Game",
            "universeId": null,
            "placeId": null,
            "preserveMe": "keep-this-value"
        })
        .to_string(),
    )
    .unwrap();

    // Mock Creator Hub CDP driver supporting status, setup (with secret echo test), 2FA/CAPTCHA blockers, and configure
    let mock_cmd = root.join("mock_creator_hub.py");
    std::fs::write(
        &mock_cmd,
        r#"#!/usr/bin/env python3
import argparse, json, os, sys

p = argparse.ArgumentParser()
p.add_argument("--action", required=True)
p.add_argument("--cdp-url", required=True)
p.add_argument("--key-out", default=None)
p.add_argument("--check-login", action="store_true")
args = p.parse_args()
spec = json.loads(sys.stdin.read() or "{}")

if args.action == "status":
    print(json.dumps({
        "ok": True,
        "cdp_reachable": True,
        "browser_version": "Chrome/135.0-mock",
        "logged_in": True,
        "user": {"id": "1170918805", "name": "Chrono_H3X", "displayName": "Chrono"},
        "experiences": [
            {"universeId": "10771116814", "rootPlaceId": "112916648108736", "name": "Untitled Anime Card Game"}
        ]
    }))
    sys.exit(0)

if args.action == "setup":
    if spec.get("key_name") == "TRIGGER_2FA":
        print(json.dumps({
            "ok": False,
            "status": "action_required",
            "blocked_reason": "2fa_required",
            "url": "https://create.roblox.com/dashboard/credentials",
            "message": "Complete 2FA in the open browser tab."
        }))
        sys.exit(0)
    if spec.get("key_name") == "TRIGGER_CAPTCHA":
        print(json.dumps({
            "ok": False,
            "status": "action_required",
            "blocked_reason": "captcha_required",
            "url": "https://create.roblox.com/dashboard/credentials",
            "message": "Complete CAPTCHA in the open browser tab."
        }))
        sys.exit(0)
    raw_secret = "SUPER_SECRET_OPENCLOUD_KEY_ABC9876"
    if args.key_out and spec.get("need_key", True):
        # Verify the temp handoff file was pre-created 0600 by Rust
        st_mode = os.stat(args.key_out).st_mode & 0o777
        assert st_mode == 0o600, f"expected 0600 handoff file, got {oct(st_mode)}"
        with open(args.key_out, "w", encoding="utf-8") as f:
            f.write(raw_secret)
    print(json.dumps({
        "ok": True,
        "status": "ready",
        "user": {"id": "1170918805", "name": "Chrono_H3X", "displayName": "Chrono"},
        "universeId": "10771116814",
        "placeId": "112916648108736",
        "experience_name": "Untitled Anime Card Game (debug echo " + raw_secret + ")",
        "experiences": [
            {"universeId": "10771116814", "rootPlaceId": "112916648108736", "name": "Untitled Anime Card Game"}
        ],
        "key_written": bool(spec.get("need_key", True))
    }))
    sys.exit(0)

if args.action == "configure":
    configured = []
    if spec.get("private_servers"):
        configured.append("private_servers")
    if spec.get("icon_abs_path"):
        assert os.path.isfile(spec["icon_abs_path"])
        configured.append("icon")
    if spec.get("thumbnail_abs_paths"):
        for tp in spec["thumbnail_abs_paths"]:
            assert os.path.isfile(tp)
        configured.append("thumbnails")
    print(json.dumps({
        "ok": True,
        "status": "ready",
        "configured": configured,
        "manual_steps_remaining": [
            {
                "step": "Complete Age & Experience Guidelines questionnaire",
                "url": f"https://create.roblox.com/dashboard/creations/experiences/{spec['universeId']}/content-rating"
            }
        ]
    }))
    sys.exit(0)
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mock_cmd, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _env = EnvGuard::set(&[
        ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
        ("OPENRO_CREATOR_HUB_CMD", mock_cmd.to_str().unwrap()),
    ]);
    let port = start_server(&data_dir);

    // 1. Foreign Host -> 403
    let (st_host, _, _) = http_req(port, "evil.example.com", "GET", "/api/creator-hub/status", b"");
    assert_eq!(st_host, 403);

    // 2. Non-loopback CDP URL -> 400 on status, setup, configure
    let (st_bad_cdp, bad_cdp) = http_json(
        port,
        "GET",
        "/api/creator-hub/status?cdp_url=http://192.168.1.99:9222",
        &json!({}),
    );
    assert_eq!(st_bad_cdp, 400, "expected 400 on remote CDP URL: {bad_cdp}");
    let (st_bad_setup, _) = http_json(
        port,
        "POST",
        "/api/projects/uacg-demo/creator-hub/setup",
        &json!({ "cdp_url": "https://127.0.0.1:9222" }),
    );
    assert_eq!(st_bad_setup, 400);

    // 3. Status with check_login=1&save_creator=1 persists creator userId
    let (st_s, st_body) = http_json(
        port,
        "GET",
        "/api/creator-hub/status?check_login=1&save_creator=1",
        &json!({}),
    );
    assert_eq!(st_s, 200, "status failed: {st_body}");
    assert_eq!(st_body["cdp_reachable"], true);
    assert_eq!(st_body["logged_in"], true);
    assert_eq!(st_body["user"]["name"], "Chrono_H3X");
    assert_eq!(st_body["creator"]["userId"], "1170918805");
    let (_, oc_status) = http_json(port, "GET", "/api/opencloud", &json!({}));
    assert_eq!(oc_status["creator"]["userId"], "1170918805");

    // 4. 2FA & CAPTCHA action_required propagation (no secret saved)
    for (trigger, expected_reason) in [
        ("TRIGGER_2FA", "2fa_required"),
        ("TRIGGER_CAPTCHA", "captcha_required"),
    ] {
        let (c, r) = http_json(
            port,
            "POST",
            "/api/projects/uacg-demo/creator-hub/setup",
            &json!({ "key_name": trigger }),
        );
        assert_eq!(c, 200, "expected 200 action_required payload: {r}");
        assert_eq!(r["ok"], false);
        assert_eq!(r["status"], "action_required");
        assert_eq!(r["blocked_reason"], expected_reason);
        assert_eq!(r["has_key"], false);
    }

    // 5. Full setup: atomic 0600 SecretStore handoff, zero secret leak, manifest universeId/placeId update
    let (st_setup, setup_res) = http_json(
        port,
        "POST",
        "/api/projects/uacg-demo/creator-hub/setup",
        &json!({ "reuse_universe_id": "10771116814", "cidr": "0.0.0.0/0" }),
    );
    assert_eq!(st_setup, 200, "setup failed: {setup_res}");
    assert_eq!(setup_res["ok"], true);
    assert_eq!(setup_res["status"], "ready");
    assert_eq!(setup_res["has_key"], true);
    assert_eq!(setup_res["key_masked"], "••••9876");
    assert_eq!(setup_res["universeId"], "10771116814");
    assert_eq!(setup_res["placeId"], "112916648108736");
    // Raw secret MUST be scrubbed even from any echoed string!
    assert!(
        !setup_res.to_string().contains("SUPER_SECRET_OPENCLOUD_KEY_ABC9876"),
        "raw secret leaked in setup response: {setup_res}"
    );
    // Secret saved with 0600 in SecretStore and zero temp handoff files left behind
    let stored = std::fs::read_to_string(secrets_dir.join("opencloud_api_key")).unwrap();
    assert_eq!(stored.trim(), "SUPER_SECRET_OPENCLOUD_KEY_ABC9876");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(secrets_dir.join("opencloud_api_key"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    let leftovers: Vec<_> = std::fs::read_dir(&secrets_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("creator-hub-key"))
        .collect();
    assert!(leftovers.is_empty(), "temp key handoff file leaked on disk: {leftovers:?}");

    // Manifest updated while preserving unknown fields
    let m: Value = serde_json::from_str(
        &std::fs::read_to_string(project_dir.join("openro.project.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(m["universeId"], 10771116814u64);
    assert_eq!(m["placeId"], 112916648108736u64);
    assert_eq!(m["preserveMe"], "keep-this-value");

    // 6. Configure: reject path traversal with 400; succeed with valid project assets
    let (st_trav, _) = http_json(
        port,
        "POST",
        "/api/projects/uacg-demo/creator-hub/configure",
        &json!({ "icon_path": "../etc/passwd" }),
    );
    assert_eq!(st_trav, 400);
    let (st_cfg, cfg_res) = http_json(
        port,
        "POST",
        "/api/projects/uacg-demo/creator-hub/configure",
        &json!({
            "private_servers": { "enabled": true, "price_robux": 0 },
            "icon_path": "art/store/icon.png",
            "thumbnail_paths": ["art/store/thumb1.png"],
            "check_questionnaire": true
        }),
    );
    assert_eq!(st_cfg, 200, "configure failed: {cfg_res}");
    assert_eq!(cfg_res["ok"], true);
    assert_eq!(cfg_res["configured"], json!(["private_servers", "icon", "thumbnails"]));
    assert_eq!(cfg_res["manual_steps_remaining"].as_array().unwrap().len(), 1);

    let _ = std::fs::remove_dir_all(&root);
}

#[derive(Default)]
struct PipeMockState {
    uploads: Vec<(String, String, String)>, // (asset_type, filename, content_type)
    rate_limit_remaining: u32,
    upload_seq: u64,
    fail_on_name: Option<String>,
    dev_products: Vec<Value>,
    game_passes: Vec<Value>,
    product_posts: Vec<String>,
    next_prod_id: u64,
}

fn spawn_pipeline_mock() -> (String, Arc<Mutex<PipeMockState>>) {
    let state = Arc::new(Mutex::new(PipeMockState {
        rate_limit_remaining: 5, // Exercises >3 retries (expanded to 6 in #69!)
        next_prod_id: 9000,
        ..PipeMockState::default()
    }));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let st = Arc::clone(&state);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let st = Arc::clone(&st);
            std::thread::spawn(move || handle_pipe_conn(stream, &st));
        }
    });
    (format!("http://127.0.0.1:{}", addr.port()), state)
}

fn handle_pipe_conn(mut stream: TcpStream, state: &Mutex<PipeMockState>) {
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
    let body_str = String::from_utf8_lossy(&buf[he..buf.len().min(he + content_len)]).to_string();
    let first = head.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    // Assetdelivery XML endpoint for Decal -> Image resolution (must NOT receive x-api-key!)
    if method == "GET" && path.starts_with("/v1/asset/?id=") {
        assert!(
            !head.to_ascii_lowercase().contains("x-api-key:"),
            "x-api-key must never be forwarded to assetdelivery!"
        );
        let decal_id = path.trim_start_matches("/v1/asset/?id=");
        let img_id = decal_id.parse::<u64>().unwrap_or(100) + 100_000;
        let xml = format!(
            r#"<roblox version="4"><Item class="Decal"><Properties><Content name="Texture"><url>http://www.roblox.com/asset/?id={img_id}</url></Content></Properties></Item></roblox>"#
        );
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{xml}",
            xml.len()
        );
        let _ = stream.write_all(resp.as_bytes());
        return;
    }

    let mut st = state.lock().unwrap();
    if method == "POST" && path == "/assets/v1/assets" && st.rate_limit_remaining > 0 {
        st.rate_limit_remaining -= 1;
        drop(st);
        let payload = json!({ "message": "rate limited" }).to_string();
        let resp = format!(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        let _ = stream.write_all(resp.as_bytes());
        return;
    }

    let (code, val): (&str, Value) = if method == "POST" && path == "/assets/v1/assets" {
        if let Some(ref fail_name) = st.fail_on_name {
            if body_str.contains(fail_name) {
                ("500 Internal Server Error", json!({ "message": "simulated mid-batch crash" }))
            } else {
                handle_asset_post(&mut st, &body_str)
            }
        } else {
            handle_asset_post(&mut st, &body_str)
        }
    } else if method == "GET" && path.contains("/developer-products/creator") {
        ("200 OK", json!({ "developerProducts": st.dev_products.clone() }))
    } else if method == "POST" && path.ends_with("/developer-products") {
        st.product_posts.push(body_str.clone());
        st.next_prod_id += 1;
        let id = st.next_prod_id;
        ("200 OK", json!({ "productId": id }))
    } else if method == "GET" && path.contains("/game-passes/creator") {
        ("200 OK", json!({ "gamePasses": st.game_passes.clone() }))
    } else if method == "POST" && path.ends_with("/game-passes") {
        st.product_posts.push(body_str.clone());
        st.next_prod_id += 1;
        let id = st.next_prod_id;
        ("200 OK", json!({ "gamePassId": id }))
    } else {
        ("404 Not Found", json!({ "message": format!("unmocked {method} {path}") }))
    };
    drop(st);
    let payload = val.to_string();
    let resp = format!(
        "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let _ = stream.write_all(resp.as_bytes());
}

fn handle_asset_post(st: &mut PipeMockState, body_str: &str) -> (&'static str, Value) {
    let fname = body_str
        .split("filename=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap_or("")
        .to_string();
    let ctype = body_str
        .split("Content-Type: ")
        .nth(1)
        .and_then(|s| s.lines().next())
        .unwrap_or("")
        .trim()
        .to_string();
    let atype = body_str
        .split("\"assetType\":\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap_or("")
        .to_string();
    if atype == "Image" && fname == "art.png" {
        return ("400 Bad Request", json!({ "message": "use Decal" }));
    }
    st.upload_seq += 1;
    let id = 770_000 + st.upload_seq;
    st.uploads.push((atype, fname, ctype));
    (
        "200 OK",
        json!({
            "path": format!("operations/op-{}", st.upload_seq),
            "done": true,
            "response": {
                "assetId": id.to_string(),
                "moderationResult": { "moderationState": "MODERATION_STATE_APPROVED" }
            }
        }),
    )
}

#[test]
fn opencloud_conversions_decal_image_incremental_lock_429_and_products_codegen_gap_69() {
    let _env_lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("gap69");
    let data_dir = root.join("data");
    let secrets_dir = root.join("secrets");
    let project_dir = data_dir.join("projects").join("uacg-pipe");
    std::fs::create_dir_all(project_dir.join("art/cards/w1_naruto")).unwrap();
    std::fs::create_dir_all(project_dir.join("assets/audio")).unwrap();
    std::fs::create_dir_all(project_dir.join("assets/meshes")).unwrap();
    std::fs::create_dir_all(project_dir.join("src/shared")).unwrap();
    std::fs::write(
        project_dir.join("stylua.toml"),
        "syntax = \"Luau\"\ncolumn_width = 120\nline_endings = \"Unix\"\nindent_type = \"Tabs\"\nindent_width = 4\nquote_style = \"AutoPreferDouble\"\n",
    )
    .unwrap();

    let png = sample_png_bytes();
    let mut png_fail = png.clone();
    png_fail.extend_from_slice(b"fail-marker");
    std::fs::write(project_dir.join("art/cards/w1_naruto/art.png"), &png).unwrap();
    std::fs::write(project_dir.join("art/cards/zz_boom.png"), &png_fail).unwrap();
    std::fs::write(project_dir.join("assets/audio/click.wav"), b"RIFF0000WAVEmockwav").unwrap();
    std::fs::write(project_dir.join("assets/meshes/roll_altar.obj"), b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    std::fs::write(project_dir.join("assets/meshes/roll_altar.png"), &png).unwrap();
    std::fs::write(
        project_dir.join("assets/meshes/manifest.json"),
        json!({ "meshes": { "roll_altar": { "size": [4.771, 2.65, 4.771], "tris": 3408 } } }).to_string(),
    )
    .unwrap();
    std::fs::write(
        project_dir.join("openro.project.json"),
        json!({
            "version": 1,
            "name": "UACG Pipe",
            "universeId": 10771116814u64,
            "placeId": 112916648108736u64,
            "assets": {
                "lockFile": "assets.lock.json",
                "authorised": [
                    "art/cards/**",
                    "assets/audio/**",
                    "assets/meshes/*.obj"
                ]
            },
            "products": [
                {
                    "key": "pass_vip",
                    "kind": "game_pass",
                    "name": "VIP",
                    "description": "VIP perks",
                    "priceRobux": 349,
                    "priceApproved": true,
                    "robloxId": null
                },
                {
                    "key": "prod_gems_pouch",
                    "kind": "developer_product",
                    "name": "Pouch of Gems",
                    "description": "1,200 Gems",
                    "priceRobux": 99,
                    "priceApproved": true,
                    "robloxId": null
                }
            ]
        })
        .to_string(),
    )
    .unwrap();

    // Mock ffmpeg & blender converters to verify transparent WAV->OGG & OBJ(+PNG)->FBX wiring
    let mock_ffmpeg = root.join("mock_ffmpeg.py");
    std::fs::write(
        &mock_ffmpeg,
        r#"#!/usr/bin/env python3
import sys
out_path = sys.argv[-1]
assert "-nostdin" in sys.argv and "libvorbis" in sys.argv
with open(out_path, "wb") as f:
    f.write(b"OggS-mock-vorbis-bytes")
"#,
    )
    .unwrap();
    let mock_blender = root.join("mock_blender.py");
    std::fs::write(
        &mock_blender,
        r#"#!/usr/bin/env python3
import os, sys
assert "--background" in sys.argv
obj_in = os.environ["OPENRO_OBJ_IN"]
png_in = os.environ.get("OPENRO_PNG_IN", "")
fbx_out = os.environ["OPENRO_FBX_OUT"]
assert os.path.isfile(obj_in)
assert os.path.isfile(png_in), f"expected sibling diffuse png, got {png_in!r}"
with open(fbx_out, "wb") as f:
    f.write(b"Kaydara FBX Binary  mock-with-tex")
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mock_ffmpeg, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&mock_blender, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let (mock_base, mock_state) = spawn_pipeline_mock();
    mock_state.lock().unwrap().fail_on_name = Some("zz_boom".to_string());

    let _env = EnvGuard::set(&[
        ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
        ("OPENRO_OPENCLOUD_BASE", &mock_base),
        ("OPENRO_OPENCLOUD_POLL_MS", "1"),
        ("OPENRO_FFMPEG_BIN", mock_ffmpeg.to_str().unwrap()),
        ("OPENRO_BLENDER_BIN", mock_blender.to_str().unwrap()),
    ]);
    let port = start_server(&data_dir);

    let (st_k, _) = http_json(port, "POST", "/api/opencloud/key", &json!({ "api_key": "valid_pipeline_key_1234" }));
    assert_eq!(st_k, 200);
    let (st_c, _) = http_json(port, "POST", "/api/opencloud/settings", &json!({ "creator": { "userId": 1170918805 } }));
    assert_eq!(st_c, 200);

    // 1. Upload authorised assets: exercises 5x 429 retries, Decal->Image resolution, WAV->OGG, OBJ->FBX, and mid-batch failure on zz_boom
    let (st_up, up) = http_json(port, "POST", "/api/projects/uacg-pipe/assets/upload", &json!({}));
    assert_eq!(st_up, 200, "upload failed: {up}");
    assert_eq!(up["summary"]["uploaded"], 3, "expected 3 succeeded + 1 failed: {up}");
    assert_eq!(up["summary"]["failed"], 1, "expected zz_boom failure: {up}");

    // Verify converted wire payloads
    let uploads = mock_state.lock().unwrap().uploads.clone();
    assert!(
        uploads.iter().any(|(t, f, m)| t == "Audio" && f == "click.ogg" && m == "audio/ogg"),
        "expected WAV->OGG wire upload: {uploads:?}"
    );
    assert!(
        uploads.iter().any(|(t, f, m)| t == "Model" && f == "roll_altar.fbx" && m == "model/fbx"),
        "expected OBJ->FBX wire upload: {uploads:?}"
    );

    // Verify incremental lock persisted all 3 successful assets under original project-relative paths, incl. imageAssetId
    let lock: Value = serde_json::from_str(
        &std::fs::read_to_string(project_dir.join("assets.lock.json")).unwrap(),
    )
    .unwrap();
    let naruto_lock = &lock["assets"]["art/cards/w1_naruto/art.png"];
    assert_eq!(naruto_lock["assetType"], "Decal");
    let decal_id = naruto_lock["assetId"].as_str().unwrap();
    let expected_img_id = (decal_id.parse::<u64>().unwrap() + 100_000).to_string();
    assert_eq!(naruto_lock["imageAssetId"], expected_img_id);
    assert!(lock["assets"].get("assets/audio/click.wav").is_some());
    assert!(lock["assets"].get("assets/meshes/roll_altar.obj").is_some());

    // Re-running upload hits unchanged on all 3 original files (lock key = original path + original sha256)
    mock_state.lock().unwrap().fail_on_name = None;
    let (_, up2) = http_json(
        port,
        "POST",
        "/api/projects/uacg-pipe/assets/upload",
        &json!({ "paths": ["art/cards/w1_naruto/art.png", "assets/audio/click.wav", "assets/meshes/roll_altar.obj"] }),
    );
    assert_eq!(up2["summary"]["unchanged"], 3, "expected unchanged on re-upload: {up2}");

    // Verify AssetIds.luau emits resolved Image ID + cards/<id> + <id> + mesh assetId
    let asset_luau = std::fs::read_to_string(project_dir.join("src/shared/AssetIds.luau")).unwrap();
    assert!(asset_luau.contains(&format!("[\"cards/w1_naruto\"] = \"rbxassetid://{expected_img_id}\"")), "got:\n{asset_luau}");
    assert!(asset_luau.contains(&format!("[\"w1_naruto\"] = \"rbxassetid://{expected_img_id}\"")), "got:\n{asset_luau}");
    assert!(asset_luau.contains(&format!("[\"w1_naruto_art\"] = \"rbxassetid://{expected_img_id}\"")), "got:\n{asset_luau}");
    let mesh_id = lock["assets"]["assets/meshes/roll_altar.obj"]["assetId"].as_str().unwrap();
    assert!(asset_luau.contains(&format!("assetId = \"rbxassetid://{mesh_id}\"")), "got:\n{asset_luau}");

    // 2. products/sync with default_for_sale=true writes both id & robloxId and auto-generates ProductIds.luau
    let (st_ps, ps) = http_json(
        port,
        "POST",
        "/api/projects/uacg-pipe/products/sync",
        &json!({ "default_for_sale": true }),
    );
    assert_eq!(st_ps, 200, "products sync failed: {ps}");
    assert_eq!(ps["product_ids_luau"]["count"], 2);
    let posts = mock_state.lock().unwrap().product_posts.clone();
    assert!(posts.iter().all(|b| b.contains("name=\"isForSale\"\r\n\r\ntrue")), "expected default_for_sale=true: {posts:?}");

    let manifest_after: Value = serde_json::from_str(
        &std::fs::read_to_string(project_dir.join("openro.project.json")).unwrap(),
    )
    .unwrap();
    for p in manifest_after["products"].as_array().unwrap() {
        assert!(p["id"].as_u64().is_some(), "missing id: {p}");
        assert_eq!(p["id"], p["robloxId"], "id and robloxId must match: {p}");
    }

    // 3. Explicit POST /api/projects/:id/products/codegen generates stylua-clean ProductIds.luau
    let (st_pcg, pcg) = http_json(port, "POST", "/api/projects/uacg-pipe/products/codegen", &json!({}));
    assert_eq!(st_pcg, 200, "products codegen failed: {pcg}");
    assert_eq!(pcg["game_passes"], 1);
    assert_eq!(pcg["developer_products"], 1);
    let prod_luau = std::fs::read_to_string(project_dir.join("src/shared/ProductIds.luau")).unwrap();
    assert!(prod_luau.contains("[\"pass_vip\"] = 9001"), "got:\n{prod_luau}");
    assert!(prod_luau.contains("[\"prod_gems_pouch\"] = 9002"), "got:\n{prod_luau}");

    if let Some(stylua) = openro_rs::projects::find_tool("stylua", "OPENRO_STYLUA") {
        for rel in ["src/shared/AssetIds.luau", "src/shared/ProductIds.luau"] {
            let out = std::process::Command::new(&stylua)
                .current_dir(&project_dir)
                .args(["--check", "--", rel])
                .output()
                .expect("run stylua");
            assert!(
                out.status.success(),
                "stylua --check failed on {rel}:\n{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn opencloud_moderation_circuit_breaker_and_weapon_sheet_gap_72_73() {
    let _env_lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = scratch_dir("gap72_73");
    let data_dir = root.join("data");
    let secrets_dir = root.join("secrets");
    let project_dir = data_dir.join("projects").join("uacg-mod");
    std::fs::create_dir_all(project_dir.join("art/cards/c1")).unwrap();
    std::fs::create_dir_all(project_dir.join("art/cards/c2")).unwrap();
    std::fs::create_dir_all(project_dir.join("art/cards/c3")).unwrap();
    std::fs::create_dir_all(project_dir.join("art/references")).unwrap();
    std::fs::create_dir_all(project_dir.join("art/weapons")).unwrap();

    let png1 = sample_png_bytes();
    let mut png2 = sample_png_bytes();
    let mut png3 = sample_png_bytes();
    png2.extend_from_slice(b"c2");
    png3.extend_from_slice(b"c3");
    std::fs::write(project_dir.join("art/cards/c1/art.png"), &png1).unwrap();
    std::fs::write(project_dir.join("art/cards/c2/art.png"), &png2).unwrap();
    std::fs::write(project_dir.join("art/cards/c3/art.png"), &png3).unwrap();
    std::fs::write(project_dir.join("art/references/toji.png"), &png1).unwrap();
    std::fs::write(project_dir.join("art/weapons/toji_weapon.png"), &png2).unwrap();

    std::fs::write(
        project_dir.join("openro.project.json"),
        json!({
            "name": "UACG Mod",
            "universeId": 10771116814u64,
            "placeId": 112916648108736u64,
            "assets": { "authorised": ["art/cards/*/art.png"], "lockFile": "assets.lock.json" }
        })
        .to_string(),
    )
    .unwrap();

    // 1. Prompt & weapon_sheet helpers (#72, #73)
    let p_safe = openro_rs::assets::ensure_roblox_safe_prompt("Itachi Uchiha in Akatsuki cloak");
    assert!(p_safe.to_ascii_lowercase().contains("zero blood"), "got: {p_safe}");
    assert_eq!(openro_rs::assets::ensure_roblox_safe_prompt(&p_safe), p_safe);

    let p_ws = openro_rs::assets::ensure_weapon_sheet_prompt("Inverted Spear of Heaven");
    assert!(p_ws.contains("orthographic prop reference sheet"), "got: {p_ws}");
    assert!(p_ws.to_ascii_lowercase().contains("zero blood"), "got: {p_ws}");

    assert_eq!(
        openro_rs::assets::default_out_path_for_test(&json!({
            "kind": "weapon_sheet",
            "card_id": "w4_sorcerer_killer_weapon"
        })),
        "art/weapons/w4_sorcerer_killer_weapon.png"
    );
    let resolved = openro_rs::assets::resolve_reference_images_for_test(
        &project_dir,
        &json!({
            "reference_image": "art/references/toji.png",
            "weapon_sheet": "art/weapons/toji_weapon.png"
        }),
    )
    .unwrap();
    assert_eq!(
        resolved,
        vec![
            "art/references/toji.png".to_string(),
            "art/weapons/toji_weapon.png".to_string()
        ]
    );

    // 2. Moderation 403 circuit-breaker (#72): mock returns 403 User is moderated on first upload; batch must halt at 1 request!
    let mod_calls = Arc::new(Mutex::new(0usize));
    let mod_calls_srv = Arc::clone(&mod_calls);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mock_addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let calls = Arc::clone(&mod_calls_srv);
            std::thread::spawn(move || {
                let mut s = stream;
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                *calls.lock().unwrap() += 1;
                let body = r#"{"code":"PERMISSION_DENIED","message":"User is moderated"}"#;
                let resp = format!(
                    "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
                let _ = s.flush();
            });
        }
    });

    let _g = EnvGuard::set(&[
        ("OPENRO_DATA_DIR", data_dir.to_str().unwrap()),
        ("OPENRO_SECRETS_DIR", secrets_dir.to_str().unwrap()),
        ("OPENRO_OPENCLOUD_BASE", &format!("http://127.0.0.1:{}", mock_addr.port())),
        ("OPENRO_OPENCLOUD_POLL_MS", "1"),
    ]);
    let store = openro_rs::secrets::SecretStore::at(secrets_dir);
    store.set(openro_rs::secrets::OPENCLOUD_KEY, "test-opencloud-key-0123456789").unwrap();
    store.set("opencloud_creator", "user:1170918805").unwrap();

    let port = start_server(&data_dir);
    let (st, res) = http_json(
        port,
        "POST",
        "/api/projects/uacg-mod/assets/upload",
        &json!({ "paths": ["art/cards/c1/art.png", "art/cards/c2/art.png", "art/cards/c3/art.png"] }),
    );
    assert_eq!(st, 200, "unexpected status: {res}");
    assert_eq!(res["summary"]["failed"], 1, "must halt after first moderated 403: {res}");
    assert_eq!(res["summary"]["halted_reason"], "account_moderated", "got: {res}");
    assert_eq!(*mod_calls.lock().unwrap(), 1, "must not send c2/c3 after User is moderated");

    let _ = std::fs::remove_dir_all(&root);
}
