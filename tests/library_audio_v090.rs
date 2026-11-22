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
use openro_rs::market_taste::{
    classify_game_niche, parse_explore_payload, parse_roblox_id_input, prompt_section,
    research_roblox_game_with_base, MarketTasteStore, MARKET_NICHE_IDS,
};
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    assert_eq!(packs.len(), 9);
    assert_eq!(initial["prompt_skills_count"], 4);

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
            "anime-fan-art-knowledge",
            "roblox-market-taste"
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
    assert!(combined_initial.contains("Joyful Pirate"));
    assert!(combined_initial.contains("Six-Eyes Sorcerer"));
    assert!(combined_initial.contains("Shadow Monarch"));
    assert!(combined_initial.contains("Golden Ascended Warrior"));
    let art_summary = openro_rs::art_knowledge::summary_json();
    assert!(art_summary["visual_fidelity_rule"]
        .as_str()
        .unwrap_or("")
        .contains("unmistakably recognizable"));
    assert!(art_summary["title_epithet_rule"]
        .as_str()
        .unwrap_or("")
        .contains("Joyful Pirate"));

    // Uninstall of default packs -> 400; unknown pack -> 404
    let err_default = mgr
        .uninstall("roblox-studio-core")
        .expect_err("must refuse uninstalling default pack");
    assert_eq!(err_default.status, 400);
    let err_market = mgr
        .uninstall("roblox-market-taste")
        .expect_err("must refuse uninstalling default market taste pack");
    assert_eq!(err_market.status, 400);
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
        "roblox-market-taste",
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

