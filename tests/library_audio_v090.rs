//! Integration tests for OpenRO v0.9.0 Built-In Developer Skill & Connection Library
//! (`src/library.rs`) and Pure-Rust Procedural `.wav` SFX Synthesizer (`src/audio_synth.rs`).

use openro_rs::audio_synth::{
    generate_audio_asset, read_audio_asset, sanitize_audio_filename, synthesize_wav, AUDIO_PRESETS,
    SAMPLE_RATE,
};
use openro_rs::library::{
    detect_blender, find_windows_blender_exe, generate_blender_mesh, read_mesh_asset,
    sanitize_mesh_filename, LibraryManager, BLENDER_PRESETS,
};
use serde_json::json;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_test_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "openro-v090-lib-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

#[test]
fn library_defaults_install_uninstall_configure_0600_and_test() {
    let tmp = temp_test_dir("mgr");
    let mut mgr = LibraryManager::open(&tmp);

    let initial = mgr.list_json();
    assert_eq!(initial["ok"], true);
    let packs = initial["packs"].as_array().expect("packs array");
    assert_eq!(packs.len(), 8);
    assert_eq!(initial["prompt_skills_count"], 3);

    let default_ids: Vec<&str> = packs
        .iter()
        .filter(|p| p["default_installed"] == true)
        .filter_map(|p| p["id"].as_str())
        .collect();
    assert_eq!(
        default_ids,
        vec![
            "roblox-studio-core",
            "blender-3d-pipeline",
            "anime-fan-art-knowledge"
        ]
    );
    let optional_ids: Vec<&str> = packs
        .iter()
        .filter(|p| p["default_installed"] == false)
        .filter_map(|p| p["id"].as_str())
        .collect();
    assert_eq!(
        optional_ids,
        vec![
            "image-gen-studio",
            "model-3d-gen",
            "video-gen-studio",
            "audio-sfx-gen",
            "rojo-open-cloud"
        ]
    );

    // Zero user-facing "Rune" jargon and zero prohibited video models anywhere in library or prompts
    let combined_initial = format!("{}\n{}", initial, mgr.active_skill_prompt());
    assert!(!combined_initial.contains("Rune"));
    assert!(!combined_initial.to_ascii_lowercase().contains("soul cinema"));
    assert!(!combined_initial.to_ascii_lowercase().contains("higgsfield"));

    // Uninstall of default pack -> 400; unknown pack -> 404
    let err_default = mgr
        .uninstall("roblox-studio-core")
        .expect_err("must refuse uninstalling default pack");
    assert_eq!(err_default.status, 400);
    let err_unknown = mgr
        .install("does-not-exist")
        .expect_err("must 404 unknown pack");
    assert_eq!(err_unknown.status, 404);

    // Install audio-sfx-gen & image-gen-studio -> dynamically expands active_tools & prompt
    let before_tools = mgr.active_tools().len();
    let inst_audio = mgr.install("audio-sfx-gen").expect("install audio-sfx-gen");
    assert_eq!(inst_audio["pack"]["installed"], true);
    assert!(mgr.active_tools().len() > before_tools);
    assert!(mgr
        .active_tools()
        .contains(&"generate_audio_sfx".to_string()));
    assert!(mgr.active_skill_prompt().contains("audio-sfx-gen"));

    mgr.install("image-gen-studio")
        .expect("install image-gen-studio");

    // Configure secret API key -> persisted 0600, masked in JSON, never echoed raw!
    let secret = "sk-openro-super-secret-987654321";
    let cfg_res = mgr
        .configure(
            "image-gen-studio",
            &json!({
                "provider": "gemini-nano-banana",
                "base_url": "https://generativelanguage.googleapis.com",
                "api_key": secret,
            }),
        )
        .expect("configure image-gen-studio");
    assert_eq!(cfg_res["pack"]["masked_key"], "sk-o...4321");
    assert_eq!(cfg_res["pack"]["configured"], true);
    assert!(
        !cfg_res.to_string().contains(secret),
        "raw API key must NEVER be echoed in JSON responses"
    );

    // Verify Unix 0600 permissions on library.json
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(tmp.join("library.json"))
            .expect("library.json metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "library.json must be mode 0600");
    }

    // Invalid URL rejected with 400
    assert_eq!(
        mgr.configure("image-gen-studio", &json!({ "base_url": "file:///etc/passwd" }))
            .expect_err("file:// base_url must fail")
            .status,
        400
    );

    // Persistence across fresh reload
    let mut reloaded = LibraryManager::open(&tmp);
    assert!(reloaded.is_installed("audio-sfx-gen"));
    assert!(reloaded.is_installed("image-gen-studio"));
    let reloaded_list = reloaded.list_json();
    assert!(!reloaded_list.to_string().contains(secret));

    // Connection tests across packs
    for id in [
        "roblox-studio-core",
        "blender-3d-pipeline",
        "anime-fan-art-knowledge",
        "audio-sfx-gen",
        "video-gen-studio",
        "rojo-open-cloud",
        "image-gen-studio",
    ] {
        let t = reloaded.test_pack(id).expect("test_pack");
        assert_eq!(t["ok"], true);
        assert_eq!(t["passed"], true, "pack {id} test failed: {t}");
    }

    // Clear secret key & uninstall optional pack
    let cleared = reloaded
        .configure("image-gen-studio", &json!({ "clear_api_key": true }))
        .expect("clear key");
    assert!(cleared["pack"]["masked_key"].is_null());

    let uninst = reloaded
        .uninstall("audio-sfx-gen")
        .expect("uninstall audio-sfx-gen");
    assert_eq!(uninst["ok"], true);
    assert!(!reloaded.is_installed("audio-sfx-gen"));

    // Windows Blender scanner pure unit test
    let win_pf = tmp.join("ProgramFiles");
    let b4 = win_pf.join("Blender Foundation").join("Blender 4.2");
    let b5 = win_pf.join("Blender Foundation").join("Blender 5.2");
    std::fs::create_dir_all(&b4).unwrap();
    std::fs::create_dir_all(&b5).unwrap();
    std::fs::write(b4.join("blender.exe"), b"4.2").unwrap();
    std::fs::write(b5.join("blender.exe"), b"5.2").unwrap();
    assert_eq!(find_windows_blender_exe(&win_pf), Some(b5.join("blender.exe")));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn audio_synth_generates_valid_pcm16_wav_for_all_six_presets() {
    let tmp = temp_test_dir("audio");

    for preset in AUDIO_PRESETS {
        let res = generate_audio_asset(
            &tmp,
            &json!({
                "preset": preset,
                "name": format!("acb_{preset}"),
            }),
        )
        .expect("generate audio wav");
        assert_eq!(res["ok"], true);
        assert_eq!(res["preset"], *preset);
        assert_eq!(res["sample_rate"], SAMPLE_RATE);
        let filename = res["filename"].as_str().expect("filename");
        assert!(filename.ends_with(".wav"));
        assert_eq!(res["wav_url"], format!("/api/assets/audio/{filename}"));

        let bytes = read_audio_asset(&tmp, filename).expect("read audio asset");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(&bytes[36..40], b"data");
        assert!(bytes.len() > 44);

        // Verify non-silent waveform energy
        let mut max_abs: i32 = 0;
        for chunk in bytes[44..].chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as i32;
            max_abs = max_abs.max(sample.abs());
        }
        assert!(
            max_abs > 1000,
            "preset {preset} waveform should be clearly audible (peak={max_abs})"
        );

        if *preset == "mythic_fanfare" {
            if let Ok(dump_path) = std::env::var("OPENRO_DUMP_WAV_PATH") {
                if !dump_path.trim().is_empty() {
                    std::fs::write(&dump_path, &bytes).expect("dump wav");
                }
            }
        }
    }

    // Reject unknown presets and traversal filenames
    assert!(synthesize_wav("not_a_real_sfx", None).is_err());
    for bad in ["../evil.wav", "/etc/passwd.wav", ".hidden.wav", "clip.mp3"] {
        assert!(sanitize_audio_filename(bad).is_err());
        assert!(read_audio_asset(&tmp, bad).is_err());
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn blender_mesh_generator_runs_real_headless_blender_and_procedural_fallback() {
    let tmp = temp_test_dir("meshes");

    // 1. Real Blender path when installed on this machine (Blender 5.2.2 LTS on host)
    let detected = detect_blender(None);
    let torii = generate_blender_mesh(
        &tmp,
        &json!({
            "preset": "torii_gate",
            "name": "ShrineTorii",
            "render_preview": true,
        }),
        None,
    )
    .expect("generate torii_gate");
    assert_eq!(torii["ok"], true);
    assert_eq!(torii["preset"], "torii_gate");
    assert_eq!(torii["filename"], "shrinetorii.obj");
    assert_eq!(torii["obj_url"], "/api/assets/meshes/shrinetorii.obj");
    assert_eq!(torii["preview_url"], "/api/assets/meshes/shrinetorii.png");
    assert!(torii["triangles"].as_u64().unwrap_or(0) >= 24);
    assert!(torii["vertices"].as_u64().unwrap_or(0) >= 16);
    if detected.detected {
        assert_eq!(
            torii["blender_used"], true,
            "Blender is installed at {:?} — expected real headless Blender obj_export: {torii}",
            detected.path
        );
    }

    let (obj_bytes, obj_mime) = read_mesh_asset(&tmp, "shrinetorii.obj").expect("read obj");
    assert!(obj_mime.starts_with("text/plain"));
    let obj_text = String::from_utf8_lossy(&obj_bytes);
    assert!(obj_text.contains("\nv ") || obj_text.starts_with("v "));
    assert!(obj_text.contains("\nf "));

    let (png_bytes, png_mime) = read_mesh_asset(&tmp, "shrinetorii.png").expect("read preview png");
    assert_eq!(png_mime, "image/png");
    assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    if let Ok(dump_dir) = std::env::var("OPENRO_DUMP_MESH_DIR") {
        if !dump_dir.trim().is_empty() {
            let p = PathBuf::from(dump_dir);
            let _ = std::fs::create_dir_all(&p);
            std::fs::write(p.join("shrinetorii.obj"), &obj_bytes).expect("dump obj");
            std::fs::write(p.join("shrinetorii.png"), &png_bytes).expect("dump png");
        }
    }

    // 2. Procedural fallback works cleanly across all 5 presets without invoking Blender
    for preset in BLENDER_PRESETS {
        let res = generate_blender_mesh(
            &tmp,
            &json!({
                "preset": preset,
                "name": format!("fallback_{preset}"),
                "use_fallback": true,
                "render_preview": true,
            }),
            None,
        )
        .expect("procedural fallback mesh");
        assert_eq!(res["ok"], true);
        assert_eq!(res["blender_used"], false);
        assert!(res["triangles"].as_u64().unwrap_or(0) >= 36);
        let preview_name = res["preview_filename"].as_str().unwrap();
        let (pbytes, _) = read_mesh_asset(&tmp, preview_name).expect("read fallback preview");
        assert!(pbytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    // 3. Traversal & bad extensions blocked cleanly
    for bad in ["../etc/passwd.obj", "/abs.obj", ".dot.png", "script.py"] {
        assert!(sanitize_mesh_filename(bad).is_err());
        assert!(read_mesh_asset(&tmp, bad).is_err());
    }

    let _ = std::fs::remove_dir_all(&tmp);
}
