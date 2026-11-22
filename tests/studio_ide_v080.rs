//! Integration tests for OpenRO v0.8.0 Studio IDE backend:
//! - Direct 3D part presets (`block`, `sphere`, `cylinder`, `spawn`, `jump_pad`, `kill_brick`, `card_billboard`)
//! - Instance duplication & recursive deletion
//! - Live Luau script editing with real-time Static Luau Prover diagnostics
//! - Bounded Undo/Redo stack (`MAX_HISTORY_DEPTH = 64`)
//! - Zero-dependency Studio & Rojo Ship Pack `.zip` export + ZIP-slip resistance.

use openro_rs::assets::{AssetManager, AssetMetadataInput};
use openro_rs::place::{validate_place, LocalPlace, MAX_HISTORY_DEPTH};
use openro_rs::rojo_export::{
    build_rojo_project_json, build_rojo_ship_pack, build_rojo_ship_pack_with_manager,
    build_upload_checklist, ShipPackAsset, ZipWriter,
};
use serde_json::Value;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Minimal valid 1x1 PNG (RGBA) with valid magic bytes and chunks.
const SAMPLE_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// Minimal valid RIFF/WEBP payload (12-byte magic header + VP8L chunk header).
const SAMPLE_WEBP: &[u8] =
    b"RIFF\x14\x00\x00\x00WEBPVP8L\x08\x00\x00\x00\x2f\x00\x00\x00\x00\x00\x00\x00";

fn temp_test_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "openro-v080-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

#[test]
fn all_seven_part_presets_and_rbxlx_shape_round_trip() {
    let mut place = LocalPlace::in_memory("StudioPresetArena".to_string());

    let block = place
        .create_part_preset(
            "block",
            Some("ArenaBlock"),
            Some([0.0, 4.0, 0.0]),
            Some([10.0, 2.0, 10.0]),
            Some("#3B82F6"),
        )
        .expect("create block");
    assert_eq!(block.path, "Workspace.ArenaBlock");
    assert_eq!(block.class_name, "Part");
    assert_eq!(block.instance["properties"]["Shape"], "Block");
    assert_eq!(block.companion_script, None);

    // Duplicate name auto-suffixes cleanly, and None arguments use sensible preset defaults
    let block2 = place
        .create_part_preset("block", Some("ArenaBlock"), None, None, None)
        .expect("create suffixed block");
    assert_eq!(block2.path, "Workspace.ArenaBlock_2");

    let default_block = place
        .create_part_preset("block", None, None, None, None)
        .expect("create default block");
    assert!(default_block.path.starts_with("Workspace."));

    let sphere = place
        .create_part_preset(
            "sphere",
            Some("EnergyOrb"),
            Some([0.0, 8.0, 8.0]),
            Some([5.0, 5.0, 5.0]),
            Some("#A855F7"),
        )
        .expect("create sphere");
    assert_eq!(sphere.instance["properties"]["Shape"], "Ball");

    let cyl = place
        .create_part_preset(
            "cylinder",
            Some("WarpPillar"),
            Some([-8.0, 6.0, 0.0]),
            Some([4.0, 12.0, 4.0]),
            Some("#14B8A6"),
        )
        .expect("create cylinder");
    assert_eq!(cyl.instance["properties"]["Shape"], "Cylinder");

    let spawn = place
        .create_part_preset(
            "spawn",
            Some("HeroSpawn"),
            Some([0.0, 1.0, -16.0]),
            Some([8.0, 1.0, 8.0]),
            Some("#22C55E"),
        )
        .expect("create spawn");
    assert_eq!(spawn.class_name, "SpawnLocation");

    let jump_pad = place
        .create_part_preset(
            "jump_pad",
            Some("SkyBounce"),
            Some([6.0, 1.0, 6.0]),
            Some([6.0, 1.0, 6.0]),
            Some("#06B6D4"),
        )
        .expect("create jump_pad");
    assert_eq!(
        jump_pad.companion_script.as_deref(),
        Some("ServerScriptService.SkyBounceController")
    );
    assert!(place
        .scripts()
        .contains_key("ServerScriptService.SkyBounceController"));

    let kill_brick = place
        .create_part_preset(
            "kill_brick",
            Some("MagmaFloor"),
            Some([-6.0, 1.0, 6.0]),
            Some([8.0, 1.0, 8.0]),
            Some("#EF4444"),
        )
        .expect("create kill_brick");
    assert_eq!(
        kill_brick.companion_script.as_deref(),
        Some("Workspace.MagmaFloor.KillBrick")
    );
    assert!(place
        .scripts()
        .contains_key("Workspace.MagmaFloor.KillBrick"));

    let billboard = place
        .create_part_preset(
            "card_billboard",
            Some("MythicCardBoard"),
            Some([0.0, 10.0, -24.0]),
            Some([12.0, 16.0, 1.0]),
            Some("#F59E0B"),
        )
        .expect("create card_billboard");
    assert_eq!(billboard.path, "Workspace.MythicCardBoard");
    assert!(place
        .instances()
        .contains_key("Workspace.MythicCardBoard.CardArtDecal"));
    assert!(place
        .instances()
        .contains_key("Workspace.MythicCardBoard.CardDisplayGui"));
    assert!(place
        .instances()
        .contains_key("Workspace.MythicCardBoard.CardDisplayGui.CardArtImage"));

    // Companion scripts must pass both Static Luau Prover and Playtest validation with 0 errors.
    let audit = place.analyze();
    assert_eq!(
        audit.error_count(),
        0,
        "Preset companion scripts should have 0 static errors: {:?}",
        audit.diagnostics
    );
    let validation = validate_place(place.scripts(), place.instances());
    assert!(
        validation.errors.is_empty(),
        "Playtest validation should pass for preset scripts: {:?}",
        validation.errors
    );

    // .rbxlx XML must emit Roblox <token name="shape"> and round-trip Ball/Cylinder/Block cleanly.
    let xml = place.to_rbxlx_xml();
    assert!(
        xml.contains("<token name=\"shape\">0</token>"),
        "Ball shape token missing"
    );
    assert!(
        xml.contains("<token name=\"shape\">2</token>"),
        "Cylinder shape token missing"
    );

    let parsed = LocalPlace::from_rbxlx_xml(&xml).expect("round-trip parse_rbxlx_xml");
    assert_eq!(
        parsed.instances()["Workspace.EnergyOrb"]["properties"]["Shape"],
        Value::String("Ball".to_string())
    );
    assert_eq!(
        parsed.instances()["Workspace.WarpPillar"]["properties"]["Shape"],
        Value::String("Cylinder".to_string())
    );
    assert_eq!(
        parsed.instances()["Workspace.ArenaBlock"]["properties"]["Shape"],
        Value::String("Block".to_string())
    );
    assert!(parsed
        .scripts()
        .contains_key("ServerScriptService.SkyBounceController"));
    assert!(parsed
        .scripts()
        .contains_key("Workspace.MagmaFloor.KillBrick"));
}

