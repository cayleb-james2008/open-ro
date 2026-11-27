//! Verification suite for OpenRO v0.6.0 — Anime Card Battles: Rebirth — Apex Edition:
//! - All 6 new Roblox Card-Battler Game Systems (expanding 22 -> 28 total systems)
//! - Zero user-facing "Rune" jargon across new systems, knowledge point, catalog & blueprint
//! - Full `anime-card-battles-apex` flagship starter blueprint execution & playtest proof
//! - Built-in Original Anime-Style Character Design Knowledge Point (`openro_rs::art_knowledge`)
//! - Game Art Asset Manager (`openro_rs::assets`): security traversal/magic-byte/symlink defenses,
//!   base64 & raw upload, honest unconfigured API refusal, loopback Gemini/OpenAI HTTP parsing,
//!   `.rbxlx` `Decal` & `ImageLabel` binding + lossless XML round-trip, and `asset-manifest.json`.

use openro_rs::art_knowledge;
use openro_rs::assets::{
    decode_base64, detect_image_mime, encode_base64, sanitize_filename, AssetManager,
    AssetMetadataInput, GenerateImageRequest,
};
use openro_rs::luau_check::{analyze_place, analyze_script};
use openro_rs::place::{LocalPlace, Place};
use openro_rs::{ideas, skills};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const NEW_6_CARD_SYSTEMS: [&str; 6] = [
    "card_gacha_roller",
    "deck_builder",
    "card_battle_arena",
    "tower_progression",
    "potions_weather",
    "card_gallery_world",
];