#[test]
fn market_taste_live_explore_refresh_niche_classifier_cache_and_prompt() {
    assert_eq!(MARKET_NICHE_IDS.len(), 8);

    // 1. Verify classifier accuracy across real top-charting Roblox titles
    let cases = [
        ("Steal An Egg", "Simulation", "viral-sim-tycoon"),
        ("Throw Food For Animals", "Simulation", "viral-sim-tycoon"),
        ("Brookhaven RP", "Roleplay & Avatar Sim", "social-rp-fashion"),
        ("Dress To Impress", "Roleplay & Avatar Sim", "social-rp-fashion"),
        ("Dandy's World [1.0]", "Survival", "horror-coop-mascot"),
        ("Hyper Realistic Ragdoll", "Simulation", "physics-crash-ragdoll"),
        ("BloxNG Crash Physics", "Sports & Racing", "physics-crash-ragdoll"),
        ("100 Miles Down the River", "Survival", "survival-expedition"),
        ("99 Nights in the Forest", "Survival", "survival-expedition"),
        ("RIVALS", "Shooter", "tactical-arena-pvp"),
        ("STAND OUT", "Action", "tactical-arena-pvp"),
        ("Blox Fruits", "RPG", "anime-action-rpg-td"),
        ("Anime Card Battles: Rebirth", "Simulation", "anime-rng-cards"),
        ("Fisch", "Simulation", "anime-rng-cards"),
    ];
    for (title, genre, expected) in cases {
        assert_eq!(
            classify_game_niche(title, genre),
            expected,
            "misclassified '{title}' ({genre})"
        );
    }

    // 2. Verify sponsored ad skipping and cross-sort deduplication in pure parser
    let mock_payload = json!({
        "sorts": [
            {
                "sortId": "top-playing-now",
                "games": [
                    {
                        "universeId": 1001,
                        "rootPlaceId": 2001,
                        "name": "Steal An Egg Live",
                        "playerCount": 980000,
                        "totalUpVotes": 92000,
                        "totalDownVotes": 8000,
                        "isSponsored": false,
                        "genreL1": "Simulation"
                    },
                    {
                        "universeId": 9999,
                        "rootPlaceId": 9999,
                        "name": "Sponsored Slop Ad",
                        "playerCount": 500000,
                        "totalUpVotes": 100,
                        "totalDownVotes": 900,
                        "isSponsored": true,
                        "genreL1": "Simulation"
                    },
                    {
                        "universeId": 1002,
                        "rootPlaceId": 2002,
                        "name": "Anime Card Battles Live",
                        "playerCount": 88500,
                        "totalUpVotes": 48000,
                        "totalDownVotes": 2000,
                        "isSponsored": false,
                        "genreL1": "Simulation"
                    }
                ]
            },
            {
                "sortId": "top-trending",
                "games": [
                    {
                        "universeId": 1001,
                        "rootPlaceId": 2001,
                        "name": "Steal An Egg Live",
                        "playerCount": 995000,
                        "totalUpVotes": 95000,
                        "totalDownVotes": 5000,
                        "isSponsored": false,
                        "genreL1": "Simulation"
                    },
                    {
                        "universeId": 1003,
                        "rootPlaceId": 2003,
                        "name": "Hyper Realistic Ragdoll Live",
                        "playerCount": 152000,
                        "totalUpVotes": 56000,
                        "totalDownVotes": 4000,
                        "isSponsored": false,
                        "genreL1": "Simulation"
                    }
                ]
            }
        ]
    });

    let (parsed, sorts) = parse_explore_payload(&mock_payload).expect("parse_explore_payload");
    assert_eq!(parsed.len(), 3, "sponsored ad must be skipped and u:1001 deduplicated");
    assert_eq!(sorts, vec!["top-playing-now", "top-trending"]);
    assert_eq!(parsed[0].name, "Steal An Egg Live");
    assert_eq!(parsed[0].ccu, 995_000);
    assert_eq!(parsed[0].like_ratio_pct, 95.0);
    assert_eq!(parsed[0].sort_tags, vec!["top-playing-now", "top-trending"]);

    // 3. Spin up a real local loopback HTTP mock Explore server and prove live refresh + cache persistence!
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock explore server");
    let addr = listener.local_addr().unwrap();
    let payload_str = mock_payload.to_string();
    let server = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut req_buf = [0u8; 1024];
            let _ = stream.read(&mut req_buf);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload_str.len(),
                payload_str
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });

    let tmp = temp_test_dir("market-taste");
    let mut store = MarketTasteStore::open(&tmp);
    let baseline_snap = store.snapshot_json(None);
    assert_eq!(baseline_snap["source"], "baseline_nov_2026");
    assert_eq!(baseline_snap["active_niche"], "anime-rng-cards");
    assert_eq!(baseline_snap["niches"].as_array().unwrap().len(), 8);

    // Refresh against mock loopback Explore API
    let url = format!("http://{addr}/explore-api/v1/get-sorts?sessionId=test");
    let refreshed = store
        .refresh_live_from_url(&url, Duration::from_secs(5))
        .expect("refresh_live_from_url");
    let _ = server.join();
    assert_eq!(refreshed["source"], "live_roblox_explore");
    assert_eq!(refreshed["live_games_count"], 3);
    assert!(refreshed["last_refreshed_ms"].as_u64().unwrap_or(0) > 0);
    assert!(tmp.join("market_taste_cache.json").is_file());

    // Switch active niche -> persists & updates LibraryManager::active_skill_prompt()
    assert!(store.set_active_niche("not-a-real-niche").is_err());
    let switched = store
        .set_active_niche("viral-sim-tycoon")
        .expect("switch to viral-sim-tycoon");
    assert_eq!(switched["active_niche"], "viral-sim-tycoon");
    assert_eq!(
        switched["niche"]["top_games"][0]["name"],
        "Steal An Egg Live"
    );

    // Fresh reload from market_taste_cache.json + LibraryManager prompt injection
    let reloaded = MarketTasteStore::open(&tmp);
    assert_eq!(reloaded.active_niche(), "viral-sim-tycoon");
    assert_eq!(reloaded.live_games_count(), 3);

    let lib = LibraryManager::open(&tmp);
    let full_prompt = lib.active_skill_prompt();
    assert!(full_prompt.contains("viral-sim-tycoon"));
    assert!(full_prompt.contains("Steal An Egg Live"));
    assert!(full_prompt.contains("Top-Trending Craft Bar"));
    assert!(full_prompt.contains("Monetization Loops"));
    assert!(full_prompt.contains("Anti-Slop Taste Guardrails"));
    assert!(full_prompt.contains("Mandatory Automatic Twists"));
    assert!(!full_prompt.contains("Rune"));

    // Verify every niche snapshot carries all 8 Taste Dossier dimensions + 3 twists + 3 quality boosts
    for niche_id in MARKET_NICHE_IDS {
        let snap = reloaded.snapshot_json(Some(niche_id));
        let d = &snap["niche"]["taste_dossier"];
        assert!(d["market_leaders"].as_array().is_some_and(|a| !a.is_empty()));
        assert!(d["gameplay_loop"].as_array().is_some_and(|a| !a.is_empty()));
        assert!(d["ui_ux_style"].as_array().is_some_and(|a| !a.is_empty()));
        assert!(d["animations_and_juice"].as_array().is_some_and(|a| !a.is_empty()));
        assert!(d["model_and_world_3d_style"].as_array().is_some_and(|a| !a.is_empty()));
        assert!(d["audio_cadence"].as_array().is_some_and(|a| !a.is_empty()));
        assert!(d["monetization_and_retention"]["monetization_loops"]
            .as_array()
            .is_some_and(|a| !a.is_empty()));
        assert_eq!(d["automatic_twists"].as_array().unwrap().len(), 3);
        assert_eq!(d["quality_boosts"].as_array().unwrap().len(), 3);
        assert!(d["craft_bar"].as_str().is_some_and(|s| !s.is_empty()));
    }

    // Standalone prompt_section helper works cleanly too
    let standalone_prompt = prompt_section(Some("physics-crash-ragdoll"));
    assert!(standalone_prompt.contains("Hyper Realistic Ragdoll"));
    assert!(standalone_prompt.contains("BloxNG Crash Physics"));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn research_roblox_game_synthesizes_8_dim_taste_dossier_and_persists_files() {
    assert_eq!(
        parse_roblox_id_input("10092619788").unwrap(),
        (10_092_619_788, false)
    );
    assert_eq!(
        parse_roblox_id_input(
            "https://www.roblox.com/games/130253630950453/Anime-Card-Battles-Rebirth"
        )
        .unwrap(),
        (130_253_630_950_453, true)
    );
    assert!(parse_roblox_id_input("not-a-roblox-url").is_err());
    assert!(parse_roblox_id_input("0").is_err());

    // Multi-route mock Roblox API server handling universes, games, votes, and thumbnails
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock roblox api");
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..4 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("/");

            let body = if path.starts_with("/universes/v1/places/130253630950453/universe") {
                json!({ "universeId": 10_092_619_788u64 }).to_string()
            } else if path.starts_with("/v1/games/votes") {
                json!({
                    "data": [{ "id": 10_092_619_788u64, "upVotes": 24323, "downVotes": 784 }]
                })
                .to_string()
            } else if path.starts_with("/v1/games/multiget/thumbnails") {
                json!({
                    "data": [{
                        "universeId": 10_092_619_788u64,
                        "thumbnails": [
                            { "state": "Completed", "imageUrl": "https://tr.rbxcdn.com/mock-thumb-1.png" },
                            { "state": "Completed", "imageUrl": "https://tr.rbxcdn.com/mock-thumb-2.png" }
                        ]
                    }]
                })
                .to_string()
            } else if path.starts_with("/v1/games?") {
                json!({
                    "data": [{
                        "id": 10_092_619_788u64,
                        "rootPlaceId": 130_253_630_950_453u64,
                        "name": "Anime Card Battles: Rebirth",
                        "description": "Roll anime cards, brew luck potions, and clash 4v4!",
                        "creator": { "name": "Apex Studio" },
                        "playing": 68000,
                        "visits": 15_122_207,
                        "favoritedCount": 9566,
                        "genre_l1": "Simulation"
                    }]
                })
                .to_string()
            } else {
                json!({ "data": [] }).to_string()
            };

            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });

    let tmp = temp_test_dir("research");
    let base = format!("http://{addr}");
    let dossier = research_roblox_game_with_base(
        &tmp,
        "https://www.roblox.com/games/130253630950453/Anime-Card-Battles-Rebirth",
        None,
        Some(&base),
        Duration::from_secs(5),
    )
    .expect("research_roblox_game_with_base");
    let _ = server.join();

    assert_eq!(dossier["ok"], true);
    assert_eq!(dossier["universe_id"], 10_092_619_788u64);
    assert_eq!(dossier["root_place_id"], 130_253_630_950_453u64);
    assert_eq!(dossier["name"], "Anime Card Battles: Rebirth");
    assert_eq!(dossier["niche"], "anime-rng-cards");
    assert_eq!(dossier["like_ratio_pct"], 96.9);
    assert_eq!(dossier["thumbnails"].as_array().unwrap().len(), 2);
    assert_eq!(dossier["automatic_twists"].as_array().unwrap().len(), 3);
    assert_eq!(dossier["quality_boosts"].as_array().unwrap().len(), 3);

    // Persisted JSON & TASTE-DOSSIER.md verified on disk
    let saved_json = tmp.join("taste-dossiers").join("10092619788.json");
    let saved_md = tmp.join("taste-dossiers").join("TASTE-DOSSIER.md");
    assert!(saved_json.is_file());
    assert!(saved_md.is_file());
    let md = std::fs::read_to_string(&saved_md).expect("read TASTE-DOSSIER.md");
    assert!(md.contains("Anime Card Battles: Rebirth"));
    assert!(md.contains("CHAIN OVERDRIVE"));
    assert!(md.contains("FUSION WEATHER STORMS"));
    assert!(md.contains("sunlit anime floating archipelago"));
    assert!(!md.contains("Rune"));

    let _ = std::fs::remove_dir_all(&tmp);
}