#[test]
fn create_part_preset_rejects_invalid_inputs_without_mutating_state() {
    let mut place = LocalPlace::in_memory("GuardTest".to_string());
    let before_count = place.instances().len();

    assert!(place
        .create_part_preset(
            "unknown_kind",
            Some("Bad"),
            Some([0.0, 1.0, 0.0]),
            Some([4.0, 4.0, 4.0]),
            Some("#ffffff"),
        )
        .is_err());
    assert!(place
        .create_part_preset(
            "block",
            Some("BadHex"),
            Some([0.0, 1.0, 0.0]),
            Some([4.0, 4.0, 4.0]),
            Some("not-a-color"),
        )
        .is_err());
    assert!(place
        .create_part_preset(
            "block",
            Some("ZeroSize"),
            Some([0.0, 1.0, 0.0]),
            Some([0.0, 4.0, 4.0]),
            Some("#ffffff"),
        )
        .is_err());
    assert!(place
        .create_part_preset(
            "block",
            Some("NanPos"),
            Some([f64::NAN, 1.0, 0.0]),
            Some([4.0, 4.0, 4.0]),
            Some("#ffffff"),
        )
        .is_err());
    assert!(place
        .create_part_preset(
            "block",
            Some("Bad.Name/Here"),
            Some([0.0, 1.0, 0.0]),
            Some([4.0, 4.0, 4.0]),
            Some("#ffffff"),
        )
        .is_err());

    assert_eq!(place.instances().len(), before_count);
    assert!(
        !place.can_undo(),
        "failed creations must not pollute undo history"
    );
}

