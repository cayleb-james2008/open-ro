use openro_rs::luau_check::analyze_place;
use openro_rs::place::Place;
use std::path::Path;

#[test]
fn rebuilt_anime_card_battles_apex_rbxlx_and_assets_are_studio_grade_and_zero_error() {
    let root = Path::new("examples/anime-card-battles-apex");
    let xml = std::fs::read_to_string(root.join("AnimeCardBattlesApex.rbxlx"))
        .expect("AnimeCardBattlesApex.rbxlx exists");

    let place = Place::from_rbxlx_xml(&xml).expect("rebuilt .rbxlx imports cleanly");

    // 1. Rich 120+ Part 3D Hub World + Lighting rig
    let part_count = place
        .instances()
        .values()
        .filter(|v| {
            let cls = v.get("class_name").and_then(|c| c.as_str()).unwrap_or("");
            cls == "Part" || cls == "SpawnLocation" || cls == "MeshPart"
        })
        .count();
    assert!(
        part_count >= 120,
        "expected >= 120 3D parts in rebuilt hub world, found {part_count}"
    );

    for lighting_path in [
        "Lighting.ApexSky",
        "Lighting.ApexAtmosphere",
        "Lighting.ApexBloom",
        "Lighting.ApexSunRays",
        "Lighting.ApexColorCorrection",
    ] {
        assert!(
            place.instances().contains_key(lighting_path),
            "missing Lighting instance: {lighting_path}"
        );
    }

    // 2. All 8 Nano Banana character pedestals bound cleanly
    let showcases = [
        "supernova_ascended_striker",
        "infinity_void_sorcerer",
        "shadow_sovereign",
        "crimson_sea_captain",
        "nine_tail_ember_shinobi",
        "thunder_flash_ronin",
        "blood_moon_gravity_ronin",
        "eclipse_arcane_alchemist",
    ];
    for id in showcases {
        let base = format!("Workspace.CardGallery.Showcase_{id}");
        assert!(place.instances().contains_key(&base), "missing {base}");
        assert!(
            place.instances().contains_key(&format!("{base}.CardArtDecal")),
            "missing Decal for {id}"
        );
        assert!(
            place
                .instances()
                .contains_key(&format!("{base}.CardDisplayGui.CardArtImage")),
            "missing ImageLabel for {id}"
        );
        assert!(
            place.instances().contains_key(&format!("{base}.CardGlowLight")),
            "missing PointLight for {id}"
        );
        assert!(
            place.instances().contains_key(&format!("{base}.CardAuraEmitter")),
            "missing ParticleEmitter for {id}"
        );
        assert!(
            place.instances().contains_key(&format!("{base}.CardInfoBillboard")),
            "missing BillboardGui for {id}"
        );
        assert!(
            place.instances().contains_key(&format!("{base}.CardHighlight")),
            "missing Highlight for {id}"
        );
    }

    // 3. All required Luau modules, server scripts, and client scripts present & 0 static errors/warnings
    for script_path in [
        "ReplicatedStorage.Modules.CardDatabase",
        "ReplicatedStorage.Modules.CombatMath",
        "ServerScriptService.DataPersistence",
        "ServerScriptService.WeatherRaidService",
        "ServerScriptService.GameCore",
        "ServerScriptService.BattleEngine",
        "StarterPlayer.StarterPlayerScripts.ClientAudio",
        "StarterPlayer.StarterPlayerScripts.ApexClientHUD",
    ] {
        assert!(
            place.scripts().contains_key(script_path),
            "missing Luau script: {script_path}"
        );
    }

    let card_db = &place.scripts()["ReplicatedStorage.Modules.CardDatabase"];
    for pack in [
        "Starter",
        "Shibuya Spirit",
        "Crimson Pirate",
        "Shadow Gate",
        "Celestial Alchemy",
        "Eclipse Legends",
    ] {
        assert!(card_db.contains(pack), "missing pack {pack}");
    }
    for weather in [
        "Clear",
        "Rain",
        "Snow",
        "Sandstorm",
        "Blood Moon",
        "Eclipse 2x Luck",
        "Starfall",
    ] {
        assert!(card_db.contains(weather), "missing weather biome {weather}");
    }
    for charm in ["Samehada", "Spirit Arrow", "Gravity Core", "Eclipse Crown"] {
        assert!(card_db.contains(charm), "missing charm {charm}");
    }
    for trait_name in ["Golden", "Voidtouched", "Overcharged", "Swift", "Immortal"] {
        assert!(card_db.contains(trait_name), "missing trait {trait_name}");
    }

    let report = analyze_place(place.scripts(), place.instances());
    assert!(
        report.is_ok() && report.warning_count() == 0,
        "rebuilt place failed static Luau prover: {:?}",
        report.diagnostics
    );

    // 4. Verify Blender 5.2.2 LTS hero meshes (<6k tris + UVs + PNG previews)
    for stem in [
        "summon_torii_gate",
        "card_pedestal",
        "boss_colosseum_arch",
        "alchemist_cauldron",
    ] {
        let obj_text = std::fs::read_to_string(root.join(format!("assets/meshes/{stem}.obj")))
            .expect("mesh obj exists");
        assert!(obj_text.contains("\nvt "), "{stem}.obj missing UV coordinates");
        assert!(obj_text.contains("\nvn "), "{stem}.obj missing normals");
        let face_count = obj_text.lines().filter(|l| l.starts_with("f ")).count();
        assert!(face_count > 10 && face_count < 6000, "{stem}.obj face count {face_count}");

        let png = std::fs::read(root.join(format!("assets/meshes/{stem}.png")))
            .expect("mesh preview png exists");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "{stem}.png valid PNG");
    }

    // 5. Verify synthesized WAV SFX
    for sfx in [
        "summon_roll",
        "mythic_fanfare",
        "battle_crit",
        "potion_drink",
        "ui_click",
    ] {
        let wav = std::fs::read(root.join(format!("assets/audio/{sfx}.wav")))
            .expect("wav SFX exists");
        assert!(wav.len() > 1000, "{sfx}.wav non-empty");
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
    }

    // 6. Verify real Roblox Studio PNG captures exist and are valid PNGs
    for cap in [
        "hub_overview",
        "torii_shrine",
        "hall_of_legends",
        "boss_colosseum_pvp",
        "tower_cauldron",
        "playtest_reveal",
        "playtest_battle",
        "playtest_view",
    ] {
        let png = std::fs::read(root.join(format!("studio-captures/{cap}.png")))
            .expect("studio capture png exists");
        assert!(png.len() > 50_000, "{cap}.png should be full Studio frame ({}B)", png.len());
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "{cap}.png valid PNG");
    }

    assert!(card_db.contains("Crimson Eclipse") && card_db.contains("Celestial Tempest"));
    assert!(place.scripts()["ServerScriptService.BattleEngine"].contains("Overdrive"));
    assert!(place.scripts()["ServerScriptService.GameCore"].contains("EquippedGuardianAura"));
}