// Minimal valid 1x1 PNG, minimal valid JPEG header, minimal valid WebP header
const TINY_PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
    b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
    0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, b'I', b'D', b'A', b'T', 0x08, 0xD7, 0x63, 0xF8,
    0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00,
    0x00, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
];
const TINY_JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0xFF, 0xD9];
const TINY_WEBP: &[u8] = &[
    b'R', b'I', b'F', b'F', 0x14, 0x00, 0x00, 0x00, b'W', b'E', b'B', b'P', b'V', b'P', b'8',
    b' ', 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn fresh_temp_dir(tag: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("openro-v060-{tag}-{nonce}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn all_28_systems_registered_and_6_card_battler_systems_pass_static_prover_with_zero_warnings() {
    let reg = skills::registry();
    assert_eq!(reg.len(), 28);

    // Every single catalog entry must have zero user-facing "Rune" wording
    let catalog = skills::catalog_json();
    let arr = catalog.as_array().unwrap();
    assert_eq!(arr.len(), 28);
    for entry in arr {
        let s = entry.to_string().to_ascii_lowercase();
        assert!(
            !s.contains("rune"),
            "catalog entry leaked internal wording: {entry}"
        );
    }

    // Generate & statically prove all 6 new Card-Battler systems
    let mut composed_scripts: BTreeMap<String, String> = BTreeMap::new();
    for name in NEW_6_CARD_SYSTEMS {
        let def = skills::find(name).unwrap_or_else(|| panic!("missing system {name}"));
        let default_args: Value = serde_json::from_str(def.default_args).unwrap();
        let luau = skills::generate(name, &default_args)
            .unwrap_or_else(|e| panic!("failed to generate {name}: {e}"));

        // Zero user-facing "Rune" in generated Luau
        assert!(
            !luau.to_ascii_lowercase().contains("rune"),
            "system '{name}' leaked internal wording in generated Luau"
        );

        let single_rep = analyze_script(def.default_path, &luau);
        assert!(
            single_rep.is_ok() && single_rep.warning_count() == 0,
            "system '{name}' failed standalone analysis: {:?}",
            single_rep.diagnostics
        );
        composed_scripts.insert(def.default_path.to_string(), luau);
    }

    // Also prove all 6 systems composed together have 0 errors & 0 warnings!
    let composed_rep = analyze_place(&composed_scripts, &BTreeMap::new());
    assert!(
        composed_rep.is_ok() && composed_rep.warning_count() == 0,
        "composed 6 card-battler systems failed cross-reference analysis: {:?}",
        composed_rep.diagnostics
    );

    // Parameter verification & strict bounds rejection across all 6 new systems
    let gacha = skills::generate(
        "card_gacha_roller",
        &json!({ "base_luck_pct": 250, "pity_threshold": 40, "fast_roll_ms": 200, "enable_borders": true }),
    )
    .unwrap();
    assert!(
        gacha.contains("Secret")
            && gacha.contains("Celestial")
            && gacha.contains("Void")
            && gacha.contains("PITY_THRESHOLD = 40")
            && gacha.contains("RemoteFunction")
    );
    assert!(skills::generate("card_gacha_roller", &json!({ "base_luck_pct": 50 })).is_err());
    assert!(skills::generate("card_gacha_roller", &json!({ "pity_threshold": 5 })).is_err());

    let deck = skills::generate(
        "deck_builder",
        &json!({ "deck_slots": 6, "fusion_cost": 4, "rebirth_floor": 15, "store_name": "My\"Decks", "bind_close": true }),
    )
    .unwrap();
    assert!(
        deck.contains("DECK_SLOTS = 6")
            && deck.contains("FUSION_COST = 4")
            && deck.contains("My\\\"Decks")
            && deck.contains("UpdateAsync")
            && deck.contains("BindToClose")
    );
    assert!(skills::generate("deck_builder", &json!({ "deck_slots": 2 })).is_err());
    assert!(skills::generate("deck_builder", &json!({ "deck_slots": 9 })).is_err());

    let arena = skills::generate(
        "card_battle_arena",
        &json!({ "team_size": 6, "ultimate_charge_max": 120, "counter_window_ms": 900, "boss_hp_scale": 6 }),
    )
    .unwrap();
    assert!(
        arena.contains("TEAM_SIZE = 6")
            && arena.contains("ULTIMATE_MAX = 120")
            && arena.contains("Impossible Overlord")
            && arena.contains("ACTIVE COUNTER")
            && arena.contains("Arcane")
    );
    assert!(skills::generate("card_battle_arena", &json!({ "boss_hp_scale": 0 })).is_err());

    let tower = skills::generate(
        "tower_progression",
        &json!({ "max_floors": 50, "world_count": 5, "base_reward": 200, "store_name": "ApexTower", "bind_close": true }),
    )
    .unwrap();
    assert!(
        tower.contains("MAX_FLOORS = 50")
            && tower.contains("GetOrderedDataStore")
            && tower.contains("Celestial Apex")
            && tower.contains("BindToClose")
    );
    assert!(skills::generate("tower_progression", &json!({ "max_floors": 5 })).is_err());

    let weather = skills::generate(
        "potions_weather",
        &json!({ "weather_cycle_seconds": 60, "luck_potion_cost": 200, "speed_potion_cost": 150, "enable_codes": true }),
    )
    .unwrap();
    assert!(
        weather.contains("Solar Flare")
            && weather.contains("Void Eclipse")
            && weather.contains("Thunderstorm")
            && weather.contains("Blood Moon")
            && weather.contains("APEX2026")
            && weather.contains("LUCKBOOST")
    );
    assert!(skills::generate("potions_weather", &json!({ "weather_cycle_seconds": 5 })).is_err());

    let gallery = skills::generate(
        "card_gallery_world",
        &json!({ "lobby_name": "ApexHall", "showcase_count": 8, "enable_holograms": true, "create_structures": true }),
    )
    .unwrap();
    assert!(
        gallery.contains("ApexHall")
            && gallery.contains("RollPedestal")
            && gallery.contains("CardArtDecal")
            && gallery.contains("CardArtImage")
            && gallery.contains("BossRaidShrine")
    );
    assert!(skills::generate("card_gallery_world", &json!({ "lobby_name": "Bad Name!" })).is_err());
}

#[test]
fn anime_card_battles_apex_blueprint_builds_binds_art_and_roundtrips_rbxlx() {
    let idea = ideas::find("anime-card-battles-apex")
        .expect("anime-card-battles-apex blueprint registered");
    assert!(
        !idea.title.to_ascii_lowercase().contains("rune")
            && !idea.pitch.to_ascii_lowercase().contains("rune")
    );

    let mut place = LocalPlace::in_memory("Anime Card Battles: Rebirth — Apex Edition".to_string());
    let mut generated: BTreeMap<String, String> = BTreeMap::new();
    for step in &idea.plan {
        if let Some(sk) = step.get("skill").and_then(|v| v.as_str()) {
            let args = step.get("args").cloned().unwrap_or_else(|| json!({}));
            let src = skills::generate(sk, &args).unwrap();
            generated.insert(sk.to_string(), src);
        } else if let Some(tool) = step.get("tool").and_then(|v| v.as_str()) {
            let mut args = step.get("args").cloned().unwrap_or_else(|| json!({}));
            if tool == "write_script" {
                let sk = args["skill"].as_str().unwrap();
                args["source"] = Value::String(generated.get(sk).unwrap().clone());
            }
            let res = place.call_tool(tool, args).unwrap();
            if tool == "playtest" {
                let verdict: Value = serde_json::from_str(&res).unwrap();
                assert_eq!(
                    verdict["status"], "passed",
                    "apex blueprint playtest failed: {}",
                    verdict["console"]
                );
            }
        }
    }

    assert_eq!(place.scripts().len(), 11);
    let report = place.analyze();
    assert!(
        report.is_ok() && report.warning_count() == 0,
        "apex blueprint static analysis findings: {:?}",
        report.diagnostics
    );

    // Upload all 8 flagship card art assets (using real staged Nano Banana PNGs if present,
    // else TINY_PNG fallback so test stays 100% hermetic in CI!)
    let dir = fresh_temp_dir("apex-bind");
    let mgr = AssetManager::open(&dir);
    let staged_dir = PathBuf::from("/home/cayleb/Work/scratch/swarm-v060/card-art");
    for guide in art_knowledge::anime_archetypes() {
        let filename = format!("{}.png", guide.card_id);
        let candidate = staged_dir.join(&filename);
        let bytes = std::fs::read(&candidate).unwrap_or_else(|_| TINY_PNG.to_vec());
        let saved = mgr
            .save_bytes(
                &filename,
                &bytes,
                AssetMetadataInput {
                    card_id: Some(guide.card_id.to_string()),
                    source: Some("nano-banana".to_string()),
                    ..Default::default()
                },
            )
            .expect("save card art");
        assert_eq!(saved.source, "nano-banana");
        assert_eq!(saved.title, guide.homage_title);
        assert_eq!(saved.rarity, guide.rarity);
        assert_eq!(saved.element, guide.element);
    }

    let bound = mgr.bind_to_place(&mut place);
    assert_eq!(bound, 8);

    // Write portable asset-manifest.json
    let manifest_path = mgr
        .write_asset_manifest(&dir, place.game_name())
        .expect("write asset-manifest.json");
    let manifest_json: Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest_json["asset_count"], json!(8));

    // Export .rbxlx XML and verify authentic <Content name="Texture"> + <Content name="Image"> bindings
    let xml = place.to_rbxlx_xml();
    assert!(xml.contains("<Item class=\"Decal\""));
    assert!(xml.contains("<Content name=\"Texture\"><url>rbxasset://openro-assets/helios-clockwork-paragon.png</url></Content>"));
    assert!(xml.contains("<Item class=\"ImageLabel\""));
    assert!(xml.contains("<Content name=\"Image\"><url>rbxasset://openro-assets/prism-rift-archivist.png</url></Content>"));
    assert!(xml.contains("<token name=\"Face\">5</token>"));

    if let Ok(dump_path) = std::env::var("OPENRO_DUMP_APEX_RBXLX") {
        std::fs::write(&dump_path, &xml).expect("write apex rbxlx dump");
    }

    // Round-trip through Place::from_rbxlx_xml and prove lossless Decal/ImageLabel/Script preservation!
    let imported = Place::from_rbxlx_xml(&xml).expect("apex rbxlx round-trips cleanly");
    assert_eq!(imported.scripts().len(), place.scripts().len());
    assert_eq!(imported.instances().len(), place.instances().len());
    let decal = &imported.instances()["Workspace.CardGallery.Showcase_helios_clockwork_paragon.CardArtDecal"];
    assert_eq!(decal["class_name"], "Decal");
    assert_eq!(
        decal["properties"]["Texture"],
        "rbxasset://openro-assets/helios-clockwork-paragon.png"
    );
    assert_eq!(decal["properties"]["Face"], "Front");

    let img = &imported.instances()
        ["Workspace.CardGallery.Showcase_prism_rift_archivist.CardDisplayGui.CardArtImage"];
    assert_eq!(img["class_name"], "ImageLabel");
    assert_eq!(
        img["properties"]["Image"],
        "rbxasset://openro-assets/prism-rift-archivist.png"
    );

    let rep2 = imported.analyze();
    assert!(rep2.is_ok() && rep2.warning_count() == 0, "{:?}", rep2.diagnostics);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn art_knowledge_point_is_non_refusal_homage_complete_and_jargon_free() {
    let summary = art_knowledge::summary_json();
    assert_eq!(summary["never_refuse_anime_homage"], json!(true));
    assert_eq!(summary["archetypes"].as_array().unwrap().len(), 8);
    assert!(!summary.to_string().to_ascii_lowercase().contains("rune"));

    let guidance = art_knowledge::prompt_guidance();
    assert!(guidance.contains("100% original anime-style characters"));
    assert!(guidance.contains("zero copied likenesses"));
    assert!(!guidance.to_ascii_lowercase().contains("rune"));

    let prompt = art_knowledge::build_homage_prompt(
        "Astral Blade Empress",
        "Storm",
        "Celestial",
        "flowing lightning scarf",
    );
    assert!(
        prompt.contains("Astral Blade Empress")
            && prompt.contains("Celestial")
            && prompt.contains("Storm")
            && prompt.contains("100% original")
            && prompt.contains("zero copied franchise likenesses")
            && prompt.contains("zero text")
            && prompt.contains("zero watermark")
    );
}

#[test]
fn asset_manager_security_sanitization_magic_bytes_symlinks_and_live_clients() {
    // 1. Filename sanitization blocks traversal, separators, NULs, hidden files, bad extensions
    for bad in [
        "",
        "   ",
        "../secret.png",
        "../../etc/passwd",
        "sub/card.png",
        "sub\\card.png",
        ".hidden.png",
        "card\0.png",
        "card.svg",
        "card.html",
        "card.exe",
        "bad name.png",
        &format!("{}.png", "a".repeat(100)),
    ] {
        assert!(
            sanitize_filename(bad).is_err(),
            "expected filename rejection for {bad:?}"
        );
    }
    assert_eq!(sanitize_filename("hero-card_01.png").unwrap(), "hero-card_01.png");
    assert_eq!(sanitize_filename("hero.webp").unwrap(), "hero.webp");

    // 2. Magic-byte sniffing accepts PNG/JPEG/WebP and rejects spoofed files
    assert_eq!(detect_image_mime(TINY_PNG).unwrap(), ("image/png", "png"));
    assert_eq!(detect_image_mime(TINY_JPEG).unwrap(), ("image/jpeg", "jpg"));
    assert_eq!(detect_image_mime(TINY_WEBP).unwrap(), ("image/webp", "webp"));
    assert!(detect_image_mime(b"<svg><script>alert(1)</script></svg>").is_err());
    assert!(detect_image_mime(b"").is_err());

    // 3. Base64 round-trip & data URL stripping
    let b64 = encode_base64(TINY_PNG);
    assert_eq!(decode_base64(&b64).unwrap(), TINY_PNG);
    assert_eq!(
        decode_base64(&format!("data:image/png;base64,{b64}")).unwrap(),
        TINY_PNG
    );

    // 4. Save, persist across reopen, read, reject symlink escape, and delete
    let dir = fresh_temp_dir("asset-sec");
    let mgr = AssetManager::open(&dir);

    let saved_png = mgr
        .save_base64(
            "velvet-bell-warden.png",
            &format!("data:image/png;base64,{b64}"),
            AssetMetadataInput {
                source: Some("nano-banana".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(saved_png.sha256.len(), 64);
    assert_eq!(saved_png.title, "Velvet-Bell Warden");

    mgr.save_bytes("alt.webp", TINY_WEBP, AssetMetadataInput::default())
        .unwrap();
    mgr.save_bytes("alt.jpg", TINY_JPEG, AssetMetadataInput::default())
        .unwrap();

    // Fake PNG rejected
    assert!(mgr
        .save_bytes("spoof.png", b"not actually a png", AssetMetadataInput::default())
        .is_err());

    // Reopen reads persisted manifest cleanly
    let mgr2 = AssetManager::open(&dir);
    assert_eq!(mgr2.list().len(), 3);
    let (read_bytes, read_mime) = mgr2.read_bytes("velvet-bell-warden.png").unwrap();
    assert_eq!(read_bytes, TINY_PNG);
    assert_eq!(read_mime, "image/png");

    // Symlink escape guard (Unix)
    #[cfg(unix)]
    {
        let symlink_path = mgr2.assets_dir().join("escape.png");
        let _ = std::os::unix::fs::symlink("/etc/hosts", &symlink_path);
        assert!(
            mgr2.read_bytes("escape.png").is_err(),
            "symlink escaping game-assets must be blocked"
        );
    }

    assert!(mgr2.delete("alt.jpg").unwrap());
    assert_eq!(mgr2.list().len(), 2);

    // 5. Honest failure when image generation provider has no key configured
    if std::env::var("GEMINI_API_KEY").is_err() {
        let err = mgr2
            .generate_image(&GenerateImageRequest {
                provider: "nano-banana".to_string(),
                card_id: "test-card".to_string(),
                title: "Test Card".to_string(),
                rarity: "Epic".to_string(),
                element: "Storm".to_string(),
                atk: 300,
                hp: 1100,
                speed: 90,
                passive: "Test".to_string(),
                ultimate: "Test".to_string(),
                prompt: "".to_string(),
                model: None,
                api_key: None,
                base_url: None,
            })
            .expect_err("must fail honestly without GEMINI_API_KEY");
        assert!(err.contains("GEMINI_API_KEY"), "{err}");
    }

    // 6. Loopback mock proving Gemini inlineData HTTP parsing + disk save
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let b64_resp = encode_base64(TINY_PNG);
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut req_buf = [0u8; 4096];
            let _ = stream.read(&mut req_buf);
            let body = json!({
                "candidates": [{
                    "content": {
                        "parts": [{
                            "inlineData": {
                                "mimeType": "image/png",
                                "data": b64_resp
                            }
                        }]
                    }
                }]
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let generated = mgr2
        .generate_image(&GenerateImageRequest {
            provider: "gemini".to_string(),
            card_id: "loopback-hero".to_string(),
            title: "Loopback Hero".to_string(),
            rarity: "Mythic".to_string(),
            element: "Flame".to_string(),
            atk: 450,
            hp: 1500,
            speed: 92,
            passive: "Spark".to_string(),
            ultimate: "Nova".to_string(),
            prompt: "test cel-shaded card".to_string(),
            model: Some("gemini-2.5-flash-image".to_string()),
            api_key: Some("test-loopback-key".to_string()),
            base_url: Some(format!("http://127.0.0.1:{port}")),
        })
        .expect("loopback Gemini image response saves cleanly");
    assert_eq!(generated.filename, "loopback-hero.png");
    assert_eq!(generated.source, "gemini-api");
    assert_eq!(generated.bytes, TINY_PNG.len());

    let _ = std::fs::remove_dir_all(&dir);
}