#[test]
fn duplicate_and_delete_instance_handle_descendants_and_scripts() {
    let mut place = LocalPlace::in_memory("DupDeleteArena".to_string());

    place
        .create_part_preset(
            "kill_brick",
            Some("LavaTile"),
            Some([2.0, 1.0, 3.0]),
            Some([6.0, 1.0, 6.0]),
            Some("#EF4444"),
        )
        .expect("create lava tile");
    place
        .create_part_preset(
            "card_billboard",
            Some("Banner"),
            Some([0.0, 8.0, 0.0]),
            Some([10.0, 14.0, 1.0]),
            Some("#F59E0B"),
        )
        .expect("create banner");

    // Duplicate kill_brick -> clones part with [+4, 0, +4] offset AND companion script
    let dup1 = place
        .duplicate_instance("Workspace.LavaTile")
        .expect("duplicate LavaTile");
    assert_eq!(dup1.new_path, "Workspace.LavaTile_Copy");
    assert_eq!(dup1.instance["properties"]["Position"][0].as_f64(), Some(6.0));
    assert_eq!(dup1.instance["properties"]["Position"][1].as_f64(), Some(1.0));
    assert_eq!(dup1.instance["properties"]["Position"][2].as_f64(), Some(7.0));
    assert!(dup1
        .duplicated_scripts
        .contains(&"Workspace.LavaTile_Copy.KillBrick".to_string()));
    assert!(place
        .scripts()
        .contains_key("Workspace.LavaTile_Copy.KillBrick"));

    // Second duplicate of original uses _Copy2
    let dup2 = place
        .duplicate_instance("Workspace.LavaTile")
        .expect("second duplicate");
    assert_eq!(dup2.new_path, "Workspace.LavaTile_Copy2");

    // Duplicate card_billboard -> clones nested Decal + SurfaceGui + ImageLabel descendants
    let dup_banner = place
        .duplicate_instance("Workspace.Banner")
        .expect("duplicate Banner");
    assert_eq!(dup_banner.new_path, "Workspace.Banner_Copy");
    assert!(place
        .instances()
        .contains_key("Workspace.Banner_Copy.CardArtDecal"));
    assert!(place
        .instances()
        .contains_key("Workspace.Banner_Copy.CardDisplayGui"));
    assert!(place
        .instances()
        .contains_key("Workspace.Banner_Copy.CardDisplayGui.CardArtImage"));

    // Delete duplicated LavaTile -> removes part AND child script cleanly
    let del_lava = place
        .delete_instance("Workspace.LavaTile_Copy")
        .expect("delete LavaTile_Copy");
    assert!(del_lava
        .removed_instances
        .contains(&"Workspace.LavaTile_Copy".to_string()));
    assert!(del_lava
        .removed_scripts
        .contains(&"Workspace.LavaTile_Copy.KillBrick".to_string()));
    assert!(!place.instances().contains_key("Workspace.LavaTile_Copy"));
    assert!(!place
        .scripts()
        .contains_key("Workspace.LavaTile_Copy.KillBrick"));

    // Delete duplicated Banner -> removes parent and all 3 descendant instances
    let del_banner = place
        .delete_instance("Workspace.Banner_Copy")
        .expect("delete Banner_Copy");
    assert_eq!(del_banner.removed_instances.len(), 4);
    assert!(!place
        .instances()
        .contains_key("Workspace.Banner_Copy.CardDisplayGui.CardArtImage"));

    // Missing path fails cleanly
    assert!(place.duplicate_instance("Workspace.DoesNotExist").is_err());
    assert!(place.delete_instance("Workspace.DoesNotExist").is_err());
}

#[test]
fn update_script_source_returns_live_luau_diagnostics() {
    let mut place = LocalPlace::in_memory("LiveEditorPlace".to_string());

    // 1. Clean Luau script -> 0 errors
    let clean_src = r#"local Players = game:GetService("Players")
Players.PlayerAdded:Connect(function(player)
    local stats = Instance.new("Folder")
    stats.Name = "leaderstats"
    stats.Parent = player
end)
"#;
    let ok_res = place
        .update_script_source("ServerScriptService.Leaderstats", clean_src)
        .expect("save clean script");
    assert!(ok_res.created);
    assert_eq!(ok_res.script_report.error_count(), 0);
    assert_eq!(ok_res.place_report.error_count(), 0);
    assert_eq!(ok_res.to_json()["error_count"], 0);

    // 2. Broken Luau (unclosed block + tight infinite while true do without task.wait)
    let broken_src = r#"while true do
    local x = 1
    if x == 1 then
        print("missing wait and missing end")
"#;
    let bad_res = place
        .update_script_source("ServerScriptService.Leaderstats", broken_src)
        .expect("save broken script for live diagnostics");
    assert!(!bad_res.created);
    assert!(
        bad_res.script_report.error_count() >= 1,
        "Expected syntax/infinite-loop errors, got: {:?}",
        bad_res.script_report.diagnostics
    );
    assert!(bad_res.place_report.error_count() >= 1);
    assert_eq!(
        place
            .scripts()
            .get("ServerScriptService.Leaderstats")
            .map(String::as_str),
        Some(broken_src)
    );

    // 3. Invalid paths and NUL bytes rejected cleanly
    assert!(place
        .update_script_source("InvalidRoot.Hack", "print('hi')")
        .is_err());
    assert!(place
        .update_script_source("ServerScriptService..Evil", "print('hi')")
        .is_err());
    assert!(place
        .update_script_source("ServerScriptService.NulTest", "print('\0')")
        .is_err());
}

