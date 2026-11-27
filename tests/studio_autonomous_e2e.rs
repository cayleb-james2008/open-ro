//! End-to-end tests for Gap #84 (Local Studio Content Bridge + AssetIds codegen),
//! Gap #85 (Live Studio Device & Audio Verification + CUA window upgrades),
//! Gap #86 (Open Cloud Moderation Polling, Upload Filtering/Cap, & Rojo `.rbxlx` Publish),
//! and `gemini-4-neon-*` fallback model support.

use openro_rs::agent::{project_system_prompt, system_prompt, Agent};
use openro_rs::model::{agy_models_fallback, ModelClient};
use openro_rs::opencloud::assets_codegen;
use openro_rs::place::LocalPlace;
use openro_rs::projects::ProjectStore;
use openro_rs::server::{ensure_agent_hooks, serve, Shared};
use openro_rs::studio_embed::{
    find_studio_content_dir, find_vinegar_studio_content_dir, find_windows_studio_content_dir,
    sanitize_studio_content_slug,
};
use openro_rs::teamwork::teamwork_role_prompt_preview;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn set(pairs: &[(&'static str, Option<&str>)]) -> Self {
        let mut saved = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            saved.push((*k, std::env::var(k).ok()));
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
        EnvGuard { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, old) in self.saved.drain(..).rev() {
            match old {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let dir = base
        .join("studio-autonomous-e2e")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_stylua_toml(proj: &Path) {
    std::fs::write(
        proj.join("stylua.toml"),
        "syntax = \"Luau\"\ncolumn_width = 120\nline_endings = \"Unix\"\nindent_type = \"Tabs\"\nindent_width = 4\nquote_style = \"AutoPreferDouble\"\n",
    )
    .unwrap();
}

fn assert_stylua_clean(proj: &Path, rel: &str) {
    if let Some(stylua) = openro_rs::projects::find_tool("stylua", "OPENRO_STYLUA") {
        let out = std::process::Command::new(&stylua)
            .current_dir(proj)
            .args(["--check", "--", rel])
            .output()
            .expect("run stylua --check");
        assert!(
            out.status.success(),
            "stylua --check failed on {rel}:\n{}\n{}\nFILE:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
            std::fs::read_to_string(proj.join(rel)).unwrap_or_default()
        );
    }
}

/// Valid minimal RGBA PNG with customizable width/height in IHDR (for 9-slice & staging tests).
fn make_png(width: u32, height: u32) -> Vec<u8> {
    // Minimal 1x1 PNG bytes with patched IHDR width/height (valid enough for IHDR parsers & PNG magic checks).
    let mut png = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    png
}

/// Synthesize a real 16-bit mono PCM RIFF/WAVE file with controlled sample rate & peak amplitude.
fn make_pcm_wav(sample_rate: u32, duration_ms: u32, amplitude: f64) -> Vec<u8> {
    let frames = ((sample_rate as u64 * duration_ms as u64) / 1000).max(1) as usize;
    let data_bytes = (frames * 2) as u32;
    let riff_size = 36 + data_bytes;
    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..frames {
        let t = i as f64 / sample_rate as f64;
        let v = (2.0 * std::f64::consts::PI * 440.0 * t).sin() * amplitude;
        let sample = (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

fn boot_server(dir: &Path, proj_name: &str) -> (u16, String, PathBuf, PathBuf, Arc<Shared>) {
    let data = dir.join("data");
    let proj = dir.join(proj_name);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(proj.join("src/shared")).unwrap();
    write_stylua_toml(&proj);
    std::fs::write(
        proj.join("openro.project.json"),
        json!({
            "version": 1,
            "name": proj_name,
            "universeId": 991122,
            "placeId": 334455,
            "assets": {
                "lockFile": "assets.lock.json",
                "authorised": [
                    "art/cards/**",
                    "art/ui/*.png",
                    "art/ui/*/*.png",
                    "assets/images/**",
                    "assets/audio/**"
                ]
            }
        })
        .to_string(),
    )
    .unwrap();

    let model = ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None);
    let agent = Agent::with_local_place(model, LocalPlace::open(data.clone(), proj_name.into()));
    let shared = Shared::with_data_dir(Some(agent), proj_name.into(), data.clone());
    let proj_id = {
        let mut store: std::sync::MutexGuard<'_, ProjectStore> = shared.projects.lock().unwrap();
        let entry = store.open(proj.to_str().unwrap()).unwrap();
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
    (port, proj_id, data, proj, shared)
}

fn http_raw(port: u16, host: &str, method: &str, path: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let hdr = if body.is_empty() {
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
    };
    s.write_all(hdr.as_bytes()).unwrap();
    if !body.is_empty() {
        s.write_all(body).unwrap();
    }
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
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
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    (status, body_bytes)
}

fn http_json(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let bytes = body.map(|v| v.to_string().into_bytes()).unwrap_or_default();
    let (status, resp) = http_raw(port, &format!("127.0.0.1:{port}"), method, path, &bytes);
    (status, serde_json::from_slice(&resp).unwrap_or(Value::Null))
}

#[test]
fn model_fallback_includes_gemini_4_neon_tier_family() {
    let models = agy_models_fallback();
    let pairs: Vec<(&str, &str)> = models
        .iter()
        .map(|m| (m.id.as_str(), m.label.as_str()))
        .collect();
    for expected in [
        ("gemini-4-hydrogen-high", "Gemini 4 Hydrogen (High)"),
        ("gemini-4-hydrogen-xhigh", "Gemini 4 Hydrogen (XHigh)"),
        ("gemini-4-hydrogen-medium", "Gemini 4 Hydrogen (Medium)"),
        ("gemini-4-hydrogen-low", "Gemini 4 Hydrogen (Low)"),
        ("gemini-4-neon-xhigh", "Gemini 4 Neon (XHigh)"),
        ("gemini-4-neon-high", "Gemini 4 Neon (High)"),
        ("gemini-4-neon-medium", "Gemini 4 Neon (Medium)"),
        ("gemini-4-neon-low", "Gemini 4 Neon (Low)"),
    ] {
        assert!(
            pairs.contains(&expected),
            "fallback_agy_models missing {expected:?}; got {pairs:?}"
        );
    }
}

#[test]
fn studio_content_dir_discovery_vinegar_windows_and_env_override() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("content-dir-discovery");

    // 1. Pure Vinegar scanner selects newest existing `version-*/content` and ignores missing content dirs.
    let vroot = dir.join("vinegar");
    let v_old = vroot.join("data/vinegar/versions/version-aaa111/content");
    let v_new = vroot.join("data/vinegar/versions/version-zzz999/content");
    let v_missing_content = vroot.join("data/vinegar/versions/version-zzz9999");
    std::fs::create_dir_all(&v_old).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    std::fs::create_dir_all(&v_new).unwrap();
    std::fs::create_dir_all(&v_missing_content).unwrap();
    assert_eq!(
        find_vinegar_studio_content_dir(&vroot),
        Some(v_new.clone())
    );

    // 2. Pure Windows scanner finds `<LOCALAPPDATA>/Roblox/Versions/version-*/content`.
    let local_app = dir.join("localappdata");
    let win_ver = local_app.join("Roblox/Versions/version-win42");
    std::fs::create_dir_all(win_ver.join("content")).unwrap();
    std::fs::write(win_ver.join("RobloxStudioBeta.exe"), b"MZ").unwrap();
    assert_eq!(
        find_windows_studio_content_dir(&local_app),
        Some(win_ver.join("content"))
    );

    // 3. Env override `OPENRO_STUDIO_CONTENT_DIR` wins over everything.
    let override_dir = dir.join("override-content");
    let _env = EnvGuard::set(&[(
        "OPENRO_STUDIO_CONTENT_DIR",
        Some(override_dir.to_str().unwrap()),
    )]);
    assert_eq!(find_studio_content_dir(), Some(override_dir));

    // 4. Slug sanitization strips traversal and special chars safely.
    assert_eq!(sanitize_studio_content_slug("my_game-01"), "my_game-01");
    assert_eq!(sanitize_studio_content_slug("../../etc/passwd"), "etc-passwd");
    assert_eq!(sanitize_studio_content_slug("///"), "project");
}

#[test]
fn studio_content_bridge_staging_and_assetids_codegen_upgrade() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("content-bridge");
    let mock_content = dir.join("studio-content");
    let _env = EnvGuard::set(&[(
        "OPENRO_STUDIO_CONTENT_DIR",
        Some(mock_content.to_str().unwrap()),
    )]);
    let (port, pid, _data, proj, _shared) = boot_server(&dir, "sky-blade-arena");

    // Populate representative art/cards, art/ui, card-borders, icons, store, assets/images, replacements, and audio.
    std::fs::create_dir_all(proj.join("art/cards/CARD-01")).unwrap();
    std::fs::create_dir_all(proj.join("art/ui/obsidian-prism")).unwrap();
    std::fs::create_dir_all(proj.join("art/ui/card-borders")).unwrap();
    std::fs::create_dir_all(proj.join("art/ui/icons")).unwrap();
    std::fs::create_dir_all(proj.join("art/store")).unwrap();
    std::fs::create_dir_all(proj.join("assets/images/replacements")).unwrap();
    std::fs::create_dir_all(proj.join("assets/audio")).unwrap();

    std::fs::write(proj.join("art/cards/CARD-01/art.png"), make_png(512, 512)).unwrap();
    std::fs::write(proj.join("art/cards/CARD-01/icon.png"), make_png(256, 256)).unwrap();
    std::fs::write(proj.join("art/cards/CARD-01/full.png"), make_png(512, 768)).unwrap();
    std::fs::write(
        proj.join("art/ui/obsidian-prism/modal_panel.png"),
        make_png(256, 256),
    )
    .unwrap();
    std::fs::write(
        proj.join("art/ui/obsidian-prism/icon_summon.png"),
        make_png(128, 128),
    )
    .unwrap();
    std::fs::write(
        proj.join("art/ui/card-borders/border_mythic.png"),
        make_png(256, 384),
    )
    .unwrap();
    std::fs::write(proj.join("art/ui/icons/gems.png"), make_png(128, 128)).unwrap();
    std::fs::write(proj.join("art/store/hero_banner.png"), make_png(512, 256)).unwrap();
    std::fs::write(proj.join("assets/images/emblem.png"), make_png(128, 128)).unwrap();
    std::fs::write(
        proj.join("assets/images/replacements/emblem_clean.png"),
        make_png(128, 128),
    )
    .unwrap();
    std::fs::write(
        proj.join("assets/images/old_superseded.png"),
        make_png(64, 64),
    )
    .unwrap();
    std::fs::write(
        proj.join("assets/audio/ui_click.wav"),
        make_pcm_wav(44_100, 120, 0.7),
    )
    .unwrap();
    std::fs::write(proj.join("assets/audio/fanfare.ogg"), b"OggS-test").unwrap();

    std::fs::write(
        proj.join("art/ui/slices.json"),
        json!({
            "obsidian-prism/modal_panel": {
                "path": "art/ui/obsidian-prism/modal_panel.png",
                "slice": [32, 32, 224, 224]
            }
        })
        .to_string(),
    )
    .unwrap();

    #[cfg(unix)]
    {
        let outside = dir.join("outside-secret.png");
        std::fs::write(&outside, make_png(16, 16)).unwrap();
        std::os::unix::fs::symlink(&outside, proj.join("art/ui/evil_symlink.png")).unwrap();
    }

    // 1. Stage via HTTP endpoint -> copies all valid files, skips symlinks and *_superseded*.
    let (st1, stage1) = http_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/studio/stage-content"),
        Some(&json!({})),
    );
    assert_eq!(st1, 200, "stage-content failed: {stage1}");
    assert_eq!(stage1["ok"], true);
    assert_eq!(stage1["prefix"], "rbxasset://openro/sky-blade-arena/");
    let staged_first = stage1["staged_count"].as_u64().unwrap();
    assert_eq!(staged_first, 12, "unexpected staged files: {stage1}");
    assert_eq!(stage1["skipped_count"], 0);
    assert!(mock_content
        .join("openro/sky-blade-arena/art/cards/CARD-01/art.png")
        .is_file());
    assert!(mock_content
        .join("openro/sky-blade-arena/assets/audio/ui_click.wav")
        .is_file());
    assert!(!mock_content
        .join("openro/sky-blade-arena/art/ui/evil_symlink.png")
        .exists());
    assert!(!mock_content
        .join("openro/sky-blade-arena/assets/images/old_superseded.png")
        .exists());

    // 2. Second stage run skips all unchanged files by byte len + modified time.
    let (st2, stage2) = http_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/studio/stage-content"),
        Some(&json!({})),
    );
    assert_eq!(st2, 200);
    assert_eq!(stage2["staged_count"], 0, "expected 0 restaged: {stage2}");
    assert_eq!(stage2["skipped_count"], staged_first);

    // 3. assets_codegen with default `studio_local_fallback: true` emits rbxasset:// for unapproved assets
    //    across all 7 tables (byPath, byKey, cards, icons, audio, meshes, slices) and passes stylua --check.
    let cg1 = assets_codegen(&proj, &json!({})).expect("codegen local fallback");
    assert_eq!(cg1["ok"], true);
    assert_eq!(cg1["studio_local_fallback"], true);
    assert_eq!(cg1["studio_project_slug"], "sky-blade-arena");
    assert!(cg1["local_fallback_count"].as_u64().unwrap() >= 12);
    let luau1 = std::fs::read_to_string(proj.join("src/shared/AssetIds.luau")).unwrap();
    assert!(
        luau1.contains("AssetIds.cards = {"),
        "missing AssetIds.cards:\n{luau1}"
    );
    assert!(
        luau1.contains("[\"CARD-01\"] = {"),
        "missing CARD-01 entry:\n{luau1}"
    );
    assert!(
        luau1.contains("art = \"rbxasset://openro/sky-blade-arena/art/cards/CARD-01/art.png\""),
        "missing CARD-01 art fallback:\n{luau1}"
    );
    assert!(
        luau1.contains("icon = \"rbxasset://openro/sky-blade-arena/art/cards/CARD-01/icon.png\""),
        "missing CARD-01 icon fallback:\n{luau1}"
    );
    assert!(
        luau1.contains("full = \"rbxasset://openro/sky-blade-arena/art/cards/CARD-01/full.png\""),
        "missing CARD-01 full fallback:\n{luau1}"
    );
    assert!(
        luau1.contains("AssetIds.icons = {")
            && luau1.contains("[\"gems\"] = \"rbxasset://openro/sky-blade-arena/art/ui/icons/gems.png\"")
            && luau1.contains("[\"icon_summon\"] = \"rbxasset://openro/sky-blade-arena/art/ui/obsidian-prism/icon_summon.png\""),
        "missing icons table:\n{luau1}"
    );
    assert!(
        luau1.contains("AssetIds.audio = {")
            && luau1.contains("[\"ui_click\"] = \"rbxasset://openro/sky-blade-arena/assets/audio/ui_click.wav\"")
            && luau1.contains("[\"fanfare\"] = \"rbxasset://openro/sky-blade-arena/assets/audio/fanfare.ogg\""),
        "missing audio table:\n{luau1}"
    );
    assert!(
        luau1.contains("[\"obsidian-prism/modal_panel\"]")
            && luau1.contains("[\"card-borders/border_mythic\"]"),
        "missing slices entries:\n{luau1}"
    );
    assert_stylua_clean(&proj, "src/shared/AssetIds.luau");

    // 4. Approve CARD-01/art.png + ui_click.wav in assets.lock.json while leaving CARD-01/icon.png pending:
    //    approved entries upgrade cleanly to rbxassetid:// while pending stays rbxasset://openro/...!
    std::fs::write(
        proj.join("assets.lock.json"),
        json!({
            "version": 1,
            "assets": {
                "art/cards/CARD-01/art.png": {
                    "assetId": "501000",
                    "imageAssetId": "501001",
                    "assetType": "Decal",
                    "state": "approved"
                },
                "art/cards/CARD-01/icon.png": {
                    "assetId": "501002",
                    "assetType": "Image",
                    "state": "pending"
                },
                "assets/audio/ui_click.wav": {
                    "assetId": "601001",
                    "assetType": "Audio",
                    "state": "approved"
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let cg2 = assets_codegen(&proj, &json!({})).expect("codegen upgraded");
    assert_eq!(cg2["ok"], true);
    let luau2 = std::fs::read_to_string(proj.join("src/shared/AssetIds.luau")).unwrap();
    assert!(
        luau2.contains("art = \"rbxassetid://501001\""),
        "approved card art did not upgrade to rbxassetid://:\n{luau2}"
    );
    assert!(
        luau2.contains("icon = \"rbxasset://openro/sky-blade-arena/art/cards/CARD-01/icon.png\""),
        "pending card icon should keep rbxasset:// fallback:\n{luau2}"
    );
    assert!(
        luau2.contains("[\"ui_click\"] = \"rbxassetid://601001\""),
        "approved audio did not upgrade to rbxassetid://:\n{luau2}"
    );
    assert_stylua_clean(&proj, "src/shared/AssetIds.luau");

    // 5. With `studio_local_fallback: false`, zero rbxasset:// entries appear and stylua stays clean.
    let cg3 = assets_codegen(&proj, &json!({ "studio_local_fallback": false })).expect("codegen cloud-only");
    assert_eq!(cg3["local_fallback_count"], 0);
    let luau3 = std::fs::read_to_string(proj.join("src/shared/AssetIds.luau")).unwrap();
    assert!(
        !luau3.contains("rbxasset://openro/"),
        "unexpected local fallback when studio_local_fallback=false:\n{luau3}"
    );
    assert_stylua_clean(&proj, "src/shared/AssetIds.luau");
}

#[test]
fn cua_window_launch_auto_stages_and_stop_resets_studio() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("cua-window");
    let mock_content = dir.join("studio-content");
    let _env = EnvGuard::set(&[(
        "OPENRO_STUDIO_CONTENT_DIR",
        Some(mock_content.to_str().unwrap()),
    )]);
    let (port, _pid, _data, proj, _shared) = boot_server(&dir, "cua-auto-stage");

    std::fs::create_dir_all(proj.join("art/cards/C1")).unwrap();
    std::fs::create_dir_all(proj.join("build")).unwrap();
    std::fs::write(proj.join("art/cards/C1/art.png"), make_png(64, 64)).unwrap();
    std::fs::write(
        proj.join("build/CuaPlace.rbxlx"),
        "<roblox version=\"4\"></roblox>",
    )
    .unwrap();

    // Traversal in place_file rejected with 400
    let (st_bad, _) = http_json(
        port,
        "POST",
        "/api/studio/cua/window",
        Some(&json!({ "action": "launch", "place_file": "../escape.rbxlx" })),
    );
    assert_eq!(st_bad, 400);

    // Launch via CUA window with mock_studio -> auto-stages project content into Studio content dir!
    let mock_bin = env!("CARGO_BIN_EXE_mock_studio");
    let (st_launch, launched) = http_json(
        port,
        "POST",
        "/api/studio/cua/window",
        Some(&json!({ "action": "launch", "command": mock_bin })),
    );
    assert_eq!(st_launch, 200, "cua window launch failed: {launched}");
    assert_eq!(launched["connected"], true);
    assert!(
        mock_content
            .join("openro/cua-auto-stage/art/cards/C1/art.png")
            .is_file(),
        "expected launch to auto-stage project content into Studio content dir"
    );

    // Stop via CUA window -> cleanly tears down and resets to local_place
    let (st_stop, stopped) = http_json(
        port,
        "POST",
        "/api/studio/cua/window",
        Some(&json!({ "action": "stop" })),
    );
    assert_eq!(st_stop, 200, "cua window stop failed: {stopped}");
    assert_eq!(stopped["ok"], true);
    assert_eq!(stopped["action"], "stop");
    assert_eq!(stopped["stopped"], true);
    assert_eq!(stopped["running"], false);
    assert_eq!(stopped["connected"], false);
}

#[cfg(unix)]
fn write_mock_verify_mcp_script(dir: &Path, state_file: &Path, log_file: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script_path = dir.join("mock_verify_mcp.py");
    let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAACklEQVR4nGMAAQAABQABDQottAAAAABJRU5ErkJggg==";
    let script = format!(
        r#"#!/usr/bin/env python3
import json, os, sys

STATE_FILE = {state_file:?}
LOG_FILE = {log_file:?}
PNG_B64 = {png_b64:?}

def load_cfg():
    try:
        with open(STATE_FILE, "r") as f:
            return json.load(f)
    except Exception:
        return {{}}

def log_call(name, args):
    with open(LOG_FILE, "a") as f:
        f.write(json.dumps({{"tool": name, "args": args}}) + "\n")

for raw in sys.stdin:
    line = raw.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
    except Exception:
        continue
    req_id = req.get("id")
    method = req.get("method", "")
    if method == "initialize":
        res = {{
            "protocolVersion": "2024-11-05",
            "capabilities": {{"tools": {{}}}},
            "serverInfo": {{"name": "mock-verify-mcp", "version": "1.0.0"}}
        }}
    elif method == "tools/list":
        res = {{
            "tools": [
                {{"name": "list_roblox_studios", "description": "list"}},
                {{"name": "get_studio_state", "description": "state"}},
                {{"name": "start_stop_play", "description": "play"}},
                {{"name": "execute_luau", "description": "luau"}},
                {{"name": "screen_capture", "description": "capture"}},
                {{"name": "get_console_output", "description": "console"}}
            ]
        }}
    elif method == "tools/call":
        name = req.get("params", {{}}).get("name", "")
        args = req.get("params", {{}}).get("arguments", {{}})
        log_call(name, args)
        cfg = load_cfg()
        if name == "list_roblox_studios":
            res = {{"content": [{{"type": "text", "text": json.dumps({{"studios": [{{"studio_id": "s-verify-1", "place_name": "VerifyPlace", "datamodel_type": "Edit"}}]}})}},], "isError": False}}
        elif name == "get_studio_state":
            res = {{"content": [{{"type": "text", "text": json.dumps({{"active_datamodel": "Edit", "mode": "edit", "place_open": True}})}}], "isError": False}}
        elif name == "start_stop_play":
            res = {{"content": [{{"type": "text", "text": json.dumps({{"ok": True, "mode": args.get("mode")}})}}], "isError": False}}
        elif name == "screen_capture":
            res = {{"content": [{{"type": "image", "data": PNG_B64, "mimeType": "image/png"}}], "isError": False}}
        elif name == "get_console_output":
            res = {{"content": [{{"type": "text", "text": "[]"}}], "isError": False}}
        elif name == "execute_luau":
            code = args.get("code", "")
            if "-- openro:v132:verify_device" in code:
                payload = cfg.get("device_result", {{
                    "ok": True,
                    "profile": "phone_844x390",
                    "viewport": {{"width": 844, "height": 390, "target_width": 844, "target_height": 390}},
                    "checked_buttons": 6,
                    "violations": []
                }})
                text = "OPENRO_V011:" + json.dumps(payload)
            elif "-- openro:v132:verify_audio" in code:
                payload = cfg.get("audio_result", {{
                    "ok": True,
                    "studio_sounds_checked": 2,
                    "studio_sounds_loaded": 2,
                    "sounds": [
                        {{"name": "ui_click", "path": "SoundService.ui_click", "sound_id": "rbxasset://openro/p/assets/audio/ui_click.wav", "is_loaded": True, "time_length": 0.25, "is_playing": False}},
                        {{"name": "summon_roll", "path": "SoundService.summon_roll", "sound_id": "rbxassetid://123456", "is_loaded": True, "time_length": 1.4, "is_playing": False}}
                    ]
                }})
                text = "OPENRO_V011:" + json.dumps(payload)
            else:
                text = "OPENRO_V011:" + json.dumps({{"ok": True}})
            res = {{"content": [{{"type": "text", "text": text}}], "isError": False}}
        else:
            res = {{"content": [{{"type": "text", "text": "unknown tool"}}], "isError": True}}
    else:
        if req_id is None:
            continue
        res = None
    if req_id is not None:
        sys.stdout.write(json.dumps({{"jsonrpc": "2.0", "id": req_id, "result": res}}) + "\n")
        sys.stdout.flush()
"#,
        state_file = state_file.to_string_lossy(),
        log_file = log_file.to_string_lossy(),
        png_b64 = png_b64,
    );
    std::fs::write(&script_path, script).unwrap();
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();
    script_path
}

#[test]
fn studio_verify_device_and_audio_fail_closed_and_live_mcp_e2e() {
    let dir = scratch("verify-device-audio");
    let (port, pid, _data, proj, _shared) = boot_server(&dir, "verify-proj");

    std::fs::create_dir_all(proj.join("assets/audio")).unwrap();
    // Clean WAV: 44100 Hz, 200ms, amplitude 0.7 (~ -3.1 dBFS < -0.5 dBFS)
    std::fs::write(
        proj.join("assets/audio/good_click.wav"),
        make_pcm_wav(44_100, 200, 0.7),
    )
    .unwrap();
    // Bad WAV: wrong sample rate (22050 Hz) + clipping amplitude (0.99 -> -0.09 dBFS >= -0.5 dBFS)
    std::fs::write(
        proj.join("assets/audio/bad_clip.wav"),
        make_pcm_wav(22_050, 150, 0.99),
    )
    .unwrap();

    // 1. Disconnected Studio MUST fail closed on verify-device (NEVER fake offline results!)
    let (st_dev_off, dev_off) = http_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/studio/verify-device"),
        Some(&json!({ "profile": "phone_844x390" })),
    );
    assert_eq!(st_dev_off, 200, "verify-device disconnected: {dev_off}");
    assert_eq!(dev_off["mode"], "studio_disconnected");
    assert_eq!(dev_off["passed"], false);
    assert_eq!(dev_off["live_studio_connected"], false);

    // Invalid profile -> 400
    let (st_bad_prof, _) = http_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/studio/verify-device"),
        Some(&json!({ "profile": "smartwatch_120x120" })),
    );
    assert_eq!(st_bad_prof, 400);

    // 2. Disconnected Studio MUST run real offline WAV checks AND fail closed (`live_studio_connected: false, passed: false`)
    let (st_aud_off, aud_off) = http_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/studio/verify-audio"),
        Some(&json!({})),
    );
    assert_eq!(st_aud_off, 200, "verify-audio disconnected: {aud_off}");
    assert_eq!(aud_off["live_studio_connected"], false);
    assert_eq!(aud_off["passed"], false);
    let wavs = aud_off["offline_wav_checks"].as_array().unwrap();
    assert_eq!(wavs.len(), 2);
    let good_row = wavs
        .iter()
        .find(|w| w["path"] == "assets/audio/good_click.wav")
        .unwrap();
    assert_eq!(good_row["passed"], true, "good wav should pass: {good_row}");
    assert_eq!(good_row["sample_rate"], 44100);
    let bad_row = wavs
        .iter()
        .find(|w| w["path"] == "assets/audio/bad_clip.wav")
        .unwrap();
    assert_eq!(bad_row["passed"], false, "bad wav should fail: {bad_row}");
    assert!(bad_row["issues"].as_array().unwrap().len() >= 2);

    // Remove bad WAV before testing live passing audio verification.
    std::fs::remove_file(proj.join("assets/audio/bad_clip.wav")).unwrap();

    #[cfg(unix)]
    {
        let state_file = dir.join("mcp_state.json");
        let log_file = dir.join("mcp_calls.jsonl");
        std::fs::write(&state_file, "{}").unwrap();
        let mcp_script = write_mock_verify_mcp_script(&dir, &state_file, &log_file);

        // Attach mock live Studio MCP
        let (st_conn, conn) = http_json(
            port,
            "POST",
            "/api/studio/embed/launch",
            Some(&json!({ "command": mcp_script.to_str().unwrap() })),
        );
        assert_eq!(st_conn, 200, "mock MCP launch failed: {conn}");
        assert_eq!(conn["connected"], true);

        // 3. Passing live phone_844x390 device verification + screenshot saved to build/reports/
        let (st_dev_ok, dev_ok) = http_json(
            port,
            "POST",
            &format!("/api/projects/{pid}/studio/verify-device"),
            Some(&json!({ "profile": "phone_844x390", "settle_ms": 0 })),
        );
        assert_eq!(st_dev_ok, 200, "live verify-device failed: {dev_ok}");
        assert_eq!(dev_ok["mode"], "live_studio");
        assert_eq!(dev_ok["passed"], true);
        assert_eq!(dev_ok["checked_buttons"], 6);
        assert_eq!(
            dev_ok["screenshot_path"],
            "build/reports/studio-device-phone_844x390.png"
        );
        assert!(proj
            .join("build/reports/studio-device-phone_844x390.png")
            .is_file());

        // 4. Failing live device verification (small button violation)
        std::fs::write(
            &state_file,
            json!({
                "device_result": {
                    "ok": true,
                    "profile": "phone_844x390",
                    "viewport": { "width": 844, "height": 390 },
                    "checked_buttons": 4,
                    "violations": [{
                        "code": "button_too_small",
                        "severity": "error",
                        "path": "PlayerGui.Hud.SmallBtn",
                        "message": "GuiButton is 32x32 (< 44x44)"
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        let (st_dev_fail, dev_fail) = http_json(
            port,
            "POST",
            &format!("/api/projects/{pid}/studio/verify-device"),
            Some(&json!({ "profile": "phone_844x390", "capture_screenshot": false, "settle_ms": 0 })),
        );
        assert_eq!(st_dev_fail, 200);
        assert_eq!(dev_fail["mode"], "live_studio");
        assert_eq!(dev_fail["passed"], false);
        assert_eq!(dev_fail["violations"].as_array().unwrap().len(), 1);

        // 5. Passing live audio verification + report written to ops/audio/studio-audio-verify.json
        let (st_aud_ok, aud_ok) = http_json(
            port,
            "POST",
            &format!("/api/projects/{pid}/studio/verify-audio"),
            Some(&json!({ "settle_ms": 0 })),
        );
        assert_eq!(st_aud_ok, 200, "live verify-audio failed: {aud_ok}");
        assert_eq!(aud_ok["live_studio_connected"], true);
        assert_eq!(aud_ok["studio_sounds_checked"], 2);
        assert_eq!(aud_ok["studio_sounds_loaded"], 2);
        assert_eq!(aud_ok["passed"], true);
        let report_file = proj.join("ops/audio/studio-audio-verify.json");
        assert!(report_file.is_file(), "missing ops/audio/studio-audio-verify.json");
        let report_doc: Value =
            serde_json::from_str(&std::fs::read_to_string(&report_file).unwrap()).unwrap();
        assert_eq!(report_doc["passed"], true);
        assert_eq!(report_doc["studio_sounds_loaded"], 2);

        // 6. Failing live audio verification when a Sound fails IsLoaded
        std::fs::write(
            &state_file,
            json!({
                "audio_result": {
                    "ok": true,
                    "studio_sounds_checked": 2,
                    "studio_sounds_loaded": 1,
                    "sounds": [
                        { "name": "ok_sfx", "is_loaded": true, "time_length": 0.3, "is_playing": false },
                        { "name": "broken_sfx", "is_loaded": false, "time_length": 0.0, "is_playing": false }
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();
        let (st_aud_fail, aud_fail) = http_json(
            port,
            "POST",
            &format!("/api/projects/{pid}/studio/verify-audio"),
            Some(&json!({ "settle_ms": 0 })),
        );
        assert_eq!(st_aud_fail, 200);
        assert_eq!(aud_fail["live_studio_connected"], true);
        assert_eq!(aud_fail["passed"], false);

        // Verify start_play & stop lifecycle calls happened in MCP log
        let calls = std::fs::read_to_string(&log_file).unwrap();
        assert!(calls.contains("\"start_play\""), "expected start_play: {calls}");
        assert!(calls.contains("\"stop\""), "expected stop: {calls}");
    }
}

#[derive(Default)]
struct MockOpenCloudState {
    uploaded_filenames: Vec<String>,
    uploaded_payloads: Vec<Vec<u8>>,
    polled_asset_ids: Vec<String>,
    published_bodies: Vec<String>,
    next_id: u64,
}

fn spawn_mock_opencloud() -> (String, Arc<Mutex<MockOpenCloudState>>) {
    let state = Arc::new(Mutex::new(MockOpenCloudState {
        next_id: 7000,
        ..Default::default()
    }));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let st = Arc::clone(&state);

    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut s = stream;
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let mut header_end = None;
            let mut content_len = 0usize;
            while let Ok(n) = s.read(&mut tmp) {
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if header_end.is_none() {
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(pos + 4);
                        let head_str = String::from_utf8_lossy(&buf[..pos]);
                        for line in head_str.lines() {
                            if let Some((k, v)) = line.split_once(':') {
                                if k.eq_ignore_ascii_case("content-length") {
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
            let Some(he) = header_end else { continue };
            let head = String::from_utf8_lossy(&buf[..he]).to_string();
            let req_line = head.lines().next().unwrap_or("");
            let mut parts = req_line.split_whitespace();
            let method = parts.next().unwrap_or("GET");
            let full_path = parts.next().unwrap_or("/");
            let body = &buf[he..buf.len().min(he + content_len)];

            let (status, ct, resp_body): (&str, &str, String) =
                if method == "POST" && full_path == "/assets/v1/assets" {
                    let body_lossy = String::from_utf8_lossy(body);
                    let fname = body_lossy
                        .split("filename=\"")
                        .nth(1)
                        .and_then(|r| r.split('"').next())
                        .unwrap_or("unknown")
                        .to_string();
                    let mut g = st.lock().unwrap();
                    g.next_id += 1;
                    let id = g.next_id;
                    g.uploaded_filenames.push(fname);
                    g.uploaded_payloads.push(body.to_vec());
                    (
                        "200 OK",
                        "application/json",
                        json!({
                            "path": format!("operations/op-{id}"),
                            "done": true,
                            "response": {
                                "assetId": id.to_string(),
                                "moderationResult": {
                                    "moderationState": "MODERATION_STATE_REVIEWING"
                                }
                            }
                        })
                        .to_string(),
                    )
                } else if method == "GET" && full_path.starts_with("/assets/v1/assets/") {
                    let asset_id = full_path
                        .trim_start_matches("/assets/v1/assets/")
                        .split('?')
                        .next()
                        .unwrap_or("")
                        .to_string();
                    st.lock().unwrap().polled_asset_ids.push(asset_id.clone());
                    let (mod_state, asset_type) = match asset_id.as_str() {
                        "5001" => ("MODERATION_STATE_APPROVED", "Decal"),
                        "5002" => ("MODERATION_STATE_REJECTED", "Audio"),
                        _ => ("MODERATION_STATE_REVIEWING", "Image"),
                    };
                    (
                        "200 OK",
                        "application/json",
                        json!({
                            "assetId": asset_id,
                            "assetType": asset_type,
                            "moderationResult": { "moderationState": mod_state }
                        })
                        .to_string(),
                    )
                } else if method == "GET" && full_path.starts_with("/v1/asset/?id=") {
                    let decal_id = full_path.trim_start_matches("/v1/asset/?id=");
                    let img_id = format!("90{decal_id}");
                    (
                        "200 OK",
                        "application/xml",
                        format!(
                            "<roblox version=\"4\"><Item class=\"Decal\"><Properties><Content name=\"Texture\"><url>http://www.roblox.com/asset/?id={img_id}</url></Content></Properties></Item></roblox>"
                        ),
                    )
                } else if method == "POST" && full_path.contains("/places/") && full_path.contains("/versions") {
                    let text = String::from_utf8_lossy(body).to_string();
                    st.lock().unwrap().published_bodies.push(text);
                    (
                        "200 OK",
                        "application/json",
                        json!({ "versionNumber": 42 }).to_string(),
                    )
                } else {
                    ("404 Not Found", "application/json", json!({"error": "not found"}).to_string())
                };

            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp_body}",
                resp_body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });

    (format!("http://127.0.0.1:{}", addr.port()), state)
}

#[test]
fn opencloud_moderation_poll_upload_filters_cap_and_rojo_publish_e2e() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("opencloud-gap86");
    let (mock_base, mock_state) = spawn_mock_opencloud();
    let _env = EnvGuard::set(&[
        ("OPENRO_OPENCLOUD_BASE", Some(&mock_base)),
        ("OPENRO_ASSETDELIVERY_BASE", Some(&mock_base)),
        ("OPENRO_OPENCLOUD_POLL_MS", Some("5")),
    ]);

    let (port, pid, _data, proj, _shared) = boot_server(&dir, "gap86-proj");

    // Configure Open Cloud key & creator
    let (st_k, _) = http_json(
        port,
        "POST",
        "/api/opencloud/key",
        Some(&json!({ "api_key": "test_gap86_opencloud_key_123456" })),
    );
    assert_eq!(st_k, 200);
    let (st_c, _) = http_json(
        port,
        "POST",
        "/api/opencloud/settings",
        Some(&json!({ "creator": { "userId": 12345678 } })),
    );
    assert_eq!(st_c, 200);

    // 1. assets_upload: skip *_superseded* & assets/images/incoming/**, honor assets/images/manifest.json local_file,
    //    and enforce max_audio_uploads cap!
    std::fs::create_dir_all(proj.join("assets/images/replacements")).unwrap();
    std::fs::create_dir_all(proj.join("assets/images/incoming")).unwrap();
    std::fs::create_dir_all(proj.join("assets/audio")).unwrap();

    let dirty_bytes = b"\x89PNG\r\n\x1a\nDIRTY_PLACEHOLDER_BYTES".to_vec();
    let clean_bytes = b"\x89PNG\r\n\x1a\nCLEAN_REPLACEMENT_BYTES".to_vec();
    std::fs::write(proj.join("assets/images/hero_card.png"), &dirty_bytes).unwrap();
    std::fs::write(
        proj.join("assets/images/replacements/hero_card_clean.png"),
        &clean_bytes,
    )
    .unwrap();
    std::fs::write(
        proj.join("assets/images/manifest.json"),
        json!({
            "assets/images/hero_card.png": {
                "local_file": "assets/images/replacements/hero_card_clean.png"
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(proj.join("assets/images/hero_superseded.png"), &dirty_bytes).unwrap();
    std::fs::write(proj.join("assets/images/incoming/draft.png"), &dirty_bytes).unwrap();

    for i in 1..=4 {
        std::fs::write(
            proj.join(format!("assets/audio/cue_{i}.ogg")),
            format!("OggS-audio-cue-{i}").as_bytes(),
        )
        .unwrap();
    }

    let (st_up, up) = http_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/assets/upload"),
        Some(&json!({ "max_audio_uploads": 2 })),
    );
    assert_eq!(st_up, 200, "upload failed: {up}");
    assert_eq!(up["summary"]["uploaded"], 3, "expected 1 clean image + 2 capped audio: {up}");
    assert_eq!(up["summary"]["audio_uploads_used"], 2);
    assert_eq!(up["summary"]["max_audio_uploads"], 2);

    let uploaded_names = mock_state.lock().unwrap().uploaded_filenames.clone();
    assert!(
        uploaded_names.iter().any(|n| n == "hero_card_clean.png"),
        "expected clean replacement file to be uploaded: {uploaded_names:?}"
    );
    assert!(
        !uploaded_names.iter().any(|n| n.contains("superseded") || n == "draft.png" || n == "hero_card.png"),
        "dirty/superseded/incoming file must never be uploaded: {uploaded_names:?}"
    );
    let uploaded_audio_count = uploaded_names.iter().filter(|n| n.ends_with(".ogg")).count();
    assert_eq!(uploaded_audio_count, 2, "max_audio_uploads cap violated: {uploaded_names:?}");

    // 2. assets_moderation_poll: polls pending/reviewing/unknown entries, resolves Decal imageAssetId,
    //    updates assets.lock.json, and auto-regenerates stylua-clean AssetIds.luau!
    std::fs::create_dir_all(proj.join("art/cards/HERO-1")).unwrap();
    std::fs::write(proj.join("art/cards/HERO-1/art.png"), make_png(256, 256)).unwrap();
    std::fs::write(
        proj.join("assets.lock.json"),
        json!({
            "version": 1,
            "assets": {
                "art/cards/HERO-1/art.png": {
                    "assetId": "5001",
                    "assetType": "Decal",
                    "state": "pending",
                    "sha256": "aaa"
                },
                "assets/audio/cue_1.ogg": {
                    "assetId": "5002",
                    "assetType": "Audio",
                    "state": "reviewing",
                    "sha256": "bbb"
                },
                "assets/images/replacements/hero_card_clean.png": {
                    "assetId": "5003",
                    "assetType": "Image",
                    "state": "unknown",
                    "sha256": "ccc"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let (st_mod, mod_res) = http_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/assets/moderation"),
        Some(&json!({})),
    );
    assert_eq!(st_mod, 200, "moderation poll failed: {mod_res}");
    assert_eq!(mod_res["ok"], true);
    assert_eq!(mod_res["polled"], 3);
    assert_eq!(mod_res["approved"], 1);
    assert_eq!(mod_res["rejected"], 1);
    assert_eq!(mod_res["pending"], 1);
    assert!(mod_res["asset_ids_luau"]["ok"] == json!(true));

    let updated_lock: Value =
        serde_json::from_str(&std::fs::read_to_string(proj.join("assets.lock.json")).unwrap())
            .unwrap();
    assert_eq!(updated_lock["assets"]["art/cards/HERO-1/art.png"]["state"], "approved");
    assert_eq!(
        updated_lock["assets"]["art/cards/HERO-1/art.png"]["imageAssetId"],
        "905001"
    );
    assert_eq!(updated_lock["assets"]["assets/audio/cue_1.ogg"]["state"], "rejected");
    assert_eq!(
        updated_lock["assets"]["assets/images/replacements/hero_card_clean.png"]["state"],
        "pending"
    );

    let asset_luau = std::fs::read_to_string(proj.join("src/shared/AssetIds.luau")).unwrap();
    assert!(
        asset_luau.contains("art = \"rbxassetid://905001\""),
        "auto-codegen should emit newly approved Decal imageAssetId:\n{asset_luau}"
    );
    assert!(
        !asset_luau.contains("rbxassetid://5002"),
        "rejected audio asset must not appear in AssetIds.luau:\n{asset_luau}"
    );
    assert_stylua_clean(&proj, "src/shared/AssetIds.luau");

    // 3. Rojo root `build/*.rbxlx` publish works without `build/builds/*.json` AND overrides older build records!
    std::fs::create_dir_all(proj.join("build")).unwrap();
    let rojo_xml = "<roblox version=\"4\"><!-- ROJO_ROOT_BUILD --></roblox>";
    std::fs::write(proj.join("build/UntitledAnimeCardGame.rbxlx"), rojo_xml).unwrap();

    let (st_pub1, pub1) = http_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/publish"),
        Some(&json!({ "versionType": "Saved" })),
    );
    assert_eq!(st_pub1, 200, "Rojo root publish failed: {pub1}");
    assert_eq!(pub1["ok"], true);
    assert_eq!(pub1["versionNumber"], 42);
    assert_eq!(pub1["build_id"], "UntitledAnimeCardGame");
    assert_eq!(
        mock_state.lock().unwrap().published_bodies.last().map(String::as_str),
        Some(rojo_xml)
    );

    // Older `build/builds/old.json` + newer `build/RojoFresh.rbxlx` -> picks `RojoFresh`!
    std::fs::create_dir_all(proj.join("build/builds")).unwrap();
    std::fs::write(
        proj.join("build/old_place.rbxlx"),
        "<roblox version=\"4\"><!-- OLD_BUILD --></roblox>",
    )
    .unwrap();
    std::fs::write(
        proj.join("build/builds/b-old.json"),
        json!({ "build_id": "b-old", "path": "build/old_place.rbxlx", "at": 1_700_000_000_000u64 })
            .to_string(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(25));
    let fresh_xml = "<roblox version=\"4\"><!-- ROJO_FRESH_BUILD --></roblox>";
    std::fs::write(proj.join("build/RojoFresh.rbxlx"), fresh_xml).unwrap();

    let (st_pub2, pub2) = http_json(
        port,
        "POST",
        &format!("/api/projects/{pid}/publish"),
        Some(&json!({ "versionType": "Saved" })),
    );
    assert_eq!(st_pub2, 200, "Rojo newer publish failed: {pub2}");
    assert_eq!(pub2["build_id"], "RojoFresh");
    assert_eq!(
        mock_state.lock().unwrap().published_bodies.last().map(String::as_str),
        Some(fresh_xml)
    );
}

#[test]
fn agent_hooks_and_teamwork_prompts_wire_all_studio_autonomous_tools() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("hooks-prompts");
    let mock_content = dir.join("studio-content");
    let _env = EnvGuard::set(&[(
        "OPENRO_STUDIO_CONTENT_DIR",
        Some(mock_content.to_str().unwrap()),
    )]);
    let (_port, _pid, _data, proj, shared) = boot_server(&dir, "hooks-proj");

    // 1. Prompt documentation across Agent & Teamwork roles
    let sp = system_prompt();
    let psp = project_system_prompt();
    let coord_p = teamwork_role_prompt_preview("coordinator", "ship", &proj);
    let ver_p = teamwork_role_prompt_preview("verifier", "ship", &proj);
    let work_p = teamwork_role_prompt_preview("worker", "ship", &proj);

    for tool in [
        "studio_stage_content",
        "studio_verify_device",
        "studio_verify_audio",
        "assets_moderation_poll",
    ] {
        assert!(sp.contains(tool), "system_prompt missing {tool}");
        assert!(psp.contains(tool), "project_system_prompt missing {tool}");
        assert!(coord_p.contains(tool), "coordinator prompt missing {tool}");
        assert!(work_p.contains(tool), "worker prompt missing {tool}");
    }
    assert!(ver_p.contains("studio_verify_device"));
    assert!(ver_p.contains("studio_verify_audio"));
    assert!(sp.contains("status|launch|attach|transfer|stop|pause|resume"));
    assert!(work_p.contains("status|launch|attach|transfer|stop|pause|resume"));

    // 2. Wire hooks via ensure_agent_hooks and invoke each hook cleanly
    std::fs::create_dir_all(proj.join("art/store")).unwrap();
    std::fs::write(proj.join("art/store/icon.png"), make_png(64, 64)).unwrap();

    let mut agent = Agent::new(
        ModelClient::new("http://127.0.0.1:1/v1".into(), "m".into(), None),
        None,
    );
    ensure_agent_hooks(&shared, &mut agent);

    let stage_res = agent
        .hooks
        .studio_stage_content
        .as_ref()
        .expect("studio_stage_content hook")(&json!({}))
        .expect("stage hook ok");
    assert_eq!(stage_res["ok"], true);
    assert_eq!(stage_res["staged_count"], 1);

    let dev_res = agent
        .hooks
        .studio_verify_device
        .as_ref()
        .expect("studio_verify_device hook")(&json!({ "profile": "phone_844x390" }))
        .expect("device hook ok");
    assert_eq!(dev_res["mode"], "studio_disconnected");
    assert_eq!(dev_res["passed"], false);

    let aud_res = agent
        .hooks
        .studio_verify_audio
        .as_ref()
        .expect("studio_verify_audio hook")(&json!({}))
        .expect("audio hook ok");
    assert_eq!(aud_res["live_studio_connected"], false);
    assert_eq!(aud_res["passed"], false);

    // Empty lock file -> moderation poll succeeds with polled: 0
    let mod_res = agent
        .hooks
        .assets_moderation_poll
        .as_ref()
        .expect("assets_moderation_poll hook")(&json!({}))
        .expect("moderation hook ok");
    assert_eq!(mod_res["ok"], true);
    assert_eq!(mod_res["polled"], 0);
}