#[test]
fn undo_redo_stack_is_exact_and_bounded() {
    let mut place = LocalPlace::in_memory("UndoRedoArena".to_string());
    assert!(!place.can_undo());
    assert!(!place.can_redo());
    assert!(place.undo().is_err());
    assert!(place.redo().is_err());

    // Step 1: create block
    place
        .create_part_preset(
            "block",
            Some("PlatformA"),
            Some([0.0, 2.0, 0.0]),
            Some([8.0, 1.0, 8.0]),
            Some("#3B82F6"),
        )
        .expect("step 1");
    // Step 2: duplicate block
    place
        .duplicate_instance("Workspace.PlatformA")
        .expect("step 2");
    // Step 3: add script
    place
        .update_script_source("ServerScriptService.Main", "local x = 42\nprint(x)\n")
        .expect("step 3");
    // Step 4: delete duplicate
    place
        .delete_instance("Workspace.PlatformA_Copy")
        .expect("step 4");

    assert_eq!(place.undo_depth(), 4);
    assert_eq!(place.redo_depth(), 0);
    assert!(!place.instances().contains_key("Workspace.PlatformA_Copy"));

    // Undo Step 4 -> PlatformA_Copy restored
    let u1 = place.undo().expect("undo 1");
    assert_eq!(u1.action, "undo");
    assert!(place.instances().contains_key("Workspace.PlatformA_Copy"));
    assert_eq!(place.undo_depth(), 3);
    assert_eq!(place.redo_depth(), 1);

    // Undo Step 3 -> script removed
    place.undo().expect("undo 2");
    assert!(!place.scripts().contains_key("ServerScriptService.Main"));

    // Undo Step 2 -> PlatformA_Copy removed
    place.undo().expect("undo 3");
    assert!(!place.instances().contains_key("Workspace.PlatformA_Copy"));
    assert!(place.instances().contains_key("Workspace.PlatformA"));

    // Undo Step 1 -> PlatformA removed
    place.undo().expect("undo 4");
    assert!(!place.instances().contains_key("Workspace.PlatformA"));
    assert_eq!(place.undo_depth(), 0);
    assert_eq!(place.redo_depth(), 4);
    assert!(place.undo().is_err());

    // Redo all 4 steps back to final state
    for _ in 0..4 {
        let r = place.redo().expect("redo step");
        assert_eq!(r.action, "redo");
    }
    assert!(place.instances().contains_key("Workspace.PlatformA"));
    assert!(!place.instances().contains_key("Workspace.PlatformA_Copy"));
    assert!(place.scripts().contains_key("ServerScriptService.Main"));
    assert_eq!(place.undo_depth(), 4);
    assert_eq!(place.redo_depth(), 0);

    // Undo once, then make a new forward edit -> redo stack must clear!
    place.undo().expect("undo before branch");
    assert_eq!(place.redo_depth(), 1);
    place
        .create_part_preset(
            "sphere",
            Some("BranchOrb"),
            Some([0.0, 5.0, 0.0]),
            Some([3.0, 3.0, 3.0]),
            Some("#EC4899"),
        )
        .expect("forward edit clears redo");
    assert_eq!(place.redo_depth(), 0);
    assert!(!place.can_redo());

    // Bounded history cap: 75 edits must cap undo_depth at MAX_HISTORY_DEPTH (64)
    for i in 0..75 {
        place
            .create_part_preset(
                "block",
                Some(&format!("CapPart{i}")),
                Some([i as f64, 1.0, 0.0]),
                Some([2.0, 2.0, 2.0]),
                Some("#64748B"),
            )
            .expect("cap part");
    }
    assert_eq!(place.undo_depth(), MAX_HISTORY_DEPTH);

    let mut undone = 0usize;
    while place.can_undo() {
        place.undo().expect("bounded undo");
        undone += 1;
    }
    assert_eq!(undone, MAX_HISTORY_DEPTH);
    assert!(place.undo().is_err());
}

#[test]
fn zip_writer_blocks_zip_slip_and_duplicates() {
    let mut zip = ZipWriter::new();
    for bad in [
        "",
        "   ",
        "/etc/passwd",
        "../escape.txt",
        "assets/../../evil.png",
        "assets\\evil.png",
        "C:evil.txt",
        "./dot.txt",
        "a/./b.txt",
        "bad\0name.txt",
        "bad\nname.txt",
        "assets//double.png",
    ] {
        assert!(
            zip.add_file(bad, b"nope").is_err(),
            "ZipWriter must reject malicious entry path: {bad:?}"
        );
    }

    zip.add_file("ok/hello.txt", b"hello openro")
        .expect("valid entry");
    assert!(
        zip.add_file("ok/hello.txt", b"duplicate").is_err(),
        "ZipWriter must reject duplicate entries"
    );
    let (bytes, list) = zip.finish().expect("finish zip");
    assert_eq!(list, vec!["ok/hello.txt".to_string()]);
    assert!(bytes.starts_with(b"PK\x03\x04"));
    assert!(bytes.windows(4).any(|w| w == b"PK\x05\x06"));
}

#[test]
fn rojo_ship_pack_exports_complete_valid_zip_with_assets_and_collision_safety() {
    let tmp = temp_test_dir("ship-pack");
    let mgr = AssetManager::open(&tmp);

    let png_meta = mgr
        .save_bytes(
            "shadow-blade.png",
            SAMPLE_PNG,
            AssetMetadataInput {
                card_id: Some("shadow-blade".to_string()),
                title: Some("Shadow Blade".to_string()),
                rarity: Some("Mythic".to_string()),
                element: Some("Void".to_string()),
                atk: Some(95),
                hp: Some(120),
                speed: Some(88),
                source: Some("upload".to_string()),
                ..AssetMetadataInput::default()
            },
        )
        .expect("save png asset");
    let webp_meta = mgr
        .save_bytes(
            "solar-phoenix.webp",
            SAMPLE_WEBP,
            AssetMetadataInput {
                card_id: Some("solar-phoenix".to_string()),
                title: Some("Solar Phoenix".to_string()),
                rarity: Some("SSR".to_string()),
                element: Some("Flame".to_string()),
                source: Some("upload".to_string()),
                ..AssetMetadataInput::default()
            },
        )
        .expect("save webp asset");

    let mut place = LocalPlace::in_memory("Anime Card Arena".to_string());
    place
        .create_part_preset(
            "spawn",
            Some("LobbySpawn"),
            Some([0.0, 1.0, 0.0]),
            Some([8.0, 1.0, 8.0]),
            Some("#22C55E"),
        )
        .expect("spawn");
    place
        .create_part_preset(
            "sphere",
            Some("ArenaCrystal"),
            Some([0.0, 12.0, 0.0]),
            Some([4.0, 4.0, 4.0]),
            Some("#A855F7"),
        )
        .expect("sphere");
    place
        .create_part_preset(
            "jump_pad",
            Some("DuelLauncher"),
            Some([10.0, 1.0, 0.0]),
            Some([6.0, 1.0, 6.0]),
            Some("#06B6D4"),
        )
        .expect("jump_pad");
    place
        .create_part_preset(
            "kill_brick",
            Some("LavaTrap"),
            Some([-10.0, 1.0, 0.0]),
            Some([6.0, 1.0, 6.0]),
            Some("#EF4444"),
        )
        .expect("kill_brick");
    place
        .create_part_preset(
            "card_billboard",
            Some("ShowcaseWall"),
            Some([0.0, 8.0, -18.0]),
            Some([12.0, 16.0, 1.0]),
            Some("#F59E0B"),
        )
        .expect("billboard");

    place
        .update_script_source(
            "ServerScriptService.BattleServer",
            "local Players = game:GetService(\"Players\")\nPlayers.PlayerAdded:Connect(function(p)\n    print(p.Name)\nend)\n",
        )
        .expect("server script");
    place
        .update_script_source(
            "StarterGui.CardHudClient",
            "local Players = game:GetService(\"Players\")\nlocal lp = Players.LocalPlayer\nprint(lp and lp.Name or \"guest\")\n",
        )
        .expect("starter gui script");
    place
        .update_script_source(
            "StarterPlayer.StarterPlayerScripts.CameraRig",
            "local RunService = game:GetService(\"RunService\")\nprint(RunService:IsClient())\n",
        )
        .expect("starter player script");
    // Deliberately collide with generated `src/shared/PlaceMeta.luau` to verify collision-free suffixing!
    place
        .update_script_source(
            "ReplicatedStorage.PlaceMeta",
            "return { Arena = \"Anime Card Arena\" }\n",
        )
        .expect("replicated module with colliding name");

    mgr.bind_to_place(&mut place);

    let bundle = build_rojo_ship_pack_with_manager(&place, Some(&mgr))
        .expect("build rojo ship pack");

    assert_eq!(bundle.filename_slug(), "anime-card-arena-ship-pack.zip");
    assert_eq!(bundle.asset_count, 2);
    assert_eq!(bundle.audit_error_count, 0);
    assert!(bundle.zip_bytes.starts_with(b"PK\x03\x04"));
    assert!(bundle.zip_bytes.windows(4).any(|w| w == b"PK\x05\x06"));

    // Required top-level & nested files
    for required in [
        "game.rbxlx",
        "default.project.json",
        "src/shared/PlaceMeta.luau",
        "src/shared/PlaceMeta_2.luau",
        "src/server/BattleServer.server.luau",
        "src/server/DuelLauncherController.server.luau",
        "src/server/LavaTrap_KillBrick.server.luau",
        "src/client/CardHudClient.client.luau",
        "src/client/CameraRig.client.luau",
        "asset-manifest.json",
        "assets/shadow-blade.png",
        "assets/solar-phoenix.webp",
        "luau-audit-report.json",
        "STUDIO-UPLOAD-CHECKLIST.md",
    ] {
        assert!(
            bundle.file_list.iter().any(|f| f == required),
            "Missing expected entry '{required}' in file_list: {:?}",
            bundle.file_list
        );
    }

    // Verify Rojo project JSON structure directly
    let rojo_json = build_rojo_project_json("Anime Card Arena", place.scripts());
    assert_eq!(rojo_json["name"], "Anime Card Arena");
    assert_eq!(rojo_json["tree"]["$className"], "DataModel");
    assert_eq!(
        rojo_json["tree"]["ServerScriptService"]["BattleServer"]["$path"],
        "src/server/BattleServer.server.luau"
    );
    assert_eq!(
        rojo_json["tree"]["StarterGui"]["CardHudClient"]["$path"],
        "src/client/CardHudClient.client.luau"
    );
    assert_eq!(
        rojo_json["tree"]["StarterPlayer"]["StarterPlayerScripts"]["CameraRig"]["$path"],
        "src/client/CameraRig.client.luau"
    );
    assert_eq!(
        rojo_json["tree"]["ReplicatedStorage"]["PlaceMeta"]["$path"],
        "src/shared/PlaceMeta.luau"
    );
    assert_eq!(
        rojo_json["tree"]["ReplicatedStorage"]["PlaceMeta_2"]["$path"],
        "src/shared/PlaceMeta_2.luau"
    );

    // Ensure malicious asset filename passed directly into build_rojo_ship_pack is rejected
    let mut bad_meta = png_meta.clone();
    bad_meta.filename = "../../etc/passwd.png".to_string();
    let bad_asset = ShipPackAsset {
        metadata: bad_meta,
        bytes: SAMPLE_PNG.to_vec(),
    };
    assert!(
        build_rojo_ship_pack(&place, &[bad_asset]).is_err(),
        "Malicious traversal asset filename must be rejected"
    );

    // Ensure zero user-facing 'Rune' jargon appears in Ship Pack checklist / project JSON / metadata.
    let audit = place.analyze();
    let checklist = build_upload_checklist("Anime Card Arena", &place, &[], &audit);
    assert!(!checklist.contains("Rune"));
    assert!(!rojo_json.to_string().contains("Rune"));

    // Verify both assets match metadata filenames
    assert_eq!(png_meta.filename, "shadow-blade.png");
    assert_eq!(webp_meta.filename, "solar-phoenix.webp");

    // Optional external-judge dump for Python zipfile + xml.etree.ElementTree + unzip -t verification
    if let Ok(dump_path) = std::env::var("OPENRO_DUMP_SHIP_PACK_ZIP") {
        if !dump_path.trim().is_empty() {
            std::fs::write(&dump_path, &bundle.zip_bytes)
                .expect("write OPENRO_DUMP_SHIP_PACK_ZIP");
        }
    }

    let _ = std::fs::remove_dir_all(&tmp);
}
